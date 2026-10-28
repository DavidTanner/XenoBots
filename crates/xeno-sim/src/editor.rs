//! The scenario editor's placing (`ui_editor_place_at_pointer`, `1000:6be8`;
//! `docs/screens.md`, "The scenario editor (T-048)"): a Ctrl+left press on the
//! situation room's map makes or moves an object at the point under the pointer.
//! [`Sim::place`] does it to the world, and hands back the object as the scenario
//! writer would save it, for the host's copy of the scenario.

use xeno_formats::editor::Editor;
use xeno_formats::orders::RobotKind;
use xeno_formats::usg::{Body, Object, ObjectKind, Side, TerrainFeature, flag};

use crate::motion::Event;
use crate::orders::{Orders, Robot};
use crate::terrain::{Paths, Terrain};
use crate::world::side_code;
use crate::{Damage, Entity, EntityId, Position, Sim, State, World, damage, script};

/// What a press places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Kinds 0 to 2: a new robot of the side, facing `heading` (`1000:6c0b`).
    Robot {
        kind: ObjectKind,
        side: Side,
        heading: u16,
    },
    /// Kind 16: a new obstacle (`1000:6c33`).
    Obstacle,
    /// Kind 3: the side's base moves there; no base is made (`1000:6c49`).
    Base(Side),
}

/// What [`Sim::place`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// A new object, as the scenario writer saves it.
    Made(Box<Object>),
    /// The object in that slot moved to the point.
    Moved(EntityId),
}

/// An obstacle's radius when made (`1000:e9c3`), 8.8.
pub const OBSTACLE_RADIUS: u16 = 0x0d00;

impl Sim {
    /// Places `what` at `at`, a robot with the user's `editor` tables. `None` when
    /// nothing could be placed: no free slot, no base for the side, no free call sign,
    /// or no movement tables for a robot.
    pub fn place(&mut self, what: Place, at: Position, editor: &Editor) -> Option<Placed> {
        match what {
            Place::Robot {
                kind,
                side,
                heading,
            } => self.make_robot(kind, side, heading, at, editor),
            Place::Base(side) => {
                let id = self.world.base(side)?;
                self.world.get_mut(id)?.position = at;
                Some(Placed::Moved(id))
            }
            Place::Obstacle => {
                // `sim_alloc_object_slot` (`1000:f4b1`): the lowest free slot, the
                // object's bytes from +4 cleared; `1000:e9c3`: radius and `IN_FIELD`.
                let (slot, pool_index) = alloc(&self.world, false)?;
                self.world.insert(
                    EntityId(slot),
                    Entity {
                        kind: ObjectKind::Obstacle,
                        side: Side::Human,
                        position: at,
                        heading: 0,
                        radius: OBSTACLE_RADIUS,
                        damage: Damage::None,
                        state: State::None,
                    },
                );
                Some(Placed::Made(Box::new(Object {
                    size: Object::SMALL,
                    slot,
                    // The object table's reference count, 1 from the allocation.
                    ref_count: 1,
                    kind: ObjectKind::Obstacle,
                    pool_index,
                    x: at.x.0,
                    y: at.y.0,
                    z: 0,
                    heading: 0,
                    model_height: 0,
                    radius: OBSTACLE_RADIUS,
                    flags: flag::IN_FIELD,
                    status: 0,
                    body: Body::Other(vec![0; usize::from(Object::SMALL) - Object::HEADER_SIZE]),
                })))
            }
        }
    }
}

/// A robot's bytes when made (`1000:7e99`): `sim_init_new_object` (`1000:f812`) and
/// its kind's routine (`DS:0xe73f`); the program, call sign and serial are written
/// by [`Sim::make_robot`].
const MADE: [(usize, u8); 9] = [
    (0x24, 100),  // the helm (`1000:d2b6`)
    (0x23, 0x80), // the program drives
    (0x4e, 100),  // power (`1000:d17d`)
    (0x4f, 0xff), // signal, energy, drive
    (0x50, 0xff),
    (0x51, 0xff),
    (0x52, 1),    // the message panel (`1000:d1cd`)
    (0xf2, 2),    // the second display's map zoom (`1000:d9ab`)
    (0xde, 0xff), // ECM charges (each kind's routine)
];

