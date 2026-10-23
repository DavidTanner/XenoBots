//! The map room (T-042), as `ui_run_map_room_loop` and its widgets run it
//! (`docs/screens.md`, "The map room (T-042)"): one box per scenario of the campaign,
//! drawn in a look chosen by its `state`, or as unavailable while its unlock level is
//! above the player's progress; the selected one highlighted; the player's name, the
//! score and the selected scenario's name and alert level in panels; five buttons.
//!
//! A scenario is picked by a click (on the release; the right button also plays it),
//! or with the next and previous keys. The tables, sprites and words all come from the
//! user's `xeno.exe`, `msprite5.b@n` and `mgavideo.dvr`
//! ([`xeno_formats::maproom`]); the fonts, colours and blit modes the drawing code
//! passes are constants here.
//!
//! Not done here (T-046): the hover label, the campaign-won box, the developer keys,
//! and the campaign's changes when a scenario is played and when a mission ends.

use xeno_formats::ban::{self, Sprite};
use xeno_formats::font::Font;
use xeno_formats::maproom::{self, HANDLE_STEP, MapRoom};
use xeno_formats::scendef::ScenarioTable;
use xeno_formats::{lzexe, mgavideo};
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};

use crate::game_dir::GameFiles;
use crate::input::MouseButton;

/// The fonts and colour of the panels' text (`ui_draw_map_room_status`: font 0 for the
/// name and score, font 4 for the selected scenario; colour 0xe0, transparent).
const NAME_FONT: u16 = 0;
const INFO_FONT: u16 = 4;
const TEXT_COLOUR: u8 = 0xe0;
/// A new campaign's progress and result flag (`scen_start_new_campaign`).
const FIRST_PROGRESS: u16 = 1;
const NEW_CAMPAIGN: u8 = 0xff;
/// The `state` of a locked, a won and an in-play scenario.
pub const LOCKED: u16 = 8;
pub const WON: u16 = 0xa;
pub const IN_PLAY: u16 = 0xc;

/// What the map room needs from the game files.
#[derive(Debug, Clone)]
pub struct Assets {
    pub tables: MapRoom,
    /// `msprite5`, from `tables.bank_base`.
    pub sprites: Vec<Sprite>,
    pub name_font: Font,
    pub info_font: Font,
}

/// Reads the tables from the user's `xeno.exe`, the sprites from `msprite5.b@n` and
/// the fonts from `mgavideo.dvr`.
pub fn load(files: &GameFiles) -> Result<Assets, String> {
    let read = |name: &str| {
        files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))
    };
    let exe = lzexe::unpack(&read("xeno.exe")?).map_err(|e| format!("xeno.exe: {e}"))?;
    let tables = maproom::read(&exe)
        .map_err(|e| format!("xeno.exe: no map room tables this port can read: {e}"))?;
    let sprites = ban::parse_sprites(&read("msprite5.b@n")?)
        .map_err(|e| format!("msprite5.b@n: {e}"))?
        .sprites;
    let video =
        mgavideo::parse(&read("mgavideo.dvr")?).map_err(|e| format!("mgavideo.dvr: {e}"))?;
    let font = |id| {
        video
            .font(id)
            .cloned()
            .ok_or(format!("mgavideo.dvr: no font {id}"))
    };
    Ok(Assets {
        name_font: font(NAME_FONT)?,
        info_font: font(INFO_FONT)?,
        tables,
        sprites,
    })
}

impl Assets {
    /// The sprite with `handle`, if the bank has it.
    fn sprite(&self, handle: u16) -> Option<&Sprite> {
        self.sprites.get(self.tables.sprite_index(handle)?)
    }

    /// Draws the sprite with `handle` with its top-left at (`x`, `y`).
    fn blit(&self, fb: &mut Framebuffer, handle: u16, (x, y): (u16, u16), mode: BlitMode) {
        let blit = SpriteBlit {
            mode,
            ..SpriteBlit::default()
        };
        match self.sprite(handle) {
            Some(sprite) => {
                fb.blit_sprite(sprite, x.into(), y.into(), blit);
            }
            None => log::warn!("map room: no sprite {handle:#x} in msprite5"),
        }
    }
}

