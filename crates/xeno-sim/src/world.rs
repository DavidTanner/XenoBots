//! The world: the mission's objects by their slot in the original's object table.
//!
//! The shape follows a `.usg` file's `DCBS` objects (`docs/formats/usg.md`): a slot, a
//! kind, a side and a 16.16 position, so loading one (T-051) is a field-for-field copy.
//! There is no map grid and no world edge: the files have no grid, and the world's
//! size and edges are open until T-059, so positions are not limited here.

use std::collections::BTreeMap;

use xeno_formats::usg::{ObjectKind, Side};

use crate::hash::{StateHash, StateHasher};
use crate::orders::OrderBook;

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
}

/// A side as a hashed byte.
pub(crate) fn side_code(side: Side) -> u8 {
    match side {
        Side::Human => 0,
        Side::Alien => 1,
    }
}

/// Every object, in slot order, and the robots' orders. A `BTreeMap`, so iteration
/// (and so the hash and any rule that walks the objects) never depends on insertion
/// order or a hasher's seed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct World {
    entities: BTreeMap<EntityId, Entity>,
    pub orders: OrderBook,
}

impl World {
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
        }
        self.orders.state_hash(h);
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
}
