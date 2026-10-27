//! The software mixer, modelled on `sounddvr.dvr`'s (`docs/formats/dsounds.md`):
//! three voices of unsigned 8-bit mono samples, each advanced by a 16.16 step per
//! tick, summed into one signed 8-bit output sample. Pure: no device, no clock.

use std::sync::Arc;

/// The driver's tick rate: PIT channel 0 at 1,193,182 Hz with divisor 165.
pub const MIX_RATE: u32 = 1_193_182 / 165;

/// Voices the driver mixes.
pub const VOICES: usize = 3;

/// The step that plays one sample a tick (all but one of `dsounds.bin`'s sounds).
pub const FULL_STEP: u16 = 0xffff;

struct Voice {
    samples: Arc<[u8]>,
    pos: usize,
    frac: u16,
    step: u16,
}

/// Three voices summed with clipping; call [`Mixer::next_sample`] once a tick.
#[derive(Default)]
pub struct Mixer {
    voices: [Option<Voice>; VOICES],
}

impl Mixer {
    /// Starts `samples` (unsigned 8-bit, silence 0x80) on `voice`, replacing what it
    /// played. Each tick adds `step` to a 16-bit fraction and advances by the carry.
    pub fn play(&mut self, voice: usize, samples: Arc<[u8]>, step: u16) {
        self.voices[voice] = Some(Voice {
            samples,
            pos: 0,
            frac: 0,
            step,
        });
    }

    /// Silences `voice`.
    pub fn stop(&mut self, voice: usize) {
        self.voices[voice] = None;
    }

    /// Whether `voice` still has samples to play.
    pub fn is_playing(&self, voice: usize) -> bool {
        self.voices[voice].is_some()
    }

    /// One tick: every voice's current sample, centred and summed, clipped to 8 bits.
    // shortcut: clips the sum to i8 like an 8-bit DAC would; T-061 should confirm the
    // Sound Blaster back end's arithmetic (`dsounds.md`, "Open questions").
    pub fn next_sample(&mut self) -> i8 {
        let mut sum = 0i16;
        for slot in &mut self.voices {
            let Some(v) = slot else { continue };
            sum += i16::from(v.samples[v.pos]) - 0x80;
            let (frac, carry) = v.frac.overflowing_add(v.step);
            v.frac = frac;
            v.pos += usize::from(carry);
            if v.pos >= v.samples.len() {
                *slot = None;
            }
        }
        sum.clamp(i8::MIN.into(), i8::MAX.into()) as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mixer: &mut Mixer, ticks: usize) -> Vec<i8> {
        (0..ticks).map(|_| mixer.next_sample()).collect()
    }

    #[test]
    fn silence_when_idle() {
        assert_eq!(run(&mut Mixer::default(), 3), [0, 0, 0]);
    }

    #[test]
    fn plays_a_voice_then_ends_it() {
        let mut m = Mixer::default();
        m.play(1, Arc::from([0x90, 0x70, 0x80]), FULL_STEP);
        // 0xffff carries on every tick but the first of each 65536.
        assert_eq!(run(&mut m, 5), [16, 16, -16, 0, 0]);
        assert!(!m.is_playing(1));
    }

    #[test]
    fn half_step_holds_each_sample_two_ticks() {
        let mut m = Mixer::default();
        m.play(0, Arc::from([0x81, 0x82]), 0x8000);
        assert_eq!(run(&mut m, 5), [1, 1, 2, 2, 0]);
        assert!(!m.is_playing(0));
    }

    #[test]
    fn sums_voices() {
        let mut m = Mixer::default();
        m.play(0, Arc::from([0x90]), 0x8000);
        m.play(2, Arc::from([0x78]), 0x8000);
        assert_eq!(m.next_sample(), 0x10 - 8);
    }

    #[test]
    fn clips_the_sum() {
        let mut m = Mixer::default();
        for v in 0..VOICES {
            m.play(v, Arc::from([0xff, 0x00]), 0x8000);
        }
        assert_eq!(run(&mut m, 4), [127, 127, -128, -128]);
    }

    #[test]
    fn stop_silences_a_voice() {
        let mut m = Mixer::default();
        m.play(0, Arc::from([0xff; 4]), 0x8000);
        m.stop(0);
        assert!(!m.is_playing(0));
        assert_eq!(m.next_sample(), 0);
    }
}
