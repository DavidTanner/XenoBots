//! The game window: `winit` for the window and events, `softbuffer` to put the
//! framebuffer on screen. It plays the start-up cutscene (`intro`) in the 320x240 mode
//! and, unless a key ended it, the intro drop (`drop`) in mode 13h, then the front
//! end's screens (`screens`), and the cutscenes they ask for after a mission in the
//! 320x240 mode, until a screen quits, or ESC or close; or the test card,
//! with markers driven by the game clock; or, with `--view`, one asset (`viewer`).
//!
//! The loop (`docs/game-loop.md`): the clock runs the retraces due, each one taking the
//! input events that arrived since the last; if any ran, a frame is requested; then the
//! loop sleeps until the next retrace is due (`ControlFlow::WaitUntil`). Leaving the
//! cutscene's mode restarts the clock at the new mode's rate, as the original measures
//! the frame period again after every mode change.
//!
//! Sizes: the softbuffer surface and `xeno_render::Layout` work in physical pixels.
//! The default window asks for a logical size (so it looks the same on a 1x and a 2x
//! display); `--scale N` asks for the picture's physical size. Fullscreen is borderless
//! on the current monitor, so the display mode never changes; the scaler fits the
//! picture to whatever size that gives.

use std::collections::HashMap;
use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size as WindowSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};
use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_render::{
    Aspect, AssetId, Framebuffer, HdImage, Layout, Palette, Rgb, Scale, Size, TextCursor, TextMode,
};

use crate::clock::{Clock, Rate, SPEEDS, Step};
use crate::drop;
use crate::input::{InputLog, Keys, Mouse};
use crate::intro::{Cutscene, Kind};
use crate::keymap::KeyMap;
use crate::legend;
use crate::music::{Cue, Jukebox};
use crate::pointer;
use crate::screens::{Screen, Screens};
use crate::test_pattern;
use crate::translate::{Translator, WindowAction};

/// The port's labels' colours: the palette's brightest colour and its darkest, leaving
/// out the colours the views' records animate (16..=31, 253..=255) so they never blink.
fn steady_colours(palette: &Palette) -> Option<(u8, u8)> {
    let steady = || (0..=252u8).filter(|c| !(16..=31).contains(c));
    let level = |c: &u8| {
        let Rgb { r, g, b } = palette.colors[usize::from(*c)];
        u16::from(r) + u16::from(g) + u16::from(b)
    };
    Some((steady().max_by_key(level)?, steady().min_by_key(level)?))
}

/// One of the port's labels: `text` in `font` in a box one pixel round it, its corner at
/// surface pixel (`x`, `y`), light on dark.
fn draw_label(
    fb: &mut Framebuffer,
    font: &Font,
    (light, dark): (u8, u8),
    (x, y): (usize, usize),
    text: &str,
) {
    let (cell, rows) = (usize::from(font.header.width), font.rows());
    let width = cell * text.len() + 2;
    let stride = fb.surface().width;
    for row in y..y + rows + 2 {
        for column in x..x + width {
            if let Some(p) = fb.pixels_mut().get_mut(row * stride + column) {
                *p = dark;
            }
        }
    }
    let mut cursor = TextCursor::at(x as i32 + 1, y as i32 + 1, light);
    cursor.background = dark;
    fb.draw_text(font, &mut cursor, text.as_bytes(), TextMode::Opaque);
}

/// Draws the game speed's label (T-215), the port's own text such as `4X`, at the
/// view's top right in `font`, in the steady colours.
fn draw_speed(fb: &mut Framebuffer, palette: &Palette, font: &Font, speed: u64) {
    let Some(colours) = steady_colours(palette) else {
        return;
    };
    let text = format!("{speed}X");
    let width = usize::from(font.header.width) * text.len() + 2;
    let (ox, oy) = fb.origin();
    let x = ox + fb.view().width.saturating_sub(width + 2);
    draw_label(fb, font, colours, (x, oy + 2), &text);
}

/// Draws the key legend (T-217): each label placed by `legend::place` in the view, as
/// the speed's label.
fn draw_legend(fb: &mut Framebuffer, palette: &Palette, font: &Font, labels: &[(Rect, String)]) {
    let Some(colours) = steady_colours(palette) else {
        return;
    };
    let view = fb.view();
    let (cell, rows) = (usize::from(font.header.width), font.rows());
    let (ox, oy) = fb.origin();
    for p in legend::place(labels, cell, rows, (view.width, view.height)) {
        draw_label(fb, font, colours, (ox + p.x, oy + p.y), &p.text);
    }
}

/// At a game speed of N (T-215) the legs step N times as often, so only every Nth
/// footstep is played. A footstep is a run of retraces in a row that post one: the
/// humanoid's and the scout's post on one retrace as a leg lands, the scorpion's on
/// every retrace its step is even, each restarting the sound. A played run plays
/// whole, so the sound is the one 1x gives, at 1x's pace.
#[derive(Debug, Default)]
struct Footsteps {
    /// Runs since the last played one.
    runs: u64,
    /// The last retrace that posted a footstep.
    last: Option<u64>,
    /// Whether the current run plays.
    play: bool,
}

impl Footsteps {
    /// Whether a footstep posted on `retrace` is played at `speed`.
    fn hear(&mut self, retrace: u64, speed: u64) -> bool {
        let same_run = self.last.is_some_and(|last| retrace <= last + 1);
        if !same_run {
            self.runs = (self.runs + 1) % speed;
            self.play = self.runs == 0;
        }
        self.last = Some(retrace);
        self.play
    }
}

/// The footstep sound events (`xeno_sim::motion`), kept at 1x at a game speed.
const FOOTSTEPS: [u8; 3] = [
    xeno_sim::motion::FOOTSTEP_HUMANOID,
    xeno_sim::motion::FOOTSTEP_SCORPION,
    xeno_sim::motion::FOOTSTEP_SCOUT,
];
use crate::viewer::Viewer;

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// What comes after the intro and the drop.
#[derive(Debug)]
pub enum FrontEnd {
    Screens(Box<Screens>),
    /// `--screen test-card`, with the stick keys it colours.
    TestCard(KeyMap),
}

