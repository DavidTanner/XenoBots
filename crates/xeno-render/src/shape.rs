//! Terrain shapes in the 3D view (`gfx_draw_terrain_shape`, `1000:1b03`;
//! `docs/formats/usg.md`, "Drawing"): a `MAPP` record's program of op bytes makes
//! points from the record's sizes, projected one by one (`1000:20f4`), takes midpoints
//! of projected points, and fills triangles and quads (`1fe6:4d7d`).

use crate::Framebuffer;
use crate::view3d::{Camera, GameFloat};

/// A terrain node's shape: the record's sizes (8.8) and its program from +0x16.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub radius: u16,
    pub diagonal: u16,
    pub height: u16,
    pub program: Vec<u8>,
}

/// The built-in programs ops 0x12 and 0x14 (or 0x16) run.
#[derive(Debug, Clone, Copy)]
pub struct Builtins<'a> {
    pub block: &'a [u8],
    pub hill: &'a [u8],
}

/// A screen point: x, y.
type Point = (i16, i16);

/// The program's state: its point slots by their byte offset / 4 (the stack frame's
/// slots below `BP - 0x38`; one never set reads as (0, 0)) and the height the next
/// point is made at.
struct Run<'a> {
    camera: &'a Camera,
    centre: [i32; 3],
    shape: &'a Shape,
    height: i16,
    points: [Point; 64],
}

impl Run<'_> {
    fn slot(offset: u8) -> usize {
        usize::from(offset >> 2)
    }

    fn point(&self, offset: u8) -> Point {
        self.points[Self::slot(offset)]
    }

    /// `1000:20f4`: the offset (`dx`, `dy`, 16.16) from the centre, at the current
    /// height (8.8) above the centre's z, to the buffer. In front, divided by the larger
    /// of the lateral and depth distances; behind, put the focal factor's width to the
    /// side.
    fn project(&self, dx: i32, dy: i32, (width, height): (u16, u16)) -> Point {
        let c = self.camera;
        let x = self.centre[0].wrapping_add(dx).wrapping_sub(c.x);
        let y = self.centre[1].wrapping_add(dy).wrapping_sub(c.y);
        let z = (i32::from(self.height) << 8).wrapping_add(self.centre[2].wrapping_sub(c.z));
        let (half_w, half_h) = ((width / 2) as i16, (height / 2) as i16);
        let (lateral, depth) = c.rotate(x, y);
        let f = c.focal;
        let fx = GameFloat::from_raw(lateral);
        if depth < 0 {
            let d = GameFloat {
                negative: false,
                ..fx
            };
            let side = GameFloat {
                negative: f.negative ^ fx.negative,
                ..f
            };
            let up = GameFloat::from_raw(z).times(f).over(d).to_int();
            return (side.to_int().wrapping_add(half_w), half_h.wrapping_sub(up));
        }
        let far = lateral.unsigned_abs().max(depth.unsigned_abs());
        let d = GameFloat {
            negative: false,
            ..GameFloat::from_raw(far as i32)
        };
        let across = fx.times(f).over(d).to_int();
        let up = GameFloat::from_raw(z).times(f).over(d).to_int();
        (
            across.checked_add(half_w).unwrap_or(0x7fff),
            up.wrapping_neg().checked_add(half_h).unwrap_or(0x7fff),
        )
    }
}

