//! `.m00` / `.m08` / `.m16` / `.m32` unit models (`mgavideo.dvr` calls them `.MXX`).
//! A model is a chain of views. Each view draws a unit from one facing and from its
//! mirror image, measured in 64ths of a turn. The suffix says which facings a file
//! holds: `.m08` the 8-way set, `.m16` the extra four for 16 ways, `.m32` the extra
//! eight for 32 ways, and `.m00` a single view with no facing.
//!
//! Each facing has animations, each a list of frames, and a frame is a list of parts:
//! images from the view (or from the model's `.m00`) drawn at an offset, perhaps
//! mirrored. The animations are the moving pieces of a unit (body, legs, arms...): the
//! game draws one frame of each at once. The images are 8-bit row spans in which
//! colour 0 is transparent. See `docs/formats/mxx.md`.

use binrw::{BinRead, binrw};

use crate::FormatError;

/// The sixteen bytes at the start of each view. Offsets count from the start of the
/// view, not from the start of the file.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewHeader {
    /// Bytes in this view, header included. 0 ends the model.
    pub size: u16,
    /// In 64ths of a turn, 0 to 32, or [`ViewHeader::NO_FACING`].
    pub facing: i16,
    /// `(64 - facing) % 64`: the facing that is the mirror image of `facing`, or
    /// [`ViewHeader::NO_FACING`].
    pub mirror_facing: i16,
    /// The two-level index for `facing`. 0 when there is no facing.
    pub index_offset: u16,
    /// The two-level index for `mirror_facing`; equal to `index_offset` when the
    /// facing is its own mirror (0 and 32). 0 when there is no facing.
    pub mirror_index_offset: u16,
    /// Where the lists that `index_offset` points to start. Not an offset when there
    /// is no facing: these two words then hold unknown values. The game never reads
    /// this field or the next; it reaches the lists through the index.
    pub lists_offset: u16,
    /// As `lists_offset`, for `mirror_facing`.
    pub mirror_lists_offset: u16,
    /// Start of the data shared by both facings, up to the end of the view.
    pub data_offset: u16,
}

impl ViewHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 16;
    /// Facings per full turn.
    pub const FACINGS_PER_TURN: i16 = 64;
    /// The `facing` (and `mirror_facing`) of a view that is not tied to a direction:
    /// the only view of a `.m00` file.
    pub const NO_FACING: i16 = -2;

    pub fn has_facing(&self) -> bool {
        self.facing != Self::NO_FACING
    }
}

/// One image of a frame, placed relative to the unit's position.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Image number in the low 14 bits, plus [`Part::DIRECTION_FREE`] and
    /// [`Part::FLIPPED`].
    pub image: u16,
    /// Left edge of the image (before flipping) relative to the unit.
    pub x: i8,
    /// Top edge of the image relative to the unit.
    pub y: i8,
}

impl Part {
    /// Image bit: the image is in the model's `.m00` view, not in this view.
    pub const DIRECTION_FREE: u16 = 0x4000;
    /// Image bit: the image is drawn mirrored left to right.
    pub const FLIPPED: u16 = 0x8000;

    /// Index into the images of this view, or of the `.m00` view when
    /// [`Part::is_direction_free`].
    pub fn image_index(&self) -> usize {
        usize::from(self.image & !(Self::DIRECTION_FREE | Self::FLIPPED))
    }

    pub fn is_direction_free(&self) -> bool {
        self.image & Self::DIRECTION_FREE != 0
    }

    pub fn is_flipped(&self) -> bool {
        self.image & Self::FLIPPED != 0
    }
}

/// One frame of an animation: a count byte, a depth byte, then the parts, drawn in
/// order (later parts over earlier ones). A frame can have no parts, and then that
/// animation shows nothing.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    #[br(temp)]
    #[bw(try_calc = u8::try_from(parts.len()))]
    count: u8,
    /// Draw-order key. The game draws one frame of every animation of a model
    /// together, sorted by this key: lower first, so higher keys end up on top, and
    /// equal keys in animation order. A multiple of 5 from 0 to 250 in a stock
    /// install.
    pub depth: u8,
    #[br(count = count)]
    pub parts: Vec<Part>,
}

