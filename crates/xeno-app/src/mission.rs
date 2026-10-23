//! A mission's views (T-043): the situation room, the view of the player's base, and
//! the console of each robot, as `ui_open_view_of_object` and their widgets run them
//! (`docs/screens.md`, "The mission's views", "Orders"). The simulation does not run
//! yet (T-050 on), so what it would draw (the 3D view, scanners, instruments, the
//! roster's contents) is a labelled placeholder for T-056; switching views and setting
//! orders work.
//!
//! The orders live in [`xeno_formats::orders::OrderBook`], built from the mission's
//! `.usg` file and kept for the whole mission, so they persist across views. The
//! widget sets, key tables, sprite buttons, program names and panel styles all come
//! from the user's `xeno.exe`, `msprite1.b@n` and `mgavideo.dvr`; what a key does is
//! chosen by the original handler it calls.

use std::collections::BTreeMap;

use xeno_formats::ban::{self, Sprite};
use xeno_formats::font::Font;
use xeno_formats::maproom::{HANDLE_STEP, Rect};
use xeno_formats::orders::{self, Change, OrderBook, Programs, RobotKind};
use xeno_formats::usg::{self, Scenario, Side};
use xeno_formats::views::{
    self, CONSOLE_CORNER_TYPE, MissionViews, PROGRAM_PANEL_TYPE, SITUATION_CORNER_TYPE,
    TrainingAction, TrainingMenu, UNIT_BUTTON_TYPE, View as ViewTables,
};
use xeno_formats::{lzexe, mgavideo};
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};

use crate::game_dir::GameFiles;
use crate::input::MouseButton;
use crate::maproom::scenario_file;
use crate::screens::View;

/// Original handlers (offsets in `xeno.exe`'s segment `1000`) whose keys the port acts
/// on; the key tables themselves are read from the user's copy.
mod handler {
    /// Consoles: the side's base (ESC, Alt+Q).
    pub const BASE: u16 = 0x7ee7;
    pub const AUTOMATIC: u16 = 0xd446;
    pub const MANUAL: u16 = 0xd42c;
    pub const AUTOMATIC_NEXT: u16 = 0xd3a2;
    pub const NEXT_UNIT: u16 = 0x7db2;
    pub const PREVIOUS_UNIT: u16 = 0x7dd9;
    /// Situation room: ends the mission on Alt+Q only (ESC shares it).
    pub const END: u16 = 0x7cf2;
    pub const VIEW_SELECTED: u16 = 0x6852;
    pub const NEXT_SELECTED: u16 = 0x67e8;
    pub const PREVIOUS_SELECTED: u16 = 0x680c;
}

/// Alt+Q, the only key word the situation room's end handler acts on.
const ALT_Q: u16 = 0x2010;
/// The situation room's sprite buttons, by widget type (`docs/screens.md`).
mod button {
    pub const SCROLL_UP: u16 = 0x54;
    pub const SCROLL_DOWN: u16 = 0x56;
    pub const VIEW: u16 = 0x58;
    pub const LAUNCH: u16 = 0x5a;
    pub const RECALL: u16 = 0x5c;
    pub const END: u16 = 0x5e;
}
/// The unit-name button's font and colour, and the XOR of its pressed frame
/// (`1000:7d55`, `ui_unit_name_button_on_mouse`); the training buttons' XOR.
const UNIT_FONT: u16 = 0;
const UNIT_COLOUR: u8 = 0;
const UNIT_PRESSED_XOR: u8 = 8;
pub const TRAINING_PRESSED_XOR: u8 = 9;
/// The colours that mark where the mission draws (`docs/formats/m@l.md`): 255 in the
/// situation room, 253 to 255 in the consoles.
const VIEW_COLOURS: [u8; 3] = [253, 254, 255];
/// The port's own placeholder marks (not the original's).
const PLACEHOLDER: u8 = 255;
const PLACEHOLDER_INK: u8 = 0;

/// What a mission needs from the game files.
#[derive(Debug, Clone)]
pub struct Assets {
    pub views: MissionViews,
    pub training: TrainingMenu,
    pub programs: Programs,
    /// By font id.
    pub fonts: BTreeMap<u16, Font>,
    /// `msprite1`, from `views.situation_bank`.
    pub sprites: Vec<Sprite>,
    /// Every training and campaign scenario, by lower-case file name; files that are
    /// missing or do not parse are left out (and logged).
    pub scenarios: BTreeMap<String, Scenario>,
}

