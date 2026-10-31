//! The situation room's panels (T-127; `docs/screens.md`, "The situation room's panels
//! (T-127)"): the status counts, the reserve bar, the clock, the power rows and the
//! selected unit's detail, drawn over the picture as their draw handlers draw them. The
//! widths, texts and tables are read from the user's `xeno.exe`
//! (`xeno_formats::sitroom`); places and colours are the handlers' immediates, short
//! code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::sitroom::{Detail, SitRoom};
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
/// `1000:5a0e`: font 4, colour 0x1a on 0; the heading at (0xa2, 0x84) and the unit's
/// name after it, then at x 0xc5 from y 0x8e, a row every 6: the place, the status
/// line, the heading in degrees, the speed, and the damage total and the signal as
/// percentages, the signal on the line the total's new line moves to. The name's dash
/// and the percent sign are `1fe6:7f38`'s and `1000:ddb6`'s immediates.
pub const DETAIL_FONT: u16 = 4;
const DETAIL_COLOUR: u8 = 0x1a;
const DETAIL_HEADING: (i32, i32) = (0xa2, 0x84);
const DETAIL_X: i32 = 0xc5;
const DETAIL_ROWS: [i32; 5] = [0x8e, 0x94, 0x9a, 0xa0, 0xa6];
const NEW_LINE: u8 = 0x0c;

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

/// The selected unit, as the detail panel reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailUnit<'a> {
    /// The kind (0 to 2) and +0x18, its serial.
    pub kind: usize,
    pub serial: u16,
    /// +0xfd bit 2; the base repairs it (+0x1a); its damage total (+0xf9).
    pub in_base: bool,
    pub repairing: bool,
    pub total: u8,
    /// The running program's name while it drives (+0x23 bit 7), none with the pilot.
    pub program: Option<&'a [u8]>,
    /// +0x10, +0x26, +0x4f.
    pub heading: u16,
    pub speed: i16,
    pub signal: u8,
}

/// What the detail panel draws, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    /// Text, from a place or where the last left the cursor.
    Text(Option<(i32, i32)>, Vec<u8>, TextMode),
    /// Slot `0x51a`: the rectangle filled with 0.
    Clear(Rect),
}

/// The detail panel: the heading, then the unit's name, place, status, heading, speed,
/// damage and signal, or the no-unit texts.
pub fn draw_detail(fb: &mut Framebuffer, font: &Font, d: &Detail, unit: Option<&DetailUnit>) {
    let mut cursor = TextCursor::at(0, 0, DETAIL_COLOUR);
    for piece in pieces(d, unit) {
        match piece {
            Piece::Text(at, text, mode) => {
                if let Some((x, y)) = at {
                    cursor.move_to(x, y);
                }
                fb.draw_text(font, &mut cursor, &text, mode);
            }
            Piece::Clear(area) => crate::mission::fill(fb, area, 0),
        }
    }
}

fn pieces(d: &Detail, unit: Option<&DetailUnit>) -> Vec<Piece> {
    use TextMode::{Opaque, Transparent};
    let at = |row: usize| Some((DETAIL_X, DETAIL_ROWS[row]));
    let mut pieces = vec![Piece::Text(Some(DETAIL_HEADING), d.heading.clone(), Opaque)];
    let Some(u) = unit else {
        pieces.push(Piece::Text(None, d.no_unit[0].clone(), Opaque));
        pieces.push(Piece::Text(at(0), d.no_unit[1].clone(), Opaque));
        return pieces;
    };
    pieces.push(Piece::Text(None, unit_name(d, u.kind, u.serial), Opaque));
    pieces.push(Piece::Text(
        at(0),
        d.places[usize::from(!u.in_base)].clone(),
        Opaque,
    ));
    let status = match (u.in_base, u.program) {
        (true, _) if u.repairing => &d.repair_states[0],
        (true, _) if u.total != 0 => &d.repair_states[1],
        (true, _) => &d.repair_states[2],
        (false, Some(name)) => name,
        (false, None) => &d.manual,
    };
    pieces.push(Piece::Clear(d.status_clear));
    pieces.push(Piece::Text(at(1), status.to_vec(), Transparent));
    let degrees = crate::editor::degrees(u.heading);
    pieces.push(Piece::Text(at(2), digits(degrees, 3), Opaque));
    // `mov al, [bx + table]` by the speed's size; past the table the original reads on.
    // shortcut: the port shows the table's last speed there; no stock robot goes so fast.
    let size = usize::from(u.speed.unsigned_abs());
    let speed = d.speeds.get(size).or(d.speeds.last()).copied().unwrap_or(0);
    pieces.push(Piece::Text(at(3), digits(speed.into(), 2), Opaque));
    pieces.push(Piece::Text(at(4), percent(u.total), Opaque));
    pieces.push(Piece::Text(None, percent(signal_percent(u.signal)), Opaque));
    pieces
}

