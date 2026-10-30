//! The humanoid console's instruments (T-130; `docs/screens.md`, "The humanoid
//! console's instruments (T-130)"), drawn over the picture as their draw handlers draw
//! them, from the viewed robot. The texts, tables and rectangles are read from the
//! user's `xeno.exe` (`xeno_formats::humancon`); sprite handles, places and colours
//! are the handlers' immediates, short code constants kept here.

use xeno_formats::ban::Sprite;
use xeno_formats::font::Font;
use xeno_formats::humancon::HumanCon;
use xeno_formats::maproom::Rect;
use xeno_render::{BlitMode, Framebuffer, Orientation, SpriteBlit, TextCursor, TextMode};

use crate::panels::digits;

/// The widgets drawn at their rectangles, by type; the speed (0x9c), the compass
/// (0xa6) and the power (0xb0) draw at fixed places.
pub const WARNING_TYPE: u16 = 0xa0;
pub const GEAR_TYPE: u16 = 0xa4;
/// The font of the speed and the compass's heading (`mov ax, 2` before
/// `gfx_select_font`).
pub const FONT: u16 = 2;
/// `1000:a6f4`, `a617`: the unlit digits in 0x58, the number in 0x0f, transparent; the
/// speed at (0x4d, 0x9f), the heading at (0x5a, 0x78).
const UNLIT_COLOUR: u8 = 0x58;
const NUMBER_COLOUR: u8 = 0x0f;
const SPEED_AT: (i32, i32) = (0x4d, 0x9f);
const HEADING_AT: (i32, i32) = (0x5a, 0x78);
/// `1000:a617`: the dial and the first needle (eight handles a needle).
const DIAL: u16 = 0x5c0;
const NEEDLE: u16 = 0x5f8;
/// `1000:a446`: the warning light, and its lit look while +0xee bit 2.
const WARNING: u16 = 0x570;
const BLINK: u8 = 0x04;
/// `1000:a874`: the gear lever's first sprite, 4 handles a gear step.
const GEAR: u16 = 0x640;
/// `1000:aab7`: the power lights (battery on, off), the bars' lit and dark segments,
/// 16 of them 2 pixels apart.
const LIGHT_ON: u16 = 0x5a0;
const LIGHT_OFF: u16 = 0x5a8;
const LIT: u16 = 0x5b8;
const DARK: u16 = 0x5b0;
const SEGMENTS: u16 = 16;
/// The next sprite: a lit or pressed look.
pub const NEXT: u16 = 8;

/// What the instruments read from the viewed robot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Robot {
    /// +0x10, +0x26, +0x10b (twice the gear's index).
    pub heading: u16,
    pub speed: i16,
    pub gear: u8,
    /// +0xfd bit 0; +0x4f, +0x50.
    pub on_battery: bool,
    pub signal: u8,
    pub energy: u8,
    /// +0xed, +0xee.
    pub locked_on: u8,
    pub blink: u8,
}

/// The console's sprites (`msprite2`) by handle.
pub type Sprites<'a> = &'a dyn Fn(u16) -> Option<&'a Sprite>;

/// Draws sprite `handle` with its corner at (`x`, `y`).
pub fn sprite(fb: &mut Framebuffer, sprites: Sprites, handle: u16, x: u16, y: u16, mode: BlitMode) {
    if let Some(s) = sprites(handle) {
        let blit = SpriteBlit {
            mode,
            ..SpriteBlit::default()
        };
        fb.blit_sprite(s, x.into(), y.into(), blit);
    }
}

/// `1000:a466`-style numbers: the unlit digits, then `text` over them in
/// [`NUMBER_COLOUR`], both transparent.
fn lit_number(fb: &mut Framebuffer, font: &Font, con: &HumanCon, at: (i32, i32), text: &[u8]) {
    let mut cursor = TextCursor::at(at.0, at.1, UNLIT_COLOUR);
    fb.draw_text(font, &mut cursor, &con.unlit, TextMode::Transparent);
    cursor.colour = NUMBER_COLOUR;
    fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
}

/// The speed (`0x9c`): a sign and two digits of the speed table's entry.
pub fn draw_speed(fb: &mut Framebuffer, font: &Font, con: &HumanCon, speeds: &[u8], speed: i16) {
    lit_number(fb, font, con, SPEED_AT, &speed_text(speeds, speed));
}

