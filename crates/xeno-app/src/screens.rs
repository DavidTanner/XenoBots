//! The front end's screens as a state machine (T-040), shaped like `xeno.exe`'s own
//! flow (`docs/screens.md`): log-in, then the map room as the hub, from which the
//! player plays a scenario, opens the training menu or the review, logs in again or
//! quits. A mission shows the situation room or a robot console, and a won or lost
//! one ends in a cutscene. The log-in is the original's (T-041, `login`): the name
//! entry with its blinking cursor. So is the map room (T-042, `maproom`): the
//! scenario boxes, selection by click and keys, the panels and the buttons. So are the
//! training menu and the mission's views (T-043, `mission`): the situation room and the
//! robot consoles, switched as the original switches them, with the robots' orders set
//! on the consoles, and the briefing typed in the situation room's orders view; the
//! simulation's parts are placeholders. So is the review (T-044, `review`): the robot
//! reference's pages, texts, keys and buttons. A won or lost mission ends in the
//! original's cutscene (T-045, played by the window in the 320x240 mode with
//! `intro::Cutscene`): the screens fade out, hand it over with [`Screens::take_cutscene`]
//! and go back to the map room when told it is over. The pointer's shape and
//! visibility follow the host's calls (T-049, `pointer::State`): hidden until the map
//! room first opens and during the cutscenes, busy for one retrace before each
//! picture loads, and its corner shape over the mission views' corners.
//!
//! Keys go through (key word, action) tables, as the original's screens do
//! (`docs/formats/key.md`, "The game's keys"); a screen claims ESC when its table
//! binds it. Everything here is plain data stepped once per retrace with that
//! retrace's input, so it is tested without a window and replays from an `InputLog`.

use xeno_formats::mal;
use xeno_formats::mgavideo::Pointer;
use xeno_render::{ColourAnims, DAC_BYTES, Dac, Fade, Framebuffer, Rect};
use xeno_sim::Random;

use crate::campaign::TRAINING;
use crate::game_dir::GameFiles;
use crate::input::{InputEvent, Keys, Mouse, ScanCode};
use crate::intro::{Cutscene, Endings};
use crate::login::{self, NameEntry, Outcome, Player};
use crate::maproom::{self, Campaign, MapState, Request};
use crate::mission::{self, Mission as MissionState, Request as MissionRequest};
use crate::pointer::{self, ARROW, BUSY};
use crate::review::{self, ReviewState};
use crate::save;
use crate::test_pattern;
use crate::viewer::to_vga6;
use xeno_formats::ending::Numbers;
use xeno_formats::usg::Side;
use xeno_formats::views::TrainingAction;
use xeno_sim::orders::RobotKind;

/// A robot console or the situation room: what a mission shows depends on the object
/// viewed (`sim_run_mission_loop`, table at `DS:0x6411`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SitRoom,
    Humanoid,
    Scorpion,
    Scout,
}

/// The cutscene after a mission (`docs/formats/vcl.md`, "Sequences").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Won,
    Lost,
    Triumph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    MapRoom,
    Training,
    Review,
    Mission(View),
    Cutscene(Ending),
}

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Go(Screen),
    /// Start a mission; `true` from the training menu (no cutscene after it). From
    /// the map room, the selected scenario's, and nothing without a selection.
    Play(bool),
    /// The map room's next and previous scenario.
    Next,
    Previous,
    End(Ending),
    /// The port's stand-in for shots (T-114): the side's base is destroyed, and the
    /// mission ends won or lost after its end script.
    DestroyBase(Side),
    Quit,
    /// The map room's Ctrl+Space: run the mission with no check and no load.
    PlayLoaded,
    /// The developer keys (unlock word 8): progress up and down, the selected
    /// scenario won, the loaded one lost; and Alt+F1 (unlock word 9), an unused flag.
    ProgressUp,
    ProgressDown,
    DevWin,
    DevLose,
    Editor,
}

/// A key word as the original's keyboard handler posts it: modifiers in the high byte
/// (0x08 Shift, 0x10 Ctrl, 0x20 Alt), the set 1 code in the low byte, `E0` dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyWord(pub u16);

const SHIFT: u16 = 0x0800;
const CTRL: u16 = 0x1000;
const ALT: u16 = 0x2000;

const fn key(code: u16) -> KeyWord {
    KeyWord(code)
}

const ESC: KeyWord = key(0x01);
const ENTER: KeyWord = key(0x1c);
const SPACE: KeyWord = key(0x39);
const ALT_Q: KeyWord = KeyWord(ALT | 0x10);

use Action::*;
use Screen::*;

/// The log-in's keys go to the name entry (`login::NameEntry`); ESC is listed so the
/// screen claims it.
const LOGIN_KEYS: &[(KeyWord, Action)] = &[(ESC, Quit)];
/// The map room's own keys (`DS:0x60bf`): P, Enter or Space play, T training, R
/// review, N log in again, Q or Alt+Q quit; Right, Down and Tab the next scenario,
/// Alt+Tab, Left and Up the previous; `=`, `-`, Ctrl+W, Ctrl+L the developer keys,
/// Ctrl+Space run the loaded mission, Alt+F1 the editor's flag. Not ESC.
const MAP_ROOM_KEYS: &[(KeyWord, Action)] = &[
    (key(0x13), Go(Review)),
    (ENTER, Play(false)),
    (SPACE, Play(false)),
    (key(0x19), Play(false)),
    (key(0x31), Go(Login)),
    (key(0x14), Go(Training)),
    (ALT_Q, Quit),
    (key(0x10), Quit),
    (key(0x4d), Next),
    (key(0x50), Next),
    (key(0x0f), Next),
    (KeyWord(ALT | 0x0f), Previous),
    (key(0x4b), Previous),
    (key(0x48), Previous),
    (key(0x0d), ProgressUp),
    (key(0x0c), ProgressDown),
    (KeyWord(CTRL | 0x11), DevWin),
    (KeyWord(CTRL | 0x26), DevLose),
    (KeyWord(CTRL | 0x39), PlayLoaded),
    (KeyWord(ALT | 0x3b), Editor),
];
/// The training menu's keys are read from the user's `xeno.exe` (`DS:0x5e9f`, in
/// `mission::Assets::training`).
const TRAINING_KEYS: &[(KeyWord, Action)] = &[];
/// The review's keys are read from the user's `xeno.exe` (`DS:0x6244`, in
/// `maproom::Assets::review`).
const REVIEW_KEYS: &[(KeyWord, Action)] = &[];
/// The port's own keys (no view's table binds them): Ctrl+W and Ctrl+L destroy the
/// alien or the player's base, as shots will (T-114), so the mission ends won or lost
/// after its end script; Ctrl+C ends it with the campaign won. The views' own keys are
/// read from the user's `xeno.exe` (`mission`).
const MISSION_KEYS: &[(KeyWord, Action)] = &[
    (KeyWord(CTRL | 0x11), DestroyBase(Side::Alien)),
    (KeyWord(CTRL | 0x26), DestroyBase(Side::Human)),
    (KeyWord(CTRL | 0x2e), End(Ending::Triumph)),
];
/// A cutscene's keys go to the player (`intro::Cutscene`): any key but P skips it.
const CUTSCENE_KEYS: &[(KeyWord, Action)] = &[];

impl Screen {
    /// The `.m@l` file shown, if any.
    pub fn file(self) -> Option<&'static str> {
        Some(match self {
            Login => "login.m@l",
            MapRoom => "maproom.m@l",
            Training => "trainmen.m@l",
            Review => "review.m@l",
            Mission(View::SitRoom) => "sitroom.m@l",
            Mission(View::Humanoid) => "humancon.m@l",
            Mission(View::Scorpion) => "scorpcon.m@l",
            Mission(View::Scout) => "scoutcon.m@l",
            Cutscene(_) => return None,
        })
    }

    fn keys(self) -> &'static [(KeyWord, Action)] {
        match self {
            Login => LOGIN_KEYS,
            MapRoom => MAP_ROOM_KEYS,
            Training => TRAINING_KEYS,
            Review => REVIEW_KEYS,
            Mission(_) => MISSION_KEYS,
            Cutscene(_) => CUTSCENE_KEYS,
        }
    }

    /// The name and keys, for the window title while there is no font.
    pub fn label(self) -> &'static str {
        match self {
            Login => "Log-in: type a name, Enter continue, ESC quit",
            MapRoom => {
                "Map room: click/Tab/arrows select, P/Enter play, T training, R review, N log-in, Q quit"
            }
            Training => "Training menu: click or H/N/S/C train, ESC back",
            Review => "Review: Tab/Alt+Tab or the next button turn the page, ESC/Alt+Q back",
            Mission(View::SitRoom) => {
                "Mission, situation room: click/Tab select, Space view unit, O orders, Alt+Q end; Ctrl+W/L destroy alien/own base, Ctrl+C campaign"
            }
            Mission(View::Humanoid) => {
                "Mission, humanoid console: click program (L next, R previous), F4 next, F2 pilot, F3 program, Tab unit, ESC back"
            }
            Mission(View::Scorpion) => {
                "Mission, scorpion console: click program (L next, R previous), F4 next, F2 pilot, F3 program, Tab unit, ESC back"
            }
            Mission(View::Scout) => {
                "Mission, scout console: click program (L next, R previous), F4 next, F2 pilot, F3 program, Tab unit, ESC back"
            }
            Cutscene(Ending::Won) => "Mission won: any key but P skips, P pauses",
            Cutscene(Ending::Lost) => "Mission lost: any key but P skips, P pauses",
            Cutscene(Ending::Triumph) => "Campaign won: any key but P skips, P pauses",
        }
    }

    /// Whether the screen uses ESC itself; otherwise ESC belongs to the window.
    pub fn claims_escape(self) -> bool {
        self.keys().iter().any(|&(word, _)| word == ESC)
    }

    fn lookup(self, word: KeyWord) -> Option<Action> {
        self.keys()
            .iter()
            .find_map(|&(w, action)| (w == word).then_some(action))
    }
}

/// The key word for a key going down with `held` keys held (before it).
pub fn key_word(code: ScanCode, held: &Keys) -> KeyWord {
    let down = |codes: [u16; 2]| codes.iter().any(|&c| held.is_down(ScanCode(c)));
    let mut word = code.0 & 0xff;
    for (codes, bit) in [
        ([0x2a, 0x36], SHIFT),
        ([0x1d, 0xe01d], CTRL),
        ([0x38, 0xe038], ALT),
    ] {
        if down(codes) {
            word |= bit;
        }
    }
    KeyWord(word)
}

/// A screen's picture: its palette in DAC levels and 320x200 pixels.
#[derive(Debug, Clone)]
pub struct Picture {
    pub dac: [u8; DAC_BYTES],
    pub pixels: Vec<u8>,
}

/// The eight `.m@l` screens, by file name.
pub type Pictures = Vec<(&'static str, Picture)>;

pub const FILES: [&str; 8] = [
    "login.m@l",
    "maproom.m@l",
    "trainmen.m@l",
    "review.m@l",
    "sitroom.m@l",
    "humancon.m@l",
    "scorpcon.m@l",
    "scoutcon.m@l",
];

/// Reads and decodes the screens.
pub fn load(files: &GameFiles) -> Result<Pictures, String> {
    FILES
        .iter()
        .map(|&name| {
            let bytes = files
                .read(name)
                .map_err(|e| format!("cannot read {name}: {e}"))?;
            let screen = mal::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
            let picture = Picture {
                dac: to_vga6(&screen.palette),
                pixels: screen.pixels,
            };
            Ok((name, picture))
        })
        .collect()
}

/// Retraces of a screen's fade out and in (`AX = 0x20` to slots `0x4c0` and `0x498`);
/// the log-in screen fades in over 64 (`ui_show_screen_fading_in`).
const FADE: u16 = 32;
const LOGIN_FADE: u16 = 64;

/// A picture the original loads with the busy pointer shown: the screen waits one
/// retrace with that shape, then draws it (T-049).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Load {
    Enter(Screen),
    View(View),
}

