//! The world: the mission's objects by their slot in the original's object table.
//!
//! The shape follows a `.usg` file's `DCBS` objects (`docs/formats/usg.md`): a slot, a
//! kind, a side, a 16.16 position, a heading, a radius and the damage taken, so
//! [`World::from_scenario`] copies them field for field, with the terrain and the paths
//! ([`crate::terrain`]). There is no map grid and no world edge: the original has
//! neither, so positions are never limited (`docs/formats/usg.md`, "The world").

use std::collections::BTreeMap;

use xeno_formats::usg::{self, Body, Object, ObjectKind, Scenario, Side};

use crate::clock::Clock;
use crate::hash::{StateHash, StateHasher};
use crate::net::Grid;
use crate::orders::OrderBook;
use crate::panel::Panel;
use crate::terrain::{Paths, Terrain};

/// A 16.16 fixed-point number, as the original keeps positions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fixed(pub i32);

impl Fixed {
    pub const ONE: Fixed = Fixed(0x1_0000);

    pub fn from_int(n: i16) -> Fixed {
        Fixed(i32::from(n) << 16)
    }

    /// The whole part, rounded towards minus infinity.
    pub fn floor(self) -> i16 {
        (self.0 >> 16) as i16
    }
}

/// Where an object is: `.usg` objects' +0x04 and +0x08.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub x: Fixed,
    pub y: Fixed,
}

/// An object's slot in the original's object table (`usg::Object::slot`, below 240).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId(pub u16);

/// One object. Its id is its key in the [`World`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub kind: ObjectKind,
    pub side: Side,
    pub position: Position,
    /// +0x0c: its height, 16.16 (a robot's is the ground's, set as it moves).
    pub z: Fixed,
    /// +0x12: where its model's anchor sits above `z`, in 1/65,536ths of a unit
    /// (`docs/render.md`, "A node"); a walking robot's bobs.
    pub model_height: u16,
    /// +0x10: 65,536 to a turn.
    pub heading: u16,
    /// +0x14: 8.8 fixed point.
    pub radius: u16,
    pub damage: Damage,
    /// What moves it: a robot's [`Movement`], a base's tick.
    pub state: State,
}

/// The per-tick state of an object (`docs/sim.md`, "Movement").
// Robots are most objects, and the state is copied freely; boxing would lose `Copy`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    Robot(Movement),
    /// A base: +0x41, its tick count; +0x2c and +0x2b, the enemy robots it has
    /// destroyed and its own side's lost; +0x16 bit 0, destroyed; +0x30, the step of
    /// its end script once destroyed (`docs/sim.md`, "The mission's end"); +0x18, its
    /// look in the 3D view, the frame of its model (`docs/render.md`); +0x21..=+0x27,
    /// the count its tick makes (`docs/sim.md`, "The bases' phases").
    Base {
        tick: u8,
        kills: u8,
        losses: u8,
        destroyed: bool,
        end_step: u16,
        look: u8,
        counts: [u8; 7],
        depot: Depot,
    },
    /// An explosion (`docs/sim.md`, "Damage"): its frame (+0x18) up to the last
    /// (+0x1a), the ticks a frame (+0x1b) and the ticks left of this one (+0x1c).
    Explosion {
        frame: u8,
        last: u8,
        ticks: u8,
        countdown: u8,
    },
    /// A shot in flight (`crate::shot`).
    Shot(crate::shot::Shot),
    /// Relays, obstacles and objects without a known layout do nothing yet.
    #[default]
    None,
}

/// A base's relays, repairs and power sliders (`docs/sim.md`, "The bases' phases").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Depot {
    /// +0x20: relays in store; +0x19 and +0xb5, the clock and carries making them.
    pub relays: u8,
    pub relay_clock: u8,
    pub relay_wait: u16,
    /// +0x1a: the robot being repaired; +0x1c, the repairs' clock.
    pub repairing: Option<EntityId>,
    pub repair_clock: u16,
    /// +0x38..=+0x3b: the pool, the net's power, repairs and relays made.
    pub power: [u8; 4],
    /// +0x2d: the slider held (1 to 6), 0 none.
    pub slider: u8,
}

impl Depot {
    fn of(b: &usg::Base) -> Depot {
        Depot {
            relays: b.relays,
            relay_clock: b.relay_clock,
            relay_wait: b.relay_wait,
            repairing: (b.repairing != 0xffff).then_some(EntityId(b.repairing)),
            repair_clock: b.repair_clock,
            power: b.power,
            slider: b.slider,
        }
    }
}

