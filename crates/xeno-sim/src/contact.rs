//! The robots' contact check (`1000:d464`, `docs/sim.md`, "Contacts (T-141)"): from
//! each kind's tick table, a robot near another object ahead is told so (event 9), and
//! one touching it bumps back off it.

use xeno_formats::sensing::Contact;
use xeno_formats::usg::{ObjectKind, Robot};

use crate::motion::{Event, Tables, match_gear, movement, robot};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::script::post;
use crate::terrain::approx_distance;
use crate::world::{Entity, EntityId, Position, State, World};

/// The event a contact ahead posts.
pub const EVENT_CONTACT: u8 = 9;

/// Robot `id`'s contact check, on its kind's tick-table entry for it.
pub fn step(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some((kind, side, m)) = robot(world, id) else {
        return;
    };
    let Some(k) = RobotKind::of(kind) else {
        return;
    };
    if (m.tick & 0x1e) >> 1 != tables.sensing.ticks[k.index()].contact {
        return;
    }
    let Some(e) = world.get(id) else {
        return;
    };
    let (me, radius) = ((e.position, words(e)), e.radius);
    let in_base = crate::base::in_base(&e.damage);
    let Some(m) = movement(world, id) else {
        return;
    };
    if in_base {
        m.mode &= !Robot::BUMPED;
        return;
    }
    if m.bump_wait != 0 {
        m.bump_wait -= 1;
        return;
    }
    let c = &tables.sensing.contact;
    // In slot order: each contact ahead before the first object touching is told.
    let (mut ahead_of, mut bump) = (false, None);
    for (other, o) in world.iter() {
        if other == id
            || !crate::shot::in_field(o)
            || (o.kind == ObjectKind::Base && o.side == side)
        {
            continue;
        }
        // Only the distance's low word: an object a unit or more away is not seen.
        let Ok(d) = u16::try_from(approx_distance(me.0, o.position).0) else {
            continue;
        };
        match d.checked_sub(radius).and_then(|g| g.checked_sub(o.radius)) {
            None => {
                bump = Some(other);
                break;
            }
            Some(gap) if gap <= c.near => ahead_of |= ahead(c, me, (o.position, words(o))),
            Some(_) => {}
        }
    }
    let Some(m) = movement(world, id) else {
        return;
    };
    if ahead_of {
        post(m, EVENT_CONTACT);
    }
    let Some(other) = bump else {
        return;
    };
    m.bump_wait = c.wait;
    // A robot already bumped (and not yet back in its base) only waits again.
    if m.mode & Robot::BUMPED != 0 {
        return;
    }
    // Back off as from a hill: the velocity waits for the speed's next step.
    m.mode |= Robot::BUMPED;
    m.speed = m.speed.wrapping_neg();
    m.target_speed = 0;
    match_gear(tables, kind, m);
    events.push(Event::Bumped {
        robot: id,
        object: other,
    });
    crate::damage::hit(world, id, 1, tables, random, events);
}

/// The words at +0x1e and +0x20, which the ahead test takes as every object's
/// velocity: a robot's velocity, a relay's clock and state, and stage and power, a
/// base's +0x1e and its relays and first count.
fn words(e: &Entity) -> (i16, i16) {
    match &e.state {
        State::Robot(m) => m.velocity,
        State::Relay(r) => (
            i16::from_le_bytes([r.clock, r.state]),
            i16::from_le_bytes([r.stage, r.power]),
        ),
        // shortcut: a base's +0x1e word (`usg::Base::unknown_1e`) is not kept and is
        // taken as 0, as in every stock file; keep it if anything is found writing it.
        State::Base { counts, depot, .. } => (0, i16::from_le_bytes([depot.relays, counts[0]])),
        // Obstacles: 0 in every stock file, cleared by the editor, written by nothing.
        _ => (0, 0),
    }
}

