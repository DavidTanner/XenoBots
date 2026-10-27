//! The instrument bank of `sounddvr.dvr`'s music driver: what each `.ms3` program
//! means on an AdLib (OPL2 patches, a frequency table, the voice and operator maps)
//! and on every back end (transpose, velocity factor, pitch envelope), plus the
//! Tandy envelope pointers and the MPU-401 program map (`docs/formats/sounddvr.md`).
//! Game data, so it is read from the user's own copy at run time.
//!
//! `sounddvr.dvr` is a plain MZ file whose code keeps its tables in a data segment
//! (`DS`). Each table is found through the instruction that indexes it, after checking
//! that instruction's bytes, so another build of the driver is refused rather than
//! misread. Table sizes are the stock driver's: the gaps between the tables.

use binrw::{BinRead, binrw};

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{invalid, word};
use crate::vgaintro::load_image;

/// `mov ds, cs:[x]` at the start of the song loader: `x` holds `DS`'s paragraph.
pub const DS_AT: (usize, &[u8]) = (0x0b2e, &[0x2e, 0x8e, 0x1e]);

/// Where each table's `DS` offset is read: (name, load-image offset of the
/// instruction, its bytes before the offset).
pub const TABLES: [(&str, usize, &[u8]); 11] = [
    // add bx, patches (in `opl_set_program`, after `bx = program * 16`)
    ("patches", 0x0fed, &[0x81, 0xc3]),
    // add bx, frequencies (in `opl_set_pitch`, after `bx = pitch * 2`)
    ("frequencies", 0x10f7, &[0x81, 0xc3]),
    // mov dx, [bx + envelopes] (in `snd_load_channel_program`, and the four below)
    ("pitch_envelopes", 0x0d49, &[0x8b, 0x97]),
    ("tandy_attack", 0x0d59, &[0x8b, 0x87]),
    ("tandy_release", 0x0d61, &[0x8b, 0x87]),
    ("velocity_factor", 0x0d69, &[0x3e, 0x8a, 0x86]),
    ("transpose", 0x0d72, &[0x3e, 0x8a, 0x86]),
    // mov cl, [bx + map] (the MPU-401 program change)
    ("midi_programs", 0x0e91, &[0x8a, 0x8f]),
    // mov ch, [bx + voices] (in `opl_note_on_off`)
    ("channel_voices", 0x111d, &[0x8a, 0xaf]),
    // mov dl, [si + modulators] (in `opl_set_program`)
    ("modulator_slots", 0x1000, &[0x8a, 0x94]),
    // mov dh, [si + carriers] (in `opl_note_on_off`)
    ("carrier_slots", 0x112c, &[0x8a, 0xb4]),
];

/// OPL2 patches in the bank; programs past them read the frequency table.
pub const PATCHES: usize = 128;
/// Entries in the per-program tables (programs 0 to 129).
pub const PROGRAMS: usize = 130;
/// Frequency words, one for each pitch byte.
pub const PITCHES: usize = 256;
/// Music channels (`.ms3` channels 0 to 9) and OPL2 melodic voices.
pub const CHANNELS: usize = 10;
pub const VOICES: usize = 9;
/// A pitch envelope byte that holds the pitch at its base, and one that restarts.
pub const ENVELOPE_END: u8 = 0x80;
pub const ENVELOPE_LOOP: u8 = 0x81;
/// No stock envelope is this long; a longer one is a misread.
const MAX_ENVELOPE: usize = 256;

/// One program's OPL2 patch, 16 bytes, as `opl_set_program` writes it. "Carrier" is
/// operator 2 of the voice (the one the note's velocity sets the level of).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    /// Register 0x20 (tremolo, vibrato, sustain, key scaling, multiplier) of the carrier.
    pub carrier_20: u8,
    /// Carrier's register 0x40 before the velocity is ORed in at note on (key-scale
    /// level in bits 6 and 7).
    pub carrier_40: u8,
    /// Registers 0x60 (attack, decay), 0x80 (sustain, release) and 0xe0 (waveform).
    pub carrier_60: u8,
    pub carrier_80: u8,
    pub carrier_e0: u8,
    pub modulator_20: u8,
    /// Written to register 0x40 XOR 0x3f: a level (0x3f loudest), key scaling above.
    pub modulator_level: u8,
    pub modulator_60: u8,
    pub modulator_80: u8,
    pub modulator_e0: u8,
    /// Register 0xc0 (feedback, connection).
    pub c0: u8,
    /// Never read by the driver (non-zero in every stock patch).
    pub unknown_0b: [u8; 5],
}

