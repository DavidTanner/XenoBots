//! How robots move and walk paths, as the original does it each tick (`docs/sim.md`,
//! "Movement"): steering, speed steps, the velocity from the sine table, the move, the
//! hill bounce, and the alien base's path walking. The tables come from the user's
//! `xeno.exe` ([`Tables`], `xeno_formats::motion`).
//!
//! The scripts (`crate::script`, T-053) decide where robots go: they run between the
//! speed step and the move, and call [`head_for_path_point`] and
//! [`join_nearest_path_point`] and set a robot's target heading and speed.

use xeno_formats::usg::{ObjectKind, Side};

pub use xeno_formats::motion::Motion as Tables;

use crate::orders::{Change, RobotKind};
use crate::random::Random;
use crate::terrain::{PathTable, Terrain, approx_distance};
use crate::world::{Damage, EntityId, Movement, Position, State, World};

/// Something a tick did that the rest of the game (or a log) may want.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The robot ended its move inside a hill: its speed was negated and it takes a
    /// 0-point hit (`sim_damage_object`).
    HillHit(EntityId),
    /// The object was destroyed (`docs/sim.md`, "Damage"): a robot or relay is gone
    /// and an explosion stands where it was; a base stays, destroyed.
    Destroyed(EntityId),
    /// The alien base gave the robot path `slot` (`sim_assign_random_path`).
    PathTaken { robot: EntityId, slot: u8 },
    /// A path's mission started a program.
    Program(Change),
    /// A destroyed base's end script reached its last step: the mission is over, won
    /// if the base was the alien one (`sim_end_mission_with_result`).
    MissionOver { won: bool },
}

/// The sine of `angle` (65,536 to a turn) as `sim_sine_magnitude` works it out: the
/// magnitude in 0.16, whether it is 1.0 exactly (the magnitude is then 0xffff), and
/// whether it is negative.
pub fn sine(table: &[u16], angle: u16) -> (u16, bool, bool) {
    let negative = angle & 0x8000 != 0;
    let mut a = angle & 0x7fff;
    if a == 0x4000 {
        return (0xffff, true, negative);
    }
    if a > 0x4000 {
        a = 0x8000 - a;
    }
    let i = usize::from(a >> 6);
    let fraction = ((a & 0x3f) << 2) as u8;
    let at = |i: usize| table.get(i).copied().unwrap_or(0);
    let (low, difference) = (at(i), at(i + 1).wrapping_sub(at(i)));
    let mut step = (u16::from(difference as u8) * u16::from(fraction) + 0x80) >> 8;
    if difference >> 8 == 1 {
        step += u16::from(fraction);
    }
    match low.checked_add(step) {
        Some(m) => (m, false, negative),
        None => (0xffff, true, negative),
    }
}

/// `speed` times the sine of `angle`, as `sim_scale_by_heading` does each component:
/// the high word of an unsigned multiply (the speed itself at 1.0), signed.
fn component(table: &[u16], angle: u16, speed: i16) -> i16 {
    let (magnitude, one, negative) = sine(table, angle);
    let size = speed.unsigned_abs();
    let mut v = if one {
        size
    } else {
        ((u32::from(magnitude) * u32::from(size)) >> 16) as u16
    };
    if negative {
        v = v.wrapping_neg();
    }
    if speed < 0 {
        v = v.wrapping_neg();
    }
    v as i16
}

/// The velocity for `heading` and `speed` (`sim_update_robot_velocity`): `speed *
/// sin(heading)` in x, `speed * cos(heading)` in y.
pub fn velocity(tables: &Tables, heading: u16, speed: i16) -> (i16, i16) {
    let x = component(&tables.sine, heading, speed);
    let y = component(&tables.sine, 0x4000u16.wrapping_sub(heading), speed);
    (x, y)
}

/// `heading` turned towards `target` by at most `step` (`sim_steer_*`).
pub fn steer(heading: u16, target: u16, step: u16) -> u16 {
    let difference = target.wrapping_sub(heading);
    let negative = difference & 0x8000 != 0;
    let size = if negative {
        difference.wrapping_neg()
    } else {
        difference
    };
    let turn = size.min(step);
    heading.wrapping_add(if negative { turn.wrapping_neg() } else { turn })
}

