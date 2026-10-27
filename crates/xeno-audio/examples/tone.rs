//! Plays a synthetic tone through the mixer: an A (440 Hz) on voice 0 for a second,
//! then an E above it joins on voice 1. `cargo run -p xeno-audio --example tone`.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use xeno_audio::{FULL_STEP, MIX_RATE, Output};

/// `secs` of a sine at `hz` as unsigned 8-bit samples at the mixer's rate, at half volume
/// so two voices stay under the clip.
fn tone(hz: f64, secs: f64) -> Arc<[u8]> {
    let n = (f64::from(MIX_RATE) * secs) as usize;
    (0..n)
        .map(|i| (128.0 + 63.0 * (TAU * hz * i as f64 / f64::from(MIX_RATE)).sin()) as u8)
        .collect()
}

fn main() -> Result<(), xeno_audio::DeviceSinkError> {
    let output = Output::open()?;
    output.mixer().play(0, tone(440.0, 2.0), FULL_STEP);
    sleep(Duration::from_secs(1));
    output.mixer().play(1, tone(659.25, 1.0), FULL_STEP);
    while output.mixer().is_playing(0) || output.mixer().is_playing(1) {
        sleep(Duration::from_millis(50));
    }
    Ok(())
}
