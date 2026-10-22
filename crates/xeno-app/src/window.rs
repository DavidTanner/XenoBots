//! The game window: `winit` for the window and events, `softbuffer` to put the
//! framebuffer on screen. For now it shows the test card until ESC or close.

use std::error::Error;
use std::num::NonZeroU32;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};
use xeno_render::{Framebuffer, Size};

use crate::{present, test_pattern};

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// Opens the window and runs until the user presses ESC or closes it.
pub fn run() -> Result<(), Box<dyn Error>> {
    let mut fb = Framebuffer::new(Size::MODE_13H);
    test_pattern::draw(&mut fb);
    let mut app = App {
        size: fb.view(),
        rgba: fb.to_rgba(&test_pattern::palette()),
        surface: None,
        error: None,
    };
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    /// The picture to show, as `Framebuffer::to_rgba` made it.
    rgba: Vec<u8>,
    size: Size,
    /// Created on the first `resumed`, as winit 0.30 requires.
    surface: Option<Surface>,
    /// The error that stopped the event loop, if any.
    error: Option<Box<dyn Error>>,
}

impl App {
    fn create_surface(&self, event_loop: &ActiveEventLoop) -> Result<Surface, Box<dyn Error>> {
        let (w, h) = (self.size.width as u32, self.size.height as u32);
        let attributes = Window::default_attributes()
            .with_title("Xenobots")
            .with_inner_size(LogicalSize::new(w * 2, h * 2))
            .with_min_inner_size(LogicalSize::new(w, h));
        let window = Rc::new(event_loop.create_window(attributes)?);
        let context = softbuffer::Context::new(window.clone())?;
        Ok(softbuffer::Surface::new(&context, window)?)
    }

    fn redraw(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(surface) = &mut self.surface else {
            return Ok(());
        };
        let inner = surface.window().inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(inner.width), NonZeroU32::new(inner.height))
        else {
            return Ok(()); // Minimised: nothing to draw.
        };
        surface.resize(w, h)?;
        let mut buffer = surface.buffer_mut()?;
        let window_size = Size::new(inner.width as usize, inner.height as usize);
        log::debug!(
            "redraw at {window_size:?}, {:?}",
            present::Layout::fit(self.size, window_size)
        );
        present::present(&self.rgba, self.size, &mut buffer, window_size);
        buffer.present()?;
        Ok(())
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
                log::info!("window open at {:?}", surface.window().inner_size());
                self.surface = Some(surface);
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Some(surface) = &self.surface {
                    surface.window().request_redraw();
                }
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
