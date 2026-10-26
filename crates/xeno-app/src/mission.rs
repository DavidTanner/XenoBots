//! A mission's views (T-043): the situation room, the view of the player's base, and
//! the console of each robot, as `ui_open_view_of_object` and their widgets run them
//! (`docs/screens.md`, "The mission's views", "Orders"). The simulation does not run
//! yet (T-050 on), so what it would draw (the 3D view, scanners, instruments, the
//! roster's contents) is a labelled placeholder for T-056; switching views and setting
//! orders work. So does the briefing (T-044): the scenario's `ORDR` text, typed into the
//! situation room's main view in its orders mode (O), as `ui_sitroom_type_orders_text`
//! types it (`docs/screens.md`, "The briefing (T-044)").
//!
//! The mission's state is a [`xeno_sim::Sim`] loaded from its `.usg` file
//! (`docs/sim.md`, "Loading a scenario"): its world holds the objects and the robots'
//! orders, which the views change only through the sim, so they persist across views.
//! The sim has the run's random word while the mission runs, and gives it back to the
//! campaign when the mission stops (`docs/sim.md`, "The run's random word"). The
//! widget sets, key tables, sprite buttons, program names and panel styles all come
//! from the user's `xeno.exe`, `msprite1.b@n` and `mgavideo.dvr`; what a key does is
//! chosen by the original handler it calls.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xeno_formats::ban::{self, Sprite};
use xeno_formats::font::Font;
use xeno_formats::maproom::{HANDLE_STEP, Rect};
use xeno_formats::orders;
use xeno_formats::usg::{self, Scenario, Side};
use xeno_formats::views::{
    self, CONSOLE_CORNER_TYPE, MissionViews, PROGRAM_PANEL_TYPE, SITUATION_CORNER_TYPE,
    TrainingAction, TrainingMenu, UNIT_BUTTON_TYPE, View as ViewTables,
};
use xeno_formats::{editor, lzexe, map, mgavideo, motion};
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};
use xeno_sim::orders::{Change, OrderBook, Programs, RobotKind};
use xeno_sim::{Parked, Random, Sim, World};

use crate::editor::{Editor, Effect};
use crate::game_dir::GameFiles;
use crate::input::{InputEvent, MouseButton};
use crate::maproom::{Campaign, scenario_file};
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
    /// Situation room: Ctrl+Alt+P, save the mission as a `.usg` (no unlock needed).
    pub const SAVE: u16 = 0xefd5;
    /// Situation room: the main view's modes, by key (O, S, M, R, C), and the mode each
    /// sets (`ui_sitroom_set_view_mode`).
    pub const MODES: [(u16, u8); 5] = [
        (0x67d0, 0),
        (0x67ca, 2),
        (0x67d6, 4),
        (0x67dc, 6),
        (0x67e2, 8),
    ];
}

/// The situation room's main view (widget type 0x50) and its orders mode, and how
/// `ui_sitroom_type_orders_text` types the briefing in it: font 4, colour 0x1d (0x17 for
/// codes with bit 7), from (96, 30), after clearing the view to colour 0x10. Short code
/// constants, kept here.
const MAIN_VIEW_TYPE: u16 = 0x50;
pub const ORDERS_MODE: u8 = 0;
const ORDERS_FONT: u16 = 4;
const ORDERS_COLOUR: u8 = 0x1d;
const ORDERS_HIGH_COLOUR: u8 = 0x17;
const ORDERS_CLEAR: u8 = 0x10;
const ORDERS_CURSOR: (i32, i32) = (96, 30);

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
    /// The movement tables the mission's sim moves robots by.
    pub motion: motion::Motion,
    /// By font id.
    pub fonts: BTreeMap<u16, Font>,
    /// `msprite1`, from `views.situation_bank`.
    pub sprites: Vec<Sprite>,
    /// Every training and campaign scenario, by lower-case file name; files that are
    /// missing or do not parse are left out (and logged).
    pub scenarios: BTreeMap<String, Scenario>,
    /// The scenario editor's tables (T-048), if the user's `xeno.exe` has them where
    /// the port reads them.
    pub editor: Option<editor::Editor>,
    /// Whether the user's `XENO.KEY` gives unlock word 9 (the editor's keys).
    pub editor_unlocked: bool,
    /// Where Ctrl+Alt+P and the editor save (`--save-dir`, else the game directory).
    pub save_dir: PathBuf,
}