/// A robot's movement fields (`usg::Robot`; `docs/formats/usg.md`, "Object fields").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Movement {
    /// +0x1a: the tick counter, stepping by 2.
    pub tick: u8,
    /// +0x1c: the eye height, the 3D view's camera above `z` (`docs/render.md`, "The
    /// camera"), in 1/65,536ths of a unit.
    pub eye_height: u16,
    /// +0x1e, +0x20: added to x and y every tick, in 1/65,536ths.
    pub velocity: (i16, i16),
    /// +0x23 without bit 7 (the program bit, kept in the [`OrderBook`]): bit 0 no
    /// path from the alien base, bit 3 recalled, bits 4 and 5 the turn keys.
    pub mode: u8,
    /// +0x26, +0x28: the speed and the speed asked for.
    pub speed: i16,
    pub target_speed: i16,
    /// +0x2a: the heading asked for.
    pub target_heading: u16,
    /// +0x2c: the path's slot (bits 0 to 3) and [`Movement::BACKWARDS`],
    /// [`Movement::NEXT`], [`Movement::ON_PATH`]; +0x2d the point it heads for.
    pub path: u8,
    pub path_point: u8,
    /// +0x32: what it heads for (0x22 the destination).
    pub goal: u8,
    /// +0x3c: where it heads, or its offset from its goal object.
    pub destination: Position,
    /// +0x3a: the slot of the object it heads for (+0x32 bit 0), 0xffff none.
    pub goal_object: u16,
    /// +0x51: the drive.
    pub drive: u8,
    /// A humanoid's (+0x10b) or scout's (+0x109) speed gear, twice an index into its
    /// gears (`Tables::gears`); 0 for a scorpion.
    pub gear: u8,
    /// +0xdd: the weapon selected (0 or 1).
    pub weapon: u8,
    /// +0x10b, +0x10c: a scout's stance and the one it moves to (a humanoid's +0x10b
    /// is its gear).
    pub stance: u8,
    pub stance_target: u8,
    /// +0x118: the relays a scorpion carries; half of it picks its speed table.
    pub relays: u8,
    /// The walking animation's step counter (humanoid +0x105, scorpion +0x116, scout
    /// +0x10a) and the frame bytes +0xff to +0x104 (`legs[k]` is +0xff + k) its legs
    /// are (`docs/render.md`, "Walking"): the humanoid's +0x101 and +0x102 (its +0xff
    /// and +0x100 are its arms, [`Guns::arms`], and 0 here), the scorpion's six, the
    /// scout's +0xff, +0x100, +0x103 and +0x104 (+0x101 and +0x102 as loaded).
    pub stride: u8,
    pub legs: [u8; 6],
    /// +0x17: the status bits (`usg::status`): cloak, ECM, spotted, tracking and the
    /// count to the next scan.
    pub status: u8,
    /// +0xf7: the enemy units the last scan found (`docs/sim.md`, "Sensing").
    pub contacts: u8,
    /// +0xe6: the target, locked (the loader clears it); +0xe8 and +0xea its
    /// distance (8.8) and bearing when the lock was last kept.
    pub target: Option<EntityId>,
    pub target_distance: u16,
    pub target_bearing: u16,
    /// +0xed: another robot's lock on this one, halved every 16 ticks.
    pub locked_on: u8,
    /// +0x10a..+0x114: a scorpion's line of relay-net cells (`net::Grid`).
    // shortcut: kept for every kind; a humanoid's or scout's script running the net
    // opcodes would overwrite its gear and light there in the original. No stock
    // program gives them a net.
    pub grid: Grid,
    pub guns: Guns,
    pub script: Script,
    pub panel: Panel,
}

/// A robot's weapons (`docs/sim.md`, "Weapons and shots"); the fire request and the
/// humanoid's ready bits are +0xdc's, in [`Components::weapons`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Guns {
    /// +0x50: the battery firing takes from.
    pub energy: u8,
    /// The two weapons' cool-downs: the humanoid's +0x108 and +0x109, the scout's +0x105
    /// and +0x106, the scorpion's cannon's +0x11f and its missile's reload +0x125.
    pub cooldown: [u16; 2],
    /// The scorpion's cannon rounds (+0x120), the scout's shells (+0x107).
    pub rounds: u16,
    /// The scorpion's tail missiles (+0x122), the fire requests its burst still queues
    /// (+0x11e), the barrels that fired (+0x11d) and its missile in flight (+0x123).
    pub missiles: u8,
    pub burst: u8,
    pub fired: u8,
    pub missile: Option<EntityId>,
    /// The humanoid's arms' frames (+0xff, +0x100) and how far each is raised to fire
    /// (+0x103, +0x104: 0 down, 1 rising, 2 ready, 3 lowering).
    pub arms: [u8; 2],
    pub raised: [u8; 2],
}

