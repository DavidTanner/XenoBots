//! What the robot consoles' instruments share (T-130 to T-132; `docs/screens.md`, "The
//! humanoid console's instruments (T-130)" and the scorpion's and scout's): what they read from the viewed robot, the buttons' shared look, the
//! sprite blits, the lit numbers, the compass's needle, the power bars and the
//! counters of the tick table's entry 4.

use xeno_formats::ban::Sprite;
use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_render::{BlitMode, Framebuffer, Orientation, SpriteBlit, TextCursor, TextMode};

use crate::panels::digits;

/// The font of the consoles' numbers and panels (`mov ax, 2` before
/// `gfx_select_font`).
pub const FONT: u16 = 2;
/// `1000:dfd5`: a pressed switch's frame is XORed with 9.
pub const PRESSED_XOR: u8 = 9;
/// The next sprite: a lit or pressed look.
pub const NEXT: u16 = 8;

/// What the instruments read from the viewed robot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// The target's name (`1fe6:7f38`), none without one; +0xe8, +0xea less the
    /// heading, +0xec.
    pub target: Option<Vec<u8>>,
    pub target_distance: u16,
    pub target_bearing: u16,
    pub targeting: u8,
    /// +0xdc, +0x108 and +0x109, +0xdd.
    pub weapons: u8,
    pub cooldowns: [u16; 2],
    pub weapon: u8,
    /// +0x4e, +0x24, +0xf6, +0xe0, +0xde.
    pub power: u8,
    pub helm: u8,
    pub systems: u8,
    pub cloaks: u8,
    pub ecm: u8,
    /// +0x23 bit 7.
    pub program_drives: bool,
    /// The status panel's page (+0x10a, cut by 3).
    pub page: u8,
    /// A scorpion's relays (+0x118), cannon rounds (+0x120), cargo gauges (+0x11a,
    /// +0x11b), its deployer damaged (+0xfd bit 7) and +0xfa bit 7 (the damage panel
    /// shows its lights).
    pub relays: u8,
    pub rounds: u16,
    pub gauges: [u8; 2],
    pub deployer_damaged: bool,
    pub damage_shown: bool,
    /// A scout's stance (+0x10b).
    pub stance: u8,
}

/// What the console keeps of a press: +0x22 bit 3, the buttons' shared look, and bit
/// 7, the lock button held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    pub pressed: bool,
    pub lock_held: bool,
}

/// A console's sprites by handle.
pub type Sprites<'a> = &'a dyn Fn(u16) -> Option<&'a Sprite>;

/// The sprite at `handle` in a bank loaded from handle `first`, 8 handles a sprite.
pub fn lookup(bank: &[Sprite], first: u16, handle: u16) -> Option<&Sprite> {
    let offset = handle.checked_sub(first)?;
    offset
        .is_multiple_of(NEXT)
        .then(|| bank.get(usize::from(offset / NEXT)))
        .flatten()
}

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

/// `1000:a6f4`-style numbers: the `unlit` digits in the first colour, then `text` over
/// them in the second, both transparent.
pub fn lit_number(
    fb: &mut Framebuffer,
    font: &Font,
    at: (i32, i32),
    unlit: &[u8],
    (dark, lit): (u8, u8),
    text: &[u8],
) {
    let mut cursor = TextCursor::at(at.0, at.1, dark);
    fb.draw_text(font, &mut cursor, unlit, TextMode::Transparent);
    cursor.colour = lit;
    fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
}

/// `02b7`'s five digits of the speed table's entry by the speed (negated if negative)
/// from the third, the third a space or `-`.
pub fn speed_text(speeds: &[u8], speed: i16) -> Vec<u8> {
    // `mov al, [bx + table]` by the speed (negated if negative); past the table the
    // original reads on. shortcut: the port shows the table's last speed there, as
    // the detail panel does; no stock robot goes so fast.
    let size = usize::from(speed.unsigned_abs());
    let shown = speeds.get(size).or(speeds.last()).copied().unwrap_or(0);
    let mut text = digits(shown.into(), 3);
    text[0] = if speed < 0 { b'-' } else { b' ' };
    text
}

