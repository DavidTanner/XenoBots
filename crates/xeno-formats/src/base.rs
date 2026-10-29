//! What the port reads from the user's `xeno.exe` for the bases' tick
//! (`docs/sim.md`, "The bases' phases"): which phase runs which routine, and the
//! numbers those routines use. Game data, read at run time through the instructions
//! that use it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{invalid, word};

/// `sim_tick_base` (`1000:6300`): `inc byte [di + 0x41]`, `mov bl, [di + 0x41]`,
/// `and bx, 0xf`, `shl bl, 1`, then `call [bx + table]`, the phases' routines.
pub(crate) const TICK_AT: usize = 0x6300;
const TICK: &[u8] = &[
    0xfe, 0x45, 0x41, 0x8a, 0x5d, 0x41, 0x83, 0xe3, 0x0f, 0xd0, 0xe3,
];
/// The phases (+0x41's low four bits) the port runs.
pub const COUNT_PHASE: u8 = 5;
pub const TAKE_IN_PHASE: u8 = 9;
/// The count's start: `test byte [di + 0x41], mask`, `jnz`, `mov byte [di + 0x21], 0`.
const COUNT: [Option<u8>; 9] = [
    Some(0xf6),
    Some(0x45),
    Some(0x41),
    None,
    Some(0x75),
    None,
    Some(0xc6),
    Some(0x45),
    Some(0x21),
];
/// The take-in's start: `test byte [di + 0x41], mask`, `jz`.
const TAKE_IN: [Option<u8>; 5] = [Some(0xf6), Some(0x45), Some(0x41), None, Some(0x74)];
/// From the take-in's start: `mov al, ah`, `mov ah, dl`, `cmp ax, reach` (the
/// distance as 8.8).
const REACH_AT: usize = 0x4b;
const REACH: &[u8] = &[0x8a, 0xc4, 0x8a, 0xe2, 0x3d];

/// The bases' numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bases {
    /// The count runs when the tick's other bits under this mask are clear (once
    /// every 256 ticks in the stock build).
    pub count_mask: u8,
    /// The take-in runs likewise (once every 128 ticks).
    pub take_in_mask: u8,
    /// How near (8.8 units) a robot comes to its base to be taken in.
    pub reach: u16,
}

/// The bytes at `at` match `pattern` (`None` any byte).
fn matches(image: &[u8], at: usize, pattern: &[Option<u8>]) -> bool {
    image.get(at..at + pattern.len()).is_some_and(|b| {
        b.iter()
            .zip(pattern)
            .all(|(b, p)| p.is_none_or(|p| p == *b))
    })
}

/// Reads the numbers from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Bases, FormatError> {
    if image.get(TICK_AT..TICK_AT + TICK.len()) != Some(TICK) {
        return Err(invalid(TICK_AT, "not the base's tick"));
    }
    let table = ds + usize::from(operand(image, TICK_AT + TICK.len(), &[0xff, 0x97])?);
    let routine = |phase: u8| word(image, table + 2 * usize::from(phase)).map(usize::from);
    let count = routine(COUNT_PHASE)?;
    if !matches(image, count, &COUNT) {
        return Err(invalid(count, "not the base's count"));
    }
    let take_in = routine(TAKE_IN_PHASE)?;
    if !matches(image, take_in, &TAKE_IN) {
        return Err(invalid(take_in, "not the base's take-in"));
    }
    Ok(Bases {
        count_mask: image[count + 3],
        take_in_mask: image[take_in + 3],
        reach: operand(image, take_in + REACH_AT, REACH)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x1_0000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1_0000];
        crate::fixtures::synthetic_bases(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_numbers_read() {
        let bases = read_image(&synthetic(), DS).unwrap();
        assert_eq!(
            (bases.count_mask, bases.take_in_mask, bases.reach),
            (0xf0, 0x70, 0x3a)
        );
        insta::assert_debug_snapshot!(bases);
    }

    #[test]
    fn another_build_is_refused() {
        let table = DS + 0x5aea;
        for (at, value) in [
            (TICK_AT + 2, 0x42),
            (TICK_AT + TICK.len() + 1, 0x87),
            (table + 2 * 5, 0x00),
            (table + 2 * 9 + 1, 0x00),
            (0x6344 + 4, 0x74),
            (0x63dd + 4, 0x75),
            (0x63dd + REACH_AT + 4, 0x3c),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's numbers: the count every 256 ticks, the take-in every 128 within
    /// 0x3a (`docs/sim.md`, "The bases' phases").
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_bases_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let ds = crate::sequence::data_segment(&exe).unwrap();
        let bases = read_image(&exe.image, ds).unwrap();
        assert_eq!(
            (bases.count_mask, bases.take_in_mask, bases.reach),
            (0xf0, 0x70, 0x3a)
        );
    }
}
