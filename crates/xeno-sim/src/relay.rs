//! The relays' tick (`sim_tick_relay`, `1000:ccd9`; `docs/sim.md`, "The relay's tick
//! (T-148)"): every tick its clock steps, and every 16 the state's routine runs (fold,
//! rise, stay up and link, fold up) and the look and height follow the stage; and the
//! signal and drive a robot takes from the net. The numbers come from the user's
//! `xeno.exe` (`xeno_formats::relay`, in `Tables::relays`).

use xeno_formats::usg::{self, Side, flag};

use xeno_formats::usg::ObjectKind;

use crate::hash::StateHasher;
use crate::motion::Tables;
use crate::orders::RobotKind;
use crate::script::bearing;
use crate::terrain::approx_distance;
use crate::world::{Damage, EntityId, State, World};

/// A link's [`Relay::links`] states.
const FED: u8 = usg::Link::FED;
const OPEN: u8 = usg::Link::FEEDS;
/// +0xfd bit 0: the robot runs on its battery.
const ON_BATTERY: u8 = usg::Robot::ON_BATTERY;

/// A relay's state (`usg::Relay`; `docs/formats/usg.md`, "Relays").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Relay {
    /// +0x18..=+0x1d: the body's frame, the stage's look (four) and the colour.
    pub frames: [u8; 6],
    /// +0x1e: the clock; +0x1f the state; +0x20 the stage (0 to 3).
    pub clock: u8,
    pub state: u8,
    pub stage: u8,
    /// +0x21: the power's reach (0 unpowered); +0x22 whose (0 none, 1 human, 2 alien).
    pub power: u8,
    pub power_side: u8,
    /// +0x23 + 4n: each quarter turn's link (`usg::Link::state`).
    pub links: [u8; 4],
    /// +0x16's `IN_FIELD` and `RELAY_LINKED` (the side is the entity's).
    pub flags: u8,
}

pub const FOLDED: u8 = usg::Relay::FOLDED;
pub const RISING: u8 = usg::Relay::RISING;
pub const UP: u8 = usg::Relay::UP;
pub const FOLDING: u8 = usg::Relay::FOLDING;
/// The flag bits a relay's tick moves.
pub const FLAGS: u8 = flag::IN_FIELD | flag::RELAY_LINKED;

impl Relay {
    /// The state a file's relay records, with its `flags` (+0x16).
    pub fn of(r: &usg::Relay, flags: u8) -> Relay {
        Relay {
            frames: r.frames,
            clock: r.clock,
            state: r.state,
            stage: r.stage,
            power: r.power,
            power_side: r.power_side,
            links: r.links.map(|l| l.state),
            flags: flags & FLAGS,
        }
    }

    /// A relay set up by `1000:cc2a`: folded, stage 0, unpowered, its clock `clock`;
    /// then its look (with its model height, returned).
    pub fn made(clock: u8, tables: &Tables) -> (Relay, u16) {
        let mut r = Relay {
            clock,
            ..Relay::default()
        };
        let height = r.look(Side::Human, tables);
        (r, height)
    }

    /// The look (`1000:ccf0`): the colour by the net's power and the side, the frames
    /// by stage, and the model height (+0x12) returned.
    // shortcut: past stage 3 the original reads the bytes after the tables; the port
    // takes frame and height 0. No relay reaches it.
    pub fn look(&mut self, side: Side, tables: &Tables) -> u16 {
        let t = &tables.relays;
        self.frames[5] = match (self.flags & flag::RELAY_LINKED != 0, side) {
            (false, _) => t.colours[0],
            (true, Side::Human) => t.colours[1],
            (true, Side::Alien) => t.colours[2],
        };
        let stage = usize::from(self.stage);
        self.frames[1..5].fill(t.looks.get(stage).copied().unwrap_or(0));
        t.heights.get(stage).copied().unwrap_or(0)
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bytes(&self.frames);
        h.bytes(&[
            self.clock,
            self.state,
            self.stage,
            self.power,
            self.power_side,
        ]);
        h.bytes(&self.links);
        h.u8(self.flags);
    }
}

