use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{at, robot};
use crate::world::{Damage, Entity, Fixed};
use xeno_formats::usg::ObjectKind;

const RELAY: EntityId = EntityId(2);
const SCORPION: EntityId = EntityId(1);

/// A relay at (128, 128) as a scorpion lays it, its clock one tick before its state
/// runs, and the scorpion on it.
fn laid(tables: &Tables) -> World {
    let mut world = World::default();
    let (state, model_height) = Relay::made(0x0f, tables);
    world.insert(
        RELAY,
        Entity {
            kind: ObjectKind::Relay,
            side: Side::Human,
            position: at(128, 128),
            z: Fixed(0),
            model_height,
            heading: 0,
            radius: 0x500,
            damage: Damage::Taken(0),
            state: State::Relay(state),
        },
    );
    world.insert(SCORPION, robot(ObjectKind::Scorpion, Side::Alien, 128, 128));
    world
}

fn relay(world: &World) -> (Relay, u16) {
    let e = world.get(RELAY).unwrap();
    match e.state {
        State::Relay(r) => (r, e.model_height),
        other => panic!("{other:?}"),
    }
}

/// 16 ticks: the state runs once, on the first.
fn run(world: &mut World, tables: &Tables) -> (Relay, u16) {
    for _ in 0..16 {
        tick(world, RELAY, tables);
    }
    relay(world)
}

#[test]
fn a_laid_relay_rises_once_the_scorpion_moves_on_and_folds_when_it_comes_back() {
    let tables = synthetic_tables();
    let mut world = laid(&tables);
    // Made folded, unpowered, with stage 0's look: frames 3, colour 1, height 0x100.
    let (r, height) = relay(&world);
    assert_eq!(
        (r.state, r.stage, r.frames, height),
        (FOLDED, 0, [0, 3, 3, 3, 3, 1], 0x100)
    );
    // The scorpion on it keeps it folded.
    let (r, _) = run(&mut world, &tables);
    assert_eq!((r.state, r.clock), (FOLDED, 0x1f));
    // Moved on two units, it rises a stage every 16 ticks and is up at 3.
    world.get_mut(SCORPION).unwrap().position = at(130, 128);
    let mut seen = Vec::new();
    for _ in 0..5 {
        let (r, height) = run(&mut world, &tables);
        seen.push((r.state, r.stage, r.frames[1], height, r.flags));
    }
    assert_eq!(
        seen,
        [
            (RISING, 0, 3, 0x100, 0),
            (RISING, 1, 2, 0x200, 0),
            (RISING, 2, 1, 0x300, 0),
            (UP, 3, 0, 0x400, 0),
            // Up, it is in the field; unpowered, not linked.
            (UP, 3, 0, 0x400, flag::IN_FIELD),
        ]
    );
    // Within the fold's reach (0x32 / 256 units) again, it folds a stage a run.
    world.get_mut(SCORPION).unwrap().position.x = Fixed((128 << 16) + 0x31 * 256);
    let stages: Vec<_> = (0..5)
        .map(|_| run(&mut world, &tables))
        .map(|(r, _)| (r.state, r.stage))
        .collect();
    assert_eq!(
        stages,
        [
            (FOLDING, 3),
            (FOLDING, 2),
            (FOLDING, 1),
            (FOLDED, 0),
            (FOLDED, 0)
        ]
    );
    // Folded, it is out of the field.
    assert_eq!(relay(&world).0.flags, 0);
}

#[test]
fn the_state_runs_when_the_clock_s_low_bits_turn_0_and_the_fold_s_reach_is_strict() {
    let tables = synthetic_tables();
    let mut world = laid(&tables);
    // Exactly the reach away is not near.
    world.get_mut(SCORPION).unwrap().position.x = Fixed((128 << 16) + 0x32 * 256);
    if let Some(State::Relay(r)) = world.get_mut(RELAY).map(|e| &mut e.state) {
        r.clock = 0x0e;
    }
    tick(&mut world, RELAY, &tables);
    assert_eq!(
        (relay(&world).0.clock, relay(&world).0.state),
        (0x0f, FOLDED)
    );
    tick(&mut world, RELAY, &tables);
    assert_eq!(
        (relay(&world).0.clock, relay(&world).0.state),
        (0x10, RISING)
    );
}

