//! What the port reads from the user's `xeno.exe` for the bases' message logs and the
//! messages sent to a whole side (`docs/screens.md`, "The message log (T-129)"): the
//! senders' names, the base's two messages and the header a side's robots are sent.
//! Read at run time through the instructions that use them, after checking their
//! bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::invalid;

/// `1fe6:7f38`'s fixed names, each three `mov word [0x1bfe + 2n], imm`: a base's, a
/// relay's and kind 0xa's.
pub(crate) const NAMES_AT: [usize; 3] = [0x17dd1, 0x17e03, 0x17dea];
const NAME_OPS: [&[u8]; 3] = [
    &[0xc7, 0x06, 0xfe, 0x1b],
    &[0xc7, 0x06, 0x00, 0x1c],
    &[0xc7, 0x06, 0x02, 0x1c],
];
/// `mov ax, [bx + letters]`: a robot's two letters by kind.
pub(crate) const LETTERS_AT: usize = crate::sitroom::KIND_LETTERS_AT;
/// `sim_damage_base` (`1000:62b7`): `mov si, text` for the base destroyed and under
/// attack, and `cmp ax, interval`, the game clock's counts between the second.
pub(crate) const DESTROYED_AT: usize = 0x62d0;
pub(crate) const ATTACKED_AT: usize = 0x62f6;
pub(crate) const INTERVAL_AT: usize = 0x62e9;
/// `1000:d54c`: `mov cx, length` (the name's), the header's first and last bytes
/// written in the string segment (`mov byte es:[header], b`, `mov byte es:[bx +
/// header + 1], b`) and `mov si, header` before each robot's copy.
pub(crate) const NAME_LENGTH_AT: usize = 0xd55e;
pub(crate) const HEADER_START_AT: usize = 0xd573;
pub(crate) const HEADER_END_AT: usize = 0xd579;
pub(crate) const HEADER_AT: usize = 0xd5a9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Messages {
    /// The names a base, a relay and an object of kind 0xa sign with.
    pub base: [u8; 6],
    pub relay: [u8; 6],
    pub other: [u8; 6],
    /// A robot's name's letters, by kind (humanoid, scorpion, scout).
    pub letters: [[u8; 2]; 3],
    /// The texts (string-segment offsets) a destroyed base sends the other side and a
    /// base under attack its own.
    pub destroyed: u16,
    pub attacked: u16,
    /// The game clock's counts (`DS:0x374`) a base waits past before it says it is
    /// under attack again.
    pub interval: u16,
    /// The header each of a side's robots is sent first (a string-segment offset),
    /// and the two bytes that bracket the name's place in it.
    pub header: u16,
    pub header_bytes: [u8; 2],
    /// The name's place's length.
    pub name_length: u16,
}

pub fn read_image(image: &[u8], ds: usize) -> Result<Messages, FormatError> {
    let byte = |at: usize| {
        image
            .get(at)
            .copied()
            .ok_or_else(|| invalid(at, "past the end"))
    };
    let name = |at: usize| -> Result<[u8; 6], FormatError> {
        let mut name = [0; 6];
        for (i, op) in NAME_OPS.into_iter().enumerate() {
            let word = operand(image, at + 6 * i, op)?;
            name[2 * i..2 * i + 2].copy_from_slice(&word.to_le_bytes());
        }
        Ok(name)
    };
    let letters = ds + usize::from(operand(image, LETTERS_AT, &[0x8b, 0x87])?);
    let pair = |kind: usize| {
        Ok::<_, FormatError>([byte(letters + 2 * kind)?, byte(letters + 2 * kind + 1)?])
    };
    let header = operand(image, HEADER_AT, &[0xbe])?;
    let name_length = operand(image, NAME_LENGTH_AT, &[0xb9])?;
    let start = &[0x26, 0xc6, 0x06][..];
    if operand(image, HEADER_START_AT, start)? != header {
        return Err(invalid(HEADER_START_AT, "not the header's first byte"));
    }
    if operand(image, HEADER_END_AT, &[0x26, 0xc6, 0x87])? != header.wrapping_add(1) {
        return Err(invalid(HEADER_END_AT, "not the header's last byte"));
    }
    Ok(Messages {
        base: name(NAMES_AT[0])?,
        relay: name(NAMES_AT[1])?,
        other: name(NAMES_AT[2])?,
        letters: [pair(0)?, pair(1)?, pair(2)?],
        destroyed: operand(image, DESTROYED_AT, &[0xbe])?,
        attacked: operand(image, ATTACKED_AT, &[0xbe])?,
        interval: operand(image, INTERVAL_AT, &[0x3d])?,
        header,
        header_bytes: [byte(HEADER_START_AT + 5)?, byte(HEADER_END_AT + 5)?],
        name_length,
    })
}

impl Messages {
    /// The header as the first copy leaves it in the string segment `strings`: its
    /// two bytes around the name's place. The original copies the name into the data
    /// segment instead (`mov [bx + header + 1], al` with no `es:`), so the place
    /// stays as the image has it.
    pub fn write_header(&self, strings: &mut [u8]) {
        let at = usize::from(self.header);
        let end = at + 1 + usize::from(self.name_length);
        if let Some(b) = strings.get_mut(at) {
            *b = self.header_bytes[0];
        }
        if let Some(b) = strings.get_mut(end) {
            *b = self.header_bytes[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    #[test]
    fn synthetic_messages_read() {
        let mut image = vec![0u8; DS + 0xa000];
        crate::fixtures::synthetic_messages(&mut image, DS);
        let m = read_image(&image, DS).unwrap();
        assert_eq!(
            (m.base, m.relay, m.other),
            (*b"BB    ", *b"RR    ", *b"??    ")
        );
        assert_eq!(m.letters, [*b"HU", *b"SC", *b"ST"]);
        assert_eq!((m.destroyed, m.attacked, m.interval), (0x30, 0x40, 0x1e0));
        assert_eq!(
            (m.header, m.header_bytes, m.name_length),
            (0x50, [0xad, 0xba], 6)
        );
        let mut strings = vec![0; 0x60];
        m.write_header(&mut strings);
        assert_eq!(&strings[0x50..0x59], b"\xad\0\0\0\0\0\0\xba\0");
        image[HEADER_AT] = 0x90;
        assert!(read_image(&image, DS).is_err());
    }

    #[test]
    #[ignore]
    fn real_assets_messages_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let m = read_image(&exe.image, crate::sequence::data_segment(&exe).unwrap()).unwrap();
        assert_eq!((m.destroyed, m.attacked, m.interval), (0x3ce, 0x3e4, 0x1e0));
        assert_eq!((m.header, m.name_length), (0x1714, 6));
        // Bit 7 set: drawn in the high colour.
        assert!(m.header_bytes.iter().all(|b| b & 0x80 != 0));
        assert!([m.base, m.relay, m.other].iter().all(|n| n.is_ascii()));
    }
}
