use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::robot;
use crate::world::{Fixed, Movement, State};
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
