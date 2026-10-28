//! The robots' sensing (`docs/sim.md`, "Sensing"): the scan that spots enemy units
//! and counts them, the searches for the nearest enemy, the target lock, its upkeep
//! and the warning it gives the target. The scan, the upkeep and the warning run from
//! each kind's tick table after the move; the scripts look and lock. The ranges, the
//! tick-table entries and the lock's messages are read from the user's `xeno.exe`
//! (`xeno_formats::sensing`, in [`Tables::sensing`]).

use xeno_formats::usg::{ObjectKind, status};

use crate::motion::{Event, Tables, movement, robot};
use crate::orders::RobotKind;
use crate::script::{bearing, post};
use crate::terrain::approx_distance;
use crate::world::{Components, Damage, Entity, EntityId, Position, State, World};

/// Events the scan posts: enemy units found where there were none, and none left.
pub const EVENT_CONTACTS: u8 = 1;
pub const EVENT_CLEAR: u8 = 2;
/// Another robot's lock on this one starts (the warning), and this one's lock is lost.
pub const EVENT_LOCKED_ON: u8 = 3;
pub const EVENT_LOST: u8 = 4;
/// +0xec: the targeting computer damaged, the target within the upkeep's range, and
/// within its arc (or no target).
const TARGETING_DAMAGED: u8 = 0x01;
const IN_RANGE: u8 = 0x02;
const IN_ARC: u8 = 0x04;
/// +0xf6 bit 5: a lock warning.
const WARNING: u8 = 0x20;

/// +0x16 bit 2 (`SCANNED`, robots and bases) and bit 1 (`TARGETABLE`, relays too), by
/// kind: every stock object has them so, and the port keeps no flags byte.
fn scanned(kind: ObjectKind) -> bool {
    RobotKind::of(kind).is_some() || kind == ObjectKind::Base
}

fn targetable(kind: ObjectKind) -> bool {
    scanned(kind) || kind == ObjectKind::Relay
}

/// +0x17: only robots keep it.
fn status_of(e: &Entity) -> u8 {
    match e.state {
        State::Robot(m) => m.status,
        _ => 0,
    }
}

fn parts(world: &mut World, id: EntityId) -> Option<&mut Components> {
    match &mut world.get_mut(id)?.damage {
        Damage::Robot(c) => Some(c),
        _ => None,
    }
}

fn say(world: &mut World, id: EntityId, text: u16) {
    if let Some(m) = movement(world, id) {
        m.panel.post(text);
    }
}

/// The true distance (`1000:0942`), 16.16.
// shortcut: the square root of x and y's squares, exact; the original's also takes z
// and rounds its own way. Upgrade when the sim keeps heights (T-101) or a DOSBox
// comparison shows a difference.
pub(crate) fn true_distance(a: Position, b: Position) -> u32 {
    let dx = i128::from(b.x.0) - i128::from(a.x.0);
    let dy = i128::from(b.y.0) - i128::from(a.y.0);
    u32::try_from((dx * dx + dy * dy).isqrt()).unwrap_or(u32::MAX)
}

/// The true distance as the lock keeps it: 8.8, the top byte dropped.
fn lock_distance(a: Position, b: Position) -> u16 {
    (true_distance(a, b) >> 8) as u16
}

/// Whether `b` is within `range` (8.8) of `a` by the original's distance
/// (`1000:08ec`): under 256 units, and its middle word at most `range`.
pub(crate) fn within(a: Position, b: Position, range: u16) -> bool {
    let d = approx_distance(a, b).0 as u32;
    d >> 24 == 0 && (d >> 8) as u16 <= range
}

/// Robot `id`'s tick-table entry for this tick, after its move stepped +0x1a: the
/// entry `(tick & 0x1e) >> 1` of its kind's table.
pub fn step(world: &mut World, id: EntityId, tables: &Tables) {
    let Some((kind, _, m)) = robot(world, id) else {
        return;
    };
    let Some(k) = RobotKind::of(kind) else {
        return;
    };
    let ticks = tables.sensing.ticks[k.index()];
    let entry = (m.tick & 0x1e) >> 1;
    if entry == ticks.scan {
        scan(world, id, tables.sensing.scan_range[k.index()]);
    } else if entry == ticks.lock {
        keep_lock(world, id, (ticks.arc, ticks.range), tables);
    } else if entry == ticks.locked_on {
        warn(world, id);
    }
}

