use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{at, base, robot};
use crate::world::{Damage, Entity, Fixed, Position};
use xeno_formats::usg::{ObjectKind, flag};

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
            flags: flag::TARGETABLE,
            status: 0,
            state: State::Relay(state),
        },
    );
    world.insert(SCORPION, robot(ObjectKind::Scorpion, Side::Alien, 128, 128));
    world
}

/// Object `slot`'s +0x16 bits a relay's tick moves.
fn bits(world: &World, slot: u16) -> u8 {
    world.get(EntityId(slot)).unwrap().flags & FLAGS
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
        seen.push((r.state, r.stage, r.frames[1], height, bits(&world, RELAY.0)));
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
    assert_eq!(bits(&world, RELAY.0), 0);
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
            (r.state, r.stage, r.power, r.clock) = (UP, 3, 4, 0x0f);
        }
        let (r, _) = run(&mut world, &tables);
        assert_eq!(
            (bits(&world, RELAY.0), r.frames[5]),
            (FLAGS, colour),
            "{side:?}"
        );
    }
}

/// An up relay at `(x, y)` whose links run on its next tick.
fn up(x: i16, y: i16) -> Entity {
    Entity {
        kind: ObjectKind::Relay,
        side: Side::Human,
        position: at(x, y),
        z: Fixed(0),
        model_height: 0,
        heading: 0,
        radius: 0x500,
        damage: Damage::Taken(0),
        flags: flag::TARGETABLE | flag::IN_FIELD,
        status: 0,
        state: State::Relay(Relay {
            state: UP,
            stage: 3,
            clock: 0xff,
            ..Relay::default()
        }),
    }
}

/// A base with its net's power slider (+0x39) at `power`.
fn powered_base(side: Side, x: i16, y: i16, power: u8) -> Entity {
    let mut b = base(side, x, y, 0);
    if let State::Base { depot, .. } = &mut b.state {
        depot.power[1] = power;
    }
    b
}

/// The relays' net: two bases, a line of relays from the human one, one past the
/// links' reach, and one by the alien base alone.
fn net() -> World {
    let mut world = World::default();
    world.insert(EntityId(0), powered_base(Side::Human, 100, 100, 0x80));
    world.insert(EntityId(1), powered_base(Side::Alien, 202, 200, 0x40));
    world.bases = [Some(EntityId(0)), Some(EntityId(1))];
    // A: 2 units +x of the human base; B 2 more; C 8 past B; D by the alien base.
    for (slot, x, y) in [(2, 102, 100), (3, 104, 100), (4, 112, 100), (5, 200, 200)] {
        world.insert(EntityId(slot), up(x, y));
    }
    world
}

fn relay_at(world: &World, slot: u16) -> (Relay, Side) {
    let e = world.get(EntityId(slot)).unwrap();
    match e.state {
        State::Relay(r) => (r, e.side),
        other => panic!("{other:?}"),
    }
}

fn tick_all(world: &mut World, tables: &Tables) {
    for slot in 2..6 {
        tick(world, EntityId(slot), tables);
    }
}

#[test]
fn power_spreads_from_a_base_a_hop_a_link_run_and_the_side_follows_it() {
    let tables = synthetic_tables();
    let mut world = net();
    tick_all(&mut world, &tables);
    // A is fed by the base (+0x39 / 4, less one) on its -x quarter (3) and opens the
    // rest; B, later in slot order, is fed by A's open +x link the same run.
    let (a, side) = relay_at(&world, 2);
    assert_eq!(
        (a.power, a.power_side, a.links, side),
        (0x1f, 1, [2, 2, 2, 1], Side::Human)
    );
    assert_eq!(bits(&world, 2), FLAGS);
    let (b, _) = relay_at(&world, 3);
    assert_eq!((b.power, b.power_side, b.links), (0x1e, 1, [2, 2, 2, 1]));
    // C has none within reach: unpowered, no side, no links.
    let (c, _) = relay_at(&world, 4);
    assert_eq!(
        (c.power, c.power_side, c.links, bits(&world, 4)),
        (0, 0, [0; 4], flag::IN_FIELD)
    );
    // D takes the alien base's side, but the power of its own (human) side's base, and
    // turns alien, coloured so.
    let (d, side) = relay_at(&world, 5);
    assert_eq!(
        (d.power, d.power_side, side, d.frames[5]),
        (0x1f, 2, Side::Alien, 2)
    );
    // `1000:ce48`: its side bit follows.
    let alien = world.get(EntityId(5)).unwrap().flags & flag::ALIEN;
    assert_eq!(alien, flag::ALIEN);
    // The links run once every 256 ticks: the next 255 change no power.
    world.get_mut(EntityId(0)).unwrap().position = at(0, 0);
    for _ in 0..255 {
        tick_all(&mut world, &tables);
    }
    assert_eq!(relay_at(&world, 2).0.power, 0x1f);
    // With the base gone from reach, A loses its power at the next run, and its side.
    tick_all(&mut world, &tables);
    let (a, _) = relay_at(&world, 2);
    assert_eq!(
        (a.power, a.power_side, a.links, bits(&world, 2)),
        (0, 0, [0; 4], flag::IN_FIELD)
    );
}

