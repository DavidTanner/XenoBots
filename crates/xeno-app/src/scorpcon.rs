//! The scorpion console's instruments (T-131; `docs/screens.md`, "The scorpion
//! console's instruments (T-131)"), drawn over the picture as their draw handlers draw
//! them, from the viewed robot. The texts, tables and rectangles are read from the
//! user's `xeno.exe` (`xeno_formats::scorpcon`); sprite handles, places and colours
//! are the handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::scorpcon::ScorpCon;
use xeno_render::{BlitMode, Framebuffer};

use crate::console::{Coloured, Look, NEXT, Robot, Sprites, sprite};
use crate::panels::digits;

/// The widgets drawn at their rectangles, by type; the speed (0xce), the compass
/// (0xd6) and the power (0xec) draw at fixed places.
pub const WARNING_TYPE: u16 = 0xdc;
pub const LEVER_TYPE: u16 = 0xde;
pub const DAMAGE_TYPE: u16 = 0xee;
pub const GAUGE_TYPES: [u16; 2] = [0xe0, 0xe2];
pub const RELAYS_TYPE: u16 = 0xe4;
/// The buttons: fire, a weapon (twice), a drop (four times), lock, pilot, program,
/// faster, stop, slower, turn left, straight and right, and the relay keys C, D, L
/// and U.
pub const FIRE_TYPE: u16 = 0xcc;
pub const WEAPON_TYPE: u16 = 0xd2;
pub const DROP_TYPE: u16 = 0xd4;
pub const LOCK_TYPE: u16 = 0xda;
pub const PILOT_TYPE: u16 = 0xf0;
pub const PROGRAM_TYPE: u16 = 0xf2;
pub const FASTER_TYPE: u16 = 0xe6;
pub const STOP_TYPE: u16 = 0xe8;
pub const SLOWER_TYPE: u16 = 0xea;
pub const LEFT_TYPE: u16 = 0xf4;
pub const AHEAD_TYPE: u16 = 0xf6;
pub const RIGHT_TYPE: u16 = 0xf8;
pub const CARGO_TYPE: u16 = 0xfa;
pub const DUMP_TYPE: u16 = 0xfc;
pub const LOAD_TYPE: u16 = 0xfe;
pub const DEPLOY_TYPE: u16 = 0x100;
/// `1000:86f4`, `87d6`: the unlit digits in 0x74, the number in 0x0e, transparent; the
/// speed at (0x55, 0x7c), the heading at (0x4c, 0xa0).
const NUMBER_COLOURS: (u8, u8) = (0x74, 0x0e);
const SPEED_AT: (i32, i32) = (0x55, 0x7c);
const HEADING_AT: (i32, i32) = (0x4c, 0xa0);
/// `1000:87d6`: the dial and the first needle (eight handles a needle).
const DIAL: u16 = 0x910;
const NEEDLE: u16 = 0x918;
/// `1000:8a71`: the warning light.
const WARNING: u16 = 0x9a0;
/// `1000:8fb9`: the power lights (on the battery, off), the bars' lit and dark
/// segments.
const LIGHTS_ON_BATTERY: [u16; 2] = [0x878, 0x890];
const LIGHTS_ON_NET: [u16; 2] = [0x880, 0x888];
const SEGMENTS: [u16; 2] = [0x870, 0x868];
/// `1000:9065`: the damage panel's cover, and its lights.
const COVER: u16 = 0x908;
const DAMAGE_LIGHTS: [u16; 6] = [0x8d8, 0x8e0, 0x8e8, 0x8f0, 0x8f8, 0x900];
/// The helm's light comes on at 75 or under (`cmp byte [di + 0x24], 0x4b; ja`).
const HELM_LOW: u8 = 0x4b;
/// `1000:8c06`, `8c1e`: the cargo gauges' first look, 8 handles a step.
const GAUGE: u16 = 0x7e8;

/// The speed (`0xce`): two digits of the speed table's entry (`02b7`'s last two; the
/// sign the handler writes before them is not drawn).
pub fn draw_speed(fb: &mut Framebuffer, font: &Font, con: &ScorpCon, speeds: &[u8], speed: i16) {
    let text = &crate::console::speed_text(speeds, speed)[1..];
    crate::console::lit_number(fb, font, SPEED_AT, &con.speed_unlit, NUMBER_COLOURS, text);
}