/// Opens the window on `viewer`, or plays `intro` and then the drop (`drop`, alone if
/// there is no intro), then runs `front`, until a screen quits or the user presses ESC
/// or closes the window. `music` plays what the cutscenes and screens cue; a mission
/// in play runs `speed` retraces a real one (T-215); `wasd` starts WASD on (T-216);
/// `hd_shapes` draws the recorded shapes again at the window's resolution (T-236), and
/// `hd_ui` the recorded glyphs (T-074); `hd_pack`'s images replace the blits they name
/// (T-072).
#[allow(clippy::too_many_arguments)]
pub fn run(
    scale: Scale,
    aspect: Aspect,
    fullscreen: bool,
    speed: u64,
    wasd: bool,
    hd_shapes: bool,
    hd_ui: bool,
    hd_pack: HashMap<AssetId, HdImage>,
    viewer: Option<Viewer>,
    intro: Option<Cutscene>,
    mut drop: Option<drop::Assets>,
    front: FrontEnd,
    music: Jukebox,
) -> Result<(), Box<dyn Error>> {
    // Without the intro, the drop starts at once, its counter where its first wait ends.
    let playing = match (&viewer, &intro, drop.take()) {
        (None, None, Some(assets)) => {
            let start = assets.drop.numbers.start;
            Some(drop::Player::new(assets, start))
        }
        (_, _, assets) => {
            drop = assets;
            None
        }
    };
    let (fb, palette, title) = match (&viewer, &intro, &playing) {
        (Some(v), _, _) => (v.framebuffer(), v.palette.clone(), v.title()),
        (None, Some(intro), _) => (Cutscene::framebuffer(), intro.palette(), "Xenobots".into()),
        (None, None, Some(p)) => (p.framebuffer(), p.palette(), "Xenobots".into()),
        (None, None, None) => {
            let (fb, palette) = front_end(&front);
            (fb, palette, "Xenobots".to_string())
        }
    };
    // The 320x240 mode has its own retrace rate (`docs/game-loop.md`, "Mode changes").
    let rate = Rate::for_height(fb.view().height);
    log::info!(
        "scale {scale:?}, aspect {aspect:?}, fullscreen {fullscreen}, view {:?} of a {:?} \
         surface, {:.3} retraces a second",
        fb.view(),
        fb.surface(),
        rate.hz()
    );
    let mut app = App {
        clock: Clock::new(rate, Duration::ZERO),
        start: None,
        input: Translator::default(),
        log: InputLog::default(),
        keys: Keys::default(),
        mouse: Mouse::default(),
        cursor_hidden: false,
        front,
        log_base: 0,
        sixty_hz: 0,
        footsteps: Footsteps::default(),
        meter: Meter::default(),
        fb,
        palette,
        viewer,
        cutscene: intro,
        drop_assets: drop,
        drop: playing,
        title,
        layout: None,
        scale,
        aspect,
        start_fullscreen: fullscreen,
        surface: None,
        size: PhysicalSize::new(0, 0),
        fullscreen: false,
        error: None,
        music,
        speed,
        legend: false,
        hd: Hd {
            shapes: hd_shapes,
            ui: hd_ui,
            pack: hd_pack,
        },
    };
    app.input.wasd = wasd;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    // The original leaves a goodbye to the player on the screen when it exits.
    if let FrontEnd::Screens(screens) = &app.front
        && let Some(message) = screens.exit_message()
    {
        println!("{message}");
    }
    app.error.map_or(Ok(()), Err)
}

struct App {
    clock: Clock,
    /// When the window opened: the clock's time 0. The clock starts then.
    start: Option<Instant>,
    input: Translator,
    /// Every retrace's input. The game reads input only from here, so a log replays.
    log: InputLog,
    /// The keys held, and the mouse, as the game sees them from the log.
    keys: Keys,
    mouse: Mouse,
    /// Whether the system cursor is hidden.
    cursor_hidden: bool,
    /// The screens, or the test card, after the intro and the drop.
    front: FrontEnd,
    /// Retraces run at earlier rates: the clock restarts its count at a mode change,
    /// the log does not.
    log_base: u64,
    /// The 60 Hz count at the last retrace, to tell a cutscene when it moves.
    sixty_hz: u64,
    /// Keeps the walking sound at 1x at a game speed (T-215).
    footsteps: Footsteps,
    meter: Meter,
    /// The picture to show.
    fb: Framebuffer,
    palette: Palette,
    /// `--view`: what to show instead of the test card.
    viewer: Option<Viewer>,
    /// The start-up cutscene, while it plays.
    /// The start-up cutscene, or one after a mission, while it plays.
    cutscene: Option<Cutscene>,
    /// The intro drop's files, until the cutscene ends, and the drop while it plays.
    drop_assets: Option<drop::Assets>,
    drop: Option<drop::Player>,
    title: String,
    /// The last layout drawn, to log changes.
    layout: Option<Layout>,
    scale: Scale,
    aspect: Aspect,
    /// `--fullscreen`.
    start_fullscreen: bool,
    /// Created on the first `resumed`, as winit 0.30 requires.
    surface: Option<Surface>,
    /// The window's inner size from the last `Resized`, in physical pixels: the size
    /// before a scale factor change, which `window.inner_size()` no longer reports.
    size: PhysicalSize<u32>,
    /// Whether the window was fullscreen at the last `Resized` (however it got there).
    fullscreen: bool,
    /// The error that stopped the event loop, if any.
    error: Option<Box<dyn Error>>,
    /// Ticked every real retrace; cued by the cutscenes and the screens.
    music: Jukebox,
    /// The game speed (T-215, one of [`SPEEDS`]): F12 cycles it, and it applies
    /// while a mission is in play.
    speed: u64,
    /// The key legend (T-217): Ctrl+F12 labels the view's widgets with their keys.
    legend: bool,
    /// What is drawn again at the window's resolution (`docs/hd.md`).
    hd: Hd,
}

