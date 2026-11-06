//! `--hd-pack` (T-072): the user's replacement images, PNGs named by the asset they
//! replace (`docs/hd.md`, "The HD pack"), for `xeno_render::compose`.

use std::collections::HashMap;
use std::path::Path;

use xeno_render::hd::Original;
use xeno_render::{AssetId, HdImage};

use crate::game_dir::GameFiles;

/// Every image a pack may replace, from the user's files, with the palette it is shown
/// with: each model's images, each bank's sprites, and each `.m@l` screen, `.vcl`
/// picture, `.pcx` backdrop and sky strip as a whole file (one that does not load has
/// none).
pub fn originals(files: &GameFiles) -> HashMap<AssetId, Original> {
    let models: Vec<_> = crate::scene::load_models(files)
        .into_iter()
        .flatten()
        .collect();
    let read = |name: &str| files.read(name).ok();
    let (images, failed) = xeno_render::hd::originals(read, files.names(), &models);
    for name in failed {
        log::warn!("HD pack: {name} does not load; its sprites stay the original's");
    }
    images.into_iter().collect()
}

/// Every PNG under `dir` that names an image in `originals` and is a whole multiple of
/// its size on each axis, painted against its palette (T-241). Anything else is warned about and left
/// out, so that image stays the original's; only a directory that cannot be read fails.
pub fn load(
    dir: &Path,
    originals: &HashMap<AssetId, Original>,
) -> Result<HashMap<AssetId, HdImage>, String> {
    let mut files = Vec::new();
    walk(dir, &mut files).map_err(|e| format!("cannot read the HD pack {}: {e}", dir.display()))?;
    let mut pack = HashMap::new();
    for file in files {
        let Ok(relative) = file.strip_prefix(dir) else {
            continue;
        };
        let path: Vec<_> = relative.iter().map(|c| c.to_string_lossy()).collect();
        match image(&path.join("/"), &file, originals) {
            Ok((asset, image)) => _ = pack.insert(asset, image),
            Err(e) => log::warn!("HD pack: {}: {e}; skipped", relative.display()),
        }
    }
    log::info!("HD pack {}: {} images", dir.display(), pack.len());
    Ok(pack)
}

/// The files under `dir`, depth first.
fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// The asset `name` names and `file` decoded as its HD image.
fn image(
    name: &str,
    file: &Path,
    originals: &HashMap<AssetId, Original>,
) -> Result<(AssetId, HdImage), String> {
    let asset = AssetId::from_pack_path(name).ok_or("not an asset's name")?;
    let original = originals
        .get(&asset)
        .ok_or("no such image in the game's files")?;
    let (w, h) = (original.width, original.height);
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let png = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (width, height) = (png.width() as usize, png.height() as usize);
    if w == 0 || h == 0 || width % w != 0 || height % h != 0 {
        return Err(format!(
            "{width}x{height} is not a whole multiple of the original's {w}x{h}"
        ));
    }
    let texels = png.pixels().map(|p| {
        let [r, g, b, a] = p.0.map(u32::from);
        a << 24 | r << 16 | g << 8 | b
    });
    let image = HdImage::new(width, height, texels.collect()).ok_or("empty")?;
    let image = image.with_reference(original.palette.clone());
    Ok((asset, image))
}
