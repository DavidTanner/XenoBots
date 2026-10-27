//! Plays a song from the user's files on the emulated OPL2, ticked at mode 13h's
//! retrace rate, until its end: `cargo run -p xeno-audio --example music -- ./gameFiles
//! mainthem`.

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use xeno_audio::{Music, Output};
use xeno_sim::Rate;

fn read(dir: &Path, name: &str) -> Vec<u8> {
    let path = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)))
        .unwrap_or_else(|| panic!("no {name} in {}", dir.display()));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, name] = &args[..] else {
        panic!("usage: music <game dir> <song, e.g. mainthem>");
    };
    let dir = Path::new(dir);
    let bank = xeno_formats::sounddvr::parse(&read(dir, "sounddvr.dvr")).expect("sounddvr.dvr");
    let song = xeno_formats::ms3::parse(&read(dir, &format!("{name}.ms3"))).expect("song");
    let output = Output::open().expect("audio device");
    let mut music = Music::new(bank);
    music.start(&song);
    output.set_music(music);
    println!(
        "{name}: {} ticks at {:.2} Hz",
        song.ticks(),
        Rate::MODE_13H.hz()
    );
    let start = Instant::now();
    let mut n = 0;
    while output
        .music()
        .as_ref()
        .is_some_and(|m| m.driver.is_playing())
    {
        n += 1;
        if let Some(wait) =
            (start + Rate::MODE_13H.time_of(n)).checked_duration_since(Instant::now())
        {
            sleep(wait);
        }
        if let Some(m) = output.music().as_mut() {
            m.tick();
        }
    }
    println!("ended after {n} ticks");
    sleep(Duration::from_secs(1)); // the last notes' release
}
