//! `.m@l` screens: the game's full-screen `.MRL` pictures (`login`, `sitroom`,
//! `maproom`, ...), packed on disk (see `crate::packed`). Unpacked, a screen is a
//! 768-byte VGA palette and then one 320x200 picture in PackBits. See
//! `docs/formats/m@l.md`.

use crate::{FormatError, Palette, packed};

/// A decoded screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The screen's own palette; the driver loads it with the picture.
    pub palette: Palette,
    /// `WIDTH * HEIGHT` palette indices, row by row.
    pub pixels: Vec<u8>,
}

impl Screen {
    pub const WIDTH: usize = 320;
    pub const HEIGHT: usize = 200;
    /// Offset of the PackBits stream in the unpacked data, right after the palette.
    pub const PICTURE_OFFSET: usize = Palette::BYTES;
}

/// One PackBits packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Control byte `n` of 0..=127: the next `n + 1` bytes, copied as they are.
    Literal(Vec<u8>),
    /// Control byte `n` of 128..=255: the next byte, repeated `257 - n` times.
    Repeat { value: u8, len: usize },
}

impl Run {
    /// Bytes this run decodes to.
    pub fn len(&self) -> usize {
        match self {
            Run::Literal(bytes) => bytes.len(),
            Run::Repeat { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes this run takes in the stream, control byte included.
    pub fn encoded_len(&self) -> usize {
        match self {
            Run::Literal(bytes) => 1 + bytes.len(),
            Run::Repeat { .. } => 2,
        }
    }
}

/// Splits a PackBits stream into the runs that decode to exactly `len` bytes. A run
/// may not reach past `len`. Error offsets are into `src`.
pub fn runs(src: &[u8], len: usize) -> Result<Vec<Run>, FormatError> {
    let invalid = |offset: usize, reason: String| FormatError::Invalid {
        offset: offset as u64,
        reason,
    };
    let mut runs = Vec::new();
    let (mut i, mut out) = (0, 0);
    while out < len {
        let &control = src
            .get(i)
            .ok_or_else(|| invalid(i, format!("stream ends after {out} of {len} bytes")))?;
        let run = if control < 0x80 {
            let n = usize::from(control) + 1;
            let bytes = src
                .get(i + 1..i + 1 + n)
                .ok_or_else(|| invalid(i, format!("literal run of {n} ends early")))?;
            Run::Literal(bytes.to_vec())
        } else {
            let &value = src
                .get(i + 1)
                .ok_or_else(|| invalid(i, "repeat run ends early".into()))?;
            Run::Repeat {
                value,
                len: 257 - usize::from(control),
            }
        };
        if out + run.len() > len {
            return Err(invalid(
                i,
                format!("run of {} goes past the {len} bytes asked for", run.len()),
            ));
        }
        out += run.len();
        i += run.encoded_len();
        runs.push(run);
    }
    Ok(runs)
}

/// Expands `runs` into the bytes they stand for.
pub fn expand(runs: &[Run]) -> Vec<u8> {
    let mut out = Vec::with_capacity(runs.iter().map(Run::len).sum());
    for run in runs {
        match run {
            Run::Literal(bytes) => out.extend_from_slice(bytes),
            &Run::Repeat { value, len } => out.resize(out.len() + len, value),
        }
    }
    out
}

/// Decodes a packed `.m@l` screen. Error offsets are into the unpacked data.
pub fn parse(bytes: &[u8]) -> Result<Screen, FormatError> {
    parse_unpacked(&packed::decompress(bytes)?)
}

/// Decodes an unpacked screen (a `.MRL`): palette, then PackBits that must end
/// exactly where the 64,000th pixel does and use up the data.
pub fn parse_unpacked(data: &[u8]) -> Result<Screen, FormatError> {
    let at = Screen::PICTURE_OFFSET;
    let raw = data.get(..at).ok_or(FormatError::Invalid {
        offset: data.len() as u64,
        reason: format!("screen ends before its {at}-byte palette"),
    })?;
    let palette = Palette::from_vga6(raw)?;
    let runs = runs(&data[at..], Screen::WIDTH * Screen::HEIGHT).map_err(|e| match e {
        FormatError::Invalid { offset, reason } => FormatError::Invalid {
            offset: offset + at as u64,
            reason,
        },
        other => other,
    })?;
    let end = at + runs.iter().map(Run::encoded_len).sum::<usize>();
    if end != data.len() {
        return Err(FormatError::Invalid {
            offset: end as u64,
            reason: format!("{} bytes after the picture", data.len() - end),
        });
    }
    Ok(Screen {
        palette,
        pixels: expand(&runs),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_bytes, synthetic_packed, synthetic_screen};

    fn vga6(seed: u64) -> Vec<u8> {
        synthetic_bytes(Palette::BYTES, seed)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect()
    }

    #[test]
    fn synthetic_screen_round_trips() {
        let palette = vga6(1);
        let mut pixels = synthetic_bytes(Screen::WIDTH * Screen::HEIGHT, 2);
        pixels[1000..1500].fill(7);
        let screen = parse(&synthetic_screen(&palette, &pixels)).unwrap();
        assert_eq!(screen.palette, Palette::from_vga6(&palette).unwrap());
        assert_eq!(screen.pixels, pixels);
    }

    #[test]
    fn runs_snapshot() {
        // Three literals, 0x81 repeats 128 times, 0xff repeats twice, and 0x80 (which
        // no stock file uses) repeats 129 times, as the game's decoder does.
        let src = [0x02, 1, 2, 3, 0x81, 9, 0xff, 4, 0x80, 5];
        let runs = runs(&src, 3 + 128 + 2 + 129).unwrap();
        insta::assert_debug_snapshot!(runs);
        let out = expand(&runs);
        assert_eq!(out[..5], [1, 2, 3, 9, 9]);
        assert_eq!(out[130..133], [9, 4, 4]);
        assert!(out[133..].iter().all(|&b| b == 5));
    }

    #[test]
    fn broken_streams_are_rejected_with_their_offset() {
        let cases: [(&[u8], usize, u64, &str); 4] = [
            (&[0x02, 1, 2], 3, 0, "literal run of 3 ends early"),
            (&[0x00, 1, 0xfe], 4, 2, "repeat run ends early"),
            (&[0x00, 1], 4, 2, "stream ends after 1 of 4 bytes"),
            (&[0x00, 1, 0xfd, 0], 3, 2, "run of 4 goes past the 3 bytes"),
        ];
        for (src, len, offset, reason) in cases {
            match runs(src, len) {
                Err(FormatError::Invalid {
                    offset: at,
                    reason: r,
                }) => {
                    assert_eq!(at, offset, "{r}");
                    assert!(r.contains(reason), "{r}");
                }
                other => panic!("expected Invalid for {src:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn screens_must_be_exactly_one_picture() {
        // 64,000 pixels as 500 repeats of 128.
        let mut data = vga6(3);
        data.extend([0x81, 0].repeat(500));
        assert_eq!(parse_unpacked(&data).unwrap().pixels.len(), 64_000);

        data.push(0);
        match parse(&synthetic_packed(&data)) {
            Err(FormatError::Invalid { offset, reason }) => {
                assert_eq!(offset, 768 + 1000);
                assert_eq!(reason, "1 bytes after the picture");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        data.truncate(768 + 998);
        match parse_unpacked(&data) {
            Err(FormatError::Invalid { offset, reason }) => {
                assert_eq!(offset, 768 + 998);
                assert!(reason.contains("after 63872 of 64000"), "{reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn short_or_out_of_range_palettes_are_refused() {
        assert!(matches!(
            parse_unpacked(&[0; 10]),
            Err(FormatError::Invalid { offset: 10, .. })
        ));
        let mut data = vga6(4);
        data[5] = 64;
        assert!(matches!(
            parse_unpacked(&data),
            Err(FormatError::Invalid { offset: 5, .. })
        ));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_screen_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let screens: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("m@l")))
            .collect();
        assert_eq!(screens.len(), 8, "a stock install has 8 .m@l files");
        for path in screens {
            let bytes = std::fs::read(&path).unwrap();
            let data = packed::decompress(&bytes).unwrap();
            let runs = runs(&data[Screen::PICTURE_OFFSET..], 64_000).unwrap();
            // The encoder worked row by row: no run crosses into the next row.
            let mut x = 0;
            for run in &runs {
                assert!(x % Screen::WIDTH + run.len() <= Screen::WIDTH, "{path:?}");
                x += run.len();
            }
            parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}