#[test]
fn a_relay_takes_no_power_from_the_other_side_s_net() {
    let tables = synthetic_tables();
    let mut world = net();
    // A already alien-powered: the human base's feed is refused.
    if let Some(State::Relay(r)) = world.get_mut(EntityId(2)).map(|e| &mut e.state) {
        r.power_side = 2;
    }
    tick(&mut world, EntityId(2), &tables);
    let (a, _) = relay_at(&world, 2);
    assert_eq!((a.power, a.power_side, a.links), (0, 0, [0; 4]));
}

#[test]
fn a_robot_s_signal_falls_with_the_last_linked_relay_s_distance() {
    let tables = synthetic_tables();
    let mut world = World::default();
    let linked = |x: i16, y: i16| {
        let mut r = up(x, y);
        r.flags |= FLAGS;
        r
    };
    world.insert(
        EntityId(1),
        robot(ObjectKind::Humanoid, Side::Human, 100, 100),
    );
    let signal = |world: &World| self::signal(world, EntityId(1), &tables);
    assert_eq!(signal(&world), 0);
    // 0x580 and then 0x5a0 away (8.8): the last in slot order counts, not the nearest.
    let off = |d: i32| Position {
        x: Fixed((100 << 16) + d * 256),
        y: Fixed(100 << 16),
    };
    let mut near = linked(0, 0);
    near.position = off(0x580);
    world.insert(EntityId(2), near.clone());
    assert_eq!(signal(&world), 0xe0);
    near.position = off(0x5a0);
    world.insert(EntityId(3), near.clone());
    assert_eq!(signal(&world), 0x60);
    // Within the full reach of any, full.
    near.position = off(0x578);
    world.insert(EntityId(4), near.clone());
    assert_eq!(signal(&world), 0xff);
    // Only its side's, linked relays count.
    world.get_mut(EntityId(1)).unwrap().side = Side::Alien;
    assert_eq!(signal(&world), 0);
    world.get_mut(EntityId(1)).unwrap().side = Side::Human;
    world.insert(EntityId(4), up(0, 0));
    world.get_mut(EntityId(4)).unwrap().position = off(0);
    assert_eq!(signal(&world), 0x60);
}

#[test]
fn the_signal_step_runs_the_battery_and_the_drive() {
    let tables = synthetic_tables();
    let mut world = World::default();
    world.insert(
        EntityId(1),
        robot(ObjectKind::Humanoid, Side::Human, 100, 100),
    );
    let mut linked = up(101, 100);
    linked.flags |= FLAGS;
    // The humanoid's signal entry is 3: its tick counter 6.
    let set = |world: &mut World, energy: u8| {
        if let Some(State::Robot(m)) = world.get_mut(EntityId(1)).map(|e| &mut e.state) {
            (m.tick, m.guns.energy) = (6, energy);
        }
    };
    let read = |world: &World| {
        let e = world.get(EntityId(1)).unwrap();
        match (e.state, e.damage) {
            (State::Robot(m), Damage::Robot(c)) => {
                (m.signal, m.guns.energy, m.drive, c.equipment & ON_BATTERY)
            }
            _ => unreachable!(),
        }
    };
    // Off the net: on the battery, the drive its energy masked by power 100 (0x66).
    world.ticks = 0x10;
    set(&mut world, 0x3c);
    signal_step(&mut world, EntityId(1), &tables);
    assert_eq!(read(&world), (0, 0x3c, 0x3c & 0x66, ON_BATTERY));
    // On it: off the battery, a charge, the signal's drive.
    world.insert(EntityId(2), linked);
    set(&mut world, 0x3c);
    signal_step(&mut world, EntityId(1), &tables);
    assert_eq!(read(&world), (0xff, 0x3d, 0x66, 0));
    // A signal at the battery's level (0x40, 0x5a8 away) leaves it on the battery.
    world.get_mut(EntityId(2)).unwrap().position.x = Fixed((100 << 16) + 0x5a8 * 256);
    set(&mut world, 0x3c);
    signal_step(&mut world, EntityId(1), &tables);
    assert_eq!(read(&world), (0x40, 0x3c, 0x3c & 0x66, ON_BATTERY));
    // Another entry does nothing; a pass with no bit of the recharge's mask charges one
    // more, to 0xff at most.
    set(&mut world, 0xfe);
    world.ticks = 0x200;
    signal_step(&mut world, EntityId(1), &tables);
    assert_eq!(read(&world).1, 0xff);
    if let Some(State::Robot(m)) = world.get_mut(EntityId(1)).map(|e| &mut e.state) {
        (m.tick, m.guns.energy) = (8, 0x10);
    }
    signal_step(&mut world, EntityId(1), &tables);
    assert_eq!(read(&world).1, 0x10);
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
    let s = Relay::of(&r);
    assert_eq!(
        (s.frames, s.clock, s.state, s.stage, s.power, s.power_side),
        (r.frames, 0x42, UP, 2, 5, 2)
    );
    assert_eq!(s.links, [1, 2, 0, 2]);
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
        if r.state == UP && e.flags & flag::IN_FIELD != 0 {
            assert_eq!((r.stage, e.model_height, r.frames[1]), (3, heights[3], 0));
            up_at = Some(t);
            break;
        }
    }
    assert!(!was_near, "the scorpion never moved on");
    assert_eq!(stages, [0, 1, 2, 3]);
    assert!(up_at.is_some_and(|t| t < 70 * 60), "{up_at:?}");
    // Within 256 ticks it links to its neighbours, takes their side and power, and
    // powers a robot near it that no other relay reaches.
    for _ in 0..256 {
        _ = sim.tick(&[]);
    }
    let e = sim.world.get(id).unwrap();
    let State::Relay(r) = e.state else {
        panic!("{:?}", e.state)
    };
    assert!(r.power > 0 && r.links.contains(&1), "{r:?}");
    assert_eq!(e.flags & FLAGS, FLAGS);
    assert_eq!(e.side, sim.world.get(scorpion).unwrap().side);
    let mut world = sim.world.clone();
    let tables = sim.tables().unwrap();
    let full = Fixed(i32::from(tables.relays.full) << 8);
    world.get_mut(scorpion).unwrap().position = Position {
        x: Fixed(e.position.x.0 + full.0),
        y: e.position.y,
    };
    assert_eq!(signal(&world, scorpion, tables), 0xff);
    world.remove(id);
    assert!(signal(&world, scorpion, tables) < 0x40);
}

