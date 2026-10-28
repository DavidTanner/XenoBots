//! `.usg` scenario files: the campaign scenarios (`scenNNa`) and the four training
//! missions. A file is a chain of tagged chunks: the start clock, terrain features,
//! the objects (robots, bases, relays, obstacles), each side's paths, the briefing text
//! and the sky picture. There is no map grid. See `docs/formats/usg.md`.

use std::fmt;

use binrw::{BinRead, BinWrite, binrw};

use crate::FormatError;
use crate::scendef::{self, ScenarioTable};

/// One chunk as stored: a four-letter tag, a u16 size and that many bytes.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub tag: [u8; 4],
    #[bw(try_calc = u16::try_from(data.len()))]
    size: u16,
    #[br(count = size)]
    pub data: Vec<u8>,
}

/// `SHDR`, always the first chunk: the format version and the in-game clock at the
/// start of the mission.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// [`Header::VERSION`] in every stock file. With any other value the game reads
    /// only the first 0x16 bytes of each object and skips the path chunks.
    pub version: u16,
    pub start: Clock,
    /// 0xa0 in every stock file.
    pub unknown_08: u16,
    /// Zero in every stock file.
    pub unknown_0a: [u8; 6],
}

impl Header {
    /// The only version this parser reads (the game's "full" layout).
    pub const VERSION: u16 = 0x80;
    pub const SIZE: usize = 16;
}

/// The game clock (`xeno.exe` `DS:0x7d46`), which the mission loop advances.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Sixtieths of a second.
    pub tick: u8,
    /// Day of the month, from 0. The loader puts the campaign's date back after
    /// reading the file, so `day` and `month` here do not take effect.
    pub day: u8,
    /// 0 is January.
    pub month: u8,
}

/// What an object is. The numbers are the game's; the names are the ones its built-in
/// scenario editor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Humanoid,
    Scorpion,
    Scout,
    Base,
    Relay,
    Obstacle,
    Other(u16),
}

impl From<u16> for ObjectKind {
    fn from(id: u16) -> Self {
        match id {
            0 => Self::Humanoid,
            1 => Self::Scorpion,
            2 => Self::Scout,
            3 => Self::Base,
            4 => Self::Relay,
            16 => Self::Obstacle,
            other => Self::Other(other),
        }
    }
}

impl From<ObjectKind> for u16 {
    fn from(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Humanoid => 0,
            ObjectKind::Scorpion => 1,
            ObjectKind::Scout => 2,
            ObjectKind::Base => 3,
            ObjectKind::Relay => 4,
            ObjectKind::Obstacle => 16,
            ObjectKind::Other(id) => id,
        }
    }
}

/// Which side an object belongs to; the player is [`Side::Human`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Human,
    Alien,
}

/// Bytes whose meaning is not known yet, kept as read. Debug output shows them as one
/// hex string, so snapshots of the 0x172-byte objects stay readable.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Unknown<const N: usize>(pub [u8; N]);

impl<const N: usize> BinRead for Unknown<N> {
    type Args<'a> = ();

    fn read_options<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        endian: binrw::Endian,
        (): Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        <[u8; N]>::read_options(reader, endian, ()).map(Self)
    }
}

impl<const N: usize> BinWrite for Unknown<N> {
    type Args<'a> = ();

    fn write_options<W: std::io::Write + std::io::Seek>(
        &self,
        writer: &mut W,
        endian: binrw::Endian,
        (): Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        self.0.write_options(writer, endian, ())
    }
}

impl<const N: usize> fmt::Debug for Unknown<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// The bits of an object's flags byte (+0x16), as `xeno.exe` uses them.
pub mod flag {
    /// Gone: destroyed or removed. A destroyed base keeps its slot while the mission's
    /// end runs; other objects leave the object table.
    pub const GONE: u8 = 0x01;
    /// A robot may take the object as its target. Robots, bases and relays.
    pub const TARGETABLE: u8 = 0x02;
    /// The scanners list the object. Robots and bases.
    pub const SCANNED: u8 = 0x04;
    /// The side: clear for human, set for alien.
    pub const ALIEN: u8 = 0x08;
    /// Relays only: picks the relay's colour in the 3D view, and the base's network
    /// check looks for it. Probably "linked"; not traced further.
    pub const RELAY_LINKED: u8 = 0x10;
    /// A unit (the robots): the loader adds it to its side's unit list and restarts
    /// its program.
    pub const UNIT: u8 = 0x20;
    /// In the field: shots and contact checks only see objects with this bit. A robot
    /// loses it inside its base and gets it back when launched. Robots, bases,
    /// obstacles and set-up relays.
    pub const IN_FIELD: u8 = 0x40;
}

/// One `DCBS` record: an object as the game keeps it in memory, behind a short header
/// that says where it goes. Offsets in field docs are in the in-memory object, which
/// starts with the kind. The first 0x18 bytes are the same for every kind; the rest is
/// the kind's [`Body`].
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Size of the object in memory. [`Object::BIG`] for robots and bases,
    /// [`Object::SMALL`] for relays and obstacles in every stock file.
    #[br(assert(usize::from(size) >= Object::HEADER_SIZE, "object of {size} bytes"))]
    pub size: u16,
    /// Index in the game's object table (240 entries).
    #[br(assert((slot as usize) < Object::SLOTS, "object slot {slot}"))]
    pub slot: u16,
    /// How many other objects refer to this one; the game drops the count when a
    /// referrer goes away.
    pub ref_count: u16,
    /// +0x00.
    #[br(map = |id: u16| ObjectKind::from(id))]
    #[bw(map = |k: &ObjectKind| u16::from(*k))]
    pub kind: ObjectKind,
    /// +0x02: the object's place in its pool when the file was saved. The game
    /// overwrites the number it allocated with this, and stock files repeat some.
    pub pool_index: u16,
    /// +0x04: position, 16.16 fixed point.
    pub x: i32,
    /// +0x08.
    pub y: i32,
    /// +0x0c: height, 16.16, the third coordinate. Zero in every stock object; a base's
    /// is cleared at load, and the line-of-sight code lifts it by a height per kind.
    pub z: i32,
    /// +0x10: heading, 65,536 to a turn. The 3D view draws the model's facing from it
    /// (`docs/formats/mxx.md`); a robot turns it towards [`Robot::target_heading`].
    pub heading: u16,
    /// +0x12: set per kind when an object is made (8.8: humanoid 10, scorpion 10.5,
    /// scout 12.5; a relay's from a table by its state). Meaning unknown.
    pub unknown_12: u16,
    /// +0x14: radius, 8.8 fixed point, set per kind when an object is made. Contact
    /// checks subtract both objects' radii from their distance.
    pub radius: u16,
    /// +0x16: the [`flag`] bits.
    pub flags: u8,
    /// +0x17: more state bits; the scanners skip an object with bit 0 set, and a
    /// robot's targeting sets bit 6. Not traced.
    pub unknown_17: u8,
    /// +0x18 to the end.
    #[br(args(u16::from(kind), size))]
    pub body: Body,
}

