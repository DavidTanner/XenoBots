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
    /// The string segment, from its start.
    pub strings: Vec<u8>,
}

impl SitRoom {
    /// The NUL-terminated string at `offset` in the string segment.
    pub fn string(&self, offset: u16) -> &[u8] {
        let s = self.strings.get(usize::from(offset)..).unwrap_or_default();
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
    }
}
