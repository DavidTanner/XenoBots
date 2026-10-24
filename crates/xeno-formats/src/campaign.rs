//! The campaign's rules and the rest of the map room's tables in `xeno.exe`
//! (`docs/screens.md`, "Campaign rules" and "The hover label"): the developer keys'
//! unlock values, the bonus a won scenario adds to the score, the open-scenario limit
//! under which a win raises the progress, the progress cap, the alert chances, the
//! hover label's words, the campaign-won box and its message, and the selection
//! panel's prompts. They are game data, so they are read from the user's own
//! `xeno.exe` at run time and never stored in this repository.
//!
//! As in [`crate::maproom`], each value is found from the instruction that uses it,
//! after checking that instruction's bytes, and segments come only from words the
//! executable relocates.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{
    ALERT_LABEL, Rect, STATES, STRINGS_SEGMENT, invalid, rect, segment, string, word,
};
use crate::review::text;
use crate::sequence;

/// The image offset of Ghidra's segment `1fe6`, the kernel.
const KERNEL: usize = 0xfe60;

/// Load-image offsets of the instructions read, and the bytes before each operand.
/// `cmp word [0x1598], imm` in `ui_map_room_dev_key_won` (unlock word 8).
pub(crate) const DEVELOPER_UNLOCK: (usize, &[u8]) = (0x752d, &[0x81, 0x3e, 0x98, 0x15]);
/// `cmp word [0x5bc1], imm` in `ui_map_room_open_editor` (unlock word 9).
pub(crate) const EDITOR_UNLOCK: (usize, &[u8]) = (0x6bcf, &[0x81, 0x3e, 0xc1, 0x5b]);
/// `add word [0x6014], imm` in `scen_win_selected_scenario`.
pub(crate) const WIN_BONUS: (usize, &[u8]) = (0x757d, &[0x81, 0x06, 0x14, 0x60]);
/// `cmp word [0x5fff], imm8` in `scen_win_selected_scenario`.
pub(crate) const OPEN_LIMIT: (usize, &[u8]) = (0x758f, &[0x83, 0x3e, 0xff, 0x5f]);
/// `cmp word [0x5f9b], imm8` in `ui_map_room_raise_progress`.
pub(crate) const MAX_PROGRESS: (usize, &[u8]) = (0x74ee, &[0x83, 0x3e, 0x9b, 0x5f]);
/// `mov bx, [bx + table]` in `scen_win_selected_scenario`.
pub(crate) const CHANCES: (usize, &[u8]) = (0x75d8, &[0x8b, 0x9f]);
/// `mov ax, [bx + table]` in `ui_draw_map_tooltip`.
pub(crate) const HOVER_WORDS: (usize, &[u8]) = (0x7764, &[0x8b, 0x87]);
/// `mov bx, rect` and `mov si, message` in `ui_draw_map_room_status`.
pub(crate) const WON_BOX: (usize, &[u8]) = (KERNEL + 0x7471, &[0xbb]);
pub(crate) const WON_MESSAGE: (usize, &[u8]) = (KERNEL + 0x7493, &[0xbe]);
/// `mov si, prompt` in `ui_draw_selection_info`, with no selection: before and after
/// the campaign is won.
pub(crate) const PROMPT: (usize, &[u8]) = (KERNEL + 0x74ec, &[0xbe]);
pub(crate) const PROMPT_WON: (usize, &[u8]) = (KERNEL + 0x74f5, &[0xbe]);

/// No chance table is longer than this.
const MAX_CHANCES: usize = 16;

/// The campaign's rules and texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    /// Unlock word 8 must equal this for the developer keys (`=`, `-`, Ctrl+W,
    /// Ctrl+L), and unlock word 9 `editor_unlock` for Alt+F1 (`docs/formats/key.md`).
    pub developer_unlock: u16,
    pub editor_unlock: u16,
    /// Added to a won mission's points before they go to the score.
    pub win_bonus: u16,
    /// A win raises the progress when fewer scenarios than this are open.
    pub open_limit: u8,
    /// The developer key `=` raises the progress up to this.
    pub max_progress: u8,
    /// After a win, an open scenario's alert rises when a random word in 0..=0xfffe is
    /// below this, by its unlock level less the lowest open one. The table runs up to
    /// the alert label that follows it in the data segment.
    pub alert_chances: Vec<u16>,
    /// The hover label's word by `state / 2`.
    pub hover_words: [Vec<u8>; STATES],
    /// Filled and framed once the campaign is won, with the message in it.
    pub won_box: Rect,
    pub won_message: Vec<u8>,
    /// The selection panel's text with nothing selected, before and after the campaign
    /// is won.
    pub prompt: Vec<u8>,
    pub prompt_won: Vec<u8>,
}

impl Campaign {
    /// The chance for a scenario `above` levels over the lowest open one: 0 past the
    /// table.
    pub fn alert_chance(&self, above: u16) -> u16 {
        self.alert_chances
            .get(usize::from(above))
            .copied()
            .unwrap_or(0)
    }
}

/// The byte operand after `op` at `at`.
fn byte_operand(image: &[u8], (at, op): (usize, &[u8])) -> Result<u8, FormatError> {
    match image.get(at..at + op.len() + 1) {
        Some(bytes) if bytes[..op.len()] == *op => Ok(bytes[op.len()]),
        Some(_) => Err(invalid(
            at,
            format!("not the expected instruction {op:02x?}"),
        )),
        None => Err(invalid(at, "past the end of the image")),
    }
}

