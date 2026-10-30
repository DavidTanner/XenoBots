//! What the port reads from the user's `xeno.exe` to move robots (`docs/sim.md`,
//! "Movement"): the sine table the velocity comes from, each kind's turn step, the
//! drive level under which a robot goes at half speed, the scorpion's speed tables, and
//! the program each path mission gives each kind, and the leg frames and model and eye
//! heights of the walking animations (`docs/render.md`, "Walking"; `docs/sim.md`,
//! "Heights"). Game data, read at run time; each
//! table is found through an instruction that uses it, after checking that
//! instruction's bytes.

use crate::base::{self, Bases};
use crate::calendar::{self, Calendar};
use crate::damage::{self, Damage};
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};
use crate::net::{self, Net};
use crate::relay::{self, Relays};
use crate::script::{self, Scripts};
use crate::sensing::{self, Sensing};
use crate::weapons::{self, Weapons};
use crate::{FormatError, sequence};

/// `mov cx, ss:[bx + table + 2]` and `mov bx, ss:[bx + table]` in
/// `sim_sine_magnitude` (`1000:05e6`): a value and the next one to interpolate to.
pub(crate) const SINE_NEXT: (usize, &[u8]) = (0x060c, &[0x36, 0x8b, 0x8f]);
pub(crate) const SINE_AT: (usize, &[u8]) = (0x0611, &[0x36, 0x8b, 0x9f]);
/// `mov bx, step` in `sim_steer_humanoid`, `sim_steer_scorpion` and
/// `sim_steer_scout`, by kind.
pub(crate) const TURN_AT: [usize; 3] = [0xac85, 0x93d3, 0xc554];
/// `cmp byte [di + 0x51], level` in `sim_limit_speed_by_drive_and_helm`.
pub(crate) const WEAK_DRIVE: (usize, &[u8]) = (0xd3b8, &[0x80, 0x7d, 0x51]);
/// `mov es, [0x60]` then `mov bx, es:[bx + table]` in `sim_map_scorpion_speed`
/// (`1000:d3f8`): the speed tables' offsets, in the string segment.
pub(crate) const SPEEDS_SEGMENT: (usize, &[u8]) = (0xd3f8, &[0x8e, 0x06, 0x60, 0x00]);
pub(crate) const SPEEDS_AT: (usize, &[u8]) = (0xd400, &[0x26, 0x8b, 0x9f]);
/// `mov al, [bx + table]` in mission 0's program routine (`1000:ec68`, through
/// `order_start_path_mission_program`): three bytes a mission, by kind.
pub(crate) const PROGRAMS_AT: (usize, &[u8]) = (0xec6a, &[0x8a, 0x87]);
/// `mov ax, ss:[bx + table]` in `sim_get_bearing` (`1000:0735`) on the path the
/// scripts take (through `sim_get_bearing_coarse`, `1000:04cc`): the arctangents.
pub(crate) const ATAN_AT: (usize, &[u8]) = (0x0835, &[0x36, 0x8b, 0x87]);
/// `jmp [bx + table]` for the script's opcodes 0x54 to 0x58: each a table of three
/// routines, by kind, that set the speed asked for.
pub(crate) const SPEED_OPS: [usize; 5] = [0xd08c, 0xd084, 0xd07c, 0xd06c, 0xd074];
/// `jmp [bx + table]` for the script's opcodes 0x51 and 0x52 (`1000:d090`, `d098`):
/// each a table of three routines, by kind, that step the speed asked for up or down
/// a gear. The cockpit's up and down arrows call the same routines.
pub(crate) const GEAR_OPS: [usize; 2] = [0xd094, 0xd09c];
/// `sub ax, turn` in the humanoid's left arrow (`1000:acc0`: the pilot takes over,
/// then the heading asked for is the heading less `turn`).
pub(crate) const KEY_TURN: (usize, &[u8]) = (0xacc6, &[0x2d]);
/// `jmp cs:[bx + table]` in F5's handler (`1000:d137`): each kind's routine that
/// selects a weapon (F6 and the humanoid's F9 reach the same routines).
pub(crate) const WEAPON_KEYS: (usize, &[u8]) = (0xd137, &[0x2e, 0xff, 0xa7]);
/// `test byte [di + 0x23], 8`, `jz`, `mov al, program` at the start of
/// `sim_move_humanoid` (`1000:ac29`): the recall's program.
pub(crate) const RECALL_AT: usize = 0xac29;
/// No more gears than this (the stock build has 6).
const MAX_GEARS: usize = 16;