/// The front end's state.
#[derive(Debug, Clone)]
pub struct Screens {
    pictures: Pictures,
    screen: Screen,
    /// Fading out to this screen.
    next: Option<Screen>,
    /// The mission came from the training menu.
    training: bool,
    dac: Dac,
    /// A fade in waiting for its black to be uploaded.
    fade_in: Option<Fade>,
    keys: Keys,
    quit: bool,
    /// The screen changed since [`Screens::take_changed`].
    changed: bool,
    login: login::Assets,
    entry: NameEntry,
    /// The log-in screen is fading in, with the colour records held
    /// (`ui_show_screen_fading_in`); the cursor starts blinking after.
    login_fading: bool,
    /// Who logged in last.
    player: Option<Player>,
    /// The campaign shown when the map room comes before any log-in (`--screen`).
    guest: Campaign,
    map_assets: maproom::Assets,
    map: MapState,
    mouse: Mouse,
    /// The scenario the last mission was started for, by its record (none for
    /// training).
    scenario: Option<usize>,
    mission_assets: mission::Assets,
    /// The mission shown, or the last one: its file, the robots' orders, the views.
    mission: MissionState,
    /// The training button a press captured the pointer for, and with which button.
    training_press: Option<(usize, crate::input::MouseButton)>,
    review: ReviewState,
    /// The cutscenes after a mission, once loaded ([`Screens::set_endings`]).
    endings: Option<Box<Endings>>,
    /// The cutscene to hand to the window.
    ending: Option<Ending>,
    /// The lose selector `DS:0x6012` / 2: the robot console opened last in the mission
    /// (0 humanoid, 1 scorpion, 2 scout), reset when a mission starts.
    console: usize,
    /// The last won mission's kills, losses and points, for the `WIN` caption.
    numbers: Numbers,
    /// The pointer's shape and hide counter.
    pointer: pointer::State,
    /// The map room's loop has shown the pointer (`1000:6f37`), once after the first
    /// log-in.
    map_room_shown: bool,
    /// A picture to load at the next retrace, with the busy pointer shown until then.
    loading: Option<Load>,
}

/// The logged-in player's campaign, or the guest's.
fn campaign<'a>(player: &'a Option<Player>, guest: &'a Campaign) -> &'a Campaign {
    player.as_ref().map_or(guest, |p| &p.campaign)
}

fn campaign_mut<'a>(player: &'a mut Option<Player>, guest: &'a mut Campaign) -> &'a mut Campaign {
    player.as_mut().map_or(guest, |p| &mut p.campaign)
}

impl Screens {
    /// Starts at `start`, which is drawn and faded in at the first retrace.
    pub fn new(
        pictures: Pictures,
        login: login::Assets,
        map_assets: maproom::Assets,
        mission_assets: mission::Assets,
        start: Screen,
    ) -> Screens {
        let mut dac = Dac::default();
        dac.set_anims(ColourAnims::new(login.anims.clone()));
        let mut guest = Campaign::new(login.campaign.clone(), Random::seeded(login.seed));
        let (mission, start) = match start {
            Mission(view) => sample_mission(&mission_assets, view, &mut guest),
            other => (MissionState::empty(), other),
        };
        // The start-up hides the pointer before the intro; the map room's loop shows
        // it after the first log-in. Every other screen comes after that.
        let map_room_shown = start != Login;
        let pointer = pointer::State::new(login.pointer, map_room_shown);
        Screens {
            pictures,
            screen: start,
            next: Some(start),
            training: false,
            dac,
            fade_in: None,
            keys: Keys::default(),
            quit: false,
            changed: true,
            login,
            entry: NameEntry::default(),
            login_fading: false,
            player: None,
            guest,
            map_assets,
            map: MapState::default(),
            mouse: Mouse::default(),
            scenario: None,
            mission_assets,
            mission,
            training_press: None,
            review: ReviewState::default(),
            endings: None,
            ending: None,
            console: 0,
            numbers: Numbers::default(),
            pointer,
            map_room_shown,
            loading: None,
        }
    }

    /// The cutscenes after a mission; without them the screens go straight back to
    /// the map room.
    pub fn set_endings(&mut self, endings: Endings) {
        self.endings = Some(Box::new(endings));
    }

    /// The mission's sound events heard since the last call (`Mission::take_sounds`).
    pub fn take_sounds(&mut self) -> Vec<crate::mission::Sound> {
        self.mission.take_sounds()
    }

    /// The cutscene to play now, if a mission has just ended in one: the window plays
    /// it in the 320x240 mode, then calls [`Screens::cutscene_over`]. As
    /// `1000:6f9e`: the campaign won, else lost (the picture by the console opened
    /// last), else won with the mission's numbers.
    pub fn take_cutscene(&mut self) -> Option<Cutscene> {
        let ending = self.ending.take()?;
        let Some(endings) = &self.endings else {
            log::warn!("{ending:?}: no cutscenes loaded; back to the map room");
            self.cutscene_over();
            return None;
        };
        log::info!("cutscene: {ending:?}");
        Some(match ending {
            Ending::Triumph => endings.triumph(),
            Ending::Lost => endings.lost(self.console),
            Ending::Won => endings.won(self.numbers),
        })
    }

    /// The cutscene has ended (in mode 13h again): the map room is drawn and fades in
    /// from black.
    pub fn cutscene_over(&mut self) {
        log::info!("cutscene over: back to the map room");
        // The cutscene routines show the pointer again at their end (`1000:702d` on).
        self.pointer.show();
        self.fade_in = None;
        self.dac.stop();
        self.dac.blank();
        self.next = Some(MapRoom);
    }

    /// Notes the robot console a mission view opens, for the lose picture.
    fn viewed(&mut self, view: View) {
        self.mission.restart_tint();
        if let Some(console) = match view {
            View::SitRoom => None,
            View::Humanoid => Some(0),
            View::Scorpion => Some(1),
            View::Scout => Some(2),
        } {
            self.console = console;
        }
    }

    /// The mission shown, or the last one.
    #[cfg(test)]
    pub fn mission(&self) -> &MissionState {
        &self.mission
    }

    /// The logged-in player's campaign, or the guest's.
    #[cfg(test)]
    pub fn campaign(&self) -> &Campaign {
        campaign(&self.player, &self.guest)
    }

    /// The mouse pointer's shape, if the screens show it.
    pub fn pointer(&self) -> Option<&Pointer> {
        let shape = usize::from(self.pointer.shape());
        self.pointer
            .shown()
            .then(|| self.login.pointers.get(shape))
            .flatten()
    }

    /// Picks pointer shape `shape` (slot `0x1496`).
    fn set_shape(&mut self, shape: u8) {
        self.pointer.set_shape(shape, self.login.pointers.len());
    }

    /// The pointer moved to `at` in a mission view: the shape the view's handlers
    /// set there ([`MissionState::pointer_shape`], by the picture's pixel under it).
    fn mission_hover(&mut self, at: (u16, u16)) {
        self.mission.hover(at, &self.mission_assets);
        let colour = self
            .screen
            .file()
            .and_then(|f| self.pictures.iter().find(|(n, _)| *n == f))
            .and_then(|(_, p)| p.pixels.get(usize::from(at.1) * 320 + usize::from(at.0)))
            .copied()
            .unwrap_or(0);
        // shortcut: the handlers test bit 0x2000 of the last key word, taken as Alt held
        // (`DS:0x1bf1`'s writer is not traced); upgrade when T-133 traces it.
        let alt = key_word(ScanCode(0), &self.keys).0 & ALT != 0;
        let shape = self
            .mission
            .pointer_shape(&self.mission_assets, at, colour, alt);
        self.set_shape(shape);
    }

    /// The scenario the last mission was started for (T-043, T-044 and T-051 load its
    /// `scenNNa.usg`).
    #[cfg(test)]
    pub fn scenario(&self) -> Option<usize> {
        self.scenario
    }

