//! Colour-animation records, as `mgavideo.dvr`'s retrace handler runs them
//! (`docs/formats/palette.md`, "Colour-animation records"). The list is kept as the
//! driver keeps it, raw bytes: records back to back from offset 0, each starting with
//! its size, ended by a size of 0; a record's colour pointer is an offset into the
//! list. So a list read from the user's own driver works unchanged, and odd records
//! behave as they would in the original, short of reading outside the list (which
//! reads 0 and writes nothing here).
//!
//! Each retrace an active record's countdown drops by one; at 0 its colours go
//! straight to the DAC (not the shadow palette), and then it moves on: a rotate slides
//! a window of `count` colours one step round a ring of colours, a frame record steps
//! to its next frame, and a blend record turns itself off.

/// Bytes before a record's colours.
pub const HEADER: usize = 10;

/// Flag bits (byte 2).
pub mod flag {
    /// The record runs.
    pub const ACTIVE: u8 = 0x80;
    /// Rotate: set steps the ring backward. Frames: set plays them first to last.
    pub const DIRECTION: u8 = 0x40;
    /// Frames: stop at the end of this pass even if looping.
    pub const STOP: u8 = 0x20;
    /// Frames: start again after the last.
    pub const LOOP: u8 = 0x10;
    /// Frames: when this record stops, start the next one.
    pub const CHAIN: u8 = 0x08;
    /// The kind: 1 rotate, 2 frames, 3 blend from the palette, 4 blend two frames.
    pub const KIND: u8 = 0x07;
}

/// A list of colour-animation records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColourAnims {
    bytes: Vec<u8>,
}

