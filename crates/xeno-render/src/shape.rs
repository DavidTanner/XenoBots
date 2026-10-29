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

/// The fill's clip rectangle (`[DS:0x634]` +6, +8, +0xa, +0xc; bottom and right one
/// past the edge): the whole buffer.
struct Clip {
    top: i16,
    left: i16,
    bottom: i16,
    right: i16,
}

impl Clip {
    /// A point's outside bits: x past `right - 1` (1), x left of the edge (2), y below
    /// the bottom (4, `bottom < y`), y above the top (8), by `less`.
    fn bits(&self, (x, y): Point, less: fn(i16, i16) -> bool) -> u8 {
        u8::from(less(self.right - 1, x))
            | u8::from(less(x, self.left)) << 1
            | u8::from(less(self.bottom, y)) << 2
            | u8::from(less(y, self.top)) << 3
    }
}

/// The bounding box's compare: the sign of the 16-bit difference, wrong when it
/// overflows (`lahf` keeps the sign flag alone).
fn sign_less(a: i16, b: i16) -> bool {
    a.wrapping_sub(b) < 0
}

/// `b - a` per axis, or, if either overflows 16 bits, the difference of the halves.
fn deltas(a: Point, b: Point) -> (i16, i16) {
    match (b.0.checked_sub(a.0), b.1.checked_sub(a.1)) {
        (Some(dx), Some(dy)) => (dx, dy),
        _ => ((b.0 >> 1) - (a.0 >> 1), (b.1 >> 1) - (a.1 >> 1)),
    }
}

/// The edge table: a left and a right x a row, rows 0 to the bottom (writes past it
/// land elsewhere in the original and are dropped here).
struct Table(Vec<[i16; 2]>);

impl Table {
    fn set(&mut self, row: i32, column: usize, x: i16) {
        if let Some(entry) = usize::try_from(row).ok().and_then(|r| self.0.get_mut(r)) {
            entry[column] = x;
        }
    }

    fn fill(&mut self, rows: std::ops::RangeInclusive<i16>, column: usize, x: i16) {
        rows.for_each(|row| self.set(row.into(), column, x));
    }
}

/// `1fe6:6336` (left, column 0) and `1fe6:6461` (right, 1): the edge from `a` to `b`,
/// both inside the clip rectangle, walked from the end with the smaller x.
fn walk(table: &mut Table, column: usize, a: Point, b: Point) {
    // The original compares unsigned, the same for the clipped ends' 0..=bottom.
    let ((mut x, y0), (x1, y1)) = if b.0 < a.0 { (b, a) } else { (a, b) };
    let dx = x1.wrapping_sub(x) as u16;
    let (dy, step) = if y0 >= y1 {
        (y0.wrapping_sub(y1) as u16, -1)
    } else {
        (y1.wrapping_sub(y0) as u16, 1)
    };
    let mut row = i32::from(y0);
    let left = column == 0;
    if dy < dx {
        let mut err = -((dx as i16) >> 1);
        let mut count = dx.wrapping_add(1);
        if left {
            table.set(row, column, x);
        }
        loop {
            x = x.wrapping_add(1);
            count = count.wrapping_sub(1);
            if count == 0 {
                // The left walker's last write lands one row past the end.
                row += i32::from(left) * step;
                return table.set(row, column, x);
            }
            err = err.wrapping_add(dy as i16);
            if err >= 0 {
                if !left {
                    table.set(row, column, x);
                }
                row += step;
                err = err.wrapping_sub(dx as i16);
                if left {
                    table.set(row, column, x);
                }
            }
        }
    }
    let mut err = -((dy as i16) >> 1);
    let mut count = dy.wrapping_add(1);
    x = x.wrapping_add(i16::from(!left));
    loop {
        table.set(row, column, x);
        row += step;
        count = count.wrapping_sub(1);
        if count == 0 {
            return;
        }
        err = err.wrapping_add(dx as i16);
        if err >= 0 {
            x = x.wrapping_add(1);
            err = err.wrapping_sub(dy as i16);
        }
    }
}

