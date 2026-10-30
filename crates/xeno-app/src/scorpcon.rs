//! The scorpion console's instruments (T-131; `docs/screens.md`, "The scorpion
//! console's instruments (T-131)"), drawn over the picture as their draw handlers draw
//! them, from the viewed robot. The texts, tables and rectangles are read from the
//! user's `xeno.exe` (`xeno_formats::scorpcon`); sprite handles, places and colours
//! are the handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::scorpcon::ScorpCon;
use xeno_render::{BlitMode, Framebuffer};

use crate::console::{Robot, Sprites, sprite};
use crate::panels::digits;

/// The widgets drawn at their rectangles, by type; the speed (0xce), the compass
/// (0xd6) and the power (0xec) draw at fixed places.
pub const WARNING_TYPE: u16 = 0xdc;
pub const LEVER_TYPE: u16 = 0xde;
pub const DAMAGE_TYPE: u16 = 0xee;
pub const GAUGE_TYPES: [u16; 2] = [0xe0, 0xe2];
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
}
