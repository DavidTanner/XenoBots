//! The ground and the paths: a `.usg` file's `MAPP` records and its `APTH`/`HPTH`
//! tables (`docs/formats/usg.md`, "The world", "`MAPP`: terrain features", "How paths
//! are used").
//!
//! The world has no edges, so nothing here limits a position. The tests below are the
//! original's: an approximate distance, the first record containing a point, and the
//! heights it gives.

use xeno_formats::usg::{self, Scenario, Side, TerrainFeature};

use crate::hash::{StateHash, StateHasher};
use crate::world::{Fixed, Position, side_code};

/// The original's distance (`sim_approx_distance_to_point`): the larger of the two
/// differences plus half the smaller, 16.16. A difference that overflows 32 bits
/// gives 0x7fff.7fff.
pub fn approx_distance(a: Position, b: Position) -> Fixed {
    let (Some(dx), Some(dy)) = (a.x.0.checked_sub(b.x.0), a.y.0.checked_sub(b.y.0)) else {
        return Fixed(0x7fff_7fff);
    };
    let (dx, dy) = (dx.unsigned_abs(), dy.unsigned_abs());
    Fixed((dx.max(dy).wrapping_add(dx.min(dy) / 2)) as i32)
}

/// One `MAPP` record (`usg::TerrainFeature`) in world terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    /// [`TerrainFeature::HILL`], [`TerrainFeature::OPEN_HILL`],
    /// [`TerrainFeature::BLOCK`] or another value.
    pub kind: u8,
    pub centre: Position,
    /// +0x0c: the height the shape is drawn from.
    pub z: Fixed,
    /// +0x10, +0x12, +0x14: 8.8.
    pub radius: u16,
    pub diagonal: u16,
    pub height: u16,
}

impl Feature {
    fn of(t: &TerrainFeature) -> Feature {
        Feature {
            kind: t.kind,
            centre: Position {
                x: Fixed(t.x),
                y: Fixed(t.y),
            },
            z: Fixed(t.z),
            radius: t.radius,
            diagonal: t.diagonal,
            height: t.height,
        }
    }

    /// Whether `p` is inside the radius: the distance, in 8.8 (its low byte
    /// dropped), is less than the radius.
    pub fn contains(&self, p: Position) -> bool {
        (approx_distance(self.centre, p).0 as u32 >> 8) < u32::from(self.radius)
    }

    /// The ground on top: `z` plus the 8.8 height.
    pub fn top(&self) -> Fixed {
        Fixed(self.z.0.wrapping_add(i32::from(self.height) << 8))
    }
}

/// The terrain records in file order: the tests take the first that contains a point.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terrain {
    pub features: Vec<Feature>,
}

impl Terrain {
    /// `scenario`'s `MAPP` records; none if it has no such chunk.
    pub fn from_scenario(scenario: &Scenario) -> Terrain {
        let features = scenario.terrain.iter().flat_map(|t| &t.features);
        Terrain {
            features: features.map(Feature::of).collect(),
        }
    }

    /// The first hill containing `p` (`sim_test_point_in_hill`): a robot that ends a
    /// move there backs off. Other kinds do not stop robots.
    pub fn hill_at(&self, p: Position) -> Option<&Feature> {
        self.features
            .iter()
            .find(|f| f.kind == TerrainFeature::HILL && f.contains(p))
    }

    /// The ground's height at `p` (`sim_set_height_to_ground`): the top of the first
    /// hill containing it, else 0.
    pub fn ground_height(&self, p: Position) -> Fixed {
        self.hill_at(p).map_or(Fixed(0), Feature::top)
    }

    /// Whether a point at height `z` is below the ground (`sim_test_below_ground`, the
    /// shots' test): under the top of the first record of any kind containing `p`,
    /// else under 0.
    pub fn below_ground(&self, p: Position, z: Fixed) -> bool {
        let ground = self.features.iter().find(|f| f.contains(p));
        z.0.wrapping_sub(ground.map_or(0, |f| f.top().0)) < 0
    }
}

impl StateHash for Terrain {
    fn state_hash(&self, h: &mut StateHasher) {
        h.u64(self.features.len() as u64);
        for f in &self.features {
            h.u8(f.kind);
            h.i32(f.centre.x.0);
            h.i32(f.centre.y.0);
            h.i32(f.z.0);
            h.u16(f.radius);
            h.u16(f.diagonal);
            h.u16(f.height);
        }
    }
}

/// One path in use (`usg::Path`): its mission, the alien base's pick weight and its
/// points. The editor's kind mix and the counts of robots on it are not kept: only
/// the editor reads the mix, and the counts are worked out during play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub mission: u8,
    /// Each path gets `weight + 1` chances when the alien base picks one.
    pub weight: u8,
    pub points: Vec<Position>,
}

/// Both sides' paths, in slot order (a robot's +0x2c names its path by slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paths {
    pub human: Vec<Path>,
    pub alien: Vec<Path>,
}

