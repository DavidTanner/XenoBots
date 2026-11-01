//! The robots' weapons (`docs/sim.md`, "Weapons and shots"): the trigger (Space and
//! the scripts' opcode 0x7d), each kind's weapon step from its tick table, the
//! humanoid's arms that raise a weapon before it fires, and the weapon state's
//! cooling and empty bits. The guns' numbers and messages come from the user's
//! `xeno.exe` (`xeno_formats::weapons`, in [`Tables::weapons`]); the shots fly in
//! [`crate::shot`].

use xeno_formats::motion::Walk;
use xeno_formats::usg::{ObjectKind, status};
use xeno_formats::weapons::Gun;

use crate::motion::{Event, Tables};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::world::{Components, Damage, Entity, EntityId, Movement, State, World};

/// +0xdc: the fire request, and the humanoid's weapons ready (bit 5 the first).
const REQUEST: u8 = 0x80;
const READY: u8 = 0x20;
/// +0xdc bit 6 on a scorpion: the barrel its cannon fires next.
const BARREL: u8 = 0x40;
/// +0xdc bit 2: the scorpion's tail missile damaged.
const TAIL_DAMAGED: u8 = 0x04;
/// +0xec bit 2: the target in the arc (or no target).
const IN_ARC: u8 = 0x04;
/// +0xfd: on the battery, and a weapon's message shown.
const ON_BATTERY: u8 = 0x01;
const TOLD: u8 = 0x20;
/// +0xf6 bits 6 and 7: the first and second weapon destroyed.
const DESTROYED: u8 = 0x40;
/// The humanoid's arm's frame when raised, and when its weapon is destroyed.
const RAISED: u8 = 5;
const BROKEN: u8 = 6;

/// The guns by kind: [`xeno_formats::weapons::Weapons::guns`]' pairs.
fn gun(tables: &Tables, k: RobotKind, w: usize) -> Gun {
    tables.weapons.guns[2 * k.index() + w]
}

fn robot_parts(world: &mut World, id: EntityId) -> Option<(&mut Movement, &mut Components)> {
    let e = world.get_mut(id)?;
    match (&mut e.state, &mut e.damage) {
        (State::Robot(m), Damage::Robot(c)) => Some((m, c)),
        _ => None,
    }
}

/// Space and opcode 0x7d (`1000:d0da`): the kind's trigger sets the fire request; the
/// scorpion's also queues its burst.
pub fn trigger(world: &mut World, id: EntityId, tables: &Tables) {
    let Some(kind) = world.get(id).map(|e| e.kind) else {
        return;
    };
    if let Some((m, c)) = robot_parts(world, id) {
        c.weapons |= REQUEST;
        if kind == ObjectKind::Scorpion {
            m.guns.burst = tables.weapons.burst;
        }
    }
}

/// A weapon's message to the robot's panel, once until a shot is fired (+0xfd bit 5).
fn tell(m: &mut Movement, c: &mut Components, text: u16) {
    if c.equipment & TOLD == 0 {
        m.panel.post(text);
        c.equipment |= TOLD;
    }
}

/// The energy check (`1000:d1b0`): on the battery `amount` comes off the energy; short,
/// the energy goes to 0 and the check fails.
fn energy(m: &mut Movement, c: &Components, amount: u8) -> bool {
    if c.equipment & ON_BATTERY == 0 {
        return true;
    }
    match m.guns.energy.checked_sub(amount) {
        Some(left) => {
            m.guns.energy = left;
            true
        }
        None => {
            m.guns.energy = 0;
            false
        }
    }
}

/// F7 (`1000:d92e`): with a charge left and the energy, one fewer, the ECM on for
/// [`Devices::ecm_time`](xeno_formats::weapons::Devices) steps (locks on the robot are
/// lost meanwhile, `crate::sense`); a message either way. No kind or damage is checked.
pub fn ecm(world: &mut World, id: EntityId, tables: &Tables) {
    let d = tables.weapons.devices;
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    let text = if m.guns.ecm == 0 {
        d.no_ecm
    } else if !energy(m, c, d.ecm_energy) {
        d.no_power
    } else {
        m.guns.ecm -= 1;
        m.guns.ecm_time = d.ecm_time;
        m.status |= status::ECM;
        d.ecm_on
    };
    crate::log::say(world, id, text, tables);
}

