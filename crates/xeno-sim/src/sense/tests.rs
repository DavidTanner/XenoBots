use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::robot;
use crate::world::{Components, Damage, Fixed, Movement, State};
use xeno_formats::usg::{ObjectKind, Side};

fn m(world: &World, id: u16) -> Movement {
    match world.get(EntityId(id)).unwrap().state {
        State::Robot(m) => m,
        other => panic!("{other:?}"),
    }
}

/// A human humanoid (slot 1) with an alien scout 3 units away (2), another 6 away
/// (3) and a human scorpion beside it (4); the synthetic humanoid scans 5 units.
fn world() -> World {
    let mut world = World::default();
    world.insert(
        EntityId(1),
        robot(ObjectKind::Humanoid, Side::Human, 100, 100),
    );
    world.insert(EntityId(2), robot(ObjectKind::Scout, Side::Alien, 103, 100));
    world.insert(EntityId(3), robot(ObjectKind::Scout, Side::Alien, 106, 100));
    world.insert(
        EntityId(4),
        robot(ObjectKind::Scorpion, Side::Human, 101, 100),
    );
    world
}

/// One step of robot 1 at tick table entry `(tick & 0x1e) >> 1`.
fn step_at(world: &mut World, tick: u8) {
    movement(world, EntityId(1)).unwrap().tick = tick;
    step(world, EntityId(1), &synthetic_tables());
}

#[test]
fn the_scan_spots_enemy_units_in_range_every_sixteenth_count() {
    let mut world = world();
    // Fifteen counts in the scan's entry find nothing yet.
    for _ in 0..15 {
        step_at(&mut world, 0x1e);
    }
    assert_eq!(m(&world, 1).status & status::SCAN_COUNT, status::SCAN_COUNT);
    assert_eq!(m(&world, 1).contacts, 0);
    // Other entries, and +0x1a with its top bits set, do not count.
    step_at(&mut world, 0x1c);
    step_at(&mut world, 0x3e);
    assert_eq!(m(&world, 1).contacts, 0);
    // The sixteenth scans: the near alien is spotted and counted, event 1 posted.
    step_at(&mut world, 0x1e);
    let scanner = m(&world, 1);
    assert_eq!(scanner.status & status::SCAN_COUNT, 0);
    assert_eq!(scanner.contacts, 1);
    assert_eq!(scanner.script.events, 1 << EVENT_CONTACTS);
    assert_ne!(m(&world, 2).status & status::SPOTTED, 0);
    assert_eq!(m(&world, 3).status & status::SPOTTED, 0, "out of range");
    assert_eq!(m(&world, 4).status & status::SPOTTED, 0, "its own side");
}

#[test]
fn the_contacts_post_events_only_when_they_appear_or_go() {
    let mut world = world();
    let scan = |world: &mut World| {
        movement(world, EntityId(1)).unwrap().status |= status::SCAN_COUNT;
        step_at(world, 0x1e);
    };
    scan(&mut world);
    // Two in range: more contacts, no event.
    world.get_mut(EntityId(3)).unwrap().position.x = Fixed::from_int(104);
    movement(&mut world, EntityId(1)).unwrap().script.events = 0;
    scan(&mut world);
    assert_eq!((m(&world, 1).contacts, m(&world, 1).script.events), (2, 0));
    // None left: event 2.
    for id in [2, 3] {
        world.get_mut(EntityId(id)).unwrap().position.x = Fixed::from_int(120);
    }
    scan(&mut world);
    assert_eq!(m(&world, 1).contacts, 0);
    assert_eq!(m(&world, 1).script.events, 1 << EVENT_CLEAR);
    // Spotted stays.
    assert_ne!(m(&world, 2).status & status::SPOTTED, 0);
}

#[test]
fn within_is_the_original_distance_to_the_range_inclusive() {
    let a = Position::default();
    let b = |x: i32, y: i32| Position {
        x: Fixed(x),
        y: Fixed(y),
    };
    // max + min / 2: (4, 2) is 5 units.
    assert!(within(a, b(4 << 16, 2 << 16), 0x500));
    assert!(!within(a, b(4 << 16, (2 << 16) + 0x200), 0x500));
    // Past 256 units nothing is in range, whatever the range.
    assert!(!within(a, b(256 << 16, 0), 0xffff));
}

