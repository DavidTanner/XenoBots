//! What the port reads from the user's `mapdvr.dvr`, the terrain editor's driver: its
//! key table (`docs/screens.md`, "The scenario editor (T-048)", T-109). Game data,
//! read at run time through the instructions that use it, after checking their bytes.
//!
//! The driver's init hands the host 0x104 bytes of its data segment from offset 0xde,
//! and its routines read them through the host's `DS` at the same offsets, so a host
//! offset `x` in that range is image offset `data * 16 + x`.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{invalid, word};
use crate::vgaintro::load_image;

/// In `drv_init`: `mov ax, length`, `mov si, data_segment` and `mov si, offset`, the
/// bytes of its data segment the host keeps.
const LENGTH_AT: usize = 0x24;
const SEGMENT_AT: usize = 0x2c;
const OFFSET_AT: usize = 0x31;
/// `mov di, table - 2` in `ui_map_editor_on_key` (`0x032a`), which then scans the
/// (key, handler) pairs from `table` up to the key itself, written at the end.
const KEYS_AT: usize = 0x333;
/// More pairs than any table holds (the stock one has 22).
const MAX_KEYS: usize = 64;

/// The terrain editor's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDriver {
    /// (key word, handler offset in the driver's code), in the table's order.
    pub keys: Vec<(u16, u16)>,
}

/// Reads the tables from the bytes of `mapdvr.dvr`.
pub fn parse(bytes: &[u8]) -> Result<MapDriver, FormatError> {
    let image = load_image(bytes)?;
    let length = usize::from(operand(image, LENGTH_AT, &[0xb8])?);
    let data = usize::from(operand(image, SEGMENT_AT, &[0xbe])?) * 16;
    let first = usize::from(operand(image, OFFSET_AT, &[0xbe])?);
    let table = usize::from(operand(image, KEYS_AT, &[0xbf])?) + 2;
    let kept = first..first + length;
    let mut keys = Vec::new();
    for i in 0..MAX_KEYS {
        let at = table + 4 * i;
        if !kept.contains(&(at + 3)) {
            return Err(invalid(at, "the key table runs past the driver's data"));
        }
        // The end: a zero word the dispatcher writes the key into.
        match word(image, data + at)? {
            0 => return Ok(MapDriver { keys }),
            key => keys.push((key, word(image, data + at + 2)?)),
        }
    }
    Err(invalid(data + table, "no end to the key table"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_mapdvr;

    #[test]
    fn a_synthetic_driver_s_keys_read() {
        let keys = [(0x48, 0x3a5), (0x2010, 0x6cf)];
        let driver = parse(&synthetic_mapdvr(&keys)).unwrap();
        assert_eq!(driver.keys, keys);
        insta::assert_debug_snapshot!(driver);
    }

    #[test]
    fn a_wrong_instruction_or_an_endless_table_is_refused() {
        let mut bytes = synthetic_mapdvr(&[(0x48, 0x3a5)]);
        bytes[0x20 + KEYS_AT] = 0x90;
        assert!(parse(&bytes).is_err());
        let full: Vec<(u16, u16)> = (1..=MAX_KEYS as u16).map(|k| (k, k)).collect();
        assert!(parse(&synthetic_mapdvr(&full)).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_mapdvr_keys_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let driver = parse(&std::fs::read(dir.join("mapdvr.dvr")).unwrap()).unwrap();
        assert_eq!(driver.keys.len(), 22);
        // Up moves the camera, Alt+Q leaves (`docs/screens.md`).
        assert!(driver.keys.contains(&(0x48, 0x3a5)));
        assert!(driver.keys.contains(&(0x2010, 0x6cf)));
    }
}