/// F8 (`1000:d95e`): with a cloak left, one fewer and the energy taken (short or not:
/// the check's answer is ignored), cloaked for [`Devices::cloak_time`] steps, and the
/// sparkle made where the robot stands if a slot is free; a message either way.
///
/// [`Devices::cloak_time`]: xeno_formats::weapons::Devices
pub fn cloak(world: &mut World, id: EntityId, tables: &Tables) {
    let d = tables.weapons.devices;
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    if m.guns.cloaks == 0 {
        crate::log::say(world, id, d.no_cloak, tables);
        return;
    }
    energy(m, c, d.cloak_energy);
    m.guns.cloaks -= 1;
    m.guns.cloak_time = d.cloak_time;
    m.status |= status::CLOAKED;
    if let (Some(slot), Some(e)) = (crate::damage::alloc_small(world), world.get(id)) {
        // `1000:dfff`: the robot's x, y and z, the first heading and radius.
        let sparkle = Entity {
            kind: d.sparkle.into(),
            side: e.side,
            position: e.position,
            z: e.z,
            model_height: 0,
            heading: d.heading,
            radius: d.radius,
            damage: Damage::None,
            state: State::Sparkle {
                frame: 0,
                ticks: 0,
                age: 0,
                stage: 0,
            },
        };
        world.insert(slot, sparkle);
    }
    crate::log::say(world, id, d.cloak_on, tables);
}

/// The sparkle's tick (`1000:e03b`): every [`Devices::frame_ticks`]` + 1` ticks the next
/// frame and one more of its age, its stage moving on at `grown` and `shrinking`; then
/// the stage's step (grow, nothing, shrink) on its heading and radius. Returns whether
/// it is gone (its age at `life`).
///
/// [`Devices::frame_ticks`]: xeno_formats::weapons::Devices
pub fn tick_sparkle(e: &mut Entity, tables: &Tables) -> bool {
    let d = tables.weapons.devices;
    let State::Sparkle {
        frame,
        ticks,
        age,
        stage,
    } = &mut e.state
    else {
        return false;
    };
    *ticks = ticks.wrapping_add(1);
    if *ticks & d.frame_ticks != 0 {
        return false;
    }
    *frame = frame.wrapping_add(1) & d.frames;
    *age = age.wrapping_add(1);
    if *age == d.grown || *age == d.shrinking {
        *stage += 1;
    }
    let step = u16::from(d.heading_step);
    match *stage {
        0 => {
            e.heading = e.heading.wrapping_add(step);
            e.radius = e.radius.wrapping_add(d.radius_step);
        }
        2 => {
            e.heading = e.heading.wrapping_sub(step);
            e.radius = e.radius.wrapping_sub(d.radius_step);
        }
        _ => {}
    }
    *age >= d.life
}

fn count_down(n: &mut u16) {
    *n = n.saturating_sub(1);
}

/// Gun `w` of robot `id`'s kind shoots: its shot, and its sound if a slot was free.
fn shoot(
    world: &mut World,
    id: EntityId,
    g: &Gun,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) -> Option<EntityId> {
    let slot = crate::shot::fire(world, id, g, tables, random);
    if slot.is_some() {
        events.push(Event::Sound {
            event: g.sound,
            only_for: (!g.heard_by_all).then_some(id),
        });
    }
    slot
}

/// Robot `id`'s weapon step, at its kind's entries of its tick table (`1000:b08a`,
/// `9713` and the reload's `9781`, `c8ab`).
pub fn step(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let w = &tables.weapons;
    let Some((kind, tick)) = world.get(id).and_then(|e| match e.state {
        State::Robot(m) => Some((e.kind, m.tick)),
        _ => None,
    }) else {
        return;
    };
    let Some(k) = RobotKind::of(kind) else {
        return;
    };
    let entry = (tick & 0x1e) >> 1;
    if kind == ObjectKind::Scorpion && entry == w.reload_step {
        if let Some((m, _)) = robot_parts(world, id) {
            count_down(&mut m.guns.cooldown[1]);
        }
        return;
    }
    if !w.steps[k.index()].contains(&entry) {
        return;
    }
    // A tail missile in flight that is gone is let go (`1000:973f`).
    let gone = robot_parts(world, id)
        .and_then(|(m, _)| m.guns.missile)
        .is_some_and(|s| !matches!(world.get(s).map(|e| e.state), Some(State::Shot(_))));
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    // Each step first counts the ECM down (and the cloak, but for the scorpion's),
    // clearing its status bit at 0.
    let g = &mut m.guns;
    if g.ecm_time != 0 {
        g.ecm_time -= 1;
        if g.ecm_time == 0 {
            m.status &= !status::ECM;
        }
    }
    if kind != ObjectKind::Scorpion && g.cloak_time != 0 {
        g.cloak_time -= 1;
        if g.cloak_time == 0 {
            m.status &= !status::CLOAKED;
        }
    }
    if kind == ObjectKind::Scorpion {
        if m.guns.burst != 0 {
            m.guns.burst -= 1;
            c.weapons |= REQUEST;
        }
        count_down(&mut m.guns.cooldown[0]);
        if gone {
            m.guns.missile = None;
        }
    } else {
        m.guns.cooldown.iter_mut().for_each(count_down);
    }
    let requested = c.weapons & REQUEST != 0;
    c.weapons &= !REQUEST;
    let weapon = usize::from(m.weapon & 1);
    match kind {
        ObjectKind::Humanoid => humanoid(
            world,
            id,
            requested.then_some(weapon),
            tables,
            random,
            events,
        ),
        _ if !requested => {}
        ObjectKind::Scorpion if weapon == 0 => cannon(world, id, tables, random, events),
        ObjectKind::Scorpion => tail(world, id, tables, random, events),
        _ => scout(world, id, weapon, tables, random, events),
    }
}

