//! The logical framebuffer: 8-bit indexed pixels at the original resolution, drawn
//! with the original rules, and turned into RGBA through a 256-colour palette.
//!
//! Nothing here opens a window, so everything can be tested headless; [`present`]
//! scales the view into a window's pixels. See `docs/formats/video-modes.md` for the
//! sizes and the presentation policy, and `docs/formats/b@n.md` for the sprite drawing
//! rules. [`Dac`] holds the palette as the original's video driver does, with fades and
//! colour-animation records ([`anim`]). [`Framebuffer::draw_text`] draws the game's fonts,
//! and [`Framebuffer::blit_model_image`] the units' images, scaled (`docs/formats/mxx.md`).

pub mod anim;
pub mod dac;
mod framebuffer;
mod model;
pub mod present;
pub mod shape;
mod sprite;
mod text;
pub mod view3d;

pub use anim::ColourAnims;
pub use dac::{DAC_BYTES, Dac, Fade};
pub use framebuffer::{Framebuffer, Rect, RenderError, Size};
pub use model::{LoadedModel, ModelScale, ModelTarget, model_facing};
pub use present::{Aspect, Layout, Scale, ViewPoint, present};
pub use sprite::{Anchor, BlitMode, Orientation, SpriteBlit};
pub use text::{TextCursor, TextMode};
pub use xeno_formats::{Palette, Rgb};