/// `mov al, cs:[bx + table]` in the walking animations: the humanoid's legs (+0x101,
/// +0x102), the scout's (+0x103, +0x104) and the scout's crouched legs.
pub(crate) const HUMANOID_LEGS_AT: [usize; 2] = [0xadbe, 0xadcc];
pub(crate) const SCOUT_LEGS_AT: [usize; 2] = [0xc7bf, 0xc7d1];
pub(crate) const SCOUT_CROUCH_AT: usize = 0xc829;
/// The scorpion's six (`1000:95dd`): each `mov al, cs:[bx + table]`, `mov [di + leg],
/// al`, 9 bytes apart; the legs are +0xff to +0x104, not in that order.
pub(crate) const SCORPION_LEGS_AT: usize = 0x95e3;
pub const SCORPION_LEGS: usize = 6;
/// `mov al, cs:[bx + table]` in the humanoid's arms (`1000:ae23`, `ae8a`): their swing
/// while walking (+0xff, +0x100).
pub(crate) const HUMANOID_ARMS_AT: [usize; 2] = [0xae74, 0xaedb];
const LEG_OP: &[u8] = &[0x2e, 0x8a, 0x87];
/// The model heights (+0x12) and eye heights (+0x1c) the walks and the scout's stance
/// set (`docs/sim.md`, "Heights"): in the humanoid's walk (`1000:ad67`) `mov ax, base`,
/// `add ax, cs:[bx + table]`, `mov [di + 0x12], ax`, `mov ax, eye`, `mov [di + 0x1c],
/// ax`; in the scout's (`1000:c799`) `mov ax, base`, `shl bx, 1`, `add ax, cs:[bx +
/// table]`, `mov [di + 0x12], ax`; crouched (`1000:c845`) `neg ax`, `add ax, base`,
/// `mov [di + 0x12], ax`; in its stance step (`1000:c5c8`) `cbw`, `shl ax, 1` three
/// times, `neg ax`, `add ax, eye`, `mov [di + 0x1c], ax`.
pub(crate) const HUMANOID_BOB_AT: usize = 0xada6;
pub(crate) const SCOUT_BOB_AT: usize = 0xc7e3;
pub(crate) const SCOUT_CROUCHED_AT: usize = 0xc845;
pub(crate) const SCOUT_EYE_AT: usize = 0xc5c8;
const ADD_TABLE: &[u8] = &[0x2e, 0x03, 0x87];
const TO_MODEL_HEIGHT: &[u8] = &[0x89, 0x45, 0x12];
const TO_EYE_HEIGHT: &[u8] = &[0x89, 0x45, 0x1c];
/// Steps in a walking animation's tables.
pub const STEPS: usize = 16;
/// A scout's stances run from 0 to 0x30, so its crouched legs are read by stance / 16
/// from 4 entries.
pub const CROUCHES: usize = 4;

/// Entries in the sine table: a quarter turn in 256 steps, and the one after it.
pub const SINE_ENTRIES: usize = 257;
/// The scorpion's speed asked for runs from -42 to 63 (its keys keep it there), so
/// the port reads each table's first 64 entries.
pub const SPEED_ENTRIES: usize = 64;
/// No more speed tables than this (the stock build has 7).
const MAX_SPEED_TABLES: usize = 16;
/// Entries in the arctangent table the scripts read.
pub const ATAN_ENTRIES: usize = 256;
/// The path missions (`docs/formats/usg.md`, "How paths are used").
pub const MISSIONS: usize = 7;

/// The movement tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion {
    /// `sin(i / 1024 of a turn)` in 0.16, for i in 0..=256 (the last entry is the
    /// table's, not 1.0: `docs/sim.md`, "Velocity").
    pub sine: Vec<u16>,
    /// The most a humanoid, scorpion and scout turns in a tick (65,536 to a turn).
    pub turn_step: [u16; 3],
    /// A robot whose drive (+0x51) is at or under this goes at half speed.
    pub weak_drive: u8,
    /// The scorpion's speed tables, by +0x118 / 2: a speed asked for, 0 to 63, as the
    /// speed it gets.
    pub scorpion_speeds: Vec<[u8; SPEED_ENTRIES]>,
    /// The program a path's mission starts, by mission and kind (humanoid, scorpion,
    /// scout).
    pub mission_programs: [[u8; 3]; MISSIONS],
    /// `atan(i / 256)`, 0x10000 to an eighth of a turn (`docs/sim.md`, "The
    /// bearing").
    pub atan: Vec<u16>,
    /// The speed the script's opcodes 0x54 to 0x58 ask for, by opcode and kind.
    pub script_speeds: [[i16; 3]; 5],
    /// The speed keys' and gear opcodes' steps.
    pub gears: Gears,
    /// What the weapon keys post and play.
    pub weapon_keys: WeaponKeys,
    /// The program a recalled robot is given (+0x23 bit 3; 4, RET BASE, in the stock
    /// build).
    pub recall_program: u8,
    /// The robots' scripts (`xeno_formats::script`).
    pub scripts: Scripts,
    /// What damage does (`xeno_formats::damage`).
    pub damage: Damage,
    /// What sensing reads (`xeno_formats::sensing`).
    pub sensing: Sensing,
    /// What the relay-net orders read (`xeno_formats::net`).
    pub net: Net,
    /// The relays' tick and the net's signal (`xeno_formats::relay`).
    pub relays: Relays,
    /// The weapons and their shots (`xeno_formats::weapons`).
    pub weapons: Weapons,
    /// The bases' tick (`xeno_formats::base`).
    pub bases: Bases,
    /// The clock's days per month (`xeno_formats::calendar`).
    pub calendar: Calendar,
    pub walk: Walk,
    /// The string segment, where the scripts' messages are.
    pub strings: Strings,
}

/// The string segment from its start to the image's end. The message panel's typing
/// (`1fe6:826d`) reads it through the word at `DS:0x6fec`, which holds the same
/// segment as `DS:0x60` in the stock build.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Strings(pub Vec<u8>);

impl Strings {
    /// The byte at `offset`; 0 past the end.
    pub fn byte(&self, offset: u16) -> u8 {
        self.0.get(usize::from(offset)).copied().unwrap_or(0)
    }
}

impl std::fmt::Debug for Strings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Strings({} bytes)", self.0.len())
    }
}