fn parts_of(world: &World, id: u16) -> Components {
    match world.get(EntityId(id)).unwrap().damage {
        Damage::Robot(c) => c,
        other => panic!("{other:?}"),
    }
}

#[test]
fn looking_locks_the_nearest_enemy_in_the_locking_range() {
    let mut world = world();
    let t = synthetic_tables();
    let text = t.sensing.messages;
    // A cloaked alien nearer than the scout at 3 units is skipped.
    world.insert(EntityId(5), robot(ObjectKind::Scout, Side::Alien, 101, 101));
    movement(&mut world, EntityId(5)).unwrap().status |= status::CLOAKED;
    let mut events = Vec::new();
    assert!(look(&mut world, EntityId(1), &t, &mut events));
    let me = m(&world, 1);
    assert_eq!(me.target, Some(EntityId(2)));
    assert_eq!(me.target_distance, 0x300, "the true distance, 8.8");
    assert_ne!(me.status & status::TRACKING, 0);
    assert_ne!(m(&world, 2).status & status::SPOTTED, 0);
    assert_eq!(me.panel.queue[0], text.locked);
    assert_eq!(
        events,
        [Event::Sound {
            event: t.sensing.lock_sound,
            only_for: Some(EntityId(1))
        }]
    );
    // Again: the same target, refused.
    assert!(look(&mut world, EntityId(1), &t, &mut events));
    assert_eq!(m(&world, 1).panel.queue[1], text.already);
    // No enemy within 8 units: a message.
    for id in [2, 3, 5] {
        world.get_mut(EntityId(id)).unwrap().position.x = Fixed::from_int(109);
    }
    let mut world2 = world.clone();
    movement(&mut world2, EntityId(1)).unwrap().target = None;
    assert!(!look(&mut world2, EntityId(1), &t, &mut events));
    assert!(m(&world2, 1).panel.queue.contains(&text.none_found));
}

#[test]
fn a_lock_is_refused_damaged_out_of_range_or_on_itself() {
    let t = synthetic_tables();
    let text = t.sensing.messages;
    let mut world = world();
    // 6 units is within the humanoid's 8, 8 is not.
    lock(&mut world, EntityId(1), EntityId(3), &t, &mut Vec::new());
    assert_eq!(m(&world, 1).target, Some(EntityId(3)));
    world.get_mut(EntityId(2)).unwrap().position.x = Fixed::from_int(108);
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    assert_eq!(
        m(&world, 1).panel.queue[..2],
        [text.locked, text.out_of_range]
    );
    movement(&mut world, EntityId(1)).unwrap().panel = Default::default();
    lock(&mut world, EntityId(1), EntityId(1), &t, &mut Vec::new());
    // Its own side: locked, with a warning.
    lock(&mut world, EntityId(1), EntityId(4), &t, &mut Vec::new());
    assert_eq!(m(&world, 1).target, Some(EntityId(4)));
    assert_eq!(
        m(&world, 1).panel.queue[..3],
        [text.itself, text.friendly, text.relocked]
    );
    let mut world = world.clone();
    if let Damage::Robot(c) = &mut world.get_mut(EntityId(1)).unwrap().damage {
        c.targeting |= 1;
    }
    movement(&mut world, EntityId(1)).unwrap().panel = Default::default();
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    assert_eq!(m(&world, 1).target, Some(EntityId(4)));
    assert_eq!(m(&world, 1).panel.queue[0], text.targeting_damaged);
    // A cloaked robot cannot be locked, silently.
    let mut world = self::world();
    movement(&mut world, EntityId(2)).unwrap().status |= status::CLOAKED;
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    assert_eq!(
        (m(&world, 1).target, m(&world, 1).panel.queue[0]),
        (None, 0)
    );
}

