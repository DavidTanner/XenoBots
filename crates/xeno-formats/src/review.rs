//! The review screen's tables in `xeno.exe` (`docs/screens.md`, "The review"): its
//! widget set (the turning model's view and the two buttons), its key table, the
//! three text panels, the font and colour they are drawn in, and its pages (each a
//! robot model and three texts from the host's string segment). They are game data,
//! so they are read from the user's own `xeno.exe` at run time and never stored in
//! this repository.
//!
//! As in [`crate::maproom`], each table is found from the instruction that uses it,
//! after checking that instruction's bytes, and segments come only from words the
//! executable relocates.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, STRINGS_SEGMENT, invalid, rect, segment, word};
use crate::sequence;

/// The widget types (offsets into the host's handler tables) of the review's
/// picture, its text panels, the model's view and its two buttons.
pub const PICTURE_TYPE: u16 = 0x40;
pub const TEXT_TYPE: u16 = 0x42;
pub const VIEW_TYPE: u16 = 0x44;
pub const NEXT_BUTTON_TYPE: u16 = 0x46;
pub const EXIT_BUTTON_TYPE: u16 = 0x48;

/// The key handlers (offsets in segment `1000`) the key table names: leave
/// (`ui_review_leave`), the next and the previous page.
pub const LEAVE: u16 = 0x7a3b;
pub const NEXT_PAGE: u16 = 0x7a41;
pub const PREVIOUS_PAGE: u16 = 0x7a4c;

/// A widget: type, state offset, then its rectangle.
const WIDGET_SIZE: usize = 12;
/// The image offset of Ghidra's segment `1fe6`, the kernel.
const KERNEL: usize = 0xfe60;

/// Load-image offsets of the instructions read, and the bytes before each operand.
pub(crate) const WIDGETS_SEGMENT: (usize, &[u8]) = (0x797d, &[0xb8]);
pub(crate) const WIDGETS_FIRST: (usize, &[u8]) = (0x7980, &[0xbb]);
pub(crate) const WIDGETS_END: (usize, &[u8]) = (0x7986, &[0xba]);
pub(crate) const KEYS: (usize, &[u8]) = (0x7a23, &[0xbb]);
pub(crate) const KEY_COUNT: (usize, &[u8]) = (0x7a26, &[0xb9]);
pub(crate) const FONT: (usize, &[u8]) = (KERNEL + 0x7874, &[0xb8]);
pub(crate) const COLOUR: (usize, &[u8]) = (KERNEL + 0x787c, &[0xb8]);
pub(crate) const PANELS: [(usize, &[u8]); 3] = [
    (KERNEL + 0x7886, &[0xbb]),
    (KERNEL + 0x78a4, &[0xbb]),
    (KERNEL + 0x78c2, &[0xbb]),
];
/// `cmp word [0x61aa], imm8`: the last page's number (pages count by 2).
pub(crate) const LAST_PAGE: (usize, &[u8]) = (KERNEL + 0x7a12, &[0x83, 0x3e, 0xaa, 0x61]);
/// `jmp cs:[bx + table]`: the pages' set-up code, by page number.
pub(crate) const PAGE_TABLE: (usize, &[u8]) = (KERNEL + 0x7a4d, &[0x2e, 0xff, 0xa7]);
/// In a page's set-up: `mov byte [0x61b9], model` first, then the three texts' offsets
/// in the string segment, each `mov word [panel's word], offset`.
pub(crate) const PAGE_MODEL: (usize, &[u8]) = (0, &[0xc6, 0x06, 0xb9, 0x61]);
pub(crate) const PAGE_TEXTS: [(usize, &[u8]); 3] = [
    (0x17, &[0xc7, 0x06, 0xcc, 0x61]),
    (0x1d, &[0xc7, 0x06, 0xce, 0x61]),
    (0x23, &[0xc7, 0x06, 0xd0, 0x61]),
];

/// No widget set or key table is longer than this, and no review has more pages.
const MAX_ENTRIES: usize = 64;
/// No page's text is longer than this.
pub(crate) const MAX_TEXT: usize = 1024;

