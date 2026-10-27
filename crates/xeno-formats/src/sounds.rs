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