/// The speed a robot may have for `target` (`sim_limit_speed_by_drive_and_helm`):
/// none with no drive, half with a weak one, then a share by the helm.
pub fn limit_speed(target: i16, drive: u8, helm: u8, weak_drive: u8) -> i16 {
    if drive == 0 {
        return 0;
    }
    let speed = if drive <= weak_drive {
        target >> 1
    } else {
        target
    };
    // A jump table by `(helm >> 3) & 0xfe`; helms over 111 would read past it.
    match (helm >> 3) & 0xfe {
        0 => 0,
        2 | 4 => speed >> 2,
        6 => speed >> 1,
        8 | 10 => speed - (speed >> 2),
        _ => speed,
    }
}

/// `speed` through the scorpion's speed table `speed_table / 2`
/// (`sim_map_scorpion_speed`), the sign kept. Past the tables, or past their 64
/// entries, the original reads other memory; the port takes the last.
pub fn map_scorpion_speed(tables: &Tables, speed_table: u8, speed: i16) -> i16 {
    let Some(table) = tables
        .scorpion_speeds
        .get(usize::from(speed_table / 2))
        .or(tables.scorpion_speeds.last())
    else {
        return speed;
    };
    let size = usize::from(speed.unsigned_abs()).min(table.len() - 1);
    let mapped = i16::from(table[size]);
    if speed < 0 { -mapped } else { mapped }
}

fn kind_index(kind: ObjectKind) -> Option<usize> {
    RobotKind::of(kind).map(|k| k.index())
}

/// The turn keys, steering, the speed step and the velocity: `sim_move_*` up to the
/// script. The recall (+0x23 bit 3) is not run (T-105).
pub fn steer_and_speed(
    tables: &Tables,
    kind: ObjectKind,
    helm: u8,
    heading: &mut u16,
    m: &mut Movement,
) {
    let Some(k) = kind_index(kind) else {
        return;
    };
    // The pilot's turn keys.
    if m.mode & 0x10 != 0 {
        m.target_heading = m.target_heading.wrapping_sub(0x40);
    }
    if m.mode & 0x20 != 0 {
        m.target_heading = m.target_heading.wrapping_add(0x40);
    }
    let turned = steer(*heading, m.target_heading, tables.turn_step[k]);
    let mut changed = turned != *heading;
    *heading = turned;

    let limit = |m: &Movement| limit_speed(m.target_speed, m.drive, helm, tables.weak_drive);
    let wanted = match kind {
        ObjectKind::Scorpion if m.tick & 0xf != 0 => None,
        ObjectKind::Scorpion => Some(map_scorpion_speed(tables, m.speed_table, limit(m))),
        // A scout moves only standing: asked to move, it first stands up.
        ObjectKind::Scout if m.target_speed != 0 && m.stance != STANDING => {
            m.stance_target = STANDING;
            None
        }
        _ => Some(limit(m)),
    };
    if let Some(wanted) = wanted
        && wanted != m.speed
    {
        m.speed = if wanted > m.speed {
            m.speed.wrapping_add(1)
        } else {
            m.speed.wrapping_sub(1)
        };
        changed = true;
    }
    if kind == ObjectKind::Scout {
        if m.speed != 0 {
            (m.stance, m.stance_target) = (STANDING, STANDING);
        } else if m.stance < m.stance_target {
            m.stance += 1;
        } else if m.stance > m.stance_target {
            m.stance -= 1;
        }
    }
    if changed {
        m.velocity = velocity(tables, *heading, m.speed);
    }
}

