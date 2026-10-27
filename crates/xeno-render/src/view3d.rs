//! The mission's 3D view as `xeno.exe`'s step list draws it (`docs/render.md`): the
//! ground, the sky strip, then the render list far to near, each model culled,
//! projected and scaled with the game's own floating-point numbers.
//!
//! The caller builds the nodes from the world (their distance and bearing from the
//! camera are the sim's) and gives the camera's sine and cosine; everything after that
//! is here, so it is tested without a window or game files.

use std::cmp::Reverse;

use xeno_formats::ban::Sky;
use xeno_formats::view3d::{Float, View3d};

use crate::{Framebuffer, LoadedModel, ModelScale, ModelTarget, Rect, model_facing};

/// The ground step's colour, and the sky's when there is no strip.
pub const GROUND: u8 = 0x3f;
pub const FLAT_SKY: u8 = 0x3e;

/// A number in the game's floating-point format (`1000:0cd8` to `0e9d`): `mantissa *
/// 2^exponent`, negated when `negative`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameFloat {
    pub mantissa: u16,
    pub exponent: i8,
    pub negative: bool,
}

/// What the overflowing conversions and divisions give.
const OVERFLOW: u16 = 0x6fff;

impl GameFloat {
    /// A table's float (`xeno_formats::view3d`), positive.
    pub fn of(f: Float) -> GameFloat {
        GameFloat {
            mantissa: f.mantissa,
            exponent: f.exponent,
            negative: false,
        }
    }

    /// The 32-bit integer `v` (a 16.16's raw value; `1000:0d2d`).
    pub fn from_raw(v: i32) -> GameFloat {
        let a = v.unsigned_abs();
        let (mantissa, exponent) = match a {
            0..0x1_0000 => (a, 0),
            0x1_0000..0x100_0000 => (a >> 8, 8),
            _ => (a >> 16, 16),
        };
        GameFloat {
            mantissa: mantissa as u16,
            exponent,
            negative: v < 0,
        }
    }

    /// `1000:0e11`.
    pub fn times(self, other: GameFloat) -> GameFloat {
        let p = u32::from(self.mantissa) * u32::from(other.mantissa);
        let e = self.exponent.wrapping_add(other.exponent);
        let (mantissa, exponent) = match p {
            0..0x1_0000 => (p, e),
            0x1_0000..0x100_0000 => (p >> 8, e.wrapping_add(8)),
            _ => (p >> 16, e.wrapping_add(16)),
        };
        GameFloat {
            mantissa: mantissa as u16,
            exponent,
            negative: self.negative ^ other.negative,
        }
    }

    /// `1000:0e3c`: the dividend shifted up by whole bytes until the quotient fills
    /// its 16 bits, truncated.
    pub fn over(self, other: GameFloat) -> GameFloat {
        let e = self.exponent.wrapping_sub(other.exponent);
        let negative = self.negative ^ other.negative;
        let (ma, mb) = (u32::from(self.mantissa), u32::from(other.mantissa));
        let (mantissa, exponent) = match mb {
            0 => (OVERFLOW, 0x38),
            1 => (self.mantissa, e),
            _ => {
                let k = if ma >> 8 >= mb {
                    0
                } else if ma >= mb {
                    8
                } else if ma >= 0x100 || ma << 8 >= mb {
                    16
                } else {
                    24
                };
                (((ma << k) / mb) as u16, e.wrapping_sub(k as i8))
            }
        };
        GameFloat {
            mantissa,
            exponent,
            negative,
        }
    }

    /// `1000:0dcf`: to a 16-bit integer, 0x6fff (signed) when it does not fit, 0
    /// below 2^-16.
    pub fn to_int(self) -> i16 {
        let m = self.mantissa;
        let value = match self.exponent {
            0 => (m < 0x8000).then_some(m),
            e if e > 0 => (m < 0x80 && e < 16).then_some(m << 8),
            e if e > -16 => Some(m >> 8),
            _ => return 0,
        };
        let value = value.unwrap_or(OVERFLOW) as i16;
        if self.negative {
            value.wrapping_neg()
        } else {
            value
        }
    }
}

/// The main view's focal factor from its table entry (`gfx_set_view_camera`, zoom 1,
/// no shift): the word normalised to its top bit, kept to 16 bits.
pub fn focal(f: Float) -> GameFloat {
    let mut v = u32::from(f.mantissa);
    let mut n = 0i8;
    if v != 0 {
        while v >> 24 == 0 {
            (v, n) = (v << 8, n + 8);
        }
        while v >> 31 == 0 {
            (v, n) = (v << 1, n + 1);
        }
        v >>= n & 7;
    }
    GameFloat {
        mantissa: (v >> 16) as u16,
        exponent: (16 - (n & !7)).wrapping_add(f.exponent),
        negative: false,
    }
}

