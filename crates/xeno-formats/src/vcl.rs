//! `.vcl` cutscene pictures (`ealogo`, `presents`, `arrival`, ...). Each file is one
//! still picture, 640 pixels wide and 240 or 400 rows tall: a 768-byte VGA palette,
//! then PackBits to the end of the file. The game loads it into video memory in the
//! 320x240 mode, whose virtual screen is 640 wide, and shows a 320x240 window of it.
//! Any motion comes from the sequence tables in `xeno.exe`, not from the file. See
//! `docs/formats/vcl.md`.

use crate::mal::{self, Run};
use crate::{FormatError, Palette};

/// Byte offset of the palette: every `.vcl` file starts with one.
pub const PALETTE_OFFSET: usize = 0;

/// A decoded `.vcl` picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// The picture's own palette.
    pub palette: Palette,
    /// Rows in the picture: 240 or 400 in a stock install.
    pub height: usize,
    /// `WIDTH * height` palette indices, row by row.
    pub pixels: Vec<u8>,
}

impl Picture {
    /// Every picture is as wide as the 320x240 mode's virtual screen.
    pub const WIDTH: usize = 640;
    /// The most rows that fit in the 256 KB of VGA memory the loader writes to.
    pub const MAX_HEIGHT: usize = 400;
    /// Offset of the PackBits stream, right after the palette.
    pub const PICTURE_OFFSET: usize = Palette::BYTES;
}

/// Reads the 256-colour palette a `.vcl` file starts with: 768 bytes of VGA DAC
/// triples (0..=63 per channel), widened to 8 bits.
pub fn palette(bytes: &[u8]) -> Result<Palette, FormatError> {
    let end = PALETTE_OFFSET + Palette::BYTES;
    let raw = bytes.get(PALETTE_OFFSET..end).ok_or(FormatError::Invalid {
        offset: bytes.len() as u64,
        reason: format!("file ends before its {}-byte palette", Palette::BYTES),
    })?;
    Palette::from_vga6(raw)
}

/// The PackBits runs after the palette. They must end exactly at the end of the file,
/// as the game's loader decodes until it runs out of data. Error offsets are into
/// `bytes`.
pub fn runs(bytes: &[u8]) -> Result<Vec<Run>, FormatError> {
    let at = Picture::PICTURE_OFFSET;
    let src = bytes.get(at..).unwrap_or_default();
    mal::runs_to_end(src).map_err(|e| match e {
        FormatError::Invalid { offset, reason } => FormatError::Invalid {
            offset: offset + at as u64,
            reason,
        },
        other => other,
    })
}

