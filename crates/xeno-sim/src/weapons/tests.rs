use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::robot;
use crate::world::{Fixed, Guns, Position};
use xeno_formats::usg::Side;

/// A human `kind` (slot 4) facing an alien scorpion 1 unit ahead (slot 6), locked on it,
/// with the target in its arc.
fn armed(kind: ObjectKind) -> World {
    let mut world = World::default();
    world.insert(EntityId(4), robot(kind, Side::Human, 10, 10));
    world.insert(
        EntityId(6),
        robot(ObjectKind::Scorpion, Side::Alien, 10, 11),
    );
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    m.target = Some(EntityId(6));
    c.targeting = IN_ARC;
    world
}

fn parts(world: &mut World) -> (Movement, Components) {
    let (m, c) = robot_parts(world, EntityId(4)).unwrap();
    (*m, *c)
}

/// The weapon step at tick-table entry `entry` (+0x1a after its step).
fn step_at(world: &mut World, entry: u8, random: &mut Random) -> Vec<Event> {
    robot_parts(world, EntityId(4)).unwrap().0.tick = entry << 1;
    let mut events = Vec::new();
    step(world, EntityId(4), &synthetic_tables(), random, &mut events);
    events
}

fn shots(world: &World) -> usize {
    world
        .iter()
        .filter(|(_, e)| matches!(e.state, State::Shot(_)))
        .count()
}

#[test]
fn a_humanoid_raises_its_arm_then_fires_when_cooled_and_in_the_arc() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = armed(ObjectKind::Humanoid);
    trigger(&mut world, EntityId(4), &tables);
    assert_eq!(parts(&mut world).1.weapons, REQUEST);
    // Not the weapon step's entry: nothing.
    step_at(&mut world, 2, &mut random);
    assert_eq!(parts(&mut world).0.guns.raised, [0, 0]);
    step_at(&mut world, 1, &mut random);
    let (m, c) = parts(&mut world);
    assert_eq!((m.guns.raised, c.weapons), ([1, 0], 0));
    // The arm lifts a frame every 4 passes to 5, then makes the cannon ready.
    let mut ticks = 0;
    while parts(&mut world).1.weapons & READY == 0 {
        let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
        arms(&synthetic_tables().walk, ticks, m, c);
        ticks += 1;
        assert!(ticks < 100);
    }
    assert_eq!((ticks, parts(&mut world).0.guns.arms[0]), (21, 5));
    // Cooling down: it waits, ready.
    robot_parts(&mut world, EntityId(4))
        .unwrap()
        .0
        .guns
        .cooldown[0] = 2;
    step_at(&mut world, 9, &mut random);
    assert_eq!(
        (shots(&world), parts(&mut world).0.guns.cooldown[0]),
        (0, 1)
    );
    let events = step_at(&mut world, 1, &mut random);
    assert_eq!(shots(&world), 1);
    assert_eq!(
        events,
        [Event::Sound {
            event: 1,
            only_for: Some(EntityId(4))
        }]
    );
    let (m, c) = parts(&mut world);
    assert_eq!((m.guns.cooldown[0], c.weapons & READY), (12, 0));
    // With the weapon fired the arm lowers to 0 and rests.
    for t in 0..40 {
        let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
        arms(&synthetic_tables().walk, t, m, c);
    }
    assert_eq!(parts(&mut world).0.guns.raised, [0, 0]);
}

#[test]
fn a_damaged_weapon_raises_no_arm_and_a_destroyed_one_shows_its_broken_arm() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = armed(ObjectKind::Humanoid);
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.weapon, c.weapons, c.systems) = (1, 0x02, 0x80);
    trigger(&mut world, EntityId(4), &tables);
    step_at(&mut world, 1, &mut random);
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    assert_eq!(m.guns.raised, [0, 0]);
    arms(&tables.walk, 1, m, c);
    assert_eq!(m.guns.arms, [0, BROKEN]);
}

