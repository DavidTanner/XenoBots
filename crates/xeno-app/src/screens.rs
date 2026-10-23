//! The front end's screens as a state machine (T-040), shaped like `xeno.exe`'s own
//! flow (`docs/screens.md`): log-in, then the map room as the hub, from which the
//! player plays a scenario, opens the training menu or the review, logs in again or
//! quits. A mission shows the situation room or a robot console, and a won or lost
//! one ends in a cutscene. Each screen is a placeholder for now: its `.m@l` picture
//! with its palette (a synthetic card for the cutscenes), faded as the original
//! fades, and the window title names it and its keys.
//!
//! Keys go through (key word, action) tables, as the original's screens do
//! (`docs/formats/key.md`, "The game's keys"); a screen claims ESC when its table
//! binds it. Everything here is plain data stepped once per retrace with that
//! retrace's input, so it is tested without a window and replays from an `InputLog`.

use xeno_formats::mal;
use xeno_render::{DAC_BYTES, Dac, Fade, Framebuffer, Rect};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, Keys, ScanCode};
use crate::test_pattern;
use crate::viewer::to_vga6;

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
    /// Start a mission; `true` from the training menu (no cutscene after it).
    Play(bool),
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

/// `ui_login_and_load_save_game`: ESC leaves the game (T-041 adds the name entry).
const LOGIN_KEYS: &[(KeyWord, Action)] = &[(ENTER, Go(MapRoom)), (ESC, Quit)];
/// The map room's own keys (`DS:0x60bf`): P, Enter or Space play, T training, R
/// review, N log in again, Q or Alt+Q quit. Not ESC.
const MAP_ROOM_KEYS: &[(KeyWord, Action)] = &[
    (key(0x13), Go(Review)),
    (ENTER, Play(false)),
    (SPACE, Play(false)),
    (key(0x19), Play(false)),
    (key(0x31), Go(Login)),
    (key(0x14), Go(Training)),
    (ALT_Q, Quit),
    (key(0x10), Quit),
];
/// The training menu's (`DS:0x5e9f`): H, N, S, C pick a training mission; ESC or
/// Alt+Q go back.
const TRAINING_KEYS: &[(KeyWord, Action)] = &[
    (ESC, Go(MapRoom)),
    (ALT_Q, Go(MapRoom)),
    (key(0x23), Play(true)),
    (key(0x31), Play(true)),
    (key(0x1f), Play(true)),
    (key(0x2e), Play(true)),
];
/// Not traced yet (T-044): ESC or Enter go back.
const REVIEW_KEYS: &[(KeyWord, Action)] = &[(ESC, Go(MapRoom)), (ENTER, Go(MapRoom))];
/// Placeholders until the sim exists: F1 to F4 pick the view, W, L and C end the
/// mission won, lost or with the campaign won.
const MISSION_KEYS: &[(KeyWord, Action)] = &[
    (key(0x3b), Go(Mission(View::SitRoom))),
    (key(0x3c), Go(Mission(View::Humanoid))),
    (key(0x3d), Go(Mission(View::Scorpion))),
    (key(0x3e), Go(Mission(View::Scout))),
    (key(0x11), End(Ending::Won)),
    (key(0x26), End(Ending::Lost)),
    (key(0x2e), End(Ending::Triumph)),
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
            Login => "Log-in: Enter continue, ESC quit",
            MapRoom => "Map room: P/Enter play, T training, R review, N log-in, Q quit",
            Training => "Training menu: H/N/S/C train, ESC back",
            Review => "Review: ESC/Enter back",
            Mission(View::SitRoom) => {
                "Mission, situation room: F1-F4 view, W won, L lost, C campaign won"
            }
            Mission(View::Humanoid) => {
                "Mission, humanoid console: F1-F4 view, W won, L lost, C campaign won"
            }
            Mission(View::Scorpion) => {
                "Mission, scorpion console: F1-F4 view, W won, L lost, C campaign won"
            }
            Mission(View::Scout) => {
                "Mission, scout console: F1-F4 view, W won, L lost, C campaign won"
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
}

impl Screens {
    /// Starts at `start`, which is drawn and faded in at the first retrace.
    pub fn new(pictures: Pictures, start: Screen) -> Screens {
        Screens {
            pictures,
            screen: start,
            next: Some(start),
            training: false,
            dac: Dac::default(),
            fade_in: None,
            keys: Keys::default(),
            quit: false,
            changed: true,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn palette(&self) -> xeno_render::Palette {
        self.dac.palette()
    }

    pub fn claims_escape(&self) -> bool {
        self.screen.claims_escape()
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
        for event in events {
            if let InputEvent::KeyDown { code, .. } = *event
                && self.next.is_none()
                && !self.quit
                && let Some(action) = self.screen.lookup(key_word(code, &self.keys))
            {
                self.act(action);
            }
            self.keys.apply(event);
        }
        if let Some(next) = self.next
            && !self.dac.fading()
            && !self.dac.dirty()
        {
            self.enter(next, fb);
        }
    }

    fn act(&mut self, action: Action) {
        log::debug!("screens: {:?} on {:?}", action, self.screen);
        let next = match action {
            Go(screen) => screen,
            Play(training) => {
                self.training = training;
                Mission(View::SitRoom)
            }
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
        let length = if screen == Login { LOGIN_FADE } else { FADE };
        self.fade_in = Some(self.dac.prepare_fade_in(&dac, 0, 255, length));
        self.screen = screen;
        self.next = None;
        self.changed = true;
    }
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

    /// Runs `retraces` retraces of the log; returns the screen after each.
    fn run(log: &InputLog, start: Screen, retraces: u64) -> (Screens, Vec<Screen>) {
        let mut screens = Screens::new(pictures(), start);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let seen = (1..=retraces)
            .map(|r| {
                screens.retrace(log.events_at(r), &mut fb);
                screens.screen()
            })
            .collect();
        (screens, seen)
    }

    /// A log pressing and releasing each code 100 retraces apart, from retrace 100.
    fn presses(codes: &[u16]) -> InputLog {
        let mut log = InputLog::default();
        for (i, &code) in codes.iter().enumerate() {
            let r = 100 * (i as u64 + 1);
            log.record(r, [down(code)]);
            log.record(r + 1, [up(code)]);
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
        assert_eq!(final_screen(&[0x1c, 0x19]), Mission(View::SitRoom));
        assert_eq!(final_screen(&[0x1c, 0x19, 0x3c]), Mission(View::Humanoid));
        assert_eq!(final_screen(&[0x1c, 0x19, 0x3e]), Mission(View::Scout));
        assert_eq!(final_screen(&[0x1c, 0x19, 0x11]), Cutscene(Ending::Won));
        assert_eq!(final_screen(&[0x1c, 0x19, 0x26]), Cutscene(Ending::Lost));
        assert_eq!(final_screen(&[0x1c, 0x39, 0x2e]), Cutscene(Ending::Triumph));
        assert_eq!(final_screen(&[0x1c, 0x19, 0x26, 0x39]), MapRoom);
        assert_eq!(final_screen(&[0x1c, 0x31]), Login);
        // A training mission goes back to the map room without a cutscene.
        assert_eq!(final_screen(&[0x1c, 0x14, 0x23, 0x11]), MapRoom);
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
        log.record(100, [down(0x1c), up(0x1c)]);
        log.record(110, [down(0x13), up(0x13)]);
        let (_, seen) = run(&log, Login, 300);
        assert_eq!(*seen.last().unwrap(), MapRoom);
    }

    #[test]
    fn escape_is_claimed_by_the_screens_that_bind_it() {
        let claims: Vec<_> = [
            Login,
            MapRoom,
            Training,
            Review,
            Mission(View::SitRoom),
            Cutscene(Ending::Won),
        ]
        .iter()
        .map(|s| s.claims_escape())
        .collect();
        assert_eq!(claims, [true, false, true, true, false, true]);
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
        // Alt+Q on the training menu goes back; Q alone does nothing there.
        assert_eq!(Training.lookup(ALT_Q), Some(Go(MapRoom)));
        assert_eq!(Training.lookup(key(0x10)), None);
    }

    #[test]
    fn a_log_replays_the_same_screens() {
        let mut log = presses(&[0x1c, 0x14, 0x01, 0x19, 0x3d, 0x26, 0x1c, 0x13]);
        // The mouse doesn't move between screens yet.
        let left = crate::input::MouseButton::Left;
        log.record(950, [InputEvent::MouseMove { x: 1, y: 2 }]);
        log.record(951, [InputEvent::MouseDown { button: left }]);
        let (_, first) = run(&log, Login, 1000);
        let (_, again) = run(&log.clone(), Login, 1000);
        assert_eq!(first, again);
        assert_eq!(first[999], Review);
        assert!(first.contains(&Mission(View::Scorpion)));
        assert!(first.contains(&Cutscene(Ending::Lost)));
    }

    #[test]
    fn screens_without_a_picture_get_a_placeholder() {
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut screens = Screens::new(Vec::new(), Cutscene(Ending::Lost));
        screens.retrace([], &mut fb);
        assert_eq!(fb.get(0, 0), Some(test_pattern::WHITE));
        assert_eq!(fb.get(160, 100), Some(test_pattern::RED));
        // A missing picture is drawn the same way rather than failing.
        screens.enter(MapRoom, &mut fb);
        assert_eq!(fb.get(160, 100), Some(test_pattern::YELLOW));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_screen_loads_and_shows() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let pictures = load(&files).unwrap();
        assert_eq!(pictures.len(), 8);
        let codes = [
            0x1c, 0x13, 0x01, 0x14, 0x23, 0x11, 0x19, 0x3c, 0x3d, 0x3e, 0x3b, 0x11, 0x1c, 0x39,
            0x26, 0x01, 0x19, 0x2e, 0x1c, 0x31,
        ];
        let log = presses(&codes);
        let mut screens = Screens::new(pictures, Login);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut seen = vec![Login];
        for r in 1..=100 * (codes.len() as u64 + 1) {
            screens.retrace(log.events_at(r), &mut fb);
            if seen.last() != Some(&screens.screen()) {
                seen.push(screens.screen());
            }
        }
        assert_eq!(seen.len(), codes.len() + 1, "{seen:?}");
        assert_eq!(screens.screen(), Login);
    }
}