/// Reads the campaign's rules from a load image whose data segment starts at `ds`;
/// `relocated` lists the image offsets of the words the executable relocates. Error
/// offsets are into the load image.
pub fn read_image(image: &[u8], ds: usize, relocated: &[usize]) -> Result<Campaign, FormatError> {
    let op = |(at, op): (usize, &[u8])| operand(image, at, op);
    let strings = segment(image, STRINGS_SEGMENT, relocated)?;

    let chances = ds + usize::from(op(CHANCES)?);
    let label = ds + usize::from(op(ALERT_LABEL)?);
    let count = label
        .checked_sub(chances)
        .filter(|n| n % 2 == 0 && (2..=2 * MAX_CHANCES).contains(n))
        .ok_or_else(|| invalid(CHANCES.0, "the alert chances do not end at the label"))?
        / 2;
    let alert_chances = (0..count)
        .map(|i| word(image, chances + 2 * i))
        .collect::<Result<_, _>>()?;

    let hover = ds + usize::from(op(HOVER_WORDS)?);
    let mut hover_words: [Vec<u8>; STATES] = Default::default();
    for (state, words) in hover_words.iter_mut().enumerate() {
        *words = string(
            image,
            strings + usize::from(word(image, hover + 2 * state)?),
        )?;
    }

    Ok(Campaign {
        developer_unlock: op(DEVELOPER_UNLOCK)?,
        editor_unlock: op(EDITOR_UNLOCK)?,
        win_bonus: op(WIN_BONUS)?,
        open_limit: byte_operand(image, OPEN_LIMIT)?,
        max_progress: byte_operand(image, MAX_PROGRESS)?,
        alert_chances,
        hover_words,
        won_box: rect(image, ds + usize::from(op(WON_BOX)?))?,
        won_message: text(image, strings + usize::from(op(WON_MESSAGE)?))?,
        prompt: string(image, strings + usize::from(op(PROMPT)?))?,
        prompt_won: string(image, strings + usize::from(op(PROMPT_WON)?))?,
    })
}

/// Reads the campaign's rules from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Campaign, FormatError> {
    let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
    read_image(&exe.image, sequence::data_segment(exe)?, &relocated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{
        SYNTHETIC_MAP_ROOM_STRINGS as STRINGS, synthetic_campaign, synthetic_map_room,
    };
    use crate::maproom::MAX_STRING;

    const DS: usize = 0x20000;

    fn synthetic() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; DS + 0x100];
        let relocated = synthetic_map_room(&mut image, DS);
        synthetic_campaign(&mut image, DS);
        (image, relocated)
    }

    #[test]
    fn a_synthetic_campaign_reads() {
        let (image, relocated) = synthetic();
        let rules = read_image(&image, DS, &relocated).unwrap();
        assert_eq!(rules.alert_chances.len(), 4);
        assert_eq!(rules.alert_chance(1), 0x3000);
        assert_eq!(rules.alert_chance(9), 0);
        assert_eq!(rules.hover_words[6], b"H");
        assert_eq!(rules.won_message, b"W\x0cW");
        insta::assert_debug_snapshot!(rules);
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let (mut image, relocated) = synthetic();
            image[at] = value;
            read_image(&image, DS, &relocated)
        };
        for (at, _) in [
            DEVELOPER_UNLOCK,
            EDITOR_UNLOCK,
            WIN_BONUS,
            OPEN_LIMIT,
            MAX_PROGRESS,
            CHANCES,
            HOVER_WORDS,
            WON_BOX,
            WON_MESSAGE,
            PROMPT,
            PROMPT_WON,
        ] {
            assert!(corrupt(at, 0x90).is_err(), "{at:#x}");
        }
        // A chance table that does not end at the alert label.
        assert!(corrupt(CHANCES.0 + 2, 0x79).is_err());
        // A hover word with no end.
        let (mut image, relocated) = synthetic();
        image[STRINGS + 0xa0..STRINGS + 0xa0 + MAX_STRING].fill(b'X');
        assert!(read_image(&image, DS, &relocated).is_err());
        // The string segment not relocated.
        let (image, relocated) = synthetic();
        assert!(read_image(&image, DS, &relocated[..1]).is_err());
    }

    /// Reads the campaign's rules from the user's `xeno.exe`, checking only their shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_campaign_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let rules = read(&exe).unwrap();
        // `docs/screens.md`: four chances, then zeros up to the label; a falling
        // chance; the box inside the map area; every word printable.
        assert_eq!(rules.alert_chances.len(), 8);
        assert!(rules.alert_chances.windows(2).all(|w| w[0] >= w[1]));
        assert!(rules.alert_chances[0] > 0 && rules.alert_chances[3..].iter().all(|&c| c == 0));
        assert!(rules.open_limit > 0 && rules.max_progress > rules.open_limit);
        assert!(rules.won_box.right <= 288 && rules.won_box.bottom <= 192);
        let texts =
            rules
                .hover_words
                .iter()
                .chain([&rules.won_message, &rules.prompt, &rules.prompt_won]);
        for t in texts {
            assert!(!t.is_empty() && !t.ends_with(b"$"));
            assert!(t.iter().all(|&b| b == 0x0c || (0x20..0x7f).contains(&b)));
        }
    }
}