/// Drawing at the window's resolution, over the recorded frame (`docs/hd.md`).
#[derive(Debug, Default)]
struct Hd {
    /// `--hd-shapes` (T-236): the frame's shapes.
    shapes: bool,
    /// `--hd-ui` (T-074): the frame's glyphs, the labels' too.
    ui: bool,
    /// `--hd-pack` (T-072): the pack's images, drawn over the recorded blits they name.
    pack: HashMap<AssetId, HdImage>,
}

impl Hd {
    /// The window's words for `fb`: `present`, or `compose` with a pack, then the shapes
    /// and the glyphs when those are on, so they stay over the pack's images as over the
    /// original's.
    fn draw(
        &self,
        fb: &Framebuffer,
        palette: &Palette,
        layout: &Layout,
        buffer: &mut [u32],
        size: Size,
    ) {
        xeno_render::compose(fb, palette, layout, &self.pack, buffer, size);
        if self.shapes {
            xeno_render::draw_shapes(fb, palette, layout, buffer, size);
        }
        if self.ui {
            xeno_render::draw_glyphs(fb, palette, layout, buffer, size);
        }
    }
}

/// Counts for the once-a-second rate log.
#[derive(Debug, Default)]
struct Meter {
    /// When the clock last started.
    since: Duration,
    /// When the last line was logged, and the counts then.
    at: Duration,
    retraces: u64,
    sixty_hz: u64,
    frames: u64,
    dropped: u64,
    /// Frames drawn so far, and the 60 Hz ticks.
    frames_now: u64,
    sixty_hz_now: u64,
}

/// The window's size when it opens (and when it leaves fullscreen). A fixed scale asks
/// for the picture's size in physical pixels. Otherwise 800x600 points: the 4:3 frame
/// at 5x6 on a 2x display, and 2x2 on a 1x one.
fn initial_size(view: Size, scale: Scale, aspect: Aspect) -> WindowSize {
    match scale {
        Scale::Auto => LogicalSize::new(800, 600).into(),
        Scale::Fixed(_) => {
            let layout = Layout::new(view, Size::new(0, 0), scale, aspect);
            physical(layout.picture(view)).into()
        }
    }
}

/// The inner size to ask for when the scale factor changes (say, the window moved to a
/// monitor with a different one), given the size before; `None` keeps the system's
/// suggestion. The suggestion keeps the window's logical size, which suits the default
/// window. A fixed-scale window keeps its physical size instead, so the picture still
/// fits it exactly. In fullscreen the monitor decides.
fn size_after_scale_change(
    scale: Scale,
    fullscreen: bool,
    before: PhysicalSize<u32>,
) -> Option<PhysicalSize<u32>> {
    let known = before.width > 0 && before.height > 0;
    (matches!(scale, Scale::Fixed(_)) && !fullscreen && known).then_some(before)
}

/// The front end's mode 13h framebuffer: blank for the screens, which draw at their
/// first retrace, or the test card.
fn front_end(front: &FrontEnd) -> (Framebuffer, Palette) {
    let mut fb = Framebuffer::new(Size::MODE_13H);
    match front {
        FrontEnd::Screens(screens) => (fb, screens.palette()),
        FrontEnd::TestCard(_) => {
            test_pattern::draw(&mut fb);
            (fb, test_pattern::palette())
        }
    }
}

fn physical(s: Size) -> PhysicalSize<u32> {
    PhysicalSize::new(s.width as u32, s.height as u32)
}

impl App {
    fn create_surface(&self, event_loop: &ActiveEventLoop) -> Result<Surface, Box<dyn Error>> {
        let view = self.fb.view();
        let attributes = Window::default_attributes()
            .with_title(self.full_title())
            .with_inner_size(initial_size(view, self.scale, self.aspect))
            .with_min_inner_size(physical(view))
            .with_fullscreen(
                self.start_fullscreen
                    .then_some(Fullscreen::Borderless(None)),
            );
        let window = Rc::new(event_loop.create_window(attributes)?);
        let context = softbuffer::Context::new(window.clone())?;
        Ok(softbuffer::Surface::new(&context, window)?)
    }

    /// Draws at the window's current size: the surface, the layout and the picture all
    /// use the one size read here, so a frame is never stretched or cropped by a stale
    /// buffer. Every resize and scale change asks for a new frame.
    fn redraw(&mut self) -> Result<(), Box<dyn Error>> {
        let pointer = self.mouse.position.filter(|_| self.pointer_shown());
        let speed = self.speed_now();
        let Some(surface) = &mut self.surface else {
            return Ok(());
        };
        let inner = surface.window().inner_size();
        let scale_factor = surface.window().scale_factor();
        let (Some(w), Some(h)) = (NonZeroU32::new(inner.width), NonZeroU32::new(inner.height))
        else {
            return Ok(()); // Minimised: nothing to draw.
        };
        surface.resize(w, h)?;
        let mut buffer = surface.buffer_mut()?;
        let window_size = Size::new(inner.width as usize, inner.height as usize);
        let layout = Layout::new(self.fb.view(), window_size, self.scale, self.aspect);
        if self.layout != Some(layout) {
            self.layout = Some(layout);
            log::debug!(
                "{:?} view in a {window_size:?} window: {layout:?}, picture {:?}",
                self.fb.view(),
                layout.picture(self.fb.view())
            );
        }
        self.input.set_layout(layout, self.fb.view());
        log::trace!(
            "redraw at {window_size:?} (scale factor {}, fullscreen {}): {layout:?}, picture {:?}",
            scale_factor,
            self.fullscreen,
            layout.picture(self.fb.view())
        );
        // The game speed's label (T-215) and the key legend (T-217), over a copy of the
        // picture for this frame.
        let saved = match &self.front {
            FrontEnd::Screens(screens) => screens.font(0).and_then(|font| {
                let labels = if self.legend {
                    screens.legend(self.input.wasd)
                } else {
                    Vec::new()
                };
                if speed == 1 && labels.is_empty() {
                    return None;
                }
                let saved = self.fb.clone();
                if speed > 1 {
                    draw_speed(&mut self.fb, &self.palette, font, speed);
                }
                draw_legend(&mut self.fb, &self.palette, font, &labels);
                Some(saved)
            }),
            _ => None,
        };
        // The game's pointer, if the screens show it (T-049), only over the picture's
        // pixels for this frame.
        let under = match (&self.front, pointer) {
            (FrontEnd::Screens(screens), Some(at)) => screens
                .pointer()
                .map(|shape| pointer::draw(&mut self.fb, at, shape)),
            _ => None,
        };
        (self.hd).draw(&self.fb, &self.palette, &layout, &mut buffer, window_size);
        if let Some(under) = under {
            under.restore(&mut self.fb);
        }
        if let Some(saved) = saved {
            self.fb = saved;
        }
        buffer.present()?;
        self.meter.frames_now += 1;
        Ok(())
    }

