//! The game's music (T-062): `xeno.exe`'s four songs played by `xeno_audio`'s copy of
//! `sounddvr.dvr`'s AdLib driver, started and stopped where the original does it
//! (`docs/formats/ms3.md`, "When songs play") and ticked once per retrace of the mode on
//! screen. Without an audio device, or without the bank or a song, the game runs on
//! silently: the cues are still followed, so tests need no device.
//!
//! The sound effects (T-061) go to the same device: `xeno.exe`'s sound events play
//! `dsounds.bin` blocks on the effects mixer's voices, as `snd_play_sound_effect` does.

use std::collections::HashMap;
use std::sync::Arc;

use xeno_audio::{Music, Output, block_data};
use xeno_formats::{dsounds, lzexe, ms3, sounddvr, sounds};

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
    /// What each sound event plays; empty without `dsounds.bin` or the event table.
    effects: Vec<Effect>,
}

/// A sound event as the mixer's play takes it (`Mixer::play`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    voice: usize,
    /// The block with its end byte; `None` silences the voice.
    data: Option<Arc<[u8]>>,
    step: u16,
    queued: bool,
}

/// Each event's effect: its block from `bank` (a block past the bank plays nothing) at
/// the event's step, or the block's own when that is 0.
pub fn effects(events: &[sounds::Event], bank: &dsounds::Bank) -> Vec<Effect> {
    let blocks: Vec<_> = bank.sounds.iter().map(block_data).collect();
    events
        .iter()
        .map(|e| {
            let sound = e.block.and_then(|b| bank.sounds.get(usize::from(b)));
            let own = sound.map_or(0, |s| s.header.step);
            Effect {
                voice: e.voice.into(),
                data: e.block.and_then(|b| blocks.get(usize::from(b)).cloned()),
                step: if e.step != 0 { e.step } else { own },
                queued: e.queued,
            }
        })
        .collect()
}

/// Reads the sound events from `xeno.exe` and the blocks from `dsounds.bin`.
fn load_effects(files: &GameFiles) -> Result<Vec<Effect>, String> {
    let exe = files.read("xeno.exe").map_err(|e| e.to_string())?;
    let exe = lzexe::unpack(&exe).map_err(|e| format!("xeno.exe: {e}"))?;
    let events = sounds::read(&exe).map_err(|e| format!("xeno.exe's sound events: {e}"))?;
    let bank = files.read("dsounds.bin").map_err(|e| e.to_string())?;
    let bank = dsounds::parse(&bank).map_err(|e| format!("dsounds.bin: {e}"))?;
    Ok(effects(&events, &bank))
}

impl Jukebox {
    /// Reads the bank and the songs and opens the device; a failure is logged and the
    /// game goes on without music.
    pub fn open(files: &GameFiles) -> Jukebox {
        let output = Output::open()
            .inspect_err(|e| log::warn!("no sound: cannot open the audio device: {e}"))
            .ok();
        let effects = load_effects(files).unwrap_or_else(|e| {
            log::warn!("no sound effects: {e}");
            Vec::new()
        });
        let bank = files
            .read("sounddvr.dvr")
            .map_err(|e| e.to_string())
            .and_then(|b| sounddvr::parse(&b).map_err(|e| format!("sounddvr.dvr: {e}")));
        let mut songs = HashMap::new();
        match (bank, &output) {
            (Err(e), _) => log::warn!("no music: {e}"),
            (Ok(bank), output) => {
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
                if let Some(output) = output {
                    output.set_music(Music::new(bank));
                }
            }
        }
        Jukebox {
            output,
            songs,
            playing: None,
            effects,
        }
    }

    /// Plays sound event `event` (`snd_play_sound_effect`, `1000:ee90`).
    pub fn effect(&mut self, event: u8) {
        log::debug!("sound event {event}");
        let (Some(output), Some(e)) = (&self.output, self.effects.get(usize::from(event))) else {
            return;
        };
        output
            .mixer()
            .play(e.voice, e.data.clone(), e.step, e.queued);
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

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::dsounds::{Bank, Header, Sound};

    #[test]
    fn events_play_their_block_with_its_end_byte_at_its_step() {
        let sound = |step| Sound {
            header: Header { end: 6, step },
            samples: vec![0x90, 0x70],
            unknown_tail: vec![0x81, 0x82],
        };
        let bank = Bank {
            sounds: vec![sound(0xffff), sound(0x4000)],
            trailing: Vec::new(),
        };
        let event = |block, voice, queued, step| sounds::Event {
            block,
            voice,
            queued,
            step,
        };
        let events = [
            event(Some(1), 2, true, 0),
            event(Some(0), 1, false, 0x8000),
            event(None, 0, false, 0),
            event(Some(9), 0, false, 0),
        ];
        let effects = effects(&events, &bank);
        assert_eq!(effects[0].data.as_deref(), Some(&[0x90, 0x70, 0x81][..]));
        assert_eq!((effects[0].voice, effects[0].step), (2, 0x4000));
        assert!(effects[0].queued);
        assert_eq!((effects[1].step, effects[1].queued), (0x8000, false));
        assert_eq!(
            (effects[2].data.clone(), effects[3].data.clone()),
            (None, None)
        );
    }

    /// Every stock event that names a block plays it: 14 distinct blocks are heard
    /// (`docs/formats/dsounds.md`, "Sound events").
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_sound_event_loads() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let effects = load_effects(&files).unwrap();
        assert_eq!(effects.len(), sounds::EVENTS);
        assert_eq!(effects[21].voice, 2);
        assert!(effects[2].data.is_some() && effects[6].data.is_none());
    }
}
