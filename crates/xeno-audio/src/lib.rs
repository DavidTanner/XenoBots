//! Sound for the Xenobots port: a pure [`Mixer`] modelled on `sounddvr.dvr`'s, and an
//! [`Output`] that plays it on the default audio device through `rodio`.

mod mixer;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Sample, SampleRate, Source};

pub use mixer::{FULL_STEP, MIX_RATE, Mixer, VOICES};
pub use rodio::DeviceSinkError;

/// The default audio device, fed by a [`Mixer`] at [`MIX_RATE`]. Sound stops when dropped.
pub struct Output {
    _sink: MixerDeviceSink,
    mixer: Arc<Mutex<Mixer>>,
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
        Ok(Self { _sink: sink, mixer })
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