/// The campaign as the map room sees it: the scenario table with each `state`, the
/// progress (`DS:0x5f9b`) and the last campaign mission's result (`DS:0x5fab`: 0xff
/// for a new campaign, 0 while a mission has no result, 1 won, 2 lost).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    pub table: ScenarioTable,
    pub progress: u16,
    pub result: u8,
}

impl Campaign {
    /// A new campaign from `scendef.bin` (`scen_start_new_campaign`), checked as the
    /// game checks it after (`scen_update_campaign_progress`).
    pub fn new(table: ScenarioTable) -> Campaign {
        let mut campaign = Campaign {
            table,
            progress: FIRST_PROGRESS,
            result: NEW_CAMPAIGN,
        };
        campaign.check();
        campaign
    }

    /// Locks the scenarios above the progress (`state` 8) and opens those at 8 that
    /// it has reached (0).
    pub fn check(&mut self) {
        for scenario in &mut self.table.scenarios {
            if scenario.unlock_level > self.progress {
                scenario.state = LOCKED;
            } else if scenario.state == LOCKED {
                scenario.state = 0;
            }
        }
    }

    /// Whether scenario `i` can be picked: within the progress and not won.
    pub fn selectable(&self, i: usize) -> bool {
        self.table
            .scenarios
            .get(i)
            .is_some_and(|s| s.unlock_level <= self.progress && s.state != WON)
    }

    /// Whether the selection may move to `i` (`ui_map_room_set_selection`): while the
    /// last campaign mission has no result, only to the scenario in play.
    fn accepts(&self, i: usize) -> bool {
        self.result != 0
            || self
                .table
                .scenarios
                .get(i)
                .is_some_and(|s| s.state == IN_PLAY)
    }

    /// Whether training is allowed (the T key's check of `DS:0x5fab`).
    pub fn may_train(&self) -> bool {
        self.result != 0
    }

    /// The first scenario whose box holds (`x`, `y`), edges included
    /// (`scen_hit_test_map_box`).
    pub fn hit(&self, x: u16, y: u16) -> Option<usize> {
        self.table.scenarios.iter().position(|s| {
            let b = s.map_box;
            (b.left..=b.right).contains(&x) && (b.top..=b.bottom).contains(&y)
        })
    }
}

/// What a click on a button or the map area asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// What this key word does: a button runs the handler its keys run.
    Key(u16),
    /// Play the selected scenario (the right button on a box).
    Play,
}

/// Where a press captured the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Map,
    Button(usize),
}

/// The map room's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapState {
    /// The selected scenario (`DS:0x5f97`).
    selection: Option<usize>,
    /// The last button pressed (`DS:0x5f0b`), and whether the pressed look is on
    /// (`DS:0x5f0d`, flipped by each press and each release over the button).
    pressed: Option<usize>,
    pressed_look: bool,
    /// The widget a press captured the pointer for, and with which button.
    capture: Option<(Target, MouseButton)>,
    /// Something changed since the last draw.
    dirty: bool,
}

impl MapState {
    pub fn selection(&self) -> Option<usize> {
        self.selection
    }

    /// Whether the map room needs drawing again; clears it.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Moves the selection to `i` if the campaign accepts it; otherwise the original
    /// beeps (`1000:0270`, not traced) and keeps the old one.
    fn select(&mut self, i: usize, campaign: &Campaign) -> bool {
        if !campaign.accepts(i) {
            log::debug!("map room: scenario {i} refused while a mission has no result");
            return false;
        }
        self.selection = Some(i);
        self.dirty = true;
        true
    }

    /// The next (`step` 1) or previous (`step` -1) selectable scenario, wrapping, for
    /// at most one round (`ui_map_room_select_next`, `_previous`); with no selection,
    /// the first selectable one, without the campaign's check.
    pub fn step(&mut self, step: isize, campaign: &Campaign) {
        let count = campaign.table.scenarios.len();
        match self.selection {
            None => {
                self.selection = (0..count).find(|&i| campaign.selectable(i));
                self.dirty = true;
            }
            Some(from) => {
                let next = (1..=count)
                    .map(|n| (from as isize + step * n as isize).rem_euclid(count as isize))
                    .map(|i| i as usize)
                    .find(|&i| campaign.selectable(i))
                    .unwrap_or(from);
                self.select(next, campaign);
            }
        }
    }

