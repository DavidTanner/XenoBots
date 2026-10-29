use super::*;
use crate::motion::synthetic_tables;
use crate::orders::{OrderBook, Orders, Robot};
use crate::world::{Components, Damage, Entity, Fixed, State};
use xeno_formats::script::Scripts;
use xeno_formats::usg::{ObjectKind, Side};

/// Made-up scripts: `bytes` at `0x100`, every program starting there, the stock
/// opcode times and both event tables sending every event to the start too.
pub(crate) fn scripts(bytes: &[u8]) -> Scripts {
    let mut times = vec![0u16; 0x88];
    for (ops, time) in [
        (&[0x04, 0x06, 0x12, 0x1f, 0x59, 0x5a, 0x5b, 0x5c][..], 1),
        (&[0x0a, 0x18], 255),
        (&[0x14, 0x65, 0x70], 60),
        (&[0x16], 3600),
        (
            &[0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x71, 0x73],
            30,
        ),
        (&[0x72, 0x78], 15),
    ] {
        ops.iter().for_each(|&op| times[op] = time);
    }
    Scripts {
        start: 0x100,
        bytes: bytes.to_vec(),
        programs: vec![0x100; 31],
        times,
        program_events: vec![0x100; 13],
        pilot_events: vec![0x100; 13],
    }
}

fn at(x: i16, y: i16) -> Position {
    Position {
        x: Fixed::from_int(x),
        y: Fixed::from_int(y),
    }
}

/// An alien humanoid (slot 1) at (10, 10) heading for (10, 20), its program driving,
/// running `bytes` from the start with no wait; its base (slot 0) at (0, 0).
fn setup(bytes: &[u8]) -> (World, Tables) {
    let mut world = World::default();
    let mut m = Movement {
        drive: 0xff,
        goal: Movement::GOAL_POINT,
        destination: at(10, 20),
        ..Movement::default()
    };
    m.script = Script {
        pointer: 0x100,
        sp: 10,
        ..Script::default()
    };
    world.insert(
        EntityId(1),
        Entity {
            kind: ObjectKind::Humanoid,
            side: Side::Alien,
            position: at(10, 10),
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
            state: State::Robot(m),
        },
    );
    world.insert(
        EntityId(0),
        Entity {
            kind: ObjectKind::Base,
            side: Side::Alien,
            position: at(0, 0),
            heading: 0,
            radius: 0x2300,
            damage: Damage::Taken(0),
            state: State::Base {
                tick: 0,
                kills: 0,
                losses: 0,
                destroyed: false,
                end_step: 0,
                look: 0,
            },
        },
    );
    world.bases[1] = Some(EntityId(0));
    let mut book = OrderBook::default();
    book.insert(
        1,
        Robot {
            kind: RobotKind::Humanoid,
            side: Side::Alien,
            orders: Orders {
                program_drives: true,
                ..Orders::given(8)
            },
        },
    );
    world.orders = book;
    let tables = Tables {
        scripts: scripts(bytes),
        ..synthetic_tables()
    };
    (world, tables)
}

fn m(world: &World) -> Movement {
    match world.get(EntityId(1)).unwrap().state {
        State::Robot(m) => m,
        other => panic!("{other:?}"),
    }
}

fn steps(world: &mut World, tables: &Tables, n: usize) -> Vec<Event> {
    let (mut random, mut events) = (Random::seeded(1), Vec::new());
    for _ in 0..n {
        step(world, EntityId(1), tables, &mut random, &mut events);
    }
    events
}

#[test]
fn programs_start_staggered() {
    let mut world = World::default();
    let waits: Vec<u16> = (0..7).map(|_| stagger(&mut world)).collect();
    assert_eq!(waits, [39, 29, 19, 9, 48, 38, 28]);
}

#[test]
fn an_opcode_waits_its_time_and_only_the_program_steps_the_script() {
    // Stop turning (time 0), then set the high speed (time 30), then halt.
    let (mut world, tables) = setup(&[0x50, 0x55, 0x0a]);
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.pointer, 0x101);
    steps(&mut world, &tables, 1);
    assert_eq!(
        (m(&world).script.pointer, m(&world).target_speed),
        (0x102, 56)
    );
    steps(&mut world, &tables, 30);
    assert_eq!(m(&world).script.pointer, 0x102, "still waiting");
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.pointer, 0x102, "halted on itself");
    // With the pilot driving, the script stands still.
    let (mut world, tables) = setup(&[0x50, 0x55]);
    world.orders.set_program_drives(1, false);
    steps(&mut world, &tables, 5);
    assert_eq!(m(&world).script.pointer, 0x100);
}