    /// One retrace: the game reads this retrace's input from the log, then a cutscene,
    /// the drop, the viewer or the screens act on it, or the test card's markers move.
    /// A screen's ESC claim is passed to the translator for the next key. Returns
    /// whether the mode changed (a cutscene began or ended, or the drop ended), so the
    /// clock must
    /// restart.
    fn tick(&mut self, step: Step) -> bool {
        let retrace = self.log_base + step.retrace;
        // A new picture (a mode change, the front end) starts unrecorded.
        if self.hd.shapes || self.hd.ui || !self.hd.pack.is_empty() {
            match self.fb.recording().map(|r| r.records().len()) {
                None => self.fb.set_recording(true),
                // shortcut: a screen that redraws without copying its picture again grows
                // the record; start it afresh before the frame's drawing past this many
                // (what was drawn becomes background, smoothed again once redrawn).
                Some(n) if n > 1 << 16 => self.fb.restart_recording(),
                _ => {}
            }
        }
        // The music keeps real time at the game speed (T-215): only real retraces tick it.
        if !step.extra {
            // The driver's hook runs first in the retrace interrupt, whatever the mode.
            self.music.tick();
        }
        // Every step takes input: the host's events are all drained by the first step
        // of a real retrace, so an extra step gets only the console arrows' own repeats,
        // which then count game steps and turn the robot as fast as it drives.
        self.log.record(retrace, self.input.take());
        for event in self.log.events_at(retrace) {
            log::debug!("retrace {retrace}: {event:?}");
            self.keys.apply(event);
            self.mouse.apply(event);
        }
        let ticked = step.sixty_hz != self.sixty_hz;
        self.sixty_hz = step.sixty_hz;
        self.meter.sixty_hz_now = step.sixty_hz;
        if let Some(cutscene) = &mut self.cutscene {
            let done = cutscene.retrace(self.log.events_at(retrace), ticked, &mut self.fb);
            self.palette = cutscene.palette();
            if let Some(cue) = cutscene.take_cue() {
                self.music.cue(cue);
            }
            if done {
                let (aborted, counter) = (cutscene.aborted(), cutscene.counter());
                let kind = cutscene.kind();
                self.cutscene = None;
                // After a mission: back to the screens, which go to the map room.
                let drop_assets = match kind {
                    Kind::Intro => self.drop_assets.take(),
                    _ => {
                        if let FrontEnd::Screens(screens) = &mut self.front {
                            screens.cutscene_over();
                        }
                        self.input.screen_claims_escape = false;
                        None
                    }
                };
                match drop_assets {
                    Some(assets) if !aborted => {
                        let player = drop::Player::new(assets, counter);
                        self.fb = player.framebuffer();
                        self.palette = player.palette();
                        self.drop = Some(player);
                    }
                    assets => {
                        if assets.is_some() {
                            log::debug!("intro aborted: the drop is skipped");
                        }
                        (self.fb, self.palette) = front_end(&self.front);
                    }
                }
                self.log_base = retrace;
            }
            return done;
        }
        if let Some(drop) = &mut self.drop {
            let done = drop.retrace(self.log.events_at(retrace), &mut self.fb);
            self.palette = drop.palette();
            if done {
                self.drop = None;
                (self.fb, self.palette) = front_end(&self.front);
                self.log_base = retrace;
            }
            return done;
        }
        if let Some(viewer) = &mut self.viewer {
            let changed = viewer.step(self.log.events_at(retrace), &self.keys);
            self.palette = viewer.shown_palette();
            if changed {
                viewer.draw(&mut self.fb);
                self.title = viewer.title();
                log::debug!("{}", self.title);
                self.set_title();
            }
            return false;
        }
        match &mut self.front {
            FrontEnd::Screens(screens) => {
                screens.extra_step = step.extra;
                screens.retrace(self.log.events_at(retrace), &mut self.fb);
                for sound in screens.take_sounds() {
                    match sound {
                        crate::mission::Sound::Play(event) if FOOTSTEPS.contains(&event) => {
                            if self.footsteps.hear(retrace, self.clock.speed()) {
                                self.music.effect(event);
                            }
                        }
                        crate::mission::Sound::Play(event) => self.music.effect(event),
                        crate::mission::Sound::Stop(event) => self.music.stop(event),
                        crate::mission::Sound::Beep(tone) => self.music.beep(tone),
                    }
                }
                self.palette = screens.palette();
                self.input.screen_claims_escape = screens.claims_escape();
                self.input.console_arrows = screens.console_open();
                let cutscene = screens.take_cutscene();
                if screens.take_changed() {
                    // `MAINTHEM` stops on entering the map room's loop (`1000:6f34`).
                    if screens.screen() == Screen::MapRoom {
                        self.music.cue(Cue::Stop);
                    }
                    self.title = format!("Xenobots - {}", screens.screen().label());
                    self.set_title();
                }
                // A cutscene after a mission: the 320x240 mode until it ends.
                if let Some(mut cutscene) = cutscene {
                    // An ending's song starts before the mode change.
                    if let Some(cue) = cutscene.take_cue() {
                        self.music.cue(cue);
                    }
                    self.fb = Cutscene::framebuffer();
                    self.palette = cutscene.palette();
                    self.cutscene = Some(cutscene);
                    self.log_base = retrace;
                    // Its keys are the game's: any key but P skips it, ESC included.
                    self.input.screen_claims_escape = true;
                    self.input.console_arrows = false;
                    return true;
                }
            }
            FrontEnd::TestCard(keymap) => test_pattern::draw_live(
                &mut self.fb,
                step.retrace,
                step.sixty_hz,
                &self.keys,
                keymap,
            ),
        }
        false
    }