/// The move, the hill bounce and the tick counter's step: `sim_move_*` after the
/// script. Returns whether the robot ended inside a hill.
pub fn travel(
    terrain: &Terrain,
    kind: ObjectKind,
    position: &mut Position,
    m: &mut Movement,
) -> bool {
    if kind_index(kind).is_none() {
        return false;
    }
    position.x.0 = position.x.0.wrapping_add(i32::from(m.velocity.0));
    position.y.0 = position.y.0.wrapping_add(i32::from(m.velocity.1));
    // Inside a hill: back off at the same speed. The velocity stays as it is until the
    // speed next steps.
    let hit = terrain.hill_at(*position).is_some();
    if hit {
        m.speed = m.speed.wrapping_neg();
        m.target_speed = 0;
    }

    m.tick = m.tick.wrapping_add(2);
    if kind != ObjectKind::Humanoid {
        m.tick &= 0x1e;
    }
    hit
}

/// A scout's stance when it stands, the only one it moves in.
pub const STANDING: u8 = 0x30;

/// Where robot `id` heads now: `at`, as a point (`sim_set_robot_destination`; an
/// object it was heading for is dropped).
pub fn set_destination(world: &mut World, id: EntityId, at: Position) {
    if let Some(m) = movement(world, id) {
        m.destination = at;
        m.goal = Movement::GOAL_POINT;
    }
}

pub(crate) fn movement(world: &mut World, id: EntityId) -> Option<&mut Movement> {
    match &mut world.get_mut(id)?.state {
        State::Robot(m) => Some(m),
        _ => None,
    }
}

/// A robot's kind, side and movement, copied.
pub(crate) fn robot(world: &World, id: EntityId) -> Option<(ObjectKind, Side, Movement)> {
    let e = world.get(id)?;
    match e.state {
        State::Robot(m) => Some((e.kind, e.side, m)),
        _ => None,
    }
}

pub(crate) fn base_position(world: &World, side: Side) -> Option<Position> {
    world
        .base(side)
        .and_then(|b| world.get(b))
        .map(|b| b.position)
}

/// Heads robot `id` for its path's point (`sim_head_for_path_point`): returns whether
/// it now heads somewhere. Point 0xff is its base; 0xfe and points past the path run
/// the return and end routines and fail.
pub fn head_for_path_point(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) -> bool {
    let Some((kind, side, m)) = robot(world, id) else {
        return false;
    };
    let slot = usize::from(m.path & 0xf);
    // The original keeps 8 slot pointers a side; slots 8 to 15 read past them.
    let points = match world.paths.table(side).slot(slot) {
        Some(p) if slot < 8 => p.points.clone(),
        _ => Vec::new(),
    };
    if points.is_empty() {
        return false;
    }
    let at = match m.path_point {
        0xff => base_position(world, side),
        0xfe => {
            // Humanoids and scouts take a new path; scorpions turn round.
            if kind == ObjectKind::Scorpion {
                turn_round(world, id);
            } else {
                assign_random_path(world, id, tables, random, events);
            }
            return false;
        }
        n => match points.get(usize::from(n)) {
            Some(&at) => Some(at),
            None => {
                // Every end routine turns the robot round.
                turn_round(world, id);
                return false;
            }
        },
    };
    let Some(at) = at else {
        return false;
    };
    set_destination(world, id, at);
    if let Some(m) = movement(world, id) {
        m.path = (m.path & !Movement::NEXT) | Movement::ON_PATH;
    }
    true
}

/// `sim_turn_round_on_path`: walk the other way and ask for the next point.
fn turn_round(world: &mut World, id: EntityId) {
    if let Some(m) = movement(world, id) {
        m.path = (m.path ^ Movement::BACKWARDS) | Movement::NEXT;
    }
}

/// The most tries [`advance_path_point`] makes. The original loops until one works,
/// forever on a path with no points; any real path needs at most three.
const MAX_TRIES: usize = 16;

/// Robot `id` heads for the next point of its path, forwards or backwards
/// (`sim_advance_path_point`), trying again from wherever a failed point leaves it.
pub fn advance_path_point(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    for _ in 0..MAX_TRIES {
        let Some(m) = movement(world, id) else {
            return;
        };
        let step: u8 = if m.path & Movement::BACKWARDS == 0 {
            1
        } else {
            0xff
        };
        m.path_point = m.path_point.wrapping_add(step);
        if head_for_path_point(world, id, tables, random, events) {
            return;
        }
        if let Some(m) = movement(world, id) {
            m.path_point = m.path_point.wrapping_sub(step);
        }
    }
}

