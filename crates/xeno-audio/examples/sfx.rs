//! Plays one `dsounds.bin` block through the mixer on voice 0:
//! `cargo run -p xeno-audio --example sfx -- ./gameFiles 6`.

use std::thread::sleep;
use std::time::Duration;

use xeno_audio::{Output, block_data};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(block)) = (args.next(), args.next()) else {
        return Err("usage: sfx <game dir> <block>".into());
    };
    let bank = xeno_formats::dsounds::parse(&std::fs::read(
        std::path::Path::new(&dir).join("dsounds.bin"),
    )?)?;
    let sound = bank
        .sounds
        .get(block.parse::<usize>()?)
        .ok_or("no such block")?;
    let output = Output::open()?;
    output
        .mixer()
        .play(0, Some(block_data(sound)), sound.header.step, false);
    while output.mixer().is_playing(0) {
        sleep(Duration::from_millis(50));
    }
    // The driver's averaging lets the output settle for a moment.
    sleep(Duration::from_millis(100));
    Ok(())
}