impl Object {
    /// In-memory bytes before [`Object::body`].
    pub const HEADER_SIZE: usize = 0x18;
    /// The big pool's object size (robots and bases).
    pub const BIG: u16 = 0x172;
    /// The small pool's object size (relays, obstacles, shots and the like).
    pub const SMALL: u16 = 0x34;
    /// Entries in the game's object table.
    pub const SLOTS: usize = 240;

    /// Bit 3 of the flags: the scenario editor writes 0 for human, 8 for alien.
    pub fn side(&self) -> Side {
        if self.flags & flag::ALIEN == 0 {
            Side::Human
        } else {
            Side::Alien
        }
    }

    pub fn robot(&self) -> Option<&Robot> {
        match &self.body {
            Body::Robot(r) => Some(r),
            _ => None,
        }
    }

    pub fn base(&self) -> Option<&Base> {
        match &self.body {
            Body::Base(b) => Some(b),
            _ => None,
        }
    }

    pub fn relay(&self) -> Option<&Relay> {
        match &self.body {
            Body::Relay(r) => Some(r),
            _ => None,
        }
    }
}

/// An object's bytes from +0x18, by kind. A kind whose layout is not known, or an
/// object of an unexpected size, is kept as bytes.
#[binrw]
#[brw(little)]
#[br(import(kind: u16, size: u16))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    #[br(pre_assert(kind <= 2 && size == Object::BIG))]
    Robot(Robot),
    #[br(pre_assert(kind == 3 && size == Object::BIG))]
    Base(Base),
    #[br(pre_assert(kind == 4 && size == Object::SMALL))]
    Relay(Relay),
    /// Obstacles (whose only known fields are in the header), other kinds and sizes.
    Other(#[br(count = usize::from(size) - Object::HEADER_SIZE)] Vec<u8>),
}

/// A humanoid's, scorpion's or scout's bytes from +0x18 (0x172-byte objects). The
/// loader keeps them all, except that it restarts the program given (`program_given`)
/// and drops `target`.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Robot {
    pub unknown_18: Unknown<2>,
    /// +0x1a: counts the robot's ticks in twos; each value picks a routine from the
    /// kind's table, and a scorpion steps its speed when the low four bits are 0
    /// (`docs/sim.md`, "Movement").
    pub tick: u8,
    /// +0x1b: a state number, picking another routine each tick. Not traced.
    pub state: u8,
    pub unknown_1c: Unknown<2>,
    /// +0x1e: velocity, added to `x` every tick (1/65536ths).
    pub velocity_x: i16,
    /// +0x20: added to `y` every tick.
    pub velocity_y: i16,
    pub unknown_22: u8,
    /// +0x23: [`Robot::MANUAL`], [`Robot::RECALL`]; bits 4 and 5 turn the target
    /// heading one way or the other every tick (the pilot's turn keys).
    pub mode: u8,
    /// +0x24: the helm, 0 to 100 (100 in stock files); hits lower it.
    pub helm: u8,
    pub unknown_25: u8,
    /// +0x26: speed; it steps by one towards `target_speed` (limited by the drive,
    /// the helm and, for a scorpion, [`Robot::speed_table`]) every tick, or every 8
    /// ticks for a scorpion (`docs/sim.md`, "Movement").
    pub speed: i16,
    /// +0x28: the speed asked for.
    pub target_speed: i16,
    /// +0x2a: the heading asked for; every tick `heading` turns towards it by at most
    /// the kind's turn step. The editor sets both to the same value.
    pub target_heading: u16,
    /// +0x2c: the robot's path: the slot in its side's [`PathTable`] (bits 0 to 3),
    /// [`Robot::PATH_BACKWARDS`], [`Robot::PATH_NEXT`] and [`Robot::ON_PATH`]; bit 4
    /// is set by one of the script's path commands (not traced).
    pub path: u8,
    /// +0x2d: the point of the path the robot is heading for; 0xff is its own base,
    /// before the first point.
    pub path_point: u8,
    /// +0x2e: the program given, a number in `xeno.exe`'s program table.
    pub program_given: u8,
    pub unknown_2f: u8,
    /// +0x30: the program running: the one given, or a state the script entered.
    pub program_running: u8,
    /// +0x31: the script's timing (10 when a program starts).
    pub unknown_31: u8,
    /// +0x32: what the robot heads for: 0x22 the point at `destination`; bit 0 an
    /// object (its slot at +0x3a); 0 nothing.
    pub goal: u8,
    /// +0x33: more of the script's timing.
    pub unknown_33: u8,
    /// +0x34: the script pointer, saved as `0xa000` + its offset from the program
    /// table.
    pub script: u16,
    /// +0x36: event bits (the message panel's), +0x38: the script's start phase.
    pub unknown_36: Unknown<6>,
    /// +0x3c: where the robot is heading, 16.16; a path point or its base.
    pub destination: Point,
    pub unknown_44: Unknown<0xa>,
    /// +0x4e: power systems, 0 to 100; hits lower it.
    pub power: u8,
    pub unknown_4f: Unknown<2>,
    /// +0x51: the drive, 0 to 0xff (0xff in nearly every stock robot): at 0 the robot cannot
    /// move, at or under a threshold in `xeno.exe` it goes at half speed. The name is
    /// a guess from that use.
    pub drive: u8,
    pub unknown_52: Unknown<0x89>,
    /// +0xdb: the robot's call sign, an index into a 64-entry table of names in
    /// `xeno.exe` (`xeno_formats::sitroom`); a new robot draws a free one at random.
    pub call_sign: u8,
    /// +0xdc: weapons damaged, bits 0 to 2 (the kind's first, second and third
    /// weapon). A second hit on a damaged weapon destroys it ([`Robot::systems`]).
    pub weapons: u8,
    pub unknown_dd: Unknown<9>,
    /// +0xe6: the robot's target, saved as its object slot (0xffff: none). The loader
    /// clears it.
    pub target: u16,
    pub unknown_e8: Unknown<4>,
    /// +0xec: bit 0, the targeting computer is damaged.
    pub targeting: u8,
    pub unknown_ed: Unknown<9>,
    /// +0xf6: systems damaged or destroyed: bits 0 visual camera, 1 thermal camera,
    /// 2 map, 3 radar, 4 EMI sensors, 6 and 7 the first and second weapon destroyed.
    pub systems: u8,
    pub unknown_f7: Unknown<0x14>,
    /// +0x10b: a scout's stance (0x30 standing; it moves only standing) and a
    /// humanoid's speed gear (an index into its cockpit's speeds).
    pub stance: u8,
    /// +0x10c: the stance a scout is rising or sinking to; the humanoid console's
    /// widget state starts here (+0x10c humanoid, +0x10d scout, +0x127 scorpion).
    pub stance_target: u8,
    pub unknown_10d: Unknown<0x0b>,
    /// +0x118: which of `xeno.exe`'s speed tables a scorpion's speed goes through.
    pub speed_table: u8,
    pub unknown_119: Unknown<0x59>,
}

