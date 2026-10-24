//! The log-in screen's name entry (T-041), as `ui_edit_name_with_blinking_cursor` and
//! its widgets run it (`docs/screens.md`, "The name entry"): up to eight characters,
//! digits and letters turned to upper case; Enter accepts a name that is not empty,
//! ESC cancels, Backspace drops the last character. The name is drawn opaque in font 0
//! in the picture's black field, with an underscore cursor in colour 254 that
//! `mgavideo.dvr`'s colour-animation record 23 blinks.
//!
//! Then the original loads `<NAME>.LOG` or starts a campaign from `scendef.bin`. Here
//! a [`Player`] remembers the name, whether the save exists, and the campaign table
//! from `scendef.bin`; reading the save is T-057. Nothing is ever written. The goodbye
//! printed on exit is read from the user's `xeno.exe` ([`xeno_formats::farewell`]).

use xeno_formats::farewell::{self, Farewell};
use xeno_formats::font::Font;
use xeno_formats::lzexe;
use xeno_formats::mgavideo::{self, Pointer};
use xeno_formats::scendef::{self, ScenarioTable};
use xeno_render::{Framebuffer, TextCursor, TextMode};
use xeno_sim::Random;

use crate::game_dir::GameFiles;
use crate::maproom::Campaign;
use crate::screens::KeyWord;

/// The longest name (the save file's 8.3 name).
pub const MAX_NAME: usize = 8;
/// The record of `mgavideo.dvr`'s list that blinks the cursor colour.
pub const CURSOR_RECORD: u8 = 23;
/// The name's font and where its first glyph goes: the field's corner (45, 66) plus
/// (5, 3) (`ui_draw_name_entry`).
const FONT: u16 = 0;
const NAME_X: i32 = 50;
const NAME_Y: i32 = 69;
const NAME_COLOUR: u8 = 251;
const CURSOR_COLOUR: u8 = 254;
/// Drawn after a name shorter than 7: clears the two cells after it and comes back.
const CLEAR_AFTER: &[u8] = b"  \x08\x08";

const ENTER: KeyWord = KeyWord(0x1c);
const ESC: KeyWord = KeyWord(0x01);
const BACKSPACE: KeyWord = KeyWord(0x0e);

/// What the log-in needs from the game files.
#[derive(Debug, Clone)]
pub struct Assets {
    /// Font 0, the name's.
    pub font: Font,
    /// `mgavideo.dvr`'s colour-animation list, for the cursor's blink.
    pub anims: Vec<u8>,
    /// The names (lower case, no extension) of the `.log` files in the game directory.
    pub saves: Vec<String>,
    /// `scendef.bin`: a new campaign's table.
    pub campaign: ScenarioTable,
    /// The goodbye from `xeno.exe`, if this port can read it there.
    pub farewell: Option<Farewell>,
    /// The driver's mouse pointer shapes, by number, which the screens show (T-042),
    /// and the one its set-up picks (T-049).
    pub pointers: Vec<Pointer>,
    pub pointer: u8,
    /// The run's seed, which the run's random word starts from.
    pub seed: u32,
}

/// Reads the font, the colour records and the pointer from the user's `mgavideo.dvr`,
/// the campaign from `scendef.bin`, which saves exist, and the goodbye from
/// `xeno.exe`. Only the goodbye may be missing: without it the game exits silently.
/// The run's random word starts from `seed`.
pub fn load(files: &GameFiles, seed: u32) -> Result<Assets, String> {
    let read = |name: &str| {
        files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))
    };
    let video = mgavideo::parse(&read("mgavideo.dvr")?)
        .map_err(|e| format!("mgavideo.dvr: no fonts this port can read: {e}"))?;
    let font = video.font(FONT).cloned().ok_or("mgavideo.dvr: no font 0")?;
    let campaign =
        scendef::parse(&read("scendef.bin")?).map_err(|e| format!("scendef.bin: {e}"))?;
    let saves = files
        .names()
        .filter_map(|name| name.strip_suffix(".log"))
        .map(str::to_string)
        .collect();
    let farewell = read("xeno.exe")
        .and_then(|exe| lzexe::unpack(&exe).map_err(|e| format!("xeno.exe: {e}")))
        .and_then(|exe| farewell::read(&exe).map_err(|e| format!("xeno.exe: {e}")));
    let farewell = farewell
        .inspect_err(|e| log::warn!("no goodbye on exit: {e}"))
        .ok();
    Ok(Assets {
        font,
        anims: video.anims,
        saves,
        campaign,
        farewell,
        pointers: video.pointers,
        pointer: video.pointer,
        seed,
    })
}

