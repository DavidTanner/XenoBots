//! The shots robots fire (`docs/sim.md`, "Weapons and shots"): the scorpion's tail
//! missile (kind 5), the cannons' shells (6), the homing shots (7) and the humanoid's
//! missile (8). [`fire`] makes one as a gun's set-up routine does; [`tick`] flies it,
//! and a hit or a blast damages through [`crate::damage::hit`]. The numbers come from
//! the user's `xeno.exe` (`xeno_formats::weapons`, in [`Tables::weapons`]).

use xeno_formats::damage::Explosion;
use xeno_formats::usg::ObjectKind;
use xeno_formats::weapons::{Gun, Hit};

use crate::damage::{alloc_small, spawn_explosion};
use crate::motion::{Event, Tables, component, velocity};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::script::bearing;
use crate::terrain::approx_distance;
use crate::world::{Damage, Entity, EntityId, Fixed, Position, State, World};

/// How a shot flies, by the set-up routine that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flight {
    /// Kind 5 (`1000:e08e`, `e0e8`): waits on the tail, boosts, then homes.
    Tail,
    /// Kind 6 (`e235`, `e2c8`): falls, two steps a tick.
    Shell,
    /// Kind 7 (`e34a`, `e3ca`): turns to its target every 8 ticks.
    Homing,
    /// Kind 8 (`e48a`, `e537`): flies straight where it was aimed.
    Missile,
}

/// A shot in flight (`docs/formats/usg.md`, "Kinds made during play").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shot {
    pub flight: Flight,
    /// +0x0c: its height, 16.16.
    pub z: i32,
    /// Added to x, y and z every step (1/65,536ths).
    pub velocity: [i16; 3],
    pub speed: u16,
    /// Its climb, 65,536 to a turn.
    pub climb: u16,
    /// The homing shot's age, the tail missile's ticks in its phase.
    pub age: u16,
    /// Steps before it can hit.
    pub arming: u16,
    /// The missile's ticks left.
    pub life: u16,
    pub damage: u8,
    /// The object it homes on (a slot).
    pub target: Option<EntityId>,
    /// The tail missile's turn limit and phase (0, 2, 4).
    pub turn: u16,
    pub phase: u8,
    /// The missile's spinning frame (+0x18).
    pub frame: u8,
}

/// The kinds of shot by their set-up routines.
pub(crate) const TAIL: u16 = 5;
pub(crate) const SHELL: u16 = 6;
pub(crate) const HOMING: u16 = 7;
pub(crate) const MISSILE: u16 = 8;

/// The scout's eye height (+0x1c), which its plasma gun fires from: 0x0d00 less 8 a
/// step of its stance (`docs/render.md`, "The camera").
fn scout_eye(stance: u8) -> u16 {
    0x0d00u16.wrapping_sub(8 * u16::from(stance))
}

fn kind_number(kind: ObjectKind) -> usize {
    usize::from(u16::from(kind))
}

/// An object shots and blasts see (+0x16 bit 6, `IN_FIELD`): a robot unless it is in
/// its base (the take-in and launch move both bits together, T-115).
// shortcut: by kind, as the port keeps no flags (T-147): robots, bases, obstacles and
// relays; the original takes it from a folded relay (T-148).
fn in_field(e: &Entity) -> bool {
    (RobotKind::of(e.kind).is_some() && !crate::base::in_base(&e.damage))
        || matches!(
            e.kind,
            ObjectKind::Base | ObjectKind::Relay | ObjectKind::Obstacle
        )
}

/// The velocity for `heading`, `climb` and `speed` (`1000:045d`): x, y, z.
pub fn velocity3(tables: &Tables, heading: u16, climb: u16, speed: i16) -> [i16; 3] {
    let z = component(&tables.sine, climb, speed);
    let level = component(&tables.sine, 0x4000u16.wrapping_sub(climb), speed);
    let (x, y) = velocity(tables, heading, level);
    [x, y, z]
}