/// A program's pitch envelope: one byte a tick, added to the pitch that tick only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The offsets in half-semitones, one a tick from the note's start.
    pub steps: Vec<i8>,
    /// After the last step: true restarts at the first, false holds offset 0.
    pub looped: bool,
}

/// The bank, every table in program (or pitch, channel, voice) order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    pub patches: Vec<Patch>,
    /// By pitch byte: OPL2 block in bits 10 to 12, F-number in bits 0 to 9.
    pub frequencies: Vec<u16>,
    pub pitch_envelopes: Vec<Envelope>,
    /// `DS` offsets of each program's Tandy volume scripts (note on, note off).
    pub tandy_attack: Vec<u16>,
    pub tandy_release: Vec<u16>,
    /// Velocity scale, `velocity * 2 * factor / 256`.
    pub velocity_factor: Vec<u8>,
    /// Added to twice the note number.
    pub transpose: Vec<i8>,
    /// MPU-401 program number plus 1, by program.
    pub midi_programs: Vec<u8>,
    /// The OPL2 voice of each channel; 9 or more plays nothing.
    pub channel_voices: Vec<u8>,
    /// Each voice's operator offsets (add to registers 0x20 to 0xe0).
    pub modulator_slots: Vec<u8>,
    pub carrier_slots: Vec<u8>,
}

/// Reads the bank from the bytes of `sounddvr.dvr`.
pub fn parse(bytes: &[u8]) -> Result<Bank, FormatError> {
    let image = load_image(bytes)?;
    let (at, op) = DS_AT;
    let ds = usize::from(word(image, usize::from(operand(image, at, op)?))?) * 16;
    // Each table as a slice of `n` items of `size` bytes, from its instruction.
    let table = |i: usize, n: usize, size: usize| -> Result<&[u8], FormatError> {
        let (name, at, op) = TABLES[i];
        let start = ds + usize::from(operand(image, at, op)?);
        image
            .get(start..start + n * size)
            .ok_or_else(|| invalid(at, format!("the {name} table is past the image")))
    };
    let words = |b: &[u8]| -> Vec<u16> {
        b.chunks(2)
            .map(|w| u16::from_le_bytes([w[0], w[1]]))
            .collect()
    };
    let signed = |b: &[u8]| b.iter().map(|&b| b as i8).collect();
    let patches = table(0, PATCHES, 16)?
        .chunks(16)
        .map(|p| Patch::read(&mut std::io::Cursor::new(p)))
        .collect::<Result<_, _>>()?;
    let pitch_envelopes = words(table(2, PROGRAMS, 2)?)
        .into_iter()
        .map(|at| envelope(image, ds + usize::from(at)))
        .collect::<Result<_, _>>()?;
    Ok(Bank {
        patches,
        frequencies: words(table(1, PITCHES, 2)?),
        pitch_envelopes,
        tandy_attack: words(table(3, PROGRAMS, 2)?),
        tandy_release: words(table(4, PROGRAMS, 2)?),
        velocity_factor: table(5, PROGRAMS, 1)?.to_vec(),
        transpose: signed(table(6, PROGRAMS, 1)?),
        midi_programs: table(7, PATCHES, 1)?.to_vec(),
        channel_voices: table(8, CHANNELS, 1)?.to_vec(),
        modulator_slots: table(9, VOICES, 1)?.to_vec(),
        carrier_slots: table(10, VOICES, 1)?.to_vec(),
    })
}