impl Robot {
    /// [`Robot::mode`]: named from T-043 as "the pilot drives; the program waits",
    /// but T-052 found the reverse: the script steps every tick only with it set, and
    /// starting a program sets it while the drive keys clear it (`docs/sim.md`,
    /// "Movement"). T-111 renames it.
    pub const MANUAL: u8 = 0x80;
    /// [`Robot::mode`]: the situation room recalled the robot; its next tick gives it
    /// program 4 and clears the bit.
    pub const RECALL: u8 = 0x08;
    /// [`Robot::path`]: walking the path from its last point back to its first.
    pub const PATH_BACKWARDS: u8 = 0x20;
    /// [`Robot::path`]: the script asked for the next point.
    pub const PATH_NEXT: u8 = 0x40;
    /// [`Robot::path`]: the robot has a path.
    pub const ON_PATH: u8 = 0x80;

    /// +0xf9 (in `unknown_f7`): its repair state; the roster's status icon is bits 4
    /// to 6, and the situation room's detail panel says `IN REPAIR`, `WAIT REPAIR` or
    /// `READY` from it. Not traced further.
    pub fn repair(&self) -> u8 {
        self.unknown_f7.0[2]
    }
}

/// A base's bytes from +0x18 (0x172-byte objects). The loader clears the height and
/// the words at +0x42..=+0x4b, and makes the base its side's command object.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub unknown_18: Unknown<0x13>,
    /// +0x2b: the side's robots lost this mission.
    pub losses: u8,
    /// +0x2c: enemy robots destroyed this mission.
    pub kills: u8,
    pub unknown_2d: u8,
    /// +0x2e: damage taken. At 300 the base is destroyed ([`flag::GONE`]). Playing a
    /// campaign scenario adds 0, 50, 100 or 150 to the player's by its alert level.
    pub damage: u16,
    /// +0x30: once destroyed, the step of the mission's end; at 0x24 the mission ends,
    /// won if this is the alien base.
    pub end_step: u16,
    /// +0x32: what the situation room's map shows ([`Base::MAP_ROBOTS`] and the other
    /// `MAP_` bits); bit 14 asks the status view to redraw.
    pub map_overlays: u16,
    /// +0x34: how much of the briefing the orders view has typed.
    pub typed: u16,
    /// +0x36: the map's zoom, 0 to 6: it scales by `1 << zoom`.
    pub map_zoom: u8,
    /// +0x37: the situation room's main view mode (0, 2, 4, 6, 8: O, S, M, R, C).
    pub view_mode: u8,
    pub unknown_38: Unknown<9>,
    /// +0x41: counts the base's ticks; its low four bits pick the tick's routine
    /// (the alien base gives idle robots paths when they are 2).
    pub tick: u8,
    /// +0x42: the roster's rows (cleared at load), its scroll and the side's unit list.
    pub unknown_42: Unknown<0xd>,
    /// +0x4f: the map's centre while [`Base::MAP_FREE`] is set.
    pub map_centre: Point,
    pub unknown_57: Unknown<2>,
    /// +0x59: the units' message log the situation room's mode C shows; the first
    /// slot with no text ends it.
    pub messages: [Message; 7],
    pub unknown_b4: Unknown<9>,
    /// +0xbd: the situation room's widget state.
    pub view_state: Unknown<0x43>,
    pub unknown_100: Unknown<0x72>,
}

impl Base {
    /// [`Base::map_overlays`] bits (`docs/screens.md`, "The situation room's main
    /// view"), as the map controls label them: the robots (UNITS), drawn as sprites
    /// rather than dots (the icon button), MINES, the relays (NET), the GRID, the
    /// editor's paths, and the centre at [`Base::map_centre`] rather than on the
    /// selected unit.
    pub const MAP_ROBOTS: u16 = 0x01;
    pub const MAP_SPRITES: u16 = 0x02;
    pub const MAP_MINES: u16 = 0x04;
    pub const MAP_RELAYS: u16 = 0x08;
    pub const MAP_GRID: u16 = 0x10;
    pub const MAP_PATHS: u16 = 0x20;
    pub const MAP_FREE: u16 = 0x8000;
}

/// One line of a base's message log (13 bytes).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message {
    /// The unit's name, NUL-terminated.
    pub name: [u8; 7],
    /// The mission clock when it was sent.
    pub hour: u8,
    pub minute: u8,
    pub unknown_09: [u8; 2],
    /// The text: an offset in `xeno.exe`'s string segment; 0 for an empty slot.
    pub text: u16,
}

/// A relay's bytes from +0x18 (0x34-byte objects).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    /// +0x18: the relay's set-up state (+0x1f, +0x20) and four words at +0x25, +0x29,
    /// +0x2d and +0x31 a new relay sets to 0xffff. Not traced.
    pub unknown_18: Unknown<0x1b>,
    /// +0x33: damage taken; at 20 the relay is removed.
    pub damage: u8,
}

/// A waypoint, 16.16 fixed point like object positions.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// One path slot of a [`PathTable`]: points a robot walks, from its base out and
/// back. See `docs/formats/usg.md`, "How paths are used".
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path {
    /// How many of `points` the path uses. The editor adds a point at the end and
    /// removes the last; a path with none is skipped.
    pub point_count: u8,
    /// What the path is for; see [`Path::mission_name`]. It picks the program a robot
    /// sent along the path runs.
    pub mission: u8,
    /// +0x02: zero in every stock path; no code read reads it.
    pub unknown_02: [u8; 2],
    /// +0x04: how often the alien base picks this path for an idle robot: each path
    /// gets `weight + 1` chances. Set from the mission's default by the editor.
    pub weight: u8,
    /// +0x05: the share of humanoids, scorpions and scouts the mission wants, in
    /// tenths (the defaults add up to 10). Set by the editor; only its display reads
    /// them.
    pub mix: [u8; 3],
    /// +0x08: the alien robots on the path, then +0x09 those of each kind (humanoid,
    /// scorpion, scout), counted by the game during play; stock files keep the counts
    /// from when they were saved, and human paths are never counted.
    pub on_path: u8,
    pub on_path_by_kind: [u8; 3],
    /// Slots past `point_count` often hold leftovers from editing.
    pub points: [Point; Path::MAX_POINTS],
}

impl Path {
    pub const MAX_POINTS: usize = 16;
    pub const SIZE: usize = 12 + 8 * Self::MAX_POINTS;
    /// The names the debug display gives `mission` values (`xeno.exe` `DS:0x5cf5`).
    pub const MISSION_NAMES: [&'static str; 8] = [
        "NET DENSE",
        "NET SPARSE",
        "MAINTAIN",
        "RECON",
        "PATROL",
        "SKIRMISH",
        "WATCH",
        "----------",
    ];

    pub fn mission_name(&self) -> Option<&'static str> {
        Self::MISSION_NAMES.get(usize::from(self.mission)).copied()
    }

    /// The points in use.
    pub fn waypoints(&self) -> &[Point] {
        &self.points[..usize::from(self.point_count).min(Self::MAX_POINTS)]
    }
}

/// `APTH` (alien) or `HPTH` (human): one side's paths, loaded as a block into
/// `DS:0x7372` or `DS:0x77da`.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTable {
    #[br(assert(usize::from(count) <= PathTable::SLOTS, "{count} paths"))]
    pub count: u16,
    /// Slots past `count` often hold leftovers from editing.
    pub paths: [Path; PathTable::SLOTS],
}

