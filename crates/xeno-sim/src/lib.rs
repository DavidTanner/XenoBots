//! The game's rules, headless and deterministic (`docs/sim.md`): the same seed and the
//! same input log always give the same state, and the same [`Sim::state_hash`]. No
//! window, audio or clock: the host passes in the input of each retrace and calls
//! [`Sim::tick`] once per retrace (`docs/game-loop.md`).

pub mod hash;
pub mod input;
pub mod motion;
pub mod orders;
pub mod random;
pub mod rate;
pub mod script;
pub mod terrain;
pub mod world;

pub use random::Random;
pub use rate::Rate;
pub use world::{
    Census, Components, Damage, Entity, EntityId, Fixed, Movement, Position, State, World,
};

use hash::{StateHash, StateHasher};
use input::{InputEvent, InputLog, Keys, Mouse};
use motion::{Event, Tables};
use orders::{Change, OrderBook};
use xeno_formats::usg::Scenario;

/// The simulation's whole state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sim {
    /// Retraces run, the last one included.
    retrace: u64,
    random: Random,
    keys: Keys,
    mouse: Mouse,
    pub world: World,
    /// The movement tables from the user's `xeno.exe`; without them nothing moves
    /// (an empty world, or a test of the input alone).
    tables: Option<Tables>,
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
            tables: None,
        }
    }

    /// A mission on `scenario` ([`World::from_scenario`]), drawing from the run's
    /// random word. The original has one word for the whole run, so the mission takes
    /// the campaign's and gives it back with [`Sim::park`] or [`Sim::end`]. Its objects
    /// move by `tables`.
    pub fn load(scenario: &Scenario, random: Random, tables: Tables) -> Sim {
        let mut world = World::from_scenario(scenario);
        script::start_all(&mut world, scenario, &tables);
        Sim {
            world,
            tables: Some(tables),
            ..Sim::with_random(random)
        }
    }

    /// The mission stops for now (it ended with no result, so the map room can resume
    /// it): the random word goes back to the caller, and the rest waits in a
    /// [`Parked`] that cannot tick or draw until [`Parked::resume`] gives it a word.
    pub fn park(self) -> (Parked, Random) {
        let Sim {
            retrace,
            random,
            keys,
            mouse,
            world,
            tables,
        } = self;
        let parked = Parked {
            retrace,
            keys,
            mouse,
            world,
            tables,
        };
        (parked, random)
    }

    /// The mission is over (won or lost): the random word goes back to the caller.
    pub fn end(self) -> Random {
        self.random
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

    /// One retrace with the input that arrived for it: the held keys and buttons are
    /// tracked (nothing acts on them yet), then every object ticks in slot order
    /// (`docs/sim.md`, "Movement"). Returns what the tick did.
    pub fn tick<'a>(&mut self, events: impl IntoIterator<Item = &'a InputEvent>) -> Vec<Event> {
        self.retrace += 1;
        for event in events {
            self.keys.apply(event);
            self.mouse.apply(event);
        }
        match &self.tables {
            Some(tables) => motion::tick(&mut self.world, tables, &mut self.random),
            None => Vec::new(),
        }
    }

    /// Changes a robot's orders as the player does: a new program starts its script
    /// (`order_set_program`), and the program or the pilot taking over posts its
    /// event (`order_switch_to_*`).
    pub fn change_orders(
        &mut self,
        f: impl FnOnce(&mut OrderBook) -> Option<Change>,
    ) -> Option<Change> {
        let change = f(&mut self.world.orders)?;
        let (id, mut events) = (EntityId(change.slot), Vec::new());
        if change.after.manual != change.before.manual {
            self.world
                .orders
                .set_manual(change.slot, change.before.manual);
            script::switch(&mut self.world, id, change.after.manual, &mut events);
        }
        if change.after.program != change.before.program
            && let Some(tables) = &self.tables
        {
            script::set_program(
                &mut self.world,
                id,
                change.after.program,
                tables,
                &mut events,
            );
        }
        Some(change)
    }

    /// Ticks until `retrace` retraces have run, each with its events from `log`.
    pub fn run(&mut self, log: &InputLog, retrace: u64) {
        while self.retrace < retrace {
            _ = self.tick(log.events_at(self.retrace + 1));
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

/// A mission whose sim gave the random word back ([`Sim::park`]): its world, input
/// and retrace count, kept to resume.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parked {
    retrace: u64,
    keys: Keys,
    mouse: Mouse,
    pub world: World,
    tables: Option<Tables>,
}

impl Parked {
    /// The mission goes on where it stopped, drawing from `random` again.
    pub fn resume(self, random: Random) -> Sim {
        let Parked {
            retrace,
            keys,
            mouse,
            world,
            tables,
        } = self;
        Sim {
            retrace,
            random,
            keys,
            mouse,
            world,
            tables,
        }
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

    #[test]
    fn a_mission_takes_the_run_s_random_word_and_gives_it_back() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        // The campaign draws, then lends its word to the mission, which draws more.
        let mut campaign = Random::seeded(9);
        campaign.word();
        let mut sim = Sim::load(&scenario, campaign, motion::synthetic_tables());
        assert_eq!(sim.world.len(), 5);
        assert_eq!(sim.world.orders, World::from_scenario(&scenario).orders);
        // Each robot's program starts anew, in the file's order, staggered.
        let waits: Vec<u16> = sim
            .world
            .iter()
            .filter_map(|(_, e)| match e.state {
                State::Robot(m) => Some(m.script.wait),
                _ => None,
            })
            .collect();
        assert_eq!(waits, [39, 29, 19]);
        sim.tick(&[InputEvent::MouseMove { x: 1, y: 2 }]);
        sim.random().word();
        // Parked, it keeps its world and input and hands the word back as it stands.
        let (parked, mut random) = sim.clone().park();
        let mut same = Random::seeded(9);
        (0..2).for_each(|_| _ = same.word());
        assert_eq!(random, same);
        // The campaign draws in between; the mission resumes with the word as it is.
        random.word();
        let resumed = parked.resume(random.clone());
        assert_eq!((resumed.retrace(), resumed.mouse()), (1, sim.mouse()));
        assert_eq!(resumed.world, sim.world);
        assert_eq!(resumed.end(), random);
    }
}
