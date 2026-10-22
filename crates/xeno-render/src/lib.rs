//! The logical framebuffer: 8-bit indexed pixels at the original resolution, drawn
//! with the original rules, and turned into RGBA through a 256-colour palette.
//!
//! Nothing here opens a window, so everything can be tested headless; [`present`]
//! scales the view into a window's pixels. See `docs/formats/video-modes.md` for the
//! sizes and the presentation policy, and `docs/formats/b@n.md` for the sprite drawing
//! rules. [`Dac`] holds the palette as the original's video driver does, with fades.

pub mod dac;
mod framebuffer;
pub mod present;
mod sprite;

pub use dac::{DAC_BYTES, Dac, Fade};
pub use framebuffer::{Framebuffer, Rect, RenderError, Size};
pub use present::{Aspect, Layout, Scale, present};
pub use sprite::{Anchor, BlitMode, Orientation, SpriteBlit};
pub use xeno_formats::{Palette, Rgb};
