//! The mouse pointer, drawn into the framebuffer as `mgavideo.dvr` draws its own each
//! retrace (`gfx_update_pointer`, slot `0x14b2`; `docs/screens.md`, "The pointer"):
//! the shape the driver sets up, read from the user's copy
//! ([`xeno_formats::mgavideo::Pointer`]), placed by its hot spot and clipped to the
//! view. The pixels under it are saved first and put back after the frame is shown,
//! so the screens never see it.

use xeno_formats::mgavideo::Pointer;
use xeno_render::Framebuffer;

/// The pixels a drawn pointer covered: (index into the surface, colour).
#[derive(Debug)]
pub struct Under(Vec<(usize, u8)>);

/// Draws `shape` with its hot spot at view pixel (`x`, `y`), clipped to the view:
/// where its mask is 0, the image byte AND the pointer colour; elsewhere nothing.
pub fn draw(fb: &mut Framebuffer, (x, y): (u16, u16), shape: &Pointer) -> Under {
    let (view, surface, (ox, oy)) = (fb.view(), fb.surface(), fb.origin());
    let left = i32::from(x) - i32::from(shape.hot_x);
    let top = i32::from(y) - i32::from(shape.hot_y);
    let mut under = Vec::new();
    for dy in 0..usize::from(shape.height) {
        for dx in 0..usize::from(shape.width) {
            let Some(colour) = shape.pixel(dx, dy) else {
                continue;
            };
            let (vx, vy) = (left + dx as i32, top + dy as i32);
            if let (Ok(vx), Ok(vy)) = (usize::try_from(vx), usize::try_from(vy))
                && vx < view.width
                && vy < view.height
            {
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

    /// A made-up 3 by 2 shape with its hot spot at (1, 1): image 0xff, 0x0f, 0xff
    /// over 0, 0xff, 0xff; the right column masked off; colour 0x3c.
    fn shape() -> Pointer {
        Pointer {
            width: 3,
            height: 2,
            hot_x: 1,
            hot_y: 1,
            image: vec![0xff, 0x0f, 0xff, 0, 0xff, 0xff],
            mask: vec![0, 0, 1, 0, 0, 1],
            colour: 0x3c,
        }
    }

    #[test]
    fn the_pointer_is_drawn_by_its_hot_spot_clipped_and_leaves_no_trace() {
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(7);
        let before = fb.clone();
        let under = draw(&mut fb, (10, 20), &shape());
        let rows: Vec<Vec<_>> = (19..21)
            .map(|y| (9..12).map(|x| fb.get(x, y).unwrap()).collect())
            .collect();
        assert_eq!(rows, [[0x3c, 0x0c, 7], [0, 0x3c, 7]]);
        under.restore(&mut fb);
        assert_eq!(fb, before);
        // At the top-left corner only the hot spot's row and column are on the view.
        let under = draw(&mut fb, (0, 0), &shape());
        assert_eq!(under.0.len(), 1);
        assert_eq!(fb.get(0, 0), Some(0x3c));
        under.restore(&mut fb);
        assert_eq!(fb, before);
        // At the bottom-right corner the shape's last row and column are off it.
        let under = draw(&mut fb, (319, 199), &shape());
        assert_eq!(under.0.len(), 4);
        under.restore(&mut fb);
        assert_eq!(fb, before);
    }
}
