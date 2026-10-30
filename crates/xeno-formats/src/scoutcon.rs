//! What the port reads from the user's `xeno.exe` for the scout console's instruments
//! (`docs/screens.md`, "The scout console's instruments (T-132)"): the sprite bank's
//! handle, the panels' texts, the compass's needle table and rectangles, the power
//! panel's rectangles and the weapon buttons' sprites. Game data, read at run time
//! through the instructions that use it.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, string, word};
use crate::sequence;

/// `mov dx, handle` at `1000:bbc0`: where the picture's draw handler loads `msprite4`.
pub(crate) const BANK_AT: usize = 0xbbc0;
/// `mov si, text`: the speed's unlit digits (`1000:c222`), the heading's (`b903`), the
/// target panel's lines without a target (`c0da`), a weapon loading, damaged and ready
/// (`c107`, `c10c`, `c111`), and the status panel's four headings (`bf51`, `bf86`,
/// `bfbf`, `bffe`).
pub(crate) const SPEED_UNLIT_AT: usize = 0xc222;
pub(crate) const HEADING_UNLIT_AT: usize = 0xb903;
pub(crate) const NO_TARGET_AT: usize = 0xc0da;
pub(crate) const LOADING_AT: usize = 0xc107;
pub(crate) const DAMAGED_AT: usize = 0xc10c;
pub(crate) const READY_AT: usize = 0xc111;
pub(crate) const PAGES_AT: [usize; PAGES] = [0xbf51, 0xbf86, 0xbfbf, 0xbffe];
/// The compass (`1000:b8b8`): `mov bx, rect` for the dial, `mov ax, [bx + table]` for
/// the needles, `mov bx, rect` in each quadrant's routine (`CS:0xb92b`'s four).
pub(crate) const DIAL_AT: usize = 0xb8b8;
pub(crate) const NEEDLES_AT: usize = 0xb8d3;
pub(crate) const QUADRANTS_AT: [usize; 4] = [0xb933, 0xb93e, 0xb949, 0xb954];
/// The power panel (`1000:c360`): `mov bx, rect` for the two lights and the two bars.
pub(crate) const LIGHTS_AT: [usize; 2] = [0xc364, 0xc389];
pub(crate) const BARS_AT: [usize; 2] = [0xc37f, 0xc3a4];
/// The weapon buttons (`1000:b7db`): `mov ax, [bx + table]` for a weapon not selected
/// and selected.
pub(crate) const WEAPON_OFF_AT: usize = 0xb7f0;
pub(crate) const WEAPON_ON_AT: usize = 0xb7f8;

/// The compass's headings, a 64th of a turn each.
pub const NEEDLES: usize = 64;
/// The status panel's pages.
pub const PAGES: usize = 4;

