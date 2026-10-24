//! The game's rules, headless and deterministic (`docs/sim.md`): the same seed and the
//! same input log always give the same state, and the same [`Sim::state_hash`]. No
//! window, audio or clock: the host passes in the input of each retrace and calls
//! [`Sim::tick`] once per retrace (`docs/game-loop.md`).

pub mod hash;
pub mod input;
pub mod orders;
pub mod random;
pub mod rate;
pub mod world;

pub use random::Random;
pub use rate::Rate;
pub use world::{Entity, EntityId, Fixed, Position, World};

use hash::{StateHash, StateHasher};
use input::{InputEvent, InputLog, Keys, Mouse};

/// The simulation's whole state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sim {
    /// Retraces run, the last one included.
    retrace: u64,
    random: Random,
    keys: Keys,
    mouse: Mouse,
    pub world: World,
}

impl Sim {
    /// An empty world whose random word starts from `seed`.
    pub fn new(seed: u32) -> Sim {
        Sim::with_random(Random::seeded(seed))
    }

    /// An empty world drawing from `random`: the original has one random word for the
    /// whole run, so a mission can carry on the campaign's.
    pub fn with_random(random: Random) -> Sim {
        Sim {
            retrace: 0,
            random,
            keys: Keys::default(),
            mouse: Mouse::default(),
            world: World::default(),
        }
    }

    pub fn retrace(&self) -> u64 {
        self.retrace
    }

    pub fn random(&mut self) -> &mut Random {
        &mut self.random
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn mouse(&self) -> &Mouse {
        &self.mouse
    }

    /// One retrace with the input that arrived for it. For now it only counts the
    /// retrace and tracks held keys and buttons; the rules come with T-051 on.
    pub fn tick<'a>(&mut self, events: impl IntoIterator<Item = &'a InputEvent>) {
        self.retrace += 1;
        for event in events {
            self.keys.apply(event);
            self.mouse.apply(event);
        }
    }

    /// Ticks until `retrace` retraces have run, each with its events from `log`.
    pub fn run(&mut self, log: &InputLog, retrace: u64) {
        while self.retrace < retrace {
            self.tick(log.events_at(self.retrace + 1));
        }
    }

    /// A hash of the whole state that is the same on every machine and toolchain.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.retrace);
        self.random.state_hash(&mut h);
        self.keys.state_hash(&mut h);
        self.mouse.state_hash(&mut h);
        self.world.state_hash(&mut h);
        h.finish()
    }
}

impl StateHash for Keys {
    fn state_hash(&self, h: &mut StateHasher) {
        h.u64(self.held().count() as u64);
        self.held().for_each(|code| h.u16(code.0));
    }
}

impl StateHash for Mouse {
    fn state_hash(&self, h: &mut StateHasher) {
        match self.position {
            None => h.u8(0),
            Some((x, y)) => {
                h.u8(1);
                h.u16(x);
                h.u16(y);
            }
        }
        h.u8(self.held().fold(0, |bits, b| bits | 1 << b as u8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input::{MouseButton, ScanCode};

    #[test]
    fn a_tick_counts_the_retrace_and_applies_its_input() {
        let mut log = InputLog::default();
        let key = ScanCode(0x1e);
        log.record(
            2,
            [InputEvent::KeyDown {
                code: key,
                repeat: false,
            }],
        );
        log.record(3, [InputEvent::MouseMove { x: 4, y: 5 }]);
        log.record(
            3,
            [InputEvent::MouseDown {
                button: MouseButton::Right,
            }],
        );
        let mut sim = Sim::new(1);
        let start = sim.state_hash();
        sim.run(&log, 2);
        assert_eq!(sim.retrace(), 2);
        assert!(sim.keys().is_down(key));
        assert_ne!(sim.state_hash(), start);
        sim.run(&log, 3);
        assert_eq!(sim.mouse().position, Some((4, 5)));
        assert!(sim.mouse().is_down(MouseButton::Right));
        // Running to a retrace already past does nothing.
        let before = sim.clone();
        sim.run(&log, 1);
        assert_eq!(sim, before);
        // Drawing a word is part of the state.
        sim.random().word();
        assert_ne!(sim.state_hash(), before.state_hash());
    }
}
