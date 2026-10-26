use super::*;
use crate::orders::OrderBook;
use crate::terrain::{Feature, Path};
use crate::world::{Components, Entity, Fixed};
use xeno_formats::usg::TerrainFeature;

pub(crate) fn at(x: i16, y: i16) -> Position {
    Position {
        x: Fixed::from_int(x),
        y: Fixed::from_int(y),
    }
}

/// A robot standing at `(x, y)`, heading 0, full helm and drive, standing (scouts).
pub(crate) fn robot(kind: ObjectKind, side: Side, x: i16, y: i16) -> Entity {
    Entity {
        kind,
        side,
        position: at(x, y),
        heading: 0,
        radius: 0xa00,
        damage: Damage::Robot(Components {
            helm: 100,
            power: 100,
            weapons: 0,
            targeting: 0,
            equipment: 0,
            systems: 0,
        }),
        state: State::Robot(Movement {
            drive: 0xff,
            stance: STANDING,
            stance_target: STANDING,
            speed_table: 0,
            ..Movement::default()
        }),
    }
}

pub(crate) fn base(side: Side, x: i16, y: i16, tick: u8) -> Entity {
    Entity {
        kind: ObjectKind::Base,
        side,
        position: at(x, y),
        heading: 0,
        radius: 0x2300,
        damage: Damage::Taken(0),
        state: State::Base {
            tick,
            kills: 0,
            losses: 0,
            destroyed: false,
        },
    }
}

fn moving(world: &World, id: u16) -> Movement {
    match world.get(EntityId(id)).unwrap().state {
        State::Robot(m) => m,
        other => panic!("{other:?}"),
    }
}

fn set(world: &mut World, id: u16, f: impl FnOnce(&mut Movement)) {
    f(movement(world, EntityId(id)).unwrap());
}

#[test]
fn the_sine_is_the_table_interpolated_and_folded() {
    let t = synthetic_tables();
    assert_eq!(sine(&t.sine, 0), (0, false, false));
    assert_eq!(sine(&t.sine, 0x4000), (0xffff, true, false));
    assert_eq!(sine(&t.sine, 0xc000), (0xffff, true, true));
    // An entry, and its mirror about the quarter turn.
    assert_eq!(sine(&t.sine, 0x40).0, t.sine[1]);
    assert_eq!(sine(&t.sine, 0x8000 - 0x40), sine(&t.sine, 0x40));
    // Halfway between two entries, rounded.
    let half = sine(&t.sine, 0x60).0;
    let (a, b) = (u32::from(t.sine[1]), u32::from(t.sine[2]));
    assert_eq!(u32::from(half), a + ((b - a) * 128 + 0x80) / 256);
    // The second half turn is negative.
    let (m, one, negative) = sine(&t.sine, 0x8000 + 0x60);
    assert_eq!((m, one, negative), (half, false, true));
    // Between entries 255 and 256 (which is 0) the difference wraps to 1: under half
    // way it adds nothing, from half way it carries to 1.0.
    let mut wrap = t.sine.clone();
    wrap[255] = 0xffff;
    assert_eq!(sine(&wrap, 0x3fc0 + 0x1f), (0xffff, false, false));
    assert_eq!(sine(&wrap, 0x3fc0 + 0x20), (0xffff, true, false));
    // A difference of 256 to 511 adds the fraction itself on top.
    let mut big = t.sine.clone();
    (big[0], big[1]) = (0, 0x180);
    assert_eq!(sine(&big, 0x20).0, (0x80 * 0x80 + 0x80) / 256 + 0x80);
}

#[test]
fn the_velocity_is_speed_times_sine_and_cosine() {
    let t = synthetic_tables();
    // Heading 0 is +y, a quarter turn +x; 1.0 gives the speed itself.
    assert_eq!(velocity(&t, 0, 100), (0, 100));
    assert_eq!(velocity(&t, 0x4000, 100), (100, 0));
    assert_eq!(velocity(&t, 0x8000, 100), (0, -100));
    assert_eq!(velocity(&t, 0xc000, -100), (100, 0));
    // An eighth of a turn: the high word of the multiply, truncated.
    let s = u32::from(sine(&t.sine, 0x2000).0);
    let v = ((s * 63) >> 16) as i16;
    assert_eq!(velocity(&t, 0x2000, 63), (v, v));
    assert_eq!(velocity(&t, 0x2000, -63), (-v, -v));
    assert_eq!(velocity(&t, 0x1234, 0), (0, 0));
}

