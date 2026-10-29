use super::*;
use crate::Sim;
use crate::motion::{movement, synthetic_tables};
use crate::world::{Fixed, Position};
use crate::{Pilot, Random};
use xeno_formats::fixtures::synthetic_mission_usg;

/// Writes `sim`'s world into a save's bytes and loads them back, as the log-in does.
fn round_trip(sim: &Sim, loaded: &Scenario, table: u16) -> (Vec<u8>, Scenario, Sim) {
    let bytes = usg::write(&sim.world.save(loaded, table)).unwrap();
    let back = usg::parse(&bytes).unwrap();
    let again = Sim::load(&back, Random::seeded(1), sim.tables.clone().unwrap());
    (bytes, back, again)
}

/// `running` as the original's loader leaves it, taking what the loader resets from
/// `loaded`: the scripts restart with the program given, the targets are dropped, and
/// the counts carried from mission to mission start over.
fn as_loaded(running: &World, loaded: &World) -> World {
    let mut expected = running.clone();
    for (id, e) in loaded.iter() {
        if let (State::Robot(m), Some(State::Robot(want))) =
            (e.state, expected.get_mut(id).map(|e| &mut e.state))
        {
            (want.target, want.script) = (None, m.script);
        }
    }
    let sides = [usg::Side::Human, usg::Side::Alien];
    for (slot, r) in sides.into_iter().flat_map(|side| running.orders.side(side)) {
        let o = loaded.orders.get(slot).unwrap().orders;
        assert_eq!(
            (o.ordered, o.program_drives),
            (r.orders.ordered, r.orders.program_drives)
        );
        assert_eq!(o.program, o.ordered, "the program given runs");
    }
    expected.orders = loaded.orders.clone();
    (expected.stagger, expected.ticks, expected.call_signs) = (loaded.stagger, loaded.ticks, 0);
    (expected.lock_window, expected.point) = (loaded.lock_window, loaded.point);
    expected
}

fn shot(flight: Flight, n: u16) -> Shot {
    Shot {
        flight,
        velocity: [-3 - n as i16, 4 + n as i16, -5],
        speed: 0x300 + n,
        climb: if matches!(flight, Flight::Tail | Flight::Homing) {
            0xfe00
        } else {
            0
        },
        age: if matches!(flight, Flight::Tail | Flight::Homing) {
            7 + n
        } else {
            0
        },
        arming: if flight == Flight::Tail { 0 } else { 9 },
        life: if flight == Flight::Missile { 0x3b0 } else { 0 },
        damage: match flight {
            Flight::Shell | Flight::Homing => 2,
            Flight::Missile => synthetic_tables().weapons.missile.damage,
            Flight::Tail => 0,
        },
        target: matches!(flight, Flight::Tail | Flight::Homing).then_some(EntityId(7)),
        turn: if flight == Flight::Tail { 0x200 } else { 0 },
        phase: if flight == Flight::Tail { 4 } else { 0 },
        frame: if flight == Flight::Missile { 2 } else { 0 },
    }
}

