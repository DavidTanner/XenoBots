use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{base, robot};
use crate::orders::{Orders, Robot};
use crate::world::{Components, Depot, Movement, Position};

fn parts(world: &World, id: u16) -> Components {
    match world.get(EntityId(id)).unwrap().damage {
        Damage::Robot(c) => c,
        _ => panic!("not a robot"),
    }
}

fn set_bits(world: &mut World, id: u16, bits: u8) {
    if let Some(Damage::Robot(c)) = world.get_mut(EntityId(id)).map(|e| &mut e.damage) {
        c.equipment |= bits;
    }
}

/// Runs one object pass.
fn pass(world: &mut World) -> Vec<Event> {
    crate::motion::tick(world, &synthetic_tables(), &mut Random::seeded(1))
}

#[test]
fn the_count_is_by_side_kind_and_place() {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Human, 0, 0, 4));
    world.insert(EntityId(1), robot(ObjectKind::Humanoid, Side::Human, 5, 5));
    world.insert(EntityId(2), robot(ObjectKind::Scorpion, Side::Human, 5, 5));
    world.insert(EntityId(3), robot(ObjectKind::Scout, Side::Alien, 5, 5));
    world.insert(EntityId(4), robot(ObjectKind::Scout, Side::Human, 5, 5));
    let mut relay = base(Side::Human, 3, 3, 0);
    (relay.kind, relay.state) = (ObjectKind::Relay, State::None);
    world.insert(EntityId(5), relay);
    set_bits(&mut world, 2, Bits::IN_BASE);
    let counts = |world: &World| match world.get(EntityId(0)).unwrap().state {
        State::Base { counts, .. } => counts,
        _ => unreachable!(),
    };
    pass(&mut world);
    // Tick 5: the relay, the humanoid out, the scorpion in, the human scout out.
    assert_eq!(counts(&world), [1, 0, 1, 1, 0, 0, 1]);
    // The next count is 256 ticks on.
    set_bits(&mut world, 1, Bits::IN_BASE);
    for _ in 0..255 {
        pass(&mut world);
    }
    assert_eq!(counts(&world), [1, 0, 1, 1, 0, 0, 1]);
    pass(&mut world);
    assert_eq!(counts(&world), [1, 1, 0, 1, 0, 0, 1]);
}

/// A human base at (0, 0) at tick 8 and robots `(slot, x, y)` in 1/256ths of a unit,
/// each with orders and its program driving.
fn home(robots: &[(u16, i32, i32)]) -> World {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Human, 0, 0, 8));
    for &(slot, x, y) in robots {
        let mut r = robot(ObjectKind::Humanoid, Side::Human, 0, 0);
        r.position = Position {
            x: Fixed(x << 8),
            y: Fixed(y << 8),
        };
        if let State::Robot(m) = &mut r.state {
            m.target_speed = 40;
        }
        world.insert(EntityId(slot), r);
        world.orders.insert(
            slot,
            Robot {
                kind: RobotKind::of(ObjectKind::Humanoid).unwrap(),
                side: Side::Human,
                orders: Orders {
                    program_drives: true,
                    ..Orders::given(8)
                },
            },
        );
    }
    world
}

fn movement(world: &World, id: u16) -> Movement {
    match world.get(EntityId(id)).unwrap().state {
        State::Robot(m) => m,
        _ => panic!("not a robot"),
    }
}