/// The scan (`1000:dcba`): only while +0x1a's top three bits are clear, +0x17's scan
/// count steps (bits 7, 5, 4, 3, lowest first); when it comes round to 0, every unit
/// of the other side within `range` is spotted, their number becomes the contacts,
/// and event 1 or 2 is posted when there were none, or none are left.
fn scan(world: &mut World, id: EntityId, range: u16) {
    let Some(at) = world.get(id).map(|e| (e.side, e.position)) else {
        return;
    };
    let Some(m) = movement(world, id) else {
        return;
    };
    if m.tick & 0xe0 != 0 {
        return;
    }
    for bit in [0x80, 0x20, 0x10, 0x08] {
        m.status ^= bit;
        if m.status & bit != 0 {
            return;
        }
    }
    // The units: the robots (+0x16 bit 5, set on every robot and nothing else).
    let found: Vec<EntityId> = world
        .iter()
        .filter(|(_, e)| {
            RobotKind::of(e.kind).is_some() && e.side != at.0 && within(at.1, e.position, range)
        })
        .map(|(id, _)| id)
        .collect();
    for &f in &found {
        if let Some(m) = movement(world, f) {
            m.status |= status::SPOTTED;
        }
    }
    let Some(m) = movement(world, id) else {
        return;
    };
    let count = found.len() as u8;
    let before = std::mem::replace(&mut m.contacts, count);
    if count != before && before == 0 {
        post(m, EVENT_CONTACTS);
    } else if count != before && count == 0 {
        post(m, EVENT_CLEAR);
    }
}

/// The nearest object the scanners list (robots and bases) of the other side within
/// `range` of robot `id`, strictly nearer winning in slot order (`2000:8005`, and
/// `1000:d757` with `looking`, which skips the robot itself and cloaked objects).
fn nearest(world: &World, id: EntityId, range: u16, looking: bool) -> Option<EntityId> {
    let me = world.get(id)?;
    let mut best = (0xffff, None);
    for (o, e) in world.iter() {
        let skip = looking && (o == id || status_of(e) & status::CLOAKED != 0);
        if skip || !scanned(e.kind) || e.side == me.side {
            continue;
        }
        let d = approx_distance(me.position, e.position).0 as u32;
        let mid = (d >> 8) as u16;
        if d >> 24 == 0 && mid <= range && mid < best.0 {
            best = (mid, Some(o));
        }
    }
    best.1
}

/// Opcode 0x63 (`1000:d165`): the target heading becomes the bearing to the nearest
/// enemy within the scanning range, not locked. Returns false with none.
pub fn steer_to_nearest(world: &mut World, id: EntityId, tables: &Tables) -> bool {
    let Some((kind, _, _)) = robot(world, id) else {
        return false;
    };
    let Some(k) = RobotKind::of(kind) else {
        return false;
    };
    let found = nearest(world, id, tables.sensing.scan_range[k.index()], false);
    let (Some(to), Some(from)) = (found.and_then(|o| world.get(o)), world.get(id)) else {
        return false;
    };
    let heading = bearing(&tables.atan, from.position, to.position);
    if let Some(m) = movement(world, id) {
        m.target_heading = heading;
    }
    true
}

/// Opcode 0x71 (`1000:d757`): the nearest enemy within the locking range is locked.
/// Returns false, with a message, when there is none (a refused lock still counts).
pub fn look(world: &mut World, id: EntityId, tables: &Tables, events: &mut Vec<Event>) -> bool {
    let Some((kind, _, _)) = robot(world, id) else {
        return false;
    };
    let Some(k) = RobotKind::of(kind) else {
        return false;
    };
    match nearest(world, id, tables.sensing.lock_range[k.index()], true) {
        Some(target) => {
            lock(world, id, target, tables, events);
            true
        }
        None => {
            say(world, id, tables.sensing.messages.none_found);
            false
        }
    }
}

/// Robot `id` locks `target` (`sim_lock_target`, `1000:d8af`), unless its targeting
/// computer is damaged, the target is itself, its target already, not targetable,
/// cloaked or out of range; a target of its own side is locked with a warning.
pub fn lock(
    world: &mut World,
    id: EntityId,
    target: EntityId,
    tables: &Tables,
    events: &mut Vec<Event>,
) {
    let text = tables.sensing.messages;
    let Some((kind, side, m)) = robot(world, id) else {
        return;
    };
    let Some(k) = RobotKind::of(kind) else {
        return;
    };
    if parts(world, id).is_some_and(|c| c.targeting & TARGETING_DAMAGED != 0) {
        return say(world, id, text.targeting_damaged);
    }
    if let Some(m) = movement(world, id) {
        m.status |= status::TRACKING;
    }
    if target == id || m.target == Some(target) {
        let text = if target == id {
            text.itself
        } else {
            text.already
        };
        return say(world, id, text);
    }
    let (Some(t), Some(me)) = (world.get(target), world.get(id)) else {
        return;
    };
    if !targetable(t.kind) || status_of(t) & status::CLOAKED != 0 {
        return;
    }
    if lock_distance(me.position, t.position) >= tables.sensing.lock_range[k.index()] {
        return say(world, id, text.out_of_range);
    }
    if t.side == side {
        say(world, id, text.friendly);
    }
    update(world, id, target, world.lock_window, tables);
    // shortcut: only robots keep +0x17, so a locked base is not marked spotted; only
    // the map reads it (T-103).
    if let Some(t) = movement(world, target) {
        t.status |= status::SPOTTED;
    }
    let Some(m) = movement(world, id) else {
        return;
    };
    let before = m.target.replace(target);
    m.panel.post(if before.is_none() {
        text.locked
    } else {
        text.relocked
    });
    events.push(Event::Sound {
        event: tables.sensing.lock_sound,
        only_for: Some(id),
    });
}