#[test]
fn a_takes_the_next_enemy_round_the_table_and_r_drops_it() {
    let t = synthetic_tables();
    let text = t.sensing.messages;
    let mut world = world();
    // A cloaked alien at slot 5 is passed over, as is the far one past slot 3.
    world.insert(EntityId(5), robot(ObjectKind::Scout, Side::Alien, 101, 101));
    movement(&mut world, EntityId(5)).unwrap().status |= status::CLOAKED;
    world.insert(EntityId(7), robot(ObjectKind::Scout, Side::Alien, 109, 100));
    let a = |world: &mut World| {
        next_target(world, EntityId(1), &t, &mut Vec::new());
        (m(world, 1).target, world.target_search)
    };
    // Slot order, not the nearest: 2, then 3, then round to 2 again (relocked).
    assert_eq!(a(&mut world), (Some(EntityId(2)), 3));
    assert_eq!(a(&mut world), (Some(EntityId(3)), 4));
    assert_eq!(a(&mut world), (Some(EntityId(2)), 3));
    assert_eq!(
        m(&world, 1).panel.queue[..3],
        [text.locked, text.relocked, text.relocked]
    );
    // From slot 239 the search wraps to 0.
    world.target_search = 239;
    assert_eq!(a(&mut world), (Some(EntityId(2)), 3));
    // R drops the target with its message; with none in range A posts its own and
    // leaves the search where it was.
    movement(&mut world, EntityId(1)).unwrap().panel = Default::default();
    drop_target(&mut world, EntityId(1), &t);
    for id in [2, 3] {
        world.get_mut(EntityId(id)).unwrap().position.x = Fixed::from_int(120);
    }
    assert_eq!(a(&mut world), (None, 3));
    assert_eq!(
        m(&world, 1).panel.queue[..2],
        [text.dropped, text.none_next]
    );
}

#[test]
fn the_upkeep_warns_the_target_and_works_out_its_bits() {
    let t = synthetic_tables();
    let mut world = world();
    // Due ahead and 2 units off: in the humanoid's arc (0x1000) and range (0x300).
    world.get_mut(EntityId(2)).unwrap().position = Position {
        x: Fixed::from_int(100),
        y: Fixed::from_int(102),
    };
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    step_at(&mut world, 10);
    assert_eq!(world.lock_window, (0x1000, 0x300));
    assert_eq!(parts_of(&world, 1).targeting, 6);
    assert_eq!(
        (m(&world, 1).target_bearing, m(&world, 1).target_distance),
        (0, 0x200)
    );
    assert_eq!(m(&world, 2).locked_on, 0xff);
    // A quarter turn off and 3 units: neither bit.
    world.get_mut(EntityId(2)).unwrap().position = Position {
        x: Fixed::from_int(103),
        y: Fixed::from_int(100),
    };
    step_at(&mut world, 10);
    assert_eq!(parts_of(&world, 1).targeting, 0);
    // The target's warning: event 3 once, then it fades over 8 halvings.
    let warn_at = |world: &mut World| {
        movement(world, EntityId(2)).unwrap().tick = 12;
        step(world, EntityId(2), &t);
    };
    warn_at(&mut world);
    assert_eq!(m(&world, 2).script.events, 1 << EVENT_LOCKED_ON);
    assert_ne!(parts_of(&world, 2).systems & 0x20, 0);
    movement(&mut world, EntityId(2)).unwrap().script.events = 0;
    for _ in 0..6 {
        warn_at(&mut world);
    }
    assert_eq!(m(&world, 2).script.events, 0);
    assert_ne!(parts_of(&world, 2).systems & 0x20, 0);
    warn_at(&mut world);
    assert_eq!(parts_of(&world, 2).systems & 0x20, 0);
}

#[test]
fn the_upkeep_loses_a_far_target_and_drops_a_destroyed_one() {
    let t = synthetic_tables();
    let text = t.sensing.messages;
    let mut world = world();
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    // Kept while its last distance is within the locking range, even if now past.
    world.get_mut(EntityId(2)).unwrap().position.x = Fixed::from_int(109);
    step_at(&mut world, 10);
    assert_eq!(m(&world, 1).target, Some(EntityId(2)));
    step_at(&mut world, 10);
    assert_eq!(m(&world, 1).target, None);
    assert_eq!(m(&world, 1).script.events, 1 << EVENT_LOST);
    assert!(m(&world, 1).panel.queue.contains(&text.lost));
    // Gone: dropped with no event.
    let mut world = self::world();
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    world.remove(EntityId(2));
    step_at(&mut world, 10);
    assert_eq!((m(&world, 1).target, m(&world, 1).script.events), (None, 0));
    assert!(m(&world, 1).panel.queue.contains(&text.destroyed));
    // No target: in the arc.
    step_at(&mut world, 10);
    assert_eq!(parts_of(&world, 1).targeting, 4);
}