/// The humanoid's step after the cool-downs (`1000:b0c2` on): a request raises the
/// selected weapon's arm unless the weapon is damaged; each weapon its arm made ready,
/// cooled down and with the target in the arc fires.
fn humanoid(
    world: &mut World,
    id: EntityId,
    request: Option<usize>,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let text = tables.weapons.messages;
    if let Some(w) = request
        && let Some((m, c)) = robot_parts(world, id)
        && c.weapons & (1 << w) == 0
    {
        m.guns.raised[w] = 1;
    }
    for w in 0..2 {
        let Some((m, c)) = robot_parts(world, id) else {
            return;
        };
        if c.weapons & (READY << w) == 0 || m.guns.cooldown[w] != 0 || c.targeting & IN_ARC == 0 {
            continue;
        }
        c.weapons &= !(READY << w);
        if w == 0 && m.target.is_none() {
            tell(m, c, text.no_target);
            continue;
        }
        let g = gun(tables, RobotKind::Humanoid, w);
        if !energy(m, c, g.energy) {
            tell(m, c, text.no_power);
            continue;
        }
        let fired = shoot(world, id, &g, tables, random, events).is_some();
        if let Some((m, c)) = robot_parts(world, id) {
            if fired {
                m.guns.cooldown[w] = g.cooldown;
            }
            c.equipment &= !TOLD;
        }
    }
}

/// The scorpion's cannon (`1000:982d`): in the arc, with rounds and cooled down (else
/// nothing), and the energy; the barrels take turns (+0xdc bit 6), a damaged one
/// firing nothing; a shell, a round fewer and the cool-down even without a free slot.
fn cannon(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    if c.targeting & IN_ARC == 0 || m.guns.rounds == 0 || m.guns.cooldown[0] != 0 {
        return;
    }
    let g = gun(tables, RobotKind::Scorpion, 0);
    if !energy(m, c, g.energy) {
        return tell(m, c, tables.weapons.messages.no_power);
    }
    c.weapons ^= BARREL;
    let barrel = u8::from(c.weapons & BARREL != 0);
    if c.weapons & (1 << barrel) != 0 {
        return;
    }
    m.guns.fired |= 1 << barrel;
    shoot(world, id, &g, tables, random, events);
    if let Some((m, c)) = robot_parts(world, id) {
        m.guns.rounds -= 1;
        m.guns.cooldown[0] = g.cooldown;
        c.equipment &= !TOLD;
    }
}

/// The scorpion's tail missile (`1000:978d`): with missiles, reloaded, not damaged, a
/// target and the energy (each else a message), the last missile let go and a new one
/// made and kept (+0x123), "missile launched", a missile fewer and the reload.
fn tail(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let text = tables.weapons.messages;
    let g = gun(tables, RobotKind::Scorpion, 1);
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    let refused = if m.guns.missiles == 0 {
        Some(text.no_missiles)
    } else if m.guns.cooldown[1] != 0 {
        Some(text.reloading)
    } else if c.weapons & TAIL_DAMAGED != 0 {
        Some(text.damaged)
    } else if m.target.is_none() {
        Some(text.no_target)
    } else if !energy(m, c, g.energy) {
        Some(text.no_power)
    } else {
        None
    };
    if let Some(text) = refused {
        return tell(m, c, text);
    }
    m.guns.missile = None;
    let slot = shoot(world, id, &g, tables, random, events);
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    let Some(slot) = slot else {
        c.equipment &= !TOLD;
        return;
    };
    // The second display's switch to the missile camera (+0xef, +0xf0) is the
    // console's: `xeno-app` sees `missile` set (T-134).
    m.guns.missile = Some(slot);
    m.panel.post(text.launched);
    c.equipment |= TOLD;
    m.guns.missiles -= 1;
    m.guns.cooldown[1] = g.cooldown;
}