#[test]
fn steering_turns_by_at_most_the_step() {
    assert_eq!(steer(0, 0x5b, 0x5b), 0x5b);
    assert_eq!(steer(0, 0x1000, 0x5b), 0x5b);
    assert_eq!(steer(0, 0xf000, 0x5b), 0u16.wrapping_sub(0x5b));
    assert_eq!(steer(0xfff0, 0x0010, 0x5b), 0x0010);
    assert_eq!(steer(7, 7, 0x5b), 7);
    // Exactly half a turn away goes the negative way.
    assert_eq!(steer(0, 0x8000, 0x5b), 0u16.wrapping_sub(0x5b));
}

#[test]
fn the_drive_and_helm_limit_the_speed() {
    assert_eq!(limit_speed(63, 0, 100, 0x28), 0);
    assert_eq!(limit_speed(63, 0x28, 100, 0x28), 31);
    assert_eq!(limit_speed(-63, 0x28, 100, 0x28), -32);
    assert_eq!(limit_speed(63, 0x29, 100, 0x28), 63);
    for (helm, speed) in [
        (15, 0),
        (16, 15),
        (47, 15),
        (48, 31),
        (64, 48),
        (95, 48),
        (96, 63),
    ] {
        assert_eq!(limit_speed(63, 0xff, helm, 0x28), speed, "helm {helm}");
    }
    let t = synthetic_tables();
    assert_eq!(map_scorpion_speed(&t, 2, 40), 20);
    assert_eq!(map_scorpion_speed(&t, 3, -41), -20);
    assert_eq!(map_scorpion_speed(&t, 0, 40), 40);
    // Past the tables or their entries: the last.
    assert_eq!(map_scorpion_speed(&t, 0x0c, 40), 20);
    assert_eq!(map_scorpion_speed(&t, 0, 100), 63);
}

/// One robot's tick with no script: steer and speed, then the move.
fn step(world: &mut World, id: u16) -> bool {
    let t = synthetic_tables();
    let (e, terrain) = world.entity_and_terrain(EntityId(id)).unwrap();
    let State::Robot(m) = &mut e.state else {
        panic!()
    };
    steer_and_speed(&t, e.kind, 100, &mut e.heading, m);
    travel(terrain, e.kind, &mut e.position, m)
}

#[test]
fn a_humanoid_steps_its_speed_every_tick_and_a_scorpion_every_8() {
    let mut world = World::default();
    world.insert(EntityId(1), robot(ObjectKind::Humanoid, Side::Human, 0, 0));
    world.insert(EntityId(2), robot(ObjectKind::Scorpion, Side::Human, 0, 0));
    for id in [1, 2] {
        set(&mut world, id, |m| m.target_speed = 10);
    }
    let speeds: Vec<_> = (0..17)
        .map(|_| {
            step(&mut world, 1);
            step(&mut world, 2);
            (moving(&world, 1).speed, moving(&world, 2).speed)
        })
        .collect();
    assert_eq!(speeds[0], (1, 1));
    assert_eq!(speeds[7], (8, 1));
    assert_eq!(speeds[8], (9, 2));
    assert_eq!(speeds[16], (10, 3));
    // The counters: the humanoid's runs on, the scorpion's is kept to 0..=0x1e.
    assert_eq!((moving(&world, 1).tick, moving(&world, 2).tick), (34, 2));
    // Each step moved it by the speed then, heading +y.
    let y = world.get(EntityId(1)).unwrap().position.y.0;
    assert_eq!(y, (1..=10).sum::<i32>() + 7 * 10);
}

#[test]
fn a_scout_stands_up_before_it_moves() {
    let mut world = World::default();
    world.insert(EntityId(3), robot(ObjectKind::Scout, Side::Human, 0, 0));
    set(&mut world, 3, |m| {
        (m.stance, m.stance_target, m.target_speed) = (0x2e, 0x2e, 5)
    });
    step(&mut world, 3);
    assert_eq!(moving(&world, 3).stance_target, STANDING);
    assert_eq!(
        (moving(&world, 3).stance, moving(&world, 3).speed),
        (0x2f, 0)
    );
    step(&mut world, 3);
    step(&mut world, 3);
    assert_eq!(
        (moving(&world, 3).stance, moving(&world, 3).speed),
        (0x30, 1)
    );
}