impl PathTable {
    pub const SLOTS: usize = 8;
    pub const SIZE: usize = 2 + Self::SLOTS * Path::SIZE;

    /// The paths in use.
    pub fn used(&self) -> &[Path] {
        &self.paths[..usize::from(self.count)]
    }

    /// A table with no paths and every slot zero.
    pub fn empty() -> PathTable {
        PathTable::read_le(&mut std::io::Cursor::new([0u8; PathTable::SIZE]))
            .expect("a zeroed table reads")
    }
}

/// One `MAPP` record: a piece of terrain drawn by one of `xeno.exe`'s shape programs,
/// placed at a point. See `docs/formats/usg.md`, "`MAPP`: terrain features".
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainFeature {
    /// Size of the record, including this field. 24 in every stock record.
    #[br(assert(usize::from(size) >= TerrainFeature::HEADER_SIZE, "terrain record of {size} bytes"))]
    pub size: u16,
    /// [`TerrainFeature::HILL`], [`TerrainFeature::BLOCK`] or
    /// [`TerrainFeature::OPEN_HILL`]. The robots' hill test and the ground height test
    /// this byte; the 3D view and the maps go by [`TerrainFeature::shape`] instead.
    pub kind: u8,
    /// +0x03: how many points the shape program works with (4 bytes of stack each):
    /// 16 for a hill, 8 for a block.
    pub point_slots: u8,
    /// +0x04: the centre, 16.16.
    pub x: i32,
    /// +0x08.
    pub y: i32,
    /// +0x0c: the height the shape is drawn from, 16.16.
    pub z: i32,
    /// +0x10: 8.8. A hill's radius: robots and the ground test it, and the shape's
    /// ring has its points this far out on the axes. A block's half width.
    pub radius: u16,
    /// +0x12: 8.8. A hill's ring points on the diagonals lie this far out on both
    /// axes (the editor writes two thirds of the radius). A block's half depth.
    pub diagonal: u16,
    /// +0x14: 8.8. A hill's height above `z`: inside the radius the ground is at
    /// `z + height`. A block's half height.
    pub height: u16,
    /// +0x16 to the end: the drawing program, op bytes ended by 0. Every stock record
    /// holds its `kind` (the op that runs the built-in shape) and the 0.
    #[br(count = usize::from(size) - TerrainFeature::HEADER_SIZE)]
    pub shape: Vec<u8>,
}

impl TerrainFeature {
    /// Bytes before [`TerrainFeature::shape`].
    pub const HEADER_SIZE: usize = 0x16;
    /// A box (`mapdvr.dvr` writes one, far outside the world, to close every save).
    /// Only shots hit it; robots ignore it.
    pub const BLOCK: u8 = 0x12;
    /// A hill: an octagonal mound that robots cannot enter and shots hit.
    pub const HILL: u8 = 0x14;
    /// Shots hit it, but robots drive through it. Drawn, in the 3D view and on the
    /// maps, by its shape program like any record (`default.map`'s one, the only one,
    /// runs the hill's). The map editor turns it into a hill.
    pub const OPEN_HILL: u8 = 0x16;

    /// The centre, 16.16.
    pub fn centre(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }
}

/// The `MAPP` chunk: records until one of size 0, then padding to the chunk's end
/// (the editor allocates whole 16-byte paragraphs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrain {
    pub features: Vec<TerrainFeature>,
    /// Bytes after the end marker; leftover memory in stock files.
    pub padding: Vec<u8>,
}

/// A decoded scenario. Chunks the file lacks are `None`; the game then keeps what it
/// had (or, for an empty `MAPP`, has no terrain features).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub header: Header,
    pub terrain: Option<Terrain>,
    pub objects: Option<Vec<Object>>,
    pub alien_paths: Option<PathTable>,
    pub human_paths: Option<PathTable>,
    /// `ORDR`: the briefing, see [`Scenario::briefing_lines`].
    pub briefing: Option<Vec<u8>>,
    /// `BGND`: the sky picture's file name, see [`Scenario::background_name`].
    pub background: Option<Vec<u8>>,
    /// `SDEF`: the campaign, in a save game only.
    pub save: Option<SaveState>,
    /// Chunks with other tags, which the game skips, in file order.
    pub other_chunks: Vec<Chunk>,
}

/// `SDEF`, a save game's campaign (`scen_write_sdef`, `1000:7666`): 30 bytes from
/// `xeno.exe` `DS:0x5f97`, then the game's copy of `scendef.bin` with each scenario's
/// `state`, in the whole paragraphs its buffer fills. The game reads it only while
/// loading a save (`scen_read_sdef`, `1000:76ab`). Scenario references are record ids
/// (twice the position), 0xffff for none.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveState {
    /// The map room's selection (`DS:0x5f97`).
    pub selected: u16,
    /// The scenario whose file is loaded (`DS:0x5f99`): this save's other chunks.
    pub loaded: u16,
    /// The player's progress (`DS:0x5f9b`), compared with each `unlock_level`.
    pub progress: u16,
    pub score: u16,
    pub wins: u16,
    pub losses: u16,
    /// Every mission's kills and own robots lost, added up.
    pub kills: u16,
    pub robots_lost: u16,
    /// `DS:0x5fa7`: a new campaign clears it; nothing else uses it.
    pub unknown_10: u16,
    #[bw(try_calc = u16::try_from(table.len()))]
    table_size: u16,
    /// The last campaign mission's result (`DS:0x5fab`: 0xff new, 0 none, 1 won, 2
    /// lost, 3 training).
    pub result: u8,
    /// Every scenario is won (`DS:0x5fac`).
    pub campaign_won: u8,
    /// `DS:0x5fad`..`0x5fb4`: nothing uses them.
    pub unknown_16: [u8; 8],
    /// The table's bytes: see [`SaveState::scenarios`].
    #[br(count = table_size)]
    pub table: Vec<u8>,
}

impl SaveState {
    /// Bytes before the table.
    pub const HEADER_SIZE: usize = 30;
    /// "None" in [`SaveState::selected`] and [`SaveState::loaded`].
    pub const NONE: u16 = 0xffff;

    /// The scenario table (the bytes after its end marker are the buffer's slack).
    pub fn scenarios(&self) -> Result<ScenarioTable, FormatError> {
        scendef::parse_records(&self.table).map(|(table, _)| table)
    }

    /// Stores `table` as the game does: its bytes, zeros to a whole paragraph.
    pub fn set_scenarios(&mut self, table: &ScenarioTable) {
        self.table = scendef::write(table);
        self.table
            .resize(self.table.len().next_multiple_of(TERRAIN_PARAGRAPH), 0);
    }
}

