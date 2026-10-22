//! Copies the logical framebuffer into the window's pixel buffer: the largest integer
//! nearest-neighbour scale that fits, centred, with black bars. No aspect correction
//! yet; T-032 replaces this with the real presentation scaler.

use xeno_render::Size;

/// Where the scaled picture lands in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub scale: usize,
    pub x: usize,
    pub y: usize,
}

impl Layout {
    /// The largest integer scale of `src` that fits `dst` (at least 1, so a window
    /// smaller than the picture shows its top-left part), centred.
    pub fn fit(src: Size, dst: Size) -> Self {
        let scale = (dst.width / src.width.max(1))
            .min(dst.height / src.height.max(1))
            .max(1);
        Layout {
            scale,
            x: dst.width.saturating_sub(src.width * scale) / 2,
            y: dst.height.saturating_sub(src.height * scale) / 2,
        }
    }
}

/// Fills `dst` (`dst_size`, one `0x00RRGGBB` word per pixel, as `softbuffer` wants)
/// from `rgba` (`src_size`, 4 bytes per pixel, as `Framebuffer::to_rgba` makes),
/// scaled by [`Layout::fit`] and letterboxed in black.
pub fn present(rgba: &[u8], src_size: Size, dst: &mut [u32], dst_size: Size) {
    assert_eq!(rgba.len(), src_size.width * src_size.height * 4);
    assert_eq!(dst.len(), dst_size.width * dst_size.height);
    dst.fill(0);
    let Layout { scale, x, y } = Layout::fit(src_size, dst_size);
    let width = (src_size.width * scale).min(dst_size.width - x);
    let height = (src_size.height * scale).min(dst_size.height - y);
    for row in 0..height {
        let src_row = row / scale * src_size.width;
        let start = (y + row) * dst_size.width + x;
        for (col, out) in dst[start..start + width].iter_mut().enumerate() {
            let i = (src_row + col / scale) * 4;
            let [r, g, b] = [rgba[i], rgba[i + 1], rgba[i + 2]].map(u32::from);
            *out = (r << 16) | (g << 8) | b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 RGBA picture: red, green / blue, white.
    const SRC: Size = Size::new(2, 2);
    const RGBA: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, //
        0, 0, 255, 255, 255, 255, 255, 255,
    ];
    const RED: u32 = 0xff0000;
    const GREEN: u32 = 0x00ff00;
    const BLUE: u32 = 0x0000ff;
    const WHITE: u32 = 0xffffff;

    fn run(dst_size: Size) -> Vec<u32> {
        let mut dst = vec![0x1234_5678; dst_size.width * dst_size.height];
        present(&RGBA, SRC, &mut dst, dst_size);
        dst
    }

    #[test]
    fn fit_picks_the_largest_integer_scale_and_centres() {
        let mode13 = Size::MODE_13H;
        let at = |w, h| {
            let l = Layout::fit(mode13, Size::new(w, h));
            (l.scale, l.x, l.y)
        };
        assert_eq!(at(640, 400), (2, 0, 0));
        assert_eq!(at(1000, 700), (3, 20, 50));
        assert_eq!(at(3840, 2160), (10, 320, 80));
        assert_eq!(at(100, 50), (1, 0, 0));
    }

    #[test]
    fn scales_by_repeating_pixels_without_blending() {
        let dst = run(Size::new(4, 4));
        #[rustfmt::skip]
        assert_eq!(dst, [
            RED, RED, GREEN, GREEN,
            RED, RED, GREEN, GREEN,
            BLUE, BLUE, WHITE, WHITE,
            BLUE, BLUE, WHITE, WHITE,
        ]);
    }

    #[test]
    fn letterbox_is_black() {
        // Scale 1 (5 / 2 = 2 across, but 3 / 2 = 1 down), centred at (1, 0).
        let dst = run(Size::new(5, 3));
        #[rustfmt::skip]
        assert_eq!(dst, [
            0, RED, GREEN, 0, 0,
            0, BLUE, WHITE, 0, 0,
            0, 0, 0, 0, 0,
        ]);
    }

    #[test]
    fn window_smaller_than_the_picture_shows_its_top_left() {
        assert_eq!(run(Size::new(1, 1)), [RED]);
        assert_eq!(run(Size::new(0, 0)), []);
    }
}