/// A robot's script state (`docs/sim.md`, "Orders"); the program running is in the
/// [`OrderBook`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Script {
    /// +0x34: the script pointer, a data-segment offset.
    pub pointer: u16,
    /// +0x38: the ticks to wait before the next opcode.
    pub wait: u16,
    /// +0x31: the stack pointer, 10 when empty, into [`Script::stack`] (+0x44).
    pub sp: u8,
    pub stack: [u8; 10],
    /// +0x33: carry (bit 0), zero (bit 6) and sign (bit 7) of the last test; bit 5
    /// steps every tick and holds events off.
    pub flags: u8,
    /// +0x36: the events waiting, a bit each.
    pub events: u16,
}

impl Movement {
    pub const BACKWARDS: u8 = usg::Robot::PATH_BACKWARDS;
    pub const NEXT: u8 = usg::Robot::PATH_NEXT;
    pub const ON_PATH: u8 = usg::Robot::ON_PATH;
    /// [`Movement::mode`]: recalled by the situation room (+0x23 bit 3).
    pub const RECALL: u8 = usg::Robot::RECALL;
    /// [`Movement::goal`]: heading for [`Movement::destination`].
    pub const GOAL_POINT: u8 = 0x22;

    fn of(r: &usg::Robot, status: u8) -> Movement {
        use usg::Parts;
        let slot = |s: u16| (s != 0xffff).then_some(EntityId(s));
        let guns = match &r.parts {
            Parts::Humanoid(p) => Guns {
                cooldown: p.cooldowns.map(u16::from),
                arms: [p.frames[1], p.frames[2]],
                raised: p.arms_raised,
                ..Guns::default()
            },
            Parts::Scorpion(p) => Guns {
                cooldown: [p.cooldown.into(), p.reload],
                rounds: p.rounds,
                missiles: p.missiles,
                burst: p.burst,
                fired: p.fired,
                missile: slot(p.missile),
                ..Guns::default()
            },
            Parts::Scout(p) => Guns {
                cooldown: p.cooldowns.map(u16::from),
                rounds: p.shells.into(),
                ..Guns::default()
            },
        };
        // (gear, stride, legs, stance and its target, relays) by kind; the frames start
        // at +0xfe.
        let frames = |f: &[u8]| -> [u8; 6] { f[1..7].try_into().expect("+0xff to +0x104") };
        let (gear, stride, legs, stance, relays) = match &r.parts {
            Parts::Humanoid(p) => {
                let [.., a, b] = p.frames;
                (p.gear, p.step, [0, 0, a, b, 0, 0], (0, 0), 0)
            }
            Parts::Scorpion(p) => (0, p.step, frames(&p.frames), (0, 0), p.relays),
            Parts::Scout(p) => (
                p.gear,
                p.step,
                frames(&p.frames),
                (p.stance, p.stance_target),
                0,
            ),
        };
        Movement {
            tick: r.tick,
            eye_height: r.eye_height,
            velocity: (r.velocity_x, r.velocity_y),
            mode: r.mode & !usg::Robot::PROGRAM,
            speed: r.speed,
            target_speed: r.target_speed,
            target_heading: r.target_heading,
            path: r.path,
            path_point: r.path_point,
            goal: r.goal,
            goal_object: r.goal_object,
            destination: Position {
                x: Fixed(r.destination.x),
                y: Fixed(r.destination.y),
            },
            drive: r.drive,
            gear,
            weapon: r.weapon,
            stance: stance.0,
            stance_target: stance.1,
            relays,
            stride,
            legs,
            status,
            contacts: r.contacts,
            target: None,
            target_distance: r.target_distance,
            target_bearing: r.target_bearing,
            locked_on: r.locked_on,
            guns: Guns {
                energy: r.energy,
                ..guns
            },
            grid: match &r.parts {
                Parts::Scorpion(p) => Grid::of(&p.grid.0),
                _ => Grid::default(),
            },
            // The rest is set when loading starts the program anew.
            script: Script {
                events: r.events,
                ..Script::default()
            },
            panel: Panel {
                flags: r.panel,
                length: r.panel_length,
                queue: r.panel_queue,
                text: r.panel_text.0,
            },
        }
    }
}

impl State {
    /// The movement state `object`'s layout records.
    pub fn of(object: &Object) -> State {
        match &object.body {
            Body::Robot(r) => State::Robot(Movement::of(r, object.status)),
            Body::Base(b) => State::Base {
                tick: b.tick,
                kills: b.kills,
                losses: b.losses,
                destroyed: object.flags & usg::flag::GONE != 0,
                end_step: b.end_step,
                look: b.look,
                counts: b.counts,
                depot: Depot::of(b),
            },
            _ => State::None,
        }
    }
}

