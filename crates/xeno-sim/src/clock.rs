//! The mission clock (`DS:0x7d46`), which `sim_advance_clock` (`1000:ed05`) moves on
//! once a retrace, and the time-of-day weight `pal_tint_for_time_of_day` (`1000:ed70`)
//! reads from it for the 3D view's colours (`docs/sim.md`, "The mission clock").

use xeno_formats::usg;

/// Hours, minutes, seconds and sixtieths, from the scenario's `SHDR`, then the
/// campaign's date: the day from 0 (`DS:0x7d4a`) and the month from 0 (`0x7d4b`),
/// which the host sets to the campaign's (the loader puts it back after the file's).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub tick: u8,
    pub day: u8,
    pub month: u8,
}

impl Clock {
    /// Sixtieths of a second added each retrace: game time runs 4/60 s a retrace,
    /// about 4.7 times real time at 70 Hz.
    pub const STEP: u8 = 4;
    /// The tint's weight at night: the blend's end frame.
    pub const NIGHT: u8 = 0x80;

    pub fn from_start(start: &usg::Clock) -> Clock {
        Clock {
            hour: start.hour,
            minute: start.minute,
            second: start.second,
            tick: start.tick,
            day: start.day,
            month: start.month,
        }
    }

    /// One retrace (`sim_advance_clock`). Each carry subtracts rather than zeroes, as
    /// the original does. Past 23:59 the day steps on, and once it reaches the month's
    /// days in `month_days` it starts again from 0 in the next month, with no wrap past
    /// December and no year.
    // shortcut: past the table's 12 months the original compares with the bytes after
    // it; here such a month never ends. A campaign starts in April and moves 0 to 3
    // days a result, so no campaign gets there.
    pub fn advance(&mut self, month_days: &[u8]) {
        self.tick = self.tick.wrapping_add(Self::STEP);
        if self.tick >= 60 {
            self.tick -= 60;
            self.second = self.second.wrapping_add(1);
        }
        if self.second >= 60 {
            self.second -= 60;
            self.minute = self.minute.wrapping_add(1);
        }
        if self.minute >= 60 {
            self.minute -= 60;
            self.hour = self.hour.wrapping_add(1);
        }
        if self.hour >= 24 {
            self.hour -= 24;
            self.day = self.day.wrapping_add(1);
            if month_days
                .get(usize::from(self.month))
                .is_some_and(|&days| self.day >= days)
            {
                self.day = 0;
                self.month = self.month.wrapping_add(1);
            }
        }
    }

    /// The weight `pal_tint_for_time_of_day` starts `mgavideo.dvr`'s records 5 and 6
    /// with: night before 06:00 and after 18:59, `0x78 - 2 * minute` over 06:xx,
    /// `2 * minute` over 18:xx, 0 (the palette itself) between.
    pub fn tint(&self) -> u8 {
        let twice = self.minute.wrapping_shl(1);
        match self.hour {
            0..6 | 19.. => Self::NIGHT,
            6 => 0x78u8.wrapping_sub(twice),
            18 => twice,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u8, minute: u8) -> Clock {
        Clock {
            hour,
            minute,
            ..Clock::default()
        }
    }

    #[test]
    fn the_tint_follows_the_hours() {
        let cases = [
            (0, 0, 0x80),
            (5, 59, 0x80),
            (6, 0, 0x78),
            (6, 30, 0x3c),
            (6, 59, 2),
            (7, 0, 0),
            (12, 0, 0),
            (17, 59, 0),
            (18, 0, 0),
            (18, 1, 2),
            (18, 59, 118),
            (19, 0, 0x80),
            (23, 59, 0x80),
        ];
        for (hour, minute, weight) in cases {
            assert_eq!(at(hour, minute).tint(), weight, "{hour:02}:{minute:02}");
        }
    }

    #[test]
    fn a_retrace_is_four_sixtieths() {
        let mut clock = Clock {
            hour: 23,
            minute: 59,
            second: 59,
            tick: 56,
            ..Clock::default()
        };
        clock.advance(&[]);
        assert_eq!(clock.hour, 0, "midnight wraps the hour");
        // 15 retraces a second, so an hour is 54,000.
        let mut clock = at(6, 0);
        for _ in 0..54_000 {
            clock.advance(&[]);
        }
        assert_eq!(clock, at(7, 0));
        // A carry subtracts: a tick of 58 leaves 2.
        let mut clock = Clock {
            tick: 58,
            ..Clock::default()
        };
        clock.advance(&[]);
        assert_eq!((clock.second, clock.tick), (1, 2));
    }

    #[test]
    fn midnight_steps_the_date() {
        let midnight = |day, month| Clock {
            hour: 23,
            minute: 59,
            second: 59,
            tick: 56,
            day,
            month,
        };
        let days = [2, 3];
        let next = |mut c: Clock| {
            c.advance(&days);
            (c.hour, c.day, c.month)
        };
        assert_eq!(next(midnight(0, 0)), (0, 1, 0), "the day steps on");
        assert_eq!(
            next(midnight(1, 0)),
            (0, 0, 1),
            "the month's last day ends it"
        );
        // The campaign's own days can pass the month's end: the next midnight ends it.
        assert_eq!(next(midnight(5, 1)), (0, 0, 2));
        // Past the table the month never ends.
        assert_eq!(next(midnight(9, 2)), (0, 10, 2));
        let mut c = midnight(0, 0);
        c.tick = 0;
        c.advance(&days);
        assert_eq!((c.hour, c.day), (23, 0));
    }
}
