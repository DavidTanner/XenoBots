//! Text in the game's fonts, drawn as `mgavideo.dvr`'s glyph renderers (and
//! `vga240.mod`'s, which draw the same pixels) do. See `docs/formats/font.md`.
//!
//! The host keeps one text state: a cursor, the left margin a new line returns to, a
//! colour pair for codes below 0x80 and another for codes with bit 7 set. Each glyph
//! is drawn at the cursor (its top-left corner) and moves it on; codes whose glyph is
//! a control code move the cursor instead.

use xeno_formats::font::{Font, Style};

use crate::Framebuffer;

/// Whether glyphs fill their background (the renderers at slot `0x598`) or leave it
/// (slot `0x594`). `vga240.mod` has no opaque renderers: in the 320x240 mode slot
/// `0x598` draws nothing and does not move the cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextMode {
    #[default]
    Transparent,
    Opaque,
}

/// The host's text state (`font.md`, "Text state"), less the font itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCursor {
    /// The next glyph's top-left corner, in surface pixels.
    pub x: i32,
    pub y: i32,
    /// Where carriage return (0x0d) and new line (0x0c) put `x` back.
    pub left: i32,
    /// The glyph colour and, when opaque, the background (slot `0x39e`: `AL`, `AH`).
    pub colour: u8,
    pub background: u8,
    /// The same for codes with bit 7 set (slot `0x3a6`).
    pub high_colour: u8,
    pub high_background: u8,
    /// Extra pixels after each glyph and each line (text state `+0x0e`, `+0x10`;
    /// a font select sets both to 0).
    pub spacing_x: i32,
    pub spacing_y: i32,
}

impl Default for TextCursor {
    /// As the host sets it up (`1fe6:4872`): at (0, 0), colour 12 and 255 on 0.
    fn default() -> Self {
        TextCursor {
            x: 0,
            y: 0,
            left: 0,
            colour: 12,
            background: 0,
            high_colour: 0xff,
            high_background: 0,
            spacing_x: 0,
            spacing_y: 0,
        }
    }
}

impl TextCursor {
    /// A cursor at (`x`, `y`) with that left margin, drawing in `colour` on 0.
    pub fn at(x: i32, y: i32, colour: u8) -> Self {
        let mut cursor = TextCursor {
            colour,
            ..TextCursor::default()
        };
        cursor.move_to(x, y);
        cursor
    }

    /// Moves the cursor and the left margin, as slot `0x396` does.
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.x = x;
        self.left = x;
        self.y = y;
    }

    /// Runs a control code (bit 7 ignored); codes with no meaning do nothing. 7 (a
    /// bell through the host's word slot `0x26c`, not traced) does nothing here.
    fn control(&mut self, font: &Font, code: u8) {
        let header = font.header;
        let line = i32::from(header.height) + self.spacing_y + i32::from(header.step_y);
        match code & 0x7f {
            0x08 => {
                self.y -= i32::from(header.step_y);
                self.x -= i32::from(header.width) + self.spacing_x + i32::from(header.step_x);
            }
            0x0a => self.y += line,
            0x0c => {
                self.x = self.left;
                self.y += line;
            }
            0x0d => self.x = self.left,
            0x0e => self.x += 1,
            0x0f => self.x -= 1,
            _ => {}
        }
    }
}

impl Framebuffer {
    /// Draws the glyph for `code` in `font` at the cursor and moves the cursor on,
    /// or runs `code` as a control code if its glyph is one (or `code` is past the
    /// font's table). Pixels off the surface are dropped (the original does not clip).
    pub fn draw_char(&mut self, font: &Font, cursor: &mut TextCursor, code: u8, mode: TextMode) {
        let Some(glyph) = font.glyph(code) else {
            cursor.control(font, code);
            return;
        };
        let (colour, background) = if code & 0x80 != 0 {
            (cursor.high_colour, cursor.high_background)
        } else {
            (cursor.colour, cursor.background)
        };
        let opaque = mode == TextMode::Opaque;
        let style = font.style();
        let width = match style {
            // An opaque bitmap glyph fills its cell; a transparent one stops at the
            // last set bit, which is never past the byte.
            Style::Bitmap => usize::from(font.header.width).min(8),
            // An opaque shaded glyph fills one more column: the gap after it.
            Style::Shaded => usize::from(glyph.width) + usize::from(opaque),
        };
        for row in 0..font.rows() {
            for column in 0..width {
                let value = font.pixel(glyph, column, row);
                let pixel = match (value, style) {
                    (0, _) if !opaque => continue,
                    (0, _) => background,
                    (_, Style::Bitmap) => colour,
                    (v, Style::Shaded) => colour.wrapping_add(v ^ 3),
                };
                self.put(cursor.x + column as i32, cursor.y + row as i32, pixel);
            }
        }
        let advance = match style {
            Style::Bitmap => i32::from(font.header.width),
            Style::Shaded => i32::from(glyph.width) + 1,
        };
        cursor.x += advance + cursor.spacing_x + i32::from(font.header.step_x);
        cursor.y += i32::from(font.header.step_y);
    }

    /// Draws `text` as the host's string services do (slots `0x3b2`–`0x3c6`): code by
    /// code up to a NUL, or up to a `$` followed by a NUL (the `$` is not drawn).
    pub fn draw_text(&mut self, font: &Font, cursor: &mut TextCursor, text: &[u8], mode: TextMode) {
        for (i, &code) in text.iter().enumerate() {
            match code {
                0 => return,
                b'$' if text.get(i + 1) == Some(&0) => return,
                _ => self.draw_char(font, cursor, code, mode),
            }
        }
    }

