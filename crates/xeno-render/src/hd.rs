//! Resolution-independent drawing (T-070): names for the original's images, a record
//! of where each blit put one, and a compositor that draws a higher-resolution image
//! over a recorded blit's pixels at output resolution. The indexed framebuffer stays
//! the source of truth; see `docs/hd.md`.

use std::collections::HashMap;

use xeno_formats::Palette;

use crate::{Framebuffer, Layout, Rect, Size, present};

mod shapes;

pub use shapes::draw_shapes;

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

/// One drawing the record keeps: a named blit, or a shape drawn at output resolution
/// by [`draw_shapes`] (T-236).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Blit(BlitRecord),
    Shape(ShapeRecord),
}

/// A line or fill primitive's logical geometry, in surface pixels (a corner or an end
/// is the pixel the original lights there), its colour, and the rectangle of surface
/// pixels it was clipped to (its buffer, moved along by [`Framebuffer::copy_through`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeRecord {
    pub shape: Geometry,
    pub colour: u8,
    pub clip: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Geometry {
    /// `shape::fill_polygon`'s corners.
    Polygon(Vec<(i32, i32)>),
    /// A sloping line of slot `0x558`, both ends drawn.
    Line([(i32, i32); 2]),
}

impl Geometry {
    fn moved(&self, (dx, dy): (i32, i32)) -> Geometry {
        let at = |(x, y): (i32, i32)| (x + dx, y + dy);
        match self {
            Geometry::Polygon(corners) => {
                Geometry::Polygon(corners.iter().copied().map(at).collect())
            }
            Geometry::Line(ends) => Geometry::Line(ends.map(at)),
        }
    }
}

/// The most records one recording keeps; drawings past it are not recorded.
const MAX_RECORDS: usize = (1 << 22) - 1;

/// The drawings since recording started, and for each surface pixel what last wrote
/// it and the colour it wrote: `stamp << 8 | colour`. Stamp 0 is unknown (a write the
/// record does not see), `2k + 2` record `k`, and `2n + 1` a background (a clear, a
/// fill, the 3D view's ground and sky) drawn after `n` records, so stamps order
/// everything the record saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    records: Vec<Record>,
    pub(crate) owners: Vec<u32>,
}

impl Recording {
    pub(crate) fn new(pixels: usize) -> Recording {
        Recording {
            records: Vec::new(),
            owners: vec![0; pixels],
        }
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// The blits among the records.
    pub fn blits(&self) -> impl Iterator<Item = &BlitRecord> {
        self.records.iter().filter_map(|r| match r {
            Record::Blit(b) => Some(b),
            Record::Shape(_) => None,
        })
    }

    /// Adds a record and returns its stamp (to be or-ed with a colour), or `None` when
    /// the stamps have run out.
    pub(crate) fn push(&mut self, record: Record) -> Option<u32> {
        let k = self.records.len();
        (k < MAX_RECORDS).then(|| {
            self.records.push(record);
            (2 * k as u32 + 2) << 8
        })
    }

    /// The stamp of a background drawn now.
    pub(crate) fn background(&self) -> u32 {
        (2 * self.records.len() as u32 + 1) << 8
    }

    /// Surface pixel `i`'s stamp, given its colour now: 0 when unknown, or when a
    /// later write of another colour (text, a sprite, a copied picture) took it.
    // shortcut: a later write of the same colour is not seen; tag every write if that shows.
    pub fn stamp(&self, i: usize, colour: u8) -> u32 {
        let owner = self.owners.get(i).copied().unwrap_or(0);
        if owner as u8 == colour { owner >> 8 } else { 0 }
    }

    /// The record whose pixel surface pixel `i` still shows, given its colour now.
    pub fn owner(&self, i: usize, colour: u8) -> Option<usize> {
        let stamp = self.stamp(i, colour);
        (stamp != 0 && stamp.is_multiple_of(2)).then(|| stamp as usize / 2 - 1)
    }

    /// `other`'s records added after these, moved by `offset`; returns what to add to
    /// a stamp of `other`'s, or `None` (nothing added) when they do not fit.
    pub(crate) fn append(&mut self, other: &Recording, (dx, dy): (i32, i32)) -> Option<u32> {
        let base = self.records.len();
        (base + other.records.len() <= MAX_RECORDS).then(|| {
            self.records.extend(other.records.iter().map(|r| match r {
                Record::Blit(b) => Record::Blit(BlitRecord {
                    rect: Rect::new(b.rect.x + dx, b.rect.y + dy, b.rect.width, b.rect.height),
                    ..*b
                }),
                Record::Shape(s) => Record::Shape(ShapeRecord {
                    shape: s.shape.moved((dx, dy)),
                    colour: s.colour,
                    clip: Rect::new(s.clip.x + dx, s.clip.y + dy, s.clip.width, s.clip.height),
                }),
            }));
            2 * base as u32
        })
    }
}

/// A replacement image: `width` by `height` `0xAARRGGBB` words. Texels with alpha
/// below 0x80 show the original pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdImage {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl HdImage {
    /// `None` unless there are exactly `width * height` pixels, and some.
    pub fn new(width: usize, height: usize, pixels: Vec<u32>) -> Option<HdImage> {
        (width > 0 && height > 0 && pixels.len() == width * height).then_some(HdImage {
            width,
            height,
            pixels,
        })
    }
}

