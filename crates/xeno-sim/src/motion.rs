//! How robots move and walk paths, as the original does it each tick (`docs/sim.md`,
//! "Movement"): steering, speed steps, the velocity from the sine table, the move, the
//! hill bounce, and the alien base's path walking. The tables come from the user's
//! `xeno.exe` ([`Tables`], `xeno_formats::motion`).
//!
//! The scripts (`crate::script`, T-053) decide where robots go: they run between the
//! speed step and the move, and call [`head_for_path_point`] and
//! [`join_nearest_path_point`] and set a robot's target heading and speed.

use xeno_formats::motion::{STEPS, Walk};
use xeno_formats::usg::{ObjectKind, Side};

pub use xeno_formats::motion::Motion as Tables;

use crate::orders::{Change, RobotKind};
use crate::random::Random;
use crate::terrain::{PathTable, Terrain, approx_distance};
use crate::world::{Damage, EntityId, Fixed, Movement, Position, State, World};

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
    /// The robot came home and its base took it in (`docs/sim.md`, "The bases'
    /// phases").
    TakenIn(EntityId),
    /// The robot left its base (`docs/sim.md`, "The bases' phases").
    Launched(EntityId),
    /// Sound event `event` plays (`docs/formats/dsounds.md`, "Sound events"): for every
    /// view, or with `only_for` only while that object's console is open
    /// (`snd_play_sound_effect_if_viewed`).
    Sound {
        event: u8,
        only_for: Option<EntityId>,
    },
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
pub(crate) fn component(table: &[u16], angle: u16, speed: i16) -> i16 {
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
/// script, after the recall ([`recall`]).
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
        ObjectKind::Scorpion => Some(map_scorpion_speed(tables, m.relays, limit(m))),
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
        } else if m.stance != m.stance_target {
            // `sim_step_scout_stance`: a step, and the eye height by the stance.
            if m.stance < m.stance_target {
                m.stance += 1;
            } else {
                m.stance -= 1;
            }
            let drop = i16::from(m.stance as i8).wrapping_shl(3) as u16;
            m.eye_height = tables.walk.scout_eye.wrapping_sub(drop);
        }
    }
    if changed {
        m.velocity = velocity(tables, *heading, m.speed);
    }
}

/// A humanoid's or scout's gears; a scorpion has none.
fn gears(tables: &Tables, kind: ObjectKind) -> Option<&[i16]> {
    match kind {
        ObjectKind::Humanoid => Some(&tables.gears.humanoid),
        ObjectKind::Scout => Some(&tables.gears.scout),
        _ => None,
    }
}

/// The start of `sim_move_*`: a robot the situation room recalled (+0x23 bit 3) is
/// given the recall's program (`Tables::recall_program`), its program takes over and
/// the bit is cleared. (The kind's routine after it, `1000:b365`, `9b87`, `cb3d`, only
/// marks the console's panels for redrawing.)
fn recall(world: &mut World, id: EntityId, tables: &Tables, events: &mut Vec<Event>) {
    let Some(m) = movement(world, id) else {
        return;
    };
    if m.mode & Movement::RECALL == 0 {
        return;
    }
    m.mode &= !Movement::RECALL;
    crate::script::set_program(world, id, tables.recall_program, tables, events);
    crate::script::switch(world, id, true, events);
}

/// One gear up or down: the speed keys (`1000:acda`, `ace0` and the other kinds')
/// and the scripts' opcodes 0x51 and 0x52 (the routines at `DS:0x722d`, `0x7233`). A
/// humanoid or scout takes the next gear's speed; a scorpion's speed asked for moves
/// by a step within its range. Returns false at the end (the routine's carry).
pub fn shift(tables: &Tables, kind: ObjectKind, m: &mut Movement, up: bool) -> bool {
    let g = &tables.gears;
    if kind == ObjectKind::Scorpion {
        let s = m.target_speed;
        m.target_speed = match up {
            true if s < g.scorpion_top => s.saturating_add(g.scorpion_step).min(g.scorpion_top),
            false if s > g.scorpion_bottom => {
                s.saturating_sub(g.scorpion_step).max(g.scorpion_bottom)
            }
            _ => return false,
        };
        return true;
    }
    let Some(gears) = gears(tables, kind) else {
        return false;
    };
    let top = 2 * (gears.len() as u8 - 1);
    let gear = match up {
        true if m.gear != top => m.gear.wrapping_add(2),
        false if m.gear != 0 => m.gear.wrapping_sub(2),
        _ => return false,
    };
    // shortcut: an odd or out-of-range gear from a file reads past the original's
    // table; the port leaves the speed asked for. None in the stock files.
    if let Some(&speed) = gears.get(usize::from(gear / 2)) {
        m.gear = gear;
        m.target_speed = speed;
    }
    true
}