impl Paths {
    /// The paths in use of `scenario`'s `HPTH` and `APTH`; none for a missing chunk.
    pub fn from_scenario(scenario: &Scenario) -> Paths {
        let side = |table: &Option<usg::PathTable>| -> Vec<Path> {
            let used = table.iter().flat_map(|t| t.used());
            used.map(|p| Path {
                mission: p.mission,
                weight: p.weight,
                points: p
                    .waypoints()
                    .iter()
                    .map(|w| Position {
                        x: Fixed(w.x),
                        y: Fixed(w.y),
                    })
                    .collect(),
            })
            .collect()
        };
        Paths {
            human: side(&scenario.human_paths),
            alien: side(&scenario.alien_paths),
        }
    }

    pub fn side(&self, side: Side) -> &[Path] {
        match side {
            Side::Human => &self.human,
            Side::Alien => &self.alien,
        }
    }
}

impl StateHash for Paths {
    fn state_hash(&self, h: &mut StateHasher) {
        for side in [Side::Human, Side::Alien] {
            let paths = self.side(side);
            h.u8(side_code(side));
            h.u64(paths.len() as u64);
            for p in paths {
                h.u8(p.mission);
                h.u8(p.weight);
                h.u64(p.points.len() as u64);
                for at in &p.points {
                    h.i32(at.x.0);
                    h.i32(at.y.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> Position {
        Position {
            x: Fixed(x),
            y: Fixed(y),
        }
    }

    #[test]
    fn distance_is_the_larger_difference_plus_half_the_smaller() {
        let o = at(0, 0);
        assert_eq!(approx_distance(at(0x3_0000, -0x4_0000), o), Fixed(0x5_8000));
        assert_eq!(approx_distance(o, at(0x3_0000, -0x4_0000)), Fixed(0x5_8000));
        assert_eq!(approx_distance(at(5, 5), at(5, 5)), Fixed(0));
        // An overflowing difference saturates.
        let far = at(i32::MIN, 0);
        assert_eq!(approx_distance(at(1, 0), far), Fixed(0x7fff_7fff));
        assert_eq!(approx_distance(at(-1, 0), far).0, i32::MAX);
    }

    fn hill(kind: u8, x: i16, radius: u16, z: i32, height: u16) -> Feature {
        Feature {
            kind,
            centre: at(i32::from(x) << 16, 0),
            z: Fixed(z),
            radius,
            diagonal: radius * 2 / 3,
            height,
        }
    }

    #[test]
    fn hills_stop_robots_and_every_record_stops_shots() {
        let terrain = Terrain {
            features: vec![
                // A hill of radius 6 at x 10, top 2 + 1; an open hill at x 30; a
                // block at x 50.
                hill(TerrainFeature::HILL, 10, 0x600, 0x2_0000, 0x100),
                hill(TerrainFeature::OPEN_HILL, 30, 0x600, 0x2_0000, 0x100),
                hill(TerrainFeature::BLOCK, 50, 0x600, 0, 0x100),
            ],
        };
        let x = |n: i32| at(n << 16, 0);
        assert!(terrain.hill_at(x(15)).is_some());
        // The distance's fraction below 1/256 does not count: 6 - 1/65536 is inside.
        assert!(terrain.hill_at(at(0x10_0000 - 0x5_ffff, 0)).is_some());
        assert!(terrain.hill_at(x(4)).is_none());
        assert_eq!(terrain.ground_height(x(12)), Fixed(0x3_0000));
        assert_eq!(terrain.ground_height(x(30)), Fixed(0));
        assert!(terrain.hill_at(x(30)).is_none());
        // Shots: under a record's top, or under 0 in the open.
        for n in [10, 30] {
            assert!(terrain.below_ground(x(n), Fixed(0x2_ffff)));
            assert!(!terrain.below_ground(x(n), Fixed(0x3_0000)));
        }
        assert!(terrain.below_ground(x(50), Fixed(0xffff)));
        assert!(terrain.below_ground(x(70), Fixed(-1)));
        assert!(!terrain.below_ground(x(70), Fixed(0)));
    }

    #[test]
    fn a_scenario_s_terrain_and_paths_load() {
        let plain = xeno_formats::usg::parse(&xeno_formats::fixtures::synthetic_usg()).unwrap();
        let terrain = Terrain::from_scenario(&plain);
        assert_eq!(terrain.features.len(), 2);
        let first = terrain.features[0];
        assert_eq!(first.kind, TerrainFeature::HILL);
        assert_eq!(first.centre, at(120 << 16, -2 << 16));
        assert_eq!(first.top(), Fixed(0x3_2100 + 0x3_2100));
        let paths = Paths::from_scenario(&plain);
        assert!(paths.side(Side::Human).is_empty());
        let recon = &paths.side(Side::Alien)[0];
        assert_eq!((recon.mission, recon.weight, recon.points.len()), (3, 1, 2));
        assert_eq!(recon.points[1], at(95 << 16 | 0x8888, 136 << 16 | 0x7171));
        let bare = Scenario {
            terrain: None,
            alien_paths: None,
            ..plain
        };
        assert_eq!(Terrain::from_scenario(&bare), Terrain::default());
        assert!(Paths::from_scenario(&bare).alien.is_empty());
    }
}
