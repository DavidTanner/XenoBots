//! The scaled model-image blit of `mgavideo.dvr` (slot `0x58c`, with the target and
//! clip rectangle from slot `0x574`, the anchor from `0x578` and the scale from
//! `0x584`; see `docs/formats/mxx.md`, "Drawing").

use xeno_formats::mxx::{Animation, Frame, Image, Model, Part, View};

use crate::{Framebuffer, Rect};

/// An 8.8 fixed-point scale (256 = 1:1) as a table of 256 repeat counts, one for each
/// unit column (and row) -128..=127 from the anchor (slot `0x584`).
///
/// Every count is `ceil(scale / 256)`, and when `scale mod 256` is not 0, the first
/// `256 - scale mod 256` entries of a fixed order are one less: the `k`-th entry of
/// that order is unit `255 - bitrev8(k) - 128`. So the counts add up to `scale`, and
/// the dropped (or extra) rows and columns are spread evenly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelScale {
    scale: u16,
    /// Screen pixels each unit covers, by `unit + 128`.
    counts: [u16; 256],
    /// Where each unit starts relative to the anchor, by `unit + 128`; the last entry
    /// is where unit 127 ends.
    starts: [i32; 257],
}

impl ModelScale {
    /// 1:1.
    pub const ONE: u16 = 256;

    /// The table for `scale`. The original keeps each count in a byte, so its counts
    /// wrap to 0 above a scale of 0xff00; here they do not.
    pub fn new(scale: u16) -> ModelScale {
        let mut counts = [u32::from(scale).div_ceil(256) as u16; 256];
        let fewer = (256 - usize::from(scale % 256)) % 256;
        for k in 0..fewer {
            counts[255 - usize::from((k as u8).reverse_bits())] -= 1;
        }
        let mut starts = [0; 257];
        for (i, &count) in counts.iter().enumerate() {
            starts[i + 1] = starts[i] + i32::from(count);
        }
        let zero = starts[128];
        starts.iter_mut().for_each(|s| *s -= zero);
        ModelScale {
            scale,
            counts,
            starts,
        }
    }

    pub fn scale(&self) -> u16 {
        self.scale
    }

    /// The screen pixels unit `unit` covers, from [`ModelScale::start`]. Units outside
    /// -128..=127 are past the original's table, and no stock part reaches them; here
    /// they cover nothing.
    pub fn count(&self, unit: i32) -> u32 {
        Self::index(unit).map_or(0, |i| self.counts[i].into())
    }

    /// Where unit `unit` starts, relative to the anchor: the sum of the counts of the
    /// units from 0 up to it, or minus those from it up to 0 for a negative unit.
    pub fn start(&self, unit: i32) -> i32 {
        Self::index(unit).map_or(0, |i| self.starts[i])
    }

    fn index(unit: i32) -> Option<usize> {
        usize::try_from(unit + 128).ok().filter(|&i| i < 256)
    }
}

/// Where model images go: the clip rectangle (slot `0x574`; the original's is from its
/// left and top edges up to but not including its right and bottom ones), the anchor
/// part offsets are measured from (slot `0x578`, in surface pixels) and the scale
/// (slot `0x584`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTarget {
    pub clip: Rect,
    pub anchor: (i32, i32),
    pub scale: ModelScale,
}

impl Framebuffer {
    /// Draws `image` with its top-left at unit offset (`x`, `y`) from the target's
    /// anchor, scaled, as slot `0x58c` does.
    ///
    /// Unit column `u` covers [`ModelScale::count`] screen columns from the anchor plus
    /// [`ModelScale::start`], and rows work the same way, so a count of 0 drops a column or
    /// row and a larger one repeats it. Only pixels inside the row spans are drawn,
    /// colour 0 is skipped, and each row stops at the image's width. `mirrored` puts
    /// source column `c` at unit column `x + width - 1 - c`. The image is clipped to
    /// the target's rectangle (and the surface), not dropped.
    pub fn blit_model_image(
        &mut self,
        image: &Image,
        x: i32,
        y: i32,
        mirrored: bool,
        target: &ModelTarget,
    ) {
        let surface = self.surface();
        let clip = |from: i32, length: u32, size: usize| {
            let to = i64::from(from) + i64::from(length);
            let from = i64::from(from).clamp(0, size as i64);
            (from, to.clamp(from, size as i64))
        };
        let (left, right) = clip(target.clip.x, target.clip.width, surface.width);
        let (top, bottom) = clip(target.clip.y, target.clip.height, surface.height);
        let scale = &target.scale;
        // Screen pixels [from, to) covered by unit `unit` on an axis anchored at
        // `anchor`, limited to [low, high).
        let cover = |anchor: i32, unit: i32, low: i64, high: i64| {
            let from = i64::from(anchor) + i64::from(scale.start(unit));
            let to = from + i64::from(scale.count(unit));
            let from = from.clamp(low, high);
            (from as usize, to.clamp(from, high) as usize)
        };
        let width = usize::from(image.width);
        let stride = surface.width;
        let pixels = self.pixels_mut();
        for (row, span) in image.rows.iter().enumerate() {
            let (y0, y1) = cover(target.anchor.1, y + row as i32, top, bottom);
            if y0 >= y1 {
                continue;
            }
            let columns = (usize::from(span.skip)..width).zip(&span.pixels);
            for (column, &colour) in columns.filter(|&(_, &colour)| colour != 0) {
                let column = if mirrored { width - 1 - column } else { column };
                let (x0, x1) = cover(target.anchor.0, x + column as i32, left, right);
                for dy in y0..y1 {
                    pixels[dy * stride + x0..dy * stride + x1].fill(colour);
                }
            }
        }
    }

