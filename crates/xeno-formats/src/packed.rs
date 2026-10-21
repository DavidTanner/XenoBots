//! The compression behind the `@` in `.b@n` and `.m@l`: the game names its files
//! `.BIN` and `.MRL`, and the packed copies on disk swap the middle letter of the
//! extension for `@`. A small LZ77 variant with a 1 KB window; `xeno.exe` decodes it
//! while reading the file. See `docs/formats/packed.md`.

use binrw::{BinRead, binrw};

use crate::FormatError;

/// The two bytes in front of the packed stream.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedHeader {
    /// Decoded size in 16-byte paragraphs, rounded up: what the loader allocates.
    pub paragraphs: u16,
}

impl PackedHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 2;
    /// Bytes per paragraph, the unit of [`PackedHeader::paragraphs`].
    pub const PARAGRAPH: usize = 16;
}

/// Control byte that ends the stream: a back-reference with no length bits.
pub const END: u8 = 0x80;

/// Decodes a packed file. After the header comes a stream of packets, each starting
/// with a control byte `c`:
///
/// - `c < 0x80`: the next `c + 1` bytes are literals.
/// - `c == 0x80`: end of stream.
/// - otherwise one more byte `b` follows, and `((c & 0x7f) >> 2) + 1` bytes are copied
///   from `d` bytes back in the output, where `d - 1` is `b << 2 | (c & 1) << 1 |
///   (c >> 1) & 1` (1 to 1024). The copy goes byte by byte, so it may overlap itself.
///
/// The end marker must be the last byte, and the output must round up to the
/// paragraph count in the header.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, FormatError> {
    let header = PackedHeader::read(&mut std::io::Cursor::new(bytes))?;
    let capacity = usize::from(header.paragraphs) * PackedHeader::PARAGRAPH;
    let invalid = |offset: usize, reason: String| FormatError::Invalid {
        offset: offset as u64,
        reason,
    };

    let mut out = Vec::with_capacity(capacity);
    let mut i = PackedHeader::SIZE;
    loop {
        let at = i;
        let &control = bytes
            .get(i)
            .ok_or_else(|| invalid(i, "stream ends without its end marker".into()))?;
        i += 1;
        if control < 0x80 {
            let len = usize::from(control) + 1;
            let literals = bytes
                .get(i..i + len)
                .ok_or_else(|| invalid(at, format!("literal run of {len} ends early")))?;
            out.extend_from_slice(literals);
            i += len;
        } else if control == END {
            break;
        } else {
            let &low = bytes
                .get(i)
                .ok_or_else(|| invalid(at, "back-reference ends early".into()))?;
            i += 1;
            let len = usize::from((control & 0x7f) >> 2) + 1;
            let distance = (usize::from(low) << 2
                | usize::from(control & 1) << 1
                | usize::from((control >> 1) & 1))
                + 1;
            let start = out.len().checked_sub(distance).ok_or_else(|| {
                invalid(
                    at,
                    format!("back-reference {distance} bytes before the start of the output"),
                )
            })?;
            for k in start..start + len {
                out.push(out[k]);
            }
        }
        if out.len() > capacity {
            return Err(invalid(
                at,
                format!(
                    "decodes past the {} paragraphs in the header",
                    header.paragraphs
                ),
            ));
        }
    }

    if i != bytes.len() {
        return Err(invalid(
            i,
            format!("{} bytes after the end marker", bytes.len() - i),
        ));
    }
    if out.len().div_ceil(PackedHeader::PARAGRAPH) != capacity / PackedHeader::PARAGRAPH {
        return Err(invalid(
            0,
            format!(
                "header says {} paragraphs, stream decodes to {} bytes",
                header.paragraphs,
                out.len()
            ),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_bytes, synthetic_packed};

    #[test]
    fn synthetic_data_round_trips() {
        let mut repetitive = vec![7u8; 3000];
        repetitive.extend(synthetic_bytes(500, 2));
        repetitive.extend_from_within(100..1600);
        for data in [
            vec![],
            vec![1],
            synthetic_bytes(17, 1),
            synthetic_bytes(5000, 3),
            repetitive,
        ] {
            let packed = synthetic_packed(&data);
            assert_eq!(decompress(&packed).unwrap(), data);
        }
    }

    #[test]
    fn hand_written_stream_decodes() {
        // Two literals, then 5 bytes from 2 back (overlapping), then 3 from 7 back.
        // Bit 0 of `d - 1` lives in bit 1 of the control byte and bit 1 in bit 0.
        let bytes = [
            1,
            0, // 1 paragraph
            0x01,
            b'a',
            b'b', // literals
            0x80 | 4 << 2 | 0b10,
            0, // len 5, d - 1 = 0b001
            0x80 | 2 << 2 | 0b01,
            1, // len 3, d - 1 = 0b110
            END,
        ];
        assert_eq!(decompress(&bytes).unwrap(), b"abababaaba");
    }

    #[test]
    fn header_snapshot() {
        let packed = synthetic_packed(&synthetic_bytes(40, 5));
        let header = PackedHeader::read(&mut std::io::Cursor::new(&packed)).unwrap();
        insta::assert_debug_snapshot!(header);
    }

    #[test]
    fn broken_streams_are_rejected_with_their_offset() {
        let cases: [(&[u8], u64, &str); 6] = [
            (&[1, 0, 0x02, 1, 2], 2, "literal run of 3 ends early"),
            (&[1, 0, 0x00, 1], 4, "without its end marker"),
            (&[1, 0, 0x00, 1, 0x84], 4, "back-reference ends early"),
            (&[1, 0, 0x00, 1, 0x84, 1, END], 4, "before the start"),
            (&[1, 0, 0x00, 1, END, 0], 5, "1 bytes after the end marker"),
            (&[2, 0, 0x00, 1, END], 0, "header says 2 paragraphs"),
        ];
        for (bytes, offset, reason) in cases {
            match decompress(bytes) {
                Err(FormatError::Invalid {
                    offset: at,
                    reason: r,
                }) => {
                    assert_eq!(at, offset, "{r}");
                    assert!(r.contains(reason), "{r}");
                }
                other => panic!("expected Invalid for {bytes:?}, got {other:?}"),
            }
        }
        assert!(matches!(decompress(&[1]), Err(FormatError::Binrw(_))));
    }

    #[test]
    fn output_may_not_outgrow_the_header() {
        let mut packed = synthetic_packed(&[9; 40]);
        packed[0] = 1;
        match decompress(&packed) {
            Err(FormatError::Invalid { reason, .. }) => {
                assert!(reason.contains("past the 1 paragraphs"), "{reason}")
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_packed_file_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let packed: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("b@n") || e.eq_ignore_ascii_case("m@l"))
            })
            .collect();
        assert_eq!(packed.len(), 22, "a stock install has 14 .b@n and 8 .m@l");
        for path in packed {
            let bytes = std::fs::read(&path).unwrap();
            decompress(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