#[test]
fn a_target_s_running_ecm_loses_the_lock() {
    let t = synthetic_tables();
    let mut world = world();
    lock(&mut world, EntityId(1), EntityId(2), &t, &mut Vec::new());
    // The time running loses it, not the status bit.
    movement(&mut world, EntityId(2)).unwrap().status |= xeno_formats::usg::status::ECM;
    step_at(&mut world, 10);
    assert_eq!(m(&world, 1).target, Some(EntityId(2)));
    movement(&mut world, EntityId(2)).unwrap().guns.ecm_time = 1;
    step_at(&mut world, 10);
    assert_eq!(m(&world, 1).target, None);
    assert_eq!(m(&world, 1).script.events, 1 << EVENT_LOST);
    assert!(m(&world, 1).panel.queue.contains(&t.sensing.messages.lost));
}

#[test]
fn steering_for_the_nearest_enemy_takes_the_scanning_range() {
    let t = synthetic_tables();
    let mut world = world();
    assert!(steer_to_nearest(&mut world, EntityId(1), &t));
    assert_eq!(m(&world, 1).target_heading, 0x4000);
    assert_eq!(m(&world, 1).target, None, "not locked");
    // 6 units is past the humanoid's 5.
    world.remove(EntityId(2));
    assert!(!steer_to_nearest(&mut world, EntityId(1), &t));
}

/// Two stock robots in range find and close on each other: scen00a's first human
/// humanoid (on ENGAGE) and the first alien robot (on PATROL), the alien put 7 units
/// off, within both locking ranges, halfway between the bases, with the mission's
/// other robots taken out.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_two_stock_robots_find_and_close_on_each_other() {
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
    // The fight decides whether the human retreats once hit: with the fine aim (T-146)
    // seed 7's human turns back, seed 3's closes (most seeds' turn back either way).
    let mut sim = crate::Sim::load(&scenario, crate::random::Random::seeded(3), tables);
    let world = &sim.world;
    let human = *world
        .units(Side::Human)
        .iter()
        .find(|&&id| world.get(id).unwrap().kind == ObjectKind::Humanoid)
        .unwrap();
    let alien = world.units(Side::Alien)[0];
    // Only the two robots stay, halfway between the bases and 7 units apart.
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
    let apart = |sim: &crate::Sim| {
        let (a, b) = (sim.world.get(human), sim.world.get(alien));
        true_distance(a.unwrap().position, b.unwrap().position) >> 8
    };
    // ENGAGE locks at once and drives at high speed; the alien's scan finds the human
    // within 256 ticks and event 1 runs ENGAGING. Within two minutes both hold each
    // other as targets and stand under 2 units apart, where ENGAGING stops. (They
    // fire too, and a hit on a targeting computer can drop a lock later: T-114.)
    let mut met = false;
    for _ in 0..70 * 120 {
        _ = sim.tick(&[]);
        met |= m(&sim.world, human.0).target == Some(alien)
            && m(&sim.world, alien.0).target == Some(human)
            && m(&sim.world, alien.0).contacts > 0
            && apart(&sim) < 0x1f4;
    }
    assert!(met, "{:#x}", apart(&sim));
}