/// The muzzle's offset for `heading` from a gun's table: its eighth of a quarter
/// turn, turned by the quarter (`DS:0x708f`).
fn muzzle(offsets: &[(i16, i16); 8], heading: u16) -> (i16, i16) {
    let (x, y) = offsets[usize::from((heading >> 11) & 7)];
    match heading >> 14 {
        0 => (x, y),
        1 => (y, x.wrapping_neg()),
        2 => (x.wrapping_neg(), y.wrapping_neg()),
        _ => (y.wrapping_neg(), x),
    }
}

fn offset(at: Position, dx: i16, dy: i16) -> Position {
    Position {
        x: Fixed(at.x.0.wrapping_add(i32::from(dx))),
        y: Fixed(at.y.0.wrapping_add(i32::from(dy))),
    }
}

/// Where the shot `id`'s object is: an object's z is 0, a shot's its own.
// shortcut: robots, bases and the rest keep no z (T-101), so they stand at 0; hills
// would lift them.
fn height(e: &Entity) -> i32 {
    match e.state {
        State::Shot(s) => s.z,
        _ => 0,
    }
}

/// The heading robot `id` aims a shot of `lead` speed at its target (`1000:4549`):
/// with none its heading; a target not a unit, the target's bearing (+0xea); a unit,
/// where it will be: the target's velocity is added a quarter at a time, up to five
/// times, until a quarter of `lead` as many times covers the distance in x and y.
fn aim(world: &mut World, id: EntityId, lead: u16, tables: &Tables) -> u16 {
    let Some(me) = world.get(id) else {
        return 0;
    };
    let State::Robot(m) = me.state else {
        return me.heading;
    };
    let Some(t) = m.target.and_then(|t| world.get(t)) else {
        return me.heading;
    };
    let State::Robot(tm) = t.state else {
        return m.target_bearing;
    };
    let mid = |a: Fixed, b: Fixed| (b.0.wrapping_sub(a.0) >> 8) as i16;
    let (mut dx, mut dy) = (
        mid(me.position.x, t.position.x),
        mid(me.position.y, t.position.y),
    );
    let (vx, vy) = (tm.velocity.0 >> 2, tm.velocity.1 >> 2);
    let step = (lead as i16 >> 2) as u16;
    // The quarter of the lead goes through the worked-out point's first word.
    world.point.x.0 = (world.point.x.0 & !0xffff) | i32::from(step);
    let covers = |d: i16, reach: u16| {
        if d >= 0 {
            reach >= d as u16
        } else {
            reach.wrapping_neg() < d as u16
        }
    };
    let mut reach = 0u16;
    for _ in 0..5 {
        reach = reach.wrapping_add(step);
        dx = dx.wrapping_add(vx);
        dy = dy.wrapping_add(vy);
        if covers(dx, reach) && covers(dy, reach) {
            break;
        }
    }
    dx = dx.wrapping_sub(vx >> 1);
    dy = dy.wrapping_sub(vy >> 1);
    // shortcut: the coarse bearing; the original's is the fine one (T-146).
    let to = Position {
        x: Fixed(i32::from(dx) << 8),
        y: Fixed(i32::from(dy) << 8),
    };
    bearing(&tables.atan, Position::default(), to)
}

/// The climb from height `dz` over a level distance (`1000:0782` on the two, as a
/// bearing).
fn pitch(tables: &Tables, dz: i32, distance: u32) -> u16 {
    let to = Position {
        x: Fixed(dz),
        y: Fixed(distance as i32),
    };
    bearing(&tables.atan, Position::default(), to)
}

/// The target's height for aiming: its z and its kind's height (`DS:0xe7a5`) above.
fn aim_height(tables: &Tables, t: &Entity) -> i32 {
    let h = tables
        .weapons
        .heights
        .get(kind_number(t.kind))
        .copied()
        .unwrap_or(0);
    height(t).wrapping_add(i32::from(h as i16) << 8)
}