impl StateHash for State {
    fn state_hash(&self, h: &mut StateHasher) {
        match *self {
            State::Robot(m) => {
                h.u8(0);
                h.bytes(&[m.tick, m.mode]);
                h.u16(m.eye_height);
                h.u16(m.velocity.0 as u16);
                h.u16(m.velocity.1 as u16);
                h.u16(m.speed as u16);
                h.u16(m.target_speed as u16);
                h.u16(m.target_heading);
                h.bytes(&[m.path, m.path_point, m.goal]);
                h.u16(m.goal_object);
                h.i32(m.destination.x.0);
                h.i32(m.destination.y.0);
                h.bytes(&[m.drive, m.gear, m.weapon, m.stance, m.stance_target]);
                h.u8(m.relays);
                h.u8(m.stride);
                h.bytes(&m.legs);
                h.bytes(&[m.status, m.contacts, m.locked_on]);
                h.u16(m.target.map_or(0xffff, |t| t.0));
                h.u16(m.target_distance);
                h.u16(m.target_bearing);
                let g = m.grid;
                h.u16(g.cell.0 as u16);
                h.u16(g.cell.1 as u16);
                h.u16(g.lengths.0);
                h.u16(g.lengths.1);
                h.u16(g.error as u16);
                h.u8(g.step);
                let g = m.guns;
                h.bytes(&[g.energy, g.missiles, g.burst, g.fired]);
                h.bytes(&[g.arms[0], g.arms[1], g.raised[0], g.raised[1]]);
                g.cooldown.iter().for_each(|&c| h.u16(c));
                h.u16(g.rounds);
                h.u16(g.missile.map_or(0xffff, |t| t.0));
                let s = m.script;
                h.u16(s.pointer);
                h.u16(s.wait);
                h.bytes(&[s.sp, s.flags]);
                h.bytes(&s.stack);
                h.u16(s.events);
                let p = m.panel;
                h.bytes(&[p.flags, p.length]);
                p.queue.iter().for_each(|&q| h.u16(q));
                h.bytes(&p.text);
            }
            State::Base {
                tick,
                kills,
                losses,
                destroyed,
                end_step,
                look,
                counts,
                depot: d,
            } => {
                h.u8(1);
                h.bytes(&[tick, kills, losses, u8::from(destroyed), look]);
                h.u16(end_step);
                h.bytes(&counts);
                h.bytes(&[d.relays, d.relay_clock, d.slider]);
                h.bytes(&d.power);
                h.u16(d.relay_wait);
                h.u16(d.repairing.map_or(0xffff, |r| r.0));
                h.u16(d.repair_clock);
            }
            State::Explosion {
                frame,
                last,
                ticks,
                countdown,
            } => {
                h.u8(3);
                h.bytes(&[frame, last, ticks, countdown]);
            }
            State::Shot(s) => {
                h.u8(4);
                h.u8(s.flight as u8);
                s.velocity.iter().for_each(|&v| h.u16(v as u16));
                for n in [s.speed, s.climb, s.age, s.arming, s.life, s.turn] {
                    h.u16(n);
                }
                h.bytes(&[s.damage, s.phase, s.frame]);
                h.u16(s.target.map_or(0xffff, |t| t.0));
            }
            State::None => h.u8(2),
        }
    }
}

/// What an object has taken (`docs/formats/usg.md`, "How robots take damage").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Damage {
    /// A robot has no hit points: hits wear its components down.
    Robot(Components),
    /// A base (destroyed at 300) or a relay (removed at 20): the damage so far.
    Taken(u16),
    /// Obstacles take none, and objects without a known layout have none recorded.
    #[default]
    None,
}

/// A robot's components (`usg::Robot`): two percentages and the damage bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Components {
    /// +0x24, +0x4e: 0 to 100.
    pub helm: u8,
    pub power: u8,
    /// +0xdc: weapons damaged.
    pub weapons: u8,
    /// +0xec: bit 0, the targeting computer is damaged.
    pub targeting: u8,
    /// +0xf6: cameras, map, radar and sensors damaged, weapons destroyed.
    pub systems: u8,
    /// +0xfd: bit 7, a scorpion's relay deployer damaged.
    pub equipment: u8,
    /// +0xf9: the damage added up at the end of every tick (`damage::total`).
    pub total: u8,
}

impl Components {
    /// The component byte at `offset` in the object, if the sim keeps it.
    pub fn byte(&mut self, offset: u16) -> Option<&mut u8> {
        match offset {
            0x24 => Some(&mut self.helm),
            0x4e => Some(&mut self.power),
            0xdc => Some(&mut self.weapons),
            0xec => Some(&mut self.targeting),
            0xf6 => Some(&mut self.systems),
            0xfd => Some(&mut self.equipment),
            _ => None,
        }
    }
}

