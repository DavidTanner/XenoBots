//! `.m00` / `.m08` / `.m16` / `.m32` unit models (`mgavideo.dvr` calls them `.MXX`).
//! A model is a chain of views. Each view draws a unit from one facing and from its
//! mirror image, measured in 64ths of a turn. The suffix says which facings a file
//! holds: `.m08` the 8-way set, `.m16` the extra four for 16 ways, `.m32` the extra
//! eight for 32 ways, and `.m00` a single view with no facing. The view header is
//! decoded here; the tables and pixels after it are kept as bytes (T-016). See
//! `docs/formats/mxx.md`.

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
    /// is no facing: these two words then hold unknown values.
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

/// One view of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub header: ViewHeader,
    /// Everything after the header: the indexes, lists and data (undecoded). Byte `i`
    /// is at view offset `ViewHeader::SIZE + i`.
    pub body: Vec<u8>,
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
        let body = bytes
            .get(at + ViewHeader::SIZE..at + size)
            .ok_or_else(|| invalid(at, format!("view of {size} bytes runs past the end")))?;
        check_header(&header).map_err(|reason| invalid(at, reason))?;
        views.push(View {
            header,
            body: body.to_vec(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A no-facing view, then facing 8 (mirror 56) and facing 32, then the terminator.
    fn three_views() -> Vec<u8> {
        let mut model = view(-2, -2, [0, 0, 0x1234, 0x5678, 16], &[9, 9]);
        model.extend(view(8, 56, [16, 18, 17, 19, 20], &[1, 2, 3, 4, 5, 6]));
        model.extend(view(32, 32, [16, 16, 17, 17, 18], &[7, 8, 9]));
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
        assert_eq!(model.views[1].body, [1, 2, 3, 4, 5, 6]);
        assert_eq!(model.views[2].header.data_offset, 18);
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
    fn malformed_models_are_rejected_with_their_offset() {
        let good = three_views();
        // (byte index, new value, offset of the error, reason)
        let cases: [(usize, u8, u64, &str); 8] = [
            (0, 15, 0, "shorter than its header"),
            (18, 200, 18, "runs past the end"),
            (20, 9, 18, "not a mirror pair"),
            (20, 40, 18, "not a mirror pair"),
            (4, 0, 0, "has a mirror facing or an index"),
            (6, 2, 0, "has a mirror facing or an index"),
            (32, 23, 18, "data offset 23 is outside the view"),
            (24, 30, 18, "table offset 30 is outside"),
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
            Err(FormatError::Invalid { offset: 61, .. })
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
        for (path, ext) in models {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            let model =
                parse(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let facings: Vec<_> = model.views.iter().map(|v| v.header.facing).collect();
            assert_eq!(facings, expected_facings(&ext), "{name}");
            for v in &model.views {
                let h = &v.header;
                if h.facing == 0 || h.facing == 32 {
                    assert_eq!(h.index_offset, h.mirror_index_offset, "{name}");
                    assert_eq!(h.lists_offset, h.mirror_lists_offset, "{name}");
                }
            }
        }
    }
}
