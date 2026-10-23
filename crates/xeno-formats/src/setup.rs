//! What the setup program `config.dvr` keeps on disk: `XENO.CFG` (its settings and two
//! keyboard tables), `XENO.KEY` (developer unlock codes), and the default keyboard
//! tables inside `config.dvr` itself. See `docs/formats/cfg.md` and `key.md`.
//!
//! Neither file is in a stock install. `config.dvr` writes `XENO.CFG` when it saves its
//! settings; `XENO.KEY` is written by hand. `xeno.exe` opens neither: it only sees the
//! configuration record `config.dvr` hands it.

use binrw::{BinRead, binread};

use crate::FormatError;
use crate::lzexe;

/// Slots in a keyboard table, in the remap screen's 3x4 grid, row by row.
pub const KEY_SLOTS: usize = 12;
/// A slot holding this is not used, and the remap screen skips it.
pub const NO_KEY: u8 = 0x60;

/// The slots the remap screen shows, by its labels. The other six hold [`NO_KEY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    UpLeft = 0,
    Up = 1,
    UpRight = 2,
    Left = 3,
    Right = 5,
    Fire = 10,
}

impl Slot {
    pub const ALL: [Slot; 6] = [
        Slot::UpLeft,
        Slot::Up,
        Slot::UpRight,
        Slot::Left,
        Slot::Right,
        Slot::Fire,
    ];
}

/// One player's keys: a set 1 make code per slot (the `E0` prefix is dropped, so a
/// grey arrow key and its keypad twin give the same code).
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyTable {
    pub slots: [u8; KEY_SLOTS],
    /// Never read or written by the remap code; zero in the defaults.
    pub unknown_0c: [u8; 4],
}

impl KeyTable {
    /// The scan code in `slot`, if it is used.
    pub fn code(&self, slot: Slot) -> Option<u8> {
        Some(self.slots[slot as usize]).filter(|&c| c != NO_KEY)
    }
}

/// One player's settings in `XENO.CFG`, as in the configuration record.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSetup {
    /// The input device letter (`cfg.md`, "The configuration record").
    pub input: u8,
    /// Bit 0 flips whenever this player's keys are remapped, so the settings differ
    /// from the saved ones and the file is written again.
    pub unknown_remap_toggle: u8,
    /// Joystick status bytes from the alignment and switched-stick screens; their
    /// values are not traced. The calibration itself is never saved.
    pub unknown_alignment: u8,
    pub unknown_switched: u8,
}

/// `XENO.CFG`, 48 bytes: the CPU and FPU classes `config.dvr` measured, the
/// configuration record's bytes 2 to 15 (ASCII letters and digits), then player 1's
/// and player 2's keys.
#[binread]
#[br(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConfig {
    pub detected_cpu: u8,
    pub detected_fpu: u8,
    pub cpu: u8,
    pub fpu: u8,
    pub graphics: u8,
    pub music: u8,
    pub sound: u8,
    pub players: u8,
    pub player: [PlayerSetup; 2],
    pub keys: [KeyTable; 2],
}

/// The size `config.dvr` insists on; it ignores a file of any other size.
pub const SAVED_CONFIG_SIZE: usize = 48;

/// Decodes `XENO.CFG`.
pub fn parse_saved_config(bytes: &[u8]) -> Result<SavedConfig, FormatError> {
    if bytes.len() != SAVED_CONFIG_SIZE {
        return Err(FormatError::WrongLength {
            expected: SAVED_CONFIG_SIZE,
            actual: bytes.len(),
        });
    }
    Ok(SavedConfig::read(&mut std::io::Cursor::new(bytes))?)
}

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// Where the menu walker loads the head of `config.dvr`'s menu list (`mov si, head`),
/// as a load-image offset, and the instruction bytes before the immediate.
pub const MENU_HEAD_AT: usize = 0x1e5c;
pub const MENU_HEAD_OP: [u8; 3] = [0x8b, 0xc8, 0xbe];
/// The label of the remap grid's last row, which ends just before the two tables.
const LAST_LABEL_END: &[u8] = b"FIRE\0";

