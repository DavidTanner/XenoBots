//! Resolution-independent drawing (T-070): names for the original's images and a
//! record of where each blit put one, beside the indexed framebuffer, which stays the
//! source of truth; see `docs/hd.md`.

use crate::Rect;

/// An original file's base name, lower case and NUL-padded (DOS names fit in 8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Name(pub [u8; 8]);

impl Name {
    /// `name` lower-cased, cut to 8 bytes.
    pub fn new(name: &str) -> Name {
        let mut bytes = [0; 8];
        for (b, c) in bytes.iter_mut().zip(name.bytes()) {
            *b = c.to_ascii_lowercase();
        }
        Name(bytes)
    }
}

/// Names one drawable image of the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetId {
    /// Sprite `index` of `msprite{bank}.b@n`, through the banks' one directory.
    Sprite { bank: u8, index: u16 },
    /// Image `image` of view `view` of a model (views numbered along the loaded chain,
    /// `.m08` first), or of its `.m00` when `view` is `None`. Frames share images, so
    /// the image, not the frame, is named.
    ModelImage {
        model: Name,
        view: Option<u16>,
        image: u16,
    },
    /// A whole file drawn as one picture: a `.m@l` screen, a `.vcl` picture, a sky.
    File(Name),
}

/// Where one blit drew an image: the logical rectangle the whole image covers, in
/// surface pixels and before any clipping, and how it was turned. A model image's
/// rectangle is its unit rectangle scaled from the anchor ([`crate::ModelScale`]);
/// `scale` is that 8.8 scale (256 for sprites and pictures).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlitRecord {
    pub asset: AssetId,
    pub rect: Rect,
    pub scale: u16,
    pub mirrored: bool,
    pub flipped: bool,
}

/// The blits since recording started, and for each surface pixel the blit that last
/// wrote it and the colour it wrote: `(k + 1) << 8 | colour`, or 0 for none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    blits: Vec<BlitRecord>,
    pub(crate) owners: Vec<u32>,
}

impl Recording {
    pub(crate) fn new(pixels: usize) -> Recording {
        Recording {
            blits: Vec::new(),
            owners: vec![0; pixels],
        }
    }

    pub fn blits(&self) -> &[BlitRecord] {
        &self.blits
    }

    /// Adds a blit and returns its tag (to be or-ed with a colour), or `None` when the
    /// tags have run out.
    pub(crate) fn push(&mut self, blit: BlitRecord) -> Option<u32> {
        let tag = u32::try_from(self.blits.len() + 1)
            .ok()
            .filter(|&k| k < 1 << 24)?;
        self.blits.push(blit);
        Some(tag << 8)
    }

    /// The blit whose pixel surface pixel `i` still shows, given its colour now. A
    /// later write of another colour (text, a polygon, a copied picture) disowns it.
    // shortcut: a later write of the same colour is not seen; tag every write if that shows.
    pub fn owner(&self, i: usize, colour: u8) -> Option<usize> {
        let owner = *self.owners.get(i)?;
        (owner != 0 && owner as u8 == colour).then(|| (owner >> 8) as usize - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Framebuffer, ModelScale, ModelTarget, Size};
    use xeno_formats::mxx::{Image, Span};

    /// An image with one span per row: `(skip, pixels)`.
    fn image(width: u8, rows: &[(u8, &[u8])]) -> Image {
        Image {
            width,
            height: rows.len() as u8,
            rows: rows
                .iter()
                .map(|&(skip, pixels)| Span {
                    skip,
                    pixels: pixels.to_vec(),
                })
                .collect(),
        }
    }

    fn target(scale: u16, anchor: (i32, i32)) -> ModelTarget {
        ModelTarget {
            clip: Rect::new(0, 0, 1000, 1000),
            anchor,
            scale: ModelScale::new(scale),
        }
    }

    fn model_image(name: &str, image: u16) -> AssetId {
        AssetId::ModelImage {
            model: Name::new(name),
            view: Some(1),
            image,
        }
    }

    #[test]
    fn recording_leaves_the_pixels_as_they_are() {
        let a = image(3, &[(0, &[0x41, 0, 0x42]), (1, &[0x43, 0x44])]);
        let b = image(2, &[(0, &[0x45, 0x46]), (0, &[0x47])]);
        let draw = |fb: &mut Framebuffer| {
            fb.blit_model_image_as(
                &a,
                Some(model_image("synth", 0)),
                -1,
                0,
                false,
                &target(300, (5, 3)),
            );
            fb.blit_model_image_as(
                &b,
                Some(model_image("synth", 1)),
                0,
                1,
                true,
                &target(300, (5, 3)),
            );
        };
        let mut plain = Framebuffer::with_surface(Size::new(10, 8), Size::new(14, 10)).unwrap();
        plain.set_origin(2, 1).unwrap();
        let mut recorded = plain.clone();
        recorded.set_recording(true);
        draw(&mut plain);
        draw(&mut recorded);
        assert_eq!(recorded.pixels(), plain.pixels());
        // At 300/256 unit 0 covers 2 pixels and units -1, 1 and 2 one each, so A's
        // units -1..=1 by 0..=1 cover 4 by 3 from (5 - 1, 3), and B's 0..=1 by 1..=2
        // cover 3 by 2 from (5, 3 + 2).
        let blits = recorded.recording().unwrap().blits();
        let rects: Vec<_> = blits.iter().map(|b| (b.rect, b.mirrored)).collect();
        assert_eq!(
            rects,
            [
                (Rect::new(4, 3, 4, 3), false),
                (Rect::new(5, 5, 3, 2), true)
            ]
        );
        assert!(plain.recording().is_none());
    }

    #[test]
    fn clearing_and_filling_disown_pixels() {
        let a = image(2, &[(0, &[0x21, 0x21])]);
        let mut fb = Framebuffer::new(Size::new(2, 1));
        fb.set_recording(true);
        fb.blit_model_image_as(
            &a,
            Some(model_image("synth", 0)),
            0,
            0,
            false,
            &target(256, (0, 0)),
        );
        let r = fb.recording().unwrap();
        assert_eq!(
            (r.owner(0, 0x21), r.owner(1, 0x21), r.owner(1, 0x22)),
            (Some(0), Some(0), None)
        );
        fb.fill_rect(Rect::new(1, 0, 1, 1), 0x21);
        assert_eq!(fb.recording().unwrap().owner(1, 0x21), None);
        fb.clear(0x21);
        assert_eq!(fb.recording().unwrap().owner(0, 0x21), None);
    }
}