    fn set_title(&self) {
        if let Some(surface) = &self.surface {
            surface.window().set_title(&self.full_title());
        }
    }

    /// The title with the port's settings that are on: the game speed (T-215), WASD
    /// (T-216).
    fn full_title(&self) -> String {
        let speed = match self.speed {
            1 => String::new(),
            n => format!(" - speed {n}x (F12)"),
        };
        let wasd = if self.input.wasd {
            " - WASD (Shift+F12)"
        } else {
            ""
        };
        format!("{}{speed}{wasd}", self.title)
    }

    /// The speed the clock should run at: the chosen one while a mission is in play,
    /// else 1 (the cutscenes, the drop and the other screens keep the original's).
    fn speed_now(&self) -> u64 {
        match &self.front {
            FrontEnd::Screens(screens) if self.pointer_shown() && screens.in_play() => self.speed,
            _ => 1,
        }
    }

    /// Restarts the clock if the speed it should run at changed; `ran` is the last
    /// retrace run, so the log's count goes on from it.
    fn speed_changed(&mut self, now: Duration, ran: u64) -> bool {
        if self.clock.speed() == self.speed_now() {
            return false;
        }
        self.log_base += ran;
        self.restart_clock(now);
        true
    }

    /// Whether the screens are up, which draw the game's pointer where they show it
    /// (the system cursor stays hidden over the picture even where they hide it, as
    /// the original shows no pointer there).
    fn pointer_shown(&self) -> bool {
        let screens = matches!(self.front, FrontEnd::Screens(_));
        screens && self.viewer.is_none() && self.cutscene.is_none() && self.drop.is_none()
    }

    /// Hides the system cursor in fullscreen (also entered by the system, such as
    /// macOS's green button) and over the picture while the game draws its pointer;
    /// shows it on the bars and outside, where the game's pointer stays at the edge.
    fn update_cursor(&mut self) {
        let hide = self.fullscreen || (self.pointer_shown() && self.input.cursor_on_picture());
        if hide != self.cursor_hidden
            && let Some(surface) = &self.surface
        {
            self.cursor_hidden = hide;
            surface.window().set_cursor_visible(!hide);
        }
    }

    /// Whether a screen asked to leave the game.
    fn quitting(&self) -> bool {
        matches!(&self.front, FrontEnd::Screens(s) if s.quit())
    }

    /// Restarts the clock at `now` at the rate of the current view's mode, as after the
    /// original's mode changes (`docs/game-loop.md`, "Mode changes").
    fn restart_clock(&mut self, now: Duration) {
        let rate = Rate::for_height(self.fb.view().height);
        let speed = self.speed_now();
        log::info!(
            "clock restarted at retrace {}: {:?} view, {:.3} retraces a second, speed {speed}x",
            self.log_base,
            self.fb.view(),
            rate.hz()
        );
        self.clock = Clock::new(rate, now).at_speed(speed);
        self.sixty_hz = 0;
        self.meter = Meter {
            since: now,
            at: now,
            frames: self.meter.frames_now,
            frames_now: self.meter.frames_now,
            ..Meter::default()
        };
        if let Some(surface) = &self.surface {
            surface
                .window()
                .set_min_inner_size(Some(physical(self.fb.view())));
            surface.window().request_redraw();
        }
    }

    /// Logs the measured rates at debug level, at most once a second.
    fn log_rates(&mut self, now: Duration) {
        let m = &mut self.meter;
        let span = (now - m.at).as_secs_f64();
        if span < 1.0 {
            return;
        }
        let (retraces, dropped) = (self.clock.retraces(), self.clock.dropped());
        log::debug!(
            "{:.3} retraces/s ({} in {span:.4} s; {:.4}/s since the clock started), \
             {:.3} 60 Hz ticks/s, {:.2} frames/s, {} retraces dropped",
            (retraces - m.retraces) as f64 / span,
            retraces - m.retraces,
            (retraces + dropped) as f64 / (now - m.since).as_secs_f64(),
            (m.sixty_hz_now - m.sixty_hz) as f64 / span,
            (m.frames_now - m.frames) as f64 / span,
            dropped - m.dropped,
        );
        (m.at, m.retraces, m.sixty_hz) = (now, retraces, m.sixty_hz_now);
        (m.frames, m.dropped) = (m.frames_now, dropped);
    }