/// The compass (`0xd6`): the dial, the heading's needle turned in the dial, the
/// heading in degrees.
pub fn draw_compass(
    fb: &mut Framebuffer,
    font: &Font,
    con: &ScorpCon,
    sprites: Sprites,
    heading: u16,
) {
    let dial = con.dial;
    sprite(fb, sprites, DIAL, dial.left, dial.top, BlitMode::Keyed);
    let needles = (&con.needles, [dial; 4]);
    crate::console::draw_needle(fb, sprites, needles, NEEDLE, heading);
    let degrees = digits(crate::editor::degrees(heading), 3);
    let unlit = &con.heading_unlit;
    crate::console::lit_number(fb, font, HEADING_AT, unlit, NUMBER_COLOURS, &degrees);
}

/// The lock warning light (`0xdc`).
pub fn draw_warning(fb: &mut Framebuffer, area: Rect, sprites: Sprites, robot: &Robot) {
    crate::console::draw_warning(fb, area, sprites, WARNING, robot);
}

/// The power panel (`0xec`): the lights and the signal's and energy's bars.
pub fn draw_power(fb: &mut Framebuffer, con: &ScorpCon, sprites: Sprites, robot: &Robot) {
    let handles = if robot.on_battery {
        LIGHTS_ON_BATTERY
    } else {
        LIGHTS_ON_NET
    };
    let areas = (con.lights, con.bars);
    crate::console::draw_power(fb, sprites, areas, handles, SEGMENTS, robot);
}

