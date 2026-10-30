//! The scout console's instruments (T-132; `docs/screens.md`, "The scout console's
//! instruments (T-132)"), drawn over the picture as their draw handlers draw them, from
//! the viewed robot. The texts, tables and rectangles are read from the user's
//! `xeno.exe` (`xeno_formats::scoutcon`); sprite handles, places and colours are the
//! handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::scoutcon::ScoutCon;
use xeno_render::{BlitMode, Framebuffer, TextMode};

use crate::console::{Coloured, Line, Look, NEXT, Robot, Sprites, count, sprite, state};
use crate::panels::digits;

/// The widgets drawn at their rectangles, by type; the compass (0x10a), the speed
/// (0x12c) and the power (0x13c) draw at fixed places.
pub const WARNING_TYPE: u16 = 0x12a;
pub const GEAR_TYPE: u16 = 0x124;
pub const STANCE_TYPE: u16 = 0x13a;
pub const STATUS_TYPE: u16 = 0x11c;
/// The buttons: fire, a weapon (twice), pilot, program, lock, a drop (four times),
/// cloak, ECM, the three stances, turn left, straight and right.
pub const FIRE_TYPE: u16 = 0x11a;
pub const WEAPON_TYPE: u16 = 0x118;
pub const PILOT_TYPE: u16 = 0x120;
pub const PROGRAM_TYPE: u16 = 0x122;
pub const LOCK_TYPE: u16 = 0x128;
pub const DROP_TYPE: u16 = 0x12e;
pub const CLOAK_TYPE: u16 = 0x130;
pub const ECM_TYPE: u16 = 0x132;
pub const STANCE_TYPES: [u16; 3] = [0x134, 0x136, 0x138];
pub const LEFT_TYPE: u16 = 0x13e;
pub const RIGHT_TYPE: u16 = 0x142;
/// `1000:c208`, `b8b8`: the unlit digits in 0x68, transparent; the speed in 0x60 at
/// (0x3b, 0xbe), the heading in 0x0e at (0x19, 0x8b).
const UNLIT_COLOUR: u8 = 0x68;
const SPEED_COLOUR: u8 = 0x60;
const HEADING_COLOUR: u8 = 0x0e;
const SPEED_AT: (i32, i32) = (0x3b, 0xbe);
const HEADING_AT: (i32, i32) = (0x19, 0x8b);
/// `1000:b8b8`: the dial and the first needle (eight handles a needle).
const DIAL: u16 = 0xa80;
const NEEDLE: u16 = 0xba8;
/// `1000:bee8`: the warning light.
const WARNING: u16 = 0xa88;
/// `1000:c360`: the power lights (on the battery, off), the bars' lit and dark
/// segments.
const LIGHTS_ON_BATTERY: [u16; 2] = [0xad8, 0xae0];
const LIGHTS_ON_NET: [u16; 2] = [0xae0, 0xad8];
const SEGMENTS: [u16; 2] = [0xad0, 0xac8];
/// `1000:c13e`: the gear lever's first sprite, 4 handles a gear step.
const GEAR: u16 = 0xb70;
/// `1000:c343`: the stance gauge's first look, 8 handles every 0x10 of stance.
const STANCE: u16 = 0xb18;

/// The speed (`0x12c`): two digits of the speed table's entry (`02b7`'s last two; the
/// sign the handler writes before them is not drawn).
pub fn draw_speed(fb: &mut Framebuffer, font: &Font, con: &ScoutCon, speeds: &[u8], speed: i16) {
    let text = &crate::console::speed_text(speeds, speed)[1..];
    let colours = (UNLIT_COLOUR, SPEED_COLOUR);
    crate::console::lit_number(fb, font, SPEED_AT, &con.speed_unlit, colours, text);
}

/// The compass (`0x10a`): the dial, the heading's needle (one of 64) in its quadrant,
/// the heading in degrees.
pub fn draw_compass(
    fb: &mut Framebuffer,
    font: &Font,
    con: &ScoutCon,
    sprites: Sprites,
    heading: u16,
) {
    let dial = con.dial;
    sprite(fb, sprites, DIAL, dial.left, dial.top, BlitMode::Keyed);
    let needles = (&con.needles[..], con.quadrants);
    crate::console::draw_needle(fb, sprites, needles, NEEDLE, heading);
    let degrees = digits(crate::editor::degrees(heading), 3);
    let colours = (UNLIT_COLOUR, HEADING_COLOUR);
    crate::console::lit_number(fb, font, HEADING_AT, &con.heading_unlit, colours, &degrees);
}

/// The lock warning light (`0x12a`).
pub fn draw_warning(fb: &mut Framebuffer, area: Rect, sprites: Sprites, robot: &Robot) {
    crate::console::draw_warning(fb, area, sprites, WARNING, robot);
}

