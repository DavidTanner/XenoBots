//! What the port reads from the user's `xeno.exe` for the situation room's main view
//! (`docs/screens.md`, "The situation room's main view"): the map's robot colours and
//! sprites and its zoom labels, the message log's heading, rule and time, the robot
//! view's at-base text, and the string segment the log's messages point into. Game
//! data, read at run time through the instructions that use it.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, string, word};
use crate::sequence;

/// `mov ah, [bx + table]` in the robots' map routine: a dot's colour by model / 2.
pub(crate) const DOTS_AT: usize = 0xf9a3;
/// `mov bx, table` there: the sprites' first handles by model, at a zoom of 8 or more
/// and below it.
pub(crate) const LARGE_AT: usize = 0xf9b5;
pub(crate) const SMALL_AT: usize = 0xf9bf;
/// The map's zoom label (`1fe6:6cb9`): `mov si, prefix`, then `mov si, [bx + names]`.
pub(crate) const ZOOM_PREFIX_AT: usize = 0x16b40;
pub(crate) const ZOOM_NAMES_AT: usize = 0x16b53;
/// Mode C (`1000:56a7`): `mov si, heading`, `mov bx, rule`.
pub(crate) const LOG_HEADING_AT: usize = 0x56e2;
pub(crate) const LOG_RULE_AT: usize = 0x56ec;
/// A message's time (`1fe6:6d5b`): `mov [hours], ax`, `mov [minutes], ax`, `mov si,
/// template`.
pub(crate) const HOURS_AT: usize = 0x16bc0;
pub(crate) const MINUTES_AT: usize = 0x16bc9;
pub(crate) const TIME_AT: usize = 0x16bcc;
/// Mode R (`1000:5630`): `mov si, text` in the string segment, for a unit at its base.
pub(crate) const AT_BASE_AT: usize = 0x5699;

/// The roster's rows (`1fe6:6d87`): `mov ax, [bx + table]` for the status icons and
/// the kind icons at the base and in the field, `mov bx, rect` for the icons' places,
/// `mov si, text` for an empty row.
pub(crate) const STATUS_ICONS_AT: usize = 0x16c2e;
pub(crate) const BASE_ICONS_AT: usize = 0x16c45;
pub(crate) const FIELD_ICONS_AT: usize = 0x16c4b;
pub(crate) const STATUS_RECT_AT: usize = 0x16c32;
pub(crate) const KIND_RECT_AT: usize = 0x16c4f;
pub(crate) const EMPTY_ROW_AT: usize = 0x16c6c;
/// `mov si, [bx + table]` in `1fe6:7fd5`: the call signs, by +0xdb.
pub(crate) const CALL_SIGNS_AT: usize = 0x17e3d;
/// `mov ax, [bx + table]` in the mode buttons' draw (`1000:5efd`): labels by mode.
pub(crate) const MODE_LABELS_AT: usize = 0x5f05;

/// The call signs' count and length.
pub const CALL_SIGNS: usize = 64;
pub const CALL_SIGN_LEN: usize = 6;
/// The main view's modes, 0 to 8 by two.
pub const MODES: usize = 5;

/// The map's zooms, 0 to 6 (the zoom keys' range).
pub const ZOOMS: usize = 7;
/// How much of the string segment is kept for the messages.
const STRINGS_BYTES: usize = 0x10000;

/// The situation room's main view tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitRoom {
    /// A robot's dot colour on the map by model / 2 (humanoid, scorpion, scout).
    pub robot_dots: [u8; 3],
    /// A robot's first sprite handle on the map by model / 2, below a zoom of 8 and
    /// from it; the eight headings follow, a handle step apart.
    pub robot_sprites: [[u16; 3]; 2],
    /// The zoom label: a prefix, then the zoom's name.
    pub zoom_prefix: Vec<u8>,
    pub zoom_names: Vec<Vec<u8>>,
    /// Mode C's heading and the rule under it (top, left, bottom, right).
    pub log_heading: Vec<u8>,
    pub log_rule: [u16; 4],
    /// A message's time: the template and where its hours and minutes go.
    pub time: Vec<u8>,
    pub time_digits: [usize; 2],
    /// Mode R's text for a unit at its base.
    pub at_base: Vec<u8>,
    /// The roster's status icons (by +0xf9 bits 4 to 6) and kind icons at the base
    /// and in the field; the icons' left edges (status, kind) and an empty row's text.
    pub status_icons: [u16; 8],
    pub kind_icons: [[u16; 3]; 2],
    pub icon_lefts: [u16; 2],
    pub empty_row: Vec<u8>,
    /// The call signs: string segment offsets, by +0xdb.
    pub call_signs: Vec<u16>,
    /// The mode buttons' labels, by mode / 2 (the next sprite is the current mode's).
    pub mode_labels: [u16; MODES],
    /// The string segment, from its start.
    pub strings: Vec<u8>,
}

