//! The bases' tick (`sim_tick_base`, `1000:6300`): +0x41 counts the ticks and its low
//! four bits pick the phase that runs (`docs/sim.md`, "The bases' phases"). The
//! numbers come from the user's `xeno.exe` (`xeno_formats::base`).

use xeno_formats::base::{COUNT_PHASE, TAKE_IN_PHASE};
use xeno_formats::usg::{ObjectKind, Robot as Bits, Side};

use crate::motion::{Event, Tables};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::terrain::approx_distance;
use crate::world::{Damage, EntityId, Fixed, State, World};

/// The paths' phase (`sim_run_alien_base_path_phase`) and the end's
/// (`sim_end_mission_with_result`).
const PATH_PHASE: u8 = 2;
const END_PHASE: u8 = 15;

/// Base `id` counts its tick and runs that tick's phase.
pub(crate) fn tick(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some(e) = world.get_mut(id) else {
        return;
    };
    let State::Base {
        tick, destroyed, ..
    } = &mut e.state
    else {
        return;
    };
    *tick = tick.wrapping_add(1);
    let (tick, destroyed, side) = (*tick, *destroyed, e.side);
    let b = &tables.bases;
    match tick & 0xf {
        PATH_PHASE if side == Side::Alien => {
            crate::motion::alien_path_phase(world, tables, random, events)
        }
        COUNT_PHASE if tick & b.count_mask == 0 => count(world, id),
        TAKE_IN_PHASE if tick & b.take_in_mask == 0 => take_in(world, id, b.reach, events),
        END_PHASE if destroyed => crate::damage::step_base_end(world, id, tables, random, events),
        _ => {}
    }
}

/// Whether robot damage `d` says the robot is in its base (+0xfd bit 2).
pub fn in_base(d: &Damage) -> bool {
    matches!(d, Damage::Robot(c) if c.equipment & Bits::IN_BASE != 0)
}

/// `1000:6344`: the side's relays (deployed ones are all on the human side), then its
/// humanoids, scorpions and scouts, each in the base and in the field, into +0x21 to
/// +0x27 for the situation room.
fn count(world: &mut World, id: EntityId) {
    let Some(side) = world.get(id).map(|e| e.side) else {
        return;
    };
    let mut counts = [0u8; 7];
    for (_, e) in world.iter().filter(|(_, e)| e.side == side) {
        let at = match e.kind {
            ObjectKind::Relay => 0,
            ObjectKind::Humanoid => 1,
            ObjectKind::Scorpion => 3,
            ObjectKind::Scout => 5,
            _ => continue,
        };
        let at = at + usize::from(at != 0 && !in_base(&e.damage));
        counts[at] = counts[at].wrapping_add(1);
    }
    if let Some(State::Base { counts: c, .. }) = world.get_mut(id).map(|e| &mut e.state) {
        *c = counts;
    }
}

/// `1000:63dd`: the first of the side's robots (in slot order) within `reach` of its
/// base is taken in, and the scan stops there; robots nearer than 256 units but out of
/// reach may be taken in again (+0xfd bit 1 cleared).
fn take_in(world: &mut World, id: EntityId, reach: u16, events: &mut Vec<Event>) {
    let Some((side, at)) = world.get(id).map(|e| (e.side, e.position)) else {
        return;
    };
    let robots: Vec<EntityId> = world
        .iter()
        .filter(|(_, e)| e.side == side && RobotKind::of(e.kind).is_some())
        .map(|(r, _)| r)
        .collect();
    for r in robots {
        let Some(e) = world.get_mut(r) else {
            continue;
        };
        // The distance as 8.8, under 256 units only (`1000:08ec`, then `DH` tested).
        let d = approx_distance(at, e.position).0 as u32;
        if d >> 24 != 0 {
            continue;
        }
        if (d >> 8) as u16 > reach {
            if let Damage::Robot(c) = &mut e.damage {
                c.equipment &= !Bits::ENTERING_BASE;
            }
            continue;
        }
        enter(world, r, events);
        return;
    }
}

/// `1000:6445`: unless it was just taken in or launched (+0xfd bit 1), robot `r` goes
/// in: bits 1 and 2 set, out of the field (its position's top bits flipped, as far as
/// the world goes), asked to stop and handed to its pilot (`order_switch_to_manual`).
fn enter(world: &mut World, r: EntityId, events: &mut Vec<Event>) {
    let Some(e) = world.get_mut(r) else {
        return;
    };
    let Damage::Robot(c) = &mut e.damage else {
        return;
    };
    if c.equipment & Bits::ENTERING_BASE != 0 {
        return;
    }
    c.equipment |= Bits::ENTERING_BASE | Bits::IN_BASE;
    let flip = |f: &mut Fixed| f.0 ^= i32::MIN;
    flip(&mut e.position.x);
    flip(&mut e.position.y);
    if let State::Robot(m) = &mut e.state {
        m.target_speed = 0;
    }
    crate::script::switch(world, r, false, events);
    events.push(Event::TakenIn(r));
}

#[cfg(test)]
mod tests;
