//! The game's four fonts, kept in `mgavideo.dvr`'s fifth data block (descriptor
//! `DS:0x62a`) and drawn by the driver's glyph renderers. See `docs/formats/font.md`.
//!
//! The block starts with a table of four words, the offsets of the fonts the host's
//! font select (slot `0x38a`) calls 0, 2, 4 and 6. A font is a six-byte header, then a
//! table of glyph offsets indexed by character code (bit 7 masked off), then the
//! glyphs. A glyph's first byte is 0 for a control code; otherwise it is the glyph's
//! width and the rows follow. The fonts are the user's to read; this module only knows
//! their shape.

use binrw::BinRead;

use crate::FormatError;

/// How many fonts the block's table holds; the host refuses a font id of 8 or more.
pub const FONT_COUNT: usize = 4;
/// Character codes index the glyph table with bit 7 masked off.
pub const MAX_GLYPHS: usize = 128;

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// A font's six-byte header, copied into the host's text state by the font select.
#[derive(BinRead, Debug, Clone, Copy, PartialEq, Eq)]
#[br(little)]
pub struct FontHeader {
    /// Picks the glyph renderer: 0 and 1 [`Style::Shaded`], 7 [`Style::Bitmap`]; 2 to 6
    /// draw nothing. Text state `+0x08`.
    pub kind: u8,
    /// Not copied anywhere; 0 in every stock font.
    pub unknown_01: u8,
    /// The cell width: a bitmap font's advance, and what a backspace moves back. Text
    /// state `+0x0a`.
    pub width: u8,
    /// The line height. Glyphs have `height - 1` rows. Text state `+0x0c`.
    pub height: u8,
    /// Extra pixels after every glyph (text state `+0x12`).
    pub step_x: u8,
    /// Pixels every glyph moves the cursor down (text state `+0x14`).
    pub step_y: u8,
}

/// How a font's glyphs are stored and drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// One byte a pixel, `width` bytes a row: 0 transparent, 1..=3 a shade drawn as
    /// `colour + (value ^ 3)`, so 3 is the colour itself. Proportional: a glyph
    /// advances its own width plus one.
    Shaded,
    /// One byte a row, the leftmost pixel in bit 7, set bits in the colour. Fixed:
    /// every glyph advances the header's width.
    Bitmap,
}

/// One glyph's bytes as the font holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    /// The first byte: a shaded glyph's width in pixels. A bitmap font's glyphs carry a
    /// non-zero value here too (the cell's width in the stock fonts), which only the
    /// host's text measuring reads.
    pub width: u8,
    /// `height - 1` rows, top first: one byte each for a bitmap font, `width` bytes
    /// each for a shaded one.
    pub rows: Vec<u8>,
}

/// One font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub header: FontHeader,
    /// By character code with bit 7 masked off. `None` is a control code (a glyph
    /// whose first byte is 0). Codes at or past the table's end have no entry: the
    /// original would read the glyph data as offsets.
    pub glyphs: Vec<Option<Glyph>>,
}

impl Font {
    /// The style the header's kind selects.
    pub fn style(&self) -> Style {
        style_of(self.header.kind).unwrap_or(Style::Bitmap)
    }

    /// The glyph for `code` (bit 7 ignored), or `None` for a control code or a code
    /// past the table.
    pub fn glyph(&self, code: u8) -> Option<&Glyph> {
        self.glyphs.get(usize::from(code & 0x7f))?.as_ref()
    }

    /// Rows a glyph has.
    pub fn rows(&self) -> usize {
        usize::from(self.header.height) - 1
    }

    /// Value of pixel (`x`, `y`) of `glyph`: for a bitmap font 1 if the bit is set,
    /// for a shaded font the stored byte; 0 off the glyph.
    pub fn pixel(&self, glyph: &Glyph, x: usize, y: usize) -> u8 {
        match self.style() {
            Style::Bitmap => match glyph.rows.get(y) {
                Some(&row) if x < 8 => (row << x) >> 7,
                _ => 0,
            },
            Style::Shaded => {
                let width = usize::from(glyph.width);
                if x < width {
                    glyph.rows.get(y * width + x).copied().unwrap_or(0)
                } else {
                    0
                }
            }
        }
    }
}

fn style_of(kind: u8) -> Option<Style> {
    match kind {
        0 | 1 => Some(Style::Shaded),
        7 => Some(Style::Bitmap),
        _ => None,
    }
}

fn word(block: &[u8], at: usize) -> Result<usize, FormatError> {
    block
        .get(at..at + 2)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
        .ok_or_else(|| invalid(at, "past the end of the font block"))
}