    /// Draws one frame of each of a model's animations as the game composes them: the
    /// frames in increasing order of `depth` (equal keys in the order given), each
    /// frame's parts in list order, so later parts and higher keys end up on top.
    /// `image` finds a part's image (in its view, or the model's `.m00` for
    /// [`Part::is_direction_free`]); parts it can't find are skipped.
    pub fn draw_model_frames<'a>(
        &mut self,
        frames: &[&Frame],
        image: impl Fn(&Part) -> Option<&'a Image>,
        target: &ModelTarget,
    ) {
        let mut sorted = frames.to_vec();
        sorted.sort_by_key(|frame| frame.depth);
        for part in sorted.iter().flat_map(|frame| &frame.parts) {
            if let Some(found) = image(part) {
                let (x, y) = (part.x.into(), part.y.into());
                self.blit_model_image(found, x, y, part.is_flipped(), target);
            }
        }
    }

    /// Draws `model` at `facing` (an even 64th of a turn, [`model_facing`]) with frame
    /// `frames[k]` of each animation `k` (0 when `frames` is short), as the 3D view's
    /// model routines do. Frame numbers past an animation's end draw nothing of it
    /// (the original does not check them).
    pub fn draw_model(
        &mut self,
        model: &LoadedModel,
        facing: u8,
        frames: &[u8],
        target: &ModelTarget,
    ) {
        let Some((view, animations)) = model.facing(facing) else {
            return;
        };
        let frames: Vec<&Frame> = animations
            .iter()
            .enumerate()
            .filter_map(|(k, a)| a.get(usize::from(frames.get(k).copied().unwrap_or(0))))
            .collect();
        let image = |part: &Part| {
            let from = if part.is_direction_free() {
                &model.direction_free
            } else {
                &view.images
            };
            from.get(part.image_index())
        };
        self.draw_model_frames(&frames, image, target);
    }
}

/// The facing a model is drawn at when its heading less the bearing to it from the
/// camera is `relative` (65,536 to a turn): rounded to the nearest even 64th (a tie
/// rounds up) and turned half way round, so facing 0 looks at the viewer
/// (`docs/formats/mxx.md`, "Choosing the facing").
pub fn model_facing(relative: u16) -> u8 {
    ((((relative >> 10) + 1) & 0x3e) ^ 0x20) as u8
}

/// A directional model as `gfx_load_model` keeps it (`docs/formats/mxx.md`,
/// "Loading"): its views, the `.m00`'s images, and for each even facing the view
/// drawn and whether it is that view's mirror facing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedModel {
    views: Vec<View>,
    direction_free: Vec<Image>,
    facings: [Option<(usize, bool)>; 32],
}

