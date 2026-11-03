//! The humanoid console's instruments (T-130; `docs/screens.md`, "The humanoid
//! console's instruments (T-130)"), drawn over the picture as their draw handlers draw
//! them, from the viewed robot. The texts, tables and rectangles are read from the
//! user's `xeno.exe` (`xeno_formats::humancon`); sprite handles, places and colours
//! are the handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::humancon::HumanCon;
use xeno_formats::maproom::Rect;
use xeno_render::{BlitMode, Framebuffer, TextMode};

use crate::console::{
    Coloured, Line, Look, NEXT, Robot, Sprites, count, draw_lines, sprite, target_part,
};
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
/// `1000:a6f4`, `a617`: the unlit digits in 0x58, the number in 0x0f, transparent; the
/// speed at (0x4d, 0x9f), the heading at (0x5a, 0x78).
const UNLIT_COLOUR: u8 = 0x58;
const NUMBER_COLOUR: u8 = 0x0f;
const SPEED_AT: (i32, i32) = (0x4d, 0x9f);
const HEADING_AT: (i32, i32) = (0x5a, 0x78);
/// `1000:a617`: the dial and the first needle (eight handles a needle).
const DIAL: u16 = 0x5c0;
const NEEDLE: u16 = 0x5f8;
/// `1000:a446`: the warning light (the next one lit).
const WARNING: u16 = 0x570;
/// `1000:a874`: the gear lever's first sprite, 4 handles a gear step.
const GEAR: u16 = 0x640;
/// `1000:aab7`: the power lights (battery on, off), the bars' lit and dark segments,
/// 16 of them 2 pixels apart.
const LIGHT_ON: u16 = 0x5a0;
const LIGHT_OFF: u16 = 0x5a8;
const LIT: u16 = 0x5b8;
const DARK: u16 = 0x5b0;

/// The humanoid's numbers: its unlit digits in 0x58, then `text` in 0x0f.
fn lit_number(fb: &mut Framebuffer, font: &Font, con: &HumanCon, at: (i32, i32), text: &[u8]) {
    let colours = (UNLIT_COLOUR, NUMBER_COLOUR);
    crate::console::lit_number(fb, font, at, &con.unlit, colours, text);
}

/// The speed (`0x9c`): a sign and two digits of the speed table's entry.
pub fn draw_speed(fb: &mut Framebuffer, font: &Font, con: &HumanCon, speeds: &[u8], speed: i16) {
    lit_number(
        fb,
        font,
        con,
        SPEED_AT,
        &crate::console::speed_text(speeds, speed),
    );
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
    let needles = (&con.needles[..], con.quadrants);
    crate::console::draw_needle(fb, sprites, needles, NEEDLE, heading);
    let degrees = crate::editor::degrees(heading);
    lit_number(fb, font, con, HEADING_AT, &digits(degrees, 3));
}

/// The lock warning light (`0xa0`): lit while another robot's lock holds and the blink
/// counter's bit 2 is set.
pub fn draw_warning(fb: &mut Framebuffer, area: Rect, sprites: Sprites, robot: &Robot) {
    crate::console::draw_warning(fb, area, sprites, WARNING, robot);
}

