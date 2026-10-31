//! The situation room's status view (mode S, T-126; `docs/screens.md`, "The status
//! view (T-126)"): the selected robot's kind's picture, its captions, the robot's name
//! and its kind's figures, or a text without one, drawn as `1fe6:704a` draws them. The
//! pictures, places, captions, figures and texts are read from the user's `xeno.exe`
//! (`xeno_formats::sitroom`); the places and colours below are the routine's
//! immediates.

use xeno_formats::ban::Sprite;
use xeno_formats::font::Font;
use xeno_formats::sitroom::{Caption, KINDS, SitRoom};
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};
use xeno_sim::Components;

/// Font 4, colour 0x1d on 0 (0x17 on 0 for codes with bit 7).
pub const FONT: u16 = 4;
const COLOUR: u8 = 0x1d;
const HIGH_COLOUR: u8 = 0x17;
/// The robot's name, opaque; the no-unit text, transparent.
const NAME_AT: (i32, i32) = (0x70, 0x7b);
const NO_UNITS_AT: (i32, i32) = (0xb0, 0x46);

/// The selected robot: its kind (0 to 2), serial (+0x18) and the bytes its figures
/// read.
#[derive(Debug, Clone, Copy)]
pub struct Unit {
    pub kind: usize,
    pub serial: u16,
    pub parts: Components,
}

/// Mode S over a main view already cleared to 0, for the selected robot or none.
pub fn draw<'a>(
    fb: &mut Framebuffer,
    room: &SitRoom,
    font: &Font,
    unit: Option<Unit>,
    sprite: impl Fn(u16) -> Option<&'a Sprite>,
) {
    let mut cursor = TextCursor::at(0, 0, COLOUR);
    cursor.high_colour = HIGH_COLOUR;
    let status = &room.status;
    let Some(Unit {
        kind,
        serial,
        mut parts,
    }) = unit.filter(|u| u.kind < KINDS)
    else {
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
    for figure in &status.figures[kind] {
        // shortcut: a byte the sim does not keep reads 0; the stock figures read only
        // kept ones.
        let value = parts.byte(figure.field).map_or(0, |b| *b) & figure.mask;
        let text = if figure.percent {
            percent(value)
        } else {
            status.flags[usize::from(value != 0)].clone()
        };
        cursor.move_to(figure.x.into(), figure.y.into());
        fb.draw_text(font, &mut cursor, &text, TextMode::Opaque);
    }
}

/// `1000:ddd5`: the detail panel's percentage, its bytes from 0x20 up with bit 7 set
/// (the second colour pair) unless it shows exactly 100.
fn percent(value: u8) -> Vec<u8> {
    let mut text = crate::panels::percent(value);
    if value & 0x7f != 100 {
        text.iter_mut()
            .filter(|b| **b >= 0x20)
            .for_each(|b| *b |= 0x80);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainview::tests::{room, roster_font};
    use xeno_formats::maproom::Rect;
    use xeno_formats::sitroom::Figure;
    use xeno_render::Size;

    fn parts(helm: u8, weapons: u8) -> Components {
        Components {
            helm,
            power: 0,
            weapons,
            targeting: 0,
            systems: 0,
            equipment: 0,
            total: 0,
        }
    }

    fn unit(kind: usize, parts: Components) -> Option<Unit> {
        Some(Unit {
            kind,
            serial: 7,
            parts,
        })
    }

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
        let font = roster_font();
        let mut fb = Framebuffer::new(Size::new(320, 200));
        fb.pixels_mut()[2 * 320 + 3] = 9;
        draw(&mut fb, &room, &font, unit(1, parts(100, 0)), |h| {
            (h == room.status.pictures[1]).then_some(&picture)
        });
        // Keyed: colour 0 leaves the pixel under it.
        assert_eq!((fb.get(3, 2), fb.get(4, 2)), (Some(9), Some(5)));
        assert_eq!(fb.get(1, 20), Some(7));
        assert_eq!((fb.get(3, 20), fb.get(4, 20)), (Some(7), Some(0)));
        // A kind past the table counts as none.
        let mut none = Framebuffer::new(Size::new(320, 200));
        draw(&mut none, &room, &font, unit(KINDS, parts(0, 0)), |_| None);
        assert!(none.pixels().contains(&COLOUR));
    }

    #[test]
    fn figures_show_flags_and_percentages_in_their_colours() {
        assert_eq!(percent(100), b"100%\x0c");
        assert_eq!(percent(0x80 | 5), [0xb0, 0xb0, 0xb5, 0xa5, 0x0c]);
        let mut room = room();
        // The synthetic font inks only `A`.
        room.status.flags = [b"A".to_vec(), vec![0x80 | b'A']];
        room.status.figures[0] = [(0x24, 0xff, true), (0xdc, 2, false), (0xdc, 1, false)]
            .into_iter()
            .enumerate()
            .map(|(i, (field, mask, percent))| Figure {
                x: 10,
                y: 10 + 10 * i as u16,
                field,
                mask,
                percent,
            })
            .collect();
        let font = roster_font();
        let mut fb = Framebuffer::new(Size::new(320, 200));
        draw(&mut fb, &room, &font, unit(0, parts(50, 2)), |_| None);
        let inked =
            |y: usize, colour| (y..y + 6).any(|y| (10..40).any(|x| fb.get(x, y) == Some(colour)));
        // Bit 1 set: the second text, in the second pair; bit 0 clear: the first.
        assert!(inked(20, HIGH_COLOUR) && !inked(20, COLOUR));
        assert!(inked(30, COLOUR) && !inked(30, HIGH_COLOUR));
    }
}
