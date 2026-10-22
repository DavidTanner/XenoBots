//! A synthetic test card for checking the window and, later, the presentation scaler
//! (T-032), and its moving parts for the game loop (T-034). Nothing here comes from the
//! game files: the palette and picture are made in code.

use xeno_render::{Framebuffer, Palette, Rect, Rgb};

use crate::input::{Keys, ScanCode};

pub const BLACK: u8 = 0;
pub const WHITE: u8 = 1;
pub const RED: u8 = 2;
pub const GREEN: u8 = 3;
pub const BLUE: u8 = 4;
pub const YELLOW: u8 = 5;

/// Index of the first entry of the hue ramp; entries 16..=255 sweep the colour wheel.
const HUE_START: u8 = 16;

/// Where the 1-pixel checkerboard is: any blur in the scaler turns it grey.
pub const CHECKER: Rect = Rect::new(12, 12, 64, 64);
/// The strip that shows every palette entry, one column each, left to right.
pub const RAMP: Rect = Rect::new(32, 160, 256, 24);
/// A circle in pixel units: round when pixels are square (320x200 shown 1:1).
pub const PIXEL_CIRCLE: (i32, i32, i32, i32) = (124, 76, 40, 40);
/// An ellipse 1.2 times wider than tall: round when 320x200 is shown at 4:3, as on
/// the original monitor (pixels 5:6). The scaler's aspect correction makes it round.
pub const CRT_CIRCLE: (i32, i32, i32, i32) = (232, 76, 48, 40);
/// The tracks of the two markers: a white one steps one pixel per retrace, a yellow
/// one per 60 Hz tick. At 70 Hz the white one crosses in 4.5 s; at 60 Hz the yellow one
/// takes 5.3 s.
pub const RETRACE_TRACK: Rect = Rect::new(2, 122, 316, 6);
pub const SIXTY_HZ_TRACK: Rect = Rect::new(2, 130, 316, 6);
/// The held keys, one 2x2 cell per scan code: plain codes in the top four rows of 32,
/// `E0` codes in the bottom four.
pub const KEY_GRID: Rect = Rect::new(128, 140, 64, 16);

/// The test card's palette: a few named colours, then a hue ramp.
pub fn palette() -> Palette {
    let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
    let named = [
        (WHITE, (255, 255, 255)),
        (RED, (255, 0, 0)),
        (GREEN, (0, 255, 0)),
        (BLUE, (0, 0, 255)),
        (YELLOW, (255, 255, 0)),
    ];
    for (index, (r, g, b)) in named {
        colors[usize::from(index)] = Rgb { r, g, b };
    }
    // Six sectors of 40 entries: red -> yellow -> green -> cyan -> blue -> magenta.
    for (i, colour) in colors[usize::from(HUE_START)..].iter_mut().enumerate() {
        let up = ((i % 40) * 256 / 40) as u8;
        let down = 255 - up;
        let (r, g, b) = match i / 40 {
            0 => (255, up, 0),
            1 => (down, 255, 0),
            2 => (0, 255, up),
            3 => (0, down, 255),
            4 => (up, 0, 255),
            _ => (255, 0, down),
        };
        *colour = Rgb { r, g, b };
    }
    Palette { colors }
}

/// Draws the test card over the whole view of `fb`: black background, a white 1-pixel
/// border, coloured corner marks (red top-left, green top-right, blue bottom-left,
/// yellow bottom-right), a 1-pixel checkerboard, the two circles and the palette ramp.
pub fn draw(fb: &mut Framebuffer) {
    let size = fb.view();
    let (w, h) = (size.width as i32, size.height as i32);
    fb.clear(BLACK);
    for edge in [
        Rect::new(0, 0, w as u32, 1),
        Rect::new(0, h - 1, w as u32, 1),
        Rect::new(0, 0, 1, h as u32),
        Rect::new(w - 1, 0, 1, h as u32),
    ] {
        fb.fill_rect(edge, WHITE);
    }
    for (x, y, colour) in [
        (2, 2, RED),
        (w - 8, 2, GREEN),
        (2, h - 8, BLUE),
        (w - 8, h - 8, YELLOW),
    ] {
        fb.fill_rect(Rect::new(x, y, 6, 6), colour);
    }
    for y in 0..CHECKER.height as i32 {
        for x in 0..CHECKER.width as i32 {
            if (x + y) % 2 == 0 {
                fb.fill_rect(Rect::new(CHECKER.x + x, CHECKER.y + y, 1, 1), WHITE);
            }
        }
    }
    ellipse_outline(fb, PIXEL_CIRCLE, WHITE);
    ellipse_outline(fb, CRT_CIRCLE, YELLOW);
    for i in 0..RAMP.width as i32 {
        fb.fill_rect(Rect::new(RAMP.x + i, RAMP.y, 1, RAMP.height), i as u8);
    }
}

/// Draws the moving parts over the card: each marker's position comes from its tick
/// count, and the key grid lights the held keys.
pub fn draw_live(fb: &mut Framebuffer, retraces: u64, sixty_hz: u64, keys: &Keys) {
    for (track, ticks, colour) in [
        (RETRACE_TRACK, retraces, WHITE),
        (SIXTY_HZ_TRACK, sixty_hz, YELLOW),
    ] {
        fb.fill_rect(track, BLACK);
        let x = track.x + (ticks % u64::from(track.width)) as i32;
        fb.fill_rect(Rect::new(x, track.y, 1, track.height), colour);
    }
    fb.fill_rect(KEY_GRID, BLUE);
    for code in keys.held() {
        let (x, y) = key_cell(code);
        fb.fill_rect(Rect::new(x, y, 2, 2), WHITE);
    }
}