/// A sine or cosine as `sim_sine_magnitude` gives it: a magnitude in 1/65,536ths, or
/// exactly 1, and a sign.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Trig {
    pub magnitude: u16,
    pub one: bool,
    pub negative: bool,
}

/// `|a|` (a 16.16) times `t`, 48 bits, as `1000:133c` keeps precision.
fn product(a: u32, t: Trig) -> i64 {
    let m = i64::from(t.magnitude);
    let a64 = i64::from(a);
    match a {
        _ if t.one => a64 << 16,
        0..0x1_0000 => a64 * m,
        0x1_0000..0x100_0000 => ((a64 >> 8) * m) << 8,
        _ => ((a64 >> 16) * m) << 16,
    }
}

/// The camera: the viewed robot's position (z with its eye height), heading, sine and
/// cosine of the heading, the focal factor, and the robot itself, never drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub heading: u16,
    pub sin: Trig,
    pub cos: Trig,
    pub focal: GameFloat,
    pub viewer: u16,
}

impl Camera {
    /// `1000:11c7`: the offset `(dx, dy)` turned to the camera, as 16.16s: lateral `X`
    /// (positive to the right) and depth `Z`, each sum of products kept in 48 bits.
    pub fn rotate(&self, dx: i32, dy: i32) -> (i32, i32) {
        let term = |a: i32, t: Trig, flip: bool| {
            let p = product(a.unsigned_abs(), t);
            if (a < 0) ^ t.negative ^ flip { -p } else { p }
        };
        let x = term(dx, self.cos, false) + term(dy, self.sin, true);
        let z = term(dx, self.sin, false) + term(dy, self.cos, false);
        ((x >> 16) as i32, (z >> 16) as i32)
    }

    /// `1000:0378`: the octant the camera looks along.
    fn octant(&self) -> u8 {
        (0x4000u16.wrapping_sub(self.heading) >> 13) as u8
    }
}

/// `sim_get_octant` (`1000:084a`): the octant of the vector `(dx, dy)`, 0 from +x
/// towards +y, anticlockwise; a vector on a boundary takes the higher one.
pub fn octant(dx: i32, dy: i32) -> u8 {
    let (mut a, mut b) = (dy as u32, dx as u32);
    let mut o = 0u8;
    if (a as i32) < 0 {
        (o, a, b) = (2, a.wrapping_neg(), b.wrapping_neg());
    }
    if (b as i32) < 0 {
        (o, a, b) = (o + 1, b.wrapping_neg(), a);
    }
    o <<= 1;
    if let Some(rest) = a.checked_sub(b) {
        if b.wrapping_mul(2).wrapping_add(rest) == 0 {
            return 0;
        }
        o += 1;
    }
    o
}

/// One render node of type 2 or 6 (`docs/render.md`, "A node").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderNode {
    /// The object's slot: the camera's own is not drawn.
    pub object: u16,
    /// The model number (0, 2, ... 20).
    pub model: u8,
    pub heading: u16,
    /// x, y and z (the object's z with its model height), 16.16.
    pub position: [i32; 3],
    /// The object's radius (+0x14); only its high byte counts.
    pub radius: u16,
    /// From the camera, 16.16 (`sim_approx_distance_between`).
    pub distance: u32,
    /// From the camera to the node (`sim_get_bearing`).
    pub bearing: u16,
    /// One frame per animation.
    pub frames: Vec<u8>,
    /// A type 6 node's own size factor.
    pub size: Option<u16>,
}

impl RenderNode {
    /// +0x1a: the distance less the radius's high byte, shifted to 16.16, not below 0.
    pub fn near(&self) -> u32 {
        self.distance
            .saturating_sub(u32::from(self.radius & 0xff00))
    }
}

/// The build step's order (`gfx_build_render_list`): far to near by
/// [`RenderNode::near`], nodes at the same distance in the order they were made.
pub fn sort_far_to_near(nodes: &mut [RenderNode]) {
    nodes.sort_by_key(|n| Reverse(n.near()));
}