/// The envelope script at `start`, up to its end or loop byte.
fn envelope(image: &[u8], start: usize) -> Result<Envelope, FormatError> {
    let script = image.get(start..).unwrap_or_default();
    let end = script
        .iter()
        .take(MAX_ENVELOPE)
        .position(|&b| b == ENVELOPE_END || b == ENVELOPE_LOOP)
        .ok_or_else(|| invalid(start, "a pitch envelope with no end"))?;
    Ok(Envelope {
        steps: script[..end].iter().map(|&b| b as i8).collect(),
        looped: script[end] == ENVELOPE_LOOP,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_sounddvr;

    #[test]
    fn reads_every_table_through_its_instruction() {
        let bank = parse(&synthetic_sounddvr()).unwrap();
        assert_eq!(bank.patches.len(), PATCHES);
        assert_eq!(bank.patches[3].carrier_20, 3);
        assert_eq!(bank.patches[3].c0, 13);
        assert_eq!(bank.patches[3].unknown_0b, [14, 15, 16, 17, 18]);
        assert_eq!(bank.frequencies[0x30], 0x1030);
        assert_eq!(bank.transpose[2], -2);
        assert_eq!(bank.velocity_factor[129], 129);
        assert_eq!(bank.midi_programs[127], 128);
        assert_eq!(bank.channel_voices, [0, 1, 2, 3, 4, 5, 6, 7, 99, 8]);
        assert_eq!(bank.carrier_slots[8], 21);
        let flat = Envelope {
            steps: vec![0],
            looped: false,
        };
        let vibrato = Envelope {
            steps: vec![2, -2],
            looped: true,
        };
        assert_eq!(bank.pitch_envelopes[0], vibrato);
        assert_eq!(bank.pitch_envelopes[1], flat);
    }

    #[test]
    fn bank_snapshot() {
        let bank = parse(&synthetic_sounddvr()).unwrap();
        // The tables are long; the first entries of each show the shape.
        let short = Bank {
            patches: bank.patches[..2].to_vec(),
            frequencies: bank.frequencies[..4].to_vec(),
            pitch_envelopes: bank.pitch_envelopes[..2].to_vec(),
            tandy_attack: bank.tandy_attack[..2].to_vec(),
            tandy_release: bank.tandy_release[..2].to_vec(),
            velocity_factor: bank.velocity_factor[..2].to_vec(),
            transpose: bank.transpose[..4].to_vec(),
            midi_programs: bank.midi_programs[..2].to_vec(),
            ..bank
        };
        insta::assert_debug_snapshot!(short);
    }

    #[test]
    fn another_build_is_refused() {
        let file = synthetic_sounddvr();
        // Each table's instruction, and the `DS` load, must be the expected one.
        for at in TABLES.iter().map(|t| t.1).chain([DS_AT.0]) {
            let mut bad = file.clone();
            bad[0x20 + at] ^= 0xff;
            assert!(parse(&bad).is_err(), "instruction at {at:#x}");
        }
        // An envelope that never ends.
        let mut bad = file.clone();
        let start = bad.len() - 0x40;
        bad[start..].fill(1);
        assert!(parse(&bad).is_err());
        assert!(parse(&file[..0x1000]).is_err());
        assert!(parse(b"ZM").is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_sounddvr_bank_parses() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let bank = parse(&std::fs::read(dir.join("sounddvr.dvr")).unwrap()).unwrap();
        // Structure only: nine voices on two operators each, all distinct; the
        // frequencies rise with pitch within a block; patches keep the 0xc0 register's
        // reserved bits clear.
        let mut slots = [bank.modulator_slots.clone(), bank.carrier_slots.clone()].concat();
        slots.sort();
        slots.dedup();
        assert_eq!(slots.len(), 2 * VOICES);
        assert!(slots.iter().all(|&s| s < 0x16));
        assert!(bank.channel_voices.iter().filter(|&&v| v < 9).count() >= 8);
        assert!(bank.frequencies.windows(2).take(24).all(|w| w[0] < w[1]));
        assert!(bank.patches.iter().all(|p| p.c0 < 0x10));
    }
}
