//! The situation room's status view (mode S, T-126; `docs/screens.md`, "The status
//! view (T-126)"): the selected robot's kind's picture, its captions and the robot's
//! name, or a text without one, drawn as `1fe6:704a` draws them. The pictures, places,
//! captions and texts are read from the user's `xeno.exe` (`xeno_formats::sitroom`);
//! the places and colours below are the routine's immediates.

use xeno_formats::ban::Sprite;
use xeno_formats::font::Font;
use xeno_formats::sitroom::{Caption, KINDS, SitRoom};
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};

/// Font 4, colour 0x1d on 0 (0x17 on 0 for codes with bit 7).
pub const FONT: u16 = 4;
const COLOUR: u8 = 0x1d;
const HIGH_COLOUR: u8 = 0x17;
/// The robot's name, opaque; the no-unit text, transparent.
const NAME_AT: (i32, i32) = (0x70, 0x7b);
const NO_UNITS_AT: (i32, i32) = (0xb0, 0x46);

/// Mode S over a main view already cleared to 0, for the selected robot's kind and
/// serial (+0x18), or none.
pub fn draw<'a>(
    fb: &mut Framebuffer,
    room: &SitRoom,
    font: &Font,
    unit: Option<(usize, u16)>,
    sprite: impl Fn(u16) -> Option<&'a Sprite>,
) {
    let mut cursor = TextCursor::at(0, 0, COLOUR);
    cursor.high_colour = HIGH_COLOUR;
    let status = &room.status;
    let Some((kind, serial)) = unit.filter(|&(kind, _)| kind < KINDS) else {
        cursor.move_to(NO_UNITS_AT.0, NO_UNITS_AT.1);
        fb.draw_text(font, &mut cursor, &status.no_units, TextMode::Transparent);
        return;
    };
    let place = status.places[kind];
    match sprite(status.pictures[kind]) {
        Some(picture) => {
            let keyed = SpriteBlit {
                mode: BlitMode::Keyed,
                ..SpriteBlit::default()
            };
            fb.blit_sprite(picture, place.left.into(), place.top.into(), keyed);
        }
        None => log::warn!("status view: no sprite {:#x}", status.pictures[kind]),
    }
    for caption in &status.captions[kind] {
        match caption {
            Caption::Text { x, y, text } => {
                cursor.move_to((*x).into(), (*y).into());
                fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
            }
            Caption::Line { colour, ends } => crate::mainview::draw_line(fb, *ends, *colour),
        }
    }
    cursor.move_to(NAME_AT.0, NAME_AT.1);
    let name = crate::panels::unit_name(&room.detail, kind, serial);
    fb.draw_text(font, &mut cursor, &name, TextMode::Opaque);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainview::tests::room;
    use xeno_formats::maproom::Rect;
    use xeno_render::Size;

    #[test]
    fn a_unit_s_picture_and_lines_or_none() {
        let mut room = room();
        room.status.places[1] = Rect {
            top: 2,
            left: 3,
            bottom: 9,
            right: 9,
        };
        room.status.captions[1] = vec![Caption::Line {
            colour: 7,
            ends: [20, 1, 20, 4],
        }];
        let mut picture = crate::mainview::tests::sprites().remove(0);
        picture.header.width = 2;
        picture.header.height = 1;
        picture.pixels = vec![0, 5];
        let font = crate::mainview::tests::roster_font();
        let mut fb = Framebuffer::new(Size::new(320, 200));
        fb.pixels_mut()[2 * 320 + 3] = 9;
        draw(&mut fb, &room, &font, Some((1, 7)), |h| {
            (h == room.status.pictures[1]).then_some(&picture)
        });
        // Keyed: colour 0 leaves the pixel under it.
        assert_eq!((fb.get(3, 2), fb.get(4, 2)), (Some(9), Some(5)));
        assert_eq!(fb.get(1, 20), Some(7));
        assert_eq!((fb.get(3, 20), fb.get(4, 20)), (Some(7), Some(0)));
        // A kind past the table counts as none.
        let mut none = Framebuffer::new(Size::new(320, 200));
        draw(&mut none, &room, &font, Some((KINDS, 0)), |_| None);
        assert!(none.pixels().contains(&COLOUR));
    }
}