/// Reads the views, the training menu and the programs from the user's `xeno.exe`,
/// `msprite1.b@n`, the fonts from `mgavideo.dvr`, and the scenarios named by the
/// training menu and the campaign's `scendef.bin` records.
pub fn load(files: &GameFiles, campaign_records: usize) -> Result<Assets, String> {
    let read = |name: &str| {
        files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))
    };
    let exe = lzexe::unpack(&read("xeno.exe")?).map_err(|e| format!("xeno.exe: {e}"))?;
    let no = |what: &str, e| format!("xeno.exe: no {what} this port can read: {e}");
    let views = views::read(&exe).map_err(|e| no("mission views", e))?;
    let training = views::read_training(&exe).map_err(|e| no("training menu", e))?;
    let programs = orders::read(&exe).map_err(|e| no("program table", e))?;
    let sprites = ban::parse_sprites(&read("msprite1.b@n")?)
        .map_err(|e| format!("msprite1.b@n: {e}"))?
        .sprites;
    let video =
        mgavideo::parse(&read("mgavideo.dvr")?).map_err(|e| format!("mgavideo.dvr: {e}"))?;
    let mut fonts = BTreeMap::new();
    for id in views
        .program_panel
        .iter()
        .map(|s| s.font)
        .chain([UNIT_FONT])
    {
        let font = video
            .font(id)
            .ok_or(format!("mgavideo.dvr: no font {id}"))?;
        fonts.insert(id, font.clone());
    }
    let names = training
        .actions
        .iter()
        .filter_map(|a| match a {
            TrainingAction::Mission(name) => Some(file_name(name)),
            TrainingAction::Back => None,
        })
        .chain((0..campaign_records).map(scenario_file));
    let mut scenarios = BTreeMap::new();
    for name in names {
        match read(&name).and_then(|b| usg::parse(&b).map_err(|e| format!("{name}: {e}"))) {
            Ok(scenario) => {
                scenarios.insert(name, scenario);
            }
            Err(e) => log::warn!("mission: {e}"),
        }
    }
    Ok(Assets {
        views,
        training,
        programs,
        fonts,
        sprites,
        scenarios,
    })
}

/// A padded 8.3 name as the game writes it, as a lower-case file name.
pub fn file_name(padded: &[u8]) -> String {
    String::from_utf8_lossy(padded)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

impl Assets {
    fn sprite(&self, handle: u16) -> Option<&Sprite> {
        let offset = handle.checked_sub(self.views.situation_bank)?;
        offset
            .is_multiple_of(HANDLE_STEP)
            .then(|| self.sprites.get(usize::from(offset / HANDLE_STEP)))
            .flatten()
    }

    #[cfg(test)]
    pub fn tables_for_test(&self, view: View) -> &ViewTables {
        self.tables(view)
    }

    fn tables(&self, view: View) -> &ViewTables {
        match view {
            View::SitRoom => &self.views.situation,
            View::Humanoid => &self.views.humanoid,
            View::Scorpion => &self.views.scorpion,
            View::Scout => &self.views.scout,
        }
    }

    /// Whether the view's key table binds ESC.
    pub fn claims_escape(&self, view: View) -> bool {
        self.tables(view).keys.iter().any(|&(word, _)| word == 0x01)
    }

    /// The first training mission, in the menu's order, with a human robot of `kind`
    /// (for `--screen` with a console), or the first one for the situation room.
    pub fn sample(&self, kind: Option<RobotKind>) -> Option<(String, &Scenario)> {
        self.training.actions.iter().find_map(|a| {
            let TrainingAction::Mission(name) = a else {
                return None;
            };
            let name = file_name(name);
            let scenario = self.scenarios.get(&name)?;
            let book = OrderBook::from_scenario(scenario);
            let fits = kind.is_none_or(|k| book.side(Side::Human).any(|(_, r)| r.kind == k));
            fits.then_some((name, scenario))
        })
    }
}

/// The object a view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewed {
    Base,
    Robot(u16),
}

/// What the screens must do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Show this view (the mission's state already points at its object).
    Show(View),
    /// The mission is over, with no result (the quit flag).
    End,
}

/// Where a press captured the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Panel,
    UnitButton,
    Corner,
    Roster(usize),
    Button(usize),
}

