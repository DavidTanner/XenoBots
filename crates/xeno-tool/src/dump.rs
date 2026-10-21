//! `dump`: decode a file and write what it contains as PNGs into an output directory.
//! Formats are added here as their parsers land.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use xeno_formats::{Palette, pcx};

/// Decodes `file` (contents `bytes`) and writes its images into `out_dir`, which is
/// created if needed. Returns the paths written.
pub fn dump(file: &Path, bytes: &[u8], out_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let image = match extension.as_str() {
        "pcx" => {
            let pcx = pcx::parse(bytes)
                .with_context(|| format!("{} is not a valid PCX", file.display()))?;
            indexed_to_rgb(pcx.width(), pcx.height(), &pcx.pixels, &pcx.palette)
        }
        "" => bail!(
            "{} has no extension; cannot tell its format",
            file.display()
        ),
        other => bail!("no decoder for .{other} files yet"),
    };

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("cannot create {}", out_dir.display()))?;
    let path = out_dir.join(format!("{stem}.png"));
    image
        .save(&path)
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(vec![path])
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
    use xeno_formats::fixtures::{synthetic_bytes, synthetic_pcx};

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

        let written = dump(Path::new("dir/DROP.PCX"), &bytes, &out).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("drop.png")]);
        assert_eq!(image.dimensions(), (4, 3));
        let palette = Palette::from_rgb8(&palette).unwrap();
        let c = palette[pixels[5]];
        assert_eq!(image.get_pixel(1, 1).0, [c.r, c.g, c.b]);
    }

    #[test]
    fn unknown_formats_are_refused() {
        let out = Path::new("unused");
        let e = dump(Path::new("scen00a.usg"), b"", out).unwrap_err();
        assert_eq!(e.to_string(), "no decoder for .usg files yet");
        assert!(dump(Path::new("README"), b"", out).is_err());
    }
}