impl Damage {
    /// The damage recorded in `object`'s layout.
    pub fn of(object: &Object) -> Damage {
        match &object.body {
            Body::Robot(r) => Damage::Robot(Components {
                helm: r.helm,
                power: r.power,
                weapons: r.weapons,
                targeting: r.targeting,
                systems: r.systems,
                equipment: r.equipment,
                total: r.damage,
            }),
            Body::Base(b) => Damage::Taken(b.damage),
            Body::Relay(r) => Damage::Taken(u16::from(r.damage)),
            Body::Other(_) => Damage::None,
        }
    }
}

impl StateHash for Damage {
    fn state_hash(&self, h: &mut StateHasher) {
        match *self {
            Damage::Robot(c) => {
                h.u8(0);
                h.bytes(&[c.helm, c.power, c.weapons, c.targeting, c.systems]);
                h.u8(c.equipment);
                h.u8(c.total);
            }
            Damage::Taken(n) => {
                h.u8(1);
                h.u16(n);
            }
            Damage::None => h.u8(2),
        }
    }
}

/// A side as a hashed byte.
pub(crate) fn side_code(side: Side) -> u8 {
    match side {
        Side::Human => 0,
        Side::Alien => 1,
    }
}

/// Every object, in slot order, the robots' orders, the terrain and both sides' paths.
/// A `BTreeMap`, so iteration
/// (and so the hash and any rule that walks the objects) never depends on insertion
/// order or a hasher's seed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct World {
    entities: BTreeMap<EntityId, Entity>,
    pub orders: OrderBook,
    pub terrain: Terrain,
    pub paths: Paths,
    /// Each side's unit list (the base's +0x4d), human then alien: its robots in the
    /// file's order, at most [`World::UNIT_LIST`].
    pub units: [Vec<EntityId>; 2],
    /// Each side's command object (`DS:0x568e`, `0x5690`): the last base loaded for it.
    pub bases: [Option<EntityId>; 2],
    /// `DS:0x4db5`: staggers the waits programs start with (`docs/sim.md`, "Orders").
    pub stagger: u8,
    /// `DS:0x62c6`: the object passes run, which the walking animations go by.
    pub ticks: u16,
    /// The mission clock (`DS:0x7d46`), from the scenario's `SHDR`.
    pub clock: Clock,
    /// `DS:0x7201`, `0x7203`: the arc and range the last lock's upkeep used, which a
    /// new lock also works its bits out with (`docs/sim.md`, "Sensing").
    pub lock_window: (u16, u16),
    /// `DS:0x7205`, `0x7207`: the slot the A key's search goes on from, back to 0
    /// whenever a console opens (`1000:d805`); not saved (`docs/sim.md`, "Sensing").
    pub target_search: u16,
    /// `DS:0x4da4`: the point the scripts work out from a goal object (opcode 0x66),
    /// one for every robot.
    pub point: Position,
    /// The call signs taken (`DS:0x72c5`, bit n for sign n): cleared when a
    /// scenario's objects are read (`scen_read_dcbs`), so only robots made since hold
    /// one. shortcut: a destroyed robot does not give its sign back (`1000:dfc9`) until
    /// the sim keeps loaded robots' signs.
    pub call_signs: u64,
}

/// How many objects of each kind one side has. The units are the robots: the kinds a
/// side's unit list, roster and consoles step through (`docs/screens.md`, "Switching
/// views"), and the ones with orders. Bases, relays and obstacles are not units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    pub humanoids: usize,
    pub scorpions: usize,
    pub scouts: usize,
    pub bases: usize,
    pub relays: usize,
    pub obstacles: usize,
    /// Kinds the editor has no name for; none in the stock files.
    pub other: usize,
}

impl Census {
    /// The robots.
    pub fn units(&self) -> usize {
        self.humanoids + self.scorpions + self.scouts
    }

    fn add(&mut self, kind: ObjectKind) {
        let count = match kind {
            ObjectKind::Humanoid => &mut self.humanoids,
            ObjectKind::Scorpion => &mut self.scorpions,
            ObjectKind::Scout => &mut self.scouts,
            ObjectKind::Base => &mut self.bases,
            ObjectKind::Relay => &mut self.relays,
            ObjectKind::Obstacle => &mut self.obstacles,
            ObjectKind::Other(_) => &mut self.other,
        };
        *count += 1;
    }
}

