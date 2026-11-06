//! `dump <game dir> --hd-template <dir>` (T-240): every image an HD pack may replace,
//! written at its pack path 4x larger, in the palette the game shows it with and with
//! colour 0 clear where it is not drawn, as a start for a pack (`docs/hd.md`,
//! "Starting a pack").

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use xeno_render::{HdImage, LoadedModel, hd};

/// Each side of a template image, in texels a pixel.
pub const SCALE: usize = 4;

/// What [`write`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub written: usize,
    pub bytes: u64,
    /// Already in the directory, so left as they are (an artist's work is never lost).
    pub kept: usize,
    /// Empty, or shown on a screen that does not load.
    pub left_out: usize,
}

/// Writes the template of the game files in `game_dir` into `out`, which must not be
/// inside this repository.
pub fn write(game_dir: &Path, out: &Path) -> anyhow::Result<Written> {
    refuse_repository(out)?;
    let mut files = HashMap::new();
    let entries = std::fs::read_dir(game_dir)
        .with_context(|| format!("cannot read the game directory {}", game_dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        files.insert(name, entry.path());
    }
    let read = |name: &str| files.get(name).and_then(|p| std::fs::read(p).ok());
    let models = LoadedModel::load_all(|n| read(n).ok_or(format!("cannot read {n}")));
    let models: Vec<_> = models
        .into_iter()
        .filter_map(|m| m.map_err(|e| eprintln!("warning: {e}; left out")).ok())
        .collect();
    let (images, failed) = hd::originals(read, files.keys().map(String::as_str), &models);
    for name in failed {
        eprintln!("warning: {name} does not load; its sprites are left out");
    }
    if images.is_empty() {
        bail!("no game images in {}", game_dir.display());
    }
    let mut done = Written::default();
    for (asset, original) in images {
        let Some(image) = original.upscaled(SCALE) else {
            done.left_out += 1;
            continue;
        };
        let path = out.join(asset.pack_path());
        if path.exists() {
            done.kept += 1;
            continue;
        }
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("cannot create {}", path.parent().unwrap().display()))?;
        save_png(&path, &image)?;
        done.written += 1;
        done.bytes += std::fs::metadata(&path).map_or(0, |m| m.len());
    }
    Ok(done)
}

/// `image` as an RGBA PNG at `path`.
pub fn save_png(path: &Path, image: &HdImage) -> anyhow::Result<()> {
    let (w, h) = image.size();
    image::RgbaImage::from_raw(w as u32, h as u32, image.to_rgba8())
        .expect("an HdImage holds width * height texels")
        .save(path)
        .with_context(|| format!("cannot write {}", path.display()))
}

/// An error if `out` is in this repository: the template is the game's art, which must
/// never be committed (AGENTS.md, "Never commit game data").
fn refuse_repository(out: &Path) -> anyhow::Result<()> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(repository) = repository.canonicalize() else {
        return Ok(()); // Built from a checkout that is gone.
    };
    let out = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    // The nearest directory that exists, with links resolved.
    let existing: Option<PathBuf> = out.ancestors().find_map(|a| a.canonicalize().ok());
    if existing.is_some_and(|e| e.starts_with(&repository)) {
        bail!(
            "{} is inside the repository {}: the template is the game's art and must never \
             be committed; write it outside the repository",
            out.display(),
            repository.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_bytes, synthetic_screen, synthetic_sprite_bank};
    use xeno_formats::{Palette, Rgb};

    #[test]
    fn an_image_is_written_four_times_larger_with_colour_0_clear() {
        let mut palette = Palette::from_rgb8(&[0; Palette::BYTES]).unwrap();
        palette.colors[0] = Rgb { r: 9, g: 9, b: 9 };
        palette.colors[7] = Rgb { r: 1, g: 2, b: 3 };
        let original = |keyed| hd::Original {
            width: 2,
            height: 1,
            pixels: vec![7, 0],
            palette: Some(palette.clone()),
            keyed,
        };
        let path = std::env::temp_dir().join(format!("xeno-tool-hd-{}.png", std::process::id()));
        save_png(&path, &original(true).upscaled(SCALE).unwrap()).unwrap();
        let png = image::open(&path).unwrap().into_rgba8();
        save_png(&path, &original(false).upscaled(SCALE).unwrap()).unwrap();
        let opaque = image::open(&path).unwrap().into_rgba8();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(png.dimensions(), (8, 4));
        for (x, y, p) in png.enumerate_pixels() {
            let want = if x < 4 { [1, 2, 3, 0xff] } else { [0; 4] };
            assert_eq!(p.0, want, "({x}, {y})");
        }
        // A whole picture keeps colour 0 opaque.
        assert_eq!(opaque.get_pixel(7, 3).0, [9, 9, 9, 0xff]);
        assert!(original(true).upscaled(0).is_none());
    }

    #[test]
    fn a_template_names_each_image_by_its_pack_path_and_keeps_what_is_there() {
        let root = std::env::temp_dir().join(format!("xeno-tool-template-{}", std::process::id()));
        let (game, out) = (root.join("game"), root.join("pack"));
        std::fs::create_dir_all(&game).unwrap();
        let dac: Vec<u8> = synthetic_bytes(Palette::BYTES, 3)
            .iter()
            .map(|b| b & 0x3f)
            .collect();
        let screen = synthetic_screen(&dac, &synthetic_bytes(64_000, 4));
        std::fs::write(game.join("SITROOM.M@L"), screen).unwrap();
        let bank = synthetic_sprite_bank(&[(2, 1, &[0, 200])]);
        std::fs::write(game.join("msprite1.b@n"), bank).unwrap();
        let first = write(&game, &out).unwrap();
        let sprite = image::open(out.join("sprite/1/0.png"))
            .unwrap()
            .into_rgba8();
        let screen = image::open(out.join("file/sitroom.png")).unwrap();
        let again = write(&game, &out).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!((first.written, first.kept, first.left_out), (2, 0, 0));
        assert_eq!((again.written, again.kept), (0, 2));
        assert_eq!(sprite.dimensions(), (8, 4));
        let c = Palette::from_vga6(&dac).unwrap()[200];
        assert_eq!(sprite.get_pixel(0, 0).0[3], 0);
        assert_eq!(sprite.get_pixel(7, 3).0, [c.r, c.g, c.b, 0xff]);
        assert_eq!((screen.width(), screen.height()), (1280, 800));
    }

    #[test]
    fn the_repository_is_refused() {
        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-dir/pack");
        assert!(refuse_repository(&inside).is_err());
        assert!(refuse_repository(&std::env::temp_dir().join("xeno-pack")).is_ok());
    }
}
