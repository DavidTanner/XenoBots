//! `dump`: decode a file and write what it contains as PNGs into an output directory.
//! Formats are added here as their parsers land.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use xeno_formats::{Palette, Rgb, ban, mal, mxx, pcx, vcl};

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
            vec![(stem, image.into())]
        }
        "m@l" => {
            let screen = mal::parse(bytes)
                .with_context(|| format!("{} is not a valid screen", file.display()))?;
            let (w, h) = (mal::Screen::WIDTH, mal::Screen::HEIGHT);
            let image = indexed_to_rgb(w, h, &screen.pixels, &screen.palette);
            vec![(stem, image.into())]
        }
        "vcl" => {
            let picture = vcl::parse(bytes)
                .with_context(|| format!("{} is not a valid cutscene picture", file.display()))?;
            let (w, h) = (vcl::Picture::WIDTH, picture.height);
            let image = indexed_to_rgb(w, h, &picture.pixels, &picture.palette);
            vec![(stem, image.into())]
        }
        "b@n" if stem.starts_with("sky") => {
            let sky = ban::parse_sky(bytes)
                .with_context(|| format!("{} is not a valid sky strip", file.display()))?;
            let image = indexed_to_rgb(ban::Sky::WIDTH, ban::Sky::HEIGHT, &sky.pixels, palette);
            vec![(stem, image.into())]
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
                    (format!("{stem}_{i:03}"), image.into())
                })
                .collect()
        }
        "m00" | "m08" | "m16" | "m32" => {
            let model = mxx::parse(bytes)
                .with_context(|| format!("{} is not a valid model", file.display()))?;
            // Parts can draw images from the model's `.m00`, which sits next to it.
            let direction_free = ["m00", "M00"]
                .iter()
                .find_map(|e| std::fs::read(file.with_extension(e)).ok())
                .and_then(|bytes| mxx::parse(&bytes).ok())
                .and_then(|m00| m00.views.into_iter().next())
                .map(|view| view.images)
                .unwrap_or_default();
            model_images(&stem, &model, &direction_free, palette)
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

/// A view with no facing gives one PNG per image. A view with a facing gives one per
/// frame of each animation, for the facing and for its mirror, named by facing,
/// animation and frame; frames with no parts are left out.
fn model_images(
    stem: &str,
    model: &mxx::Model,
    direction_free: &[mxx::Image],
    palette: &Palette,
) -> Vec<(String, image::DynamicImage)> {
    let mut out = Vec::new();
    for view in &model.views {
        let h = &view.header;
        if !h.has_facing() {
            for (i, image) in view.images.iter().enumerate() {
                let (w, h) = (usize::from(image.width), usize::from(image.height));
                // A PNG can't be empty.
                if w * h > 0 {
                    let rgb = indexed_to_rgb(w, h, &image.to_indexed(), palette);
                    out.push((format!("{stem}_{i:03}"), rgb.into()));
                }
            }
            continue;
        }
        let facings = [(h.facing, &view.animations)]
            .into_iter()
            .chain(view.mirror_animations.iter().map(|a| (h.mirror_facing, a)));
        for (facing, animations) in facings {
            for (a, frames) in animations.iter().enumerate() {
                for (f, frame) in frames.iter().enumerate() {
                    let resolve = |p: &mxx::Part| {
                        let images = if p.is_direction_free() {
                            direction_free
                        } else {
                            &view.images
                        };
                        images.get(p.image_index())
                    };
                    if let Some(image) = compose(frame, resolve, palette) {
                        out.push((format!("{stem}_f{facing:02}_a{a}_{f:02}"), image.into()));
                    }
                }
            }
        }
    }
    out
}

/// Draws a frame's parts, in order, on a transparent canvas just big enough to hold
/// them. Only pixels inside the row spans are drawn. Parts whose image `resolve`
/// can't find (a missing `.m00`) are skipped. `None` when nothing is left to draw.
fn compose<'a>(
    frame: &mxx::Frame,
    resolve: impl Fn(&mxx::Part) -> Option<&'a mxx::Image>,
    palette: &Palette,
) -> Option<image::RgbaImage> {
    let parts: Vec<_> = frame
        .parts
        .iter()
        .filter_map(|p| Some((p, resolve(p)?)))
        .filter(|(_, image)| image.width > 0 && image.height > 0)
        .collect();
    let left = parts.iter().map(|(p, _)| i32::from(p.x)).min()?;
    let top = parts.iter().map(|(p, _)| i32::from(p.y)).min()?;
    let right = parts
        .iter()
        .map(|(p, i)| i32::from(p.x) + i32::from(i.width))
        .max()?;
    let bottom = parts
        .iter()
        .map(|(p, i)| i32::from(p.y) + i32::from(i.height))
        .max()?;
    let mut canvas = image::RgbaImage::new((right - left) as u32, (bottom - top) as u32);
    for (part, image) in parts {
        for (y, span) in image.rows.iter().enumerate() {
            for (i, &index) in span.pixels.iter().enumerate() {
                let column = i32::from(span.skip) + i as i32;
                let column = if part.is_flipped() {
                    i32::from(image.width) - 1 - column
                } else {
                    column
                };
                let c = palette[index];
                canvas.put_pixel(
                    (i32::from(part.x) + column - left) as u32,
                    (i32::from(part.y) + y as i32 - top) as u32,
                    image::Rgba([c.r, c.g, c.b, 255]),
                );
            }
        }
    }
    Some(canvas)
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
        synthetic_bytes, synthetic_model, synthetic_packed, synthetic_pcx, synthetic_screen,
        synthetic_sprite_bank, synthetic_vcl,
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
    fn screens_dump_in_their_own_palette() {
        let out = std::env::temp_dir().join(format!("xeno-tool-mal-{}", std::process::id()));
        let palette: Vec<u8> = synthetic_bytes(Palette::BYTES, 7)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect();
        let pixels = synthetic_bytes(64_000, 8);
        let bytes = synthetic_screen(&palette, &pixels);
        let grey = Palette::from_rgb8(&[0; Palette::BYTES]).unwrap();

        let written = dump(Path::new("SitRoom.M@L"), &bytes, &out, Some(&grey)).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("sitroom.png")]);
        assert_eq!(image.dimensions(), (320, 200));
        let c = Palette::from_vga6(&palette).unwrap()[pixels[320 + 5]];
        assert_eq!(image.get_pixel(5, 1).0, [c.r, c.g, c.b]);
    }

    #[test]
    fn vcl_pictures_dump_whole_in_their_own_palette() {
        let out = std::env::temp_dir().join(format!("xeno-tool-vcl-{}", std::process::id()));
        let palette: Vec<u8> = synthetic_bytes(Palette::BYTES, 9)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect();
        let pixels = synthetic_bytes(640 * 400, 10);
        let bytes = synthetic_vcl(&palette, &pixels);

        let written = dump(Path::new("Arrival.VCL"), &bytes, &out, None).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("arrival.png")]);
        assert_eq!(image.dimensions(), (640, 400));
        let c = Palette::from_vga6(&palette).unwrap()[pixels[640 * 399 + 639]];
        assert_eq!(image.get_pixel(639, 399).0, [c.r, c.g, c.b]);
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
    fn models_dump_one_png_per_frame_and_facing() {
        let out = std::env::temp_dir().join(format!("xeno-tool-mxx-{}", std::process::id()));
        // Each frame draws both images at (0, 0); the mirror facing's frame flips them.
        let bytes = synthetic_model(&[(8, &[(3, 1, &[10, 20, 30]), (2, 1, &[40, 50])])]);

        let written = dump(Path::new("Scout.M08"), &bytes, &out, None).unwrap();
        let facing = image::open(&written[0]).unwrap().into_rgba8();
        let mirror = image::open(&written[1]).unwrap().into_rgba8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(
            written,
            [
                out.join("scout_f08_a0_00.png"),
                out.join("scout_f56_a0_00.png")
            ]
        );
        assert_eq!(facing.dimensions(), (3, 1));
        let row = |image: &image::RgbaImage| {
            (0..3)
                .map(|x| image.get_pixel(x, 0).0[0])
                .collect::<Vec<_>>()
        };
        // The second part covers the first.
        assert_eq!(row(&facing), [40, 50, 30]);
        assert_eq!(row(&mirror), [50, 40, 10]);
    }

    #[test]
    fn direction_free_models_dump_their_images() {
        let out = std::env::temp_dir().join(format!("xeno-tool-m00-{}", std::process::id()));
        let bytes = synthetic_model(&[(-2, &[(1, 2, &[1, 2])])]);

        let written = dump(Path::new("netrelay.m00"), &bytes, &out, None).unwrap();
        let image = image::open(&written[0]).unwrap().into_rgb8();
        let _ = std::fs::remove_dir_all(&out);

        assert_eq!(written, [out.join("netrelay_000.png")]);
        assert_eq!(image.get_pixel(0, 1).0, [2, 2, 2]);
    }

    #[test]
    fn unknown_formats_are_refused() {
        let out = Path::new("unused");
        let e = dump(Path::new("scen00a.usg"), b"", out, None).unwrap_err();
        assert_eq!(e.to_string(), "no decoder for .usg files yet");
        assert!(dump(Path::new("README"), b"", out, None).is_err());
    }
}
