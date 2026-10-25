//! The world: the mission's objects by their slot in the original's object table.
//!
//! The shape follows a `.usg` file's `DCBS` objects (`docs/formats/usg.md`): a slot, a
//! kind, a side, a 16.16 position, a heading, a radius and the damage taken, so
//! [`World::from_scenario`] copies them field for field, with the terrain and the paths
//! ([`crate::terrain`]). There is no map grid and no world edge: the original has
//! neither, so positions are never limited (`docs/formats/usg.md`, "The world").

use std::collections::BTreeMap;

use xeno_formats::usg::{Body, Object, ObjectKind, Scenario, Side};

use crate::hash::{StateHash, StateHasher};
use crate::orders::OrderBook;
use crate::terrain::{Paths, Terrain};

/// A 16.16 fixed-point number, as the original keeps positions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fixed(pub i32);

impl Fixed {
    pub const ONE: Fixed = Fixed(0x1_0000);

    pub fn from_int(n: i16) -> Fixed {
        Fixed(i32::from(n) << 16)
    }

    /// The whole part, rounded towards minus infinity.
    pub fn floor(self) -> i16 {
        (self.0 >> 16) as i16
    }
}

/// Where an object is: `.usg` objects' +0x04 and +0x08.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub x: Fixed,
    pub y: Fixed,
}

/// An object's slot in the original's object table (`usg::Object::slot`, below 240).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId(pub u16);

/// One object. Its id is its key in the [`World`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub kind: ObjectKind,
    pub side: Side,
    pub position: Position,
    /// +0x10: 65,536 to a turn.
    pub heading: u16,
    /// +0x14: 8.8 fixed point.
    pub radius: u16,
    pub damage: Damage,
}

/// What an object has taken (`docs/formats/usg.md`, "How robots take damage").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Damage {
    /// A robot has no hit points: hits wear its components down.
    Robot(Components),
    /// A base (destroyed at 300) or a relay (removed at 20): the damage so far.
    Taken(u16),
    /// Obstacles take none, and objects without a known layout have none recorded.
    #[default]
    None,
}

/// A robot's components (`usg::Robot`): two percentages and the damage bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Components {
    /// +0x24, +0x4e: 0 to 100.
    pub helm: u8,
    pub power: u8,
    /// +0xdc: weapons damaged.
    pub weapons: u8,
    /// +0xec bit 0: the targeting computer is damaged.
    pub targeting: bool,
    /// +0xf6: cameras, map, radar and sensors damaged, weapons destroyed.
    pub systems: u8,
}

impl Damage {
    /// The damage recorded in `object`'s layout.
    pub fn of(object: &Object) -> Damage {
        match &object.body {
            Body::Robot(r) => Damage::Robot(Components {
                helm: r.helm,
                power: r.power,
                weapons: r.weapons,
                targeting: r.targeting & 1 != 0,
                systems: r.systems,
            }),
            Body::Base(b) => Damage::Taken(b.damage),
            Body::Relay(r) => Damage::Taken(u16::from(r.damage)),
            Body::Other(_) => Damage::None,
        }
    }
}

impl StateHash for Damage {
    fn state_hash(&self, h: &mut StateHasher) {
        match *self {
            Damage::Robot(c) => {
                h.u8(0);
                h.bytes(&[c.helm, c.power, c.weapons, u8::from(c.targeting), c.systems]);
            }
            Damage::Taken(n) => {
                h.u8(1);
                h.u16(n);
            }
            Damage::None => h.u8(2),
        }
    }
}

/// A side as a hashed byte.
pub(crate) fn side_code(side: Side) -> u8 {
    match side {
        Side::Human => 0,
        Side::Alien => 1,
    }
}

/// Every object, in slot order, the robots' orders, the terrain and both sides' paths.
/// A `BTreeMap`, so iteration
/// (and so the hash and any rule that walks the objects) never depends on insertion
/// order or a hasher's seed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct World {
    entities: BTreeMap<EntityId, Entity>,
    pub orders: OrderBook,
    pub terrain: Terrain,
    pub paths: Paths,
}

