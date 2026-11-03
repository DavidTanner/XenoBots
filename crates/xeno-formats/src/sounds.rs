//! The sound events of `xeno.exe` (`docs/formats/dsounds.md`, "Sound events"): the
//! table `snd_play_sound_effect` indexes to play a `dsounds.bin` block on one of the
//! driver's three voices. Game data, read at run time through the instructions that
//! index it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::invalid;

/// The events in the stock table: the next data follows it (the code never checks an
/// event number; callers pass twice the number in `AX`).
pub const EVENTS: usize = 29;
/// `shl bx, 1` then `mov ax, [bx + table]` in `snd_play_sound_effect` (`1000:ee90`).
pub(crate) const TABLE_AT: (usize, &[u8]) = (0xee9b, &[0xd1, 0xe3, 0x8b, 0x87]);
/// `mov cx, [bx + table + 2]` in the same routine: the step override.
pub(crate) const STEP_AT: (usize, &[u8]) = (0xeea5, &[0x8b, 0x8f]);

/// One event: what `snd_play_sound_effect` passes to the driver's play slot `0x44c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The `dsounds.bin` block (`AL`); `None` (0xff) plays nothing.
    pub block: Option<u8>,
    /// The voice, 0 to 2 (`AH` bits 0 to 6).
    pub voice: u8,
    /// `AH` bit 7: start when the voice's current sound ends instead of at once.
    pub queued: bool,
    /// `CX`: a step that replaces the block's own when not 0.
    pub step: u16,
}

/// Reads the events from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<[Event; EVENTS], FormatError> {
    let table = operand(image, TABLE_AT.0, TABLE_AT.1)?;
    if operand(image, STEP_AT.0, STEP_AT.1)? != table.wrapping_add(2) {
        return Err(invalid(STEP_AT.0, "not the step override's operand"));
    }
    let start = ds + usize::from(table);
    let bytes = image
        .get(start..start + 4 * EVENTS)
        .ok_or_else(|| invalid(start, "the sound events pass the end of the image"))?;
    let mut events = [Event {
        block: None,
        voice: 0,
        queued: false,
        step: 0,
    }; EVENTS];
    for (n, (event, e)) in events.iter_mut().zip(bytes.as_chunks::<4>().0).enumerate() {
        if e[1] & 0x7f > 2 {
            return Err(invalid(
                start + 4 * n + 1,
                "a voice past the driver's three",
            ));
        }
        *event = Event {
            block: (e[0] != 0xff).then_some(e[0]),
            voice: e[1] & 0x7f,
            queued: e[1] & 0x80 != 0,
            step: u16::from_le_bytes([e[2], e[3]]),
        };
    }
    Ok(events)
}

/// Reads the events from the user's unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<[Event; EVENTS], FormatError> {
    read_image(&exe.image, crate::sequence::data_segment(exe)?)
}

/// The beep `1000:0261` (`docs/sim.md`, "The beep (T-193)"): the far jump there, to a
/// `call` to the routine that sounds the PC speaker.
pub const BEEP_AT: usize = 0x261;
/// The beep routine: `pushf`, `test ss:[clock], 0xffff`, `jz`, `push ax`, `push bx`,
/// `mov ax, divisor`, `mov bx, on`, `call` the tone, `mov ax, off`, `call` the wait.
const BEEP_SHAPE: &str =
    "9c 36 f7 06 ?? ?? ff ff 74 ?? 50 53 b8 ?? ?? bb ?? ?? e8 ?? ?? b8 ?? ?? e8";

/// A PC speaker beep: PIT channel 2's divisor (the tone is 1,193,182 Hz / `divisor`),
/// then how many counts of the game clock (`DS:0x374`, the 60 Hz chain) it sounds and
/// how many it stays silent after. The game waits for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beep {
    pub divisor: u16,
    pub on: u16,
    pub off: u16,
}

/// Reads the beep whose far-jump thunk is at `thunk` (`BEEP_AT`) in a load image.
pub fn read_beep_image(image: &[u8], thunk: usize) -> Result<Beep, FormatError> {
    let offset = operand(image, thunk, &[0xea])?;
    let segment = usize::from(crate::maproom::word(image, thunk + 3)?) * 16;
    let at = segment + usize::from(offset);
    let rel = operand(image, at, &[0xe8])?;
    let routine = segment + usize::from(offset.wrapping_add(3).wrapping_add(rel));
    crate::base::shape(image, routine, BEEP_SHAPE, "beep routine")?;
    let word = |n| crate::maproom::word(image, routine + n);
    Ok(Beep {
        divisor: word(0x0d)?,
        on: word(0x10)?,
        off: word(0x16)?,
    })
}

