//! Synthetic test data. Tests build their inputs with these helpers instead of reading
//! game files, which may never be committed (see `AGENTS.md`).
//!
//! Available to this crate's tests, and to other crates through the `test-fixtures`
//! feature:
//!
//! ```toml
//! [dev-dependencies]
//! xeno-formats = { path = "../xeno-formats", features = ["test-fixtures"] }
//! ```

/// `len` pseudo-random bytes, the same for the same `seed` on every platform and run.
///
/// Use it to fill the parts of a synthetic file whose contents do not matter to the
/// test, such as pixel data, so tests stay reproducible without hand-written arrays.
pub fn synthetic_bytes(len: usize, seed: u64) -> Vec<u8> {
    // SplitMix64: tiny, fast and well distributed; quality beyond that is not needed.
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let mut bytes = Vec::with_capacity(len.next_multiple_of(8));
    while bytes.len() < len {
        bytes.extend_from_slice(&next().to_le_bytes());
    }
    bytes.truncate(len);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_the_requested_length() {
        for len in [0, 1, 7, 8, 9, 1000] {
            assert_eq!(synthetic_bytes(len, 1).len(), len);
        }
    }

    #[test]
    fn same_seed_same_bytes_and_prefix_stable() {
        assert_eq!(synthetic_bytes(100, 42), synthetic_bytes(100, 42));
        assert_eq!(synthetic_bytes(100, 42)[..10], synthetic_bytes(10, 42)[..]);
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(synthetic_bytes(32, 1), synthetic_bytes(32, 2));
    }

    #[test]
    fn output_is_pinned() {
        // Guards against accidental changes to the generator, which would silently
        // change every test that uses it.
        assert_eq!(
            synthetic_bytes(8, 0),
            0xe220_a839_7b1d_cdafu64.to_le_bytes()
        );
    }
}