/// [`present`], then each recorded blit with an image in `hd` drawn over the output
/// pixels of the surface pixels it still owns, stretched (nearest texel) from its
/// logical rectangle. With no recording or no matching image, this is [`present`].
pub fn compose(
    fb: &Framebuffer,
    palette: &Palette,
    layout: &Layout,
    hd: &HashMap<AssetId, HdImage>,
    dst: &mut [u32],
    dst_size: Size,
) {
    present(fb, palette, layout, dst, dst_size);
    let Some(recording) = fb.recording().filter(|_| !hd.is_empty()) else {
        return;
    };
    let images: Vec<Option<(&HdImage, &BlitRecord)>> = recording
        .records
        .iter()
        .map(|r| match r {
            Record::Blit(b) => hd.get(&b.asset).map(|image| (image, b)),
            Record::Shape(_) => None,
        })
        .collect();
    let (view, stride) = (fb.view(), fb.surface().width);
    let Layout { sx, sy, x, y } = *layout;
    let (sx, sy) = (sx as i64, sy as i64);
    // Texel along an axis: output pixel `at` of a `length`-pixel rectangle, scaled.
    let texel = |at: i64, length: u32, factor: i64, size: usize, turned: bool| {
        let t = (at * size as i64 / (i64::from(length) * factor)).clamp(0, size as i64 - 1);
        (if turned { size as i64 - 1 - t } else { t }) as usize
    };
    for vy in 0..view.height {
        let start = fb.view_row_start(vy);
        for vx in 0..view.width {
            let i = start + vx;
            let Some(k) = recording.owner(i, fb.pixels()[i]) else {
                continue;
            };
            let Some((image, blit)) = images[k] else {
                continue;
            };
            let r = blit.rect;
            let (px, py) = ((i % stride) as i64, (i / stride) as i64);
            for oy in 0..sy {
                let wy = y as i64 + vy as i64 * sy + oy;
                if !(0..dst_size.height as i64).contains(&wy) {
                    continue;
                }
                let ty = texel(
                    (py - i64::from(r.y)) * sy + oy,
                    r.height,
                    sy,
                    image.height,
                    blit.flipped,
                );
                for ox in 0..sx {
                    let wx = x as i64 + vx as i64 * sx + ox;
                    if !(0..dst_size.width as i64).contains(&wx) {
                        continue;
                    }
                    let tx = texel(
                        (px - i64::from(r.x)) * sx + ox,
                        r.width,
                        sx,
                        image.width,
                        blit.mirrored,
                    );
                    let t = image.pixels[ty * image.width + tx];
                    if t >> 24 >= 0x80 {
                        dst[wy as usize * dst_size.width + wx as usize] = t & 0x00ff_ffff;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelScale, ModelTarget};
    use xeno_formats::Rgb;
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

    /// Index `i` is `(i, 255 - i, 7)`: never black, all distinct.
    fn ramp() -> Palette {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = Rgb {
                r: i as u8,
                g: 255 - i as u8,
                b: 7,
            };
        }
        Palette { colors }
    }

    fn output(
        fb: &Framebuffer,
        layout: &Layout,
        hd: Option<&HashMap<AssetId, HdImage>>,
        size: Size,
    ) -> Vec<u32> {
        let mut dst = vec![0xdead_beef; size.width * size.height];
        match hd {
            Some(hd) => compose(fb, &ramp(), layout, hd, &mut dst, size),
            None => present(fb, &ramp(), layout, &mut dst, size),
        }
        dst
    }

    /// `image` as a `sx` by `sy` times larger HD image in `palette`'s colours, its
    /// uncovered and colour-0 pixels transparent.
    fn upscaled(image: &Image, palette: &Palette, sx: usize, sy: usize) -> HdImage {
        let (w, h) = (usize::from(image.width), usize::from(image.height));
        let texel = |x: usize, y: usize| {
            let span = &image.rows[y];
            let colour = x
                .checked_sub(span.skip.into())
                .and_then(|i| span.pixels.get(i))
                .copied()
                .unwrap_or(0);
            let c = palette[colour];
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            if colour == 0 { 0 } else { 0xff00_0000 | rgb }
        };
        let pixels = (0..h * sy)
            .flat_map(|ty| (0..w * sx).map(move |tx| (tx / sx, ty / sy)))
            .map(|(x, y)| texel(x, y))
            .collect();
        HdImage::new(w * sx, h * sy, pixels).unwrap()
    }

    #[test]
    fn recording_leaves_the_pixels_and_the_output_as_they_are() {
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
        let blits: Vec<_> = recorded.recording().unwrap().blits().collect();
        let rects: Vec<_> = blits.iter().map(|b| (b.rect, b.mirrored)).collect();
        assert_eq!(
            rects,
            [
                (Rect::new(4, 3, 4, 3), false),
                (Rect::new(5, 5, 3, 2), true)
            ]
        );
        assert!(plain.recording().is_none());
        // With no HD image for what was drawn, composing is presenting, byte for byte.
        let layout = Layout {
            sx: 3,
            sy: 4,
            x: 1,
            y: -2,
        };
        let size = Size::new(33, 30);
        let before = output(&plain, &layout, None, size);
        let elsewhere = HashMap::from([(model_image("other", 0), upscaled(&a, &ramp(), 3, 4))]);
        assert_eq!(
            output(&recorded, &layout, Some(&HashMap::new()), size),
            before
        );
        assert_eq!(output(&recorded, &layout, Some(&elsewhere), size), before);
        // Not recording: an image for a drawn asset changes nothing either.
        let drawn = HashMap::from([(model_image("synth", 0), upscaled(&a, &ramp(), 3, 4))]);
        assert_eq!(output(&plain, &layout, Some(&drawn), size), before);
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
        assert_eq!(HdImage::new(2, 2, vec![0; 3]), None);
    }

    #[test]
    fn a_copy_carries_the_records_along() {
        let a = image(2, &[(0, &[0x3c, 0x3c])]);
        let draw = |fb: &mut Framebuffer| {
            fb.clear(0x19);
            fb.blit_model_image_as(
                &a,
                Some(model_image("synth", 3)),
                0,
                0,
                false,
                &target(256, (1, 1)),
            );
            crate::shape::fill_polygon(fb, &[(2, 3), (3, 7), (8, 5)], 0x4e);
        };
        let mut plain = Framebuffer::new(Size::new(11, 9));
        draw(&mut plain);
        let mut buffer = plain.clone();
        buffer.set_recording(true);
        draw(&mut buffer);
        assert_eq!(buffer.pixels(), plain.pixels());
        let mut fb = Framebuffer::new(Size::new(17, 13));
        fb.set_recording(true);
        fb.fill_rect(Rect::new(4, 2, 11, 9), 0xff);
        fb.pixels_mut()[3 * 17 + 5] = 0x07;
        fb.copy_through(&buffer, (4, 2), 0xff);
        let r = fb.recording().unwrap();
        let moved = [
            Record::Blit(BlitRecord {
                asset: model_image("synth", 3),
                rect: Rect::new(5, 3, 2, 1),
                scale: 256,
                mirrored: false,
                flipped: false,
            }),
            Record::Shape(ShapeRecord {
                shape: Geometry::Polygon(vec![(6, 5), (7, 9), (12, 7)]),
                colour: 0x4e,
                clip: Rect::new(4, 2, 11, 9),
            }),
        ];
        assert_eq!(r.records(), moved);
        // The blit's pixel under the region's hole stays the hole's, unowned.
        let owner = |x: usize, y: usize| r.owner(y * 17 + x, fb.get(x, y).unwrap());
        assert_eq!((owner(5, 3), owner(6, 3)), (None, Some(0)));
        assert_eq!(
            (owner(6, 5), owner(9, 6), owner(4, 2)),
            (Some(1), Some(1), None)
        );
        // The background under both is older than either: stamp 1 (after no record).
        assert_eq!(r.stamp(2 * 17 + 4, 0x19), 1);
        // A buffer with no record copies in unknown pixels.
        fb.copy_through(&plain, (4, 2), 0x19);
        assert_eq!(fb.recording().unwrap().stamp(2 * 17 + 4, 0x19), 0);
    }

    #[test]
    fn an_upscaled_copy_of_the_original_reproduces_it() {
        let a = image(
            3,
            &[(0, &[0x51, 0, 0x52]), (1, &[0x53, 0x54]), (0, &[0x55])],
        );
        let mut fb = Framebuffer::new(Size::new(8, 6));
        fb.set_recording(true);
        fb.blit_model_image_as(
            &a,
            Some(model_image("synth", 2)),
            0,
            0,
            true,
            &target(256, (2, 1)),
        );
        let layout = Layout {
            sx: 3,
            sy: 2,
            x: 2,
            y: 1,
        };
        let size = Size::new(27, 14);
        let hd = HashMap::from([(model_image("synth", 2), upscaled(&a, &ramp(), 3, 2))]);
        assert_eq!(
            output(&fb, &layout, Some(&hd), size),
            output(&fb, &layout, None, size)
        );
    }

    #[test]
    fn one_hd_image_replaces_exactly_its_blit_s_pixels() {
        // A covers (1, 1)..(5, 4) with a hole at (2, 2); B covers (3, 2)..(5, 4) on top;
        // text then writes over (1, 1).
        let a = image(
            4,
            &[
                (0, &[0x31; 4]),
                (0, &[0x31, 0, 0x31, 0x31]),
                (0, &[0x31; 4]),
            ],
        );
        let b = image(2, &[(0, &[0x52; 2]), (0, &[0x52; 2])]);
        let mut fb = Framebuffer::new(Size::new(8, 6));
        fb.set_recording(true);
        let t = target(256, (1, 1));
        fb.blit_model_image_as(&a, Some(model_image("synth", 0)), 0, 0, false, &t);
        fb.blit_model_image_as(&b, Some(model_image("synth", 1)), 2, 1, false, &t);
        fb.pixels_mut()[8 + 1] = 0x07;
        // Twice A's logical size: texel (x, y) is 0xff_00xxyy, but (2, 0) is clear.
        let texels = (0..6u32).flat_map(|y| (0..8u32).map(move |x| 0xff00_0000 | x << 8 | y));
        let mut texels: Vec<u32> = texels.collect();
        texels[2] = 0;
        let hd = HashMap::from([(model_image("synth", 0), HdImage::new(8, 6, texels).unwrap())]);
        let layout = Layout {
            sx: 2,
            sy: 2,
            x: 0,
            y: 0,
        };
        let size = Size::new(16, 12);
        let before = output(&fb, &layout, None, size);
        let after = output(&fb, &layout, Some(&hd), size);
        let owned = |x: usize, y: usize| {
            (1..5).contains(&x)
                && (1..4).contains(&y)
                && (x, y) != (2, 2)
                && (x, y) != (1, 1)
                && !((3..5).contains(&x) && (2..4).contains(&y))
        };
        let mut replaced = 0;
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            let (ox, oy) = (i % 16, i / 16);
            let (tx, ty) = ((ox as u32).wrapping_sub(2), (oy as u32).wrapping_sub(2));
            if owned(ox / 2, oy / 2) && (tx, ty) != (2, 0) {
                assert_eq!(now, tx << 8 | ty, "({ox}, {oy})");
                replaced += 1;
            } else {
                assert_eq!(now, was, "({ox}, {oy})");
            }
        }
        // A's 12 logical pixels less the hole, the text and B's 4: 6 pixels, four
        // output pixels each, less the clear texel.
        assert_eq!(replaced, 6 * 4 - 1);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_a_console_with_robots_composes_as_it_presents() {
        use xeno_formats::{mal, mxx};
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let read = |name: &str| {
            let entry = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.file_name().unwrap().to_string_lossy().to_lowercase() == name);
            std::fs::read(entry.unwrap()).unwrap()
        };
        let console = mal::parse(&read("scorpcon.m@l")).unwrap();
        let model = |name: &str| mxx::parse(&read(&format!("scorpion.{name}"))).unwrap();
        let mut scorpion = crate::LoadedModel::new(&model("m00"), &[model("m08"), model("m16")]);
        scorpion.name = Name::new("scorpion");
        let draw = |fb: &mut Framebuffer, scale: u16| {
            fb.pixels_mut().copy_from_slice(&console.pixels);
            for (i, facing) in (0..64).step_by(6).enumerate() {
                let anchor = (30 + 25 * i as i32 % 280, 60 + 40 * (i as i32 % 3));
                let t = ModelTarget {
                    clip: Rect::new(8, 8, 304, 150),
                    anchor,
                    scale: ModelScale::new(scale),
                };
                fb.draw_model(&scorpion, facing, &[0, 1, 2, 3], &t);
            }
        };
        let window = Size::new(1920, 1080);
        let palette = &console.palette;
        for scale in [96, 256, 700] {
            let mut plain = Framebuffer::new(Size::MODE_13H);
            draw(&mut plain, scale);
            let mut recorded = Framebuffer::new(Size::MODE_13H);
            recorded.set_recording(true);
            draw(&mut recorded, scale);
            assert_eq!(recorded.pixels(), plain.pixels(), "scale {scale}");
            let blits: Vec<_> = recorded.recording().unwrap().blits().collect();
            assert!(blits.len() > 20, "scale {scale}: {}", blits.len());
            let layout = Layout::new(
                plain.view(),
                window,
                crate::Scale::Auto,
                crate::Aspect::Correct,
            );
            let mut before = vec![0; window.width * window.height];
            present(&plain, palette, &layout, &mut before, window);
            let mut after = vec![1; before.len()];
            compose(
                &recorded,
                palette,
                &layout,
                &HashMap::new(),
                &mut after,
                window,
            );
            assert!(after == before, "scale {scale}: no HD image");
            if scale == 256 {
                // Every image replaced by itself at 4x5 draws the same frame.
                let mut hd = HashMap::new();
                for b in blits {
                    let AssetId::ModelImage { view, image, .. } = b.asset else {
                        unreachable!()
                    };
                    let (v, i) = (view.unwrap(), usize::from(image));
                    let images = [model("m08"), model("m16")]
                        .into_iter()
                        .flat_map(|m| m.views)
                        .nth(v.into())
                        .unwrap()
                        .images;
                    hd.insert(b.asset, upscaled(&images[i], palette, layout.sx, layout.sy));
                }
                compose(&recorded, palette, &layout, &hd, &mut after, window);
                assert!(after == before, "every image replaced by itself");
            }
        }
    }
}