/// Reads the beep `1000:0261` from the user's unpacked `xeno.exe`.
pub fn read_beep(exe: &Unpacked) -> Result<Beep, FormatError> {
    read_beep_image(&exe.image, BEEP_AT)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;
    const TABLE: usize = 0x100;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + TABLE + 4 * EVENTS];
        image[TABLE_AT.0..TABLE_AT.0 + 6].copy_from_slice(&[0xd1, 0xe3, 0x8b, 0x87, 0x00, 0x01]);
        image[STEP_AT.0..STEP_AT.0 + 4].copy_from_slice(&[0x8b, 0x8f, 0x02, 0x01]);
        for (n, e) in image[DS + TABLE..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            e.copy_from_slice(&[n as u8, n as u8 % 3, 0, 0]);
        }
        image[DS + TABLE + 4..DS + TABLE + 8].copy_from_slice(&[0xff, 0x82, 0, 0]);
        image[DS + TABLE + 8..DS + TABLE + 12].copy_from_slice(&[3, 0x01, 0x00, 0x40]);
        image
    }

    #[test]
    fn synthetic_events_read() {
        let events = read_image(&synthetic(), DS).unwrap();
        assert_eq!((events[0].block, events[0].voice), (Some(0), 0));
        assert_eq!(
            events[1],
            Event {
                block: None,
                voice: 2,
                queued: true,
                step: 0
            }
        );
        assert_eq!((events[2].block, events[2].step), (Some(3), 0x4000));
        assert_eq!(events[28].voice, 1);
        insta::assert_debug_snapshot!(events[..3]);
    }

    #[test]
    fn another_build_is_refused() {
        for at in [TABLE_AT.0, TABLE_AT.0 + 3, STEP_AT.0 + 1, STEP_AT.0 + 2] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
        // A voice past 2.
        let mut image = synthetic();
        image[DS + TABLE + 1] = 3;
        assert!(read_image(&image, DS).is_err());
    }

    /// A synthetic thunk at `BEEP_AT` jumping to 0x20:0x10, whose `call` reaches the
    /// routine at 0x200 + 0x40.
    fn beep_image() -> Vec<u8> {
        let mut image = vec![0u8; 0x300];
        image[BEEP_AT..BEEP_AT + 5].copy_from_slice(&[0xea, 0x10, 0x00, 0x20, 0x00]);
        image[0x210..0x213].copy_from_slice(&[0xe8, 0x2d, 0x00]);
        let routine = [
            0x9c, 0x36, 0xf7, 0x06, 0x74, 0x03, 0xff, 0xff, 0x74, 0x13, 0x50, 0x53, 0xb8, 0x34,
            0x12, 0xbb, 0x03, 0x00, 0xe8, 0, 0, 0xb8, 0x05, 0x00, 0xe8,
        ];
        image[0x240..0x240 + routine.len()].copy_from_slice(&routine);
        image
    }

    #[test]
    fn synthetic_beep_read() {
        let beep = read_beep_image(&beep_image(), BEEP_AT).unwrap();
        assert_eq!(
            beep,
            Beep {
                divisor: 0x1234,
                on: 3,
                off: 5
            }
        );
        for at in [BEEP_AT, 0x210, 0x240, 0x24c, 0x24f, 0x255, 0x258] {
            let mut image = beep_image();
            image[at] ^= 0x01;
            assert!(read_beep_image(&image, BEEP_AT).is_err(), "{at:#x}");
        }
    }

    /// The user's beep is about 994 Hz, 4 counts on and 4 off (`docs/sim.md`).
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_beep_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let beep = read_beep(&exe).unwrap();
        assert_eq!(1_193_182 / u32::from(beep.divisor), 994);
        assert_eq!((beep.on, beep.off), (4, 4));
    }

    /// The user's events play at the file's rates, and none plays blocks 4 or 15
    /// (`docs/formats/dsounds.md`, "Sound events").
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_sound_events_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let events = read(&exe).unwrap();
        assert!(events.iter().all(|e| e.step == 0));
        let blocks: Vec<u8> = events.iter().filter_map(|e| e.block).collect();
        assert!(blocks.iter().all(|&b| b < 17 && b != 4 && b != 15));
        assert_eq!(events.iter().filter(|e| e.block.is_none()).count(), 10);
        assert!(events[28].queued);
    }
}