/// A robot's name (`1fe6:7f38`): the kind's letters, `-`, the serial's last three
/// digits and a new line.
/// shortcut: with the developer's Ctrl+Alt+F (`DS:0x5ba0`) the original shows
/// `DS:0x62cc` instead; not ported, as nothing the port runs writes it.
pub(crate) fn unit_name(d: &Detail, kind: usize, serial: u16) -> Vec<u8> {
    let mut name = d
        .kind_letters
        .get(kind)
        .copied()
        .unwrap_or_default()
        .to_vec();
    name.push(b'-');
    name.extend(digits(serial & 0x7fff, 3));
    name.push(NEW_LINE);
    name
}

/// `1000:02b7`'s five digits with zeros, the last `n` of them.
pub(crate) fn digits(value: u16, n: usize) -> Vec<u8> {
    format!("{value:05}").into_bytes().split_off(5 - n)
}

/// `1000:ddb6`: the low seven bits as three digits, `%` and a new line.
pub(crate) fn percent(value: u8) -> Vec<u8> {
    let mut text = digits((value & 0x7f).into(), 3);
    text.extend([b'%', NEW_LINE]);
    text
}

/// `1000:df90`: 0 to 255 as 0 to 99, and 99 as 100.
fn signal_percent(value: u8) -> u8 {
    match ((u16::from(value) * 100) >> 8) as u8 {
        99 => 100,
        p => p,
    }
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
pub(crate) mod tests {
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

    pub(crate) fn detail() -> Detail {
        Detail {
            heading: b"D:".to_vec(),
            no_unit: [b"-0-".to_vec(), b"~".to_vec()],
            places: [b"IN".to_vec(), b"OUT".to_vec()],
            status_clear: rect(1, 2, 3, 4),
            repair_states: [b"R1".to_vec(), b"R2".to_vec(), b"R3".to_vec()],
            manual: b"MAN".to_vec(),
            kind_letters: [*b"AA", *b"BB", *b"CC"],
            speeds: vec![0, 1, 1, 2],
        }
    }

    /// A text piece: where it starts, what it says.
    type Line<'a> = (Option<(i32, i32)>, &'a [u8]);

    fn texts(pieces: &[Piece]) -> Vec<Line<'_>> {
        pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Text(at, text, _) => Some((*at, text.as_slice())),
                Piece::Clear(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_detail_panel_shows_the_unit_or_none() {
        let d = detail();
        let none = pieces(&d, None);
        assert_eq!(
            texts(&none),
            [
                (Some((0xa2, 0x84)), &b"D:"[..]),
                (None, b"-0-"),
                (Some((0xc5, 0x8e)), b"~")
            ]
        );
        let mut unit = DetailUnit {
            kind: 1,
            serial: 0x8000 | 1234,
            in_base: false,
            repairing: false,
            total: 0x85,
            program: Some(b"PATROL"),
            heading: 0x4000,
            speed: -3,
            signal: 0xff,
        };
        let shown = pieces(&d, Some(&unit));
        assert_eq!(
            texts(&shown)[1..],
            [
                (None, &b"BB-234\x0c"[..]),
                (Some((0xc5, 0x8e)), b"OUT"),
                (Some((0xc5, 0x94)), b"PATROL"),
                (Some((0xc5, 0x9a)), b"090"),
                (Some((0xc5, 0xa0)), b"02"),
                (Some((0xc5, 0xa6)), b"005%\x0c"),
                (None, b"100%\x0c"),
            ]
        );
        // The status line is cleared, then drawn transparent.
        assert_eq!(shown[3], Piece::Clear(rect(1, 2, 3, 4)));
        assert!(matches!(shown[4], Piece::Text(_, _, TextMode::Transparent)));
        let status = |u: &DetailUnit| texts(&pieces(&d, Some(u)))[3].1.to_vec();
        unit.program = None;
        assert_eq!(status(&unit), b"MAN");
        unit.in_base = true;
        assert_eq!(status(&unit), b"R2");
        unit.total = 0;
        assert_eq!(status(&unit), b"R3");
        unit.repairing = true;
        assert_eq!(status(&unit), b"R1");
        assert_eq!(texts(&pieces(&d, Some(&unit)))[2].1, b"IN");
        assert_eq!(
            (
                signal_percent(0x80),
                signal_percent(0xfd),
                signal_percent(0xfe)
            ),
            (50, 98, 100)
        );
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
