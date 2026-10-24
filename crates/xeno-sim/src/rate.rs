//! Tick rates: the original's vertical retrace rates as exact fractions from the VGA
//! timings (`docs/game-loop.md`, "The clock"). The sim steps once per retrace, so a
//! rate is its time base; the host's `Clock` counts retraces with it.

use std::time::Duration;

const NANOS: u128 = 1_000_000_000;

/// The VGA's 25 MHz dot clock, nominally 25.175 MHz (IBM VGA Technical Reference).
const VGA_DOT_CLOCK: u64 = 25_175_000;

/// A tick rate, `num / den` ticks a second, kept as a fraction so counts never drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    num: u64,
    den: u64,
}

impl Rate {
    /// Mode 13h: the dot clock over 800 dots a line (CRTC horizontal total `5Fh` + 5
    /// characters of 8 dots) and 449 lines a frame (vertical total `1BFh` + 2):
    /// 25,175,000 / 359,200, about 70.086 Hz.
    pub const MODE_13H: Rate = Rate::from_timing(VGA_DOT_CLOCK, 800, 449);
    /// The 320x240 mode keeps mode 12h's timing: 800 dots, 525 lines (`20Bh` + 2):
    /// 25,175,000 / 420,000, about 59.940 Hz.
    pub const MODE_240: Rate = Rate::from_timing(VGA_DOT_CLOCK, 800, 525);
    /// The original's second hook chain, 60 times a second whatever the mode.
    pub const SIXTY_HZ: Rate = Rate { num: 60, den: 1 };

    const fn from_timing(dot_clock: u64, dots_per_line: u64, lines: u64) -> Rate {
        Rate {
            num: dot_clock,
            den: dots_per_line * lines,
        }
    }

    /// The retrace rate of a screen of `height` rows: 240 is the 320x240 mode, anything
    /// else mode 13h (`docs/formats/video-modes.md`).
    pub fn for_height(height: usize) -> Rate {
        if height == 240 {
            Rate::MODE_240
        } else {
            Rate::MODE_13H
        }
    }

    /// The rate in hertz, for logs and display only: no float goes into the state.
    pub fn hz(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Whole ticks that fit in `t`.
    pub fn ticks_in(self, t: Duration) -> u64 {
        (t.as_nanos() * u128::from(self.num) / (u128::from(self.den) * NANOS)) as u64
    }

    /// The first instant at which `n` whole ticks fit.
    pub fn time_of(self, n: u64) -> Duration {
        let ns = (u128::from(n) * u128::from(self.den) * NANOS).div_ceil(u128::from(self.num));
        Duration::from_nanos(ns as u64)
    }

    /// Ticks of `other` in the time of `n` ticks of `self`, rounded down.
    pub fn convert(self, n: u64, other: Rate) -> u64 {
        let ticks = u128::from(n) * u128::from(self.den) * u128::from(other.num)
            / (u128::from(self.num) * u128::from(other.den));
        ticks as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_come_from_the_vga_timings() {
        assert!((Rate::MODE_13H.hz() - 70.0863).abs() < 1e-4);
        assert!((Rate::MODE_240.hz() - 59.9405).abs() < 1e-4);
        assert_eq!(Rate::for_height(200), Rate::MODE_13H);
        assert_eq!(Rate::for_height(240), Rate::MODE_240);
    }
}
