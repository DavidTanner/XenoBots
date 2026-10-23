//! The front end's screens as a state machine (T-040), shaped like `xeno.exe`'s own
//! flow (`docs/screens.md`): log-in, then the map room as the hub, from which the
//! player plays a scenario, opens the training menu or the review, logs in again or
//! quits. A mission shows the situation room or a robot console, and a won or lost
//! one ends in a cutscene. The log-in is the original's (T-041, `login`): the name
//! entry with its blinking cursor. So is the map room (T-042, `maproom`): the
//! scenario boxes, selection by click and keys, the panels and the buttons. So are the
//! training menu and the mission's views (T-043, `mission`): the situation room and the
//! robot consoles, switched as the original switches them, with the robots' orders set
//! on the consoles; the simulation's parts are placeholders. The review and the
//! cutscenes are still placeholders: each one's `.m@l` picture with its palette (a
//! synthetic card for the cutscenes), faded as the original fades, and the window
//! title names it and its keys.
//!
//! Keys go through (key word, action) tables, as the original's screens do
//! (`docs/formats/key.md`, "The game's keys"); a screen claims ESC when its table
//! binds it. Everything here is plain data stepped once per retrace with that
//! retrace's input, so it is tested without a window and replays from an `InputLog`.

use xeno_formats::mal;
use xeno_formats::mgavideo::Pointer;
use xeno_render::{ColourAnims, DAC_BYTES, Dac, Fade, Framebuffer, Rect};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, Keys, Mouse, ScanCode};
use crate::login::{self, NameEntry, Outcome, Player};
use crate::maproom::{self, Campaign, MapState, Request};
use crate::mission::{self, Mission as MissionState, Request as MissionRequest};
use crate::test_pattern;
use crate::viewer::to_vga6;
use xeno_formats::orders::RobotKind;
use xeno_formats::views::TrainingAction;

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
    Quit,
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
/// Alt+Tab, Left and Up the previous. Not ESC. The developer keys are T-046's.
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
];
/// The training menu's keys are read from the user's `xeno.exe` (`DS:0x5e9f`, in
/// `mission::Assets::training`).
const TRAINING_KEYS: &[(KeyWord, Action)] = &[];
/// Not traced yet (T-044): ESC or Enter go back.
const REVIEW_KEYS: &[(KeyWord, Action)] = &[(ESC, Go(MapRoom)), (ENTER, Go(MapRoom))];
/// Placeholders until the sim exists: Ctrl+W, Ctrl+L and Ctrl+C end the mission won,
/// lost or with the campaign won (no view's table binds them). The views' own keys are
/// read from the user's `xeno.exe` (`mission`).
const MISSION_KEYS: &[(KeyWord, Action)] = &[
    (KeyWord(CTRL | 0x11), End(Ending::Won)),
    (KeyWord(CTRL | 0x26), End(Ending::Lost)),
    (KeyWord(CTRL | 0x2e), End(Ending::Triumph)),
];
/// A key ends a cutscene (the original's abort hook takes any key but P).
const CUTSCENE_KEYS: &[(KeyWord, Action)] = &[
    (ESC, Go(MapRoom)),
    (ENTER, Go(MapRoom)),
    (SPACE, Go(MapRoom)),
];

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
            Review => "Review: ESC/Enter back",
            Mission(View::SitRoom) => {
                "Mission, situation room: click/Tab select, Space view unit, Alt+Q end; Ctrl+W/L/C won/lost/campaign"
            }
            Mission(View::Humanoid) => {
                "Mission, humanoid console: click program (L next, R previous), F4 next, F2 auto, F3 manual, Tab unit, ESC back"
            }
            Mission(View::Scorpion) => {
                "Mission, scorpion console: click program (L next, R previous), F4 next, F2 auto, F3 manual, Tab unit, ESC back"
            }
            Mission(View::Scout) => {
                "Mission, scout console: click program (L next, R previous), F4 next, F2 auto, F3 manual, Tab unit, ESC back"
            }
            Cutscene(Ending::Won) => "Mission won (cutscene placeholder): Enter/Space/ESC",
            Cutscene(Ending::Lost) => "Mission lost (cutscene placeholder): Enter/Space/ESC",
            Cutscene(Ending::Triumph) => "Campaign won (cutscene placeholder): Enter/Space/ESC",
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
}

