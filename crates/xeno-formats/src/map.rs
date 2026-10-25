//! `.map` files: terrain saved by the map editor driver (`mapdvr.dvr`). Not a key or
//! joystick mapping. The records are the same as a `.usg` `MAPP` chunk's, with
//! positions relative to the editor's map origin. See `docs/formats/map.md`.

use binrw::BinWrite;

use crate::FormatError;
use crate::usg::{self, Point, Terrain, TerrainFeature};

/// The map editor's origin (`xeno.exe` `DS:0x14e8`, set at start-up to (128, 128),
/// with a height of 0 at `DS:0x14f0`): loading a `.map` adds it to each record's
/// x and y (and the height to its `z`), saving takes it off x and y.
pub const ORIGIN: Point = Point {
    x: 128 << 16,
    y: 128 << 16,
};

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

/// Encodes terrain records as a `.map` file: the records and the 0 end word, with no
/// padding (`file_write_map`, `mapdvr.dvr` `0x0711`). [`Terrain::padding`] is not
/// written.
pub fn write(terrain: &Terrain) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    for feature in &terrain.features {
        feature.write(&mut out).expect("writing to memory");
    }
    let mut bytes = out.into_inner();
    bytes.extend([0, 0]);
    bytes
}

/// The block the terrain editor writes after its circles (the driver's second record
/// template): at the most negative position, a fraction of a unit in size. With the
/// origin added it is the last `MAPP` record of every stock scenario with terrain.
pub fn closing_block() -> TerrainFeature {
    TerrainFeature {
        size: 24,
        kind: TerrainFeature::BLOCK,
        point_slots: 8,
        x: i32::MIN,
        y: i32::MIN,
        z: 0x2d00,
        radius: 5,
        diagonal: 0x14,
        height: 0x2d,
        shape: vec![TerrainFeature::BLOCK, 0],
    }
}

/// The terrain as the game holds it once loaded from a `.map`
/// (`file_load_map_terrain`): every record moved by [`ORIGIN`] (and the origin's
/// height of 0 added to `z`).
pub fn to_world(terrain: &Terrain) -> Terrain {
    let features = terrain
        .features
        .iter()
        .map(|f| TerrainFeature {
            x: f.x.wrapping_add(ORIGIN.x),
            y: f.y.wrapping_add(ORIGIN.y),
            ..f.clone()
        })
        .collect();
    Terrain {
        features,
        padding: Vec::new(),
    }
}

