//! The running world written back into a scenario's chunks, as `file_write_save_game`
//! (`1000:f01b`) writes it from memory (`docs/sim.md`, "Saving the world (T-116)"):
//! every object in slot order, after `scen_pack_all_objects` (`1000:f0f4`) puts each
//! robot's script pointer and target in their saved form; both path tables; and the
//! game clock. [`World::save`] packs; [`unpack_made`] reads back the kinds made during
//! play that the sim keeps (shots and explosions) when a save is loaded.

use std::collections::{BTreeMap, BTreeSet};

use xeno_formats::usg::{self, Body, Object, ObjectKind, Scenario, flag};

use crate::motion::Tables;
use crate::orders::RobotKind;
use crate::shot::{Flight, HOMING, MISSILE, SHELL, Shot, TAIL};
use crate::terrain::PathTable;
use crate::world::{Damage, Entity, EntityId, Movement, State, World, side_code};

/// A saved script pointer's mark (`1000:4826`): its offset from the program table with
/// the top bits 0xa0, which the loader takes as "restore, then restart".
pub(crate) const SAVED_SCRIPT: u16 = 0xa000;
/// A saved "none" for a slot reference.
const NONE: u16 = 0xffff;

/// A `DCBS` record being patched: offsets are the object's, after the 6-byte header.
struct Record(Vec<u8>);

impl Record {
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[6 + at..6 + at + bytes.len()].copy_from_slice(bytes);
    }
    fn u8(&mut self, at: usize, v: u8) {
        self.put(at, &[v]);
    }
    fn u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn i32(&mut self, at: usize, v: i32) {
        self.put(at, &v.to_le_bytes());
    }
    fn get(&self, at: usize) -> u8 {
        self.0[6 + at]
    }
}

fn slot(id: Option<EntityId>) -> u16 {
    id.map_or(NONE, |id| id.0)
}

/// Robots and bases come from the big pool (`DS:0xe7c7` bit 0).
fn big(kind: ObjectKind) -> bool {
    RobotKind::of(kind).is_some() || kind == ObjectKind::Base
}

impl World {
    /// `loaded` (the scenario as the host keeps it: its terrain, briefing and sky, and
    /// each object's bytes as loaded) with the world written over it: the clock's hour
    /// to tick in `SHDR` (the day and month stay `loaded`'s: the campaign's calendar),
    /// every object in the world, each over its loaded bytes when its slot held the
    /// same kind, and both path tables. `program_table` is the address saved script
    /// pointers count from (`xeno_formats::editor::Editor::program_table`).
    ///
    /// Bytes the sim does not keep stay as loaded; an object made during play starts
    /// from zeros, its reference count 1 and the lowest pool index free in its pool.
    // shortcut: the reference counts are the loaded ones (stock files hold 0, 1 and
    // junk; the port reads none), not the live counts with the pack's one a target (T-154).
    // shortcut: relays, shots and explosions made in play keep only what the sim keeps
    // (no relay set-up bytes or flags beyond the side; T-148).
    pub fn save(&self, loaded: &Scenario, program_table: u16) -> Scenario {
        let w = self;
        let mut s = loaded.clone();
        let (start, c) = (&mut s.header.start, w.clock);
        (start.hour, start.minute, start.second, start.tick) = (c.hour, c.minute, c.second, c.tick);

        let templates: BTreeMap<u16, &Object> = loaded
            .objects
            .iter()
            .flatten()
            .map(|o| (o.slot, o))
            .collect();
        let template =
            |id: EntityId, e: &Entity| templates.get(&id.0).copied().filter(|o| o.kind == e.kind);
        let mut pools: BTreeSet<(bool, u16)> = w
            .iter()
            .filter_map(|(id, e)| template(id, e).map(|o| (big(e.kind), o.pool_index)))
            .collect();
        let objects = w
            .iter()
            .map(|(id, e)| {
                let mut r = match template(id, e) {
                    Some(o) => Record(o.to_record()),
                    None => {
                        let size = if big(e.kind) {
                            Object::BIG
                        } else {
                            Object::SMALL
                        };
                        let index = (0..)
                            .find(|&i| pools.insert((big(e.kind), i)))
                            .expect("a free pool index");
                        let mut b = [size, id.0, 1].map(u16::to_le_bytes).concat();
                        b.resize(6 + usize::from(size), 0);
                        let mut r = Record(b);
                        r.u16(2, index);
                        r
                    }
                };
                pack(&mut r, w, id, e, program_table);
                Object::from_record(&r.0).expect("a packed object reads back")
            })
            .collect();
        s.objects = Some(objects);
        s.human_paths = paths(&w.paths.human, loaded.human_paths.as_ref());
        s.alien_paths = paths(&w.paths.alien, loaded.alien_paths.as_ref());
        s
    }
}