/// One page: a robot model turning in the view, and its three texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The model the page shows (`DS:0x61b9`; 0, 2 and 4 in the stock game, whose
    /// texts are about the humanoid, the scorpion and the scout).
    pub model: u8,
    /// In the panels' order; drawn up to their NUL, with the font's control codes.
    pub texts: [Vec<u8>; 3],
}

/// The review's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Where the page's model turns (drawn by the 3D renderer).
    pub view: Rect,
    /// The buttons: the next page, and back to the map room. The picture draws them.
    pub next_button: Rect,
    pub exit_button: Rect,
    /// (key word, handler), searched in order (`DS:0x6244`).
    pub keys: Vec<(u16, u16)>,
    /// The panels' font and colour.
    pub font: u16,
    pub colour: u8,
    /// Each filled with colour 0, then its text from the top-left corner.
    pub panels: [Rect; 3],
    pub pages: Vec<Page>,
}

/// A NUL-terminated string of up to [`MAX_TEXT`] bytes at `at`, without the NUL.
fn text(image: &[u8], at: usize) -> Result<Vec<u8>, FormatError> {
    let bytes = image
        .get(at..(at + MAX_TEXT).min(image.len()))
        .ok_or_else(|| invalid(at, "the text is past the end of the image"))?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid(at, "no text ends here"))?;
    Ok(bytes[..end].to_vec())
}

/// Reads the review's tables from a load image whose data segment starts at `ds`;
/// `relocated` lists the image offsets of the words the executable relocates. Error
/// offsets are into the load image.
pub fn read_image(image: &[u8], ds: usize, relocated: &[usize]) -> Result<Review, FormatError> {
    let op = |(at, op): (usize, &[u8])| operand(image, at, op);

    let set = segment(image, WIDGETS_SEGMENT, relocated)?;
    let (first, end) = (
        usize::from(op(WIDGETS_FIRST)?),
        usize::from(op(WIDGETS_END)?),
    );
    if first > end || (end - first) % WIDGET_SIZE != 0 || end - first > MAX_ENTRIES * WIDGET_SIZE {
        return Err(invalid(WIDGETS_END.0, "not a list of widgets"));
    }
    let mut areas = [None; 3];
    for at in (set + first..set + end).step_by(WIDGET_SIZE) {
        let slot = match word(image, at)? {
            VIEW_TYPE => 0,
            NEXT_BUTTON_TYPE => 1,
            EXIT_BUTTON_TYPE => 2,
            _ => continue,
        };
        if areas[slot].replace(rect(image, at + 4)?).is_some() {
            return Err(invalid(at, "a widget type twice"));
        }
    }
    let [Some(view), Some(next_button), Some(exit_button)] = areas else {
        return Err(invalid(set, "no view or no buttons"));
    };

    let keys_at = ds + usize::from(op(KEYS)?);
    let count = usize::from(op(KEY_COUNT)?);
    if count > MAX_ENTRIES {
        return Err(invalid(KEY_COUNT.0, "too many keys"));
    }
    let keys = (0..count)
        .map(|i| {
            Ok((
                word(image, keys_at + 4 * i)?,
                word(image, keys_at + 4 * i + 2)?,
            ))
        })
        .collect::<Result<_, FormatError>>()?;

    let colour = u8::try_from(op(COLOUR)?).map_err(|_| invalid(COLOUR.0, "not a colour"))?;
    let mut panels = [Rect {
        top: 0,
        left: 0,
        bottom: 0,
        right: 0,
    }; 3];
    for (panel, entry) in panels.iter_mut().zip(PANELS) {
        *panel = rect(image, ds + usize::from(op(entry)?))?;
    }

    // `cmp word [page], last; jbe` then wrap: pages 0, 2, ..., last.
    let last = match image.get(LAST_PAGE.0..LAST_PAGE.0 + LAST_PAGE.1.len() + 1) {
        Some(bytes) if bytes[..LAST_PAGE.1.len()] == *LAST_PAGE.1 => bytes[LAST_PAGE.1.len()],
        _ => return Err(invalid(LAST_PAGE.0, "not the page count")),
    };
    if last % 2 != 0 || usize::from(last) >= 2 * MAX_ENTRIES {
        return Err(invalid(LAST_PAGE.0, "not a page number"));
    }
    let table = KERNEL + usize::from(op(PAGE_TABLE)?);
    let strings = segment(image, STRINGS_SEGMENT, relocated)?;
    let pages = (0..=usize::from(last) / 2)
        .map(|n| {
            let code = KERNEL + usize::from(word(image, table + 2 * n)?);
            let (at, model_op) = PAGE_MODEL;
            let model = match image.get(code + at..code + at + model_op.len() + 1) {
                Some(bytes) if bytes[..model_op.len()] == *model_op => bytes[model_op.len()],
                _ => return Err(invalid(code, "not a page's set-up")),
            };
            let mut texts: [Vec<u8>; 3] = Default::default();
            for (text_at, (at, entry_op)) in texts.iter_mut().zip(PAGE_TEXTS) {
                let offset = operand(image, code + at, entry_op)?;
                *text_at = text(image, strings + usize::from(offset))?;
            }
            Ok(Page { model, texts })
        })
        .collect::<Result<_, FormatError>>()?;

    Ok(Review {
        view,
        next_button,
        exit_button,
        keys,
        font: op(FONT)?,
        colour,
        panels,
        pages,
    })
}