/// The default keys in the user's `config.dvr` (LZEXE-packed): player 1's and 2's
/// tables, the last 32 bytes of the first entry of its menu list. Checks the walker's
/// instruction, the label before the tables and that every slot holds a key or
/// [`NO_KEY`], so another build is refused rather than misread.
pub fn default_keys(config_dvr: &[u8]) -> Result<[KeyTable; 2], FormatError> {
    let image = lzexe::unpack(config_dvr)?.image;
    let word = |at: usize| {
        image
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
            .ok_or_else(|| invalid(at, "past the end of the image"))
    };
    let op = MENU_HEAD_AT - MENU_HEAD_OP.len();
    if image.get(op..MENU_HEAD_AT) != Some(&MENU_HEAD_OP[..]) {
        return Err(invalid(op, "not the menu walker's instruction"));
    }
    let head = word(MENU_HEAD_AT)?;
    let end = head + word(head)?;
    let tables = end
        .checked_sub(2 * 16)
        .filter(|&t| t >= head + LAST_LABEL_END.len())
        .ok_or_else(|| invalid(head, "the first menu entry is too short"))?;
    if image.get(tables - LAST_LABEL_END.len()..tables) != Some(LAST_LABEL_END) {
        return Err(invalid(tables, "no remap labels before the key tables"));
    }
    let mut cursor = std::io::Cursor::new(&image[tables..end]);
    let keys = [KeyTable::read(&mut cursor)?, KeyTable::read(&mut cursor)?];
    for (i, table) in keys.iter().enumerate() {
        if let Some(bad) = table.slots.iter().position(|&c| c == 0 || c > 0x7f) {
            return Err(invalid(tables + 16 * i + bad, "not a scan code"));
        }
    }
    Ok(keys)
}

/// Slots in `XENO.KEY`.
pub const UNLOCK_SLOTS: usize = 16;

/// `XENO.KEY`: up to 16 text lines `n:code`. `config.dvr` hashes each code into word
/// `n - 1` of the record it hands `xeno.exe`, which compares some of the words with
/// fixed values to turn on debugging features (`key.md`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnlockCodes {
    /// The hash of each slot's code, for slots that have a line.
    pub hashes: [Option<u16>; UNLOCK_SLOTS],
}

/// One step of the hash: OR the byte into the low half, then eight times rotate left
/// by one and XOR 0x480 if the bit rotated round is set.
fn hash_step(hash: u16, byte: u8) -> u16 {
    (0..8).fold(hash | u16::from(byte), |h, _| {
        let h = h.rotate_left(1);
        if h & 1 != 0 { h ^ 0x480 } else { h }
    })
}

