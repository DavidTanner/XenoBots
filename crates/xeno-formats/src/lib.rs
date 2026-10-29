//! Parsers for the original Xenobots file formats: bytes in, structs out.
//!
//! This crate does no I/O and has no native dependencies, so everything in it can be
//! tested headless. Each format gets its own module as it is decoded; see
//! `docs/formats/` for what is known about each one.

pub mod ban;
pub mod campaign;
pub mod damage;
pub mod dsounds;
pub mod editor;
pub mod ending;
mod error;
pub mod farewell;
pub mod fgd;
pub mod filedialog;
pub mod files;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
pub mod font;
pub mod lzexe;
pub mod mal;
pub mod map;
pub mod mapdvr;
pub mod maproom;
pub mod mgavideo;
pub mod motion;
pub mod ms3;
pub mod mxx;
pub mod net;
pub mod orders;
pub mod packed;
mod palette;
pub mod pcx;
pub mod review;
pub mod rst;
pub mod scendef;
pub mod script;
pub mod sensing;
pub mod sequence;
pub mod setup;
pub mod sitroom;
pub mod sounddvr;
pub mod sounds;
pub mod usg;
pub mod vcl;
pub mod vgaintro;
pub mod view3d;
pub mod views;

pub use error::FormatError;
pub use palette::{Palette, Rgb};