#[test]
fn on_the_battery_firing_takes_energy_and_short_it_tells_once() {
    let mut random = Random::seeded(1);
    let mut world = armed(ObjectKind::Humanoid);
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.guns.energy, c.equipment, c.weapons) = (15, ON_BATTERY, READY);
    step_at(&mut world, 1, &mut random);
    assert_eq!((shots(&world), parts(&mut world).0.guns.energy), (1, 5));
    for _ in 0..2 {
        let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
        (m.guns.cooldown, c.weapons) = ([0, 0], READY);
        step_at(&mut world, 1, &mut random);
    }
    let (m, c) = parts(&mut world);
    assert_eq!(
        (shots(&world), m.guns.energy, c.equipment & TOLD),
        (1, 0, TOLD)
    );
    let no_power = synthetic_tables().weapons.messages.no_power;
    assert_eq!(m.panel.queue.iter().filter(|&&q| q == no_power).count(), 1);
}

#[test]
fn a_scorpion_s_burst_fires_its_barrels_by_turns_and_its_rounds_run_out() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = armed(ObjectKind::Scorpion);
    robot_parts(&mut world, EntityId(4)).unwrap().0.guns.rounds = 2;
    trigger(&mut world, EntityId(4), &tables);
    let mut sounds = 0;
    for n in 0..6 {
        let events = step_at(&mut world, if n % 2 == 0 { 1 } else { 12 }, &mut random);
        sounds += events.len();
    }
    // Five requests, a cool-down of 2 steps: shells at the 1st and 3rd; the 5th has no
    // round left. Its shells are heard by everyone.
    let (m, c) = parts(&mut world);
    assert_eq!(
        (shots(&world), sounds, m.guns.rounds, m.guns.fired),
        (2, 2, 0, 3)
    );
    assert_eq!((m.guns.burst, c.weapons & REQUEST), (0, 0));
    // No rounds left, and (made with none) no missiles; then a missile reloading.
    assert_eq!(state_bits(&world, EntityId(4), 0), 0x40);
    robot_parts(&mut world, EntityId(4)).unwrap().0.guns = Guns {
        missiles: 1,
        cooldown: [0, 3],
        ..Guns::default()
    };
    assert_eq!(state_bits(&world, EntityId(4), 1), 0x02);
}

#[test]
fn a_scorpion_launches_one_tail_missile_and_reloads() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = armed(ObjectKind::Scorpion);
    let (m, _) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.weapon, m.guns.missiles) = (1, 2);
    trigger(&mut world, EntityId(4), &tables);
    step_at(&mut world, 1, &mut random);
    let (m, c) = parts(&mut world);
    assert_eq!(
        (m.guns.missiles, m.guns.cooldown[1], m.guns.missile),
        (1, 400, Some(EntityId(0)))
    );
    assert_eq!(c.equipment & TOLD, TOLD);
    // Again at once: reloading, told only if the launch's message bit is cleared.
    robot_parts(&mut world, EntityId(4)).unwrap().1.equipment = 0;
    trigger(&mut world, EntityId(4), &tables);
    step_at(&mut world, 1, &mut random);
    let reloading = tables.weapons.messages.reloading;
    assert!(parts(&mut world).0.panel.queue.contains(&reloading));
    // The reload counts down at its own entry; a gone missile is let go.
    step_at(&mut world, 9, &mut random);
    assert_eq!(parts(&mut world).0.guns.cooldown[1], 399);
    world.remove(EntityId(0));
    step_at(&mut world, 12, &mut random);
    assert_eq!(parts(&mut world).0.guns.missile, None);
}

#[test]
fn a_scout_s_cannon_tells_when_out_of_shells() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = armed(ObjectKind::Scout);
    let (m, _) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.weapon, m.guns.rounds) = (1, 1);
    for _ in 0..2 {
        trigger(&mut world, EntityId(4), &tables);
        robot_parts(&mut world, EntityId(4))
            .unwrap()
            .0
            .guns
            .cooldown = [0, 0];
        step_at(&mut world, 11, &mut random);
    }
    let (m, _) = parts(&mut world);
    assert_eq!((shots(&world), m.guns.rounds), (1, 0));
    assert!(m.panel.queue.contains(&tables.weapons.messages.no_shells));
    assert_eq!(state_bits(&world, EntityId(4), 1), 0x40);
}

