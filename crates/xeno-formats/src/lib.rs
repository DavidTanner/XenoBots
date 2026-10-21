//! Parsers for the original Xenobots file formats: bytes in, structs out.
//!
//! This crate does no I/O and has no native dependencies, so everything in it can be
//! tested headless. Each format gets its own module as it is decoded; see
//! `docs/formats/` for what is known about each one.

pub mod ban;
mod error;
pub mod fgd;
pub mod files;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
pub mod mal;
pub mod mxx;
pub mod packed;
mod palette;
pub mod pcx;
pub mod scendef;
pub mod usg;
pub mod vcl;

pub use error::FormatError;
pub use palette::{Palette, Rgb};