/// The scout console's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoutCon {
    /// The first handle of the console's sprite bank (`msprite4`).
    pub bank: u16,
    /// Drawn before the speed and the heading: unlit digits, then the cursor back over
    /// them.
    pub speed_unlit: Vec<u8>,
    pub heading_unlit: Vec<u8>,
    /// The target panel: the lines without a target, and a weapon's state.
    pub no_target: Vec<u8>,
    pub loading: Vec<u8>,
    pub damaged: Vec<u8>,
    pub ready: Vec<u8>,
    /// The status panel's pages' headings.
    pub pages: [Vec<u8>; PAGES],
    /// The compass: the dial's rectangle, each heading's needle (low byte the
    /// quadrant times 2, high byte the needle) and the quadrants' rectangles.
    pub dial: Rect,
    pub needles: [u16; NEEDLES],
    pub quadrants: [Rect; 4],
    /// The power panel: the two lights' and the two bars' rectangles.
    pub lights: [Rect; 2],
    pub bars: [Rect; 2],
    /// The weapon buttons' sprites by weapon: not selected, selected.
    pub weapon_sprites: [[u16; 2]; 2],
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<ScoutCon, FormatError> {
    let at_ds = |at, op: &[u8]| -> Result<usize, FormatError> {
        Ok(ds + usize::from(operand(image, at, op)?))
    };
    let text = |at| string(image, at_ds(at, &[0xbe])?);
    let area = |at| rect(image, at_ds(at, &[0xbb])?);
    let words = |at, n: usize| -> Result<Vec<u16>, FormatError> {
        let table = at_ds(at, &[0x8b, 0x87])?;
        (0..n).map(|i| word(image, table + 2 * i)).collect()
    };
    let pair = |at| -> Result<[u16; 2], FormatError> {
        let w = words(at, 2)?;
        Ok([w[0], w[1]])
    };
    Ok(ScoutCon {
        bank: operand(image, BANK_AT, &[0xba])?,
        speed_unlit: text(SPEED_UNLIT_AT)?,
        heading_unlit: text(HEADING_UNLIT_AT)?,
        no_target: text(NO_TARGET_AT)?,
        loading: text(LOADING_AT)?,
        damaged: text(DAMAGED_AT)?,
        ready: text(READY_AT)?,
        pages: [
            text(PAGES_AT[0])?,
            text(PAGES_AT[1])?,
            text(PAGES_AT[2])?,
            text(PAGES_AT[3])?,
        ],
        dial: area(DIAL_AT)?,
        needles: words(NEEDLES_AT, NEEDLES)?
            .try_into()
            .map_err(|_| invalid(NEEDLES_AT, "short table"))?,
        quadrants: [
            area(QUADRANTS_AT[0])?,
            area(QUADRANTS_AT[1])?,
            area(QUADRANTS_AT[2])?,
            area(QUADRANTS_AT[3])?,
        ],
        lights: [area(LIGHTS_AT[0])?, area(LIGHTS_AT[1])?],
        bars: [area(BARS_AT[0])?, area(BARS_AT[1])?],
        weapon_sprites: [pair(WEAPON_OFF_AT)?, pair(WEAPON_ON_AT)?],
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<ScoutCon, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x20000;

    /// Made-up tables: bank 0x100; texts "t0".."t9" at DS + 0x100 on; needles 0..63;
    /// rectangles (n, n + 1, n + 2, n + 3) for the dial (1), quadrants (2..5), lights
    /// (6, 7) and bars (8, 9); weapon sprites 0x10, 0x11 and 0x20, 0x21.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1000];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        let table = |first: u16, n: u16| -> Vec<u8> {
            (first..first + n).flat_map(u16::to_le_bytes).collect()
        };
        put(BANK_AT, &op(&[0xba], 0x100));
        let texts = [
            SPEED_UNLIT_AT,
            HEADING_UNLIT_AT,
            NO_TARGET_AT,
            LOADING_AT,
            DAMAGED_AT,
            READY_AT,
        ]
        .into_iter()
        .chain(PAGES_AT);
        for (i, at) in texts.enumerate() {
            let value = 0x100 + 8 * i as u16;
            put(at, &op(&[0xbe], value));
            put(DS + usize::from(value), format!("t{i}\0").as_bytes());
        }
        put(NEEDLES_AT, &op(&[0x8b, 0x87], 0x200));
        put(DS + 0x200, &table(0, NEEDLES as u16));
        let rects = [DIAL_AT]
            .into_iter()
            .chain(QUADRANTS_AT)
            .chain(LIGHTS_AT)
            .chain(BARS_AT);
        for (i, at) in rects.enumerate() {
            let n = i as u16 + 1;
            let value = 0x300 + 8 * n;
            put(at, &op(&[0xbb], value));
            put(DS + usize::from(value), &table(n, 4));
        }
        put(WEAPON_OFF_AT, &op(&[0x8b, 0x87], 0x400));
        put(WEAPON_ON_AT, &op(&[0x8b, 0x87], 0x404));
        put(DS + 0x400, &[0x10, 0, 0x11, 0, 0x20, 0, 0x21, 0]);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let con = read_image(&synthetic(), DS).unwrap();
        assert_eq!((con.bank, con.speed_unlit.as_slice()), (0x100, &b"t0"[..]));
        assert_eq!(
            (con.ready.as_slice(), con.pages[3].as_slice()),
            (&b"t5"[..], &b"t9"[..])
        );
        assert_eq!((con.needles[63], con.quadrants[3].top), (63, 5));
        assert_eq!((con.lights[1].left, con.bars[1].right), (8, 12));
        assert_eq!(con.weapon_sprites, [[0x10, 0x11], [0x20, 0x21]]);
        insta::assert_debug_snapshot!(con);
    }

    #[test]
    fn another_build_is_refused() {
        let checked = [
            BANK_AT,
            SPEED_UNLIT_AT,
            READY_AT,
            PAGES_AT[3],
            DIAL_AT,
            NEEDLES_AT + 1,
            QUADRANTS_AT[2],
            LIGHTS_AT[1],
            BARS_AT[0],
            WEAPON_ON_AT + 1,
        ];
        for at in checked {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/screens.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_scoutcon_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let con = read(&exe).unwrap();
        assert_eq!(con.bank, 0x9b0);
        // Two and three unlit digits, then the cursor back over them.
        assert_eq!(con.speed_unlit[2..], [8, 8]);
        assert_eq!(con.heading_unlit[3..], [8, 8, 8]);
        // The humanoid's texts for a damaged weapon and a heading's unlit digits.
        let human = crate::humancon::read(&exe).unwrap();
        assert_eq!(
            (&con.damaged, &con.heading_unlit),
            (&human.damaged, &human.unlit)
        );
        // A quarter turn on, the needle is the first of the next quadrant.
        assert_eq!((con.needles[0], con.needles[16]), (0, 0x1000));
        assert_eq!(con.needles[32] & 0xff, 2);
        assert_eq!((con.dial.top, con.dial.left), (122, 7));
        // Every quadrant is inside the compass's widget (121, 1, 163, 59).
        assert!(con.quadrants.iter().all(|r| r.top >= 121 && r.right <= 59));
        // The bars stand 32 rows high: 16 segments 2 rows apart.
        assert!(con.bars.iter().all(|r| r.bottom - r.top == 32));
        assert_eq!(con.weapon_sprites, [[0xa10, 0xa20], [0xa18, 0xa28]]);
        assert!(con.pages.iter().all(|p| p.len() > 20 && p.is_ascii()));
    }
}
