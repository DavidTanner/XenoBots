//! The robots' sensing, run from each kind's tick table after the move (`docs/sim.md`,
//! "Sensing"): the scan that spots enemy units and counts them. The ranges and where
//! each kind's tick table runs it are read from the user's `xeno.exe`
//! (`xeno_formats::sensing`, in [`Tables::sensing`]).

use xeno_formats::usg::status;

use crate::motion::{Tables, movement, robot};
use crate::orders::RobotKind;
use crate::script::post;
use crate::terrain::approx_distance;
use crate::world::{EntityId, Position, World};

/// Events the scan posts: enemy units found where there were none, and none left.
pub const EVENT_CONTACTS: u8 = 1;
pub const EVENT_CLEAR: u8 = 2;

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
    if (m.tick & 0x1e) >> 1 == ticks.scan {
        scan(world, id, tables.sensing.scan_range[k.index()]);
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

#[cfg(test)]
mod tests;
