//! The game's music (T-062): `xeno.exe`'s four songs played by `xeno_audio`'s copy of
//! `sounddvr.dvr`'s AdLib driver, started and stopped where the original does it
//! (`docs/formats/ms3.md`, "When songs play") and ticked once per retrace of the mode on
//! screen. Without an audio device, or without the bank or a song, the game runs on
//! silently: the cues are still followed, so tests need no device.

use std::collections::HashMap;

use xeno_audio::{Music, Output};
use xeno_formats::{ms3, sounddvr};

use crate::game_dir::GameFiles;

/// The song table at `DS:0xec4c`, by file name.
pub const MAIN_THEME: &str = "mainthem.ms3";
pub const WIN1: &str = "win1.ms3";
pub const WIN2: &str = "win2.ms3";
pub const BIG_WIN3: &str = "bigwin3.ms3";

/// `snd_start_song` (`1000:fc36`) or `snd_stop_song` (`1000:fc60`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start(&'static str),
    Stop,
}

/// The music, on the default audio device when there is one.
#[derive(Default)]
pub struct Jukebox {
    output: Option<Output>,
    songs: HashMap<&'static str, ms3::Song>,
    /// The song last started and not stopped since (it may have ended).
    pub playing: Option<&'static str>,
}

impl Jukebox {
    /// Reads the bank and the songs and opens the device; a failure is logged and the
    /// game goes on without music.
    pub fn open(files: &GameFiles) -> Jukebox {
        let bank = files
            .read("sounddvr.dvr")
            .map_err(|e| e.to_string())
            .and_then(|b| sounddvr::parse(&b).map_err(|e| format!("sounddvr.dvr: {e}")));
        let bank = match bank {
            Ok(bank) => bank,
            Err(e) => {
                log::warn!("no music: {e}");
                return Jukebox::default();
            }
        };
        let mut songs = HashMap::new();
        for name in [MAIN_THEME, WIN1, WIN2, BIG_WIN3] {
            match files
                .read(name)
                .map_err(|e| e.to_string())
                .and_then(|b| ms3::parse(&b).map_err(|e| format!("{name}: {e}")))
            {
                Ok(song) => _ = songs.insert(name, song),
                Err(e) => log::warn!("{name} will not play: {e}"),
            }
        }
        let output = match Output::open() {
            Ok(output) => {
                output.set_music(Music::new(bank));
                Some(output)
            }
            Err(e) => {
                log::warn!("no sound: cannot open the audio device: {e}");
                None
            }
        };
        Jukebox {
            output,
            songs,
            playing: None,
        }
    }

    pub fn cue(&mut self, cue: Cue) {
        log::debug!("music: {cue:?}");
        self.playing = match cue {
            Cue::Start(name) => Some(name),
            Cue::Stop => None,
        };
        let Some(output) = &self.output else { return };
        let mut music = output.music();
        let Some(music) = music.as_mut() else { return };
        match cue {
            Cue::Start(name) => {
                if let Some(song) = self.songs.get(name) {
                    music.start(song);
                }
            }
            Cue::Stop => music.stop(),
        }
    }

    /// One retrace.
    pub fn tick(&mut self) {
        if let Some(output) = &self.output
            && let Some(music) = &mut *output.music()
        {
            music.tick();
        }
    }
}
