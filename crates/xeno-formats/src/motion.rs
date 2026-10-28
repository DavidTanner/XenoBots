//! What the port reads from the user's `xeno.exe` to move robots (`docs/sim.md`,
//! "Movement"): the sine table the velocity comes from, each kind's turn step, the
//! drive level under which a robot goes at half speed, the scorpion's speed tables, and
//! the program each path mission gives each kind, and the leg frames of the walking
//! animations (`docs/render.md`, "Walking"). Game data, read at run time; each
//! table is found through an instruction that uses it, after checking that
//! instruction's bytes.

use crate::damage::{self, Damage};
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};
use crate::script::{self, Scripts};
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

/// `mov al, cs:[bx + table]` in the walking animations: the humanoid's legs (+0x101,
/// +0x102), the scout's (+0x103, +0x104) and the scout's crouched legs.
pub(crate) const HUMANOID_LEGS_AT: [usize; 2] = [0xadbe, 0xadcc];
pub(crate) const SCOUT_LEGS_AT: [usize; 2] = [0xc7bf, 0xc7d1];
pub(crate) const SCOUT_CROUCH_AT: usize = 0xc829;
const LEG_OP: &[u8] = &[0x2e, 0x8a, 0x87];
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
    /// The robots' scripts (`xeno_formats::script`).
    pub scripts: Scripts,
    /// What damage does (`xeno_formats::damage`).
    pub damage: Damage,
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

/// The leg frames of the walking animations that make footsteps, by step
/// (`docs/render.md`, "Walking").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// The humanoid's legs, +0x101 and +0x102.
    pub humanoid_legs: [[u8; STEPS]; 2],
    /// The scout's legs, +0x103 and +0x104 (+0xff and +0x100 follow them).
    pub scout_legs: [[u8; STEPS]; 2],
    /// A scout's four legs when it stands still crouched, by stance / 16.
    pub scout_crouch: [u8; CROUCHES],
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
        scripts: script::read_image(image, ds)?,
        damage: damage::read_image(image, ds)?,
        walk: Walk {
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
        let mut image = vec![0u8; DS + 0x9c00];
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
            let routines = [0x9000, 0x9200, 0x9300].map(|r: u16| r + 8 * i as u16);
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
        assert_eq!(motion.walk.scout_crouch[3], 3);
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
        let scout = w.scout_legs.iter().flatten().chain(&w.scout_crouch);
        assert!(scout.into_iter().all(|&f| f < 12));
        assert!(w.scout_legs.iter().all(|legs| legs.contains(&0)));
    }
}
