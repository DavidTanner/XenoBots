//! The game's rules, headless and deterministic (`docs/sim.md`): the same seed and the
//! same input log always give the same state, and the same [`Sim::state_hash`]. No
//! window, audio or clock: the host passes in the input of each retrace and calls
//! [`Sim::tick`] once per retrace (`docs/game-loop.md`).

pub mod base;
pub mod clock;
pub mod contact;
pub mod damage;
pub mod drops;
pub mod editor;
pub mod hash;
pub mod input;
pub mod log;
pub mod motion;
pub mod net;
pub mod orders;
mod pack;
pub mod panel;
pub mod random;
pub mod rate;
pub mod relay;
pub mod script;
pub mod sense;
pub mod shot;
pub mod terrain;
pub mod weapons;
pub mod world;

pub use clock::Clock;
pub use random::Random;
pub use rate::Rate;
pub use world::{
    Census, Components, Damage, Entity, EntityId, Fixed, Guns, Movement, Position, State, World,
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
        pack::unpack_made(&mut world, scenario, &tables);
        script::start_all(&mut world, scenario, &tables);
        // shortcut: the original carries the worked-out point from mission to mission
        // (from the image's value); every stock script works it out before using it.
        let (x, y) = tables.sensing.point;
        world.point = Position {
            x: Fixed(x),
            y: Fixed(y),
        };
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

    /// The tables from the user's `xeno.exe`, if loaded.
    pub fn tables(&self) -> Option<&Tables> {
        self.tables.as_ref()
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
        let month_days = self.tables.as_ref().map(|t| &t.calendar.month_days[..]);
        self.world.clock.advance(month_days.unwrap_or_default());
        // shortcut: the 60 Hz chain from 70 Hz retraces (docs/game-loop.md).
        self.world.game_clock = (self.retrace * 60 / 70) as u16;
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
        if change.after.program_drives != change.before.program_drives {
            self.world
                .orders
                .set_program_drives(change.slot, change.before.program_drives);
            script::switch(
                &mut self.world,
                id,
                change.after.program_drives,
                &mut events,
            );
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
        let world = &mut self.world;
        match key {
            Pilot::Deploy => {
                net::deploy(world, id, tables, &mut self.random, &mut events);
            }
            Pilot::Fire => weapons::trigger(world, id, tables),
            Pilot::Ecm => weapons::ecm(world, id, tables),
            Pilot::Cloak => weapons::cloak(world, id, tables),
            Pilot::Drop(n) => drops::drop(world, id, n, tables, &mut self.random),
            Pilot::NextTarget => sense::next_target(world, id, tables, &mut events),
            Pilot::DropTarget => sense::drop_target(world, id, tables),
            Pilot::Lock(target) => sense::lock(world, id, target, tables, &mut events),
            Pilot::Destination(point) => script::set_destination(world, id, point, tables),
            Pilot::GoalObject(object) => script::set_goal_object(world, id, object, tables),
            Pilot::Untrack => {
                if let Some(e) = world.get_mut(id)
                    && matches!(e.state, State::Robot(_))
                {
                    e.status &= !xeno_formats::usg::status::TRACKING;
                }
            }
            Pilot::Charge => {
                if let Some(e) = world.get_mut(id)
                    && let (Damage::Robot(c), State::Robot(m)) = (&mut e.damage, &mut e.state)
                {
                    c.power = 100;
                    (m.signal, m.guns.energy, m.drive) = (0xff, 0xff, 0xff);
                }
            }
            _ => {}
        }
        // The rest act on the robot's own movement.
        if !matches!(
            key,
            Pilot::Left
                | Pilot::Right
                | Pilot::Faster
                | Pilot::Slower
                | Pilot::Stop
                | Pilot::Stance(_)
                | Pilot::Weapon(_)
                | Pilot::OtherWeapon
        ) {
            return events;
        }
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
                motion::select_weapon(tables, kind, weapons, id, m, weapon, &mut events);
            }
            _ => {}
        }
        events
    }

    /// A console opens (`1000:d805`, from each kind's console set-up): the A key's
    /// search starts again from slot 0.
    pub fn open_console(&mut self) {
        self.world.target_search = 0;
    }

    /// The situation room recalls robot `id` (F3, its recall button: +0x23 bit 3); its
    /// next tick gives it the recall's program (`motion::tick`).
    pub fn recall(&mut self, id: EntityId) {
        if let Some(m) = motion::movement(&mut self.world, id) {
            m.mode |= Movement::RECALL;
        }
    }

    /// The situation room launches robot `id` (F2 `1000:6704`, its launch button
    /// `5ba0`): only while it is in the base (+0xfd bit 2), and whatever its damage or
    /// repairs, it leaves at its side's base's position (`base::launch`). Returns what
    /// it did.
    pub fn launch(&mut self, id: EntityId) -> Vec<Event> {
        let mut events = Vec::new();
        let (Some(tables), Some(e)) = (&self.tables, self.world.get(id)) else {
            return events;
        };
        if let (true, Some(b)) = (base::in_base(&e.damage), self.world.base(e.side)) {
            base::launch(&mut self.world, b, id, tables, &mut events);
        }
        events
    }

    /// Side `side`'s base holds slider `slider` of the situation room's power rows
    /// (+0x2d: 1 to 6 lower and raise the net's power, repairs and relays made; 0
    /// none); each tick moves it (`docs/sim.md`, "The bases' phases").
    pub fn hold_slider(&mut self, side: xeno_formats::usg::Side, slider: u8) {
        if let Some(State::Base { depot, .. }) = self
            .world
            .base(side)
            .and_then(|b| self.world.get_mut(b))
            .map(|e| &mut e.state)
        {
            depot.slider = slider;
        }
    }

    /// Side `side`'s base moves slider routine `slider` once now (the situation room's
    /// keys 1 to 6, `1000:6629` to `6694`, the routines the held slider runs).
    pub fn step_slider(&mut self, side: xeno_formats::usg::Side, slider: u8) {
        let (Some(tables), Some(b)) = (&self.tables, self.world.base(side)) else {
            return;
        };
        if let Some(State::Base { depot, .. }) = self.world.get_mut(b).map(|e| &mut e.state) {
            base::slide(depot, &tables.bases, slider);
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
    /// The scorpion's U: a relay deployed where it stands (`net::deploy`).
    Deploy,
    /// Space: the kind's trigger (`weapons::trigger`), as the scripts' opcode 0x7d.
    Fire,
    /// A: the next enemy in locking range locked (`sense::next_target`); R: the target
    /// dropped (`sense::drop_target`). Neither takes the pilot's seat.
    NextTarget,
    DropTarget,
    /// F7 and F8 on the humanoid and scout (and their consoles' ECM and cloak buttons):
    /// the ECM (`weapons::ecm`) and the sparkle cloak (`weapons::cloak`).
    Ecm,
    Cloak,
    /// Keys 1 to 4 (0 for key 1) on the scorpion and scout, and their consoles' drop
    /// buttons (`xeno_formats::weapons::Drops::buttons`): a mine, an ECM buoy, an alarm buoy or a decoy
    /// dropped (`drops::drop`).
    Drop(u8),
    /// A left release on an object in the console's views (`1000:d8a7`): that object
    /// locked (`sense::lock`).
    Lock(EntityId),
    /// The map display's Alt press (`1000:466b`): on nothing, the destination there
    /// (`script::set_destination`); on an object, that object the goal
    /// (`script::set_goal_object`).
    Destination(Position),
    GoalObject(EntityId),
    /// The second display's zoom and pan keys (`1000:da0e`, `da37`, `b377`, `b389`,
    /// `b39b` and the other kinds'): +0x17 bit 6 (`TRACKING`) cleared, so the display
    /// no longer follows the target (`docs/screens.md`, "The display's other modes
    /// (T-134)").
    Untrack,
    /// Ctrl+P (`1000:d193`), which the caller lets through only with unlock word 8
    /// (`docs/formats/key.md`): power (+0x4e) 100, the signal (+0x4f), the battery
    /// (+0x50) and the drive (+0x51) 0xff. The original then beeps (`1000:0261`).
    Charge,
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
            sim.world.orders.get(2).unwrap().orders.program_drives,
            "program drives"
        );
        // `.` stops it and leaves the program driving.
        assert!(sim.pilot(id, Pilot::Stop).is_empty());
        assert_eq!((robot(&sim).0.target_speed, robot(&sim).0.gear), (0, 4));
        assert!(sim.world.orders.get(2).unwrap().orders.program_drives);
        // An arrow: the pilot takes over (its event waits) and the speed goes up a gear.
        let events = sim.pilot(id, Pilot::Faster);
        assert!(matches!(events[..], [Event::Program(_)]));
        assert!(!sim.world.orders.get(2).unwrap().orders.program_drives);
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
        // A display key stops the tracking and nothing else.
        sim.world.get_mut(id).unwrap().status |= xeno_formats::usg::status::TRACKING | 1;
        assert!(sim.pilot(id, Pilot::Untrack).is_empty());
        assert_eq!(sim.world.get(id).unwrap().status & 0x41, 1);
        // The keys are part of the state, and nothing acts on a slot without a robot.
        let before = sim.state_hash();
        assert!(sim.pilot(EntityId(1), Pilot::Faster).is_empty());
        assert!(sim.pilot(EntityId(1), Pilot::Charge).is_empty());
        assert_eq!(sim.state_hash(), before);
        // Ctrl+P charges the robot up, as part of the state.
        assert!(sim.pilot(id, Pilot::Charge).is_empty());
        let m = robot(&sim).0;
        assert_eq!((m.signal, m.guns.energy, m.drive), (0xff, 0xff, 0xff));
        assert!(matches!(
            sim.world.get(id).map(|e| e.damage),
            Some(Damage::Robot(c)) if c.power == 100
        ));
        assert_ne!(sim.state_hash(), before);
    }

    #[test]
    fn the_scorpion_s_u_deploys_a_relay_where_it_stands() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        // Slot 5 is a scorpion; the relay takes the lowest free slot, 1.
        let (id, at) = (EntityId(5), sim.world.get(EntityId(5)).unwrap().position);
        let carried = |sim: &Sim| net::relays(&sim.world, id);
        motion::movement(&mut sim.world, id).unwrap().relays = 12;
        // The fixture's deployer is damaged: none to deploy, and a message.
        assert_eq!(carried(&sim), 0);
        assert!(sim.pilot(id, Pilot::Deploy).is_empty());
        if let Damage::Robot(c) = &mut sim.world.get_mut(id).unwrap().damage {
            c.equipment &= 0x7f;
        }
        let before = carried(&sim);
        let word = sim.random.clone().word();
        let events = sim.pilot(id, Pilot::Deploy);
        assert!(matches!(events[..], [Event::Sound { event: 25, .. }]));
        assert_eq!(carried(&sim), before - 1);
        let relay = sim.world.get(EntityId(1)).unwrap();
        assert_eq!(
            (relay.kind, relay.position),
            (xeno_formats::usg::ObjectKind::Relay, at)
        );
        // Making it drew a random word (the relay's clock).
        assert_ne!(sim.random.clone().word(), word);
    }

    #[test]
    fn a_launch_puts_a_robot_in_the_base_at_the_base_s_position() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        use xeno_formats::usg::Robot as Bits;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let id = EntityId(2);
        let set = |sim: &mut Sim, bits: u8, total: u8| {
            if let Some(Damage::Robot(c)) = sim.world.get_mut(id).map(|e| &mut e.damage) {
                (c.equipment, c.total) = (bits, total);
            }
        };
        // In the field: nothing.
        set(&mut sim, 0, 0);
        let before = sim.state_hash();
        assert!(sim.launch(id).is_empty());
        assert_eq!(sim.state_hash(), before);
        // In the base, still damaged (no repair is waited for): out at the base.
        set(&mut sim, Bits::ENTERING_BASE | Bits::IN_BASE, 30);
        sim.world.get_mut(id).unwrap().position.x.0 ^= i32::MIN;
        let events = sim.launch(id);
        assert!(events.contains(&Event::Launched(id)));
        let base = sim.world.get(EntityId(0)).unwrap().position;
        let e = sim.world.get(id).unwrap();
        assert_eq!(e.position, base);
        assert!(!base::in_base(&e.damage));
        assert!(sim.world.orders.get(2).unwrap().orders.program_drives);
        // Launched once: a second press does nothing.
        assert!(sim.launch(id).is_empty());
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
        sim.world.orders.set_program_drives(5, false);
        sim.recall(EntityId(5));
        assert_ne!(m(&sim, 5).mode & Movement::RECALL, 0);
        let events = sim.tick(&[]);
        assert!(events.iter().any(|e| matches!(e, Event::Program(_))));
        let orders = sim.world.orders.get(5).unwrap().orders;
        assert_eq!(
            (orders.program, orders.ordered, orders.program_drives),
            (4, 4, true)
        );
        assert_eq!(m(&sim, 5).mode & Movement::RECALL, 0);
    }
}