impl Scenario {
    /// The briefing's lines: ASCII up to the first NUL or DOS end-of-file byte
    /// (0x1a), split at CR LF.
    pub fn briefing_lines(&self) -> Vec<String> {
        let Some(text) = &self.briefing else {
            return Vec::new();
        };
        let end = text
            .iter()
            .position(|&b| b == 0 || b == 0x1a)
            .unwrap_or(text.len());
        let text = String::from_utf8_lossy(&text[..end]);
        let mut lines: Vec<String> = text.split("\r\n").map(str::to_owned).collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }

    /// The sky picture's name as the game asks for it, with the padding spaces of its
    /// 8.3 form removed (`SKY_B   .BIN` gives `SKY_B.BIN`).
    pub fn background_name(&self) -> Option<String> {
        let raw = self.background.as_ref()?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some(
            String::from_utf8_lossy(&raw[..end])
                .chars()
                .filter(|&c| c != ' ')
                .collect(),
        )
    }
}

/// Converts a 16.16 fixed-point coordinate to a float, for display.
pub fn fixed_to_f64(value: i32) -> f64 {
    f64::from(value) / 65536.0
}

fn invalid(offset: usize, reason: String) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason,
    }
}

/// Decodes a `.usg` file: `SHDR` first, then any chunks, then `TERM` at the end of
/// the file. Each known chunk may appear once.
pub fn parse(bytes: &[u8]) -> Result<Scenario, FormatError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut header = None;
    let mut terrain = None;
    let mut objects = None;
    let mut alien_paths = None;
    let mut human_paths = None;
    let mut briefing = None;
    let mut background = None;
    let mut save = None;
    let mut other_chunks = Vec::new();
    loop {
        let at = cursor.position() as usize;
        let chunk = Chunk::read(&mut cursor)?;
        let data_at = at + 6;
        let tag = chunk.tag;
        let name = String::from_utf8_lossy(&tag).into_owned();
        if header.is_none() && &tag != b"SHDR" {
            return Err(invalid(
                at,
                format!("first chunk is {name:?}, not \"SHDR\""),
            ));
        }
        let exact_size = |want: usize| {
            if chunk.data.len() == want {
                Ok(())
            } else {
                let found = chunk.data.len();
                Err(invalid(
                    at + 4,
                    format!("{name:?} chunk of {found} bytes, not {want}"),
                ))
            }
        };
        let once = |seen: bool| {
            if seen {
                Err(invalid(at, format!("second {name:?} chunk")))
            } else {
                Ok(())
            }
        };
        match &tag {
            b"SHDR" => {
                once(header.is_some())?;
                exact_size(Header::SIZE)?;
                let h = Header::read(&mut std::io::Cursor::new(&chunk.data))?;
                if h.version != Header::VERSION {
                    let reason = format!(
                        "version {:#x}; only {:#x} is known",
                        h.version,
                        Header::VERSION
                    );
                    return Err(invalid(data_at, reason));
                }
                header = Some(h);
            }
            b"TERM" => {
                let end = cursor.position() as usize;
                if end != bytes.len() {
                    let extra = bytes.len() - end;
                    return Err(invalid(end, format!("{extra} bytes after TERM")));
                }
                break;
            }
            b"MAPP" => {
                once(terrain.is_some())?;
                terrain = Some(parse_terrain(&chunk.data)?);
            }
            b"DCBS" => {
                once(objects.is_some())?;
                objects = Some(parse_objects(&chunk.data, data_at)?);
            }
            b"APTH" | b"HPTH" => {
                let slot = if &tag == b"APTH" {
                    &mut alien_paths
                } else {
                    &mut human_paths
                };
                once(slot.is_some())?;
                exact_size(PathTable::SIZE)?;
                *slot = Some(parse_paths(&chunk.data, data_at)?);
            }
            b"ORDR" => {
                once(briefing.is_some())?;
                briefing = Some(chunk.data);
            }
            b"BGND" => {
                once(background.is_some())?;
                background = Some(chunk.data);
            }
            b"SDEF" => {
                once(save.is_some())?;
                save = Some(parse_save(&chunk.data, data_at)?);
            }
            _ => other_chunks.push(chunk),
        }
    }
    Ok(Scenario {
        header: header.expect("TERM is only accepted after SHDR"),
        terrain,
        objects,
        alien_paths,
        human_paths,
        briefing,
        background,
        save,
        other_chunks,
    })
}

/// The game reads 30 bytes and then the table's size from them, not the chunk's: the
/// two must agree, and the table must decode.
fn parse_save(data: &[u8], data_at: usize) -> Result<SaveState, FormatError> {
    let save = SaveState::read(&mut std::io::Cursor::new(data))?;
    let want = SaveState::HEADER_SIZE + save.table.len();
    if data.len() != want {
        let reason = format!("\"SDEF\" chunk of {} bytes, not {want}", data.len());
        return Err(invalid(data_at, reason));
    }
    save.scenarios()?;
    Ok(save)
}

fn parse_paths(data: &[u8], data_at: usize) -> Result<PathTable, FormatError> {
    let table = PathTable::read(&mut std::io::Cursor::new(data))?;
    if let Some((i, p)) = table
        .used()
        .iter()
        .enumerate()
        .find(|(_, p)| usize::from(p.point_count) > Path::MAX_POINTS)
    {
        let reason = format!("path {i} has {} points", p.point_count);
        return Err(invalid(data_at + 2 + i * Path::SIZE, reason));
    }
    Ok(table)
}

pub(crate) fn parse_terrain(data: &[u8]) -> Result<Terrain, FormatError> {
    let mut cursor = std::io::Cursor::new(data);
    let mut features = Vec::new();
    if !data.is_empty() {
        while u16::read_le(&mut cursor)? != 0 {
            cursor.set_position(cursor.position() - 2);
            features.push(TerrainFeature::read(&mut cursor)?);
        }
    }
    let padding = data[cursor.position() as usize..].to_vec();
    Ok(Terrain { features, padding })
}

fn parse_objects(data: &[u8], data_at: usize) -> Result<Vec<Object>, FormatError> {
    let mut cursor = std::io::Cursor::new(data);
    let count = u16::read_le(&mut cursor)?;
    let objects = (0..count)
        .map(|_| Object::read(&mut cursor))
        .collect::<Result<Vec<_>, _>>()?;
    let end = cursor.position() as usize;
    if end != data.len() {
        return Err(invalid(
            data_at + end,
            format!("{} bytes after {count} objects", data.len() - end),
        ));
    }
    Ok(objects)
}

/// The size of the `BGND` chunk the game writes: its 64-byte sky name buffer.
pub const BACKGROUND_SIZE: usize = 64;
/// The `MAPP` chunk is the whole terrain buffer, which the game allocates in 16-byte
/// paragraphs.
pub const TERRAIN_PARAGRAPH: usize = 16;

