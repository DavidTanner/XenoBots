use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{base, robot};
use xeno_formats::usg::{ObjectKind, Side};

const ME: EntityId = EntityId(4);
const ENEMY: EntityId = EntityId(6);

/// A human scorpion (slot 4) with `left` of each count (+0xe2 to +0xe5), and the human
/// base in slot 0.
fn scorpion(left: u8) -> World {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Human, 0, 0, 0));
    world.bases[0] = Some(EntityId(0));
    world.insert(ME, robot(ObjectKind::Scorpion, Side::Human, 10, 10));
    crate::motion::movement(&mut world, ME).unwrap().guns.drops = [left; 4];
    world
}

fn queue(world: &mut World) -> Vec<u16> {
    let m = crate::motion::movement(world, ME).unwrap();
    m.panel.queue.iter().copied().filter(|&t| t != 0).collect()
}

fn dropped(world: &World) -> Vec<(EntityId, Entity)> {
    world
        .iter()
        .filter(|(_, e)| matches!(e.state, State::Drop(_)))
        .map(|(id, e)| (id, e.clone()))
        .collect()
}

#[test]
fn with_none_left_a_key_only_says_so() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let d = tables.weapons.drops;
    for key in 0..4 {
        let mut world = scorpion(0);
        drop(&mut world, ME, key, &tables, &mut random);
        assert!(dropped(&world).is_empty());
        assert_eq!(queue(&mut world), [d.none_left[usize::from(key)]]);
    }
}

#[test]
fn each_key_drops_its_kind_near_the_robot_and_takes_one() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let d = tables.weapons.drops;
    let mut world = scorpion(2);
    crate::motion::movement(&mut world, ME).unwrap().guns.drops = [2, 3, 4, 5];
    if let Some(Entity {
        damage: Damage::Robot(c),
        ..
    }) = world.get_mut(ME)
    {
        c.equipment = usg::Robot::ON_BATTERY;
    }
    for key in 0..4 {
        drop(&mut world, ME, key, &tables, &mut random);
    }
    // Counts by offset: keys 1 to 4 take +0xe2, +0xe4, +0xe3, +0xe5.
    let m = crate::motion::movement(&mut world, ME).unwrap();
    assert_eq!(m.guns.drops, [1, 2, 3, 4]);
    assert_eq!(queue(&mut world), d.dropped);
    let me = world.get(ME).unwrap().clone();
    let made = dropped(&world);
    let kinds: Vec<_> = made.iter().map(|(_, e)| e.kind).collect();
    assert_eq!(kinds, d.kinds.map(ObjectKind::from));
    for (_, e) in &made {
        assert_eq!((e.side, e.z, e.heading, e.radius), (me.side, me.z, 0, 0));
        // Within a quarter of a word (1/8 unit) of the robot.
        assert!((e.position.x.0 - me.position.x.0).abs() <= 0x2000);
        assert!((e.position.y.0 - me.position.y.0).abs() <= 0x2000);
        let State::Drop(s) = e.state else {
            unreachable!()
        };
        assert_eq!(s.battery, 1);
    }
    let state = |n: usize| match made[n].1.state {
        State::Drop(s) => s,
        _ => unreachable!(),
    };
    assert_eq!(state(1).charges, d.ecm_charges);
    // The decoy keeps the scorpion's model.
    assert_eq!((state(3).ticks, state(3).level), (d.models[1], 0));
}

#[test]
fn with_no_slot_free_the_mine_key_says_nothing() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let d = tables.weapons.drops;
    let mut world = scorpion(1);
    for slot in 7..240 {
        world.insert(
            EntityId(slot),
            robot(ObjectKind::Obstacle, Side::Human, 0, 0),
        );
    }
    drop(&mut world, ME, 0, &tables, &mut random);
    drop(&mut world, ME, 1, &tables, &mut random);
    assert!(dropped(&world).is_empty());
    assert_eq!(queue(&mut world), [d.none_left[1]]);
    assert_eq!(
        crate::motion::movement(&mut world, ME).unwrap().guns.drops,
        [1; 4]
    );
}

/// Drop `key`'s object at the scorpion and an alien scout `away` (8.8) east of it;
/// returns the object.
fn armed(key: u8, away: i32, tables: &Tables) -> (World, EntityId) {
    let mut world = scorpion(1);
    drop(&mut world, ME, key, tables, &mut Random::seeded(1));
    let (id, e) = dropped(&world).remove(0);
    let mut enemy = robot(ObjectKind::Scout, Side::Alien, 0, 0);
    enemy.position = Position {
        x: Fixed(e.position.x.0 + (away << 8)),
        y: e.position.y,
    };
    world.insert(ENEMY, enemy);
    (world, id)
}

/// Runs object `id`'s tick `n` times; the events, and whether it went.
fn run(world: &mut World, id: EntityId, n: u32, tables: &Tables) -> (Vec<Event>, bool) {
    let mut events = Vec::new();
    for _ in 0..n {
        if tick(world, id, tables, &mut Random::seeded(1), &mut events) {
            world.remove(id);
            return (events, true);
        }
    }
    (events, false)
}

fn state(world: &World, id: EntityId) -> Drop {
    match world.get(id).unwrap().state {
        State::Drop(s) => s,
        _ => unreachable!(),
    }
}

fn log(world: &World) -> Vec<([u8; 6], u16)> {
    match world.get(EntityId(0)).unwrap().state {
        State::Base { depot, .. } => depot
            .log
            .iter()
            .filter(|l| l.text != 0)
            .map(|l| (l.name[..6].try_into().unwrap(), l.text))
            .collect(),
        _ => unreachable!(),
    }
}