/// Robot `id` selects `weapon` (F5, F6, F9 and the scripts' 0x7b and 0x7c: the kinds'
/// routines through `cs:0xd13c`): with a change, the kind's message to its panel and
/// the sound for it (`Tables::weapon_keys`). `weapons` is its damaged bits (+0xdc).
pub fn select_weapon(
    tables: &Tables,
    kind: ObjectKind,
    weapons: u8,
    id: EntityId,
    m: &mut Movement,
    weapon: u8,
    events: &mut Vec<Event>,
) {
    if weapon == m.weapon {
        return;
    }
    m.weapon = weapon;
    let w = &tables.weapon_keys;
    let k = usize::from(kind == ObjectKind::Scorpion) + 2 * usize::from(kind == ObjectKind::Scout);
    let damaged = w.scorpion_bits.get(usize::from(weapon));
    // The count's low byte (`cmp byte [bx + di], 0`, by `DS:0x65f6`).
    let empty = match tables.weapons.counts.get(usize::from(weapon)) {
        Some(0x120) => m.guns.rounds as u8 == 0,
        Some(0x122) => m.guns.missiles == 0,
        _ => false,
    };
    let messages = match damaged {
        Some(&bit) if k == 1 && weapons & bit != 0 => &w.scorpion_damaged,
        _ if k == 1 && empty => &w.scorpion_empty,
        _ => &w.selected[k],
    };
    if let Some(&text) = messages.get(usize::from(weapon)) {
        m.panel.post(text);
    }
    events.push(Event::Sound {
        event: w.sound,
        only_for: Some(id),
    });
}

/// The kind's bounce routine (`DS:0x7209`; the hill bounce, the stop opcode 0x53 and
/// the `.` key): a humanoid's or scout's gear becomes the first whose speed is at
/// least the speed asked for (the top gear past them all). A scorpion's does nothing.
pub fn match_gear(tables: &Tables, kind: ObjectKind, m: &mut Movement) {
    if let Some(gears) = gears(tables, kind) {
        let i = gears.iter().position(|&s| m.target_speed <= s);
        m.gear = 2 * i.unwrap_or(gears.len() - 1) as u8;
    }
}

/// The move, the hill bounce or the ground's height (`sim_check_robot_against_hills`),
/// and the tick counter's step: `sim_move_*` after the script. Returns whether the
/// robot ended inside a hill.
pub fn travel(
    terrain: &Terrain,
    kind: ObjectKind,
    (position, z): (&mut Position, &mut Fixed),
    m: &mut Movement,
) -> bool {
    if kind_index(kind).is_none() {
        return false;
    }
    position.x.0 = position.x.0.wrapping_add(i32::from(m.velocity.0));
    position.y.0 = position.y.0.wrapping_add(i32::from(m.velocity.1));
    // Inside a hill: back off at the same speed. The velocity stays as it is until the
    // speed next steps. Otherwise it stands on the ground (`sim_set_height_to_ground`),
    // which tests the same hills, so it is always 0 there.
    let hit = terrain.hill_at(*position).is_some();
    if hit {
        m.speed = m.speed.wrapping_neg();
        m.target_speed = 0;
    } else {
        *z = terrain.ground_height(*position);
    }

    m.tick = m.tick.wrapping_add(2);
    if kind != ObjectKind::Humanoid {
        m.tick &= 0x1e;
    }
    hit
}

/// A scout's stance when it stands, the only one it moves in.
pub const STANDING: u8 = 0x30;

/// Footstep sound events (`docs/formats/dsounds.md`, "Sound events").
pub const FOOTSTEP_HUMANOID: u8 = 13;
pub const FOOTSTEP_SCORPION: u8 = 14;
pub const FOOTSTEP_SCOUT: u8 = 15;

