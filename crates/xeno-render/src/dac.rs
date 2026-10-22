//! The VGA DAC as `mgavideo.dvr` drives it (`docs/formats/palette.md`, "Fades"): code
//! writes a shadow palette and marks it dirty; once per retrace the shadow is uploaded
//! if dirty, and then a running fade computes the next shadow. So a change shows from
//! the next retrace, and each fade step from the one after it.
//!
//! Only what the cutscene player needs (T-036): fading colours `0..=last` in from black
//! or out to black. T-037 generalises it (a first colour, colour-animation records).
//! Values are 6-bit DAC levels, 0..=63, as `r, g, b` triples.

use xeno_formats::Palette;

/// Bytes in a DAC palette: 256 `r, g, b` triples.
pub const DAC_BYTES: usize = 768;

/// The ramp the driver builds each fade step: entry `i` is `i * scale / 65536`,
/// rounded down, for the 64 DAC levels.
pub fn ramp(scale: u16) -> [u8; 64] {
    std::array::from_fn(|i| ((i as u32 * u32::from(scale)) >> 16) as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    In,
    Out,
}

/// A fade running once per retrace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fade {
    direction: Direction,
    /// The colours it scales, and the end of their bytes.
    saved: [u8; DAC_BYTES],
    end: usize,
    /// A 16-bit level that moves by `step` a retrace.
    level: u16,
    step: u16,
    /// Steps left before it stops anyway.
    left: u16,
}

impl Fade {
    /// `length` retraces: the level moves by `0x10000 / length` a step (`length` 0 is
    /// taken as 1). With `length` 1 the original divides into 0x10000 and faults; here
    /// the step is the largest there is instead.
    fn new(direction: Direction, saved: [u8; DAC_BYTES], last: u8, length: u16, max: u16) -> Fade {
        let step = (0x10000 / u32::from(length.max(1))).min(0xffff) as u16;
        Fade {
            direction,
            saved,
            end: (usize::from(last) + 1) * 3,
            level: 0,
            step,
            left: max,
        }
    }

    /// One step into `shadow`; returns whether the fade is over. A fade in saturates at
    /// level 0xffff, whose ramp is `i - 1`, so it ends one level short of its target
    /// (until something puts the target back, [`Dac::restore`]). A fade out ends at 0:
    /// black. Either also ends when `max` steps have run.
    fn step(&mut self, shadow: &mut [u8; DAC_BYTES]) -> bool {
        let (sum, carry) = self.level.overflowing_add(self.step);
        let (scale, done) = match self.direction {
            Direction::In => {
                self.level = if carry { 0xffff } else { sum };
                (self.level, self.level == 0xffff)
            }
            Direction::Out => {
                self.level = if carry { 0 } else { sum };
                (self.level.wrapping_neg(), self.level == 0)
            }
        };
        let ramp = ramp(scale);
        for (dst, &src) in shadow[..self.end].iter_mut().zip(&self.saved) {
            *dst = ramp[usize::from(src & 0x3f)];
        }
        if done {
            return true;
        }
        self.left = self.left.saturating_sub(1);
        self.left == 0
    }
}

/// The shadow palette, the DAC itself, and the running fade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dac {
    shadow: [u8; DAC_BYTES],
    hardware: [u8; DAC_BYTES],
    dirty: bool,
    fade: Option<Fade>,
}

impl Default for Dac {
    fn default() -> Self {
        Dac {
            shadow: [0; DAC_BYTES],
            hardware: [0; DAC_BYTES],
            dirty: false,
            fade: None,
        }
    }
}

impl Dac {
    /// The retrace handler: upload the shadow if it changed, then run a fade step.
    pub fn retrace(&mut self) {
        if self.dirty {
            self.hardware = self.shadow;
            self.dirty = false;
        }
        if let Some(fade) = &mut self.fade {
            if fade.step(&mut self.shadow) {
                self.fade = None;
            }
            self.dirty = true;
        }
    }

    /// What the screen shows now.
    pub fn hardware(&self) -> &[u8; DAC_BYTES] {
        &self.hardware
    }

    pub fn palette(&self) -> Palette {
        Palette::from_vga6(&self.hardware).expect("DAC levels are 6-bit")
    }

    /// Whether the shadow is waiting to be uploaded.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Slot `0x4b4`: the whole shadow to black, uploaded at the next retrace. A running
    /// fade is left alone.
    pub fn blank(&mut self) {
        self.shadow = [0; DAC_BYTES];
        self.dirty = true;
    }

    /// What every fade does first: stop the running one and drop a pending upload.
    pub fn stop(&mut self) {
        self.fade = None;
        self.dirty = false;
    }