/// Who logged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub name: String,
    /// Whether `<NAME>.LOG` exists. The original loads it; the port does not yet
    /// (T-057) and starts a new campaign either way.
    pub save_found: bool,
    /// The campaign: the scenario table with each scenario's `state`, the progress.
    pub campaign: Campaign,
    /// What the original leaves on the screen when the game exits, with the name in it.
    pub farewell: Option<String>,
}

/// What a key did to the name entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Enter with a name.
    Accepted(String),
    /// ESC.
    Cancelled,
}

/// The name being typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameEntry {
    name: String,
    /// The field needs drawing: only after a change, as in the original.
    dirty: bool,
}

impl NameEntry {
    #[cfg(test)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A key going down (`ui_name_entry_on_key`'s table: the exact key word, so not
    /// with Shift, Ctrl or Alt held).
    pub fn key(&mut self, word: KeyWord) -> Option<Outcome> {
        match word {
            ENTER if !self.name.is_empty() => Some(Outcome::Accepted(self.name.clone())),
            ESC => Some(Outcome::Cancelled),
            BACKSPACE => {
                self.dirty |= self.name.pop().is_some();
                None
            }
            _ => None,
        }
    }

    /// A character typed (`ui_name_entry_type_char`): digits and letters, lower case
    /// turned to upper, while the name is shorter than [`MAX_NAME`].
    pub fn typed(&mut self, c: char) {
        if self.name.len() < MAX_NAME && c.is_ascii_alphanumeric() {
            self.name.push(c.to_ascii_uppercase());
            self.dirty = true;
        }
    }

    /// Draws the field if it changed (`ui_draw_name_entry`): the name in colour 251,
    /// then the two cells after it cleared, then the cursor in colour 254, all opaque
    /// on black.
    pub fn draw(&mut self, font: &Font, fb: &mut Framebuffer) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let mut cursor = TextCursor::at(NAME_X, NAME_Y, NAME_COLOUR);
        fb.draw_text(font, &mut cursor, self.name.as_bytes(), TextMode::Opaque);
        if self.name.len() < MAX_NAME - 1 {
            fb.draw_text(font, &mut cursor, CLEAR_AFTER, TextMode::Opaque);
        }
        if self.name.len() < MAX_NAME {
            cursor.colour = CURSOR_COLOUR;
            fb.draw_char(font, &mut cursor, b'_', TextMode::Opaque);
        }
    }
}

impl Assets {
    /// The player who logged in as `name`: whether a save exists, and a new campaign
    /// drawing from `random`, the run's random word, carried on from the last one.
    pub fn player(&self, name: String, random: Random) -> Player {
        let save_found = self.saves.contains(&name.to_lowercase());
        if save_found {
            log::info!(
                "log-in: {name}.LOG exists; loading saves is not done yet (T-057), so a new campaign starts"
            );
        } else {
            log::info!("log-in: {name}, a new campaign from scendef.bin");
        }
        Player {
            farewell: self.farewell.as_ref().map(|f| f.message(&name)),
            name,
            save_found,
            campaign: Campaign::new(self.campaign.clone(), random),
        }
    }
}

