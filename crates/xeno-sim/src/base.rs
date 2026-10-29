//! The bases' tick (`sim_tick_base`, `1000:6300`): +0x41 counts the ticks and its low
//! four bits pick the phase that runs (`docs/sim.md`, "The bases' phases"). The
//! numbers come from the user's `xeno.exe` (`xeno_formats::base`).

use xeno_formats::base::{
    Bases, COUNT_PHASE, LAUNCH_PHASE, RELAY_PHASE, REPAIR_PHASE, TAKE_IN_PHASE,
};
use xeno_formats::usg::{ObjectKind, Robot as Bits, Side};

use crate::motion::{Event, Tables};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::terrain::approx_distance;
use crate::world::{Components, Damage, Depot, EntityId, Fixed, Movement, State, World};

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
        RELAY_PHASE => {
            if let Some(d) = depot(world, id) {
                make_relays(d, b);
            }
        }
        REPAIR_PHASE => repair(world, id, b),
        LAUNCH_PHASE if side == Side::Alien => launch_whole(world, id, tables, events),
        END_PHASE if destroyed => crate::damage::step_base_end(world, id, tables, random, events),
        _ => {}
    }
    // The slider held (`DS:0x5b0a` by +0x2d), every tick.
    if let Some(d) = depot(world, id) {
        slide(d, b);
    }
}

fn depot(world: &mut World, id: EntityId) -> Option<&mut Depot> {
    match world.get_mut(id).map(|e| &mut e.state) {
        Some(State::Base { depot, .. }) => Some(depot),
        _ => None,
    }
}

/// `1000:648b`: the third slider runs a clock; each carry counts towards the next
/// relay made, while the store is under its most.
fn make_relays(d: &mut Depot, b: &Bases) {
    let carry;
    (d.relay_clock, carry) = d.relay_clock.overflowing_add(d.power[3]);
    if !carry {
        return;
    }
    d.relay_wait = d.relay_wait.wrapping_add(1);
    if d.relay_wait >= b.relay_wait && d.relays < b.relays_most {
        d.relays += 1;
        d.relay_wait = 0;
    }
}

/// Whether the robot's damage total (+0xf9) is not 0, which the repairs and the launch
/// go by.
// shortcut: the port keeps no damage total (T-140); any part the repair step mends
// not whole counts, so a robot worn by under 4 points of helm or power counts as
// damaged where the original's quartered total is already 0. Upgrade with T-140.
fn damaged(c: &Components, whole: u8) -> bool {
    c.helm < whole
        || c.power < whole
        || c.weapons & 0x07 != 0
        || c.targeting & 1 != 0
        || c.systems & 0xdf != 0
        || c.equipment & 0x80 != 0
}

/// `1000:64af`: the second slider runs a clock; at the period one repair step is made
/// on the robot in repair, or, with none, the first damaged robot in the base in the
/// side's unit list is picked for the next.
fn repair(world: &mut World, id: EntityId, b: &Bases) {
    let Some(d) = depot(world, id) else {
        return;
    };
    d.repair_clock = d.repair_clock.wrapping_add(u16::from(d.power[2]));
    if d.repair_clock < b.repair_period {
        return;
    }
    d.repair_clock = 0;
    let repairing = d.repairing;
    // `1000:f691`: an empty slot or one that is no robot is none.
    let robot =
        repairing.filter(|&r| matches!(world.get(r).map(|e| &e.damage), Some(Damage::Robot(_))));
    let next = match robot {
        Some(r) => {
            repair_step(world, id, r, b);
            match world.get(r).map(|e| e.damage) {
                Some(Damage::Robot(c)) if damaged(&c, b.whole) => Some(r),
                _ => None,
            }
        }
        None => {
            let side = world.get(id).map(|e| e.side);
            let units = side.map_or(&[][..], |s| world.units(s));
            units.iter().copied().find(|&u| {
                world.get(u).is_some_and(|e| {
                    in_base(&e.damage)
                        && matches!(e.damage, Damage::Robot(c) if damaged(&c, b.whole))
                })
            })
        }
    };
    if let Some(d) = depot(world, id) {
        d.repairing = next;
    }
}