/// The scout's plasma gun (`1000:c8ff`, weapon 0) and cannon (`c990`).
fn scout(
    world: &mut World,
    id: EntityId,
    w: usize,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let text = tables.weapons.messages;
    let g = gun(tables, RobotKind::Scout, w);
    let Some((m, c)) = robot_parts(world, id) else {
        return;
    };
    let in_arc = c.targeting & IN_ARC != 0;
    let damaged = c.weapons & (1 << w) != 0;
    // Some(Some(text)): a message; Some(None): nothing, the message bit cleared.
    let cooled = m.guns.cooldown[w] == 0;
    let refused = if w == 0 {
        match m.target {
            None => Some(Some(text.no_target)),
            Some(_) if !in_arc || !cooled || damaged => Some(None),
            Some(_) => None,
        }
    } else if !in_arc {
        Some(None)
    } else if m.guns.rounds == 0 {
        Some(Some(text.no_shells))
    } else if !cooled {
        Some(None)
    } else if damaged {
        Some(Some(text.damaged))
    } else {
        None
    };
    let refused = refused.or_else(|| (!energy(m, c, g.energy)).then_some(Some(text.no_power)));
    match refused {
        Some(Some(text)) => return tell(m, c, text),
        Some(None) => {
            c.equipment &= !TOLD;
            return;
        }
        None => {}
    }
    let fired = shoot(world, id, &g, tables, random, events).is_some();
    if let Some((m, c)) = robot_parts(world, id) {
        if fired {
            if w == 1 {
                m.guns.rounds -= 1;
            }
            m.guns.cooldown[w] = g.cooldown;
        }
        c.equipment &= !TOLD;
    }
}

/// The humanoid's arms after its walking animation (`1000:ae23`, `ae8a`): a raised
/// arm steps every 4 passes, lifting to its raised frame, then making its weapon
/// ready, waiting for it to fire, and lowering; otherwise it swings while walking and
/// settles a frame a tick when still. A destroyed weapon's arm shows its broken frame.
/// The swing's step is the legs' (`counter >> 4`): `1000:ad80` sets BX, and neither the
/// legs' routine nor the footstep (which saves it) changes it before the arms (T-120).
pub fn arms(walk: &Walk, ticks: u16, m: &mut Movement, c: &mut Components) {
    let step = usize::from(m.stride >> 4);
    for w in 0..2 {
        let (arm, raised) = (&mut m.guns.arms[w], &mut m.guns.raised[w]);
        match *raised {
            0 if m.speed != 0 => *arm = walk.humanoid_arms[w][step],
            0 => *arm = arm.saturating_sub(1),
            _ if ticks & 3 != 0 => {}
            1 if *arm != RAISED => *arm += 1,
            1 => {
                *raised = 2;
                c.weapons |= READY << w;
            }
            2 if c.weapons & (READY << w) == 0 => *raised = 3,
            2 => {}
            _ if *arm != 0 => *arm -= 1,
            _ => *raised = 0,
        }
        if c.systems & (DESTROYED << w) != 0 {
            *arm = BROKEN;
        }
    }
}

/// The weapon state's cooling (bit 1) and none-left (bit 6) bits for robot `id`'s
/// weapon `w`: the cool-down (the scorpion's missile: its reload) not 0; the
/// scorpion's rounds or missiles, or the scout's shells, at 0.
pub fn state_bits(world: &World, id: EntityId, w: usize) -> u8 {
    let Some(e) = world.get(id) else {
        return 0;
    };
    let State::Robot(m) = e.state else {
        return 0;
    };
    let g = m.guns;
    let empty = match (e.kind, w) {
        (ObjectKind::Scorpion, 0) => g.rounds == 0,
        (ObjectKind::Scorpion, _) => g.missiles == 0,
        (ObjectKind::Scout, 1) => g.rounds == 0,
        _ => false,
    };
    (u8::from(g.cooldown[w & 1] != 0) << 1) | (u8::from(empty) << 6)
}

#[cfg(test)]
mod tests;