/// Reads the font at `start` in `block`.
pub fn parse_font(block: &[u8], start: usize) -> Result<Font, FormatError> {
    let header_bytes = block
        .get(start..start + 6)
        .ok_or_else(|| invalid(start, "font header past the end of the block"))?;
    let header = FontHeader::read(&mut std::io::Cursor::new(header_bytes))?;
    let style = style_of(header.kind)
        .ok_or_else(|| invalid(start, format!("font kind {} draws nothing", header.kind)))?;
    // A height of 0 or 1 would make the renderers loop 65,536 rows.
    if header.height < 2 {
        return Err(invalid(start + 3, "a font needs at least two rows"));
    }
    if style == Style::Bitmap && !(1..=8).contains(&header.width) {
        return Err(invalid(start + 2, "a bitmap font is 1 to 8 pixels wide"));
    }
    let rows = usize::from(header.height) - 1;
    let table = start + 6;
    // The table ends where the first glyph starts.
    let mut end = table + 2 * MAX_GLYPHS;
    let mut at = table;
    while at < end {
        let glyph = word(block, at)?;
        if glyph < at + 2 {
            return Err(invalid(at, "a glyph offset inside the font's own table"));
        }
        end = end.min(glyph);
        at += 2;
    }
    let glyphs = (table..end)
        .step_by(2)
        .map(|at| {
            let glyph = word(block, at)?;
            let width = *block
                .get(glyph)
                .ok_or_else(|| invalid(at, "glyph past the end of the block"))?;
            if width == 0 {
                return Ok(None);
            }
            let size = match style {
                Style::Bitmap => rows,
                Style::Shaded => rows * usize::from(width),
            };
            let bytes = block
                .get(glyph + 1..glyph + 1 + size)
                .ok_or_else(|| invalid(glyph, "glyph past the end of the block"))?;
            Ok(Some(Glyph {
                width,
                rows: bytes.to_vec(),
            }))
        })
        .collect::<Result<_, FormatError>>()?;
    Ok(Font { header, glyphs })
}

/// Reads all four fonts from the font block (`mgavideo.dvr`'s data block
/// `DS:0x62a`; [`crate::mgavideo`] finds it).
pub fn parse(block: &[u8]) -> Result<Vec<Font>, FormatError> {
    (0..FONT_COUNT)
        .map(|i| parse_font(block, word(block, 2 * i)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_font_block;

    #[test]
    fn synthetic_fonts_parse() {
        let fonts = parse(&synthetic_font_block()).unwrap();
        assert_eq!(fonts.len(), FONT_COUNT);
        let bitmap = &fonts[0];
        assert_eq!(bitmap.style(), Style::Bitmap);
        assert_eq!(bitmap.glyphs.len(), 128);
        // Control codes, then 'A' (two rows: a bar, then the two outer pixels).
        assert!(bitmap.glyph(b'\n').is_none());
        let a = bitmap.glyph(b'A').unwrap();
        assert_eq!(bitmap.pixel(a, 0, 0), 1);
        assert_eq!(bitmap.pixel(a, 1, 1), 0);
        assert_eq!(bitmap.pixel(a, 2, 1), 1);
        // Bit 7 picks another colour, not another glyph.
        assert_eq!(bitmap.glyph(b'A' | 0x80), Some(a));
        let shaded = &fonts[3];
        assert_eq!(shaded.style(), Style::Shaded);
        // A shorter table: codes past it have no glyph.
        assert_eq!(shaded.glyphs.len(), 0x60);
        assert!(shaded.glyph(b'a').is_none());
        let b = shaded.glyph(b'B').unwrap();
        assert_eq!(b.width, 2);
        assert_eq!(shaded.pixel(b, 0, 0), 3);
        assert_eq!(shaded.pixel(b, 1, 0), 1);
        assert_eq!(shaded.pixel(b, 2, 0), 0);
    }

    #[test]
    fn fonts_snapshot() {
        let fonts = parse(&synthetic_font_block()).unwrap();
        // The headers and the glyphs that differ from the blank one.
        let summary: Vec<_> = fonts
            .iter()
            .map(|f| {
                let glyphs: Vec<_> = f
                    .glyphs
                    .iter()
                    .enumerate()
                    .filter_map(|(c, g)| Some((c, g.as_ref()?)))
                    .filter(|(_, g)| g.rows.iter().any(|&b| b != 0))
                    .collect();
                (f.header, f.glyphs.len(), glyphs)
            })
            .collect();
        insta::assert_debug_snapshot!(summary);
    }

    #[test]
    fn bad_fonts_are_refused() {
        let block = synthetic_font_block();
        let first = word(&block, 0).unwrap();
        // A kind that draws nothing, too few rows, a bitmap wider than a byte.
        for (at, value) in [(0, 3), (3, 1), (2, 9)] {
            let mut bad = block.clone();
            bad[first + at] = value;
            assert!(parse(&bad).is_err(), "byte {at} = {value}");
        }
        // A glyph offset pointing back into the table.
        let mut bad = block.clone();
        bad[first + 6..first + 8].copy_from_slice(&(first as u16).to_le_bytes());
        assert!(parse(&bad).is_err());
        // Cut short.
        assert!(parse(&block[..block.len() - 1]).is_err());
        assert!(parse(&block[..4]).is_err());
    }
}