/// What the cockpit's arrow keys and the scripts' opcodes 0x51 and 0x52 step
/// (`docs/sim.md`, "The cockpit keys"). A humanoid and a scout step through gears,
/// each a speed asked for; a scorpion steps its speed asked for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gears {
    /// The speed asked for in each of a humanoid's gears (`DS:0x68ec` in the stock
    /// build), slowest first. The gear (+0x10b) is kept as twice its index.
    pub humanoid: Vec<i16>,
    /// The scout's (`DS:0x6df8`; its gear is at +0x109).
    pub scout: Vec<i16>,
    /// A scorpion's speed asked for goes up or down by `scorpion_step`, within
    /// `scorpion_bottom..=scorpion_top`.
    pub scorpion_step: i16,
    pub scorpion_top: i16,
    pub scorpion_bottom: i16,
    /// What the left and right arrows take from or add to the heading.
    pub turn: u16,
}

/// What a weapon key does when the weapon selected (+0xdd) changes (`docs/sim.md`,
/// "The cockpit keys"): a message to the robot's panel and a sound event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponKeys {
    /// The sound event played (27 in the stock build).
    pub sound: u8,
    /// The message (a string-segment offset) by kind (humanoid, scorpion, scout) and
    /// weapon; the scorpion's when the weapon is neither damaged nor empty.
    pub selected: [[u16; 2]; 3],
    /// The scorpion's when the weapon's count is 0, and when it is damaged.
    pub scorpion_empty: [u16; 2],
    pub scorpion_damaged: [u16; 2],
    /// The scorpion's weapons-damaged bit (+0xdc) by weapon.
    pub scorpion_bits: [u8; 2],
}

/// The leg frames of the walking animations that make footsteps, by step, and the
/// model and eye heights they set (`docs/render.md`, "Walking").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// The humanoid's legs, +0x101 and +0x102.
    pub humanoid_legs: [[u8; STEPS]; 2],
    /// The humanoid's arms' swing while walking, +0xff and +0x100.
    pub humanoid_arms: [[u8; STEPS]; 2],
    /// The scout's legs, +0x103 and +0x104 (+0xff and +0x100 follow them).
    pub scout_legs: [[u8; STEPS]; 2],
    /// A scout's four legs when it stands still crouched, by stance / 16.
    pub scout_crouch: [u8; CROUCHES],
    /// The scorpion's legs, +0xff to +0x104 in that order.
    pub scorpion_legs: [[u8; STEPS]; SCORPION_LEGS],
    /// The humanoid's model height by step: `height + bob[step]`; its eye height.
    pub humanoid_height: u16,
    pub humanoid_bob: [u16; STEPS],
    pub humanoid_eye: u16,
    /// The scout's model height by step while it moves; still and crouched,
    /// `crouched - 8 * stance`; its eye height `eye - 8 * stance` as the stance steps.
    pub scout_height: u16,
    pub scout_bob: [u16; STEPS],
    pub scout_crouched: u16,
    pub scout_eye: u16,
}