/// A mission in progress: its file, the orders, and the views' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    /// The `.usg` file it was loaded from.
    pub file: String,
    pub book: OrderBook,
    viewed: Viewed,
    /// The situation room's roster: its selection (base +0x3e) and first row (+0x4c).
    selected: Option<u16>,
    roster_top: usize,
    /// The program panel is pressed, and which way its colours are swapped (+0x22).
    panel_pressed: bool,
    panel_swap: bool,
    /// The unit-name button's frame is XORed.
    unit_pressed: bool,
    /// The situation room's buttons' shared pressed look (base +0x40).
    button_look: bool,
    capture: Option<(Target, MouseButton)>,
    dirty: bool,
}

impl Mission {
    /// A mission on `scenario`, starting in the situation room.
    pub fn new(file: String, scenario: &Scenario) -> Mission {
        Mission::with_book(file, OrderBook::from_scenario(scenario))
    }

    /// A mission with no file (no robots): only the views' pictures.
    pub fn empty() -> Mission {
        Mission::with_book(String::new(), OrderBook::default())
    }

    fn with_book(file: String, book: OrderBook) -> Mission {
        if !file.is_empty() {
            log::info!(
                "mission: {file}, {} robots of the player's with orders",
                book.side(Side::Human).count()
            );
        }
        let selected = book.side(Side::Human).map(|(slot, _)| slot).next();
        Mission {
            file,
            book,
            viewed: Viewed::Base,
            selected,
            roster_top: 0,
            panel_pressed: false,
            panel_swap: false,
            unit_pressed: false,
            button_look: false,
            capture: None,
            dirty: true,
        }
    }

    #[cfg(test)]
    pub fn viewed(&self) -> Viewed {
        self.viewed
    }

    /// Views `robot` (the player's), as with Tab or the roster.
    pub fn view_robot(&mut self, slot: u16) -> Option<View> {
        let robot = *self.book.get(slot)?;
        if robot.side != Side::Human {
            log::debug!("mission: cannot view the enemy's unit {slot}");
            return None;
        }
        self.viewed = Viewed::Robot(slot);
        self.reset_looks();
        Some(view_of(robot.kind))
    }

    /// The object viewed, for the log: the base, or the robot's slot and kind.
    pub fn viewed_label(&self) -> String {
        match self.viewed {
            Viewed::Base => "the base".into(),
            Viewed::Robot(slot) => match self.book.get(slot) {
                Some(robot) => format!("unit {slot} ({:?})", robot.kind),
                None => format!("unit {slot}"),
            },
        }
    }

    /// The view of the object viewed.
    pub fn view(&self) -> View {
        match self.viewed {
            Viewed::Base => View::SitRoom,
            Viewed::Robot(slot) => self
                .book
                .get(slot)
                .map_or(View::SitRoom, |r| view_of(r.kind)),
        }
    }

