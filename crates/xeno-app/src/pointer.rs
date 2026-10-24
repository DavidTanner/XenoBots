//! The mouse pointer, drawn into the framebuffer as `mgavideo.dvr` draws its own each
//! retrace (`gfx_update_pointer`, slot `0x14b2`; `docs/screens.md`, "The pointer"):
//! the shape the driver sets up, read from the user's copy
//! ([`xeno_formats::mgavideo::Pointer`]), placed by its hot spot and clipped to the
//! view. The pixels under it are saved first and put back after the frame is shown,
//! so the screens never see it.
//!
//! Which shape is shown, and whether it is shown at all, follow the host's calls to
//! the driver's slots (`State`; `docs/screens.md`, "The pointer", T-049).

use xeno_formats::mgavideo::Pointer;
use xeno_render::Framebuffer;

/// Shape numbers the host passes to slot `0x1496` (`mov al, n` in its code): the
/// arrow, the busy shape it shows while a picture loads, and the one over the
/// mission views' corners.
pub const ARROW: u8 = 0;
pub const BUSY: u8 = 1;
pub const CORNER: u8 = 2;

/// The pointer's shape (`DS:0x14b6`) and its hide counter (`DS:0x14d8`), as the
/// driver keeps them: `gfx_hide_pointer` (slot `0x149e`) adds one and
/// `gfx_show_pointer` (`0x14a2`) takes one away, and the pointer is drawn only while
/// the counter is 0xff. It starts at 0, hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    shape: u8,
    counter: u8,
}

impl State {
    /// `shape`, hidden (`shown` false) or shown.
    pub fn new(shape: u8, shown: bool) -> State {
        let counter = if shown { 0xff } else { 0 };
        State { shape, counter }
    }

    pub fn hide(&mut self) {
        self.counter = self.counter.wrapping_add(1);
    }

    pub fn show(&mut self) {
        self.counter = self.counter.wrapping_sub(1);
    }

    pub fn shown(self) -> bool {
        self.counter == 0xff
    }

    pub fn shape(self) -> u8 {
        self.shape
    }

    /// Picks `shape` of `shapes`, as `gfx_set_pointer_shape` does: a number past the
    /// last shape picks shape 0.
    pub fn set_shape(&mut self, shape: u8, shapes: usize) {
        self.shape = if usize::from(shape) < shapes {
            shape
        } else {
            0
        };
    }
}

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
    fn the_counter_nests_and_shapes_past_the_last_pick_the_first() {
        let mut state = State::new(ARROW, false);
        assert!(!state.shown());
        state.show();
        assert!(state.shown());
        state.hide();
        state.hide();
        state.show();
        assert!(!state.shown());
        state.show();
        assert!(state.shown());
        // A second show hides it again, as the driver's counter does.
        state.show();
        assert!(!state.shown());
        state.set_shape(BUSY, 8);
        assert_eq!(state.shape(), BUSY);
        state.set_shape(CORNER, 2);
        assert_eq!(state.shape(), ARROW);
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
