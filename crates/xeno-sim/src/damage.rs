//! Hits, destruction and explosions, as `sim_damage_object` and the kinds' routines do
//! them (`docs/sim.md`, "Damage"). The thresholds, chance tables and explosion
//! templates come from the user's `xeno.exe` (`xeno_formats::damage`, in
//! [`Tables::damage`]).

use xeno_formats::damage::{Explosion, Part};
use xeno_formats::usg::{self, ObjectKind, Side};

use crate::motion::{Event, Tables, movement};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::world::{Components, Damage, Entity, EntityId, Fixed, Position, State, World};

/// The event a hit posts to the robot's script.
const EVENT_HIT: u8 = 7;
/// The event the damage total posts when the robot becomes crippled.
const EVENT_CRIPPLED: u8 = 12;
/// Sound events (`docs/formats/dsounds.md`, "Sound events"): a robot hit, a robot
/// destroyed (and a base's big explosion).
const SOUND_HIT: u8 = 21;
const SOUND_DESTROYED: u8 = 2;
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
    let (d, kind, at, z) = (&tables.damage, e.kind, e.position, e.z);
    match (RobotKind::of(kind), &mut e.damage, &mut e.state) {
        (Some(kind), Damage::Robot(parts), _) => {
            // shortcut: the screen flash is the console's (T-104).
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
            events.push(Event::Sound {
                event: SOUND_HIT,
                only_for: Some(id),
            });
        }
        (_, Damage::Taken(taken), State::Base { destroyed, .. }) => {
            if !*destroyed {
                *taken = taken.wrapping_add(points.into());
                // shortcut: the messages to each side are the consoles' (T-104).
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
                spawn_explosion(world, at, z, d.explosion_kind, d.relay_explosion);
                world.remove(id);
                events.push(Event::Destroyed(id));
            }
        }
        _ => {}
    }
}