fn bytes(image: &[u8], at: usize, n: usize) -> Result<&[u8], FormatError> {
    image
        .get(at..at + n)
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

/// Reads the tables from a load image whose data segment starts at `ds` and whose
/// string segment (the word at `DS:0x60`) starts at `strings`.
pub fn read_image(image: &[u8], ds: usize, strings: usize) -> Result<Motion, FormatError> {
    let op = |(at, op): (usize, &[u8])| operand(image, at, op);

    let sine_at = op(SINE_AT)?;
    if op(SINE_NEXT)? != sine_at.wrapping_add(2) {
        return Err(invalid(SINE_NEXT.0, "not the next sine entry"));
    }
    let sine = (0..SINE_ENTRIES)
        .map(|i| word(image, ds + usize::from(sine_at) + 2 * i))
        .collect::<Result<_, _>>()?;

    let mut turn_step = [0; 3];
    for (step, at) in turn_step.iter_mut().zip(TURN_AT) {
        *step = operand(image, at, &[0xbb])?;
    }
    let (at, prefix) = WEAK_DRIVE;
    let weak_drive = bytes(image, at, prefix.len() + 1).and_then(|b| match b.split_last() {
        Some((&level, rest)) if rest == prefix => Ok(level),
        _ => Err(invalid(
            at,
            format!("not the expected instruction {prefix:02x?}"),
        )),
    })?;

    let (at, load) = SPEEDS_SEGMENT;
    if bytes(image, at, load.len())? != load {
        return Err(invalid(at, "not the string segment's load"));
    }
    // The pointer table is followed by the first table, so its first pointer says how
    // many there are.
    let table = strings + usize::from(op(SPEEDS_AT)?);
    let first = strings + usize::from(word(image, table)?);
    let count = first
        .checked_sub(table)
        .filter(|n| n % 2 == 0 && (2..=2 * MAX_SPEED_TABLES).contains(n))
        .ok_or_else(|| invalid(table, "the speed tables do not follow their offsets"))?
        / 2;
    let scorpion_speeds = (0..count)
        .map(|i| {
            let at = strings + usize::from(word(image, table + 2 * i)?);
            Ok(bytes(image, at, SPEED_ENTRIES)?
                .try_into()
                .expect("64 bytes"))
        })
        .collect::<Result<_, FormatError>>()?;

    let programs = bytes(image, ds + usize::from(op(PROGRAMS_AT)?), 3 * MISSIONS)?;
    let mut mission_programs = [[0; 3]; MISSIONS];
    for (mission, kinds) in mission_programs.iter_mut().enumerate() {
        kinds.copy_from_slice(&programs[3 * mission..3 * mission + 3]);
    }

    let atan_at = ds + usize::from(op(ATAN_AT)?);
    let atan = (0..ATAN_ENTRIES)
        .map(|i| word(image, atan_at + 2 * i))
        .collect::<Result<_, _>>()?;
    let mut script_speeds = [[0; 3]; 5];
    for (speeds, at) in script_speeds.iter_mut().zip(SPEED_OPS) {
        let table = ds + usize::from(operand(image, at, &[0xff, 0xa7])?);
        for (kind, speed) in speeds.iter_mut().enumerate() {
            *speed = script_speed(image, ds, usize::from(word(image, table + 2 * kind)?))?;
        }
    }

    Ok(Motion {
        sine,
        turn_step,
        weak_drive,
        scorpion_speeds,
        mission_programs,
        atan,
        script_speeds,
        gears: gears(image, ds)?,
        weapon_keys: weapon_keys(image, ds)?,
        recall_program: match *bytes(image, RECALL_AT, 8)? {
            [0xf6, 0x45, 0x23, 0x08, 0x74, _, 0xb0, program] => program,
            _ => return Err(invalid(RECALL_AT, "not the recall's test")),
        },
        scripts: script::read_image(image, ds)?,
        damage: damage::read_image(image, ds)?,
        sensing: sensing::read_image(image, ds)?,
        net: net::read_image(image)?,
        relays: relay::read_image(image, ds)?,
        weapons: weapons::read_image(image, ds)?,
        bases: base::read_image(image, ds)?,
        calendar: calendar::read_image(image, ds)?,
        walk: Walk {
            humanoid_height: operand(image, HUMANOID_BOB_AT, &[0xb8])?,
            humanoid_bob: bob(image, HUMANOID_BOB_AT + 3)?,
            humanoid_eye: match *bytes(image, HUMANOID_BOB_AT + 11, 6)? {
                [0xb8, lo, hi, 0x89, 0x45, 0x1c] => u16::from_le_bytes([lo, hi]),
                _ => return Err(invalid(HUMANOID_BOB_AT + 11, "not the eye height's set")),
            },
            scout_height: operand(image, SCOUT_BOB_AT, &[0xb8])?,
            scout_bob: bob(image, SCOUT_BOB_AT + 5)?,
            scout_crouched: after(
                image,
                SCOUT_CROUCHED_AT,
                &[0xf7, 0xd8, 0x05],
                TO_MODEL_HEIGHT,
            )?,
            scout_eye: after(
                image,
                SCOUT_EYE_AT,
                &[0x98, 0xd1, 0xe0, 0xd1, 0xe0, 0xd1, 0xe0, 0xf7, 0xd8, 0x05],
                TO_EYE_HEIGHT,
            )?,
            humanoid_arms: [
                legs(image, HUMANOID_ARMS_AT[0])?,
                legs(image, HUMANOID_ARMS_AT[1])?,
            ],
            humanoid_legs: [
                legs(image, HUMANOID_LEGS_AT[0])?,
                legs(image, HUMANOID_LEGS_AT[1])?,
            ],
            scout_legs: [
                legs(image, SCOUT_LEGS_AT[0])?,
                legs(image, SCOUT_LEGS_AT[1])?,
            ],
            scout_crouch: legs(image, SCOUT_CROUCH_AT)?[..CROUCHES]
                .try_into()
                .expect("4 bytes"),
            scorpion_legs: scorpion_legs(image)?,
        },
        strings: Strings(
            image
                .get(strings..)
                .ok_or_else(|| invalid(strings, "no string segment"))?
                .to_vec(),
        ),
    })
}

/// A walking table in the code segment, through the `mov al, cs:[bx + table]` at `at`.
fn legs(image: &[u8], at: usize) -> Result<[u8; STEPS], FormatError> {
    let table = usize::from(operand(image, at, LEG_OP)?);
    Ok(bytes(image, table, STEPS)?.try_into().expect("16 bytes"))
}

/// The scorpion's six leg tables, each put by the leg its `mov [di + leg], al` stores.
fn scorpion_legs(image: &[u8]) -> Result<[[u8; STEPS]; SCORPION_LEGS], FormatError> {
    let mut tables = [None; SCORPION_LEGS];
    for i in 0..SCORPION_LEGS {
        let at = SCORPION_LEGS_AT + 9 * i;
        let leg = match *bytes(image, at + 5, 4)? {
            [0x88, 0x85, lo, hi] => usize::from(u16::from_le_bytes([lo, hi])),
            _ => return Err(invalid(at + 5, "not a leg's store")),
        };
        let slot = leg
            .checked_sub(0xff)
            .and_then(|k| tables.get_mut(k))
            .filter(|t| t.is_none())
            .ok_or_else(|| invalid(at + 5, "not one of the six legs"))?;
        *slot = Some(legs(image, at)?);
    }
    Ok(tables.map(|t| t.expect("six different legs")))
}

/// A walk's model heights by step, through the `add ax, cs:[bx + table]`, `mov [di +
/// 0x12], ax` at `at` (words, by twice the step).
fn bob(image: &[u8], at: usize) -> Result<[u16; STEPS], FormatError> {
    let table = usize::from(operand(image, at, ADD_TABLE)?);
    if bytes(image, at + 5, 3)? != TO_MODEL_HEIGHT {
        return Err(invalid(at + 5, "not the model height's set"));
    }
    let mut bob = [0; STEPS];
    for (i, b) in bob.iter_mut().enumerate() {
        *b = word(image, table + 2 * i)?;
    }
    Ok(bob)
}

/// The word after the instruction bytes `op` at `at`, followed by `then`.
fn after(image: &[u8], at: usize, op: &[u8], then: &[u8]) -> Result<u16, FormatError> {
    let value = operand(image, at, op)?;
    if bytes(image, at + op.len() + 2, then.len())? != then {
        return Err(invalid(at, "not followed by the expected store"));
    }
    Ok(value)
}

/// The speed a set-speed routine at `at` asks for: `mov word [di + 0x28], speed`,
/// or `mov bl, gear` and a call to a routine that looks the gear up in a word table
/// (`sim_set_humanoid_gear`: `mov [di + x], bl`, then `mov ax, [bx + table]`).
fn script_speed(image: &[u8], ds: usize, at: usize) -> Result<i16, FormatError> {
    match *bytes(image, at, 5)? {
        [0xc7, 0x45, 0x28, lo, hi] => Ok(i16::from_le_bytes([lo, hi])),
        [0xb3, gear, 0xe8, lo, hi] => {
            let callee = (at as u16)
                .wrapping_add(5)
                .wrapping_add(u16::from_le_bytes([lo, hi]));
            let table = operand(image, usize::from(callee) + 4, &[0x8b, 0x87])?;
            Ok(word(image, ds + usize::from(table) + usize::from(gear))? as i16)
        }
        _ => Err(invalid(at, "not a set-speed routine")),
    }
}

/// The gear steps, through the opcodes' routine tables: the humanoid's and scout's
/// up routines (`mov bl, [di + gear]`, `cmp bl, top`, ..., `add bx, 2`, `call set`,
/// where `set` is `mov [di + gear], bl`, `mov ax, [bx + table]`), and the scorpion's
/// (`mov ax, [di + 0x28]`, `cmp ax, limit`, `j..`, `add`/`sub ax, step`; the stock
/// build puts an `int 3` before each).
fn gears(image: &[u8], ds: usize) -> Result<Gears, FormatError> {
    let routine = |op: usize, kind: usize| -> Result<usize, FormatError> {
        let table = ds + usize::from(operand(image, op, &[0xff, 0xa7])?);
        Ok(usize::from(word(image, table + 2 * kind)?))
    };
    let gear_table = |kind: usize| -> Result<Vec<i16>, FormatError> {
        let up = routine(GEAR_OPS[0], kind)?;
        let down = routine(GEAR_OPS[1], kind)?;
        let b = bytes(image, up, 17)?;
        let d = bytes(image, down, 7)?;
        let shape = b[..2] == [0x8a, 0x9d]
            && b[4..6] == [0x80, 0xfb]
            && b[9..15] == [0x32, 0xff, 0x83, 0xc3, 0x02, 0xe8]
            && d[..4] == b[..4]
            && d[4..] == [0x80, 0xfb, 0x00];
        let top = usize::from(b[6]);
        if !shape || top % 2 != 0 || top >= 2 * MAX_GEARS {
            return Err(invalid(up, "not a gear step routine"));
        }
        let set = (up as u16)
            .wrapping_add(17)
            .wrapping_add(u16::from_le_bytes([b[15], b[16]]));
        let set = usize::from(set);
        if bytes(image, set, 4)? != [0x88, 0x9d, b[2], b[3]] {
            return Err(invalid(set, "not a gear setter"));
        }
        let at = ds + usize::from(operand(image, set + 4, &[0x8b, 0x87])?);
        (0..=top / 2)
            .map(|i| Ok(word(image, at + 2 * i)? as i16))
            .collect()
    };
    let scorpion = |op: usize, test: u8, step: u8| -> Result<(i16, i16), FormatError> {
        let mut at = routine(op, 1)?;
        if image.get(at) == Some(&0xcc) {
            at += 1;
        }
        let b = bytes(image, at, 11)?;
        if b[..4] != [0x8b, 0x45, 0x28, 0x3d] || b[6] != test || b[8] != step {
            return Err(invalid(at, "not the scorpion's speed step"));
        }
        let w = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]);
        Ok((w(4), w(9)))
    };
    let (scorpion_top, scorpion_step) = scorpion(GEAR_OPS[0], 0x7d, 0x05)?;
    let (scorpion_bottom, down_step) = scorpion(GEAR_OPS[1], 0x7e, 0x2d)?;
    if down_step != scorpion_step {
        return Err(invalid(
            GEAR_OPS[1],
            "the scorpion steps down by another step",
        ));
    }
    Ok(Gears {
        humanoid: gear_table(0)?,
        scout: gear_table(2)?,
        scorpion_step,
        scorpion_top,
        scorpion_bottom,
        turn: operand(image, KEY_TURN.0, KEY_TURN.1)?,
    })
}