    /// What the original prints when the game exits after a log-in.
    pub fn exit_message(&self) -> Option<String> {
        self.player.as_ref().and_then(|p| p.farewell.clone())
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn palette(&self) -> xeno_render::Palette {
        self.dac.palette()
    }

    /// Whether the screen shown uses ESC: its key table binds it.
    pub fn claims_escape(&self) -> bool {
        match self.screen {
            // An open file dialog takes ESC as cancel.
            Mission(view) => self.mission.dialog_open() || self.mission_assets.claims_escape(view),
            Training => self
                .mission_assets
                .training
                .keys
                .iter()
                .any(|&(word, _)| KeyWord(word) == ESC),
            Review => self
                .map_assets
                .review
                .keys
                .iter()
                .any(|&(word, _)| KeyWord(word) == ESC),
            screen => screen.claims_escape(),
        }
    }

    /// Whether a key asked to leave the game.
    pub fn quit(&self) -> bool {
        self.quit
    }

    /// Whether the screen changed since the last call (for the window title).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// One retrace with its input. Keys act only on a screen that is shown, not
    /// fading out; a fade in runs on while its screen takes keys.
    pub fn retrace<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a InputEvent>,
        fb: &mut Framebuffer,
    ) {
        let events: Vec<&InputEvent> = events.into_iter().collect();
        // A picture whose load showed the busy pointer last retrace is drawn now, and
        // the arrow comes back, as each loader ends (T-049). A view opens with the
        // pointer outside its corner.
        if let Some(load) = self.loading.take() {
            match load {
                Load::Enter(screen) => self.enter(screen, fb),
                Load::View(view) => self.show_view(view, fb),
            }
            self.set_shape(ARROW);
        }
        self.dac.retrace();
        if !self.dac.dirty()
            && let Some(fade) = self.fade_in.take()
        {
            self.dac.start(fade);
        }
        if self.login_fading && self.fade_in.is_none() && !self.dac.fading() {
            // The fade is over: the records run again, and the name entry starts the
            // cursor's blink.
            self.login_fading = false;
            self.dac.release_anims();
            self.dac.start_anim(login::CURSOR_RECORD, 0);
        }
        for &event in &events {
            if self.next.is_none() && self.loading.is_none() && !self.quit {
                match (*event, self.screen) {
                    (InputEvent::Text(c), Login) => self.entry.typed(c),
                    (InputEvent::Text(c), Mission(_)) => self.mission.typed(c),
                    (InputEvent::KeyDown { code, .. }, Login) => {
                        if let Some(outcome) = self.entry.key(key_word(code, &self.keys)) {
                            self.logged_in(outcome);
                        }
                    }
                    (InputEvent::KeyDown { code, .. }, Training) => {
                        let word = key_word(code, &self.keys);
                        let action = self.mission_assets.training.keys.iter();
                        let action = action
                            .clone()
                            .find_map(|&(w, a)| (KeyWord(w) == word).then_some(a));
                        if let Some(action) = action {
                            self.training_action(action);
                        }
                    }
                    (InputEvent::KeyDown { code, .. }, Review) => {
                        let word = key_word(code, &self.keys).0;
                        let request = self.review.key(word, &self.map_assets.review);
                        self.review_request(request);
                    }
                    (InputEvent::MouseDown { button }, Review) => {
                        let at = self.mouse.position;
                        self.review.mouse_down(button, at, &self.map_assets.review);
                    }
                    (InputEvent::MouseUp { button }, Review) => {
                        let at = self.mouse.position;
                        let request = self.review.mouse_up(button, at, &self.map_assets.review);
                        self.review_request(request);
                    }
                    (InputEvent::KeyDown { code, .. }, Mission(_)) => {
                        let word = key_word(code, &self.keys);
                        let dialog = self.mission.dialog_open();
                        if let (Some(action), false) = (self.screen.lookup(word), dialog) {
                            self.act(action);
                        } else if let Some(request) = self.mission.key(word.0, &self.mission_assets)
                        {
                            self.mission_request(request);
                        }
                        // A file dialog shows shape 0 while it runs, then the old comes
                        // back.
                        if dialog != self.mission.dialog_open()
                            && let Some(at) = self.mouse.position
                        {
                            self.mission_hover(at);
                        }
                    }
                    (InputEvent::KeyDown { code, .. }, screen) => {
                        if let Some(action) = screen.lookup(key_word(code, &self.keys)) {
                            self.act(action);
                        }
                    }
                    (InputEvent::MouseDown { button }, Training) => {
                        self.training_mouse(button, true, fb);
                    }
                    (InputEvent::MouseUp { button }, Training) => {
                        self.training_mouse(button, false, fb);
                    }
                    (InputEvent::MouseDown { button }, Mission(_))
                        if self.mission.dialog_mouse(
                            button,
                            true,
                            self.mouse.position,
                            &self.mission_assets,
                        ) => {}
                    (InputEvent::MouseUp { button }, Mission(_))
                        if self.mission.dialog_mouse(
                            button,
                            false,
                            self.mouse.position,
                            &self.mission_assets,
                        ) =>
                    {
                        if let (false, Some(at)) = (self.mission.dialog_open(), self.mouse.position)
                        {
                            self.mission_hover(at);
                        }
                    }
                    (InputEvent::MouseDown { button }, Mission(_)) => {
                        let at = self.mouse.position;
                        // The event's key word carries Ctrl (`1000:5781` tests 0x1000).
                        let held = key_word(ScanCode(0), &self.keys).0;
                        let ctrl = held & CTRL != 0;
                        let assets = &self.mission_assets;
                        let placed = self.mission.terrain_mouse(button, true, at, held, assets)
                            || button == xeno_sim::input::MouseButton::Left
                                && ctrl
                                && self.mission.place_at_pointer(at, &self.mission_assets);
                        if !placed {
                            self.mission.mouse_down(button, at, &self.mission_assets);
                        }
                    }
                    (InputEvent::MouseUp { button }, Mission(_)) => {
                        let at = self.mouse.position;
                        let assets = &self.mission_assets;
                        if !self.mission.terrain_mouse(button, false, at, 0, assets)
                            && let Some(request) =
                                self.mission.mouse_up(button, at, &self.mission_assets)
                        {
                            self.mission_request(request);
                        }
                    }
                    (InputEvent::MouseMove { x, y }, Mission(_)) => self.mission_hover((x, y)),
                    (InputEvent::MouseMove { x, y }, MapRoom) => {
                        let campaign = campaign(&self.player, &self.guest);
                        self.map.mouse_move((x, y), &self.map_assets, campaign);
                    }
                    (InputEvent::MouseDown { button }, MapRoom) => {
                        self.map
                            .mouse_down(button, self.mouse.position, &self.map_assets);
                    }
                    (InputEvent::MouseUp { button }, MapRoom) => {
                        let campaign = campaign(&self.player, &self.guest);
                        let at = self.mouse.position;
                        match self.map.mouse_up(button, at, &self.map_assets, campaign) {
                            Some(Request::Key(word)) => {
                                if let Some(action) = MapRoom.lookup(KeyWord(word)) {
                                    self.act(action);
                                }
                            }
                            Some(Request::Play) => self.act(Play(false)),
                            None => {}
                        }
                    }
                    _ => {}
                }
            }
            self.keys.apply(event);
            self.mouse.apply(event);
        }
        if self.screen == MapRoom && self.next.is_none() && self.map.take_dirty() {
            self.draw_map(fb);
        }
        if self.screen == Login && self.next.is_none() {
            self.entry.draw(&self.login.font, fb);
        }
        if self.screen == Mission(View::SitRoom) && self.next.is_none() {
            self.mission.retrace();
        }
        // The mission's world runs once a retrace on every mission view, with that
        // retrace's input (`docs/sim.md`, "Movement").
        if matches!(self.screen, Mission(_))
            && self.next.is_none()
            && let Some(won) = self.mission.tick(events.iter().copied())
        {
            self.act(End(if won { Ending::Won } else { Ending::Lost }));
        }
        if matches!(self.screen, Mission(_)) && self.next.is_none() {
            self.mission.check_noise(&self.mission_assets);
        }
        // The time-of-day tint, once no fade is pending or running (the records blend
        // from the shadow, and a fade's uploads would wipe them).
        if matches!(self.screen, Mission(_))
            && self.next.is_none()
            && self.fade_in.is_none()
            && !self.dac.fading()
            && let Some(weight) = self.mission.tint()
        {
            log::debug!("mission: time-of-day tint {weight}");
            for record in mission::TINT_RECORDS {
                self.dac.start_anim(record, weight);
            }
        }
        if matches!(self.screen, Mission(_))
            && self.next.is_none()
            && self.loading.is_none()
            && self.mission.take_dirty()
        {
            self.draw_mission(fb);
        }
        if self.screen == Review && self.next.is_none() && self.loading.is_none() {
            self.review.retrace(&self.map_assets.review);
        }
        if self.screen == Review && self.next.is_none() && self.review.take_dirty() {
            self.draw_review(fb);
        }
        if let Some(next) = self.next
            && !self.dac.fading()
            && !self.dac.dirty()
        {
            if matches!(next, MapRoom | Training | Review | Mission(_)) {
                // These screens' pictures load with the busy pointer shown. The map
                // room's loop shows the pointer first, once, after the first log-in.
                if next == MapRoom && !self.map_room_shown {
                    self.map_room_shown = true;
                    self.pointer.show();
                }
                self.next = None;
                self.loading = Some(Load::Enter(next));
                self.set_shape(BUSY);
            } else {
                self.enter(next, fb);
            }
        }
    }

    /// A training menu action (`ui_run_training_mission`): load the mission's file and
    /// run it, or go back. A file that could not be loaded does nothing, as in the
    /// original.
    fn training_action(&mut self, number: u16) {
        match self.mission_assets.training.action(number) {
            Some(TrainingAction::Mission(name)) => {
                let file = mission::file_name(name);
                match self.mission_assets.scenarios.get(&file) {
                    Some(scenario) => {
                        log::info!("training menu: {file} loaded");
                        let campaign = campaign_mut(&mut self.player, &mut self.guest);
                        let motion = &self.mission_assets.motion;
                        self.mission =
                            MissionState::new(file, scenario, campaign.take_random(), motion);
                        self.act(Play(true));
                    }
                    None => log::warn!("training menu: {file} could not be loaded"),
                }
            }
            Some(TrainingAction::Back) => self.act(Go(MapRoom)),
            None => log::debug!("training menu: no action {number}"),
        }
    }

    /// A mouse button went down (`down`) or up on the training menu
    /// (`ui_training_button_on_mouse`): a press on a button captures the pointer and
    /// XORs the button's frame; a release over it XORs it back and acts. Released
    /// elsewhere, the frame stays XORed, as in the original.
    fn training_mouse(
        &mut self,
        button: crate::input::MouseButton,
        down: bool,
        fb: &mut Framebuffer,
    ) {
        let buttons = &self.mission_assets.training.buttons;
        let over = self
            .mouse
            .position
            .and_then(|(x, y)| buttons.iter().position(|(area, _)| area.contains(x, y)));
        if down {
            if self.training_press.is_none()
                && button != crate::input::MouseButton::Middle
                && let Some(i) = over
            {
                self.training_press = Some((i, button));
                mission::xor_frame(fb, buttons[i].0, mission::TRAINING_PRESSED_XOR);
            }
            return;
        }
        let Some((i, by)) = self.training_press else {
            return;
        };
        if by != button {
            return;
        }
        self.training_press = None;
        if over == Some(i) {
            let (area, action) = buttons[i];
            mission::xor_frame(fb, area, mission::TRAINING_PRESSED_XOR);
            self.training_action(action);
        }
    }

    /// What the review asked for: leaving goes back to the map room.
    fn review_request(&mut self, request: Option<review::Request>) {
        if request == Some(review::Request::Leave) {
            self.act(Go(MapRoom));
        }
    }

    /// Draws the review: its picture, then the page.
    fn draw_review(&mut self, fb: &mut Framebuffer) {
        if let Some((_, picture)) = self.pictures.iter().find(|(n, _)| *n == "review.m@l") {
            fb.pixels_mut()[..picture.pixels.len()].copy_from_slice(&picture.pixels);
        }
        let assets = &self.map_assets;
        let models = &assets.review_models;
        self.review
            .draw(fb, &assets.review, &assets.review_font, models);
        self.review.take_dirty();
    }

    /// What a mission view asked for: another view (shown at the next retrace, after
    /// one with the busy pointer), or the end of the mission.
    fn mission_request(&mut self, request: MissionRequest) {
        match request {
            MissionRequest::Show(view) => {
                // The view's picture loads with the busy pointer shown.
                self.loading = Some(Load::View(view));
                self.set_shape(BUSY);
            }
            MissionRequest::End => {
                log::info!("mission: ended with no result");
                self.act(Go(MapRoom));
            }
        }
    }

    /// Shows a mission view at once with its palette (`ui_open_view_of_object`).
    fn show_view(&mut self, view: View, fb: &mut Framebuffer) {
        log::info!("mission: {view:?} view of {}", self.mission.viewed_label());
        self.viewed(view);
        self.screen = Mission(view);
        self.changed = true;
        if let Some((_, picture)) = Mission(view)
            .file()
            .and_then(|f| self.pictures.iter().find(|(name, _)| *name == f))
        {
            let dac = picture.dac;
            self.fade_in = None;
            self.dac.stop();
            self.dac.set(&dac);
        }
        self.draw_mission(fb);
    }

    /// Draws the mission's view: its picture, then its own parts.
    fn draw_mission(&mut self, fb: &mut Framebuffer) {
        let file = self.screen.file();
        if let Some((_, picture)) = file.and_then(|f| self.pictures.iter().find(|(n, _)| *n == f)) {
            fb.pixels_mut()[..picture.pixels.len()].copy_from_slice(&picture.pixels);
        }
        self.mission.draw(fb, &self.mission_assets);
        self.mission.take_dirty();
    }