/// Robot `id` fires `gun` (each gun's routine from the shot's allocation on). Returns
/// the slot it took, none if none was free; the set-up may still drop the shot (a
/// homing shot or tail missile whose target is gone), as the original frees it.
pub fn fire(
    world: &mut World,
    id: EntityId,
    gun: &Gun,
    tables: &Tables,
    random: &mut Random,
) -> Option<EntityId> {
    let slot = alloc_small(world)?;
    let me = world.get(id)?;
    let State::Robot(m) = me.state else {
        return None;
    };
    let w = &tables.weapons;
    let (kind, side, heading) = (me.kind, me.side, me.heading);
    let mut at = me.position;
    if let Some(offsets) = &gun.offsets {
        let (dx, dy) = muzzle(offsets, heading);
        at = offset(at, dx, dy);
    }
    let base = height(me);
    let target = m.target.filter(|&t| world.get(t).is_some());
    let mut shot = Shot {
        flight: Flight::Shell,
        z: base,
        velocity: [0; 3],
        speed: 0,
        climb: 0,
        age: 0,
        arming: 0,
        life: 0,
        damage: gun.damage,
        target: None,
        turn: 0,
        phase: 0,
        frame: 0,
    };
    let mut shot_heading = heading;
    match gun.kind {
        TAIL => {
            let Some(target) = target else {
                return Some(slot);
            };
            let t = &w.tail;
            (shot.flight, shot.turn, shot.target) = (Flight::Tail, t.turn, Some(target));
            // Its blast has its own points; the object keeps no damage (T-116).
            shot.damage = 0;
            shot.z = base.wrapping_add(i32::from(t.height));
        }
        SHELL => {
            let s = &w.shell;
            (shot.arming, shot.speed) = (s.arming, s.speed);
            shot.z = base.wrapping_add(i32::from(s.height));
            let spread = |mask: u16, r: &mut Random| (r.word() & mask).wrapping_sub(mask / 2 + 1);
            if m.target.is_some() {
                let spread = spread(s.spread, random);
                shot_heading = aim(world, id, s.lead, tables).wrapping_add(spread);
            } else {
                shot_heading = heading.wrapping_add(spread(s.wild_spread, random));
            }
            let (vx, vy) = velocity(tables, shot_heading, s.speed as i16);
            let vz = match m.target {
                Some(_) => s.climb,
                None => random.word() & s.wild_climb,
            };
            shot.velocity = [vx, vy, vz as i16];
        }
        HOMING => {
            let Some(target) = target else {
                return Some(slot);
            };
            let h = &w.homing;
            (shot.flight, shot.arming, shot.speed) = (Flight::Homing, h.arming, h.speed);
            shot.target = Some(target);
            let lift = match kind {
                ObjectKind::Scout => scout_eye(m.stance).wrapping_sub(gun.height),
                _ => gun.height,
            };
            shot.z = base.wrapping_add(i32::from(lift as i16));
        }
        MISSILE => {
            let ms = &w.missile;
            (shot.flight, shot.arming, shot.speed) = (Flight::Missile, ms.arming, ms.speed);
            (shot.life, shot.damage) = (ms.life, ms.damage);
            shot.z = base.wrapping_add(i32::from(gun.height as i16));
            let spread = (random.word() & ms.spread).wrapping_sub(ms.spread / 2 + 1);
            let aimed = m.target.and_then(|t| world.get(t)).map(|t| {
                let level = crate::sense::true_distance(at, t.position);
                (aim_height(tables, t).wrapping_sub(shot.z), level)
            });
            // The climb is not kept: the object has no field for it (T-116).
            let climb;
            (shot_heading, climb) = match aimed {
                // shortcut: the true distance takes z too (`1000:0942`); the port's is
                // level (T-146).
                Some((dz, level)) => (
                    aim(world, id, ms.lead, tables).wrapping_add(spread),
                    pitch(tables, dz, level),
                ),
                None => (heading.wrapping_add(spread), 0),
            };
            shot.velocity = velocity3(tables, shot_heading, climb, ms.speed as i16);
            // It moves once as it is made, and keeps its model's heading.
            at = step(at, &mut shot);
            shot_heading = ms.heading;
        }
        _ => return Some(slot),
    }
    world.insert(
        slot,
        Entity {
            kind: gun.kind.into(),
            side,
            position: at,
            heading: shot_heading,
            radius: 0,
            damage: Damage::None,
            state: State::Shot(shot),
        },
    );
    Some(slot)
}