    fn set_fullscreen(&self, on: bool) {
        if let Some(surface) = &self.surface {
            log::info!("fullscreen {on}");
            surface
                .window()
                .set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.surface
            .as_ref()
            .is_some_and(|s| s.window().fullscreen().is_some())
    }

    /// Notes a new inner size and fullscreen, and asks for a frame at that size.
    fn resized(&mut self, size: PhysicalSize<u32>) {
        log::debug!("resized to {size:?}");
        self.size = size;
        let Some(surface) = &self.surface else {
            return;
        };
        self.fullscreen = surface.window().fullscreen().is_some();
        surface.window().request_redraw();
        self.update_cursor();
    }

    /// Stops the event loop; `run` returns `error`.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: Box<dyn Error>) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_some() {
            return;
        }
        match self.create_surface(event_loop) {
            Ok(surface) => {
                let size = surface.window().inner_size();
                let scale_factor = surface.window().scale_factor();
                log::info!("window open at {size:?}, scale factor {scale_factor}");
                self.surface = Some(surface);
                self.start = Some(Instant::now());
                self.resized(size);
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    /// Runs the retraces due, asks for a frame if any ran, and sleeps until the next.
    /// This runs after every batch of events, the timer's wake-up included. Hidden or
    /// minimised, the window keeps the game running; only the drawing is skipped.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(start) = self.start else {
            return;
        };
        let now = start.elapsed();
        // F12 (or anything else) may have changed the speed since the last wake-up.
        self.speed_changed(now, self.clock.retraces());
        let mut ran = false;
        // At a game speed (T-215) a slow machine or a debug build would fall behind and
        // then catch up for many steps at once, holding the mouse and keys back; the
        // extra steps past half a retrace's time are dropped instead, so the game runs
        // slower than chosen but the input stays prompt. Real retraces always run.
        let budget = Rate::for_height(self.fb.view().height).time_of(1) / 2;
        for step in self.clock.advance(now) {
            if step.extra && start.elapsed() > now + budget {
                self.clock.drop_from(step.retrace, now);
                break;
            }
            ran = true;
            if self.tick(step) {
                self.restart_clock(now);
                break;
            }
            // Entering or leaving a mission in play changes it too; the rest of this
            // wake-up's steps at the old speed are dropped, as at a mode change.
            if self.speed_changed(now, step.retrace) {
                break;
            }
        }
        if self.quitting() {
            event_loop.exit();
            return;
        }
        if ran && let Some(surface) = &self.surface {
            surface.window().request_redraw();
        }
        self.log_rates(now);
        event_loop.set_control_flow(ControlFlow::WaitUntil(start + self.clock.next_due()));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match self.input.window_event(&event, self.fullscreen) {
            Some(WindowAction::Quit) => event_loop.exit(),
            Some(WindowAction::ToggleFullscreen) => self.set_fullscreen(!self.is_fullscreen()),
            Some(WindowAction::LeaveFullscreen) => self.set_fullscreen(false),
            Some(WindowAction::CycleSpeed) => {
                let at = SPEEDS.iter().position(|&s| s == self.speed).unwrap_or(0);
                self.speed = SPEEDS[(at + 1) % SPEEDS.len()];
                log::info!("game speed {}x (in a mission)", self.speed);
                self.set_title();
            }
            Some(WindowAction::ToggleWasd) => {
                self.input.wasd = !self.input.wasd;
                log::info!("WASD on the consoles {}", self.input.wasd);
                self.set_title();
            }
            Some(WindowAction::ToggleLegend) => {
                self.legend = !self.legend;
                log::info!("key legend {}", self.legend);
            }
            None => {}
        }
        match event {
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                let Some(surface) = &self.surface else {
                    return;
                };
                let window = surface.window();
                // The minimum was given in physical pixels, but some systems keep it
                // in points; give it again at the new factor.
                window.set_min_inner_size(Some(physical(self.fb.view())));
                // The suggestion isn't readable (`inner_size()` is the old or the new
                // size, depending on the system), so it is only overwritten.
                let keep =
                    size_after_scale_change(self.scale, window.fullscreen().is_some(), self.size);
                log::debug!(
                    "scale factor {scale_factor} at {:?}: asking for {keep:?} (None: the same logical size)",
                    self.size
                );
                if let Some(size) = keep
                    && let Err(e) = inner_size_writer.request_inner_size(size)
                {
                    log::warn!("could not keep the window at {size:?}: {e}");
                }
                window.request_redraw();
            }
            WindowEvent::Resized(size) => self.resized(size),
            WindowEvent::CursorMoved { .. } | WindowEvent::CursorLeft { .. } => {
                self.update_cursor();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, e);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footsteps_play_every_nth_run_at_a_speed() {
        // A scorpion's: runs of 3 retraces in a row, 5 apart.
        let posts: Vec<u64> = (0..8)
            .flat_map(|run| (0..3).map(move |i| run * 5 + i))
            .collect();
        for speed in SPEEDS {
            let mut footsteps = Footsteps::default();
            let played: Vec<u64> = posts
                .iter()
                .copied()
                .filter(|&r| footsteps.hear(r, speed))
                .collect();
            // Whole runs, every Nth: the first of them N runs in.
            let runs: Vec<u64> = (0..8).filter(|run| (run + 1) % speed == 0).collect();
            let want: Vec<u64> = runs
                .iter()
                .flat_map(|run| (0..3).map(move |i| run * 5 + i))
                .collect();
            assert_eq!(played, want, "{speed}x");
        }
        // A humanoid's: single retraces, all played at 1x.
        let mut footsteps = Footsteps::default();
        assert!([3, 9, 15].iter().all(|&r| footsteps.hear(r, 1)));
    }

    #[test]
    fn the_speed_label_sits_top_right_in_steady_colours() {
        let fonts = xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block());
        let font = &fonts.unwrap()[0];
        // The brightest and darkest colours are animated ones, which the label skips.
        let mut palette = Palette {
            colors: [Rgb {
                r: 30,
                g: 30,
                b: 30,
            }; 256],
        };
        palette.colors[20] = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        palette.colors[254] = Rgb { r: 0, g: 0, b: 0 };
        palette.colors[100] = Rgb {
            r: 200,
            g: 200,
            b: 200,
        };
        palette.colors[7] = Rgb { r: 5, g: 5, b: 5 };
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.pixels_mut().fill(50);
        draw_speed(&mut fb, &palette, font, 4);
        let width = usize::from(font.header.width) * 2 + 2;
        let (left, bottom) = (320 - width - 2, 2 + font.rows() + 2);
        for (i, &p) in fb.pixels().iter().enumerate() {
            let (x, y) = (i % 320, i / 320);
            let inside = (left..320 - 2).contains(&x) && (2..bottom).contains(&y);
            if inside {
                assert!(p == 7 || p == 100, "({x}, {y}): {p}");
            } else {
                assert_eq!(p, 50, "({x}, {y})");
            }
        }
        // The box's corner is the dark colour.
        assert_eq!(fb.pixels()[2 * 320 + left], 7);
    }

    #[test]
    fn default_window_is_logical_and_a_fixed_scale_is_physical() {
        let view = Size::MODE_13H;
        let size = |scale, aspect| initial_size(view, scale, aspect);
        let logical = LogicalSize::new(800, 600).into();
        assert_eq!(size(Scale::Auto, Aspect::Correct), logical);
        let fixed = |w: u32, h: u32| PhysicalSize::new(w, h).into();
        assert_eq!(size(Scale::Fixed(2), Aspect::Correct), fixed(640, 400));
        assert_eq!(size(Scale::Fixed(5), Aspect::Correct), fixed(1600, 1200));
        assert_eq!(size(Scale::Fixed(5), Aspect::Off), fixed(1600, 1000));
    }