    /// A mouse button went down at `at` (`None` off the picture).
    pub fn mouse_down(&mut self, button: MouseButton, at: Option<(u16, u16)>, assets: &Assets) {
        if self.capture.is_some() || button == MouseButton::Middle {
            return;
        }
        let Some(target) = at.and_then(|at| target(assets, at)) else {
            return;
        };
        self.capture = Some((target, button));
        if let Target::Button(i) = target {
            self.pressed = Some(i);
            self.pressed_look = !self.pressed_look;
            self.dirty = true;
        }
    }

    /// A mouse button went up with the pointer at `at`.
    pub fn mouse_up(
        &mut self,
        button: MouseButton,
        at: Option<(u16, u16)>,
        assets: &Assets,
        campaign: &Campaign,
    ) -> Option<Request> {
        let (target, by) = self.capture?;
        if by != button {
            return None;
        }
        self.capture = None;
        match target {
            // Over the button that was pressed: flip the look back and run it. Released
            // elsewhere, nothing happens and the look stays as it is.
            Target::Button(i) => {
                if at.and_then(|at| self::target(assets, at)) != Some(target) {
                    return None;
                }
                self.pressed_look = !self.pressed_look;
                self.pressed = None;
                self.dirty = true;
                let button = &assets.tables.buttons[i];
                assets.tables.button_keys(button).next().map(Request::Key)
            }
            // The map area hit-tests the release, wherever it is.
            Target::Map => {
                let (x, y) = at?;
                let i = campaign.hit(x, y).filter(|&i| campaign.selectable(i))?;
                let selected = self.select(i, campaign);
                (selected && button == MouseButton::Right).then_some(Request::Play)
            }
        }
    }

    /// Draws the boxes, the selection, the panels and the buttons over the map room's
    /// picture, for the player `name`.
    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets, campaign: &Campaign, name: &str) {
        let tables = &assets.tables;
        for scenario in &campaign.table.scenarios {
            let corner = (scenario.map_box.left, scenario.map_box.top);
            let look = if scenario.unlock_level > campaign.progress {
                Some(tables.unavailable)
            } else {
                look(tables, scenario.state)
            };
            match look {
                Some(handle) => assets.blit(fb, handle, corner, BlitMode::Keyed),
                None => log::warn!("map room: no look for state {}", scenario.state),
            }
        }
        let selected = self.selection.and_then(|i| campaign.table.scenarios.get(i));
        if let Some(scenario) = selected {
            let corner = (scenario.map_box.left, scenario.map_box.top);
            assets.blit(fb, tables.highlight, corner, BlitMode::Keyed);
        }

        // ui_draw_map_room_status: the name, the score (none yet: T-046), and the
        // selected scenario's name, the label and its alert level.
        let text = |fb: &mut Framebuffer, panel: maproom::Panel, font: &Font, lines: &[&[u8]]| {
            let corner = (panel.area.left, panel.area.top);
            assets.blit(fb, panel.sprite, corner, BlitMode::Opaque);
            let mut cursor = TextCursor::at(corner.0.into(), corner.1.into(), TEXT_COLOUR);
            for line in lines {
                fb.draw_text(font, &mut cursor, line, TextMode::Transparent);
            }
        };
        text(fb, tables.name_panel, &assets.name_font, &[name.as_bytes()]);
        text(fb, tables.score_panel, &assets.name_font, &[b"00000"]);
        let info: Vec<&[u8]> = match selected {
            Some(scenario) => {
                let level = state_index(scenario.state)
                    .map_or(&[][..], |i| tables.alert_levels[i].as_slice());
                vec![&scenario.name[..], &tables.alert_label[..], level]
            }
            None => Vec::new(),
        };
        text(fb, tables.info_panel, &assets.info_font, &info);

        for (i, button) in tables.buttons.iter().enumerate() {
            let pressed = self.pressed == Some(i) && self.pressed_look;
            let handle = button.sprite + if pressed { HANDLE_STEP } else { 0 };
            let corner = (button.area.left, button.area.top);
            assets.blit(fb, handle, corner, BlitMode::Opaque);
        }
    }
}

/// The widget under (`x`, `y`): a button (they lie over the map area's corner), the
/// map area, or none.
fn target(assets: &Assets, (x, y): (u16, u16)) -> Option<Target> {
    let tables = &assets.tables;
    if let Some(i) = tables.buttons.iter().position(|b| b.area.contains(x, y)) {
        return Some(Target::Button(i));
    }
    tables.map_area.contains(x, y).then_some(Target::Map)
}