/// scen00a's first human humanoid with the first two alien robots moved 3 and 5 units
/// east of it: A locks the first in slot order, a second A steps on to the other, R
/// drops it, with the user's messages.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_steps_through_the_stock_enemies_and_r_drops() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let text = tables.sensing.messages;
    let scenario =
        xeno_formats::usg::parse(&std::fs::read(dir.join("scen00a.usg")).unwrap()).unwrap();
    let mut sim = crate::Sim::load(&scenario, crate::random::Random::seeded(7), tables);
    let world = &mut sim.world;
    let human = *world
        .units(Side::Human)
        .iter()
        .find(|&&id| world.get(id).unwrap().kind == ObjectKind::Humanoid)
        .unwrap();
    let mut aliens = world.units(Side::Alien)[..2].to_vec();
    aliens.sort();
    let at = world.get(human).unwrap().position;
    for (&id, units) in aliens.iter().zip([3, 5]) {
        world.get_mut(id).unwrap().position = Position {
            x: Fixed(at.x.0 + units * 0x10000),
            y: at.y,
        };
    }
    sim.open_console();
    for &want in &aliens {
        sim.pilot(human, crate::Pilot::NextTarget);
        assert_eq!(m(&sim.world, human.0).target, Some(want));
    }
    sim.pilot(human, crate::Pilot::DropTarget);
    let me = m(&sim.world, human.0);
    assert_eq!(me.target, None);
    assert_eq!(
        me.panel.queue[..3],
        [text.locked, text.relocked, text.dropped]
    );
}

fn raw(x: i32, y: i32) -> Position {
    Position {
        x: Fixed(x),
        y: Fixed(y),
    }
}

#[test]
fn the_true_distance_is_rooted_at_its_sum_s_scale() {
    let o = Position::default();
    for ((x, y), d) in [
        ((3, 4), 5),
        ((0, 0), 0),
        // Exact squares and their neighbours: the floor, under 2^32 exact.
        ((0xffff, 0), 0xffff),
        ((0xffff, 1), 0xffff),
        ((-0xffff, -1), 0xffff),
        ((12, 0), 12),
        ((12, 4), 12),
        ((12, 5), 13),
        // From 1 unit an axis is squared from its middle word: 0x100^2 << 16.
        ((0x1_0000, 0), 0x1_0000),
        ((0x1_00ff, 0), 0x1_0000),
        // A sum from 2^32 is rooted from its top 32 bits of 40 and shifted back:
        // 2 * 0xffff^2 = 0x1_fffc_0002, root(0x1ff_fc00) = 5792, times 16.
        ((0xffff, 0xffff), 5792 << 4),
        // From 2^40, its top 32 of 48: (2^20 >> 8)^2 << 16 = 2^40, root 2^12, times 2^8.
        ((0x10_0000, 0), 0x10_0000),
        // From 2^48 its top 32 of 64: 2 * 0x7fff^2 << 32, root 46339.
        ((0x7fff_ffff, 0x7fff_ffff), 46339 << 16),
        // The widest: two differences of 2^31 (wrapped), 2^63, root 46340.
        ((i32::MIN, i32::MIN), 46340 << 16),
    ] {
        assert_eq!(true_distance(o, raw(x, y)), d, "{x:#x}, {y:#x}");
        assert_eq!(true_distance(raw(x, y), o), d, "{x:#x}, {y:#x}");
    }
}

proptest::proptest! {
    #[test]
    fn the_true_distance_is_the_floor_root_under_one_unit(
        x in -0xffff..=0xffffi32, y in -0xb504..=0xb504i32,
    ) {
        // Both squares exact and the sum under 2^32: the floor of the root.
        let exact = (i64::from(x) * i64::from(x) + i64::from(y) * i64::from(y)).isqrt();
        if exact < 0x1_0000 && i64::from(x).pow(2) + i64::from(y).pow(2) < 1 << 32 {
            proptest::prop_assert_eq!(i64::from(true_distance(raw(0, 0), raw(x, y))), exact);
        }
    }

    #[test]
    fn the_true_distance_never_passes_the_exact_root(
        ax in proptest::num::i32::ANY, ay in proptest::num::i32::ANY,
        bx in proptest::num::i32::ANY, by in proptest::num::i32::ANY,
    ) {
        let (dx, dy) = (bx.wrapping_sub(ax).unsigned_abs(), by.wrapping_sub(ay).unsigned_abs());
        let exact = (u128::from(dx).pow(2) + u128::from(dy).pow(2)).isqrt();
        let d = u128::from(true_distance(raw(ax, ay), raw(bx, by)));
        // An axis squared from its top 16 bits loses under 1/256 of it; the root at a
        // coarser scale less.
        proptest::prop_assert!(d <= exact && exact - d <= (exact >> 7) + 1, "{d} {exact}");
    }
}