/// How many objects of each kind one side has. The units are the robots: the kinds a
/// side's unit list, roster and consoles step through (`docs/screens.md`, "Switching
/// views"), and the ones with orders. Bases, relays and obstacles are not units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    pub humanoids: usize,
    pub scorpions: usize,
    pub scouts: usize,
    pub bases: usize,
    pub relays: usize,
    pub obstacles: usize,
    /// Kinds the editor has no name for; none in the stock files.
    pub other: usize,
}

impl Census {
    /// The robots.
    pub fn units(&self) -> usize {
        self.humanoids + self.scorpions + self.scouts
    }

    fn add(&mut self, kind: ObjectKind) {
        let count = match kind {
            ObjectKind::Humanoid => &mut self.humanoids,
            ObjectKind::Scorpion => &mut self.scorpions,
            ObjectKind::Scout => &mut self.scouts,
            ObjectKind::Base => &mut self.bases,
            ObjectKind::Relay => &mut self.relays,
            ObjectKind::Obstacle => &mut self.obstacles,
            ObjectKind::Other(_) => &mut self.other,
        };
        *count += 1;
    }
}

impl World {
    /// The objects of `scenario` (its `DCBS` chunk), each in its slot with its kind,
    /// side, position, heading, radius and damage, the robots' orders as loading sets
    /// them, the terrain (`MAPP`) and the paths (`APTH`, `HPTH`) (robots without
    /// the stock layout have none; no stock robot lacks it). If two objects share a
    /// slot, the later one is kept; no stock file has that.
    /// A file with no `DCBS` chunk gives an empty world. The objects' other bytes are
    /// not copied: they stay in `scenario` until T-058 names them.
    pub fn from_scenario(scenario: &Scenario) -> World {
        let entities = scenario
            .objects
            .iter()
            .flatten()
            .map(|o| {
                let entity = Entity {
                    kind: o.kind,
                    side: o.side(),
                    position: Position {
                        x: Fixed(o.x),
                        y: Fixed(o.y),
                    },
                    heading: o.heading,
                    radius: o.radius,
                    damage: Damage::of(o),
                };
                (EntityId(o.slot), entity)
            })
            .collect();
        World {
            entities,
            orders: OrderBook::from_scenario(scenario),
            terrain: Terrain::from_scenario(scenario),
            paths: Paths::from_scenario(scenario),
        }
    }

    /// Playing a campaign scenario that was not loaded (`sim_apply_scenario_alert`):
    /// `damage`, the alert level's from the user's `xeno.exe`
    /// (`xeno_formats::campaign::Campaign::alert_damage`), is added to the player's
    /// base, the human base in the lowest slot (the stock files have one). Returns
    /// whether there was one to damage.
    pub fn apply_alert(&mut self, damage: u16) -> bool {
        let base = self
            .entities
            .values_mut()
            .find(|e| e.kind == ObjectKind::Base && e.side == Side::Human);
        match base {
            Some(Entity {
                damage: Damage::Taken(taken),
                ..
            }) => {
                *taken = taken.wrapping_add(damage);
                true
            }
            _ => false,
        }
    }

    /// How many objects of each kind `side` has.
    pub fn census(&self, side: Side) -> Census {
        let mut census = Census::default();
        self.entities
            .values()
            .filter(|e| e.side == side)
            .for_each(|e| census.add(e.kind));
        census
    }

    /// Puts `entity` in slot `id`, returning the one it replaces.
    pub fn insert(&mut self, id: EntityId, entity: Entity) -> Option<Entity> {
        self.entities.insert(id, entity)
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        self.entities.remove(&id)
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(&id)
    }

