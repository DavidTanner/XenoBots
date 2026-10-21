//! `.map` files: terrain saved by the map editor driver (`mapdvr.dvr`). Not a key or
//! joystick mapping. The records are the same as a `.usg` `MAPP` chunk's, with
//! positions relative to the editor's map origin. See `docs/formats/map.md`.

use crate::FormatError;
use crate::usg::{self, Terrain};

/// Decodes a `.map` file: terrain records ([`usg::TerrainFeature`]) until a size word
/// of 0, which must end the file. Unlike a `MAPP` chunk there is no padding, so
/// [`Terrain::padding`] is always empty.
pub fn parse(bytes: &[u8]) -> Result<Terrain, FormatError> {
    if bytes.is_empty() {
        return Err(FormatError::Invalid {
            offset: 0,
            reason: "no end marker".into(),
        });
    }
    let terrain = usg::parse_terrain(bytes)?;
    if !terrain.padding.is_empty() {
        let end = bytes.len() - terrain.padding.len();
        return Err(FormatError::Invalid {
            offset: end as u64,
            reason: format!("{} bytes after the end marker", terrain.padding.len()),
        });
    }
    Ok(terrain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usg::{Point, TerrainFeature};

    /// One made-up circle at (-2.5, 3.0), radius 6.0, as the editor writes it (+0x0c
    /// is a third of the radius, +0x12 and +0x14 two thirds and a third), then the
    /// end marker.
    fn one_circle() -> Vec<u8> {
        let mut map = vec![24, 0, TerrainFeature::CIRCLE, 0x10];
        map.extend((-0x2_8000i32).to_le_bytes());
        map.extend(0x3_0000i32.to_le_bytes());
        map.extend(0x2_0000i32.to_le_bytes());
        for word in [0x600u16, 0x400, 0x200, 0x14, 0] {
            map.extend(word.to_le_bytes());
        }
        map
    }

    #[test]
    fn synthetic_map_decodes() {
        let terrain = parse(&one_circle()).unwrap();
        assert_eq!(terrain.features.len(), 1);
        let circle = &terrain.features[0];
        let centre = Point {
            x: -0x2_8000,
            y: 0x3_0000,
        };
        assert_eq!(
            (circle.centre(), circle.radius()),
            (Some(centre), Some(0x600))
        );
        assert!(terrain.padding.is_empty());
        assert!(parse(&[0, 0]).unwrap().features.is_empty());
    }

    #[test]
    fn map_snapshot() {
        // A record with no data keeps the snapshot short.
        insta::assert_debug_snapshot!(parse(&[4, 0, 0x12, 0x08, 0, 0]).unwrap());
    }

    #[test]
    fn malformed_maps_are_rejected() {
        assert!(matches!(
            parse(&[]),
            Err(FormatError::Invalid { offset: 0, .. })
        ));

        let mut trailing = one_circle();
        trailing.push(0);
        assert!(matches!(
            parse(&trailing),
            Err(FormatError::Invalid { offset: 26, reason }) if reason.contains("1 bytes after")
        ));

        let unterminated = &one_circle()[..24];
        assert!(matches!(parse(unterminated), Err(FormatError::Binrw(_))));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_maps_decode() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for (name, count) in [("default.map", 5), ("backup.map", 1)] {
            let terrain = parse(&std::fs::read(dir.join(name)).unwrap()).unwrap();
            assert_eq!(terrain.features.len(), count, "{name}");
            assert!(terrain.features.iter().all(|f| f.size == 24), "{name}");
            // Both end with the editor's closing record.
            assert_eq!(terrain.features.last().unwrap().kind, 0x12, "{name}");
        }
    }
}