/// A mission run a while, with a relay deployed, a target locked, a robot recalled,
/// one of each shot and an explosion in flight: everything the sim keeps comes back
/// from the save, but what the original's loader also resets (the scripts restart
/// with the program given, the targets are dropped, and the counts carried from
/// mission to mission start over).
#[test]
fn a_saved_world_loads_back_as_the_loader_leaves_it() {
    let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
    let tables = synthetic_tables();
    let mut sim = Sim::load(&scenario, Random::seeded(3), tables.clone());
    for _ in 0..300 {
        sim.tick(&[]);
    }
    // The scorpion (slot 5) deploys a relay; the humanoid (2) locks the scout (7).
    let scorpion = EntityId(5);
    movement(&mut sim.world, scorpion).unwrap().relays = 12;
    if let Damage::Robot(c) = &mut sim.world.get_mut(scorpion).unwrap().damage {
        c.equipment &= 0x7f;
    }
    sim.pilot(scorpion, Pilot::Deploy);
    movement(&mut sim.world, EntityId(2)).unwrap().target = Some(EntityId(7));
    sim.recall(EntityId(7));
    // The scout is in its base, out of the field; the humanoid is out.
    for (slot, in_base) in [(7, true), (2, false)] {
        if let Damage::Robot(c) = &mut sim.world.get_mut(EntityId(slot)).unwrap().damage {
            c.equipment =
                c.equipment & !usg::Robot::IN_BASE | (u8::from(in_base) * usg::Robot::IN_BASE);
        }
    }
    sim.pilot(EntityId(2), Pilot::Faster);
    // Heights the file did not hold.
    let scout = sim.world.get_mut(EntityId(7)).unwrap();
    (scout.z, scout.model_height) = (Fixed(0x3_0042), 0x0cb0);
    movement(&mut sim.world, EntityId(7)).unwrap().eye_height = 0x0c58;
    let at = Position {
        x: Fixed(0x70_8000),
        y: Fixed(-0x10_0000),
    };
    for (n, flight) in [Flight::Tail, Flight::Shell, Flight::Homing, Flight::Missile]
        .into_iter()
        .enumerate()
    {
        let kind = [TAIL, SHELL, HOMING, MISSILE][n];
        let e = Entity {
            kind: kind.into(),
            side: if n % 2 == 0 {
                usg::Side::Alien
            } else {
                usg::Side::Human
            },
            position: at,
            z: Fixed(0x1_2345 + i32::from(n as u16)),
            model_height: 0,
            heading: 0x1234 + n as u16,
            radius: 0,
            damage: Damage::None,
            state: State::Shot(shot(flight, n as u16)),
        };
        sim.world.insert(EntityId(20 + n as u16), e);
    }
    crate::damage::spawn_explosion(
        &mut sim.world,
        at,
        Fixed(-0x200),
        tables.damage.explosion_kind,
        tables.damage.robot_explosion,
    );
    let table = tables.scripts.start;
    let (bytes, back, again) = round_trip(&sim, &scenario, table);

    assert_eq!(again.world, as_loaded(&sim.world, &again.world));
    assert_eq!(
        again.world.len(),
        scenario.objects.as_ref().unwrap().len() + 6
    );

    // The saved forms: the script from the program table, the target as its slot.
    let humanoid = back
        .objects
        .as_ref()
        .unwrap()
        .iter()
        .find(|o| o.slot == 2)
        .unwrap();
    let r = humanoid.robot().unwrap();
    let State::Robot(m) = sim.world.get(EntityId(2)).unwrap().state else {
        panic!()
    };
    assert_eq!(
        r.script,
        m.script.pointer.wrapping_sub(table) | SAVED_SCRIPT
    );
    assert_eq!((r.target, r.mode & usg::Robot::PROGRAM), (7, 0));
    let objects = back.objects.as_ref().unwrap();
    // The clock is the world's; the day and month the scenario's.
    let (c, start) = (sim.world.clock, back.header.start);
    assert_eq!(
        (start.hour, start.minute, start.second, start.tick),
        (c.hour, c.minute, c.second, c.tick)
    );
    assert_eq!(
        (start.day, start.month),
        (scenario.header.start.day, scenario.header.start.month)
    );
    let flags = |slot: u16| objects.iter().find(|o| o.slot == slot).unwrap().flags;
    assert_eq!(
        (flags(7) & flag::IN_FIELD, flags(2) & flag::IN_FIELD),
        (0, flag::IN_FIELD)
    );
    // Objects made in play take free indices of their pool, a reference each.
    let relay = objects.iter().find(|o| o.slot == 1).unwrap();
    assert_eq!((relay.kind, relay.ref_count), (ObjectKind::Relay, 1));
    let small: BTreeSet<u16> = objects
        .iter()
        .filter(|o| o.size == Object::SMALL)
        .map(|o| o.pool_index)
        .collect();
    assert_eq!(
        small.len(),
        objects.iter().filter(|o| o.size == Object::SMALL).count()
    );

    // Saved and loaded again, the same world and the same bytes.
    let (bytes_again, _, twice) = round_trip(&again, &back, table);
    assert_eq!(twice.state_hash(), again.state_hash());
    assert_eq!(twice.world, again.world);
    let (third, ..) = round_trip(&twice, &back, table);
    assert_eq!(third, bytes_again);
    assert_ne!(bytes, bytes_again, "the first load restarted the scripts");
}

#[test]
fn the_paths_go_back_over_their_loaded_slots() {
    let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
    let mut sim = Sim::load(&scenario, Random::seeded(3), synthetic_tables());
    let mut paths = sim.world.paths.clone();
    paths.alien.slots[0].points.pop();
    paths.alien.slots[0].weight = 5;
    paths.human.count = 1;
    sim.set_paths(paths);
    let saved = sim.world.save(&scenario, 0);
    let (alien, before) = (
        saved.alien_paths.unwrap(),
        scenario.alien_paths.clone().unwrap(),
    );
    let (p, q) = (alien.paths[0], before.paths[0]);
    assert_eq!((p.weight, p.point_count), (5, q.point_count - 1));
    // The point dropped stays behind as a leftover, and the counts as loaded.
    assert_eq!((p.points, p.on_path, p.mix), (q.points, q.on_path, q.mix));
    assert_eq!(saved.human_paths.unwrap().count, 1);
    // No table loaded and none in the world: none written.
    let bare = Scenario {
        human_paths: None,
        ..scenario.clone()
    };
    let mut world = Sim::load(&bare, Random::seeded(3), synthetic_tables());
    world.world.paths.human = PathTable::default();
    assert_eq!(world.world.save(&bare, 0).human_paths, None);
}

/// Every stock scenario, run five minutes of game time (fights, shots, explosions and
/// relays included), saved and loaded: the world comes back as the loader leaves it,
/// a second save and load changes nothing, and both go on identically.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_stock_missions_save_and_load_back() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let table = xeno_formats::editor::read(&exe).unwrap().program_table;
    let names = (0..13)
        .map(|n| format!("scen{n:02}a.usg"))
        .chain(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
    let mut made = 0;
    for name in names {
        let scenario = usg::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(5), tables.clone());
        for _ in 0..4500 {
            _ = sim.tick(&[]);
        }
        made += sim
            .world
            .iter()
            .filter(|(_, e)| matches!(e.state, State::Shot(_) | State::Explosion { .. }))
            .count();
        let (_, back, mut again) = round_trip(&sim, &scenario, table);
        assert_eq!(again.world, as_loaded(&sim.world, &again.world), "{name}");
        let (_, _, mut twice) = round_trip(&again, &back, table);
        for _ in 0..2000 {
            _ = again.tick(&[]);
            _ = twice.tick(&[]);
        }
        assert_eq!(twice.state_hash(), again.state_hash(), "{name}");
    }
    assert!(made > 0, "no shot or explosion was saved");
}
