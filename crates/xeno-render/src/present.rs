//! Presentation: scaling the framebuffer's view into a window's pixels with whole-pixel
//! nearest-neighbour repeats, centred, with black bars. Nothing here knows about a
//! window system; the caller hands over a `0x00RRGGBB` buffer (what `softbuffer`
//! wants). The policy and its numbers are in `docs/formats/video-modes.md`,
//! "Presentation".

use std::str::FromStr;

use xeno_formats::Palette;

use crate::{Framebuffer, Size};

/// The largest forced scale `--scale` accepts (320 * 32 = 10,240 pixels across).
pub const MAX_SCALE: usize = 32;

/// How far `Aspect::Correct` lets the shape of a pixel drift from the target, as a
/// fraction (1/20 = 5%).
const TOLERANCE: (u128, u128) = (1, 20);

/// Pixel shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Aspect {
    /// Fill a 4:3 frame, as the original monitor did (320x200 pixels 1.2 times taller
    /// than wide), allowing up to 5% error for a larger picture.
    #[default]
    Correct,
    /// Fill a 4:3 frame with the smallest aspect error, even if the picture is smaller.
    Exact,
    /// Square pixels.
    Off,
}

impl FromStr for Aspect {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "correct" => Ok(Aspect::Correct),
            "exact" => Ok(Aspect::Exact),
            "off" => Ok(Aspect::Off),
            _ => Err(format!("expected correct, exact or off, not {s:?}")),
        }
    }
}

/// Picture size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scale {
    /// The largest that fits the window.
    #[default]
    Auto,
    /// This horizontal factor (1..=[`MAX_SCALE`]); the vertical one follows the aspect.
    /// The picture is cropped, centred, if the window is too small.
    Fixed(usize),
}

impl FromStr for Scale {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if s == "auto" {
            return Ok(Scale::Auto);
        }
        match s.parse::<usize>() {
            Ok(n @ 1..=MAX_SCALE) => Ok(Scale::Fixed(n)),
            _ => Err(format!("expected auto or 1 to {MAX_SCALE}, not {s:?}")),
        }
    }
}

/// Where the picture lands in the window: each view pixel becomes `sx` by `sy` window
/// pixels, with the picture's top-left at (`x`, `y`). Negative when the picture is
/// larger than the window, which then shows its middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub sx: usize,
    pub sy: usize,
    pub x: isize,
    pub y: isize,
}

impl Layout {
    /// Lays out `view` in a `window` (both in pixels).
    pub fn new(view: Size, window: Size, scale: Scale, aspect: Aspect) -> Self {
        let (num, den) = target(view, aspect);
        // The vertical factor nearest `sx * num / den` (never a tie: see the doc).
        let vertical = |sx: usize| ((2 * sx * num + den) / (2 * den)).max(1);
        let (sx, sy) = match scale {
            Scale::Fixed(n) => (n.max(1), vertical(n.max(1))),
            Scale::Auto => {
                let max_sx = (window.width / view.width.max(1)).max(1);
                let max_sy = (window.height / view.height.max(1)).max(1);
                let error = |sx: usize, sy: usize| {
                    let (a, b) = ((sy * den) as u128, (sx * num) as u128);
                    (a.abs_diff(b), b)
                };
                let within = |(e, of): (u128, u128)| e * TOLERANCE.1 <= of * TOLERANCE.0;
                let largest_within = (1..=max_sx).rev().find_map(|sx| {
                    let sy = vertical(sx);
                    (aspect == Aspect::Correct && sy <= max_sy && within(error(sx, sy)))
                        .then_some((sx, sy))
                });
                // Otherwise the smallest error, ties to the larger picture.
                largest_within.unwrap_or_else(|| {
                    (1..=max_sx)
                        .map(|sx| (sx, vertical(sx).min(max_sy)))
                        .min_by(|&(ax, ay), &(bx, by)| {
                            let ((ea, oa), (eb, ob)) = (error(ax, ay), error(bx, by));
                            (ea * ob).cmp(&(eb * oa)).then(bx.cmp(&ax))
                        })
                        .unwrap_or((1, 1))
                })
            }
        };
        let offset = |window: usize, picture: usize| (window as isize - picture as isize) / 2;
        Layout {
            sx,
            sy,
            x: offset(window.width, view.width * sx),
            y: offset(window.height, view.height * sy),
        }
    }