/// `2000:7e6d`: one repair step on robot `r` mends the first of: the helm, the power,
/// the first and second weapons (and their destroyed bits), +0xdc bit 2, the targeting
/// computer, the cameras, map, radar and sensors; with none, the kind's restock
/// (`1000:df87`): a scorpion takes relays from base `id`'s store up to its load, gets
/// its missiles and its deployer mended.
fn repair_step(world: &mut World, id: EntityId, r: EntityId, b: &Bases) {
    let store = depot(world, id).map_or(0, |d| d.relays);
    let Some(e) = world.get_mut(r) else {
        return;
    };
    let kind = e.kind;
    let Damage::Robot(c) = &mut e.damage else {
        return;
    };
    // `add byte [si + n], step`, `and byte [si + n], 0xfc` (the mask checked by the
    // reader).
    let mend = |v: &mut u8| *v = v.wrapping_add(b.repair_step) & 0xfc;
    if c.helm != b.whole {
        return mend(&mut c.helm);
    }
    if c.power != b.whole {
        return mend(&mut c.power);
    }
    for (bit, destroyed) in [(1, 0x40), (2, 0x80), (4, 0)] {
        if c.weapons & bit != 0 {
            c.weapons &= !bit;
            c.systems &= !destroyed;
            return;
        }
    }
    if c.targeting & 1 != 0 {
        c.targeting &= !1;
        return;
    }
    if let Some(bit) = (0..5).map(|n| 1 << n).find(|&bit| c.systems & bit != 0) {
        c.systems &= !bit;
        return;
    }
    if kind != ObjectKind::Scorpion {
        return;
    }
    // `1000:9b97` and `2000:699f`.
    c.weapons &= !0x08;
    c.equipment &= !0x80;
    let State::Robot(m) = &mut e.state else {
        return;
    };
    m.guns.missiles = b.restock.1;
    let take = b.restock.0.wrapping_sub(m.relays).min(store);
    m.relays = m.relays.wrapping_add(take);
    if let Some(d) = depot(world, id) {
        d.relays -= take;
    }
}

/// `1000:6515`: the alien base launches every robot in its unit list that is in the
/// base and whole.
fn launch_whole(world: &mut World, id: EntityId, tables: &Tables, events: &mut Vec<Event>) {
    // The base's own unit list (+0x4d).
    let Some(side) = world.get(id).map(|e| e.side) else {
        return;
    };
    let units = world.units(side).to_vec();
    for r in units {
        if world.get(r).is_some_and(|e| {
            in_base(&e.damage)
                && matches!(e.damage, Damage::Robot(c) if !damaged(&c, tables.bases.whole))
        }) {
            launch(world, id, r, tables, events);
        }
    }
}

/// `1000:6716`: robot `r` leaves base `id` at the base's position, back in the field,
/// off its path and with no contacts, and with a new robot's orders (`1000:d2b6`).
/// +0xfd bit 1 stays, so it is not taken in again before it has left the reach.
pub fn launch(
    world: &mut World,
    id: EntityId,
    r: EntityId,
    tables: &Tables,
    events: &mut Vec<Event>,
) {
    let Some(at) = world.get(id).map(|e| e.position) else {
        return;
    };
    let Some(e) = world.get_mut(r) else {
        return;
    };
    let (State::Robot(m), Damage::Robot(c)) = (&mut e.state, &mut e.damage) else {
        return;
    };
    e.position = at;
    c.equipment &= !Bits::IN_BASE;
    // `mov byte [di + 0x24], 100`, checked by the reader.
    c.helm = 100;
    m.path &= !Movement::ON_PATH;
    m.contacts = 0;
    (m.mode, m.speed) = (0, 0);
    crate::script::set_program(world, r, tables.bases.launch_program, tables, events);
    // The program bit set directly, with no event.
    if let Some(change) = world.orders.set_program_drives(r.0, true) {
        events.push(Event::Program(change));
    }
    events.push(Event::Launched(r));
}

/// The slider held moves (`DS:0x5b0a`): odd ones lower a slider into the pool, even
/// ones raise it, taking from the pool or the other two (`1000:66c5`).
fn slide(d: &mut Depot, b: &Bases) {
    if !(1..=6).contains(&d.slider) {
        return;
    }
    let i = usize::from(d.slider + 1) / 2;
    let p = &mut d.power;
    let (least, step) = (b.slider_least, b.slider_step);
    if d.slider % 2 == 1 {
        if p[i] >= least {
            p[i] -= step;
            p[0] = p[0].wrapping_add(step);
        }
        return;
    }
    if p[i] > b.slider_most {
        return;
    }
    p[i] = p[i].wrapping_add(step);
    let [x, y] = match i {
        1 => [2, 3],
        2 => [1, 3],
        _ => [1, 2],
    };
    if p[0] >= least {
        p[0] -= step;
    } else if p[x] >= least && p[y] >= least {
        p[x] -= 1;
        p[y] -= 1;
    } else if p[x] >= least {
        p[x] -= step;
    } else if p[y] >= least {
        p[y] -= step;
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