/// One step of the shot's velocity.
fn step(at: Position, s: &mut Shot) -> Position {
    s.z = s.z.wrapping_add(i32::from(s.velocity[2]));
    offset(at, s.velocity[0], s.velocity[1])
}

/// The first object in slot order a shot at `at`, height `z`, is over and hits
/// (`sim_find_shot_hit`, `1000:e8ac`): in the field, not the shot, within the object's
/// radius in x and y, and passing its kind's hit test.
fn find_hit(
    world: &World,
    id: EntityId,
    at: Position,
    z: i32,
    tables: &Tables,
) -> Option<EntityId> {
    let within = |a: Fixed, b: Fixed, r: u16| {
        let d = a.0.wrapping_sub(b.0).unsigned_abs();
        d >> 16 == 0 && d as u16 <= r
    };
    world.iter().find_map(|(o, e)| {
        let over = o != id
            && in_field(e)
            && within(at.x, e.position.x, e.radius)
            && within(at.y, e.position.y, e.radius);
        let dz = z.wrapping_sub(height(e));
        let hit = match tables.weapons.hits.get(kind_number(e.kind)) {
            Some(Hit::Always) => true,
            Some(&Hit::Band(low, high)) => dz > i32::from(low) && dz <= i32::from(high),
            Some(&Hit::Below(below)) => (0..i32::from(below)).contains(&z),
            _ => false,
        };
        (over && hit).then_some(o)
    })
}

/// The distance in three axes (`1000:0a11`): the largest, and a quarter of each of the
/// others as the original pairs them.
fn approx_distance3(a: Position, az: i32, b: Position, bz: i32) -> u32 {
    let d = |p: i32, q: i32| p.checked_sub(q).map(i32::unsigned_abs);
    let (Some(x), Some(y), Some(z)) = (d(a.x.0, b.x.0), d(a.y.0, b.y.0), d(az, bz)) else {
        return 0x7fff_ffff;
    };
    // The smaller of x and y is quartered; the larger meets z, and the smaller of those.
    let (kept, quarter) = if x < y { (y, x / 4) } else { (x, y / 4) };
    let (big, small) = if z < kept {
        (kept, z / 4)
    } else {
        (z, kept / 4)
    };
    big.wrapping_add(small).wrapping_add(quarter)
}

/// A blast of `points` in `radius` (8.8) at `at`, height `z`
/// (`sim_damage_objects_in_radius`, `1000:e8eb`): every object in the field within the
/// radius takes the points, halved for each step out from half the radius.
fn blast(
    world: &mut World,
    at: Position,
    z: i32,
    (points, radius): (u8, u16),
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let hit: Vec<(EntityId, u8)> = world
        .iter()
        .filter(|(_, e)| in_field(e))
        .filter_map(|(o, e)| {
            let d = approx_distance3(e.position, height(e), at, z);
            let mid = (d >> 8) as u16;
            if d >> 24 != 0 || mid > radius {
                return None;
            }
            let (mut reach, mut points) = (radius >> 1, points);
            while mid > reach {
                reach = reach.wrapping_add(reach >> 1);
                points >>= 1;
                if points == 0 {
                    break;
                }
            }
            Some((o, points))
        })
        .collect();
    for (o, points) in hit {
        crate::damage::hit(world, o, points, tables, random, events);
    }
}

