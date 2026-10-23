//! A placeholder mouse pointer, drawn into the framebuffer as the original draws its
//! own each retrace (`mgavideo.dvr`'s slots `0x1496`–`0x14b2`, `docs/re/dvr-protocol.md`):
//! the pixels under it are saved first and put back after the frame is shown, so the
//! screens never see it. The shape is synthetic, an arrow with its tip on the pointer's
//! pixel, outlined in the palette's darkest colour and filled with its brightest.

use xeno_render::{Framebuffer, Palette};

use crate::viewer::contrast_pair;

/// `#` outline, `o` fill, anything else clear.
#[rustfmt::skip]
const SHAPE: [&str; 11] = [
    "#",
    "##",
    "#o#",
    "#oo#",
    "#ooo#",
    "#oooo#",
    "#ooooo#",
    "#oooo###",
    "#o##o#",
    "##  #o#",
    "     ##",
];

/// The pixels a drawn pointer covered: (index into the surface, colour).
#[derive(Debug)]
pub struct Under(Vec<(usize, u8)>);

/// Draws the pointer with its tip at view pixel (`x`, `y`), clipped to the view, in
/// colours of `palette`.
pub fn draw(fb: &mut Framebuffer, (x, y): (u16, u16), palette: &Palette) -> Under {
    let (dark, light) = contrast_pair(palette);
    let (view, surface, (ox, oy)) = (fb.view(), fb.surface(), fb.origin());
    let mut under = Vec::new();
    for (dy, row) in SHAPE.iter().enumerate() {
        for (dx, c) in row.bytes().enumerate() {
            let colour = match c {
                b'#' => dark,
                b'o' => light,
                _ => continue,
            };
            let (vx, vy) = (usize::from(x) + dx, usize::from(y) + dy);
            if vx < view.width && vy < view.height {
                let i = (oy + vy) * surface.width + ox + vx;
                under.push((i, fb.pixels()[i]));
                fb.pixels_mut()[i] = colour;
            }
        }
    }
    Under(under)
}

impl Under {
    /// Puts back what the pointer covered.
    pub fn restore(self, fb: &mut Framebuffer) {
        let pixels = fb.pixels_mut();
        for (i, colour) in self.0.into_iter().rev() {
            pixels[i] = colour;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_render::Size;

    #[test]
    fn the_pointer_is_drawn_clipped_and_leaves_no_trace() {
        // Synthetic: grey levels, so 0 is the darkest and 252 the first brightest.
        let grey: Vec<u8> = (0..=255u8).flat_map(|i| [i / 4; 3]).collect();
        let palette = Palette::from_vga6(&grey).unwrap();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(7);
        let before = fb.clone();
        let under = draw(&mut fb, (10, 20), &palette);
        assert_eq!(fb.get(10, 20), Some(0));
        assert_eq!(fb.get(11, 22), Some(252));
        assert_eq!(fb.get(12, 20), Some(7));
        under.restore(&mut fb);
        assert_eq!(fb, before);
        // At the corner only the tip is on the view.
        let under = draw(&mut fb, (319, 199), &palette);
        assert_eq!(under.0.len(), 1);
        under.restore(&mut fb);
        assert_eq!(fb, before);
    }
}