/// The names the port saves under until it has the original's file dialogs (T-110):
/// the scenario (Ctrl+Alt+P), and the terrain editor's leave file.
pub const SAVED_SCENARIO: &str = "EDITED.USG";
pub const SAVED_TERRAIN: &str = "BACKUP.MAP";

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
    let motion = motion::read(&exe).map_err(|e| no("movement tables", e))?;
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
        .chain([UNIT_FONT, ORDERS_FONT])
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
    let editor = editor::read(&exe)
        .inspect_err(|e| log::warn!("xeno.exe: no scenario editor tables this port can read: {e}"))
        .ok();
    Ok(Assets {
        views,
        training,
        programs,
        motion,
        fonts,
        sprites,
        scenarios,
        editor,
        editor_unlocked: false,
        save_dir: files.dir().path.clone(),
    })
}

/// Logs what a mission loaded: each side's units (its robots) by kind, and the rest.
fn log_census(file: &str, world: &World) {
    let side = |side: Side| {
        let c = world.census(side);
        format!(
            "{} units ({} humanoid, {} scorpion, {} scout), {} base, {} relays, {} obstacles",
            c.units(),
            c.humanoids,
            c.scorpions,
            c.scouts,
            c.bases,
            c.relays,
            c.obstacles
        )
    };
    log::info!(
        "mission: {file}, {} objects; the player's: {}, {} robots with orders; the enemy's: {}",
        world.len(),
        side(Side::Human),
        world.orders.side(Side::Human).count(),
        side(Side::Alien)
    );
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

/// The mission's sim: running, with the run's random word, or parked without it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Run {
    Running(Sim),
    Parked(Parked),
}

impl Default for Run {
    fn default() -> Self {
        Run::Parked(Parked::default())
    }
}

/// A mission in progress: its file, its sim, and the views' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    /// The `.usg` file it was loaded from.
    pub file: String,
    run: Run,
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
    /// The briefing (`ORDR`), the main view's mode (base +0x37) and how many of the
    /// briefing's bytes are typed (+0x34).
    orders: Vec<u8>,
    mode: u8,
    typed: usize,
    capture: Option<(Target, MouseButton)>,
    dirty: bool,
    /// The scenario as loaded, which the editor's keys change and Ctrl+Alt+P saves
    /// (the sim does not run yet, so it is the mission's state; T-057 for saving the
    /// running sim).
    scenario: Option<Scenario>,
    editor: Editor,
}

impl Mission {
    /// A mission on `scenario`, starting in the situation room, its sim drawing from
    /// `random`, the run's random word (from [`Campaign::take_random`]), its robots
    /// moving by `motion`.
    pub fn new(
        file: String,
        scenario: &Scenario,
        random: Random,
        motion: &motion::Motion,
    ) -> Mission {
        let sim = Sim::load(scenario, random, motion.clone());
        log_census(&file, &sim.world);
        let mut mission = Mission::with_run(file, Run::Running(sim));
        mission.orders = scenario.briefing.clone().unwrap_or_default();
        mission.scenario = Some(scenario.clone());
        // The mission starts in the mode saved in the player's base.
        let base = scenario
            .objects
            .iter()
            .flatten()
            .find(|o| o.kind == usg::ObjectKind::Base && o.side() == Side::Human);
        mission.mode = base
            .and_then(|o| o.base())
            .map_or(ORDERS_MODE, |b| b.view_mode);
        log::debug!(
            "mission: the situation room starts in view mode {} ({} briefing bytes)",
            mission.mode,
            mission.orders_end()
        );
        mission
    }

    /// A mission with no file (no robots), parked: only the views' pictures.
    pub fn empty() -> Mission {
        Mission::with_run(String::new(), Run::default())
    }