/// Every stock scenario's relay nets are as the links leave them: a run of the links
/// (256 ticks, the robots taken out) keeps every relay's side and links, and its power
/// but for one offset a scenario (`ntrain`'s and `strain`'s relays hold 68 more than
/// the links give, up to 99, more than any base gives, +0x39 / 4 being at most 63:
/// saved by another build or set by hand).
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_stock_relay_nets_are_steady() {
    use crate::orders::RobotKind;
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let mut relays = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("usg"))
        {
            continue;
        }
        let scenario = xeno_formats::usg::parse(&std::fs::read(&path).unwrap()).unwrap();
        let mut sim = crate::Sim::load(&scenario, crate::Random::seeded(1), tables.clone());
        let robots: Vec<EntityId> = (sim.world.iter())
            .filter(|(_, e)| RobotKind::of(e.kind).is_some())
            .map(|(id, _)| id)
            .collect();
        robots.into_iter().for_each(|id| _ = sim.world.remove(id));
        let net = |sim: &crate::Sim| -> Vec<_> {
            (sim.world.iter())
                .filter_map(|(id, e)| match e.state {
                    State::Relay(r) => Some((id, e.side, r.power, r.power_side, r.links)),
                    _ => None,
                })
                .collect()
        };
        let before = net(&sim);
        relays += before.len();
        for _ in 0..256 {
            _ = sim.tick(&[]);
        }
        let after = net(&sim);
        let offset = before.first().map_or(0, |b| b.2 - after[0].2);
        let shifted: Vec<_> = (after.iter())
            .map(|&(id, side, p, ps, l)| (id, side, p + offset, ps, l))
            .collect();
        assert_eq!(shifted, before, "{path:?}");
        let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
        let saved_high = ["ntrain.usg", "strain.usg"].contains(&name.as_str());
        assert_eq!(offset != 0, saved_high, "{path:?}");
    }
    assert!(relays > 100, "{relays}");
}

/// `ctrain`'s scorpion dumps its cargo with D: a relay every `wait` runs of its tick
/// entry (160 ticks in the stock build), each where it stands; D again stops it.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_stock_scorpion_dumps_its_cargo() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("ctrain.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, crate::Random::seeded(7), tables);
    let w = &sim.world;
    let scorpion = *w
        .units(Side::Human)
        .iter()
        .find(|&&id| w.get(id).unwrap().kind == ObjectKind::Scorpion)
        .unwrap();
    let relays = |sim: &crate::Sim| {
        let n = sim
            .world
            .iter()
            .filter(|(_, e)| e.kind == ObjectKind::Relay);
        n.count()
    };
    let (carried, laid) = (crate::net::relays(&sim.world, scorpion), relays(&sim));
    assert!(carried >= 2, "{carried}");
    sim.pilot(scorpion, crate::Pilot::Dump);
    let wait = 16 * usize::from(sim.tables().unwrap().net.dump.wait);
    for _ in 0..2 * wait {
        _ = sim.tick(&[]);
    }
    assert_eq!(relays(&sim) - laid, 2);
    assert_eq!(crate::net::relays(&sim.world, scorpion), carried - 2);
    sim.pilot(scorpion, crate::Pilot::Dump);
    for _ in 0..2 * wait {
        _ = sim.tick(&[]);
    }
    assert_eq!(relays(&sim) - laid, 2);
}