#[test]
fn a_patrol_steers_to_its_point_and_asks_for_the_next_one_there() {
    // A made-up loop in the shape of a patrol: near the point? Else high speed, steer,
    // wait, again. There: stop and ask for the next point.
    let patrol = [
        0x5d, 0x40, 0x00, 0x43, 0x06, 0x55, 0x5f, 0x15, 0x02, 0x42, 0xf5, 0x65, 0x77, 0x04, 0x0a,
    ];
    let (mut world, tables) = setup(&patrol);
    set_program(&mut world, EntityId(1), 8, &tables, &mut Vec::new());
    assert_eq!(m(&world).script.wait, 39);
    // The destination is due +y: heading 0. Make it due +x first.
    movement(&mut world, EntityId(1)).unwrap().destination = at(20, 10);
    steps(&mut world, &tables, 40 + 1 + 1 + 1 + 31);
    let after = m(&world);
    assert_eq!((after.target_speed, after.target_heading), (56, 0x4000));
    // Arrived: the robot stops and asks for its next point.
    world.get_mut(EntityId(1)).unwrap().position = at(20, 10);
    steps(&mut world, &tables, 500);
    let after = m(&world);
    assert_eq!(after.target_speed, 0);
    assert_ne!(after.path & Movement::NEXT, 0);
}

#[test]
fn the_bearing_points_at_the_goal() {
    let t = synthetic_tables();
    let o = at(0, 0);
    for ((x, y), expected) in [
        ((0, 5), 0x0000),
        ((5, 0), 0x4000),
        ((0, -5), 0x8000),
        ((-5, 0), 0xc000),
        ((5, 5), 0x2000),
        ((-3, 4), 0x0001),
    ] {
        let b = bearing(&t.atan, o, at(x, y));
        let exact = (f64::from(x).atan2(f64::from(y)) / std::f64::consts::TAU * 65536.0)
            .rem_euclid(65536.0);
        assert!(
            (f64::from(b) - exact).abs() < 64.0,
            "{x}, {y}: {b:#x} {exact}"
        );
        if expected != 1 {
            assert_eq!(b, expected, "{x}, {y}");
        }
    }
    assert_eq!(bearing(&t.atan, o, o), 0);
}

#[test]
fn an_event_runs_the_program_s_handler_and_returns() {
    // A header sending event 5 to `0x109` (a return); then a test and a halt.
    let (mut world, tables) = setup(&[0x0f, 0x05, 0x09, 0x01, 0x11, 0x0b, 0x0a, 0x00, 0x00, 0x06]);
    steps(&mut world, &tables, 3);
    assert_eq!(m(&world).script.pointer, 0x106, "halted with the carry set");
    post(movement(&mut world, EntityId(1)).unwrap(), 5);
    steps(&mut world, &tables, 1);
    let s = m(&world).script;
    assert_eq!(
        (s.pointer, s.events, s.sp, s.flags & BUSY),
        (0x109, 0, 8, BUSY)
    );
    steps(&mut world, &tables, 1);
    let s = m(&world).script;
    assert_eq!((s.pointer, s.sp, s.flags), (0x106, 10, CARRY));
}

#[test]
fn the_program_taking_over_posts_its_event() {
    let (mut world, tables) = setup(&[0x1f, 0x0a]);
    let events = steps(&mut world, &tables, 1);
    assert!(!world.orders.get(1).unwrap().orders.program_drives);
    assert!(matches!(events[..], [Event::Program(_)]));
    assert_eq!(m(&world).script.events, 1 << EVENT_PILOT);
}