/// The alien base gives robot `id` a random path in use (`sim_assign_random_path`):
/// each slot `weight + 1` times, picked by the random word's low byte. The robot heads
/// for point 0 and starts the mission's program. The alien table is used whatever the
/// robot's side.
pub fn assign_random_path(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let alien: &PathTable = &world.paths.alien;
    if alien.count == 0 {
        return;
    }
    let mut list = Vec::new();
    for slot in 0..alien.count {
        let weight = alien.slot(slot).map_or(0, |p| p.weight);
        list.extend(std::iter::repeat_n(slot as u8, usize::from(weight) + 1));
    }
    let mut pick = usize::from(random.word() as u8);
    while pick >= list.len() {
        pick -= list.len();
    }
    let slot = list[pick];
    if let Some(m) = movement(world, id) {
        (m.path, m.path_point) = (slot, 0);
    }
    events.push(Event::PathTaken { robot: id, slot });
    head_for_path_point(world, id, tables, random, events);
    start_mission_program(world, id, tables, events);
}

/// `order_start_path_mission_program`: the program the alien table's slot's mission
/// gives the robot's kind, and the program bit set. A mission past the table starts
/// nothing (the original would call through other memory).
fn start_mission_program(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    events: &mut Vec<Event>,
) {
    let Some((kind, _, m)) = robot(world, id) else {
        return;
    };
    let mission = world
        .paths
        .alien
        .slot(usize::from(m.path & 0xf))
        .map(|p| usize::from(p.mission));
    let program = mission
        .and_then(|mission| tables.mission_programs.get(mission))
        .zip(kind_index(kind))
        .map(|(programs, k)| programs[k]);
    if let Some(program) = program {
        crate::script::set_program(world, id, program, tables, events);
        crate::script::switch(world, id, true, events);
    }
}

/// Robot `id` heads for the nearest of its base and every point of its side's paths in
/// use within 256 units (`sim_join_nearest_path_point`, a script command): its path and
/// point become that slot and point with no flags (the base: slot 0, point 0).
pub fn join_nearest_path_point(world: &mut World, id: EntityId) {
    let Some(e) = world.get(id) else {
        return;
    };
    let (side, from) = (e.side, e.position);
    let Some(base) = base_position(world, side) else {
        return;
    };
    let used = world.paths.table(side).used();
    if used.is_empty() {
        set_destination(world, id, base);
        return;
    }
    // The distance as 8.8, counted only under 256 units.
    let near = |at: Position| {
        let d = approx_distance(from, at).0 as u32;
        (d >> 24 == 0).then_some((d >> 8) as u16)
    };
    let mut best = (near(base).unwrap_or(0xffff), 0u8, 0u8, base);
    for (slot, path) in used.iter().enumerate() {
        for (point, &at) in path.points.iter().enumerate() {
            if let Some(d) = near(at)
                && d < best.0
            {
                best = (d, slot as u8, point as u8, at);
            }
        }
    }
    if let Some(m) = movement(world, id) {
        (m.path, m.path_point) = (best.1, best.2);
    }
    set_destination(world, id, best.3);
}

/// The alien base's path phase (`sim_run_alien_base_path_phase`): robots in its unit
/// list with no path (and not +0x23 bit 0) get a random one, then those that asked for
/// their next point move on.
fn alien_path_phase(
    world: &mut World,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    if world.paths.alien.count == 0 {
        return;
    }
    let units = world.units(Side::Alien).to_vec();
    for &id in &units {
        if let Some((_, _, m)) = robot(world, id)
            && m.path & Movement::ON_PATH == 0
            && m.mode & 1 == 0
        {
            assign_random_path(world, id, tables, random, events);
        }
    }
    for &id in &units {
        if let Some((_, _, m)) = robot(world, id)
            && m.mode & 1 == 0
            && m.path & Movement::NEXT != 0
        {
            advance_path_point(world, id, tables, random, events);
        }
    }
}