/// One tick of a robot's walking animation after its move (`docs/render.md`,
/// "Walking"): the step counter, the leg frames, and the model height (+0x12) and the
/// humanoid's eye height that go with them (`docs/sim.md`, "Heights"). `ticks` is the
/// pass count (`DS:0x62c6`); `footstep` hears each footstep event.
pub fn walk(
    walk: &Walk,
    kind: ObjectKind,
    heading: u16,
    ticks: u16,
    (m, model_height): (&mut Movement, &mut u16),
    mut footstep: impl FnMut(u8),
) {
    m.stride = m.stride.wrapping_add((m.speed >> 3) as u8);
    let step = usize::from(m.stride >> 4);
    // A leg takes its frame for the step; one that comes back to 0 is a footstep.
    let mut stride = |legs: &mut [u8], tables: &[[u8; STEPS]; 2], event| {
        for (leg, table) in legs.iter_mut().zip(tables) {
            let old = std::mem::replace(leg, table[step]);
            if *leg == 0 && old != 0 {
                footstep(event);
            }
        }
    };
    match kind {
        ObjectKind::Humanoid if m.speed != 0 => {
            stride(&mut m.legs[2..4], &walk.humanoid_legs, FOOTSTEP_HUMANOID)
        }
        ObjectKind::Humanoid => {
            // Still: the legs settle, one frame every 8 passes, 4 wrapping to 0 (the
            // scorpion's the same, 5 wrapping).
            m.stride = 0;
            if ticks & 7 == 0 {
                settle(&mut m.legs[2..4], 5);
            }
        }
        ObjectKind::Scorpion if m.speed != 0 => {
            for (leg, table) in m.legs.iter_mut().zip(&walk.scorpion_legs) {
                *leg = table[step];
            }
            if step % 2 == 0 {
                footstep(FOOTSTEP_SCORPION);
            }
        }
        ObjectKind::Scorpion if ticks & 7 == 0 => settle(&mut m.legs, 6),
        ObjectKind::Scout if m.speed != 0 => {
            stride(&mut m.legs[4..], &walk.scout_legs, FOOTSTEP_SCOUT);
            // +0xff and +0x100 take +0x103's and +0x104's frames (`1000:c7c6`, `c7d8`).
            (m.legs[0], m.legs[1]) = (m.legs[4], m.legs[5]);
            *model_height = walk.scout_height.wrapping_add(walk.scout_bob[step]);
        }
        ObjectKind::Scout if m.target_heading != heading || m.stance == STANDING => {
            m.stride = 0;
            scout_legs(&mut m.legs, 0);
        }
        ObjectKind::Scout => {
            // shortcut: stances past 0x3f would read past the 4 entries; none occur.
            let crouch = walk.scout_crouch.get(usize::from(m.stance >> 4));
            scout_legs(&mut m.legs, crouch.copied().unwrap_or(0));
            let drop = u16::from(m.stance).wrapping_shl(3);
            *model_height = walk.scout_crouched.wrapping_sub(drop);
        }
        _ => {}
    }
    // The humanoid's are set every tick, moving or not, by the step before a still
    // one's counter is cleared.
    if kind == ObjectKind::Humanoid {
        *model_height = walk.humanoid_height.wrapping_add(walk.humanoid_bob[step]);
        m.eye_height = walk.humanoid_eye;
    }
}

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

/// Still legs settle: each not at 0 steps up a frame, `frames` (only) wrapping to 0.
fn settle(legs: &mut [u8], frames: u8) {
    for leg in legs.iter_mut().filter(|l| **l != 0) {
        *leg = leg.wrapping_add(1);
        if *leg == frames {
            *leg = 0;
        }
    }
}