    /// The name entry is over (`ui_login_and_load_save_game`): the cursor stops
    /// blinking where it stands; a name logs in and goes to the map room; ESC leaves
    /// the game at the first log-in, and goes back to the map room after N.
    fn logged_in(&mut self, outcome: Outcome) {
        self.dac.anims_mut().set_active(login::CURSOR_RECORD, false);
        if std::mem::take(&mut self.login_fading) {
            self.dac.release_anims();
        }
        match outcome {
            Outcome::Accepted(name) => {
                // One random word for the run: the new campaign carries on the last one's.
                let random = campaign_mut(&mut self.player, &mut self.guest).take_random();
                let (mut player, loaded) = self.login.player(name, random);
                // A new campaign: nothing selected (`scen_start_new_campaign`).
                self.map = MapState::default();
                if let Some((selection, scenario)) = loaded {
                    // The save's scenario is the loaded one: playing it resumes it.
                    let file = save::file_name(&player.name);
                    let (random, motion) =
                        (player.campaign.take_random(), &self.mission_assets.motion);
                    self.mission = MissionState::new(file, &scenario, random, motion);
                    self.mission.park(&mut player.campaign);
                    self.map.set_selection(selection);
                }
                self.player = Some(player);
                self.act(Go(MapRoom));
            }
            Outcome::Cancelled if self.player.is_some() => self.act(Go(MapRoom)),
            Outcome::Cancelled => self.act(Quit),
        }
    }

    fn act(&mut self, action: Action) {
        log::debug!("screens: {:?} on {:?}", action, self.screen);
        let selection = self.map.selection();
        let rules = &self.map_assets.rules;
        if let DestroyBase(side) = action {
            self.mission.destroy_base(side);
            return;
        }
        let campaign = campaign_mut(&mut self.player, &mut self.guest);
        // Every action but playing leaves the mission, whatever its result, or is taken
        // away from it: its sim is parked, so the run's random word is back with the
        // campaign before the campaign draws from it.
        if !matches!(action, Play(_) | PlayLoaded) {
            self.mission.park(campaign);
        }
        match (self.screen, action) {
            (MapRoom, Next | Previous) => {
                let step = if action == Next { 1 } else { -1 };
                self.map.step(step, campaign);
                return;
            }
            (MapRoom, ProgressUp | ProgressDown | DevWin | DevLose) => {
                if !self.map_assets.developer {
                    log::debug!("map room: {action:?} needs unlock word 8 (XENO.KEY)");
                    return;
                }
                match action {
                    ProgressUp => campaign.raise_progress(rules),
                    ProgressDown => campaign.lower_progress(),
                    DevWin => {
                        if let Some((_, next)) = campaign.win(selection, Numbers::default(), rules)
                        {
                            self.map.set_selection(next);
                        }
                    }
                    _ => _ = campaign.lose(Numbers::default()),
                }
                log::info!("map room: {action:?}, {}", summary(campaign));
                self.map.redraw();
                return;
            }
            (MapRoom, Editor) => {
                // `ui_editor_key_toggle_flag`: a flag nothing reads; the editor's keys
                // work in a mission's situation room (T-048).
                if self.map_assets.editor {
                    log::info!(
                        "map room: Alt+F1 toggles a flag nothing reads; the editor's keys \
                         work in a mission's situation room"
                    );
                }
                return;
            }
            (MapRoom, Play(false)) => {
                let Some(i) = selection else {
                    log::debug!("map room: nothing selected to play");
                    return;
                };
                // The loaded scenario resumes where it was left; another is loaded.
                if campaign.loaded != Some(i) {
                    let file = maproom::scenario_file(i);
                    let Some(scenario) = self.mission_assets.scenarios.get(&file) else {
                        log::warn!("map room: {file} could not be loaded");
                        campaign.loaded = None;
                        return;
                    };
                    log::info!(
                        "map room: playing scenario {i}, {file} at state {}",
                        campaign.table.scenarios[i].state
                    );
                    let motion = &self.mission_assets.motion;
                    self.mission =
                        MissionState::new(file, scenario, campaign.take_random(), motion);
                    let state = campaign.table.scenarios[i].state;
                    let damage = rules.alert_damage.get(usize::from(state / 2));
                    self.mission.apply_alert(damage.copied().unwrap_or(0));
                    campaign.loaded = Some(i);
                } else {
                    log::info!("map room: scenario {i} is loaded; the mission resumes");
                    self.mission.resume(campaign);
                }
                campaign.run_mission(selection);
                campaign.game_loaded = true;
                self.scenario = Some(i);
                self.console = 0;
            }
            (MapRoom, PlayLoaded) => {
                log::info!(
                    "map room: running the loaded mission ({:?})",
                    campaign.loaded
                );
                self.mission.resume(campaign);
                campaign.run_mission(selection);
                self.scenario = campaign.loaded;
                self.console = 0;
            }
            (MapRoom, Go(Training)) if !campaign.may_train() => {
                log::debug!("map room: no training while a mission has no result");
                return;
            }
            (MapRoom, Go(Training)) => {
                campaign.open_training();
                self.map.set_selection(None);
            }
            (_, Play(true)) => self.scenario = None,
            (Mission(_), End(ending)) => {
                let (kills, losses) = self.mission.kills_and_losses();
                let points = self.endings.as_ref();
                let mission = Numbers {
                    kills,
                    losses,
                    points: points.map_or(0, |e| e.table.points(kills, losses)),
                };
                match ending {
                    Ending::Lost => _ = campaign.lose(mission),
                    Ending::Won | Ending::Triumph => {
                        if let Some((shown, next)) = campaign.win(selection, mission, rules) {
                            self.numbers = shown;
                            self.map.set_selection(next);
                        }
                        // Ctrl+C: the port's stand-in for the campaign's last win.
                        if ending == Ending::Triumph && campaign.result == 1 {
                            campaign.won = true;
                        }
                    }
                }
                log::info!("mission: {ending:?}, {}", summary(campaign));
            }
            _ => {}
        }
        // `ui_map_room_save_if_loaded`: N and Q save once a game is loaded.
        if let (MapRoom, Go(Login) | Quit, Some(player)) = (self.screen, action, &self.player)
            && player.campaign.game_loaded
        {
            let path = self.login.save_dir.join(save::file_name(&player.name));
            let written = match self.mission.scenario() {
                Some(scenario) => save::write(&player.campaign, selection, scenario)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        std::fs::create_dir_all(&self.login.save_dir)
                            .and_then(|()| std::fs::write(&path, bytes))
                            .map_err(|e| e.to_string())
                    }),
                None => Err("no scenario loaded".into()),
            };
            match written {
                Ok(()) => log::info!("map room: saved {}", path.display()),
                Err(e) => log::warn!("map room: cannot save {}: {e}", path.display()),
            }
        }
        let next = match action {
            Go(screen) => screen,
            Play(training) => {
                self.training = training;
                Mission(View::SitRoom)
            }
            Next | Previous | ProgressUp | ProgressDown | DevWin | DevLose | Editor
            | DestroyBase(_) => return,
            PlayLoaded => {
                self.training = false;
                Mission(View::SitRoom)
            }
            // As `ui_map_room_run_mission`: the campaign won first, then lost, then won;
            // no result (and training) goes straight back.
            End(_) => {
                let c = self::campaign(&self.player, &self.guest);
                match c.result {
                    _ if c.won && c.result != TRAINING => Cutscene(Ending::Triumph),
                    2 => Cutscene(Ending::Lost),
                    1 => Cutscene(Ending::Won),
                    _ => MapRoom,
                }
            }
            Quit => {
                self.quit = true;
                return;
            }
        };
        self.next = Some(next);
        self.fade_in = None;
        self.dac.stop();
        // The map room opens the review at once, and the review goes straight back to
        // the map room, which fades in again (`ui_run_review_screen`).
        if next == Review || self.screen == Review {
            return;
        }
        self.dac.fade_out(0, 255, FADE, u16::MAX);
    }

    /// Draws `screen` and starts its fade in.
    fn enter(&mut self, screen: Screen, fb: &mut Framebuffer) {
        // A mission shows the view of the object it views.
        let screen = match screen {
            Mission(_) => Mission(self.mission.view()),
            other => other,
        };
        log::info!("screen: {:?}", screen);
        if let Cutscene(ending) = screen {
            // The palette stays black; the window plays it, with the pointer hidden
            // (`1000:702d` on).
            self.pointer.hide();
            self.ending = Some(ending);
            self.screen = screen;
            self.next = None;
            self.changed = true;
            return;
        }
        if let Mission(view) = screen {
            self.viewed(view);
        }
        let picture = screen
            .file()
            .and_then(|f| self.pictures.iter().find(|(name, _)| *name == f));
        let dac = match picture {
            Some((_, picture)) => {
                fb.pixels_mut()[..picture.pixels.len()].copy_from_slice(&picture.pixels);
                picture.dac
            }
            None => {
                draw_placeholder(fb);
                to_vga6(&test_pattern::palette())
            }
        };
        if screen == Review {
            // Shown at once with its palette (slot `0x494`), on its first page.
            self.dac.set(&dac);
            self.screen = screen;
            self.next = None;
            self.changed = true;
            self.review = ReviewState::open();
            self.draw_review(fb);
            return;
        }
        let length = if screen == Login {
            // `ui_show_screen_fading_in` holds the colour records over its fade in.
            self.entry = NameEntry::default();
            if !self.login_fading {
                self.dac.hold_anims();
                self.login_fading = true;
            }
            LOGIN_FADE
        } else {
            FADE
        };
        self.fade_in = Some(self.dac.prepare_fade_in(&dac, 0, 255, length));
        self.screen = screen;
        self.next = None;
        self.changed = true;
        if screen == MapRoom {
            self.draw_map(fb);
        }
        if let Mission(_) = screen {
            self.mission.draw(fb, &self.mission_assets);
            self.mission.take_dirty();
        }
        if screen == Training {
            self.training_press = None;
        }
    }

    /// Draws the map room: its picture, then the boxes, panels and buttons.
    fn draw_map(&mut self, fb: &mut Framebuffer) {
        if let Some((_, picture)) = self.pictures.iter().find(|(n, _)| *n == "maproom.m@l") {
            fb.pixels_mut()[..picture.pixels.len()].copy_from_slice(&picture.pixels);
        }
        let name = self.player.as_ref().map_or("", |p| p.name.as_str());
        let campaign = campaign(&self.player, &self.guest);
        self.map.draw(fb, &self.map_assets, campaign, name);
        self.map.take_dirty();
    }
}

/// The mission `--screen` starts a view with: the first training mission with one of
/// the player's robots of the view's kind, viewing that robot; with none, an empty
/// mission in the situation room.
fn sample_mission(
    assets: &mission::Assets,
    view: View,
    campaign: &mut Campaign,
) -> (MissionState, Screen) {
    let kind = match view {
        View::SitRoom => None,
        View::Humanoid => Some(RobotKind::Humanoid),
        View::Scorpion => Some(RobotKind::Scorpion),
        View::Scout => Some(RobotKind::Scout),
    };
    let Some((file, scenario)) = assets.sample(kind) else {
        log::warn!("mission: no training mission has a robot for {view:?}; no robots shown");
        return (MissionState::empty(), Mission(View::SitRoom));
    };
    let mut mission = MissionState::new(file, scenario, campaign.take_random(), &assets.motion);
    let robot = kind.and_then(|k| {
        mission
            .book()
            .side(xeno_formats::usg::Side::Human)
            .find(|(_, r)| r.kind == k)
            .map(|(slot, _)| slot)
    });
    let shown = robot
        .and_then(|slot| mission.view_robot(slot))
        .unwrap_or(View::SitRoom);
    (mission, Mission(shown))
}

