//! The situation room's panels (T-127; `docs/screens.md`, "The situation room's panels
//! (T-127)"): the status counts, the reserve bar, the clock and the power rows, drawn
//! over the picture as their draw handlers draw them. The widths and the clock's texts
//! are read from the user's `xeno.exe` (`xeno_formats::sitroom`); places and colours
//! are the handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::sitroom::SitRoom;
use xeno_render::{Framebuffer, TextCursor, TextMode};

/// The panels' widget types: the reserve bar, the clock, the power rows' bars (NET,
/// REP, CON: the base's +0x39 to +0x3b) and their left and right arrows. The counts'
/// (0x60) draw at fixed places, not from its rectangle.
pub const RESERVE_TYPE: u16 = 0x7c;
pub const CLOCK_TYPE: u16 = 0x7e;
pub const BAR_TYPES: [u16; 3] = [0x76, 0x78, 0x7a];
pub const LOWER_TYPE: u16 = 0x72;
pub const RAISE_TYPE: u16 = 0x74;
/// The font of the counts and the clock.
pub const PANEL_FONT: u16 = 2;
/// `1000:5979`: the base's +0x20 to +0x27 as two digits each (`1000:02b7`'s five, from
/// the fourth), opaque in 0x1d on 0, at x 0x3c and 0x49 (in pairs) from y 0x75, a row
/// every 7.
const COUNTS_COLOUR: u8 = 0x1d;
const COUNTS_X: [i32; 2] = [0x3c, 0x49];
const COUNTS_TOP: i32 = 0x75;
const COUNTS_ROW: i32 = 7;
/// `1000:6047` to `61a4`: the bar's width in 0x0d from the left, the rest of the widget
/// in 0x41 (the reserve's in 0) unless the width is 0x17 or more (an unsigned compare);
/// slot `0x51e` (`mgavideo.dvr` 0x0eae) fills all but the pixels of 0x7f.
const BAR_COLOUR: u8 = 0x0d;
pub const BAR_REST: u8 = 0x41;
pub const RESERVE_REST: u8 = 0;
const BAR_FULL: u16 = 0x17;
const BAR_KEEP: u8 = 0x7f;
/// `1000:61a5`: the clock opaque in 0x14 on 0 at the widget's corner, then glyph 0x1e
/// transparent, in 0x14, or 0 while the clock's seconds (`DS:0x7d48`) are odd.
const CLOCK_COLOUR: u8 = 0x14;
const CLOCK_MARK: u8 = 0x1e;

/// The counts: the relays in store (+0x20), then the count of +0x21 to +0x27.
pub fn draw_counts(fb: &mut Framebuffer, font: &Font, counts: [u8; 8]) {
    for (i, n) in counts.into_iter().enumerate() {
        let ((x, y), text) = count_text(i, n);
        let mut cursor = TextCursor::at(x, y, COUNTS_COLOUR);
        fb.draw_text(font, &mut cursor, &text, TextMode::Opaque);
    }
}

/// Where count `i` goes, and its last two digits.
fn count_text(i: usize, n: u8) -> ((i32, i32), [u8; 2]) {
    let at = (COUNTS_X[i % 2], COUNTS_TOP + COUNTS_ROW * (i / 2) as i32);
    (at, [b'0' + n / 10 % 10, b'0' + n % 10])
}

/// A bar over the widget's rectangle: `value / 8`'s width (`room.bar_widths`) in the
/// bar's colour, the rest in `rest`.
pub fn draw_bar(fb: &mut Framebuffer, area: Rect, value: u8, room: &SitRoom, rest: u8) {
    let w = room.bar_widths[usize::from(value >> 3)] as i16 as u16;
    let split = area.left.wrapping_add(w);
    if w != 0 {
        fill_except(
            fb,
            Rect {
                right: split,
                ..area
            },
            BAR_COLOUR,
        );
    }
    if w < BAR_FULL {
        fill_except(
            fb,
            Rect {
                left: split,
                ..area
            },
            rest,
        );
    }
}

