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

/// The beeps `1000:0261`, `0266`, `026b` and `0270` (`docs/sim.md`, "The beep
/// (T-193)"): far jumps there, each to a `call` to a routine that sounds the PC
/// speaker. [`read_beeps`] returns them in this order.
pub const BEEPS_AT: [usize; 4] = [0x261, 0x266, 0x26b, 0x270];
/// A beep routine with the clock test: `pushf`, `test ss:[clock], 0xffff`, `jz`,
/// `push ax`, `push bx`, `mov ax, divisor`, `mov bx, on`, `call` the tone, `mov ax,
/// off`, `call` the wait.
const BEEP_SHAPE: &str =
    "9c 36 f7 06 ?? ?? ff ff 74 ?? 50 53 b8 ?? ?? bb ?? ?? e8 ?? ?? b8 ?? ?? e8";
/// The same without the clock test (`0270`'s): `pushf`, `push ax`, `push bx`, then as
/// above.
const SHORT_SHAPE: &str = "9c 50 53 b8 ?? ?? bb ?? ?? e8 ?? ?? b8 ?? ?? e8";

/// A PC speaker beep: PIT channel 2's divisor (the tone is 1,193,182 Hz / `divisor`),
/// then how many counts of the game clock (`DS:0x374`, the 60 Hz chain) it sounds and
/// how many it stays silent after. The game waits for both. With `clock_test`, it
/// sounds nothing while the clock is 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beep {
    pub divisor: u16,
    pub on: u16,
    pub off: u16,
    pub clock_test: bool,
}

/// Reads the beep whose far-jump thunk is at `thunk` (one of `BEEPS_AT`) in a load
/// image: the routine has either shape.
pub fn read_beep_image(image: &[u8], thunk: usize) -> Result<Beep, FormatError> {
    let offset = operand(image, thunk, &[0xea])?;
    let segment = usize::from(crate::maproom::word(image, thunk + 3)?) * 16;
    let at = segment + usize::from(offset);
    let rel = operand(image, at, &[0xe8])?;
    let routine = segment + usize::from(offset.wrapping_add(3).wrapping_add(rel));
    let word = |n| crate::maproom::word(image, routine + n);
    if crate::base::shape(image, routine, SHORT_SHAPE, "beep routine").is_ok() {
        return Ok(Beep {
            divisor: word(0x04)?,
            on: word(0x07)?,
            off: word(0x0d)?,
            clock_test: false,
        });
    }
    crate::base::shape(image, routine, BEEP_SHAPE, "beep routine")?;
    Ok(Beep {
        divisor: word(0x0d)?,
        on: word(0x10)?,
        off: word(0x16)?,
        clock_test: true,
    })
}

/// Reads the four beeps of `BEEPS_AT` from the user's unpacked `xeno.exe`.
pub fn read_beeps(exe: &Unpacked) -> Result<[Beep; 4], FormatError> {
    let [a, b, c, d] = BEEPS_AT.map(|at| read_beep_image(&exe.image, at));
    Ok([a?, b?, c?, d?])
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

    /// Synthetic thunks: `BEEPS_AT[0]` jumping to 0x20:0x10, whose `call` reaches the
    /// clock-tested routine at 0x200 + 0x40, and `BEEPS_AT[3]` jumping to 0x20:0x18,
    /// whose `call` reaches the short one at 0x200 + 0x90.
    fn beep_image() -> Vec<u8> {
        let mut image = vec![0u8; 0x300];
        let [high, .., short] = BEEPS_AT;
        image[high..high + 5].copy_from_slice(&[0xea, 0x10, 0x00, 0x20, 0x00]);
        image[short..short + 5].copy_from_slice(&[0xea, 0x18, 0x00, 0x20, 0x00]);
        image[0x210..0x213].copy_from_slice(&[0xe8, 0x2d, 0x00]);
        image[0x218..0x21b].copy_from_slice(&[0xe8, 0x75, 0x00]);
        let routine = [
            0x9c, 0x36, 0xf7, 0x06, 0x74, 0x03, 0xff, 0xff, 0x74, 0x13, 0x50, 0x53, 0xb8, 0x34,
            0x12, 0xbb, 0x03, 0x00, 0xe8, 0, 0, 0xb8, 0x05, 0x00, 0xe8,
        ];
        image[0x240..0x240 + routine.len()].copy_from_slice(&routine);
        let routine = [
            0x9c, 0x50, 0x53, 0xb8, 0x21, 0x43, 0xbb, 0x02, 0x00, 0xe8, 0, 0, 0xb8, 0x07, 0x00,
            0xe8,
        ];
        image[0x290..0x290 + routine.len()].copy_from_slice(&routine);
        image
    }

    #[test]
    fn synthetic_beep_read() {
        let [high, .., short] = BEEPS_AT;
        let beep = read_beep_image(&beep_image(), high).unwrap();
        assert_eq!(
            beep,
            Beep {
                divisor: 0x1234,
                on: 3,
                off: 5,
                clock_test: true
            }
        );
        let beep = read_beep_image(&beep_image(), short).unwrap();
        assert_eq!(
            beep,
            Beep {
                divisor: 0x4321,
                on: 2,
                off: 7,
                clock_test: false
            }
        );
        for at in [high, 0x210, 0x240, 0x24c, 0x24f, 0x255, 0x258] {
            let mut image = beep_image();
            image[at] ^= 0x01;
            assert!(read_beep_image(&image, high).is_err(), "{at:#x}");
        }
        for at in [short, 0x218, 0x290, 0x293, 0x296, 0x299, 0x29c, 0x29f] {
            let mut image = beep_image();
            image[at] ^= 0x01;
            assert!(read_beep_image(&image, short).is_err(), "{at:#x}");
        }
    }

    /// The user's beeps (`docs/sim.md`, "The beep (T-193)"): about 994, 497 and
    /// 249 Hz for 4 counts and 4 off with the clock test, and 497 Hz for 1 and 1
    /// without.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_beeps_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let read: Vec<_> = read_beeps(&exe)
            .unwrap()
            .iter()
            .map(|b| (1_193_182 / u32::from(b.divisor), b.on, b.off, b.clock_test))
            .collect();
        assert_eq!(
            read,
            [
                (994, 4, 4, true),
                (497, 4, 4, true),
                (248, 4, 4, true),
                (497, 1, 1, false)
            ]
        );
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