/// The gear lever (`0xa4`): one sprite per gear step.
pub fn draw_gear(fb: &mut Framebuffer, area: Rect, sprites: Sprites, gear: u8) {
    let handle = GEAR.wrapping_add_signed(i16::from(gear as i8) * 4);
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// The compass's widget type.
pub const COMPASS_TYPE: u16 = 0xa6;

/// `(x, y)` within `area`, its bottom and right included (`1000:a8e9`'s tests).
fn within(area: &Rect, (x, y): (u16, u16)) -> bool {
    (area.left..=area.right).contains(&x) && (area.top..=area.bottom).contains(&y)
}

/// A gear lever's press at `at` (humanoid `1000:a88c`, scout `c156`) with the robot
/// in `gear`: on the lever's own rectangle nothing (a drag starts); else the gear of
/// the notch under the pointer, if any (twice its index).
pub fn lever_press(gears: &[Rect], notches: &[Rect], gear: u8, at: (u16, u16)) -> Option<u8> {
    if gears
        .get(usize::from(gear / 2))
        .is_some_and(|r| within(r, at))
    {
        return None;
    }
    let i = notches.iter().position(|r| within(r, at))?;
    Some(2 * i as u8)
}

/// The lever dragged to row `y`: the first gear whose rectangle's top is at or above
/// it, the last below them all (its own top is never compared).
pub fn lever_drag(gears: &[Rect], y: u16) -> u8 {
    let last = gears.len().saturating_sub(1);
    let i = gears[..last].iter().position(|r| y >= r.top);
    2 * i.unwrap_or(last) as u8
}

/// The power panel (`0xb0`): the battery lights and the signal's and energy's bars.
pub fn draw_power(fb: &mut Framebuffer, con: &HumanCon, sprites: Sprites, robot: &Robot) {
    let handles = if robot.on_battery {
        [LIGHT_ON, LIGHT_OFF]
    } else {
        [LIGHT_OFF, LIGHT_ON]
    };
    let areas = (con.lights, con.bars);
    let bar = crate::console::BAR_RIGHT;
    crate::console::draw_power(fb, sprites, areas, (handles, [LIT, DARK]), bar, robot);
}

/// `1000:a466`: the target panel's colours, 0x57 (0xff for a warning, 0x5c for a
/// weapon's state) on 0x68, and where its two parts start.
const TARGET_COLOUR: u8 = 0x57;
const TARGET_WARNING: u8 = 0xff;
const TARGET_STATE: u8 = 0x5c;
const TARGET_BACKGROUND: u8 = 0x68;
const TARGET_AT: (i32, i32) = (0x116, 0x71);
const WEAPONS_AT: (i32, i32) = (0x116, 0x88);

/// The target panel (`0xa2`): the target's name, distance and bearing, or the no-target
/// lines; then each weapon's state.
pub fn draw_target(fb: &mut Framebuffer, font: &Font, con: &HumanCon, robot: &Robot) {
    draw_lines(fb, font, TARGET_BACKGROUND, target_lines(con, robot));
}

/// The target panel's texts in order, all opaque.
fn target_lines(con: &HumanCon, robot: &Robot) -> Vec<Coloured> {
    let colours = (TARGET_COLOUR, TARGET_WARNING);
    let texts = (&con.no_target[..], crate::console::METRES);
    let mut lines = target_part(robot, texts, TARGET_AT, colours);
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

/// `1000:a93e`: the status panel's heading and lines.
const STATUS_HEADING_AT: (i32, i32) = (0x113, 7);
const STATUS_LEFT_AT: (i32, i32) = (0x12a, 0x13);
const STATUS_RIGHT_AT: (i32, i32) = (0x12e, 0x13);
const STATUS_COUNTS_AT: (i32, i32) = (0x12a, 0x25);
/// The status panel (`0xa8`): filled in the kind's message-panel colours, the page's
/// heading and its lines.
pub fn draw_status(
    fb: &mut Framebuffer,
    area: Rect,
    font: &Font,
    con: &HumanCon,
    colours: [u8; 2],
    robot: &Robot,
) {
    crate::console::draw_status(fb, area, font, colours, status_lines(con, robot));
}

/// The status panel's page: its heading, then page 0 the percentages of life support
/// (always 100), power and helm and the targeting computer's and communications'
/// (always working) states; page 1 the weapons' states and the cloaks and ECM charges
/// left; page 2 the sensors' states.
fn status_lines(con: &HumanCon, robot: &Robot) -> Vec<Line> {
    use TextMode::{Opaque, Transparent};
    // `1000:a96f`: the draw cuts +0x10a by 3 when 3 or more (the sim's
    // `Pilot::StatusShown`), and its low two bits pick the page (3 is page 2).
    let cut = if robot.page >= 3 {
        robot.page - 3
    } else {
        robot.page
    };
    let page = usize::from(cut & 3).min(con.pages.len() - 1);
    let state = |broken| crate::console::state((&con.ok, &con.out), broken);
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
            count(&con.out, robot.cloaks),
            Transparent,
        ));
        lines.push((None, count(&con.out, robot.ecm), Transparent));
    }
    lines
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
    use crate::console::IN_ARC;

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
            // Three gears up a lever: rows 30 on, 20 to 29, above 20.
            gears: vec![r(30, 0, 40, 9), r(20, 0, 29, 9), r(10, 0, 19, 9)],
            notches: vec![r(30, 10, 32, 12), r(20, 10, 22, 12), r(10, 10, 12, 12)],
            gear_messages: vec![7, 8, 9],
            centre: (50, 60),
        }
    }

    #[test]
    fn the_lever_and_compass_under_the_pointer() {
        let con = con();
        // On the lever's own gear a press only starts the drag; a notch picks its gear,
        // its edges included.
        assert_eq!(lever_press(&con.gears, &con.notches, 2, (5, 25)), None);
        assert_eq!(lever_press(&con.gears, &con.notches, 2, (12, 32)), Some(0));
        assert_eq!(lever_press(&con.gears, &con.notches, 0, (10, 10)), Some(4));
        assert_eq!(lever_press(&con.gears, &con.notches, 0, (13, 10)), None);
        // The drag goes by the row alone.
        assert_eq!(lever_drag(&con.gears, 35), 0);
        assert_eq!(lever_drag(&con.gears, 20), 2);
        assert_eq!(lever_drag(&con.gears, 0), 4);
        assert_eq!(crate::console::compass_offset(con.centre, (53, 56)), (3, 4));
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
        // The draw cuts 3 or more by 3: page 3 is page 0, 4 page 1.
        robot.page = 3;
        assert_eq!(texts(&robot)[0], b"P0");
        robot.page = 4;
        assert_eq!(texts(&robot)[0], b"P1");
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
}