#[test]
fn the_first_robot_in_reach_is_taken_in() {
    // Slot 1 just out of reach (0x3b), 2 and 3 in it, 4 past 256 units.
    let mut world = home(&[(1, 0x3b, 0), (2, 0x20, 0x20), (3, 0, 0), (4, 0x1_0000, 0)]);
    for id in [1, 4] {
        set_bits(&mut world, id, Bits::ENTERING_BASE);
    }
    let events = pass(&mut world);
    assert!(events.contains(&Event::TakenIn(EntityId(2))));
    assert!(!events.contains(&Event::TakenIn(EntityId(3))));
    let e = world.get(EntityId(2)).unwrap();
    assert_eq!(
        e.position,
        Position {
            x: Fixed(0x2000 ^ i32::MIN),
            y: Fixed(0x2000 ^ i32::MIN),
        }
    );
    assert_eq!(
        parts(&world, 2).equipment,
        Bits::ENTERING_BASE | Bits::IN_BASE
    );
    assert_eq!(movement(&world, 2).target_speed, 0);
    assert!(!world.orders.get(2).unwrap().orders.program_drives);
    // Out of reach the bit is cleared; past 256 units it is not looked at.
    assert_eq!(parts(&world, 1).equipment, 0);
    assert_eq!(parts(&world, 4).equipment, Bits::ENTERING_BASE);
    assert_eq!(parts(&world, 3).equipment, 0);
    // Slot 2 is now far out of reach, so the next run takes slot 3.
    for _ in 0..127 {
        assert!(!pass(&mut world).contains(&Event::TakenIn(EntityId(3))));
    }
    assert!(pass(&mut world).contains(&Event::TakenIn(EntityId(3))));
    assert_eq!(
        parts(&world, 2).equipment,
        Bits::ENTERING_BASE | Bits::IN_BASE
    );
}

#[test]
fn a_robot_just_out_is_not_taken_in_again() {
    // The bit still set stops the scan with nothing taken.
    let mut world = home(&[(1, 0, 0), (2, 0, 0)]);
    set_bits(&mut world, 1, Bits::ENTERING_BASE);
    let events = pass(&mut world);
    assert!(!events.iter().any(|e| matches!(e, Event::TakenIn(_))));
    assert_eq!(parts(&world, 2).equipment, 0);
}

/// On stock data a robot recalled to its base (scen00a's first human humanoid, slot 2)
/// drives home and is taken in, out of the field.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_recalled_robot_is_taken_in() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, Random::seeded(1), tables);
    let id = EntityId(2);
    sim.recall(id);
    let taken = (1..=20_000).find(|_| sim.tick(&[]).contains(&Event::TakenIn(id)));
    // About 46 s of play in the port; not timed against the original.
    assert!(taken.is_some_and(|t| t > 1000), "{taken:?}");
    let e = sim.world.get(id).unwrap();
    assert!(in_base(&e.damage));
    assert!(e.position.x.0.unsigned_abs() > 0x4000_0000);
}

#[test]
fn the_third_slider_makes_relays_up_to_the_most() {
    let b = synthetic_tables().bases;
    let mut d = Depot {
        relays: 8,
        relay_wait: b.relay_wait - 1,
        power: [0, 0, 0, 0x80],
        ..Depot::default()
    };
    make_relays(&mut d, &b);
    assert_eq!(
        (d.relays, d.relay_clock, d.relay_wait),
        (8, 0x80, b.relay_wait - 1)
    );
    make_relays(&mut d, &b);
    assert_eq!((d.relays, d.relay_clock, d.relay_wait), (9, 0, 0));
    // At the most (9) the carries go on counting, and nothing is made.
    d.relay_wait = b.relay_wait + 5;
    d.relay_clock = 0x80;
    make_relays(&mut d, &b);
    assert_eq!((d.relays, d.relay_wait), (9, b.relay_wait + 6));
}

#[test]
fn a_slider_moves_through_the_pool_or_the_others() {
    // Least 6, most 0xf0, step 3.
    let b = synthetic_tables().bases;
    let moved = |power: [u8; 4], slider: u8| {
        let mut d = Depot {
            power,
            slider,
            ..Depot::default()
        };
        slide(&mut d, &b);
        d.power
    };
    assert_eq!(moved([0, 0x80, 0x40, 0x40], 1), [3, 0x7d, 0x40, 0x40]);
    assert_eq!(moved([0, 5, 0x40, 0x40], 1), [0, 5, 0x40, 0x40]);
    // Raising takes the step from the pool, else one from each of the others, else
    // the step from the one that has it.
    assert_eq!(moved([6, 0x80, 0x40, 0x40], 4), [3, 0x80, 0x43, 0x40]);
    assert_eq!(moved([0, 0x80, 0x40, 0x40], 4), [0, 0x7f, 0x43, 0x3f]);
    assert_eq!(moved([0, 0x80, 0x40, 2], 6), [0, 0x7f, 0x3f, 5]);
    assert_eq!(moved([0, 5, 0x40, 0x40], 2), [0, 8, 0x3f, 0x3f]);
    assert_eq!(moved([0, 0x80, 2, 5], 6), [0, 0x7d, 2, 8]);
    assert_eq!(moved([0, 2, 5, 0xf4], 4), [0, 2, 8, 0xf1]);
    assert_eq!(moved([0, 0xf1, 0, 0], 2), [0, 0xf1, 0, 0]);
    assert_eq!(moved([0, 0x80, 0x40, 0x40], 7), [0, 0x80, 0x40, 0x40]);
}

