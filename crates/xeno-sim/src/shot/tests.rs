use super::*;
use crate::motion::tests::{at, robot};
use crate::motion::{movement, synthetic_tables};
use xeno_formats::usg::Side;

/// A human humanoid (slot 4) facing +y towards an alien scorpion `gap` units ahead
/// (slot 6), locked on it.
fn duel(gap: i16) -> World {
    let mut world = World::default();
    world.insert(
        EntityId(4),
        robot(ObjectKind::Humanoid, Side::Human, 10, 10),
    );
    world.insert(
        EntityId(6),
        robot(ObjectKind::Scorpion, Side::Alien, 10, 10 + gap),
    );
    if let Some(m) = movement(&mut world, EntityId(4)) {
        m.target = Some(EntityId(6));
    }
    world
}

fn shot(world: &World, id: u16) -> Shot {
    match world.get(EntityId(id)).map(|e| e.state) {
        Some(State::Shot(s)) => s,
        other => panic!("{other:?}"),
    }
}

fn hit_points(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::Sound { event: 21, .. }))
        .count()
}

/// Ticks every shot in `world` once, as `motion::tick` does.
fn tick_shots(world: &mut World, random: &mut Random) -> Vec<Event> {
    let (tables, mut events) = (synthetic_tables(), Vec::new());
    let shots: Vec<EntityId> = world
        .iter()
        .filter(|(_, e)| matches!(e.state, State::Shot(_)))
        .map(|(id, _)| id)
        .collect();
    for id in shots {
        tick(world, id, &tables, random, &mut events);
    }
    events
}

#[test]
fn the_muzzle_turns_with_the_heading_s_quarter() {
    let o = std::array::from_fn(|i| (i as i16 + 1, -(i as i16) - 10));
    assert_eq!(muzzle(&o, 0x0800), (2, -11));
    assert_eq!(muzzle(&o, 0x4000), (-10, -1));
    assert_eq!(muzzle(&o, 0x8000 + 0x3800), (-8, 17));
    assert_eq!(muzzle(&o, 0xc000), (10, 1));
}

#[test]
fn a_climb_shares_the_speed_between_level_and_up() {
    let t = synthetic_tables();
    assert_eq!(velocity3(&t, 0, 0, 0x400), [0, 0x400, 0]);
    assert_eq!(velocity3(&t, 0x4000, 0x4000, 0x400), [0, 0, 0x400]);
    let [x, y, z] = velocity3(&t, 0, 0x2000, 0x400);
    assert_eq!(x, 0);
    assert!((0x2d0..0x2e0).contains(&y) && y == z);
}

#[test]
fn a_humanoid_s_homing_shot_waits_turns_on_its_target_and_hits_it() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = duel(1);
    let cannon = tables.weapons.guns[0];
    assert!(fire(&mut world, EntityId(4), &cannon, &tables, &mut random));
    // The lowest free slot, at the muzzle (heading 0: the first offset), its gun's
    // height up, with no speed until its first turn.
    let s = shot(&world, 0);
    assert_eq!(
        (s.flight, s.z, s.damage, s.target),
        (Flight::Homing, 0xe00, 4, Some(EntityId(6)))
    );
    assert_eq!(
        world.get(EntityId(0)).unwrap().position,
        offset(at(10, 10), 0, 0)
    );
    for _ in 0..7 {
        tick_shots(&mut world, &mut random);
    }
    assert_eq!(
        world.get(EntityId(0)).unwrap().position.y,
        Fixed::from_int(10)
    );
    // At 8 ticks old it turns and flies (0x400 a tick, a unit in 64 ticks).
    let mut events = Vec::new();
    for _ in 0..80 {
        events.extend(tick_shots(&mut world, &mut random));
    }
    assert_eq!(hit_points(&events), 1, "{events:?}");
    assert!(events.contains(&Event::Sound {
        event: 4,
        only_for: None
    }));
    assert!(
        world
            .iter()
            .all(|(_, e)| !matches!(e.state, State::Shot(_)))
    );
    // Its explosion stands where it hit.
    let (_, x) = world
        .iter()
        .find(|(_, e)| matches!(e.state, State::Explosion { .. }))
        .unwrap();
    assert!(x.position.y.0 > Fixed::from_int(10).0);
}

