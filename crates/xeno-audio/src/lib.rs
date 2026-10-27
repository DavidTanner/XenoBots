//! Sound for the Xenobots port: a pure [`Mixer`] modelled on `sounddvr.dvr`'s, its
//! `.ms3` [`Music`] driver on an emulated OPL2, and an [`Output`] that plays both on the
//! default audio device through `rodio`.

mod mixer;
mod music;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Sample, SampleRate, Source};

pub use mixer::{FULL_STEP, MIX_RATE, Mixer, VOICES};
pub use music::{Driver, Music, OPL_RATE, Opl};
pub use rodio::DeviceSinkError;

/// The default audio device, fed by a [`Mixer`] at [`MIX_RATE`] and, once given a bank,
/// [`Music`] at [`OPL_RATE`]: two sources that rodio resamples and sums, so the
/// effects and the music keep their own rates as on the original's two devices.
/// Sound stops when dropped.
pub struct Output {
    _sink: MixerDeviceSink,
    mixer: Arc<Mutex<Mixer>>,
    music: Arc<Mutex<Option<Music>>>,
}

impl Output {
    /// Opens the default output device and starts the mixer on it (silent until played).
    pub fn open() -> Result<Self, DeviceSinkError> {
        // rodio's fixed rate/50 buffer can exceed a device's range (2048 > 1024 on a
        // 96 kHz Mac); the device's own default size always fits.
        let mut sink = DeviceSinkBuilder::from_default_device()?
            .with_buffer_size(rodio::cpal::BufferSize::Default)
            .open_stream()?;
        sink.log_on_drop(false);
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        sink.mixer().add(MixerSource(Arc::clone(&mixer)));
        let music = Arc::new(Mutex::new(None));
        sink.mixer().add(MusicSource {
            music: Arc::clone(&music),
            buf: [0; MUSIC_BATCH],
            at: MUSIC_BATCH,
        });
        Ok(Self {
            _sink: sink,
            mixer,
            music,
        })
    }

    /// The music the device is playing, to start, stop and tick songs: `None` until
    /// [`Output::set_music`].
    pub fn music(&self) -> MutexGuard<'_, Option<Music>> {
        self.music.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Plays `music` (its driver and chip) from now on, in place of any before.
    pub fn set_music(&self, music: Music) {
        *self.music() = Some(music);
    }

    /// The mixer the device is playing, to start and stop voices.
    pub fn mixer(&self) -> MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Endless mono stream of the mixer's ticks, for rodio to resample to the device.
struct MixerSource(Arc<Mutex<Mixer>>);

impl Iterator for MixerSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        // shortcut: one lock a sample (7231 a second); batch if it shows up in profiles.
        let s = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .next_sample();
        Some(Sample::from(s) / 128.0)
    }
}

impl Source for MixerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::MIN
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(MIX_RATE).expect("non-zero rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Samples rendered under one lock of the music.
const MUSIC_BATCH: usize = 256;

/// Endless mono stream of the emulated OPL2 at its own rate (silence with no music).
// shortcut: register writes land between batches the device pulls (a few ms), not at
// the retrace's exact sample; queue timestamped writes if notes sound uneven.
struct MusicSource {
    music: Arc<Mutex<Option<Music>>>,
    buf: [i16; MUSIC_BATCH],
    at: usize,
}

impl Iterator for MusicSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.at == MUSIC_BATCH {
            match &mut *self.music.lock().unwrap_or_else(|e| e.into_inner()) {
                Some(m) => m.render(&mut self.buf),
                None => self.buf.fill(0),
            }
            self.at = 0;
        }
        self.at += 1;
        Some(Sample::from(self.buf[self.at - 1]) / 32768.0)
    }
}

impl Source for MusicSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::MIN
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(OPL_RATE).expect("non-zero rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
