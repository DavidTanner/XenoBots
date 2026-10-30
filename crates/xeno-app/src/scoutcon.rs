//! The scout console's instruments (T-132; `docs/screens.md`, "The scout console's
//! instruments (T-132)"), drawn over the picture as their draw handlers draw them, from
//! the viewed robot. The texts, tables and rectangles are read from the user's
//! `xeno.exe` (`xeno_formats::scoutcon`); sprite handles, places and colours are the
//! handlers' immediates, short code constants kept here.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::scoutcon::ScoutCon;
use xeno_render::{BlitMode, Framebuffer};

use crate::console::{Robot, Sprites, sprite};
use crate::panels::digits;

/// The widgets drawn at their rectangles, by type; the compass (0x10a), the speed
/// (0x12c) and the power (0x13c) draw at fixed places.
pub const WARNING_TYPE: u16 = 0x12a;
pub const GEAR_TYPE: u16 = 0x124;
pub const STANCE_TYPE: u16 = 0x13a;
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

#[cfg(test)]
mod tests {
    use super::*;

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