/// `1000:45f2`: whether `to` comes near `from` on their closing velocity: the
/// position of one from the other in 8.8 (16 bits, wrapping), stepped `steps` times
/// by a quarter of the velocities' difference, falls in the box on both axes.
fn ahead(c: &Contact, from: (Position, (i16, i16)), to: (Position, (i16, i16))) -> bool {
    let at = |a: i32, b: i32| (b.wrapping_sub(a) >> 8) as i16;
    let closing = |a: i16, b: i16| b.wrapping_sub(a) >> 2;
    let (mut x, mut y) = (at(from.0.x.0, to.0.x.0), at(from.0.y.0, to.0.y.0));
    let (dx, dy) = (closing(from.1.0, to.1.0), closing(from.1.1, to.1.1));
    let inside = |v: i16| v > c.back && v <= c.reach;
    (0..c.steps).any(|_| {
        x = x.wrapping_add(dx);
        y = y.wrapping_add(dy);
        inside(x) && inside(y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::synthetic_tables;
    use crate::motion::tests::robot;
    use crate::world::{Fixed, Movement, Position, State};
    use xeno_formats::usg::Side;

    fn m(world: &World, id: u16) -> Movement {
        match world.get(EntityId(id)).unwrap().state {
            State::Robot(m) => m,
            other => panic!("{other:?}"),
        }
    }

    /// A human humanoid (slot 1) driving +y at `speed` 65,536ths of a unit a tick
    /// at an alien scout (slot 2) `gap` 65,536ths of a unit past both radii; the
    /// synthetic contact entry (8), near 0x5000, a wait of 3 and an ahead box of
    /// -0x14 (exclusive) to 0x20 tried for 3 steps.
    fn field(gap: i32, speed: i16) -> World {
        let mut world = World::default();
        world.insert(
            EntityId(1),
            robot(ObjectKind::Humanoid, Side::Human, 100, 100),
        );
        world.insert(EntityId(2), scout(gap));
        let m = movement(&mut world, EntityId(1)).unwrap();
        (m.tick, m.speed, m.target_speed, m.velocity) = (0x10, 40, 40, (0, speed));
        world
    }

    fn scout(gap: i32) -> crate::world::Entity {
        let mut scout = robot(ObjectKind::Scout, Side::Alien, 100, 100);
        scout.position.y = Fixed(scout.position.y.0 + 0x1400 + gap);
        scout
    }

    fn check(world: &mut World) -> Vec<Event> {
        let mut events = Vec::new();
        let tables = synthetic_tables();
        step(
            world,
            EntityId(1),
            &tables,
            &mut Random::seeded(1),
            &mut events,
        );
        events
    }

    #[test]
    fn a_robot_closing_on_another_is_told() {
        // 0x54 (8.8) apart, closing by a quarter of 0x60 a step: 0x24, then 0x0c, in
        // the box on the third step.
        let mut world = field(0x4000, 0x60);
        assert!(check(&mut world).is_empty());
        let r = m(&world, 1);
        assert_eq!((r.script.events, r.speed), (1 << EVENT_CONTACT, 40));
        // Out of reach (near is the last gap that counts), too slow to come into the
        // box in three steps, standing, or driving away: nothing.
        for (gap, speed) in [(0x5001, 0x80), (0x4000, 0x40), (0x1000, 0), (0x4000, -0x60)] {
            let mut world = field(gap, speed);
            check(&mut world);
            assert_eq!(m(&world, 1).script.events, 0, "{gap:#x} {speed}");
        }
        let mut world = field(0x5000, 0x80);
        check(&mut world);
        assert_eq!(m(&world, 1).script.events, 1 << EVENT_CONTACT);
    }

    #[test]
    fn the_ahead_box() {
        let tables = synthetic_tables();
        let c = &tables.sensing.contact;
        let here = Position {
            x: Fixed(0),
            y: Fixed(0),
        };
        let at = |x: i32| Position {
            x: Fixed(x << 8),
            y: Fixed(0),
        };
        // From above the back (exclusive) up to the reach; the velocity is stepped
        // before the first test, so where it stands now does not count.
        for (x, inside) in [(-0x14, false), (-0x13, true), (0x20, true), (0x21, false)] {
            assert_eq!(ahead(c, (here, (0, 0)), (at(x), (0, 0))), inside, "{x:#x}");
        }
        assert!(!ahead(c, (here, (0, 0)), (at(0), (0x100, 0))));
        // The other's velocity counts as well as its own.
        assert!(ahead(c, (here, (0, 0)), (at(0x80), (-0x80, 0))));
    }

    #[test]
    fn robots_driven_into_each_other_bump_back() {
        let mut world = field(-0x100, 0x60);
        let events = check(&mut world);
        let bump = Event::Bumped {
            robot: EntityId(1),
            object: EntityId(2),
        };
        assert_eq!(events.first(), Some(&bump));
        let r = m(&world, 1);
        assert_eq!((r.speed, r.target_speed, r.bump_wait), (-40, 0, 3));
        assert_ne!(r.mode & Robot::BUMPED, 0);
        // The 1-point hit (event 7) only: a bump is not a contact ahead.
        assert_eq!(r.script.events, 1 << 7);
        // The next three checks are skipped; other entries do not count.
        for tick in [0x12, 0x10, 0x10, 0x10] {
            movement(&mut world, EntityId(1)).unwrap().tick = tick;
            assert!(check(&mut world).is_empty());
        }
        assert_eq!(m(&world, 1).bump_wait, 0);
        // Still bumped: touching again only waits again, with no backing off or hit.
        let m1 = movement(&mut world, EntityId(1)).unwrap();
        (m1.speed, m1.script.events) = (40, 0);
        assert!(check(&mut world).is_empty());
        let r = m(&world, 1);
        assert_eq!((r.speed, r.bump_wait, r.script.events), (40, 3, 0));
    }

    #[test]
    fn the_check_stops_at_the_first_bump() {
        // A contact ahead in slot 2 is told before slot 3 bumps.
        let mut world = field(0x4000, 0x60);
        world.insert(EntityId(3), scout(-0x100));
        assert_eq!(check(&mut world).len(), 2);
        let events = m(&world, 1).script.events;
        assert_eq!(events, 1 << 7 | 1 << EVENT_CONTACT);
        // One in slot 3 after a bump in slot 2 is not.
        let mut world = field(-0x100, 0x60);
        world.insert(EntityId(3), scout(0x4000));
        check(&mut world);
        assert_eq!(m(&world, 1).script.events, 1 << 7);
    }

    #[test]
    fn an_object_a_unit_away_is_not_seen() {
        // However wide: only the distance's low word is taken.
        let mut world = field(0x10000 - 0x1400, 0);
        world.get_mut(EntityId(2)).unwrap().radius = 0xffff;
        assert!(check(&mut world).is_empty());
        let mut world = field(0xffff - 0x1400, 0);
        world.get_mut(EntityId(2)).unwrap().radius = 0xffff;
        assert_eq!(check(&mut world).len(), 2);
    }

    #[test]
    fn a_robot_in_its_base_is_not_checked() {
        let mut world = field(-0x100, 0);
        if let crate::world::Damage::Robot(c) = &mut world.get_mut(EntityId(1)).unwrap().damage {
            c.equipment |= Robot::IN_BASE;
        }
        movement(&mut world, EntityId(1)).unwrap().mode = Robot::BUMPED;
        assert!(check(&mut world).is_empty());
        assert_eq!(m(&world, 1).mode, 0);
    }
}