    /// The scaled picture's size, before any cropping by the window.
    pub fn picture(&self, view: Size) -> Size {
        Size::new(view.width * self.sx, view.height * self.sy)
    }
}

/// The pixel shape that fills the frame, as height over width: `num / den`. A 4:3 frame
/// of `w` by `h` pixels needs pixels `3w / 4h` times taller than wide.
fn target(view: Size, aspect: Aspect) -> (usize, usize) {
    if aspect == Aspect::Off || view.width == 0 || view.height == 0 {
        return (1, 1);
    }
    let (num, den) = (3 * view.width, 4 * view.height);
    let gcd = (1..=num.min(den))
        .rev()
        .find(|d| num % d == 0 && den % d == 0)
        .unwrap_or(1);
    (num / gcd, den / gcd)
}

/// Draws `fb`'s view, coloured by `palette`, into `dst` (`dst_size`, one `0x00RRGGBB`
/// word per pixel) as `layout` says, with black around it. Only whole source pixels are
/// repeated, so every output pixel is black or exactly a palette colour.
pub fn present(
    fb: &Framebuffer,
    palette: &Palette,
    layout: &Layout,
    dst: &mut [u32],
    dst_size: Size,
) {
    assert_eq!(dst.len(), dst_size.width * dst_size.height);
    dst.fill(0);
    let colours: [u32; 256] = std::array::from_fn(|i| {
        let c = palette[i as u8];
        (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
    });
    // The part of `len` window pixels from `start` that lies inside `0..max`.
    let clip = |start: isize, len: usize, max: usize| {
        let max = max as isize;
        start.clamp(0, max) as usize..(start + len as isize).clamp(0, max) as usize
    };
    let view = fb.view();
    let Layout { sx, sy, x, y } = *layout;
    let width = dst_size.width;
    for row in 0..view.height {
        let rows = clip(y + (row * sy) as isize, sy, dst_size.height);
        if rows.is_empty() {
            continue;
        }
        let first = rows.start * width;
        let line = &mut dst[first..first + width];
        let start = fb.view_row_start(row);
        for (col, &index) in fb.pixels()[start..start + view.width].iter().enumerate() {
            line[clip(x + (col * sx) as isize, sx, width)].fill(colours[usize::from(index)]);
        }
        for copy in rows.start + 1..rows.end {
            dst.copy_within(first..first + width, copy * width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use xeno_formats::Rgb;

    const MODES: [Size; 2] = [Size::MODE_13H, Size::MODE_240];
    const WINDOWS: [(usize, usize); 5] = [
        (640, 400),
        (1280, 720),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
    ];

    fn auto(view: Size, w: usize, h: usize, aspect: Aspect) -> (usize, usize, isize, isize) {
        let l = Layout::new(view, Size::new(w, h), Scale::Auto, aspect);
        (l.sx, l.sy, l.x, l.y)
    }

    #[test]
    fn mode_13h_is_corrected_within_five_percent_where_it_fits() {
        let m = Size::MODE_13H;
        let c = Aspect::Correct;
        assert_eq!(auto(m, 640, 400, c), (2, 2, 0, 0));
        assert_eq!(auto(m, 1280, 720, c), (3, 3, 160, 60));
        assert_eq!(auto(m, 1920, 1080, c), (4, 5, 320, 40));
        assert_eq!(auto(m, 2560, 1440, c), (6, 7, 320, 20));
        assert_eq!(auto(m, 3840, 2160, c), (8, 10, 640, 80));
        assert_eq!(auto(m, 1600, 1200, c), (5, 6, 0, 0));
    }

    #[test]
    fn exact_prefers_the_aspect_and_off_squares_the_pixels() {
        let m = Size::MODE_13H;
        assert_eq!(auto(m, 1920, 1080, Aspect::Exact), (4, 5, 320, 40));
        assert_eq!(auto(m, 2560, 1440, Aspect::Exact), (5, 6, 480, 120));
        assert_eq!(auto(m, 3840, 2160, Aspect::Exact), (5, 6, 1120, 480));
        assert_eq!(auto(m, 1920, 1080, Aspect::Off), (5, 5, 160, 40));
        assert_eq!(auto(m, 3840, 2160, Aspect::Off), (10, 10, 320, 80));
    }

    #[test]
    fn mode_240_is_square_in_every_aspect() {
        for aspect in [Aspect::Correct, Aspect::Exact, Aspect::Off] {
            for ((w, h), k) in WINDOWS.into_iter().zip([1, 3, 4, 6, 9]) {
                let l = auto(Size::MODE_240, w, h, aspect);
                assert_eq!((l.0, l.1), (k, k), "{w}x{h} {aspect:?}");
            }
        }
    }

    #[test]
    fn small_windows_crop_the_middle_of_a_1x_picture() {
        let m = Size::MODE_13H;
        assert_eq!(auto(m, 0, 0, Aspect::Correct), (1, 1, -160, -100));
        assert_eq!(auto(m, 300, 199, Aspect::Correct), (1, 1, -10, 0));
        assert_eq!(auto(Size::new(0, 0), 10, 10, Aspect::Correct).0, 10);
    }

    #[test]
    fn fixed_scale_sets_the_horizontal_factor_and_may_crop() {
        let fixed = |n, aspect| {
            let l = Layout::new(
                Size::MODE_13H,
                Size::new(1920, 1080),
                Scale::Fixed(n),
                aspect,
            );
            (l.sx, l.sy, l.x, l.y)
        };
        assert_eq!(fixed(1, Aspect::Correct), (1, 1, 800, 440));
        assert_eq!(fixed(3, Aspect::Correct), (3, 4, 480, 140));
        assert_eq!(fixed(5, Aspect::Exact), (5, 6, 160, -60));
        assert_eq!(fixed(9, Aspect::Correct), (9, 11, -480, -560));
        assert_eq!(fixed(4, Aspect::Off), (4, 4, 320, 140));
    }

    #[test]
    fn scale_and_aspect_parse() {
        assert_eq!("auto".parse(), Ok(Scale::Auto));
        assert_eq!("3".parse(), Ok(Scale::Fixed(3)));
        assert_eq!("32".parse(), Ok(Scale::Fixed(MAX_SCALE)));
        for bad in ["0", "33", "-1", "2.5", "", "Auto"] {
            assert!(bad.parse::<Scale>().is_err(), "{bad}");
        }
        assert_eq!("correct".parse(), Ok(Aspect::Correct));
        assert_eq!("exact".parse(), Ok(Aspect::Exact));
        assert_eq!("off".parse(), Ok(Aspect::Off));
        assert!("4:3".parse::<Aspect>().is_err());
    }

    #[test]
    fn layout_table() {
        let mut out = String::from("mode     window     aspect   sx x sy  picture    vs 4:3\n");
        for view in MODES {
            for (w, h) in [(800, 600), (1600, 1200)].into_iter().chain(WINDOWS) {
                for aspect in [Aspect::Correct, Aspect::Exact, Aspect::Off] {
                    let l = Layout::new(view, Size::new(w, h), Scale::Auto, aspect);
                    let p = l.picture(view);
                    let (num, den) = target(view, Aspect::Correct);
                    let error = (l.sy * den) as f64 / (l.sx * num) as f64 - 1.0;
                    out += &format!(
                        "{:<8} {:<10} {:<8} {:>2} x {:<2}  {:<10} {:+.1}%\n",
                        format!("{}x{}", view.width, view.height),
                        format!("{w}x{h}"),
                        format!("{aspect:?}"),
                        l.sx,
                        l.sy,
                        format!("{}x{}", p.width, p.height),
                        error * 100.0
                    );
                }
            }
        }
        insta::assert_snapshot!(out);
    }

    /// A palette where index `i` is `(i, 255 - i, 1)`: never black, all distinct.
    fn ramp() -> Palette {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = Rgb {
                r: i as u8,
                g: 255 - i as u8,
                b: 1,
            };
        }
        Palette { colors }
    }

    fn word(i: u8) -> u32 {
        (u32::from(i) << 16) | (u32::from(255 - i) << 8) | 1
    }

    fn run(fb: &Framebuffer, layout: Layout, dst_size: Size) -> Vec<u32> {
        let mut dst = vec![0x1234_5678; dst_size.width * dst_size.height];
        present(fb, &ramp(), &layout, &mut dst, dst_size);
        dst
    }

    #[test]
    fn repeats_whole_pixels_of_the_view_with_black_bars() {
        // A 5x3 surface showing the 2x2 view at (3, 1): indices 8, 9 / 13, 14.
        let mut fb = Framebuffer::with_surface(Size::new(2, 2), Size::new(5, 3)).unwrap();
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            *p = i as u8;
        }
        fb.set_origin(3, 1).unwrap();
        let layout = Layout {
            sx: 2,
            sy: 1,
            x: 1,
            y: 1,
        };
        let [a, b, c, d] = [8, 9, 13, 14].map(word);
        #[rustfmt::skip]
        assert_eq!(run(&fb, layout, Size::new(6, 4)), [
            0, 0, 0, 0, 0, 0,
            0, a, a, b, b, 0,
            0, c, c, d, d, 0,
            0, 0, 0, 0, 0, 0,
        ]);
        let cropped = Layout {
            sx: 3,
            sy: 2,
            x: -2,
            y: -1,
        };
        #[rustfmt::skip]
        assert_eq!(run(&fb, cropped, Size::new(3, 3)), [
            a, b, b,
            c, d, d,
            c, d, d,
        ]);
        assert_eq!(run(&fb, layout, Size::new(0, 0)), []);
    }

    proptest! {
        #[test]
        fn auto_fits_is_centred_and_never_blends(
            w in 0usize..900,
            h in 0usize..700,
            tall in any::<bool>(),
            aspect in proptest::sample::select(vec![Aspect::Correct, Aspect::Exact, Aspect::Off]),
            pixels in proptest::collection::vec(any::<u8>(), 32 * 24),
        ) {
            let view = Size::new(32, if tall { 24 } else { 20 });
            let window = Size::new(w, h);
            let l = Layout::new(view, window, Scale::Auto, aspect);
            let p = l.picture(view);
            if w >= view.width && h >= view.height {
                prop_assert!(p.width <= w && p.height <= h);
            } else {
                prop_assert_eq!((l.sx, l.sy), (1, 1));
            }
            prop_assert!((w as isize - p.width as isize - 2 * l.x).abs() <= 1);
            prop_assert!((h as isize - p.height as isize - 2 * l.y).abs() <= 1);
            let mut fb = Framebuffer::new(view);
            let n = view.width * view.height;
            fb.pixels_mut().copy_from_slice(&pixels[..n]);
            let out = run(&fb, l, window);
            for (i, &px) in out.iter().enumerate() {
                let (ox, oy) = ((i % w) as isize - l.x, (i / w) as isize - l.y);
                let inside = (0..p.width as isize).contains(&ox) && (0..p.height as isize).contains(&oy);
                let expected = if inside {
                    word(fb.get(ox as usize / l.sx, oy as usize / l.sy).unwrap())
                } else {
                    0
                };
                prop_assert_eq!(px, expected);
            }
        }
    }
}
