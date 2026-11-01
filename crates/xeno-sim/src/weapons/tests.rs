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
fn a_walking_humanoid_swings_its_arms_by_its_legs_step_and_settles_them_still() {
    let walk = synthetic_tables().walk;
    let mut world = armed(ObjectKind::Humanoid);
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    // Step 6: the arms' tables `step & 3` and `(step + 1) & 3`.
    (m.speed, m.stride) = (0x80, 0x60);
    arms(&walk, 1, m, c);
    assert_eq!(m.guns.arms, [2, 3]);
    // Still: down a frame a tick, every tick.
    m.speed = 0;
    arms(&walk, 1, m, c);
    assert_eq!(m.guns.arms, [1, 2]);
    arms(&walk, 2, m, c);
    arms(&walk, 3, m, c);
    assert_eq!(m.guns.arms, [0, 0]);
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
/// fire, shots fly and hit, and the alien is worn to its limit (35 for an alien
/// humanoid) and destroyed by its damage total (T-140). With the limits out of reach
/// the same fight leaves it standing with its total past 35.
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
    let mut endless = tables.clone();
    for t in &mut endless.damage.totals {
        t.destroyed = [0xff; 2];
    }
    // The fight, `ticks` long or until a robot is destroyed: the shots made, the hits
    // on the human and the alien, what was destroyed when, and the sim.
    let fight = |tables: xeno_formats::motion::Motion, ticks: u32| {
        let mut sim = crate::Sim::load(&scenario, Random::seeded(5), tables);
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
        for t in 0..ticks {
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
        (shots, hits, destroyed, sim, alien)
    };
    // Within five minutes both are hit and the alien is destroyed by wear.
    let (shots, hits, destroyed, _, alien) = fight(tables, 70 * 60 * 5);
    assert!(shots > 4, "{shots}");
    assert!(hits.iter().all(|&n| n > 0), "{hits:?}");
    let [(t, gone)] = destroyed[..] else {
        panic!("{destroyed:?}");
    };
    assert_eq!(gone, alien);
    // The same fight to that tick with no limits: the alien stands, its total past 35.
    let (_, _, destroyed, sim, alien) = fight(endless, t + 1);
    assert!(destroyed.is_empty());
    let total = match sim.world.get(alien).unwrap().damage {
        Damage::Robot(c) => c.total,
        _ => 0,
    };
    assert!(total >= 35, "{total}");
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

fn sparkles(world: &World) -> Vec<(EntityId, Entity)> {
    world
        .iter()
        .filter(|(_, e)| matches!(e.state, State::Sparkle { .. }))
        .map(|(id, e)| (id, e.clone()))
        .collect()
}

#[test]
fn the_ecm_takes_a_charge_and_energy_and_runs_for_its_steps() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let d = tables.weapons.devices;
    let mut world = armed(ObjectKind::Scout);
    // None left: only the message.
    ecm(&mut world, EntityId(4), &tables);
    let (m, _) = parts(&mut world);
    assert_eq!((m.guns.ecm_time, m.status & status::ECM), (0, 0));
    assert!(m.panel.queue.contains(&d.no_ecm));
    // Short on the battery: the energy goes, the charge stays.
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.guns.ecm, m.guns.energy, c.equipment) = (2, d.ecm_energy - 1, ON_BATTERY);
    ecm(&mut world, EntityId(4), &tables);
    let (m, _) = parts(&mut world);
    assert_eq!((m.guns.ecm, m.guns.energy, m.guns.ecm_time), (2, 0, 0));
    assert!(m.panel.queue.contains(&d.no_power));
    // Off the battery: on.
    robot_parts(&mut world, EntityId(4)).unwrap().1.equipment = 0;
    ecm(&mut world, EntityId(4), &tables);
    let (m, _) = parts(&mut world);
    assert_eq!((m.guns.ecm, m.guns.ecm_time), (1, d.ecm_time));
    assert_ne!(m.status & status::ECM, 0);
    assert!(m.panel.queue.contains(&d.ecm_on));
    // Each weapon step counts it down; at 0 the bit goes.
    for n in (0..d.ecm_time).rev() {
        step_at(&mut world, 1, &mut random);
        assert_eq!(parts(&mut world).0.guns.ecm_time, n);
    }
    assert_eq!(parts(&mut world).0.status & status::ECM, 0);
    // A step between the weapon steps leaves it.
    ecm(&mut world, EntityId(4), &tables);
    step_at(&mut world, 2, &mut random);
    assert_eq!(parts(&mut world).0.guns.ecm_time, d.ecm_time);
}

