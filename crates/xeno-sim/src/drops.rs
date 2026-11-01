//! Keys 1 to 4 on the scorpion and scout (and their consoles' drop buttons): a mine, an
//! ECM buoy, an alarm buoy or an EMI decoy dropped where the robot stands, and those
//! objects' ticks (`docs/sim.md`, "Mines and buoys (T-137)").

use xeno_formats::usg;

use crate::damage::{alloc_small, spawn_explosion};
use crate::motion::{Event, Tables};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::world::{Damage, Entity, EntityId, Fixed, Position, State, World};

/// Each key's routine, by key (0 for key 1).
const MINE: usize = 0;
const ECM_BUOY: usize = 1;
const ALARM_BUOY: usize = 2;
const DECOY: usize = 3;

/// A dropped object's bytes +0x18 to +0x1b, and an alarm buoy's +0x16 bit 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Drop {
    /// +0x18 bit 0: the dropper was on its battery. Nothing reads it.
    pub battery: u8,
    /// +0x19: the ticks, a 256-tick wrap each look; a decoy's model (+0x19 a word,
    /// so `level` is its high byte, 0).
    pub ticks: u8,
    /// +0x1a: a buoy's level, set by an alert and halved every look; the next alert
    /// waits for 0.
    pub level: u8,
    /// +0x1b: an ECM buoy's alerts left.
    pub charges: u8,
    /// +0x16 bit 4: an alarm buoy shows its alarm (on the map).
    pub alarm: bool,
}

/// Key `key + 1` (`1000:d5e7`, `d609`, `d62b`, `d64d`): with one left of its count, an
/// object of its kind set up where robot `id` stands, a message, and one fewer; with
/// none, the "none left" message. With no slot free, the same message but the mine's
/// key says nothing. No kind, energy, damage or field is checked.
pub fn drop(world: &mut World, id: EntityId, key: u8, tables: &Tables, random: &mut Random) {
    let d = &tables.weapons.drops;
    let k = usize::from(key);
    let Some(e) = world.get(id) else { return };
    let (State::Robot(m), Damage::Robot(c)) = (&e.state, &e.damage) else {
        return;
    };
    let count = usize::from(d.counts[k] - 0xe2);
    if m.guns.drops[count] == 0 {
        crate::log::say(world, id, d.none_left[k], tables);
        return;
    }
    let Some(slot) = alloc_small(world) else {
        if k != MINE {
            crate::log::say(world, id, d.none_left[k], tables);
        }
        return;
    };
    // `1000:e801`: a random word's quarter (signed) off x, then another off y; z
    // and the side the robot's.
    let mut offset = |at: Fixed| Fixed(at.0.wrapping_add(i32::from(random.word() as i16 >> 2)));
    let x = offset(e.position.x);
    let y = offset(e.position.y);
    let model = |kind: usg::ObjectKind| d.models[usize::from(u16::from(kind)) % d.models.len()];
    let object = Entity {
        kind: d.kinds[k].into(),
        side: e.side,
        position: Position { x, y },
        z: e.z,
        model_height: 0,
        heading: 0,
        radius: 0,
        damage: Damage::None,
        state: State::Drop(Drop {
            battery: c.equipment & usg::Robot::ON_BATTERY,
            ticks: if k == DECOY { model(e.kind) } else { 0 },
            charges: if k == ECM_BUOY { d.ecm_charges } else { 0 },
            ..Drop::default()
        }),
    };
    world.insert(slot, object);
    crate::log::say(world, id, d.dropped[k], tables);
    if let Some(m) = crate::motion::movement(world, id) {
        m.guns.drops[count] -= 1;
    }
}

/// `1000:e84f`: whether a robot of the other side is within `range` (8.8) of `at`.
/// Cloaked, gone or not, in its base or not: only the unit flag and the side count.
fn enemy_near(world: &World, at: Position, side: usg::Side, range: u16) -> bool {
    world.iter().any(|(_, e)| {
        RobotKind::of(e.kind).is_some()
            && e.side != side
            && crate::sense::within(at, e.position, range)
    })
}

/// Dropped object `id`'s tick (`DS:0xe6e8`: `1000:e5b5` mine, `e64b` ECM buoy, `e60c`
/// alarm buoy; a decoy has none). Every 256 ticks it looks for an enemy: a mine goes
/// off, a buoy sends its alert when its level is back to 0. Returns whether it is
/// gone.
pub fn tick(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) -> bool {
    let d = &tables.weapons.drops;
    let Some(e) = world.get_mut(id) else {
        return false;
    };
    let State::Drop(s) = &mut e.state else {
        return false;
    };
    let Some(k) = d
        .kinds
        .iter()
        .position(|&k| usg::ObjectKind::from(k) == e.kind)
    else {
        return false;
    };
    if k == DECOY {
        return false;
    }
    s.ticks = s.ticks.wrapping_add(1);
    if s.ticks != 0 {
        return false;
    }
    let (at, z, side) = (e.position, e.z, e.side);
    if k != MINE {
        s.level >>= 1;
        if k == ALARM_BUOY {
            s.alarm = s.level != 0;
        }
    }
    let level = s.level;
    let range = [d.mine_range, d.ecm_range, d.alarm_range][k];
    if !enemy_near(world, at, side, range) {
        return false;
    }
    match k {
        MINE => {
            crate::log::alert(world, id, d.mine_alert, tables);
            spawn_explosion(world, at, z, tables.damage.explosion_kind, d.mine_explosion);
            events.push(Event::Sound {
                event: d.mine_sound,
                only_for: None,
            });
            let blast = (d.mine_points, d.mine_radius);
            crate::shot::blast(world, at, z.0, blast, tables, random, events);
            true
        }
        _ if level != 0 => false,
        ALARM_BUOY => {
            set_level(world, id, d.alarm_level);
            crate::log::alert(world, id, d.alarm_alert, tables);
            false
        }
        _ => {
            set_level(world, id, d.ecm_level);
            crate::log::alert(world, id, d.ecm_alert, tables);
            match world.get_mut(id).map(|e| &mut e.state) {
                Some(State::Drop(s)) => {
                    s.charges = s.charges.wrapping_sub(1);
                    s.charges == 0
                }
                _ => false,
            }
        }
    }
}

fn set_level(world: &mut World, id: EntityId, level: u8) {
    if let Some(State::Drop(s)) = world.get_mut(id).map(|e| &mut e.state) {
        s.level = level;
    }
}

#[cfg(test)]
mod tests;