/// Decodes a `.vcl` file: palette, then whole 640-pixel rows, no more than fit in
/// video memory.
pub fn parse(bytes: &[u8]) -> Result<Picture, FormatError> {
    let palette = palette(bytes)?;
    let pixels = mal::expand(&runs(bytes)?);
    let rows = pixels.len() / Picture::WIDTH;
    let reason = if pixels.is_empty() {
        Some("no pixels after the palette".to_string())
    } else if !pixels.len().is_multiple_of(Picture::WIDTH) {
        Some(format!(
            "{} pixels are not whole {}-pixel rows",
            pixels.len(),
            Picture::WIDTH
        ))
    } else if rows > Picture::MAX_HEIGHT {
        Some(format!(
            "{rows} rows are more than the {} that fit in video memory",
            Picture::MAX_HEIGHT
        ))
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(FormatError::Invalid {
            offset: bytes.len() as u64,
            reason,
        });
    }
    Ok(Picture {
        palette,
        height: rows,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;
    use crate::fixtures::{synthetic_bytes, synthetic_vcl};

    fn vga6(seed: u64) -> Vec<u8> {
        synthetic_bytes(Palette::BYTES, seed)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect()
    }

    #[test]
    fn synthetic_picture_round_trips() {
        let palette = vga6(1);
        let mut pixels = synthetic_bytes(Picture::WIDTH * 240, 2);
        pixels[700..1200].fill(9);
        let picture = parse(&synthetic_vcl(&palette, &pixels)).unwrap();
        assert_eq!(picture.palette, Palette::from_vga6(&palette).unwrap());
        assert_eq!(picture.height, 240);
        assert_eq!(picture.pixels, pixels);
    }

    #[test]
    fn runs_snapshot() {
        // One row: 2 literals, 638 repeats in 128s and a 126, as the encoder writes it.
        let mut row = vec![5u8; Picture::WIDTH];
        row[..2].copy_from_slice(&[1, 2]);
        let bytes = synthetic_vcl(&vga6(2), &row);
        insta::assert_debug_snapshot!(runs(&bytes).unwrap());
        assert_eq!(parse(&bytes).unwrap().height, 1);
    }

    #[test]
    fn pictures_must_be_whole_rows_that_fit_in_video_memory() {
        let palette = vga6(3);
        let max = synthetic_vcl(&palette, &vec![0; Picture::WIDTH * Picture::MAX_HEIGHT]);
        assert_eq!(parse(&max).unwrap().height, 400);

        let too_tall = synthetic_vcl(&palette, &vec![0; Picture::WIDTH * 401]);
        let part_row = [&palette[..], &[0x81, 0]].concat();
        let cases: [(&[u8], &str); 3] = [
            (&too_tall, "401 rows are more than the 400"),
            (&part_row, "128 pixels are not whole 640-pixel rows"),
            (&palette, "no pixels after the palette"),
        ];
        for (bytes, reason) in cases {
            match parse(bytes) {
                Err(FormatError::Invalid { offset, reason: r }) => {
                    assert_eq!(offset, bytes.len() as u64);
                    assert!(r.contains(reason), "{r}");
                }
                other => panic!("expected Invalid ({reason}), got {other:?}"),
            }
        }
    }

    #[test]
    fn a_stream_cut_short_is_refused_where_its_packet_starts() {
        let mut bytes = synthetic_vcl(&vga6(4), &[7; Picture::WIDTH]);
        bytes.extend([0x05, 1, 2]);
        let at = bytes.len() as u64 - 3;
        match parse(&bytes) {
            Err(FormatError::Invalid { offset, reason }) => {
                assert_eq!(offset, at);
                assert_eq!(reason, "literal run of 6 ends early");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn palette_is_read_from_the_start_and_widened() {
        let mut bytes: Vec<u8> = synthetic_bytes(Palette::BYTES + 100, 3)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect();
        bytes[3..6].copy_from_slice(&[63, 0, 32]);
        // Frame data after the palette is not limited to 0..=63.
        bytes[Palette::BYTES] = 0xff;

        let palette = palette(&bytes).unwrap();
        assert_eq!(
            palette,
            Palette::from_vga6(&bytes[..Palette::BYTES]).unwrap()
        );
        assert_eq!(
            palette[1],
            Rgb {
                r: 255,
                g: 0,
                b: 130
            }
        );
    }

    #[test]
    fn short_files_are_refused() {
        match palette(&[0; 10]) {
            Err(FormatError::Invalid { offset, .. }) => assert_eq!(offset, 10),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_vcl_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let vcls: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vcl")))
            .collect();
        assert_eq!(vcls.len(), 14, "a stock install has 14 .vcl files");
        for path in vcls {
            let bytes = std::fs::read(&path).unwrap();
            let picture = parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!([240, 400].contains(&picture.height), "{path:?}");
            // The encoder worked row by row: no run crosses into the next row.
            let mut x = 0;
            for run in runs(&bytes).unwrap() {
                assert!(x % Picture::WIDTH + run.len() <= Picture::WIDTH, "{path:?}");
                x += run.len();
            }
        }
    }

    #[test]
    fn values_above_63_are_refused() {
        let mut bytes = vec![0u8; Palette::BYTES];
        bytes[7] = 64;
        assert!(matches!(
            palette(&bytes),
            Err(FormatError::Invalid { offset: 7, .. })
        ));
    }
}