    /// Whether the view needs drawing again; clears it.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn reset_looks(&mut self) {
        self.panel_pressed = false;
        self.unit_pressed = false;
        self.capture = None;
        self.dirty = true;
    }

    fn base(&mut self) -> Request {
        self.viewed = Viewed::Base;
        self.reset_looks();
        Request::Show(View::SitRoom)
    }

    /// The player's robots, by slot (the port's order for the side's unit list).
    fn robots(&self) -> Vec<u16> {
        self.book.side(Side::Human).map(|(slot, _)| slot).collect()
    }

    /// The next (`step` 1) or previous robot after `from`, wrapping.
    fn step_robot(&self, from: Option<u16>, step: isize) -> Option<u16> {
        let robots = self.robots();
        let i = from.and_then(|s| robots.iter().position(|&r| r == s));
        let n = robots.len() as isize;
        if n == 0 {
            return None;
        }
        let next = match i {
            Some(i) => (i as isize + step).rem_euclid(n),
            None if step > 0 => 0,
            None => n - 1,
        };
        Some(robots[next as usize])
    }

    fn log_change(&mut self, change: Option<Change>, programs: &Programs) {
        if let Some(c) = change {
            log::info!(
                "orders: unit {} program {} ({}) -> {} ({}), {} -> {}",
                c.slot,
                c.before.program,
                String::from_utf8_lossy(programs.name(c.before.program)),
                c.after.program,
                String::from_utf8_lossy(programs.name(c.after.program)),
                mode(c.before.manual),
                mode(c.after.manual)
            );
            self.dirty = true;
        }
    }

    fn select_unit(&mut self, slot: Option<u16>) {
        if let Some(slot) = slot {
            log::debug!("situation room: unit {slot} selected");
            self.selected = Some(slot);
            let robots = self.robots();
            if let Some(i) = robots.iter().position(|&r| r == slot) {
                let rows = 5;
                if i < self.roster_top {
                    self.roster_top = i;
                } else if i >= self.roster_top + rows {
                    self.roster_top = i + 1 - rows;
                }
            }
            self.dirty = true;
        }
    }

    fn view_selected(&mut self) -> Option<Request> {
        let slot = self.selected?;
        self.view_robot(slot).map(Request::Show)
    }

    /// The key word `word` went down in the view shown.
    pub fn key(&mut self, word: u16, assets: &Assets) -> Option<Request> {
        let view = self.view();
        let handler = assets
            .tables(view)
            .keys
            .iter()
            .find_map(|&(w, h)| (w == word).then_some(h))?;
        let programs = &assets.programs;
        match (self.viewed, handler) {
            (Viewed::Robot(_), handler::BASE) => Some(self.base()),
            (Viewed::Robot(slot), handler::AUTOMATIC) => {
                let change = self.book.set_manual(slot, false);
                self.log_change(change, programs);
                None
            }
            (Viewed::Robot(slot), handler::MANUAL) => {
                let change = self.book.set_manual(slot, true);
                self.log_change(change, programs);
                None
            }
            (Viewed::Robot(slot), handler::AUTOMATIC_NEXT) => {
                let change = self.book.select_next(slot, programs);
                self.log_change(change, programs);
                None
            }
            (Viewed::Robot(slot), handler::NEXT_UNIT | handler::PREVIOUS_UNIT) => {
                let step = if handler == handler::NEXT_UNIT { 1 } else { -1 };
                let next = self.step_robot(Some(slot), step)?;
                self.view_robot(next).map(Request::Show)
            }
            (Viewed::Base, handler::END) => (word == ALT_Q).then_some(Request::End),
            (Viewed::Base, handler::VIEW_SELECTED) => self.view_selected(),
            (Viewed::Base, handler::NEXT_SELECTED | handler::PREVIOUS_SELECTED) => {
                let step = if handler == handler::NEXT_SELECTED {
                    1
                } else {
                    -1
                };
                let next = self.step_robot(self.selected, step);
                self.select_unit(next);
                None
            }
            _ => {
                log::debug!(
                    "mission: key {word:#06x} (handler {handler:#06x}) acts on the running mission, not modelled yet (T-056)"
                );
                None
            }
        }
    }

    /// The widget under (`x`, `y`) in the view shown.
    fn target(&self, assets: &Assets, (x, y): (u16, u16)) -> Option<Target> {
        let view = self.view();
        let tables = assets.tables(view);
        let hit = |kind: u16| {
            tables
                .find(kind)
                .and_then(|w| w.area)
                .is_some_and(|a| a.contains(x, y))
        };
        if view == View::SitRoom {
            if hit(SITUATION_CORNER_TYPE) {
                return Some(Target::Corner);
            }
            if let Some(i) = assets
                .views
                .situation_buttons
                .iter()
                .position(|b| b.area.contains(x, y))
            {
                return Some(Target::Button(i));
            }
            return assets
                .views
                .roster_rows
                .iter()
                .position(|r| r.contains(x, y))
                .map(Target::Roster);
        }
        [
            (CONSOLE_CORNER_TYPE, Target::Corner),
            (PROGRAM_PANEL_TYPE, Target::Panel),
            (UNIT_BUTTON_TYPE, Target::UnitButton),
        ]
        .into_iter()
        .find_map(|(kind, target)| hit(kind).then_some(target))
    }

    /// A mouse button went down at `at` (`None` off the picture).
    pub fn mouse_down(&mut self, button: MouseButton, at: Option<(u16, u16)>, assets: &Assets) {
        if self.capture.is_some() || button == MouseButton::Middle {
            return;
        }
        let Some(target) = at.and_then(|at| self.target(assets, at)) else {
            return;
        };
        self.capture = Some((target, button));
        match target {
            Target::Panel => {
                self.panel_pressed = true;
                self.panel_swap = !self.panel_swap;
            }
            Target::UnitButton => self.unit_pressed = !self.unit_pressed,
            Target::Button(_) => self.button_look = !self.button_look,
            // The roster selects on the press (`ui_sitroom_roster_on_mouse`).
            Target::Roster(row) if button == MouseButton::Left => {
                let slot = self.robots().get(self.roster_top + row).copied();
                self.select_unit(slot);
            }
            Target::Roster(_) | Target::Corner => {}
        }
        self.dirty = true;
    }

    /// A mouse button went up with the pointer at `at`.
    pub fn mouse_up(
        &mut self,
        button: MouseButton,
        at: Option<(u16, u16)>,
        assets: &Assets,
    ) -> Option<Request> {
        let (target, by) = self.capture?;
        if by != button {
            return None;
        }
        self.capture = None;
        // The handlers act only on events over the widget that captured the pointer:
        // released elsewhere, nothing happens and a pressed look stays as it is.
        if at.and_then(|at| self.target(assets, at)) != Some(target) {
            return None;
        }
        let right = button == MouseButton::Right;
        let programs = &assets.programs;
        self.dirty = true;
        match target {
            Target::Panel => {
                // The release clears the panel's state (+0x22 = 0) and redraws it.
                self.panel_pressed = false;
                self.panel_swap = false;
                let Viewed::Robot(slot) = self.viewed else {
                    return None;
                };
                let change = if right {
                    self.book.select_previous(slot, programs)
                } else {
                    self.book.select_next(slot, programs)
                };
                self.log_change(change, programs);
                None
            }
            Target::UnitButton => {
                self.unit_pressed = !self.unit_pressed;
                let Viewed::Robot(slot) = self.viewed else {
                    return None;
                };
                let next = self.step_robot(Some(slot), if right { -1 } else { 1 })?;
                self.view_robot(next).map(Request::Show)
            }
            Target::Corner => match self.viewed {
                Viewed::Robot(_) => Some(self.base()),
                Viewed::Base => Some(Request::End),
            },
            Target::Roster(_) if right => self.view_selected(),
            Target::Roster(_) => None,
            Target::Button(i) => {
                self.button_look = !self.button_look;
                self.situation_button(assets.views.situation_buttons[i].kind)
            }
        }
    }

    /// A situation room button was released over itself.
    fn situation_button(&mut self, kind: u16) -> Option<Request> {
        match kind {
            button::SCROLL_UP => {
                self.roster_top = self.roster_top.saturating_sub(1);
                None
            }
            button::SCROLL_DOWN => {
                if self.roster_top + 1 < self.robots().len() {
                    self.roster_top += 1;
                }
                None
            }
            button::VIEW => self.view_selected(),
            button::END => Some(Request::End),
            button::LAUNCH | button::RECALL => {
                log::info!(
                    "situation room: launching or recalling unit {:?} needs the running mission (T-056)",
                    self.selected
                );
                None
            }
            _ => {
                log::debug!("situation room: button {kind:#x} acts on the running mission (T-056)");
                None
            }
        }
    }

    /// Draws the view's own parts over its picture, which `fb` already shows.
    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        let view = self.view();
        let colours: &[u8] = if view == View::SitRoom {
            &VIEW_COLOURS[2..]
        } else {
            &VIEW_COLOURS
        };
        let font = assets.fonts.get(&UNIT_FONT);
        for &colour in colours {
            hatch(fb, colour, font);
        }
        match self.viewed {
            Viewed::Base => self.draw_situation(fb, assets),
            Viewed::Robot(slot) => self.draw_console(fb, assets, slot),
        }
    }

    fn draw_situation(&self, fb: &mut Framebuffer, assets: &Assets) {
        for button in &assets.views.situation_buttons {
            let handle = button.handle + if self.button_look { HANDLE_STEP } else { 0 };
            match assets.sprite(handle) {
                Some(sprite) => {
                    let blit = SpriteBlit {
                        mode: BlitMode::Opaque,
                        ..SpriteBlit::default()
                    };
                    fb.blit_sprite(
                        sprite,
                        button.area.left.into(),
                        button.area.top.into(),
                        blit,
                    );
                }
                None => log::warn!("situation room: no sprite {handle:#x} in msprite1"),
            }
        }
        // The roster's rows: the port's placeholder (unit number and program); the
        // original's rows are drawn by untraced code (T-056).
        let Some(font) = assets.fonts.get(&UNIT_FONT) else {
            return;
        };
        let robots = self.robots();
        for (row, area) in assets.views.roster_rows.iter().enumerate() {
            let Some(&slot) = robots.get(self.roster_top + row) else {
                break;
            };
            let program = self.book.get(slot).map_or(0, |r| r.orders.program);
            let mut text = format!("{slot:>3} ").into_bytes();
            text.extend(assets.programs.name(program));
            let mut cursor = TextCursor::at(
                i32::from(area.left) + 1,
                i32::from(area.top) + 1,
                PLACEHOLDER,
            );
            fb.draw_text(font, &mut cursor, &text, TextMode::Transparent);
            if self.selected == Some(slot) {
                frame(fb, *area, PLACEHOLDER);
            }
        }
    }

    fn draw_console(&self, fb: &mut Framebuffer, assets: &Assets, slot: u16) {
        let Some(robot) = self.book.get(slot) else {
            return;
        };
        let tables = assets.console_tables(robot.kind);
        if let Some(area) = tables.find(PROGRAM_PANEL_TYPE).and_then(|w| w.area) {
            let style = assets.views.program_panel[robot.kind.index()];
            if self.panel_pressed {
                // `gfx_recolour_rect`: one pressed colour becomes the other.
                let [a, b] = style.pressed;
                let (from, to) = if self.panel_swap { (b, a) } else { (a, b) };
                recolour(fb, area, from, to);
            } else {
                fill(fb, area, style.fill);
                let colour = if robot.orders.manual {
                    style.manual_text
                } else {
                    assets.views.automatic_text
                };
                if let Some(font) = assets.fonts.get(&style.font) {
                    let mut cursor = TextCursor::at(
                        i32::from(area.left + style.x_offset),
                        i32::from(area.top) + 1,
                        colour,
                    );
                    let name = assets.programs.name(robot.orders.program);
                    fb.draw_text(font, &mut cursor, name, TextMode::Transparent);
                }
            }
        }
        // The call sign is not traced (T-056): the port prints the unit's number.
        if let Some(font) = assets.fonts.get(&UNIT_FONT) {
            let (x, y) = assets.views.unit_label;
            let mut cursor = TextCursor::at(x.into(), y.into(), UNIT_COLOUR);
            fb.draw_text(
                font,
                &mut cursor,
                format!("#{slot}").as_bytes(),
                TextMode::Transparent,
            );
        }
        if self.unit_pressed
            && let Some(area) = tables.find(UNIT_BUTTON_TYPE).and_then(|w| w.area)
        {
            xor_frame(fb, area, UNIT_PRESSED_XOR);
        }
    }
}