/// The bytes of a `MAPP` chunk as the game writes its terrain buffer
/// (`scen_write_mapp`): the records, the 0 end word and the padding, filled with
/// zeros to a whole paragraph. No records and no padding is an empty chunk (no
/// buffer).
pub fn terrain_bytes(terrain: &Terrain) -> Vec<u8> {
    if terrain.features.is_empty() && terrain.padding.is_empty() {
        return Vec::new();
    }
    let mut out = std::io::Cursor::new(Vec::new());
    for feature in &terrain.features {
        feature.write(&mut out).expect("writing to memory");
    }
    let mut out = out.into_inner();
    out.extend([0, 0]);
    out.extend(&terrain.padding);
    out.resize(out.len().next_multiple_of(TERRAIN_PARAGRAPH), 0);
    out
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) -> Result<(), FormatError> {
    let size = u16::try_from(data.len()).map_err(|_| {
        let name = String::from_utf8_lossy(tag);
        invalid(out.len(), format!("{name:?} chunk of {} bytes", data.len()))
    })?;
    out.extend(tag);
    out.extend(size.to_le_bytes());
    out.extend(data);
    Ok(())
}

fn binary(value: &impl for<'a> BinWrite<Args<'a> = ()>) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    value.write_le(&mut out).expect("writing to memory");
    out.into_inner()
}

