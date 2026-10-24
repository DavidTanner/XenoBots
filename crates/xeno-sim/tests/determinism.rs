//! The sim's determinism (`docs/sim.md`): the same seed, world and input log give the
//! same state and hash; a different seed or input gives a different one.

use proptest::prelude::*;
use xeno_formats::usg::{ObjectKind, Side};
use xeno_sim::input::{InputEvent, InputLog, MouseButton, ScanCode};
use xeno_sim::{Damage, Entity, EntityId, Fixed, Position, Sim};

fn button() -> impl Strategy<Value = MouseButton> {
    prop_oneof![
        Just(MouseButton::Left),
        Just(MouseButton::Right),
        Just(MouseButton::Middle),
    ]
}

fn event() -> impl Strategy<Value = InputEvent> {
    // Plain and E0 codes, a few of each so downs and ups meet.
    let code = (0u16..8, any::<bool>())
        .prop_map(|(c, e0)| ScanCode(if e0 { 0xe010 | c } else { 0x10 | c }));
    prop_oneof![
        (code.clone(), any::<bool>())
            .prop_map(|(code, repeat)| InputEvent::KeyDown { code, repeat }),
        code.prop_map(|code| InputEvent::KeyUp { code }),
        any::<char>().prop_map(InputEvent::Text),
        (0u16..320, 0u16..200).prop_map(|(x, y)| InputEvent::MouseMove { x, y }),
        button().prop_map(|button| InputEvent::MouseDown { button }),
        button().prop_map(|button| InputEvent::MouseUp { button }),
    ]
}

/// A log over a few hundred retraces: each entry waits 0 to 9 retraces after the last.
fn log() -> impl Strategy<Value = (InputLog, u64)> {
    prop::collection::vec((0u64..10, prop::collection::vec(event(), 0..4)), 0..60).prop_map(
        |entries| {
            let (mut log, mut retrace) = (InputLog::default(), 1);
            for (wait, events) in entries {
                retrace += wait;
                log.record(retrace, events);
            }
            (log, retrace + 1)
        },
    )
}

fn entities() -> impl Strategy<Value = Vec<(u16, Entity)>> {
    let kind = prop_oneof![
        Just(0u16),
        Just(1),
        Just(2),
        Just(3),
        Just(4),
        Just(16),
        any::<u16>()
    ];
    let side = prop_oneof![Just(Side::Human), Just(Side::Alien)];
    prop::collection::vec(
        (
            0u16..240,
            kind,
            side,
            any::<i32>(),
            any::<i32>(),
            any::<u16>(),
            any::<u16>(),
        ),
        0..20,
    )
    .prop_map(|objects| {
        objects
            .into_iter()
            .map(|(slot, kind, side, x, y, heading, taken)| {
                let position = Position {
                    x: Fixed(x),
                    y: Fixed(y),
                };
                (
                    slot,
                    Entity {
                        kind: ObjectKind::from(kind),
                        side,
                        position,
                        heading,
                        radius: 0xa00,
                        damage: Damage::Taken(taken),
                    },
                )
            })
            .collect()
    })
}

fn run(seed: u32, objects: &[(u16, Entity)], log: &InputLog, retraces: u64) -> Sim {
    let mut sim = Sim::new(seed);
    for (slot, entity) in objects {
        sim.world.insert(EntityId(*slot), entity.clone());
    }
    sim.run(log, retraces);
    sim
}

proptest! {
    #[test]
    fn the_same_seed_and_input_give_the_same_hash(
        seed in any::<u32>(),
        objects in entities(),
        (log, retraces) in log(),
    ) {
        let a = run(seed, &objects, &log, retraces);
        let b = run(seed, &objects, &log.clone(), retraces);
        prop_assert_eq!(a.state_hash(), b.state_hash());
        prop_assert_eq!(a, b);
    }

    #[test]
    fn another_seed_or_input_gives_another_hash(
        seeds in (any::<u32>(), any::<u32>()).prop_filter("two seeds", |(a, b)| a != b),
        (log, retraces) in log(),
        code in 0x40u16..0x50,
    ) {
        let a = run(seeds.0, &[], &log, retraces);
        prop_assert_ne!(a.state_hash(), run(seeds.1, &[], &log, retraces).state_hash());
        // The log never holds 0x40..0x50, so one more press of it is new state.
        let mut pressed = log.clone();
        pressed.record(retraces, [InputEvent::KeyDown { code: ScanCode(code), repeat: false }]);
        prop_assert_ne!(a.state_hash(), run(seeds.0, &[], &pressed, retraces).state_hash());
        // So is one more retrace.
        prop_assert_ne!(a.state_hash(), run(seeds.0, &[], &log, retraces + 1).state_hash());
    }
}