/// Whether an object of the folding kind (a scorpion) is within the fold's reach of
/// `id` (`1000:f703`: any slot, any side, under 256 units, the distance as 8.8 under
/// the reach).
fn folder_near(world: &World, id: EntityId, tables: &Tables) -> bool {
    let t = &tables.relays;
    let Some(at) = world.get(id).map(|e| e.position) else {
        return false;
    };
    world.iter().any(|(_, e)| {
        let d = approx_distance(at, e.position).0 as u32;
        u16::from(e.kind) == t.folder && d >> 24 == 0 && ((d >> 8) as u16) < t.fold_reach
    })
}

/// The distance from `a` to `b` as 8.8, under 256 units only (`1000:08ec`, `DH`
/// tested).
fn distance(a: crate::Position, b: crate::Position) -> Option<u16> {
    let d = approx_distance(a, b).0 as u32;
    (d >> 24 == 0).then_some((d >> 8) as u16)
}

/// `side` takes `offered` (`1000:cf55`, `cf66`): a relay with no side yet takes it.
fn takes(side: &mut u8, offered: u8) -> bool {
    if *side == 0 {
        *side = offered;
    }
    *side == offered
}

/// The links (`1000:cd88`), once every 256 ticks of an up relay `r` (clock 0): the
/// nearest relay or base each quarter turn within reach feeds it if it is a base of
/// its side or a relay whose opposite link is open and of its side; it takes the best
/// power offered less one, its side follows the power, and powered it opens its other
/// links. Returns the relay's side.
// shortcut: an object 256 units or more away is skipped; the original jumps back into
// its loop with two words still pushed. No stock relay or base is that far from another.
fn links(world: &World, id: EntityId, r: &mut Relay, side: Side, tables: &Tables) -> Side {
    let t = &tables.relays;
    let Some(at) = world.get(id).map(|e| e.position) else {
        return side;
    };
    // `2000:6978`: the relay's own side's base's net power.
    let base_power = match world.base(side).and_then(|b| world.get(b)).map(|b| b.state) {
        Some(State::Base { depot, .. }) => depot.power[1] >> 2,
        _ => 0,
    };
    let mut nearest: [Option<(EntityId, u16)>; 4] = [None; 4];
    for (n, e) in world.iter() {
        if n == id || !matches!(e.kind, ObjectKind::Relay | ObjectKind::Base) {
            continue;
        }
        let Some(d) = distance(e.position, at).filter(|&d| d <= t.link_reach) else {
            continue;
        };
        let quarter = usize::from(bearing(&tables.atan, e.position, at).wrapping_add(0x8000) >> 14);
        if nearest[quarter].is_none_or(|(_, best)| d < best) {
            nearest[quarter] = Some((n, d));
        }
    }
    let (mut power_side, mut best) = (r.power_side, 0u8);
    r.links = [0; 4];
    for (q, n) in nearest.iter().enumerate() {
        let Some(e) = n.and_then(|(n, _)| world.get(n)) else {
            continue;
        };
        let offered = match e.state {
            _ if e.kind == ObjectKind::Base => {
                let theirs = if e.side == Side::Alien { 2 } else { 1 };
                takes(&mut power_side, theirs).then_some(base_power)
            }
            State::Relay(o) if o.links[(q + 2) & 3] == OPEN => {
                takes(&mut power_side, o.power_side).then_some(o.power)
            }
            _ => None,
        };
        if let Some(p) = offered {
            r.links[q] = FED;
            best = best.max(p);
        }
    }
    r.power_side = power_side;
    r.power = best.saturating_sub(1);
    if r.power != 0 {
        r.links
            .iter_mut()
            .filter(|l| **l != FED)
            .for_each(|l| *l = OPEN);
    } else {
        r.power_side = 0;
    }
    if power_side == 2 {
        Side::Alien
    } else {
        Side::Human
    }
}