#[test]
fn a_homing_shot_without_a_target_is_not_made() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(1));
    let mut world = duel(1);
    world.remove(EntityId(6));
    assert!(fire(
        &mut world,
        EntityId(4),
        &tables.weapons.guns[0],
        &tables,
        &mut random
    ));
    assert_eq!(world.len(), 1);
}

#[test]
fn a_wild_shell_arcs_and_falls_to_the_ground_heard_only_if_it_does_more_than_1() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(7));
    let mut world = duel(100);
    if let Some(m) = movement(&mut world, EntityId(4)) {
        m.target = None;
    }
    let cannon = tables.weapons.guns[5];
    assert!(fire(&mut world, EntityId(4), &cannon, &tables, &mut random));
    let s = shot(&world, 0);
    assert_eq!((s.flight, s.z, s.damage), (Flight::Shell, 0x900, 2));
    let mut ticks = 0;
    let mut events = Vec::new();
    while world
        .get(EntityId(0))
        .is_some_and(|e| matches!(e.state, State::Shot(_)))
    {
        events.extend(tick_shots(&mut world, &mut random));
        ticks += 1;
        assert!(ticks < 200);
    }
    // Up at most 0x7f, falling 3 a step from 0x900: it lands within a few dozen ticks.
    assert!((10..60).contains(&ticks), "{ticks}");
    assert_eq!(
        events,
        [Event::Sound {
            event: 4,
            only_for: None
        }]
    );
    // The scorpion's cannon does 1 point, and its shells land quietly.
    let mut world = duel(100);
    if let Some(m) = movement(&mut world, EntityId(4)) {
        m.target = None;
    }
    fire(
        &mut world,
        EntityId(4),
        &tables.weapons.guns[2],
        &tables,
        &mut random,
    );
    let mut events = Vec::new();
    for _ in 0..200 {
        events.extend(tick_shots(&mut world, &mut random));
    }
    assert!(events.is_empty());
}

#[test]
fn a_shot_hits_what_it_is_over_by_its_kind_s_test() {
    let tables = synthetic_tables();
    let mut world = duel(1);
    let mut wall = robot(ObjectKind::Obstacle, Side::Human, 20, 20);
    wall.state = State::None;
    world.insert(EntityId(8), wall);
    let over = |dx: i32| Position {
        x: Fixed(Fixed::from_int(10).0 + dx),
        y: Fixed::from_int(10),
    };
    // The humanoid only from just above its feet up to 0x1300.
    let me = EntityId(99);
    assert_eq!(
        find_hit(&world, me, over(0xa00), 0x100, &tables),
        Some(EntityId(4))
    );
    assert_eq!(find_hit(&world, me, over(0xa01), 0x100, &tables), None);
    assert_eq!(find_hit(&world, me, over(0), 0, &tables), None);
    assert_eq!(
        find_hit(&world, me, over(0), 0x1300, &tables),
        Some(EntityId(4))
    );
    assert_eq!(find_hit(&world, me, over(0), 0x1301, &tables), None);
    // A scorpion at any height; an obstacle under 0x900.
    let scorpion = Position {
        y: Fixed::from_int(11),
        ..over(0)
    };
    assert_eq!(
        find_hit(&world, me, scorpion, -5, &tables),
        Some(EntityId(6))
    );
    let wall = at(20, 20);
    assert_eq!(
        find_hit(&world, me, wall, 0x8ff, &tables),
        Some(EntityId(8))
    );
    assert_eq!(find_hit(&world, me, wall, 0x900, &tables), None);
    assert_eq!(find_hit(&world, EntityId(8), wall, 0, &tables), None);
}

