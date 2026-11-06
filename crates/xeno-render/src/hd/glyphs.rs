//! Text at output resolution (T-074, `docs/hd.md`, "Text at output resolution"): each
//! recorded glyph is drawn again over the window's pixels it owns, its diagonal steps
//! cut and filled into straight 45-degree edges, in its own palette colours.

use xeno_formats::Palette;

use super::{GlyphRecord, Record, Recording};
use crate::{Framebuffer, Layout, Size};

/// What the corner of a cell becomes, beyond the line between its edges' midpoints:
/// the colour drawn there.
type Corners = [[Option<u8>; 2]; 2];

/// The corners of cell (`cx`, `cy`) of `g`, smoothed only where two cells touch
/// diagonally: the gap's corners are filled and the step's outer corners cut, so a
/// diagonal stroke gets straight 45-degree edges while level and upright strokes, their
/// ends and the inside of corners stay square. `colour_at` is the colour a neighbouring
/// pixel shows, if it may be drawn into or taken from.
fn corners(
    g: &GlyphRecord,
    (cx, cy): (i32, i32),
    colour_at: impl Fn(i32, i32) -> Option<u8>,
) -> Corners {
    let (w, h) = (i32::from(g.width), i32::from(g.height));
    let label = |dx: i32, dy: i32| {
        let (x, y) = (cx + dx, cy + dy);
        if (0..w).contains(&x) && (0..h).contains(&y) {
            g.cells[(y * w + x) as usize]
        } else {
            0
        }
    };
    let p = label(0, 0);
    let mut out = [[None; 2]; 2];
    for (qy, row) in [-1, 1].into_iter().zip(&mut out) {
        for (qx, corner) in [-1, 1].into_iter().zip(row.iter_mut()) {
            let (side, upright, diagonal) = (label(qx, 0), label(0, qy), label(qx, qy));
            if side != upright {
                continue;
            }
            let q = side;
            *corner = if q > p && diagonal != q {
                // Two cells touch across this corner: fill it.
                g.colour_of(q)
            } else if q < p
                && diagonal == q
                && ((label(qx, -qy) == p && label(0, -qy) != p)
                    || (label(-qx, qy) == p && label(-qx, 0) != p))
            {
                // The outer corner of a diagonal step: cut it to what is beside it.
                colour_at(qx, 0).or_else(|| colour_at(0, qy))
            } else {
                None
            };
        }
    }
    out
}