impl ColourAnims {
    /// The list as the driver holds it. The original's is one segment; this one is cut at
    /// 60 KB, so offsets near a record never pass 64 KB.
    pub fn new(mut bytes: Vec<u8>) -> Self {
        bytes.truncate(0xf000);
        ColourAnims { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn byte(&self, at: u16) -> u8 {
        self.bytes.get(usize::from(at)).copied().unwrap_or(0)
    }

    fn word(&self, at: u16) -> u16 {
        u16::from_le_bytes([self.byte(at), self.byte(at.wrapping_add(1))])
    }

    fn set_byte(&mut self, at: u16, value: u8) {
        if let Some(b) = self.bytes.get_mut(usize::from(at)) {
            *b = value;
        }
    }

    fn set_word(&mut self, at: u16, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.set_byte(at, lo);
        self.set_byte(at.wrapping_add(1), hi);
    }

    /// The records' offsets, in list order.
    fn offsets(&self) -> Vec<u16> {
        let mut offsets = Vec::new();
        let mut at = 0usize;
        while at + 1 < self.bytes.len() && at <= usize::from(u16::MAX) {
            let size = self.word(at as u16);
            if size == 0 {
                break;
            }
            offsets.push(at as u16);
            at += usize::from(size);
        }
        offsets
    }

    /// The offset of record `index` (slot `0x316`'s lookup, `1000:14e2`).
    fn find(&self, index: u8) -> Option<u16> {
        self.offsets().get(usize::from(index)).copied()
    }

    /// The handler's walk: count down each active record and write those that reach 0.
    pub fn fire(&mut self, dac: &mut [u8; crate::DAC_BYTES]) {
        for r in self.offsets() {
            let flags = self.byte(r + 2);
            if flags & flag::ACTIVE == 0 {
                continue;
            }
            let countdown = self.byte(r + 3).wrapping_sub(1);
            self.set_byte(r + 3, countdown);
            if countdown != 0 {
                continue;
            }
            let (count, pos, ptr) = (self.byte(r + 6), self.byte(r + 7), self.word(r + 8));
            let runs = match flags & flag::KIND {
                // A first part from the pointer, then `pos` colours from the start.
                1 => [(ptr, count.wrapping_sub(pos)), (r + HEADER as u16, pos)],
                2..=4 => [(ptr, count), (0, 0)],
                _ => continue,
            };
            let mut index = usize::from(self.byte(r + 5)) * 3;
            for (from, colours) in runs {
                for k in 0..u16::from(colours) * 3 {
                    dac[index % crate::DAC_BYTES] = self.byte(from.wrapping_add(k));
                    index += 1;
                }
            }
        }
    }

    /// `pal_advance_colour_anims` (0xa66): reload and move on every record that fired.
    pub fn advance(&mut self) {
        for r in self.offsets() {
            let flags = self.byte(r + 2);
            if flags & flag::ACTIVE == 0 || self.byte(r + 3) != 0 {
                continue;
            }
            self.set_byte(r + 3, self.byte(r + 4));
            let (end, data) = (r.wrapping_add(self.word(r)), r + HEADER as u16);
            let (count, ptr) = (self.byte(r + 6), self.word(r + 8));
            match flags & flag::KIND {
                1 => {
                    let tail =
                        ptr.wrapping_add(u16::from(count.wrapping_sub(self.byte(r + 7))) * 3);
                    if flags & flag::DIRECTION == 0 {
                        let ptr = ptr.wrapping_add(3);
                        self.set_word(r + 8, ptr);
                        if tail >= end {
                            self.set_byte(r + 7, self.byte(r + 7).wrapping_add(1));
                            if ptr >= end {
                                self.set_byte(r + 7, 0);
                                self.set_word(r + 8, data);
                            }
                        }
                    } else {
                        let ptr = ptr.wrapping_sub(3);
                        self.set_word(r + 8, ptr);
                        if ptr < data {
                            self.set_byte(r + 7, count.wrapping_sub(1));
                            self.set_word(r + 8, end.wrapping_sub(3));
                        } else if tail >= end {
                            self.set_byte(r + 7, self.byte(r + 7).saturating_sub(1));
                        }
                    }
                }
                2 => {
                    let frame = u16::from(count) * 3;
                    let wrapped = if flags & flag::DIRECTION == 0 {
                        let ptr = ptr.wrapping_sub(frame);
                        let wrapped = ptr < data;
                        self.set_word(
                            r + 8,
                            if wrapped {
                                end.wrapping_sub(frame)
                            } else {
                                ptr
                            },
                        );
                        wrapped
                    } else {
                        let ptr = ptr.wrapping_add(frame);
                        let wrapped = ptr >= end;
                        self.set_word(r + 8, if wrapped { data } else { ptr });
                        wrapped
                    };
                    let looping = flags & (flag::STOP | flag::LOOP) == flag::LOOP;
                    if wrapped && !looping {
                        let flags = flags & !(flag::ACTIVE | flag::STOP);
                        self.set_byte(r + 2, flags);
                        if flags & flag::CHAIN != 0 {
                            let next = end.wrapping_add(2);
                            self.set_byte(next, self.byte(next) | flag::ACTIVE);
                            self.set_byte(next.wrapping_add(1), self.byte(r + 4));
                        }
                    }
                }
                3 | 4 => self.set_byte(r + 2, flags & !flag::ACTIVE),
                _ => {}
            }
        }
    }

    /// Slot `0x316` (`pal_start_anim`, `1000:140b`): start record `index` from its
    /// beginning, shown at the next retrace. Blends are computed now, with `weight` out
    /// of 128 (128 or more is the target itself) against `palette`, the shadow palette.
    /// Returns false if there is no such record or its kind is unknown; kind 0 is
    /// accepted and left off.
    pub fn start(&mut self, index: u8, weight: u8, palette: &[u8; crate::DAC_BYTES]) -> bool {
        let Some(r) = self.find(index) else {
            return false;
        };
        let flags = self.byte(r + 2) & !flag::ACTIVE;
        self.set_byte(r + 2, flags);
        let (data, count) = (r + HEADER as u16, self.byte(r + 6));
        let frame = u16::from(count) * 3;
        match flags & flag::KIND {
            0 => return true,
            1 => {
                self.set_byte(r + 7, 0);
                self.set_word(r + 8, data);
            }
            2 if flags & flag::DIRECTION != 0 => self.set_word(r + 8, data),
            2 => self.set_word(r + 8, r.wrapping_add(self.word(r)).wrapping_sub(frame)),
            // `pal_blend_anim_from_palette` (slot 0x4ec): frame 1 from the palette to frame 0.
            3 if weight >= 0x80 => self.set_word(r + 8, data),
            3 => {
                let first = usize::from(self.byte(r + 5)) * 3;
                for k in 0..frame {
                    let base = palette.get(first + usize::from(k)).copied().unwrap_or(0);
                    let to = self.byte(data + k);
                    self.set_byte(data + frame + k, blend(base, to, weight << 1));
                }
                self.set_word(r + 8, data + frame);
            }
            // `pal_blend_anim_frames` (slot 0x4f0): frame 2 from frame 0 to frame 1.
            4 if weight >= 0x80 => self.set_word(r + 8, data + frame),
            4 if weight == 0 => self.set_word(r + 8, data),
            4 => {
                for k in 0..frame {
                    let (from, to) = (self.byte(data + k), self.byte(data + frame + k));
                    self.set_byte(data + 2 * frame + k, blend(from, to, weight << 1));
                }
                self.set_word(r + 8, data + 2 * frame);
            }
            _ => return false,
        }
        self.set_byte(r + 3, 1);
        self.set_byte(r + 2, flags | flag::ACTIVE);
        true
    }

    /// Slots `0x318` / `0x31a`: turn record `index` on or off where it stands.
    pub fn set_active(&mut self, index: u8, active: bool) {
        if let Some(r) = self.find(index) {
            let flags = self.byte(r + 2) & !flag::ACTIVE;
            self.set_byte(r + 2, flags | if active { flag::ACTIVE } else { 0 });
        }
    }

    /// Slot `0x31c`: record `index`'s flags.
    pub fn flags(&self, index: u8) -> Option<u8> {
        self.find(index).map(|r| self.byte(r + 2))
    }

    /// Slot `0x31e`: set record `index`'s direction bit.
    pub fn set_direction(&mut self, index: u8, set: bool) {
        if let Some(r) = self.find(index) {
            let flags = self.byte(r + 2) & !flag::DIRECTION;
            self.set_byte(r + 2, flags | if set { flag::DIRECTION } else { 0 });
        }
    }

    /// Slot `0x320`: retraces between record `index`'s steps (0 is 256).
    pub fn set_speed(&mut self, index: u8, retraces: u8) {
        if let Some(r) = self.find(index) {
            self.set_byte(r + 4, retraces);
        }
    }

    /// Slot `0x310`: turn every record off.
    pub fn stop_all(&mut self) {
        for r in self.offsets() {
            self.set_byte(r + 2, self.byte(r + 2) & !flag::ACTIVE);
        }
    }
}

/// The drivers' blend: `from` moved towards `to` by `k / 256`, rounding towards `from`.
fn blend(from: u8, to: u8, k: u8) -> u8 {
    let toward = |distance: u8| ((u16::from(distance) * u16::from(k)) >> 8) as u8;
    if to < from {
        from.wrapping_sub(toward(from - to))
    } else {
        from.wrapping_add(toward(to - from))
    }
}

/// Builds one record's bytes (for tests and for synthetic animations): header, then
/// `colours` (`r, g, b` triples), with the pointer at the colours of a record placed
/// at offset `at` in its list.
pub fn record(at: u16, flags: u8, speed: u8, first: u8, count: u8, colours: &[u8]) -> Vec<u8> {
    let size = (HEADER + colours.len()) as u16;
    let mut bytes = size.to_le_bytes().to_vec();
    bytes.extend([flags, 1, speed, first, count, 0]);
    bytes.extend((at + HEADER as u16).to_le_bytes());
    bytes.extend(colours);
    bytes
}

/// A list of `records`, each built by [`record`] at its place, and the end marker.
pub fn list(records: &[(u8, u8, u8, u8, &[u8])]) -> ColourAnims {
    let mut bytes = Vec::new();
    for &(flags, speed, first, count, colours) in records {
        let at = bytes.len() as u16;
        bytes.extend(record(at, flags, speed, first, count, colours));
    }
    bytes.extend([0, 0]);
    ColourAnims::new(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DAC_BYTES;
    use proptest::prelude::*;

    /// Colour `i` of a ring is grey level `i`.
    fn ring(n: u8) -> Vec<u8> {
        (0..n).flat_map(|i| [i; 3]).collect()
    }

    /// Runs `retraces` and returns the first level of colours `first..first + count`
    /// after each retrace that wrote them.
    fn run(anims: &mut ColourAnims, retraces: usize, first: usize, count: usize) -> Vec<Vec<u8>> {
        let mut dac = [0xff; DAC_BYTES];
        let mut seen = Vec::new();
        for _ in 0..retraces {
            let before = dac;
            anims.fire(&mut dac);
            anims.advance();
            if dac != before {
                seen.push((first..first + count).map(|c| dac[c * 3]).collect());
            }
            dac = [0xff; DAC_BYTES];
        }
        seen
    }

    #[test]
    fn a_rotate_slides_a_window_round_its_ring() {
        // A window of 3 over a ring of 5, one step every 2 retraces.
        let mut anims = list(&[(1, 2, 10, 3, &ring(5))]);
        assert!(anims.start(0, 0, &[0; DAC_BYTES]));
        let seen = run(&mut anims, 11, 10, 3);
        let expected: Vec<Vec<u8>> = [
            [0, 1, 2],
            [1, 2, 3],
            [2, 3, 4],
            [3, 4, 0],
            [4, 0, 1],
            [0, 1, 2],
        ]
        .iter()
        .map(|w| w.to_vec())
        .collect();
        assert_eq!(seen, expected);
        // Backward: the window's start moves down and wraps to the ring's end.
        anims.set_direction(0, true);
        assert!(anims.start(0, 0, &[0; DAC_BYTES]));
        let seen = run(&mut anims, 7, 10, 3);
        assert_eq!(
            seen,
            [vec![0, 1, 2], vec![4, 0, 1], vec![3, 4, 0], vec![2, 3, 4]]
        );
    }

    #[test]
    fn a_full_ring_rotation_wraps_past_colour_255() {
        let mut anims = list(&[(1, 1, 254, 4, &ring(4))]);
        anims.start(0, 0, &[0; DAC_BYTES]);
        let mut dac = [9; DAC_BYTES];
        anims.fire(&mut dac);
        anims.advance();
        anims.fire(&mut dac);
        // Second step: ring 1, 2, 3, 0 at colours 254, 255, 0, 1.
        assert_eq!([dac[254 * 3], dac[255 * 3], dac[0], dac[3]], [1, 2, 3, 0]);
        assert_eq!(dac[2 * 3], 9);
    }

    #[test]
    fn frames_play_backward_by_default_and_loop_or_stop() {
        // Three frames of one colour, played last to first.
        let frames = ring(3);
        let mut anims = list(&[(2 | flag::LOOP, 1, 7, 1, &frames)]);
        anims.start(0, 0, &[0; DAC_BYTES]);
        assert_eq!(run(&mut anims, 5, 7, 1).concat(), [2, 1, 0, 2, 1]);
        // Forward and without the loop bit: one pass, then off.
        let mut anims = list(&[(2 | flag::DIRECTION, 1, 7, 1, &frames)]);
        anims.start(0, 0, &[0; DAC_BYTES]);
        assert_eq!(run(&mut anims, 6, 7, 1).concat(), [0, 1, 2]);
        assert_eq!(anims.flags(0), Some(2 | flag::DIRECTION));
        // The stop bit ends a loop at the end of its pass and clears itself.
        let mut anims = list(&[(
            2 | flag::LOOP | flag::STOP | flag::DIRECTION,
            1,
            7,
            1,
            &frames,
        )]);
        anims.start(0, 0, &[0; DAC_BYTES]);
        assert_eq!(run(&mut anims, 6, 7, 1).concat(), [0, 1, 2]);
        assert_eq!(anims.flags(0), Some(2 | flag::LOOP | flag::DIRECTION));
    }

    #[test]
    fn a_chained_record_starts_the_next_one_at_its_speed() {
        let mut anims = list(&[
            (2 | flag::DIRECTION | flag::CHAIN, 2, 0, 1, &[5, 5, 5]),
            (2 | flag::DIRECTION, 9, 1, 1, &[6, 6, 6]),
        ]);
        anims.start(0, 0, &[0; DAC_BYTES]);
        let mut dac = [0; DAC_BYTES];
        let mut when = Vec::new();
        for retrace in 0..8 {
            let before = dac;
            anims.fire(&mut dac);
            anims.advance();
            for colour in [0, 1] {
                if dac[colour * 3] != before[colour * 3] {
                    when.push((retrace, colour));
                }
            }
        }
        // Record 0 shows at once and stops; record 1 follows its speed of 2.
        assert_eq!(when, [(0, 0), (2, 1)]);
        assert_eq!(anims.flags(1), Some(2 | flag::DIRECTION));
    }

    #[test]
    fn blends_mix_the_palette_or_two_frames_once() {
        let palette = [10; DAC_BYTES];
        // Kind 3: halfway from the palette's 10 to 30 and to 0.
        let mut anims = list(&[(3, 5, 4, 2, &[30, 30, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0])]);
        anims.start(0, 0x40, &palette);
        let mut dac = [0xff; DAC_BYTES];
        anims.fire(&mut dac);
        anims.advance();
        assert_eq!(dac[4 * 3..6 * 3], [20, 20, 20, 5, 5, 5]);
        assert_eq!(anims.flags(0), Some(3));
        // Weight 0 is the palette, 128 the frame itself.
        anims.start(0, 0, &palette);
        anims.fire(&mut dac);
        assert_eq!(dac[4 * 3], 10);
        anims.start(0, 0x80, &palette);
        anims.fire(&mut dac);
        assert_eq!(dac[4 * 3], 30);
        // Kind 4: a quarter of the way from frame 0 to frame 1, rounding towards frame 0.
        let mut anims = list(&[(4, 5, 0, 1, &[0, 63, 9, 63, 0, 9, 0, 0, 0])]);
        anims.start(0, 0x20, &palette);
        anims.fire(&mut dac);
        assert_eq!(dac[..3], [15, 48, 9]);
        anims.start(0, 0, &palette);
        anims.fire(&mut dac);
        assert_eq!(dac[..3], [0, 63, 9]);
        anims.start(0, 0xff, &palette);
        anims.fire(&mut dac);
        assert_eq!(dac[..3], [63, 0, 9]);
    }

    #[test]
    fn records_are_found_by_index_and_can_be_switched() {
        let mut anims = list(&[
            (0, 1, 0, 0, &[]),
            (2, 3, 0, 1, &[1, 1, 1]),
            (5, 1, 0, 0, &[]),
        ]);
        assert!(anims.start(0, 0, &[0; DAC_BYTES]));
        assert_eq!(anims.flags(0), Some(0));
        assert!(!anims.start(2, 0, &[0; DAC_BYTES]));
        assert!(!anims.start(3, 0, &[0; DAC_BYTES]));
        anims.set_active(1, true);
        anims.set_speed(1, 7);
        assert_eq!(anims.flags(1), Some(2 | flag::ACTIVE));
        anims.stop_all();
        assert_eq!(anims.flags(1), Some(2));
        assert_eq!(anims.bytes()[0x0a + 4], 7);
        assert_eq!(anims.flags(3), None);
        // Inactive records are not counted down.
        let mut dac = [0; DAC_BYTES];
        anims.fire(&mut dac);
        assert_eq!(anims.bytes()[0x0a + 3], 1);
    }

    proptest! {
        /// However a record is set up, its steps write only its own colours.
        #[test]
        fn steps_write_only_their_colours(
            kind in 1u8..=4,
            direction in any::<bool>(),
            speed in 1u8..4,
            first in any::<u8>(),
            count in 1u8..=40,
            frames in 1usize..4,
            weight in any::<u8>(),
            retraces in 1usize..200,
        ) {
            let colours = vec![7; usize::from(count) * 3 * frames.max(3)];
            let flags = kind | flag::LOOP | if direction { flag::DIRECTION } else { 0 };
            let mut anims = list(&[(flags, speed, first, count, &colours)]);
            anims.start(0, weight, &[1; DAC_BYTES]);
            let mut dac = [0; DAC_BYTES];
            for _ in 0..retraces {
                anims.fire(&mut dac);
                anims.advance();
            }
            for colour in 0..256usize {
                let inside = (colour + 256 - usize::from(first)) % 256 < usize::from(count);
                if !inside {
                    prop_assert_eq!(&dac[colour * 3..colour * 3 + 3], &[0, 0, 0]);
                }
            }
        }
    }
}
