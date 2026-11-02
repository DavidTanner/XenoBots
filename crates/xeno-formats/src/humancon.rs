//! What the port reads from the user's `xeno.exe` for the humanoid console's
//! instruments (`docs/screens.md`, "The humanoid console's instruments (T-130)"): the
//! sprite bank's handle, the panels' texts, the compass's needle table and rectangles,
//! the weapon buttons' sprites, the power panel's rectangles, the gear lever's
//! rectangles and messages, the compass's centre and a base's name. Game
//! data, read at run time through the instructions that use it.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, string, word};
use crate::sequence;

/// `mov dx, handle` at `1000:a0f2`: where the opener loads `msprite2`.
pub(crate) const BANK_AT: usize = 0xa0f2;
/// `mov si, text`: the unlit digits (`1000:a70e`, the speed's), the target panel's
/// texts without a target, loading, ready and damaged (`a51b`, `a548`, `a555`,
/// `a55a`), the status panel's OK and OUT (`de13`, `de1a`) and its three headings
/// (`a993`, `a9c8`, `aa09`).
pub(crate) const UNLIT_AT: usize = 0xa70e;
pub(crate) const NO_TARGET_AT: usize = 0xa51b;
pub(crate) const LOADING_AT: usize = 0xa548;
pub(crate) const READY_AT: usize = 0xa555;
pub(crate) const DAMAGED_AT: usize = 0xa55a;
pub(crate) const OK_AT: usize = 0xde13;
pub(crate) const OUT_AT: usize = 0xde1a;
pub(crate) const PAGES_AT: [usize; PAGES] = [0xa993, 0xa9c8, 0xaa09];
/// `mov word [0x1bfe], imm`, `[0x1c00]`, `[0x1c02]` at `1fe6:7f71`: a base's name.
pub(crate) const BASE_NAME_AT: usize = 0x17dd1;
const BASE_NAME_OPS: [[u8; 4]; 3] = [
    [0xc7, 0x06, 0xfe, 0x1b],
    [0xc7, 0x06, 0x00, 0x1c],
    [0xc7, 0x06, 0x02, 0x1c],
];
/// The compass (`1000:a617`): `mov bx, rect` for the dial, `mov ax, [bx + table]` for
/// the needles, `mov bx, rect` in each quadrant's routine (`CS:0xa98c`'s four).
pub(crate) const DIAL_AT: usize = 0xa617;
pub(crate) const NEEDLES_AT: usize = 0xa634;
pub(crate) const QUADRANTS_AT: [usize; 4] = [0xa694, 0xa69f, 0xa6aa, 0xa6b5];
/// The weapon buttons (`1000:a35e`): `mov ax, [bx + table]` for a weapon not selected
/// and selected.
pub(crate) const WEAPON_OFF_AT: usize = 0xa373;
pub(crate) const WEAPON_ON_AT: usize = 0xa37b;
/// The power panel (`1000:aab7`): `mov bx, rect` for the two lights and the two bars.
pub(crate) const LIGHTS_AT: [usize; 2] = [0xaabb, 0xaae0];
pub(crate) const BARS_AT: [usize; 2] = [0xaad6, 0xaafb];
/// The gear lever's mouse handler (`1000:a88c`): `mov si, [bx + table]` for the
/// gears' rectangles (the drag), the notches' (a press) and the release's messages;
/// `cmp bl, last` closing the drag's scan and the notches'.
pub(crate) const GEARS_AT: usize = 0xa8a5;
pub(crate) const NOTCHES_AT: usize = 0xa91a;
pub(crate) const GEAR_MESSAGES_AT: usize = 0xa906;
pub(crate) const DRAG_LAST_AT: usize = 0xa8b0;
pub(crate) const NOTCH_LAST_AT: usize = 0xa934;
/// The compass's mouse handler (`1000:a6c0`): `mov bx, x`, `mov cx, y`, the centre.
pub(crate) const CENTRE_AT: [usize; 2] = [0xa6d5, 0xa6d8];