#[test]
fn turning_recomputes_the_velocity_and_positions_wrap() {
    let mut world = World::default();
    world.insert(EntityId(1), robot(ObjectKind::Humanoid, Side::Human, 0, 0));
    set(&mut world, 1, |m| {
        (m.speed, m.target_speed, m.velocity) = (50, 50, (0, 50));
        m.target_heading = 0x4000;
        m.mode = 0x20;
    });
    step(&mut world, 1);
    let m = moving(&world, 1);
    // The turn key added 0x40 to the target; the heading turned one step.
    assert_eq!(m.target_heading, 0x4040);
    assert_eq!(world.get(EntityId(1)).unwrap().heading, 0x100);
    assert_eq!(m.velocity, velocity(&synthetic_tables(), 0x100, 50));
    // No world edges: x wraps as a 32-bit number.
    world.get_mut(EntityId(1)).unwrap().position.x = Fixed(i32::MAX);
    step(&mut world, 1);
    assert!(world.get(EntityId(1)).unwrap().position.x.0 < 0);
}

#[test]
fn a_robot_in_a_hill_backs_off_at_its_old_velocity_until_its_speed_steps() {
    let mut world = World::default();
    world.terrain.features.push(Feature {
        kind: TerrainFeature::HILL,
        centre: at(0, 20),
        z: Fixed(0),
        radius: 0x0a00,
        diagonal: 0x0600,
        height: 0x0300,
    });
    world.insert(EntityId(2), robot(ObjectKind::Scorpion, Side::Human, 0, 0));
    set(&mut world, 2, |m| {
        (m.speed, m.target_speed, m.velocity, m.tick) = (0x7fff, 0x7fff, (0, 0x7fff), 2)
    });
    // A tick of 0x7fff/65536 a unit; start just outside the radius.
    world.get_mut(EntityId(2)).unwrap().position.y = Fixed(Fixed::from_int(10).0 - 0x10);
    assert!(step(&mut world, 2));
    let m = moving(&world, 2);
    assert_eq!(
        (m.speed, m.target_speed, m.velocity),
        (-0x7fff, 0, (0, 0x7fff))
    );
    // Still inside (the velocity is kept): the speed flips back.
    assert!(step(&mut world, 2));
    assert_eq!(moving(&world, 2).speed, 0x7fff);
    let mut events = Vec::new();
    let mut random = Random::seeded(1);
    let tables = synthetic_tables();
    let before = world.get(EntityId(2)).unwrap().position;
    events.extend(tick(&mut world, &tables, &mut random));
    assert_eq!(events, [Event::HillHit(EntityId(2))]);
    // The bounce is a 0-point hit: event 7 waits for the script, nothing is worn.
    assert_eq!(moving(&world, 2).script.events, 1 << 7);
    assert_eq!(
        world.get(EntityId(2)).unwrap().damage,
        robot(ObjectKind::Scorpion, Side::Human, 0, 0).damage
    );
    assert_ne!(world.get(EntityId(2)).unwrap().position, before);
}

/// A world with an alien base (slot 0) at (0, 0) and its robots, and alien paths:
/// slot 0 of 3 points (weight 0, mission 1), slot 1 of 1 point (weight 2, mission 2).
fn walkers() -> World {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Alien, 0, 0, 0));
    world.insert(EntityId(4), robot(ObjectKind::Humanoid, Side::Alien, 1, 1));
    world.insert(EntityId(5), robot(ObjectKind::Scorpion, Side::Alien, 2, 2));
    world.bases[1] = Some(EntityId(0));
    world.units[1] = vec![EntityId(5), EntityId(4)];
    let path = |mission, weight, points: &[(i16, i16)]| Path {
        mission,
        weight,
        points: points.iter().map(|&(x, y)| at(x, y)).collect(),
    };
    world.paths.alien.slots = vec![
        path(1, 0, &[(10, 0), (20, 0), (30, 0)]),
        path(2, 2, &[(0, 50)]),
        path(0, 0, &[]),
    ];
    world.paths.alien.count = 2;
    world.orders = OrderBook::default();
    world
}