#[test]
fn an_up_relay_is_linked_while_powered_and_coloured_by_its_side() {
    let tables = synthetic_tables();
    let mut world = laid(&tables);
    world.remove(SCORPION);
    for (side, colour) in [(Side::Human, 0), (Side::Alien, 2)] {
        let e = world.get_mut(RELAY).unwrap();
        e.side = side;
        if let State::Relay(r) = &mut e.state {
            (r.state, r.stage, r.power, r.clock) = (UP, 3, 4, 0xff);
        }
        let (r, _) = run(&mut world, &tables);
        assert_eq!((r.flags, r.frames[5]), (FLAGS, colour), "{side:?}");
    }
}

#[test]
fn a_file_s_relay_loads_with_its_state() {
    let link = |state| usg::Link {
        state,
        unknown_01: 0,
        unknown_02: 0,
    };
    let r = usg::Relay {
        frames: [0, 1, 1, 1, 1, 2],
        clock: 0x42,
        state: UP,
        stage: 2,
        power: 5,
        power_side: 2,
        links: [link(1), link(2), link(0), link(2)],
        damage: 3,
    };
    let s = Relay::of(&r, 0x5a);
    assert_eq!(
        (s.frames, s.clock, s.state, s.stage, s.power, s.power_side),
        (r.frames, 0x42, UP, 2, 5, 2)
    );
    assert_eq!((s.links, s.flags), ([1, 2, 0, 2], FLAGS));
}

/// scen00a with its alien scorpion (on DENSE NET) the only robot, and the scorpion.
pub(crate) fn stock_scorpion_alone() -> Option<(crate::Sim, EntityId)> {
    use crate::orders::RobotKind;
    let dir = std::env::var_os("XENOBOTS_GAME_DIR")?;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, crate::Random::seeded(7), tables);
    let w = &sim.world;
    let scorpion = *w
        .units(Side::Alien)
        .iter()
        .find(|&&id| w.get(id).unwrap().kind == ObjectKind::Scorpion)
        .unwrap();
    let others: Vec<EntityId> = w
        .iter()
        .filter(|&(id, e)| id != scorpion && RobotKind::of(e.kind).is_some())
        .map(|(id, _)| id)
        .collect();
    for id in others {
        sim.world.remove(id);
    }
    Some((sim, scorpion))
}

/// The first relay laid after `stock`'s, ticking up to `ticks`.
pub(crate) fn first_laid(sim: &mut crate::Sim, ticks: usize) -> EntityId {
    let stock: Vec<EntityId> = sim.world.iter().map(|(id, _)| id).collect();
    for _ in 0..ticks {
        _ = sim.tick(&[]);
        if let Some((id, _)) = sim
            .world
            .iter()
            .find(|(id, e)| e.kind == ObjectKind::Relay && !stock.contains(id))
        {
            return id;
        }
    }
    panic!("no relay laid in {ticks} ticks");
}

/// A relay a stock scorpion lays stays folded while the scorpion stands on it, then
/// rises a stage every 16 ticks once it moves on (`docs/sim.md`, "The relay's tick").
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_relay_a_stock_scorpion_lays_rises_once_it_moves_on() {
    let Some((mut sim, scorpion)) = stock_scorpion_alone() else {
        return;
    };
    let id = first_laid(&mut sim, 70 * 60 * 20);
    let heights = sim.tables().unwrap().relays.heights;
    let reach = sim.tables().unwrap().relays.fold_reach;
    let near = |sim: &crate::Sim| {
        let d = approx_distance(
            sim.world.get(id).unwrap().position,
            sim.world.get(scorpion).unwrap().position,
        );
        (d.0 >> 8) < i32::from(reach)
    };
    let (mut was_near, mut up_at) = (true, None);
    let mut stages = Vec::new();
    for t in 0..70 * 60 {
        _ = sim.tick(&[]);
        let e = sim.world.get(id).unwrap();
        let State::Relay(r) = e.state else {
            panic!("{:?}", e.state)
        };
        if was_near {
            assert_eq!((r.state, r.stage), (FOLDED, 0), "tick {t}");
        }
        was_near &= near(&sim);
        if stages.last() != Some(&r.stage) {
            stages.push(r.stage);
        }
        if r.state == UP && r.flags & flag::IN_FIELD != 0 {
            assert_eq!((r.stage, e.model_height, r.frames[1]), (3, heights[3], 0));
            up_at = Some(t);
            break;
        }
    }
    assert!(!was_near, "the scorpion never moved on");
    assert_eq!(stages, [0, 1, 2, 3]);
    assert!(up_at.is_some_and(|t| t < 70 * 60), "{up_at:?}");
}