impl Frame {
    fn size(&self) -> usize {
        2 + 4 * self.parts.len()
    }
}

/// One stored row of an image: `skip` blank pixels, then `pixels`.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Ignored by [`Image::to_indexed`] when `pixels` is empty: empty rows hold
    /// assorted values here, some wider than the image.
    pub skip: u8,
    #[br(temp)]
    #[bw(try_calc = u8::try_from(pixels.len()))]
    count: u8,
    #[br(count = count)]
    pub pixels: Vec<u8>,
}

/// An 8-bit image in row spans, like a `.b@n` row-span sprite but with a two-byte
/// header (width, height).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u8,
    pub height: u8,
    /// One span per row from the top. Normally `height` of them; fewer when the image's
    /// bytes run out at a row boundary (one stock image stores none), and the missing
    /// rows are blank.
    pub rows: Vec<Span>,
}

impl Image {
    /// `width * height` palette indices, row by row, with 0 outside the spans.
    pub fn to_indexed(&self) -> Vec<u8> {
        let width = usize::from(self.width);
        let mut pixels = vec![0; width * usize::from(self.height)];
        for (y, span) in self.rows.iter().enumerate() {
            if !span.pixels.is_empty() {
                let start = y * width + usize::from(span.skip);
                pixels[start..start + span.pixels.len()].copy_from_slice(&span.pixels);
            }
        }
        pixels
    }
}

/// Frames of one animation, in order.
pub type Animation = Vec<Frame>;

/// One view of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub header: ViewHeader,
    /// The animations for `facing`. Empty when there is no facing.
    pub animations: Vec<Animation>,
    /// The animations for `mirror_facing`, or `None` when `animations` serve both
    /// (the facing is its own mirror, or there is none).
    pub mirror_animations: Option<Vec<Animation>>,
    /// Images that both facings' parts draw from, and that other views' parts reach
    /// through [`Part::DIRECTION_FREE`] when this is a `.m00` view.
    pub images: Vec<Image>,
}

/// A decoded model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub views: Vec<View>,
}

/// Decodes a model file and checks every view header.
pub fn parse(bytes: &[u8]) -> Result<Model, FormatError> {
    let invalid = |offset: usize, reason: String| FormatError::Invalid {
        offset: offset as u64,
        reason,
    };

    let mut cursor = std::io::Cursor::new(bytes);
    let mut views = Vec::new();
    loop {
        let at = cursor.position() as usize;
        let size = u16::read_le(&mut cursor)?;
        if size == 0 {
            break;
        }
        cursor.set_position(at as u64);
        let header = ViewHeader::read(&mut cursor)?;
        let size = usize::from(size);
        if size < ViewHeader::SIZE {
            return Err(invalid(
                at,
                format!("view of {size} bytes is shorter than its header"),
            ));
        }
        let view = bytes
            .get(at..at + size)
            .ok_or_else(|| invalid(at, format!("view of {size} bytes runs past the end")))?;
        check_header(&header).map_err(|reason| invalid(at, reason))?;
        views.push(decode_view(header, view).map_err(|(o, reason)| invalid(at + o, reason))?);
        cursor.set_position((at + size) as u64);
    }

    let end = cursor.position() as usize;
    if end != bytes.len() {
        return Err(invalid(
            end,
            format!("{} bytes after the end of the model", bytes.len() - end),
        ));
    }
    Ok(Model { views })
}

