//! `.usg` scenario files: the campaign scenarios (`scenNNa`) and the four training
//! missions. A file is a chain of tagged chunks: the start clock, terrain features,
//! the objects (robots, bases, relays, obstacles), each side's paths, the briefing text
//! and the sky picture. There is no map grid. See `docs/formats/usg.md`.

use std::fmt;

use binrw::{BinRead, BinWrite, binrw};

use crate::FormatError;

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
    /// +0x18: +0x1a counts the robot's ticks in twos and +0x1b is a state number,
    /// each picking a routine from a table.
    pub unknown_18: Unknown<6>,
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
    /// +0x26: speed; every 16 ticks it steps by one towards `target_speed`.
    pub speed: i16,
    /// +0x28: the speed asked for.
    pub target_speed: i16,
    /// +0x2a: the heading asked for; every tick `heading` turns towards it by at most
    /// 0x5b (a scorpion's step). The editor sets both to the same value.
    pub target_heading: u16,
    pub unknown_2c: Unknown<2>,
    /// +0x2e: the program given, a number in `xeno.exe`'s program table.
    pub program_given: u8,
    pub unknown_2f: u8,
    /// +0x30: the program running: the one given, or a state the script entered.
    pub program_running: u8,
    /// +0x31, +0x33: the script's timing (+0x31 is 10 when a program starts).
    pub unknown_31: Unknown<3>,
    /// +0x34: the script pointer, saved as `0xa000` + its offset from the program
    /// table.
    pub script: u16,
    /// +0x36: event bits (the message panel's), +0x38: the script's start phase.
    pub unknown_36: Unknown<0x18>,
    /// +0x4e: power systems, 0 to 100; hits lower it.
    pub power: u8,
    pub unknown_4f: Unknown<0x8d>,
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
    /// +0xf7 on, including the console's widget state (+0x10c humanoid, +0x10d scout,
    /// +0x127 scorpion).
    pub unknown_f7: Unknown<0x7b>,
}

impl Robot {
    /// [`Robot::mode`]: the pilot drives; the program waits.
    pub const MANUAL: u8 = 0x80;
    /// [`Robot::mode`]: the situation room recalled the robot; its next tick gives it
    /// program 4 and clears the bit.
    pub const RECALL: u8 = 0x08;
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
    pub unknown_32: Unknown<2>,
    /// +0x34: how much of the briefing the orders view has typed.
    pub typed: u16,
    pub unknown_36: u8,
    /// +0x37: the situation room's main view mode (0, 2, 4, 6, 8: O, S, M, R, C).
    pub view_mode: u8,
    pub unknown_38: Unknown<0x85>,
    /// +0xbd: the situation room's widget state.
    pub view_state: Unknown<0x43>,
    pub unknown_100: Unknown<0x72>,
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

/// One path slot of a [`PathTable`].
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path {
    /// How many of `points` the path uses. Not traced in the code, but in every stock
    /// path whose later points are empty it is the number of points before them.
    pub point_count: u8,
    /// What the path is for; see [`Path::mission_name`].
    pub mission: u8,
    /// The game's debug display prints +0x04 as a number, +0x05..=+0x07 as tenths
    /// (0 to 100 %) and +0x08..=+0x0b as numbers. Meaning unknown.
    pub unknown_02: [u8; 10],
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
}

/// One `MAPP` record.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainFeature {
    /// Size of the record, including this field.
    #[br(assert(size >= 4, "terrain record of {size} bytes"))]
    pub size: u16,
    /// [`TerrainFeature::CIRCLE`] or 0x12 in stock files.
    pub kind: u8,
    /// 0x10 with kind 0x14, 0x08 with kind 0x12.
    pub unknown_03: u8,
    /// +0x04 to the end of the record.
    #[br(count = size - 4)]
    pub data: Vec<u8>,
}

impl TerrainFeature {
    /// A round feature the ground-height code tests points against: a centre at +0x04
    /// and +0x08 (16.16) and a radius at +0x10 (8.8). What else it holds (a height at
    /// +0x0c?) is unknown.
    pub const CIRCLE: u8 = 0x14;

    /// The bytes at record offset `offset`.
    fn bytes<const N: usize>(&self, offset: usize) -> Option<[u8; N]> {
        let i = offset.checked_sub(4)?;
        self.data.get(i..i + N)?.try_into().ok()
    }

    /// The centre of a [`TerrainFeature::CIRCLE`], 16.16.
    pub fn centre(&self) -> Option<Point> {
        if self.kind != Self::CIRCLE {
            return None;
        }
        Some(Point {
            x: i32::from_le_bytes(self.bytes(0x04)?),
            y: i32::from_le_bytes(self.bytes(0x08)?),
        })
    }

    /// The radius of a [`TerrainFeature::CIRCLE`], 8.8 fixed point.
    pub fn radius(&self) -> Option<u16> {
        if self.kind != Self::CIRCLE {
            return None;
        }
        Some(u16::from_le_bytes(self.bytes(0x10)?))
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
    /// Chunks with other tags, which the game skips (a save file's `SDEF` is read by
    /// other code), in file order.
    pub other_chunks: Vec<Chunk>,
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
        other_chunks,
    })
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
        assert_eq!(
            hill.centre(),
            Some(Point {
                x: 120 << 16,
                y: -(2 << 16)
            })
        );
        assert_eq!(hill.radius(), Some(0x0965));
        assert_eq!(terrain.features[1].centre(), None);
        assert_eq!(terrain.padding.len(), 6);

        let alien = s.alien_paths.as_ref().unwrap().used();
        assert_eq!(alien.len(), 1);
        assert_eq!(alien[0].mission_name(), Some("RECON"));
        assert_eq!(alien[0].waypoints().len(), 2);
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
        chunks.insert(1, (*b"SDEF", vec![1, 2, 3]));
        let s = parse(&synthetic_usg_from(&chunks)).unwrap();
        assert_eq!(s.objects, None);
        assert_eq!(s.background_name(), None);
        assert!(s.briefing_lines().is_empty());
        assert_eq!(
            s.other_chunks,
            [Chunk {
                tag: *b"SDEF",
                data: vec![1, 2, 3]
            }]
        );
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
            assert!(s.background_name().unwrap().ends_with(".BIN"), "{name}");
            assert!(!s.briefing_lines().is_empty(), "{name}");
        }
    }
}