/// A human base (slot 0) with the repairs' slider at 0x80 (a step every other run of
/// a period of 0x100), and robots in its unit list.
fn workshop(robots: &[(u16, ObjectKind)]) -> World {
    let mut world = World::default();
    let mut b = base(Side::Human, 0, 0, 0);
    if let State::Base { depot, .. } = &mut b.state {
        (depot.power, depot.relays) = ([0, 0, 0x80, 0], 20);
    }
    world.insert(EntityId(0), b);
    world.bases[0] = Some(EntityId(0));
    for &(slot, kind) in robots {
        world.insert(EntityId(slot), robot(kind, Side::Human, 3, 3));
        world.units[0].push(EntityId(slot));
    }
    world
}

#[test]
fn repairs_mend_one_thing_a_step_in_order() {
    let b = Bases {
        whole: 100,
        repair_step: 4,
        ..synthetic_tables().bases
    };
    // Slot 1 is damaged but out in the field; slot 2 in the base.
    let mut world = workshop(&[(1, ObjectKind::Humanoid), (2, ObjectKind::Humanoid)]);
    for id in [1, 2] {
        if let Some(Damage::Robot(c)) = world.get_mut(EntityId(id)).map(|e| &mut e.damage) {
            (c.helm, c.weapons, c.systems) = (93, 0x01, 0x42);
        }
    }
    set_bits(&mut world, 2, Bits::IN_BASE);
    // The robot's tick adds its total up after the base's (the base is in slot 0), so
    // a step goes by the total the robot's last tick left.
    let tables = synthetic_tables();
    let total = |world: &mut World| crate::damage::total(world, EntityId(2), &tables, &mut vec![]);
    total(&mut world);
    let repairing = |world: &mut World| depot(world, EntityId(0)).unwrap().repairing;
    let mut steps = Vec::new();
    for _ in 0..12 {
        repair(&mut world, EntityId(0), &b);
        total(&mut world);
        let c = parts(&world, 2);
        steps.push((
            repairing(&mut world).map(|r| r.0),
            c.helm,
            c.weapons,
            c.systems,
        ));
    }
    assert_eq!(
        steps,
        [
            (None, 93, 1, 0x42),
            (Some(2), 93, 1, 0x42), // picked, no step yet
            (Some(2), 93, 1, 0x42),
            (Some(2), 96, 1, 0x42), // the helm, rounded down to 4s
            (Some(2), 96, 1, 0x42),
            (Some(2), 100, 1, 0x42),
            (Some(2), 100, 1, 0x42),
            (Some(2), 100, 0, 0x02), // the weapon; the total was 10, so kept
            (Some(2), 100, 0, 0x02),
            (None, 100, 0, 0), // the thermal camera; the total was 0, so released
            (None, 100, 0, 0),
            (None, 100, 0, 0),
        ]
    );
    assert_eq!(parts(&world, 1).helm, 93);
}