/// What the terrain editor saves for the world terrain `world` when nothing is
/// edited: it imports each hill and open hill as a circle (centre and radius;
/// `scen_import_terrain_circles`, `0x080d`), skipping the rest, and writes each circle
/// as a hill (`file_write_map_circles`, `0x078c`) with the origin taken off its
/// centre, `diagonal` = `radius * 0xaaaa >> 16`, `height` = `diagonal >> 1` and `z` =
/// `height` as 16.16, then [`closing_block`].
pub fn editor_save(world: &[TerrainFeature]) -> Terrain {
    let circles = world
        .iter()
        .filter(|f| matches!(f.kind, TerrainFeature::HILL | TerrainFeature::OPEN_HILL));
    let mut features: Vec<TerrainFeature> = circles
        .map(|f| {
            let diagonal = ((u32::from(f.radius) * 0xaaaa) >> 16) as u16;
            let height = diagonal >> 1;
            TerrainFeature {
                size: 24,
                kind: TerrainFeature::HILL,
                point_slots: 0x10,
                x: f.x.wrapping_sub(ORIGIN.x),
                y: f.y.wrapping_sub(ORIGIN.y),
                z: i32::from(height) << 8,
                radius: f.radius,
                diagonal,
                height,
                shape: vec![TerrainFeature::HILL, 0],
            }
        })
        .collect();
    features.push(closing_block());
    Terrain {
        features,
        padding: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usg::TerrainFeature;

    /// One made-up hill at (-2.5, 3.0), radius 6.0, as the editor writes it (+0x0c
    /// is a third of the radius, +0x12 and +0x14 two thirds and a third), then the
    /// end marker.
    fn one_circle() -> Vec<u8> {
        let mut map = vec![24, 0, TerrainFeature::HILL, 0x10];
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
        assert_eq!((circle.centre(), circle.z), (centre, 0x2_0000));
        assert_eq!(
            (circle.radius, circle.diagonal, circle.height),
            (0x600, 0x400, 0x200)
        );
        assert_eq!(circle.shape, [TerrainFeature::HILL, 0]);
        // In the world the editor's origin is added.
        assert_eq!(ORIGIN.x + circle.x, 0x7d_8000);
        assert!(terrain.padding.is_empty());
        assert!(parse(&[0, 0]).unwrap().features.is_empty());
    }

    #[test]
    fn map_snapshot() {
        // A block with a made-up position and size, and an empty shape program.
        let mut block = vec![0x16, 0, TerrainFeature::BLOCK, 8];
        block.extend([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 0, 0,
        ]);
        insta::assert_debug_snapshot!(parse(&block).unwrap());
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
    fn maps_write_and_read_back() {
        let map = one_circle();
        assert_eq!(write(&parse(&map).unwrap()), map);
        assert_eq!(write(&parse(&[0, 0]).unwrap()), [0, 0]);
    }

    #[test]
    fn the_editor_saves_circles_as_hills_and_closes_with_a_block() {
        // In the world: an open hill of radius 7.5 at (130.25, 120), a block, and a
        // hill whose sizes do not follow the editor's.
        let world = to_world(&parse(&one_circle()).unwrap()).features;
        let mut open = world[0].clone();
        (open.kind, open.x, open.y, open.radius) =
            (TerrainFeature::OPEN_HILL, 0x82_4000, 0x78_0000, 0x780);
        let block = closing_block();
        let saved = editor_save(&[open, block, world[0].clone()]);
        let [first, second, last] = &saved.features[..] else {
            panic!("{saved:?}");
        };
        assert_eq!(
            (first.kind, first.x, first.y),
            (TerrainFeature::HILL, 0x2_4000, -0x8_0000)
        );
        // 0x780 * 0xaaaa >> 16 = 0x4ff; half of that, and that as 16.16.
        assert_eq!(
            (first.radius, first.diagonal, first.height),
            (0x780, 0x4ff, 0x27f)
        );
        assert_eq!(
            (first.z, first.shape.as_slice()),
            (0x2_7f00, &[0x14, 0][..])
        );
        // The made-up circle's own sizes give way to the editor's.
        assert_eq!(
            (second.x, second.radius, second.diagonal, second.height),
            (-0x2_8000, 0x600, 0x3ff, 0x1ff)
        );
        assert_eq!(*last, closing_block());
        // Saved and loaded again, nothing changes.
        let again = editor_save(&to_world(&saved).features);
        assert_eq!(again, saved);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_maps_round_trip() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for name in ["default.map", "backup.map"] {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            assert_eq!(write(&parse(&bytes).unwrap()), bytes, "{name}");
        }
        // `backup.map` is a save with no circles.
        let backup = parse(&std::fs::read(dir.join("backup.map")).unwrap()).unwrap();
        assert_eq!(backup, editor_save(&[]));
        // Every stock scenario's terrain is what the terrain editor saves for it,
        // loaded back with the origin added.
        let mut names: Vec<String> = (0..13).map(|n| format!("scen{n:02}a.usg")).collect();
        names.extend(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
        for name in names {
            let s = usg::parse(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
            let features = s.terrain.unwrap().features;
            if features.is_empty() {
                continue;
            }
            let saved = to_world(&editor_save(&features)).features;
            assert_eq!(saved, features, "{name}");
        }
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
            // Both end with the editor's closing block, at the most negative position
            // (moved by 128 in `default.map`).
            let last = terrain.features.last().unwrap();
            assert_eq!(last.kind, TerrainFeature::BLOCK, "{name}");
            assert!(last.x.unsigned_abs() >= 0x7f80_0000, "{name}");
            // Each shape program is one built-in shape and the end (the open hill in
            // `default.map` is drawn by the hill's).
            for f in &terrain.features {
                assert_eq!(f.shape.len(), 2, "{name}");
                assert!(matches!(f.shape[..], [0x12 | 0x14, 0]), "{name}");
            }
        }
    }
}