    /// The objects in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter().map(|(&id, e)| (id, e))
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

impl StateHash for World {
    fn state_hash(&self, h: &mut StateHasher) {
        h.u64(self.entities.len() as u64);
        for (id, e) in self.iter() {
            h.u16(id.0);
            h.u16(e.kind.into());
            h.u8(side_code(e.side));
            h.i32(e.position.x.0);
            h.i32(e.position.y.0);
            h.u16(e.heading);
            h.u16(e.radius);
            e.damage.state_hash(h);
        }
        self.orders.state_hash(h);
        self.terrain.state_hash(h);
        self.paths.state_hash(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn robot(x: i16, y: i16) -> Entity {
        Entity {
            kind: ObjectKind::Scout,
            side: Side::Human,
            position: Position {
                x: Fixed::from_int(x),
                y: Fixed::from_int(y),
            },
            heading: 0,
            radius: 0xa00,
            damage: Damage::None,
        }
    }

    fn hash(world: &World) -> u64 {
        let mut h = StateHasher::default();
        world.state_hash(&mut h);
        h.finish()
    }

    #[test]
    fn objects_are_kept_by_slot_whatever_the_order_they_came_in() {
        let (mut a, mut b) = (World::default(), World::default());
        for (slot, x) in [(7, 1), (2, 2), (200, 3)] {
            a.insert(EntityId(slot), robot(x, 0));
        }
        for (slot, x) in [(200, 3), (7, 1), (2, 2)] {
            b.insert(EntityId(slot), robot(x, 0));
        }
        assert_eq!(a, b);
        assert_eq!(hash(&a), hash(&b));
        let slots: Vec<_> = a.iter().map(|(id, _)| id.0).collect();
        assert_eq!(slots, [2, 7, 200]);
        // A move, a new side or a removal changes the hash.
        b.get_mut(EntityId(7)).unwrap().position.x.0 += 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().position.x.0 -= 1;
        b.get_mut(EntityId(2)).unwrap().side = Side::Alien;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(2)).unwrap().side = Side::Human;
        assert_eq!(hash(&a), hash(&b));
        // So do a turn and damage.
        b.get_mut(EntityId(7)).unwrap().heading = 1;
        assert_ne!(hash(&a), hash(&b));
        b.get_mut(EntityId(7)).unwrap().heading = 0;
        b.get_mut(EntityId(7)).unwrap().damage = Damage::Taken(1);
        assert_ne!(hash(&a), hash(&b));
        assert_eq!(a.remove(EntityId(2)).map(|e| e.side), Some(Side::Human));
        assert_eq!((a.len(), a.get(EntityId(2))), (2, None));
    }

    #[test]
    fn fixed_point_is_16_16() {
        assert_eq!(Fixed::from_int(3), Fixed(0x3_0000));
        assert_eq!(Fixed(0x3_8000).floor(), 3);
        assert_eq!(Fixed(-0x8000).floor(), -1);
        assert_eq!(Fixed::ONE, Fixed::from_int(1));
    }

    #[test]
    fn a_scenario_s_objects_and_orders_fill_the_world() {
        use xeno_formats::fixtures::synthetic_mission_usg;
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let world = World::from_scenario(&scenario);
        // A human base (slot 0), humanoid (2), relay (3) and scorpion (5); an alien
        // scout (7), at x 100 + slot, y 130.
        let slots: Vec<_> = world.iter().map(|(id, _)| id.0).collect();
        assert_eq!(slots, [0, 2, 3, 5, 7]);
        let scout = world.get(EntityId(7)).unwrap();
        assert_eq!((scout.kind, scout.side), (ObjectKind::Scout, Side::Alien));
        assert_eq!(scout.position.x, Fixed::from_int(107));
        assert_eq!(scout.position.y, Fixed::from_int(130));
        // Headings, radii and damage come from each kind's layout.
        assert_eq!(scout.heading, 0x7000);
        assert_eq!(scout.radius, scenario.objects.as_ref().unwrap()[4].radius);
        let Damage::Robot(parts) = scout.damage else {
            panic!("{:?}", scout.damage)
        };
        assert_eq!((parts.helm, parts.power), (90, 80));
        assert_eq!(world.get(EntityId(0)).unwrap().damage, Damage::Taken(20));
        assert_eq!(world.get(EntityId(3)).unwrap().damage, Damage::Taken(3));
        // The alert damages the player's base and nothing else.
        let mut alerted = world.clone();
        assert!(alerted.apply_alert(50));
        assert_eq!(alerted.get(EntityId(0)).unwrap().damage, Damage::Taken(70));
        alerted.insert(EntityId(0), world.get(EntityId(0)).unwrap().clone());
        assert_eq!(alerted, world);
        assert!(!World::default().apply_alert(50));
        let human = Census {
            humanoids: 1,
            scorpions: 1,
            bases: 1,
            relays: 1,
            ..Census::default()
        };
        assert_eq!(world.census(Side::Human), human);
        assert_eq!(world.census(Side::Alien).units(), 1);
        assert_eq!(world.census(Side::Human).units(), 2);
        // The robots' orders come with them, and only the robots have orders.
        assert_eq!(world.orders, OrderBook::from_scenario(&scenario));
        assert_eq!(world.orders.side(Side::Human).count(), 2);
        // So do the terrain and the paths, both in the hash.
        assert_eq!(world.terrain, Terrain::from_scenario(&scenario));
        assert_eq!(world.paths, Paths::from_scenario(&scenario));
        let mut moved = world.clone();
        moved.terrain.features[0].height += 1;
        assert_ne!(hash(&moved), hash(&world));
        let mut moved = world.clone();
        moved.paths.alien[0].points[0].x.0 += 1;
        assert_ne!(hash(&moved), hash(&world));
        // Fractions are kept: the plain fixture's humanoid is at (95.5, 136.25).
        let plain = xeno_formats::usg::parse(&xeno_formats::fixtures::synthetic_usg()).unwrap();
        let world = World::from_scenario(&plain);
        let at = world.get(EntityId(0)).unwrap().position;
        assert_eq!((at.x, at.y), (Fixed(0x5f_8000), Fixed(0x88_4000)));
        // No objects chunk: an empty world.
        let empty = Scenario {
            objects: None,
            ..plain
        };
        assert!(World::from_scenario(&empty).is_empty());
    }

    /// Every stock scenario loads every object, and each side's units, bases and the
    /// rest are what the parser reports, within the original's limits
    /// (`docs/formats/usg.md`, `docs/screens.md`).
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_scenario_loads_with_its_units() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let names = (0..13)
            .map(|n| format!("scen{n:02}a.usg"))
            .chain(["ctrain", "htrain", "ntrain", "strain"].map(|n| format!("{n}.usg")));
        for name in names {
            let file = std::fs::read(dir.join(&name)).unwrap();
            let scenario = xeno_formats::usg::parse(&file).unwrap();
            let objects = scenario.objects.as_deref().unwrap_or_default();
            let world = World::from_scenario(&scenario);
            assert_eq!(
                world.len(),
                objects.len(),
                "{name}: every object, one a slot"
            );
            let (mut units, mut big, mut small) = (0, 0, 0);
            for side in [Side::Human, Side::Alien] {
                // The parser's own count of each kind on this side.
                let parsed = |kind: ObjectKind| {
                    objects
                        .iter()
                        .filter(|o| o.kind == kind && o.side() == side)
                        .count()
                };
                let census = world.census(side);
                let expected = Census {
                    humanoids: parsed(ObjectKind::Humanoid),
                    scorpions: parsed(ObjectKind::Scorpion),
                    scouts: parsed(ObjectKind::Scout),
                    bases: parsed(ObjectKind::Base),
                    relays: parsed(ObjectKind::Relay),
                    obstacles: parsed(ObjectKind::Obstacle),
                    other: 0,
                };
                assert_eq!(census, expected, "{name} {side:?}");
                // One base a side; every unit has orders; at most 38 units a side.
                assert_eq!(census.bases, 1, "{name} {side:?}");
                assert_eq!(world.orders.side(side).count(), census.units(), "{name}");
                assert!(census.units() <= 38, "{name} {side:?}");
                units += census.units();
                big += census.units() + census.bases;
                small += census.relays + census.obstacles;
            }
            // No object starts inside a hill, and every path in use has points.
            for (_, e) in world.iter() {
                assert!(world.terrain.hill_at(e.position).is_none(), "{name}");
            }
            for side in [Side::Human, Side::Alien] {
                assert!(world.paths.side(side).iter().all(|p| !p.points.is_empty()));
            }
            // The 0x172-byte pool holds 40 objects, the 0x34-byte one 200.
            assert!(big <= 40 && small <= 200, "{name}");
            assert!(units > 0, "{name}: no units");
        }
    }
}
