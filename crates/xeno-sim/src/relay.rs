//! The relays' tick (`sim_tick_relay`, `1000:ccd9`; `docs/sim.md`, "The relay's tick
//! (T-148)"): every tick its clock steps, and every 16 the state's routine runs (fold,
//! rise, stay up, fold up) and the look and height follow the stage. The numbers come
//! from the user's `xeno.exe` (`xeno_formats::relay`, in `Tables::relays`).

use xeno_formats::usg::{self, Side, flag};

use crate::hash::StateHasher;
use crate::motion::Tables;
use crate::terrain::approx_distance;
use crate::world::{EntityId, State, World};

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

/// Relay `id`'s tick (`sim_tick_relay`).
pub fn tick(world: &mut World, id: EntityId, tables: &Tables) {
    let t = &tables.relays;
    let Some(State::Relay(mut r)) = world.get(id).map(|e| e.state) else {
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
                // shortcut: the links and the power (`1000:cd88`) are part 2 of T-148
                // (`t-148-signal`); until then the power stays as loaded.
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
    if r.clock & t.state_mask == 0 {
        e.model_height = r.look(e.side, tables);
    }
    e.state = State::Relay(r);
}

#[cfg(test)]
mod tests;