/// The speed lever (`0xde`): opaque at its corner.
pub fn draw_lever(fb: &mut Framebuffer, area: Rect, con: &ScorpCon, sprites: Sprites, speed: i16) {
    let handle = lever(con, speed);
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// `1000:8760`: forward, `(speed >> 2) & 0x1e` bytes into the first table; in reverse,
/// `(-speed >> 2) & 0x0e` into the second (both from the speed's low byte).
fn lever(con: &ScorpCon, speed: i16) -> u16 {
    let (looks, size, mask) = if speed < 0 {
        (&con.lever[1], speed.unsigned_abs(), 0x0e)
    } else {
        (&con.lever[0], speed as u16, 0x1e)
    };
    let i = usize::from((size as u8 >> 2) & mask) / 2;
    // shortcut: past 63 the original reads on past the table; the port keeps the
    // last look. No scorpion asks for more than 63.
    looks.get(i).or(looks.last()).copied().unwrap_or(0)
}

/// The damage panel (`0xee`): the cover while +0xfa bit 7 is clear; while it is set,
/// a keyed light for each part damaged over what the panel showed (the cover, drawn
/// every other 16 ticks).
pub fn draw_damage(
    fb: &mut Framebuffer,
    area: Rect,
    con: &ScorpCon,
    sprites: Sprites,
    robot: &Robot,
) {
    // shortcut: the original leaves the picture under the lights when the console
    // opens with bit 7 set; the port draws the cover under them every time.
    sprite(fb, sprites, COVER, area.left, area.top, BlitMode::Opaque);
    if !robot.damage_shown {
        return;
    }
    for ((handle, at), on) in DAMAGE_LIGHTS.iter().zip(con.damage).zip(damaged(robot)) {
        if on {
            sprite(fb, sprites, *handle, at.left, at.top, BlitMode::Keyed);
        }
    }
}

/// Which of the damage panel's lights are on: the helm at 75 or under, the second
/// barrel, the first, the missile (+0xdc bits 1, 0, 2), the deployer, a sensor
/// (+0xf6's low four bits).
fn damaged(robot: &Robot) -> [bool; 6] {
    [
        robot.helm <= HELM_LOW,
        robot.weapons & 0x02 != 0,
        robot.weapons & 0x01 != 0,
        robot.weapons & 0x04 != 0,
        robot.deployer_damaged,
        robot.systems & 0x0f != 0,
    ]
}

/// A cargo gauge (`0xe0`, `0xe2`): its look by what it last drew, opaque at its corner.
pub fn draw_gauge(fb: &mut Framebuffer, area: Rect, sprites: Sprites, gauge: u8) {
    let handle = GAUGE + u16::from((gauge >> 1) & 0x78);
    sprite(fb, sprites, handle, area.left, area.top, BlitMode::Opaque);
}

/// What the gauges last drew (+0x11a, +0x11b), as the tick table's entry 5
/// (`1000:9a50`) sets them: the relays times 16 plus +0x119, and the relays times 8
/// plus twice +0x119.
pub fn gauges(relays: u8, extra: u8) -> [u8; 2] {
    [
        relays.wrapping_mul(16).wrapping_add(extra),
        relays.wrapping_mul(8).wrapping_add(extra.wrapping_mul(2)),
    ]
}

/// `1000:88b3`: the target panel's colours, 0x7a (0xff for a warning) on 0, where its
/// two parts start, and the barrels' bits (+0xdc bits 0 and 1).
const TARGET_COLOURS: (u8, u8) = (0x7a, 0xff);
const TARGET_AT: (i32, i32) = (0x124, 0x9b);
const ROUNDS_AT: (i32, i32) = (0x124, 0xad);
const BARRELS: u8 = 0x03;

/// The target panel (`0xd0`): the target's name, distance and bearing, or the no-target
/// lines; then the cannon's rounds.
pub fn draw_target(fb: &mut Framebuffer, font: &Font, con: &ScorpCon, robot: &Robot) {
    crate::console::draw_lines(fb, font, 0, target_lines(con, robot));
}

/// The target panel's texts in order, all opaque.
fn target_lines(con: &ScorpCon, robot: &Robot) -> Vec<Coloured> {
    let mut lines = crate::console::target_part(robot, &con.no_target, TARGET_AT, TARGET_COLOURS);
    // `02b7` from its second digit: four digits, leading zeros kept.
    let rounds = if robot.weapons & BARRELS == BARRELS {
        con.no_rounds.clone()
    } else {
        digits(robot.rounds, 5).split_off(1)
    };
    lines.push((Some(ROUNDS_AT), TARGET_COLOURS.0, rounds));
    lines
}

/// `1000:8c36`: the relay panel's colour, 0x77 on 0, from (2, 1) past its corner.
const RELAYS_COLOUR: u8 = 0x77;

/// The relay panel (`0xe4`): the relays carried, two digits over the start of its text,
/// or its text for none.
pub fn draw_relays(fb: &mut Framebuffer, area: Rect, font: &Font, con: &ScorpCon, relays: u8) {
    let at = (i32::from(area.left) + 2, i32::from(area.top) + 1);
    let text = relays_text(con, relays);
    crate::console::draw_lines(fb, font, 0, vec![(Some(at), RELAYS_COLOUR, text)]);
}

fn relays_text(con: &ScorpCon, relays: u8) -> Vec<u8> {
    if relays == 0 {
        return con.no_relays.clone();
    }
    let mut text = con.relays.clone();
    let count = digits(relays.into(), 5);
    for (t, d) in text.iter_mut().zip(&count[3..]) {
        *t = *d;
    }
    text
}

/// `1000:89a5` to `8d90`: the shared-look buttons' sprites and where each goes from its
/// rectangle's corner.
const SHARED: [(u16, u16, i16, i16); 12] = [
    (FIRE_TYPE, 0x748, 0, 0),
    (FASTER_TYPE, 0x7b8, 1, 1),
    (STOP_TYPE, 0x7c8, 1, 1),
    (SLOWER_TYPE, 0x7d8, 1, 1),
    (LEFT_TYPE, 0x898, 0, 0),
    (AHEAD_TYPE, 0x8a8, 0, 0),
    (RIGHT_TYPE, 0x8b8, 0, 0),
    (CARGO_TYPE, 0x960, 1, 1),
    (DUMP_TYPE, 0x970, 1, 1),
    (LOAD_TYPE, 0x980, 1, 1),
    (DEPLOY_TYPE, 0x990, 1, 1),
    (DROP_TYPE, 0, 0, 0),
];
/// The lock button (one pixel up), and the switches' look off and lit.
const LOCK: u16 = 0x758;
const SWITCH: u16 = 0x6d8;

/// A button (`kind`, its rectangle and its state's second byte: a weapon's or a drop's
/// number) as its draw handler draws it; `held` while the pointer holds it.
pub fn draw_button(
    fb: &mut Framebuffer,
    sprites: Sprites,
    con: &ScorpCon,
    (kind, area, n): (u16, Rect, u8),
    robot: &Robot,
    (look, held): (Look, bool),
) {
    let at = |dx: i16, dy: i16| {
        (
            area.left.wrapping_add_signed(dx),
            area.top.wrapping_add_signed(dy),
        )
    };
    let (handle, (x, y)) = match button(con, kind, n, robot, (look, held)) {
        Some(Place::At(handle, dx, dy)) => (handle, at(dx, dy)),
        Some(Place::In(handle, r)) => (handle, (r.left, r.top)),
        None => return,
    };
    sprite(fb, sprites, handle, x, y, BlitMode::Opaque);
}

/// Where a button's sprite goes: off its widget's corner, or at a rectangle of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    At(u16, i16, i16),
    In(u16, Rect),
}