/// Runs `program` (and the built-ins it calls) into `fb`; `false` when an op ends the
/// whole shape.
fn run(fb: &mut Framebuffer, r: &mut Run, program: &[u8], builtins: Builtins) -> bool {
    let size = (fb.surface().width as u16, fb.surface().height as u16);
    let mut bytes = program.iter().copied();
    let mut next = || bytes.next().unwrap_or(0);
    loop {
        match next() {
            0 => return true,
            0x12 => {
                if !run(fb, r, builtins.block, builtins) {
                    return false;
                }
            }
            0x14 | 0x16 => {
                if !run(fb, r, builtins.hill, builtins) {
                    return false;
                }
            }
            // `1000:1cb0`: eight points on a ring `height` below the centre, at the
            // radius on the axes and at the diagonal size on both axes between, from +y
            // anticlockwise, in slots -0x1c down to -0x38; then the centre itself, at
            // height 0 (kept for later ops), in slot -0x18.
            0x26 => {
                let (radius, diagonal) = (
                    i32::from(r.shape.radius) << 8,
                    i32::from(r.shape.diagonal) << 8,
                );
                r.height = r.height.wrapping_neg();
                let ring = [
                    (0, radius),
                    (-diagonal, diagonal),
                    (-radius, 0),
                    (-diagonal, -diagonal),
                    (0, -radius),
                    (diagonal, -diagonal),
                    (radius, 0),
                    (diagonal, diagonal),
                ];
                for (k, (dx, dy)) in ring.into_iter().enumerate() {
                    r.points[Run::slot(0xe4u8.wrapping_sub(4 * k as u8))] = r.project(dx, dy, size);
                }
                r.height = 0;
                r.points[Run::slot(0xe8)] = r.project(0, 0, size);
            }
            // `1000:1fc1`: the midpoint of two points.
            0x1c => {
                let (to, a, b) = (next(), next(), next());
                let (a, b) = (r.point(a), r.point(b));
                let mid = |a: i16, b: i16| ((i32::from(a) + i32::from(b)) >> 1) as i16;
                r.points[Run::slot(to)] = (mid(a.0, b.0), mid(a.1, b.1));
            }
            // `1000:1f75`: a quarter of the way from the first point to the second.
            0x1e => {
                let (to, a, b) = (next(), next(), next());
                let (a, b) = (r.point(a), r.point(b));
                let quarter =
                    |a: i16, b: i16| a.wrapping_add(((i32::from(b) - i32::from(a)) >> 2) as i16);
                r.points[Run::slot(to)] = (quarter(a.0, b.0), quarter(a.1, b.1));
            }
            // `1000:1ee6`, `1000:1f2e`: a triangle or quad of points in a colour.
            op @ (0x0e | 0x10) => {
                let corners: Vec<Point> = (0..if op == 0x0e { 3 } else { 4 })
                    .map(|_| r.point(next()))
                    .collect();
                fill_polygon(fb, &corners, next());
            }
            0x0a => (0..8).for_each(|_| {
                next();
            }),
            // 0, 0x0c, 0x18 and 0x1a end the whole shape (`1000:1ee2`).
            // shortcut: the point ops 0x02 to 0x08, 0x22 and 0x24 are not traced (no
            // stock record uses them, only the block, which nothing sees); they end it too.
            _ => return false,
        }
    }
}

/// `gfx_draw_terrain_shape`: `shape`'s program from the centre (x, y, z; 16.16).
pub fn draw(
    fb: &mut Framebuffer,
    camera: &Camera,
    centre: [i32; 3],
    shape: &Shape,
    builtins: Builtins,
) {
    let mut r = Run {
        camera,
        centre,
        shape,
        height: shape.height as i16,
        points: [(0, 0); 64],
    };
    run(fb, &mut r, &shape.program, builtins);
}