    fn with_run(file: String, run: Run) -> Mission {
        let mut mission = Mission {
            file,
            run,
            viewed: Viewed::Base,
            selected: None,
            roster_top: 0,
            panel_pressed: false,
            panel_swap: false,
            unit_pressed: false,
            button_look: false,
            orders: Vec::new(),
            mode: ORDERS_MODE,
            typed: 0,
            capture: None,
            dirty: true,
            scenario: None,
            editor: Editor::default(),
        };
        let first = mission.book().side(Side::Human).map(|(s, _)| s).next();
        mission.selected = first;
        mission
    }

    /// The mission stops (it ended, won, lost or with no result): its sim gives the
    /// run's random word back to `campaign`. The objects stay, as the original keeps
    /// them until another scenario loads, so the map room can run the mission again.
    pub fn park(&mut self, campaign: &mut Campaign) {
        if let Run::Running(sim) = std::mem::take(&mut self.run) {
            let (parked, random) = sim.park();
            self.run = Run::Parked(parked);
            campaign.give_random(random);
        }
    }

    /// The mission goes on where it stopped, its sim taking the run's random word from
    /// `campaign`.
    pub fn resume(&mut self, campaign: &mut Campaign) {
        self.run = match std::mem::take(&mut self.run) {
            Run::Parked(parked) => Run::Running(parked.resume(campaign.take_random())),
            running => running,
        };
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        matches!(self.run, Run::Running(_))
    }

    /// The alert's damage to the player's base, at the start of a campaign scenario
    /// (`sim_apply_scenario_alert`); a parked mission changes nothing.
    pub fn apply_alert(&mut self, damage: u16) {
        if let Run::Running(sim) = &mut self.run
            && !sim.world.apply_alert(damage)
        {
            log::warn!("mission: no human base takes the alert's {damage} damage");
        }
    }

    pub fn world(&self) -> &World {
        match &self.run {
            Run::Running(sim) => &sim.world,
            Run::Parked(parked) => &parked.world,
        }
    }

    /// The robots' orders, in the sim's world.
    pub fn book(&self) -> &OrderBook {
        &self.world().orders
    }

    /// Changes the orders through the running sim, and logs the change; a parked
    /// mission changes nothing.
    fn change_orders(
        &mut self,
        programs: &Programs,
        f: impl FnOnce(&mut OrderBook) -> Option<Change>,
    ) {
        let change = match &mut self.run {
            Run::Running(sim) => f(&mut sim.world.orders),
            Run::Parked(_) => None,
        };
        self.log_change(change, programs);
    }

    #[cfg(test)]
    pub fn viewed(&self) -> Viewed {
        self.viewed
    }