/// The compass's headings, a 32nd of a turn each.
pub const NEEDLES: usize = 32;
/// The status panel's pages.
pub const PAGES: usize = 3;

/// The humanoid console's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanCon {
    /// The first handle of the console's sprite bank (`msprite2`).
    pub bank: u16,
    /// Drawn before a number: unlit digits, then the cursor back over them.
    pub unlit: Vec<u8>,
    /// The target panel: the lines without a target, and a weapon's state.
    pub no_target: Vec<u8>,
    pub loading: Vec<u8>,
    pub ready: Vec<u8>,
    pub damaged: Vec<u8>,
    /// The status panel: a part working and out (codes with bit 7), and the pages'
    /// headings.
    pub ok: Vec<u8>,
    pub out: Vec<u8>,
    pub pages: [Vec<u8>; PAGES],
    /// A base's name as the target panel shows it.
    pub base_name: [u8; 6],
    /// The compass: the dial's rectangle, each heading's needle (low byte the
    /// quadrant times 2, high byte the needle) and the quadrants' rectangles.
    pub dial: Rect,
    pub needles: [u16; NEEDLES],
    pub quadrants: [Rect; 4],
    /// The weapon buttons' sprites by weapon: not selected, selected.
    pub weapon_sprites: [[u16; 2]; 2],
    /// The power panel: the two lights' and the two bars' rectangles.
    pub lights: [Rect; 2],
    pub bars: [Rect; 2],
    /// The gear lever, by gear index: the rectangle whose top the drag compares the
    /// pointer's row with, a notch a press picks the gear by, and the message a
    /// release sends the robot (a string-segment offset).
    pub gears: Vec<Rect>,
    pub notches: Vec<Rect>,
    pub gear_messages: Vec<u16>,
    /// The compass's centre (x, y), the bearings to the pointer are taken from.
    pub centre: (u16, u16),
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<HumanCon, FormatError> {
    let at_ds = |at, op: &[u8]| -> Result<usize, FormatError> {
        Ok(ds + usize::from(operand(image, at, op)?))
    };
    let text = |at| string(image, at_ds(at, &[0xbe])?);
    let area = |at| rect(image, at_ds(at, &[0xbb])?);
    // `mov ax, [bx + table]` or, `si`, `mov si, [bx + table]`.
    let table = |at, n: usize, si: bool| -> Result<Vec<u16>, FormatError> {
        let table = at_ds(at, &[0x8b, if si { 0xb7 } else { 0x87 }])?;
        (0..n).map(|i| word(image, table + 2 * i)).collect()
    };
    let words = |at, n| table(at, n, false);
    let pair = |at| -> Result<[u16; 2], FormatError> {
        let w = words(at, 2)?;
        Ok([w[0], w[1]])
    };
    // The notches' scan ends at its last gear; the drag's checks one fewer, the last
    // gear taking the rows below them all.
    let imm8 = |at: usize| -> Result<u8, FormatError> {
        match image.get(at..at + 3) {
            Some([0x80, 0xfb, n]) if n % 2 == 0 => Ok(*n),
            _ => Err(invalid(at, "not the expected cmp bl")),
        }
    };
    let last = imm8(NOTCH_LAST_AT)?;
    if imm8(DRAG_LAST_AT)?.wrapping_add(2) != last {
        return Err(invalid(
            DRAG_LAST_AT,
            "the drag scans another count of gears",
        ));
    }
    let count = usize::from(last / 2) + 1;
    let gear_rects = |at| -> Result<Vec<Rect>, FormatError> {
        table(at, count, true)?
            .into_iter()
            .map(|r| rect(image, ds + usize::from(r)))
            .collect()
    };
    let mut base_name = [0; 6];
    for (i, op) in BASE_NAME_OPS.iter().enumerate() {
        let w = operand(image, BASE_NAME_AT + 6 * i, op)?;
        base_name[2 * i..2 * i + 2].copy_from_slice(&w.to_le_bytes());
    }
    Ok(HumanCon {
        bank: operand(image, BANK_AT, &[0xba])?,
        unlit: text(UNLIT_AT)?,
        no_target: text(NO_TARGET_AT)?,
        loading: text(LOADING_AT)?,
        ready: text(READY_AT)?,
        damaged: text(DAMAGED_AT)?,
        ok: text(OK_AT)?,
        out: text(OUT_AT)?,
        pages: [text(PAGES_AT[0])?, text(PAGES_AT[1])?, text(PAGES_AT[2])?],
        base_name,
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
        weapon_sprites: [pair(WEAPON_OFF_AT)?, pair(WEAPON_ON_AT)?],
        lights: [area(LIGHTS_AT[0])?, area(LIGHTS_AT[1])?],
        bars: [area(BARS_AT[0])?, area(BARS_AT[1])?],
        gears: gear_rects(GEARS_AT)?,
        notches: gear_rects(NOTCHES_AT)?,
        gear_messages: table(GEAR_MESSAGES_AT, count, true)?,
        centre: (
            operand(image, CENTRE_AT[0], &[0xbb])?,
            operand(image, CENTRE_AT[1], &[0xb9])?,
        ),
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<HumanCon, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x20000;

    /// Made-up tables: bank 0x100; texts "u0".."p2" at DS + 0x100 on; needles
    /// 0, 1, .. 31; rectangles (n, n + 1, n + 2, n + 3) for the dial (1), quadrants
    /// (2..5), lights (6, 7) and bars (8, 9); weapon sprites 0x10, 0x11 and 0x20, 0x21;
    /// the base "BASE01"; three gears (rectangles 0x10 + n.., notches 0x20 + n..,
    /// messages 0x30 + n) and the centre (0x41, 0x42).
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1000];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        put(BANK_AT, &op(&[0xba], 0x100));
        let texts = [
            UNLIT_AT,
            NO_TARGET_AT,
            LOADING_AT,
            READY_AT,
            DAMAGED_AT,
            OK_AT,
            OUT_AT,
            PAGES_AT[0],
            PAGES_AT[1],
            PAGES_AT[2],
        ];
        for (i, at) in texts.into_iter().enumerate() {
            let value = 0x100 + 8 * i as u16;
            put(at, &op(&[0xbe], value));
            put(DS + usize::from(value), format!("t{i}\0").as_bytes());
        }
        for (i, o) in BASE_NAME_OPS.iter().enumerate() {
            let w = u16::from_le_bytes([b"BASE01"[2 * i], b"BASE01"[2 * i + 1]]);
            put(BASE_NAME_AT + 6 * i, &op(o, w));
        }
        put(NEEDLES_AT, &op(&[0x8b, 0x87], 0x200));
        for i in 0..NEEDLES {
            put(DS + 0x200 + 2 * i, &(i as u16).to_le_bytes());
        }
        let rects = [DIAL_AT]
            .into_iter()
            .chain(QUADRANTS_AT)
            .chain(LIGHTS_AT)
            .chain(BARS_AT);
        for (i, at) in rects.enumerate() {
            let n = i as u16 + 1;
            let value = 0x300 + 8 * n;
            put(at, &op(&[0xbb], value));
            for (k, w) in [n, n + 1, n + 2, n + 3].into_iter().enumerate() {
                put(DS + usize::from(value) + 2 * k, &w.to_le_bytes());
            }
        }
        put(WEAPON_OFF_AT, &op(&[0x8b, 0x87], 0x400));
        put(WEAPON_ON_AT, &op(&[0x8b, 0x87], 0x404));
        put(DS + 0x400, &[0x10, 0, 0x11, 0, 0x20, 0, 0x21, 0]);
        put(DRAG_LAST_AT, &[0x80, 0xfb, 2]);
        put(NOTCH_LAST_AT, &[0x80, 0xfb, 4]);
        for (k, (at, base)) in [(GEARS_AT, 0x10), (NOTCHES_AT, 0x20)]
            .into_iter()
            .enumerate()
        {
            let table = 0x500 + 0x40 * k as u16;
            put(at, &op(&[0x8b, 0xb7], table));
            for n in 0..3u16 {
                let r = table + 8 + 8 * n;
                put(DS + usize::from(table + 2 * n), &r.to_le_bytes());
                for (i, w) in [base + n, 1, 2, 3].into_iter().enumerate() {
                    put(DS + usize::from(r) + 2 * i, &w.to_le_bytes());
                }
            }
        }
        put(GEAR_MESSAGES_AT, &op(&[0x8b, 0xb7], 0x600));
        put(DS + 0x600, &[0x30, 0, 0x31, 0, 0x32, 0]);
        put(CENTRE_AT[0], &op(&[0xbb], 0x41));
        put(CENTRE_AT[1], &op(&[0xb9], 0x42));
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let con = read_image(&synthetic(), DS).unwrap();
        assert_eq!((con.bank, con.unlit.as_slice()), (0x100, &b"t0"[..]));
        assert_eq!(
            (con.out.as_slice(), &con.base_name),
            (&b"t6"[..], b"BASE01")
        );
        assert_eq!((con.needles[31], con.quadrants[3].top), (31, 5));
        assert_eq!(con.weapon_sprites, [[0x10, 0x11], [0x20, 0x21]]);
        assert_eq!((con.gears[2].top, con.notches[1].top), (0x12, 0x21));
        assert_eq!(
            (&con.gear_messages[..], con.centre),
            (&[0x30, 0x31, 0x32][..], (0x41, 0x42))
        );
        insta::assert_debug_snapshot!(con);
    }

    #[test]
    fn another_build_is_refused() {
        let mut checked = vec![BANK_AT, UNLIT_AT, NO_TARGET_AT, OUT_AT, PAGES_AT[2]];
        checked.extend([BASE_NAME_AT + 12, DIAL_AT, NEEDLES_AT + 1, QUADRANTS_AT[1]]);
        checked.extend([WEAPON_OFF_AT, WEAPON_ON_AT + 1, LIGHTS_AT[1], BARS_AT[0]]);
        checked.extend([
            GEARS_AT + 1,
            NOTCHES_AT,
            GEAR_MESSAGES_AT + 1,
            DRAG_LAST_AT + 2,
        ]);
        checked.extend([NOTCH_LAST_AT + 1, CENTRE_AT[0], CENTRE_AT[1]]);
        for at in checked {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/screens.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_humancon_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let con = read(&exe).unwrap();
        assert_eq!(con.bank, 0x4b0);
        // Three unlit digits, then the cursor back over them.
        assert_eq!(con.unlit[3..], [8, 8, 8]);
        // The dial is the compass's widget less a pixel or two; its quadrants tile it.
        assert_eq!(con.dial.top, con.quadrants[0].top);
        assert_eq!(con.dial.bottom, con.quadrants[2].bottom);
        assert_eq!(con.dial.left, con.quadrants[3].left);
        // A turn's needles: quadrant 0 from straight up, nine needles to a quarter.
        assert_eq!(
            (con.needles[0], con.needles[8], con.needles[9]),
            (0, 0x800, 0x702)
        );
        assert_eq!(con.weapon_sprites, [[0x510, 0x520], [0x518, 0x528]]);
        assert!(con.out.iter().all(|&b| !(0x20..0x80).contains(&b)));
        assert!(con.base_name.iter().all(u8::is_ascii));
        assert_eq!(con.bars[0].right - con.bars[0].left, 32);
        // Six gears, reverse first: the drag's rectangles go up the lever, and the
        // compass's centre is inside its widget (110, 81, 135, 110).
        assert_eq!((con.gears.len(), con.notches.len()), (6, 6));
        assert!(con.gears.windows(2).all(|g| g[0].top > g[1].top));
        assert!((81..110).contains(&con.centre.0) && (110..135).contains(&con.centre.1));
    }
}