/// Encodes `scenario` as the game's scenario writer does (`file_write_scenario`,
/// `1000:efde`, behind the situation room's Ctrl+Alt+P): `SHDR`, `MAPP`, `DCBS`,
/// `APTH`, `HPTH`, `ORDR`, `BGND`, `TERM`, every one always. A save game
/// (`file_write_save_game`) writes [`Scenario::save`] as an `SDEF` chunk after
/// `SHDR`. Unknown chunks are dropped, as the game never writes any.
///
/// - The objects go in slot order, the order the game walks its object table.
/// - `ORDR` is the briefing up to and including its first NUL (one is added if it
///   has none).
/// - `BGND` is the 64-byte sky name buffer: the name, then NULs.
/// - A chunk the scenario lacks is written as the game's empty state: no terrain, no
///   objects, path tables of zeros, an empty briefing and a sky name of NULs.
pub fn write(scenario: &Scenario) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::new();
    chunk(&mut out, b"SHDR", &binary(&scenario.header))?;
    if let Some(save) = &scenario.save {
        chunk(&mut out, b"SDEF", &binary(save))?;
    }
    let terrain = scenario.terrain.as_ref().map(terrain_bytes);
    chunk(&mut out, b"MAPP", &terrain.unwrap_or_default())?;

    let mut objects: Vec<&Object> = scenario.objects.iter().flatten().collect();
    objects.sort_by_key(|o| o.slot);
    let count = u16::try_from(objects.len())
        .map_err(|_| invalid(out.len(), format!("{} objects", objects.len())))?;
    let mut dcbs = count.to_le_bytes().to_vec();
    for object in objects {
        dcbs.extend(binary(object));
    }
    chunk(&mut out, b"DCBS", &dcbs)?;

    for (tag, table) in [
        (b"APTH", &scenario.alien_paths),
        (b"HPTH", &scenario.human_paths),
    ] {
        let bytes = table
            .as_ref()
            .map_or_else(|| vec![0; PathTable::SIZE], binary);
        chunk(&mut out, tag, &bytes)?;
    }

    let briefing = scenario.briefing.as_deref().unwrap_or_default();
    let end = briefing
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(briefing.len());
    let mut ordr = briefing[..end].to_vec();
    ordr.push(0);
    chunk(&mut out, b"ORDR", &ordr)?;

    let mut bgnd = scenario.background.clone().unwrap_or_default();
    bgnd.resize(BACKGROUND_SIZE, 0);
    chunk(&mut out, b"BGND", &bgnd)?;
    chunk(&mut out, b"TERM", &[])?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_usg, synthetic_usg_chunks, synthetic_usg_from};

    #[test]
    fn synthetic_scenario_decodes() {
        let s = parse(&synthetic_usg()).unwrap();
        let c = s.header.start;
        assert_eq!((c.hour, c.minute, c.second, c.tick), (7, 29, 37, 20));
        assert_eq!((c.day, c.month), (0, 3));

        let objects = s.objects.as_ref().unwrap();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].kind, ObjectKind::Humanoid);
        assert_eq!(objects[0].side(), Side::Human);
        assert_eq!(fixed_to_f64(objects[0].x), 95.5);
        assert_eq!(objects[1].kind, ObjectKind::Relay);
        assert_eq!(objects[1].side(), Side::Alien);
        // 0x18-byte objects have no room for a kind's layout.
        assert_eq!(objects[1].body, Body::Other(Vec::new()));
        assert_eq!(objects[1].relay(), None);

        let terrain = s.terrain.as_ref().unwrap();
        assert_eq!(terrain.features.len(), 2);
        let hill = &terrain.features[0];
        assert_eq!((hill.kind, hill.point_slots), (TerrainFeature::HILL, 0x10));
        assert_eq!(
            hill.centre(),
            Point {
                x: 120 << 16,
                y: -(2 << 16)
            }
        );
        assert_eq!(hill.z, 0x3_2100);
        assert_eq!(
            (hill.radius, hill.diagonal, hill.height),
            (0x965, 0x643, 0x321)
        );
        assert_eq!(hill.shape, [TerrainFeature::HILL, 0]);
        let block = &terrain.features[1];
        assert_eq!((block.kind, block.shape.len()), (TerrainFeature::BLOCK, 2));
        assert_eq!(terrain.padding.len(), 6);

        let alien = s.alien_paths.as_ref().unwrap().used();
        assert_eq!(alien.len(), 1);
        let recon = &alien[0];
        assert_eq!(recon.mission_name(), Some("RECON"));
        assert_eq!(recon.waypoints().len(), 2);
        assert_eq!((recon.weight, recon.mix), (1, [0, 3, 7]));
        assert_eq!((recon.on_path, recon.on_path_by_kind), (7, [3, 1, 3]));
        assert!(s.human_paths.as_ref().unwrap().used().is_empty());

        assert_eq!(s.briefing_lines(), ["ATTACK THE BASE.", "", "GOOD LUCK."]);
        assert_eq!(s.background_name().as_deref(), Some("SKY_A.BIN"));
        assert!(s.other_chunks.is_empty());
    }

    #[test]
    fn scenario_snapshot() {
        let mut s = parse(&synthetic_usg()).unwrap();
        let paths = s.alien_paths.take().unwrap();
        s.human_paths = None;
        insta::assert_debug_snapshot!(s);
        insta::assert_debug_snapshot!(paths.used());
    }

    #[test]
    fn stock_sized_objects_get_their_kind_s_layout() {
        use crate::fixtures::synthetic_mission_usg;
        let s = parse(&synthetic_mission_usg()).unwrap();
        let objects = s.objects.unwrap();
        let base = objects[0].base().unwrap();
        assert_eq!((base.damage, base.view_mode), (20, 0));
        let humanoid = &objects[1];
        assert_eq!(humanoid.heading, 0x2000);
        assert_ne!(humanoid.flags & flag::UNIT, 0);
        let robot = humanoid.robot().unwrap();
        assert_eq!((robot.program_given, robot.program_running), (1, 1));
        assert_eq!((robot.helm, robot.power), (90, 80));
        assert_ne!(robot.mode & Robot::MANUAL, 0);
        assert_eq!(objects[2].relay().unwrap().damage, 3);
        let scout = &objects[4];
        assert_eq!((scout.side(), scout.heading), (Side::Alien, 0x7000));
        assert!(scout.base().is_none() && scout.robot().is_some());
        // A robot kind in a small object, or an unknown kind, is kept as bytes.
        let mut small = humanoid.clone();
        small.size = Object::SMALL;
        let mut bytes = std::io::Cursor::new(Vec::new());
        small.body = Body::Other(vec![7; usize::from(Object::SMALL) - Object::HEADER_SIZE]);
        small.write(&mut bytes).unwrap();
        let back = Object::read(&mut std::io::Cursor::new(bytes.into_inner())).unwrap();
        assert_eq!(back, small);
    }

    #[test]
    fn stock_sized_object_snapshot() {
        use crate::fixtures::synthetic_mission_usg;
        let objects = parse(&synthetic_mission_usg()).unwrap().objects.unwrap();
        // The base, the humanoid and the relay.
        insta::assert_debug_snapshot!(&objects[..3]);
    }

    #[test]
    fn unknown_chunks_are_kept_and_others_are_optional() {
        let mut chunks = synthetic_usg_chunks();
        chunks.retain(|(tag, _)| matches!(tag, b"SHDR" | b"TERM"));
        chunks.insert(1, (*b"XTRA", vec![1, 2, 3]));
        let s = parse(&synthetic_usg_from(&chunks)).unwrap();
        assert_eq!(s.objects, None);
        assert_eq!(s.background_name(), None);
        assert!(s.briefing_lines().is_empty());
        assert_eq!(
            s.other_chunks,
            [Chunk {
                tag: *b"XTRA",
                data: vec![1, 2, 3]
            }]
        );
    }

    /// A save's campaign: scenario 2 selected and loaded, progress 3, score 1500, 2
    /// wins, 1 loss, 40 kills, 9 robots lost, 0x5fa7 = 7, no result yet, and the two
    /// records of `scendef`'s tests (the second won) padded to 48 bytes with 0xaa.
    fn synthetic_sdef() -> Vec<u8> {
        use crate::fixtures::synthetic_scendef;
        let mut table = synthetic_scendef(&[
            ((10, 20, 19, 29), 1, b"FIRST\x0c"),
            ((100, 200, 109, 209), 3, b"SECOND\x0c"),
        ]);
        table[12] = 2; // the first record's state: open at alert 2
        table[21 + 12] = 0xa; // the second record (after 14 + 7 bytes): won
        table.resize(48, 0xaa);
        let mut sdef: Vec<u8> = [2u16, 2, 3, 1500, 2, 1, 40, 9, 7, 48]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        sdef.extend([0, 0]);
        sdef.extend([0x55; 8]);
        sdef.extend(table);
        sdef
    }

    #[test]
    fn a_save_s_campaign_decodes() {
        let mut chunks = synthetic_usg_chunks();
        chunks.insert(1, (*b"SDEF", synthetic_sdef()));
        let s = parse(&synthetic_usg_from(&chunks)).unwrap();
        assert!(s.other_chunks.is_empty());
        let save = s.save.unwrap();
        assert_eq!((save.selected, save.loaded, save.progress), (2, 2, 3));
        assert_eq!((save.score, save.wins, save.losses), (1500, 2, 1));
        assert_eq!((save.kills, save.robots_lost, save.unknown_10), (40, 9, 7));
        assert_eq!((save.result, save.campaign_won), (0, 0));
        assert_eq!(save.table.len(), 48);
        let table = save.scenarios().unwrap();
        let states: Vec<u16> = table.scenarios.iter().map(|r| r.state).collect();
        assert_eq!(states, [2, 0xa]);
        insta::assert_debug_snapshot!(save);

        // The game writes the whole table in paragraphs; the writer pads with zeros.
        let mut again = save.clone();
        again.set_scenarios(&table);
        assert_eq!(again.table.len(), 48);
        assert_eq!(again.scenarios().unwrap(), table);
    }

    #[test]
    fn a_save_s_table_must_fill_its_chunk_and_decode() {
        for (what, sdef) in [
            ("short", {
                let mut d = synthetic_sdef();
                d.pop();
                d
            }),
            ("long", {
                let mut d = synthetic_sdef();
                d.push(0);
                d
            }),
            ("no end marker", {
                let mut d = synthetic_sdef();
                d.truncate(SaveState::HEADER_SIZE + 4);
                d[0x12] = 4;
                d
            }),
        ] {
            let mut chunks = synthetic_usg_chunks();
            chunks.insert(1, (*b"SDEF", sdef));
            assert!(parse(&synthetic_usg_from(&chunks)).is_err(), "{what}");
        }
    }

    #[test]
    fn an_empty_terrain_chunk_has_no_features() {
        let mut chunks = synthetic_usg_chunks();
        chunks
            .iter_mut()
            .find(|(t, _)| t == b"MAPP")
            .unwrap()
            .1
            .clear();
        let terrain = parse(&synthetic_usg_from(&chunks))
            .unwrap()
            .terrain
            .unwrap();
        assert!(terrain.features.is_empty() && terrain.padding.is_empty());
    }

    fn reason(chunks: &[([u8; 4], Vec<u8>)]) -> String {
        match parse(&synthetic_usg_from(chunks)) {
            Err(FormatError::Invalid { reason, .. }) => reason,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn malformed_scenarios_are_rejected() {
        let good = synthetic_usg_chunks();

        let mut no_header = good.clone();
        no_header.remove(0);
        assert!(reason(&no_header).contains("not \"SHDR\""));

        let mut twice = good.clone();
        twice.insert(2, good[1].clone());
        assert!(reason(&twice).contains("second"));

        let mut old = good.clone();
        old[0].1[0] = 0x7f;
        assert!(reason(&old).contains("version 0x7f"));

        let mut short_paths = good.clone();
        short_paths
            .iter_mut()
            .find(|(t, _)| t == b"HPTH")
            .unwrap()
            .1
            .pop();
        assert!(reason(&short_paths).contains("not 1122"));

        let mut long_path = good.clone();
        long_path.iter_mut().find(|(t, _)| t == b"APTH").unwrap().1[2] = 17;
        assert!(reason(&long_path).contains("17 points"));

        let mut extra = good.clone();
        extra
            .iter_mut()
            .find(|(t, _)| t == b"DCBS")
            .unwrap()
            .1
            .push(0);
        assert!(reason(&extra).contains("1 bytes after 2 objects"));

        let mut bytes = synthetic_usg();
        bytes.push(0);
        assert!(
            matches!(parse(&bytes), Err(FormatError::Invalid { reason, .. }) if reason.contains("after TERM"))
        );

        let mut unterminated = good.clone();
        unterminated.pop();
        assert!(matches!(
            parse(&synthetic_usg_from(&unterminated)),
            Err(FormatError::Binrw(_))
        ));

        let mut bad_slot = good;
        let dcbs = &mut bad_slot.iter_mut().find(|(t, _)| t == b"DCBS").unwrap().1;
        dcbs[4..6].copy_from_slice(&240u16.to_le_bytes());
        assert!(matches!(
            parse(&synthetic_usg_from(&bad_slot)),
            Err(FormatError::Binrw(_))
        ));
    }

    #[test]
    fn a_written_scenario_reads_back() {
        let mut s = parse(&synthetic_usg()).unwrap();
        // The game's terrain buffer is whole paragraphs: the synthetic chunk's 56 bytes
        // are written as 64, the padding filled with zeros.
        let terrain = s.terrain.as_mut().unwrap();
        assert_eq!(terrain_bytes(terrain).len(), 64);
        terrain.padding.extend([0; 8]);
        assert_eq!(parse(&write(&s).unwrap()).unwrap(), s);
        // Objects go in slot order whatever order they are given in.
        s.objects.as_mut().unwrap().reverse();
        let back = parse(&write(&s).unwrap()).unwrap();
        let slots: Vec<u16> = back.objects.unwrap().iter().map(|o| o.slot).collect();
        assert!(slots.is_sorted() && slots.len() == 2, "{slots:?}");
    }

    #[test]
    fn the_writer_adds_what_the_game_always_writes() {
        let mut chunks = synthetic_usg_chunks();
        chunks.retain(|(tag, _)| matches!(tag, b"SHDR" | b"TERM"));
        chunks.insert(1, (*b"XTRA", vec![9]));
        chunks.insert(1, (*b"SDEF", synthetic_sdef()));
        let s = parse(&synthetic_usg_from(&chunks)).unwrap();
        let bytes = write(&s).unwrap();
        let tags: Vec<&[u8]> = {
            let mut at = 0;
            let mut tags = Vec::new();
            while at < bytes.len() {
                tags.push(&bytes[at..at + 4]);
                at += 6 + usize::from(u16::from_le_bytes([bytes[at + 4], bytes[at + 5]]));
            }
            tags
        };
        let want: [&[u8]; 9] = [
            b"SHDR", b"SDEF", b"MAPP", b"DCBS", b"APTH", b"HPTH", b"ORDR", b"BGND", b"TERM",
        ];
        assert_eq!(tags, want);
        let back = parse(&bytes).unwrap();
        assert_eq!(back.objects, Some(Vec::new()));
        assert_eq!(back.terrain.unwrap().features, []);
        assert_eq!(back.human_paths.unwrap().count, 0);
        assert_eq!(back.briefing.as_deref(), Some(&[0][..]));
        assert_eq!(back.background, Some(vec![0; BACKGROUND_SIZE]));
        assert!(back.other_chunks.is_empty(), "XTRA is dropped");
        assert_eq!(back.save, s.save);

        // The briefing ends at its first NUL; the terrain fills a paragraph.
        let mut s = parse(&synthetic_usg()).unwrap();
        s.briefing = Some(b"GO\0LEFT".to_vec());
        let terrain = s.terrain.as_mut().unwrap();
        terrain.padding.clear();
        let back = parse(&write(&s).unwrap()).unwrap();
        assert_eq!(back.briefing.as_deref(), Some(&b"GO\0"[..]));
        let size = 2 * 24 + 2;
        let padding = back.terrain.unwrap().padding;
        assert_eq!(
            (padding.len(), padding.iter().all(|&b| b == 0)),
            (64 - size, true)
        );
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_usg_round_trip() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let mut names: Vec<String> = (0..13).map(|n| format!("scen{n:02}a.usg")).collect();
        names.extend(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
        for name in names {
            let bytes = std::fs::read(dir.join(&name)).unwrap();
            let s = parse(&bytes).unwrap();
            // Written as the game writes it, every stock file comes back byte for byte.
            assert!(write(&s).unwrap() == bytes, "{name}");
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_usg_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let mut names: Vec<String> = (0..13).map(|n| format!("scen{n:02}a.usg")).collect();
        names.extend(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
        for name in names {
            let s = parse(&std::fs::read(dir.join(&name)).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(s.other_chunks.is_empty(), "{name}");
            let objects = s.objects.as_ref().unwrap();
            for side in [Side::Human, Side::Alien] {
                let bases = objects
                    .iter()
                    .filter(|o| o.kind == ObjectKind::Base && o.side() == side)
                    .count();
                assert_eq!(bases, 1, "{name}: {side:?} bases");
            }
            for o in objects {
                let want = match o.kind {
                    ObjectKind::Relay | ObjectKind::Obstacle => Object::SMALL,
                    ObjectKind::Other(k) => panic!("{name}: object kind {k}"),
                    _ => Object::BIG,
                };
                let at = format!("{name}: slot {} {:?}", o.slot, o.kind);
                assert_eq!(o.size, want, "{at}");
                // Every object is present, and only robots are units (the loader
                // registers those and nothing else).
                assert_eq!(o.flags & flag::GONE, 0, "{at}");
                let robot = matches!(
                    o.kind,
                    ObjectKind::Humanoid | ObjectKind::Scorpion | ObjectKind::Scout
                );
                assert_eq!(o.flags & flag::UNIT != 0, robot, "{at}");
                // Each kind with a layout gets it; the rest stay bytes.
                match (&o.body, o.kind) {
                    (Body::Robot(r), _) => {
                        assert!(robot, "{at}");
                        // Percentages, and a script pointer in its saved form.
                        assert!(r.helm <= 100 && r.power <= 100, "{at}");
                        assert_eq!(r.script & 0xf800, 0xa000, "{at}");
                    }
                    (Body::Base(b), ObjectKind::Base) => assert!(b.damage < 300, "{at}"),
                    (Body::Relay(r), ObjectKind::Relay) => assert!(r.damage < 20, "{at}"),
                    (Body::Other(_), ObjectKind::Obstacle) => {}
                    (body, kind) => panic!("{at}: {kind:?} read as {body:?}"),
                }
            }
            // Hills as the map editor writes them, then its closing block.
            let features = &s.terrain.as_ref().unwrap().features;
            for (i, t) in features.iter().enumerate() {
                let at = format!("{name}: terrain {i}");
                assert_eq!((t.size, t.shape.as_slice()), (24, &[t.kind, 0][..]), "{at}");
                let last = i + 1 == features.len();
                let want = if last {
                    (TerrainFeature::BLOCK, 8)
                } else {
                    (TerrainFeature::HILL, 0x10)
                };
                assert_eq!((t.kind, t.point_slots), want, "{at}");
                if !last {
                    assert!(t.radius > t.diagonal && t.diagonal > t.height, "{at}");
                }
            }
            // Every used path has points and the editor's header for its mission;
            // the counts per kind add up.
            for table in [&s.alien_paths, &s.human_paths] {
                for p in table.as_ref().unwrap().used() {
                    let at = format!("{name}: {:?}", p.mission_name());
                    assert!(p.point_count > 0, "{at}");
                    assert_eq!(p.unknown_02, [0, 0], "{at}");
                    assert_eq!(p.mix.iter().sum::<u8>(), 10, "{at}");
                    assert_eq!(p.on_path_by_kind.iter().sum::<u8>(), p.on_path, "{at}");
                }
            }
            assert!(s.background_name().unwrap().ends_with(".BIN"), "{name}");
            assert!(!s.briefing_lines().is_empty(), "{name}");
        }
    }
}