/// Slot `0x51e`: the rectangle in `colour`, but for the pixels of [`BAR_KEEP`].
fn fill_except(fb: &mut Framebuffer, area: Rect, colour: u8) {
    let width = fb.surface().width;
    for y in usize::from(area.top)..usize::from(area.bottom) {
        for x in usize::from(area.left)..usize::from(area.right) {
            if fb.get(x, y).is_some_and(|p| p != BAR_KEEP) {
                fb.pixels_mut()[y * width + x] = colour;
            }
        }
    }
}

/// The clock (`room.clock_of`) at the widget's corner, and its mark.
pub fn draw_clock(fb: &mut Framebuffer, area: Rect, font: &Font, text: &[u8], second: u8) {
    let mut cursor = TextCursor::at(area.left.into(), area.top.into(), CLOCK_COLOUR);
    fb.draw_text(font, &mut cursor, text, TextMode::Opaque);
    cursor.colour = mark_colour(second);
    fb.draw_char(font, &mut cursor, CLOCK_MARK, TextMode::Transparent);
}

/// The mark's colour: 0 while the seconds are odd (`test byte [0x7d48], 1`).
fn mark_colour(second: u8) -> u8 {
    if second & 1 != 0 { 0 } else { CLOCK_COLOUR }
}

/// The slider routine an arrow runs while held (`1000:5f8c`, `5ff3`, by the row's byte
/// 0, 2 or 4): 1, 3, 5 lower the row, 2, 4, 6 raise it.
pub fn slider(kind: u16, row: u8) -> Option<u8> {
    match kind {
        LOWER_TYPE => Some(row + 1),
        RAISE_TYPE => Some(row + 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainview::tests::room;
    use xeno_render::Size;

    fn rect(top: u16, left: u16, bottom: u16, right: u16) -> Rect {
        Rect {
            top,
            left,
            bottom,
            right,
        }
    }

    #[test]
    fn a_bar_fills_its_width_and_the_rest_but_keeps_0x7f() {
        let mut room = room();
        room.bar_widths = std::array::from_fn(|i| i as i8);
        room.bar_widths[31] = 0x17;
        let mut fb = Framebuffer::new(Size::new(40, 2));
        fb.pixels_mut()[3] = BAR_KEEP;
        let area = rect(0, 0, 1, 30);
        // 0x28 / 8 = 5 wide.
        draw_bar(&mut fb, area, 0x28, &room, BAR_REST);
        let row = &fb.pixels()[..40];
        assert_eq!(row[..6], [BAR_COLOUR, 13, 13, BAR_KEEP, 13, BAR_REST]);
        assert!(row[5..30].iter().all(|&p| p == BAR_REST));
        assert!(row[30..].iter().all(|&p| p == 0));
        // At the full width the rest is left alone.
        fb.pixels_mut()[25] = 9;
        draw_bar(&mut fb, area, 0xff, &room, BAR_REST);
        assert_eq!(fb.pixels()[22..26], [13, BAR_REST, BAR_REST, 9]);
        // Nothing wide: all the rest.
        draw_bar(&mut fb, area, 0, &room, RESERVE_REST);
        assert_eq!(fb.pixels()[..4], [0, 0, 0, BAR_KEEP]);
    }

    #[test]
    fn counts_are_two_digits_in_pairs_and_the_mark_blinks() {
        assert_eq!(count_text(0, 5), ((0x3c, 0x75), *b"05"));
        assert_eq!(count_text(3, 42), ((0x49, 0x7c), *b"42"));
        assert_eq!(count_text(7, 123), ((0x49, 0x8a), *b"23"));
        assert_eq!((mark_colour(58), mark_colour(59)), (CLOCK_COLOUR, 0));
        assert_eq!(slider(LOWER_TYPE, 2), Some(3));
        assert_eq!(slider(RAISE_TYPE, 4), Some(6));
        assert_eq!(slider(0x76, 0), None);
    }
}