/// `02b7`'s five digits of the table's entry from the third, the third a space or
/// `-`.
fn speed_text(speeds: &[u8], speed: i16) -> Vec<u8> {
    // `mov al, [bx + table]` by the speed (negated if negative); past the table the
    // original reads on. shortcut: the port shows the table's last speed there, as
    // the detail panel does; no stock robot goes so fast.
    let size = usize::from(speed.unsigned_abs());
    let shown = speeds.get(size).or(speeds.last()).copied().unwrap_or(0);
    let mut text = digits(shown.into(), 3);
    text[0] = if speed < 0 { b'-' } else { b' ' };
    text
}

/// The compass (`0xa6`): the dial, the heading's needle in its quadrant, the heading in
/// degrees.
pub fn draw_compass(
    fb: &mut Framebuffer,
    font: &Font,
    con: &HumanCon,
    sprites: Sprites,
    heading: u16,
) {
    sprite(
        fb,
        sprites,
        DIAL,
        con.dial.left,
        con.dial.top,
        BlitMode::Keyed,
    );
    let (quadrant, handle) = needle(con, heading);
    if let Some(s) = sprites(handle) {
        let (orientation, x, y) = turn(con.quadrants[quadrant], quadrant, s);
        let blit = SpriteBlit {
            mode: BlitMode::Keyed,
            orientation,
            ..SpriteBlit::default()
        };
        fb.blit_sprite(s, x.into(), y.into(), blit);
    }
    let degrees = crate::editor::degrees(heading);
    lit_number(fb, font, con, HEADING_AT, &digits(degrees, 3));
}

/// How quadrant `quadrant`'s blitter turns the needle (slots `0x5d8`, `0x5dc`, `0x5e4`,
/// `0x5e0`) and where its corner lands: the flipped blitters start from the
/// rectangle's bottom or right edge.
fn turn(area: Rect, quadrant: usize, s: &Sprite) -> (Orientation, u16, u16) {
    let (w, h) = (s.width() as u16, s.height() as u16);
    let orientation = [
        Orientation::Normal,
        Orientation::FlipVertical,
        Orientation::Rotate180,
        Orientation::Mirror,
    ][quadrant];
    let x = match orientation {
        Orientation::Mirror | Orientation::Rotate180 => area.right.wrapping_sub(w),
        _ => area.left,
    };
    let y = match orientation {
        Orientation::FlipVertical | Orientation::Rotate180 => area.bottom.wrapping_sub(h),
        _ => area.top,
    };
    (orientation, x, y)
}

/// The quadrant and needle sprite for `heading`: `((heading + 0x400) >> 11) & 31`
/// into the needle table, the low byte the quadrant's word offset, the high byte the
/// needle (`cbw`, times 8).
fn needle(con: &HumanCon, heading: u16) -> (usize, u16) {
    let entry = con.needles[usize::from((heading.wrapping_add(0x400) >> 11) & 31)];
    let quadrant = usize::from((entry & 0xff) as u8 / 2) % 4;
    let n = (entry >> 8) as u8 as i8 as i16;
    (quadrant, NEEDLE.wrapping_add_signed(n * 8))
}

