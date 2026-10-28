//! The scenario editor's placing (`ui_editor_place_at_pointer`, `1000:6be8`;
//! `docs/screens.md`, "The scenario editor (T-048)"): a Ctrl+left press on the
//! situation room's map makes or moves an object at the point under the pointer.
//! [`Sim::place`] does it to the world, and hands back the object as the scenario
//! writer would save it, for the host's copy of the scenario.

use xeno_formats::usg::{Body, Object, ObjectKind, Side, flag};

use crate::terrain::Paths;
use crate::{Damage, Entity, EntityId, Position, Sim, State};

/// What a press places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Kind 16: a new obstacle (`1000:6c33`).
    Obstacle,
    /// Kind 3: the side's base moves there; no base is made (`1000:6c49`).
    Base(Side),
}

/// What [`Sim::place`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placed {
    /// A new object, as the scenario writer saves it.
    Made(Box<Object>),
    /// The object in that slot moved to the point.
    Moved(EntityId),
}

/// An obstacle's radius when made (`1000:e9c3`), 8.8.
pub const OBSTACLE_RADIUS: u16 = 0x0d00;

impl Sim {
    /// Places `what` at `at`. `None` when nothing could be placed: no free slot, or
    /// no base for the side.
    pub fn place(&mut self, what: Place, at: Position) -> Option<Placed> {
        match what {
            Place::Base(side) => {
                let id = self.world.base(side)?;
                self.world.get_mut(id)?.position = at;
                Some(Placed::Moved(id))
            }
            Place::Obstacle => {
                // `sim_alloc_object_slot` (`1000:f4b1`): the lowest free slot, the
                // object's bytes from +4 cleared; `1000:e9c3`: radius and `IN_FIELD`.
                let slot =
                    (0..Object::SLOTS as u16).find(|&s| self.world.get(EntityId(s)).is_none())?;
                // shortcut: the pool's lowest free entry is taken as the count of the
                // small pool's objects (true while none has been freed); only the save
                // shows it. Upgrade if the sim keeps the pools.
                let pool_index = self.world.iter().filter(|(_, e)| !big(e.kind)).count() as u16;
                self.world.insert(
                    EntityId(slot),
                    Entity {
                        kind: ObjectKind::Obstacle,
                        side: Side::Human,
                        position: at,
                        heading: 0,
                        radius: OBSTACLE_RADIUS,
                        damage: Damage::None,
                        state: State::None,
                    },
                );
                Some(Placed::Made(Box::new(Object {
                    size: Object::SMALL,
                    slot,
                    // The object table's reference count, 1 from the allocation.
                    ref_count: 1,
                    kind: ObjectKind::Obstacle,
                    pool_index,
                    x: at.x.0,
                    y: at.y.0,
                    z: 0,
                    heading: 0,
                    model_height: 0,
                    radius: OBSTACLE_RADIUS,
                    flags: flag::IN_FIELD,
                    status: 0,
                    body: Body::Other(vec![0; usize::from(Object::SMALL) - Object::HEADER_SIZE]),
                })))
            }
        }
    }
}

impl Sim {
    /// The editor's path keys and points change a side's table in place
    /// (`DS:0x5cd1`); the host hands over the edited tables.
    pub fn set_paths(&mut self, paths: Paths) {
        self.world.paths = paths;
    }
}

/// Whether `kind`'s objects come from the big pool (`DS:0xe7c7` bit 0): the robots
/// and bases.
fn big(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Humanoid | ObjectKind::Scorpion | ObjectKind::Scout | ObjectKind::Base
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fixed, Random, motion};
    use xeno_formats::fixtures::synthetic_mission_usg;
    use xeno_formats::usg;

    fn at(x: i16, y: i16) -> Position {
        Position {
            x: Fixed::from_int(x),
            y: Fixed::from_int(y),
        }
    }

    #[test]
    fn an_obstacle_takes_the_lowest_free_slot_and_a_base_moves() {
        let scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        let mut sim = Sim::load(&scenario, Random::seeded(1), motion::synthetic_tables());
        let before = sim.state_hash();
        // Slots 0, 2, 3, 5 and 7 are taken: the obstacle goes in 1.
        let Some(Placed::Made(o)) = sim.place(Place::Obstacle, at(40, -3)) else {
            panic!()
        };
        assert_eq!((o.slot, o.pool_index, o.x, o.y), (1, 1, 40 << 16, -3 << 16));
        assert_ne!(sim.state_hash(), before);
        let e = sim.world.get(EntityId(1)).unwrap();
        assert_eq!((e.kind, e.radius, e.position), (o.kind, 0xd00, at(40, -3)));
        // The written object reads back as itself.
        let mut back = scenario.clone();
        back.objects.as_mut().unwrap().push(*o.clone());
        let read = usg::parse(&usg::write(&back).unwrap()).unwrap();
        assert!(read.objects.unwrap().contains(&*o));
        // The next one goes in 4.
        let Some(Placed::Made(o)) = sim.place(Place::Obstacle, at(0, 0)) else {
            panic!()
        };
        assert_eq!((o.slot, o.pool_index), (4, 2));
        // The human base (slot 0) moves; the alien side has none.
        assert_eq!(
            sim.place(Place::Base(Side::Human), at(7, 8)),
            Some(Placed::Moved(EntityId(0)))
        );
        assert_eq!(sim.world.get(EntityId(0)).unwrap().position, at(7, 8));
        assert_eq!(sim.place(Place::Base(Side::Alien), at(7, 8)), None);
    }
}