/// A button's sprite; none for other kinds.
fn button(
    con: &ScorpCon,
    kind: u16,
    n: u8,
    robot: &Robot,
    (look, held): (Look, bool),
) -> Option<Place> {
    let pressed = if look.pressed { NEXT } else { 0 };
    let lit = |on: bool| if on { NEXT } else { 0 };
    // shortcut: the original adds all of +0x22, so while the lock button or the
    // program panel is held the shared-look buttons show another sprite; they are not
    // drawn again until marked then.
    if let Some(&(_, base, dx, dy)) = SHARED.iter().find(|b| b.0 == kind) {
        let base = if kind == DROP_TYPE {
            *con.drops.get(usize::from(n))?
        } else {
            base
        };
        return Some(Place::At(base + pressed, dx, dy));
    }
    match kind {
        WEAPON_TYPE => {
            let sprites = con.weapon_sprites[usize::from(n == robot.weapon)];
            Some(Place::At(sprites[usize::from(n & 1)], 1, 0))
        }
        LOCK_TYPE => {
            let on = robot.target.is_some() != (look.lock_held && look.pressed);
            Some(Place::At(LOCK + lit(on), 0, -1))
        }
        // `1000:8b31`, `8b9c`: lit for whoever drives; held, the look `DS:0x6530`
        // flipped by the press.
        PILOT_TYPE | PROGRAM_TYPE => {
            let i = usize::from(kind == PROGRAM_TYPE);
            let on = held || robot.program_drives == (kind == PROGRAM_TYPE);
            Some(Place::In(SWITCH + lit(on), con.switches[i]))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con() -> ScorpCon {
        let r = Rect {
            top: 0,
            left: 0,
            bottom: 1,
            right: 1,
        };
        ScorpCon {
            bank: 0,
            speed_unlit: Vec::new(),
            heading_unlit: Vec::new(),
            no_target: Vec::new(),
            no_rounds: Vec::new(),
            relays: Vec::new(),
            no_relays: Vec::new(),
            dial: r,
            needles: [0; 32],
            lever: [
                std::array::from_fn(|i| 0x100 + i as u16),
                std::array::from_fn(|i| 0x200 + i as u16),
            ],
            lights: [r; 2],
            bars: [r; 2],
            damage: [r; 6],
            switches: [r; 2],
            weapon_sprites: [[0; 2]; 2],
            drops: [0; 4],
        }
    }

    #[test]
    fn the_lever_steps_every_8_of_speed() {
        let con = con();
        assert_eq!(
            [0, 7, 8, 21, 42, 63].map(|s| lever(&con, s)),
            [0x100, 0x100, 0x101, 0x102, 0x105, 0x107]
        );
        // In reverse only the low three bits of the step count.
        assert_eq!(
            [-1, -8, -21, -42, -64].map(|s| lever(&con, s)),
            [0x200, 0x201, 0x202, 0x205, 0x200]
        );
    }

    #[test]
    fn the_damage_lights_and_gauges() {
        let mut robot = Robot {
            helm: 100,
            ..Robot::default()
        };
        assert_eq!(damaged(&robot), [false; 6]);
        (robot.helm, robot.weapons, robot.systems) = (75, 0x05, 0x10);
        assert_eq!(damaged(&robot), [true, false, true, true, false, false]);
        robot.systems = 0x08;
        assert!(damaged(&robot)[5]);
        // 12 relays: the first gauge's 13th look, the second's 7th.
        let g = gauges(12, 0);
        assert_eq!(g, [192, 96]);
        assert_eq!(g.map(|g| (g >> 1) & 0x78), [0x60, 0x30]);
    }

    #[test]
    fn the_target_panel_ends_with_the_rounds() {
        let mut con = con();
        (con.no_target, con.no_rounds) = (b"NONE".to_vec(), b"OUT".to_vec());
        let mut robot = Robot {
            rounds: 2000,
            ..Robot::default()
        };
        assert_eq!(
            target_lines(&con, &robot),
            [
                (Some(TARGET_AT), 0x7a, b"NONE".to_vec()),
                (Some(ROUNDS_AT), 0x7a, b"2000".to_vec()),
            ]
        );
        robot.rounds = 45;
        robot.weapons = 0x01;
        assert_eq!(target_lines(&con, &robot)[1].2, b"0045");
        // Both barrels damaged: the text instead of the count.
        robot.weapons = 0x03;
        assert_eq!(target_lines(&con, &robot)[1].2, b"OUT");
        // With a target out of the arc, its bearing in the warning colour.
        (robot.target, robot.targeting) = (Some(b"AA-001".to_vec()), 0x02);
        let lines = target_lines(&con, &robot);
        assert_eq!((lines.len(), lines[1].1, lines[2].1), (4, 0x7a, 0xff));
    }

    #[test]
    fn the_relay_count_is_written_over_its_text() {
        let mut con = con();
        (con.relays, con.no_relays) = (b"xx LEFT".to_vec(), b"NONE".to_vec());
        assert_eq!(relays_text(&con, 0), b"NONE");
        assert_eq!(relays_text(&con, 7), b"07 LEFT");
        assert_eq!(relays_text(&con, 12), b"12 LEFT");
    }

    #[test]
    fn the_buttons_follow_the_robot_and_the_look() {
        let mut con = con();
        con.weapon_sprites = [[1, 2], [3, 4]];
        con.drops = [0x10, 0x20, 0x30, 0x40];
        let mut robot = Robot::default();
        let mut look = Look::default();
        let at = |kind, n, robot: &Robot, look, held| button(&con, kind, n, robot, (look, held));
        assert_eq!(
            at(FIRE_TYPE, 0, &robot, look, false),
            Some(Place::At(0x748, 0, 0))
        );
        assert_eq!(
            at(DROP_TYPE, 2, &robot, look, false),
            Some(Place::At(0x30, 0, 0))
        );
        assert_eq!(
            at(LOCK_TYPE, 0, &robot, look, false),
            Some(Place::At(LOCK, 0, -1))
        );
        // The weapon selected shows its lit sprite, a pixel right.
        robot.weapon = 1;
        assert_eq!(
            at(WEAPON_TYPE, 0, &robot, look, false),
            Some(Place::At(1, 1, 0))
        );
        assert_eq!(
            at(WEAPON_TYPE, 1, &robot, look, false),
            Some(Place::At(4, 1, 0))
        );
        // The switches: lit for whoever drives, or while held.
        let r = con.switches[0];
        assert_eq!(
            at(PILOT_TYPE, 0, &robot, look, false),
            Some(Place::In(SWITCH + NEXT, r))
        );
        assert_eq!(
            at(PROGRAM_TYPE, 0, &robot, look, false),
            Some(Place::In(SWITCH, r))
        );
        assert_eq!(
            at(PROGRAM_TYPE, 0, &robot, look, true),
            Some(Place::In(SWITCH + NEXT, r))
        );
        robot.program_drives = true;
        assert_eq!(
            at(PILOT_TYPE, 0, &robot, look, false),
            Some(Place::In(SWITCH, r))
        );
        // Pressed: the shared look; the lock button held shows the other light.
        look.pressed = true;
        assert_eq!(
            at(STOP_TYPE, 0, &robot, look, false),
            Some(Place::At(0x7c8 + NEXT, 1, 1))
        );
        look.lock_held = true;
        assert_eq!(
            at(LOCK_TYPE, 0, &robot, look, false),
            Some(Place::At(LOCK + NEXT, 0, -1))
        );
        robot.target = Some(Vec::new());
        assert_eq!(
            at(LOCK_TYPE, 0, &robot, look, false),
            Some(Place::At(LOCK, 0, -1))
        );
        assert_eq!(at(0xd0, 0, &robot, look, false), None);
    }
}