/// The top-left pixel of a key's cell in [`KEY_GRID`].
fn key_cell(code: ScanCode) -> (i32, i32) {
    let i = code.index() as i32;
    (KEY_GRID.x + i % 32 * 2, KEY_GRID.y + i / 32 * 2)
}

/// Draws the outline of the ellipse `(cx, cy, rx, ry)`: the pixels inside it with a
/// 4-neighbour outside it. Integer-only, so the result is exact and symmetric.
fn ellipse_outline(fb: &mut Framebuffer, (cx, cy, rx, ry): (i32, i32, i32, i32), colour: u8) {
    let inside = |dx: i32, dy: i32| {
        i64::from(dx * dx) * i64::from(ry * ry) + i64::from(dy * dy) * i64::from(rx * rx)
            <= i64::from(rx * rx) * i64::from(ry * ry)
    };
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(ex, ey)| !inside(dx + ex, dy + ey));
            if inside(dx, dy) && edge {
                fb.fill_rect(Rect::new(cx + dx, cy + dy, 1, 1), colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_render::Size;

    fn card() -> Framebuffer {
        let mut fb = Framebuffer::new(Size::MODE_13H);
        draw(&mut fb);
        fb
    }

    #[test]
    fn palette_has_the_named_colours_and_a_distinct_hue_ramp() {
        let p = palette();
        assert_eq!(p[BLACK], Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            p[WHITE],
            Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(p[RED], Rgb { r: 255, g: 0, b: 0 });
        let hues = &p.colors[usize::from(HUE_START)..];
        for pair in hues.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
    }

    #[test]
    fn border_is_white_and_one_pixel_wide() {
        let fb = card();
        for x in 0..320 {
            assert_eq!((fb.get(x, 0), fb.get(x, 199)), (Some(WHITE), Some(WHITE)));
        }
        for y in 0..200 {
            assert_eq!((fb.get(0, y), fb.get(319, y)), (Some(WHITE), Some(WHITE)));
        }
        assert_eq!(fb.get(1, 100), Some(BLACK));
        assert_eq!(fb.get(2, 2), Some(RED));
        assert_eq!(fb.get(317, 197), Some(YELLOW));
    }

    #[test]
    fn checkerboard_alternates_every_pixel() {
        let fb = card();
        let (x0, y0) = (CHECKER.x as usize, CHECKER.y as usize);
        for y in 0..CHECKER.height as usize {
            for x in 0..CHECKER.width as usize {
                let expected = if (x + y) % 2 == 0 { WHITE } else { BLACK };
                assert_eq!(fb.get(x0 + x, y0 + y), Some(expected), "({x}, {y})");
            }
        }
    }

    #[test]
    fn ramp_shows_every_palette_entry_in_order() {
        let fb = card();
        for i in 0..256 {
            for y in RAMP.y as usize..(RAMP.y as u32 + RAMP.height) as usize {
                assert_eq!(fb.get(RAMP.x as usize + i, y), Some(i as u8));
            }
        }
    }

    #[test]
    fn markers_step_with_their_ticks_and_held_keys_light_up() {
        let mut fb = card();
        let mut keys = Keys::default();
        keys.press(ScanCode(0x01));
        keys.press(ScanCode(0xe048));
        draw_live(&mut fb, 316 + 5, 7, &keys);
        let row = |track: Rect| -> Vec<usize> {
            (0..320)
                .filter(|&x| fb.get(x, track.y as usize) != Some(BLACK))
                .collect()
        };
        assert_eq!(row(RETRACE_TRACK), [0, 7, 319]);
        assert_eq!(row(SIXTY_HZ_TRACK), [0, 9, 319]);
        assert_eq!(fb.get(7, 127), Some(WHITE));
        assert_eq!(fb.get(9, 135), Some(YELLOW));
        let lit: Vec<_> = (0..16)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.get(128 + x, 140 + y) == Some(WHITE))
            .collect();
        // 0x01 at column 1, row 0; E0 48 is cell 128 + 72: column 8, row 6.
        assert_eq!(
            lit,
            [
                (2, 0),
                (3, 0),
                (2, 1),
                (3, 1),
                (16, 12),
                (17, 12),
                (16, 13),
                (17, 13)
            ]
        );
    }

    #[test]
    fn circles_are_symmetric_and_span_their_radii() {
        let fb = card();
        for ((cx, cy, rx, ry), colour) in [(PIXEL_CIRCLE, WHITE), (CRT_CIRCLE, YELLOW)] {
            let at = |dx: i32, dy: i32| fb.get((cx + dx) as usize, (cy + dy) as usize);
            assert_eq!(at(rx, 0), Some(colour));
            assert_eq!(at(0, -ry), Some(colour));
            assert_eq!(at(rx + 1, 0), Some(BLACK));
            assert_eq!(at(0, 0), Some(BLACK));
            for dy in -ry..=ry {
                for dx in 0..=rx {
                    assert_eq!(at(dx, dy), at(-dx, dy));
                    assert_eq!(at(dx, dy), at(dx, -dy));
                }
            }
        }
    }
}
