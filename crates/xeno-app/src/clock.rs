//! The game clock: a fixed timestep at the original's vertical retrace rate, counted in
//! exact integers from a time the caller supplies, so it is tested without sleeping or
//! a window. See `docs/game-loop.md`.
//!
//! The rates are the sim's time base, [`xeno_sim::Rate`] (T-050); the stepping stays
//! with the host.

use std::time::Duration;

pub use xeno_sim::Rate;

/// The most retraces one [`Clock::advance`] runs. A longer stall (a debugger, a blocked
/// event loop, a sleeping laptop) drops the rest: game time pauses instead of racing.
pub const MAX_CATCH_UP: u64 = 10;

/// One retrace step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Retraces run so far, this one included (1 for the first).
    pub retrace: u64,
    /// The 60 Hz ticks so far, including any that fire on this retrace: 0 or 1 a
    /// retrace at 70 Hz, 1 or 2 at 59.94 Hz. The original advances that chain from each
    /// retrace (`docs/re/dvr-protocol.md`), so it is derived from the retrace count.
    pub sixty_hz: u64,
}

/// A fixed-timestep clock. Retrace `n` is due once `n` whole periods have passed since
/// the epoch; the count is recomputed from the elapsed time each call, never summed, so
/// it stays exact over any run.
#[derive(Debug, Clone)]
pub struct Clock {
    rate: Rate,
    /// When the count was last rebased, and the retraces run by then.
    epoch: Duration,
    base: u64,
    /// Retraces run.
    retraces: u64,
    /// Retraces dropped after stalls.
    dropped: u64,
}

impl Clock {
    pub fn new(rate: Rate, now: Duration) -> Self {
        Clock {
            rate,
            epoch: now,
            base: 0,
            retraces: 0,
            dropped: 0,
        }
    }

    pub fn retraces(&self) -> u64 {
        self.retraces
    }

