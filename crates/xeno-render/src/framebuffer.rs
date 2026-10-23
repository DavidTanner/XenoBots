//! An indexed surface, possibly wider (or taller) than the screen, with a viewport.

use xeno_formats::Palette;

/// A width and height in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    /// Mode 13h, the default mode: front end and game (5:6 pixels on a 4:3 screen).
    pub const MODE_13H: Size = Size::new(320, 200);
    /// `vga240.mod`'s unchained mode, used for the `.vcl` cutscenes (square pixels).
    pub const MODE_240: Size = Size::new(320, 240);

    pub const fn new(width: usize, height: usize) -> Self {
        Size { width, height }
    }
}

/// A rectangle in surface pixels. It may reach outside the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("a {view:?} view does not fit in a {surface:?} surface")]
    ViewLargerThanSurface { view: Size, surface: Size },
    #[error("a {view:?} view at ({x}, {y}) reaches outside the {surface:?} surface")]
    OriginOutsideSurface {
        x: usize,
        y: usize,
        view: Size,
        surface: Size,
    },
}

/// 8-bit palette indices on a surface of `surface` size, of which the `view` size
/// rectangle at `origin` is on screen.
///
/// The surface is usually the size of the screen. The hardware-scrolled screens draw
/// on a wider virtual surface (640 pixels for the `.vcl` cutscenes, 768 for the intro
/// drop) and move the view over it one pixel at a time; [`Framebuffer::set_origin`]
/// does the same. All drawing uses surface coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framebuffer {
    surface: Size,
    view: Size,
    origin: (usize, usize),
    /// The VGA's line compare: view rows from this one on show the surface from (0, 0).
    split: Option<usize>,
    pixels: Vec<u8>,
}

impl Framebuffer {
    /// A surface exactly the size of the view, filled with colour 0.
    pub fn new(view: Size) -> Self {
        Framebuffer {
            surface: view,
            view,
            origin: (0, 0),
            split: None,
            pixels: vec![0; view.width * view.height],
        }
    }

    /// A surface larger than the view, filled with colour 0, viewed from (0, 0).
    pub fn with_surface(view: Size, surface: Size) -> Result<Self, RenderError> {
        if view.width > surface.width || view.height > surface.height {
            return Err(RenderError::ViewLargerThanSurface { view, surface });
        }
        Ok(Framebuffer {
            surface,
            view,
            origin: (0, 0),
            split: None,
            pixels: vec![0; surface.width * surface.height],
        })
    }

    pub fn surface(&self) -> Size {
        self.surface
    }

    pub fn view(&self) -> Size {
        self.view
    }

    pub fn origin(&self) -> (usize, usize) {
        self.origin
    }

    /// Moves the view's top-left corner to surface pixel (`x`, `y`). The view must stay
    /// on the surface: the VGA would wrap around video memory instead, which no screen
    /// of the game relies on.
    pub fn set_origin(&mut self, x: usize, y: usize) -> Result<(), RenderError> {
        if x + self.view.width > self.surface.width || y + self.view.height > self.surface.height {
            return Err(RenderError::OriginOutsideSurface {
                x,
                y,
                view: self.view,
                surface: self.surface,
            });
        }
        self.origin = (x, y);
        Ok(())
    }

    /// Splits the view, as the VGA's line compare register does: view rows from `row`
    /// on show the surface's rows from 0, from column 0, whatever the origin (the intro
    /// drop's credits band, `docs/formats/fgd.md`). `None` shows the whole view from
    /// the origin.
    pub fn set_split(&mut self, row: Option<usize>) {
        self.split = row;
    }

    pub fn split(&self) -> Option<usize> {
        self.split
    }

    /// Where view row `row` starts in [`Framebuffer::pixels`].
    pub fn view_row_start(&self, row: usize) -> usize {
        match self.split {
            Some(split) if row >= split => (row - split) * self.surface.width,
            _ => (self.origin.1 + row) * self.surface.width + self.origin.0,
        }
    }