/// Where `gfx_draw_model` puts a node in a `width` by `height` buffer, and its scale,
/// or `None` when one of its tests skips it (`docs/render.md`, "Drawing the list").
/// `animated` is whether the model has animations.
pub fn place(
    view: &View3d,
    camera: &Camera,
    node: &RenderNode,
    animated: bool,
    (width, height): (u16, u16),
) -> Option<((i32, i32), u16)> {
    let [x, y, z] = node.position;
    let (dx, dy) = (x.wrapping_sub(camera.x), y.wrapping_sub(camera.y));
    let d = octant(dx, dy).wrapping_sub(camera.octant()) as i8;
    if node.object == camera.viewer
        || (d.unsigned_abs().wrapping_add(2) & 7) > 4
        || !animated
        || node.distance >> 16 >= u32::from(view.max_distance)
    {
        return None;
    }
    let p = camera.focal.over(GameFloat::from_raw(node.distance as i32));
    let table = view.model_sizes.get(usize::from(node.model / 2));
    let size = match node.size.or(table.copied()).unwrap_or(0x100) {
        0x100 => GameFloat::of(view.default_size),
        s => GameFloat::of(Float {
            mantissa: s,
            exponent: 0,
        }),
    };
    let mut scaled = size.times(p);
    scaled.exponent = scaled.exponent.wrapping_add(8);
    let scale = scaled.to_int() as u16;
    let (lateral, depth) = camera.rotate(dx, dy);
    if scale == 0 || depth < 0 {
        return None;
    }
    let project = |v: i32| GameFloat::from_raw(v).times(p).to_int();
    let ax = project(lateral).wrapping_add((width / 2) as i16);
    let ay = ((height / 2) as i16).wrapping_sub(project(z.wrapping_sub(camera.z)));
    Some(((ax.into(), ay.into()), scale))
}

impl Framebuffer {
    /// The ground step (`1000:2bc3`): colour 0x3f from the middle byte to the end.
    pub fn fill_ground(&mut self) {
        let pixels = self.pixels_mut();
        let half = pixels.len() / 2;
        pixels[half..].fill(GROUND);
    }

    /// `gfx_draw_sky` at detail 1 (`docs/formats/b@n.md`, "Sky strip"): the strip's
    /// last `ceil(h / 2)` rows over the upper rows, wrapping every 342 columns from
    /// `heading / 64 - w / 2`; without a strip, the upper `h / 2` rows 0x3e and the
    /// rest 0x3f.
    pub fn draw_sky(&mut self, sky: Option<&Sky>, heading: u16) {
        let width = self.surface().width;
        let height = self.surface().height;
        let pixels = self.pixels_mut();
        let Some(sky) = sky else {
            let (upper, lower) = pixels.split_at_mut(width * (height / 2));
            upper.fill(FLAT_SKY);
            lower.fill(GROUND);
            return;
        };
        let rows = height.div_ceil(2).min(Sky::HEIGHT);
        let first = (i64::from(heading / 64) - (width / 2) as i64).rem_euclid(Sky::WIDTH as i64);
        for (y, row) in pixels.chunks_mut(width).take(rows).enumerate() {
            let from = &sky.pixels[(Sky::HEIGHT - rows + y) * Sky::WIDTH..][..Sky::WIDTH];
            for (x, p) in row.iter_mut().enumerate() {
                *p = from[(first as usize + x) % Sky::WIDTH];
            }
        }
    }