#[test]
fn a_whole_scorpion_is_restocked_from_the_store() {
    let b = Bases {
        whole: 100,
        ..synthetic_tables().bases
    };
    let mut world = workshop(&[(1, ObjectKind::Scorpion)]);
    if let Some(e) = world.get_mut(EntityId(1)) {
        if let Damage::Robot(c) = &mut e.damage {
            (c.weapons, c.equipment) = (0x08, 0x80 | Bits::IN_BASE);
        }
        if let State::Robot(m) = &mut e.state {
            (m.relays, m.guns.missiles) = (3, 0);
        }
    }
    repair_step(&mut world, EntityId(0), EntityId(1), &b);
    // Up to 10 carried and 4 missiles (the synthetic restock), 7 from the store.
    let c = parts(&world, 1);
    assert_eq!((c.weapons, c.equipment), (0, Bits::IN_BASE));
    let m = movement(&world, 1);
    assert_eq!((m.relays, m.guns.missiles), (10, 4));
    assert_eq!(depot(&mut world, EntityId(0)).unwrap().relays, 13);
}

#[test]
fn the_alien_base_launches_whole_robots_in_it() {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Alien, 9, 9, 12));
    world.bases[1] = Some(EntityId(0));
    for slot in [1, 2, 3] {
        let mut r = robot(ObjectKind::Humanoid, Side::Alien, 0, 0);
        r.position.x.0 ^= i32::MIN;
        if let (State::Robot(m), Damage::Robot(c)) = (&mut r.state, &mut r.damage) {
            (m.path, m.mode, m.speed, m.contacts) = (0x83, 0x08, 30, 2);
            c.equipment = Bits::ENTERING_BASE | Bits::IN_BASE;
            c.helm = if slot == 2 { 50 } else { 100 };
            // The total the robots' last ticks left; the base in slot 0 goes first.
            c.total = if slot == 2 { 12 } else { 0 };
        }
        world.insert(EntityId(slot), r);
        world.orders.insert(
            slot,
            Robot {
                kind: RobotKind::of(ObjectKind::Humanoid).unwrap(),
                side: Side::Alien,
                orders: Orders::given(8),
            },
        );
    }
    // Slot 3 is not in the unit list.
    world.units[1] = vec![EntityId(1), EntityId(2)];
    let events = pass(&mut world);
    assert!(events.contains(&Event::Launched(EntityId(1))));
    assert!(!events.contains(&Event::Launched(EntityId(2))));
    assert!(!events.contains(&Event::Launched(EntityId(3))));
    let e = world.get(EntityId(1)).unwrap();
    assert_eq!(e.position, world.get(EntityId(0)).unwrap().position);
    assert_eq!(parts(&world, 1).equipment, Bits::ENTERING_BASE);
    let m = movement(&world, 1);
    assert_eq!((m.path, m.mode, m.speed, m.contacts), (0x03, 0, 0, 0));
    let orders = world.orders.get(1).unwrap().orders;
    // The synthetic launch program, 7, with the program driving.
    assert_eq!((orders.program, orders.program_drives), (7, true));
    // A human base launches nothing.
    let mut world = workshop(&[(1, ObjectKind::Humanoid)]);
    set_bits(&mut world, 1, Bits::IN_BASE);
    if let Some(State::Base { tick, .. }) = world.get_mut(EntityId(0)).map(|e| &mut e.state) {
        *tick = 12;
    }
    assert!(!pass(&mut world).contains(&Event::Launched(EntityId(1))));
}

/// On stock data the alien base launches a robot it took in (scen00a: an alien robot
/// comes home on its path, is whole, and leaves within 16 ticks at the base).
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_the_alien_base_launches_a_robot_it_took_in() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, Random::seeded(1), tables);
    let base = sim.world.base(Side::Alien).unwrap();
    let mut taken = None;
    for t in 1..=30_000u32 {
        for event in sim.tick(&[]) {
            match (event, taken) {
                (Event::TakenIn(r), None) if sim.world.get(r).unwrap().side == Side::Alien => {
                    taken = Some((r, t));
                }
                (Event::Launched(r), Some((took, at))) if r == took => {
                    assert!(t - at <= 16, "{t} {at}");
                    let e = sim.world.get(r).unwrap();
                    // At the base, and moved on by its own tick since (it ticks after
                    // the base).
                    let from = sim.world.get(base).unwrap().position;
                    assert!(approx_distance(from, e.position).0 < 0x1000);
                    assert!(!in_base(&e.damage));
                    return;
                }
                _ => {}
            }
        }
    }
    panic!("nothing launched: {taken:?}");
}