/// The campaign's numbers for the log.
fn summary(c: &Campaign) -> String {
    format!(
        "progress {}, result {}, score {}, wins {}, losses {}, day {}{}",
        c.progress,
        c.result,
        c.score,
        c.wins,
        c.losses,
        c.day,
        if c.won { ", campaign won" } else { "" }
    )
}

/// A screen whose picture is missing: a border and a yellow block.
fn draw_placeholder(fb: &mut Framebuffer) {
    use test_pattern::{BLACK, WHITE, YELLOW};
    let colour = YELLOW;
    let view = fb.view();
    let (w, h) = (view.width as u32, view.height as u32);
    fb.clear(WHITE);
    fb.fill_rect(Rect::new(1, 1, w - 2, h - 2), BLACK);
    fb.fill_rect(Rect::new(40, 40, w - 80, h - 80), colour);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::input::InputLog;
    use xeno_render::Size;

    /// A picture per file: every pixel is the file's position + 1, and every colour
    /// level 7 times that.
    fn pictures() -> Pictures {
        FILES
            .iter()
            .enumerate()
            .map(|(i, &name)| {
                let n = i as u8 + 1;
                let picture = Picture {
                    dac: [7 * n; DAC_BYTES],
                    pixels: vec![n; 320 * 200],
                };
                (name, picture)
            })
            .collect()
    }

    fn down(code: u16) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat: false,
        }
    }

    fn up(code: u16) -> InputEvent {
        InputEvent::KeyUp {
            code: ScanCode(code),
        }
    }

    pub(crate) fn new(start: Screen) -> Screens {
        let map = maproom::tests::test_assets();
        let missions = mission::tests::test_assets();
        Screens::new(pictures(), login::test_assets(), map, missions, start)
    }

    /// Ctrl with a key, for [`presses`].
    const CTRL_KEY: u16 = 0x1000;
    const CTRL_W: u16 = CTRL_KEY | 0x11;
    const CTRL_L: u16 = CTRL_KEY | 0x26;
    const CTRL_C: u16 = CTRL_KEY | 0x2e;

    /// Runs `retraces` retraces of the log; returns the screen after each. As the
    /// window does, it takes a cutscene the screens ask for; with no cutscenes loaded
    /// the screens go back to the map room at once.
    fn run(log: &InputLog, start: Screen, retraces: u64) -> (Screens, Vec<Screen>) {
        let mut screens = new(start);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let seen = (1..=retraces)
            .map(|r| {
                screens.retrace(log.events_at(r), &mut fb);
                let screen = screens.screen();
                assert!(screens.take_cutscene().is_none());
                screen
            })
            .collect();
        (screens, seen)
    }

    /// The retraces after Ctrl+W or Ctrl+L destroys a base before the next key: the
    /// base's end script (33 runs 16 retraces apart) and the fade out.
    const END: u64 = 600;

    /// A log typing `A` at retrace 50, then pressing and releasing each code 100
    /// retraces apart, from retrace 100, and [`END`] more after Ctrl+W and Ctrl+L.
    fn presses(codes: &[u16]) -> InputLog {
        let mut log = InputLog::default();
        log.record(50, [InputEvent::Text('A')]);
        let mut r = 0;
        for &code in codes {
            r += 100;
            if code & CTRL_KEY != 0 {
                let code = code & 0xff;
                log.record(r, [down(0x1d), down(code)]);
                log.record(r + 1, [up(code), up(0x1d)]);
            } else {
                log.record(r, [down(code)]);
                log.record(r + 1, [up(code)]);
            }
            if [CTRL_W, CTRL_L].contains(&code) {
                r += END;
            }
        }
        log
    }

    /// The retraces [`presses`] takes for `codes`, and 100 more.
    fn span(codes: &[u16]) -> u64 {
        let ends = codes
            .iter()
            .filter(|c| [CTRL_W, CTRL_L].contains(c))
            .count();
        100 * (codes.len() as u64 + 1) + END * ends as u64
    }

    fn final_screen(codes: &[u16]) -> Screen {
        let (_, seen) = run(&presses(codes), Login, span(codes));
        *seen.last().unwrap()
    }

    #[test]
    fn keys_step_through_every_screen() {
        assert_eq!(final_screen(&[]), Login);
        assert_eq!(final_screen(&[0x1c]), MapRoom);
        assert_eq!(final_screen(&[0x1c, 0x13]), Review);
        assert_eq!(final_screen(&[0x1c, 0x13, 0x01]), MapRoom);
        assert_eq!(final_screen(&[0x1c, 0x14]), Training);
        // Play needs a selection: Tab picks the first scenario.
        assert_eq!(final_screen(&[0x1c, 0x19]), MapRoom);
        assert_eq!(final_screen(&[0x1c, 0x0f, 0x19]), Mission(View::SitRoom));
        // Space views the roster's unit (a humanoid), Tab the next one (a scorpion),
        // ESC goes back to the situation room, Alt+Q (here Q with Alt) ends the mission.
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, 0x39]),
            Mission(View::Humanoid)
        );
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, 0x39, 0x0f]),
            Mission(View::Scorpion)
        );
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, 0x39, 0x0f, 0x01]),
            Mission(View::SitRoom)
        );
        // A mission's end fades to its cutscene, then back to the map room, where P
        // plays again.
        for (key, ending) in [
            (CTRL_W, Ending::Won),
            (CTRL_L, Ending::Lost),
            (CTRL_C, Ending::Triumph),
        ] {
            let codes = [0x1c, 0x0f, 0x19, key];
            let (_, seen) = run(&presses(&codes), Login, span(&codes));
            let at = seen.iter().position(|&s| s == Cutscene(ending)).unwrap();
            assert_eq!(seen[at - 1], Mission(View::SitRoom));
            assert_eq!(seen.last(), Some(&MapRoom));
        }
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, CTRL_L, 0x19]),
            Mission(View::SitRoom)
        );
        assert_eq!(final_screen(&[0x1c, 0x31]), Login);
        // A training mission (A, the synthetic menu's) goes back to the map room
        // without a cutscene.
        assert_eq!(final_screen(&[0x1c, 0x14, 0x1e, CTRL_W]), MapRoom);
        // Keys a screen doesn't bind do nothing.
        assert_eq!(final_screen(&[0x1c, 0x11, 0x01]), MapRoom);
    }

    #[test]
    fn a_mission_s_result_changes_the_campaign() {
        // Won: the scenario is won, the score has the bonus, the next one is selected,
        // and the WIN caption gets the points.
        let codes = [0x1c, 0x0f, 0x19, CTRL_W];
        let (screens, _) = run(&presses(&codes), Login, span(&codes));
        let c = screens.campaign();
        assert_eq!(c.table.scenarios[0].state, maproom::WON);
        assert_eq!((c.result, c.score, c.wins, c.loaded), (1, 1000, 1, Some(0)));
        // The mission gave the run's random word back for the win to draw from.
        assert!(c.has_random() && !screens.mission().is_running());
        assert_eq!(screens.numbers.points, 1000);
        // The caption's kills and losses are the player's base's.
        let n = screens.numbers;
        assert_eq!((n.kills, n.losses), screens.mission().kills_and_losses());
        assert_ne!((n.kills, n.losses), (0, 0));
        assert_ne!(screens.map.selection(), Some(0));
        // Lost: in play, forgotten, no score.
        let codes = [0x1c, 0x0f, 0x19, CTRL_L];
        let (screens, _) = run(&presses(&codes), Login, span(&codes));
        let c = screens.campaign();
        assert_eq!(c.table.scenarios[0].state, maproom::IN_PLAY);
        assert_eq!((c.result, c.score, c.losses, c.loaded), (2, 0, 1, None));
        // No result: training is refused and only the scenario in play is selectable;
        // Ctrl+Space runs the mission again.
        let (mut screens, _) = run(&presses(&[0x1c, 0x0f, 0x19]), Login, 300);
        assert!(screens.mission().is_running() && !screens.campaign().has_random());
        let mut fb = Framebuffer::new(Size::MODE_13H);
        screens.act(Go(MapRoom));
        (0..100).for_each(|_| screens.retrace(&[], &mut fb));
        assert_eq!((screens.screen(), screens.campaign().result), (MapRoom, 0));
        assert!(!screens.mission().is_running() && screens.campaign().has_random());
        screens.act(Go(Training));
        assert_eq!(screens.next, None);
        screens.act(PlayLoaded);
        assert_eq!(screens.next, Some(Mission(View::SitRoom)));
        assert!(screens.mission().is_running() && !screens.campaign().has_random());
    }

    #[test]
    fn n_saves_a_played_campaign_and_the_log_in_loads_it() {
        let dir = std::env::temp_dir().join(format!("xeno-screens-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let codes = [0x1c, 0x0f, 0x19, CTRL_W];
        let log = presses(&codes);
        let mut screens = new(Login);
        screens.login.save_dir = dir.clone();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        for r in 1..=span(&codes) {
            screens.retrace(log.events_at(r), &mut fb);
            screens.take_cutscene();
        }
        assert_eq!(screens.screen(), MapRoom);
        let path = dir.join("A.LOG");
        // N saves (a scenario was played), as Q would.
        screens.act(Go(Login));
        let saved = screens.campaign().clone();
        let bytes = std::fs::read(&path).unwrap();
        let (player, loaded) = screens.login.player("A".into(), Random::seeded(9));
        let (selection, scenario) = loaded.unwrap();
        let c = &player.campaign;
        assert_eq!(c.table, saved.table);
        assert_eq!((c.score, c.wins, c.result, c.loaded), (1000, 1, 1, Some(0)));
        assert_eq!((c.progress, c.day), (saved.progress, saved.day));
        assert_eq!(selection, screens.map.selection());
        assert_eq!(
            scenario.objects,
            screens.mission().scenario().unwrap().objects
        );
        assert_eq!(save::write(c, selection, &scenario).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_campaign_that_played_only_training_saves_nothing() {
        let dir = std::env::temp_dir().join(format!("xeno-unsaved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut screens = new(Login);
        screens.login.save_dir = dir.clone();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let log = presses(&[0x1c]);
        (1..=200).for_each(|r| screens.retrace(log.events_at(r), &mut fb));
        // A training mission loads a scenario but no game (`DS:0x6005`).
        screens.training_action(0);
        assert_eq!(screens.mission().file, "atrain.usg");
        screens.act(Quit);
        assert!(screens.quit());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_log_in_carries_the_run_s_random_word_on() {
        let (mut screens, _) = run(&presses(&[0x1c]), Login, 200);
        assert_eq!(screens.screen(), MapRoom);
        // The campaign draws, as a result would; then the player logs in again.
        let random = campaign_mut(&mut screens.player, &mut screens.guest).random();
        random.word();
        let drawn = random.clone();
        screens.logged_in(Outcome::Accepted("B".into()));
        assert_eq!(screens.player.as_ref().unwrap().name, "B");
        let random = campaign_mut(&mut screens.player, &mut screens.guest).random();
        assert_eq!(*random, drawn);
        assert_ne!(*random, Random::seeded(screens.login.seed));
    }

    #[test]
    fn training_and_the_developer_keys_follow_the_rules() {
        // The training menu clears the selection and sets the result to 3.
        let (screens, _) = run(&presses(&[0x1c, 0x0f, 0x14]), Login, 400);
        assert_eq!(screens.screen(), Training);
        assert_eq!(
            (screens.campaign().result, screens.map.selection()),
            (3, None)
        );
        // `=` raises the progress (the test assets unlock the developer keys); Ctrl+W
        // wins the selection with no cutscene.
        let (screens, _) = run(&presses(&[0x1c, 0x0d, 0x0f, CTRL_W]), Login, 500);
        let c = screens.campaign();
        assert_eq!(
            (c.progress, c.wins, c.table.scenarios[0].state),
            (2, 1, maproom::WON)
        );
        assert_eq!(screens.screen(), MapRoom);
        // Locked without the unlock word.
        let mut locked = new(Login);
        locked.map_assets.developer = false;
        locked.screen = MapRoom;
        locked.act(ProgressUp);
        assert_eq!(locked.campaign().progress, 1);
    }

    #[test]
    fn a_change_fades_out_then_in() {
        let (mut screens, seen) = run(&presses(&[0x1c]), Login, 200);
        // The log-in screen is drawn at retrace 1 and its black uploaded; the fade in
        // then runs 64 retraces. Enter at 100 fades out over 32 retraces and the map
        // room is drawn once the last step has been uploaded.
        assert_eq!(seen[98], Login);
        let changed = seen.iter().position(|&s| s == MapRoom).unwrap() + 1;
        assert!((133..=136).contains(&changed), "map room at {changed}");
        assert!(screens.take_changed());
        assert!(!screens.take_changed());
        // By 200 the map room's colours (level 14) show, one level short as the
        // driver's fade in leaves them.
        assert_eq!(screens.dac.hardware()[0], 13);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        screens.enter(MapRoom, &mut fb);
        assert_eq!(fb.get(5, 5), Some(2));
    }

    #[test]
    fn keys_during_a_fade_out_are_dropped() {
        let mut log = InputLog::default();
        log.record(50, [InputEvent::Text('A')]);
        log.record(100, [down(0x1c), up(0x1c)]);
        log.record(110, [down(0x13), up(0x13)]);
        let (_, seen) = run(&log, Login, 300);
        assert_eq!(*seen.last().unwrap(), MapRoom);
    }

    #[test]
    fn the_review_opens_at_once_turns_its_pages_and_goes_back() {
        // Enter (map room), R at 200 (the busy pointer over the map room for one
        // retrace, then the review, its palette at once), Tab at 300 (the second
        // page), Alt+Tab at 400, ESC at 500 (the map room, fading in).
        let mut log = presses(&[0x1c, 0x13, 0x0f]);
        log.record(400, [down(0x38), down(0x0f)]);
        log.record(401, [up(0x0f), up(0x38)]);
        log.record(500, [down(0x01)]);
        let mut screens = new(Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        for r in 1..=520 {
            screens.retrace(log.events_at(r), &mut fb);
            match r {
                200 => {
                    assert_eq!(screens.screen(), MapRoom);
                    assert_eq!(screens.pointer.shape(), BUSY);
                }
                201 => {
                    assert_eq!(screens.screen(), Review);
                    assert_eq!(screens.pointer.shape(), ARROW);
                    assert!(!screens.dac.fading(), "no fade in");
                }
                300 => assert_eq!(screens.review.page(), 1),
                400 => assert_eq!(screens.review.page(), 0),
                500 => assert_eq!(screens.pointer.shape(), BUSY),
                501 => assert!(screens.dac.fading() || screens.fade_in.is_some()),
                _ => {}
            }
        }
        assert_eq!(screens.screen(), MapRoom);
    }

    #[test]
    fn escape_is_claimed_by_the_screens_that_bind_it() {
        let claims: Vec<_> = [Login, MapRoom, Cutscene(Ending::Won)]
            .iter()
            .map(|s| s.claims_escape())
            .collect();
        // A cutscene's keys go to the player, which the window gives ESC to.
        assert_eq!(claims, [true, false, false]);
        // The training menu's, the review's and the views' tables (the user's) bind ESC.
        for screen in [
            Training,
            Review,
            Mission(View::SitRoom),
            Mission(View::Humanoid),
        ] {
            let mut screens = new(screen);
            screens.retrace([], &mut Framebuffer::new(Size::MODE_13H));
            assert!(screens.claims_escape(), "{screen:?}");
        }
        // ESC on the log-in screen asks to quit; Q and Alt+Q do on the map room.
        assert!(run(&presses(&[0x01]), Login, 200).0.quit());
        assert!(run(&presses(&[0x1c, 0x10]), Login, 300).0.quit());
        let mut log = presses(&[0x1c]);
        log.record(200, [down(0x38), down(0x10)]);
        assert!(run(&log, Login, 300).0.quit());
    }

    #[test]
    fn key_words_carry_the_modifiers_held_and_drop_e0() {
        let mut held = Keys::default();
        assert_eq!(key_word(ScanCode(0x10), &held), KeyWord(0x10));
        assert_eq!(key_word(ScanCode(0xe048), &held), KeyWord(0x48));
        held.press(ScanCode(0xe038)); // right Alt
        assert_eq!(key_word(ScanCode(0x10), &held), ALT_Q);
        held.press(ScanCode(0x36));
        held.press(ScanCode(0x1d));
        assert_eq!(key_word(ScanCode(0x01), &held), KeyWord(0x3801));
        // A modifier's own press doesn't count itself.
        assert_eq!(key_word(ScanCode(0x2a), &Keys::default()), KeyWord(0x2a));
        // Alt+Q on the map room quits; with Ctrl it does nothing.
        assert_eq!(MapRoom.lookup(ALT_Q), Some(Quit));
        assert_eq!(MapRoom.lookup(KeyWord(CTRL | 0x10)), None);
    }

    #[test]
    fn a_log_replays_the_same_screens() {
        let mut log = presses(&[0x1c, 0x14, 0x01, 0x0f, 0x19, 0x39, 0x0f, CTRL_L, 0x13]);
        // A click outside the map area and the buttons does nothing.
        let left = crate::input::MouseButton::Left;
        log.record(1650, [InputEvent::MouseMove { x: 1, y: 2 }]);
        log.record(1651, [InputEvent::MouseDown { button: left }]);
        log.record(1652, [InputEvent::MouseUp { button: left }]);
        let (_, first) = run(&log, Login, 1700);
        let (_, again) = run(&log.clone(), Login, 1700);
        assert_eq!(first, again);
        assert_eq!(first[1699], Review);
        assert!(first.contains(&Mission(View::Scorpion)));
        assert!(first.contains(&Cutscene(Ending::Lost)));
    }

    #[test]
    fn the_log_in_takes_a_name_and_blinks_its_cursor_after_the_fade() {
        use xeno_render::anim::flag::ACTIVE;
        let blinking =
            |s: &Screens| s.dac.anims().flags(login::CURSOR_RECORD).unwrap() & ACTIVE != 0;
        let mut log = InputLog::default();
        // Typed during the fade in, a key with no character, Backspace, more text.
        log.record(10, [InputEvent::Text('a'), down(0x3b), up(0x3b)]);
        log.record(80, [InputEvent::Text('e'), down(0x0e), up(0x0e)]);
        log.record(90, [InputEvent::Text('o'), InputEvent::Text('1')]);
        // Enter with no modifier.
        log.record(100, [down(0x1c), up(0x1c)]);
        let mut screens = new(Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        for r in 1..=200 {
            screens.retrace(log.events_at(r), &mut fb);
            match r {
                // Held over the 64-retrace fade in, then blinking until Enter.
                60 => assert!(!blinking(&screens)),
                70 => assert!(blinking(&screens)),
                95 => {
                    assert_eq!(screens.entry.name(), "AO1");
                    assert_eq!(fb.get(50, 69), Some(251), "the name is drawn");
                }
                101 => assert!(!blinking(&screens)),
                _ => {}
            }
        }
        assert_eq!(screens.screen(), MapRoom);
        let player = screens.player.as_ref().unwrap();
        assert_eq!(player.name, "AO1");
        assert!(!player.campaign.game_loaded);
        assert_eq!(screens.exit_message().unwrap(), "SEE YOU, AO1!");
    }

    #[test]
    fn the_map_room_picks_a_scenario_by_click_and_its_buttons_act() {
        use crate::input::MouseButton::{Left, Right};
        let click = |log: &mut InputLog, r: u64, (x, y), button| {
            log.record(
                r,
                [
                    InputEvent::MouseMove { x, y },
                    InputEvent::MouseDown { button },
                ],
            );
            log.record(r + 1, [InputEvent::MouseUp { button }]);
        };
        // Log in, click the fourth box (scenario 3, at 80..=89) then Enter.
        let mut log = presses(&[0x1c]);
        click(&mut log, 200, (85, 85), Left);
        log.record(300, [down(0x1c), up(0x1c)]);
        let mut screens = new(Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        for r in 1..=400 {
            screens.retrace(log.events_at(r), &mut fb);
            if r == 250 {
                // The map room's boxes over its picture: scenario 0 open (sprite 10),
                // and the highlight (sprite 8) on scenario 3.
                assert_eq!((fb.get(20, 20), fb.get(80, 80)), (Some(11), Some(9)));
                assert_eq!(fb.get(0, 0), Some(2), "the map room's picture");
            }
        }
        assert_eq!(
            (screens.screen(), screens.scenario()),
            (Mission(View::SitRoom), Some(3))
        );
        // The right button on a box plays it at once.
        let mut log = presses(&[0x1c]);
        click(&mut log, 200, (105, 105), Right);
        let (screens, seen) = run(&log, Login, 300);
        assert_eq!(
            (seen[299], screens.scenario()),
            (Mission(View::SitRoom), Some(4))
        );
        // The first button runs what R runs; the training menu's missions carry none.
        let mut log = presses(&[0x1c]);
        click(&mut log, 200, (285, 155), Left);
        assert_eq!(*run(&log, Login, 300).1.last().unwrap(), Review);
        let (screens, _) = run(
            &presses(&[0x1c, 0x0f, 0x19, CTRL_W, 0x14, 0x1e]),
            Login,
            1400,
        );
        assert_eq!(screens.scenario(), None);
        assert_eq!(screens.mission().file, "atrain.usg");
    }

    #[test]
    fn escape_quits_at_the_first_log_in_and_goes_back_after_n() {
        let (screens, _) = run(&presses(&[0x01]), Login, 200);
        assert!(screens.quit() && screens.player.is_none());
        // Log in, N, then ESC: back to the map room as the same player.
        let (screens, seen) = run(&presses(&[0x1c, 0x31, 0x01]), Login, 400);
        assert!(!screens.quit());
        assert_eq!(*seen.last().unwrap(), MapRoom);
        assert!(seen.contains(&Login));
        assert_eq!(screens.player.as_ref().unwrap().name, "A");
        // Enter with an empty name does nothing.
        let mut log = InputLog::default();
        log.record(100, [down(0x1c), up(0x1c)]);
        assert_eq!(*run(&log, Login, 200).1.last().unwrap(), Login);
    }

    #[test]
    fn screens_without_a_picture_get_a_placeholder() {
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let map = maproom::tests::test_assets();
        let mut screens = Screens::new(
            Vec::new(),
            login::test_assets(),
            map,
            mission::tests::test_assets(),
            MapRoom,
        );
        // One retrace with the busy pointer, then the picture.
        screens.retrace([], &mut fb);
        screens.retrace([], &mut fb);
        assert_eq!(fb.get(0, 0), Some(test_pattern::WHITE));
        assert_eq!(fb.get(160, 100), Some(test_pattern::YELLOW));
    }

    /// Endings with flat synthetic pictures and the synthetic tables
    /// (`xeno_formats::fixtures::synthetic_endings`).
    pub(crate) fn endings() -> Endings {
        let ds = 0x200;
        let mut image = vec![0u8; 0x1c000];
        xeno_formats::fixtures::synthetic_endings(&mut image, ds);
        let strings = xeno_formats::fixtures::SYNTHETIC_MAP_ROOM_STRINGS;
        let table = xeno_formats::ending::read_image(&image, ds, strings).unwrap();
        let pictures = [
            "win.vcl",
            "slose.vcl",
            "nlose.vcl",
            "slose.vcl",
            "triumph.vcl",
        ]
        .map(|name| crate::intro::Picture {
            name,
            dac: [9; DAC_BYTES],
            pixels: vec![1; 640 * 240],
            caption: Vec::new(),
        });
        Endings::new(table, pictures.to_vec(), None)
    }

    /// Plays the cutscene the screens hand over to its end, as the window does.
    fn play_cutscene(cutscene: &mut Cutscene) -> u64 {
        let mut fb = Cutscene::framebuffer();
        (1..).find(|_| cutscene.retrace([], true, &mut fb)).unwrap()
    }

    #[test]
    fn the_lose_cutscene_is_picked_by_the_console_opened_last() {
        let mut screens = new(Login);
        screens.set_endings(endings());
        let mut fb = Framebuffer::new(Size::MODE_13H);
        // Log in, select, play; Space opens the first unit's console (a humanoid), Tab
        // the next (a scorpion), ESC back to the situation room; Ctrl+L.
        let log = presses(&[0x1c, 0x0f, 0x19, 0x39, 0x0f, 0x01, CTRL_L]);
        let mut cutscene = None;
        for r in 1..=1400 {
            screens.retrace(log.events_at(r), &mut fb);
            if let Some(c) = screens.take_cutscene() {
                cutscene = Some((r, c));
                break;
            }
        }
        let (at, mut cutscene) = cutscene.expect("a cutscene after Ctrl+L");
        assert_eq!(screens.screen(), Cutscene(Ending::Lost));
        // The scorpion's entry: `GONE` on `nlose.vcl`.
        assert_eq!(screens.console, 1);
        assert_eq!(cutscene.kind(), crate::intro::Kind::Ending);
        // The end script's 33 runs, 16 retraces apart, then the fade out.
        assert!(at > 700 + 32 * 16 && at < 700 + 33 * 16 + 40, "{at}");
        assert!(play_cutscene(&mut cutscene) > 100);
        // Then the map room, from black, which fades in and takes keys again.
        screens.cutscene_over();
        assert_eq!(screens.dac.hardware()[0], 0);
        for _ in 0..40 {
            screens.retrace([], &mut fb);
        }
        assert_eq!(screens.screen(), MapRoom);
        assert!(screens.dac.hardware()[3 * 2] > 0, "the map room faded in");
        // A new mission starts with the humanoid's.
        screens.act(Play(false));
        assert_eq!(screens.console, 0);
    }

    #[test]
    fn won_and_triumph_hand_over_their_cutscenes() {
        for (start, kind) in [
            (Cutscene(Ending::Won), crate::intro::Kind::Ending),
            (Cutscene(Ending::Triumph), crate::intro::Kind::Triumph),
        ] {
            let mut screens = new(start);
            screens.set_endings(endings());
            let mut fb = Framebuffer::new(Size::MODE_13H);
            screens.retrace([], &mut fb);
            let cutscene = screens.take_cutscene().expect("--screen starts it");
            assert_eq!(cutscene.kind(), kind);
            assert!(screens.take_cutscene().is_none());
            assert_eq!(screens.screen(), start);
        }
    }

    #[test]
    fn the_training_menu_loads_the_mission_its_button_names() {
        use crate::input::MouseButton::Left;
        let mut log = presses(&[0x1c, 0x14]);
        // The first button (14, 200, 25, 275): its frame is XORed while pressed.
        log.record(300, [InputEvent::MouseMove { x: 210, y: 20 }]);
        log.record(301, [InputEvent::MouseDown { button: Left }]);
        log.record(303, [InputEvent::MouseUp { button: Left }]);
        let mut screens = new(Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        for r in 1..=400 {
            screens.retrace(log.events_at(r), &mut fb);
            if r == 302 {
                // The training menu's picture is pixel 3 everywhere; XOR 9 gives 10.
                assert_eq!((fb.get(200, 14), fb.get(210, 20)), (Some(10), Some(3)));
            }
        }
        assert_eq!(screens.screen(), Mission(View::SitRoom));
        assert_eq!(screens.mission().file, "atrain.usg");
        assert_eq!(screens.scenario(), None);
        // ESC on the menu goes back (its table binds it).
        assert_eq!(final_screen(&[0x1c, 0x14, 0x01]), MapRoom);
    }

    #[test]
    fn orders_set_on_a_console_survive_switching_views() {
        use crate::input::MouseButton::Left;
        use xeno_sim::orders::Orders;
        // Log in, play, Space: the humanoid console of unit 2.
        let mut log = presses(&[0x1c, 0x0f, 0x19, 0x39]);
        // A click on the program panel: the next program.
        log.record(500, [InputEvent::MouseMove { x: 130, y: 92 }]);
        log.record(501, [InputEvent::MouseDown { button: Left }]);
        log.record(502, [InputEvent::MouseUp { button: Left }]);
        // ESC to the situation room, Space back to the console.
        for (r, code) in [(600, 0x01), (700, 0x39)] {
            log.record(r, [down(code)]);
            log.record(r + 1, [up(code)]);
        }
        let mut screens = new(Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut seen = Vec::new();
        for r in 1..=800 {
            screens.retrace(log.events_at(r), &mut fb);
            if seen.last() != Some(&screens.screen()) {
                seen.push(screens.screen());
            }
        }
        assert_eq!(
            seen[seen.len() - 3..],
            [
                Mission(View::Humanoid),
                Mission(View::SitRoom),
                Mission(View::Humanoid)
            ]
        );
        assert_eq!(
            screens.mission().book().get(2).unwrap().orders,
            Orders::given(2)
        );
        // The console shows it: the panel filled with the humanoid's colour.
        assert_eq!(fb.get(193, 97), Some(0x10));
    }

    #[test]
    fn the_pointer_hides_until_the_map_room_goes_busy_for_loads_and_marks_corners() {
        // Log in, play, Space: the humanoid console; over its corner and away; ESC
        // back to the situation room.
        let mut log = presses(&[0x1c, 0x0f, 0x19, 0x39]);
        log.record(500, [InputEvent::MouseMove { x: 1, y: 1 }]);
        log.record(510, [InputEvent::MouseMove { x: 50, y: 60 }]);
        log.record(600, [down(0x01)]);
        log.record(601, [up(0x01)]);
        // Shapes up to 3, so the corner's and the 3D view's can be picked; the
        // console's 3D view (colour 255) under (50, 60).
        let mut login = login::test_assets();
        login.pointers.resize(4, login.pointers[0].clone());
        let missions = mission::tests::test_assets();
        let map = maproom::tests::test_assets();
        let mut pictures = pictures();
        let console = pictures.iter_mut().find(|(n, _)| *n == "humancon.m@l");
        console.unwrap().1.pixels[60 * 320 + 50] = 255;
        let mut screens = Screens::new(pictures, login, map, missions, Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut states = Vec::new();
        for r in 1..=610 {
            screens.retrace(log.events_at(r), &mut fb);
            let state = (screens.pointer().is_some(), screens.pointer.shape());
            if states.last().map(|&(_, s)| s) != Some(state) {
                states.push((r, state));
            }
        }
        let (busy, shown) = ((true, BUSY), (true, ARROW));
        let changes: Vec<_> = states.iter().map(|&(_, s)| s).collect();
        // Hidden at the log-in; busy then the arrow for the map room, the situation
        // room, the console (one retrace each) and back; the corner's shape, the 3D
        // view's.
        assert_eq!(
            changes,
            [
                (false, ARROW),
                busy,
                shown,
                busy,
                shown,
                busy,
                shown,
                (true, pointer::CORNER),
                (true, 3),
                busy,
                shown
            ]
        );
        let at = |i: usize| states[i].0;
        assert_eq!((at(5), at(6)), (400, 401));
        assert_eq!((at(7), at(8), at(9), at(10)), (500, 510, 600, 601));
        // The cutscenes hide it; it comes back with the map room.
        let mut screens = new(Cutscene(Ending::Won));
        screens.retrace([], &mut fb);
        assert!(screens.pointer().is_none());
        screens.cutscene_over();
        assert!(screens.pointer().is_some());
    }

    #[test]
    fn a_console_screen_starts_on_a_sample_mission() {
        let mut screens = new(Mission(View::Scorpion));
        screens.retrace([], &mut Framebuffer::new(Size::MODE_13H));
        assert_eq!(screens.screen(), Mission(View::Scorpion));
        assert_eq!(screens.mission().file, "atrain.usg");
        // No human scout in the synthetic mission: the situation room instead.
        let mut screens = new(Mission(View::Scout));
        screens.retrace([], &mut Framebuffer::new(Size::MODE_13H));
        assert_eq!(screens.screen(), Mission(View::SitRoom));
    }

    #[test]
    fn a_mission_view_starts_the_time_of_day_tint_after_its_fade_in() {
        use xeno_render::anim::flag::ACTIVE;
        let tinted = |s: &Screens| {
            mission::TINT_RECORDS
                .map(|r| s.dac.anims().flags(r).unwrap() & ACTIVE != 0)
                .contains(&true)
        };
        let mut screens = new(Mission(View::Scorpion));
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut faded = 0;
        for _ in 0..80 {
            screens.retrace([], &mut fb);
            if screens.screen() == Mission(View::Scorpion) && screens.dac.fading() {
                faded += 1;
                assert!(!tinted(&screens), "not during the fade in");
            }
        }
        assert!(faded > 0);
        assert!(tinted(&screens));
    }

    /// The user's files: the screens' pictures and the assets of the log-in, the map
    /// room and the missions.
    fn real_screens(start: Screen) -> Option<Screens> {
        use crate::game_dir::{ENV_VAR, GameDir};
        let path = std::env::var_os(ENV_VAR)?;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let pictures = load(&files).unwrap();
        assert_eq!(pictures.len(), 8);
        let mut login = login::load(&files, 1).unwrap();
        assert_eq!(login.campaign.scenarios.len(), 13);
        // Never write saves into the user's game directory.
        login.save_dir = std::env::temp_dir().join(format!("xeno-real-{}", std::process::id()));
        std::fs::create_dir_all(&login.save_dir).unwrap();
        let missions = mission::load(&files, login.campaign.scenarios.len()).unwrap();
        let map = maproom::load(&files).unwrap();
        Some(Screens::new(pictures, login, map, missions, start))
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_screen_loads_and_shows() {
        // Enter, R, ESC, T, ESC (back), Tab, P (the mission's situation room), Space
        // (the first unit's console), ESC (back), Ctrl+W (won, then the map room), P
        // (play the next scenario), Ctrl+L, P, Ctrl+C, N.
        let codes = [
            0x1c, 0x13, 0x01, 0x14, 0x01, 0x0f, 0x19, 0x39, 0x01, CTRL_W, 0x19, CTRL_L, 0x19,
            CTRL_C, 0x31,
        ];
        let log = presses(&codes);
        let Some(mut screens) = real_screens(Login) else {
            return;
        };
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut seen = vec![Login];
        let mut blink = Vec::new();
        for r in 1..=span(&codes) {
            screens.retrace(log.events_at(r), &mut fb);
            // The cutscenes are not loaded here: each goes back to the map room.
            assert!(screens.take_cutscene().is_none());
            // The user's record 23 blinks colour 254 once the fade in is over.
            if (70..100).contains(&r) {
                blink.push(screens.dac.hardware()[3 * 254..3 * 255].to_vec());
            }
            if seen.last() != Some(&screens.screen()) {
                seen.push(screens.screen());
            }
        }
        // Tab stays on the map room; every other key changes the screen, and each
        // ending goes through its cutscene to the map room.
        assert_eq!(seen.len(), codes.len() + 3, "{seen:?}");
        assert!(seen.contains(&Mission(View::Humanoid)), "{seen:?}");
        // The win moved the selection to the next scenario, which P then played.
        assert_eq!(screens.scenario(), Some(1));
        assert_eq!(screens.mission().file, "scen01a.usg");
        assert_eq!(screens.campaign().table.scenarios[0].state, maproom::WON);
        blink.dedup();
        assert!(blink.len() >= 2, "colour 254 never changed: {blink:?}");
        assert_eq!(screens.player.as_ref().unwrap().name, "A");
        assert_eq!(screens.screen(), Login);
        // N saved the campaign with the loaded scenario, and it loads back.
        let (player, loaded) = screens.login.player("A".into(), Random::seeded(1));
        let (_, scenario) = loaded.expect("A.LOG was written");
        assert_eq!(player.campaign.table, screens.campaign().table);
        assert_eq!(player.campaign.loaded, Some(1));
        assert_eq!(
            scenario.objects,
            screens.mission().scenario().unwrap().objects
        );
        std::fs::remove_dir_all(&screens.login.save_dir).unwrap();
    }

    /// Opens the review with the user's files, turns through every page with Tab and
    /// leaves it with ESC.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_review_opens_and_turns_every_page() {
        let Some(mut screens) = real_screens(Review) else {
            return;
        };
        let mut fb = Framebuffer::new(Size::MODE_13H);
        screens.retrace([], &mut fb);
        assert_eq!(screens.screen(), Review);
        let pages = screens.map_assets.review.pages.len();
        assert_eq!(pages, 3);
        assert!(screens.map_assets.review_models.iter().all(Option::is_some));
        // The picture's colour-255 pixels, all inside the view's rectangle.
        let picture = &screens
            .pictures
            .iter()
            .find(|(n, _)| *n == "review.m@l")
            .unwrap()
            .1;
        let view = screens.map_assets.review.view;
        let region: Vec<usize> = (0..64_000).filter(|&i| picture.pixels[i] == 255).collect();
        assert!(
            region
                .iter()
                .all(|&i| view.contains((i % 320) as u16, (i / 320) as u16))
        );
        for page in 1..=pages {
            // Once grown (32 ticks) the robot covers part of the region.
            (0..100).for_each(|_| screens.retrace([], &mut fb));
            let drawn = region.iter().filter(|&&i| fb.pixels()[i] != 0).count();
            assert!(drawn > 200, "page {}: {drawn} pixels", page - 1);
            screens.retrace(&[down(0x0f)], &mut fb);
            screens.retrace(&[up(0x0f)], &mut fb);
            assert_eq!(screens.review.page(), page % pages);
        }
        screens.retrace(&[down(0x01)], &mut fb);
        screens.retrace(&[up(0x01)], &mut fb);
        assert_eq!(screens.screen(), MapRoom);
    }

    /// Opens every console and the situation room, and every training mission, from
    /// the user's files; sets orders on each console and checks they are kept.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_view_and_training_mission_opens() {
        use crate::input::MouseButton::{Left, Right};
        for view in [View::SitRoom, View::Humanoid, View::Scorpion, View::Scout] {
            let Some(mut screens) = real_screens(Mission(view)) else {
                return;
            };
            let mut fb = Framebuffer::new(Size::MODE_13H);
            for _ in 0..40 {
                screens.retrace([], &mut fb);
            }
            assert_eq!(screens.screen(), Mission(view));
            if view == View::SitRoom {
                continue;
            }
            // A left then two right releases on the program panel.
            let panel = screens
                .mission_assets
                .tables_for_test(view)
                .find(xeno_formats::views::PROGRAM_PANEL_TYPE)
                .and_then(|w| w.area)
                .unwrap();
            let at = InputEvent::MouseMove {
                x: panel.left + 2,
                y: panel.top + 2,
            };
            let slot = match screens.mission().viewed() {
                mission::Viewed::Robot(slot) => slot,
                other => panic!("{other:?}"),
            };
            let before = screens.mission().book().get(slot).unwrap().orders;
            for button in [Left, Right, Right] {
                screens.retrace(&[at, InputEvent::MouseDown { button }], &mut fb);
                screens.retrace(&[InputEvent::MouseUp { button }], &mut fb);
            }
            let after = screens.mission().book().get(slot).unwrap().orders;
            assert!(!after.program_drives, "{view:?}");
            assert_ne!(after.program, before.program, "{view:?}");
            // ESC to the situation room and back with Space keeps them.
            for code in [0x01, 0x39] {
                screens.retrace(&[down(code)], &mut fb);
                screens.retrace(&[up(code)], &mut fb);
            }
            assert_eq!(screens.mission().book().get(slot).unwrap().orders, after);
        }
        // Each training mission (H, N, S, C) loads and opens its situation room.
        for code in [0x23, 0x31, 0x1f, 0x2e] {
            let Some(mut screens) = real_screens(Training) else {
                return;
            };
            let mut log = InputLog::default();
            log.record(100, [down(code)]);
            log.record(101, [up(code)]);
            let mut fb = Framebuffer::new(Size::MODE_13H);
            for r in 1..=200 {
                screens.retrace(log.events_at(r), &mut fb);
            }
            assert_eq!(screens.screen(), Mission(View::SitRoom), "{code:#x}");
            assert!(screens.mission().file.ends_with("train.usg"));
            assert!(
                screens
                    .mission()
                    .book()
                    .side(xeno_formats::usg::Side::Human)
                    .next()
                    .is_some()
            );
        }
    }

    /// Opens each console of the first training mission with a robot of its kind: the
    /// view is drawn through the picture's colour-255 region; then, turned towards each
    /// stock unit within 30 units, every one lands inside the view and draws pixels of
    /// its model there.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_consoles_draw_the_units_in_view() {
        use xeno_render::view3d::{FLAT_SKY, GROUND, place};
        for view in [View::Humanoid, View::Scorpion, View::Scout] {
            let Some(mut screens) = real_screens(Mission(view)) else {
                return;
            };
            let mut fb = Framebuffer::new(Size::MODE_13H);
            for _ in 0..40 {
                screens.retrace([], &mut fb);
            }
            let mission = screens.mission();
            let assets = &screens.mission_assets;
            let tables = assets.view3d.as_ref().expect("the 3D view's tables");
            assert!(assets.models.iter().all(Option::is_some));
            let mission::Viewed::Robot(slot) = mission.viewed() else {
                panic!("{view:?}");
            };
            let picture = screens
                .pictures
                .iter()
                .find(|(n, _)| Some(*n) == Mission(view).file())
                .map(|(_, p)| p)
                .unwrap();
            let region: Vec<usize> = (0..64_000).filter(|&i| picture.pixels[i] == 255).collect();
            let ground = region.iter().filter(|&&i| fb.pixels()[i] == GROUND).count();
            // The ground shows below the horizon, and no colour 255 is left.
            assert!(ground > 0, "{view:?}: no ground");
            assert!(region.iter().all(|&i| fb.pixels()[i] != 255), "{view:?}");
            let mut plain = Framebuffer::new(Size::MODE_13H);
            plain.pixels_mut().copy_from_slice(&picture.pixels);
            let (_, _, width, height) = crate::review::region(&plain).unwrap();
            let heights = crate::scene::heights(mission.scenario());
            let id = xeno_sim::EntityId(slot);
            let mut world = mission.world().clone();
            let at = world.get(id).unwrap().position;
            let near: Vec<_> = world
                .iter()
                .filter(|&(other, e)| {
                    let d = xeno_sim::terrain::approx_distance(at, e.position);
                    other != id && d.0 > 0x2_0000 && d.0 < 30 << 16
                })
                .map(|(other, e)| (other, e.position))
                .collect();
            assert!(!near.is_empty(), "{view:?}");
            let mut big = 0;
            for (other, position) in near {
                world.get_mut(id).unwrap().heading =
                    xeno_sim::script::bearing(&assets.motion.atan, at, position);
                let camera =
                    crate::scene::camera(&world, id, tables, &assets.motion, &heights).unwrap();
                let nodes = crate::scene::nodes(
                    &world,
                    tables,
                    &camera,
                    &assets.motion.atan,
                    &heights,
                    &[],
                );
                let node = nodes.iter().find(|n| n.object == other.0).unwrap();
                let size = (width as u16, height as u16);
                let ((x, y), scale) = place(tables, &camera, node, true, size)
                    .unwrap_or_else(|| panic!("{view:?}: {other:?} not drawn"));
                assert!(
                    x.abs_diff(width as i32 / 2) <= 1,
                    "{view:?}: {other:?} at {x}"
                );
                assert!(
                    (0..height as i32).contains(&y),
                    "{view:?}: {other:?} at {y}"
                );
                let mut alone = Framebuffer::new(Size::new(width, height));
                alone.draw_view(
                    tables,
                    &camera,
                    std::slice::from_ref(node),
                    &assets.models,
                    None,
                );
                let drawn = alone
                    .pixels()
                    .iter()
                    .filter(|&&p| p != FLAT_SKY && p != GROUND);
                // Far units shrink below a pixel (a scale of 256 is 1:1).
                if scale >= 32 {
                    assert!(drawn.count() > 0, "{view:?}: {other:?} drew nothing");
                    big += 1;
                }
            }
            assert!(big > 0, "{view:?}: no unit near enough to see");
            // Turned towards each hill of the mission's terrain, the view shows its
            // facets (colours 57 to 61), nearer than 20 units.
            let terrain = mission
                .scenario()
                .and_then(|s| s.terrain.as_ref())
                .map_or(&[][..], |t| &t.features);
            for (h, hill) in terrain.iter().enumerate().filter(|(_, f)| f.kind == 0x14) {
                let centre = xeno_sim::Position {
                    x: xeno_sim::Fixed(hill.x),
                    y: xeno_sim::Fixed(hill.y),
                };
                if xeno_sim::terrain::approx_distance(at, centre).0 > 20 << 16 {
                    continue;
                }
                world.get_mut(id).unwrap().heading =
                    xeno_sim::script::bearing(&assets.motion.atan, at, centre);
                let mut buffer = Framebuffer::new(Size::new(width, height));
                crate::scene::draw(
                    &mut buffer,
                    &world,
                    id,
                    tables,
                    &assets.motion,
                    &heights,
                    std::slice::from_ref(hill),
                    &[],
                    None,
                );
                let facets = buffer
                    .pixels()
                    .iter()
                    .filter(|p| (57..=61).contains(*p))
                    .count();
                assert!(facets > 0, "{view:?}: hill {h} not drawn");
            }
        }
    }
}