/// The heading's needle (`1000:a617`, `87d6`, `b8b8`): sprite `first` plus 8 a needle,
/// keyed and turned by its quadrant in that quadrant's rectangle.
pub fn draw_needle(
    fb: &mut Framebuffer,
    sprites: Sprites,
    (needles, areas): (&[u16], [Rect; 4]),
    first: u16,
    heading: u16,
) {
    let (quadrant, handle) = needle(needles, first, heading);
    if let Some(s) = sprites(handle) {
        let (orientation, x, y) = turn(areas[quadrant], quadrant, s);
        let blit = SpriteBlit {
            mode: BlitMode::Keyed,
            orientation,
            ..SpriteBlit::default()
        };
        fb.blit_sprite(s, x.into(), y.into(), blit);
    }
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

/// The quadrant and needle sprite for `heading`: the nearest of the table's headings
/// (32 of them: `((heading + 0x400) >> 11) & 31`; the scout's 64: `((heading + 0x200)
/// >> 10) & 63`), the low byte the quadrant's word offset, the high byte the needle
/// (`cbw`, times 8).
fn needle(needles: &[u16], first: u16, heading: u16) -> (usize, u16) {
    let shift = 16 - needles.len().trailing_zeros();
    let i = heading.wrapping_add(1 << (shift - 1)) >> shift;
    let entry = needles[usize::from(i) % needles.len()];
    let quadrant = usize::from((entry & 0xff) as u8 / 2) % 4;
    let n = (entry >> 8) as u8 as i8 as i16;
    (quadrant, first.wrapping_add_signed(n * 8))
}

/// The lock warning light (`1000:a446`, `8a71`): sprite `base`, the next one while
/// another robot's lock holds and the blink counter's bit 2 is set.
pub fn draw_warning(fb: &mut Framebuffer, area: Rect, sprites: Sprites, base: u16, robot: &Robot) {
    let lit = robot.locked_on != 0 && robot.blink & 0x04 != 0;
    let handle = base + if lit { NEXT } else { 0 };
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// The humanoid's and scorpion's bars run right from the corner, a segment every 2
/// pixels; the scout's (`1000:c3ac`) up from 30 rows below it, every 2 rows.
pub const BAR_RIGHT: [(i16, i16); 2] = [(0, 0), (2, 0)];
pub const BAR_UP: [(i16, i16); 2] = [(0, 30), (0, -2)];

/// The power panel (`1000:aab7`, `8fb9`, `c360`): each light's sprite, then its bar of
/// the signal and of the energy: `lit` segments, then `dark` ones, the first and each
/// next one placed by `[first, step]`.
pub fn draw_power(
    fb: &mut Framebuffer,
    sprites: Sprites,
    (lights, bars): ([Rect; 2], [Rect; 2]),
    (handles, [lit, dark]): ([u16; 2], [u16; 2]),
    [first, step]: [(i16, i16); 2],
    robot: &Robot,
) {
    let values = [robot.signal, robot.energy];
    for i in 0..2 {
        let (area, bar) = (lights[i], bars[i]);
        sprite(
            fb,
            sprites,
            handles[i],
            area.left,
            area.top,
            BlitMode::Opaque,
        );
        let n = segments(values[i]);
        for k in 0..SEGMENTS as i16 {
            let handle = if k < n as i16 { lit } else { dark };
            let x = bar.left.wrapping_add_signed(first.0 + k * step.0);
            let y = bar.top.wrapping_add_signed(first.1 + k * step.1);
            sprite(fb, sprites, handle, x, y, BlitMode::Opaque);
        }
    }
}

/// A bar's segments.
const SEGMENTS: u16 = 16;

/// `1000:ab02`, `9004`: 255 counts as 256, then `(value >> 1) >> 3` lit segments.
fn segments(value: u8) -> u16 {
    let v = if value == 0xff {
        0x100
    } else {
        u16::from(value)
    };
    (v >> 1) >> 3
}

/// +0xec bits 1 and 2: in range, in the arc.
pub const IN_RANGE: u8 = 0x02;
pub const IN_ARC: u8 = 0x04;
/// The target panels' immediates: after the distance `M`, after the bearing glyph
/// 0x17, then a new line.
const METRES: u8 = b'M';
const DEGREE: u8 = 0x17;
pub const NEW_LINE: u8 = 0x0c;

/// A piece of text with its colour: where it starts (none: where the last left the
/// cursor), the colour, what it says.
pub type Coloured = (Option<(i32, i32)>, u8, Vec<u8>);

/// Draws `lines` opaque on `background`.
pub fn draw_lines(fb: &mut Framebuffer, font: &Font, background: u8, lines: Vec<Coloured>) {
    let mut cursor = TextCursor::at(0, 0, 0);
    cursor.background = background;
    for (at, colour, text) in lines {
        if let Some((x, y)) = at {
            cursor.move_to(x, y);
        }
        cursor.colour = colour;
        fb.draw_text(font, &mut cursor, &text, TextMode::Opaque);
    }
}

/// A target panel's first part (`1000:a466`, `88b3`) from `at`: the target's name,
/// its distance and its bearing, each in the warning colour while out of range or of
/// the arc; without a target, the `no_target` lines.
pub fn target_part(
    robot: &Robot,
    no_target: &[u8],
    at: (i32, i32),
    (colour, warning): (u8, u8),
) -> Vec<Coloured> {
    let warn = |bit: u8| {
        if robot.targeting & bit == 0 {
            warning
        } else {
            colour
        }
    };
    match &robot.target {
        Some(name) => {
            let mut bearing = digits(crate::editor::degrees(robot.target_bearing), 3);
            bearing.extend([DEGREE, NEW_LINE]);
            vec![
                (Some(at), colour, name.clone()),
                (None, warn(IN_RANGE), distance(robot.target_distance)),
                (None, warn(IN_ARC), bearing),
            ]
        }
        None => vec![(Some(at), colour, no_target.to_vec())],
    }
}

/// `02b7`'s five digits with leading zeros as spaces (`1000:0301`, the last kept),
/// `M` and a new line.
fn distance(value: u16) -> Vec<u8> {
    let mut text = digits(value, 5);
    for d in text.iter_mut().take(4) {
        if *d != b'0' {
            break;
        }
        *d = b' ';
    }
    text.extend([METRES, NEW_LINE]);
    text
}

/// How often the tick table's entry 4 (humanoid `1000:b318`, scorpion `9a14`) ran in
/// the `passes` object passes since the load: on each pass whose stepped +0x1a has
/// `& 0x1e` = 8.
// shortcut: the sim does not keep +0xee, +0xfa or +0x10a; this assumes the robot ticked
// on every pass since the load, and goes wrong once `World::ticks` wraps (T-167).
pub fn calls(loaded_tick: u8, passes: u16) -> u16 {
    // The `j`th pass steps +0x1a to `loaded_tick + 2j`: entry 4 when `j` is `r` mod 16.
    let r = match 4u16.wrapping_sub(u16::from(loaded_tick >> 1)) & 15 {
        0 => 16,
        r => r,
    };
    if passes >= r {
        (passes - r) / 16 + 1
    } else {
        0
    }
}

/// The status panel's page `passes` object passes after the load, of `pages`: each call
/// of entry 4 steps the page (humanoid +0x10a, scout +0x108) when +0xfa's low five bits
/// are 0, then +0xfa. The humanoid's panel cuts its page by 3 when 3 or more; the
/// scout's goes round all four (`1000:caf5`).
// shortcut: with the humanoid's console open the draw cuts 3 to 0 before the next step,
// so its pages go round in threes; closed, the original's +0x10a runs 0 to 3 (T-167).
pub fn page(loaded_tick: u8, loaded_clock: u8, loaded_page: u8, passes: u16, pages: u8) -> u8 {
    let calls = calls(loaded_tick, passes);
    let first = u16::from(loaded_clock.wrapping_neg() & 0x1f);
    let steps = if calls > first {
        (calls - 1 - first) / 32 + 1
    } else {
        0
    };
    ((u16::from(loaded_page) + steps) % u16::from(pages)) as u8
}

/// +0xee as it stands `passes` object passes after the load: 2 a call of entry 4.
pub fn blink(loaded_tick: u8, loaded_blink: u8, passes: u16) -> u8 {
    loaded_blink.wrapping_add((calls(loaded_tick, passes) as u8).wrapping_mul(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quadrants() -> [Rect; 4] {
        let r = |top, left, bottom, right| Rect {
            top,
            left,
            bottom,
            right,
        };
        [r(0, 2, 2, 4), r(2, 2, 4, 4), r(2, 0, 4, 2), r(0, 0, 2, 2)]
    }

    #[test]
    fn needles_speeds_and_segments() {
        let needles: [u16; 32] = std::array::from_fn(|i| match i {
            0 => 0,
            1 => 0x0102,
            2 => 0x0204,
            _ => 0xff06,
        });
        const FIRST: u16 = 0x5f8;
        // 0x400 either side of straight up is the first needle.
        assert_eq!(needle(&needles, FIRST, 0xfc00), (0, FIRST));
        assert_eq!(needle(&needles, FIRST, 0x0400), (1, FIRST + 8));
        assert_eq!(needle(&needles, FIRST, 0x0c00), (2, FIRST + 16));
        // A negative needle counts down.
        assert_eq!(needle(&needles, FIRST, 0x1400), (3, FIRST - 8));
        // The scout's 64 headings: 0x200 either side of each.
        let scout: [u16; 64] = std::array::from_fn(|i| (i as u16) << 8);
        assert_eq!(needle(&scout, 0, 0x01ff), (0, 0));
        assert_eq!(needle(&scout, 0, 0x0200), (0, 8));
        assert_eq!(needle(&scout, 0, 0xfe00), (0, 0));
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
        let q = quadrants();
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

    #[test]
    fn the_pages_turn_every_512_passes() {
        // +0x1a loaded at 6: entry 4 on pass 1, then every 16; +0xfa at 31 steps the
        // page on the second call (pass 17), then every 32 calls.
        assert_eq!((page(6, 31, 0, 16, 3), page(6, 31, 0, 17, 3)), (0, 1));
        assert_eq!((page(6, 31, 0, 528, 3), page(6, 31, 0, 529, 3)), (1, 2));
        assert_eq!(page(6, 31, 2, 17, 3), 0);
        // The humanoid's page of 3 is cut to 0 by the first draw; the scout's shows.
        assert_eq!((page(6, 0, 3, 0, 3), page(6, 0, 3, 0, 4)), (0, 3));
        assert_eq!(page(6, 31, 3, 17, 4), 0);
    }

    #[test]
    fn distances_keep_their_last_digit() {
        assert_eq!(distance(0), b"    0M\x0c");
        assert_eq!(distance(1203), b" 1203M\x0c");
    }
}