/// A still scout's four legs, +0xff, +0x100, +0x103 and +0x104, all at `frame`; its
/// +0x101 and +0x102 are never set.
fn scout_legs(legs: &mut [u8; 6], frame: u8) {
    for k in [0, 1, 4, 5] {
        legs[k] = frame;
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
pub(crate) fn alien_path_phase(
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
    world.ticks = world.ticks.wrapping_add(1);
    // Slot by slot, as `sim_tick_all_objects` walks the table: an object made in a
    // higher slot during the pass ticks in it too.
    let mut next = world.next_from(0);
    while let Some(id) = next {
        next = world.next_from(id.0 + 1);
        recall(world, id, tables, &mut events);
        let Some(e) = world.get_mut(id) else {
            continue;
        };
        let (mut base, mut is_robot, mut gone, mut shot) = (false, false, false, false);
        let mut relay = false;
        match &mut e.state {
            State::Robot(m) => {
                let helm = match e.damage {
                    Damage::Robot(c) => c.helm,
                    _ => 100,
                };
                steer_and_speed(tables, e.kind, helm, &mut e.heading, m);
                is_robot = true;
            }
            State::Base { .. } => base = true,
            state @ State::Explosion { .. } => gone = crate::damage::tick_explosion(state),
            State::Shot(_) => shot = true,
            State::Relay(_) => relay = true,
            State::None => {}
        }
        if relay {
            crate::relay::tick(world, id, tables);
        }
        if shot {
            crate::shot::tick(world, id, tables, random, &mut events);
        }
        if gone {
            world.remove(id);
        }
        if is_robot {
            crate::script::step(world, id, tables, random, &mut events);
            if let Some((e, terrain)) = world.entity_and_terrain(id)
                && let State::Robot(m) = &mut e.state
                && travel(terrain, e.kind, (&mut e.position, &mut e.z), m)
            {
                match_gear(tables, e.kind, m);
                events.push(Event::HillHit(id));
                crate::damage::hit(world, id, 0, tables, random, &mut events);
            }
            // `1fe6:826d` comes after the move.
            if let Some(m) = movement(world, id) {
                m.panel.type_one(&tables.strings);
            }
            crate::sense::step(world, id, tables);
            crate::weapons::step(world, id, tables, random, &mut events);
            crate::relay::signal_step(world, id, tables);
            let ticks = world.ticks;
            if let Some(e) = world.get_mut(id)
                && let State::Robot(m) = &mut e.state
            {
                let parts = (&mut *m, &mut e.model_height);
                walk(&tables.walk, e.kind, e.heading, ticks, parts, |event| {
                    events.push(Event::Sound {
                        event,
                        only_for: Some(id),
                    })
                });
                if e.kind == ObjectKind::Humanoid
                    && let Damage::Robot(c) = &mut e.damage
                {
                    crate::weapons::arms(&tables.walk, ticks, m, c);
                }
            }
            // The last call of the kind's tick.
            crate::damage::total(world, id, tables, &mut events);
        }
        if base {
            crate::base::tick(world, id, tables, random, &mut events);
        }
    }
    events
}

#[cfg(test)]
pub(crate) mod tests;

/// Made-up tables for tests: a true quarter sine (the last entry 0, as the original's),
/// a turn step of 0x100, a weak drive at 0x28, two speed tables (the speed, and half
/// of it) and programs numbered `10 * mission + kind`.
/// Made-up walking tables: legs `step & 3` and `(step + 2) & 3`, crouched `stance / 16`,
/// the scorpion's leg +0xff + k `(step + k) % 6`;
/// model heights 0x0a00 (humanoid) and 0x0c80 (scout) plus `0x10 * step`, crouched
/// `0x0c90 - 8 * stance`; eyes 0x0f80 and `0x0d00 - 8 * stance`.
#[cfg(test)]
pub(crate) fn synthetic_walk() -> Walk {
    let legs = |shift: u8| std::array::from_fn(|k| (k as u8 + shift) & 3);
    let bob = std::array::from_fn(|k| 0x10 * k as u16);
    Walk {
        humanoid_legs: [legs(0), legs(2)],
        humanoid_arms: [legs(0), legs(1)],
        scout_legs: [legs(0), legs(2)],
        scout_crouch: [0, 1, 2, 3],
        scorpion_legs: std::array::from_fn(|k| std::array::from_fn(|i| ((i + k) % 6) as u8)),
        humanoid_height: 0x0a00,
        humanoid_bob: bob,
        humanoid_eye: 0x0f80,
        scout_height: 0x0c80,
        scout_bob: bob,
        scout_crouched: 0x0c90,
        scout_eye: 0x0d00,
    }
}

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
        gears: xeno_formats::motion::Gears {
            humanoid: vec![-43, -23, 0, 28, 56, 95],
            scout: vec![-71, -35, 0, 39, 85, 113],
            scorpion_step: 21,
            scorpion_top: 63,
            scorpion_bottom: -42,
            turn: 0x100,
        },
        weapon_keys: xeno_formats::motion::WeaponKeys {
            sound: 27,
            selected: [[0x10, 0x11], [0x20, 0x21], [0x30, 0x31]],
            scorpion_empty: [0x22, 0x23],
            scorpion_damaged: [0x24, 0x25],
            scorpion_bits: [1, 2],
        },
        recall_program: 4,
        script_speeds: [
            [95, 63, 113],
            [56, 42, 85],
            [28, 21, 39],
            [-23, -21, -35],
            [-43, -42, -71],
        ],
        scripts: crate::script::tests::scripts(&[0x0a]),
        damage: xeno_formats::fixtures::synthetic_damage_tables(),
        sensing: xeno_formats::fixtures::synthetic_sensing_tables(),
        net: xeno_formats::fixtures::synthetic_net_tables(),
        relays: xeno_formats::fixtures::synthetic_relay_tables(),
        weapons: xeno_formats::fixtures::synthetic_weapon_tables(),
        walk: synthetic_walk(),
        bases: xeno_formats::fixtures::synthetic_base_tables(),
        strings: xeno_formats::motion::Strings(b"\0AB\r\xc3\0".to_vec()),
    }
}