    #[test]
    fn scale_change_keeps_a_fixed_scale_windows_pixels() {
        let p = PhysicalSize::new;
        // Moved between a 1x and a 2x display: only a fixed-scale window keeps its
        // pixels, and not in fullscreen or before its first size is known.
        let before = p(640, 400);
        let after = |scale, fullscreen| size_after_scale_change(scale, fullscreen, before);
        assert_eq!(after(Scale::Auto, false), None);
        assert_eq!(after(Scale::Fixed(2), false), Some(before));
        assert_eq!(after(Scale::Fixed(2), true), None);
        assert_eq!(after(Scale::Auto, true), None);
        assert_eq!(
            size_after_scale_change(Scale::Fixed(2), false, p(0, 0)),
            None
        );
    }

    /// An app with no window around `front`, as `run` builds it.
    fn test_app(front: FrontEnd) -> App {
        let (fb, palette) = front_end(&front);
        App {
            clock: Clock::new(Rate::MODE_13H, Duration::ZERO),
            start: None,
            input: Translator::default(),
            log: InputLog::default(),
            keys: Keys::default(),
            mouse: Mouse::default(),
            cursor_hidden: false,
            front,
            log_base: 0,
            sixty_hz: 0,
            footsteps: Footsteps::default(),
            meter: Meter::default(),
            fb,
            palette,
            viewer: None,
            cutscene: None,
            drop_assets: None,
            drop: None,
            title: String::new(),
            layout: None,
            scale: Scale::Auto,
            aspect: Aspect::Correct,
            start_fullscreen: false,
            surface: None,
            size: PhysicalSize::new(0, 0),
            fullscreen: false,
            error: None,
            music: Jukebox::default(),
            speed: 1,
            legend: false,
            hd: Hd::default(),
        }
    }