    /// The whole surface, row by row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// The colour at surface pixel (`x`, `y`), if it is on the surface.
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        (x < self.surface.width && y < self.surface.height)
            .then(|| self.pixels[y * self.surface.width + x])
    }

    /// Fills the whole surface with `colour`.
    pub fn clear(&mut self, colour: u8) {
        self.pixels.fill(colour);
    }

    /// Fills the part of `rect` that lies on the surface with `colour`.
    pub fn fill_rect(&mut self, rect: Rect, colour: u8) {
        let clamp = |v: i64, max: usize| v.clamp(0, max as i64) as usize;
        let x0 = clamp(rect.x.into(), self.surface.width);
        let x1 = clamp(
            i64::from(rect.x) + i64::from(rect.width),
            self.surface.width,
        );
        let y0 = clamp(rect.y.into(), self.surface.height);
        let y1 = clamp(
            i64::from(rect.y) + i64::from(rect.height),
            self.surface.height,
        );
        for y in y0..y1 {
            let row = y * self.surface.width;
            self.pixels[row + x0..row + x1].fill(colour);
        }
    }

    /// The view as RGBA bytes (alpha 255), row by row: `view.width * view.height * 4`
    /// bytes, each pixel looked up in `palette`.
    pub fn to_rgba(&self, palette: &Palette) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.view.width * self.view.height * 4);
        for row in 0..self.view.height {
            let start = self.view_row_start(row);
            for &index in &self.pixels[start..start + self.view.width] {
                let c = palette[index];
                out.extend([c.r, c.g, c.b, 255]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::Rgb;

    /// A palette where index `i` is `(i, 255 - i, i / 2)`.
    fn ramp() -> Palette {
        let bytes: Vec<u8> = (0..=255u8).flat_map(|i| [i, 255 - i, i / 2]).collect();
        Palette::from_rgb8(&bytes).unwrap()
    }

    #[test]
    fn mode_sizes_match_the_two_video_modes() {
        let fb = Framebuffer::new(Size::MODE_13H);
        assert_eq!((fb.surface(), fb.view()), (Size::MODE_13H, Size::MODE_13H));
        assert_eq!(fb.pixels().len(), 64_000);
        let fb = Framebuffer::with_surface(Size::MODE_240, Size::new(640, 400)).unwrap();
        assert_eq!(fb.pixels().len(), 640 * 400);
        assert_eq!(fb.to_rgba(&ramp()).len(), 320 * 240 * 4);
    }

    #[test]
    fn view_must_fit_the_surface() {
        let err = Framebuffer::with_surface(Size::MODE_240, Size::new(768, 200)).unwrap_err();
        assert!(matches!(err, RenderError::ViewLargerThanSurface { .. }));
    }

    #[test]
    fn origin_moves_one_pixel_at_a_time_and_stays_on_the_surface() {
        let mut fb = Framebuffer::with_surface(Size::MODE_13H, Size::new(768, 200)).unwrap();
        fb.set_origin(447, 0).unwrap();
        fb.set_origin(448, 0).unwrap();
        assert_eq!(fb.origin(), (448, 0));
        assert!(fb.set_origin(449, 0).is_err());
        assert!(fb.set_origin(0, 1).is_err());
        assert_eq!(fb.origin(), (448, 0));
    }

    #[test]
    fn to_rgba_shows_only_the_view() {
        let mut fb = Framebuffer::with_surface(Size::new(2, 2), Size::new(5, 3)).unwrap();
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            *p = i as u8;
        }
        fb.set_origin(3, 1).unwrap();
        let palette = ramp();
        let expected: Vec<u8> = [8u8, 9, 13, 14]
            .iter()
            .flat_map(|&i| {
                let Rgb { r, g, b } = palette[i];
                [r, g, b, 255]
            })
            .collect();
        assert_eq!(fb.to_rgba(&palette), expected);
    }

    #[test]
    fn a_split_view_shows_the_top_of_the_surface_below_it() {
        let mut fb = Framebuffer::with_surface(Size::new(2, 3), Size::new(5, 4)).unwrap();
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            *p = i as u8;
        }
        fb.set_origin(3, 1).unwrap();
        fb.set_split(Some(1));
        assert_eq!(fb.split(), Some(1));
        let rows: Vec<usize> = (0..3).map(|r| fb.view_row_start(r)).collect();
        // Row 0 from the origin, then surface rows 0 and 1 from column 0.
        assert_eq!(rows, [8, 0, 5]);
        let red: Vec<u8> = fb.to_rgba(&ramp()).chunks(4).map(|c| c[0]).collect();
        assert_eq!(red, [8, 9, 0, 1, 5, 6]);
        fb.set_split(None);
        assert_eq!(fb.view_row_start(1), 13);
    }

    #[test]
    fn vga6_palettes_expand_to_full_range_rgba() {
        let mut dac = vec![0u8; Palette::BYTES];
        dac[3..6].copy_from_slice(&[63, 32, 1]);
        let mut fb = Framebuffer::new(Size::new(2, 1));
        fb.pixels_mut()[1] = 1;
        let rgba = fb.to_rgba(&Palette::from_vga6(&dac).unwrap());
        assert_eq!(rgba, [0, 0, 0, 255, 255, 130, 4, 255]);
    }

    #[test]
    fn fill_rect_clips_to_the_surface() {
        let mut fb = Framebuffer::new(Size::new(4, 3));
        fb.fill_rect(Rect::new(-1, 1, 3, 9), 7);
        fb.fill_rect(Rect::new(3, -5, 4, 6), 5);
        fb.fill_rect(Rect::new(i32::MAX, 0, u32::MAX, 1), 9);
        fb.fill_rect(Rect::new(1, 0, 0, 3), 9);
        assert_eq!(fb.pixels(), [0, 0, 0, 5, 7, 7, 0, 0, 7, 7, 0, 0]);
        fb.clear(2);
        assert!(fb.pixels().iter().all(|&p| p == 2));
        assert_eq!((fb.get(3, 2), fb.get(4, 0)), (Some(2), None));
    }
}