/// One side's table over the loaded one: the count, and each slot's mission, weight
/// and points in use; the editor's mix, the counts of robots on it and leftover points
/// stay as loaded.
fn paths(table: &PathTable, loaded: Option<&usg::PathTable>) -> Option<usg::PathTable> {
    if loaded.is_none() && table.slots.is_empty() {
        return None;
    }
    let mut t = loaded.cloned().unwrap_or_else(usg::PathTable::empty);
    t.count = table.count as u16;
    for (p, path) in t.paths.iter_mut().zip(&table.slots) {
        (p.mission, p.weight) = (path.mission, path.weight);
        p.point_count = path.points.len() as u8;
        for (q, at) in p.points.iter_mut().zip(&path.points) {
            *q = usg::Point {
                x: at.x.0,
                y: at.y.0,
            };
        }
    }
    Some(t)
}

/// Writes entity `id` over its record.
fn pack(r: &mut Record, world: &World, id: EntityId, e: &Entity, program_table: u16) {
    r.u16(0, e.kind.into());
    r.i32(4, e.position.x.0);
    r.i32(8, e.position.y.0);
    r.i32(0x0c, e.z.0);
    r.u16(0x10, e.heading);
    r.u16(0x12, e.model_height);
    r.u16(0x14, e.radius);
    let side = side_code(e.side) * flag::ALIEN;
    r.u8(0x16, (r.get(0x16) & !flag::ALIEN) | side);
    match e.damage {
        Damage::Robot(c) => {
            for (at, v) in [
                (0x24, c.helm),
                (0x4e, c.power),
                (0xdc, c.weapons),
                (0xec, c.targeting),
                (0xf6, c.systems),
                (0xfd, c.equipment),
                (0xf9, c.total),
            ] {
                r.u8(at, v);
            }
            // The take-in and the launch move `IN_FIELD` with `IN_BASE` (T-115).
            let field = if c.equipment & usg::Robot::IN_BASE == 0 {
                flag::IN_FIELD
            } else {
                0
            };
            r.u8(0x16, (r.get(0x16) & !flag::IN_FIELD) | field);
        }
        Damage::Taken(n) if e.kind == ObjectKind::Base => r.u16(0x2e, n),
        Damage::Taken(n) => r.u8(0x33, n as u8),
        Damage::None => {}
    }
    match &e.state {
        State::Robot(m) => robot(r, world, id, e.kind, m, program_table),
        State::Base {
            tick,
            kills,
            losses,
            destroyed,
            end_step,
            look,
            counts,
            depot: d,
        } => {
            r.u8(
                0x16,
                r.get(0x16) & !flag::GONE | (u8::from(*destroyed) * flag::GONE),
            );
            r.put(0x18, &[*look, d.relay_clock]);
            r.u16(0x1a, slot(d.repairing));
            r.u16(0x1c, d.repair_clock);
            r.u8(0x20, d.relays);
            r.put(0x21, counts);
            r.put(0x2b, &[*losses, *kills, d.slider]);
            r.u16(0x30, *end_step);
            r.put(0x38, &d.power);
            r.u8(0x41, *tick);
            r.u16(0xb5, d.relay_wait);
        }
        State::Explosion {
            frame,
            last,
            ticks,
            countdown,
        } => {
            r.u8(0x18, *frame);
            r.put(0x1a, &[*last, *ticks, *countdown]);
        }
        State::Shot(s) => {
            for &(at, f) in layout(s.flight) {
                match f {
                    Field::Damage | Field::Phase | Field::Frame => r.u8(at, f.get(s) as u8),
                    _ => r.u16(at, f.get(s)),
                }
            }
        }
        State::None => {}
    }
}

