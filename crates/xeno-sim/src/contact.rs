//! The robots' contact check (`1000:d464`, `docs/sim.md`, "Contacts (T-141)"): from
//! each kind's tick table, a robot near another object ahead is told so (event 9), and
//! one touching it bumps back off it.

use xeno_formats::usg::{ObjectKind, Robot};

use crate::motion::{Event, Tables, match_gear, movement, robot};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::script::{bearing, post};
use crate::terrain::approx_distance;
use crate::world::{EntityId, World};

/// The event a contact ahead posts.
pub const EVENT_CONTACT: u8 = 9;
/// How near, past both radii, another object is a contact (the distance's units).
const NEAR: i64 = 0x6400;
/// Contact checks skipped after a bump.
const BUMP_WAIT: u8 = 4;

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
    let (at, heading, radius) = (e.position, e.heading, e.radius);
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
    let near: Vec<(EntityId, bool, bool)> = world
        .iter()
        .filter(|&(other, o)| {
            other != id
                && crate::shot::in_field(o)
                && !(o.kind == ObjectKind::Base && o.side == side)
        })
        .filter_map(|(other, o)| {
            let gap = i64::from(approx_distance(at, o.position).0)
                - i64::from(radius)
                - i64::from(o.radius);
            // shortcut: "ahead" (`1000:45f2`) is untraced; taken as the coarse bearing
            // within a quarter turn of the heading. Upgrade with T-183.
            let off = bearing(&tables.atan, at, o.position).wrapping_sub(heading) as i16;
            (gap < NEAR).then_some((other, off.unsigned_abs() < 0x4000, gap < 0))
        })
        .collect();
    for (other, ahead, touching) in near {
        let Some(m) = movement(world, id) else {
            return;
        };
        if ahead {
            post(m, EVENT_CONTACT);
        }
        if touching {
            // Back off as from a hill: the velocity waits for the speed's next step.
            m.mode |= Robot::BUMPED;
            m.speed = m.speed.wrapping_neg();
            m.target_speed = 0;
            m.bump_wait = BUMP_WAIT;
            match_gear(tables, kind, m);
            events.push(Event::Bumped {
                robot: id,
                object: other,
            });
            crate::damage::hit(world, id, 1, tables, random, events);
            // shortcut: one bump a check, the rest of the field unchecked; whether the
            // original goes on is untraced. Upgrade with T-183.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::synthetic_tables;
    use crate::motion::tests::robot;
    use crate::world::{Fixed, Movement, State};
    use xeno_formats::usg::Side;

    fn m(world: &World, id: u16) -> Movement {
        match world.get(EntityId(id)).unwrap().state {
            State::Robot(m) => m,
            other => panic!("{other:?}"),
        }
    }

    /// A human humanoid (slot 1, facing +y) driving at an alien scout `gap` 65,536ths
    /// of a unit past both radii ahead (slot 2), on the synthetic contact entry (8).
    fn field(gap: i32) -> World {
        let mut world = World::default();
        world.insert(
            EntityId(1),
            robot(ObjectKind::Humanoid, Side::Human, 100, 100),
        );
        let mut scout = robot(ObjectKind::Scout, Side::Alien, 100, 100);
        scout.position.y = Fixed(scout.position.y.0 + 0x1400 + gap);
        world.insert(EntityId(2), scout);
        let m = movement(&mut world, EntityId(1)).unwrap();
        (m.tick, m.speed, m.target_speed) = (0x10, 40, 40);
        world
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
    fn a_robot_near_another_ahead_is_told() {
        let mut world = field(0x6000);
        assert!(check(&mut world).is_empty());
        let r = m(&world, 1);
        assert_eq!((r.script.events, r.speed), (1 << EVENT_CONTACT, 40));
        // Out of reach, or behind it: nothing.
        for (gap, heading) in [(0x6400, 0), (0x6000, 0x8000)] {
            let mut world = field(gap);
            world.get_mut(EntityId(1)).unwrap().heading = heading;
            check(&mut world);
            assert_eq!(m(&world, 1).script.events, 0);
        }
    }

    #[test]
    fn robots_driven_into_each_other_bump_back() {
        let mut world = field(-0x100);
        let events = check(&mut world);
        let bump = Event::Bumped {
            robot: EntityId(1),
            object: EntityId(2),
        };
        assert_eq!(events.first(), Some(&bump));
        let r = m(&world, 1);
        assert_eq!((r.speed, r.target_speed, r.bump_wait), (-40, 0, BUMP_WAIT));
        assert_ne!(r.mode & Robot::BUMPED, 0);
        // The 1-point hit (event 7) and the contact ahead (event 9).
        assert_eq!(r.script.events, 1 << 7 | 1 << EVENT_CONTACT);
        // The next four checks are skipped; other entries do not count.
        for tick in [0x12, 0x10, 0x10, 0x10, 0x10] {
            movement(&mut world, EntityId(1)).unwrap().tick = tick;
            assert!(check(&mut world).is_empty());
        }
        assert_eq!(m(&world, 1).bump_wait, 0);
        assert_eq!(check(&mut world).first(), Some(&bump));
    }

    #[test]
    fn a_robot_in_its_base_is_not_checked() {
        let mut world = field(-0x100);
        if let crate::world::Damage::Robot(c) = &mut world.get_mut(EntityId(1)).unwrap().damage {
            c.equipment |= Robot::IN_BASE;
        }
        movement(&mut world, EntityId(1)).unwrap().mode = Robot::BUMPED;
        assert!(check(&mut world).is_empty());
        assert_eq!(m(&world, 1).mode, 0);
    }
}