/// The facings must be a mirror pair (or both absent), and every offset must fall
/// inside the view, with the tables before the data.
fn check_header(h: &ViewHeader) -> Result<(), String> {
    let data = usize::from(h.data_offset);
    if !(ViewHeader::SIZE..=usize::from(h.size)).contains(&data) {
        return Err(format!("data offset {data} is outside the view"));
    }
    if !h.has_facing() {
        if h.mirror_facing != ViewHeader::NO_FACING
            || h.index_offset != 0
            || h.mirror_index_offset != 0
        {
            return Err("a view with no facing has a mirror facing or an index".into());
        }
        return Ok(());
    }

    let turn = ViewHeader::FACINGS_PER_TURN;
    if !(0..=turn / 2).contains(&h.facing) || h.mirror_facing != (turn - h.facing) % turn {
        return Err(format!(
            "facings {} and {} are not a mirror pair",
            h.facing, h.mirror_facing
        ));
    }
    let tables = [
        h.index_offset,
        h.mirror_index_offset,
        h.lists_offset,
        h.mirror_lists_offset,
    ];
    if let Some(bad) = tables
        .into_iter()
        .map(usize::from)
        .find(|o| !(ViewHeader::SIZE..=data).contains(o))
    {
        return Err(format!("table offset {bad} is outside the view's tables"));
    }
    Ok(())
}

/// An error inside a view: the offset from the start of the view, and why.
type ViewError = (usize, String);

/// Decodes the indexes, frame lists and images after a checked header. Together they
/// must cover the whole view: the first index right after the header, each index's
/// frame lists up to the next table, and the images up to the end.
fn decode_view(header: ViewHeader, view: &[u8]) -> Result<View, ViewError> {
    let h = &header;
    let data = usize::from(h.data_offset);
    let images = decode_images(view, data)?;
    let (mut animations, mut mirror_animations) = (Vec::new(), None);
    if h.has_facing() {
        let index = usize::from(h.index_offset);
        let mirror_index = usize::from(h.mirror_index_offset);
        if index.min(mirror_index) != ViewHeader::SIZE {
            return Err((0, "the first index does not follow the header".into()));
        }
        let tables = [index, mirror_index, data];
        let decode = |index, lists: u16| {
            let lists = usize::from(lists);
            // The lists run to whichever table comes next.
            let end = tables
                .into_iter()
                .filter(|&t| t > lists)
                .min()
                .unwrap_or(data);
            decode_animations(view, index, lists, end, images.len())
        };
        animations = decode(index, h.lists_offset)?;
        if mirror_index != index {
            mirror_animations = Some(decode(mirror_index, h.mirror_lists_offset)?);
        }
    }
    Ok(View {
        header,
        animations,
        mirror_animations,
        images,
    })
}

/// The u16 at `at` in the view, as an offset.
fn word(view: &[u8], at: usize) -> Result<usize, ViewError> {
    match view.get(at..at + 2) {
        Some(w) => Ok(usize::from(u16::from_le_bytes([w[0], w[1]]))),
        None => Err((at, "table runs past the end of the view".into())),
    }
}

/// A two-level index at `index`: a word per animation pointing at its frame list
/// offsets, which run on to the next animation's (the last to `lists`). The frame
/// lists themselves fill `lists..end` in order.
fn decode_animations(
    view: &[u8],
    index: usize,
    lists: usize,
    end: usize,
    image_count: usize,
) -> Result<Vec<Animation>, ViewError> {
    let second = word(view, index)?;
    if second <= index || second > lists || !(second - index).is_multiple_of(2) {
        return Err((
            index,
            format!("index's second level at {second} is misplaced"),
        ));
    }
    let starts = (index..second)
        .step_by(2)
        .map(|at| word(view, at))
        .collect::<Result<Vec<_>, _>>()?;
    let mut next_list = lists;
    let mut animations = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let stop = starts.get(k + 1).copied().unwrap_or(lists);
        if start < second || stop <= start || stop > lists || !(stop - start).is_multiple_of(2) {
            return Err((
                index + 2 * k,
                format!("animation {k}'s frames are misplaced"),
            ));
        }
        let mut frames = Vec::new();
        for at in (start..stop).step_by(2) {
            let list = word(view, at)?;
            if list != next_list {
                return Err((at, format!("frame list at {list} is out of order")));
            }
            let frame = Frame::read(&mut std::io::Cursor::new(&view[list..end]))
                .map_err(|_| (list, "frame list runs past its table".to_string()))?;
            if let Some(p) = frame
                .parts
                .iter()
                .find(|p| !p.is_direction_free() && p.image_index() >= image_count)
            {
                return Err((
                    list,
                    format!("image {} but the view has {image_count}", p.image_index()),
                ));
            }
            next_list += frame.size();
            frames.push(frame);
        }
        animations.push(frames);
    }
    if next_list != end {
        return Err((
            next_list,
            format!("{} bytes after the frame lists", end - next_list),
        ));
    }
    Ok(animations)
}