/// A robot's movement, orders, script and parts (`usg::Robot`, `usg::Parts`).
fn robot(
    r: &mut Record,
    world: &World,
    id: EntityId,
    kind: ObjectKind,
    m: &Movement,
    program_table: u16,
) {
    r.u8(0x17, m.status);
    r.u8(0x1a, m.tick);
    r.u16(0x1c, m.eye_height);
    r.u16(0x1e, m.velocity.0 as u16);
    r.u16(0x20, m.velocity.1 as u16);
    let mut mode = m.mode;
    if let Some(o) = world.orders.get(id.0) {
        mode |= u8::from(o.orders.program_drives) * usg::Robot::PROGRAM;
        r.u8(0x2e, o.orders.ordered);
        r.u8(0x30, o.orders.program);
    }
    r.u8(0x23, mode);
    r.u16(0x26, m.speed as u16);
    r.u16(0x28, m.target_speed as u16);
    r.u16(0x2a, m.target_heading);
    r.put(0x2c, &[m.path, m.path_point]);
    let s = &m.script;
    r.u8(0x31, s.sp);
    r.put(0x32, &[m.goal, s.flags]);
    // `1000:4826`: the pointer from the program table, marked.
    r.u16(0x34, s.pointer.wrapping_sub(program_table) | SAVED_SCRIPT);
    r.u16(0x36, s.events);
    r.u16(0x38, s.wait);
    r.u16(0x3a, m.goal_object);
    r.i32(0x3c, m.destination.x.0);
    r.i32(0x40, m.destination.y.0);
    r.put(0x44, &s.stack);
    r.put(
        0x50,
        &[m.guns.energy, m.drive, m.panel.flags, m.panel.length],
    );
    for (i, q) in m.panel.queue.iter().enumerate() {
        r.u16(0x54 + 2 * i, *q);
    }
    r.put(0x5e, &m.panel.text);
    r.u8(0xdd, m.weapon);
    // `1000:f656`: the target's pointer to its slot (the pack also counts a reference).
    r.u16(0xe6, slot(m.target));
    r.u16(0xe8, m.target_distance);
    r.u16(0xea, m.target_bearing);
    r.u8(0xed, m.locked_on);
    r.u8(0xf7, m.contacts);
    let g = &m.guns;
    match RobotKind::of(kind) {
        Some(RobotKind::Humanoid) => {
            r.put(
                0xff,
                &[
                    g.arms[0],
                    g.arms[1],
                    m.legs[2],
                    m.legs[3],
                    g.raised[0],
                    g.raised[1],
                ],
            );
            r.u8(0x105, m.stride);
            r.put(0x108, &[g.cooldown[0] as u8, g.cooldown[1] as u8]);
            r.u8(0x10b, m.gear);
        }
        Some(RobotKind::Scorpion) => {
            let n = &m.grid;
            for (i, v) in [n.cell.0 as u16, n.cell.1 as u16, n.lengths.0, n.lengths.1]
                .into_iter()
                .chain([n.error as u16])
                .enumerate()
            {
                r.u16(0x10a + 2 * i, v);
            }
            r.u8(0x114, n.step);
            r.put(0xff, &m.legs);
            r.u8(0x116, m.stride);
            r.u8(0x118, m.relays);
            r.put(0x11d, &[g.fired, g.burst, g.cooldown[0] as u8]);
            r.u16(0x120, g.rounds);
            r.u8(0x122, g.missiles);
            r.u16(0x123, slot(g.missile));
            r.u16(0x125, g.cooldown[1]);
        }
        Some(RobotKind::Scout) => {
            let c = g.cooldown.map(|c| c as u8);
            r.put(0xff, &m.legs);
            r.put(0x105, &[c[0], c[1], g.rounds as u8]);
            r.put(0x109, &[m.gear, m.stride, m.stance, m.stance_target]);
        }
        None => {}
    }
}

/// A shot's fields as the object keeps them (`docs/formats/usg.md`, "Kinds made during
/// play"). The flight is the kind; bytes, not words: the damage, phase and frame.
#[derive(Debug, Clone, Copy)]
enum Field {
    Speed,
    Climb,
    Vx,
    Vy,
    Vz,
    Arming,
    Age,
    Life,
    Turn,
    Target,
    Damage,
    Phase,
    Frame,
}