impl Assets {
    fn console_tables(&self, kind: RobotKind) -> &ViewTables {
        self.views.console(kind)
    }
}

fn mode(manual: bool) -> &'static str {
    if manual { "manual" } else { "automatic" }
}

/// The console of a robot kind.
pub fn view_of(kind: RobotKind) -> View {
    match kind {
        RobotKind::Humanoid => View::Humanoid,
        RobotKind::Scorpion => View::Scorpion,
        RobotKind::Scout => View::Scout,
    }
}

fn pixels(area: Rect) -> impl Iterator<Item = (usize, usize)> {
    (area.top..area.bottom)
        .flat_map(move |y| (area.left..area.right).map(move |x| (usize::from(x), usize::from(y))))
}

fn set(fb: &mut Framebuffer, (x, y): (usize, usize), colour: u8) {
    let width = fb.surface().width;
    if let Some(p) = fb.pixels_mut().get_mut(y * width + x) {
        *p = colour;
    }
}

/// Slot `0x51a`: the rectangle in one colour.
fn fill(fb: &mut Framebuffer, area: Rect, colour: u8) {
    for at in pixels(area) {
        set(fb, at, colour);
    }
}

/// Slot `0x522` (`gfx_recolour_rect`): the rectangle's pixels of colour `from` become
/// `to`.
pub fn recolour(fb: &mut Framebuffer, area: Rect, from: u8, to: u8) {
    for (x, y) in pixels(area) {
        if fb.get(x, y) == Some(from) {
            set(fb, (x, y), to);
        }
    }
}