/// By kind, the routine's other bytes (`1000:ab8a`, `9293`, `c40e`): the radius, the
/// model's and the eye's heights (words), then bytes.
type KindBytes = ([(usize, u16); 3], &'static [(usize, u8)]);
const KINDS: [KindBytes; 3] = [
    (
        [(0x14, 0x0a00), (0x12, 0x0a00), (0x1c, 0x0f80)],
        &[(0x10b, 4), (0xe0, 10)],
    ),
    (
        [(0x14, 0x1200), (0x12, 0x0a80), (0x1c, 0x0b80)],
        &[
            (0x120, 0xd0), // 2000 cannon rounds, a word
            (0x121, 0x07),
            (0x122, 6),
            (0xe4, 8),
            (0xe3, 8),
            (0xe2, 20),
            (0xe5, 4),
            (0x123, 0xff), // a word
            (0x124, 0xff),
            (0x118, 12),
        ],
    ),
    (
        [(0x14, 0x0a00), (0x12, 0x0c80), (0x1c, 0x0a00)],
        &[
            (0x107, 0x50),
            (0xe0, 4),
            (0xe2, 6),
            (0xe4, 2),
            (0xe3, 2),
            (0xe5, 6),
            (0x109, 4),
            (0x10b, 0x2f),
            (0x10c, 0x30),
        ],
    ),
];

/// The program a new robot is given (`1000:d2b6`).
const NEW_PROGRAM: u8 = 9;
/// A saved script pointer's mark (`1000:4826`), and a saved "no target".
const SAVED_SCRIPT: u16 = 0xa000;
const NO_TARGET: u16 = 0xffff;

impl Sim {
    fn make_robot(
        &mut self,
        kind: ObjectKind,
        side: Side,
        heading: u16,
        at: Position,
        editor: &Editor,
    ) -> Option<Placed> {
        let k = RobotKind::of(kind)?;
        let tables = self.tables.as_ref()?;
        let (slot, pool_index) = alloc(&self.world, true)?;
        let mut b = vec![0u8; usize::from(Object::BIG)];
        let word =
            |b: &mut [u8], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
        word(&mut b, 0, kind.into());
        word(&mut b, 2, pool_index);
        b[0x16] = (side_code(side) * flag::ALIEN) | 0x66;
        for &(at, v) in &MADE {
            b[at] = v;
        }
        // `1000:dfaf`: from a random number's low byte, the next free of 64 signs.
        let start = (self.random.word() as u8).wrapping_add(1) & 0x3f;
        let sign = (0..64u8)
            .map(|i| (start + i) & 0x3f)
            .find(|&c| self.world.call_signs & 1 << c == 0)?;
        self.world.call_signs |= 1 << sign;
        b[0xdb] = sign;
        let (words, bytes) = KINDS[k.index()];
        word(&mut b, 0x18, self.random.word() & 0x1ff);
        for (at, v) in words {
            word(&mut b, at, v);
        }
        for &(at, v) in bytes {
            b[at] = v;
        }
        let (offset, parts) = &editor.made_parts[k.index()];
        let offset = usize::from(*offset);
        b[offset..offset + parts.len()].copy_from_slice(parts);
        b[4..8].copy_from_slice(&at.x.0.to_le_bytes());
        b[8..12].copy_from_slice(&at.y.0.to_le_bytes());
        word(&mut b, 0x10, heading);
        word(&mut b, 0x2a, heading);
        let record = |b: &[u8]| {
            let mut r = [Object::BIG, slot, 1].map(u16::to_le_bytes).concat();
            r.extend_from_slice(b);
            Object::from_record(&r).ok()
        };
        let o = record(&b)?;
        let id = EntityId(slot);
        self.world.insert(
            id,
            Entity {
                kind,
                side,
                position: at,
                heading,
                radius: o.radius,
                damage: Damage::of(&o),
                state: State::of(&o),
            },
        );
        let orders = Orders {
            program_drives: true,
            ..Orders::given(NEW_PROGRAM)
        };
        self.world.orders.insert(
            slot,
            Robot {
                kind: k,
                side,
                orders,
            },
        );
        script::set_program(&mut self.world, id, NEW_PROGRAM, tables, &mut Vec::new());
        // `sim_add_unit_to_side_list`: the first free entry of the side's 38.
        let units = &mut self.world.units[usize::from(side_code(side))];
        if units.len() < World::UNIT_LIST {
            units.push(id);
        }
        // The save's form (`scen_pack_all_objects`): the script from the program
        // table with 0xa000, no target as 0xffff.
        let program = self
            .world
            .orders
            .get(slot)
            .map_or(NEW_PROGRAM, |r| r.orders.program);
        if let Some(State::Robot(m)) = self.world.get(id).map(|e| e.state) {
            // `order_set_program`: given and running, the stack empty.
            (b[0x2e], b[0x30], b[0x31]) = (program, program, m.script.sp);
            word(
                &mut b,
                0x34,
                m.script.pointer.wrapping_sub(editor.program_table) | SAVED_SCRIPT,
            );
            word(&mut b, 0x38, m.script.wait);
        }
        word(&mut b, 0xe6, NO_TARGET);
        record(&b).map(|o| Placed::Made(Box::new(o)))
    }

    /// Ctrl+D (`1000:6ab7`): robot `id` is destroyed (`sim_destroy_robot`). Returns
    /// what it did; without tables or a robot, nothing.
    pub fn destroy_robot(&mut self, id: EntityId) -> Vec<Event> {
        let mut events = Vec::new();
        if let (Some(tables), Some(_)) = (&self.tables, self.world.orders.get(id.0)) {
            damage::destroy(&mut self.world, id, tables, &mut events);
        }
        events
    }

    /// The editor's path keys and points change a side's table in place
    /// (`DS:0x5cd1`); the host hands over the edited tables.
    pub fn set_paths(&mut self, paths: Paths) {
        self.world.paths = paths;
    }

    /// The terrain editor's Apply (`0x06b5`): the `MAPP` buffer is replaced by the
    /// records `world` (in the world, the origin added), which hills now stop robots by.
    pub fn set_terrain(&mut self, world: &[TerrainFeature]) {
        self.world.terrain = Terrain::of(world);
    }
}

/// `sim_alloc_object_slot` (`1000:f4b1`): the lowest free slot, and the entry in
/// the object's pool.
fn alloc(world: &World, big_pool: bool) -> Option<(u16, u16)> {
    let slot = (0..Object::SLOTS as u16).find(|&s| world.get(EntityId(s)).is_none())?;
    // shortcut: the pool's lowest free entry is taken as the count of the pool's
    // objects (true while none has been freed); only the save shows it. Upgrade if
    // the sim keeps the pools.
    let pool_index = world
        .iter()
        .filter(|(_, e)| big(e.kind) == big_pool)
        .count();
    Some((slot, pool_index as u16))
}

/// Whether `kind`'s objects come from the big pool (`DS:0xe7c7` bit 0): the robots
/// and bases.
fn big(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Humanoid | ObjectKind::Scorpion | ObjectKind::Scout | ObjectKind::Base
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fixed, Random, motion};
    use xeno_formats::fixtures::synthetic_mission_usg;
    use xeno_formats::usg;

    fn at(x: i16, y: i16) -> Position {
        Position {
            x: Fixed::from_int(x),
            y: Fixed::from_int(y),
        }
    }

    /// Made-up editor tables: two, one and three bytes of parts, the program table at
    /// the scripts' start.
    fn editor() -> Editor {
        Editor {
            keys: Vec::new(),
            kind_names: Vec::new(),
            side_names: [Vec::new(), Vec::new()],
            show_names: [Vec::new(), Vec::new()],
            mission_names: Vec::new(),
            mission_defaults: Vec::new(),
            mix_percent: Vec::new(),
            status: Vec::new(),
            made_parts: [
                (0x10c, vec![0x11, 0x12]),
                (0x127, vec![0x21]),
                (0x10d, vec![0x31, 0x32, 0x33]),
            ],
            program_table: motion::synthetic_tables().scripts.start,
        }
    }

    #[test]
    fn the_terrain_editor_s_apply_replaces_the_terrain() {
        let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let before = sim.state_hash();
        let hill = TerrainFeature {
            size: 24,
            kind: TerrainFeature::HILL,
            point_slots: 0x10,
            x: 40 << 16,
            y: 40 << 16,
            z: 0,
            radius: 0x300,
            diagonal: 0x200,
            height: 0x100,
            shape: vec![TerrainFeature::HILL, 0],
        };
        sim.set_terrain(std::slice::from_ref(&hill));
        assert!(sim.world.terrain.hill_at(at(41, 40)).is_some());
        assert_eq!(sim.world.terrain.features.len(), 1);
        assert_ne!(sim.state_hash(), before);
    }

    #[test]
    fn a_robot_is_made_as_the_editor_makes_one() {
        let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let mut random = sim.random.clone();
        let place = |sim: &mut Sim, kind, side| {
            let what = Place::Robot {
                kind,
                side,
                heading: 0x1234,
            };
            match sim.place(what, at(3, 4), &editor()) {
                Some(Placed::Made(o)) => o,
                other => panic!("{other:?}"),
            }
        };
        let o = place(&mut sim, ObjectKind::Scorpion, Side::Alien);
        // Slot 1; the big pool holds the base and three robots.
        assert_eq!((o.slot, o.pool_index, o.ref_count), (1, 4, 1));
        assert_eq!(
            (o.x, o.y, o.heading, o.radius),
            (3 << 16, 4 << 16, 0x1234, 0x1200)
        );
        assert_eq!((o.model_height, o.flags), (0x0a80, 0x6e));
        let r = o.robot().unwrap();
        // The call sign from the first word drawn, the serial from the second.
        let sign = (random.word() as u8).wrapping_add(1) & 0x3f;
        assert_eq!(r.call_sign, sign);
        assert_eq!(r.serial, random.word() & 0x1ff);
        assert_eq!(
            (r.helm, r.power, r.signal, r.energy, r.drive),
            (100, 100, 0xff, 0xff, 0xff)
        );
        // Program 9, the last the synthetic tables have.
        let last = motion::synthetic_tables().scripts.programs.len() as u8 - 1;
        assert_eq!(
            (r.program_given, r.program_running, r.stack_pointer),
            (9.min(last), 9.min(last), 10)
        );
        assert_eq!(
            (r.mode, r.target_heading, r.eye_height),
            (0x80, 0x1234, 0x0b80)
        );
        assert_eq!(
            (r.ecm, r.mines, r.alarm_buoys, r.ecm_buoys, r.decoys),
            (0xff, 20, 8, 8, 4)
        );
        assert_eq!(r.target, 0xffff);
        let pointer =
            sim.world
                .orders
                .get(1)
                .map(|_| match sim.world.get(EntityId(1)).unwrap().state {
                    State::Robot(m) => m.script.pointer,
                    _ => 0,
                });
        let table = motion::synthetic_tables().scripts.start;
        assert_eq!(
            Some(r.script),
            pointer.map(|p| p.wrapping_sub(table) | 0xa000)
        );
        // The scorpion's parts, and its rounds, missiles and relays.
        let record = |o: &Object| {
            let mut s = scenario.clone();
            s.objects = Some(vec![o.clone()]);
            usg::write(&s).unwrap()
        };
        let bytes = record(&o);
        let start = bytes.windows(4).position(|w| w == b"DCBS").unwrap() + 6 + 2 + 6;
        assert_eq!(bytes[start + 0x127], 0x21);
        assert_eq!(
            &bytes[start + 0x118..start + 0x125],
            &[12, 0, 0, 0, 0, 0, 0, 0, 0xd0, 7, 6, 0xff, 0xff]
        );
        // In the world: its side's unit, with orders, the sign taken.
        assert_eq!(sim.world.units(Side::Alien).last(), Some(&EntityId(1)));
        assert!(sim.world.orders.get(1).unwrap().orders.program_drives);
        assert_eq!(sim.world.call_signs, 1 << sign);
        let e = sim.world.get(EntityId(1)).unwrap();
        assert_eq!(
            (e.kind, e.side, e.heading),
            (ObjectKind::Scorpion, Side::Alien, 0x1234)
        );
        // The next robot takes slot 4 and another sign; a humanoid's and a scout's
        // own bytes.
        let h = place(&mut sim, ObjectKind::Humanoid, Side::Human);
        assert_eq!(h.slot, 4);
        assert_ne!(h.robot().unwrap().call_sign, sign);
        let r = h.robot().unwrap();
        assert_eq!(
            (h.radius, h.model_height, r.eye_height, r.cloaks),
            (0x0a00, 0x0a00, 0x0f80, 10)
        );
        assert_eq!(h.flags, 0x66);
        let s = place(&mut sim, ObjectKind::Scout, Side::Human);
        let r = s.robot().unwrap();
        assert_eq!(
            (s.model_height, r.eye_height, r.cloaks, r.mines),
            (0x0c80, 0x0a00, 4, 6)
        );
        let bytes = record(&s);
        assert_eq!(
            &bytes[start + 0x107..start + 0x110],
            &[0x50, 0, 4, 0, 0x2f, 0x30, 0x31, 0x32, 0x33]
        );
        // Ctrl+D: the scorpion is destroyed; a base is not a robot.
        assert!(!sim.destroy_robot(EntityId(1)).is_empty());
        assert!(sim.world.get(EntityId(1)).is_none());
        assert!(sim.destroy_robot(EntityId(0)).is_empty());
        assert!(sim.world.get(EntityId(0)).is_some());
        // Bases and obstacles are not robots.
        let what = Place::Robot {
            kind: ObjectKind::Base,
            side: Side::Human,
            heading: 0,
        };
        assert_eq!(sim.place(what, at(0, 0), &editor()), None);
    }

    #[test]
    fn an_obstacle_takes_the_lowest_free_slot_and_a_base_moves() {
        let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let before = sim.state_hash();
        // Slots 0, 2, 3, 5 and 7 are taken: the obstacle goes in 1.
        let Some(Placed::Made(o)) = sim.place(Place::Obstacle, at(40, -3), &editor()) else {
            panic!()
        };
        assert_eq!((o.slot, o.pool_index, o.x, o.y), (1, 1, 40 << 16, -3 << 16));
        assert_ne!(sim.state_hash(), before);
        let e = sim.world.get(EntityId(1)).unwrap();
        assert_eq!((e.kind, e.radius, e.position), (o.kind, 0xd00, at(40, -3)));
        // The written object reads back as itself.
        let mut back = scenario.clone();
        back.objects.as_mut().unwrap().push(*o.clone());
        let read = usg::parse(&usg::write(&back).unwrap()).unwrap();
        assert!(read.objects.unwrap().contains(&*o));
        // The next one goes in 4.
        let Some(Placed::Made(o)) = sim.place(Place::Obstacle, at(0, 0), &editor()) else {
            panic!()
        };
        assert_eq!((o.slot, o.pool_index), (4, 2));
        // The human base (slot 0) moves; the alien side has none.
        assert_eq!(
            sim.place(Place::Base(Side::Human), at(7, 8), &editor()),
            Some(Placed::Moved(EntityId(0)))
        );
        assert_eq!(sim.world.get(EntityId(0)).unwrap().position, at(7, 8));
        assert_eq!(
            sim.place(Place::Base(Side::Alien), at(7, 8), &editor()),
            None
        );
    }
}
