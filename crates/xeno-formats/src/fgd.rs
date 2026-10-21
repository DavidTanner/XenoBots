//! `.fgd` foreground layers (and the unused `fgarray.kyl`): the black silhouettes that
//! scroll in front of the intro drop backdrops. A layer is not a grid: it is one row
//! number for each of 1920 columns. See `docs/formats/fgd.md`.

use std::fmt;

use binrw::{BinRead, binrw};

use crate::FormatError;

/// One layer: for each column, the backdrop row where the silhouette's edge is.
///
/// A floor layer is drawn in colour 0 from that row down to the bottom of the
/// backdrop; a ceiling layer from the top of the backdrop down to that row (both
/// inclusive). Which kind a file is depends on which buffer `vgaintro.dvr` loads it
/// into, not on its contents; see [`Role::of`].
#[binrw]
#[brw(little)]
#[derive(Clone, PartialEq, Eq)]
pub struct Layer {
    pub rows: [u8; Layer::LEN],
}

impl Layer {
    /// Columns in a layer, and the size of every file.
    pub const LEN: usize = 1920;
    /// Columns drawn at once: the width of the drop backdrops and of `vgaintro.dvr`'s
    /// virtual screen.
    pub const WINDOW: usize = 768;
    /// Where every layer's window starts. The scroll then lowers the offset, so the
    /// scene moves right over the screen and the start of the file comes last.
    pub const START_OFFSET: usize = Self::LEN - Self::WINDOW;

    /// The 768 rows drawn when the layer's scroll offset is `offset`, or `None` if
    /// the window would run past the end of the layer.
    pub fn window(&self, offset: usize) -> Option<&[u8]> {
        self.rows.get(offset..offset.checked_add(Self::WINDOW)?)
    }

    /// The floor edge drawn from two floor layers: for each column, the higher (lower
    /// numbered) of the two rows, as `vgaintro.dvr` combines them.
    pub fn floor(near: &[u8], far: &[u8]) -> Vec<u8> {
        near.iter().zip(far).map(|(&a, &b)| a.min(b)).collect()
    }
}

/// Run-length form, so a 1920-column layer prints as a few lines.
impl fmt::Debug for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut runs = Vec::new();
        for chunk in self.rows.chunk_by(|a, b| a == b) {
            runs.push(match chunk.len() {
                1 => format!("{}", chunk[0]),
                n => format!("{}x{n}", chunk[0]),
            });
        }
        f.debug_struct("Layer")
            .field("rows", &format_args!("[{}]", runs.join(", ")))
            .finish()
    }
}

/// What `vgaintro.dvr` uses a file for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The near floor layer (buffer A, the faster scroll).
    NearFloor,
    /// The far floor layer (buffer B, the slower scroll).
    FarFloor,
    /// The cave ceiling (buffer C, same speed as the near floor).
    Ceiling,
    /// Not named by any binary in the install.
    Unused,
}

impl Role {
    /// The role of a file by name (case-insensitive), with the backdrop it scrolls
    /// over, or `None` for a name that is not one of the six layer files.
    pub fn of(file_name: &str) -> Option<(Role, &'static str)> {
        Some(match file_name.to_ascii_lowercase().as_str() {
            "layer3.fgd" => (Role::NearFloor, "xenodrop.pcx"),
            "layer4.fgd" => (Role::FarFloor, "xenodrop.pcx"),
            "layer1.fgd" => (Role::NearFloor, "cavedrop.pcx"),
            "layer2.fgd" => (Role::FarFloor, "cavedrop.pcx"),
            "layer5.fgd" => (Role::Ceiling, "cavedrop.pcx"),
            "fgarray.kyl" => (Role::Unused, "none"),
            _ => return None,
        })
    }
}

/// Decodes a layer, which must be exactly [`Layer::LEN`] bytes.
pub fn parse(bytes: &[u8]) -> Result<Layer, FormatError> {
    if bytes.len() != Layer::LEN {
        return Err(FormatError::WrongLength {
            expected: Layer::LEN,
            actual: bytes.len(),
        });
    }
    Ok(Layer::read(&mut std::io::Cursor::new(bytes))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up ridge: flat at row 100, a ramp up to row 60 and back down, then a
    /// lower flat stretch at the end.
    fn ridge() -> Vec<u8> {
        let mut rows = vec![100; Layer::LEN];
        for i in 0..9 {
            rows[1000 + i] = 100 - 10 * i.min(8 - i) as u8;
        }
        rows[Layer::LEN - 100..].fill(120);
        rows
    }

    #[test]
    fn synthetic_layer_decodes() {
        let layer = parse(&ridge()).unwrap();
        assert_eq!(layer.rows[1004], 60);
        assert_eq!(layer.rows[Layer::LEN - 1], 120);
        let window = layer.window(Layer::START_OFFSET).unwrap();
        assert_eq!(window.len(), Layer::WINDOW);
        assert_eq!(window[Layer::WINDOW - 1], 120);
        assert!(layer.window(Layer::START_OFFSET + 1).is_none());
        assert!(layer.window(usize::MAX).is_none());
    }

    #[test]
    fn layer_snapshot() {
        insta::assert_debug_snapshot!(parse(&ridge()).unwrap());
    }

    #[test]
    fn floor_takes_the_higher_edge() {
        assert_eq!(Layer::floor(&[5, 90, 7], &[50, 9, 7]), [5, 9, 7]);
    }

    #[test]
    fn wrong_sizes_are_rejected() {
        assert!(matches!(
            parse(&[0; 1919]),
            Err(FormatError::WrongLength {
                expected: 1920,
                actual: 1919
            })
        ));
        assert!(parse(&[0; 1921]).is_err());
    }

    #[test]
    fn roles_follow_the_loader() {
        assert_eq!(
            Role::of("LAYER5.FGD"),
            Some((Role::Ceiling, "cavedrop.pcx"))
        );
        assert_eq!(Role::of("fgarray.kyl").map(|r| r.0), Some(Role::Unused));
        assert_eq!(Role::of("layer6.fgd"), None);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_fgd_layers_decode() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for name in [
            "layer1.fgd",
            "layer2.fgd",
            "layer3.fgd",
            "layer4.fgd",
            "layer5.fgd",
            "fgarray.kyl",
        ] {
            let layer = parse(&std::fs::read(dir.join(name)).unwrap()).unwrap();
            // Rows stay within the 128- and 130-row backdrops (plus a little slack).
            assert!(layer.rows.iter().all(|&r| r <= 130), "{name}");
            // Profiles, not pictures: neighbouring columns are close on average.
            let steps: usize = layer
                .rows
                .windows(2)
                .map(|w| usize::from(w[0].abs_diff(w[1])))
                .sum();
            assert!(steps < Layer::LEN, "{name}: {steps}");
        }
    }
}