    /// T-072: a pack with one usable image replaces that model image's pixels with its
    /// texels and leaves every other output pixel `present`'s; with no pack and no
    /// flags the frame is `present`'s byte for byte. The pack is written here.
    #[test]
    fn an_hd_pack_replaces_one_model_image_and_leaves_the_rest() {
        use xeno_formats::mxx::{Frame, Image, Model, Part, Span, View, ViewHeader};
        use xeno_render::{LoadedModel, ModelScale, ModelTarget, Rect};
        // One view at facing 0 drawing two 2x2 images side by side.
        let square = |colour| Image {
            width: 2,
            height: 2,
            rows: vec![
                Span {
                    skip: 0,
                    pixels: vec![colour; 2],
                };
                2
            ],
        };
        let part = |image, x| Part { image, x, y: 0 };
        let view = View {
            header: ViewHeader {
                size: 0,
                facing: 0,
                mirror_facing: 0,
                index_offset: 0,
                mirror_index_offset: 0,
                lists_offset: 0,
                mirror_lists_offset: 0,
                data_offset: 0,
            },
            animations: vec![vec![Frame {
                depth: 0,
                parts: vec![part(0, 0), part(1, 2)],
            }]],
            mirror_animations: None,
            images: vec![square(0x31), square(0x52)],
        };
        let mut model =
            LoadedModel::new(&Model { views: Vec::new() }, &[Model { views: vec![view] }]);
        model.name = xeno_render::Name::new("synth");
        use crate::screens::{Screen, tests};
        let mut app = test_app(FrontEnd::Screens(Box::new(tests::new(Screen::MapRoom))));
        app.fb = Framebuffer::new(Size::new(8, 6));
        app.palette = Palette {
            colors: [Rgb { r: 0, g: 0, b: 9 }; Palette::LEN],
        };
        app.fb.set_recording(true);
        let target = ModelTarget {
            clip: Rect::new(0, 0, 8, 6),
            anchor: (1, 1),
            scale: ModelScale::new(256),
        };
        app.fb.draw_model(&model, 0, &[], &target);
        // A 2x pack: image 0 a red square with a clear corner; a missing image, a
        // wrong size and a stray file are skipped.
        let dir = std::env::temp_dir().join(format!("xeno-hd-pack-{}", std::process::id()));
        let images = dir.join("model/synth");
        std::fs::create_dir_all(&images).unwrap();
        let mut red = image::RgbaImage::from_pixel(4, 4, image::Rgba([0xff, 0, 0, 0xff]));
        red.put_pixel(3, 0, image::Rgba([0, 0xff, 0, 0x7f]));
        red.save(images.join("0_0.png")).unwrap();
        red.save(images.join("0_9.png")).unwrap();
        image::RgbaImage::new(3, 4)
            .save(images.join("0_1.png"))
            .unwrap();
        std::fs::write(dir.join("notes.txt"), "not an image").unwrap();
        let originals = model.image_sizes().into_iter().collect();
        let pack = crate::hd_pack::load(&dir, &originals).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(crate::hd_pack::load(&dir, &originals).is_err());
        assert_eq!(pack.len(), 1);
        let layout = Layout {
            sx: 2,
            sy: 2,
            x: 0,
            y: 0,
        };
        let size = Size::new(16, 12);
        let frame = |hd: &Hd| {
            let mut buffer = vec![0xdead_beef; 16 * 12];
            hd.draw(&app.fb, &app.palette, &layout, &mut buffer, size);
            buffer
        };
        let mut before = vec![0; 16 * 12];
        xeno_render::present(&app.fb, &app.palette, &layout, &mut before, size);
        assert!(frame(&Hd::default()) == before);
        let after = frame(&Hd {
            pack,
            ..Hd::default()
        });
        let mut replaced = 0;
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            let (ox, oy) = (i % 16, i / 16);
            // Image 0 covers logical (1, 1)..(3, 3): output (2, 2)..(6, 6).
            if (2..6).contains(&ox) && (2..6).contains(&oy) && (ox, oy) != (5, 2) {
                assert_eq!(now, 0xff_0000, "({ox}, {oy})");
                replaced += 1;
            } else {
                assert_eq!(now, was, "({ox}, {oy})");
            }
        }
        assert_eq!(replaced, 15);
    }

    #[test]
    fn a_cutscene_after_a_mission_switches_modes_and_returns_to_the_map_room() {
        use crate::screens::{Ending, Screen, tests};
        let mut screens = tests::new(Screen::Cutscene(Ending::Won));
        screens.set_endings(tests::endings());
        let mut app = test_app(FrontEnd::Screens(Box::new(screens)));
        let (mut n, mut changes) = (0, Vec::new());
        for r in 1..2_000u64 {
            n += 1;
            let step = Step {
                retrace: n,
                sixty_hz: n,
                extra: false,
            };
            if app.tick(step) {
                // As `about_to_wait` does: the clock restarts at the new mode's rate.
                app.restart_clock(Duration::from_millis(r));
                n = 0;
                changes.push((r, app.fb.view(), app.clock.rate()));
                assert_eq!(app.input.screen_claims_escape, app.cutscene.is_some());
            }
        }
        // The screens hand the cutscene over at their first retrace: the 320x240 mode
        // at its rate; when it ends, mode 13h at its rate, and the map room.
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert_eq!(
            (changes[0].0, changes[0].1, changes[0].2),
            (1, Size::MODE_240, Rate::MODE_240)
        );
        assert_eq!(
            (changes[1].1, changes[1].2),
            (Size::MODE_13H, Rate::MODE_13H)
        );
        assert!(changes[1].0 > 100);
        let FrontEnd::Screens(screens) = &app.front else {
            unreachable!()
        };
        assert_eq!(screens.screen(), Screen::MapRoom);
        assert!(app.cutscene.is_none() && app.pointer_shown());
    }

    #[test]
    fn the_title_shows_the_speed_and_wasd_when_on() {
        use crate::screens::{Screen, tests};
        let mut app = test_app(FrontEnd::Screens(Box::new(tests::new(Screen::MapRoom))));
        app.title = "Xenobots".into();
        assert_eq!(app.full_title(), "Xenobots");
        app.input.wasd = true;
        assert_eq!(app.full_title(), "Xenobots - WASD (Shift+F12)");
        app.speed = 2;
        assert_eq!(
            app.full_title(),
            "Xenobots - speed 2x (F12) - WASD (Shift+F12)"
        );
    }

    #[test]
    fn the_speed_applies_only_to_a_mission_in_play() {
        use crate::screens::{Ending, Screen, View, tests};
        let app = |screen| {
            let mut app = test_app(FrontEnd::Screens(Box::new(tests::new(screen))));
            app.speed = 4;
            app
        };
        let mut mission = app(Screen::Mission(View::SitRoom));
        assert_eq!(mission.speed_now(), 4);
        for screen in [
            Screen::MapRoom,
            Screen::Login,
            Screen::Cutscene(Ending::Won),
        ] {
            assert_eq!(app(screen).speed_now(), 1, "{screen:?}");
        }
        // The clock restarts at the speed once, and the log counts on from retrace 5.
        assert!(mission.speed_changed(Duration::from_secs(1), 5));
        assert_eq!((mission.clock.speed(), mission.log_base), (4, 5));
        assert!(!mission.speed_changed(Duration::from_secs(1), 0));
        // At 1x nothing restarts: the clock is today's.
        let mut once = app(Screen::Mission(View::SitRoom));
        once.speed = 1;
        assert!(!once.speed_changed(Duration::from_secs(1), 5));
        assert_eq!((once.clock.speed(), once.log_base), (1, 0));
    }

    /// Runs `app` for `n` retraces from its start; the retraces where the song cued
    /// changed, and where the mode did.
    type Changes = Vec<(u64, Option<&'static str>)>;
    fn run_app(app: &mut App, n: u64) -> (Changes, Vec<u64>) {
        let (mut cues, mut modes, mut playing, mut at) = (Vec::new(), Vec::new(), None, 0);
        for r in 1..n {
            at += 1;
            let changed = app.tick(Step {
                retrace: at,
                sixty_hz: at,
                extra: false,
            });
            if changed {
                app.restart_clock(Duration::from_millis(r));
                (at, _) = (0, modes.push(r));
            }
            if app.music.playing != playing {
                playing = app.music.playing;
                cues.push((r, playing));
            }
        }
        (cues, modes)
    }

    #[test]
    fn each_cutscene_after_a_mission_plays_its_song() {
        use crate::music::{BIG_WIN3, WIN1, WIN2};
        use crate::screens::{Ending, Screen, tests};
        for (ending, song, starts) in [
            (Ending::Won, WIN1, 1),
            (Ending::Lost, WIN2, 1),
            // At the cutscene's second retrace, after the mode change and the blank.
            (Ending::Triumph, BIG_WIN3, 3),
        ] {
            let mut screens = tests::new(Screen::Cutscene(ending));
            screens.set_endings(tests::endings());
            let mut app = test_app(FrontEnd::Screens(Box::new(screens)));
            let (cues, modes) = run_app(&mut app, 2_000);
            // Started with the cutscene, stopped before its last 15-tick wait.
            assert_eq!(modes.len(), 2, "{ending:?}");
            assert_eq!(cues.len(), 2, "{ending:?}: {cues:?}");
            assert_eq!(cues[0], (starts, Some(song)));
            assert_eq!(cues[1].1, None);
            assert!(
                (14..=16).contains(&(modes[1] - cues[1].0)),
                "{cues:?} {modes:?}"
            );
        }
    }

    #[test]
    fn the_main_theme_stops_on_entering_the_map_room() {
        use crate::music::MAIN_THEME;
        use crate::screens::{Screen, tests};
        for (screen, playing) in [(Screen::Login, Some(MAIN_THEME)), (Screen::MapRoom, None)] {
            let mut app = test_app(FrontEnd::Screens(Box::new(tests::new(screen))));
            app.music.cue(Cue::Start(MAIN_THEME));
            run_app(&mut app, 10);
            assert_eq!(app.music.playing, playing, "{screen:?}");
        }
    }
}