/// An explosion from `template` where a shot ends, and the explosions' sound unless
/// `quiet` (`sim_spawn_explosion`, then `snd_play_sound_effect`).
fn burst(
    world: &mut World,
    at: Position,
    template: Explosion,
    quiet: bool,
    tables: &Tables,
    events: &mut Vec<Event>,
) {
    spawn_explosion(world, at, tables.damage.explosion_kind, template);
    if !quiet {
        events.push(Event::Sound {
            event: tables.weapons.blast_sound,
            only_for: None,
        });
    }
}

/// What a step of the shot's flight ended in.
enum End {
    Flying,
    Ground,
    Hit(EntityId),
}

/// Moves shot `id` a step and looks for the ground and, once armed, a hit.
fn fly(world: &mut World, id: EntityId, s: &mut Shot, tables: &Tables) -> End {
    let Some(e) = world.get(id) else {
        return End::Flying;
    };
    let at = step(e.position, s);
    if let Some(e) = world.get_mut(id) {
        e.position = at;
    }
    if world.terrain.below_ground(at, Fixed(s.z)) {
        return End::Ground;
    }
    if s.arming != 0 {
        s.arming -= 1;
        return End::Flying;
    }
    find_hit(world, id, at, s.z, tables).map_or(End::Flying, End::Hit)
}

/// The target of a homing shot or tail missile, if still there.
// shortcut: the port frees a destroyed robot's slot at once (T-054), so a slot holding
// a shot or explosion is the target gone; the original holds the slot by reference.
fn target(world: &World, s: &Shot) -> Option<EntityId> {
    s.target.filter(|&t| {
        world
            .get(t)
            .is_some_and(|e| !matches!(e.state, State::Shot(_) | State::Explosion { .. }))
    })
}

/// The heading and climb from shot `at`, `z` to `t` and its level distance
/// (`1000:e95e`, through `1000:0550`).
fn home(tables: &Tables, at: Position, z: i32, t: &Entity) -> (u16, u16, u32) {
    let heading = bearing(&tables.atan, at, t.position);
    let distance = approx_distance(at, t.position).0 as u32;
    let dz = aim_height(tables, t).wrapping_sub(z);
    (heading, pitch(tables, dz, distance), distance)
}

/// `heading` turned towards `to` by at most `limit`.
fn turn(heading: u16, to: u16, limit: u16) -> u16 {
    crate::motion::steer(heading, to, limit)
}

/// One tick of shot `id` (the kinds' ticks in `DS:0xe6e8`).
pub fn tick(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some(&Entity {
        state: State::Shot(mut s),
        position,
        heading,
        ..
    }) = world.get(id)
    else {
        return;
    };
    let w = &tables.weapons;
    let mut heading = heading;
    let end = match s.flight {
        Flight::Shell => {
            let quiet = s.damage == 1;
            let mut end = End::Flying;
            for _ in 0..2 {
                end = fly(world, id, &mut s, tables);
                s.velocity[2] = s.velocity[2].wrapping_sub(i16::from(w.shell.fall));
                if !matches!(end, End::Flying) {
                    break;
                }
            }
            let at = world.get(id).map_or(position, |e| e.position);
            match end {
                End::Ground => burst(world, at, w.shell.ground, quiet, tables, events),
                End::Hit(o) => {
                    burst(world, at, w.shell.hit, quiet, tables, events);
                    crate::damage::hit(world, o, s.damage, tables, random, events);
                }
                End::Flying => {}
            }
            end
        }
        Flight::Homing => {
            let h = &w.homing;
            s.age = s.age.wrapping_add(1);
            if s.age >= h.life {
                world.remove(id);
                return;
            }
            if s.age & h.steer_mask == 0 {
                let Some(t) = target(world, &s).and_then(|t| world.get(t)) else {
                    world.remove(id);
                    return;
                };
                let (to, climb, _) = home(tables, position, s.z, t);
                s.climb = climb;
                heading = turn(heading, to, h.turn);
                s.velocity = velocity3(tables, heading, s.climb, s.speed as i16);
            }
            fly(world, id, &mut s, tables)
        }
        Flight::Missile => {
            s.frame = (s.frame + 1) & 3;
            s.life = s.life.wrapping_sub(1);
            if s.life == 0 {
                world.remove(id);
                return;
            }
            fly(world, id, &mut s, tables)
        }
        Flight::Tail => return tick_tail(world, id, s, heading, tables, random, events),
    };
    let at = world.get(id).map_or(position, |e| e.position);
    let (hit, ground) = match s.flight {
        Flight::Missile => (w.missile.hit, w.missile.ground),
        _ => (w.homing.hit, w.homing.ground),
    };
    match end {
        End::Flying => {
            if let Some(e) = world.get_mut(id) {
                (e.heading, e.state) = (heading, State::Shot(s));
            }
            return;
        }
        // Only the shell's explosion on the ground is heard.
        End::Ground if s.flight != Flight::Shell => burst(world, at, ground, true, tables, events),
        End::Hit(o) if s.flight != Flight::Shell => {
            burst(world, at, hit, false, tables, events);
            crate::damage::hit(world, o, s.damage, tables, random, events);
        }
        _ => {}
    }
    // Freed after the damage, so an explosion it causes takes another slot.
    world.remove(id);
}