/// Reads the review's tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Review, FormatError> {
    let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
    read_image(&exe.image, sequence::data_segment(exe)?, &relocated)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fixtures::{SYNTHETIC_REVIEW_STRINGS as STRINGS, synthetic_review};

    const DS: usize = 0x20000;

    fn synthetic() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; DS + 0x100];
        let relocated = synthetic_review(&mut image, DS);
        (image, relocated)
    }

    #[test]
    fn a_synthetic_review_reads() {
        let (image, relocated) = synthetic();
        let review = read_image(&image, DS, &relocated).unwrap();
        assert_eq!(review.pages.len(), 2);
        assert_eq!(review.pages[1].texts[2], b"F\x0cG");
        assert_eq!(review.keys[1], (0x0f, NEXT_PAGE));
        assert!(review.exit_button.contains(60, 190));
        insta::assert_debug_snapshot!(review);
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let (mut image, relocated) = synthetic();
            image[at] = value;
            read_image(&image, DS, &relocated)
        };
        for (at, _) in [WIDGETS_FIRST, KEYS, FONT, COLOUR, PAGE_TABLE, LAST_PAGE] {
            assert!(corrupt(at, 0x90).is_err(), "{at:#x}");
        }
        // An odd last page, too many keys, a colour past a byte, a page's set-up that is
        // not one, a widget set without its view.
        assert!(corrupt(LAST_PAGE.0 + 4, 3).is_err());
        assert!(corrupt(KEY_COUNT.0 + 2, 1).is_err());
        assert!(corrupt(COLOUR.0 + 2, 1).is_err());
        assert!(corrupt(KERNEL + 0x7b40, 0x90).is_err());
        assert!(corrupt(KERNEL + 0x7b70 + 0x1d, 0x90).is_err());
        assert!(corrupt(crate::fixtures::SYNTHETIC_REVIEW_WIDGETS + 0x28, 0).is_err());
        // A text with no end.
        let (mut image, relocated) = synthetic();
        image[STRINGS..STRINGS + MAX_TEXT].fill(b'X');
        assert!(read_image(&image, DS, &relocated).is_err());
        // A segment that is not relocated.
        assert!(read_image(&image, DS, &relocated[1..]).is_err());
    }

    /// Reads the review from the user's `xeno.exe`, checking only its shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_review_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let review = read(&exe).unwrap();
        // `docs/screens.md`: three pages, four keys (two leave, next, previous), font 4.
        assert_eq!(review.pages.len(), 3);
        assert_eq!(review.keys.len(), 4);
        assert_eq!(review.font, 4);
        for handler in [LEAVE, NEXT_PAGE, PREVIOUS_PAGE] {
            assert!(review.keys.iter().any(|&(_, h)| h == handler));
        }
        for area in review
            .panels
            .iter()
            .chain([&review.view, &review.next_button])
        {
            assert!(area.right <= 320 && area.bottom <= 200, "{area:?}");
        }
        for page in &review.pages {
            assert!(page.texts.iter().all(|t| !t.is_empty()));
        }
    }
}
