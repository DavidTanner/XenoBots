//! A console's 3D view (T-101): the render list built from the sim's world as
//! `gfx_build_render_list` and `gfx_emit_object_render_nodes` build it, from the camera
//! `1000:dba0` sets on the viewed robot (`docs/render.md`), drawn by
//! `xeno_render::view3d`.

use xeno_formats::ban::{self, Sky};
use xeno_formats::mxx;
use xeno_formats::usg::{ObjectKind, Scenario, TerrainFeature};
use xeno_formats::view3d::View3d;
use xeno_render::shape::Shape;
use xeno_render::view3d::{
    Camera, RenderNode, TERRAIN, Trig, focal, focal_shifted, sort_far_to_near,
};
use xeno_render::{Framebuffer, LoadedModel};
use xeno_sim::motion::{Tables, sine};
use xeno_sim::script::bearing;
use xeno_sim::terrain::approx_distance;
use xeno_sim::{Entity, EntityId, State, World};

use crate::game_dir::GameFiles;
use crate::review::MODEL_FILES;

/// The eye height (+0x1c, `docs/render.md`, "The camera"): a robot's, a signed word.
fn eye_height(e: &Entity) -> i16 {
    match e.state {
        State::Robot(m) => m.eye_height as i16,
        _ => 0,
    }
}

pub(crate) fn trig(tables: &Tables, angle: u16) -> Trig {
    let (magnitude, one, negative) = sine(&tables.sine, angle);
    Trig {
        magnitude,
        one,
        negative,
    }
}

/// The main view's camera on robot `id`: its x, y, z plus its eye height, its heading.
pub fn camera(world: &World, id: EntityId, view: &View3d, tables: &Tables) -> Option<Camera> {
    let e = world.get(id)?;
    Some(Camera {
        x: e.position.x.0,
        y: e.position.y.0,
        z: e.z.0.wrapping_add(eye_height(e).into()),
        heading: e.heading,
        sin: trig(tables, e.heading),
        cos: trig(tables, 0x4000u16.wrapping_sub(e.heading)),
        focal: focal(view.focal[0]),
        viewer: id.0,
    })
}

/// The frames the sim keeps, from the kind's frame bytes (`docs/render.md`, "Frame
/// numbers"): the humanoid's arms and legs, the scorpion's and scout's legs, a base's
/// look, an explosion's or the cloak's sparkle's frame, the missile's spin (+0x18; the other shots' +0x18 is 0),
/// a relay's stage and colour (+0x18..=+0x1d); the others are 0 (the scorpion's tail
/// and claws, T-151).
fn frames(e: &Entity) -> Vec<u8> {
    match (e.kind, e.state) {
        (ObjectKind::Humanoid, State::Robot(m)) => {
            let [a, b] = m.guns.arms;
            vec![0, a, b, m.legs[2], m.legs[3]]
        }
        (_, State::Shot(s)) => vec![s.frame],
        (ObjectKind::Scorpion | ObjectKind::Scout, State::Robot(m)) => [&[0][..], &m.legs].concat(),
        (_, State::Base { look, .. }) => vec![look],
        (_, State::Explosion { frame, .. }) => vec![frame],
        (_, State::Sparkle { frame, .. }) => vec![frame],
        (_, State::Relay(r)) => r.frames.to_vec(),
        _ => Vec::new(),
    }
}

/// The second display's camera in the camera mode (`1000:dbe8`): the main view's,
/// turned by the pan (+0xf4), at a zoom of `1 << zoom` (+0xf1) and a zoom shift of 1.
pub fn second_camera(
    world: &World,
    id: EntityId,
    view: &View3d,
    tables: &Tables,
    (zoom, pan): (u8, u16),
) -> Option<Camera> {
    let main = camera(world, id, view, tables)?;
    let heading = main.heading.wrapping_add(pan);
    Some(Camera {
        heading,
        sin: trig(tables, heading),
        cos: trig(tables, 0x4000u16.wrapping_sub(heading)),
        focal: focal_shifted(view.focal[0], 1u8.checked_shl(zoom.into()).unwrap_or(0), 1),
        ..main
    })
}

/// The scorpion's missile camera (`1000:8190`): from its tail missile `missile` in
/// flight, the missile's x, y and z with no eye height and its heading, at a zoom of 1
/// and a zoom shift of 1, its own node left out.
pub fn missile_camera(
    world: &World,
    missile: EntityId,
    view: &View3d,
    tables: &Tables,
) -> Option<Camera> {
    let e = world.get(missile)?;
    Some(Camera {
        x: e.position.x.0,
        y: e.position.y.0,
        z: e.z.0,
        heading: e.heading,
        sin: trig(tables, e.heading),
        cos: trig(tables, 0x4000u16.wrapping_sub(e.heading)),
        focal: focal_shifted(view.focal[0], 1, 1),
        viewer: missile.0,
    })
}

