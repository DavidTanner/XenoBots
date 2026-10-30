//! The mission clock's calendar in `xeno.exe` (`docs/sim.md`, "The mission clock"):
//! the days in each month, which `sim_advance_clock` steps the campaign's date by past
//! midnight. Game data, read at run time through the instruction that uses it.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::invalid;
use crate::sequence;

/// `cmp al, [bx + table]` in `sim_advance_clock` (`1000:ed53`), the day against the
/// month's length, `bx` the month.
pub(crate) const MONTH_DAYS_AT: usize = 0xed53;
pub(crate) const MONTH_DAYS_OP: &[u8] = &[0x3a, 0x87];
/// The months, January first (the clock's month names: `crate::sitroom::MONTHS`).
pub const MONTHS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    /// The days in each month, by month from 0.
    pub month_days: [u8; MONTHS],
}

/// Reads the calendar from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Calendar, FormatError> {
    let table = ds + usize::from(operand(image, MONTH_DAYS_AT, MONTH_DAYS_OP)?);
    let days = image
        .get(table..table + MONTHS)
        .ok_or_else(|| invalid(table, "past the end"))?;
    Ok(Calendar {
        month_days: days.try_into().expect("12 bytes"),
    })
}

/// Reads the calendar from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Calendar, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x1_0000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0; 0x2_0000];
        crate::fixtures::synthetic_calendar(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_calendar_reads() {
        let calendar = read_image(&synthetic(), DS).unwrap();
        assert_eq!(calendar.month_days[0], 1);
        assert_eq!(calendar.month_days[11], 12);
        insta::assert_debug_snapshot!(calendar);
    }

    #[test]
    fn another_instruction_is_refused() {
        let mut image = synthetic();
        image[MONTH_DAYS_AT + 1] = 0x86;
        assert!(read_image(&image, DS).is_err());
        assert!(read_image(&image[..MONTH_DAYS_AT + 2], DS).is_err());
    }

    /// The user's table: twelve months making a 365-day year.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_calendar_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let calendar = read(&exe).unwrap();
        let year: u32 = calendar.month_days.iter().map(|&d| u32::from(d)).sum();
        assert_eq!(year, 365);
    }
}
