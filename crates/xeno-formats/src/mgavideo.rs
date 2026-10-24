//! What the port reads from the user's `mgavideo.dvr`: its own colour-animation list
//! (`docs/formats/palette.md`, "Reading them from the user's copy"), the fonts
//! (`docs/formats/font.md`) and the mouse pointer (`docs/screens.md`, "The pointer").
//! All are game data, read at run time and never stored in this repository.
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

/// The mouse pointer's shapes (its fourth block; the pointer routines read them
/// through `DS:0x626`).
pub const POINTERS: DataBlock = DataBlock {
    at: 0x3c,
    descriptor: 0x626,
};
/// Load-image offsets of the pointer set-up's last two calls (`gfx_init_pointer`):
/// `mov al, shape; call gfx_set_pointer_shape` and `mov al, colour; call
/// gfx_set_pointer_colour`, with their call displacements.
const POINTER_SHAPE_AT: usize = 0x2949;
const POINTER_SHAPE_CALL: [u8; 3] = [0xe8, 0x18, 0x00];
const POINTER_COLOUR_AT: usize = 0x294e;
const POINTER_COLOUR_CALL: [u8; 3] = [0xe8, 0x65, 0x00];
/// `gfx_set_pointer_shape` refuses shapes from this number on (`cmp al, 8`).
const POINTER_SHAPES_AT: usize = 0x2966;

/// The mouse pointer as the driver draws it (`gfx_draw_pointer_saving_under`): each
/// pixel whose mask byte is 0 is written as its image byte AND the pointer colour, the
/// rest are left alone; the hot spot is the pixel at the pointer's position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    pub width: u8,
    pub height: u8,
    pub hot_x: u8,
    pub hot_y: u8,
    /// `width * height` bytes, row by row.
    pub image: Vec<u8>,
    /// `width * height` bytes: 0 where the pointer is drawn.
    pub mask: Vec<u8>,
    pub colour: u8,
}

impl Pointer {
    /// The colour of pixel (`x`, `y`) of the shape, or `None` where it is transparent.
    pub fn pixel(&self, x: usize, y: usize) -> Option<u8> {
        if x >= usize::from(self.width) {
            return None;
        }
        let i = y * usize::from(self.width) + x;
        (*self.mask.get(i)? == 0).then(|| self.image[i] & self.colour)
    }
}

/// The `mov al, value` at `at`, if a call with `call`'s bytes follows it.
fn set_up_value(image: &[u8], at: usize, call: &[u8; 3]) -> Result<u8, FormatError> {
    match image.get(at..at + 5) {
        Some(&[0xb0, value, a, b, c]) if [a, b, c] == *call => Ok(value),
        _ => Err(invalid(at, "not the pointer set-up")),
    }
}

/// The pointer the driver sets up at start-up, by its number in [`pointers`].
pub fn pointer_shape(image: &[u8]) -> Result<u8, FormatError> {
    set_up_value(image, POINTER_SHAPE_AT, &POINTER_SHAPE_CALL)
}

/// Every pointer shape `gfx_set_pointer_shape` accepts (it refuses numbers from the
/// count in its `cmp al, count` on), by number, in the colour the set-up picks. The
/// block of shapes holds a word offset per shape, then each shape's width, height,
/// hot spot `x` and `y` bytes and the offsets of its image and mask.
pub fn pointers(image: &[u8]) -> Result<Vec<Pointer>, FormatError> {
    let colour = set_up_value(image, POINTER_COLOUR_AT, &POINTER_COLOUR_CALL)?;
    let shapes = match image.get(POINTER_SHAPES_AT..POINTER_SHAPES_AT + 2) {
        Some(&[0x3c, count]) => count,
        _ => return Err(invalid(POINTER_SHAPES_AT, "not the pointer shape check")),
    };
    let block = data_block(image, POINTERS)?;
    let word = |at: usize| {
        block
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
            .ok_or_else(|| invalid(at, "past the end of the pointer block"))
    };
    (0..usize::from(shapes))
        .map(|shape| {
            let at = word(2 * shape)?;
            let head = block
                .get(at..at + 4)
                .ok_or_else(|| invalid(at, "the pointer shape is past its block"))?;
            let (width, height, hot_x, hot_y) = (head[0], head[1], head[2], head[3]);
            let size = usize::from(width) * usize::from(height);
            let plane = |offset: usize| {
                block
                    .get(offset..offset + size)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| invalid(offset, "the pointer's pixels are past its block"))
            };
            Ok(Pointer {
                width,
                height,
                hot_x,
                hot_y,
                image: plane(word(at + 4)?)?,
                mask: plane(word(at + 6)?)?,
                colour,
            })
        })
        .collect()
}

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
    /// The mouse pointer's shapes, by number (`docs/screens.md`, "The pointer").
    pub pointers: Vec<Pointer>,
    /// The shape the driver sets up at start-up.
    pub pointer: u8,
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

    /// The shape the driver sets up at start-up.
    pub fn set_up_pointer(&self) -> &Pointer {
        &self.pointers[usize::from(self.pointer)]
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
    let pointers = pointers(image)?;
    let pointer = pointer_shape(image)?;
    if usize::from(pointer) >= pointers.len() {
        return Err(invalid(
            POINTER_SHAPE_AT,
            "the pointer shape is out of range",
        ));
    }
    Ok(Mgavideo {
        anims,
        fonts,
        pointers,
        pointer,
    })
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
    fn the_synthetic_pointer_reads() {
        let video = parse(&synthetic_mgavideo()).unwrap();
        assert_eq!((video.pointers.len(), video.pointer), (2, 1));
        let pointer = video.set_up_pointer();
        assert_eq!(
            (pointer.width, pointer.height, pointer.hot_x, pointer.hot_y),
            (3, 2, 1, 0)
        );
        let rows: Vec<Vec<_>> = (0..2)
            .map(|y| (0..4).map(|x| pointer.pixel(x, y)).collect())
            .collect();
        assert_eq!(
            rows,
            [
                [Some(0x3c), Some(0x0c), None, None],
                [Some(0), Some(0x3c), None, None]
            ]
        );
        assert_eq!(pointer.pixel(0, 2), None);
    }

    #[test]
    fn another_pointer_set_up_is_refused() {
        let file = synthetic_mgavideo();
        let corrupt = |at: usize, value: u8| {
            let mut bad = file.clone();
            bad[0x20 + at] = value;
            parse(&bad)
        };
        // Not the set-up calls, a shape past the count, a shape past its block.
        assert!(corrupt(POINTER_SHAPE_AT, 0xb4).is_err());
        assert!(corrupt(POINTER_COLOUR_AT + 3, 0x66).is_err());
        assert!(corrupt(POINTER_SHAPES_AT, 0x3d).is_err());
        assert!(corrupt(POINTER_SHAPE_AT + 1, 2).is_err());
        assert!(corrupt(0x180 + 2, 0x40).is_err());
        assert!(corrupt(0x180 + 8, 0x20).is_err());
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
        // An arrow 9 by 11 with its hot spot one pixel in (`docs/screens.md`).
        let pointer = video.set_up_pointer();
        assert_eq!(
            (pointer.width, pointer.height, pointer.hot_x, pointer.hot_y),
            (9, 11, 1, 1)
        );
        // Eight shapes, each drawing something; the set-up's is the arrow, 0; the
        // busy (1) and corner (2) shapes the host picks are among them (T-049).
        assert_eq!((video.pointers.len(), video.pointer), (8, 0));
        for shape in &video.pointers {
            assert!(shape.mask.contains(&0));
        }
    }
}