impl LoadedModel {
    /// From the `.m00` and the directional files read in order (`.m08`, then `.m16`,
    /// then `.m32`). Facings with no view of their own show another's, as after the
    /// last pass: after `.m08` facings `8j + 2` and `8j + 4` show `8j` and `8j + 6`
    /// shows `8j + 8`; after `.m16` facings `4j + 2` show `4j`.
    pub fn new(m00: &Model, passes: &[Model]) -> LoadedModel {
        let direction_free = m00
            .views
            .iter()
            .find(|v| !v.header.has_facing())
            .map(|v| v.images.clone())
            .unwrap_or_default();
        let views: Vec<View> = passes.iter().flat_map(|m| m.views.clone()).collect();
        let mut facings = [None; 32];
        let mut set = |facing: i16, entry| {
            if let Some(slot) = usize::try_from(facing / 2)
                .ok()
                .and_then(|f| facings.get_mut(f))
            {
                *slot = Some(entry);
            }
        };
        for (v, view) in views.iter().enumerate() {
            let h = &view.header;
            if h.has_facing() {
                set(h.mirror_facing, (v, true));
                set(h.facing, (v, false));
            }
        }
        // In units of 2: facing `2f` shows facing `2 * shown(f)`.
        let shown = |f: usize| match (passes.len(), f % 4) {
            (1, 1) => f - 1,
            (1, 2) => f - 2,
            (1, 3) => (f + 1) % 32,
            (2, _) => f & !1,
            _ => f,
        };
        let own = facings;
        for (f, entry) in facings.iter_mut().enumerate() {
            if entry.is_none() {
                *entry = own[shown(f)];
            }
        }
        LoadedModel {
            views,
            direction_free,
            facings,
        }
    }

    /// `gfx_load_model_single_view` (`explode`, `plasma`, `basedome`, `cloak`, `obst`):
    /// facing 0, unflipped, for every direction.
    pub fn single_view(m00: &Model, m08: &Model) -> LoadedModel {
        let mut model = LoadedModel::new(m00, std::slice::from_ref(m08));
        model.repeat(1);
        model
    }

    /// `gfx_load_model_quarter_turn` (`netrelay`): facing `f` shows facing `f mod 16`,
    /// unflipped, so the model looks the same every quarter turn.
    pub fn quarter_turn(m00: &Model, passes: &[Model]) -> LoadedModel {
        let mut model = LoadedModel::new(m00, passes);
        model.repeat(8);
        model
    }

    /// Every facing shows the first `n` (in units of 2), with their plain frames.
    fn repeat(&mut self, n: usize) {
        for f in 0..32 {
            self.facings[f] = self.facings[f % n].map(|(v, _)| (v, false));
        }
    }

