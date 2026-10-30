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
/// (0xa6), the power (0xb0) and the target panel (0xa2) draw at fixed places.
pub const WARNING_TYPE: u16 = 0xa0;
pub const GEAR_TYPE: u16 = 0xa4;
pub const STATUS_TYPE: u16 = 0xa8;
/// The buttons: fire, a weapon (twice), pilot, program, lock, cloak, ECM, turn left,
/// straight and right.
pub const FIRE_TYPE: u16 = 0x94;
pub const WEAPON_TYPE: u16 = 0x96;
pub const PILOT_TYPE: u16 = 0x98;
pub const PROGRAM_TYPE: u16 = 0x9a;
pub const LOCK_TYPE: u16 = 0x9e;
pub const CLOAK_TYPE: u16 = 0xac;
pub const ECM_TYPE: u16 = 0xae;
pub const LEFT_TYPE: u16 = 0xb2;
pub const AHEAD_TYPE: u16 = 0xb4;
pub const RIGHT_TYPE: u16 = 0xb6;
/// `1000:dfd5`: a pressed switch's frame is XORed with 9.
pub const PRESSED_XOR: u8 = 9;
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
}

/// What the console keeps of a press: +0x22 bit 3, the buttons' shared look, and bit
/// 7, the lock button held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
    pub pressed: bool,
    pub lock_held: bool,
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

