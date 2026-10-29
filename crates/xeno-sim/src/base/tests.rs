use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{base, robot};
use crate::orders::{Orders, Robot};
use crate::world::{Components, Movement, Position};

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