impl Field {
    fn get(self, s: &Shot) -> u16 {
        match self {
            Field::Speed => s.speed,
            Field::Climb => s.climb,
            Field::Vx => s.velocity[0] as u16,
            Field::Vy => s.velocity[1] as u16,
            Field::Vz => s.velocity[2] as u16,
            Field::Arming => s.arming,
            Field::Age => s.age,
            Field::Life => s.life,
            Field::Turn => s.turn,
            Field::Target => slot(s.target),
            Field::Damage => s.damage.into(),
            Field::Phase => s.phase.into(),
            Field::Frame => s.frame.into(),
        }
    }

    fn set(self, s: &mut Shot, v: u16) {
        match self {
            Field::Speed => s.speed = v,
            Field::Climb => s.climb = v,
            Field::Vx => s.velocity[0] = v as i16,
            Field::Vy => s.velocity[1] = v as i16,
            Field::Vz => s.velocity[2] = v as i16,
            Field::Arming => s.arming = v,
            Field::Age => s.age = v,
            Field::Life => s.life = v,
            Field::Turn => s.turn = v,
            Field::Target => s.target = (v != NONE).then_some(EntityId(v)),
            Field::Damage => s.damage = v as u8,
            Field::Phase => s.phase = v as u8,
            Field::Frame => s.frame = v as u8,
        }
    }
}

/// Each kind's fields by offset (the set-ups at `1000:e08e`, `e235`, `e34a`, `e48a`).
fn layout(flight: Flight) -> &'static [(usize, Field)] {
    use Field::*;
    match flight {
        Flight::Tail => &[
            (0x19, Target),
            (0x1b, Speed),
            (0x1d, Climb),
            (0x1f, Vx),
            (0x21, Vy),
            (0x23, Vz),
            (0x25, Age),
            (0x27, Turn),
            (0x29, Phase),
        ],
        Flight::Shell => &[
            (0x18, Speed),
            (0x1a, Vx),
            (0x1c, Vy),
            (0x1e, Vz),
            (0x20, Arming),
            (0x22, Damage),
        ],
        Flight::Homing => &[
            (0x19, Target),
            (0x1b, Speed),
            (0x1d, Climb),
            (0x1f, Vx),
            (0x21, Vy),
            (0x23, Vz),
            (0x25, Arming),
            (0x27, Age),
            (0x29, Damage),
        ],
        Flight::Missile => &[
            (0x18, Frame),
            (0x1b, Speed),
            (0x1d, Vx),
            (0x1f, Vy),
            (0x21, Vz),
            (0x23, Arming),
            (0x25, Life),
        ],
    }
}

/// The objects of `scenario` made during play that the sim keeps, read back after
/// [`World::from_scenario`]: shots (kinds 5 to 8) and explosions. A missile's damage is
/// the kind's (`1000:e587`), not the object's.
pub(crate) fn unpack_made(world: &mut World, scenario: &Scenario, tables: &Tables) {
    for o in scenario.objects.iter().flatten() {
        let Body::Other(b) = &o.body else { continue };
        if o.size != Object::SMALL {
            continue;
        }
        let at = |i: usize| b[i - Object::HEADER_SIZE];
        let word = |i: usize| u16::from_le_bytes([at(i), at(i + 1)]);
        let kind = u16::from(o.kind);
        let state = if kind == tables.damage.explosion_kind {
            State::Explosion {
                frame: at(0x18),
                last: at(0x1a),
                ticks: at(0x1b),
                countdown: at(0x1c),
            }
        } else {
            let flight = match kind {
                TAIL => Flight::Tail,
                SHELL => Flight::Shell,
                HOMING => Flight::Homing,
                MISSILE => Flight::Missile,
                _ => continue,
            };
            let mut s = Shot {
                flight,
                velocity: [0; 3],
                speed: 0,
                climb: 0,
                age: 0,
                arming: 0,
                life: 0,
                damage: 0,
                target: None,
                turn: 0,
                phase: 0,
                frame: 0,
            };
            for &(i, f) in layout(flight) {
                let v = match f {
                    Field::Damage | Field::Phase | Field::Frame => at(i).into(),
                    _ => word(i),
                };
                f.set(&mut s, v);
            }
            if flight == Flight::Missile {
                s.damage = tables.weapons.missile.damage;
            }
            State::Shot(s)
        };
        if let Some(e) = world.get_mut(EntityId(o.slot)) {
            e.state = state;
        }
    }
}

#[cfg(test)]
mod tests;