/// `gfx_walk_polygon_edge` (`1fe6:60ea`): the edge from `a` to `b` into `column`,
/// through the clipper when the polygon's box needs it.
fn edge(table: &mut Table, clip: &Clip, clipping: bool, column: usize, a: Point, b: Point) {
    // The sign of dx ^ dy is only worked out when clipping; unclipped, the fills
    // below write only the end row, which the walk overwrites.
    let mut opposite = false;
    let (mut a, mut b) = (a, b);
    let (a_y, b_y) = (a.1, b.1);
    let side = |table: &mut Table, x: i16| {
        table.fill(clip.top.max(a_y)..=clip.bottom.min(b_y), column, x);
    };
    if clipping {
        let mut moved = false;
        loop {
            let less = |p: i16, q: i16| p < q;
            let (ca, cb) = (clip.bits(a, less), clip.bits(b, less));
            if ca & cb & 0b1100 != 0 {
                return;
            }
            if ca & cb != 0 {
                return side(table, if ca & 2 != 0 { clip.left } else { clip.right });
            }
            if ca | cb == 0 {
                break;
            }
            if moved {
                return side(
                    table,
                    if a.0 > clip.left {
                        clip.right
                    } else {
                        clip.left
                    },
                );
            }
            moved = true;
            let (dx, dy) = deltas(a, b);
            opposite = (dx ^ dy) < 0;
            a = onto(clip, a, ca, dx, dy);
            b = onto(clip, b, cb, dx, dy);
        }
    }
    let (before, after) = if opposite {
        (clip.right, clip.left)
    } else {
        (clip.left, clip.right)
    };
    table.fill(clip.top.max(a_y)..=a.1 - 1, column, before);
    table.fill(b.1..=clip.bottom.min(b_y), column, after);
    walk(table, column, a, b);
}

/// `1fe6:623c`: an endpoint with outside bits `bits` moved onto the clip rectangle along
/// the edge (`dx`, `dy`).
fn onto(clip: &Clip, (x0, y0): Point, bits: u8, dx: i16, dy: i16) -> Point {
    // `y0 + (to - x0) * num / den`, 16-bit wrapping, the division truncating.
    // shortcut: when the quotient overflows 16 bits (or a halved difference is 0) the
    // original's `idiv` faults; it wraps (or is 0) here. Only edges over 32,767 pixels
    // long reach it; upgrade if a capture shows what the game does.
    let along = |from: i16, to: i16, at: i16, num: i16, den: i16| {
        let q = (i32::from(to.wrapping_sub(at)) * i32::from(num))
            .checked_div(den.into())
            .unwrap_or(0);
        from.wrapping_add(q as i16)
    };
    let (mut x, mut y) = (x0, y0);
    if bits & 1 != 0 {
        x = clip.right - 1;
        y = along(y0, x, x0, dy, dx);
    } else if bits & 2 != 0 {
        x = clip.left;
        y = along(y0, x, x0, dy, dx);
    }
    if bits & 4 != 0 && y >= clip.bottom {
        y = clip.bottom - 1;
        x = along(x0, y, y0, dx, dy);
    } else if bits & 8 != 0 && y < clip.top {
        y = clip.top;
        x = along(x0, y, y0, dx, dy);
    }
    (x, y)
}

/// `gfx_fill_polygon` (`1fe6:4d7d`): a polygon in one colour, clipped to the buffer, if
/// it faces the viewer: with `t` the top corner (the highest; of those the leftmost,
/// the last one listed on a tie), `n` the next and `p` the one before, `(n.x - t.x) *
/// (p.y - t.y) < (n.y - t.y) * (p.x - t.x)`, so its corners run anticlockwise on
/// screen. The edges and spans are `docs/render.md`'s, "The polygon fill (T-121)".
// shortcut: rows no edge writes (only for corners that are not convex) are left
// empty, where the original fills them from what an earlier polygon left in its
// table; keep the table between calls if a capture needs it.
pub fn fill_polygon(fb: &mut Framebuffer, corners: &[Point], colour: u8) {
    let Some(&(_, first_y)) = corners.first() else {
        return;
    };
    let (width, height) = (fb.surface().width, fb.surface().height);
    let (mut top, mut min_y, mut max_y, mut min_x, mut max_x) = (0, i16::MAX, first_y, i16::MAX, 0);
    for (i, &(x, y)) in corners.iter().enumerate() {
        if y <= min_y {
            if y < min_y || x <= corners[top].0 {
                top = i;
            }
            min_y = y;
        } else {
            max_y = max_y.max(y);
        }
        (min_x, max_x) = (min_x.min(x), max_x.max(x));
    }
    let clip = Clip {
        top: 0,
        left: 0,
        bottom: height as i16,
        right: width as i16,
    };
    let (low, high) = (
        clip.bits((min_x, min_y), sign_less),
        clip.bits((max_x, max_y), sign_less),
    );
    let n = corners.len();
    let (t, next, prev) = (
        corners[top],
        corners[(top + 1) % n],
        corners[(top + n - 1) % n],
    );
    if low & high != 0 || !faces(t, next, prev) {
        return;
    }
    let mut table = Table(vec![[0; 2]; height + 1]);
    for (column, forward) in [(0, 1), (1, n - 1)] {
        let mut i = top;
        loop {
            let j = (i + forward) % n;
            edge(
                &mut table,
                &clip,
                low | high != 0,
                column,
                corners[i],
                corners[j],
            );
            i = j;
            if corners[i].1 >= max_y {
                break;
            }
        }
    }
    let rows = min_y.max(clip.top)..=max_y.min(clip.bottom - 1);
    let w = width as i16;
    for (row, &[left, right]) in table.0.iter().enumerate() {
        // The clipped edges keep both in 0..=width, where the original's unsigned
        // compare is this one.
        let (left, right) = (left.clamp(0, w) as usize, right.clamp(0, w) as usize);
        if rows.contains(&(row as i16)) && right > left {
            fb.pixels_mut()[row * width + left..row * width + right].fill(colour);
        }
    }
}

