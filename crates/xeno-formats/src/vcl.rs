//! `.vcl` cutscenes and animations (`ealogo`, `presents`, `arrival`, ...). Only the
//! palette at the start of the file is decoded so far (T-013); the frame data after it
//! is T-018. See `docs/formats/palette.md`.

use crate::{FormatError, Palette};

/// Byte offset of the palette: every `.vcl` file starts with one.
pub const PALETTE_OFFSET: usize = 0;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;
    use crate::fixtures::synthetic_bytes;

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
    fn real_assets_every_vcl_starts_with_a_palette() {
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
            palette(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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
