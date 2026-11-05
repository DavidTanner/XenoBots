//! The edge-aware scaler (T-071, `--filter xbrz`): the view's colours scaled by xBRZ
//! (the `xbrz-rs` port of xBRZ 1.8) at `k = min(sx, sy, 6)`, then repeated, nearest
//! neighbour, to the same [`Layout`] picture `present` fills, so the picture, its bars
//! and the mouse mapping are the same as `nearest`'s. The scaled image is kept until
//! the view's pixels, the palette or `k` change. See `docs/formats/video-modes.md`,
//! "Filters".

use std::str::FromStr;

use xeno_formats::Palette;

use crate::{Framebuffer, Layout, Size, present};

/// The upscaling filter (`--filter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    /// Whole-pixel repeats ([`present`]), pixel-exact.
    #[default]
    Nearest,
    /// xBRZ, then whole-pixel repeats ([`Xbrz`]).
    Xbrz,
}

impl FromStr for Filter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "nearest" => Ok(Filter::Nearest),
            "xbrz" => Ok(Filter::Xbrz),
            _ => Err(format!("expected nearest or xbrz, not {s:?}")),
        }
    }
}

/// The largest factor xBRZ scales by.
pub const MAX_XBRZ: usize = 6;

/// The xBRZ presenter and its last scaled view.
#[derive(Debug, Clone, Default)]
pub struct Xbrz {
    /// What `scaled` was made from: the view's indices, its colours and `k`.
    indices: Vec<u8>,
    colours: Vec<u32>,
    view: Option<Size>,
    k: usize,
    /// The view scaled `k` times by xBRZ, `0x00RRGGBB`.
    scaled: Vec<u32>,
}