/// The alien robots of `scen00a`, on PATROL (and a scorpion on DENSE NET, which halts
/// at the relay net's opcodes, T-113), all but one move within a minute of game time,
/// and some are still driving then.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_stock_robots_drive_to_their_points() {
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
    let mut sim = crate::Sim::load(&scenario, Random::seeded(7), tables.clone());
    let start: Vec<(EntityId, Position)> = sim
        .world
        .units(Side::Alien)
        .iter()
        .map(|&id| (id, sim.world.get(id).unwrap().position))
        .collect();
    for _ in 0..70 * 60 {
        _ = sim.tick(&[]);
    }
    let moved = start
        .iter()
        .filter(|&&(id, p)| sim.world.get(id).unwrap().position != p)
        .count();
    // The scorpion's DENSE NET halts at the relay net (T-113).
    assert!(moved >= start.len() - 1, "{moved} of {}", start.len());
    let heading_out = start.iter().any(|&(id, _)| {
        let (_, _, m) = robot(&sim.world, id).unwrap();
        m.target_speed > 0
    });
    assert!(heading_out);
}

#[test]
fn arithmetic_works_on_a_field_or_the_stack_with_the_flags() {
    // (opcode and operand, the start value, the result, carry, zero, sign); the byte
    // field is +0x2c, the word field +0x28.
    let field_byte = |op: u8, v: u8| vec![op, 0x2c, v];
    let field_word = |op: u8, v: u16| [&[op, 0x28][..], &v.to_le_bytes()].concat();
    let cases: [(Vec<u8>, u16, u16, bool, bool, bool); 8] = [
        (field_byte(0x2a, 0x20), 0xf0, 0x10, true, false, false), // add
        (field_byte(0x30, 0x20), 0x10, 0xf0, true, false, true),  // sub
        (field_byte(0x34, 0x20), 0x20, 0x20, false, true, false), // cmp: unchanged
        (field_byte(0x36, 0xf0), 0x0f, 0x0f, false, true, false), // test: unchanged
        (field_byte(0x3a, 0xf0), 0x3f, 0xff, false, false, true), // or
        (field_byte(0x40, 0x0f), 0x3c, 0x0c, false, false, false), // and
        (field_word(0x2b, 1), 0xffff, 0, true, true, false),      // add a word
        (field_word(0x41, 0x8000), 0xffff, 0x8000, false, false, true), // and a word
    ];
    for (op, start, result, carry, zero, sign) in cases {
        let word = op[1] == 0x28;
        let set: Vec<u8> = if word {
            [&[0x4e, 0x28][..], &start.to_le_bytes()].concat()
        } else {
            vec![0x4d, 0x2c, start as u8]
        };
        let (mut world, tables) = setup(&[set, op.clone(), vec![0x0a]].concat());
        steps(&mut world, &tables, 2);
        let after = m(&world);
        let value = if word {
            after.target_speed as u16
        } else {
            u16::from(after.path)
        };
        let f = after.script.flags;
        assert_eq!(
            (value, f & CARRY != 0, f & ZERO != 0, f & SIGN != 0),
            (result, carry, zero, sign),
            "{op:02x?}"
        );
    }
    // On the stack: push +0x2c, work on the top, pop it back into +0x2c.
    for (op, result) in [
        (vec![0x2c, 3], 8), // add
        (vec![0x2e, 3], 2), // sub
        (vec![0x32, 3], 5), // cmp: unchanged
        (vec![0x38, 2], 5), // test: unchanged
        (vec![0x3c, 4], 5), // or
        (vec![0x3e, 6], 4), // and
    ] {
        let script = [&[0x4d, 0x2c, 5, 0x24, 0x2c][..], &op, &[0x28, 0x2c, 0x0a]].concat();
        let (mut world, tables) = setup(&script);
        steps(&mut world, &tables, 4);
        assert_eq!(m(&world).path, result, "{op:02x?}");
        assert_eq!(m(&world).script.sp, EMPTY);
    }
    // A word on the stack: push +0x28, add, pop.
    let script = [
        0x4e, 0x28, 0xff, 0x00, 0x25, 0x28, 0x2d, 0x01, 0x01, 0x29, 0x28, 0x0a,
    ];
    let (mut world, tables) = setup(&script);
    steps(&mut world, &tables, 4);
    assert_eq!(m(&world).target_speed, 0x200);
}

