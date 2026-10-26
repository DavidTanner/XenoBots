//! `xeno-tool simulate`: runs a scenario in the sim with no window and prints where
//! its robots are (`docs/sim.md`, "Movement", "Orders"). The robots' scripts drive
//! them; `--drive` gives one a speed and heading to start with (its script may change
//! them).

use std::fmt::Write;

use anyhow::{Context, bail};
use xeno_sim::motion::{Event, Tables};
use xeno_sim::{EntityId, Random, Sim, State};

/// A robot to drive: its slot, the speed asked for and the heading asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drive {
    pub slot: u16,
    pub speed: i16,
    pub heading: u16,
}

/// Parses `SLOT:SPEED:HEADING` (numbers in decimal, or hex with `0x`).
pub fn parse_drive(text: &str) -> Result<Drive, String> {
    let number = |s: &str| -> Result<i64, String> {
        let parsed = match s.strip_prefix("0x") {
            Some(hex) => i64::from_str_radix(hex, 16),
            None => s.parse(),
        };
        parsed.map_err(|e| format!("{s}: {e}"))
    };
    let parts: Vec<&str> = text.split(':').collect();
    let [slot, speed, heading] = parts[..] else {
        return Err("expected SLOT:SPEED:HEADING".into());
    };
    Ok(Drive {
        slot: u16::try_from(number(slot)?).map_err(|e| e.to_string())?,
        speed: i16::try_from(number(speed)?).map_err(|e| e.to_string())?,
        heading: number(heading)? as u16,
    })
}

fn fixed(n: i32) -> String {
    format!("{:.3}", f64::from(n) / 65536.0)
}

/// The robots' lines: slot, kind, side, position, heading, speed, path and point.
fn robots(sim: &Sim) -> String {
    let mut out = String::new();
    for (id, e) in sim.world.iter() {
        if let State::Robot(m) = e.state {
            let _ = writeln!(
                out,
                "  {:3} {:?} {:?} at ({}, {}) heading {:#06x} speed {} path {:#04x} point {:#04x} to ({}, {})",
                id.0,
                e.kind,
                e.side,
                fixed(e.position.x.0),
                fixed(e.position.y.0),
                e.heading,
                m.speed,
                m.path,
                m.path_point,
                fixed(m.destination.x.0),
                fixed(m.destination.y.0),
            );
        }
    }
    out
}

/// Runs `scenario` for `ticks` retraces with `tables`, printing the robots every
/// `every` ticks (and at the start) and each event as it happens.
pub fn run(
    scenario: &xeno_formats::usg::Scenario,
    tables: Tables,
    seed: u32,
    ticks: u64,
    every: u64,
    drives: &[Drive],
) -> anyhow::Result<String> {
    let mut sim = Sim::load(scenario, Random::seeded(seed), tables);
    for d in drives {
        let Some(e) = sim.world.get_mut(EntityId(d.slot)) else {
            bail!("no object in slot {}", d.slot);
        };
        let State::Robot(m) = &mut e.state else {
            bail!("slot {} is not a robot", d.slot);
        };
        (m.target_speed, m.target_heading) = (d.speed, d.heading);
    }
    let mut out = format!("tick 0\n{}", robots(&sim));
    for _ in 0..ticks {
        for event in sim.tick(&[]) {
            let _ = writeln!(out, "tick {}: {}", sim.retrace(), describe(event));
        }
        if every != 0 && sim.retrace().is_multiple_of(every) {
            let _ = write!(out, "tick {}\n{}", sim.retrace(), robots(&sim));
        }
    }
    let _ = writeln!(out, "state hash {:016x}", sim.state_hash());
    Ok(out)
}

fn describe(event: Event) -> String {
    match event {
        Event::HillHit(id) => format!("{} hit a hill", id.0),
        Event::PathTaken { robot, slot } => format!("{} took path {slot}", robot.0),
        Event::Program(c) => format!("{} runs program {}", c.slot, c.after.program),
    }
}

/// Reads the movement tables from `exe`.
pub fn tables(exe: &std::path::Path) -> anyhow::Result<Tables> {
    let bytes = std::fs::read(exe).with_context(|| format!("cannot read {}", exe.display()))?;
    let unpacked = xeno_formats::lzexe::unpack(&bytes).context("cannot unpack xeno.exe")?;
    xeno_formats::motion::read(&unpacked).context("no movement tables this port can read")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_parse() {
        assert_eq!(
            parse_drive("5:63:0x4000"),
            Ok(Drive {
                slot: 5,
                speed: 63,
                heading: 0x4000
            })
        );
        assert_eq!(parse_drive("2:-21:0").unwrap().speed, -21);
        assert!(parse_drive("2:3").is_err());
        assert!(parse_drive("x:3:4").is_err());
    }
}