/// The objects' nodes in slot order, each kind's model and node type from `view`,
/// after the terrain's, sorted far to near.
pub fn nodes(
    world: &World,
    view: &View3d,
    camera: &Camera,
    tables: &Tables,
    terrain: &[TerrainFeature],
) -> Vec<RenderNode> {
    let atan = &tables.atan;
    let from = xeno_sim::Position {
        x: xeno_sim::Fixed(camera.x),
        y: xeno_sim::Fixed(camera.y),
    };
    let objects = world.iter().filter_map(|(id, e)| {
        let kind = usize::from(u16::from(e.kind));
        let node_type = (*view.node_types.get(kind)?)?;
        let z = e.z.0;
        Some(RenderNode {
            object: id.0,
            model: view.kind_models[kind],
            heading: e.heading,
            // The height goes into the low word only, with no carry.
            position: [
                e.position.x.0,
                e.position.y.0,
                (z & !0xffff) | i32::from((z as u16).wrapping_add(e.model_height)),
            ],
            radius: e.radius >> 8,
            distance: approx_distance(from, e.position).0 as u32,
            bearing: bearing(atan, from, e.position),
            frames: frames(e),
            size: (node_type == 6).then_some(e.heading),
            shape: None,
        })
    });
    // The terrain records come first, in file order (`gfx_emit_terrain_render_node`).
    // Their heading (+0x08) is `[DS:0x1486] << 4`, the retraces the developer's fill
    // benchmark took (unlock word 2's Ctrl+Alt+F11 and F12, `docs/formats/key.md`): 0
    // from load, and the port has no benchmark.
    let terrain = terrain.iter().map(|f| {
        let centre = xeno_sim::Position {
            x: xeno_sim::Fixed(f.x),
            y: xeno_sim::Fixed(f.y),
        };
        let bearing = bearing(atan, from, centre);
        let turn = 0u16.wrapping_sub(bearing);
        RenderNode {
            object: TERRAIN,
            model: 0,
            heading: 0,
            position: [f.x, f.y, f.z],
            radius: f.radius,
            distance: approx_distance(from, centre).0 as u32,
            bearing,
            frames: Vec::new(),
            size: None,
            shape: Some(Shape {
                radius: f.radius,
                diagonal: f.diagonal,
                height: f.height,
                program: f.shape.clone(),
                turn: [
                    trig(tables, turn),
                    trig(tables, 0x4000u16.wrapping_sub(turn)),
                ],
            }),
        }
    });
    let mut nodes: Vec<RenderNode> = terrain.chain(objects).collect();
    sort_far_to_near(&mut nodes);
    nodes
}

/// The view of robot `id` into `buffer` (the view's own, the region's size) with the
/// pointer probe at `probe` (in the buffer), and the object under it; `None` with
/// nothing drawn when the robot is not in the world.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    buffer: &mut Framebuffer,
    world: &World,
    id: EntityId,
    view: &View3d,
    tables: &Tables,
    terrain: &[TerrainFeature],
    models: &[Option<LoadedModel>],
    sky: Option<&Sky>,
    probe: Option<(usize, usize)>,
) -> Option<Option<u16>> {
    let camera = camera(world, id, view, tables)?;
    Some(draw_from(
        buffer, world, &camera, view, tables, terrain, models, sky, probe,
    ))
}

/// The view from `camera` into `buffer`, and the object under `probe`.
#[allow(clippy::too_many_arguments)]
pub fn draw_from(
    buffer: &mut Framebuffer,
    world: &World,
    camera: &Camera,
    view: &View3d,
    tables: &Tables,
    terrain: &[TerrainFeature],
    models: &[Option<LoadedModel>],
    sky: Option<&Sky>,
    probe: Option<(usize, usize)>,
) -> Option<u16> {
    let nodes = nodes(world, view, camera, tables, terrain);
    buffer.draw_view(view, camera, &nodes, models, sky, probe)
}

/// Every model as `gfx_load_all_models` loads it (`docs/formats/mxx.md`, "Loading"),
/// by model number / 2, or none, with a warning, when its files can't be read.
pub fn load_models(files: &GameFiles) -> Vec<Option<LoadedModel>> {
    let read = |name: &str, suffix: &str| -> Result<mxx::Model, String> {
        let name = format!("{name}.{suffix}");
        let bytes = files
            .read(&name)
            .map_err(|e| format!("cannot read {name}: {e}"))?;
        mxx::parse(&bytes).map_err(|e| format!("{name}: {e}"))
    };
    let load = |name: &str| -> Result<LoadedModel, String> {
        let m00 = read(name, "m00")?;
        let m08 = read(name, "m08")?;
        Ok(match name {
            "explode" | "plasma" | "basedome" | "cloak" | "obst" => {
                LoadedModel::single_view(&m00, &m08)
            }
            "netrelay" => {
                LoadedModel::quarter_turn(&m00, &[m08, read(name, "m16")?, read(name, "m32")?])
            }
            "laser" => LoadedModel::new(&m00, &[m08, read(name, "m16")?, read(name, "m32")?]),
            _ => LoadedModel::new(&m00, &[m08, read(name, "m16")?]),
        })
    };
    MODEL_FILES
        .iter()
        .map(|name| load(name).map_err(|e| log::warn!("3D view: {e}")).ok())
        .collect()
}

