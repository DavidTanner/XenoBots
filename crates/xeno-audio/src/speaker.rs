//! The PC speaker: PIT channel 2 in mode 3, a square wave at 1,193,182 Hz over the
//! divisor, gated on and off by port 0x61 (`docs/sim.md`, "The beep (T-193)"). Pure: no
//! device, no clock.

use std::collections::VecDeque;

/// The PIT's input clock.
pub const PIT_HZ: u64 = 1_193_182;

/// The speaker's output rate: 800 samples a count of the 60 Hz chain.
pub const SPEAKER_RATE: u32 = 48_000;

/// Samples a count of the game clock (the port runs the chain at exactly 60 Hz).
const SAMPLES_A_COUNT: u32 = SPEAKER_RATE / 60;

/// The wave's amplitude, out of `i16::MAX`.
const LEVEL: i16 = 0x1800;

/// Beeps in turn, each a tone and then silence; call [`Speaker::next_sample`] at
/// [`SPEAKER_RATE`].
#[derive(Default)]
pub struct Speaker {
    /// (divisor, samples left); divisor 0 is silence.
    queue: VecDeque<(u16, u32)>,
    /// The PIT's count into the wave, in PIT clocks times [`SPEAKER_RATE`].
    phase: u64,
}

impl Speaker {
    /// A tone at PIT `divisor` for `on` counts of the game clock, then `off` silent,
    /// after the beeps before it: the original waits out each beep before going on.
    pub fn beep(&mut self, divisor: u16, on: u16, off: u16) {
        let samples = |n: u16| u32::from(n) * SAMPLES_A_COUNT;
        self.queue.push_back((divisor.max(1), samples(on)));
        self.queue.push_back((0, samples(off)));
    }

    /// Whether a beep, or its silence after, is still to finish.
    pub fn is_playing(&self) -> bool {
        !self.queue.is_empty()
    }

    /// One sample: high for the first half of each period, low for the second.
    pub fn next_sample(&mut self) -> i16 {
        while self.queue.front().is_some_and(|&(_, left)| left == 0) {
            self.queue.pop_front();
            self.phase = 0;
        }
        let Some((divisor, left)) = self.queue.front_mut() else {
            return 0;
        };
        *left -= 1;
        if *divisor == 0 {
            return 0;
        }
        let period = u64::from(*divisor) * u64::from(SPEAKER_RATE);
        let high = self.phase < period / 2;
        self.phase = (self.phase + PIT_HZ) % period;
        if high { LEVEL } else { -LEVEL }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_when_idle() {
        let mut s = Speaker::default();
        assert_eq!(s.next_sample(), 0);
        assert!(!s.is_playing());
    }

    /// A divisor of 1,193,182 / 600 is a 600 Hz tone: 80 samples a period at 48 kHz,
    /// 40 high then 40 low, for 2 counts (1600 samples), then 3 counts of silence.
    #[test]
    fn square_wave_then_silence() {
        let mut s = Speaker::default();
        let divisor = (PIT_HZ / 600) as u16;
        s.beep(divisor, 2, 3);
        let tone: Vec<i16> = (0..1600).map(|_| s.next_sample()).collect();
        let highs = tone.iter().filter(|&&v| v == LEVEL).count();
        assert_eq!(tone.iter().filter(|&&v| v == -LEVEL).count(), 1600 - highs);
        // Duty about a half, edges about every 40 samples.
        assert!((790..=810).contains(&highs), "{highs}");
        let edges = tone.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((39..=41).contains(&edges), "{edges}");
        assert!(tone[..39].iter().all(|&v| v == LEVEL) && tone[41] == -LEVEL);
        assert!((0..2400).all(|_| s.next_sample() == 0));
        assert!(s.is_playing(), "the silence is part of the beep");
        s.next_sample();
        assert!(!s.is_playing());
    }

    #[test]
    fn beeps_follow_each_other() {
        let mut s = Speaker::default();
        s.beep(1000, 1, 1);
        s.beep(1000, 1, 0);
        let out: Vec<i16> = (0..3 * SAMPLES_A_COUNT).map(|_| s.next_sample()).collect();
        let n = SAMPLES_A_COUNT as usize;
        assert!(out[..n].contains(&LEVEL));
        assert!(out[n..2 * n].iter().all(|&v| v == 0));
        // The second tone starts its wave afresh, high.
        assert_eq!(out[2 * n], LEVEL);
        assert!(out[2 * n..].contains(&-LEVEL));
    }
}