/// The tables' index for `state`: 0, 2, ..., 12 map to 0..=6.
fn state_index(state: u16) -> Option<usize> {
    let i = usize::from(state / 2);
    (state.is_multiple_of(2) && i < maproom::STATES).then_some(i)
}

/// The box sprite for `state`. The original reads past its table for any other value;
/// nothing is drawn here.
fn look(tables: &MapRoom, state: u16) -> Option<u16> {
    state_index(state).map(|i| tables.looks[i])
}

/// The scenario file record `i` loads (`ui_map_room_play_selected`).
pub fn scenario_file(i: usize) -> String {
    format!("scen{i:02}a.usg")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use xeno_formats::ban::SpriteHeader;
    use xeno_formats::maproom::{Button, Panel, Rect};
    use xeno_formats::scendef::{MapBox, Scenario};
    use xeno_render::Size;

    /// One raw 2x2 sprite per bank entry, every pixel the sprite's number + 1, but
    /// colour 0 in its bottom-right pixel.
    fn sprites(count: u8) -> Vec<Sprite> {
        (0..count)
            .map(|n| Sprite {
                header: SpriteHeader {
                    size: 10,
                    width: 2,
                    height: 2,
                    flags: 0,
                    unknown_05: 8,
                },
                pixels: vec![n + 1, n + 1, n + 1, 0],
                spans: Vec::new(),
                trailing_empty_span: false,
            })
            .collect()
    }

    const fn rect(top: u16, left: u16, bottom: u16, right: u16) -> Rect {
        Rect {
            top,
            left,
            bottom,
            right,
        }
    }

    /// Synthetic assets: handles from 0x100, two buttons (review, then play) with the
    /// keys R and P, looks 0..=6 as sprites 10..=16 (state 6 as 16), unavailable 7,
    /// highlight 8, panels 4..=6, the synthetic fonts and made-up words.
    pub fn test_assets() -> Assets {
        use xeno_formats::fixtures::synthetic_font_block;
        let fonts = xeno_formats::font::parse(&synthetic_font_block()).unwrap();
        let tables = MapRoom {
            bank_base: 0x100,
            map_area: rect(10, 5, 190, 280),
            buttons: vec![
                Button {
                    id: 8,
                    area: rect(150, 280, 160, 310),
                    sprite: 0x100,
                    handler: 0x7408,
                },
                Button {
                    id: 0,
                    area: rect(160, 280, 170, 310),
                    sprite: 0x110,
                    handler: 0x739c,
                },
            ],
            keys: vec![(0x13, 0x7408), (0x1c, 0x739c), (0x19, 0x739c)],
            looks: [0x150, 0x158, 0x160, 0x160, 0x138, 0x140, 0x148],
            unavailable: 0x138,
            highlight: 0x140,
            name_panel: Panel {
                sprite: 0x120,
                area: rect(2, 2, 4, 4),
            },
            score_panel: Panel {
                sprite: 0x128,
                area: rect(2, 300, 4, 302),
            },
            info_panel: Panel {
                sprite: 0x130,
                area: rect(195, 100, 197, 102),
            },
            alert_label: b"L".to_vec(),
            alert_levels: Default::default(),
        };
        Assets {
            tables,
            sprites: sprites(20),
            name_font: fonts[0].clone(),
            info_font: fonts[2].clone(),
        }
    }

    fn scenario(i: u16, unlock_level: u16, state: u16) -> Scenario {
        let (top, left) = (20 + 20 * i, 20 + 20 * i);
        Scenario {
            id: 2 * i,
            map_box: MapBox {
                top,
                left,
                bottom: top + 9,
                right: left + 9,
            },
            unlock_level,
            state,
            name: "X\x0c".into(),
        }
    }

    /// Five scenarios unlocked at 1, 1, 2, 1, 1; the second won after the check.
    pub fn test_campaign() -> Campaign {
        let table = ScenarioTable {
            scenarios: [1, 1, 2, 1, 1]
                .iter()
                .enumerate()
                .map(|(i, &unlock)| scenario(i as u16, unlock, 0))
                .collect(),
        };
        let mut campaign = Campaign::new(table);
        campaign.table.scenarios[1].state = WON;
        campaign
    }

    fn corner(campaign: &Campaign, i: usize) -> (usize, usize) {
        let b = campaign.table.scenarios[i].map_box;
        (b.left.into(), b.top.into())
    }

    #[test]
    fn a_new_campaign_locks_what_is_above_its_progress() {
        let campaign = test_campaign();
        let states: Vec<_> = campaign.table.scenarios.iter().map(|s| s.state).collect();
        assert_eq!(states, [0, WON, LOCKED, 0, 0]);
        assert_eq!((campaign.progress, campaign.result), (1, 0xff));
        let selectable: Vec<_> = (0..6).map(|i| campaign.selectable(i)).collect();
        assert_eq!(selectable, [true, false, false, true, true, false]);
        assert!(campaign.may_train());
        let mut later = campaign.clone();
        later.progress = 2;
        later.check();
        assert_eq!(later.table.scenarios[2].state, 0);
    }

    #[test]
    fn boxes_take_their_look_by_state_and_level() {
        let assets = test_assets();
        let mut campaign = test_campaign();
        campaign.table.scenarios[3].state = 4;
        campaign.table.scenarios[4].state = IN_PLAY;
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(200);
        MapState::default().draw(&mut fb, &assets, &campaign, "");
        // Sprite n is drawn in colour n + 1: looks from sprite 10 (0x150 = 0x100 + 8 *
        // 10); unavailable (locked) is sprite 7, won sprite 8, in play sprite 9.
        let colours: Vec<_> = (0..5)
            .map(|i| {
                let (x, y) = corner(&campaign, i);
                fb.get(x, y).unwrap()
            })
            .collect();
        assert_eq!(colours, [11, 9, 8, 13, 10]);
        // Keyed: colour 0 leaves the picture.
        let (x, y) = corner(&campaign, 0);
        assert_eq!(fb.get(x + 1, y + 1), Some(200));
        // An unlock level above the progress wins over the state.
        campaign.table.scenarios[0].unlock_level = 3;
        MapState::default().draw(&mut fb, &assets, &campaign, "");
        assert_eq!(fb.get(x, y), Some(8));
        // A state outside the tables draws nothing.
        campaign.table.scenarios[3].state = 3;
        fb.clear(200);
        MapState::default().draw(&mut fb, &assets, &campaign, "");
        let (x, y) = corner(&campaign, 3);
        assert_eq!(fb.get(x, y), Some(200));
    }

    #[test]
    fn the_selection_panels_and_buttons_are_drawn() {
        let assets = test_assets();
        let campaign = test_campaign();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut state = MapState::default();
        state.draw(&mut fb, &assets, &campaign, "A");
        // Panels (sprites 4, 5, 6), buttons (0 and 2). The name panel's corner is under
        // the name's `A` in the synthetic font 0 (its first row is three pixels), drawn
        // transparent; the panel itself is opaque, its colour 0 included.
        assert_eq!(
            (fb.get(2, 2), fb.get(4, 2)),
            (Some(TEXT_COLOUR), Some(TEXT_COLOUR))
        );
        assert_eq!(fb.get(3, 3), Some(0));
        assert_eq!(fb.get(300, 2), Some(6));
        assert_eq!(fb.get(100, 195), Some(7));
        assert_eq!((fb.get(280, 150), fb.get(280, 160)), (Some(1), Some(3)));
        state.step(1, &campaign);
        state.draw(&mut fb, &assets, &campaign, "A");
        // The highlight (sprite 8) over the first box.
        let (x, y) = corner(&campaign, 0);
        assert_eq!(fb.get(x, y), Some(9));
    }

    #[test]
    fn keys_step_through_the_selectable_scenarios() {
        let campaign = test_campaign();
        let mut state = MapState::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            state.step(1, &campaign);
            seen.push(state.selection().unwrap());
        }
        // Skips the won and the locked ones, and wraps.
        assert_eq!(seen, [0, 3, 4, 0]);
        state.step(-1, &campaign);
        assert_eq!(state.selection(), Some(4));
        // With nothing selected, previous also picks the first.
        let mut state = MapState::default();
        state.step(-1, &campaign);
        assert_eq!(state.selection(), Some(0));
        // While a mission has no result, only the scenario in play is accepted.
        let mut busy = campaign.clone();
        busy.result = 0;
        busy.table.scenarios[3].state = IN_PLAY;
        state.step(1, &busy);
        assert_eq!(state.selection(), Some(3));
        state.step(1, &busy);
        assert_eq!(state.selection(), Some(3));
        assert!(!busy.may_train());
    }

    #[test]
    fn clicks_select_on_release_and_the_right_button_plays() {
        let assets = test_assets();
        let campaign = test_campaign();
        let mut state = MapState::default();
        let click = |state: &mut MapState, button: MouseButton, at: (u16, u16)| {
            state.mouse_down(button, Some(at), &assets);
            state.mouse_up(button, Some(at), &assets, &campaign)
        };
        let (left, right) = (MouseButton::Left, MouseButton::Right);
        // The box's edges count; the won and locked boxes and misses do nothing.
        assert_eq!(click(&mut state, left, (89, 89)), None);
        assert_eq!(state.selection(), Some(3));
        assert_eq!(click(&mut state, left, (40, 40)), None);
        assert_eq!(click(&mut state, left, (60, 60)), None);
        assert_eq!(click(&mut state, left, (150, 150)), None);
        assert_eq!(state.selection(), Some(3));
        assert_eq!(click(&mut state, right, (20, 20)), Some(Request::Play));
        assert_eq!(state.selection(), Some(0));
        // Pressed on the map, released on a box elsewhere: the release decides.
        state.mouse_down(left, Some((150, 150)), &assets);
        assert_eq!(
            state.mouse_up(right, Some((100, 100)), &assets, &campaign),
            None
        );
        state.mouse_up(left, Some((100, 100)), &assets, &campaign);
        assert_eq!(state.selection(), Some(4));
        // Outside the map area nothing is captured.
        assert_eq!(click(&mut state, left, (2, 195)), None);
    }

    #[test]
    fn buttons_run_their_keys_when_released_over_them() {
        let assets = test_assets();
        let campaign = test_campaign();
        let mut state = MapState::default();
        let left = MouseButton::Left;
        let mut fb = Framebuffer::new(Size::MODE_13H);
        state.mouse_down(left, Some((285, 165)), &assets);
        state.draw(&mut fb, &assets, &campaign, "");
        assert_eq!(
            fb.get(280, 160),
            Some(4),
            "the pressed look, the next sprite"
        );
        let request = state.mouse_up(left, Some((309, 169)), &assets, &campaign);
        assert_eq!(request, Some(Request::Key(0x1c)));
        state.draw(&mut fb, &assets, &campaign, "");
        assert_eq!(fb.get(280, 160), Some(3));
        // Released off the button: nothing, and the button stays pressed until the
        // next press flips the look back, as in the original.
        state.mouse_down(left, Some((285, 155)), &assets);
        assert_eq!(
            state.mouse_up(left, Some((285, 170)), &assets, &campaign),
            None
        );
        state.draw(&mut fb, &assets, &campaign, "");
        assert_eq!(fb.get(280, 150), Some(2));
        state.mouse_down(left, Some((285, 155)), &assets);
        state.draw(&mut fb, &assets, &campaign, "");
        assert_eq!(fb.get(280, 150), Some(1));
        let request = state.mouse_up(left, Some((285, 155)), &assets, &campaign);
        assert_eq!(request, Some(Request::Key(0x13)));
    }

    #[test]
    fn record_n_loads_its_numbered_scenario() {
        assert_eq!(scenario_file(0), "scen00a.usg");
        assert_eq!(scenario_file(12), "scen12a.usg");
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_map_room_draws_every_box() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files).unwrap();
        let table = crate::login::load(&files).unwrap().campaign;
        let mut campaign = Campaign::new(table);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut state = MapState::default();
        // Every state, every scenario selected in turn, every button pressed.
        for s in (0..=12).step_by(2) {
            for scenario in &mut campaign.table.scenarios {
                scenario.state = s;
            }
            campaign.progress = 16;
            for _ in 0..campaign.table.scenarios.len() {
                state.step(1, &campaign);
                state.draw(&mut fb, &assets, &campaign, "ABCDEFGH");
            }
        }
        for button in &assets.tables.buttons {
            let at = (button.area.left, button.area.top);
            state.mouse_down(MouseButton::Left, Some(at), &assets);
            state.draw(&mut fb, &assets, &campaign, "");
            assert!(
                state
                    .mouse_up(MouseButton::Left, Some(at), &assets, &campaign)
                    .is_some()
            );
        }
    }
}