/// The target's bearing and distance and the targeting bits (`1000:d6e6`): bit 2
/// within `arc` of the heading either way, bit 1 under `range`.
fn update(
    world: &mut World,
    id: EntityId,
    target: EntityId,
    (arc, range): (u16, u16),
    tables: &Tables,
) {
    let (Some(me), Some(t)) = (world.get(id), world.get(target)) else {
        return;
    };
    // shortcut: the coarse bearing; the original's upkeep interpolates the arctangent
    // (`1000:0545`), at most a few 65,536ths of a turn finer. Upgrade with the shots'
    // aim (T-114).
    let b = bearing(&tables.atan, me.position, t.position);
    let off = b.wrapping_sub(me.heading);
    let distance = lock_distance(me.position, t.position);
    let mut bits = 0;
    if off < arc || off >= arc.wrapping_neg() {
        bits |= IN_ARC;
    }
    if distance < range {
        bits |= IN_RANGE;
    }
    if let Some(c) = parts(world, id) {
        c.targeting = (c.targeting & !(IN_ARC | IN_RANGE)) | bits;
    }
    if let Some(m) = movement(world, id) {
        (m.target_bearing, m.target_distance) = (b, distance);
    }
}

/// The lock's upkeep (`1000:d66f`) with the kind's arc and range: a destroyed target
/// is dropped; with the targeting computer damaged or the target past the locking
/// range the lock is lost (event 4); otherwise a robot target is warned and the
/// bearing, distance and bits are worked out.
fn keep_lock(world: &mut World, id: EntityId, window: (u16, u16), tables: &Tables) {
    world.lock_window = window;
    let text = tables.sensing.messages;
    let Some((kind, _, m)) = robot(world, id) else {
        return;
    };
    let Some(target) = m.target else {
        if let Some(c) = parts(world, id) {
            c.targeting |= IN_ARC;
        }
        return;
    };
    // shortcut: the port frees a destroyed robot's slot at once (T-054), so an empty
    // slot, or one another kind took, is the target gone; the original keeps the slot
    // while robots reference it.
    let gone = world.get(target).is_none_or(|t| {
        !targetable(t.kind)
            || matches!(
                t.state,
                State::Base {
                    destroyed: true,
                    ..
                }
            )
    });
    let range = RobotKind::of(kind).map_or(0, |k| tables.sensing.lock_range[k.index()]);
    let damaged = parts(world, id).is_some_and(|c| c.targeting & TARGETING_DAMAGED != 0);
    // shortcut: a robot target's ECM (+0xdf running) also breaks the lock; the port has
    // no ECM until T-136.
    let lost = damaged || m.target_distance > range;
    if gone || lost {
        if let Some(m) = movement(world, id) {
            m.target = None;
            m.panel.post(if gone { text.destroyed } else { text.lost });
            if !gone {
                post(m, EVENT_LOST);
            }
        }
        return;
    }
    if let Some(t) = movement(world, target) {
        t.locked_on = 0xff;
    }
    update(world, id, target, window, tables);
}

/// The lock warning (`1000:dc95`): `locked_on` halves; while it is not 0 `systems`
/// bit 5 is set, posting event 3 as it is set; at 0 it is cleared.
fn warn(world: &mut World, id: EntityId) {
    let Some(m) = movement(world, id) else {
        return;
    };
    m.locked_on >>= 1;
    let on = m.locked_on != 0;
    let Some(c) = parts(world, id) else {
        return;
    };
    let was = c.systems & WARNING != 0;
    if on {
        c.systems |= WARNING;
    } else {
        c.systems &= !WARNING;
    }
    if on
        && !was
        && let Some(m) = movement(world, id)
    {
        post(m, EVENT_LOCKED_ON);
    }
}

#[cfg(test)]
mod tests;
