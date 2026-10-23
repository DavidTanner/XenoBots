//! The goodbye `xeno.exe` leaves on the screen when it exits after a log-in
//! (`docs/screens.md`, "The name entry"). It is game text, so it is read from the user's
//! own `xeno.exe` at run time and never stored in this repository.
//!
//! The text is a fixed prefix in the data segment, padded so the name can be copied in
//! after it. `ui_name_entry_accept` (`1000:790b`) copies the name to the end of the
//! prefix, then writes one closing character and a NUL after it; the exit shows the
//! buffer from its start. So the prefix's start, the name's place and the closing
//! character are each read from those instructions, after checking their bytes, and the
//! prefix is the data between the start and the name's place.

use crate::FormatError;
use crate::lzexe::Unpacked;
use crate::sequence;

/// Load-image offset of `mov si, start`: the buffer the exit shows.
const SHOW_AT: usize = 0x7946;
const SHOW_OP: [u8; 1] = [0xbe];
/// Load-image offset of `mov [bx+name], al`: each name byte copied in.
const COPY_AT: usize = 0x7920;
const COPY_OP: [u8; 2] = [0x88, 0x87];
/// Load-image offset of `mov word [bx+name], close`: the closing character and a NUL.
const CLOSE_AT: usize = 0x7927;
const CLOSE_OP: [u8; 2] = [0xc7, 0x87];
/// No goodbye is longer than this before the name.
const MAX_PREFIX: usize = 64;

/// The goodbye around the player's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Farewell {
    /// The text before the name.
    pub prefix: String,
    /// The character after the name.
    pub close: char,
}

impl Farewell {
    /// The goodbye for `name`.
    pub fn message(&self, name: &str) -> String {
        format!("{}{name}{}", self.prefix, self.close)
    }
}

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// The word after `op` at `at`, if the bytes there are that instruction.
fn operand(image: &[u8], at: usize, op: &[u8]) -> Result<u16, FormatError> {
    match image.get(at..at + op.len() + 2) {
        Some(bytes) if bytes[..op.len()] == *op => {
            Ok(u16::from_le_bytes([bytes[op.len()], bytes[op.len() + 1]]))
        }
        Some(_) => Err(invalid(
            at,
            format!("not the expected instruction {op:02x?}"),
        )),
        None => Err(invalid(at, "past the end of the image")),
    }
}

/// Reads the goodbye from a load image whose data segment starts at `ds`. Error offsets
/// are into the load image.
pub fn read_image(image: &[u8], ds: usize) -> Result<Farewell, FormatError> {
    let start = usize::from(operand(image, SHOW_AT, &SHOW_OP)?);
    let name = operand(image, COPY_AT, &COPY_OP)?;
    let close_name = operand(image, CLOSE_AT, &CLOSE_OP)?;
    if close_name != name {
        return Err(invalid(
            CLOSE_AT,
            "the closing character does not follow the copied name",
        ));
    }
    // The immediate follows the displacement.
    let [close, nul] = operand(image, CLOSE_AT + CLOSE_OP.len() + 2, &[])?.to_le_bytes();
    if nul != 0 || !close.is_ascii_graphic() {
        return Err(invalid(
            CLOSE_AT,
            "the closing word is not a character and a NUL",
        ));
    }
    let name = usize::from(name);
    if name <= start || name - start > MAX_PREFIX {
        return Err(invalid(
            SHOW_AT,
            "the name is not just after the shown text",
        ));
    }
    let prefix = image
        .get(ds + start..ds + name)
        .ok_or_else(|| invalid(ds + start, "the text is past the end of the image"))?;
    if !prefix.iter().all(|b| (0x20..0x7f).contains(b)) {
        return Err(invalid(
            ds + start,
            "the text before the name is not printable",
        ));
    }
    Ok(Farewell {
        prefix: prefix.iter().map(|&b| char::from(b)).collect(),
        close: char::from(close),
    })
}

/// Reads the goodbye from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Farewell, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x8000;

    /// A made-up image: "SEE YOU, " at `DS:0x100`, the name copied to `DS:0x109`,
    /// then `!` and a NUL.
    fn image() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x200];
        image[SHOW_AT..SHOW_AT + 3].copy_from_slice(&[0xbe, 0x00, 0x01]);
        image[COPY_AT..COPY_AT + 4].copy_from_slice(&[0x88, 0x87, 0x09, 0x01]);
        image[CLOSE_AT..CLOSE_AT + 6].copy_from_slice(&[0xc7, 0x87, 0x09, 0x01, b'!', 0]);
        image[DS + 0x100..DS + 0x109].copy_from_slice(b"SEE YOU, ");
        image
    }

    #[test]
    fn a_synthetic_goodbye_reads_around_the_name() {
        let farewell = read_image(&image(), DS).unwrap();
        assert_eq!(farewell.message("ACE"), "SEE YOU, ACE!");
        insta::assert_debug_snapshot!(farewell);
    }

    #[test]
    fn other_code_or_text_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let mut image = image();
            image[at] = value;
            read_image(&image, DS)
        };
        // Not the instructions.
        assert!(corrupt(SHOW_AT, 0xb8).is_err());
        assert!(corrupt(COPY_AT + 1, 0x86).is_err());
        assert!(corrupt(CLOSE_AT, 0xc6).is_err());
        // The close goes somewhere other than after the name.
        assert!(corrupt(CLOSE_AT + 2, 0x0a).is_err());
        // The close is not a character and a NUL.
        assert!(corrupt(CLOSE_AT + 5, b'x').is_err());
        assert!(corrupt(CLOSE_AT + 4, b' ').is_err());
        // The name is before the text, or the text is not printable.
        assert!(corrupt(SHOW_AT + 1, 0x20).is_err());
        assert!(corrupt(DS + 0x104, 0x07).is_err());
        // Too short.
        assert!(read_image(&image()[..CLOSE_AT + 3], DS).is_err());
        assert!(read_image(&image()[..DS + 0x104], DS).is_err());
    }

    /// Reads the goodbye from the user's `xeno.exe`, checking only its shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_farewell_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let farewell = read(&exe).unwrap();
        // `docs/screens.md`: the name goes 20 bytes after the text's start, then a full
        // stop.
        assert_eq!((farewell.prefix.len(), farewell.close), (20, '.'));
        assert!(farewell.prefix.ends_with(", "));
    }
}