#[test]
fn heading_for_points_turns_round_at_the_ends() {
    let t = synthetic_tables();
    let (mut random, mut events) = (Random::seeded(1), Vec::new());
    let mut world = walkers();
    let id = EntityId(5);
    set(&mut world, 5, |m| (m.path, m.path_point) = (0, 0));
    assert!(head_for_path_point(
        &mut world,
        id,
        &t,
        &mut random,
        &mut events
    ));
    assert_eq!(moving(&world, 5).destination, at(10, 0));
    assert_eq!(moving(&world, 5).goal, Movement::GOAL_POINT);
    assert_eq!(moving(&world, 5).path, Movement::ON_PATH);
    // On to the end: the last point, then it turns and heads back one.
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    assert_eq!(moving(&world, 5).path_point, 2);
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    let m = moving(&world, 5);
    assert_eq!((m.path_point, m.destination), (1, at(20, 0)));
    assert_eq!(m.path, Movement::ON_PATH | Movement::BACKWARDS);
    // Back past point 0 to its base (0xff), then a scorpion turns round at 0xfe.
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    assert_eq!(moving(&world, 5).path_point, 0xff);
    assert_eq!(moving(&world, 5).destination, at(0, 0));
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    let m = moving(&world, 5);
    assert_eq!((m.path_point, m.path & Movement::BACKWARDS), (0, 0));
    // A path with no points fails, and advancing on it stops.
    set(&mut world, 5, |m| m.path = 2);
    assert!(!head_for_path_point(
        &mut world,
        id,
        &t,
        &mut random,
        &mut events
    ));
    advance_path_point(&mut world, id, &t, &mut random, &mut events);
    assert!(events.is_empty());
}

#[test]
fn the_alien_base_picks_paths_by_weight_and_a_new_path_starts_at_point_2() {
    let t = synthetic_tables();
    let mut world = walkers();
    // The list is slot 0 once and slot 1 three times; the word's low byte, reduced
    // modulo 4, picks.
    let mut random = Random::seeded(5);
    let mut copy = random.clone();
    let picks: Vec<u8> = (0..2)
        .map(|_| [0, 1, 1, 1][usize::from(copy.word() as u8) % 4])
        .collect();
    let mut events = Vec::new();
    assign_random_path(&mut world, EntityId(5), &t, &mut random, &mut events);
    assign_random_path(&mut world, EntityId(4), &t, &mut random, &mut events);
    assert_eq!(moving(&world, 5).path & 0xf, picks[0]);
    assert_eq!(moving(&world, 4).path & 0xf, picks[1]);
    assert_eq!(moving(&world, 5).path_point, 0);
    assert!(events.contains(&Event::PathTaken {
        robot: EntityId(5),
        slot: picks[0]
    }));

    // A humanoid back at its base takes a new path and heads for its point 2.
    let mut world = walkers();
    world.paths.alien.slots[1].points = vec![at(0, 50), at(0, 60), at(0, 70)];
    set(&mut world, 4, |m| {
        (m.path, m.path_point) = (Movement::ON_PATH | Movement::BACKWARDS, 0xff)
    });
    let mut events = Vec::new();
    advance_path_point(&mut world, EntityId(4), &t, &mut random, &mut events);
    let m = moving(&world, 4);
    assert_eq!(m.path_point, 2);
    assert_eq!(
        m.path & (Movement::BACKWARDS | Movement::ON_PATH),
        Movement::ON_PATH
    );
    assert!(matches!(events[0], Event::PathTaken { .. }));
}

#[test]
fn the_alien_base_hands_out_paths_once_every_16_ticks() {
    let t = synthetic_tables();
    let mut world = walkers();
    // The scorpion asked for its next point; the humanoid has no path.
    set(&mut world, 5, |m| {
        (m.path, m.path_point) = (Movement::ON_PATH | Movement::NEXT, 0)
    });
    let mut random = Random::seeded(9);
    // The base's count reaches 2 on the second tick.
    assert!(tick(&mut world, &t, &mut random).is_empty());
    let events = tick(&mut world, &t, &mut random);
    assert!(matches!(
        events[0],
        Event::PathTaken {
            robot: EntityId(4),
            ..
        }
    ));
    assert_eq!(moving(&world, 5).path_point, 1);
    assert_eq!(moving(&world, 5).path & Movement::NEXT, 0);
    // Not again for 16 ticks, and the human base never.
    set(&mut world, 5, |m| m.path |= Movement::NEXT);
    for _ in 0..15 {
        assert!(tick(&mut world, &t, &mut random).is_empty());
    }
    assert_eq!(moving(&world, 5).path_point, 1);
    tick(&mut world, &t, &mut random);
    assert_eq!(moving(&world, 5).path_point, 2);
    // A robot with +0x23 bit 0 is left alone.
    let mut world = walkers();
    set(&mut world, 4, |m| m.mode = 1);
    set(&mut world, 5, |m| m.path = Movement::ON_PATH);
    let mut random = Random::seeded(9);
    tick(&mut world, &t, &mut random);
    assert!(tick(&mut world, &t, &mut random).is_empty());
}