/// The sky strip a scenario's `BGND` names (`BACKLAND.BIN` without one), found as
/// `.bin`, `.b!n` or `.b@n`, or none (the flat sky) when it is not there.
// shortcut: an empty or long `BGND` name keeps the previous mission's sky in the
// original; here it gives `BACKLAND`, which no stock install has.
pub fn load_sky(files: &GameFiles, scenario: &Scenario) -> Option<Sky> {
    let name = scenario
        .background_name()
        .unwrap_or_default()
        .to_lowercase();
    let base = name
        .split('.')
        .next()
        .filter(|b| !b.is_empty() && b.len() <= 8);
    let base = base.unwrap_or("backland");
    let bytes = ["bin", "b!n", "b@n"]
        .iter()
        .find_map(|ext| files.read(&format!("{base}.{ext}")).ok())?;
    ban::parse_sky(&bytes)
        .map_err(|e| log::warn!("3D view: {base}: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::usg::Side;
    use xeno_formats::view3d::Float;
    use xeno_sim::{Damage, Fixed, Movement, Position};

    fn entity(kind: ObjectKind, x: i16, y: i16, state: State) -> Entity {
        Entity {
            kind,
            side: Side::Human,
            position: Position {
                x: Fixed::from_int(x),
                y: Fixed::from_int(y),
            },
            z: Fixed(0),
            model_height: 0,
            heading: 0x1234,
            radius: 0x0a00,
            damage: Damage::None,
            state,
        }
    }

    /// Made-up tables: humanoids (model 0), bases (model 16) and missiles (kind 8,
    /// model 12) make type 2 nodes, kind 13 (model 10) type 6; nothing else makes one.
    fn view() -> View3d {
        let mut node_types = [None; 17];
        (node_types[0], node_types[3], node_types[13]) = (Some(2), Some(2), Some(6));
        node_types[8] = Some(2);
        let mut kind_models = [0; 17];
        (kind_models[3], kind_models[13], kind_models[8]) = (16, 10, 12);
        View3d {
            kind_models,
            frame_offsets: [0; 17],
            node_types,
            focal: [Float {
                mantissa: 230,
                exponent: 0,
            }; 3],
            model_sizes: [0x100; 11],
            default_size: Float {
                mantissa: 0x8000,
                exponent: -8,
            },
            max_distance: 0x7d,
            box_program: Vec::new(),
            hill_program: Vec::new(),
        }
    }

    #[test]
    fn a_robot_s_frames_are_its_arms_and_legs() {
        let m = Movement {
            legs: [1, 2, 3, 4, 5, 6],
            guns: xeno_sim::Guns {
                arms: [7, 8],
                ..xeno_sim::Guns::default()
            },
            ..Movement::default()
        };
        let legs = vec![0, 1, 2, 3, 4, 5, 6];
        for (kind, want) in [
            (ObjectKind::Humanoid, vec![0, 7, 8, 3, 4]),
            (ObjectKind::Scorpion, legs.clone()),
            (ObjectKind::Scout, legs),
        ] {
            assert_eq!(frames(&entity(kind, 0, 0, State::Robot(m))), want);
        }
        // A relay's stage look and colour.
        let relay = xeno_sim::relay::Relay {
            frames: [0, 2, 2, 2, 2, 1],
            ..Default::default()
        };
        let e = entity(ObjectKind::Relay, 0, 0, State::Relay(relay));
        assert_eq!(frames(&e), [0, 2, 2, 2, 2, 1]);
    }

    #[test]
    fn nodes_come_from_the_world_by_kind_far_to_near() {
        let tables = crate::mission::tests::test_assets().motion;
        let walking = State::Robot(Movement {
            legs: [0, 0, 3, 4, 0, 0],
            eye_height: 0x0f80,
            guns: xeno_sim::Guns {
                arms: [1, 2],
                ..xeno_sim::Guns::default()
            },
            ..Movement::default()
        });
        let mut world = World::default();
        world.insert(EntityId(2), entity(ObjectKind::Humanoid, 0, 0, walking));
        let crouched = match walking {
            State::Robot(m) => State::Robot(Movement {
                eye_height: 0x0c80,
                ..m
            }),
            _ => unreachable!(),
        };
        world.insert(EntityId(5), entity(ObjectKind::Scout, 0, 3, crouched));
        let base = State::Base {
            tick: 0,
            kills: 0,
            losses: 0,
            destroyed: false,
            end_step: 0,
            look: 2,
            counts: [0; 7],
            depot: Default::default(),
        };
        world.insert(EntityId(7), entity(ObjectKind::Base, 0, 9, base));
        let boom = State::Explosion {
            frame: 5,
            last: 9,
            ticks: 1,
            countdown: 1,
        };
        world.insert(EntityId(9), entity(ObjectKind::Other(13), 1, 4, boom));
        world.insert(EntityId(11), entity(ObjectKind::Humanoid, 0, 4, walking));
        // A missile 8 units off, 0x1800 up, spinning at frame 3.
        let missile = State::Shot(xeno_sim::shot::Shot {
            flight: xeno_sim::shot::Flight::Missile,
            velocity: [0; 3],
            speed: 0,
            climb: 0,
            age: 0,
            arming: 0,
            life: 1,
            damage: 8,
            target: None,
            turn: 0,
            phase: 0,
            frame: 3,
        });
        world.insert(EntityId(13), entity(ObjectKind::Other(8), 0, 8, missile));
        let mut stand = |id, z, model_height| {
            let e = world.get_mut(EntityId(id)).unwrap();
            (e.z, e.model_height) = (Fixed(z), model_height);
        };
        stand(13, 0x1800, 0);
        stand(2, 0x1_0000, 0xa00);
        stand(11, 0x2_f000, 0x2000);
        let view = view();
        let eye = camera(&world, EntityId(2), &view, &tables).unwrap();
        // The humanoid's eye height on its z.
        assert_eq!((eye.z, eye.viewer, eye.heading), (0x1_0f80, 2, 0x1234));
        let nodes = nodes(&world, &view, &eye, &tables, &[]);
        let summary: Vec<_> = nodes
            .iter()
            .map(|n| (n.object, n.model, n.frames.clone(), n.size))
            .collect();
        // The scout makes no node here; 9 (4.5 units away) is farther than 11 (4);
        // the viewer's own node is made (and skipped when drawn).
        assert_eq!(
            summary,
            [
                (7, 16, vec![2], None),
                (13, 12, vec![3], None),
                (9, 10, vec![5], Some(0x1234)),
                (11, 0, vec![0, 1, 2, 3, 4], None),
                (2, 0, vec![0, 1, 2, 3, 4], None),
            ]
        );
        // The shot at its own height.
        assert_eq!(nodes[1].position[2], 0x1800);
        // The model height goes into the low word without a carry.
        assert_eq!(nodes[3].position[2], 0x2_1000);
        assert_eq!(nodes[4].position[2], 0x1_0a00);
        assert_eq!(nodes[2].distance, 0x4_8000);
        // The eye height is the robot's own.
        let scout = camera(&world, EntityId(5), &view, &tables).unwrap();
        assert_eq!(scout.z, 0x0c80);
    }

    #[test]
    fn terrain_shapes_turn_by_the_heading_less_the_bearing() {
        let tables = crate::mission::tests::test_assets().motion;
        let mut world = World::default();
        world.insert(EntityId(2), entity(ObjectKind::Humanoid, 0, 0, State::None));
        let eye = Camera {
            heading: 0,
            ..camera(&world, EntityId(2), &view(), &tables).unwrap()
        };
        let feature = |x: i16, y: i16| TerrainFeature {
            size: 0x18,
            kind: TerrainFeature::BLOCK,
            point_slots: 8,
            x: Fixed::from_int(x).0,
            y: Fixed::from_int(y).0,
            z: 0,
            radius: 0x100,
            diagonal: 0x100,
            height: 0x100,
            shape: vec![TerrainFeature::BLOCK, 0],
        };
        let terrain = [feature(0, 10), feature(10, 0)];
        let nodes = nodes(&world, &view(), &eye, &tables, &terrain);
        let at = |x: i16| nodes.iter().find(|n| n.position[0] == Fixed::from_int(x).0);
        let turn = |x: i16| at(x).map(|n| (n.bearing, n.shape.as_ref().unwrap().turn));
        let one = Trig {
            magnitude: 0xffff,
            one: true,
            negative: false,
        };
        // Ahead, no turn: sine 0. To the right, a quarter turn: the heading (0) less
        // it is a quarter turn back, sine -1.
        assert_eq!(turn(0), Some((0, [Trig::default(), one])));
        let right = Trig {
            negative: true,
            ..one
        };
        assert_eq!(
            turn(10).map(|(b, [sin, _])| (b, sin)),
            Some((0x4000, right))
        );
    }
}