/// The facing test, in 32 bits; when a corner difference overflows 16 bits, of the
/// halved coordinates.
fn faces(t: Point, n: Point, p: Point) -> bool {
    let pairs = [(n.0, t.0), (n.1, t.1), (p.0, t.0), (p.1, t.1)];
    let [nx, ny, px, py] = if pairs.iter().any(|&(a, b)| a.checked_sub(b).is_none()) {
        pairs.map(|(a, b)| i32::from((a >> 1) - (b >> 1)))
    } else {
        pairs.map(|(a, b)| i32::from(a - b))
    };
    (nx * py).wrapping_sub(ny * px) < 0
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
        // The edges' own pixels are inside on both sides.
        fill_polygon(&mut fb, &[(3, 1), (1, 4), (5, 4)], 7);
        assert_eq!(
            ascii(&fb),
            [
                "........", "...7....", "...77...", "..7777..", ".77777..", "........"
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

    fn filled(size: (usize, usize), corners: &[Point]) -> Framebuffer {
        let mut fb = Framebuffer::new(Size::new(size.0, size.1));
        fill_polygon(&mut fb, corners, 7);
        fb
    }

    #[test]
    fn a_left_edge_walked_upward_writes_the_row_above_its_corner() {
        // Two down-left edges on the left chain, the second x-major: walked from its
        // lower end, it writes 8 + 1 into row 2, where the first edge's row starts at 10.
        let fb = filled((24, 7), &[(16, 0), (8, 3), (2, 5), (20, 5)]);
        assert_eq!(ascii(&fb)[2], ".........7777777777.....");
        insta::assert_snapshot!(ascii(&fb).join("\n"));
    }

    #[test]
    fn a_clipped_edge_is_walked_from_where_it_meets_the_edge() {
        // (2, 0) to (-4, 4) meets x = 0 at y = 4 + 4 * 4 / -6 = 2 (truncated); the rows
        // below get the left edge's x; the walk is (0, 2) to (2, 0).
        let fb = filled((8, 6), &[(2, 0), (-4, 4), (6, 4)]);
        assert_eq!(
            ascii(&fb),
            [
                "..7.....", ".777....", "77777...", "777777..", "7777777.", "........"
            ]
        );
        // (3, 0) to (9, 8) meets x = 7 at y = 8 - 2 * 8 / 6 = 6, the bottom itself: it is
        // moved again, to y = 5 and x = 9 - 3 * 6 / 8 = 7.
        let fb = filled((8, 6), &[(3, 0), (0, 5), (9, 8)]);
        assert_eq!(
            ascii(&fb),
            [
                "...7....", "...77...", "..7777..", ".777777.", ".777777.", "77777777"
            ]
        );
        // (2, 7) to (3, 9) is wholly below: it writes nothing, where moved onto row 5 it
        // would start that row at 1.
        let fb = filled((8, 6), &[(4, 0), (2, 7), (3, 9), (7, 5)]);
        assert_eq!(ascii(&fb)[5], "..777777");
        // (2, -3) to (-3, 2) passes outside the corner: moved once, to (-1, 0) and (0, -1),
        // it is still out, and its rows get the left edge's x, as `a` is not past it.
        let fb = filled((8, 6), &[(2, -3), (-3, 2), (4, 5), (7, -1)]);
        assert_eq!(ascii(&fb)[..3], ["77777777", "77777777", "7777777."]);
        // Clipped on every side.
        let fb = filled((16, 10), &[(6, -3), (-4, 3), (9, 13), (20, 5)]);
        insta::assert_snapshot!(ascii(&fb).join("\n"));
    }

    #[test]
    fn overflowing_differences_are_taken_of_the_halves() {
        // Exactly, -1 (facing); halved, (8500 - -8500) * 0 - 0 * 8500 = 0 (not).
        let fb = filled((8, 6), &[(-17000, 0), (17000, 1), (17001, 1)]);
        assert!(fb.pixels().iter().all(|&p| p == 0));
    }

    #[test]
    fn the_box_test_reads_the_sign_of_the_difference() {
        // 6 - -32765 overflows to negative: the box's top counts as below the
        // bottom, as its bottom (300) is, so nothing is drawn.
        let fb = filled((8, 6), &[(3, -32765), (-100, 300), (100, 300)]);
        assert!(fb.pixels().iter().all(|&p| p == 0));
    }

    /// The traced rules restated per pixel (`docs/render.md`, "The polygon fill"), for
    /// corners inside the buffer: an edge is the `D + 1` pixels of a line from its end
    /// with the smaller x, the minor axis moved `(i * m - D / 2 + D) / D` (at most `i`)
    /// after step `i`; a row's left x is the first pixel's of the last left-chain edge on it, the
    /// right x one past the last pixel's of the last right-chain edge on it, and an
    /// x-major left edge writes its last x + 1 one row past its last pixel.
    fn reference(corners: &[Point], (w, h): (usize, usize)) -> Vec<bool> {
        let n = corners.len();
        let c = |i: usize| (i64::from(corners[i].0), i64::from(corners[i].1));
        let top = (0..n).rev().min_by_key(|&i| (c(i).1, c(i).0)).unwrap();
        let (t, next, prev) = (c(top), c((top + 1) % n), c((top + n - 1) % n));
        let mut mask = vec![false; w * h];
        if (next.0 - t.0) * (prev.1 - t.1) >= (next.1 - t.1) * (prev.0 - t.0) {
            return mask;
        }
        let max_y = (0..n).map(|i| c(i).1).max().unwrap();
        let mut rows = vec![[0i64; 2]; h + 2];
        for (column, forward) in [(0, 1), (1, n - 1)] {
            let mut i = top;
            loop {
                let j = (i + forward) % n;
                let (a, b) = (c(i), c(j));
                let (p0, p1) = if b.0 < a.0 { (b, a) } else { (a, b) };
                let s = if p0.1 >= p1.1 { -1 } else { 1 };
                let (dx, dy) = (p1.0 - p0.0, (p1.1 - p0.1).abs());
                let (major, minor) = (dx.max(dy), dx.min(dy));
                // At most one minor step a major step (only `D = 1` meets it).
                let moved = |i: i64| match i {
                    0 => 0,
                    _ => i.min((i * minor - major / 2 + major) / major),
                };
                let pixels: Vec<(i64, i64)> = (0..=major)
                    .map(|i| match dy < dx {
                        true => (p0.0 + i, p0.1 + s * moved(i)),
                        false => (p0.0 + moved(i), p0.1 + s * i),
                    })
                    .collect();
                let mut ends = std::collections::BTreeMap::new();
                for &(x, y) in &pixels {
                    let e = ends.entry(y).or_insert((x, x));
                    *e = (e.0.min(x), e.1.max(x));
                }
                for (y, (first, last)) in ends {
                    rows[(y + 1) as usize][column] = if column == 0 { first } else { last + 1 };
                }
                if column == 0 && dy < dx {
                    let (x, y) = pixels[pixels.len() - 1];
                    rows[(y + s + 1) as usize][0] = x + 1;
                }
                i = j;
                if c(i).1 >= max_y {
                    break;
                }
            }
        }
        for y in t.1..=max_y.min(h as i64 - 1) {
            let [left, right] = rows[(y + 1) as usize];
            for x in left..right {
                mask[y as usize * w + x as usize] = true;
            }
        }
        mask
    }

    proptest::proptest! {
        #[test]
        fn polygons_inside_fill_as_the_traced_rules(
            corners in proptest::collection::vec((0i16..20, 0i16..14), 3..=4)
        ) {
            let fb = filled((20, 14), &corners);
            let mask: Vec<bool> = fb.pixels().iter().map(|&p| p != 0).collect();
            proptest::prop_assert_eq!(mask, reference(&corners, (20, 14)));
        }

        #[test]
        fn polygons_anywhere_stay_in_the_buffer(
            corners in proptest::collection::vec((-40i16..60, -40i16..60), 3..=8)
        ) {
            let fb = filled((20, 14), &corners);
            proptest::prop_assert!(fb.pixels().iter().all(|&p| p == 0 || p == 7));
        }
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