/// Draws each recorded glyph again at output resolution over `dst`, which holds
/// [`crate::present`]'s (or [`super::compose`]'s, or [`super::draw_shapes`]'s) picture
/// of `fb` in `layout`, in record order. Only the glyph's
/// cell changes, and in it only pixels the glyph still owns or that were drawn before it
/// (never a later drawing or a write the record did not see, such as the pointer); a
/// cell's corner beyond the line between its edges' midpoints takes the colour of the
/// glyph's cells or of the pixel beside it. Without a recording this changes nothing.
pub fn draw_glyphs(
    fb: &Framebuffer,
    palette: &Palette,
    layout: &Layout,
    dst: &mut [u32],
    dst_size: Size,
) {
    let Some(recording) = fb.recording() else {
        return;
    };
    if !recording
        .records()
        .iter()
        .any(|r| matches!(r, Record::Glyph(_)))
    {
        return;
    }
    let colours: [u32; 256] = std::array::from_fn(|i| {
        let c = palette[i as u8];
        (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
    });
    for (k, record) in recording.records().iter().enumerate() {
        if let Record::Glyph(g) = record {
            draw_glyph(fb, recording, k, g, &colours, layout, dst, dst_size);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_glyph(
    fb: &Framebuffer,
    recording: &Recording,
    k: usize,
    g: &GlyphRecord,
    colours: &[u32; 256],
    layout: &Layout,
    dst: &mut [u32],
    dst_size: Size,
) {
    let (surface, view, (ox, oy)) = (fb.surface(), fb.view(), fb.origin());
    let pixels = fb.pixels();
    let clip = g.clip;
    let index = |x: i32, y: i32| {
        let on = (clip.x..clip.x + clip.width as i32).contains(&x)
            && (clip.y..clip.y + clip.height as i32).contains(&y)
            && (0..surface.width as i32).contains(&x)
            && (0..surface.height as i32).contains(&y);
        on.then(|| y as usize * surface.width + x as usize)
    };
    // The colour of a pixel the glyph still owns, or that still shows what the glyph
    // was drawn over (its colour then, and no later record's).
    let usable = |x: i32, y: i32| {
        let i = index(x, y)?;
        let colour = pixels[i];
        if recording.owner(i, colour) == Some(k) {
            return Some(colour);
        }
        let (ux, uy) = (x - g.at.0 + 1, y - g.at.1 + 1);
        let w = i32::from(g.width) + 2;
        let then = (0..w)
            .contains(&ux)
            .then(|| g.under.get((uy * w + ux) as usize))??;
        let stamp = recording.stamp(i, colour) as usize;
        (*then == colour && stamp < 2 * k + 2).then_some(colour)
    };
    let Layout {
        sx,
        sy,
        x: lx,
        y: ly,
    } = *layout;
    for cy in 0..i32::from(g.height) {
        for cx in 0..i32::from(g.width) {
            let (x, y) = (g.at.0 + cx, g.at.1 + cy);
            if usable(x, y).is_none() {
                continue;
            }
            let corners = corners(g, (cx, cy), |dx, dy| usable(x + dx, y + dy));
            if corners.iter().flatten().all(Option::is_none) {
                continue;
            }
            // Where the pixel shows in the view: from the origin above a split, and from
            // the surface's top-left corner below it.
            let (x, y) = (x as usize, y as usize);
            let split = fb.split().unwrap_or(view.height);
            let above = (x >= ox && y >= oy && x - ox < view.width && y - oy < split)
                .then(|| (x - ox, y - oy));
            let below = (fb.split().is_some() && x < view.width && y + split < view.height)
                .then_some((x, y + split));
            for (vx, vy) in above.into_iter().chain(below) {
                let (wx0, wy0) = (lx + (vx * sx) as isize, ly + (vy * sy) as isize);
                for row in 0..sy {
                    let wy = wy0 + row as isize;
                    if !(0..dst_size.height as isize).contains(&wy) {
                        continue;
                    }
                    let fy = (row as f64 + 0.5) / sy as f64 - 0.5;
                    for column in 0..sx {
                        let wx = wx0 + column as isize;
                        let fx = (column as f64 + 0.5) / sx as f64 - 0.5;
                        if !(0..dst_size.width as isize).contains(&wx) || fx.abs() + fy.abs() <= 0.5
                        {
                            continue;
                        }
                        if let Some(c) = corners[usize::from(fy > 0.0)][usize::from(fx > 0.0)] {
                            dst[wy as usize * dst_size.width + wx as usize] =
                                colours[usize::from(c)];
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rect, TextCursor, TextMode};
    use xeno_formats::Rgb;
    use xeno_formats::font::{Font, FontHeader, Glyph};

    const BACKGROUND: u8 = 0x37;
    const INK: u8 = 0x82;
    const LATER: u8 = 0xc5;
    const LAYOUT: Layout = Layout {
        sx: 8,
        sy: 10,
        x: 5,
        y: -3,
    };

    /// A made-up bitmap font, 7 wide and 8 high: `Z` is a bar, a 45-degree diagonal
    /// from (4, 1) to (1, 4) and a bar; `L` a stem and a foot, with no diagonal.
    fn bitmap() -> Font {
        let mut glyphs = vec![None; 128];
        glyphs[usize::from(b'Z')] = Some(Glyph {
            width: 7,
            rows: vec![0xfc, 0x08, 0x10, 0x20, 0x40, 0xfc, 0],
        });
        glyphs[usize::from(b'L')] = Some(Glyph {
            width: 7,
            rows: vec![0x60, 0x60, 0x60, 0x60, 0x7c, 0x7c, 0],
        });
        Font {
            header: FontHeader {
                kind: 7,
                unknown_01: 0,
                width: 7,
                height: 8,
                step_x: 0,
                step_y: 0,
            },
            glyphs,
        }
    }

    /// A made-up shaded font: `/`, a diagonal in 3 with a shade of 1 to its right.
    fn shaded() -> Font {
        let mut glyphs = vec![None; 64];
        glyphs[usize::from(b'/')] = Some(Glyph {
            width: 4,
            rows: vec![0, 0, 3, 1, 0, 3, 1, 0, 3, 1, 0, 0, 0, 0, 0, 0],
        });
        Font {
            header: FontHeader {
                kind: 0,
                unknown_01: 0,
                width: 5,
                height: 5,
                step_x: 0,
                step_y: 0,
            },
            glyphs,
        }
    }

    /// Index `i` is `(i, 255 - i, 11)`.
    fn ramp() -> Palette {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = Rgb {
                r: i as u8,
                g: 255 - i as u8,
                b: 11,
            };
        }
        Palette { colors }
    }

    fn rgb(colour: u8) -> u32 {
        u32::from(colour) << 16 | u32::from(255 - colour) << 8 | 11
    }

    /// `text` in `font` at (2, 1) on a 12 by 10 surface of `BACKGROUND`, then `after`.
    fn scene(
        font: &Font,
        text: &[u8],
        recording: bool,
        after: impl Fn(&mut Framebuffer),
    ) -> Framebuffer {
        let mut fb = Framebuffer::new(Size::new(12, 10));
        fb.set_recording(recording);
        fb.clear(BACKGROUND);
        let mut cursor = TextCursor::at(2, 1, INK);
        fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
        after(&mut fb);
        fb
    }

    /// `present`'s picture of `fb`, and that with the glyphs drawn again.
    fn pictures(fb: &Framebuffer) -> (Size, Vec<u32>, Vec<u32>) {
        let size = Size::new(12 * 8 + 7, 10 * 10 - 5);
        let mut before = vec![0; size.width * size.height];
        crate::present(fb, &ramp(), &LAYOUT, &mut before, size);
        let mut after = before.clone();
        draw_glyphs(fb, &ramp(), &LAYOUT, &mut after, size);
        (size, before, after)
    }

    /// The surface pixel under output pixel `i`.
    fn logical(i: usize, size: Size) -> (i32, i32) {
        let (wx, wy) = ((i % size.width) as i32, (i / size.width) as i32);
        ((wx - 5).div_euclid(8), (wy + 3).div_euclid(10))
    }

    #[test]
    fn without_a_record_the_output_is_present_s() {
        let (plain, recorded) = (
            scene(&bitmap(), b"Z", false, |_| {}),
            scene(&bitmap(), b"Z", true, |_| {}),
        );
        assert_eq!(plain.pixels(), recorded.pixels());
        let (_, before, after) = pictures(&plain);
        assert!(after == before);
        assert!(pictures(&recorded).2 != before);
    }

    #[test]
    fn a_diagonal_at_8x10_is_smooth_inside_the_cell_in_the_glyph_s_colours() {
        let fb = scene(&bitmap(), b"Z", true, |_| {});
        let (size, before, after) = pictures(&fb);
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            if was != now {
                let (x, y) = logical(i, size);
                assert!((2..9).contains(&x) && (1..8).contains(&y), "({x}, {y})");
                assert!(now == rgb(INK) || now == rgb(BACKGROUND));
            }
        }
        // The diagonal's left edge, output row by output row over its middle (glyph rows
        // 2 and 3): one output pixel a row at most, where the original jumps 8.
        let left = |out: &[u32], wy: usize| {
            (0..size.width)
                .find(|&wx| out[wy * size.width + wx] == rgb(INK))
                .unwrap()
        };
        let rows = (1 + 2) * 10 - 3..(1 + 4) * 10 - 3;
        let edge: Vec<usize> = rows.clone().map(|wy| left(&after, wy)).collect();
        assert!(
            edge.windows(2).all(|w| w[0] >= w[1] && w[0] - w[1] <= 1),
            "{edge:?}"
        );
        assert!(
            rows.clone()
                .zip(rows.skip(1))
                .any(|(a, b)| left(&before, a) - left(&before, b) == 8)
        );
    }

    #[test]
    fn level_and_upright_strokes_stay_as_they_are() {
        let fb = scene(&bitmap(), b"L", true, |_| {});
        let (_, before, after) = pictures(&fb);
        assert!(after == before);
        // The bars of `Z` keep their top and bottom edges.
        let fb = scene(&bitmap(), b"Z", true, |_| {});
        let (size, before, after) = pictures(&fb);
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            let (x, y) = logical(i, size);
            if (y == 1 && x < 6) || (y == 6 && x > 3) {
                assert_eq!(now, was, "({x}, {y})");
            }
        }
    }

    #[test]
    fn shades_keep_their_colours() {
        let fb = scene(&shaded(), b"/", true, |_| {});
        let (_, before, after) = pictures(&fb);
        let allowed = [INK, INK + 2, BACKGROUND].map(rgb);
        let mut changed = 0;
        for (&was, &now) in before.iter().zip(&after) {
            if was != now {
                assert!(allowed.contains(&now));
                changed += 1;
            }
        }
        assert!(changed > 0);
    }

    #[test]
    fn a_later_drawing_stays_on_top() {
        // A dot over the diagonal and one in the gap the glyph would fill, after it.
        let fb = scene(&bitmap(), b"Z", true, |fb| {
            fb.pixels_mut()[3 * 12 + 2 + 3] = LATER;
            fb.pixels_mut()[2 * 12 + 2 + 3] = LATER;
        });
        let (size, before, after) = pictures(&fb);
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            if fb.get(logical(i, size).0 as usize, logical(i, size).1 as usize) == Some(LATER) {
                assert_eq!(now, was);
            }
        }
        assert!(after != before);
    }

    #[test]
    fn a_copy_carries_the_glyphs_along() {
        let draw = |fb: &mut Framebuffer, at: (i32, i32)| {
            fb.set_recording(true);
            fb.clear(BACKGROUND);
            let mut cursor = TextCursor::at(at.0, at.1, INK);
            fb.draw_text(&bitmap(), &mut cursor, b"Z", TextMode::Transparent);
        };
        let mut direct = Framebuffer::new(Size::new(12, 10));
        draw(&mut direct, (3, 2));
        let mut buffer = Framebuffer::new(Size::new(9, 8));
        draw(&mut buffer, (1, 1));
        let mut copied = Framebuffer::new(Size::new(12, 10));
        copied.set_recording(true);
        copied.clear(BACKGROUND);
        copied.fill_rect(Rect::new(2, 1, 9, 8), 0xff);
        copied.copy_through(&buffer, (2, 1), 0xff);
        assert_eq!(copied.pixels(), direct.pixels());
        let (_, before, after) = pictures(&copied);
        assert!(after == pictures(&direct).2 && after != before);
    }
}