/// Every object's tick, in slot order (`sim_tick_all_objects`): robots move, bases
/// count their ticks, the alien base walks its robots along paths and a destroyed
/// base steps the mission's end.
pub fn tick(world: &mut World, tables: &Tables, random: &mut Random) -> Vec<Event> {
    let mut events = Vec::new();
    // Slot by slot, as `sim_tick_all_objects` walks the table: an object made in a
    // higher slot during the pass ticks in it too.
    let mut next = world.next_from(0);
    while let Some(id) = next {
        next = world.next_from(id.0 + 1);
        let Some(e) = world.get_mut(id) else {
            continue;
        };
        let (mut path_phase, mut is_robot, mut gone, mut end) = (false, false, false, false);
        match &mut e.state {
            State::Robot(m) => {
                let helm = match e.damage {
                    Damage::Robot(c) => c.helm,
                    _ => 100,
                };
                steer_and_speed(tables, e.kind, helm, &mut e.heading, m);
                is_robot = true;
            }
            State::Base {
                tick, destroyed, ..
            } => {
                *tick = tick.wrapping_add(1);
                path_phase = *tick & 0xf == 2 && e.side == Side::Alien;
                end = *tick & 0xf == 0xf && *destroyed;
            }
            state @ State::Explosion { .. } => gone = crate::damage::tick_explosion(state),
            State::None => {}
        }
        if gone {
            world.remove(id);
        }
        if is_robot {
            crate::script::step(world, id, tables, random, &mut events);
            if let Some((e, terrain)) = world.entity_and_terrain(id)
                && let State::Robot(m) = &mut e.state
                && travel(terrain, e.kind, &mut e.position, m)
            {
                events.push(Event::HillHit(id));
                crate::damage::hit(world, id, 0, tables, random, &mut events);
            }
        }
        if path_phase {
            alien_path_phase(world, tables, random, &mut events);
        }
        if end {
            crate::damage::step_base_end(world, id, tables, random, &mut events);
        }
    }
    events
}

#[cfg(test)]
pub(crate) mod tests;

/// Made-up tables for tests: a true quarter sine (the last entry 0, as the original's),
/// a turn step of 0x100, a weak drive at 0x28, two speed tables (the speed, and half
/// of it) and programs numbered `10 * mission + kind`.
#[cfg(test)]
pub(crate) fn synthetic_tables() -> Tables {
    use xeno_formats::motion::{MISSIONS, SINE_ENTRIES, SPEED_ENTRIES};
    let sine = (0..SINE_ENTRIES)
        .map(|i| {
            let s = (i as f64 * std::f64::consts::PI / 512.0).sin() * 65536.0;
            if i == 256 {
                0
            } else {
                s.round().min(65535.0) as u16
            }
        })
        .collect();
    let speeds = |f: fn(u8) -> u8| std::array::from_fn::<u8, SPEED_ENTRIES, _>(|i| f(i as u8));
    Tables {
        sine,
        turn_step: [0x100; 3],
        weak_drive: 0x28,
        scorpion_speeds: vec![speeds(|s| s), speeds(|s| s / 2)],
        mission_programs: std::array::from_fn::<_, MISSIONS, _>(|m| {
            std::array::from_fn(|k| (10 * m + k) as u8)
        }),
        atan: (0..xeno_formats::motion::ATAN_ENTRIES)
            .map(|i| ((i as f64 / 256.0).atan() / std::f64::consts::FRAC_PI_4 * 65536.0) as u16)
            .collect(),
        script_speeds: [
            [95, 63, 113],
            [56, 42, 85],
            [28, 21, 39],
            [-23, -21, -35],
            [-43, -42, -71],
        ],
        scripts: crate::script::tests::scripts(&[0x0a]),
        damage: xeno_formats::fixtures::synthetic_damage_tables(),
    }
}