/// Relay `id`'s tick (`sim_tick_relay`).
pub fn tick(world: &mut World, id: EntityId, tables: &Tables) {
    let t = &tables.relays;
    let Some((State::Relay(mut r), mut side)) = world.get(id).map(|e| (e.state, e.side)) else {
        return;
    };
    r.clock = r.clock.wrapping_add(1);
    if r.clock & t.state_mask == 0 {
        match r.state {
            FOLDED => {
                r.flags &= !FLAGS;
                r.stage = 0;
                if !folder_near(world, id, tables) {
                    r.state = RISING;
                }
            }
            RISING => {
                r.flags &= !flag::RELAY_LINKED;
                r.stage = r.stage.wrapping_add(1);
                if r.stage == t.top {
                    r.state = UP;
                }
            }
            UP => {
                if r.clock == 0 {
                    side = links(world, id, &mut r, side, tables);
                }
                r.flags = r.flags & !flag::RELAY_LINKED | flag::IN_FIELD;
                if r.power != 0 {
                    r.flags |= flag::RELAY_LINKED;
                }
                if folder_near(world, id, tables) {
                    r.state = FOLDING;
                }
            }
            FOLDING => {
                r.flags &= !flag::RELAY_LINKED;
                r.stage = r.stage.wrapping_sub(1);
                if r.stage == 0 {
                    r.state = FOLDED;
                }
            }
            // State 8 does nothing. Nothing sets any other: 10 would run the links'
            // first direction routine (`DS:0x6fd1` follows the table), odd ones jump
            // into the middle of an address.
            _ => {}
        }
    }
    let Some(e) = world.get_mut(id) else {
        return;
    };
    e.side = side;
    if r.clock & t.state_mask == 0 {
        e.model_height = r.look(side, tables);
    }
    e.state = State::Relay(r);
}

/// The signal robot `id` takes from the net (`2000:7d16`): 0xff within the full reach
/// of a linked relay or base of its side, else falling with the distance of the last
/// in slot order under the reach of none, else 0.
// The sim keeps no base's flags: nothing in play links a base and no stock base is.
pub fn signal(world: &World, id: EntityId, tables: &Tables) -> u8 {
    let t = &tables.relays;
    let Some(me) = world.get(id) else {
        return 0;
    };
    let mut last = 0xffff;
    for (_, e) in world.iter() {
        let linked = matches!(e.state, State::Relay(r) if r.flags & flag::RELAY_LINKED != 0);
        if e.side != me.side || !linked {
            continue;
        }
        match distance(e.position, me.position) {
            Some(d) if d <= t.full => return 0xff,
            Some(d) if d < t.none => last = d,
            _ => {}
        }
    }
    match last.wrapping_sub(t.full) {
        d if d < t.none.wrapping_sub(t.full) => (d as u8).wrapping_mul(4).wrapping_neg(),
        _ => 0,
    }
}

/// Robot `id`'s signal step, at its kind's entry of its tick table (`2000:8087`, from
/// `1000:b2fc`, `99f8`, `cad9`): the signal; over the battery level off the battery and
/// the energy up one, else on it; the drive the signal (or the energy on the battery)
/// masked by the power; then every 512 passes the energy up one more.
pub fn signal_step(world: &mut World, id: EntityId, tables: &Tables) {
    let t = &tables.relays;
    let Some((k, tick)) = world.get(id).and_then(|e| match e.state {
        State::Robot(m) => Some((RobotKind::of(e.kind)?, m.tick)),
        _ => None,
    }) else {
        return;
    };
    if (tick & 0x1e) >> 1 != t.signal_steps[k.index()] {
        return;
    }
    let signal = self::signal(world, id, tables);
    let ticks = world.ticks;
    let Some(e) = world.get_mut(id) else {
        return;
    };
    let (State::Robot(m), Damage::Robot(c)) = (&mut e.state, &mut e.damage) else {
        return;
    };
    let up = |energy: &mut u8| *energy = energy.saturating_add(1);
    m.signal = signal;
    if signal > t.battery {
        c.equipment &= !ON_BATTERY;
        up(&mut m.guns.energy);
    } else {
        c.equipment |= ON_BATTERY;
    }
    let drive = if c.equipment & ON_BATTERY != 0 {
        m.guns.energy
    } else {
        signal
    };
    m.drive = drive & t.drive_masks[usize::from(c.power >> 4)];
    if ticks & t.recharge == 0 {
        up(&mut m.guns.energy);
    }
}

#[cfg(test)]
mod tests;
