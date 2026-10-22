//! The logical framebuffer: 8-bit indexed pixels at the original resolution, drawn
//! with the original rules, and turned into RGBA through a 256-colour palette.
//!
//! Nothing here opens a window, so everything can be tested headless; scaling the
//! view to a window's size is still to come (T-032). See
//! `docs/formats/video-modes.md` for the sizes and `docs/formats/b@n.md` for the
//! sprite drawing rules.

mod framebuffer;
mod sprite;

pub use framebuffer::{Framebuffer, Rect, RenderError, Size};
pub use sprite::{Anchor, BlitMode, Orientation, SpriteBlit};
pub use xeno_formats::{Palette, Rgb};
