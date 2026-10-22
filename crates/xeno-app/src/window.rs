//! The game window: `winit` for the window and events, `softbuffer` to put the
//! framebuffer on screen. For now it shows the test card, with markers driven by the
//! game clock, until ESC or close; or, with `--view`, one asset (`viewer`).
//!
//! The loop (`docs/game-loop.md`): the clock runs the retraces due, each one taking the
//! input events that arrived since the last; if any ran, a frame is requested; then the
//! loop sleeps until the next retrace is due (`ControlFlow::WaitUntil`).
//!
//! Sizes: the softbuffer surface and `xeno_render::Layout` work in physical pixels.
//! The default window asks for a logical size (so it looks the same on a 1x and a 2x
//! display); `--scale N` asks for the picture's physical size. Fullscreen is borderless
//! on the current monitor, so the display mode never changes; the scaler fits the
//! picture to whatever size that gives.

use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size as WindowSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};
use xeno_render::{Aspect, Framebuffer, Layout, Palette, Scale, Size};

use crate::clock::{Clock, Rate, Step};
use crate::input::{InputLog, Keys};
use crate::test_pattern;
use crate::translate::{Translator, WindowAction};
use crate::viewer::Viewer;

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// Opens the window on `viewer`, or the test card without one, and runs until the
/// user presses ESC or closes it.
pub fn run(
    scale: Scale,
    aspect: Aspect,
    fullscreen: bool,
    viewer: Option<Viewer>,
) -> Result<(), Box<dyn Error>> {
    let (fb, palette, title) = match &viewer {
        Some(v) => (v.framebuffer(), v.palette.clone(), v.title()),
        None => {
            let mut fb = Framebuffer::new(Size::MODE_13H);
            test_pattern::draw(&mut fb);
            (fb, test_pattern::palette(), "Xenobots".to_string())
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
        meter: Meter::default(),
        fb,
        palette,
        viewer,
        title,
        layout: None,
        scale,
        aspect,
        start_fullscreen: fullscreen,
        surface: None,
        size: PhysicalSize::new(0, 0),
        fullscreen: false,
        error: None,
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    clock: Clock,
    /// When the window opened: the clock's time 0. The clock starts then.
    start: Option<Instant>,
    input: Translator,
    /// Every retrace's input. The game reads input only from here, so a log replays.
    log: InputLog,
    /// The keys held, as the game sees them from the log.
    keys: Keys,
    meter: Meter,
    /// The picture to show.
    fb: Framebuffer,
    palette: Palette,
    /// `--view`: what to show instead of the test card.
    viewer: Option<Viewer>,
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
}

/// Counts for the once-a-second rate log.
#[derive(Debug, Default)]
struct Meter {
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

fn physical(s: Size) -> PhysicalSize<u32> {
    PhysicalSize::new(s.width as u32, s.height as u32)
}

impl App {
    fn create_surface(&self, event_loop: &ActiveEventLoop) -> Result<Surface, Box<dyn Error>> {
        let view = self.fb.view();
        let attributes = Window::default_attributes()
            .with_title(&self.title)
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
        log::trace!(
            "redraw at {window_size:?} (scale factor {}, fullscreen {}): {layout:?}, picture {:?}",
            scale_factor,
            self.fullscreen,
            layout.picture(self.fb.view())
        );
        xeno_render::present(&self.fb, &self.palette, &layout, &mut buffer, window_size);
        buffer.present()?;
        self.meter.frames_now += 1;
        Ok(())
    }

    /// One retrace: the game reads this retrace's input from the log, then the viewer
    /// acts on it, or the test card's markers move.
    fn tick(&mut self, step: Step) {
        self.log.record(step.retrace, self.input.take());
        for event in self.log.events_at(step.retrace) {
            log::debug!("retrace {}: {event:?}", step.retrace);
            self.keys.apply(event);
        }
        match &mut self.viewer {
            Some(viewer) => {
                if viewer.step(self.log.events_at(step.retrace), &self.keys) {
                    viewer.draw(&mut self.fb);
                    self.title = viewer.title();
                    log::debug!("{}", self.title);
                    if let Some(surface) = &self.surface {
                        surface.window().set_title(&self.title);
                    }
                }
            }
            None => test_pattern::draw_live(&mut self.fb, step.retrace, step.sixty_hz, &self.keys),
        }
        self.meter.sixty_hz_now = step.sixty_hz;
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
            "{:.3} retraces/s ({} in {span:.4} s; {:.4}/s since the window opened), \
             {:.3} 60 Hz ticks/s, {:.2} frames/s, {} retraces dropped",
            (retraces - m.retraces) as f64 / span,
            retraces - m.retraces,
            (retraces + dropped) as f64 / now.as_secs_f64(),
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

    /// Notes a new inner size, hides the cursor in fullscreen (also entered by the
    /// system, such as macOS's green button), and asks for a frame at that size.
    fn resized(&mut self, size: PhysicalSize<u32>) {
        log::debug!("resized to {size:?}");
        self.size = size;
        let Some(surface) = &self.surface else {
            return;
        };
        let window = surface.window();
        let fullscreen = window.fullscreen().is_some();
        if fullscreen != self.fullscreen {
            self.fullscreen = fullscreen;
            window.set_cursor_visible(!fullscreen);
        }
        window.request_redraw();
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
        let mut ran = false;
        for step in self.clock.advance(now) {
            self.tick(step);
            ran = true;
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
}
