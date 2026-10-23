//! The VGA DAC as `mgavideo.dvr` drives it (`docs/formats/palette.md`): code writes a
//! shadow palette and marks it dirty; once per retrace the shadow is uploaded if dirty,
//! then the active colour-animation records write their colours straight to the DAC,
//! and then a running fade computes the next shadow. So a change shows from the next
//! retrace, and each fade step from the one after it.
//!
//! Fades take a first and a last colour, with the driver's arithmetic: a fade from
//! colour 0 does what it says; from a later colour it scales a shifted range (see
//! [`Dac::fade_out`]). Values are 6-bit DAC levels, 0..=63, as `r, g, b` triples.

use xeno_formats::Palette;

use crate::anim::ColourAnims;

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
    /// The bytes it scales from the saved buffer into the shadow.
    start: usize,
    end: usize,
    /// A 16-bit level that moves by `step` a retrace.
    level: u16,
    step: u16,
    /// Steps left before it stops anyway.
    left: u16,
}

impl Fade {
    /// Colours `first..=last` over `length` retraces: the level moves by
    /// `0x10000 / length` a step (`length` 0 is taken as 1). With `length` 1 the
    /// original divides into 0x10000 and faults; here the step is the largest there is.
    ///
    /// The driver loops from byte `first * 3` up to `count * 3`, `count` being the
    /// number of colours, so from a first colour above 0 it scales colours
    /// `first..=last - first`, and none if that is empty (the original then runs its
    /// loop 65,536 times over whatever memory follows).
    fn new(direction: Direction, first: u8, last: u8, length: u16, max: u16) -> Fade {
        let step = (0x10000 / u32::from(length.max(1))).min(0xffff) as u16;
        let count = usize::from(last.saturating_sub(first)) + 1;
        let start = usize::from(first) * 3;
        Fade {
            direction,
            start,
            end: (count * 3).max(start).min(DAC_BYTES),
            level: 0,
            step,
            left: max,
        }
    }

    /// One step from `saved` into `shadow`; returns whether the fade is over. A fade in
    /// saturates at level 0xffff, whose ramp is `i - 1`, so it ends one level short of
    /// its target (until something puts the target back, [`Dac::restore`]). A fade out
    /// ends at 0: black. Either also ends when `max` steps have run.
    fn step(&mut self, saved: &[u8; DAC_BYTES], shadow: &mut [u8; DAC_BYTES]) -> bool {
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
        let range = self.start..self.end;
        for (dst, &src) in shadow[range.clone()].iter_mut().zip(&saved[range]) {
            *dst = ramp[usize::from(src & 0x3f)];
        }
        if done {
            return true;
        }
        self.left = self.left.saturating_sub(1);
        self.left == 0
    }
}

/// The shadow palette, the DAC itself, the fade's saved colours, the running fade and
/// the colour-animation records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dac {
    shadow: [u8; DAC_BYTES],
    hardware: [u8; DAC_BYTES],
    dirty: bool,
    /// The driver's second buffer (`0x696`): a fade in's target, a fade out's readback.
    saved: [u8; DAC_BYTES],
    fade: Option<Fade>,
    anims: ColourAnims,
    /// Slots `0x312` / `0x314`: while non-zero no record runs.
    hold: u8,
}

impl Default for Dac {
    fn default() -> Self {
        Dac::with_palette(&[0; DAC_BYTES])
    }
}

impl Dac {
    /// A DAC already showing `palette`, with no animation records.
    pub fn with_palette(palette: &[u8; DAC_BYTES]) -> Dac {
        Dac {
            shadow: *palette,
            hardware: *palette,
            dirty: false,
            saved: [0; DAC_BYTES],
            fade: None,
            anims: ColourAnims::default(),
            hold: 0,
        }
    }

    /// The retrace handler (`pal_on_retrace`): upload the shadow if it changed, run the
    /// colour-animation records unless held, then run a fade step.
    pub fn retrace(&mut self) {
        if self.dirty {
            self.hardware = self.shadow;
            self.dirty = false;
        }
        if self.hold == 0 {
            self.anims.fire(&mut self.hardware);
            self.anims.advance();
        }
        if let Some(fade) = &mut self.fade {
            if fade.step(&self.saved, &mut self.shadow) {
                self.fade = None;
            }
            self.dirty = true;
        }
    }

    /// What the screen shows now.
    pub fn hardware(&self) -> &[u8; DAC_BYTES] {
        &self.hardware
    }