/// The tail missile's tick (`1000:e0e8`): on the tail for its launch ticks, then
/// boosting straight with a rising climb, then homing with a growing turn; on the
/// ground it explodes, and on a hit or with its target gone it blasts.
fn tick_tail(
    world: &mut World,
    id: EntityId,
    mut s: Shot,
    mut heading: u16,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let t = &tables.weapons.tail;
    s.age = s.age.wrapping_add(1);
    let store = |world: &mut World, s: Shot, heading: u16| {
        if let Some(e) = world.get_mut(id) {
            (e.heading, e.state) = (heading, State::Shot(s));
        }
    };
    if s.phase == 0 {
        if s.age >= u16::from(t.launch) {
            (s.phase, s.age, s.speed, s.climb) = (2, 0, t.speed, t.climb);
        }
        return store(world, s, heading);
    }
    if s.phase == 2 {
        if s.age < u16::from(t.boost_ticks) {
            if s.speed < t.top {
                s.speed = s.speed.wrapping_add(t.boost);
            }
            s.velocity = velocity3(tables, heading, s.climb, s.speed as i16);
            if let Some(e) = world.get_mut(id) {
                e.position = step(e.position, &mut s);
            }
            return store(world, s, heading);
        }
        (s.phase, s.age, s.climb) = (4, 0, 0);
    }
    if s.phase != 4 {
        return store(world, s, heading);
    }
    s.turn = s.turn.wrapping_add(u16::from(t.turn_step));
    let Some(e) = world.get(id) else {
        return;
    };
    let position = e.position;
    let blasts = match target(world, &s).and_then(|o| world.get(o)) {
        None => true,
        Some(target) => {
            let (to, climb, distance) = home(tables, position, s.z, target);
            if distance >> 24 == 0 && (distance >> 8) as u16 <= t.near {
                s.climb = (climb as i16).clamp(-(t.dive as i16), t.dive as i16) as u16;
            }
            heading = turn(heading, to, s.turn);
            s.velocity = velocity3(tables, heading, s.climb, s.speed as i16);
            let at = step(position, &mut s);
            if let Some(e) = world.get_mut(id) {
                e.position = at;
            }
            if world.terrain.below_ground(at, Fixed(s.z)) {
                burst(world, at, t.explosion, false, tables, events);
                world.remove(id);
                return;
            }
            let hit = find_hit(world, id, at, s.z, tables).is_some();
            if !hit {
                return store(world, s, heading);
            }
            true
        }
    };
    let at = world.get(id).map_or(position, |e| e.position);
    if blasts {
        blast(world, at, s.z, (t.points, t.radius), tables, random, events);
    }
    burst(world, at, t.explosion, false, tables, events);
    // shortcut: removal makes the original's kind 15 for two ticks (`1000:f5a4`); the
    // port frees the slot at once, as it does every object's.
    world.remove(id);
}

#[cfg(test)]
mod tests;