impl World {
    /// The objects of `scenario` (its `DCBS` chunk), each in its slot with its kind,
    /// side, position, heading, radius and damage, the robots' orders as loading sets
    /// them, the terrain (`MAPP`) and the paths (`APTH`, `HPTH`) (robots without
    /// the stock layout have none; no stock robot lacks it). If two objects share a
    /// slot, the later one is kept; no stock file has that.
    /// A file with no `DCBS` chunk gives an empty world. The objects' other bytes are
    /// not copied: they stay in `scenario` until T-058 names them.
    pub fn from_scenario(scenario: &Scenario) -> World {
        let entities = scenario
            .objects
            .iter()
            .flatten()
            .map(|o| {
                let entity = Entity {
                    kind: o.kind,
                    side: o.side(),
                    position: Position {
                        x: Fixed(o.x),
                        y: Fixed(o.y),
                    },
                    z: Fixed(o.z),
                    model_height: o.model_height,
                    heading: o.heading,
                    radius: o.radius,
                    damage: Damage::of(o),
                    state: State::of(o),
                };
                (EntityId(o.slot), entity)
            })
            .collect();
        // The loader adds each robot to its side's list and makes each base its side's
        // command object, in file order (`scen_on_object_loaded`).
        let (mut units, mut bases) = (<[Vec<EntityId>; 2]>::default(), [None; 2]);
        for o in scenario.objects.iter().flatten() {
            let side = usize::from(side_code(o.side()));
            if o.kind == ObjectKind::Base {
                bases[side] = Some(EntityId(o.slot));
            } else if o.flags & usg::flag::UNIT != 0 && units[side].len() < World::UNIT_LIST {
                units[side].push(EntityId(o.slot));
            }
        }
        World {
            entities,
            orders: OrderBook::from_scenario(scenario),
            terrain: Terrain::from_scenario(scenario),
            paths: Paths::from_scenario(scenario),
            units,
            bases,
            // shortcut: the original carries the stagger from mission to mission
            // (from 0 at start-up); it only decides which robots start in step.
            stagger: 0,
            // shortcut: the original carries this count from mission to mission too;
            // only the settling legs read it (every 8 passes).
            ticks: 0,
            clock: Clock::from_start(&scenario.header.start),
            call_signs: 0,
            // shortcut: the original carries this from mission to mission (from the
            // image's value); a lock before any upkeep is rare and only its bits differ.
            lock_window: (0, 0),
            target_search: 0,
            point: Position::default(),
        }
    }

    /// The most robots a side's unit list holds.
    pub const UNIT_LIST: usize = 38;

    /// `side`'s unit list.
    pub fn units(&self, side: Side) -> &[EntityId] {
        &self.units[usize::from(side_code(side))]
    }

    /// `side`'s command object.
    pub fn base(&self, side: Side) -> Option<EntityId> {
        self.bases[usize::from(side_code(side))]
    }

    /// Playing a campaign scenario that was not loaded (`sim_apply_scenario_alert`):
    /// `damage`, the alert level's from the user's `xeno.exe`
    /// (`xeno_formats::campaign::Campaign::alert_damage`), is added to the player's
    /// base, the human base in the lowest slot (the stock files have one). Returns
    /// whether there was one to damage.
    pub fn apply_alert(&mut self, damage: u16) -> bool {
        let base = self
            .entities
            .values_mut()
            .find(|e| e.kind == ObjectKind::Base && e.side == Side::Human);
        match base {
            Some(Entity {
                damage: Damage::Taken(taken),
                ..
            }) => {
                *taken = taken.wrapping_add(damage);
                true
            }
            _ => false,
        }
    }

    /// How many objects of each kind `side` has.
    pub fn census(&self, side: Side) -> Census {
        let mut census = Census::default();
        self.entities
            .values()
            .filter(|e| e.side == side)
            .for_each(|e| census.add(e.kind));
        census
    }