    /// The main view's steps after the camera and the build: the ground, the sky,
    /// then each node far to near (`nodes` already sorted) with its model, by model
    /// number / 2 in `models`, at its facing (heading less bearing) and frames.
    pub fn draw_view(
        &mut self,
        view: &View3d,
        camera: &Camera,
        nodes: &[RenderNode],
        models: &[Option<LoadedModel>],
        sky: Option<&Sky>,
    ) {
        let size = self.surface();
        self.fill_ground();
        self.draw_sky(sky, camera.heading);
        let clip = Rect::new(0, 0, size.width as u32, size.height as u32);
        let buffer = (size.width as u16, size.height as u16);
        for node in nodes {
            let model = models
                .get(usize::from(node.model / 2))
                .and_then(Option::as_ref);
            let animated = model.is_some_and(|m| m.facing(0).is_some_and(|(_, a)| !a.is_empty()));
            let (Some(model), Some((anchor, scale))) =
                (model, place(view, camera, node, animated, buffer))
            else {
                continue;
            };
            let target = ModelTarget {
                clip,
                anchor,
                scale: ModelScale::new(scale),
            };
            let facing = model_facing(node.heading.wrapping_sub(node.bearing));
            self.draw_model(model, facing, &node.frames, &target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;

    fn f(mantissa: u16, exponent: i8) -> GameFloat {
        GameFloat {
            mantissa,
            exponent,
            negative: false,
        }
    }

    #[test]
    fn floats_convert_multiply_divide_and_truncate_as_the_game_does() {
        assert_eq!(GameFloat::from_raw(0xffff), f(0xffff, 0));
        assert_eq!(GameFloat::from_raw(0x12_3456), f(0x1234, 8));
        assert_eq!(
            GameFloat::from_raw(-0x1234_5678),
            GameFloat {
                negative: true,
                ..f(0x1234, 16)
            }
        );
        // Products keep their top 16 bits, by whole bytes.
        assert_eq!(f(0x100, 0).times(f(0x100, -8)), f(0x100, 0));
        assert_eq!(f(0xff, 0).times(f(0xff, 0)), f(0xfe01, 0));
        assert_eq!(f(0x1000, 0).times(f(0x1000, 0)), f(0x100, 16));
        // Quotients shift the dividend by 0, 8, 16 or 24.
        assert_eq!(f(0x6400, 0).over(f(0x10, 0)), f(0x640, 0));
        assert_eq!(f(0x300, 0).over(f(0x200, 0)), f(0x180, -8));
        assert_eq!(f(0x300, 0).over(f(0x300, 0)), f(0x100, -8));
        assert_eq!(f(0x100, 0).over(f(0x1000, 0)), f(0x1000, -16));
        assert_eq!(f(3, 0).over(f(0x1000, 0)), f(0x3000, -24));
        assert_eq!(f(9, 2).over(f(1, 1)), f(9, 1));
        assert_eq!(f(9, 2).over(f(0, 0)), f(0x6fff, 0x38));
        // To an integer: whole bytes either way, 0x6fff when it does not fit.
        assert_eq!(f(0x1234, 0).to_int(), 0x1234);
        assert_eq!(f(0x8000, 0).to_int(), 0x6fff);
        assert_eq!(f(0x7f, 8).to_int(), 0x7f00);
        assert_eq!(f(0x80, 8).to_int(), 0x6fff);
        assert_eq!(f(1, 16).to_int(), 0x6fff);
        assert_eq!(f(0x1234, -8).to_int(), 0x12);
        assert_eq!(f(0xffff, -16).to_int(), 0);
        assert_eq!(
            GameFloat {
                negative: true,
                ..f(0x1234, 0)
            }
            .to_int(),
            -0x1234
        );
        // The main view's focal factor is 230.
        let focal = focal(Float {
            mantissa: 230,
            exponent: 0,
        });
        assert_eq!(focal, f(0xe600, -8));
    }

    #[test]
    fn octants_count_anticlockwise_from_plus_x() {
        let o = |x: i32, y: i32| octant(x << 16, y << 16);
        assert_eq!([o(5, 1), o(1, 5), o(-1, 5), o(-5, 1)], [0, 1, 2, 3]);
        assert_eq!([o(-5, -1), o(-1, -5), o(1, -5), o(5, -1)], [4, 5, 6, 7]);
        // On a diagonal the higher octant, on an axis but +x the lower; nothing at
        // all is octant 0.
        assert_eq!([o(3, 3), o(-3, -3), o(3, 0), o(0, 3)], [1, 5, 0, 1]);
        assert_eq!([o(-3, 0), o(0, -3), o(0, 0)], [3, 5, 0]);
    }

    /// Exactly sin 0 and cos 0 (heading 0, looking along +y), or the reverse.
    fn camera(heading: u16) -> Camera {
        let (zero, one) = (
            Trig::default(),
            Trig {
                one: true,
                ..Trig::default()
            },
        );
        let (sin, cos) = match heading {
            0 => (zero, one),
            0x4000 => (one, zero),
            _ => unreachable!(),
        };
        Camera {
            x: 0,
            y: 0,
            z: 0,
            heading,
            sin,
            cos,
            focal: focal(Float {
                mantissa: 230,
                exponent: 0,
            }),
            viewer: 99,
        }
    }

    #[test]
    fn rotation_gives_lateral_and_depth() {
        let c = camera(0);
        assert_eq!(c.rotate(3 << 16, 5 << 16), (3 << 16, 5 << 16));
        let c = camera(0x4000);
        assert_eq!(c.rotate(3 << 16, 5 << 16), (-5 << 16, 3 << 16));
        // A magnitude below 1 drops the low bits of large offsets.
        let half = Trig {
            magnitude: 0x8000,
            ..Trig::default()
        };
        let c = Camera {
            sin: half,
            cos: half,
            ..camera(0)
        };
        assert_eq!(c.rotate(0x1_00ff, 0), (0x8000, 0x8000));
        assert_eq!(c.rotate(0xff, 0), (0x7f, 0x7f));
    }

    fn view() -> View3d {
        View3d {
            kind_models: [0; 17],
            frame_offsets: [0; 17],
            node_types: [None; 17],
            focal: [Float {
                mantissa: 230,
                exponent: 0,
            }; 3],
            model_sizes: [0x40, 0x100, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            default_size: Float {
                mantissa: 0x8000,
                exponent: -8,
            },
            max_distance: 0x7d,
        }
    }

    /// Model 0 at (x, y) units in front of the camera, at distance `d` units.
    fn node(x: i32, y: i32, d: u32) -> RenderNode {
        RenderNode {
            object: 1,
            model: 0,
            heading: 0,
            position: [x << 16, y << 16, 0],
            radius: 0,
            distance: d << 16,
            bearing: 0,
            frames: Vec::new(),
            size: None,
        }
    }

    #[test]
    fn nodes_project_by_focal_factor_over_distance() {
        let (v, c) = (view(), camera(0));
        // Straight ahead 10 units: the centre; scale 256 * 0x40 * 230 / (65536 * 10).
        assert_eq!(
            place(&v, &c, &node(0, 10, 10), true, (100, 60)),
            Some(((50, 30), 5))
        );
        // 1 unit to the right and 2 up, 230 / 10 = 23 pixels a unit.
        let mut n = node(1, 10, 10);
        n.position[2] = 2 << 16;
        assert_eq!(place(&v, &c, &n, true, (100, 60)), Some(((73, -16), 5)));
        // Model 2's size factor is 0x100: the stand-in 128.
        n.model = 2;
        assert_eq!(place(&v, &c, &n, true, (100, 60)).unwrap().1, 11);
        // A type 6 node's own size.
        n.size = Some(0x80);
        assert_eq!(place(&v, &c, &n, true, (100, 60)).unwrap().1, 11);
    }

    #[test]
    fn culling_skips_the_viewer_behind_far_and_unanimated() {
        let (v, c) = (view(), camera(0));
        let at = |n: &RenderNode| place(&v, &c, n, true, (100, 60)).is_some();
        assert!(at(&node(0, 10, 10)));
        let mut viewer = node(0, 10, 10);
        viewer.object = 99;
        assert!(!at(&viewer));
        // The camera looks along octant 2: 0 to 4 pass the cone, 5 to 7 do not.
        assert!(at(&node(10, 1, 10)));
        assert!(at(&node(-10, 1, 10)));
        assert!(!at(&node(-1, -10, 10)));
        assert!(!at(&node(10, -1, 10)));
        // Looking along +x (octant 0), straight left (+y, octant 1) and right (-y,
        // octant 5) are at depth 0: the cone alone drops the right.
        let side = camera(0x4000);
        assert!(place(&v, &side, &node(0, 10, 10), true, (100, 60)).is_some());
        assert!(place(&v, &side, &node(0, -10, 10), true, (100, 60)).is_none());
        // Octant 4 passes the cone, but is behind (depth < 0).
        assert!(!at(&node(-10, -1, 10)));
        // Under 125 units (a big size, so the scale is not 0).
        let far = |d| RenderNode {
            size: Some(0x400),
            ..node(0, d, d as u32)
        };
        assert!(at(&far(124)));
        assert!(!at(&far(125)));
        assert_eq!(place(&v, &c, &node(0, 10, 10), false, (100, 60)), None);
        // So small it scales to 0.
        let mut tiny = node(0, 100, 100);
        tiny.size = Some(1);
        assert!(!at(&tiny));
    }

    #[test]
    fn the_list_runs_far_to_near_ties_in_order() {
        let mut nodes: Vec<RenderNode> = [3, 9, 5, 5, 6]
            .into_iter()
            .enumerate()
            .map(|(i, d)| RenderNode {
                object: i as u16,
                ..node(0, d as i32, d)
            })
            .collect();
        // Only a radius's high byte counts: 6 units less 0xff00 is 5 and 1/256.
        nodes[4].radius = 0xffff;
        assert_eq!(nodes[4].near(), 0x5_0100);
        sort_far_to_near(&mut nodes);
        let order: Vec<u16> = nodes.iter().map(|n| n.object).collect();
        assert_eq!(order, [1, 4, 2, 3, 0]);
        // Many at one distance (past the sort's small-slice cut-off) keep their order.
        let mut ties: Vec<RenderNode> = (0..64u16)
            .map(|i| RenderNode {
                object: i,
                ..node(0, 2 + i32::from(i % 2), 2 + u32::from(i % 2))
            })
            .collect();
        sort_far_to_near(&mut ties);
        let order: Vec<u16> = ties.iter().map(|n| n.object).collect();
        let (odd, even): (Vec<u16>, Vec<u16>) = (0..64).partition(|i| i % 2 == 1);
        assert_eq!(order, [odd, even].concat());
        let mut near = node(0, 1, 0);
        near.radius = 0xff00;
        assert_eq!(near.near(), 0);
    }

    fn sky() -> Sky {
        Sky {
            pixels: (0..Sky::WIDTH * Sky::HEIGHT)
                .map(|i| (i % Sky::WIDTH % 200 + i / Sky::WIDTH % 2) as u8)
                .collect(),
        }
    }

    #[test]
    fn the_sky_wraps_over_the_upper_half_and_the_ground_fills_the_rest() {
        let mut fb = Framebuffer::new(Size::new(8, 5));
        fb.fill_ground();
        fb.draw_sky(Some(&sky()), 64 * 3);
        // ceil(5 / 2) = 3 rows from strip row 51, from column 3 - 4 = -1 (341).
        let p = fb.pixels();
        assert_eq!(&p[..8], &[142, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(p[8], 141);
        assert_eq!(&p[24..], &[GROUND; 16]);
        let mut fb = Framebuffer::new(Size::new(4, 5));
        fb.draw_sky(None, 0);
        assert_eq!(
            fb.pixels(),
            &[&[FLAT_SKY; 8][..], &[GROUND; 12]].concat()[..]
        );
    }

    /// A model with one facing (0) whose one frame is a 2x2 image of colour 9 with its
    /// top-left at the anchor.
    fn square() -> LoadedModel {
        use xeno_formats::mxx::{Frame, Image, Model, Part, Span, View, ViewHeader};
        let image = Image {
            width: 2,
            height: 2,
            rows: vec![
                Span {
                    skip: 0,
                    pixels: vec![9, 9]
                };
                2
            ],
        };
        let view = View {
            header: ViewHeader {
                size: 0,
                facing: 0,
                mirror_facing: 0,
                index_offset: 0,
                mirror_index_offset: 0,
                lists_offset: 0,
                mirror_lists_offset: 0,
                data_offset: 0,
            },
            animations: vec![vec![Frame {
                depth: 0,
                parts: vec![Part {
                    image: 0,
                    x: 0,
                    y: 0,
                }],
            }]],
            mirror_animations: None,
            images: vec![image],
        };
        let m08 = Model { views: vec![view] };
        LoadedModel::single_view(&Model { views: Vec::new() }, &m08)
    }

    #[test]
    fn the_view_draws_ground_sky_then_models_far_to_near() {
        let mut v = view();
        // 0x40 * 230 * 256 / (65536 * d): scale 256 at d = 0x40 * 230 / 65536 units.
        v.model_sizes[0] = 0x2000;
        let c = camera(0);
        let mut fb = Framebuffer::new(Size::new(12, 8));
        // 0x2000 * 230 / 65536 = 28.75 units for 1:1; at 14.375 units twice the size.
        let mut near = node(0, 14, 14);
        near.distance = 0xe_6000;
        near.position[1] = 0xe_6000;
        let models = [Some(square())];
        fb.draw_view(&v, &c, &[near.clone()], &models, None);
        let row = |fb: &Framebuffer, y: usize| fb.pixels()[y * 12..][..12].to_vec();
        assert_eq!(row(&fb, 0), [FLAT_SKY; 12]);
        // Anchored at the centre (6, 4), each unit 2 by 2.
        let drawn = [&[GROUND; 6][..], &[9; 4], &[GROUND; 2]].concat();
        assert_eq!(row(&fb, 3), [FLAT_SKY; 12]);
        assert_eq!(row(&fb, 4), drawn);
        assert_eq!(row(&fb, 7), drawn);
        // Behind the camera, nothing.
        let mut behind = near;
        behind.position[1] = -behind.position[1];
        fb.draw_view(&v, &c, &[behind], &models, None);
        assert_eq!(row(&fb, 4), [GROUND; 12]);
    }
}