/// The rectangle's one-pixel frame, inside its bottom and right edges.
fn frame_pixels(area: Rect) -> impl Iterator<Item = (usize, usize)> {
    pixels(area).filter(move |&(x, y)| {
        x == usize::from(area.left)
            || y == usize::from(area.top)
            || x + 1 == usize::from(area.right)
            || y + 1 == usize::from(area.bottom)
    })
}

/// Slot `0x536` (`gfx_xor_rect_frame`): the rectangle's frame XORed with `value`.
pub fn xor_frame(fb: &mut Framebuffer, area: Rect, value: u8) {
    for (x, y) in frame_pixels(area) {
        if let Some(p) = fb.get(x, y) {
            set(fb, (x, y), p ^ value);
        }
    }
}

fn frame(fb: &mut Framebuffer, area: Rect, colour: u8) {
    for at in frame_pixels(area) {
        set(fb, at, colour);
    }
}

/// The port's placeholder over the pixels of `colour`, where the mission would draw:
/// a sparse grid of the colour on black, and a label for T-056 at the region's
/// top-left corner.
fn hatch(fb: &mut Framebuffer, colour: u8, font: Option<&Font>) {
    let view = fb.view();
    let mut corner: Option<(usize, usize)> = None;
    for y in 0..view.height {
        for x in 0..view.width {
            if fb.get(x, y) == Some(colour) {
                corner.get_or_insert((x, y));
                let ink = if (x + y) % 4 == 0 {
                    colour
                } else {
                    PLACEHOLDER_INK
                };
                set(fb, (x, y), ink);
            }
        }
    }
    if let (Some((x, y)), Some(font)) = (corner, font) {
        let mut cursor = TextCursor::at(x as i32 + 2, y as i32 + 2, PLACEHOLDER);
        fb.draw_text(font, &mut cursor, b"T-056", TextMode::Opaque);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_mission_usg, synthetic_programs, synthetic_views};
    use xeno_formats::orders::Orders;
    use xeno_render::Size;

    const DS: usize = 0x20000;
    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    /// The synthetic views and programs of `xeno_formats::fixtures`, the synthetic
    /// fonts, 80 raw 2x2 sprites (every pixel its number + 1), and the synthetic
    /// mission as `atrain.usg`, the training menu's mission, and as every campaign
    /// scenario.
    pub fn test_assets() -> Assets {
        use xeno_formats::ban::SpriteHeader;
        use xeno_formats::fixtures::synthetic_font_block;
        let mut image = vec![0u8; DS + 0x800];
        let relocated = synthetic_views(&mut image, DS);
        let mut programs = vec![0u8; DS + 0x900];
        synthetic_programs(&mut programs, DS);
        let fonts = xeno_formats::font::parse(&synthetic_font_block()).unwrap();
        let sprites = (0..80u8)
            .map(|n| Sprite {
                header: SpriteHeader {
                    size: 10,
                    width: 2,
                    height: 2,
                    flags: 0,
                    unknown_05: 8,
                },
                pixels: vec![n + 1; 4],
                spans: Vec::new(),
                trailing_empty_span: false,
            })
            .collect();
        let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        Assets {
            views: views::read_image(&image, DS, &relocated).unwrap(),
            training: views::read_training_image(&image, DS, &relocated).unwrap(),
            programs: orders::read_image(&programs, DS).unwrap(),
            fonts: [0, 2]
                .into_iter()
                .map(|id| (id, fonts[0].clone()))
                .collect(),
            sprites,
            scenarios: std::iter::once("atrain.usg".to_string())
                .chain((0..13).map(scenario_file))
                .map(|name| (name, scenario.clone()))
                .collect(),
        }
    }

    fn mission(assets: &Assets) -> Mission {
        let (file, scenario) = assets.sample(None).unwrap();
        Mission::new(file, scenario)
    }

    fn click(
        m: &mut Mission,
        assets: &Assets,
        at: (u16, u16),
        button: MouseButton,
    ) -> Option<Request> {
        m.mouse_down(button, Some(at), assets);
        m.mouse_up(button, Some(at), assets)
    }

    #[test]
    fn padded_names_become_file_names() {
        assert_eq!(file_name(b"ATRAIN.USG"), "atrain.usg");
        assert_eq!(file_name(b"ABC     .USG"), "abc.usg");
    }

    #[test]
    fn orders_are_set_on_the_console_and_kept_across_views() {
        let assets = test_assets();
        let mut m = mission(&assets);
        assert_eq!(m.view(), View::SitRoom);
        assert_eq!(m.viewed_label(), "the base");
        // Space? The synthetic situation room binds only F2 and ESC: neither does
        // anything there. The roster's first row selects unit 2; a right release opens
        // its console.
        assert_eq!(m.key(0x3c, &assets), None);
        assert_eq!(click(&mut m, &assets, (5, 16), LEFT), None);
        assert_eq!(
            click(&mut m, &assets, (5, 16), RIGHT),
            Some(Request::Show(View::Humanoid))
        );
        assert_eq!(m.viewed(), Viewed::Robot(2));
        assert_eq!(m.viewed_label(), "unit 2 (Humanoid)");
        // A left release on the program panel: the next program, automatic.
        assert_eq!(click(&mut m, &assets, (130, 92), LEFT), None);
        assert_eq!(m.book.get(2).unwrap().orders, Orders::given(2));
        // A right release: back to 1.
        click(&mut m, &assets, (130, 92), RIGHT);
        assert_eq!(m.book.get(2).unwrap().orders.program, 1);
        // F2 binds the automatic handler; it is already automatic.
        assert_eq!(m.key(0x3c, &assets), None);
        // The unit-name button: the next unit, the scorpion in slot 5.
        assert_eq!(
            click(&mut m, &assets, (300, 190), LEFT),
            Some(Request::Show(View::Scorpion))
        );
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book.get(5).unwrap().orders.program, 3);
        // ESC: back to the situation room; the orders are kept.
        assert_eq!(m.key(0x01, &assets), Some(Request::Show(View::SitRoom)));
        assert_eq!(m.book.get(2).unwrap().orders.program, 1);
        assert_eq!(m.book.get(5).unwrap().orders.program, 3);
        assert_eq!(
            m.book.get(7).unwrap().orders.program,
            2,
            "the enemy's unchanged"
        );
        // A release off the panel changes nothing, and leaves it looking pressed.
        m.view_robot(2);
        m.mouse_down(LEFT, Some((130, 92)), &assets);
        m.mouse_up(LEFT, Some((10, 150)), &assets);
        assert_eq!(m.book.get(2).unwrap().orders.program, 1);
        assert!(m.panel_pressed);
        m.view_robot(2);
        // The corner goes back to the situation room, and there ends the mission.
        assert_eq!(
            click(&mut m, &assets, (1, 1), LEFT),
            Some(Request::Show(View::SitRoom))
        );
        assert_eq!(click(&mut m, &assets, (1, 1), LEFT), Some(Request::End));
        // The enemy's robots are never viewed.
        assert_eq!(m.view_robot(7), None);
    }

    #[test]
    fn the_situation_room_buttons_flip_together_and_act_on_release() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.mouse_down(LEFT, Some((15, 75)), &assets);
        m.draw(&mut fb, &assets);
        // Handle 0x2a8 is sprite 1 (pixels 2).
        assert_eq!(fb.get(13, 73), Some(2));
        m.mouse_up(LEFT, Some((15, 75)), &assets);
        fb.clear(0);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(13, 73), Some(1));
    }

    #[test]
    fn the_console_draws_the_program_and_its_pressed_look() {
        let mut assets = test_assets();
        // The synthetic font draws only `A`.
        assets.programs.programs[1].name = b"AA".to_vec();
        let mut m = mission(&assets);
        m.view_robot(2);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.draw(&mut fb, &assets);
        // Filled with the humanoid's fill colour (0x10), the name in its manual colour.
        assert_eq!(fb.get(193, 97), Some(0x10));
        let inked: Vec<_> = (126..194).filter_map(|x| fb.get(x, 92)).collect();
        assert!(inked.contains(&0x20), "{inked:?}");
        // Pressed: the fill (0x10) is recoloured to 0x30.
        m.mouse_down(LEFT, Some((130, 92)), &assets);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(193, 97), Some(0x30));
        // The unit-name button's frame is XORed while pressed.
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.mouse_up(LEFT, Some((0, 199)), &assets);
        m.mouse_down(RIGHT, Some((300, 190)), &assets);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(273, 185), Some(8));
    }

    #[test]
    fn view_colours_get_a_placeholder() {
        let assets = test_assets();
        let m = Mission::empty();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.fill_rect(xeno_render::Rect::new(100, 50, 40, 20), 255);
        m.draw(&mut fb, &assets);
        let left: Vec<_> = (100..140).filter_map(|x| fb.get(x, 69)).collect();
        assert!(left.contains(&0) && left.contains(&255));
        assert!(assets.claims_escape(View::Humanoid));
    }
}
