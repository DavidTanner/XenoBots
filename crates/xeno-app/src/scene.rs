//! A console's 3D view (T-101): the render list built from the sim's world as
//! `gfx_build_render_list` and `gfx_emit_object_render_nodes` build it, from the camera
//! `1000:dba0` sets on the viewed robot (`docs/render.md`), drawn by
//! `xeno_render::view3d`.

use xeno_formats::ban::{self, Sky};
use xeno_formats::mxx;
use xeno_formats::usg::{ObjectKind, Scenario, TerrainFeature};
use xeno_formats::view3d::View3d;
use xeno_render::shape::Shape;
use xeno_render::view3d::{Camera, RenderNode, TERRAIN, Trig, focal, sort_far_to_near};
use xeno_render::{Framebuffer, LoadedModel};
use xeno_sim::motion::{Tables, sine};
use xeno_sim::script::bearing;
use xeno_sim::terrain::approx_distance;
use xeno_sim::{Entity, EntityId, State, World};

use crate::game_dir::GameFiles;
use crate::review::MODEL_FILES;

/// The objects' z (+0x0c) and model height (+0x12) by slot.
pub type Heights = [(i32, u16); 240];

/// shortcut: the sim keeps neither a z nor the model height, so both are the
/// scenario's as loaded (0 for objects made since); hills, the walking bob and the
/// explosions' drop are left out until the sim keeps them.
pub fn heights(scenario: Option<&Scenario>) -> Heights {
    let mut heights = [(0, 0); 240];
    for o in scenario.iter().flat_map(|s| s.objects.iter().flatten()) {
        if let Some(h) = heights.get_mut(usize::from(o.slot)) {
            *h = (o.z, o.unknown_12);
        }
    }
    heights
}