/// Synthetic assets for tests: the synthetic font 0, a list of 24 records whose last
/// blinks colour 254 between black and white every 2 retraces, no saves but `old`,
/// the map room's test campaign, a made-up goodbye and the synthetic pointer's two shapes, the first set up.
#[cfg(test)]
pub fn test_assets() -> Assets {
    use xeno_formats::fixtures::synthetic_font_block;
    use xeno_formats::font;
    use xeno_render::anim::{flag, list};

    let fonts = font::parse(&synthetic_font_block()).unwrap();
    let mut records: Vec<(u8, u8, u8, u8, &[u8])> = vec![(1, 1, 0, 1, &[0, 0, 0]); 23];
    let blink: &[u8] = &[0, 0, 0, 63, 63, 63];
    records.push((2 | flag::LOOP | flag::DIRECTION, 2, CURSOR_COLOUR, 1, blink));
    Assets {
        font: fonts[0].clone(),
        anims: list(&records).bytes().to_vec(),
        saves: vec!["old".into()],
        campaign: crate::maproom::tests::test_campaign().table,
        farewell: Some(Farewell {
            prefix: "SEE YOU, ".into(),
            close: '!',
        }),
        pointers: mgavideo::parse(&xeno_formats::fixtures::synthetic_mgavideo())
            .unwrap()
            .pointers,
        pointer: 0,
        seed: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_render::Size;

    #[test]
    fn names_take_eight_digits_and_upper_case_letters() {
        let mut entry = NameEntry::default();
        for c in "ab-1 é?cdefghij".chars() {
            entry.typed(c);
        }
        assert_eq!(entry.name(), "AB1CDEFG");
        assert_eq!(entry.key(BACKSPACE), None);
        assert_eq!(entry.name(), "AB1CDEF");
        assert_eq!(entry.key(ENTER), Some(Outcome::Accepted("AB1CDEF".into())));
        // Shift+Enter is not Enter.
        assert_eq!(entry.key(KeyWord(0x081c)), None);
        assert_eq!(entry.key(ESC), Some(Outcome::Cancelled));
    }

    #[test]
    fn enter_needs_a_name_and_backspace_stops_at_empty() {
        let mut entry = NameEntry::default();
        assert_eq!(entry.key(ENTER), None);
        assert_eq!(entry.key(BACKSPACE), None);
        assert!(!entry.dirty);
        entry.typed('x');
        entry.key(BACKSPACE);
        assert_eq!((entry.name(), entry.dirty), ("", true));
    }

    #[test]
    fn the_field_is_drawn_only_after_a_change() {
        let font = test_assets().font;
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(9);
        let mut entry = NameEntry::default();
        entry.draw(&font, &mut fb);
        assert_eq!(fb.get(NAME_X as usize, NAME_Y as usize), Some(9));
        entry.typed('A');
        entry.draw(&font, &mut fb);
        // The synthetic font's cells are 4 wide and 2 rows high: `A`, two cleared
        // cells, then the cursor (blank in this font) on black.
        let (x, y) = (NAME_X as usize, NAME_Y as usize);
        assert_eq!(fb.get(x, y), Some(NAME_COLOUR));
        assert_eq!(fb.get(x + 3, y), Some(0));
        assert_eq!(fb.get(x + 4, y), Some(0));
        assert_eq!(fb.get(x + 7, y + 1), Some(0));
        assert_eq!(fb.get(x + 8, y), Some(0));
        assert_eq!(fb.get(x + 16, y), Some(9));
        assert_eq!(fb.get(x, y + 2), Some(9));
    }

    #[test]
    fn a_player_knows_whether_a_save_exists() {
        let assets = test_assets();
        let random = || Random::seeded(assets.seed);
        assert!(assets.player("OLD".into(), random()).save_found);
        let player = assets.player("NEW".into(), random());
        assert!(!player.save_found);
        assert_eq!(player.farewell.as_deref(), Some("SEE YOU, NEW!"));
    }
}