/// The lock warning light (`0xa0`): lit while another robot's lock holds and the blink
/// counter's bit 2 is set.
pub fn draw_warning(fb: &mut Framebuffer, area: Rect, sprites: Sprites, robot: &Robot) {
    let lit = robot.locked_on != 0 && robot.blink & BLINK != 0;
    let handle = WARNING + if lit { NEXT } else { 0 };
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// The gear lever (`0xa4`): one sprite per gear step.
pub fn draw_gear(fb: &mut Framebuffer, area: Rect, sprites: Sprites, gear: u8) {
    let handle = GEAR.wrapping_add_signed(i16::from(gear as i8) * 4);
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// The power panel (`0xb0`): the battery lights and the signal's and energy's bars.
pub fn draw_power(fb: &mut Framebuffer, con: &HumanCon, sprites: Sprites, robot: &Robot) {
    let lights = if robot.on_battery {
        [LIGHT_ON, LIGHT_OFF]
    } else {
        [LIGHT_OFF, LIGHT_ON]
    };
    for ((area, handle), (bar, value)) in con
        .lights
        .iter()
        .zip(lights)
        .zip(con.bars.iter().zip([robot.signal, robot.energy]))
    {
        sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
        let lit = segments(value);
        for i in 0..SEGMENTS {
            let handle = if i < lit { LIT } else { DARK };
            sprite(
                fb,
                sprites,
                handle,
                bar.left + 2 * i,
                bar.top,
                BlitMode::Opaque,
            );
        }
    }
}

/// `1000:ab02`: 255 counts as 256, then `(value >> 1) >> 3` lit segments.
fn segments(value: u8) -> u16 {
    let v = if value == 0xff {
        0x100
    } else {
        u16::from(value)
    };
    (v >> 1) >> 3
}

/// +0xee as it stands `passes` object passes after the load: entry 4 of the tick table
/// (`1000:b318`) adds 2 on each pass whose stepped +0x1a has `& 0x1e` = 8, from +0x1a
/// and +0xee as loaded.
// shortcut: the sim does not keep +0xee; this assumes the robot ticked on every pass
// since the load, and goes wrong once `World::ticks` wraps (T-167).
pub fn blink(loaded_tick: u8, loaded_blink: u8, passes: u16) -> u8 {
    // The `j`th pass steps +0x1a to `loaded_tick + 2j`: entry 4 when `j` is `r` mod 16.
    let r = match 4u16.wrapping_sub(u16::from(loaded_tick >> 1)) & 15 {
        0 => 16,
        r => r,
    };
    let calls = if passes >= r {
        (passes - r) / 16 + 1
    } else {
        0
    };
    loaded_blink.wrapping_add((calls as u8).wrapping_mul(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con() -> HumanCon {
        let r = |top, left, bottom, right| Rect {
            top,
            left,
            bottom,
            right,
        };
        HumanCon {
            bank: 0,
            unlit: b"88\x08\x08".to_vec(),
            no_target: Vec::new(),
            loading: Vec::new(),
            ready: Vec::new(),
            damaged: Vec::new(),
            ok: Vec::new(),
            out: Vec::new(),
            pages: Default::default(),
            base_name: *b"BASE  ",
            dial: r(0, 0, 4, 4),
            needles: std::array::from_fn(|i| match i {
                0 => 0,
                1 => 0x0102,
                2 => 0x0204,
                _ => 0xff06,
            }),
            quadrants: [r(0, 2, 2, 4), r(2, 2, 4, 4), r(2, 0, 4, 2), r(0, 0, 2, 2)],
            weapon_sprites: [[1, 2], [3, 4]],
            lights: [r(0, 0, 1, 1), r(1, 0, 2, 1)],
            bars: [r(0, 1, 1, 33), r(1, 1, 2, 33)],
        }
    }

    #[test]
    fn needles_speeds_and_segments() {
        let con = con();
        // 0x400 either side of straight up is the first needle.
        assert_eq!(needle(&con, 0xfc00), (0, NEEDLE));
        assert_eq!(needle(&con, 0x0400), (1, NEEDLE + 8));
        assert_eq!(needle(&con, 0x0c00), (2, NEEDLE + 16));
        // A negative needle counts down.
        assert_eq!(needle(&con, 0x1400), (3, NEEDLE - 8));
        let speeds = [0, 1, 2, 12];
        assert_eq!(speed_text(&speeds, 3), b" 12");
        assert_eq!(speed_text(&speeds, -2), b"-02");
        assert_eq!(speed_text(&speeds, 99), b" 12");
        assert_eq!(
            [0, 15, 16, 0xfe, 0xff].map(segments),
            [0, 0, 1, 15, SEGMENTS]
        );
    }

    #[test]
    fn each_quadrant_turns_its_needle() {
        use xeno_formats::ban::SpriteHeader;
        let s = Sprite {
            header: SpriteHeader {
                size: 0,
                width: 2,
                height: 3,
                flags: 0,
                unknown_05: 0,
            },
            pixels: vec![1; 6],
            spans: Vec::new(),
            trailing_empty_span: false,
        };
        let q = con().quadrants;
        // Upright, upside down, both, mirrored: clockwise from the top right.
        assert_eq!(turn(q[0], 0, &s), (Orientation::Normal, 2, 0));
        assert_eq!(turn(q[1], 1, &s), (Orientation::FlipVertical, 2, 1));
        assert_eq!(turn(q[2], 2, &s), (Orientation::Rotate180, 0, 1));
        assert_eq!(turn(q[3], 3, &s), (Orientation::Mirror, 0, 0));
    }

    #[test]
    fn the_blink_steps_on_one_pass_in_16() {
        // +0x1a loaded at 2: the 3rd pass steps it to 8.
        assert_eq!(blink(2, 0, 2), 0);
        assert_eq!(blink(2, 0, 3), 2);
        assert_eq!(blink(2, 0, 18), 2);
        assert_eq!(blink(2, 0, 19), 4);
        // Loaded at 8: the 16th pass; an odd +0x1a counts the same.
        assert_eq!((blink(8, 1, 15), blink(9, 1, 16)), (1, 3));
        assert_eq!(blink(0, 0xff, 4), 1);
    }
}
