//! `palette`: find the 256-colour palette in a file and print it, one colour per line.

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, bail};
use clap::ValueEnum;
use xeno_formats::{Palette, pcx, vcl};

/// How a raw palette read with `--offset` is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Encoding {
    /// VGA DAC triples, 0..=63 per channel, as the game writes them to the DAC.
    Vga6,
    /// 8 bits per channel, as in a PCX trailer.
    Rgb8,
}

/// Where to read a palette that the file's extension does not locate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raw {
    pub offset: usize,
    pub encoding: Encoding,
}

/// The palette in `file` (contents `bytes`), and a line saying where it came from.
/// `raw` overrides detection by extension.
pub fn find(file: &Path, bytes: &[u8], raw: Option<Raw>) -> anyhow::Result<(Palette, String)> {
    if let Some(Raw { offset, encoding }) = raw {
        let end = offset
            .checked_add(Palette::BYTES)
            .filter(|&end| end <= bytes.len())
            .with_context(|| {
                format!(
                    "{} is {} bytes; no {}-byte palette fits at offset {offset:#x}",
                    file.display(),
                    bytes.len(),
                    Palette::BYTES
                )
            })?;
        let slice = &bytes[offset..end];
        let (palette, name) = match encoding {
            Encoding::Vga6 => (Palette::from_vga6(slice)?, "VGA 6-bit"),
            Encoding::Rgb8 => (Palette::from_rgb8(slice)?, "8-bit"),
        };
        return Ok((palette, format!("raw {name} palette at offset {offset:#x}")));
    }

    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pcx" => {
            let pcx = pcx::parse(bytes)
                .with_context(|| format!("{} is not a valid PCX", file.display()))?;
            Ok((pcx.palette, "pcx trailer, 8-bit".into()))
        }
        "vcl" => {
            let palette = vcl::palette(bytes)
                .with_context(|| format!("{} has no valid palette", file.display()))?;
            Ok((
                palette,
                format!("vcl header at offset {:#x}, VGA 6-bit", vcl::PALETTE_OFFSET),
            ))
        }
        "" => bail!(
            "{} has no extension; pass --offset to read a raw palette",
            file.display()
        ),
        other => bail!("no known palette in .{other} files; pass --offset to read a raw palette"),
    }
}

/// One line per colour: index, `#rrggbb`, then the three 8-bit channels in decimal.
pub fn listing(palette: &Palette) -> String {
    let mut out = String::new();
    for (i, c) in palette.colors.iter().enumerate() {
        writeln!(
            out,
            "{i:3}  #{:02x}{:02x}{:02x}  {:3} {:3} {:3}",
            c.r, c.g, c.b, c.r, c.g, c.b
        )
        .unwrap();
    }
    out
}

/// Parses an offset in decimal or, with a `0x` prefix, hex.
pub fn parse_offset(s: &str) -> Result<usize, String> {
    let parsed = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16),
        None => s.parse(),
    };
    parsed.map_err(|e| format!("`{s}` is not an offset: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_bytes, synthetic_pcx};

    fn vga6_bytes(len: usize, seed: u64) -> Vec<u8> {
        synthetic_bytes(len, seed)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect()
    }

    #[test]
    fn vcl_listing_of_a_synthetic_file() {
        let bytes = vga6_bytes(Palette::BYTES + 20, 5);
        let (palette, source) = find(Path::new("dir/EALOGO.VCL"), &bytes, None).unwrap();
        assert_eq!(source, "vcl header at offset 0x0, VGA 6-bit");
        insta::assert_snapshot!(listing(&palette));
    }

    #[test]
    fn pcx_palette_comes_from_the_trailer() {
        let trailer = synthetic_bytes(Palette::BYTES, 2);
        let bytes = synthetic_pcx(2, 2, &synthetic_bytes(4, 1), &trailer);
        let (palette, source) = find(Path::new("drop.pcx"), &bytes, None).unwrap();
        assert_eq!(palette, Palette::from_rgb8(&trailer).unwrap());
        assert_eq!(source, "pcx trailer, 8-bit");
    }

    #[test]
    fn raw_offset_overrides_the_extension() {
        let mut bytes = vec![0xffu8; 0x10];
        bytes.extend(vga6_bytes(Palette::BYTES, 9));
        let raw = Raw {
            offset: 0x10,
            encoding: Encoding::Vga6,
        };
        let (palette, source) = find(Path::new("mgavideo.dvr"), &bytes, Some(raw)).unwrap();
        assert_eq!(palette, Palette::from_vga6(&bytes[0x10..]).unwrap());
        assert_eq!(source, "raw VGA 6-bit palette at offset 0x10");

        let raw = Raw {
            offset: 0,
            encoding: Encoding::Rgb8,
        };
        let (palette, _) = find(Path::new("x.vcl"), &bytes, Some(raw)).unwrap();
        assert_eq!(palette[0].r, 0xff);
    }

    #[test]
    fn raw_offset_past_the_end_is_refused() {
        let raw = Raw {
            offset: 1,
            encoding: Encoding::Rgb8,
        };
        let e = find(Path::new("p.bin"), &[0; Palette::BYTES], Some(raw)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "p.bin is 768 bytes; no 768-byte palette fits at offset 0x1"
        );
        let raw = Raw {
            offset: usize::MAX,
            ..raw
        };
        assert!(find(Path::new("p.bin"), &[0; 10], Some(raw)).is_err());
    }

    #[test]
    fn unknown_formats_ask_for_an_offset() {
        let e = find(Path::new("scen00a.usg"), b"", None).unwrap_err();
        assert_eq!(
            e.to_string(),
            "no known palette in .usg files; pass --offset to read a raw palette"
        );
        assert!(find(Path::new("README"), b"", None).is_err());
    }

    #[test]
    fn listing_has_one_line_per_colour() {
        let palette = Palette::from_vga6(&vga6_bytes(Palette::BYTES, 1)).unwrap();
        let out = listing(&palette);
        assert_eq!(out.lines().count(), 256);
        let c = palette[255];
        let last = format!(
            "255  #{:02x}{:02x}{:02x}  {:3} {:3} {:3}",
            c.r, c.g, c.b, c.r, c.g, c.b
        );
        assert_eq!(out.lines().last(), Some(last.as_str()));
    }

    #[test]
    fn offsets_parse_in_decimal_and_hex() {
        assert_eq!(parse_offset("768"), Ok(768));
        assert_eq!(parse_offset("0x300"), Ok(768));
        assert_eq!(parse_offset("0X1f"), Ok(31));
        assert!(parse_offset("0xzz").is_err());
        assert!(parse_offset("-1").is_err());
    }
}
