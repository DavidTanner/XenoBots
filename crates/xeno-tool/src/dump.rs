//! `dump`: decode a file and write what it contains as PNGs into an output directory.
//! Formats are added here as their parsers land.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use xeno_formats::{Palette, Rgb, ban, pcx};

/// Decodes `file` (contents `bytes`) and writes its images into `out_dir`, which is
/// created if needed. Returns the paths written. Files that carry no palette of their
/// own are drawn with `palette`, or in grey (index `i` as `i, i, i`) without one.
pub fn dump(
    file: &Path,
    bytes: &[u8],
    out_dir: &Path,
    palette: Option<&Palette>,
) -> anyhow::Result<Vec<PathBuf>> {
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let grey = grey_palette();
    let palette = palette.unwrap_or(&grey);
    let images = match extension.as_str() {
        "pcx" => {
            let pcx = pcx::parse(bytes)
                .with_context(|| format!("{} is not a valid PCX", file.display()))?;
            let image = indexed_to_rgb(pcx.width(), pcx.height(), &pcx.pixels, &pcx.palette);
            vec![(stem, image)]
        }
        "b@n" if stem.starts_with("sky") => {
            let sky = ban::parse_sky(bytes)
                .with_context(|| format!("{} is not a valid sky strip", file.display()))?;
            let image = indexed_to_rgb(ban::Sky::WIDTH, ban::Sky::HEIGHT, &sky.pixels, palette);
            vec![(stem, image)]
        }
        "b@n" => {
            let bank = ban::parse_sprites(bytes)
                .with_context(|| format!("{} is not a valid sprite bank", file.display()))?;
            bank.sprites
                .iter()
                .enumerate()
                // A PNG can't be empty; no stock sprite is.
                .filter(|(_, s)| !s.pixels.is_empty())
                .map(|(i, s)| {
                    let image = indexed_to_rgb(s.width(), s.height(), &s.pixels, palette);
                    (format!("{stem}_{i:03}"), image)
                })
                .collect()
        }
        "" => bail!(
            "{} has no extension; cannot tell its format",
            file.display()
        ),
        other => bail!("no decoder for .{other} files yet"),
    };

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    images
        .into_iter()
        .map(|(name, image)| {
            let path = out_dir.join(format!("{name}.png"));
            image
                .save(&path)
                .with_context(|| format!("cannot write {}", path.display()))?;
            Ok(path)
        })
        .collect()
}

/// Index `i` as grey `i, i, i`: a stand-in for files with no palette of their own.
fn grey_palette() -> Palette {
    Palette {
        colors: std::array::from_fn(|i| {
            let v = i as u8;
            Rgb { r: v, g: v, b: v }
        }),
    }
}

/// Looks every pixel up in `palette`.
fn indexed_to_rgb(
    width: usize,
    height: usize,
    pixels: &[u8],
    palette: &Palette,
) -> image::RgbImage {
    let rgb = pixels
        .iter()
        .flat_map(|&i| {
            let c = palette[i];
            [c.r, c.g, c.b]
        })
        .collect();
    image::RgbImage::from_raw(width as u32, height as u32, rgb)
        .expect("pixel count matches the dimensions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{
        synthetic_bytes, synthetic_packed, synthetic_pcx, synthetic_sprite_bank,
    };

    #[test]
    fn indexed_pixels_take_their_palette_colours() {
        let palette = Palette::from_rgb8(&synthetic_bytes(Palette::BYTES, 4)).unwrap();
        let image = indexed_to_rgb(2, 1, &[0, 255], &palette);
        let c = palette[255];
        assert_eq!(image.get_pixel(1, 0).0, [c.r, c.g, c.b]);
        let c = palette[0];
        assert_eq!(image.get_pixel(0, 0).0, [c.r, c.g, c.b]);
    }

    #[test]
    fn pcx_dumps_to_a_png_named_after_the_file() {
        let out = std::env::temp_dir().join(format!("xeno-tool-dump-{}", std::process::id()));
        let pixels = synthetic_bytes(12, 1);
        let palette = synthetic_bytes(Palette::BYTES, 2);
        let bytes = synthetic_pcx(4, 3, &pixels, &palette);

        let written = dump(Path::new("dir/DROP.PCX"), &bytes, &out, None).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("drop.png")]);
        assert_eq!(image.dimensions(), (4, 3));
        let palette = Palette::from_rgb8(&palette).unwrap();
        let c = palette[pixels[5]];
        assert_eq!(image.get_pixel(1, 1).0, [c.r, c.g, c.b]);
    }

    #[test]
    fn sprite_banks_dump_one_png_per_sprite() {
        let out = std::env::temp_dir().join(format!("xeno-tool-bank-{}", std::process::id()));
        let big = synthetic_bytes(6 * 4, 3);
        let bytes = synthetic_sprite_bank(&[(2, 1, &[0, 200]), (0, 0, &[]), (6, 4, &big)]);
        let palette = Palette::from_rgb8(&synthetic_bytes(Palette::BYTES, 5)).unwrap();

        let written = dump(Path::new("MSPRITE9.B@N"), &bytes, &out, Some(&palette)).unwrap();
        let first = image::open(&written[0]).unwrap().into_rgb8();
        let last = image::open(&written[1]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(
            written,
            [out.join("msprite9_000.png"), out.join("msprite9_002.png")]
        );
        let c = palette[200];
        assert_eq!(first.get_pixel(1, 0).0, [c.r, c.g, c.b]);
        assert_eq!(last.dimensions(), (6, 4));
    }

    #[test]
    fn sky_strips_dump_in_grey_without_a_palette() {
        let out = std::env::temp_dir().join(format!("xeno-tool-sky-{}", std::process::id()));
        let pixels = synthetic_bytes(ban::Sky::WIDTH * ban::Sky::HEIGHT, 6);
        let bytes = synthetic_packed(&pixels);

        let written = dump(Path::new("sky_z.b@n"), &bytes, &out, None).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("sky_z.png")]);
        assert_eq!(image.dimensions(), (342, 54));
        let v = pixels[ban::Sky::WIDTH + 1];
        assert_eq!(image.get_pixel(1, 1).0, [v, v, v]);
    }

    #[test]
    fn unknown_formats_are_refused() {
        let out = Path::new("unused");
        let e = dump(Path::new("scen00a.usg"), b"", out, None).unwrap_err();
        assert_eq!(e.to_string(), "no decoder for .usg files yet");
        assert!(dump(Path::new("README"), b"", out, None).is_err());
    }
}
