//! `dsounds.bin`, the digitised sound effects: a list of blocks, each holding one
//! unsigned 8-bit mono sample and the step `sounddvr.dvr` plays it at. See
//! `docs/formats/dsounds.md`.

use std::fmt;

use binrw::{BinRead, binrw};

use crate::FormatError;

/// The whole file: the sounds in the order the driver loads them into its slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    pub sounds: Vec<Sound>,
    /// Bytes after the terminating zero word. None in the stock file.
    pub trailing: Vec<u8>,
}

/// The first four bytes of a block.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Offset in the block where playback stops (exclusive). Samples start at 4.
    pub end: u16,
    /// How far the play position moves per driver tick, in 1/65536ths of a sample.
    pub step: u16,
}

/// One sound: a block of whole 16-byte paragraphs.
#[derive(Clone, PartialEq, Eq)]
pub struct Sound {
    pub header: Header,
    /// Unsigned 8-bit samples, silence at 0x80: block bytes `4..end`.
    pub samples: Vec<u8>,
    /// Block bytes from `end` to the paragraph boundary. The driver never plays them;
    /// in the stock file they look like leftovers of the authoring tool's buffer.
    pub unknown_tail: Vec<u8>,
}

impl Sound {
    /// The rate the driver plays the samples at, in whole Hz: its tick rate times
    /// `step / 65536`.
    pub fn sample_rate(&self) -> u32 {
        let num = PIT_HZ * u64::from(self.header.step);
        let den = u64::from(PIT_DIVISOR) * 65_536;
        ((num + den / 2) / den) as u32
    }
}

/// Short form, so a snapshot shows the samples as one hex line.
impl fmt::Debug for Sound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
        f.debug_struct("Sound")
            .field("header", &self.header)
            .field("samples", &format_args!("{}", hex(&self.samples)))
            .field("unknown_tail", &format_args!("{}", hex(&self.unknown_tail)))
            .finish()
    }
}

/// The PC's programmable interval timer input clock, in Hz (rounded).
pub const PIT_HZ: u64 = 1_193_182;
/// The divisor `sounddvr.dvr` programs into PIT channel 0: one tick is one output
/// sample, about 7231 Hz.
pub const PIT_DIVISOR: u16 = 165;
/// The driver's slot table holds this many sounds; it refuses a file with more.
pub const MAX_SOUNDS: usize = 17;

/// Decodes the file: blocks of `u16` paragraph count then that many paragraphs, until a
/// zero count.
pub fn parse(bytes: &[u8]) -> Result<Bank, FormatError> {
    let mut sounds = Vec::new();
    let mut at = 0;
    loop {
        let count = bytes.get(at..at + 2).ok_or_else(|| FormatError::Invalid {
            offset: at as u64,
            reason: "missing the terminating zero word".into(),
        })?;
        let paragraphs = usize::from(u16::from_le_bytes([count[0], count[1]]));
        at += 2;
        if paragraphs == 0 {
            break;
        }
        let invalid = |reason: String| FormatError::Invalid {
            offset: at as u64,
            reason,
        };
        if sounds.len() == MAX_SOUNDS {
            return Err(invalid(format!("more than {MAX_SOUNDS} sounds")));
        }
        let block = bytes
            .get(at..at + paragraphs * 16)
            .ok_or_else(|| invalid(format!("block of {paragraphs} paragraphs is cut short")))?;
        let header = Header::read(&mut std::io::Cursor::new(block))?;
        let end = usize::from(header.end);
        if !(4..=block.len()).contains(&end) {
            return Err(invalid(format!("end {end} outside the block")));
        }
        sounds.push(Sound {
            header,
            samples: block[4..end].to_vec(),
            unknown_tail: block[end..].to_vec(),
        });
        at += block.len();
    }
    Ok(Bank {
        sounds,
        trailing: bytes[at..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_dsounds;

    #[test]
    fn synthetic_bank_decodes() {
        let bytes = synthetic_dsounds(&[(0xffff, &[0x80, 0xff, 0x00]), (0x4000, &[1; 12])]);
        let bank = parse(&bytes).unwrap();
        assert_eq!(bank.sounds.len(), 2);
        assert_eq!(bank.sounds[0].samples, [0x80, 0xff, 0x00]);
        assert_eq!(bank.sounds[0].unknown_tail.len(), 16 - 7);
        // 12 samples and the header fill one paragraph exactly.
        assert_eq!(bank.sounds[1].unknown_tail, []);
        assert!(bank.trailing.is_empty());
    }

    #[test]
    fn bank_snapshot() {
        let bytes = synthetic_dsounds(&[(0xffff, &[0x80, 0x90, 0x70]), (0x4000, &[0x81; 13])]);
        insta::assert_debug_snapshot!(parse(&bytes).unwrap());
    }

    #[test]
    fn rates_follow_the_step() {
        let bytes = synthetic_dsounds(&[(0xffff, &[]), (0x4000, &[]), (0, &[])]);
        let rates: Vec<_> = parse(&bytes)
            .unwrap()
            .sounds
            .iter()
            .map(Sound::sample_rate)
            .collect();
        assert_eq!(rates, [7231, 1808, 0]);
    }

    #[test]
    fn bad_files_are_rejected() {
        let good = synthetic_dsounds(&[(0xffff, &[1, 2, 3])]);
        // No terminator, a short block, and an end past the block.
        assert!(parse(&good[..good.len() - 2]).is_err());
        assert!(parse(&good[..10]).is_err());
        let mut bad = good.clone();
        bad[2] = 17;
        assert!(matches!(
            parse(&bad),
            Err(FormatError::Invalid { offset: 2, .. })
        ));
        let too_many = synthetic_dsounds(&vec![(1, &[][..]); MAX_SOUNDS + 1]);
        assert!(parse(&too_many).is_err());
        assert_eq!(parse(&[0, 0]).unwrap().sounds, []);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_dsounds_decode() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let bank = parse(&std::fs::read(dir.join("dsounds.bin")).unwrap()).unwrap();
        assert_eq!(bank.sounds.len(), MAX_SOUNDS);
        assert!(bank.trailing.is_empty());
        for (i, sound) in bank.sounds.iter().enumerate() {
            // Unsigned samples hovering around the 0x80 midpoint, at a sane rate.
            let mean = sound.samples.iter().map(|&s| u64::from(s)).sum::<u64>()
                / sound.samples.len() as u64;
            assert!((0x70..=0x90).contains(&mean), "{i}: mean {mean:#x}");
            assert!((1000..=8000).contains(&sound.sample_rate()), "{i}");
        }
    }
}
