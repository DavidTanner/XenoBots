//! The software mixer, modelled on `sounddvr.dvr`'s (`docs/formats/sounddvr.md`, "The
//! effects mixer"): three voices of unsigned 8-bit mono samples, each advanced by a
//! 16.16 step per tick, averaged with the previous output and sent through the Sound
//! Blaster back end's DAC table. Pure: no device, no clock.

use std::sync::Arc;

/// The driver's tick rate: PIT channel 0 at 1,193,182 Hz with divisor 165.
pub const MIX_RATE: u32 = 1_193_182 / 165;

/// Voices the driver mixes.
pub const VOICES: usize = 3;

/// The step that plays one sample a tick (all but one of `dsounds.bin`'s sounds).
pub const FULL_STEP: u16 = 0xffff;

/// A silent voice's byte (the driver's built-in silent block).
const SILENCE: u8 = 0x80;

#[derive(Clone)]
struct Voice {
    /// The block's samples and then the byte at its `end`, which is mixed once.
    data: Arc<[u8]>,
    pos: usize,
    frac: u16,
    step: u16,
}

/// Three voices mixed as the driver does; call [`Mixer::next_sample`] once a tick.
pub struct Mixer {
    /// `None` is the silent block.
    voices: [Option<Voice>; VOICES],
    /// Each voice's queued sound: `Some(None)` is a queued stop.
    next: [Option<Option<Voice>>; VOICES],
    /// The previous output `v`.
    history: u16,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Default::default(),
            next: Default::default(),
            history: SILENCE.into(),
        }
    }
}

impl Mixer {
    /// The driver's play slot (`0x44c`) at full volume: `data` (a block's samples, then
    /// the byte at its `end`; unsigned, silence 0x80) on `voice` at `step`, or `None`
    /// to silence it. Not `queued`: from now. `queued`: once the voice's sound ends, or
    /// now if it is silent or already playing the same `data`.
    // shortcut: the volume shift is not modelled (`xeno.exe` always passes 0); add it if
    // a caller ever needs another volume.
    pub fn play(&mut self, voice: usize, data: Option<Arc<[u8]>>, step: u16, queued: bool) {
        let same = match (&self.voices[voice], &data) {
            (None, _) => true,
            (Some(v), Some(d)) => Arc::ptr_eq(&v.data, d),
            (Some(_), None) => false,
        };
        let sound = data.map(|data| Voice {
            data,
            pos: 0,
            frac: 0,
            step,
        });
        if queued && !same {
            self.next[voice] = Some(sound);
        } else {
            self.voices[voice] = sound;
        }
    }

    /// Whether `voice` is playing a sound.
    pub fn is_playing(&self, voice: usize) -> bool {
        self.voices[voice].is_some()
    }

    /// One tick: each voice advances, then fetches; a voice whose position reaches its
    /// end takes its queued sound (or silence) after its byte is mixed. The sum and the
    /// previous output give `v = (sum + h + 1) >> 2`; returned is the Sound Blaster's DAC
    /// byte for the 9-bit index `(sum + h + 1) >> 1`, centred on 0.
    pub fn next_sample(&mut self) -> i8 {
        let mut sum = 0u16;
        for (slot, next) in self.voices.iter_mut().zip(&mut self.next) {
            let Some(v) = slot else {
                sum += u16::from(SILENCE);
                continue;
            };
            let (frac, carry) = v.frac.overflowing_add(v.step);
            v.frac = frac;
            v.pos += usize::from(carry);
            sum += u16::from(v.data[v.pos]);
            if v.pos + 1 >= v.data.len() {
                *slot = next.take().flatten();
            }
        }
        let x = sum + self.history + 1;
        self.history = x >> 2;
        // The DAC table at `0x1b7c`: 1 below index 128, then `min(index - 128, 255)`.
        let index = x >> 1;
        let dac = if index < 128 {
            1
        } else {
            (index - 128).min(255)
        };
        (dac as i16 - 128) as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mixer: &mut Mixer, ticks: usize) -> Vec<i8> {
        (0..ticks).map(|_| mixer.next_sample()).collect()
    }

    fn data(bytes: &[u8]) -> Option<Arc<[u8]>> {
        Some(Arc::from(bytes))
    }

    #[test]
    fn silence_when_idle() {
        assert_eq!(run(&mut Mixer::default(), 3), [0, 0, 0]);
    }

    #[test]
    fn advances_then_fetches_and_mixes_the_end_byte_once() {
        let mut m = Mixer::default();
        // Two samples and the end byte, each 0x80 + 4 * n above silence.
        m.play(1, data(&[0x84, 0x88, 0x8c]), FULL_STEP, false);
        // Tick 1 fetches sample 0 (0xffff does not carry yet), tick 2 sample 1, tick 3
        // the end byte, then silence. Each output is ((sum + h + 1) >> 1) - 256, h the last
        // (sum + h + 1) >> 2: sums 0x184, 0x188, 0x18c, 0x180, 0x180; h from 0x80.
        assert_eq!(run(&mut m, 5), [2, 5, 7, 2, 1]);
        assert!(!m.is_playing(1));
    }

    #[test]
    fn half_step_holds_each_sample_two_ticks() {
        let mut m = Mixer::default();
        m.play(0, data(&[0xc0, 0x80]), 0x8000, false);
        // Positions after each advance: 0, 1 (the end byte, then silence).
        assert_eq!(run(&mut m, 3), [32, 8, 2]);
        assert!(!m.is_playing(0));
    }

    #[test]
    fn averages_with_the_previous_output() {
        let mut m = Mixer::default();
        m.play(0, data(&[0xff, 0xff, 0xff, 0xff]), FULL_STEP, false);
        // A step up rises over a few ticks instead of at once.
        assert_eq!(run(&mut m, 4), [64, 80, 84, 85]);
    }

    #[test]
    fn clips_through_the_dac_table() {
        let mut m = Mixer::default();
        for v in 0..VOICES {
            m.play(v, data(&[0xff; 8]), FULL_STEP, false);
        }
        assert_eq!(*run(&mut m, 8).last().unwrap(), 127);
        let mut m = Mixer::default();
        for v in 0..VOICES {
            m.play(v, data(&[0x00; 8]), FULL_STEP, false);
        }
        assert_eq!(*run(&mut m, 8).last().unwrap(), -127);
    }

    #[test]
    fn queued_sounds_wait_for_the_end() {
        let mut m = Mixer::default();
        let a = data(&[0xff, 0xff]);
        // On a silent voice a queued sound starts at once.
        m.play(2, a.clone(), FULL_STEP, true);
        assert!(m.is_playing(2));
        // Queued behind another: it starts after the end byte.
        m.play(2, data(&[0x00]), FULL_STEP, true);
        run(&mut m, 2);
        assert!(m.is_playing(2));
        assert_eq!(m.voices[2].as_ref().unwrap().data[0], 0x00);
        run(&mut m, 1);
        assert!(!m.is_playing(2));
        // A queued stop lets the sound finish; an immediate one silences now.
        m.play(2, a.clone(), FULL_STEP, false);
        m.play(2, None, FULL_STEP, true);
        assert!(m.is_playing(2));
        run(&mut m, 2);
        assert!(!m.is_playing(2));
        m.play(2, a.clone(), FULL_STEP, false);
        m.play(2, None, FULL_STEP, false);
        assert!(!m.is_playing(2));
        // Queued while playing the same block: it restarts now.
        m.play(2, a.clone(), FULL_STEP, false);
        run(&mut m, 1);
        m.play(2, a, FULL_STEP, true);
        assert_eq!(m.voices[2].as_ref().unwrap().frac, 0);
    }
}