    /// The view drawn at `facing` (an even 64th of a turn) and its animations there.
    pub fn facing(&self, facing: u8) -> Option<(&View, &[Animation])> {
        let (v, mirror) = (*self.facings.get(usize::from(facing / 2) % 32)?)?;
        let view = &self.views[v];
        let animations = match (&view.mirror_animations, mirror) {
            (Some(mirrored), true) => mirrored,
            _ => &view.animations,
        };
        Some((view, animations))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;
    use proptest::prelude::*;
    use xeno_formats::mxx::Span;

    /// An image with one span per row: `(skip, pixels)`.
    fn image(width: u8, rows: &[(u8, &[u8])]) -> Image {
        Image {
            width,
            height: rows.len() as u8,
            rows: rows
                .iter()
                .map(|&(skip, pixels)| Span {
                    skip,
                    pixels: pixels.to_vec(),
                })
                .collect(),
        }
    }

    fn target(scale: u16, anchor: (i32, i32), clip: Rect) -> ModelTarget {
        ModelTarget {
            clip,
            anchor,
            scale: ModelScale::new(scale),
        }
    }

    /// The surface as text: `.` for colour 0, a hex digit for 1 to 15, `#` above.
    fn ascii(fb: &Framebuffer) -> String {
        fb.pixels()
            .chunks(fb.surface().width)
            .map(|row| {
                row.iter()
                    .map(|&p| match p {
                        0 => '.',
                        1..=15 => char::from_digit(p.into(), 16).unwrap(),
                        _ => '#',
                    })
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    fn counts(scale: u16, units: std::ops::Range<i32>) -> Vec<u32> {
        let table = ModelScale::new(scale);
        units.map(|u| table.count(u)).collect()
    }

    #[test]
    fn repeat_counts_spread_the_dropped_and_repeated_units() {
        assert!(counts(256, -128..128).iter().all(|&c| c == 1));
        assert!(counts(512, -128..128).iter().all(|&c| c == 2));
        assert!(counts(0, -128..128).iter().all(|&c| c == 0));
        // Half size drops every odd unit; 1.5 repeats every even one.
        assert_eq!(counts(128, -4..4), [1, 0, 1, 0, 1, 0, 1, 0]);
        assert_eq!(counts(384, -4..4), [2, 1, 2, 1, 2, 1, 2, 1]);
        // The order's first entry is unit 127, then -1, 63, -65 (bit-reversed).
        assert_eq!(counts(255, 126..128), [1, 0]);
        assert_eq!(counts(254, -2..1), [1, 0, 1]);
        assert_eq!(counts(252, 62..64), [1, 0]);
        assert_eq!(counts(252, -66..-63), [1, 0, 1]);
        assert_eq!(counts(1, -128..-126), [1, 0]);
        // Outside the table: nothing.
        assert_eq!(counts(256, -130..-128), [0, 0]);
        assert_eq!(counts(256, 128..130), [0, 0]);
    }

    #[test]
    fn units_start_from_the_anchor_by_summed_counts() {
        let table = ModelScale::new(384);
        // Units 0, 1, 2 are 2, 1, 2 wide; -1 and -2 are 1 and 2.
        let starts: Vec<i32> = (-2..4).map(|u| table.start(u)).collect();
        assert_eq!(starts, [-3, -1, 0, 2, 3, 5]);
        assert_eq!(table.scale(), 384);
    }

    proptest! {
        #[test]
        fn counts_add_up_to_the_scale(scale in 0u16..=0xff00) {
            let table = ModelScale::new(scale);
            let total: u32 = (-128..128).map(|u| table.count(u)).sum();
            prop_assert_eq!(total, u32::from(scale));
            for u in -128..127 {
                prop_assert_eq!(table.start(u + 1), table.start(u) + table.count(u) as i32);
            }
        }
    }

    /// A 3x2 image with a colour-0 pixel and a short second row.
    fn sample() -> Image {
        image(3, &[(0, &[1, 0, 2]), (1, &[3, 4])])
    }

    fn drawn(image: &Image, x: i32, y: i32, mirrored: bool, target: &ModelTarget) -> String {
        let mut fb = Framebuffer::new(Size::new(8, 5));
        fb.clear(0xe);
        fb.blit_model_image(image, x, y, mirrored, target);
        ascii(&fb)
    }

    const WHOLE: Rect = Rect::new(0, 0, 8, 5);

    #[test]
    fn one_to_one_draws_spans_and_skips_colour_0() {
        let t = target(256, (2, 1), WHOLE);
        assert_eq!(
            drawn(&sample(), 0, 0, false, &t),
            "eeeeeeee\nee1e2eee\neee34eee\neeeeeeee\neeeeeeee\n"
        );
        // Negative offsets are measured from the anchor too.
        assert_eq!(
            drawn(&sample(), -2, -1, false, &t),
            "1e2eeeee\ne34eeeee\neeeeeeee\neeeeeeee\neeeeeeee\n"
        );
    }

    #[test]
    fn mirrored_images_keep_their_columns() {
        let t = target(256, (2, 1), WHOLE);
        assert_eq!(
            drawn(&sample(), 0, 0, true, &t),
            "eeeeeeee\nee2e1eee\nee43eeee\neeeeeeee\neeeeeeee\n"
        );
    }

    #[test]
    fn below_256_drops_odd_units() {
        // At 128 units 1 and -1 cover nothing: column 1 and row 1 vanish.
        let t = target(128, (2, 1), WHOLE);
        let square = image(3, &[(0, &[1, 2, 3]), (0, &[4, 5, 6]), (0, &[7, 8, 9])]);
        assert_eq!(
            drawn(&square, 0, 0, false, &t),
            "eeeeeeee\nee13eeee\nee79eeee\neeeeeeee\neeeeeeee\n"
        );
    }

    #[test]
    fn above_256_repeats_units() {
        let t = target(512, (1, 0), WHOLE);
        assert_eq!(
            drawn(&sample(), 0, 0, false, &t),
            "e11ee22e\ne11ee22e\neee3344e\neee3344e\neeeeeeee\n"
        );
        // 384: units 0 and 2 twice, unit 1 once.
        let t = target(384, (1, 0), WHOLE);
        assert_eq!(
            drawn(&sample(), 0, 0, false, &t),
            "e11e22ee\ne11e22ee\neee344ee\neeeeeeee\neeeeeeee\n"
        );
    }

    #[test]
    fn rows_stop_at_the_image_width() {
        // The parser never makes such a span; the game cuts it at the width.
        let wide = image(2, &[(1, &[5, 6, 7])]);
        assert_eq!(
            drawn(&wide, 0, 0, false, &target(256, (0, 0), WHOLE)),
            "e5eeeeee\neeeeeeee\neeeeeeee\neeeeeeee\neeeeeeee\n"
        );
        // A row with no pixels may have any skip.
        let empty = image(2, &[(0xff, &[])]);
        let t = target(256, (0, 0), WHOLE);
        assert_eq!(drawn(&empty, 0, 0, false, &t), "eeeeeeee\n".repeat(5));
    }

    #[test]
    fn images_are_clipped_to_the_rectangle_not_dropped() {
        let t = target(512, (1, 0), Rect::new(2, 1, 4, 2));
        assert_eq!(
            drawn(&sample(), 0, 0, false, &t),
            "eeeeeeee\nee1ee2ee\neee334ee\neeeeeeee\neeeeeeee\n"
        );
        // A rectangle reaching off the surface clips to the surface.
        let t = target(512, (5, 3), Rect::new(-9, -9, 99, 99));
        assert_eq!(
            drawn(&sample(), 0, 0, false, &t),
            "eeeeeeee\neeeeeeee\neeeeeeee\neeeee11e\neeeee11e\n"
        );
        let t = target(256, (0, 0), Rect::new(0, 0, 0, 5));
        assert_eq!(drawn(&sample(), 0, 0, false, &t), "eeeeeeee\n".repeat(5));
    }

    #[test]
    fn facings_round_to_the_nearest_even_64th_turned_half_round() {
        assert_eq!(model_facing(0), 32);
        assert_eq!(model_facing(0x8000), 0);
        // 1/64 of a turn is 0x400: just under a 64th rounds down, a 64th up.
        assert_eq!(model_facing(0x3ff), 32);
        assert_eq!(model_facing(0x400), 34);
        assert_eq!(model_facing(0x4000), 48);
        assert_eq!(model_facing(0xfc00), 32);
        assert_eq!(model_facing(0xf800), 30);
    }

    /// A view at `facing` and its mirror, whose one animation's one frame draws image 0
    /// at x 0 (the mirror's at x 1), and whose image is one pixel of colour `facing + 1`.
    fn view(facing: i16) -> View {
        let mirror = (64 - facing) % 64;
        let frame = |x| {
            vec![vec![Frame {
                depth: 0,
                parts: vec![Part { image: 0, x, y: 0 }],
            }]]
        };
        View {
            header: xeno_formats::mxx::ViewHeader {
                size: 0,
                facing,
                mirror_facing: mirror,
                index_offset: 0,
                mirror_index_offset: 0,
                lists_offset: 0,
                mirror_lists_offset: 0,
                data_offset: 0,
            },
            animations: frame(0),
            mirror_animations: (mirror != facing).then(|| frame(1)),
            images: vec![image(1, &[(0, &[facing as u8 + 1])])],
        }
    }

    fn model(facings: &[i16]) -> Model {
        Model {
            views: facings.iter().map(|&f| view(f)).collect(),
        }
    }

    /// The facing whose view `facing` draws, and the column (1 for a mirror facing).
    fn shown(loaded: &LoadedModel, facing: u8) -> (u8, usize) {
        let mut fb = Framebuffer::new(Size::new(2, 1));
        let t = target(256, (0, 0), Rect::new(0, 0, 2, 1));
        fb.draw_model(loaded, facing, &[], &t);
        let column = fb.pixels().iter().position(|&p| p != 0).unwrap();
        (fb.pixels()[column] - 1, column)
    }

    #[test]
    fn facings_without_a_view_show_another_as_after_the_last_pass() {
        let m00 = Model { views: Vec::new() };
        let m08 = model(&[0, 8, 16, 24, 32]);
        let one = LoadedModel::new(&m00, std::slice::from_ref(&m08));
        // 8j + 2 and 8j + 4 show 8j, 8j + 6 shows 8j + 8; mirrors draw at column 1.
        assert_eq!(shown(&one, 2), (0, 0));
        assert_eq!(shown(&one, 4), (0, 0));
        assert_eq!(shown(&one, 6), (8, 0));
        assert_eq!(shown(&one, 14), (16, 0));
        assert_eq!(shown(&one, 32), (32, 0));
        assert_eq!(shown(&one, 40), (24, 1));
        assert_eq!(shown(&one, 62), (0, 0));
        // After `.m16`, 4j + 2 shows 4j.
        let two = LoadedModel::new(&m00, &[m08, model(&[4, 12, 20, 28])]);
        assert_eq!(shown(&two, 6), (4, 0));
        assert_eq!(shown(&two, 10), (8, 0));
        assert_eq!(shown(&two, 62), (4, 1));
        assert_eq!(shown(&two, 58), (8, 1));
    }

    #[test]
    fn single_view_and_quarter_turn_models_repeat_unflipped() {
        let m00 = Model { views: Vec::new() };
        let one = LoadedModel::single_view(&m00, &model(&[0, 8, 16, 24, 32]));
        assert_eq!(shown(&one, 0), (0, 0));
        assert_eq!(shown(&one, 24), (0, 0));
        assert_eq!(shown(&one, 40), (0, 0));
        let relay =
            LoadedModel::quarter_turn(&m00, &[model(&[0, 8, 16, 24, 32]), model(&[4, 12, 20, 28])]);
        assert_eq!(shown(&relay, 4), (4, 0));
        assert_eq!(shown(&relay, 20), (4, 0));
        assert_eq!(shown(&relay, 46), (12, 0));
        assert_eq!(shown(&relay, 60), (12, 0));
    }

    #[test]
    fn frames_draw_in_depth_order_then_part_order() {
        let images = [image(2, &[(0, &[1, 1])]), image(1, &[(0, &[2])])];
        let part = |image: u16, x: i8| Part { image, x, y: 0 };
        let high = Frame {
            depth: 10,
            parts: vec![part(0, 0)],
        };
        let low = Frame {
            depth: 5,
            parts: vec![part(0, 2), part(1, 2), part(0x8001, 0)],
        };
        let mut fb = Framebuffer::new(Size::new(4, 1));
        let t = target(256, (0, 0), Rect::new(0, 0, 4, 1));
        fb.draw_model_frames(&[&high, &low], |p| images.get(p.image_index()), &t);
        // `low` first: 2 at column 0 (mirrored), 1 1 at 2, then 2 over column 2;
        // `high` last puts 1 1 over columns 0 and 1.
        assert_eq!(ascii(&fb), "1121\n");
    }

    #[test]
    fn scaled_render_snapshot() {
        let robot = image(
            5,
            &[
                (1, &[6, 6, 6]),
                (0, &[6, 1, 6, 1, 6]),
                (1, &[7, 7, 7]),
                (1, &[7, 0, 7]),
            ],
        );
        let mut fb = Framebuffer::new(Size::new(46, 12));
        fb.fill_rect(Rect::new(0, 8, 46, 4), 2);
        for (scale, anchor, mirrored) in [
            (128, (3, 6), false),
            (256, (10, 6), false),
            (256, (18, 6), true),
            (384, (27, 4), false),
            (640, (40, 2), false),
        ] {
            let t = target(scale, anchor, Rect::new(0, 0, 46, 11));
            fb.blit_model_image(&robot, -2, -2, mirrored, &t);
        }
        insta::assert_snapshot!(ascii(&fb));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_model_frame_draws_within_the_table() {
        use xeno_formats::mxx;
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let read = |name: &str| {
            let entry = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.file_name().unwrap().to_string_lossy().to_lowercase() == name);
            mxx::parse(&std::fs::read(entry.unwrap()).unwrap()).unwrap()
        };
        let targets: Vec<ModelTarget> = [64, 256, 300, 1024]
            .into_iter()
            .map(|s| target(s, (160, 100), Rect::new(0, 0, 320, 200)))
            .collect();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        let mut frames = 0;
        for model in [
            "basedome", "cloak", "explode", "humanoid", "laser", "missile", "netrelay", "obst",
            "plasma", "scorpion", "scout",
        ] {
            let m00 = read(&format!("{model}.m00"));
            for suffix in ["m08", "m16", "m32"] {
                if suffix == "m32" && ["humanoid", "missile", "scorpion", "scout"].contains(&model)
                {
                    continue;
                }
                for view in read(&format!("{model}.{suffix}")).views {
                    let resolve = |p: &Part| {
                        let images = if p.is_direction_free() {
                            &m00.views[0].images
                        } else {
                            &view.images
                        };
                        images.get(p.image_index())
                    };
                    let facings = [&view.animations]
                        .into_iter()
                        .chain(&view.mirror_animations);
                    for frame in facings.flatten().flatten() {
                        for part in &frame.parts {
                            let image = resolve(part).expect("every part resolves");
                            // Every unit a part covers is inside the repeat table.
                            assert!(i32::from(part.x) + i32::from(image.width) <= 128);
                            assert!(i32::from(part.y) + i32::from(image.height) <= 128);
                        }
                        for t in &targets {
                            fb.draw_model_frames(&[frame], resolve, t);
                        }
                        frames += 1;
                    }
                }
            }
        }
        assert!(frames > 1000, "{frames} frames");
    }
}