    /// Puts `entity` in slot `id`, returning the one it replaces.
    pub fn insert(&mut self, id: EntityId, entity: Entity) -> Option<Entity> {
        self.entities.insert(id, entity)
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        self.entities.remove(&id)
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    /// Object `id`, to change, and the terrain it moves over.
    pub(crate) fn entity_and_terrain(&mut self, id: EntityId) -> Option<(&mut Entity, &Terrain)> {
        let entity = self.entities.get_mut(&id)?;
        Some((entity, &self.terrain))
    }

    /// The first object in slot `from` or above.
    pub fn next_from(&self, from: u16) -> Option<EntityId> {
        self.entities
            .range(EntityId(from)..)
            .next()
            .map(|(&id, _)| id)
    }

    /// The objects in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter().map(|(&id, e)| (id, e))
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

impl StateHash for World {
    fn state_hash(&self, h: &mut StateHasher) {
        h.u64(self.entities.len() as u64);
        for (id, e) in self.iter() {
            h.u16(id.0);
            h.u16(e.kind.into());
            h.u8(side_code(e.side));
            h.i32(e.position.x.0);
            h.i32(e.position.y.0);
            h.i32(e.z.0);
            h.u16(e.model_height);
            h.u16(e.heading);
            h.u16(e.radius);
            e.damage.state_hash(h);
            e.state.state_hash(h);
        }
        self.orders.state_hash(h);
        self.terrain.state_hash(h);
        self.paths.state_hash(h);
        for (units, base) in self.units.iter().zip(self.bases) {
            h.u64(units.len() as u64);
            units.iter().for_each(|id| h.u16(id.0));
            h.u16(base.map_or(0xffff, |id| id.0));
        }
        h.u8(self.stagger);
        h.u16(self.ticks);
        h.u64(self.call_signs);
        h.u16(self.lock_window.0);
        h.u16(self.lock_window.1);
        h.u16(self.target_search);
        h.i32(self.point.x.0);
        h.i32(self.point.y.0);
        let c = self.clock;
        for n in [c.hour, c.minute, c.second, c.tick] {
            h.u8(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn robot(x: i16, y: i16) -> Entity {
        Entity {
            kind: ObjectKind::Scout,
            side: Side::Human,
            position: Position {
                x: Fixed::from_int(x),
                y: Fixed::from_int(y),
            },
            z: Fixed(0),
            model_height: 0,
            heading: 0,
            radius: 0xa00,
            damage: Damage::None,
            state: State::None,
        }
    }

    fn hash(world: &World) -> u64 {
        let mut h = StateHasher::default();
        world.state_hash(&mut h);
        h.finish()
    }

    #[test]
    fn objects_are_kept_by_slot_whatever_the_order_they_came_in() {
        let (mut a, mut b) = (World::default(), World::default());
        for (slot, x) in [(7, 1), (2, 2), (200, 3)] {
            a.insert(EntityId(slot), robot(x, 0));
        }
        for (slot, x) in [(200, 3), (7, 1), (2, 2)] {
            b.insert(EntityId(slot), robot(x, 0));
        }
        assert_eq!(a, b);
        assert_eq!(hash(&a), hash(&b));
        let slots: Vec<_> = a.iter().map(|(id, _)| id.0).collect();
        assert_eq!(slots, [2, 7, 200]);
        // A move, a new side or a removal changes the hash.
        b.get_mut(EntityId(7)).unwrap().position.x.0 += 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().position.x.0 -= 1;
        b.get_mut(EntityId(2)).unwrap().side = Side::Alien;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(2)).unwrap().side = Side::Human;
        assert_eq!(hash(&a), hash(&b));
        // So do a height, a model height, a turn and damage.
        b.get_mut(EntityId(7)).unwrap().z.0 += 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().z.0 -= 1;
        b.get_mut(EntityId(7)).unwrap().model_height = 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().model_height = 0;
        b.get_mut(EntityId(7)).unwrap().heading = 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().heading = 0;
        b.get_mut(EntityId(7)).unwrap().damage = Damage::Taken(1);
        assert_ne!(hash(&a), hash(&b));
        assert_eq!(a.remove(EntityId(2)).map(|e| e.side), Some(Side::Human));
        assert_eq!((a.len(), a.get(EntityId(2))), (2, None));
    }

    #[test]
    fn fixed_point_is_16_16() {
        assert_eq!(Fixed::from_int(3), Fixed(0x3_0000));
        assert_eq!(Fixed(0x3_8000).floor(), 3);
        assert_eq!(Fixed(-0x8000).floor(), -1);
        assert_eq!(Fixed::ONE, Fixed::from_int(1));
    }

    #[test]
    fn a_scenario_s_objects_and_orders_fill_the_world() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let world = World::from_scenario(&scenario);
        // A human base (slot 0), humanoid (2), relay (3) and scorpion (5); an alien
        // scout (7), at x 100 + slot, y 130.
        let slots: Vec<_> = world.iter().map(|(id, _)| id.0).collect();
        assert_eq!(slots, [0, 2, 3, 5, 7]);
        let scout = world.get(EntityId(7)).unwrap();
        assert_eq!((scout.kind, scout.side), (ObjectKind::Scout, Side::Alien));
        assert_eq!(scout.position.x, Fixed::from_int(107));
        assert_eq!(scout.position.y, Fixed::from_int(130));
        // Headings, radii and damage come from each kind's layout.
        assert_eq!(scout.heading, 0x7000);
        assert_eq!(scout.radius, scenario.objects.as_ref().unwrap()[4].radius);
        let Damage::Robot(parts) = scout.damage else {
            panic!("{:?}", scout.damage)
        };
        assert_eq!((parts.helm, parts.power), (90, 80));
        assert_eq!(world.get(EntityId(0)).unwrap().damage, Damage::Taken(20));
        assert_eq!(world.get(EntityId(3)).unwrap().damage, Damage::Taken(3));
        // The alert damages the player's base and nothing else.
        let mut alerted = world.clone();
        assert!(alerted.apply_alert(50));
        assert_eq!(alerted.get(EntityId(0)).unwrap().damage, Damage::Taken(70));
        alerted.insert(EntityId(0), world.get(EntityId(0)).unwrap().clone());
        assert_eq!(alerted, world);
        assert!(!World::default().apply_alert(50));
        let human = Census {
            humanoids: 1,
            scorpions: 1,
            bases: 1,
            relays: 1,
            ..Census::default()
        };
        assert_eq!(world.census(Side::Human), human);
        assert_eq!(world.census(Side::Alien).units(), 1);
        assert_eq!(world.census(Side::Human).units(), 2);
        // The robots' orders come with them, and only the robots have orders.
        assert_eq!(world.orders, OrderBook::from_scenario(&scenario));
        assert_eq!(world.orders.side(Side::Human).count(), 2);
        // So do the terrain and the paths, both in the hash.
        assert_eq!(world.terrain, Terrain::from_scenario(&scenario));
        assert_eq!(world.paths, Paths::from_scenario(&scenario));
        let mut moved = world.clone();
        moved.terrain.features[0].height += 1;
        assert_ne!(hash(&moved), hash(&world));
        let mut moved = world.clone();
        moved.paths.alien.slots[0].points[0].x.0 += 1;
        assert_ne!(hash(&moved), hash(&world));
        // The scorpion's line of net cells is hashed.
        let mut moved = world.clone();
        let State::Robot(m) = &mut moved.get_mut(EntityId(5)).unwrap().state else {
            panic!("the scorpion moves")
        };
        m.grid.step ^= 0x40;
        assert_ne!(hash(&moved), hash(&world));
        // So is the eye height.
        let mut moved = world.clone();
        let State::Robot(m) = &mut moved.get_mut(EntityId(5)).unwrap().state else {
            panic!("the scorpion moves")
        };
        m.eye_height ^= 1;
        assert_ne!(hash(&moved), hash(&world));
        // Fractions are kept: the plain fixture's humanoid is at (95.5, 136.25).
        let plain = xeno_formats::usg::parse(&xeno_formats::fixtures::synthetic_usg()).unwrap();
        let world = World::from_scenario(&plain);
        let at = world.get(EntityId(0)).unwrap().position;
        assert_eq!((at.x, at.y), (Fixed(0x5f_8000), Fixed(0x88_4000)));
        // No objects chunk: an empty world.
        let empty = Scenario {
            objects: None,
            ..plain
        };
        assert!(World::from_scenario(&empty).is_empty());
    }

    /// Every stock scenario loads every object, and each side's units, bases and the
    /// rest are what the parser reports, within the original's limits
    /// (`docs/formats/usg.md`, `docs/screens.md`).
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_scenario_loads_with_its_units() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let names = (0..13)
            .map(|n| format!("scen{n:02}a.usg"))
            .chain(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
        for name in names {
            let file = std::fs::read(dir.join(&name)).unwrap();
            let scenario = xeno_formats::usg::parse(&file).unwrap();
            let objects = scenario.objects.as_deref().unwrap_or_default();
            let world = World::from_scenario(&scenario);
            assert_eq!(
                world.len(),
                objects.len(),
                "{name}: every object, one a slot"
            );
            // No stock scenario starts with a unit in its base (T-135).
            let in_base = |(_, e): (EntityId, &Entity)| crate::base::in_base(&e.damage);
            assert!(!world.iter().any(in_base), "{name}: a unit in the base");
            let (mut units, mut big, mut small) = (0, 0, 0);
            for side in [Side::Human, Side::Alien] {
                // The parser's own count of each kind on this side.
                let parsed = |kind: ObjectKind| {
                    objects
                        .iter()
                        .filter(|o| o.kind == kind && o.side() == side)
                        .count()
                };
                let census = world.census(side);
                let expected = Census {
                    humanoids: parsed(ObjectKind::Humanoid),
                    scorpions: parsed(ObjectKind::Scorpion),
                    scouts: parsed(ObjectKind::Scout),
                    bases: parsed(ObjectKind::Base),
                    relays: parsed(ObjectKind::Relay),
                    obstacles: parsed(ObjectKind::Obstacle),
                    other: 0,
                };
                assert_eq!(census, expected, "{name} {side:?}");
                // One base a side; every unit has orders; at most 38 units a side.
                assert_eq!(census.bases, 1, "{name} {side:?}");
                assert_eq!(world.orders.side(side).count(), census.units(), "{name}");
                assert!(census.units() <= 38, "{name} {side:?}");
                units += census.units();
                big += census.units() + census.bases;
                small += census.relays + census.obstacles;
            }
            // No object starts inside a hill, and every path in use has points.
            for (_, e) in world.iter() {
                assert!(world.terrain.hill_at(e.position).is_none(), "{name}");
            }
            for side in [Side::Human, Side::Alien] {
                assert!(world.paths.side(side).iter().all(|p| !p.points.is_empty()));
            }
            // The 0x172-byte pool holds 40 objects, the 0x34-byte one 200.
            assert!(big <= 40 && small <= 200, "{name}");
            assert!(units > 0, "{name}: no units");
        }
    }
}