#[test]
fn joining_takes_the_nearest_point_within_256_units() {
    let mut world = walkers();
    world.get_mut(EntityId(4)).unwrap().position = at(18, 2);
    join_nearest_path_point(&mut world, EntityId(4));
    let m = moving(&world, 4);
    assert_eq!((m.path, m.path_point, m.destination), (0, 1, at(20, 0)));
    // Nearer the base: slot 0, point 0, heading for the base.
    world.get_mut(EntityId(4)).unwrap().position = at(1, -3);
    join_nearest_path_point(&mut world, EntityId(4));
    let m = moving(&world, 4);
    assert_eq!((m.path, m.path_point, m.destination), (0, 0, at(0, 0)));
    // Everything 256 units or more away: the base still.
    world.get_mut(EntityId(4)).unwrap().position = at(-1000, 0);
    join_nearest_path_point(&mut world, EntityId(4));
    assert_eq!(moving(&world, 4).destination, at(0, 0));
}

#[test]
fn a_path_starts_its_mission_s_program_with_the_program_bit() {
    use xeno_formats::fixtures::synthetic_mission_usg;
    let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
    let mut world = World::from_scenario(&scenario);
    let walkers = walkers();
    world.paths = walkers.paths;
    world.insert(EntityId(9), base(Side::Alien, 0, 0, 1));
    world.bases[1] = Some(EntityId(9));
    world.units[1] = vec![EntityId(7)];
    set(&mut world, 7, |m| (m.path, m.mode) = (0, 0));
    let t = synthetic_tables();
    let events = tick(&mut world, &t, &mut Random::seeded(3));
    let slot = moving(&world, 7).path & 0xf;
    let mission = usize::from(world.paths.alien.slots[usize::from(slot)].mission);
    // The scout (kind 2) gets 10 * mission + 2, given and running, the program bit set.
    let orders = world.orders.get(7).unwrap().orders;
    let program = (10 * mission + 2) as u8;
    assert_eq!(
        (orders.ordered, orders.program, orders.manual),
        (program, program, true)
    );
    assert!(matches!(events.last(), Some(Event::Program(c)) if c.slot == 7));
}

/// Every stock scenario ticks deterministically with the user's tables: within 16
/// ticks every idle alien robot has a path and a destination; robots driven at speed
/// 63 in all directions for 2,000 ticks end outside every hill or turned back by one,
/// and two runs give the same hash.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_stock_scenarios_tick() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let names = (0..13)
        .map(|n| format!("scen{n:02}a.usg"))
        .chain(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
    for name in names {
        let scenario = xeno_formats::usg::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
        let run = |drive: bool| {
            let mut sim = crate::Sim::load(&scenario, Random::seeded(7), tables.clone());
            let idle: Vec<EntityId> = sim
                .world
                .units(Side::Alien)
                .iter()
                .copied()
                .filter(|&id| {
                    super::robot(&sim.world, id)
                        .is_some_and(|(_, _, m)| m.path & 0x80 == 0 && m.mode & 1 == 0)
                })
                .collect();
            for _ in 0..16 {
                _ = sim.tick(&[]);
            }
            if !sim.world.paths.alien.used().is_empty() {
                for &id in &idle {
                    let (_, _, m) = super::robot(&sim.world, id).unwrap();
                    assert_ne!(m.path & 0x80, 0, "{name}: {id:?} has no path");
                    assert_eq!(m.goal, Movement::GOAL_POINT, "{name}");
                }
            }
            if drive {
                let ids: Vec<EntityId> = sim.world.iter().map(|(id, _)| id).collect();
                for (n, id) in ids.into_iter().enumerate() {
                    if let Some(m) = movement(&mut sim.world, id) {
                        (m.target_speed, m.target_heading) = (63, (n as u16).wrapping_mul(0x2400));
                    }
                }
                for _ in 0..2000 {
                    _ = sim.tick(&[]);
                }
                for (id, e) in sim.world.iter() {
                    if matches!(e.state, State::Robot(_)) {
                        assert!(
                            sim.world.terrain.hill_at(e.position).is_none() || {
                                // Inside only while backing out: its speed points out.
                                let (_, _, m) = super::robot(&sim.world, id).unwrap();
                                m.target_speed == 0
                            },
                            "{name}: {id:?} in a hill"
                        );
                    }
                }
            }
            sim.state_hash()
        };
        assert_eq!(run(true), run(true), "{name}");
        assert_ne!(run(true), run(false), "{name}");
    }
}