/// `1fe6:4d7d`: a convex polygon in one colour, clipped to the buffer, if it faces the
/// viewer: with `t` the top corner (the highest; of those the leftmost, the last one
/// listed on a tie), `n` the next and `p` the one before, `(n.x - t.x) * (p.y - t.y) <
/// (n.y - t.y) * (p.x - t.x)`, so its corners run anticlockwise on screen.
// shortcut: the edge walker (`1fe6:60ea`) is not traced; spans here run from the
// leftmost to the rightmost edge crossing at each row, right end excluded as the
// original's span fill does. Trace it for pixel-exact edges.
pub fn fill_polygon(fb: &mut Framebuffer, corners: &[Point], colour: u8) {
    let n = corners.len();
    let Some(top) = (0..n).rev().min_by_key(|&i| (corners[i].1, corners[i].0)) else {
        return;
    };
    let p = |i: usize| (i64::from(corners[i].0), i64::from(corners[i].1));
    let (t, prev, next) = (p(top), p((top + n - 1) % n), p((top + 1) % n));
    if (next.0 - t.0) * (prev.1 - t.1) - (next.1 - t.1) * (prev.0 - t.0) >= 0 {
        return;
    }
    let (width, height) = (fb.surface().width as i64, fb.surface().height as i64);
    let low = corners.iter().map(|c| i64::from(c.1)).max().unwrap_or(0);
    let stride = fb.surface().width;
    for y in t.1.max(0)..=low.min(height - 1) {
        let crossings = (0..n).flat_map(|i| {
            let (a, b) = (p(i), p((i + 1) % n));
            let (lo, hi) = if a.1 <= b.1 { (a, b) } else { (b, a) };
            match (y < lo.1 || y > hi.1, hi.1 == lo.1) {
                (true, _) => vec![],
                (false, true) => vec![lo.0, hi.0],
                (false, false) => vec![lo.0 + (hi.0 - lo.0) * (y - lo.1) / (hi.1 - lo.1)],
            }
        });
        let (left, right) = crossings.fold((i64::MAX, i64::MIN), |(l, r), x| (l.min(x), r.max(x)));
        let (left, right) = (left.max(0), right.min(width));
        if left < right {
            let row = y as usize * stride;
            fb.pixels_mut()[row + left as usize..row + right as usize].fill(colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;
    use crate::view3d::{Trig, focal};
    use xeno_formats::view3d::Float;

    /// Looking along +y from the origin at height 0.
    fn camera() -> Camera {
        Camera {
            x: 0,
            y: 0,
            z: 0,
            heading: 0,
            sin: Trig::default(),
            cos: Trig {
                one: true,
                ..Trig::default()
            },
            focal: focal(Float {
                mantissa: 230,
                exponent: 0,
            }),
            viewer: 0,
        }
    }

    fn shape(program: &[u8]) -> Shape {
        Shape {
            radius: 0x100,
            diagonal: 0x80,
            height: 0x40,
            program: program.to_vec(),
        }
    }

    /// The buffer as text: `.` for 0, a hex digit for 1 to 15.
    fn ascii(fb: &Framebuffer) -> Vec<String> {
        fb.pixels()
            .chunks(fb.surface().width)
            .map(|row| {
                row.iter()
                    .map(|&p| char::from_digit(p.into(), 16).unwrap_or('#'))
                    .map(|c| if c == '0' { '.' } else { c })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn points_project_by_the_larger_of_lateral_and_depth() {
        let c = camera();
        let s = shape(&[]);
        let r = Run {
            camera: &c,
            centre: [0, 10 << 16, 0],
            shape: &s,
            height: 0x100,
            points: [(0, 0); 64],
        };
        // 10 ahead, 1 up: 23 pixels a unit at 10 units.
        assert_eq!(r.project(0, 0, (100, 60)), (50, 30 - 23));
        // 1 right: 23 to the right; 20 right of 10 ahead, divided by 20.
        assert_eq!(r.project(1 << 16, 0, (100, 60)), (73, 7));
        assert_eq!(r.project(20 << 16, 0, (100, 60)), (50 + 230, 30 - 11));
        // Behind (4 units): the focal factor's width to the side of the offset, the
        // height over the lateral distance (2 units).
        assert_eq!(
            r.project(2 << 16, -14 << 16, (100, 60)),
            (50 + 230, 30 - 115)
        );
        assert_eq!(
            r.project(-2 << 16, -14 << 16, (100, 60)),
            (50 - 230, 30 - 115)
        );
    }

    #[test]
    fn polygons_fill_when_anticlockwise_on_screen() {
        let mut fb = Framebuffer::new(Size::new(8, 6));
        // Top (3, 1), then down-left, then down-right: anticlockwise on screen. Each
        // row runs from the left edge up to, not including, the right one.
        fill_polygon(&mut fb, &[(3, 1), (1, 4), (5, 4)], 7);
        assert_eq!(
            ascii(&fb),
            [
                "........", "........", "........", "..77....", ".7777...", "........"
            ]
        );
        // The other way round faces away.
        let mut back = Framebuffer::new(Size::new(8, 6));
        fill_polygon(&mut back, &[(3, 1), (5, 4), (1, 4)], 7);
        assert!(back.pixels().iter().all(|&p| p == 0));
        // Clipped to the buffer.
        let mut fb = Framebuffer::new(Size::new(4, 3));
        fill_polygon(&mut fb, &[(-5, -5), (-5, 9), (9, 9), (9, -5)], 2);
        assert!(fb.pixels().iter().all(|&p| p == 2));
    }

    #[test]
    fn programs_make_points_midpoints_and_fills() {
        let c = camera();
        let builtins = Builtins {
            block: &[0x0c],
            hill: &[0x26, 0],
        };
        let mut fb = Framebuffer::new(Size::new(16, 8));
        // The hill's points, then a midpoint into -4 and a quarter point into -8 from
        // the nearest ring point (4) to the centre, then a quad.
        let s = shape(&[
            0x14, 0x1c, 0xfc, 0xd4, 0xe8, 0x1e, 0xf8, 0xd4, 0xe8, 0x1c, 0xf4, 0xe4, 0xe8, 0x10,
            0xd4, 0xe4, 0xf8, 0xfc, 5, 0,
        ]);
        let mut r = Run {
            camera: &c,
            centre: [0, 10 << 16, 0],
            shape: &s,
            height: 0x40,
            points: [(0, 0); 64],
        };
        assert!(run(&mut fb, &mut r, &s.program, builtins));
        // The centre at height 0 is on the horizon; the ring a quarter unit below it,
        // 11 and 9 units away.
        assert_eq!(r.point(0xe8), (8, 4));
        let (far, near) = (r.point(0xe4), r.point(0xd4));
        assert_eq!((far, near), ((8, 4 + 5), (8, 4 + 6)));
        assert_eq!(r.point(0xfc), (8, 7));
        assert_eq!(r.point(0xf8), (8, 8));
        // Midpoints round down: (9 + 4) / 2.
        assert_eq!(r.point(0xf4), (8, 6));
        assert_eq!(r.height, 0);
        // The block runs a 0x0c: the whole shape ends there.
        let mut r = Run {
            points: [(0, 0); 64],
            ..r
        };
        assert!(!run(&mut fb, &mut r, &[0x12, 0x26, 0], builtins));
        assert_eq!(r.point(0xe8), (0, 0));
    }
}
