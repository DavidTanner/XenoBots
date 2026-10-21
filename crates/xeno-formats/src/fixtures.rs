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

/// A PCX 3.0 file, 8 bits per pixel, holding `pixels` (`width * height` indices, row
/// by row) and `palette` (768 bytes of 8-bit `r, g, b`). Scanlines are padded to an
/// even length, as the format asks, and each is run-length encoded on its own.
pub fn synthetic_pcx(width: usize, height: usize, pixels: &[u8], palette: &[u8]) -> Vec<u8> {
    use binrw::BinWrite;

    assert_eq!(pixels.len(), width * height, "pixel count");
    assert_eq!(palette.len(), crate::Palette::BYTES, "palette size");
    let stride = width.next_multiple_of(2);
    let header = crate::pcx::PcxHeader {
        version: 5,
        encoding: 1,
        bits_per_pixel: 8,
        x_min: 0,
        y_min: 0,
        x_max: (width - 1) as u16,
        y_max: (height - 1) as u16,
        h_dpi: 72,
        v_dpi: 72,
        ega_palette: [0; 48],
        reserved_40: 0,
        planes: 1,
        bytes_per_line: stride as u16,
        palette_info: 1,
        h_screen_size: 0,
        v_screen_size: 0,
        reserved_4a: [0; 54],
    };
    let mut out = std::io::Cursor::new(Vec::new());
    header.write(&mut out).unwrap();
    let mut out = out.into_inner();

    for row in pixels.chunks_exact(width) {
        let mut row = row.to_vec();
        row.resize(stride, 0);
        for run in row.chunk_by(|a, b| a == b) {
            for part in run.chunks(0x3f) {
                if part.len() > 1 || part[0] >= 0xc0 {
                    out.push(0xc0 | part.len() as u8);
                }
                out.push(part[0]);
            }
        }
    }
    out.push(crate::pcx::Pcx::PALETTE_MARKER);
    out.extend_from_slice(palette);
    out
}

/// `data` compressed the way `.b@n` and `.m@l` files are (see `crate::packed`), header
/// included. A greedy matcher: not the game's compressor, but the same stream format.
pub fn synthetic_packed(data: &[u8]) -> Vec<u8> {
    use crate::packed::{END, PackedHeader};

    const WINDOW: usize = 1024;
    const MAX_MATCH: usize = 32;
    // Length 1 can collide with the end marker; 3 is where a match starts to pay.
    const MIN_MATCH: usize = 3;

    let paragraphs = data.len().div_ceil(PackedHeader::PARAGRAPH);
    let mut out = u16::try_from(paragraphs)
        .expect("data fits in a u16 paragraph count")
        .to_le_bytes()
        .to_vec();
    let mut literals: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literals: &mut Vec<u8>| {
        for run in literals.chunks(0x80) {
            out.push(run.len() as u8 - 1);
            out.extend_from_slice(run);
        }
        literals.clear();
    };

    let mut pos = 0;
    while pos < data.len() {
        let (mut best_len, mut best_distance) = (0, 0);
        for distance in 1..=pos.min(WINDOW) {
            let len = (0..MAX_MATCH.min(data.len() - pos))
                .take_while(|&k| data[pos - distance + k] == data[pos + k])
                .count();
            if len > best_len {
                (best_len, best_distance) = (len, distance);
            }
        }
        if best_len < MIN_MATCH {
            literals.push(data[pos]);
            pos += 1;
            continue;
        }
        flush(&mut out, &mut literals);
        let d = best_distance - 1;
        out.push(0x80 | ((best_len - 1) as u8) << 2 | ((d >> 1) & 1) as u8 | ((d & 1) as u8) << 1);
        out.push((d >> 2) as u8);
        pos += best_len;
    }
    flush(&mut out, &mut literals);
    out.push(END);
    out
}

/// A packed `.b@n` sprite bank holding `sprites`, each `(width, height, pixels)` with
/// `width * height` pixels stored raw (no row spans).
pub fn synthetic_sprite_bank(sprites: &[(u8, u8, &[u8])]) -> Vec<u8> {
    let mut bank = vec![0; 4];
    for &(width, height, pixels) in sprites {
        assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
        let size = u16::try_from(crate::ban::SpriteHeader::SIZE + pixels.len()).unwrap();
        bank.extend(size.to_le_bytes());
        bank.extend([width, height, 0, 8]);
        bank.extend_from_slice(pixels);
    }
    bank.extend([0, 0]);
    synthetic_packed(&bank)
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
