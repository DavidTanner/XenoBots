//! Shapes at output resolution (T-236, `docs/hd.md`, "Shapes at output resolution"):
//! each recorded polygon and sloping line is drawn again over the window's pixels at
//! the edges it owns, from its logical geometry, in its own palette colour.

use xeno_formats::Palette;

use super::{Geometry, Record};
use crate::{Framebuffer, Layout, Size};

/// A shape's area in surface pixels, measured so that a pixel's centre is its whole
/// number: the pixels the original lights there, as a smooth outline.
enum Cover {
    /// A convex polygon grown by half a pixel each way (the corners' pixel squares, so
    /// the box is the original's, inclusive of both ends): half-planes `a x + b y <= c`.
    Convex(Vec<[f64; 3]>),
    /// A line one pixel thick across its longer axis `u` (as the original steps one
    /// pixel along it), half a pixel past each end: `v = v0 + (u - u0) * slope`.
    Band {
        steep: bool,
        u: (f64, f64),
        u0: f64,
        v0: f64,
        slope: f64,
    },
}

impl Cover {
    fn new(shape: &Geometry) -> Option<Cover> {
        match shape {
            Geometry::Polygon(corners) => {
                let p: Vec<(f64, f64)> =
                    corners.iter().map(|&(x, y)| (x.into(), y.into())).collect();
                let n = p.len();
                let edge = |i: usize| {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    (b.0 - a.0, b.1 - a.1)
                };
                let area: f64 = (0..n)
                    .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
                    .sum();
                // shortcut: only convex polygons are redrawn (the shape ops' triangles and
                // quads are, unless degenerate); others keep the original's pixels.
                let convex = (0..n).all(|i| {
                    let (e, f) = (edge(i), edge((i + 1) % n));
                    (e.0 * f.1 - e.1 * f.0) * area >= 0.0
                });
                if n < 3 || area == 0.0 || !convex {
                    return None;
                }
                let sign = area.signum();
                let mut planes: Vec<[f64; 3]> = (0..n)
                    .filter_map(|i| {
                        let (ex, ey) = edge(i);
                        let (a, b) = (sign * ey, -sign * ex);
                        ((a, b) != (0.0, 0.0))
                            .then(|| [a, b, a * p[i].0 + b * p[i].1 + 0.5 * (a.abs() + b.abs())])
                    })
                    .collect();
                let fold = |f: fn(f64, f64) -> f64, axis: fn(&(f64, f64)) -> f64| {
                    p.iter().map(axis).reduce(f).unwrap_or(0.0)
                };
                let (x0, x1) = (fold(f64::min, |c| c.0), fold(f64::max, |c| c.0));
                let (y0, y1) = (fold(f64::min, |c| c.1), fold(f64::max, |c| c.1));
                planes.extend([
                    [-1.0, 0.0, 0.5 - x0],
                    [1.0, 0.0, x1 + 0.5],
                    [0.0, -1.0, 0.5 - y0],
                    [0.0, 1.0, y1 + 0.5],
                ]);
                Some(Cover::Convex(planes))
            }
            &Geometry::Line([(x0, y0), (x1, y1)]) => {
                let steep = (y1 - y0).abs() > (x1 - x0).abs();
                let [u0, v0, u1, v1] = if steep {
                    [y0, x0, y1, x1]
                } else {
                    [x0, y0, x1, y1]
                }
                .map(f64::from);
                (u0 != u1).then(|| Cover::Band {
                    steep,
                    u: (u0.min(u1) - 0.5, u0.max(u1) + 0.5),
                    u0,
                    v0,
                    slope: (v1 - v0) / (u1 - u0),
                })
            }
        }
    }

    fn covers(&self, x: f64, y: f64) -> bool {
        match *self {
            Cover::Convex(ref planes) => planes.iter().all(|&[a, b, c]| a * x + b * y <= c),
            Cover::Band {
                steep,
                u,
                u0,
                v0,
                slope,
            } => {
                let (pu, pv) = if steep { (y, x) } else { (x, y) };
                (u.0..=u.1).contains(&pu) && (pv - (v0 + (pu - u0) * slope)).abs() <= 0.5
            }
        }
    }
}