/// Decodes `XENO.KEY` the way `config.dvr` reads it, so it never fails. Up to 16 lines,
/// each up to and including a CR (LF is skipped; the file may end without one). Digits
/// before the first `:` give the slot, 1 to 16 (others are ignored); every other byte,
/// the CR included, is hashed from 0xffff, with `a` to `y` (not `z`) upper-cased.
pub fn parse_unlock_codes(bytes: &[u8]) -> Result<UnlockCodes, FormatError> {
    let mut codes = UnlockCodes::default();
    let mut rest = bytes.iter().copied().filter(|&b| b != b'\n');
    for _ in 0..UNLOCK_SLOTS {
        let (mut slot, mut colon, mut hash) = (0u16, false, 0xffffu16);
        for byte in rest.by_ref() {
            match byte {
                b':' if !colon => colon = true,
                b'0'..=b'9' if !colon => {
                    slot = slot.wrapping_mul(10).wrapping_add(u16::from(byte - b'0'));
                }
                b'a'..=b'y' => hash = hash_step(hash, byte & 0xdf),
                _ => hash = hash_step(hash, byte),
            }
            if byte == b'\r' {
                break;
            }
        }
        if (1..=UNLOCK_SLOTS as u16).contains(&slot) {
            codes.hashes[usize::from(slot - 1)] = Some(hash);
        }
    }
    Ok(codes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_config_dvr;

    /// A made-up file: classes and devices as letters, then two key tables.
    fn synthetic_cfg() -> Vec<u8> {
        let mut cfg = b"2032VAC2K!  K   ".to_vec();
        for first in [1, 7] {
            let mut keys = [NO_KEY; KEY_SLOTS];
            for (i, slot) in Slot::ALL.iter().enumerate() {
                keys[*slot as usize] = first + i as u8;
            }
            cfg.extend(keys);
            cfg.extend([0; 4]);
        }
        cfg
    }

    #[test]
    fn synthetic_cfg_decodes() {
        let cfg = parse_saved_config(&synthetic_cfg()).unwrap();
        assert_eq!(
            (cfg.detected_cpu, cfg.cpu, cfg.graphics),
            (b'2', b'3', b'V')
        );
        assert_eq!(cfg.player.map(|p| p.input), [b'K', b'K']);
        assert_eq!(cfg.player[0].unknown_remap_toggle, b'!');
        assert_eq!(cfg.keys[0].code(Slot::Fire), Some(6));
        assert_eq!(cfg.keys[1].code(Slot::UpLeft), Some(7));
        assert_eq!(cfg.keys[1].code(Slot::Fire), Some(12));
        assert_eq!(cfg.keys[1].slots[4], NO_KEY);
        let used: Vec<_> = Slot::ALL
            .iter()
            .filter_map(|&s| cfg.keys[0].code(s))
            .collect();
        assert_eq!(used, [1, 2, 3, 4, 5, 6]);
        assert!(matches!(
            parse_saved_config(&synthetic_cfg()[..47]),
            Err(FormatError::WrongLength {
                expected: 48,
                actual: 47
            })
        ));
    }

    #[test]
    fn saved_config_snapshot() {
        insta::assert_debug_snapshot!(parse_saved_config(&synthetic_cfg()).unwrap());
    }

    #[test]
    fn unlock_lines_hash_into_their_slots() {
        // One round from 0xffff, worked by hand: "1:" then only the CR.
        let codes = parse_unlock_codes(b"1:\r\n").unwrap();
        assert_eq!(codes.hashes[0], Some(0x71fc));
        assert!(codes.hashes[1..].iter().all(Option::is_none));
        // Case is folded for a..y, LF is skipped, the last line may lack its CR, and
        // slots 0 and 17 are ignored.
        let codes = parse_unlock_codes(b"0:x\r\n2:Abc\r\n17:x\r\n16:abc").unwrap();
        let upper = parse_unlock_codes(b"2:ABC\r16:ABC\r").unwrap();
        assert_eq!(codes.hashes[1], upper.hashes[1]);
        assert_ne!(codes.hashes[15], upper.hashes[15], "the CR is hashed");
        assert_eq!(codes.hashes.iter().flatten().count(), 2);
        // 'z' is not folded. The byte is ORed in, so the first one, ORed into 0xffff,
        // never counts.
        let hash = |line: &[u8]| parse_unlock_codes(line).unwrap().hashes[2];
        assert_ne!(hash(b"3:aaaz\r"), hash(b"3:aaaZ\r"));
        assert_eq!(hash(b"3:z\r"), hash(b"3:Q\r"));
        // Only 16 lines are read.
        let many = "1:a\r".repeat(16) + "2:b\r";
        assert!(parse_unlock_codes(many.as_bytes()).unwrap().hashes[1].is_none());
        assert_eq!(parse_unlock_codes(b"").unwrap(), UnlockCodes::default());
    }

    #[test]
    fn unlock_codes_snapshot() {
        insta::assert_debug_snapshot!(parse_unlock_codes(b"1:one\r\n9:nine\r\n").unwrap());
    }

    #[test]
    fn default_keys_are_read_after_checks() {
        let p1 = [
            11, 12, 13, 14, NO_KEY, 15, NO_KEY, NO_KEY, NO_KEY, NO_KEY, 16, NO_KEY,
        ];
        let p2 = [
            21, 22, 23, 24, NO_KEY, 25, NO_KEY, NO_KEY, NO_KEY, NO_KEY, 26, NO_KEY,
        ];
        let file = synthetic_config_dvr(&[p1, p2]);
        let keys = default_keys(&file).unwrap();
        assert_eq!((keys[0].slots, keys[1].slots), (p1, p2));
        assert_eq!(keys[1].code(Slot::Right), Some(25));

        let mut bad = p1;
        bad[3] = 0;
        assert!(default_keys(&synthetic_config_dvr(&[bad, p2])).is_err());
        assert!(default_keys(b"MZ").is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_setup_files_parse() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let keys = default_keys(&std::fs::read(dir.join("config.dvr")).unwrap()).unwrap();
        // Structure only: both players have all six keys, none shared.
        let mut codes: Vec<_> = keys
            .iter()
            .flat_map(|t| Slot::ALL.map(|s| t.code(s)))
            .collect();
        assert!(codes.iter().all(Option::is_some));
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 12);
        // Stock installs have neither file; parse them when a user has made them.
        let read = |name: &str| {
            std::fs::read(dir.join(name))
                .or_else(|_| std::fs::read(dir.join(name.to_uppercase())))
                .ok()
        };
        if let Some(bytes) = read("xeno.cfg") {
            parse_saved_config(&bytes).unwrap();
        }
        if let Some(bytes) = read("xeno.key") {
            parse_unlock_codes(&bytes).unwrap();
        }
    }
}
