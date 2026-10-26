//! Hits, destruction and explosions, as `sim_damage_object` and the kinds' routines do
//! them (`docs/sim.md`, "Damage"). The thresholds, chance tables and explosion
//! templates come from the user's `xeno.exe` (`xeno_formats::damage`, in
//! [`Tables::damage`]).

use xeno_formats::damage::{Explosion, Part};
use xeno_formats::usg::{ObjectKind, Side};

use crate::motion::{Event, Tables, movement};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::world::{Components, Damage, Entity, EntityId, Position, State, World};

/// The event a hit posts to the robot's script.
const EVENT_HIT: u8 = 7;
/// The object table's slots, and the small objects' pool (everything but robots and
/// bases).
const SLOTS: u16 = 240;
const SMALL_POOL: usize = 200;

/// Object `id` takes a hit of `points` (`sim_damage_object`). Robots wear down or are
/// destroyed, bases are destroyed and relays removed at their thresholds; anything
/// else takes nothing.
pub fn hit(
    world: &mut World,
    id: EntityId,
    points: u8,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some(e) = world.get_mut(id) else {
        return;
    };
    let (d, kind, at) = (&tables.damage, e.kind, e.position);
    match (RobotKind::of(kind), &mut e.damage, &mut e.state) {
        (Some(kind), Damage::Robot(parts), _) => {
            // shortcut: the screen flash and the hit sound are the console's and
            // audio's (T-104, M5).
            let robot = &d.robots[kind.index()];
            if points > robot.destroy_over {
                destroy(world, id, tables, events);
                return;
            }
            for _ in 0..points {
                let high = (random.word() >> 8) as u8;
                if let Some(part) = pick(&robot.parts, high) {
                    wear(parts, part, random);
                }
            }
            if let Some(m) = movement(world, id) {
                crate::script::post(m, EVENT_HIT);
            }
        }
        (_, Damage::Taken(taken), State::Base { destroyed, .. }) => {
            if !*destroyed {
                *taken = taken.wrapping_add(points.into());
                // shortcut: the messages to each side are the consoles' (T-104); the
                // mission's end the base's +0x30 then runs is T-055.
                *destroyed = *taken >= d.base_destroyed;
                if *destroyed {
                    events.push(Event::Destroyed(id));
                }
            }
        }
        (_, Damage::Taken(taken), _) if kind == ObjectKind::Relay => {
            let (sum, carry) = (*taken as u8).overflowing_add(points);
            *taken = sum.into();
            if carry || sum >= d.relay_removed {
                spawn_explosion(world, at, d.explosion_kind, d.relay_explosion);
                world.remove(id);
                events.push(Event::Destroyed(id));
            }
        }
        _ => {}
    }
}

/// The part a random byte `high` picks: the first entry where the chances, added up
/// in a byte, carry or reach `high`.
fn pick(parts: &[(u8, Part)], high: u8) -> Option<&Part> {
    let mut sum = 0u8;
    parts.iter().find_map(|(chance, part)| {
        let carry;
        (sum, carry) = sum.overflowing_add(*chance);
        (carry || high <= sum).then_some(part)
    })
}

/// One component routine on the robot's `parts`.
fn wear(parts: &mut Components, part: &Part, random: &mut Random) {
    match part {
        Part::Wear { offset, mask } => {
            let off = random.word() as u8 & mask;
            if let Some(byte) = parts.byte(*offset) {
                *byte = byte.saturating_sub(off);
            }
        }
        Part::Bits(bits) => {
            for &(offset, bit) in bits {
                if let Some(byte) = parts.byte(offset)
                    && *byte & bit == 0
                {
                    *byte |= bit;
                    break;
                }
            }
        }
        Part::Nothing => {}
    }
}

/// Robot `id` is destroyed (`sim_destroy_robot`): a loss for its side and a kill for
/// the other, an explosion where it stood, and it leaves its unit list, its orders
/// and the world.
// shortcut: its call sign's slot, the roster's redraw and selection and the view's
// change are the consoles' and situation room's (T-103, T-104).
pub fn destroy(world: &mut World, id: EntityId, tables: &Tables, events: &mut Vec<Event>) {
    let Some(e) = world.get(id) else {
        return;
    };
    let (side, at) = (e.side, e.position);
    let other = match side {
        Side::Human => Side::Alien,
        Side::Alien => Side::Human,
    };
    for (side, loss) in [(side, true), (other, false)] {
        if let Some(State::Base { kills, losses, .. }) = world
            .base(side)
            .and_then(|b| world.get_mut(b))
            .map(|b| &mut b.state)
        {
            let count = if loss { losses } else { kills };
            *count = count.wrapping_add(1);
        }
    }
    let d = &tables.damage;
    spawn_explosion(world, at, d.explosion_kind, d.robot_explosion);
    world.units[usize::from(crate::world::side_code(side))].retain(|&u| u != id);
    world.orders.remove(id.0);
    world.remove(id);
    events.push(Event::Destroyed(id));
}

/// An explosion of `kind` at `at` from `template` (`sim_spawn_explosion`), in the
/// lowest free slot; none if the slots or the small objects' pool are full.
// shortcut: a freed slot is reused at once; the original waits for the robots that
// target the object to let go of it (T-112).
fn spawn_explosion(world: &mut World, at: Position, kind: u16, template: Explosion) {
    let small = world
        .iter()
        .filter(|(_, e)| !matches!(e.kind, ObjectKind::Base) && RobotKind::of(e.kind).is_none())
        .count();
    let Some(slot) = (0..SLOTS).find(|&s| world.get(EntityId(s)).is_none()) else {
        return;
    };
    if small >= SMALL_POOL {
        return;
    }
    let explosion = Entity {
        kind: kind.into(),
        side: Side::Human,
        position: at,
        heading: template.heading,
        radius: 0x800,
        damage: Damage::None,
        state: State::Explosion {
            frame: template.first,
            last: template.last,
            ticks: template.ticks,
            countdown: template.ticks,
        },
    };
    world.insert(EntityId(slot), explosion);
}

/// An explosion's tick (`sim_tick_explosion`): the next frame every `ticks` ticks, and
/// gone after the last. Returns whether it is gone.
pub fn tick_explosion(state: &mut State) -> bool {
    let State::Explosion {
        frame,
        last,
        ticks,
        countdown,
    } = state
    else {
        return false;
    };
    *countdown = countdown.wrapping_sub(1);
    if *countdown != 0 {
        return false;
    }
    *countdown = *ticks;
    if frame == last {
        return true;
    }
    *frame = frame.wrapping_add(1);
    false
}

#[cfg(test)]
mod tests;