    /// Views `robot` (the player's), as with Tab or the roster.
    pub fn view_robot(&mut self, slot: u16) -> Option<View> {
        let robot = *self.book().get(slot)?;
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
            Viewed::Robot(slot) => match self.book().get(slot) {
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
                .book()
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
        // Drawing the situation room's picture starts the orders again (`1000:54fc`).
        self.typed = 0;
        self.reset_looks();
        Request::Show(View::SitRoom)
    }

    /// The player's robots, by slot (the port's order for the side's unit list).
    fn robots(&self) -> Vec<u16> {
        self.book()
            .side(Side::Human)
            .map(|(slot, _)| slot)
            .collect()
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

    /// The briefing's length as typed: up to its first NUL or 0x1a.
    fn orders_end(&self) -> usize {
        let end = self.orders.iter().position(|&b| b == 0 || b == 0x1a);
        end.unwrap_or(self.orders.len())
    }

    /// One retrace in the situation room: in the orders mode, one more byte of the
    /// briefing, until its end (one per pass of the original's mission loop).
    pub fn retrace(&mut self) {
        if self.viewed == Viewed::Base && self.mode == ORDERS_MODE && self.typed < self.orders_end()
        {
            self.typed += 1;
            self.dirty = true;
        }
    }

    /// One retrace of the mission's world with that retrace's input, if it runs (a
    /// parked mission waits). What it did goes to the log.
    pub fn tick<'a>(&mut self, events: impl IntoIterator<Item = &'a InputEvent>) {
        let Run::Running(sim) = &mut self.run else {
            return;
        };
        for event in sim.tick(events) {
            log::debug!("mission: retrace {}: {event:?}", sim.retrace());
        }
    }

    /// The key word `word` went down in the view shown.
    pub fn key(&mut self, word: u16, assets: &Assets) -> Option<Request> {
        // The situation room's editor widget comes before its key table.
        if let (Viewed::Base, true, Some(tables)) =
            (self.viewed, assets.editor_unlocked, &assets.editor)
            && let Some(&(_, handler)) = tables.keys.iter().find(|&&(w, _)| w == word)
        {
            self.editor_key(handler, tables, &assets.save_dir);
            return None;
        }
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
                self.change_orders(programs, |book| book.set_manual(slot, false));
                None
            }
            (Viewed::Robot(slot), handler::MANUAL) => {
                self.change_orders(programs, |book| book.set_manual(slot, true));
                None
            }
            (Viewed::Robot(slot), handler::AUTOMATIC_NEXT) => {
                self.change_orders(programs, |book| book.select_next(slot, programs));
                None
            }
            (Viewed::Robot(slot), handler::NEXT_UNIT | handler::PREVIOUS_UNIT) => {
                let step = if handler == handler::NEXT_UNIT { 1 } else { -1 };
                let next = self.step_robot(Some(slot), step)?;
                self.view_robot(next).map(Request::Show)
            }
            (Viewed::Base, handler::END) => (word == ALT_Q).then_some(Request::End),
            (Viewed::Base, handler::SAVE) => {
                self.save_scenario(&assets.save_dir.join(SAVED_SCENARIO));
                None
            }
            (Viewed::Base, handler::VIEW_SELECTED) => self.view_selected(),
            (Viewed::Base, h) if handler::MODES.iter().any(|&(m, _)| m == h) => {
                let mode = handler::MODES.iter().find(|&&(m, _)| m == h)?.1;
                log::debug!("situation room: main view mode {mode}");
                if mode != ORDERS_MODE {
                    log::debug!("situation room: mode {mode} needs the running mission (T-056)");
                }
                self.mode = mode;
                self.typed = 0;
                self.dirty = true;
                None
            }
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

    /// Runs an editor key on the mission's scenario, logs the editor's status, and
    /// saves the terrain if the key asks.
    fn editor_key(&mut self, handler: u16, tables: &editor::Editor, dir: &Path) {
        let Some(scenario) = &mut self.scenario else {
            log::debug!("editor: no scenario loaded");
            return;
        };
        if self.editor.key(handler, scenario, tables) == Effect::SaveTerrain {
            let world = scenario.terrain.as_ref().map(|t| t.features.as_slice());
            let saved = map::editor_save(world.unwrap_or_default());
            let path = dir.join(SAVED_TERRAIN);
            match std::fs::write(&path, map::write(&saved)) {
                Ok(()) => log::info!(
                    "editor: the terrain editor needs the 3D view (T-109); left at once, \
                     {} circles saved to {}",
                    saved.features.len() - 1,
                    path.display()
                ),
                Err(e) => log::warn!("editor: cannot write {}: {e}", path.display()),
            }
        }
        for row in self.editor.status(scenario, tables) {
            log::info!("editor: {row}");
        }
    }

    /// Ctrl+Alt+P: writes the mission's scenario as the original's writer does.
    fn save_scenario(&self, path: &Path) {
        let Some(scenario) = &self.scenario else {
            log::info!("mission: nothing loaded to save");
            return;
        };
        let written = usg::write(scenario)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
        match written {
            Ok(()) => log::info!("mission: saved the scenario to {}", path.display()),
            Err(e) => log::warn!("mission: cannot save {}: {e}", path.display()),
        }
    }

    /// Whether (`x`, `y`) is over the view's corner, which shows its own pointer shape.
    pub fn over_corner(&self, assets: &Assets, at: (u16, u16)) -> bool {
        self.target(assets, at) == Some(Target::Corner)
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
                self.change_orders(programs, |book| {
                    if right {
                        book.select_previous(slot, programs)
                    } else {
                        book.select_next(slot, programs)
                    }
                });
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
        if self.viewed == Viewed::Base && self.mode == ORDERS_MODE {
            self.draw_orders(fb, assets);
        }
        let font = assets.fonts.get(&UNIT_FONT);
        for &colour in colours {
            hatch(fb, colour, font, "T-056");
        }
        match self.viewed {
            Viewed::Base => self.draw_situation(fb, assets),
            Viewed::Robot(slot) => self.draw_console(fb, assets, slot),
        }
    }

    /// The orders mode: the main view's colour-255 pixels cleared to 0x10 (slot `0x5b4`,
    /// as we read it), then the briefing's bytes typed so far, transparent, CR LF as the
    /// font's control codes, with no wrapping.
    fn draw_orders(&self, fb: &mut Framebuffer, assets: &Assets) {
        let tables = &assets.views.situation;
        if let Some(area) = tables.find(MAIN_VIEW_TYPE).and_then(|w| w.area) {
            recolour(fb, area, 255, ORDERS_CLEAR);
        }
        if let Some(font) = assets.fonts.get(&ORDERS_FONT) {
            let (x, y) = ORDERS_CURSOR;
            let mut cursor = TextCursor::at(x, y, ORDERS_COLOUR);
            cursor.high_colour = ORDERS_HIGH_COLOUR;
            let typed = &self.orders[..self.typed.min(self.orders.len())];
            fb.draw_text(font, &mut cursor, typed, TextMode::Transparent);
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
            let program = self.book().get(slot).map_or(0, |r| r.orders.program);
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
        let Some(robot) = self.book().get(slot) else {
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
pub fn fill(fb: &mut Framebuffer, area: Rect, colour: u8) {
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
/// a sparse grid of the colour on black, and `label` (the task that fills it) at the
/// region's top-left corner.
pub fn hatch(fb: &mut Framebuffer, colour: u8, font: Option<&Font>, label: &str) {
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
        fb.draw_text(font, &mut cursor, label.as_bytes(), TextMode::Opaque);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_mission_usg, synthetic_programs, synthetic_views};
    use xeno_render::Size;
    use xeno_sim::orders::Orders;

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
            fonts: [0, 2, ORDERS_FONT]
                .into_iter()
                .map(|id| (id, fonts[0].clone()))
                .collect(),
            sprites,
            scenarios: std::iter::once("atrain.usg".to_string())
                .chain((0..13).map(scenario_file))
                .map(|name| (name, scenario.clone()))
                .collect(),
            editor: None,
            editor_unlocked: false,
            save_dir: PathBuf::new(),
            motion: motion::Motion {
                sine: vec![0; motion::SINE_ENTRIES],
                turn_step: [0x5b; 3],
                weak_drive: 0x28,
                scorpion_speeds: vec![[0; motion::SPEED_ENTRIES]],
                mission_programs: [[0; 3]; motion::MISSIONS],
                atan: vec![0; motion::ATAN_ENTRIES],
                script_speeds: [[0; 3]; 5],
            },
        }
    }

    fn mission(assets: &Assets) -> Mission {
        let (file, scenario) = assets.sample(None).unwrap();
        Mission::new(file, scenario, Random::seeded(1), &assets.motion)
    }

    /// With unlock word 9 the editor's keys act in the situation room; its terrain
    /// key and Ctrl+Alt+P's save write files the parsers read back.
    #[test]
    fn the_editor_s_keys_change_and_save_the_scenario() {
        let dir = std::env::temp_dir().join(format!("xeno-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut assets = test_assets();
        let tables = |keys| editor::Editor {
            keys,
            kind_names: vec![None; editor::KINDS],
            side_names: [b"H".to_vec(), b"A".to_vec()],
            show_names: [b"N".to_vec(), b"Y".to_vec()],
            mission_names: vec![b"M".to_vec(); 8],
            mission_defaults: vec![[0; 4]; 7],
            mix_percent: vec![0; 11],
            status: b"T:\r".to_vec(),
        };
        // Ctrl+Alt+R removes the relays; Ctrl+M saves the terrain.
        let (remove, terrain) = ((0x3013, 0x6ac8), (0x1032, 0x448c));
        assets.editor = Some(tables(vec![remove, terrain]));
        assets.save_dir = dir.clone();
        let mut m = mission(&assets);
        let relays = |m: &Mission| {
            let objects = m.scenario.as_ref().unwrap().objects.iter().flatten();
            objects.filter(|o| o.kind == usg::ObjectKind::Relay).count()
        };
        assert!(relays(&m) > 0);
        m.key(remove.0, &assets);
        assert!(relays(&m) > 0, "locked: nothing happens");
        assets.editor_unlocked = true;
        m.key(remove.0, &assets);
        assert_eq!(relays(&m), 0);
        m.key(terrain.0, &assets);
        let saved = map::parse(&std::fs::read(dir.join(SAVED_TERRAIN)).unwrap()).unwrap();
        assert_eq!(saved.features.last(), Some(&map::closing_block()));
        m.save_scenario(&dir.join(SAVED_SCENARIO));
        // It reads back as saved (the terrain filled to a paragraph, as the game's).
        let bytes = std::fs::read(dir.join(SAVED_SCENARIO)).unwrap();
        let back = usg::parse(&bytes).unwrap();
        assert_eq!(back.objects, m.scenario.as_ref().unwrap().objects);
        assert_eq!(usg::write(&back).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).unwrap();
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
    fn the_alert_damages_the_player_s_base_while_running() {
        let assets = test_assets();
        let mut campaign = crate::maproom::tests::test_campaign();
        let (file, scenario) = assets.sample(None).unwrap();
        let mut m = Mission::new(file, scenario, campaign.take_random(), &assets.motion);
        // The fixture's human base (slot 0) has taken 20.
        m.apply_alert(30);
        let damage = |m: &Mission| m.world().get(xeno_sim::EntityId(0)).unwrap().damage;
        assert_eq!(damage(&m), xeno_sim::Damage::Taken(50));
        m.park(&mut campaign);
        m.apply_alert(30);
        assert_eq!(damage(&m), xeno_sim::Damage::Taken(50));
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
        assert_eq!(m.book().get(2).unwrap().orders, Orders::given(2));
        // A right release: back to 1.
        click(&mut m, &assets, (130, 92), RIGHT);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        // F2 binds the automatic handler; it is already automatic.
        assert_eq!(m.key(0x3c, &assets), None);
        // The unit-name button: the next unit, the scorpion in slot 5.
        assert_eq!(
            click(&mut m, &assets, (300, 190), LEFT),
            Some(Request::Show(View::Scorpion))
        );
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(5).unwrap().orders.program, 3);
        // ESC: back to the situation room; the orders are kept.
        assert_eq!(m.key(0x01, &assets), Some(Request::Show(View::SitRoom)));
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        assert_eq!(m.book().get(5).unwrap().orders.program, 3);
        assert_eq!(
            m.book().get(7).unwrap().orders.program,
            2,
            "the enemy's unchanged"
        );
        // A release off the panel changes nothing, and leaves it looking pressed.
        m.view_robot(2);
        m.mouse_down(LEFT, Some((130, 92)), &assets);
        m.mouse_up(LEFT, Some((10, 150)), &assets);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
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
    fn a_mission_holds_the_run_s_random_word_only_while_it_runs() {
        let assets = test_assets();
        let mut campaign = crate::maproom::tests::test_campaign();
        let (file, scenario) = assets.sample(None).unwrap();
        let mut m = Mission::new(file, scenario, campaign.take_random(), &assets.motion);
        assert!(m.is_running() && !campaign.has_random());
        // The sim's world is the scenario's.
        assert_eq!(m.world().len(), 5);
        assert_eq!(m.world().census(Side::Human).units(), 2);
        // Parked, the word is back as it was; the orders stay but cannot change.
        m.park(&mut campaign);
        assert!(!m.is_running());
        assert_eq!(campaign.random().clone(), Random::seeded(1));
        m.view_robot(2);
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        // Resumed, it takes the word again, and the orders change through its sim.
        m.resume(&mut campaign);
        assert!(m.is_running() && !campaign.has_random());
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(2).unwrap().orders, Orders::given(2));
        // Parking twice gives back one word.
        m.park(&mut campaign);
        m.park(&mut campaign);
        assert!(campaign.has_random());
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
    fn the_orders_type_one_byte_a_retrace_with_no_wrapping() {
        let mut assets = test_assets();
        let area = Rect {
            top: 26,
            left: 91,
            bottom: 130,
            right: 292,
        };
        let main_view = views::Widget {
            kind: MAIN_VIEW_TYPE,
            state: 0,
            area: Some(area),
        };
        assets.views.situation.widgets.push(main_view);
        // O and M, bound as the original binds them.
        assets
            .views
            .situation
            .keys
            .extend([(0x18, 0x67d0), (0x32, 0x67d6)]);
        let mut m = mission(&assets);
        // A synthetic briefing: `A`, CR LF, `A`, a line too long for the view, then the
        // end-of-file byte and more text that is never typed.
        let mut orders = b"A\r\nA\r\n".to_vec();
        orders.extend([b'A'; 60]);
        orders.extend(b"\x1a\x1aA");
        m.orders = orders;
        m.mode = ORDERS_MODE;
        for _ in 0..100 {
            m.retrace();
        }
        assert_eq!(m.typed, 66, "stops at 0x1a");
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.fill_rect(xeno_render::Rect::new(91, 26, 201, 104), 255);
        m.draw(&mut fb, &assets);
        // The synthetic font: `A` is a bar `XXX.` over `X.X.`, cells 4 wide, lines 3
        // high. CR goes back to x 96, LF down a line; the long line runs on past the
        // view's right edge (no wrapping).
        assert_eq!(fb.get(96, 30), Some(0x1d));
        assert_eq!(fb.get(96, 33), Some(0x1d));
        assert_eq!(fb.get(96 + 4 * 49, 36), Some(0x1d), "past the right edge");
        assert_eq!(fb.get(96, 39), Some(0x10), "cleared, no fourth line");
        // M: another mode, its placeholder, nothing typed; O starts the orders again.
        assert_eq!(m.key(0x32, &assets), None);
        m.retrace();
        assert_eq!((m.mode, m.typed), (4, 0));
        m.key(0x18, &assets);
        m.retrace();
        assert_eq!((m.mode, m.typed), (ORDERS_MODE, 1));
        // Viewing a console stops it; coming back starts it again.
        m.view_robot(2);
        m.retrace();
        assert_eq!(m.typed, 1);
        m.key(0x01, &assets);
        assert_eq!(m.typed, 0);
    }

    /// Types every stock scenario's briefing from the user's files to its end in the
    /// orders mode, and checks it stays inside the main view.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_briefing_types_inside_the_main_view() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let tables = &assets.views.situation;
        let area = tables.find(MAIN_VIEW_TYPE).and_then(|w| w.area).unwrap();
        assert_eq!(assets.scenarios.len(), 17);
        for (name, scenario) in &assets.scenarios {
            let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
            m.mode = ORDERS_MODE;
            while m.typed < m.orders_end() {
                m.retrace();
            }
            assert!(m.typed > 0, "{name}");
            let mut fb = Framebuffer::new(Size::MODE_13H);
            let (w, h) = (area.right - area.left, area.bottom - area.top);
            let rect =
                xeno_render::Rect::new(area.left.into(), area.top.into(), w.into(), h.into());
            fb.fill_rect(rect, 255);
            m.draw(&mut fb, &assets);
            let mut inked = 0;
            for (x, y) in pixels(Rect {
                top: 0,
                left: 0,
                bottom: 200,
                right: 320,
            }) {
                if fb.get(x, y).is_some_and(|p| (0x1d..=0x1f).contains(&p)) {
                    assert!(area.contains(x as u16, y as u16), "{name}: ({x}, {y})");
                    inked += 1;
                }
            }
            assert!(inked > 100, "{name}: {inked} pixels inked");
        }
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