/// The eye height (+0x1c, `docs/render.md`, "The camera"): set per kind when a robot is
/// made and as a scout's stance steps. Short code constants, kept here.
fn eye_height(e: &Entity) -> i16 {
    match (e.kind, e.state) {
        (ObjectKind::Humanoid, _) => 0x0f80,
        (ObjectKind::Scout, State::Robot(m)) => 0x0d00 - 8 * i16::from(m.stance),
        _ => 0x0b80,
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
pub fn camera(
    world: &World,
    id: EntityId,
    view: &View3d,
    tables: &Tables,
    heights: &Heights,
) -> Option<Camera> {
    let e = world.get(id)?;
    let z = heights.get(usize::from(id.0)).map_or(0, |h| h.0);
    Some(Camera {
        x: e.position.x.0,
        y: e.position.y.0,
        z: z.wrapping_add(eye_height(e).into()),
        heading: e.heading,
        sin: trig(tables, e.heading),
        cos: trig(tables, 0x4000u16.wrapping_sub(e.heading)),
        focal: focal(view.focal[0]),
        viewer: id.0,
    })
}

/// The frames the sim keeps, from the kind's frame bytes (`docs/render.md`, "Frame
/// numbers"): the legs that make footsteps, a base's look, an explosion's frame; the
/// others are 0 (T-114 and the remaining walking animations).
fn frames(e: &Entity) -> Vec<u8> {
    match (e.kind, e.state) {
        (ObjectKind::Humanoid, State::Robot(m)) => vec![0, 0, 0, m.legs[0], m.legs[1]],
        (ObjectKind::Scout, State::Robot(m)) => vec![0, 0, 0, 0, 0, m.legs[0], m.legs[1]],
        (_, State::Base { look, .. }) => vec![look],
        (_, State::Explosion { frame, .. }) => vec![frame],
        _ => Vec::new(),
    }
}

/// The objects' nodes in slot order, each kind's model and node type from `view`,
/// after the terrain's, sorted far to near.
pub fn nodes(
    world: &World,
    view: &View3d,
    camera: &Camera,
    atan: &[u16],
    heights: &Heights,
    terrain: &[TerrainFeature],
) -> Vec<RenderNode> {
    let from = xeno_sim::Position {
        x: xeno_sim::Fixed(camera.x),
        y: xeno_sim::Fixed(camera.y),
    };
    let objects = world.iter().filter_map(|(id, e)| {
        let kind = usize::from(u16::from(e.kind));
        let node_type = (*view.node_types.get(kind)?)?;
        let (z, height) = heights.get(usize::from(id.0)).copied().unwrap_or_default();
        Some(RenderNode {
            object: id.0,
            model: view.kind_models[kind],
            heading: e.heading,
            // The height goes into the low word only, with no carry.
            position: [
                e.position.x.0,
                e.position.y.0,
                (z & !0xffff) | i32::from((z as u16).wrapping_add(height)),
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
    // shortcut: their heading (+0x08, `[DS:0x1486] << 4`) only turns the untraced
    // point ops' shapes, so it is left at 0.
    let terrain = terrain.iter().map(|f| {
        let centre = xeno_sim::Position {
            x: xeno_sim::Fixed(f.x),
            y: xeno_sim::Fixed(f.y),
        };
        RenderNode {
            object: TERRAIN,
            model: 0,
            heading: 0,
            position: [f.x, f.y, f.z],
            radius: f.radius,
            distance: approx_distance(from, centre).0 as u32,
            bearing: bearing(atan, from, centre),
            frames: Vec::new(),
            size: None,
            shape: Some(Shape {
                radius: f.radius,
                diagonal: f.diagonal,
                height: f.height,
                program: f.shape.clone(),
            }),
        }
    });
    let mut nodes: Vec<RenderNode> = terrain.chain(objects).collect();
    sort_far_to_near(&mut nodes);
    nodes
}

/// The view of robot `id` into `buffer` (the view's own, the region's size), or
/// `false` with nothing drawn when the robot is not in the world.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    buffer: &mut Framebuffer,
    world: &World,
    id: EntityId,
    view: &View3d,
    tables: &Tables,
    heights: &Heights,
    terrain: &[TerrainFeature],
    models: &[Option<LoadedModel>],
    sky: Option<&Sky>,
) -> bool {
    let Some(camera) = camera(world, id, view, tables, heights) else {
        return false;
    };
    let nodes = nodes(world, view, &camera, &tables.atan, heights, terrain);
    buffer.draw_view(view, &camera, &nodes, models, sky);
    true
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
            heading: 0x1234,
            radius: 0x0a00,
            damage: Damage::None,
            state,
        }
    }

    /// Made-up tables: humanoids (model 0) and bases (model 16) make type 2 nodes,
    /// kind 13 (model 10) type 6; nothing else makes one.
    fn view() -> View3d {
        let mut node_types = [None; 17];
        (node_types[0], node_types[3], node_types[13]) = (Some(2), Some(2), Some(6));
        let mut kind_models = [0; 17];
        (kind_models[3], kind_models[13]) = (16, 10);
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
    fn nodes_come_from_the_world_by_kind_far_to_near() {
        let tables = crate::mission::tests::test_assets().motion;
        let walking = State::Robot(Movement {
            legs: [3, 4],
            stance: 0x10,
            ..Movement::default()
        });
        let mut world = World::default();
        world.insert(EntityId(2), entity(ObjectKind::Humanoid, 0, 0, walking));
        world.insert(EntityId(5), entity(ObjectKind::Scout, 0, 3, walking));
        let base = State::Base {
            tick: 0,
            kills: 0,
            losses: 0,
            destroyed: false,
            end_step: 0,
            look: 2,
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
        let mut heights = [(0, 0); 240];
        heights[2] = (0x1_0000, 0xa00);
        heights[11] = (0x2_f000, 0x2000);
        let view = view();
        let eye = camera(&world, EntityId(2), &view, &tables, &heights).unwrap();
        // The humanoid's eye height on its z.
        assert_eq!((eye.z, eye.viewer, eye.heading), (0x1_0f80, 2, 0x1234));
        let nodes = nodes(&world, &view, &eye, &tables.atan, &heights, &[]);
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
                (9, 10, vec![5], Some(0x1234)),
                (11, 0, vec![0, 0, 0, 3, 4], None),
                (2, 0, vec![0, 0, 0, 3, 4], None),
            ]
        );
        // The model height goes into the low word without a carry.
        assert_eq!(nodes[2].position[2], 0x2_1000);
        assert_eq!(nodes[3].position[2], 0x1_0a00);
        assert_eq!(nodes[1].distance, 0x4_8000);
        // A scout's eye height falls as it crouches.
        let scout = camera(&world, EntityId(5), &view, &tables, &heights).unwrap();
        assert_eq!(scout.z, 0x0d00 - 0x80);
    }
}