/// The weapon keys' messages and sound, through each kind's routine: the humanoid's
/// and scout's (`mov bl, al`, `cmp bl, [di + 0xdd]`, `jz`, ..., then `mov si, [bx +
/// messages]`, `call`, `mov ax, 2 * sound`) and the scorpion's (`1000:98a9`: the
/// damaged bits, then the loaded, empty and damaged messages at fixed places).
fn weapon_keys(image: &[u8], ds: usize) -> Result<WeaponKeys, FormatError> {
    let table = usize::from(operand(image, WEAPON_KEYS.0, WEAPON_KEYS.1)?);
    let routine = |kind: usize| -> Result<usize, FormatError> {
        Ok(usize::from(word(image, table + 2 * kind)?))
    };
    let words = |at: usize| -> Result<[u16; 2], FormatError> {
        Ok([word(image, ds + at)?, word(image, ds + at + 2)?])
    };
    let sound = |at: usize| -> Result<u8, FormatError> {
        let w = operand(image, at, &[0xb8])?;
        u8::try_from(w / 2)
            .ok()
            .filter(|_| w % 2 == 0)
            .ok_or_else(|| invalid(at, "not a sound event"))
    };
    // The humanoid's and scout's: the messages' load, a call and the sound.
    let plain = |kind: usize| -> Result<([u16; 2], u8), FormatError> {
        let at = routine(kind)?;
        let code = bytes(image, at, 48)?;
        let found = (code[..7] == [0x8a, 0xd8, 0x3a, 0x9d, 0xdd, 0x00, 0x74])
            .then(|| {
                code.windows(8)
                    .position(|w| w[..2] == [0x8b, 0xb7] && w[4] == 0xe8 && w[7] == 0xb8)
            })
            .flatten();
        match found {
            Some(i) => Ok((
                words(usize::from(operand(image, at + i, &[0x8b, 0xb7])?))?,
                sound(at + i + 7)?,
            )),
            None => Err(invalid(at, "not a weapon key routine")),
        }
    };
    let ((humanoid, sound_h), (scout, sound_s)) = (plain(0)?, plain(2)?);
    let at = routine(1)?;
    if bytes(image, at, 9)? != [0x32, 0xe4, 0x8b, 0xf0, 0x3a, 0x85, 0xdd, 0x00, 0x74] {
        return Err(invalid(at, "not the scorpion's weapon key routine"));
    }
    let bits = usize::from(operand(image, at + 0x18, &[0x8a, 0x84])?);
    let message = |offset: usize| words(usize::from(operand(image, at + offset, &[0x8b, 0xb4])?));
    let sound_c = sound(at + 0x40)?;
    if sound_h != sound_c || sound_s != sound_c {
        return Err(invalid(at, "the kinds play other sounds"));
    }
    Ok(WeaponKeys {
        sound: sound_c,
        selected: [humanoid, message(0x2d)?, scout],
        scorpion_empty: message(0x33)?,
        scorpion_damaged: message(0x39)?,
        scorpion_bits: bytes(image, ds + bits, 2)?.try_into().expect("2 bytes"),
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Motion, FormatError> {
    read_image(
        &exe.image,
        sequence::data_segment(exe)?,
        sequence::string_segment(exe)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;
    const STRINGS: usize = 0x18000;

    fn put(image: &mut [u8], at: usize, bytes: &[u8]) {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// Made-up tables: a sine table of `i * 0x100`, turn steps 1, 2 and 3, a weak
    /// drive at 7, two speed tables (the speed itself and half of it) and programs
    /// numbered by mission and kind, an arctangent and script speeds, and the scripts
    /// of `synthetic_scripts`.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0xa000];
        put(&mut image, SINE_NEXT.0, &[0x36, 0x8b, 0x8f, 0x02, 0x01]);
        put(&mut image, SINE_AT.0, &[0x36, 0x8b, 0x9f, 0x00, 0x01]);
        for i in 0..SINE_ENTRIES {
            let value = (i as u16).wrapping_mul(0x100);
            put(&mut image, DS + 0x100 + 2 * i, &value.to_le_bytes());
        }
        for (kind, at) in TURN_AT.into_iter().enumerate() {
            put(&mut image, at, &[0xbb, kind as u8 + 1, 0]);
        }
        put(&mut image, WEAK_DRIVE.0, &[0x80, 0x7d, 0x51, 7]);
        put(&mut image, SPEEDS_SEGMENT.0, SPEEDS_SEGMENT.1);
        put(&mut image, SPEEDS_AT.0, &[0x26, 0x8b, 0x9f, 0x00, 0x02]);
        put(&mut image, STRINGS + 0x200, &[0x04, 0x02, 0x44, 0x02]);
        for i in 0..SPEED_ENTRIES {
            image[STRINGS + 0x204 + i] = i as u8;
            image[STRINGS + 0x244 + i] = i as u8 / 2;
        }
        put(&mut image, PROGRAMS_AT.0, &[0x8a, 0x87, 0x00, 0x04]);
        for i in 0..3 * MISSIONS {
            image[DS + 0x400 + i] = i as u8;
        }
        crate::fixtures::synthetic_scripts(&mut image, DS);
        crate::fixtures::synthetic_damage(&mut image, DS);
        crate::fixtures::synthetic_walk(&mut image);
        crate::fixtures::synthetic_gears(&mut image, DS);
        crate::fixtures::synthetic_weapon_keys(&mut image, DS);
        crate::fixtures::synthetic_sensing(&mut image, DS);
        crate::fixtures::synthetic_net(&mut image);
        crate::fixtures::synthetic_relays(&mut image, DS);
        crate::fixtures::synthetic_weapons(&mut image, DS);
        crate::fixtures::synthetic_bases(&mut image, DS);
        crate::fixtures::synthetic_calendar(&mut image, DS);
        put(&mut image, ATAN_AT.0, &[0x36, 0x8b, 0x87, 0x00, 0x0c]);
        for i in 0..ATAN_ENTRIES {
            put(
                &mut image,
                DS + 0xc00 + 2 * i,
                &((i as u16) << 8).to_le_bytes(),
            );
        }
        // Humanoids by gear (2 * op) through a table of gear * 5; scorpions and
        // scouts by their own words.
        put(
            &mut image,
            0x9100,
            &[0x88, 0x9d, 0x0b, 0x01, 0x8b, 0x87, 0x40, 0x0a],
        );
        for gear in 0..10u16 {
            put(
                &mut image,
                DS + 0xa40 + 2 * usize::from(gear),
                &(gear * 5).to_le_bytes(),
            );
        }
        for (i, at) in SPEED_OPS.into_iter().enumerate() {
            let table = 0xa00 + 6 * i as u16;
            put(
                &mut image,
                at,
                &[&[0xff, 0xa7][..], &table.to_le_bytes()].concat(),
            );
            let routines = [0x9000, 0x9200, 0x9380].map(|r: u16| r + 8 * i as u16);
            for (kind, routine) in routines.into_iter().enumerate() {
                let r = usize::from(routine);
                put(
                    &mut image,
                    DS + usize::from(table) + 2 * kind,
                    &routine.to_le_bytes(),
                );
                if kind == 0 {
                    let call = 0x9100u16.wrapping_sub(routine + 5);
                    put(&mut image, r, &[0xb3, 2 * i as u8, 0xe8]);
                    put(&mut image, r + 3, &call.to_le_bytes());
                } else {
                    let speed = (i as i16 + 1) * if kind == 1 { 7 } else { -3 };
                    put(&mut image, r, &[0xc7, 0x45, 0x28]);
                    put(&mut image, r + 3, &speed.to_le_bytes());
                }
            }
        }
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let motion = read_image(&synthetic(), DS, STRINGS).unwrap();
        assert_eq!(motion.sine.len(), SINE_ENTRIES);
        assert_eq!((motion.sine[1], motion.sine[256]), (0x100, 0));
        assert_eq!(motion.turn_step, [1, 2, 3]);
        assert_eq!(motion.weak_drive, 7);
        assert_eq!(motion.scorpion_speeds.len(), 2);
        assert_eq!(motion.scorpion_speeds[1][63], 31);
        assert_eq!(motion.mission_programs[6], [18, 19, 20]);
        assert_eq!(motion.atan[255], 0xff00);
        assert_eq!(motion.script_speeds[4], [20, 35, -15]);
        assert_eq!(motion.walk.humanoid_legs[1][..3], [2, 3, 0]);
        assert_eq!(motion.walk.humanoid_arms[1][..3], [1, 2, 3]);
        assert_eq!(motion.walk.scout_crouch[3], 3);
        // Leg +0xff + k is `(step + k) % 6`, whatever order the stores come in.
        assert_eq!(motion.walk.scorpion_legs[1][..3], [1, 2, 3]);
        assert_eq!(motion.walk.scorpion_legs[5][..3], [5, 0, 1]);
        let w = &motion.walk;
        assert_eq!((w.humanoid_height, w.humanoid_eye), (0x0a00, 0x0f80));
        assert_eq!((w.humanoid_bob[1], w.scout_bob[15]), (0x10, 0xf0));
        assert_eq!(
            (w.scout_height, w.scout_crouched, w.scout_eye),
            (0x0c80, 0x0c90, 0x0d00)
        );
        assert_eq!(motion.gears.humanoid, [-20, -10, 0, 10, 20]);
        assert_eq!(motion.gears.scout, [-30, 0, 30]);
        let g = &motion.gears;
        assert_eq!(
            (g.scorpion_step, g.scorpion_bottom, g.scorpion_top, g.turn),
            (5, -15, 25, 0x80)
        );
        let w = &motion.weapon_keys;
        assert_eq!((w.sound, motion.recall_program), (5, 6));
        assert_eq!(w.selected, [[0x10, 0x11], [0x20, 0x21], [0x30, 0x31]]);
        assert_eq!(
            (w.scorpion_empty, w.scorpion_damaged),
            ([0x22, 0x23], [0x24, 0x25])
        );
        assert_eq!(w.scorpion_bits, [1, 2]);
        assert_eq!(motion.strings.byte(0x201), 0x02);
        assert_eq!(motion.strings.byte(0xffff), 0);
        insta::assert_debug_snapshot!(motion);
    }

    #[test]
    fn another_build_is_refused() {
        for (at, value) in [
            (SINE_AT.0, 0x90),
            (SINE_NEXT.0 + 3, 0x04),
            (TURN_AT[1], 0xb8),
            (WEAK_DRIVE.0 + 2, 0x50),
            (SPEEDS_SEGMENT.0 + 2, 0x62),
            (SPEEDS_AT.0 + 1, 0x8a),
            (STRINGS + 0x200, 0x03),
            (PROGRAMS_AT.0, 0x8b),
            (ATAN_AT.0, 0x8b),
            (SPEED_OPS[2], 0xfe),
            (0x9000, 0xb4),
            (0x9104, 0x8a),
            (HUMANOID_LEGS_AT[1], 0x3e),
            (SCOUT_LEGS_AT[0] + 2, 0x86),
            (SCOUT_CROUCH_AT + 1, 0x8b),
            (HUMANOID_BOB_AT, 0xb9),
            (HUMANOID_BOB_AT + 9, 0x13),
            (HUMANOID_BOB_AT + 16, 0x1d),
            (SCOUT_BOB_AT + 6, 0x02),
            (SCOUT_CROUCHED_AT + 1, 0xd9),
            (SCOUT_CROUCHED_AT + 6, 0x1c),
            (SCOUT_EYE_AT, 0x99),
            (SCOUT_EYE_AT + 14, 0x12),
            (GEAR_OPS[1] + 1, 0xa6),
            (0x8400 + 6, 7),
            (0x8418 + 6, 2),
            (0x8420 + 14, 0xe9),
            (0x8448 + 2, 0x0b),
            (0x8450 + 9, 0x2d),
            (0x8460 + 9, 4),
            (KEY_TURN.0, 0x05),
            (WEAPON_KEYS.0 + 2, 0xa6),
            (0x8490 + 3, 0x85),
            (0x84c0 + 0x08 + 7, 0xb9),
            (0x84c0 + 0x08 + 8, 0x0b),
            (0x84f0 + 1, 0xe5),
            (0x84f0 + 0x2d, 0x8a),
            (0x84f0 + 0x40 + 1, 0x0c),
            (RECALL_AT + 3, 0x10),
            (RECALL_AT + 6, 0x84),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS, STRINGS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables have the shapes the docs describe: a rising quarter sine that
    /// reaches 0xffff, one turn step for every kind, speed tables that only slow a
    /// scorpion, and programs in the 31-entry program table.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_motion_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let motion = read(&exe).unwrap();
        assert_eq!(motion.sine[0], 0);
        assert!(motion.sine[..256].windows(2).all(|w| w[0] < w[1]));
        assert_eq!(motion.sine[255], 0xffff);
        assert!(motion.turn_step.iter().all(|&s| s == motion.turn_step[0]));
        assert!(!motion.scorpion_speeds.is_empty());
        for table in &motion.scorpion_speeds {
            assert!(table.windows(2).all(|w| w[0] <= w[1]));
            assert!(table.iter().enumerate().all(|(i, &s)| usize::from(s) <= i));
        }
        assert!(motion.mission_programs.iter().flatten().all(|&p| p < 31));
        // A rising arctangent from 0 towards an eighth of a turn.
        assert_eq!(motion.atan[0], 0);
        assert!(motion.atan.windows(2).all(|w| w[0] < w[1]));
        // Opcode 0x54 is each kind's top speed, 0x58 its fastest reverse.
        assert!(motion.script_speeds[0].iter().all(|&s| s > 0));
        assert!(motion.script_speeds[4].iter().all(|&s| s < 0));
        // Leg frames within their animations (humanoid 5 frames, scout 12), each leg
        // coming back to 0 in a stride.
        let w = &motion.walk;
        for legs in &w.humanoid_legs {
            assert!(legs.iter().all(|&f| f < 5) && legs.contains(&0));
        }
        // The bobs stay within a 256th of a unit over their bases, down to 0 in a
        // stride; crouching starts from the scout's standing model height.
        for bob in [&w.humanoid_bob, &w.scout_bob] {
            assert!(bob.iter().all(|&b| b <= 0x100) && bob.contains(&0));
        }
        assert!(w.humanoid_eye > w.humanoid_height && w.scout_crouched == w.scout_height);
        // The arms swing below their raised frame, 5.
        assert!(w.humanoid_arms.iter().flatten().all(|&f| f < 5));
        let scout = w.scout_legs.iter().flatten().chain(&w.scout_crouch);
        assert!(scout.into_iter().all(|&f| f < 12));
        assert!(w.scout_legs.iter().all(|legs| legs.contains(&0)));
        // The scorpion's six run through their 6 frames, each leg at a different phase.
        for legs in &w.scorpion_legs {
            assert!((0..6).all(|f| legs.contains(&f)) && legs.iter().all(|&f| f < 6));
        }
        assert!((1..6).all(|k| w.scorpion_legs[k] != w.scorpion_legs[0]));
        // Gears rise from reverse through a stop, and the scripts' speeds are gears;
        // the scorpion's steps meet its script speeds' ends.
        let g = &motion.gears;
        for (k, gears) in [(0, &g.humanoid), (2, &g.scout)] {
            assert!(gears.windows(2).all(|w| w[0] < w[1]) && gears.contains(&0));
            assert!(motion.script_speeds.iter().all(|s| gears.contains(&s[k])));
        }
        assert_eq!(g.scorpion_top, motion.script_speeds[0][1]);
        assert_eq!(g.scorpion_bottom, motion.script_speeds[4][1]);
        assert!(g.turn > 0 && g.turn < 0x4000);
        // The weapon keys' messages are text in the string segment, and the recall
        // gives a program in the table.
        let w = &motion.weapon_keys;
        let messages = w
            .selected
            .iter()
            .chain([&w.scorpion_empty, &w.scorpion_damaged]);
        for &m in messages.flatten() {
            let text: Vec<u8> = (m..)
                .map(|o| motion.strings.byte(o))
                .take_while(|&b| b != 0)
                .collect();
            assert!(
                text.len() > 2 && text.iter().all(|b| (0x20..0x7f).contains(&(b & 0x7f))),
                "{m:#x}"
            );
        }
        assert!(w.scorpion_bits.iter().all(|b| b.count_ones() == 1));
        assert!(motion.recall_program < 31);
    }
}