/// The logged-in player's campaign, or the guest's.
fn campaign<'a>(player: &'a Option<Player>, guest: &'a Campaign) -> &'a Campaign {
    player.as_ref().map_or(guest, |p| &p.campaign)
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
        let guest = Campaign::new(login.campaign.clone());
        let (mission, start) = match start {
            Mission(view) => sample_mission(&mission_assets, view),
            other => (MissionState::empty(), other),
        };
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
        }
    }

    /// The mission shown, or the last one.
    #[cfg(test)]
    pub fn mission(&self) -> &MissionState {
        &self.mission
    }

    /// The mouse pointer the screens show.
    pub fn pointer(&self) -> &Pointer {
        &self.login.pointer
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
            Mission(view) => self.mission_assets.claims_escape(view),
            Training => self
                .mission_assets
                .training
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
        for event in events {
            if self.next.is_none() && !self.quit {
                match (*event, self.screen) {
                    (InputEvent::Text(c), Login) => self.entry.typed(c),
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
                    (InputEvent::KeyDown { code, .. }, Mission(_)) => {
                        let word = key_word(code, &self.keys);
                        if let Some(action) = self.screen.lookup(word) {
                            self.act(action);
                        } else if let Some(request) = self.mission.key(word.0, &self.mission_assets)
                        {
                            self.mission_request(request, fb);
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
                    (InputEvent::MouseDown { button }, Mission(_)) => {
                        let at = self.mouse.position;
                        self.mission.mouse_down(button, at, &self.mission_assets);
                    }
                    (InputEvent::MouseUp { button }, Mission(_)) => {
                        let at = self.mouse.position;
                        if let Some(request) =
                            self.mission.mouse_up(button, at, &self.mission_assets)
                        {
                            self.mission_request(request, fb);
                        }
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
        if matches!(self.screen, Mission(_)) && self.next.is_none() && self.mission.take_dirty() {
            self.draw_mission(fb);
        }
        if let Some(next) = self.next
            && !self.dac.fading()
            && !self.dac.dirty()
        {
            self.enter(next, fb);
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
                        log::info!(
                            "training menu: {file} loaded; the mission itself cannot run yet (T-050 on)"
                        );
                        self.mission = MissionState::new(file, scenario);
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

    /// What a mission view asked for: another view (drawn at once, with its palette,
    /// as `ui_open_view_of_object` does), or the end of the mission.
    fn mission_request(&mut self, request: MissionRequest, fb: &mut Framebuffer) {
        match request {
            MissionRequest::Show(view) => {
                log::info!("mission: {view:?} view of {}", self.mission.viewed_label());
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
            MissionRequest::End => {
                log::info!("mission: ended with no result");
                self.act(Go(MapRoom));
            }
        }
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
                self.player = Some(self.login.player(name));
                // A new campaign: nothing selected (`scen_start_new_campaign`).
                self.map = MapState::default();
                self.act(Go(MapRoom));
            }
            Outcome::Cancelled if self.player.is_some() => self.act(Go(MapRoom)),
            Outcome::Cancelled => self.act(Quit),
        }
    }

    fn act(&mut self, action: Action) {
        log::debug!("screens: {:?} on {:?}", action, self.screen);
        let campaign = campaign(&self.player, &self.guest);
        match (self.screen, action) {
            (MapRoom, Next | Previous) => {
                let step = if action == Next { 1 } else { -1 };
                self.map.step(step, campaign);
                return;
            }
            (MapRoom, Play(false)) => {
                let Some(i) = self.map.selection() else {
                    log::debug!("map room: nothing selected to play");
                    return;
                };
                let file = maproom::scenario_file(i);
                self.mission = match self.mission_assets.scenarios.get(&file) {
                    Some(scenario) => {
                        log::info!(
                            "map room: playing scenario {i}, {file}; the mission itself cannot run yet (T-050 on)"
                        );
                        MissionState::new(file, scenario)
                    }
                    None => {
                        log::warn!(
                            "map room: {file} could not be loaded; a mission with no robots"
                        );
                        MissionState::empty()
                    }
                };
                self.scenario = Some(i);
            }
            (MapRoom, Go(Training)) if !campaign.may_train() => {
                log::debug!("map room: no training while a mission has no result");
                return;
            }
            (_, Play(true)) => self.scenario = None,
            _ => {}
        }
        if let (MapRoom, Go(Login) | Quit, Some(player)) = (self.screen, action, &self.player) {
            log::info!(
                "{}.LOG would be saved here; saving is not done yet (T-057)",
                player.name
            );
        }
        let next = match action {
            Go(screen) => screen,
            Play(training) => {
                self.training = training;
                Mission(View::SitRoom)
            }
            Next | Previous => return,
            End(_) if self.training => MapRoom,
            End(ending) => Cutscene(ending),
            Quit => {
                self.quit = true;
                return;
            }
        };
        self.next = Some(next);
        self.fade_in = None;
        self.dac.stop();
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
        let picture = screen
            .file()
            .and_then(|f| self.pictures.iter().find(|(name, _)| *name == f));
        let dac = match picture {
            Some((_, picture)) => {
                fb.pixels_mut()[..picture.pixels.len()].copy_from_slice(&picture.pixels);
                picture.dac
            }
            None => {
                draw_placeholder(screen, fb);
                to_vga6(&test_pattern::palette())
            }
        };
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
fn sample_mission(assets: &mission::Assets, view: View) -> (MissionState, Screen) {
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
    let mut mission = MissionState::new(file, scenario);
    let robot = kind.and_then(|k| {
        mission
            .book
            .side(xeno_formats::usg::Side::Human)
            .find(|(_, r)| r.kind == k)
            .map(|(slot, _)| slot)
    });
    let shown = robot
        .and_then(|slot| mission.view_robot(slot))
        .unwrap_or(View::SitRoom);
    (mission, Mission(shown))
}

/// A screen with no picture: a border and a block in a colour of its own.
fn draw_placeholder(screen: Screen, fb: &mut Framebuffer) {
    use test_pattern::{BLACK, GREEN, RED, WHITE, YELLOW};
    let colour = match screen {
        Cutscene(Ending::Won) => GREEN,
        Cutscene(Ending::Lost) => RED,
        _ => YELLOW,
    };
    let view = fb.view();
    let (w, h) = (view.width as u32, view.height as u32);
    fb.clear(WHITE);
    fb.fill_rect(Rect::new(1, 1, w - 2, h - 2), BLACK);
    fb.fill_rect(Rect::new(40, 40, w - 80, h - 80), colour);
}

#[cfg(test)]
mod tests {
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

    fn new(start: Screen) -> Screens {
        let map = maproom::tests::test_assets();
        let missions = mission::tests::test_assets();
        Screens::new(pictures(), login::test_assets(), map, missions, start)
    }

    /// Ctrl with a key, for [`presses`].
    const CTRL_KEY: u16 = 0x1000;
    const CTRL_W: u16 = CTRL_KEY | 0x11;
    const CTRL_L: u16 = CTRL_KEY | 0x26;
    const CTRL_C: u16 = CTRL_KEY | 0x2e;

    /// Runs `retraces` retraces of the log; returns the screen after each.
    fn run(log: &InputLog, start: Screen, retraces: u64) -> (Screens, Vec<Screen>) {
        let mut screens = new(start);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let seen = (1..=retraces)
            .map(|r| {
                screens.retrace(log.events_at(r), &mut fb);
                screens.screen()
            })
            .collect();
        (screens, seen)
    }

    /// A log typing `A` at retrace 50, then pressing and releasing each code 100
    /// retraces apart, from retrace 100.
    fn presses(codes: &[u16]) -> InputLog {
        let mut log = InputLog::default();
        log.record(50, [InputEvent::Text('A')]);
        for (i, &code) in codes.iter().enumerate() {
            let r = 100 * (i as u64 + 1);
            if code & CTRL_KEY != 0 {
                let code = code & 0xff;
                log.record(r, [down(0x1d), down(code)]);
                log.record(r + 1, [up(code), up(0x1d)]);
            } else {
                log.record(r, [down(code)]);
                log.record(r + 1, [up(code)]);
            }
        }
        log
    }

    fn final_screen(codes: &[u16]) -> Screen {
        let (_, seen) = run(&presses(codes), Login, 100 * (codes.len() as u64 + 1));
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
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, CTRL_W]),
            Cutscene(Ending::Won)
        );
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x19, CTRL_L]),
            Cutscene(Ending::Lost)
        );
        assert_eq!(
            final_screen(&[0x1c, 0x0f, 0x39, CTRL_C]),
            Cutscene(Ending::Triumph)
        );
        assert_eq!(final_screen(&[0x1c, 0x0f, 0x19, CTRL_L, 0x39]), MapRoom);
        assert_eq!(final_screen(&[0x1c, 0x31]), Login);
        // A training mission (A, the synthetic menu's) goes back to the map room
        // without a cutscene.
        assert_eq!(final_screen(&[0x1c, 0x14, 0x1e, CTRL_W]), MapRoom);
        // Keys a screen doesn't bind do nothing.
        assert_eq!(final_screen(&[0x1c, 0x11, 0x01]), MapRoom);
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
    fn escape_is_claimed_by_the_screens_that_bind_it() {
        let claims: Vec<_> = [Login, MapRoom, Review, Cutscene(Ending::Won)]
            .iter()
            .map(|s| s.claims_escape())
            .collect();
        assert_eq!(claims, [true, false, true, true]);
        // The training menu's and the views' tables (the user's) bind ESC.
        for screen in [Training, Mission(View::SitRoom), Mission(View::Humanoid)] {
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
        let mut log = presses(&[0x1c, 0x14, 0x01, 0x0f, 0x19, 0x39, 0x0f, CTRL_L, 0x1c, 0x13]);
        // A click outside the map area and the buttons does nothing.
        let left = crate::input::MouseButton::Left;
        log.record(1050, [InputEvent::MouseMove { x: 1, y: 2 }]);
        log.record(1051, [InputEvent::MouseDown { button: left }]);
        log.record(1052, [InputEvent::MouseUp { button: left }]);
        let (_, first) = run(&log, Login, 1100);
        let (_, again) = run(&log.clone(), Login, 1100);
        assert_eq!(first, again);
        assert_eq!(first[1099], Review);
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
        assert_eq!((player.name.as_str(), player.save_found), ("AO1", false));
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
            &presses(&[0x1c, 0x0f, 0x19, CTRL_W, 0x1c, 0x14, 0x1e]),
            Login,
            800,
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
            Cutscene(Ending::Lost),
        );
        screens.retrace([], &mut fb);
        assert_eq!(fb.get(0, 0), Some(test_pattern::WHITE));
        assert_eq!(fb.get(160, 100), Some(test_pattern::RED));
        // A missing picture is drawn the same way rather than failing.
        screens.enter(MapRoom, &mut fb);
        assert_eq!(fb.get(160, 100), Some(test_pattern::YELLOW));
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
        use xeno_formats::orders::Orders;
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
            screens.mission().book.get(2).unwrap().orders,
            Orders::given(2)
        );
        // The console shows it: the panel filled with the humanoid's colour.
        assert_eq!(fb.get(193, 97), Some(0x10));
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
        let login = login::load(&files).unwrap();
        assert_eq!(login.campaign.scenarios.len(), 13);
        let missions = mission::load(&files, login.campaign.scenarios.len()).unwrap();
        let map = maproom::load(&files).unwrap();
        Some(Screens::new(pictures, login, map, missions, start))
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_screen_loads_and_shows() {
        // Enter, R, ESC, T, ESC (back), Tab, P (the mission's situation room), Space
        // (the first unit's console), ESC (back), Ctrl+W (won), Enter, Space (play
        // again), Ctrl+L, ESC (map room), P, Ctrl+C, Enter, N.
        let codes = [
            0x1c, 0x13, 0x01, 0x14, 0x01, 0x0f, 0x19, 0x39, 0x01, CTRL_W, 0x1c, 0x39, CTRL_L, 0x01,
            0x19, CTRL_C, 0x1c, 0x31,
        ];
        let log = presses(&codes);
        let Some(mut screens) = real_screens(Login) else {
            return;
        };
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut seen = vec![Login];
        let mut blink = Vec::new();
        for r in 1..=100 * (codes.len() as u64 + 1) {
            screens.retrace(log.events_at(r), &mut fb);
            // The user's record 23 blinks colour 254 once the fade in is over.
            if (70..100).contains(&r) {
                blink.push(screens.dac.hardware()[3 * 254..3 * 255].to_vec());
            }
            if seen.last() != Some(&screens.screen()) {
                seen.push(screens.screen());
            }
        }
        // Tab stays on the map room; every other key changes the screen.
        assert_eq!(seen.len(), codes.len(), "{seen:?}");
        assert!(seen.contains(&Mission(View::Humanoid)), "{seen:?}");
        assert_eq!(screens.scenario(), Some(0));
        assert_eq!(screens.mission().file, "scen00a.usg");
        blink.dedup();
        assert!(blink.len() >= 2, "colour 254 never changed: {blink:?}");
        assert_eq!(screens.player.as_ref().unwrap().name, "A");
        assert_eq!(screens.screen(), Login);
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
            let before = screens.mission().book.get(slot).unwrap().orders;
            for button in [Left, Right, Right] {
                screens.retrace(&[at, InputEvent::MouseDown { button }], &mut fb);
                screens.retrace(&[InputEvent::MouseUp { button }], &mut fb);
            }
            let after = screens.mission().book.get(slot).unwrap().orders;
            assert!(!after.manual, "{view:?}");
            assert_ne!(after.program, before.program, "{view:?}");
            // ESC to the situation room and back with Space keeps them.
            for code in [0x01, 0x39] {
                screens.retrace(&[down(code)], &mut fb);
                screens.retrace(&[up(code)], &mut fb);
            }
            assert_eq!(screens.mission().book.get(slot).unwrap().orders, after);
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
                    .book
                    .side(xeno_formats::usg::Side::Human)
                    .next()
                    .is_some()
            );
        }
    }
}