impl SitRoom {
    /// The NUL-terminated string at `offset` in the string segment.
    pub fn string(&self, offset: u16) -> &[u8] {
        let s = self.strings.get(usize::from(offset)..).unwrap_or_default();
        &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
    }

    /// Call sign `index` (+0xdb): six bytes, `1fe6:7fd5` copies no more.
    pub fn call_sign(&self, index: u8) -> &[u8] {
        let at = self
            .call_signs
            .get(usize::from(index))
            .map_or(usize::MAX, |&o| usize::from(o));
        let s = self.strings.get(at..).unwrap_or_default();
        let s = &s[..s.len().min(CALL_SIGN_LEN)];
        &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
    }

    /// A message's time as the log shows it.
    pub fn time_of(&self, hour: u8, minute: u8) -> Vec<u8> {
        let mut time = self.time.clone();
        for (at, n) in self.time_digits.into_iter().zip([hour, minute]) {
            // `1fe6:6d76`: tens counted up from '0', then the units.
            if let Some(d) = time.get_mut(at..at + 2) {
                d.copy_from_slice(&[b'0' + n / 10, b'0' + n % 10]);
            }
        }
        time
    }
}

/// Reads the tables from a load image whose data and string segments start at `ds`
/// and `strings`.
pub fn read_image(image: &[u8], ds: usize, strings: usize) -> Result<SitRoom, FormatError> {
    let at_ds = |at, op: &[u8]| -> Result<usize, FormatError> {
        Ok(ds + usize::from(operand(image, at, op)?))
    };
    let dots = at_ds(DOTS_AT, &[0x8a, 0xa7])?;
    let sprites = |at| -> Result<[u16; 3], FormatError> {
        let table = at_ds(at, &[0xbb])?;
        Ok([
            word(image, table)?,
            word(image, table + 2)?,
            word(image, table + 4)?,
        ])
    };
    let names = at_ds(ZOOM_NAMES_AT, &[0x8b, 0xb7])?;
    let rule = at_ds(LOG_RULE_AT, &[0xbb])?;
    let time = operand(image, TIME_AT, &[0xbe])?;
    let words = |table: usize, n: usize| -> Result<Vec<u16>, FormatError> {
        (0..n).map(|i| word(image, table + 2 * i)).collect()
    };
    let table = |at| at_ds(at, &[0x8b, 0x87]);
    let left = |at| -> Result<u16, FormatError> { word(image, at_ds(at, &[0xbb])? + 2) };
    let digit = |at| -> Result<usize, FormatError> {
        let d = operand(image, at, &[0xa3])?.wrapping_sub(time);
        (d < 8)
            .then_some(usize::from(d))
            .ok_or_else(|| invalid(at, "not in the time template"))
    };
    Ok(SitRoom {
        robot_dots: [
            *image
                .get(dots)
                .ok_or_else(|| invalid(dots, "past the end"))?,
            *image
                .get(dots + 1)
                .ok_or_else(|| invalid(dots, "past the end"))?,
            *image
                .get(dots + 2)
                .ok_or_else(|| invalid(dots, "past the end"))?,
        ],
        robot_sprites: [sprites(SMALL_AT)?, sprites(LARGE_AT)?],
        zoom_prefix: string(image, at_ds(ZOOM_PREFIX_AT, &[0xbe])?)?,
        zoom_names: (0..ZOOMS)
            .map(|z| string(image, ds + usize::from(word(image, names + 2 * z)?)))
            .collect::<Result<_, _>>()?,
        log_heading: string(image, at_ds(LOG_HEADING_AT, &[0xbe])?)?,
        log_rule: [
            word(image, rule)?,
            word(image, rule + 2)?,
            word(image, rule + 4)?,
            word(image, rule + 6)?,
        ],
        time: string(image, ds + usize::from(time))?,
        time_digits: [digit(HOURS_AT)?, digit(MINUTES_AT)?],
        at_base: string(
            image,
            strings + usize::from(operand(image, AT_BASE_AT, &[0xbe])?),
        )?,
        status_icons: words(table(STATUS_ICONS_AT)?, 8)?
            .try_into()
            .unwrap_or_default(),
        kind_icons: [
            words(table(FIELD_ICONS_AT)?, 3)?
                .try_into()
                .unwrap_or_default(),
            words(table(BASE_ICONS_AT)?, 3)?
                .try_into()
                .unwrap_or_default(),
        ],
        icon_lefts: [left(STATUS_RECT_AT)?, left(KIND_RECT_AT)?],
        empty_row: string(image, at_ds(EMPTY_ROW_AT, &[0xbe])?)?,
        call_signs: words(at_ds(CALL_SIGNS_AT, &[0x8b, 0xb7])?, CALL_SIGNS)?,
        mode_labels: words(table(MODE_LABELS_AT)?, MODES)?
            .try_into()
            .unwrap_or_default(),
        strings: image
            .get(strings..(strings + STRINGS_BYTES).min(image.len()))
            .ok_or_else(|| invalid(strings, "no string segment"))?
            .to_vec(),
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<SitRoom, FormatError> {
    read_image(
        &exe.image,
        sequence::data_segment(exe)?,
        sequence::string_segment(exe)?,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const DS: usize = 0x20000;
    pub(crate) const STRINGS: usize = 0x30000;

    /// Made-up tables: dots 1, 2, 3; sprites 0x10.. small and 0x40.. large; zoom
    /// labels "Z" and "z0".."z6"; heading "LOG", rule (5, 6, 5, 60); the time template
    /// "[00-00]" with hours at 1 and minutes at 4; "HOME" at base.
    pub(crate) fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; STRINGS + 0x100];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        put(DOTS_AT, &op(&[0x8a, 0xa7], 0x100));
        put(DS + 0x100, &[1, 2, 3]);
        put(SMALL_AT, &op(&[0xbb], 0x110));
        put(LARGE_AT, &op(&[0xbb], 0x118));
        for (i, h) in [0x10u16, 0x18, 0x20, 0x40, 0x48, 0x50]
            .into_iter()
            .enumerate()
        {
            put(DS + 0x110 + 2 * i + 2 * (i / 3), &h.to_le_bytes());
        }
        put(ZOOM_PREFIX_AT, &op(&[0xbe], 0x200));
        put(DS + 0x200, b"Z\0");
        put(ZOOM_NAMES_AT, &op(&[0x8b, 0xb7], 0x210));
        for z in 0..ZOOMS {
            let at = 0x220 + 4 * z as u16;
            put(DS + 0x210 + 2 * z, &at.to_le_bytes());
            put(DS + usize::from(at), &[b'z', b'0' + z as u8, 0]);
        }
        put(LOG_HEADING_AT, &op(&[0xbe], 0x300));
        put(DS + 0x300, b"LOG\0");
        put(LOG_RULE_AT, &op(&[0xbb], 0x310));
        put(DS + 0x310, &[5, 0, 6, 0, 5, 0, 60, 0]);
        put(HOURS_AT, &op(&[0xa3], 0x321));
        put(MINUTES_AT, &op(&[0xa3], 0x324));
        put(TIME_AT, &op(&[0xbe], 0x320));
        put(DS + 0x320, b"[00-00]\0");
        put(STATUS_ICONS_AT, &op(&[0x8b, 0x87], 0x400));
        put(FIELD_ICONS_AT, &op(&[0x8b, 0x87], 0x410));
        put(BASE_ICONS_AT, &op(&[0x8b, 0x87], 0x416));
        put(MODE_LABELS_AT, &op(&[0x8b, 0x87], 0x420));
        for i in 0..21u16 {
            put(
                DS + 0x400 + 2 * usize::from(i),
                &(0x100 + 8 * i).to_le_bytes(),
            );
        }
        put(STATUS_RECT_AT, &op(&[0xbb], 0x440));
        put(KIND_RECT_AT, &op(&[0xbb], 0x448));
        put(DS + 0x442, &[5, 0]);
        put(DS + 0x44a, &[14, 0]);
        put(EMPTY_ROW_AT, &op(&[0xbe], 0x450));
        put(DS + 0x450, b"  \0");
        put(CALL_SIGNS_AT, &op(&[0x8b, 0xb7], 0x500));
        put(DS + 0x502, &0x50u16.to_le_bytes());
        put(STRINGS + 0x50, b"AB-123XYZ\0");
        put(AT_BASE_AT, &op(&[0xbe], 0x40));
        put(STRINGS + 0x40, b"HOME\0HELLO\0");
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let room = read_image(&synthetic(), DS, STRINGS).unwrap();
        assert_eq!(room.robot_sprites, [[0x10, 0x18, 0x20], [0x40, 0x48, 0x50]]);
        assert_eq!(room.zoom_names[6], b"z6");
        assert_eq!(room.time_of(9, 45), b"[09-45]");
        assert_eq!(room.string(0x45), b"HELLO");
        assert_eq!(room.string(0xffff), b"");
        // Call sign 1 is six bytes; 0 points at the start of the segment (empty).
        assert_eq!(room.call_sign(1), b"AB-123");
        assert_eq!(room.call_sign(0), b"");
        assert_eq!(room.call_sign(200), b"");
        assert_eq!(room.status_icons[7], 0x138);
        assert_eq!(
            room.kind_icons,
            [[0x140, 0x148, 0x150], [0x158, 0x160, 0x168]]
        );
        assert_eq!(room.mode_labels, [0x180, 0x188, 0x190, 0x198, 0x1a0]);
        assert_eq!(room.icon_lefts, [5, 14]);
        insta::assert_debug_snapshot!(SitRoom {
            strings: Vec::new(),
            ..room
        });
    }

    #[test]
    fn another_build_is_refused() {
        for at in [
            DOTS_AT,
            SMALL_AT,
            LARGE_AT,
            ZOOM_PREFIX_AT,
            ZOOM_NAMES_AT + 1,
            LOG_HEADING_AT,
            LOG_RULE_AT,
            HOURS_AT,
            MINUTES_AT + 2,
            TIME_AT,
            AT_BASE_AT,
            STATUS_ICONS_AT,
            BASE_ICONS_AT + 1,
            FIELD_ICONS_AT,
            STATUS_RECT_AT,
            KIND_RECT_AT,
            EMPTY_ROW_AT,
            CALL_SIGNS_AT + 1,
            MODE_LABELS_AT,
        ] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS, STRINGS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/screens.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_sitroom_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let room = read(&exe).unwrap();
        assert_eq!(room.robot_dots, [0x30, 0x0e, 0x0d]);
        assert_eq!(room.robot_sprites, [[0xc0, 0x100, 0x140], [0, 0x40, 0x80]]);
        assert_eq!(room.zoom_names.len(), ZOOMS);
        assert!(
            room.zoom_names
                .iter()
                .all(|n| n.ends_with(b"M") || n.ends_with(b"K"))
        );
        assert_eq!(room.log_rule, [36, 95, 36, 288]);
        assert_eq!(room.time_digits, [2, 5]);
        assert_eq!(room.time_of(14, 7)[2..7], *b"14:07");
        assert!(room.at_base.starts_with(b"UNIT IS AT BASE"));
        assert_eq!(room.status_icons[0], 0x430);
        assert_eq!(
            room.kind_icons,
            [[0x480, 0x488, 0x490], [0x498, 0x4a0, 0x4a8]]
        );
        assert_eq!(room.icon_lefts, [5, 14]);
        assert_eq!(room.mode_labels, [0x3a0, 0x3b0, 0x3c0, 0x3d0, 0x3e0]);
        assert_eq!(room.empty_row, b"        ");
        // Every call sign is six printable bytes (names like `EAGLE `).
        for i in 0..CALL_SIGNS as u8 {
            let sign = room.call_sign(i);
            assert!(
                sign.len() == CALL_SIGN_LEN
                    && sign.iter().all(|&b| b == b' ' || b.is_ascii_graphic()),
                "{i}: {sign:?}"
            );
        }
    }
}