/// The images from `data` to the end of the view: a table of offsets (the first
/// marks the table's end), then each image up to the next.
fn decode_images(view: &[u8], data: usize) -> Result<Vec<Image>, ViewError> {
    if data == view.len() {
        return Ok(Vec::new());
    }
    let first = word(view, data)?;
    if first <= data || first > view.len() || !(first - data).is_multiple_of(2) {
        return Err((data, format!("image table end {first} is misplaced")));
    }
    let starts = (data..first)
        .step_by(2)
        .map(|at| word(view, at))
        .collect::<Result<Vec<_>, _>>()?;
    let mut images = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let stop = starts.get(k + 1).copied().unwrap_or(view.len());
        if start < first || stop < start + 2 || stop > view.len() {
            return Err((data + 2 * k, format!("image {k} is misplaced")));
        }
        images.push(decode_image(&view[start..stop]).map_err(|(o, r)| (start + o, r))?);
    }
    Ok(images)
}

/// Width, height, then one span per row until the bytes run out.
fn decode_image(bytes: &[u8]) -> Result<Image, ViewError> {
    let (width, height) = (bytes[0], bytes[1]);
    let mut cursor = std::io::Cursor::new(&bytes[2..]);
    let mut rows = Vec::new();
    while rows.len() < usize::from(height) && (cursor.position() as usize) < bytes.len() - 2 {
        let at = 2 + cursor.position() as usize;
        let span = Span::read(&mut cursor).map_err(|_| (at, "row span is cut off".to_string()))?;
        let right = usize::from(span.skip) + span.pixels.len();
        if !span.pixels.is_empty() && right > usize::from(width) {
            return Err((
                at,
                format!("row span ends at {right}, past the width {width}"),
            ));
        }
        rows.push(span);
    }
    let left = bytes.len() - 2 - cursor.position() as usize;
    if left != 0 {
        return Err((
            bytes.len() - left,
            format!("{left} bytes after the last row"),
        ));
    }
    Ok(Image {
        width,
        height,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_bytes, synthetic_model};

    /// A view header followed by `body`, with its size filled in.
    fn view(facing: i16, mirror: i16, offsets: [u16; 5], body: &[u8]) -> Vec<u8> {
        let size = (ViewHeader::SIZE + body.len()) as u16;
        let mut out = Vec::new();
        out.extend(size.to_le_bytes());
        out.extend(facing.to_le_bytes());
        out.extend(mirror.to_le_bytes());
        for o in offsets {
            out.extend(o.to_le_bytes());
        }
        out.extend(body);
        out
    }

    /// The body of a view for facing 8 (mirror 56), laid out by hand. View offsets:
    /// 0x10 index, 0x12 its two frames, 0x16 and 0x1c their lists; 0x1e mirror index,
    /// 0x20 its frame, 0x22 its list; 0x28 image table, 0x2c a 3x2 image, 0x35 a 2x2
    /// image that stores one empty row.
    fn facing_8_body() -> Vec<u8> {
        let mut body = vec![0x12, 0, 0x16, 0, 0x1c, 0];
        body.extend([1, 5, 0, 0, 0xfe, 0xfd]); // image 0 at (-2, -3)
        body.extend([0, 10]); // no parts
        body.extend([0x20, 0, 0x22, 0]);
        body.extend([1, 0, 0, 0x80, 0, 0xfd]); // image 0 flipped at (0, -3)
        body.extend([0x2c, 0, 0x35, 0]);
        body.extend([3, 2, 1, 2, 7, 8, 0, 1, 9]);
        body.extend([2, 2, 0xfe, 0]);
        body
    }

    /// A no-facing view with one image, facing 8 (mirror 56), and facing 32 with one
    /// empty frame, then the terminator. Views start at 0, 23 and 80.
    fn three_views() -> Vec<u8> {
        let mut model = view(
            -2,
            -2,
            [0, 0, 0x1234, 0x5678, 16],
            &[0x12, 0, 1, 1, 0, 1, 4],
        );
        model.extend(view(8, 56, [16, 0x1e, 0x16, 0x22, 0x28], &facing_8_body()));
        model.extend(view(
            32,
            32,
            [16, 16, 0x14, 0x14, 0x16],
            &[0x12, 0, 0x14, 0, 0, 0],
        ));
        model.extend([0, 0]);
        model
    }

    #[test]
    fn views_and_their_facings_decode() {
        let model = parse(&three_views()).unwrap();
        let facings: Vec<_> = model
            .views
            .iter()
            .map(|v| (v.header.facing, v.header.mirror_facing))
            .collect();
        assert_eq!(facings, [(-2, -2), (8, 56), (32, 32)]);
        assert!(!model.views[0].header.has_facing());
        assert_eq!(model.views[0].header.lists_offset, 0x1234);
        assert_eq!(model.views[2].header.data_offset, 0x16);
    }

    #[test]
    fn animations_frames_and_images_decode() {
        let model = parse(&three_views()).unwrap();
        let [free, eight, thirty_two] = &model.views[..] else {
            panic!("three views");
        };
        assert!(free.animations.is_empty() && free.mirror_animations.is_none());
        assert_eq!(free.images[0].to_indexed(), [4]);

        let frames = &eight.animations[0];
        assert_eq!(frames.len(), 2);
        let part = &frames[0].parts[0];
        assert_eq!((part.image_index(), part.x, part.y), (0, -2, -3));
        assert!(!part.is_flipped() && !part.is_direction_free());
        assert_eq!((frames[0].depth, frames[1].depth), (5, 10));
        assert!(frames[1].parts.is_empty());
        let mirror = &eight.mirror_animations.as_ref().unwrap()[0][0].parts[0];
        assert!(mirror.is_flipped());
        assert_eq!(mirror.image_index(), 0);
        assert_eq!(eight.images[0].to_indexed(), [0, 7, 8, 9, 0, 0]);
        let short = &eight.images[1];
        assert_eq!((short.rows.len(), short.rows[0].skip), (1, 0xfe));
        assert_eq!(short.to_indexed(), [0; 4]);

        assert_eq!(
            thirty_two.animations,
            [vec![Frame {
                depth: 0,
                parts: vec![]
            }]]
        );
        assert!(thirty_two.mirror_animations.is_none() && thirty_two.images.is_empty());
    }

    #[test]
    fn model_snapshot() {
        insta::assert_debug_snapshot!(parse(&three_views()).unwrap());
    }

    #[test]
    fn a_lone_terminator_is_an_empty_model() {
        assert!(parse(&[0, 0]).unwrap().views.is_empty());
    }

    #[test]
    fn synthetic_models_round_trip() {
        let big = synthetic_bytes(20 * 7, 3);
        let images: [(u8, u8, &[u8]); 2] = [(1, 1, &[5]), (20, 7, &big)];
        let model = parse(&synthetic_model(&[(-2, &images), (4, &images), (0, &[])])).unwrap();
        for view in &model.views[..2] {
            let decoded: Vec<_> = view.images.iter().map(Image::to_indexed).collect();
            assert_eq!(decoded, [vec![5], big.clone()]);
        }
        let flipped = &model.views[1].mirror_animations.as_ref().unwrap()[0][0];
        assert!(flipped.parts.iter().all(Part::is_flipped));
        assert!(model.views[2].animations[0][0].parts.is_empty());
    }

    #[test]
    fn malformed_models_are_rejected_with_their_offset() {
        let good = three_views();
        // (byte index, new value, offset of the error, reason)
        let cases: [(usize, u8, u64, &str); 13] = [
            (0, 15, 0, "shorter than its header"),
            (23, 200, 23, "runs past the end"),
            (25, 9, 23, "not a mirror pair"),
            (25, 40, 23, "not a mirror pair"),
            (4, 0, 0, "has a mirror facing or an index"),
            (6, 2, 0, "has a mirror facing or an index"),
            (37, 15, 23, "data offset 15 is outside the view"),
            (29, 0x50, 23, "table offset 80 is outside"),
            (29, 0x12, 23, "first index does not follow the header"),
            (43, 0x1d, 43, "frame list at 29 is out of order"),
            (47, 2, 45, "image 2 but the view has 2"),
            (69, 2, 69, "row span ends at 4, past the width 3"),
            (68, 1, 73, "3 bytes after the last row"),
        ];
        for (index, value, offset, reason) in cases {
            let mut model = good.clone();
            model[index] = value;
            match parse(&model) {
                Err(FormatError::Invalid {
                    offset: at,
                    reason: r,
                }) => {
                    assert_eq!(at, offset, "{r}");
                    assert!(r.contains(reason), "{r}");
                }
                other => panic!("expected Invalid for byte {index} = {value}, got {other:?}"),
            }
        }

        let mut trailing = good.clone();
        trailing.push(0);
        assert!(matches!(
            parse(&trailing),
            Err(FormatError::Invalid { offset: 104, .. })
        ));
        let mut unterminated = good;
        unterminated.truncate(unterminated.len() - 2);
        assert!(matches!(parse(&unterminated), Err(FormatError::Binrw(_))));
    }

    /// The facings each suffix holds in a stock install: `.m08` + `.m16` + `.m32`
    /// together cover every even facing from 0 to 32, each with its mirror.
    fn expected_facings(extension: &str) -> Vec<i16> {
        match extension {
            "m00" => vec![ViewHeader::NO_FACING],
            "m08" => (0..=32).step_by(8).collect(),
            "m16" => (4..32).step_by(8).collect(),
            "m32" => (2..32).step_by(4).collect(),
            other => panic!("not a model extension: {other}"),
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_model_decodes_with_its_suffixs_facings() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let mut models: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter_map(|p| {
                let ext = p.extension()?.to_string_lossy().to_lowercase();
                ["m00", "m08", "m16", "m32"]
                    .contains(&ext.as_str())
                    .then_some((p, ext))
            })
            .collect();
        models.sort();
        assert_eq!(models.len(), 40, "a stock install has 40 model files");
        // Sorting puts each model's `.m00` first, so its images are known by the time
        // the other files' direction-free parts refer to them.
        let mut direction_free_images = 0;
        for (path, ext) in models {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            let model =
                parse(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let facings: Vec<_> = model.views.iter().map(|v| v.header.facing).collect();
            assert_eq!(facings, expected_facings(&ext), "{name}");
            if ext == "m00" {
                direction_free_images = model.views[0].images.len();
            }
            for v in &model.views {
                let h = &v.header;
                if h.facing == 0 || h.facing == 32 {
                    assert_eq!(h.index_offset, h.mirror_index_offset, "{name}");
                    assert_eq!(h.lists_offset, h.mirror_lists_offset, "{name}");
                }
                let animations = v
                    .animations
                    .iter()
                    .chain(v.mirror_animations.iter().flatten());
                for part in animations.flatten().flat_map(|f| &f.parts) {
                    if part.is_direction_free() {
                        assert!(part.image_index() < direction_free_images, "{name}");
                    }
                }
            }
        }
    }
}
