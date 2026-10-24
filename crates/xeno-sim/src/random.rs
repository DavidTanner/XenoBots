//! The original's random word (`docs/screens.md`, "Campaign rules"), the sim's only
//! source of chance. The original seeds it from the timer chip, so it never repeats;
//! the port seeds it from the run's seed, so a run replays.

use crate::hash::{StateHash, StateHasher};

/// The host's random word (`sim_random_word`, `1000:0283`): four 16-bit shift
/// registers used in turn; each step shifts one right and, if a 1 fell out, XORs its
/// high byte with 0xb4. A call returns the new word less one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Random {
    words: [u16; 4],
    next: usize,
}

impl Random {
    /// Four odd words from `seed` (the original makes its timer reads odd too), each
    /// the high half of the next step of a 32-bit linear congruential generator. The
    /// mapping is the port's; the words' sequence after it is the original's.
    pub fn seeded(seed: u32) -> Random {
        let mut state = seed;
        let words = std::array::from_fn(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 16) as u16 | 1
        });
        Random { words, next: 0 }
    }

    /// The next word, 0 to 0xfffe.
    pub fn word(&mut self) -> u16 {
        let at = self.next;
        self.next = (at + 1) % self.words.len();
        let word = &mut self.words[at];
        let carry = *word & 1 != 0;
        *word >>= 1;
        if carry {
            *word ^= 0xb400;
        }
        word.wrapping_sub(1)
    }

    /// Whether the next word is below `chance` (`sim_random_below`).
    pub fn below(&mut self, chance: u16) -> bool {
        self.word() < chance
    }
}

impl StateHash for Random {
    fn state_hash(&self, h: &mut StateHasher) {
        self.words.iter().for_each(|&w| h.u16(w));
        h.u8(self.next as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_word_is_the_hosts_shift_register() {
        let mut random = Random {
            words: [1, 2, 3, 4],
            next: 0,
        };
        // 1 shifts out a 1: 0 ^ 0xb400; 2 shifts to 1; 3 to 1 ^ 0xb400; 4 to 2.
        let words: Vec<_> = (0..5).map(|_| random.word()).collect();
        assert_eq!(words, [0xb3ff, 0, 0xb400, 1, 0x59ff]);
        assert_eq!(Random::seeded(7), Random::seeded(7));
    }

    #[test]
    fn a_known_seed_gives_known_words() {
        let mut random = Random::seeded(1);
        assert_eq!(random.words, [0x3c89, 0x5e89, 0x8117, 0xb473]);
        // 0x3c89 shifts out a 1: 0x1e44 ^ 0xb400 = 0xaa44, less one.
        let words: Vec<_> = (0..8).map(|_| random.word()).collect();
        assert_eq!(
            words,
            [
                0xaa43, 0x9b43, 0xf48a, 0xee38, 0x5521, 0x4da1, 0xce44, 0xc31b
            ]
        );
        // A word is at most 0xfffe.
        assert!(random.below(0xffff) && !random.below(0));
    }
}