#[test]
fn the_conditional_jumps_follow_the_flags() {
    // Compare +0x2c (a) with b, then the jump; taken, it skips setting +0x2c to 0x99.
    for (a, b, taken) in [
        (1u8, 2u8, [0x43, 0x45, 0x48, 0x49, 0x4b]), // carry, sign
        (2, 2, [0x44, 0x45, 0x47, 0x4a, 0x4c]),     // zero
        (3, 2, [0x44, 0x46, 0x48, 0x4a, 0x4c]),     // none
    ] {
        for jump in 0x43..=0x4c {
            let script = [
                0x4d, 0x2c, a, 0x34, 0x2c, b, jump, 3, 0x4d, 0x2c, 0x99, 0x0a,
            ];
            let (mut world, tables) = setup(&script);
            steps(&mut world, &tables, 4);
            let skipped = m(&world).path == a;
            assert_eq!(skipped, taken.contains(&jump), "{a} vs {b}, {jump:#x}");
        }
    }
}

#[test]
fn a_message_waits_in_the_panel_and_is_typed_after_the_move() {
    // Message 1 ("AB", a CR and a high-colour C in the synthetic strings), then halt.
    let (mut world, tables) = setup(&[0x74, 0x01, 0x00, 0x0a]);
    let mut random = Random::seeded(1);
    crate::motion::tick(&mut world, &tables, &mut random);
    assert_eq!(m(&world).panel.shown(), b"A");
    (0..5).for_each(|_| _ = crate::motion::tick(&mut world, &tables, &mut random));
    assert_eq!(m(&world).panel.shown(), b"AB\x0c\xc3\x0c");
    assert_eq!(m(&world).panel.queue, [0; 5]);
}

#[test]
fn the_gear_opcodes_step_the_gear_and_the_set_speeds_match_it() {
    // High speed, a gear up, a gear down twice, stop, halt: each waits 30 ticks.
    let (mut world, tables) = setup(&[0x55, 0x51, 0x52, 0x52, 0x53, 0x0a]);
    let mut seen = Vec::new();
    for _ in 0..5 {
        steps(&mut world, &tables, 1);
        seen.push((m(&world).target_speed, m(&world).gear));
        steps(&mut world, &tables, 30);
    }
    assert_eq!(seen, [(56, 8), (95, 10), (56, 8), (28, 6), (0, 4)]);
    assert_eq!(m(&world).script.pointer, 0x105, "past every gear opcode");
}

#[test]
fn the_target_opcodes_look_lock_measure_and_steer() {
    // Look, test +0xe6 against 0xffff, push the target's distance, steer to it, is it
    // a base? Halt.
    let (mut world, tables) = setup(&[0x71, 0x37, 0xe6, 0xff, 0xff, 0x6b, 0x6c, 0x73, 0x0a]);
    let mut enemy = world.get(EntityId(1)).unwrap().clone();
    (enemy.side, enemy.position) = (Side::Human, at(14, 10));
    world.insert(EntityId(2), enemy);
    steps(&mut world, &tables, 31);
    assert_eq!(m(&world).target, Some(EntityId(2)));
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.flags & ZERO, 0, "a target is not 0");
    steps(&mut world, &tables, 2);
    assert_eq!(top(&m(&world), true), 0x400);
    assert_eq!(m(&world).target_heading, 0x4000);
    assert_eq!(m(&world).goal, Movement::GOAL_POINT, "put back");
    assert_eq!(m(&world).script.flags & CARRY, 0);
    steps(&mut world, &tables, 31);
    assert_eq!(m(&world).script.flags & CARRY, CARRY, "not a base");
    // With no enemy near, the look and the steering set the carry.
    let (mut world, tables) = setup(&[0x71, 0x0a]);
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.flags & CARRY, CARRY);
    let (mut world, tables) = setup(&[0x6c, 0x0a]);
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.flags & CARRY, CARRY);
}

#[test]
fn steering_for_the_nearest_enemy_sets_the_carry_with_none() {
    let (mut world, tables) = setup(&[0x63, 0x0a]);
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.flags & CARRY, CARRY);
    let (mut world, tables) = setup(&[0x63, 0x0a]);
    let mut enemy = world.get(EntityId(1)).unwrap().clone();
    (enemy.side, enemy.position) = (Side::Human, at(6, 10));
    world.insert(EntityId(2), enemy);
    steps(&mut world, &tables, 1);
    assert_eq!(m(&world).script.flags & CARRY, 0);
    assert_eq!(m(&world).target_heading, 0xc000);
}
