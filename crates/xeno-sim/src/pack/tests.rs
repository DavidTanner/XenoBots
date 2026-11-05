use super::*;
use crate::Sim;
use crate::motion::{movement, synthetic_tables};
use crate::world::{Fixed, Position};
use crate::{Pilot, Random};
use xeno_formats::fixtures::synthetic_mission_usg;
use xeno_formats::usg::flag;

/// Writes `sim`'s world into a save's bytes and loads them back, as the log-in does.
fn round_trip(sim: &Sim, loaded: &Scenario, table: u16) -> (Vec<u8>, Scenario, Sim) {
    let bytes = usg::write(&sim.world.save(loaded, table)).unwrap();
    let back = usg::parse(&bytes).unwrap();
    let again = Sim::load(&back, Random::seeded(1), sim.tables.clone().unwrap());
    (bytes, back, again)
}

/// `running` as the original's loader leaves it, taking what the loader resets from
/// `loaded`: the scripts restart with the program given, the targets are dropped, and
/// the counts carried from mission to mission start over; an empty slot's reference
/// count is not saved, so it starts over at 0.
fn as_loaded(running: &World, loaded: &World) -> World {
    let mut expected = running.clone();
    expected.refs.retain(|&id, _| running.get(id).is_some());
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
    expected.target_search = loaded.target_search;
    (expected.game_clock, expected.attack_told) = (loaded.game_clock, loaded.attack_told);
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
/// one of each shot, an explosion, a sparkle and an alarm buoy in flight and the ECM and cloak on: everything the sim keeps comes back
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
        let e = sim.world.get_mut(EntityId(slot)).unwrap();
        e.flags = e.flags & !flag::IN_FIELD | (u8::from(!in_base) * flag::IN_FIELD);
        if let Damage::Robot(c) = &mut e.damage {
            c.equipment =
                c.equipment & !usg::Robot::IN_BASE | (u8::from(in_base) * usg::Robot::IN_BASE);
        }
    }
    sim.pilot(EntityId(2), Pilot::Faster);
    // The humanoid's ECM and cloak on, its sparkle part way through.
    let g = &mut movement(&mut sim.world, EntityId(2)).unwrap().guns;
    (g.ecm, g.cloaks) = (3, 2);
    sim.pilot(EntityId(2), Pilot::Ecm);
    sim.pilot(EntityId(2), Pilot::Cloak);
    let sparkle = sim
        .world
        .iter()
        .find(|(_, e)| matches!(e.state, State::Sparkle { .. }))
        .map(|(id, _)| id)
        .unwrap();
    sim.world.get_mut(sparkle).unwrap().state = State::Sparkle {
        frame: 2,
        ticks: 5,
        age: 9,
        stage: 1,
    };
    // The scorpion drops an alarm buoy, its alarm showing part way through a look.
    movement(&mut sim.world, scorpion).unwrap().guns.drops = [0, 2, 0, 0];
    sim.pilot(scorpion, Pilot::Drop(2));
    let buoy = sim
        .world
        .iter()
        .find(|(_, e)| matches!(e.state, State::Drop(_)))
        .map(|(id, _)| id)
        .unwrap();
    let e = sim.world.get_mut(buoy).unwrap();
    e.state = State::Drop(crate::drops::Drop {
        battery: 1,
        ticks: 0x41,
        level: 8,
        charges: 0,
    });
    e.flags |= flag::RELAY_LINKED;
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
            side: usg::Side::Human,
            position: at,
            z: Fixed(0x1_2345 + i32::from(n as u16)),
            model_height: 0,
            heading: 0x1234 + n as u16,
            radius: 0,
            damage: Damage::None,
            // Shots have no side bit; a status byte kept as any other.
            flags: 0,
            status: n as u8,
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
    // A midnight passed in play.
    (sim.world.clock.day, sim.world.clock.month) = (9, 5);
    let table = tables.scripts.start;
    let (bytes, back, again) = round_trip(&sim, &scenario, table);

    assert_eq!(again.world, as_loaded(&sim.world, &again.world));
    assert_eq!(
        again.world.len(),
        scenario.objects.as_ref().unwrap().len() + 8
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
    // The clock is the world's, the date with it.
    let (c, start) = (sim.world.clock, back.header.start);
    assert_eq!(
        (start.hour, start.minute, start.second, start.tick),
        (c.hour, c.minute, c.second, c.tick)
    );
    assert_eq!((start.day, start.month), (c.day, c.month));
    let flags = |slot: u16| objects.iter().find(|o| o.slot == slot).unwrap().flags;
    assert_eq!(
        (flags(7) & flag::IN_FIELD, flags(2) & flag::IN_FIELD),
        (0, flag::IN_FIELD)
    );
    // Objects made in play take the place their pool gave them, a reference each.
    let relay = objects.iter().find(|o| o.slot == 1).unwrap();
    assert_eq!((relay.kind, relay.ref_count), (ObjectKind::Relay, 1));
    // Every object's +0x02 is the pools': the file's for the loaded ones, the place
    // taken for the made ones. They need not differ: the fixture's relay names
    // place 3 but was loaded into 0, so a made object takes 3 as well (`f4ee`).
    for o in objects {
        assert_eq!(Some(o.pool_index), sim.world.pool_index(EntityId(o.slot)));
    }
    assert_eq!(relay.pool_index, 1);

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
        // Each object's +0x02 is the pools' (T-221), made in play or loaded.
        for o in back.objects.iter().flatten() {
            let index = sim.world.pool_index(EntityId(o.slot));
            assert_eq!(Some(o.pool_index), index, "{name} slot {}", o.slot);
        }
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

/// The references objects in `world` hold on each slot: a robot's object goal, a
/// scorpion's missile, a base's robot being repaired and a homing shot's or tail
/// missile's target (`docs/sim.md`, "Reference counts (T-154)").
fn held(world: &World) -> BTreeMap<EntityId, u16> {
    let mut held = BTreeMap::new();
    for (_, e) in world.iter() {
        let slots = match &e.state {
            State::Robot(m) => [
                (m.goal & 1 != 0).then_some(EntityId(m.goal_object)),
                m.guns.missile,
            ],
            State::Base { depot, .. } => [depot.repairing, None],
            State::Shot(s) => [s.target, None],
            _ => [None, None],
        };
        for s in slots.into_iter().flatten() {
            *held.entry(s).or_default() += 1;
        }
    }
    held
}

/// Every stock scenario saves its loaded counts unchanged; run a while with each human
/// robot given the alien base as its goal object, each slot's count has moved by its objects made and freed plus the references held on it, at
/// least (a homing shot on the ground keeps its target's, as the original's does).
#[test]
#[ignore]
fn real_assets_reference_counts_follow_the_references_held() {
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
    let mut references = 0;
    for name in names {
        let scenario = usg::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
        let counts = |s: &Scenario| -> Vec<(u16, u16)> {
            s.objects
                .iter()
                .flatten()
                .map(|o| (o.slot, o.ref_count))
                .collect()
        };
        let mut sim = Sim::load(&scenario, Random::seeded(5), tables.clone());
        assert_eq!(
            counts(&sim.world.save(&scenario, table)),
            counts(&scenario),
            "{name}"
        );
        let start = sim.world.clone();
        // Every human robot heads for the alien base: a reference each.
        let alien = sim.world.base(usg::Side::Alien).unwrap();
        for id in sim.world.units(usg::Side::Human).to_vec() {
            sim.pilot(id, Pilot::GoalObject(alien));
        }
        for tick in 1..=4500 {
            _ = sim.tick(&[]);
            if tick % 10 != 0 {
                continue;
            }
            let held = held(&sim.world);
            for slot in (0..Object::SLOTS as u16).map(EntityId) {
                let lives = |w: &World| i32::from(w.get(slot).is_some());
                let moved = sim.world.refs(slot).wrapping_sub(start.refs(slot)) as i16;
                let taken = i32::from(moved) - (lives(&sim.world) - lives(&start));
                let want = i32::from(held.get(&slot).copied().unwrap_or(0));
                assert_eq!(taken, want, "{name} tick {tick} slot {}", slot.0);
                references += want;
            }
        }
    }
    assert!(references > 0, "no reference was held");
}

#[test]
fn a_removed_object_saved_in_its_last_ticks_loads_back_and_frees_on_time() {
    let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
    let tables = synthetic_tables();
    let mut sim = Sim::load(&scenario, Random::seeded(3), tables.clone());
    let slot = sim.world.free_slot().unwrap();
    let removed = Entity {
        kind: tables.weapons.tail.removed.into(),
        side: usg::Side::Human,
        position: Position {
            x: Fixed(0x12_3400),
            y: Fixed(-0x5_6700),
        },
        z: Fixed(0x2_0000),
        model_height: 0,
        heading: 0x4321,
        radius: 0,
        damage: Damage::None,
        flags: flag::GONE,
        status: 2,
        state: State::Removed,
    };
    sim.world.insert(slot, removed.clone());
    let (_, back, mut again) = round_trip(&sim, &scenario, tables.scripts.start);
    let o = back
        .objects
        .iter()
        .flatten()
        .find(|o| o.slot == slot.0)
        .unwrap();
    assert_eq!(
        (u16::from(o.kind), o.size),
        (tables.weapons.tail.removed, Object::SMALL)
    );
    assert_eq!(again.world.get(slot), Some(&removed));
    assert_eq!(again.world.refs(slot), 1);
    again.tick(&[]);
    assert_eq!(again.world.get(slot).map(|e| e.status), Some(1));
    again.tick(&[]);
    assert_eq!((again.world.get(slot), again.world.refs(slot)), (None, 0));
}

/// Every stock scenario saved as loaded keeps its file's +0x02s (T-221); in scen06a,
/// with a relay out of order destroyed, an obstacle placed in the editor takes the
/// place the relay's index named (`f4ee`), and the save writes it so.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_made_objects_save_the_place_they_took() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let editor = xeno_formats::editor::read(&exe).unwrap();
    let names = (0..13)
        .map(|n| format!("scen{n:02}a.usg"))
        .chain(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
    for name in names {
        let scenario = usg::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
        let sim = Sim::load(&scenario, Random::seeded(5), tables.clone());
        let saved = sim.world.save(&scenario, editor.program_table);
        let index = |s: &Scenario| -> Vec<_> {
            s.objects
                .iter()
                .flatten()
                .map(|o| (o.slot, o.pool_index))
                .collect()
        };
        assert_eq!(index(&saved), index(&scenario), "{name}");
    }
    let scenario = usg::parse(&std::fs::read(dir.join("scen06a.usg")).unwrap()).unwrap();
    let small: Vec<_> = scenario
        .objects
        .iter()
        .flatten()
        .filter(|o| o.size == Object::SMALL)
        .collect();
    // Loaded in file order, the n-th small object lives in place n.
    let (relay, named) = small
        .iter()
        .enumerate()
        .find(|(n, o)| o.kind == ObjectKind::Relay && usize::from(o.pool_index) != *n)
        .map(|(_, o)| (o.slot, o.pool_index))
        .expect("a relay out of order");
    let mut sim = Sim::load(&scenario, Random::seeded(5), tables);
    sim.world.remove(EntityId(relay));
    let at = Position {
        x: Fixed::from_int(100),
        y: Fixed::from_int(100),
    };
    let Some(crate::editor::Placed::Made(o)) =
        sim.place(crate::editor::Place::Obstacle, at, &editor)
    else {
        panic!("no obstacle placed")
    };
    assert_eq!(o.pool_index, named);
    let saved = sim.world.save(&scenario, editor.program_table);
    let s = saved
        .objects
        .iter()
        .flatten()
        .find(|s| s.slot == o.slot)
        .unwrap();
    assert_eq!(s.pool_index, named);
}