/// How often the tick table's entry 4 (`1000:b318`) ran in the `passes` object passes
/// since the load: on each pass whose stepped +0x1a has `& 0x1e` = 8.
// shortcut: the sim does not keep +0xee, +0xfa or +0x10a; this assumes the robot ticked
// on every pass since the load, and goes wrong once `World::ticks` wraps (T-167).
fn calls(loaded_tick: u8, passes: u16) -> u16 {
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

/// +0xee as it stands `passes` object passes after the load: 2 a call of entry 4.
pub fn blink(loaded_tick: u8, loaded_blink: u8, passes: u16) -> u8 {
    loaded_blink.wrapping_add((calls(loaded_tick, passes) as u8).wrapping_mul(2))
}

/// The status panel's page `passes` object passes after the load: each call of entry
/// 4 steps +0x10a when +0xfa's low five bits are 0, then +0xfa; the panel cuts +0x10a
/// by 3 when 3 or more.
// shortcut: with the console open the draw cuts 3 to 0 before the next step, so the
// pages go round in threes; closed, the original's +0x10a runs 0 to 3 (T-167).
pub fn page(loaded_tick: u8, loaded_clock: u8, loaded_light: u8, passes: u16) -> u8 {
    let calls = calls(loaded_tick, passes);
    let first = u16::from(loaded_clock.wrapping_neg() & 0x1f);
    let steps = if calls > first {
        (calls - 1 - first) / 32 + 1
    } else {
        0
    };
    let light = if loaded_light >= 3 { 0 } else { loaded_light };
    ((u16::from(light) + steps) % 3) as u8
}

/// `1000:a466`: the target panel's colours, 0x57 (0xff for a warning, 0x5c for a
/// weapon's state) on 0x68, and where its two parts start.
const TARGET_COLOUR: u8 = 0x57;
const TARGET_WARNING: u8 = 0xff;
const TARGET_STATE: u8 = 0x5c;
const TARGET_BACKGROUND: u8 = 0x68;
const TARGET_AT: (i32, i32) = (0x116, 0x71);
const WEAPONS_AT: (i32, i32) = (0x116, 0x88);
/// +0xec bits 1 and 2: in range, in the arc.
const IN_RANGE: u8 = 0x02;
const IN_ARC: u8 = 0x04;
/// `1000:a466`'s immediates: after the distance `M`, after the bearing glyph 0x17.
const METRES: u8 = b'M';
const DEGREE: u8 = 0x17;
const NEW_LINE: u8 = 0x0c;

/// The target panel (`0xa2`): the target's name, distance and bearing, or the no-target
/// lines; then each weapon's state.
pub fn draw_target(fb: &mut Framebuffer, font: &Font, con: &HumanCon, robot: &Robot) {
    let mut cursor = TextCursor::at(0, 0, TARGET_COLOUR);
    cursor.background = TARGET_BACKGROUND;
    for (at, colour, text) in target_lines(con, robot) {
        if let Some((x, y)) = at {
            cursor.move_to(x, y);
        }
        cursor.colour = colour;
        fb.draw_text(font, &mut cursor, &text, TextMode::Opaque);
    }
}

/// A piece of text with its colour: where it starts (none: where the last left the
/// cursor), the colour, what it says.
type Coloured = (Option<(i32, i32)>, u8, Vec<u8>);

/// The target panel's texts in order, all opaque.
fn target_lines(con: &HumanCon, robot: &Robot) -> Vec<Coloured> {
    let warn = |bit: u8| {
        if robot.targeting & bit == 0 {
            TARGET_WARNING
        } else {
            TARGET_COLOUR
        }
    };
    let mut lines = match &robot.target {
        Some(name) => {
            let mut bearing = digits(crate::editor::degrees(robot.target_bearing), 3);
            bearing.extend([DEGREE, NEW_LINE]);
            vec![
                (Some(TARGET_AT), TARGET_COLOUR, name.clone()),
                (None, warn(IN_RANGE), distance(robot.target_distance)),
                (None, warn(IN_ARC), bearing),
            ]
        }
        None => vec![(Some(TARGET_AT), TARGET_COLOUR, con.no_target.clone())],
    };
    for (w, bit) in [0x01, 0x02].into_iter().enumerate() {
        let (colour, state) = if robot.weapons & bit != 0 {
            (TARGET_STATE, &con.damaged)
        } else if robot.cooldowns[w] != 0 {
            (TARGET_STATE, &con.loading)
        } else {
            (TARGET_COLOUR, &con.ready)
        };
        lines.push(((w == 0).then_some(WEAPONS_AT), colour, state.clone()));
    }
    lines
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

/// `1000:a93e`: the status panel's heading and lines.
const STATUS_HEADING_AT: (i32, i32) = (0x113, 7);
const STATUS_LEFT_AT: (i32, i32) = (0x12a, 0x13);
const STATUS_RIGHT_AT: (i32, i32) = (0x12e, 0x13);
const STATUS_COUNTS_AT: (i32, i32) = (0x12a, 0x25);
/// What `0x71d1`'s bit-7 codes are drawn in: whatever the situation room left in the
/// text state's second pair.
// shortcut: the briefing's and status mode's 0x17 on 0 (`1fe6:700d`); after the log
// mode (C) the original has 0xff on 0x10.
const OUT_COLOURS: (u8, u8) = (0x17, 0);

/// The status panel (`0xa8`): filled in the kind's message-panel colours, the page's
/// heading and its lines.
pub fn draw_status(
    fb: &mut Framebuffer,
    area: Rect,
    font: &Font,
    con: &HumanCon,
    [fill, colour]: [u8; 2],
    robot: &Robot,
) {
    crate::mission::fill(fb, area, fill);
    let mut cursor = TextCursor::at(0, 0, colour);
    (
        cursor.background,
        cursor.high_colour,
        cursor.high_background,
    ) = (fill, OUT_COLOURS.0, OUT_COLOURS.1);
    for (at, text, mode) in status_lines(con, robot) {
        if let Some((x, y)) = at {
            cursor.move_to(x, y);
        }
        fb.draw_text(font, &mut cursor, &text, mode);
    }
}

/// A piece of text: where it starts (none: where the last left the cursor), what it
/// says, how it is drawn.
type Line = (Option<(i32, i32)>, Vec<u8>, TextMode);

/// The status panel's page: its heading, then page 0 the percentages of life support
/// (always 100), power and helm and the targeting computer's and communications'
/// (always working) states; page 1 the weapons' states and the cloaks and ECM charges
/// left; page 2 the sensors' states.
fn status_lines(con: &HumanCon, robot: &Robot) -> Vec<Line> {
    use TextMode::{Opaque, Transparent};
    let page = usize::from(robot.page).min(con.pages.len() - 1);
    let state = |broken: bool| {
        if broken {
            con.out.clone()
        } else {
            con.ok.clone()
        }
    };
    let percent = crate::panels::percent;
    let mut lines = vec![(
        Some(STATUS_HEADING_AT),
        con.pages[page].clone(),
        Transparent,
    )];
    let (first, rest): (_, Vec<Vec<u8>>) = match page {
        0 => (
            STATUS_LEFT_AT,
            vec![
                percent(100),
                percent(robot.power),
                percent(robot.helm),
                state(robot.targeting & 1 != 0),
                state(false),
            ],
        ),
        1 => (
            STATUS_RIGHT_AT,
            vec![state(robot.weapons & 1 != 0), state(robot.weapons & 2 != 0)],
        ),
        _ => (
            STATUS_RIGHT_AT,
            [0x01, 0x02, 0x04, 0x08, 0x10]
                .map(|bit| state(robot.systems & bit != 0))
                .to_vec(),
        ),
    };
    for (i, text) in rest.into_iter().enumerate() {
        lines.push(((i == 0).then_some(first), text, Opaque));
    }
    if page == 1 {
        lines.push((
            Some(STATUS_COUNTS_AT),
            count(con, robot.cloaks),
            Transparent,
        ));
        lines.push((None, count(con, robot.ecm), Transparent));
    }
    lines
}

/// `1000:de24`: none left is OUT; else the last two digits, a leading zero a space,
/// and a new line.
fn count(con: &HumanCon, n: u8) -> Vec<u8> {
    if n == 0 {
        return con.out.clone();
    }
    let mut text = digits(n.into(), 2);
    if text[0] == b'0' {
        text[0] = b' ';
    }
    text.push(NEW_LINE);
    text
}

/// `1000:a30c` to `aa81`: the buttons' sprites and where each goes from its
/// rectangle's corner.
const FIRE: (u16, i16, i16) = (0x530, 1, 1);
const CLOAK: (u16, i16, i16) = (0x580, 0, 0);
const ECM: (u16, i16, i16) = (0x590, 0, 0);
const LEFT: (u16, i16, i16) = (0x5c8, 1, 1);
const AHEAD: (u16, i16, i16) = (0x5d8, 0, 1);
const RIGHT: (u16, i16, i16) = (0x5e8, 1, 1);
/// Pilot (lit while the program drives) and program (lit while the pilot drives) at
/// -1; the lock button at its corner.
const PILOT: [u16; 2] = [0x548, 0x540];
const PROGRAM: [u16; 2] = [0x550, 0x558];
const LOCK: u16 = 0x560;

/// A button (`kind`, its rectangle and, for a weapon button, its weapon) as its draw
/// handler draws it.
pub fn draw_button(
    fb: &mut Framebuffer,
    sprites: Sprites,
    con: &HumanCon,
    (kind, area, weapon): (u16, Rect, u8),
    robot: &Robot,
    look: Look,
) {
    if let Some((handle, dx, dy)) = button(con, kind, weapon, robot, look) {
        let (x, y) = (
            area.left.wrapping_add_signed(dx),
            area.top.wrapping_add_signed(dy),
        );
        sprite(fb, sprites, handle, x, y, BlitMode::Opaque);
    }
}

/// A button's sprite and its offset from the rectangle's corner; none for other kinds.
fn button(
    con: &HumanCon,
    kind: u16,
    weapon: u8,
    robot: &Robot,
    look: Look,
) -> Option<(u16, i16, i16)> {
    let pressed = if look.pressed { NEXT } else { 0 };
    // shortcut: the original adds all of +0x22, so while the lock button or the
    // program panel is held the shared-look buttons show another sprite; they are not
    // drawn again until marked then.
    let shared = |(base, dx, dy): (u16, i16, i16)| Some((base + pressed, dx, dy));
    match kind {
        FIRE_TYPE => shared(FIRE),
        CLOAK_TYPE => shared(CLOAK),
        ECM_TYPE => shared(ECM),
        LEFT_TYPE => shared(LEFT),
        AHEAD_TYPE => shared(AHEAD),
        RIGHT_TYPE => shared(RIGHT),
        WEAPON_TYPE => {
            let sprites = con.weapon_sprites[usize::from(weapon == robot.weapon)];
            Some((sprites[usize::from(weapon & 1)], -1, -1))
        }
        PILOT_TYPE => Some((PILOT[usize::from(robot.program_drives)], -1, -1)),
        PROGRAM_TYPE => Some((PROGRAM[usize::from(!robot.program_drives)], -1, -1)),
        LOCK_TYPE => {
            let lit = robot.target.is_some() != (look.lock_held && look.pressed);
            Some((LOCK + if lit { NEXT } else { 0 }, 0, 0))
        }
        _ => None,
    }
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
    fn the_target_panel_warns_out_of_range_and_arc() {
        let mut con = con();
        (con.no_target, con.damaged, con.loading, con.ready) = (
            b"NONE".to_vec(),
            b"D".to_vec(),
            b"L".to_vec(),
            b"R".to_vec(),
        );
        let mut robot = Robot {
            cooldowns: [0, 3],
            ..Robot::default()
        };
        let lines = target_lines(&con, &robot);
        assert_eq!(
            lines,
            [
                (Some(TARGET_AT), TARGET_COLOUR, b"NONE".to_vec()),
                (Some(WEAPONS_AT), TARGET_COLOUR, b"R".to_vec()),
                (None, TARGET_STATE, b"L".to_vec()),
            ]
        );
        (robot.target, robot.target_distance, robot.target_bearing) =
            (Some(b"AA-001\x0c".to_vec()), 640, 0xc000);
        (robot.targeting, robot.weapons) = (IN_ARC, 0x02);
        let lines = target_lines(&con, &robot);
        assert_eq!(lines[1], (None, TARGET_WARNING, b"  640M\x0c".to_vec()));
        assert_eq!(lines[2], (None, TARGET_COLOUR, b"270\x17\x0c".to_vec()));
        assert_eq!(lines[4], (None, TARGET_STATE, b"D".to_vec()));
        assert_eq!(distance(0), b"    0M\x0c");
    }

    #[test]
    fn the_status_pages() {
        let mut con = con();
        (con.ok, con.out) = (b"Y".to_vec(), b"N".to_vec());
        con.pages = [b"P0".to_vec(), b"P1".to_vec(), b"P2".to_vec()];
        let mut robot = Robot {
            power: 90,
            helm: 7,
            targeting: 1,
            weapons: 2,
            cloaks: 4,
            systems: 0x0a,
            ..Robot::default()
        };
        let texts = |robot: &Robot| -> Vec<Vec<u8>> {
            status_lines(&con, robot).into_iter().map(|l| l.1).collect()
        };
        assert_eq!(
            texts(&robot),
            [
                &b"P0"[..],
                b"100%\x0c",
                b"090%\x0c",
                b"007%\x0c",
                b"N",
                b"Y"
            ]
        );
        robot.page = 1;
        assert_eq!(texts(&robot), [&b"P1"[..], b"Y", b"N", b" 4\x0c", b"N"]);
        let lines = status_lines(&con, &robot);
        assert_eq!(
            (lines[3].0, lines[3].2),
            (Some(STATUS_COUNTS_AT), TextMode::Transparent)
        );
        robot.page = 2;
        assert_eq!(texts(&robot)[1..], [&b"Y"[..], b"N", b"Y", b"N", b"Y"]);
    }

    #[test]
    fn the_buttons_follow_the_robot_and_the_look() {
        let con = con();
        let mut robot = Robot::default();
        let mut look = Look::default();
        let handle =
            |robot: &Robot, look, kind, weapon| button(&con, kind, weapon, robot, look).unwrap().0;
        assert_eq!(handle(&robot, look, FIRE_TYPE, 0), 0x530);
        assert_eq!(handle(&robot, look, LOCK_TYPE, 0), LOCK);
        // The weapon selected shows its lit sprite.
        robot.weapon = 1;
        assert_eq!(
            (
                handle(&robot, look, WEAPON_TYPE, 0),
                handle(&robot, look, WEAPON_TYPE, 1)
            ),
            (1, 4)
        );
        assert_eq!(
            (
                handle(&robot, look, PILOT_TYPE, 0),
                handle(&robot, look, PROGRAM_TYPE, 0)
            ),
            (0x548, 0x558)
        );
        robot.program_drives = true;
        assert_eq!(
            (
                handle(&robot, look, PILOT_TYPE, 0),
                handle(&robot, look, PROGRAM_TYPE, 0)
            ),
            (0x540, 0x550)
        );
        // Pressed: the shared look; the lock button held shows the other light.
        look.pressed = true;
        assert_eq!(handle(&robot, look, AHEAD_TYPE, 0), 0x5d8 + NEXT);
        assert_eq!(
            button(&con, AHEAD_TYPE, 0, &robot, look).map(|b| (b.1, b.2)),
            Some((0, 1))
        );
        look.lock_held = true;
        assert_eq!(handle(&robot, look, LOCK_TYPE, 0), LOCK + NEXT);
        robot.target = Some(Vec::new());
        assert_eq!(handle(&robot, look, LOCK_TYPE, 0), LOCK);
        assert!(button(&con, 0xa2, 0, &robot, look).is_none());
    }

    #[test]
    fn the_pages_turn_every_512_passes() {
        // +0x1a loaded at 6: entry 4 on pass 1, then every 16; +0xfa at 31 steps the
        // light on the second call (pass 17), then every 32 calls.
        assert_eq!((page(6, 31, 0, 16), page(6, 31, 0, 17)), (0, 1));
        assert_eq!((page(6, 31, 0, 528), page(6, 31, 0, 529)), (1, 2));
        assert_eq!(page(6, 31, 2, 17), 0);
        // A light of 3 is cut to 0 by the first draw.
        assert_eq!(page(6, 0, 3, 0), 0);
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