/// Robot `id`'s damage total (+0xf9), added up at the end of its kind's tick
/// (humanoid `1000:afee`, scorpion `94e9`, scout `c6fd`): a quarter of the power and
/// the helm lost and a weight per damaged part, in a byte, capped. At the crippled
/// level `CRIPPLED` is set (event 12 when it is new), below it cleared; at its kind's
/// limit for its side the robot is destroyed.
pub fn total(world: &mut World, id: EntityId, tables: &Tables, events: &mut Vec<Event>) {
    let Some(e) = world.get_mut(id) else {
        return;
    };
    let (Some(kind), Damage::Robot(c), State::Robot(m)) =
        (RobotKind::of(e.kind), &mut e.damage, &mut e.state)
    else {
        return;
    };
    let t = &tables.damage.totals[kind.index()];
    let mut parts = *c;
    let mut sum = 0u8;
    for &(offset, whole) in &t.lost {
        let v = parts.byte(offset).map_or(whole, |b| *b);
        sum = sum.wrapping_add(whole.wrapping_sub(v) >> 2);
    }
    for &(offset, bit, weight) in &t.weights {
        if parts.byte(offset).is_some_and(|b| *b & bit != 0) {
            sum = sum.wrapping_add(weight);
        }
    }
    let sum = sum.min(t.most);
    c.total = sum;
    if sum < t.crippled {
        c.equipment &= !usg::Robot::CRIPPLED;
    } else if c.equipment & usg::Robot::CRIPPLED == 0 {
        c.equipment |= usg::Robot::CRIPPLED;
        crate::script::post(m, EVENT_CRIPPLED);
    }
    if sum >= t.destroyed[usize::from(crate::world::side_code(e.side))] {
        destroy(world, id, tables, events);
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
    let (side, at, z) = (e.side, e.position, e.z);
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
    spawn_explosion(world, at, z, d.explosion_kind, d.robot_explosion);
    world.units[usize::from(crate::world::side_code(side))].retain(|&u| u != id);
    world.orders.remove(id.0);
    world.remove(id);
    events.push(Event::Sound {
        event: SOUND_DESTROYED,
        only_for: None,
    });
    events.push(Event::Destroyed(id));
}

/// An explosion of `kind` at `at`, height `z` (the object it replaces), from `template`
/// (`sim_spawn_explosion`), in the lowest free slot; none if the slots or the small
/// objects' pool are full.
// shortcut: a freed slot is reused at once; the original waits for the robots that
// target the object to let go of it (T-112).
pub(crate) fn spawn_explosion(
    world: &mut World,
    at: Position,
    z: Fixed,
    kind: u16,
    template: Explosion,
) -> Option<EntityId> {
    let slot = alloc_small(world)?;
    let explosion = Entity {
        kind: kind.into(),
        side: Side::Human,
        position: at,
        z,
        model_height: 0,
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
    world.insert(slot, explosion);
    Some(slot)
}

/// `sim_alloc_object_slot` (`1000:f4b1`) for an object of the small pool: the lowest
/// free slot, none if the slots or the pool of 200 are full.
pub(crate) fn alloc_small(world: &World) -> Option<EntityId> {
    let small = world
        .iter()
        .filter(|(_, e)| !matches!(e.kind, ObjectKind::Base) && RobotKind::of(e.kind).is_none())
        .count();
    let slot = (0..SLOTS).find(|&s| world.get(EntityId(s)).is_none())?;
    (small < SMALL_POOL).then_some(EntityId(slot))
}

/// One step of destroyed base `id`'s end script (`sim_end_mission_with_result`, run
/// every 16 ticks): explosions on it, and at the last step the mission is over, won
/// if the base is the alien one (`docs/sim.md`, "The mission's end").
pub fn step_base_end(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let (d, end) = (&tables.damage, &tables.damage.base_end);
    let Some(e) = world.get(id) else {
        return;
    };
    let (at, z, won) = (e.position, e.z, e.side == Side::Alien);
    let State::Base { end_step, .. } = e.state else {
        return;
    };
    // `snd_play_base_end_step_sound`'s table at `DS:0x7342` (`docs/sim.md`), and the
    // big explosion's sound at step 20.
    // shortcut: the table's events are written here, not read from `xeno.exe`; read
    // them if another build turns up.
    let sound = match end_step {
        0 | 5 | 12 | 17 => Some(3),
        3 | 8 | 15 => Some(4),
        10 | 16 => Some(5),
        20 => Some(SOUND_DESTROYED),
        _ => None,
    };
    if let Some(event) = sound {
        events.push(Event::Sound {
            event,
            only_for: None,
        });
    }
    // `sim_step_base_end_script`'s jump table by `step / 4`.
    let (template, radius, next) = match end_step >> 2 & 0xf {
        n @ 0..=4 => (Some(n.min(3)), end.radii[0], end_step + 1),
        5 => (Some(4), end.radii[1], end_step + 4),
        6..=8 => (None, 0, end_step + 1),
        // shortcut: past its ninth entry the original's table runs into code; only a
        // step of 0x24 or more from a file or a resumed mission gets there.
        _ => (None, 0, end_step),
    };
    if let Some(t) = template {
        let explosion = spawn_explosion(
            world,
            at,
            z,
            d.explosion_kind,
            end.explosions[usize::from(t)],
        );
        // shortcut: when no explosion could be made the original moves whatever its
        // allocator left in `SI`; the port moves nothing (the 200-object pool is full).
        let mut scatter = || i32::from((random.word() & 0x3fff) as i16 - 0x2000);
        let (dx, dy) = if t < 4 {
            (scatter(), scatter())
        } else {
            (0, 0)
        };
        // Then lowered by a random amount (`sim_lower_explosion_randomly`).
        let drop = i32::from(random.word() & 0x7ff);
        if let Some(x) = explosion.and_then(|x| world.get_mut(x)) {
            x.position.x.0 = x.position.x.0.wrapping_add(dx);
            x.position.y.0 = x.position.y.0.wrapping_add(dy);
            x.z.0 = x.z.0.wrapping_sub(drop);
            x.radius = radius;
        }
    }
    if let Some(State::Base { end_step, look, .. }) = world.get_mut(id).map(|e| &mut e.state) {
        *end_step = next;
        // Its damaged look (+0x18, `docs/render.md`).
        if next == 4 || next == 14 {
            *look = look.wrapping_add(1);
        }
        if next >= end.last_step.into() {
            events.push(Event::MissionOver { won });
        }
    }
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