/// The power panel (`0x13c`): the lights and the signal's and energy's bars, upwards.
pub fn draw_power(fb: &mut Framebuffer, con: &ScoutCon, sprites: Sprites, robot: &Robot) {
    let handles = if robot.on_battery {
        LIGHTS_ON_BATTERY
    } else {
        LIGHTS_ON_NET
    };
    let areas = (con.lights, con.bars);
    let bar = crate::console::BAR_UP;
    crate::console::draw_power(fb, sprites, areas, (handles, SEGMENTS), bar, robot);
}

/// The gear lever (`0x124`): one sprite per gear step, opaque at its corner.
pub fn draw_gear(fb: &mut Framebuffer, area: Rect, sprites: Sprites, gear: u8) {
    let handle = GEAR.wrapping_add_signed(i16::from(gear as i8) * 4);
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// The stance gauge (`0x13a`): a look every 0x10 of stance, opaque at its corner.
pub fn draw_stance(fb: &mut Framebuffer, area: Rect, sprites: Sprites, stance: u8) {
    sprite(
        fb,
        sprites,
        stance_look(stance),
        area.left,
        area.top,
        BlitMode::Opaque,
    );
}

/// `1000:c343`: `(stance >> 1) & 0x38` handles past the first.
fn stance_look(stance: u8) -> u16 {
    STANCE + u16::from((stance >> 1) & 0x38)
}

/// `1000:c02e`: the target panel's colours, 0x60 (0xff for a warning) on 0x01, and
/// where its two parts start.
const TARGET_COLOURS: (u8, u8) = (0x60, 0xff);
const TARGET_BACKGROUND: u8 = 0x01;
const TARGET_AT: (i32, i32) = (0x122, 0x71);
const WEAPONS_AT: (i32, i32) = (0x122, 0x86);

/// The target panel (`0x11e`): the target's name, distance and bearing, or the
/// no-target lines; then each weapon's state.
pub fn draw_target(fb: &mut Framebuffer, font: &Font, con: &ScoutCon, robot: &Robot) {
    let lines = target_lines(con, robot);
    crate::console::draw_lines(fb, font, TARGET_BACKGROUND, lines);
}

/// The target panel's texts in order, all opaque in the panel's colour but for the
/// warnings.
fn target_lines(con: &ScoutCon, robot: &Robot) -> Vec<Coloured> {
    let texts = (&con.no_target[..], crate::console::METRES_SPACED);
    let mut lines = crate::console::target_part(robot, texts, TARGET_AT, TARGET_COLOURS);
    for (w, bit) in [0x01, 0x02].into_iter().enumerate() {
        let text = if robot.weapons & bit != 0 {
            &con.damaged
        } else if robot.cooldowns[w] != 0 {
            &con.loading
        } else {
            &con.ready
        };
        let at = (w == 0).then_some(WEAPONS_AT);
        lines.push((at, TARGET_COLOURS.0, text.clone()));
    }
    lines
}

/// `1000:bf08`: the status panel's heading and its lines (page 3's from 4 pixels
/// further left).
const STATUS_HEADING_AT: (i32, i32) = (0x10e, 5);
const STATUS_AT: (i32, i32) = (0x12a, 0x11);
const STATUS_COUNTS_AT: (i32, i32) = (0x12a, 0x23);
const STATUS_DROPS_AT: (i32, i32) = (0x126, 0x11);

/// The status panel (`0x11c`): filled in the kind's message-panel colours, the page's
/// heading and its lines; `ok` and `out` are the shared `1000:de12` texts.
pub fn draw_status(
    fb: &mut Framebuffer,
    (area, font): (Rect, &Font),
    con: &ScoutCon,
    (ok, out): (&[u8], &[u8]),
    colours: [u8; 2],
    robot: &Robot,
) {
    let lines = status_lines(con, (ok, out), robot);
    crate::console::draw_status(fb, area, font, colours, lines);
}

/// The status panel's page (+0x108 AND 3): its heading, then page 0 the percentages of
/// life support (100), helm and power, the targeting computer's state and
/// communications' (100); page 1 the sensors' states; page 2 the weapons' states and
/// the cloaks and ECM charges left; page 3 the drops left.
fn status_lines(con: &ScoutCon, (ok, out): (&[u8], &[u8]), robot: &Robot) -> Vec<Line> {
    use TextMode::{Opaque, Transparent};
    let page = usize::from(robot.page & 3);
    let broken = |bit| state((ok, out), bit != 0);
    let percent = crate::panels::percent;
    let mut lines = vec![(
        Some(STATUS_HEADING_AT),
        con.pages[page].clone(),
        Transparent,
    )];
    let (first, rest, mode) = match page {
        0 => (
            STATUS_AT,
            vec![
                percent(100),
                percent(robot.helm),
                percent(robot.power),
                broken(robot.targeting & 1),
                percent(100),
            ],
            Opaque,
        ),
        1 => (
            STATUS_AT,
            [0x01, 0x02, 0x04, 0x08]
                .map(|bit| broken(robot.systems & bit))
                .to_vec(),
            Opaque,
        ),
        2 => (
            STATUS_AT,
            vec![broken(robot.weapons & 1), broken(robot.weapons & 2)],
            Opaque,
        ),
        // `1000:bffe`: the mines, ECM buoys, alarm buoys and decoys, in that order.
        _ => (
            STATUS_DROPS_AT,
            [0, 2, 1, 3].map(|i| count(out, robot.drops[i])).to_vec(),
            Transparent,
        ),
    };
    for (i, text) in rest.into_iter().enumerate() {
        lines.push(((i == 0).then_some(first), text, mode));
    }
    if page == 2 {
        lines.push((
            Some(STATUS_COUNTS_AT),
            count(out, robot.cloaks),
            Transparent,
        ));
        lines.push((None, count(out, robot.ecm), Transparent));
    }
    lines
}

/// `1000:bd7f` to `ba18`: the shared-look buttons' sprites and where each goes from its
/// rectangle's corner.
const SHARED: [(u16, u16, i16, i16); 9] = [
    (FIRE_TYPE, 0xa40, 0, 0),
    (DROP_TYPE, 0xa98, 5, 3),
    (CLOAK_TYPE, 0xaa8, -3, 0),
    (ECM_TYPE, 0xab8, -4, 0),
    (STANCE_TYPES[0], 0xae8, 0, 0),
    (STANCE_TYPES[1], 0xaf8, 0, 0),
    (STANCE_TYPES[2], 0xb08, 0, 0),
    (LEFT_TYPE, 0xb50, 0, 0),
    (RIGHT_TYPE, 0xb60, 0, 0),
];
/// The lock button (one pixel right and down), the switches (lit for whoever drives).
const LOCK: u16 = 0xa70;
const PILOT: u16 = 0xa60;
const PROGRAM: u16 = 0xa50;

/// A button (`kind`, its rectangle and its state's second byte: a weapon's number) as
/// its draw handler draws it; `held` while the pointer holds it.
pub fn draw_button(
    fb: &mut Framebuffer,
    sprites: Sprites,
    con: &ScoutCon,
    (kind, area, n): (u16, Rect, u8),
    robot: &Robot,
    (look, held): (Look, bool),
) {
    if let Some((handle, dx, dy)) = button(con, kind, n, robot, (look, held)) {
        let (x, y) = (
            area.left.wrapping_add_signed(dx),
            area.top.wrapping_add_signed(dy),
        );
        sprite(fb, sprites, handle, x, y, BlitMode::Opaque);
    }
}

/// A button's sprite and its offset from the rectangle's corner; none for other kinds
/// (the straight button, `1000:b9f3`, draws nothing).
fn button(
    con: &ScoutCon,
    kind: u16,
    n: u8,
    robot: &Robot,
    (look, held): (Look, bool),
) -> Option<(u16, i16, i16)> {
    let lit = |on: bool| if on { NEXT } else { 0 };
    // shortcut: the original adds all of +0x22, so while the lock button, the program
    // panel or the gear lever is held the shared-look buttons show another sprite; they
    // are not drawn again until marked then.
    if let Some(&(_, base, dx, dy)) = SHARED.iter().find(|b| b.0 == kind) {
        return Some((base + lit(look.pressed), dx, dy));
    }
    match kind {
        // `1000:b7db`: the weapon held (and not already selected) adds its own look.
        WEAPON_TYPE => {
            let selected = n == robot.weapon;
            let base = con.weapon_sprites[usize::from(selected)][usize::from(n & 1)];
            Some((base + lit(held && !selected), -1, -1))
        }
        LOCK_TYPE => {
            let on = robot.target.is_some() != (look.lock_held && look.pressed);
            Some((LOCK + lit(on), 1, 1))
        }
        PILOT_TYPE => Some((PILOT + lit(!robot.program_drives), 0, 0)),
        PROGRAM_TYPE => Some((PROGRAM + lit(robot.program_drives), 0, 0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con() -> ScoutCon {
        let r = Rect {
            top: 0,
            left: 0,
            bottom: 1,
            right: 1,
        };
        ScoutCon {
            bank: 0,
            speed_unlit: Vec::new(),
            heading_unlit: Vec::new(),
            no_target: b"NONE".to_vec(),
            loading: b"LOAD".to_vec(),
            damaged: b"BROKEN".to_vec(),
            ready: b"GO".to_vec(),
            pages: [0, 1, 2, 3].map(|i| vec![b'p', b'0' + i]),
            dial: r,
            needles: [0; 64],
            quadrants: [r; 4],
            lights: [r; 2],
            bars: [r; 2],
            weapon_sprites: [[1, 2], [3, 4]],
        }
    }

    #[test]
    fn the_target_panel_ends_with_the_weapons() {
        let con = con();
        let mut robot = Robot {
            cooldowns: [0, 5],
            ..Robot::default()
        };
        let texts = |robot: &Robot| -> Vec<Vec<u8>> {
            target_lines(&con, robot).into_iter().map(|l| l.2).collect()
        };
        assert_eq!(texts(&robot), [&b"NONE"[..], b"GO", b"LOAD"]);
        robot.weapons = 0x01;
        assert_eq!(texts(&robot)[1], b"BROKEN");
        // With a target out of range: its distance in the warning colour, `M` and a
        // space after it.
        (robot.target, robot.targeting) = (Some(b"SC-001".to_vec()), 0x04);
        let lines = target_lines(&con, &robot);
        assert_eq!((lines.len(), lines[1].1, lines[2].1), (5, 0xff, 0x60));
        assert!(lines[1].2.ends_with(b"0M \x0c"));
        assert_eq!(lines[3].0, Some(WEAPONS_AT));
    }

    #[test]
    fn the_status_pages() {
        let con = con();
        let (ok, out) = (&b"OK"[..], &b"OUT"[..]);
        let mut robot = Robot {
            helm: 50,
            power: 75,
            systems: 0x02,
            weapons: 0x02,
            drops: [1, 2, 0, 12],
            cloaks: 3,
            ..Robot::default()
        };
        let page = |robot: &Robot| -> Vec<Vec<u8>> {
            status_lines(&con, (ok, out), robot)
                .into_iter()
                .map(|l| l.1)
                .collect()
        };
        let p = page(&robot);
        assert_eq!(p[0], b"p0");
        assert_eq!(p.len(), 6);
        robot.page = 1;
        assert_eq!(page(&robot)[1..], [&b"OK"[..], b"OUT", b"OK", b"OK"]);
        robot.page = 2;
        assert_eq!(page(&robot)[1..], [&b"OK"[..], b"OUT", b" 3\x0c", b"OUT"]);
        // Page 3: mines, ECM buoys, alarm buoys, decoys.
        robot.page = 3;
        assert_eq!(
            page(&robot)[1..],
            [&b" 1\x0c"[..], b"OUT", b" 2\x0c", b"12\x0c"]
        );
        assert_eq!(
            status_lines(&con, (ok, out), &robot)[1].0,
            Some(STATUS_DROPS_AT)
        );
    }

    #[test]
    fn the_buttons_follow_the_robot_and_the_look() {
        let con = con();
        let mut robot = Robot::default();
        let mut look = Look::default();
        let at = |kind, n, robot: &Robot, look, held| button(&con, kind, n, robot, (look, held));
        assert_eq!(at(DROP_TYPE, 3, &robot, look, false), Some((0xa98, 5, 3)));
        assert_eq!(at(ECM_TYPE, 0, &robot, look, false), Some((0xab8, -4, 0)));
        assert_eq!(at(0x140, 0, &robot, look, false), None);
        // Weapon 0 is selected: lit, and holding it changes nothing; weapon 1 held
        // shows its next sprite.
        assert_eq!(at(WEAPON_TYPE, 0, &robot, look, true), Some((3, -1, -1)));
        assert_eq!(
            at(WEAPON_TYPE, 1, &robot, look, true),
            Some((2 + NEXT, -1, -1))
        );
        // The switches: lit for whoever drives.
        assert_eq!(
            at(PILOT_TYPE, 0, &robot, look, false),
            Some((PILOT + NEXT, 0, 0))
        );
        assert_eq!(
            at(PROGRAM_TYPE, 0, &robot, look, false),
            Some((PROGRAM, 0, 0))
        );
        robot.program_drives = true;
        assert_eq!(at(PILOT_TYPE, 0, &robot, look, false), Some((PILOT, 0, 0)));
        // Pressed: the shared look; the lock button held shows the other light.
        look.pressed = true;
        assert_eq!(
            at(LEFT_TYPE, 0, &robot, look, false),
            Some((0xb50 + NEXT, 0, 0))
        );
        look.lock_held = true;
        assert_eq!(
            at(LOCK_TYPE, 0, &robot, look, false),
            Some((LOCK + NEXT, 1, 1))
        );
        robot.target = Some(Vec::new());
        assert_eq!(at(LOCK_TYPE, 0, &robot, look, false), Some((LOCK, 1, 1)));
    }

    #[test]
    fn the_stance_gauge_steps_every_0x10() {
        // Crouched (0), standing (0x30), raised (0x60).
        assert_eq!(
            [0, 0x30, 0x60].map(stance_look),
            [STANCE, STANCE + 0x18, STANCE + 0x30]
        );
        assert_eq!(stance_look(0x2f), STANCE + 0x10);
    }
}