#[test]
fn a_mine_looks_every_256_ticks_and_goes_off_on_an_enemy_in_range() {
    let tables = synthetic_tables();
    let d = tables.weapons.drops;
    // Out of range: it stays.
    let (mut world, mine) = armed(0, i32::from(d.mine_range) + 1, &tables);
    assert!(!run(&mut world, mine, 512, &tables).1);
    // In range: only on the 256th tick.
    let (mut world, mine) = armed(0, i32::from(d.mine_range), &tables);
    assert!(!run(&mut world, mine, 255, &tables).1);
    let (events, gone) = run(&mut world, mine, 1, &tables);
    assert!(gone);
    assert!(events.contains(&Event::Sound {
        event: d.mine_sound,
        only_for: None
    }));
    assert_eq!(log(&world), [(tables.messages.alert, d.mine_alert)]);
    let explosion = world
        .iter()
        .find(|(_, e)| matches!(e.state, State::Explosion { .. }));
    assert!(explosion.is_some());
}

#[test]
fn a_robot_of_its_own_side_sets_nothing_off() {
    let tables = synthetic_tables();
    let (mut world, mine) = armed(0, 0, &tables);
    world.get_mut(ENEMY).unwrap().side = Side::Human;
    assert!(!run(&mut world, mine, 512, &tables).1);
    assert!(log(&world).is_empty());
}

#[test]
fn an_alarm_buoy_alerts_then_waits_for_its_level_to_halve_to_nothing() {
    let tables = synthetic_tables();
    let d = tables.weapons.drops;
    let (mut world, buoy) = armed(2, i32::from(d.alarm_range), &tables);
    run(&mut world, buoy, 256, &tables);
    let s = state(&world, buoy);
    assert_eq!((s.level, s.alarm), (d.alarm_level, false));
    assert_eq!(log(&world).len(), 1);
    // Each look halves it, showing the alarm while it is not 0; the next alert comes
    // on the look that finds it at 0.
    let mut looks = 0;
    while log(&world).len() == 1 {
        run(&mut world, buoy, 256, &tables);
        looks += 1;
        let s = state(&world, buoy);
        assert_eq!(s.alarm, s.level != 0 && s.level != d.alarm_level);
    }
    assert_eq!(looks, d.alarm_level.ilog2() + 1);
    assert_eq!(log(&world)[1], (tables.messages.alert, d.alarm_alert));
}

#[test]
fn an_ecm_buoy_spends_a_charge_an_alert_and_goes_with_the_last() {
    let tables = synthetic_tables();
    let d = tables.weapons.drops;
    let (mut world, buoy) = armed(1, i32::from(d.ecm_range), &tables);
    let per_alert = 256 * (d.ecm_level.ilog2() + 1);
    run(&mut world, buoy, 256, &tables);
    assert_eq!(state(&world, buoy).level, d.ecm_level);
    assert_eq!(state(&world, buoy).charges, d.ecm_charges - 1);
    for n in 2..d.ecm_charges {
        run(&mut world, buoy, per_alert, &tables);
        assert_eq!(state(&world, buoy).charges, d.ecm_charges - n);
    }
    assert!(run(&mut world, buoy, per_alert, &tables).1);
    assert!(world.get(buoy).is_none());
    assert!(
        log(&world)
            .iter()
            .all(|&l| l == (tables.messages.alert, d.ecm_alert))
    );
}

#[test]
fn a_decoy_never_ticks() {
    let tables = synthetic_tables();
    let (mut world, decoy) = armed(3, 0, &tables);
    let before = state(&world, decoy);
    assert!(!run(&mut world, decoy, 1024, &tables).1);
    assert_eq!(state(&world, decoy), before);
    assert!(log(&world).is_empty());
}

/// Scen00a's first human scorpion drops one of each: the counts and messages are
/// stock, and a mine dropped on an alien robot goes off within 256 ticks.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_stock_scorpion_drops_mines_and_buoys() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let d = tables.weapons.drops;
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, Random::seeded(5), tables.clone());
    let world = &sim.world;
    let find = |side: Side, kind: Option<ObjectKind>| {
        *world
            .units(side)
            .iter()
            .find(|&&id| kind.is_none_or(|k| world.get(id).unwrap().kind == k))
            .unwrap()
    };
    let id = find(Side::Human, Some(ObjectKind::Scorpion));
    let enemy = find(Side::Alien, None);
    let left = |sim: &crate::Sim| match sim.world.get(id).unwrap().state {
        State::Robot(m) => m.guns.drops,
        _ => unreachable!(),
    };
    let before = left(&sim);
    assert!(before.iter().all(|&n| n > 0), "{before:?}");
    for key in 0..4 {
        sim.pilot(id, crate::Pilot::Drop(key));
    }
    assert_eq!(left(&sim), before.map(|n| n - 1));
    assert_eq!(dropped(&sim.world).len(), 4);
    for text in d.dropped.iter().chain(&d.none_left) {
        assert_ne!(tables.strings.byte(*text), 0, "{text:#x}");
    }
    // Move the alien robot onto the mine: it goes off at its next look.
    let (mine, at) = dropped(&sim.world)
        .into_iter()
        .find(|(_, e)| e.kind == ObjectKind::from(d.kinds[0]))
        .map(|(id, e)| (id, e.position))
        .unwrap();
    sim.world.get_mut(enemy).unwrap().position = at;
    let mut gone = None;
    for t in 1..=256u32 {
        sim.tick(&[]);
        if sim
            .world
            .get(mine)
            .is_none_or(|e| e.kind != ObjectKind::from(d.kinds[0]))
        {
            gone = Some(t);
            break;
        }
    }
    assert!(gone.is_some());
}