    /// Writes surface pixel (`x`, `y`) if it is on the surface.
    fn put(&mut self, x: i32, y: i32, colour: u8) {
        let surface = self.surface();
        if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
            && x < surface.width
            && y < surface.height
        {
            self.pixels_mut()[y * surface.width + x] = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;
    use xeno_formats::fixtures::synthetic_font_block;
    use xeno_formats::font::{self, Font};

    fn fonts() -> Vec<Font> {
        font::parse(&synthetic_font_block()).unwrap()
    }

    /// The top-left `w` x `h` pixels, one line a row: `.` for 0, else the colour in
    /// hex.
    fn picture(fb: &Framebuffer, w: usize, h: usize) -> String {
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match fb.get(x, y).unwrap() {
                        0 => ".".to_string(),
                        c => format!("{:x}", c % 16),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn bitmap_text_advances_by_the_cell() {
        let fonts = fonts();
        let mut fb = Framebuffer::new(Size::new(12, 4));
        let mut cursor = TextCursor::at(1, 1, 5);
        fb.draw_text(&fonts[0], &mut cursor, b"AA\0A", TextMode::Transparent);
        assert_eq!(
            picture(&fb, 12, 4),
            "............\n.555.555....\n.5.5.5.5....\n............"
        );
        assert_eq!((cursor.x, cursor.y), (9, 1));
    }

    #[test]
    fn opaque_bitmap_text_fills_the_cell_and_bit_7_picks_the_high_colours() {
        let fonts = fonts();
        let mut fb = Framebuffer::new(Size::new(8, 2));
        fb.clear(9);
        let mut cursor = TextCursor::at(0, 0, 5);
        cursor.background = 1;
        cursor.high_colour = 6;
        cursor.high_background = 2;
        fb.draw_char(&fonts[0], &mut cursor, b'A', TextMode::Opaque);
        fb.draw_char(&fonts[0], &mut cursor, b'A' | 0x80, TextMode::Opaque);
        assert_eq!(picture(&fb, 8, 2), "55516662\n51516262");
    }

    #[test]
    fn shaded_glyphs_shade_from_the_colour_and_advance_their_width_plus_one() {
        let fonts = fonts();
        let mut fb = Framebuffer::new(Size::new(8, 2));
        let mut cursor = TextCursor::at(0, 0, 0x10);
        fb.draw_text(&fonts[3], &mut cursor, b"BB", TextMode::Transparent);
        // 3 is the colour itself, 2 the next, 1 the one after.
        assert_eq!(picture(&fb, 8, 2), "02.02...\n1..1....");
        assert_eq!(cursor.x, 6);
        let mut fb = Framebuffer::new(Size::new(8, 2));
        fb.clear(9);
        let mut cursor = TextCursor::at(0, 0, 0x10);
        cursor.background = 7;
        fb.draw_char(&fonts[3], &mut cursor, b'B', TextMode::Opaque);
        assert_eq!(picture(&fb, 8, 2), "02799999\n17799999");
    }

    #[test]
    fn control_codes_move_the_cursor() {
        let fonts = fonts();
        let font = &fonts[0];
        let mut fb = Framebuffer::new(Size::new(4, 4));
        let mut cursor = TextCursor::at(10, 20, 1);
        let mut step = |code: u8| {
            fb.draw_char(font, &mut cursor, code, TextMode::Transparent);
            (cursor.x, cursor.y)
        };
        assert_eq!(step(b' '), (14, 20));
        assert_eq!(step(0x08), (10, 20));
        assert_eq!(step(0x0e), (11, 20));
        assert_eq!(step(0x0f), (10, 20));
        assert_eq!(step(b' '), (14, 20));
        assert_eq!(step(0x0d), (10, 20));
        assert_eq!(step(b' '), (14, 20));
        assert_eq!(step(0x0a), (14, 23));
        assert_eq!(step(0x0c), (10, 26));
        // Codes with no meaning, and codes past a short table, do nothing.
        assert_eq!(step(0x07), (10, 26));
        let mut fb = Framebuffer::new(Size::new(4, 4));
        let mut cursor = TextCursor::at(0, 0, 1);
        fb.draw_char(&fonts[3], &mut cursor, b'a', TextMode::Transparent);
        assert_eq!((cursor.x, cursor.y), (0, 0));
    }

    #[test]
    fn a_dollar_before_the_end_is_not_drawn() {
        let fonts = fonts();
        let mut fb = Framebuffer::new(Size::new(4, 4));
        let mut cursor = TextCursor::at(0, 0, 1);
        fb.draw_text(&fonts[0], &mut cursor, b"$A$\0", TextMode::Transparent);
        // The first `$` is drawn (blank), then `A`; the last is not.
        assert_eq!(cursor.x, 8);
        let mut cursor = TextCursor::at(0, 0, 1);
        fb.draw_text(&fonts[0], &mut cursor, b"$$", TextMode::Transparent);
        assert_eq!(cursor.x, 8);
    }

    #[test]
    fn glyphs_are_clipped_to_the_surface() {
        let fonts = fonts();
        let mut fb = Framebuffer::new(Size::new(2, 1));
        let mut cursor = TextCursor::at(-1, -1, 3);
        fb.draw_char(&fonts[0], &mut cursor, b'A', TextMode::Opaque);
        assert_eq!(picture(&fb, 2, 1), ".3");
    }
}