#[test]
fn a_blast_halves_its_points_for_each_step_out_from_half_its_radius() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(3));
    // Robots at 0.25, 0.375 and 0.5 units from the blast, and one past its radius.
    let mut world = World::default();
    for (slot, x) in [(1, 0x4000), (2, 0x6000), (3, 0x8000), (4, 0x8100)] {
        let mut r = robot(ObjectKind::Scout, Side::Human, 0, 0);
        r.position.x = Fixed(x);
        world.insert(EntityId(slot), r);
    }
    let mut events = Vec::new();
    // Radius 0.5 units (0x80): full points to 0x40, half to 0x60, a quarter to 0x90.
    blast(
        &mut world,
        at(0, 0),
        0,
        (8, 0x80),
        &tables,
        &mut random,
        &mut events,
    );
    let worn = |slot| {
        let hit = Event::Sound {
            event: 21,
            only_for: Some(EntityId(slot)),
        };
        events.iter().filter(|&&e| e == hit).count()
    };
    assert_eq!([worn(1), worn(2), worn(3), worn(4)], [1, 1, 1, 0]);
    // Over 80 points destroys: 160 does it only within half the radius, 255 within
    // three quarters.
    let destroyed = |points: u8| {
        let mut world = World::default();
        for (slot, x) in [(1, 0x4000), (2, 0x4100), (3, 0x6000), (4, 0x6100)] {
            let mut r = robot(ObjectKind::Scout, Side::Human, 0, 0);
            r.position.x = Fixed(x);
            world.insert(EntityId(slot), r);
        }
        let (mut random, mut events) = (Random::seeded(9), Vec::new());
        blast(
            &mut world,
            at(0, 0),
            0,
            (points, 0x80),
            &tables,
            &mut random,
            &mut events,
        );
        events
            .iter()
            .filter_map(|e| match e {
                Event::Destroyed(id) => Some(id.0),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(destroyed(160), [1]);
    assert_eq!(destroyed(255), [1, 2, 3]);
}

#[test]
fn the_tail_missile_waits_boosts_then_homes_and_blasts_its_target() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(5));
    let mut world = duel(2);
    let tail = tables.weapons.guns[3];
    assert!(fire(&mut world, EntityId(4), &tail, &tables, &mut random));
    let s = shot(&world, 0);
    assert_eq!((s.flight, s.z, s.turn), (Flight::Tail, 0x19c0, 0x16c));
    for _ in 0..20 {
        tick_shots(&mut world, &mut random);
    }
    assert_eq!(world.get(EntityId(0)).unwrap().position, at(10, 10));
    assert_eq!(shot(&world, 0).phase, 2);
    let mut events = Vec::new();
    for _ in 0..400 {
        events.extend(tick_shots(&mut world, &mut random));
    }
    assert!(
        world
            .get(EntityId(0))
            .is_none_or(|e| !matches!(e.state, State::Shot(_)))
    );
    // 100 points: the scorpion is destroyed.
    assert!(
        events.contains(&Event::Destroyed(EntityId(6))),
        "{events:?}"
    );
}

#[test]
fn a_lead_aims_where_a_moving_target_will_be() {
    let tables = synthetic_tables();
    let mut world = duel(2);
    assert_eq!(aim(&mut world, EntityId(4), 0x300, &tables), 0);
    // Crossing to +x, it is led to the right.
    if let Some(m) = movement(&mut world, EntityId(6)) {
        m.velocity = (0x200, 0);
    }
    let led = aim(&mut world, EntityId(4), 0x300, &tables);
    assert!((0x100..0x2000).contains(&led), "{led:#x}");
    assert_eq!(world.point.x.0 & 0xffff, 0xc0);
}

#[test]
fn a_missile_flies_straight_and_ends_with_its_life() {
    let (tables, mut random) = (synthetic_tables(), Random::seeded(2));
    let mut world = duel(100);
    if let Some(m) = movement(&mut world, EntityId(4)) {
        m.target = None;
    }
    assert!(fire(
        &mut world,
        EntityId(4),
        &tables.weapons.guns[1],
        &tables,
        &mut random
    ));
    let s = shot(&world, 0);
    assert_eq!(
        (s.flight, s.life, s.damage, s.climb),
        (Flight::Missile, 0x3c0, 8, 0)
    );
    assert_eq!(world.get(EntityId(0)).unwrap().heading, 0x40);
    for _ in 0..0x3bf {
        tick_shots(&mut world, &mut random);
    }
    assert_eq!(shot(&world, 0).frame, (0x3bf & 3) as u8);
    tick_shots(&mut world, &mut random);
    assert!(world.get(EntityId(0)).is_none());
}
