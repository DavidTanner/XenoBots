//! Every image an HD pack may replace, from the user's files, in the palette the game
//! shows it with (T-240): what `--hd-pack` checks a pack against and what
//! `xeno-tool dump --hd-template` writes (`docs/hd.md`, "Starting a pack").

use std::collections::HashMap;

use xeno_formats::{Palette, ban, mal, vcl};

use super::{AssetId, HdImage, Name};
use crate::LoadedModel;

/// One image as the user's files have it: `width * height` palette indices, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    /// The palette it is shown with, or `None` when that screen does not load.
    pub palette: Option<Palette>,
    /// Colour 0 is not drawn (model images and sprites).
    pub keyed: bool,
}

impl Original {
    /// Each pixel as an `n` by `n` block of its colour, opaque, or clear (so the
    /// original shows) where colour 0 is not drawn, painted against its palette;
    /// `None` with no palette or pixels.
    pub fn upscaled(&self, n: usize) -> Option<HdImage> {
        let palette = self.palette.as_ref()?;
        let texel = |colour: u8| {
            let c = palette[colour];
            let rgb = u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b);
            if self.keyed && colour == 0 {
                0
            } else {
                0xff00_0000 | rgb
            }
        };
        let row = |y: usize| {
            let row = &self.pixels[y * self.width..(y + 1) * self.width];
            row.iter()
                .flat_map(move |&c| std::iter::repeat_n(texel(c), n))
        };
        let pixels = (0..self.height * n).flat_map(|ty| row(ty / n)).collect();
        let image = HdImage::new(self.width * n, self.height * n, pixels)?;
        Some(image.with_reference(self.palette.clone()))
    }
}

/// The screen an image with no palette of its own is shown on: a bank's the one it is
/// loaded with (`msprite0`, loaded at start-up, the situation room's), a model's its
/// own robot kind's console (the Humanoid's for the rest), and a sky the Humanoid's.
/// The consoles differ only in colours 253 and 254 (`docs/formats/mxx.md`, "Palette").
fn shown_on(asset: &AssetId) -> &'static str {
    match asset {
        AssetId::Sprite { bank: 2, .. } => "humancon.m@l",
        AssetId::Sprite { bank: 3, .. } => "scorpcon.m@l",
        AssetId::Sprite { bank: 4, .. } => "scoutcon.m@l",
        AssetId::Sprite { bank: 5, .. } => "maproom.m@l",
        AssetId::Sprite { .. } => "sitroom.m@l",
        AssetId::ModelImage { model, .. } if *model == Name::new("scorpion") => "scorpcon.m@l",
        AssetId::ModelImage { model, .. } if *model == Name::new("scout") => "scoutcon.m@l",
        _ => "humancon.m@l",
    }
}

/// Every image of `models`, of `msprite0` to `msprite5`, and each `.m@l` screen,
/// `.vcl` picture and sky strip among `names` as a whole file, read with `read`; and
/// the banks that do not load. A whole file that does not load is left out.
pub fn originals<'a>(
    read: impl Fn(&str) -> Option<Vec<u8>>,
    names: impl IntoIterator<Item = &'a str>,
    models: &[LoadedModel],
) -> (Vec<(AssetId, Original)>, Vec<String>) {
    let mut screens = HashMap::new();
    let mut palette = |asset: &AssetId| {
        let name = shown_on(asset);
        let screen = screens
            .entry(name)
            .or_insert_with(|| read(name).and_then(|b| mal::parse(&b).ok().map(|s| s.palette)));
        screen.clone()
    };
    let mut images = Vec::new();
    for (asset, i) in models.iter().flat_map(|m| m.images()) {
        let (width, height) = (usize::from(i.width), usize::from(i.height));
        let (pixels, palette) = (i.to_indexed(), palette(&asset));
        let keyed = true;
        images.push((
            asset,
            Original {
                width,
                height,
                pixels,
                palette,
                keyed,
            },
        ));
    }
    let mut failed = Vec::new();
    for bank in 0..6u8 {
        let name = format!("msprite{bank}.b@n");
        let Some(sprites) = read(&name).and_then(|b| ban::parse_sprites(&b).ok()) else {
            failed.push(name);
            continue;
        };
        for (index, s) in sprites.sprites.into_iter().enumerate() {
            let asset = AssetId::Sprite {
                bank,
                index: index as u16,
            };
            let (width, height, palette) = (s.width(), s.height(), palette(&asset));
            let (pixels, keyed) = (s.pixels, true);
            images.push((
                asset,
                Original {
                    width,
                    height,
                    pixels,
                    palette,
                    keyed,
                },
            ));
        }
    }
    for name in names {
        let Some(bytes) = read(name) else {
            continue;
        };
        let asset = AssetId::file(name);
        let (width, pixels, palette) = match name.rsplit_once('.').map_or("", |(_, ext)| ext) {
            "m@l" => match mal::parse(&bytes) {
                Ok(s) => (mal::Screen::WIDTH, s.pixels, Some(s.palette)),
                Err(_) => continue,
            },
            "vcl" => match vcl::parse(&bytes) {
                Ok(p) => (vcl::Picture::WIDTH, p.pixels, Some(p.palette)),
                Err(_) => continue,
            },
            "b@n" if name.starts_with("sky") => match ban::parse_sky(&bytes) {
                Ok(s) => (ban::Sky::WIDTH, s.pixels, palette(&asset)),
                Err(_) => continue,
            },
            _ => continue,
        };
        let (height, keyed) = (pixels.len() / width, false);
        images.push((
            asset,
            Original {
                width,
                height,
                pixels,
                palette,
                keyed,
            },
        ));
    }
    (images, failed)
}
