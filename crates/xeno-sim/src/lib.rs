//! The game's rules, headless and deterministic (`docs/sim.md`): the same seed and the
//! same input log always give the same state, and the same [`Sim::state_hash`]. No
//! window, audio or clock: the host passes in the input of each retrace and calls
//! [`Sim::tick`] once per retrace (`docs/game-loop.md`).

pub mod clock;
pub mod damage;
pub mod hash;
pub mod input;
pub mod motion;
pub mod orders;
pub mod panel;
pub mod random;
pub mod rate;
pub mod script;
pub mod terrain;
pub mod world;

pub use clock::Clock;
pub use random::Random;
pub use rate::Rate;
pub use world::{
    Census, Components, Damage, Entity, EntityId, Fixed, Movement, Position, State, World,
};

use hash::{StateHash, StateHasher};
use input::{InputEvent, InputLog, Keys, Mouse};
use motion::{Event, Tables};
use orders::{Change, OrderBook};
use xeno_formats::usg::{ObjectKind, Scenario};

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

    /// One retrace with the input that arrived for it: the mission clock moves on
    /// ([`Clock::advance`]), the held keys and buttons are tracked (nothing acts on
    /// them yet), then every object ticks in slot order
    /// (`docs/sim.md`, "Movement"). Returns what the tick did.
    pub fn tick<'a>(&mut self, events: impl IntoIterator<Item = &'a InputEvent>) -> Vec<Event> {
        self.retrace += 1;
        self.world.clock.advance();
        for event in events {
            self.keys.apply(event);
            self.mouse.apply(event);
        }
        match &self.tables {
            Some(tables) => motion::tick(&mut self.world, tables, &mut self.random),
            None => Vec::new(),
        }
    }

    /// Object `id` takes a hit of `points` between ticks (`damage::hit`), as a shot's
    /// would (T-114). Without tables, nothing.
    pub fn hit(&mut self, id: EntityId, points: u8) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(tables) = &self.tables {
            damage::hit(
                &mut self.world,
                id,
                points,
                tables,
                &mut self.random,
                &mut events,
            );
        }
        events
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

    /// A cockpit key acts on robot `id` (`docs/sim.md`, "The cockpit keys"), as the
    /// open console's handler does between ticks. Returns what it did. Without tables
    /// or a robot, nothing.
    pub fn pilot(&mut self, id: EntityId, key: Pilot) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(tables) = &self.tables else {
            return events;
        };
        let Some(e) = self.world.get(id) else {
            return events;
        };
        let (kind, heading, weapons) = match e.damage {
            Damage::Robot(c) => (e.kind, e.heading, c.weapons),
            _ => (e.kind, e.heading, 0),
        };
        if matches!(
            key,
            Pilot::Left | Pilot::Right | Pilot::Faster | Pilot::Slower
        ) {
            // `order_switch_to_manual`: the pilot takes over first.
            script::switch(&mut self.world, id, false, &mut events);
        }
        let Some(m) = motion::movement(&mut self.world, id) else {
            return events;
        };
        let turn = tables.gears.turn;
        match key {
            Pilot::Left => m.target_heading = heading.wrapping_sub(turn),
            Pilot::Right => m.target_heading = heading.wrapping_add(turn),
            Pilot::Faster | Pilot::Slower => {
                motion::shift(tables, kind, m, key == Pilot::Faster);
            }
            Pilot::Stop => {
                m.target_speed = 0;
                motion::match_gear(tables, kind, m);
            }
            Pilot::Stance(stance) => m.stance_target = stance,
            Pilot::Weapon(_) | Pilot::OtherWeapon => {
                let weapon = match key {
                    Pilot::Weapon(w) => w,
                    _ => m.weapon ^ 1,
                };
                if weapon != m.weapon {
                    m.weapon = weapon;
                    let w = &tables.weapon_keys;
                    let k = usize::from(kind == ObjectKind::Scorpion)
                        + 2 * usize::from(kind == ObjectKind::Scout);
                    let damaged = w.scorpion_bits.get(usize::from(weapon));
                    // shortcut: the scorpion's weapon counts (+0x120, +0x122) are not
                    // kept until T-114, so its "empty" message is never posted.
                    let messages = match damaged {
                        Some(&bit) if k == 1 && weapons & bit != 0 => &w.scorpion_damaged,
                        _ => &w.selected[k],
                    };
                    if let Some(&text) = messages.get(usize::from(weapon)) {
                        m.panel.post(text);
                    }
                    events.push(Event::Sound {
                        event: w.sound,
                        only_for: Some(id),
                    });
                }
            }
        }
        events
    }

    /// The situation room recalls robot `id` (F3, its recall button: +0x23 bit 3); its
    /// next tick gives it the recall's program (`motion::tick`).
    pub fn recall(&mut self, id: EntityId) {
        if let Some(m) = motion::movement(&mut self.world, id) {
            m.mode |= Movement::RECALL;
        }
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

/// A cockpit key's action on the robot whose console is open (`Sim::pilot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pilot {
    /// The left and right arrows: the pilot takes over and the heading asked for is
    /// the heading turned by `Tables::gears.turn`.
    Left,
    Right,
    /// The up and down arrows: the pilot takes over and the speed goes a gear up or
    /// down (`motion::shift`).
    Faster,
    Slower,
    /// `.`: the speed asked for is 0 and the gear matches it (opcode 0x53's routine);
    /// the program keeps driving.
    Stop,
    /// A scout's Q, W and E: the stance it rises or sinks to (+0x10c).
    Stance(u8),
    /// F5 and F6: weapon 0 or 1 selected (+0xdd); a change posts the kind's message
    /// and plays its sound for the robot (`Tables::weapon_keys`).
    Weapon(u8),
    /// The humanoid's F9: the other weapon, as `Weapon`.
    OtherWeapon,
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
        assert_eq!(
            sim.world.clock.tick,
            2 * Clock::STEP,
            "the clock moves each retrace"
        );
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
        let start = (
            sim.world.clock.hour,
            sim.world.clock.minute,
            sim.world.clock.tick,
        );
        assert_eq!(start, (7, 29, 20), "the clock starts from SHDR");
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

    #[test]
    fn the_cockpit_keys_take_over_and_drive_the_viewed_robot() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let (id, robot) = (EntityId(2), |sim: &Sim| match sim.world.get(EntityId(2)) {
            Some(Entity {
                state: State::Robot(m),
                heading,
                ..
            }) => (*m, *heading),
            other => panic!("{other:?}"),
        });
        assert!(
            sim.world.orders.get(2).unwrap().orders.manual,
            "program drives"
        );
        // `.` stops it and leaves the program driving.
        assert!(sim.pilot(id, Pilot::Stop).is_empty());
        assert_eq!((robot(&sim).0.target_speed, robot(&sim).0.gear), (0, 4));
        assert!(sim.world.orders.get(2).unwrap().orders.manual);
        // An arrow: the pilot takes over (its event waits) and the speed goes up a gear.
        let events = sim.pilot(id, Pilot::Faster);
        assert!(matches!(events[..], [Event::Program(_)]));
        assert!(!sim.world.orders.get(2).unwrap().orders.manual);
        assert_ne!(robot(&sim).0.script.events & 1 << script::EVENT_PILOT, 0);
        assert_eq!((robot(&sim).0.target_speed, robot(&sim).0.gear), (28, 6));
        sim.pilot(id, Pilot::Slower);
        sim.pilot(id, Pilot::Slower);
        assert_eq!(robot(&sim).0.target_speed, -23);
        // Turning is from the heading, not the heading asked for.
        let heading = robot(&sim).1;
        sim.pilot(id, Pilot::Left);
        sim.pilot(id, Pilot::Left);
        assert_eq!(robot(&sim).0.target_heading, heading.wrapping_sub(0x100));
        sim.pilot(id, Pilot::Right);
        assert_eq!(robot(&sim).0.target_heading, heading.wrapping_add(0x100));
        sim.pilot(id, Pilot::Stance(0x60));
        assert_eq!(robot(&sim).0.stance_target, 0x60);
        // The keys are part of the state, and nothing acts on a slot without a robot.
        let before = sim.state_hash();
        assert!(sim.pilot(EntityId(1), Pilot::Faster).is_empty());
        assert_eq!(sim.state_hash(), before);
    }

    #[test]
    fn a_new_weapon_posts_its_message_and_sound_and_a_recall_runs_its_program() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let m = |sim: &Sim, slot: u16| match sim.world.get(EntityId(slot)).map(|e| e.state) {
            Some(State::Robot(m)) => m,
            other => panic!("{other:?}"),
        };
        // Slot 2 is a humanoid, slot 5 a scorpion; their panels' queues empty.
        for slot in [2, 5] {
            motion::movement(&mut sim.world, EntityId(slot))
                .unwrap()
                .panel
                .queue = [0; 5];
        }
        let sound = |slot| Event::Sound {
            event: 27,
            only_for: Some(EntityId(slot)),
        };
        sim.pilot(EntityId(2), Pilot::Weapon(0));
        assert_eq!(sim.pilot(EntityId(2), Pilot::OtherWeapon), [sound(2)]);
        assert_eq!(m(&sim, 2).weapon, 1);
        assert!(m(&sim, 2).panel.queue.contains(&0x11));
        assert!(
            sim.pilot(EntityId(2), Pilot::Weapon(1)).is_empty(),
            "no change"
        );
        // The scorpion's damaged weapon has its own message.
        if let Some(Damage::Robot(c)) = sim.world.get_mut(EntityId(5)).map(|e| &mut e.damage) {
            c.weapons = 2;
        }
        sim.pilot(EntityId(5), Pilot::Weapon(0));
        assert_eq!(sim.pilot(EntityId(5), Pilot::Weapon(1)), [sound(5)]);
        assert_eq!(m(&sim, 5).panel.queue[..2], [0x20, 0x25]);
        // Recalled with the pilot driving: the next tick gives program 4 and hands the
        // robot to it.
        sim.world.orders.set_manual(5, false);
        sim.recall(EntityId(5));
        assert_ne!(m(&sim, 5).mode & Movement::RECALL, 0);
        let events = sim.tick(&[]);
        assert!(events.iter().any(|e| matches!(e, Event::Program(_))));
        let orders = sim.world.orders.get(5).unwrap().orders;
        assert_eq!(
            (orders.program, orders.ordered, orders.manual),
            (4, 4, true)
        );
        assert_eq!(m(&sim, 5).mode & Movement::RECALL, 0);
    }
}
