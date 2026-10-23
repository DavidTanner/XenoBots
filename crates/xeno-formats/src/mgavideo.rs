//! What the port reads from the user's `mgavideo.dvr`: its own colour-animation list
//! (`docs/formats/palette.md`, "Reading them from the user's copy") and the fonts
//! (`docs/formats/font.md`). Both are game data, read at run time and never stored in
//! this repository.
//!
//! The driver's init (load offset 0x0a) copies five data blocks into memory blocks of
//! their own, each with the same five instructions: `mov ax, size; mov bx, descriptor;
//! xor si, si; mov dx, paragraph; call far [0x36]`. Each block is read from the
//! paragraph and size in its instructions, after checking every byte of them, so
//! another build of the driver is refused rather than misread.

use crate::{FormatError, font, vgaintro};

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// Where the init copies one data block: the load-image offset of its `mov ax, size`
/// and the host descriptor it fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataBlock {
    pub at: usize,
    pub descriptor: u16,
}

/// The driver's colour-animation list (its third block; `pal_install_anim_slots`
/// selects it at start-up).
pub const ANIMS: DataBlock = DataBlock {
    at: 0x28,
    descriptor: 0x622,
};
/// The fonts (its fifth block; the text services read them through `DS:0x62a`).
pub const FONTS: DataBlock = DataBlock {
    at: 0x50,
    descriptor: 0x62a,
};

/// The bytes of `block` in `image`, from the instructions that copy it.
pub fn data_block(image: &[u8], block: DataBlock) -> Result<&[u8], FormatError> {
    let at = block.at;
    let op = image
        .get(at..at + 15)
        .ok_or_else(|| invalid(at, "the init is past the end of the image"))?;
    let word = |i: usize| usize::from(u16::from_le_bytes([op[i], op[i + 1]]));
    let expected = op[0] == 0xb8
        && op[3] == 0xbb
        && word(4) == usize::from(block.descriptor)
        && op[6..9] == [0x33, 0xf6, 0xba]
        && op[11..15] == [0xff, 0x1e, 0x36, 0x00];
    if !expected {
        return Err(invalid(
            at,
            format!("not the copy of data block {:#x}", block.descriptor),
        ));
    }
    let start = word(9) * 16;
    image
        .get(start..start + word(1))
        .ok_or_else(|| invalid(at, "the data block is past the end of the image"))
}

/// What the port reads from `mgavideo.dvr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mgavideo {
    /// The colour-animation list, up to and including its end marker, for
    /// `xeno_render::ColourAnims`.
    pub anims: Vec<u8>,
    /// Fonts 0, 2, 4 and 6, by `id / 2`.
    pub fonts: Vec<font::Font>,
}

impl Mgavideo {
    /// How many records the colour-animation list holds.
    pub fn anim_records(&self) -> usize {
        let mut at = 0;
        let mut records = 0;
        while let Some(size) = self.anims.get(at..at + 2) {
            match usize::from(u16::from_le_bytes([size[0], size[1]])) {
                0 => break,
                size => at += size,
            }
            records += 1;
        }
        records
    }

    /// The font the host selects with `id` (0, 2, 4 or 6); `None` for any other id
    /// (the host falls back to font 0 and returns carry).
    pub fn font(&self, id: u16) -> Option<&font::Font> {
        if id.is_multiple_of(2) {
            self.fonts.get(usize::from(id / 2))
        } else {
            None
        }
    }
}

/// Reads the colour-animation list and the fonts from the bytes of `mgavideo.dvr`.
pub fn parse(bytes: &[u8]) -> Result<Mgavideo, FormatError> {
    let image = vgaintro::load_image(bytes)?;
    let anims = data_block(image, ANIMS)?;
    let anims = vgaintro::anim_list(anims, 0)?;
    let fonts = font::parse(data_block(image, FONTS)?)?;
    Ok(Mgavideo { anims, fonts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_font_block, synthetic_mgavideo};

    #[test]
    fn synthetic_driver_parses() {
        let video = parse(&synthetic_mgavideo()).unwrap();
        assert_eq!(video.anims.len(), 16 + 2);
        assert_eq!(video.anim_records(), 1);
        assert_eq!(video.fonts, font::parse(&synthetic_font_block()).unwrap());
        assert_eq!(video.font(6), Some(&video.fonts[3]));
        assert_eq!(video.font(3), None);
        assert_eq!(video.font(8), None);
    }

    #[test]
    fn another_build_is_refused() {
        let file = synthetic_mgavideo();
        // Each byte of the font block's instructions but the size and paragraph.
        for i in [0, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14] {
            let mut bad = file.clone();
            bad[0x20 + FONTS.at + i] ^= 0x40;
            assert!(parse(&bad).is_err(), "byte {i}");
        }
        // A block reaching past the image.
        let mut bad = file.clone();
        bad[0x20 + FONTS.at + 2] = 0x7f;
        assert!(parse(&bad).is_err());
        assert!(parse(b"MZ").is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_mgavideo_fonts_parse() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let video = parse(&std::fs::read(dir.join("mgavideo.dvr")).unwrap()).unwrap();
        // Structure only (`font.md`): four fonts, two of each style; every printable
        // upper-case letter and digit has a glyph in each; the caption font 6 is shaded.
        assert_eq!(video.fonts.len(), font::FONT_COUNT);
        for font in &video.fonts {
            assert!(font.glyphs.len() > usize::from(b'Z'));
            for code in (b'0'..=b'9').chain(b'A'..=b'Z') {
                let glyph = font.glyph(code).unwrap();
                assert!(glyph.rows.iter().any(|&b| b != 0), "{}", code as char);
            }
        }
        assert_eq!(video.font(6).unwrap().style(), font::Style::Shaded);
        assert_eq!(video.font(0).unwrap().style(), font::Style::Bitmap);
        // The list has 24 records (`palette.md`).
        assert_eq!(video.anim_records(), 24);
    }
}