const NEIGHBOURS: [(i64, i64); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Draws each recorded shape again at output resolution over `dst`, which holds
/// [`crate::present`]'s (or [`super::compose`]'s) picture of `fb` in `layout`, in record
/// order. A shape's output pixels are those of its edge pixels (the ones it still owns
/// with a neighbour it does not) and of the neighbours it could cover: on the surface,
/// in its clip, and holding something drawn before it (a background or an earlier
/// record), never a later drawing or a write the record did not see (text, sprites).
/// There an output pixel whose centre is in the shape's outline takes its colour; one
/// of its own pixels that is not takes the colour of the neighbour on that side, if
/// that neighbour was drawn before it. Pixels with every neighbour its own keep the
/// presented colour. Without a recording this changes nothing.
pub fn draw_shapes(
    fb: &Framebuffer,
    palette: &Palette,
    layout: &Layout,
    dst: &mut [u32],
    dst_size: Size,
) {
    let Some(recording) = fb.recording() else {
        return;
    };
    // shortcut: a split view (only the intro drop's credits) stays as presented.
    if fb.split().is_some() {
        return;
    }
    let records = recording.records();
    let covers: Vec<Option<Cover>> = records
        .iter()
        .map(|r| match r {
            Record::Shape(s) => Cover::new(&s.shape),
            Record::Blit(_) => None,
        })
        .collect();
    if covers.iter().all(Option::is_none) {
        return;
    }
    let colours: [u32; 256] = std::array::from_fn(|i| {
        let c = palette[i as u8];
        (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
    });
    let (stride, view, (ox, oy)) = (fb.surface().width as i64, fb.view(), fb.origin());
    let (ox, oy, vw, vh) = (ox as i64, oy as i64, view.width as i64, view.height as i64);
    let pixels = fb.pixels();
    let in_view = |x: i64, y: i64| (ox..ox + vw).contains(&x) && (oy..oy + vh).contains(&y);
    let at = |x: i64, y: i64| (y * stride + x) as usize;
    let shape_of = |i: usize| {
        recording
            .owner(i, pixels[i])
            .filter(|&k| covers[k].is_some())
    };
    // Drawn before record `k`, so it may be covered by it or show from behind it.
    let before = |i: usize, k: usize| {
        let stamp = recording.stamp(i, pixels[i]);
        stamp != 0 && (stamp as usize) < 2 * k + 2
    };
    let mut lists = vec![Vec::new(); records.len()];
    for y in oy..oy + vh {
        for x in ox..ox + vw {
            let Some(k) = shape_of(at(x, y)) else {
                continue;
            };
            let Record::Shape(s) = &records[k] else {
                continue;
            };
            let clip = s.clip;
            let in_clip = |x: i64, y: i64| {
                (i64::from(clip.x)..i64::from(clip.x) + i64::from(clip.width)).contains(&x)
                    && (i64::from(clip.y)..i64::from(clip.y) + i64::from(clip.height)).contains(&y)
            };
            let mut edge = false;
            for (dx, dy) in NEIGHBOURS {
                let (nx, ny) = (x + dx, y + dy);
                if !in_view(nx, ny) {
                    edge = true;
                    continue;
                }
                let j = at(nx, ny);
                if shape_of(j) == Some(k) {
                    continue;
                }
                edge = true;
                if before(j, k) && in_clip(nx, ny) {
                    lists[k].push((nx, ny));
                }
            }
            if edge {
                lists[k].push((x, y));
            }
        }
    }
    let Layout {
        sx,
        sy,
        x: lx,
        y: ly,
    } = *layout;
    let (sx, sy) = (sx as i64, sy as i64);
    for (k, list) in lists.iter_mut().enumerate() {
        let (Some(cover), Record::Shape(s)) = (&covers[k], &records[k]) else {
            continue;
        };
        list.sort_unstable();
        list.dedup();
        for &(x, y) in list.iter() {
            let owned = shape_of(at(x, y)) == Some(k);
            // The neighbour on the side of (`fx`, `fy`) in the pixel drawn before `k`.
            let behind = |fx: f64, fy: f64| {
                let (dx, dy) = (if fx < 0.5 { -1 } else { 1 }, if fy < 0.5 { -1 } else { 1 });
                let across = (fx - 0.5).abs() * sx as f64 >= (fy - 0.5).abs() * sy as f64;
                let sides = if across {
                    [(dx, 0), (0, dy), (dx, dy)]
                } else {
                    [(0, dy), (dx, 0), (dx, dy)]
                };
                sides.into_iter().find_map(|(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    let j = in_view(nx, ny).then(|| at(nx, ny))?;
                    (shape_of(j) != Some(k) && before(j, k)).then(|| pixels[j])
                })
            };
            let (wx0, wy0) = (lx as i64 + (x - ox) * sx, ly as i64 + (y - oy) * sy);
            for row in 0..sy {
                let wy = wy0 + row;
                if !(0..dst_size.height as i64).contains(&wy) {
                    continue;
                }
                let fy = (row as f64 + 0.5) / sy as f64;
                for column in 0..sx {
                    let wx = wx0 + column;
                    if !(0..dst_size.width as i64).contains(&wx) {
                        continue;
                    }
                    let fx = (column as f64 + 0.5) / sx as f64;
                    let colour = if cover.covers(x as f64 + fx - 0.5, y as f64 + fy - 0.5) {
                        Some(s.colour)
                    } else if owned {
                        behind(fx, fy)
                    } else {
                        None
                    };
                    if let Some(c) = colour {
                        dst[wy as usize * dst_size.width + wx as usize] = colours[usize::from(c)];
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;
    use crate::shape::fill_polygon;
    use xeno_formats::Rgb;

    const BACKGROUND: u8 = 0x2b;
    const FILL: u8 = 0x6d;
    const TEXT: u8 = 0x91;
    const LAYOUT: Layout = Layout {
        sx: 8,
        sy: 10,
        x: 3,
        y: -4,
    };

    /// Index `i` is `(i, 255 - i, 9)`.
    fn ramp() -> Palette {
        let mut colors = [Rgb { r: 0, g: 0, b: 0 }; Palette::LEN];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = Rgb {
                r: i as u8,
                g: 255 - i as u8,
                b: 9,
            };
        }
        Palette { colors }
    }

    fn rgb(colour: u8) -> u32 {
        u32::from(colour) << 16 | u32::from(255 - colour) << 8 | 9
    }

    /// `present`'s picture of `fb`, and that with the shapes drawn again.
    fn pictures(fb: &Framebuffer) -> (Size, Vec<u32>, Vec<u32>) {
        let view = fb.view();
        let size = Size::new(view.width * 8 + 5, view.height * 10 - 3);
        let mut before = vec![0; size.width * size.height];
        crate::present(fb, &ramp(), &LAYOUT, &mut before, size);
        let mut after = before.clone();
        draw_shapes(fb, &ramp(), &LAYOUT, &mut after, size);
        (size, before, after)
    }

    /// The surface pixel under output pixel `i`.
    fn logical(i: usize, size: Size) -> (i64, i64) {
        let (wx, wy) = ((i % size.width) as i64, (i / size.width) as i64);
        ((wx - 3).div_euclid(8), (wy + 4).div_euclid(10))
    }

    fn polygon_scene(recording: bool) -> Framebuffer {
        let mut fb = Framebuffer::new(Size::new(26, 17));
        fb.set_recording(recording);
        fb.clear(BACKGROUND);
        fill_polygon(&mut fb, &[(3, 2), (5, 14), (22, 7)], FILL);
        // Text over the right corner, after the polygon.
        for x in 19..23 {
            fb.pixels_mut()[7 * 26 + x] = TEXT;
        }
        fb
    }

    #[test]
    fn without_a_record_the_output_is_present_s() {
        let (plain, recorded) = (polygon_scene(false), polygon_scene(true));
        assert_eq!(plain.pixels(), recorded.pixels());
        let (_, before, after) = pictures(&plain);
        assert!(after == before);
        assert!(recorded.recording().unwrap().records().len() == 1);
    }

    #[test]
    fn a_polygon_at_8x10_has_smooth_edges_in_its_colour_within_one_pixel() {
        let fb = polygon_scene(true);
        let (size, before, after) = pictures(&fb);
        let r = fb.recording().unwrap();
        let owned = |x: i64, y: i64| {
            (0..26).contains(&x)
                && (0..17).contains(&y)
                && r.owner((y * 26 + x) as usize, fb.pixels()[(y * 26 + x) as usize]) == Some(0)
        };
        let mut changed = 0;
        for (i, (&was, &now)) in before.iter().zip(&after).enumerate() {
            let (x, y) = logical(i, size);
            if fb.get(x as usize, y as usize) == Some(TEXT) {
                assert_eq!(now, was, "text at ({x}, {y}) stays on top");
            }
            if was != now {
                changed += 1;
                assert!(
                    (-1..=1).any(|dy| (-1..=1).any(|dx| owned(x + dx, y + dy))),
                    "({x}, {y}) is not the polygon's or beside it"
                );
                assert!(now == rgb(FILL) || now == rgb(BACKGROUND));
            }
        }
        assert!(changed > 100, "{changed}");
        // The long upper edge, from (3, 2) to (22, 7): its top in each output column
        // moves down at most one output row from the last (a logical step is 10).
        let top = |wx: usize| (0..size.height).find(|&wy| after[wy * size.width + wx] == rgb(FILL));
        let columns: Vec<usize> = (6 * 8 + 3..17 * 8 + 3).map(|wx| top(wx).unwrap()).collect();
        assert!(
            columns.windows(2).all(|w| w[0] <= w[1] && w[1] - w[0] <= 1),
            "{columns:?}"
        );
        // Where the original steps down whole pixels at once.
        let top_before =
            |wx: usize| (0..size.height).find(|&wy| before[wy * size.width + wx] == rgb(FILL));
        assert!(
            (6 * 8 + 3..17 * 8 + 3)
                .any(|wx| top_before(wx + 1).unwrap() - top_before(wx).unwrap() >= 10)
        );
    }

    #[test]
    fn a_copy_carries_the_shapes_along() {
        let draw = |fb: &mut Framebuffer, (dx, dy): (i16, i16)| {
            fill_polygon(
                fb,
                &[(3 + dx, 2 + dy), (5 + dx, 14 + dy), (22 + dx, 7 + dy)],
                FILL,
            )
        };
        let mut direct = Framebuffer::new(Size::new(34, 22));
        direct.set_recording(true);
        direct.clear(BACKGROUND);
        draw(&mut direct, (5, 3));
        let mut buffer = Framebuffer::new(Size::new(26, 17));
        buffer.set_recording(true);
        buffer.clear(BACKGROUND);
        draw(&mut buffer, (0, 0));
        let mut copied = Framebuffer::new(Size::new(34, 22));
        copied.set_recording(true);
        copied.clear(BACKGROUND);
        copied.fill_rect(Rect::new(5, 3, 26, 17), 0xff);
        copied.copy_through(&buffer, (5, 3), 0xff);
        assert_eq!(copied.pixels(), direct.pixels());
        let Some(Record::Shape(s)) = copied.recording().unwrap().records().first() else {
            panic!("no shape carried");
        };
        assert_eq!(
            (&s.shape, s.clip),
            (
                &Geometry::Polygon(vec![(8, 5), (10, 17), (27, 10)]),
                Rect::new(5, 3, 26, 17)
            )
        );
        let (_, before, direct) = pictures(&direct);
        let (_, _, copied) = pictures(&copied);
        assert!(copied == direct && copied != before);
    }
}