    /// Slot `0x4e8`: if a fade is running, stop it and put its saved colours back
    /// (for a fade in, its target).
    pub fn restore(&mut self) {
        if let Some(fade) = self.fade.take() {
            self.shadow = fade.saved;
            self.dirty = true;
        }
    }

    /// Slot `0x4d8`'s first half: stop, set colours `0..=last` of the shadow to black
    /// and mark it for upload. Once it is uploaded, [`Dac::start`] the fade it returns,
    /// which brings them up to `target` over `length` retraces.
    pub fn prepare_fade_in(&mut self, target: &[u8; DAC_BYTES], last: u8, length: u16) -> Fade {
        self.stop();
        let end = (usize::from(last) + 1) * 3;
        self.shadow[..end].copy_from_slice(&target[..end]);
        let fade = Fade::new(Direction::In, self.shadow, last, length, u16::MAX);
        self.shadow[..end].fill(0);
        self.dirty = true;
        fade
    }

    /// Slots `0x4c8` / `0x4c0` after their wait for a retrace: fade colours `0..=last`
    /// from what the DAC shows now to black over `length` retraces, at most `max`.
    pub fn fade_out(&mut self, last: u8, length: u16, max: u16) {
        self.start(Fade::new(Direction::Out, self.hardware, last, length, max));
    }

    pub fn start(&mut self, fade: Fade) {
        self.fade = Some(fade);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(level: u8) -> [u8; DAC_BYTES] {
        [level; DAC_BYTES]
    }

    /// Runs retraces until the fade ends; returns each retrace's first DAC value.
    fn run(dac: &mut Dac) -> Vec<u8> {
        let mut seen = Vec::new();
        for _ in 0..1000 {
            dac.retrace();
            seen.push(dac.hardware()[0]);
            if !dac.fading() && !dac.dirty() {
                return seen;
            }
        }
        panic!("the fade never ends");
    }

    #[test]
    fn ramps_scale_by_a_sixteen_bit_level() {
        assert_eq!(ramp(0), [0; 64]);
        assert_eq!(ramp(0x8000)[63], 31);
        let full = ramp(0xffff);
        assert_eq!((full[0], full[1], full[63]), (0, 0, 62));
    }

    #[test]
    fn a_fade_in_shows_a_retrace_after_each_step_and_stops_one_short() {
        let mut dac = Dac::default();
        let fade = dac.prepare_fade_in(&grey(63), 255, 4);
        dac.retrace(); // black uploaded
        assert_eq!(dac.hardware(), &grey(0));
        dac.start(fade);
        // Levels 0x4000, 0x8000, 0xc000, then saturated at 0xffff: 63 * level >> 16.
        assert_eq!(run(&mut dac), [0, 15, 31, 47, 62]);
        assert_eq!(dac.hardware(), &grey(62));
        // Restoring after the fade ended does nothing; during one it puts the target back.
        dac.restore();
        assert!(!dac.dirty());
        let fade = dac.prepare_fade_in(&grey(40), 255, 64);
        dac.start(fade);
        dac.retrace();
        dac.restore();
        dac.retrace();
        assert_eq!((dac.hardware()[0], dac.fading()), (40, false));
    }

    #[test]
    fn a_fade_out_starts_from_the_dac_and_ends_black() {
        let mut dac = Dac::default();
        let fade = dac.prepare_fade_in(&grey(63), 255, 1);
        dac.retrace();
        dac.start(fade);
        run(&mut dac);
        dac.stop();
        dac.fade_out(251, 4, 4);
        // From 62: scales 0xc000, 0x8000, 0x4000, then 0.
        assert_eq!(run(&mut dac), [62, 46, 31, 15, 0]);
        // Colours 252..=255 are left alone.
        assert_eq!(dac.hardware()[252 * 3..], [62; 12]);
        assert_eq!(dac.palette()[252].r, 62 << 2 | 62 >> 4);
    }

    #[test]
    fn a_short_maximum_stops_a_fade_early() {
        let mut dac = Dac {
            shadow: grey(60),
            dirty: true,
            ..Dac::default()
        };
        dac.retrace();
        // 20 retraces step 3276 at a time, but 3 steps are allowed.
        dac.fade_out(255, 20, 3);
        let seen = run(&mut dac);
        assert_eq!(seen.len(), 4);
        assert_eq!(
            *seen.last().unwrap(),
            ((60 * (0x10000 - 3 * 3276)) >> 16) as u8
        );
    }

    #[test]
    fn stop_drops_a_pending_upload_and_blank_leaves_the_fade() {
        let mut dac = Dac::default();
        dac.blank();
        dac.stop();
        dac.retrace();
        assert!(!dac.dirty());
        dac.fade_out(255, 8, u16::MAX);
        dac.blank();
        assert!(dac.fading());
    }
}