#[test]
fn the_cloak_ignores_short_energy_makes_the_sparkle_and_runs_out() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let d = tables.weapons.devices;
    let mut world = armed(ObjectKind::Humanoid);
    cloak(&mut world, EntityId(4), &tables);
    assert!(parts(&mut world).0.panel.queue.contains(&d.no_cloak));
    assert!(sparkles(&world).is_empty());
    let (m, c) = robot_parts(&mut world, EntityId(4)).unwrap();
    (m.guns.cloaks, m.guns.energy, c.equipment) = (1, 1, ON_BATTERY);
    cloak(&mut world, EntityId(4), &tables);
    let (m, _) = parts(&mut world);
    assert_eq!((m.guns.cloaks, m.guns.energy), (0, 0));
    assert_eq!(
        (m.guns.cloak_time, m.status & status::CLOAKED),
        (d.cloak_time, status::CLOAKED)
    );
    assert!(m.panel.queue.contains(&d.cloak_on));
    let [(_, e)] = &sparkles(&world)[..] else {
        panic!("one sparkle");
    };
    let me = world.get(EntityId(4)).unwrap();
    assert_eq!(
        (e.kind, e.position, e.z),
        (ObjectKind::Other(d.sparkle), me.position, me.z)
    );
    assert_eq!((e.heading, e.radius), (d.heading, d.radius));
    for _ in 0..d.cloak_time {
        step_at(&mut world, 1, &mut random);
    }
    let (m, _) = parts(&mut world);
    assert_eq!((m.guns.cloak_time, m.status & status::CLOAKED), (0, 0));
    // A scorpion's step counts no cloak.
    let mut world = armed(ObjectKind::Scorpion);
    robot_parts(&mut world, EntityId(4))
        .unwrap()
        .0
        .guns
        .cloak_time = 5;
    step_at(&mut world, 1, &mut random);
    assert_eq!(parts(&mut world).0.guns.cloak_time, 5);
}

#[test]
fn the_sparkle_grows_holds_shrinks_and_goes_at_its_life() {
    let tables = synthetic_tables();
    let d = tables.weapons.devices;
    let mut world = armed(ObjectKind::Humanoid);
    robot_parts(&mut world, EntityId(4)).unwrap().0.guns.cloaks = 1;
    cloak(&mut world, EntityId(4), &tables);
    let (id, mut e) = sparkles(&world).remove(0);
    let per_frame = u32::from(d.frame_ticks) + 1;
    let (mut ticks, mut sizes) = (0, Vec::new());
    while !tick_sparkle(&mut e, &tables) {
        ticks += 1;
        if ticks % per_frame == 0 {
            sizes.push(e.heading);
        }
    }
    assert_eq!(ticks + 1, u32::from(d.life) * per_frame);
    let step = u16::from(d.heading_step);
    let grown = d.heading + step * u16::from(d.grown - 1);
    // Grows to frame `grown`, holds, shrinks from `shrinking` through the last.
    assert_eq!(sizes[usize::from(d.grown) - 2], grown);
    assert_eq!(sizes[usize::from(d.shrinking) - 2], grown);
    assert_eq!(
        e.heading,
        grown - step * u16::from(d.life - d.shrinking + 1)
    );
    assert!(matches!(e.state, State::Sparkle { frame, .. } if frame == d.life & d.frames));
    // In the world, its tick removes it.
    for _ in 0..u32::from(d.life) * per_frame {
        crate::motion::tick(&mut world, &tables, &mut Random::seeded(1));
    }
    assert!(world.get(id).is_none());
}

/// Scen00a's first human humanoid turns on its ECM and cloak: each takes a charge, its
/// message is a stock one, the sparkle stands where it was, and the ECM, then the cloak
/// and the sparkle, run out as the user's numbers say.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_stock_humanoid_s_ecm_and_cloak_run_out() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let d = tables.weapons.devices;
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, Random::seeded(5), tables.clone());
    let world = &sim.world;
    let id = *world
        .units(Side::Human)
        .iter()
        .find(|&&id| world.get(id).unwrap().kind == ObjectKind::Humanoid)
        .unwrap();
    let guns = |sim: &crate::Sim| match sim.world.get(id).unwrap().state {
        State::Robot(m) => (m.guns, m.status),
        _ => unreachable!(),
    };
    let before = guns(&sim).0;
    assert!(before.ecm > 0 && before.cloaks > 0, "{before:?}");
    sim.pilot(id, crate::Pilot::Ecm);
    sim.pilot(id, crate::Pilot::Cloak);
    let (g, s) = guns(&sim);
    assert_eq!((g.ecm, g.cloaks), (before.ecm - 1, before.cloaks - 1));
    assert_eq!(
        s & (status::ECM | status::CLOAKED),
        status::ECM | status::CLOAKED
    );
    for text in [d.ecm_on, d.cloak_on, d.no_ecm, d.no_cloak] {
        assert_ne!(tables.strings.byte(text), 0, "{text:#x}");
    }
    let [(sparkle, e)] = &sparkles(&sim.world)[..] else {
        panic!("one sparkle");
    };
    assert_eq!(e.position, sim.world.get(id).unwrap().position);
    let (mut ecm_off, mut cloak_off, mut gone) = (None, None, None);
    for t in 1..2000u32 {
        sim.tick(&[]);
        let (_, s) = guns(&sim);
        if s & status::ECM == 0 {
            ecm_off.get_or_insert(t);
        }
        if s & status::CLOAKED == 0 {
            cloak_off.get_or_insert(t);
        }
        if sim
            .world
            .get(*sparkle)
            .is_none_or(|e| e.kind != ObjectKind::Other(d.sparkle))
        {
            gone.get_or_insert(t);
        }
    }
    // Two weapon steps in 16 ticks; a sparkle frame every `frame_ticks + 1` ticks.
    let near = |t: Option<u32>, want: u32| t.is_some_and(|t| t.abs_diff(want) <= 8);
    assert!(near(ecm_off, 8 * u32::from(d.ecm_time)), "{ecm_off:?}");
    assert!(
        near(cloak_off, 8 * u32::from(d.cloak_time)),
        "{cloak_off:?}"
    );
    let life = u32::from(d.life) * (u32::from(d.frame_ticks) + 1);
    assert_eq!(gone, Some(life), "{gone:?}");
}