/// scen00a's first human humanoid and first alien robot, the others taken out, 7 units
/// apart halfway between the bases (as `sense`'s closing test): they lock, close and
/// fire, and shots fly, hit and wear each other down.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_two_stock_robots_fight() {
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
    let mut sim = crate::Sim::load(&scenario, Random::seeded(7), tables);
    let world = &sim.world;
    let human = *world
        .units(Side::Human)
        .iter()
        .find(|&&id| world.get(id).unwrap().kind == ObjectKind::Humanoid)
        .unwrap();
    let alien = world.units(Side::Alien)[0];
    let base = |side| world.get(world.base(side).unwrap()).unwrap().position;
    let (h, a) = (base(Side::Human), base(Side::Alien));
    let mid = Position {
        x: Fixed(h.x.0 / 2 + a.x.0 / 2),
        y: Fixed(h.y.0 / 2 + a.y.0 / 2),
    };
    let others: Vec<EntityId> = world
        .iter()
        .filter(|&(id, e)| id != human && id != alien && RobotKind::of(e.kind).is_some())
        .map(|(id, _)| id)
        .collect();
    for id in others {
        sim.world.remove(id);
    }
    sim.world.get_mut(human).unwrap().position = mid;
    sim.world.get_mut(alien).unwrap().position = Position {
        x: Fixed(mid.x.0 + 7 * 0x10000),
        y: mid.y,
    };
    let (mut shots, mut hits, mut destroyed) = (0, [0; 2], Vec::new());
    let mut flying = std::collections::BTreeSet::new();
    for t in 0..70 * 60 * 5 {
        for e in sim.tick(&[]) {
            match e {
                Event::Sound {
                    event: 21,
                    only_for: Some(r),
                } => {
                    hits[usize::from(r == alien)] += 1;
                }
                Event::Destroyed(r) => destroyed.push((t, r)),
                _ => {}
            }
        }
        let now: std::collections::BTreeSet<EntityId> = sim
            .world
            .iter()
            .filter(|(_, e)| matches!(e.state, State::Shot(_)))
            .map(|(id, _)| id)
            .collect();
        shots += now.difference(&flying).count();
        flying = now;
        if !destroyed.is_empty() {
            break;
        }
    }
    // In five minutes both fire many shots and hit each other (the shells' blasts and
    // homing shots both wear); neither is destroyed outright, as only the damage
    // total (T-140) ends a robot worn by small hits.
    assert!(shots > 10, "{shots}");
    assert!(hits.iter().all(|&n| n > 0), "{hits:?}");
    assert!(destroyed.is_empty());
    let worn = |id| match sim.world.get(id).unwrap().damage {
        Damage::Robot(c) => c.helm < 100 || c.power < 100 || c.weapons & 7 != 0 || c.systems != 0,
        _ => false,
    };
    assert!(worn(human) && worn(alien));
}

#[test]
fn a_scorpion_selecting_an_empty_weapon_hears_it_is_empty() {
    let tables = synthetic_tables();
    let mut world = armed(ObjectKind::Scorpion);
    let (m, _) = robot_parts(&mut world, EntityId(4)).unwrap();
    // Rounds are counted by their low byte (`cmp byte`): 0x100 rounds read as none.
    m.guns.rounds = 0x100;
    (m.weapon, m.guns.missiles) = (1, 3);
    let mut events = Vec::new();
    let s = ObjectKind::Scorpion;
    crate::motion::select_weapon(&tables, s, 0, EntityId(4), m, 0, &mut events);
    crate::motion::select_weapon(&tables, s, 0, EntityId(4), m, 1, &mut events);
    let w = &tables.weapon_keys;
    assert_eq!(m.panel.queue[..2], [w.scorpion_empty[0], w.selected[1][1]]);
}