    pub fn shadow(&self) -> &[u8; DAC_BYTES] {
        &self.shadow
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

    /// Slot `0x4ac`: a new shadow palette, uploaded at the next retrace.
    pub fn set(&mut self, palette: &[u8; DAC_BYTES]) {
        self.shadow = *palette;
        self.dirty = true;
    }

    /// Slot `0x4a8`: a new shadow palette, not marked for upload: it shows only once
    /// something marks it ([`Dac::mark`]) or uploads it with a fade.
    pub fn copy(&mut self, palette: &[u8; DAC_BYTES]) {
        self.shadow = *palette;
    }

    /// Slot `0x4b0`: upload the shadow at the next retrace.
    pub fn mark(&mut self) {
        self.dirty = true;
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

    /// Slot `0x4e8`: if a fade is running, stop it and copy the whole saved buffer into
    /// the shadow: for a fade in, its target, but also whatever an earlier fade left
    /// outside its range.
    pub fn restore(&mut self) {
        if self.fade.take().is_some() {
            self.shadow = self.saved;
            self.dirty = true;
        }
    }

    /// Slot `0x4e0`'s first half: stop, put colours `first..=last` of `target` in the
    /// shadow and the saved buffer, set them to black and mark the shadow for upload.
    /// Once it is uploaded, [`Dac::start`] the fade it returns, which brings them up
    /// over `length` retraces.
    pub fn prepare_fade_in(
        &mut self,
        target: &[u8; DAC_BYTES],
        first: u8,
        last: u8,
        length: u16,
    ) -> Fade {
        self.stop();
        let range = usize::from(first) * 3..(usize::from(last) + 1) * 3;
        let range = range.start..range.end.max(range.start);
        self.shadow[range.clone()].copy_from_slice(&target[range.clone()]);
        self.saved[range.clone()].copy_from_slice(&self.shadow[range.clone()]);
        self.shadow[range].fill(0);
        self.dirty = true;
        Fade::new(Direction::In, first, last, length, u16::MAX)
    }

    /// Slots `0x4cc` / `0x4c4` after their wait for a retrace: read colours
    /// `first..=last` back from the DAC into the start of the saved buffer, then fade
    /// to black over `length` retraces, at most `max`. The step reads that buffer from
    /// byte `first * 3`, so from a first colour above 0 colour `c` is faded from what
    /// colour `c + first` showed.
    pub fn fade_out(&mut self, first: u8, last: u8, length: u16, max: u16) {
        let from = usize::from(first) * 3..(usize::from(last) + 1) * 3;
        let count = from.len().min(DAC_BYTES);
        self.saved[..count].copy_from_slice(&self.hardware[from.start..from.start + count]);
        self.start(Fade::new(Direction::Out, first, last, length, max));
    }

    pub fn start(&mut self, fade: Fade) {
        self.fade = Some(fade);
    }

    /// Slot `0x30e`: the colour-animation records to run from now on.
    pub fn set_anims(&mut self, anims: ColourAnims) {
        self.anims = anims;
    }

    pub fn anims(&self) -> &ColourAnims {
        &self.anims
    }

    /// The records, to start or stop them ([`ColourAnims::start`] takes
    /// [`Dac::shadow`] as its palette).
    pub fn anims_mut(&mut self) -> &mut ColourAnims {
        &mut self.anims
    }

    /// Slot `0x316` against the shadow palette, as the driver does.
    pub fn start_anim(&mut self, index: u8, weight: u8) -> bool {
        self.anims.start(index, weight, &self.shadow)
    }

    /// Slot `0x312`: freeze the records (nested; the game holds them over its
    /// cutscenes and its fading screen loader).
    pub fn hold_anims(&mut self) {
        self.hold = self.hold.saturating_add(1);
    }

    /// Slot `0x314`: undo one [`Dac::hold_anims`].
    pub fn release_anims(&mut self) {
        self.hold = self.hold.saturating_sub(1);
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
    fn a_copied_palette_shows_only_once_marked() {
        let mut dac = Dac::default();
        dac.copy(&grey(20));
        dac.retrace();
        assert_eq!((dac.hardware()[0], dac.shadow()[0]), (0, 20));
        dac.mark();
        dac.retrace();
        assert_eq!(dac.hardware(), &grey(20));
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
        let fade = dac.prepare_fade_in(&grey(63), 0, 255, 4);
        dac.retrace(); // black uploaded
        assert_eq!(dac.hardware(), &grey(0));
        dac.start(fade);
        // Levels 0x4000, 0x8000, 0xc000, then saturated at 0xffff: 63 * level >> 16.
        assert_eq!(run(&mut dac), [0, 15, 31, 47, 62]);
        assert_eq!(dac.hardware(), &grey(62));
        // Restoring after the fade ended does nothing; during one it puts the target back.
        dac.restore();
        assert!(!dac.dirty());
        let fade = dac.prepare_fade_in(&grey(40), 0, 255, 64);
        dac.start(fade);
        dac.retrace();
        dac.restore();
        dac.retrace();
        assert_eq!((dac.hardware()[0], dac.fading()), (40, false));
    }

    #[test]
    fn a_fade_out_starts_from_the_dac_and_ends_black() {
        let mut dac = Dac::default();
        let fade = dac.prepare_fade_in(&grey(63), 0, 255, 1);
        dac.retrace();
        dac.start(fade);
        run(&mut dac);
        dac.stop();
        dac.fade_out(0, 251, 4, 4);
        // From 62: scales 0xc000, 0x8000, 0x4000, then 0.
        assert_eq!(run(&mut dac), [62, 46, 31, 15, 0]);
        // Colours 252..=255 are left alone.
        assert_eq!(dac.hardware()[252 * 3..], [62; 12]);
        assert_eq!(dac.palette()[252].r, 62 << 2 | 62 >> 4);
    }

    #[test]
    fn a_short_maximum_stops_a_fade_early() {
        let mut dac = Dac::default();
        dac.set(&grey(60));
        dac.retrace();
        // 20 retraces step 3276 at a time, but 3 steps are allowed.
        dac.fade_out(0, 255, 20, 3);
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
        dac.fade_out(0, 255, 8, u16::MAX);
        dac.blank();
        assert!(dac.fading());
    }

    /// Colour `i` is grey level `i % 64`.
    fn ramp_palette() -> [u8; DAC_BYTES] {
        std::array::from_fn(|b| (b / 3 % 64) as u8)
    }

    #[test]
    fn fades_from_a_later_colour_scale_a_shifted_range() {
        let mut dac = Dac::with_palette(&ramp_palette());
        // Colours 4..=11 are 8 colours, so bytes 12..24: colours 4..=7, each faded from
        // the colour four above it. Half way after one of two steps.
        dac.fade_out(4, 11, 2, u16::MAX);
        dac.retrace();
        dac.retrace();
        let shown: Vec<u8> = (3..=9).map(|c| dac.hardware()[c * 3]).collect();
        assert_eq!(shown, [3, 4, 4, 5, 5, 8, 9]);
        // From colour 8 the range is empty.
        let mut dac = Dac::with_palette(&ramp_palette());
        dac.fade_out(8, 11, 2, u16::MAX);
        dac.retrace();
        dac.retrace();
        assert_eq!(dac.hardware(), &ramp_palette());
        // A fade in blackens all of 4..=11 but brings back only 4..=7.
        let mut dac = Dac::with_palette(&ramp_palette());
        let fade = dac.prepare_fade_in(&[40; DAC_BYTES], 4, 11, 1);
        dac.retrace();
        dac.start(fade);
        dac.retrace();
        dac.retrace();
        let shown: Vec<u8> = (3..=12).map(|c| dac.hardware()[c * 3]).collect();
        assert_eq!(shown, [3, 39, 39, 39, 39, 0, 0, 0, 0, 12]);
    }

    #[test]
    fn restore_copies_the_whole_saved_buffer() {
        let mut dac = Dac::with_palette(&grey(20));
        dac.fade_out(0, 255, 8, u16::MAX);
        dac.retrace();
        let fade = dac.prepare_fade_in(&grey(50), 0, 127, 64);
        dac.start(fade);
        dac.retrace();
        dac.restore();
        dac.retrace();
        assert_eq!(dac.hardware()[127 * 3..129 * 3], [50, 50, 50, 20, 20, 20]);
    }

    #[test]
    fn records_write_over_each_upload_only_when_they_fire() {
        use crate::anim::{flag, list};
        let mut dac = Dac::with_palette(&grey(40));
        dac.set_anims(list(&[(2 | flag::LOOP, 2, 0, 1, &[63; 3])]));
        assert!(dac.start_anim(0, 0));
        dac.fade_out(0, 255, 4, u16::MAX);
        let mut seen = Vec::new();
        for _ in 0..6 {
            dac.retrace();
            seen.push((dac.hardware()[0], dac.hardware()[3]));
        }
        // Colour 0 fires on retraces 1, 3 and 5 and is faded only in between; once
        // the fade has stopped uploading, the DAC keeps the record's colour.
        assert_eq!(
            seen,
            [(63, 40), (30, 30), (63, 20), (10, 10), (63, 0), (63, 0)]
        );
        // Held records neither count down nor write.
        dac.hold_anims();
        dac.hold_anims();
        dac.release_anims();
        dac.set(&grey(1));
        for _ in 0..4 {
            dac.retrace();
        }
        assert_eq!(dac.hardware()[0], 1);
        dac.release_anims();
        dac.retrace();
        assert_eq!(dac.hardware()[0], 63);
    }

    proptest::proptest! {
        /// A fade never changes a colour outside `first..=last`.
        #[test]
        fn fades_stay_in_their_range(
            first in 0u8..=255,
            last in 0u8..=255,
            length in 0u16..80,
            fade_in in proptest::bool::ANY,
            retraces in 1usize..100,
        ) {
            let mut dac = Dac::with_palette(&ramp_palette());
            if fade_in {
                let fade = dac.prepare_fade_in(&[63; DAC_BYTES], first, last, length);
                dac.retrace();
                dac.start(fade);
            } else {
                dac.fade_out(first, last, length, u16::MAX);
            }
            for _ in 0..retraces {
                dac.retrace();
            }
            for colour in 0..=255u8 {
                if !(first..=last).contains(&colour) {
                    let at = usize::from(colour) * 3;
                    proptest::prop_assert_eq!(
                        &dac.hardware()[at..at + 3],
                        &ramp_palette()[at..at + 3]
                    );
                }
            }
        }
    }
}
