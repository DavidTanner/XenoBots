//! The game window: `winit` for the window and events, `softbuffer` to put the
//! framebuffer on screen. For now it shows the test card until ESC or close.
//!
//! Sizes: the softbuffer surface and `xeno_render::Layout` work in physical pixels.
//! The default window asks for a logical size (so it looks the same on a 1x and a 2x
//! display); `--scale N` asks for the picture's physical size. Fullscreen is borderless
//! on the current monitor, so the display mode never changes; the scaler fits the
//! picture to whatever size that gives.

use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize, Size as WindowSize};
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};
use xeno_render::{Aspect, Framebuffer, Layout, Palette, Scale, Size};

use crate::test_pattern;

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// Opens the window and runs until the user presses ESC or closes it.
pub fn run(scale: Scale, aspect: Aspect, fullscreen: bool) -> Result<(), Box<dyn Error>> {
    let mut fb = Framebuffer::new(Size::MODE_13H);
    test_pattern::draw(&mut fb);
    log::info!("scale {scale:?}, aspect {aspect:?}, fullscreen {fullscreen}");
    let mut app = App {
        fb,
        palette: test_pattern::palette(),
        scale,
        aspect,
        start_fullscreen: fullscreen,
        surface: None,
        size: PhysicalSize::new(0, 0),
        fullscreen: false,
        alt: false,
        error: None,
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    /// The picture to show.
    fb: Framebuffer,
    palette: Palette,
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
    /// Whether Alt (Option on macOS) is held.
    alt: bool,
    /// The error that stopped the event loop, if any.
    error: Option<Box<dyn Error>>,
}

/// What a key press asks for.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Quit,
    ToggleFullscreen,
}

/// The action for a key event: ESC quits (in fullscreen too), F11 or Alt+Enter toggles
/// fullscreen. Held keys don't repeat a toggle.
fn key_action(key: &Key, pressed: bool, repeat: bool, alt: bool) -> Option<Action> {
    match key {
        _ if !pressed => None,
        Key::Named(NamedKey::Escape) => Some(Action::Quit),
        _ if repeat => None,
        Key::Named(NamedKey::F11) => Some(Action::ToggleFullscreen),
        Key::Named(NamedKey::Enter) if alt => Some(Action::ToggleFullscreen),
        _ => None,
    }
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
            .with_title("Xenobots")
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
        log::debug!(
            "redraw at {window_size:?} (scale factor {}, fullscreen {}): {layout:?}, picture {:?}",
            scale_factor,
            self.fullscreen,
            layout.picture(self.fb.view())
        );
        xeno_render::present(&self.fb, &self.palette, &layout, &mut buffer, window_size);
        buffer.present()?;
        Ok(())
    }

    fn toggle_fullscreen(&self) {
        if let Some(surface) = &self.surface {
            let window = surface.window();
            let next = match window.fullscreen() {
                Some(_) => None,
                None => Some(Fullscreen::Borderless(None)),
            };
            log::info!("fullscreen {}", next.is_some());
            window.set_fullscreen(next);
        }
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
                self.resized(size);
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => self.alt = modifiers.state().alt_key(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state,
                        repeat,
                        ..
                    },
                ..
            } => match key_action(&logical_key, state.is_pressed(), repeat, self.alt) {
                Some(Action::Quit) => event_loop.exit(),
                Some(Action::ToggleFullscreen) => self.toggle_fullscreen(),
                None => {}
            },
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
    fn keys_quit_or_toggle_fullscreen() {
        let named = |k| Key::Named(k);
        let f11 = named(NamedKey::F11);
        let enter = named(NamedKey::Enter);
        let esc = named(NamedKey::Escape);
        assert_eq!(key_action(&esc, true, false, false), Some(Action::Quit));
        assert_eq!(key_action(&esc, true, true, true), Some(Action::Quit));
        assert_eq!(key_action(&esc, false, false, false), None);
        let toggle = Some(Action::ToggleFullscreen);
        assert_eq!(key_action(&f11, true, false, false), toggle);
        assert_eq!(key_action(&enter, true, false, true), toggle);
        assert_eq!(key_action(&enter, true, false, false), None);
        assert_eq!(key_action(&f11, true, true, false), None);
        assert_eq!(key_action(&f11, false, false, false), None);
        assert_eq!(
            key_action(&Key::Character("f".into()), true, false, true),
            None
        );
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
}
