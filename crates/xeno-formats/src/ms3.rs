//! `.ms3` music: a 48-byte header, then one MIDI-like stream of events for up to 16
//! channels, each event followed by a delay in ticks. It is not an OPL register dump:
//! the notes name programs from an instrument bank inside `sounddvr.dvr`, which plays
//! them on an AdLib (OPL2) or sends them to an MPU-401 MIDI device. See
//! `docs/formats/ms3.md`.

use std::fmt;

use binrw::{BinRead, binrw};

use crate::FormatError;

/// The fixed start of every file.
#[binrw]
#[brw(little)]
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    /// Text naming the format (`MS3-KGF'91` or `MS3 V2 - KGF` in the stock files),
    /// ending in 0x1a so DOS `type` stops there. The driver never reads it.
    pub signature: [u8; 14],
    /// 0xff in every stock file. The driver never reads it.
    pub unknown_0e: u8,
    /// 0x00, 0x01 or 0x80 in the stock files. The driver never reads it.
    pub unknown_0f: u8,
    /// Per channel: 0, or one more than the channel that also plays every note played
    /// on this one (it can chain on). All zero in the stock files.
    pub doubled_on: [u8; 16],
    /// Per channel: the program (instrument number in the driver's bank) the channel
    /// starts with.
    pub programs: [u8; 16],
}

impl Header {
    /// Bytes in the header; the events start here.
    pub const SIZE: usize = 0x30;

    /// The signature as text, up to the 0x1a end marker, with control characters
    /// dropped.
    pub fn signature_text(&self) -> String {
        self.signature
            .iter()
            .take_while(|&&b| b != 0x1a)
            .filter(|b| b.is_ascii_graphic() || **b == b' ')
            .map(|&b| char::from(b))
            .collect::<String>()
            .trim()
            .to_string()
    }
}

/// Compact form: the signature as an escaped string, each table on one line.
impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("signature", &self.signature.escape_ascii().to_string())
            .field("unknown_0e", &self.unknown_0e)
            .field("unknown_0f", &self.unknown_0f)
            .field("doubled_on", &format_args!("{:?}", self.doubled_on))
            .field("programs", &format_args!("{:?}", self.programs))
            .finish()
    }
}

/// One event, as `sounddvr.dvr` reads it. The high nibble of the status byte picks
/// the kind and the low nibble is the channel. There is no running status: every
/// event starts with its status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `9n note velocity`: start `note` (MIDI numbering, 60 = middle C).
    NoteOn { channel: u8, note: u8, velocity: u8 },
    /// `8n`, with no data bytes: stop the channel's note.
    NoteOff { channel: u8 },
    /// `Cn program`: switch the channel to another instrument.
    Program { channel: u8, program: u8 },
    /// `En offset`, one data byte (not MIDI's two): added to the channel's pitch, in
    /// half-semitones, from the next tick on. Not used by the stock files.
    PitchOffset { channel: u8, offset: i8 },
    /// `Bn a b`: two data bytes that the driver skips. Not used by the stock files.
    Controller {
        channel: u8,
        unknown_1: u8,
        unknown_2: u8,
    },
    /// Any other status (`0x00..=0x7f`, `An`, `Dn`, `Fn`, and `FF` not followed by
    /// `2F`): one data byte that the driver skips. Not used by the stock files.
    Skipped { status: u8, unknown_1: u8 },
}

/// An event and the delay after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub event: Event,
    /// Ticks to wait before the next event; 0 runs it in the same tick.
    pub delay: u16,
}

/// A decoded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    pub header: Header,
    pub steps: Vec<Step>,
    /// Bytes after the end marker, which the driver never reads (one zero byte, the
    /// MIDI meta length, in the stock files).
    pub unknown_trailer: Vec<u8>,
}

impl Song {
    /// `FF 2F`, MIDI's end-of-track, ends the events. The song then stops or, if the
    /// game asked for it, starts again.
    pub const END: [u8; 2] = [0xff, 0x2f];

    /// The song's length in ticks.
    pub fn ticks(&self) -> u32 {
        self.steps.iter().map(|s| u32::from(s.delay)).sum()
    }
}