    /// The retrace rate it runs at.
    #[cfg(test)]
    pub fn rate(&self) -> Rate {
        self.rate
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Runs the retraces due by `now`, at most [`MAX_CATCH_UP`]. If more were due, the
    /// rest are dropped and the clock restarts from `now`, so the next one is a full
    /// period away.
    pub fn advance(&mut self, now: Duration) -> impl Iterator<Item = Step> + use<> {
        let due = self.base + self.rate.ticks_in(now.saturating_sub(self.epoch));
        let behind = due.saturating_sub(self.retraces);
        let run = behind.min(MAX_CATCH_UP);
        let first = self.retraces + 1;
        self.retraces += run;
        if behind > run {
            self.dropped += behind - run;
            self.epoch = now;
            self.base = self.retraces;
        }
        let rate = self.rate;
        (first..=self.retraces).map(move |n| Step {
            retrace: n,
            sixty_hz: rate.convert(n, Rate::SIXTY_HZ),
        })
    }

    /// When the next retrace is due.
    pub fn next_due(&self) -> Duration {
        self.epoch + self.rate.time_of(self.retraces - self.base + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_for(clock: &mut Clock, now: Duration) -> Vec<Step> {
        clock.advance(now).collect()
    }

    #[test]
    fn an_hour_of_uneven_frames_counts_exactly() {
        for rate in [Rate::MODE_13H, Rate::MODE_240] {
            let mut clock = Clock::new(rate, Duration::ZERO);
            let (mut now, mut steps, mut sixty, mut seed) = (Duration::ZERO, 0u64, 0u64, 1u64);
            let hour = Duration::from_secs(3600);
            while now < hour {
                // Waits of 1 to 40 ms, like a vsynced or late event loop.
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                now = (now + Duration::from_micros(1000 + (seed >> 33) % 39_000)).min(hour);
                for step in clock.advance(now) {
                    steps += 1;
                    assert_eq!(step.retrace, steps);
                    assert!(step.sixty_hz - sixty <= 2);
                    sixty = step.sixty_hz;
                }
            }
            // 3600 * 25,175,000 / 359,200 = 252,310.7; / 420,000 = 215,785.7.
            assert_eq!(
                steps,
                [252_310, 215_785][usize::from(rate == Rate::MODE_240)]
            );
            assert_eq!(sixty, 3600 * 60 - 1); // 216,000 minus the part-tick at the end.
            assert_eq!(clock.dropped(), 0);
        }
    }

    #[test]
    fn retraces_fall_due_on_their_period() {
        let mut clock = Clock::new(Rate::MODE_13H, Duration::from_secs(5));
        assert_eq!(run_for(&mut clock, Duration::from_secs(5)), []);
        // 359,200 / 25,175,000 s = 14,268,123.1 ns.
        assert_eq!(
            clock.next_due(),
            Duration::from_nanos(5_000_000_000 + 14_268_124)
        );
        let due = clock.next_due();
        assert_eq!(run_for(&mut clock, due - Duration::from_nanos(1)), []);
        assert_eq!(run_for(&mut clock, due).len(), 1);
        assert_eq!(run_for(&mut clock, due), []);
        assert_eq!(
            clock.next_due(),
            Duration::from_nanos(5_000_000_000 + 28_536_247)
        );
    }

    #[test]
    fn sixty_hz_ticks_ride_on_retraces() {
        for (rate, allowed) in [(Rate::MODE_13H, [0, 1]), (Rate::MODE_240, [1, 2])] {
            let mut clock = Clock::new(rate, Duration::ZERO);
            let (mut seen, mut total) = (Vec::new(), 0);
            for ms in 1..=20_000 {
                for step in clock.advance(Duration::from_millis(ms)) {
                    let fired = step.sixty_hz - total;
                    assert!(allowed.contains(&fired), "{rate:?} {step:?}");
                    seen.push(fired);
                    total = step.sixty_hz;
                }
            }
            assert_eq!(total, 1199); // 20 s of retraces, each rate's last one before 20 s.
            assert!(allowed.iter().all(|n| seen.contains(n)));
        }
    }

    /// Advances in 1 ms steps for a second from `from`, as a smooth loop would.
    fn a_second_from(clock: &mut Clock, from: Duration) -> usize {
        (1..=1000)
            .map(|ms| run_for(clock, from + Duration::from_millis(ms)).len())
            .sum()
    }

    #[test]
    fn a_hiccup_is_caught_up_and_a_stall_drops_time() {
        let ms = Duration::from_millis;
        let mut clock = Clock::new(Rate::MODE_13H, Duration::ZERO);
        assert_eq!(a_second_from(&mut clock, Duration::ZERO), 70);
        // 100 ms late: 7 retraces at once, nothing lost.
        assert_eq!(run_for(&mut clock, ms(1100)).len(), 7);
        assert_eq!(clock.dropped(), 0);
        // A 5 s stall: 427 due by 6.1 s, 77 run, so 350 behind: 10 run, 340 dropped.
        let steps = run_for(&mut clock, ms(6100));
        assert_eq!(steps.len() as u64, MAX_CATCH_UP);
        assert_eq!(steps.last().unwrap().retrace, 87);
        assert_eq!(clock.dropped(), 340);
        // The clock restarts at the stall's end: the next retrace is one period later,
        // and from then on the rate is exact again.
        assert_eq!(clock.next_due(), ms(6100) + Rate::MODE_13H.time_of(1));
        assert_eq!(a_second_from(&mut clock, ms(6100)), 70);
        assert_eq!(run_for(&mut clock, ms(16_100)).len(), 10);
        assert_eq!((clock.retraces(), clock.dropped()), (167, 340 + 620));
    }

    #[test]
    fn time_going_backwards_runs_nothing() {
        let mut clock = Clock::new(Rate::MODE_240, Duration::from_secs(2));
        assert_eq!(run_for(&mut clock, Duration::from_secs(1)), []);
        assert_eq!(run_for(&mut clock, Duration::from_millis(2100)).len(), 5);
        assert_eq!(run_for(&mut clock, Duration::from_millis(2050)), []);
    }
}