impl Xbrz {
    /// Draws `fb`'s view as [`present`] would, but smoothed: the same picture rectangle
    /// and black bars, filled from the xBRZ image. With a factor of 1 on either axis it
    /// is [`present`].
    pub fn present(
        &mut self,
        fb: &Framebuffer,
        palette: &Palette,
        layout: &Layout,
        dst: &mut [u32],
        dst_size: Size,
    ) {
        let Layout { sx, sy, x, y } = *layout;
        let k = sx.min(sy).min(MAX_XBRZ);
        if k < 2 {
            return present(fb, palette, layout, dst, dst_size);
        }
        assert_eq!(dst.len(), dst_size.width * dst_size.height);
        let view = fb.view();
        let colours: Vec<u32> = (0..=255u8)
            .map(|i| {
                let c = palette[i];
                (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
            })
            .collect();
        let indices: Vec<u8> = (0..view.height)
            .flat_map(|row| {
                let start = fb.view_row_start(row);
                fb.pixels()[start..start + view.width].iter().copied()
            })
            .collect();
        if (&indices, &colours, Some(view), k) != (&self.indices, &self.colours, self.view, self.k)
        {
            let rgba: Vec<u8> = indices
                .iter()
                .flat_map(|&i| {
                    let [_, r, g, b] = colours[usize::from(i)].to_be_bytes();
                    [r, g, b, 0xff]
                })
                .collect();
            let out = xbrz::scale_rgba(&rgba, view.width, view.height, k);
            self.scaled = out
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_be_bytes([0, p[0], p[1], p[2]]))
                .collect();
            (self.indices, self.colours, self.view, self.k) = (indices, colours, Some(view), k);
        }
        dst.fill(0);
        let width = dst_size.width;
        let scaled_width = view.width * k;
        // For each window column on the picture, its column of the scaled image.
        let columns: Vec<(usize, usize)> = (0..view.width * sx)
            .filter_map(|px| {
                let wx = x + px as isize;
                (0..width as isize)
                    .contains(&wx)
                    .then_some((wx as usize, px * k / sx))
            })
            .collect();
        let mut last: Option<(usize, usize)> = None; // (scaled row, window row)
        for py in 0..view.height * sy {
            let wy = y + py as isize;
            if !(0..dst_size.height as isize).contains(&wy) {
                continue;
            }
            let (row, wy) = (py * k / sy, wy as usize);
            match last {
                Some((r, from)) if r == row => {
                    dst.copy_within(from * width..(from + 1) * width, wy * width)
                }
                _ => {
                    let src = &self.scaled[row * scaled_width..(row + 1) * scaled_width];
                    let line = &mut dst[wy * width..(wy + 1) * width];
                    for &(wx, col) in &columns {
                        line[wx] = src[col];
                    }
                    last = Some((row, wy));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scale;
    use crate::present::Aspect;
    use xeno_formats::Rgb;

    /// Index `i` is `(3i, 255 - i, 7)`: never black, all distinct.
    fn palette() -> Palette {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = Rgb {
                r: (3 * i) as u8,
                g: 255 - i as u8,
                b: 7,
            };
        }
        Palette { colors }
    }

    fn word(i: u8) -> u32 {
        let c = palette()[i];
        (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
    }

    fn run(fb: &Framebuffer, layout: Layout, dst_size: Size) -> Vec<u32> {
        let mut dst = vec![0x0bad_cafe; dst_size.width * dst_size.height];
        Xbrz::default().present(fb, &palette(), &layout, &mut dst, dst_size);
        dst
    }

    #[test]
    fn filters_parse() {
        assert_eq!("nearest".parse(), Ok(Filter::Nearest));
        assert_eq!("xbrz".parse(), Ok(Filter::Xbrz));
        assert!("xBRZ".parse::<Filter>().is_err());
        assert_eq!(Filter::default(), Filter::Nearest);
    }

    #[test]
    fn a_flat_view_stays_flat_inside_black_bars() {
        let mut fb = Framebuffer::new(Size::new(12, 10));
        fb.pixels_mut().fill(41);
        let window = Size::new(100, 80);
        let layout = Layout::new(fb.view(), window, Scale::Auto, Aspect::Correct);
        assert_eq!((layout.sx, layout.sy, layout.x, layout.y), (8, 7, 2, 5));
        let out = run(&fb, layout, window);
        for (i, &px) in out.iter().enumerate() {
            let (wx, wy) = (i % 100, i / 100);
            let inside = (2..98).contains(&wx) && (5..75).contains(&wy);
            assert_eq!(px, if inside { word(41) } else { 0 }, "({wx}, {wy})");
        }
    }

    #[test]
    fn a_diagonal_edge_is_smoothed() {
        // Upper-left triangle 3, the rest 200: nearest gives a staircase of two colours.
        let size = Size::new(16, 16);
        let mut fb = Framebuffer::new(size);
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            *p = if i % 16 + i / 16 < 16 { 3 } else { 200 };
        }
        let layout = Layout {
            sx: 6,
            sy: 6,
            x: 0,
            y: 0,
        };
        let window = Size::new(96, 96);
        let out = run(&fb, layout, window);
        let blended = out
            .iter()
            .filter(|&&px| px != word(3) && px != word(200))
            .count();
        assert!(blended > 0, "no blended pixel on the edge");
        // Away from the edge the colours are untouched.
        assert_eq!(out[0], word(3));
        assert_eq!(out[96 * 96 - 1], word(200));
        let mut nearest = vec![0; 96 * 96];
        present(&fb, &palette(), &layout, &mut nearest, window);
        assert_ne!(out, nearest);
    }

    #[test]
    fn uneven_factors_fill_the_layouts_picture_and_crop() {
        let mut fb = Framebuffer::new(Size::new(4, 3));
        fb.pixels_mut().fill(9);
        // 8x10 as at 4K: xBRZ 6x, stretched to 32x30 at (2, 1).
        let layout = Layout {
            sx: 8,
            sy: 10,
            x: 2,
            y: 1,
        };
        let out = run(&fb, layout, Size::new(36, 32));
        let lit = out.iter().filter(|&&px| px == word(9)).count();
        assert_eq!(lit, 32 * 30);
        assert_eq!(out.iter().filter(|&&px| px == 0).count(), 36 * 32 - 32 * 30);
        assert_eq!(
            (out[36 + 1], out[36 + 2], out[31 * 36 + 2]),
            (0, word(9), 0)
        );
        // Larger than the window: the middle, no bars.
        let cropped = Layout {
            sx: 8,
            sy: 10,
            x: -3,
            y: -4,
        };
        assert!(
            run(&fb, cropped, Size::new(20, 20))
                .iter()
                .all(|&px| px == word(9))
        );
        assert!(run(&fb, layout, Size::new(0, 0)).is_empty());
    }

    #[test]
    fn factor_one_is_nearest_and_a_change_rescales() {
        let mut fb = Framebuffer::new(Size::new(4, 4));
        fb.pixels_mut()
            .iter_mut()
            .enumerate()
            .for_each(|(i, p)| *p = i as u8);
        let one = Layout {
            sx: 1,
            sy: 2,
            x: 1,
            y: 0,
        };
        let window = Size::new(6, 8);
        let mut nearest = vec![0; 48];
        present(&fb, &palette(), &one, &mut nearest, window);
        assert_eq!(run(&fb, one, window), nearest);
        // The cache follows the pixels.
        let two = Layout {
            sx: 2,
            sy: 2,
            x: 0,
            y: 0,
        };
        let mut xbrz = Xbrz::default();
        let mut dst = vec![0; 64];
        fb.pixels_mut().fill(5);
        xbrz.present(&fb, &palette(), &two, &mut dst, Size::new(8, 8));
        assert!(dst.iter().all(|&px| px == word(5)));
        fb.pixels_mut().fill(6);
        xbrz.present(&fb, &palette(), &two, &mut dst, Size::new(8, 8));
        assert!(dst.iter().all(|&px| px == word(6)));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_screens_smooth_at_4k_inside_the_nearest_picture() {
        use xeno_formats::mal;
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let window = Size::new(3840, 2160);
        for name in ["login.m@l", "scorpcon.m@l"] {
            let path = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.file_name().unwrap().to_string_lossy().to_lowercase() == name)
                .unwrap();
            let screen = mal::parse(&std::fs::read(path).unwrap()).unwrap();
            let mut fb = Framebuffer::new(Size::MODE_13H);
            fb.pixels_mut().copy_from_slice(&screen.pixels);
            let layout = Layout::new(fb.view(), window, Scale::Auto, Aspect::Correct);
            let mut nearest = vec![0; window.width * window.height];
            present(&fb, &screen.palette, &layout, &mut nearest, window);
            let mut smooth = vec![0; nearest.len()];
            Xbrz::default().present(&fb, &screen.palette, &layout, &mut smooth, window);
            // The same bars, and inside them more colours than the palette has.
            let picture = layout.picture(fb.view());
            let on = |i: usize| {
                let (wx, wy) = (
                    (i % 3840) as isize - layout.x,
                    (i / 3840) as isize - layout.y,
                );
                (0..picture.width as isize).contains(&wx)
                    && (0..picture.height as isize).contains(&wy)
            };
            let differ = (0..nearest.len())
                .filter(|&i| nearest[i] != smooth[i])
                .count();
            assert!(
                (0..nearest.len()).all(|i| on(i) || smooth[i] == 0),
                "{name}"
            );
            assert!(differ > 0 && differ < nearest.len() / 2, "{name}: {differ}");
        }
    }
}