/// Decodes a file, reading the events exactly as `sounddvr.dvr` does.
pub fn parse(bytes: &[u8]) -> Result<Song, FormatError> {
    if bytes.len() < Header::SIZE {
        return Err(FormatError::Invalid {
            offset: bytes.len() as u64,
            reason: format!("shorter than the {}-byte header", Header::SIZE),
        });
    }
    let header = Header::read(&mut std::io::Cursor::new(bytes))?;
    let mut steps = Vec::new();
    let mut at = Header::SIZE;
    let byte = |at: usize| {
        bytes.get(at).copied().ok_or_else(|| FormatError::Invalid {
            offset: at as u64,
            reason: "events end without the FF 2F end marker".into(),
        })
    };
    loop {
        let status = byte(at)?;
        if status == Song::END[0] && byte(at + 1)? == Song::END[1] {
            return Ok(Song {
                header,
                steps,
                unknown_trailer: bytes[at + 2..].to_vec(),
            });
        }
        let channel = status & 0x0f;
        let (event, len) = match status & 0xf0 {
            0x90 => {
                let (note, velocity) = (byte(at + 1)?, byte(at + 2)?);
                let event = Event::NoteOn {
                    channel,
                    note,
                    velocity,
                };
                (event, 3)
            }
            0x80 => (Event::NoteOff { channel }, 1),
            0xc0 => {
                let program = byte(at + 1)?;
                (Event::Program { channel, program }, 2)
            }
            0xe0 => {
                let offset = byte(at + 1)? as i8;
                (Event::PitchOffset { channel, offset }, 2)
            }
            0xb0 => {
                let (unknown_1, unknown_2) = (byte(at + 1)?, byte(at + 2)?);
                let event = Event::Controller {
                    channel,
                    unknown_1,
                    unknown_2,
                };
                (event, 3)
            }
            _ => {
                let unknown_1 = byte(at + 1)?;
                (Event::Skipped { status, unknown_1 }, 2)
            }
        };
        at += len;
        // One byte, or two when the first has bit 7 set: its low 7 bits, shifted left
        // by 7, ORed with the whole second byte (the driver does not mask it).
        let first = byte(at)?;
        let delay = if first & 0x80 == 0 {
            at += 1;
            u16::from(first)
        } else {
            at += 2;
            u16::from(first & 0x7f) << 7 | u16::from(byte(at - 1)?)
        };
        steps.push(Step { event, delay });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_ms3;

    /// A made-up tune on two channels with one of each event kind.
    fn tune() -> Vec<u8> {
        synthetic_ms3(
            &[5, 40],
            &[
                0x90, 60, 100, 0x00, // note on, same tick
                0x91, 64, 90, 0x18, // note on, 24 ticks
                0x80, 0x00, // note off
                0xc0, 41, 0x00, // program change
                0xe1, 0xfe, 0x00, // pitch offset -2
                0xb0, 7, 127, 0x00, // controller (skipped)
                0xd1, 9, 0x00, // aftertouch (skipped)
                0x90, 62, 100, 0x81, 0x44, // two-byte delay: 196
                0x81, 0x00, 0x80, 0x00, // note off, then note off
                0xff, 0x2f, 0x00,
            ],
        )
    }

    #[test]
    fn synthetic_song_decodes() {
        let song = parse(&tune()).unwrap();
        assert_eq!(song.header.signature_text(), "MS3-TEST");
        assert_eq!(song.header.programs[..3], [5, 40, 0]);
        assert_eq!(song.steps.len(), 10);
        assert_eq!(
            song.steps[1],
            Step {
                event: Event::NoteOn {
                    channel: 1,
                    note: 64,
                    velocity: 90
                },
                delay: 24
            }
        );
        assert_eq!(
            song.steps[4].event,
            Event::PitchOffset {
                channel: 1,
                offset: -2
            }
        );
        assert_eq!(song.steps[7].delay, 196);
        assert_eq!(song.ticks(), 24 + 196);
        assert_eq!(song.unknown_trailer, [0]);
    }

    #[test]
    fn song_snapshot() {
        insta::assert_debug_snapshot!(parse(&tune()).unwrap());
    }

    #[test]
    fn second_delay_byte_is_not_masked() {
        let song = parse(&synthetic_ms3(&[], &[0x80, 0x82, 0x85, 0xff, 0x2f])).unwrap();
        assert_eq!(song.steps[0].delay, 0x185);
    }

    #[test]
    fn ff_without_2f_is_skipped() {
        let song = parse(&synthetic_ms3(&[], &[0xff, 0x01, 0x00, 0xff, 0x2f])).unwrap();
        assert_eq!(
            song.steps[0].event,
            Event::Skipped {
                status: 0xff,
                unknown_1: 1
            }
        );
        assert!(song.unknown_trailer.is_empty());
    }

    #[test]
    fn truncated_files_are_rejected() {
        let tune = tune();
        for end in [
            0,
            Header::SIZE - 1,
            Header::SIZE,
            Header::SIZE + 2,
            tune.len() - 2,
        ] {
            assert!(
                matches!(parse(&tune[..end]), Err(FormatError::Invalid { .. })),
                "{end}"
            );
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_ms3_songs_decode() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for name in [
            "bigwin3.ms3",
            "lose.ms3",
            "mainthem.ms3",
            "robot3.ms3",
            "win1.ms3",
            "win2.ms3",
        ] {
            let song = parse(&std::fs::read(dir.join(name)).unwrap()).unwrap();
            assert!(song.header.signature_text().starts_with("MS3"), "{name}");
            assert_eq!(song.unknown_trailer, [0], "{name}");
            // Only notes and program changes, at most nine channels (the OPL2's
            // melodic voices), and every note started is stopped.
            let (mut on, mut off) = (0, 0);
            for step in &song.steps {
                match step.event {
                    Event::NoteOn { channel, .. } => {
                        on += 1;
                        assert!(channel < 9, "{name}");
                    }
                    Event::NoteOff { channel } => {
                        off += 1;
                        assert!(channel < 9, "{name}");
                    }
                    Event::Program { channel, .. } => assert!(channel < 9, "{name}"),
                    other => panic!("{name}: {other:?}"),
                }
            }
            assert!(on > 0 && on == off, "{name}: {on} on, {off} off");
            assert!(song.ticks() > 0, "{name}");
        }
    }
}
