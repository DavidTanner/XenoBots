//! The robots' scripts, stepped as `order_step_script` does each tick (`docs/sim.md`,
//! "Orders"): the program's start, the wait, events and their handlers, and the
//! opcodes for flow, waits, the stack and fields, speeds, steering and paths. The
//! scripts and their tables are read from the user's `xeno.exe`
//! (`xeno_formats::script`, in [`Tables::scripts`]).

use xeno_formats::script::{Instruction, Operand};

use crate::motion::{
    Event, Tables, base_position, head_for_path_point, join_nearest_path_point, match_gear,
    movement, robot, shift,
};
use crate::orders::RobotKind;
use crate::random::Random;
use crate::terrain::approx_distance;
use crate::world::{EntityId, Fixed, Movement, Position, Script, World};

const CARRY: u8 = 0x01;
const ZERO: u8 = 0x40;
const SIGN: u8 = 0x80;
/// +0x33 bit 5: step every tick and hold events off.
const BUSY: u8 = 0x20;
/// The stack's empty pointer.
const EMPTY: u8 = 10;
/// Programs a broken script starts: `BUS FAULT` and `CORE DUMP`.
const BUS_FAULT: u8 = 29;
const CORE_DUMP: u8 = 30;
/// Events the program and the pilot post when they take over, and the hold order's.
pub const EVENT_PROGRAM: u8 = 0xa;
pub const EVENT_PILOT: u8 = 0xb;
const EVENT_HOLD: u8 = 5;

/// The stagger's next wait (`order_set_program`): down 10, up 49 more when that runs
/// out.
fn stagger(world: &mut World) -> u16 {
    loop {
        let (s, borrow) = world.stagger.overflowing_sub(10);
        world.stagger = s;
        if !borrow && s != 0 {
            return u16::from(s);
        }
        world.stagger = s.wrapping_add(59);
    }
}

/// Gives robot `id` program `program` (`order_set_program`): given and running, its
/// script from the start after a staggered wait, flags clear and the stack empty.
pub fn set_program(
    world: &mut World,
    id: EntityId,
    program: u8,
    tables: &Tables,
    events: &mut Vec<Event>,
) {
    let program = program.min(tables.scripts.programs.len() as u8 - 1);
    if let Some(change) = world.orders.set_program(id.0, program) {
        events.push(Event::Program(change));
    }
    restart(world, id, program, tables);
}

/// The script part of [`set_program`].
fn restart(world: &mut World, id: EntityId, program: u8, tables: &Tables) {
    let wait = stagger(world);
    if let Some(m) = movement(world, id) {
        m.script = Script {
            pointer: tables.scripts.program(program),
            wait,
            sp: EMPTY,
            events: m.script.events,
            ..Script::default()
        };
    }
}

/// The program (`true`) or the pilot takes robot `id` over (`order_switch_to_*`):
/// with a change, the turn keys are released (to the program) and its event posted.
pub fn switch(world: &mut World, id: EntityId, program: bool, events: &mut Vec<Event>) {
    let Some(change) = world.orders.set_program_drives(id.0, program) else {
        return;
    };
    events.push(Event::Program(change));
    if let Some(m) = movement(world, id) {
        if program {
            m.mode &= !0x30;
        }
        post(m, if program { EVENT_PROGRAM } else { EVENT_PILOT });
    }
}

/// `order_post_event`: event `n` waits for the script.
pub fn post(m: &mut Movement, n: u8) {
    m.script.events |= 1 << n;
}

fn program_drives(world: &World, id: EntityId) -> bool {
    world
        .orders
        .get(id.0)
        .is_some_and(|r| r.orders.program_drives)
}

fn running(world: &World, id: EntityId) -> u8 {
    world.orders.get(id.0).map_or(0, |r| r.orders.program)
}

/// Robot `id`'s script for this tick (`order_step_script`): an event's handler
/// started, or the next opcode run once the wait is over.
pub fn step(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some((_, _, m)) = robot(world, id) else {
        return;
    };
    if world.orders.get(id.0).is_none() {
        return;
    }
    if m.script.flags & BUSY == 0 {
        if m.script.events != 0 {
            dispatch(world, id, tables);
            return;
        }
        if !program_drives(world, id) {
            if world.ticks & 0x3f == 0 {
                check_object_goal(world, id);
            }
            return;
        }
    }
    let Some(m) = movement(world, id) else {
        return;
    };
    let wait = m.script.wait;
    m.script.wait = wait.wrapping_sub(1);
    if wait != 0 {
        return;
    }
    let broken = match m.script.sp {
        sp if sp > EMPTY => Some(CORE_DUMP),
        _ => None,
    };
    let instruction = tables.scripts.decode(m.script.pointer);
    let fault = broken.or(instruction.is_err().then_some(BUS_FAULT));
    if let Some(program) = fault {
        set_program(world, id, program, tables, events);
        if let Some(m) = movement(world, id) {
            m.script.flags |= BUSY;
        }
        return;
    }
    let i = instruction.expect("decoded");
    if let Some(m) = movement(world, id) {
        m.script.pointer = i.next;
        m.script.wait = tables.scripts.times[usize::from(i.op)];
    }
    run(world, id, &i, tables, random, events);
    check_object_goal(world, id);
}

/// Starts the handler of the highest event waiting: the running program's own
/// while the program drives, else the program table's default, or the pilot table's
/// while the pilot drives.
fn dispatch(world: &mut World, id: EntityId, tables: &Tables) {
    let drives = program_drives(world, id);
    let program = running(world, id);
    let Some(m) = movement(world, id) else {
        return;
    };
    let from = m.script.pointer;
    push_return(m, from, true, tables);
    m.script.flags |= BUSY;
    m.script.wait = 0;
    let n = 15 - m.script.events.leading_zeros() as u8;
    m.script.events &= !(1 << n);
    let scripts = &tables.scripts;
    let table = if drives {
        &scripts.program_events
    } else {
        &scripts.pilot_events
    };
    let header = drives.then(|| scripts.header_handler(program, n)).flatten();
    // Past a table the original reads other memory; 0xffff faults the next step.
    m.script.pointer = header
        .or(table.get(usize::from(n)).copied())
        .unwrap_or(0xffff);
}

fn push(m: &mut Movement, bytes: &[u8]) {
    let s = &mut m.script;
    s.sp = s.sp.wrapping_sub(bytes.len() as u8);
    // Out of range the original writes past the stack; the next step faults.
    if let Some(slot) = s
        .stack
        .get_mut(usize::from(s.sp)..usize::from(s.sp) + bytes.len())
    {
        slot.copy_from_slice(bytes);
    }
}

fn pop(m: &mut Movement, n: u8) -> u16 {
    let s = &mut m.script;
    let at = |i: u8| {
        s.stack
            .get(usize::from(s.sp.wrapping_add(i)))
            .copied()
            .unwrap_or(0)
    };
    let value = u16::from_le_bytes([at(0), if n == 2 { at(1) } else { 0 }]);
    s.sp = s.sp.wrapping_add(n);
    value
}

fn top(m: &Movement, word: bool) -> u16 {
    let at = |i: u8| {
        let i = usize::from(m.script.sp.wrapping_add(i));
        m.script.stack.get(i).copied().unwrap_or(0)
    };
    u16::from_le_bytes([at(0), if word { at(1) } else { 0 }])
}

/// A return address as the original pushes it: its offset, with the flags' carry,
/// busy, zero and sign bits in the top nibble.
// shortcut: the offset is from the script area, not the program table (0x40 lower in
// stock), which only a script reading its own stack would see; none does.
fn push_return(m: &mut Movement, to: u16, flags: bool, tables: &Tables) {
    let mut word = to.wrapping_sub(tables.scripts.start);
    if flags {
        word |= u16::from(m.script.flags.rotate_right(1) & 0xf0) << 8;
    }
    push(m, &word.to_le_bytes());
}

fn pop_return(m: &mut Movement, flags: bool, tables: &Tables) {
    let word = pop(m, 2);
    if flags {
        m.script.pointer = (word & 0x0fff).wrapping_add(tables.scripts.start);
        m.script.flags = ((word >> 8) as u8 & 0xf0).rotate_left(1);
    } else {
        m.script.pointer = word.wrapping_add(tables.scripts.start);
    }
}

/// The carry of a routine that only sets or clears it.
fn set_carry(world: &mut World, id: EntityId, carry: bool) {
    if let Some(m) = movement(world, id) {
        m.script.flags = (m.script.flags & !CARRY) | u8::from(carry);
    }
}

fn set_flags(m: &mut Movement, carry: bool, result: u16, word: bool) {
    let sign = if word { result & 0x8000 } else { result & 0x80 } != 0;
    let mut f = m.script.flags & 0x3e;
    f |= if carry { CARRY } else { 0 };
    f |= if result == 0 { ZERO } else { 0 };
    f |= if sign { SIGN } else { 0 };
    m.script.flags = f;
}

/// A robot's byte at +`at`, for the field opcodes: the fields the stock scripts touch.
// shortcut: other offsets read 0 and ignore writes; add fields as scripts need them.
fn field(world: &World, id: EntityId, at: u8) -> u8 {
    let Some((kind, _, m)) = robot(world, id) else {
        return 0;
    };
    let lo = |w: u16| (w & 0xff) as u8;
    let hi = |w: u16| (w >> 8) as u8;
    match at {
        0x00 => RobotKind::of(kind).map_or(0, |k| k.index() as u8),
        0x23 => m.mode | if program_drives(world, id) { 0x80 } else { 0 },
        0x26 => lo(m.speed as u16),
        0x27 => hi(m.speed as u16),
        0x28 => lo(m.target_speed as u16),
        0x29 => hi(m.target_speed as u16),
        0x2a => lo(m.target_heading),
        0x2b => hi(m.target_heading),
        0x2c => m.path,
        0x2d => m.path_point,
        0x32 => m.goal,
        // shortcut: +0xe6 holds a pointer in the original, which the scripts only test
        // against 0; the port gives the target's slot plus one (under 0x100).
        0xe6 => m.target.map_or(0, |t| (t.0 + 1) as u8),
        _ => 0,
    }
}

fn set_field(world: &mut World, id: EntityId, at: u8, v: u8) {
    if at == 0x23 {
        world.orders.set_program_drives(id.0, v & 0x80 != 0);
    }
    let Some(m) = movement(world, id) else {
        return;
    };
    let set_lo = |w: u16| (w & 0xff00) | u16::from(v);
    let set_hi = |w: u16| (w & 0xff) | u16::from(v) << 8;
    match at {
        0x23 => m.mode = v & 0x7f,
        0x26 => m.speed = set_lo(m.speed as u16) as i16,
        0x27 => m.speed = set_hi(m.speed as u16) as i16,
        0x28 => m.target_speed = set_lo(m.target_speed as u16) as i16,
        0x29 => m.target_speed = set_hi(m.target_speed as u16) as i16,
        0x2a => m.target_heading = set_lo(m.target_heading),
        0x2b => m.target_heading = set_hi(m.target_heading),
        0x2c => m.path = v,
        0x2d => m.path_point = v,
        0x32 => m.goal = v,
        _ => {}
    }
}

fn read(world: &World, id: EntityId, at: u8, word: bool) -> u16 {
    let hi = if word {
        field(world, id, at.wrapping_add(1))
    } else {
        0
    };
    u16::from_le_bytes([field(world, id, at), hi])
}

fn write(world: &mut World, id: EntityId, at: u8, value: u16, word: bool) {
    set_field(world, id, at, value as u8);
    if word {
        set_field(world, id, at.wrapping_add(1), (value >> 8) as u8);
    }
}

/// Opcodes 0x2a to 0x41: add, subtract, compare, test, or and and, four each, on a
/// field (the operand names one) or the top of the stack, a byte or a word.
fn arithmetic(world: &mut World, id: EntityId, i: &Instruction) {
    let operation = (i.op - 0x2a) / 4;
    let (on_field, at, value, word) = match i.operand {
        Operand::Bytes(f, v) => (true, f, u16::from(v), false),
        Operand::ByteWord(f, v) => (true, f, v, true),
        Operand::Byte(v) => (false, 0, u16::from(v), false),
        Operand::Word(v) => (false, 0, v, true),
        _ => return,
    };
    let Some(m) = movement(world, id) else {
        return;
    };
    let a = if on_field {
        read(world, id, at, word)
    } else {
        top(m, word)
    };
    let mask = if word { 0xffff } else { 0xff };
    let (result, carry) = match operation {
        0 => (
            u32::from(a) + u32::from(value),
            u32::from(a) + u32::from(value) > mask,
        ),
        1 | 2 => (u32::from(a.wrapping_sub(value)), a < value),
        4 => (u32::from(a | value), false),
        _ => (u32::from(a & value), false),
    };
    let result = (result & mask) as u16;
    if let Some(m) = movement(world, id) {
        set_flags(m, carry, result, word);
    }
    if matches!(operation, 2 | 3) {
        return;
    }
    if on_field {
        write(world, id, at, result, word);
    } else if let Some(m) = movement(world, id) {
        let sp = usize::from(m.script.sp);
        let bytes = result.to_le_bytes();
        let n = if word { 2 } else { 1 };
        if let Some(slot) = m.script.stack.get_mut(sp..sp + n) {
            slot.copy_from_slice(&bytes[..n]);
        }
    }
}

/// Robot `id`'s target's position, if it has one.
fn target(world: &World, id: EntityId) -> Option<Position> {
    let (_, _, m) = robot(world, id)?;
    world.get(m.target?).map(|t| t.position)
}

/// The object in `slot` (`1000:f691`): none for 0xffff or an empty slot.
fn object(world: &World, slot: u16) -> Option<Position> {
    (slot != 0xffff)
        .then(|| world.get(EntityId(slot)))
        .flatten()
        .map(|o| o.position)
}

/// What robot `id` measures its distance to (`order_push_goal_distance`): by +0x32,
/// the worked-out point (bit 4), its target (bit 3), its destination (bit 1), its
/// goal object (bit 0), its destination again (bit 2), else its own base.
fn goal_distance(world: &World, id: EntityId) -> Option<Position> {
    let (_, side, m) = robot(world, id)?;
    match m.goal {
        g if g & 0x10 != 0 => Some(world.point),
        g if g & 0x08 != 0 => target(world, id),
        g if g & 0x02 != 0 => Some(m.destination),
        g if g & 0x01 != 0 => object(world, m.goal_object),
        g if g & 0x04 != 0 => Some(m.destination),
        _ => base_position(world, side),
    }
}

/// What robot `id` steers to (`order_steer_to_goal`): as [`goal_distance`], but bit 2
/// is its own base and no bit is nothing.
fn goal(world: &World, id: EntityId) -> Option<Position> {
    let (_, side, m) = robot(world, id)?;
    match m.goal {
        g if g & 0x1b != 0 => goal_distance(world, id),
        g if g & 0x04 != 0 => base_position(world, side),
        _ => None,
    }
}

/// The step's object check (`1000:48f2`): an object goal with no object (0xffff) is
/// dropped.
fn check_object_goal(world: &mut World, id: EntityId) {
    if let Some(m) = movement(world, id)
        && m.goal & 0x01 != 0
        && m.goal_object == 0xffff
    {
        m.goal = 0;
    }
}

/// The distance as the scripts see it: 8.8, 0xffff from 256 units or with no goal.
fn distance(world: &World, id: EntityId, to: Option<Position>) -> u16 {
    let (Some(e), Some(to)) = (world.get(id), to) else {
        return 0xffff;
    };
    let d = approx_distance(e.position, to).0 as u32;
    if d >> 24 != 0 {
        0xffff
    } else {
        (d >> 8) as u16
    }
}

/// The bearing from `from` to `to` (`sim_get_bearing` through
/// `sim_get_bearing_coarse`): 65,536 to a turn, 0 towards +y, a quarter turn +x.
pub fn bearing(atan: &[u16], from: Position, to: Position) -> u16 {
    let mut x = to.x.0.wrapping_sub(from.x.0) as u32;
    let mut y = to.y.0.wrapping_sub(from.y.0) as u32;
    let mut octants = 0u16;
    if (x as i32) < 0 {
        octants = 2;
        (x, y) = (x.wrapping_neg(), y.wrapping_neg());
    }
    if (y as i32) < 0 {
        octants += 1;
        (x, y) = (y.wrapping_neg(), x);
    }
    octants <<= 1;
    // Past the diagonal, measure from it: (x - y) / (x + y), an octant on.
    let (mut num, mut den) = (x, y);
    while let Some(rest) = num.checked_sub(den) {
        octants += 1;
        den = den.wrapping_mul(2).wrapping_add(rest);
        if den == 0 {
            return 0;
        }
        num = rest;
    }
    while den >> 24 == 0 {
        (num, den) = (num << 8, den << 8);
    }
    while den >> 31 == 0 {
        (num, den) = (num << 1, den << 1);
    }
    let (top, mut high, mut low) = (den >> 16, num >> 16, num & 0xffff);
    if high >= top {
        (high, low) = (high - 1, 0xffff);
    }
    let ratio = (high << 16 | low) / top;
    let a = atan.get((ratio >> 8) as usize).copied().unwrap_or(0);
    ((octants & 7) << 13 | a >> 3).wrapping_add((a >> 2) & 1)
}

fn steer(world: &mut World, id: EntityId, tables: &Tables, offset: impl FnOnce(u16) -> u16) {
    let (Some(to), Some(e)) = (goal(world, id), world.get(id)) else {
        return;
    };
    let heading = offset(bearing(&tables.atan, e.position, to));
    if let Some(m) = movement(world, id) {
        m.target_heading = heading;
    }
}

fn run(
    world: &mut World,
    id: EntityId,
    i: &Instruction,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) {
    let Some(e) = world.get(id) else {
        return;
    };
    let (heading, position, kind) = (e.heading, e.position, e.kind);
    let (drives, program) = (program_drives(world, id), running(world, id));
    let Some(m) = movement(world, id) else {
        return;
    };
    let jump = |m: &mut Movement, taken: bool| {
        if taken && let Some(to) = i.target() {
            m.script.pointer = to;
        }
    };
    let f = m.script.flags;
    let (carry, zero, sign) = (f & CARRY != 0, f & ZERO != 0, f & SIGN != 0);
    match (i.op, &i.operand) {
        // Nothing, or only their time: names, headers, waits, debug counters.
        (0x00..=0x02 | 0x0f..=0x12 | 0x14 | 0x16 | 0x18 | 0x85..=0x87, _) => {}
        (0x03, _) => {
            set_program(world, id, program, tables, events);
            let wait = (random.word() & 0x1f) + 0x1e;
            if let Some(m) = movement(world, id) {
                m.script.wait = wait;
            }
        }
        (0x04 | 0x42, _) => jump(m, true),
        (0x05 | 0x08, &Operand::Word(to)) => {
            push_return(m, i.next, i.op == 0x05, tables);
            m.script.pointer = to;
            if i.op == 0x05 {
                m.script.wait = 1;
            }
        }
        (0x06 | 0x09, _) => pop_return(m, i.op == 0x06, tables),
        (0x07, _) => {
            m.script.sp = m.script.sp.wrapping_add(2);
            m.script.wait = 1;
        }
        (0x0a, _) => m.script.pointer = i.at,
        (0x0b, _) => m.script.flags |= CARRY,
        (0x0c, _) => m.script.flags &= !CARRY,
        (0x0d, _) => m.script.flags |= BUSY,
        (0x0e, _) => m.script.flags &= !BUSY,
        (0x13, &Operand::Byte(n)) => m.script.wait = u16::from(n),
        (0x15, &Operand::Byte(n)) => m.script.wait = u16::from(n) * 60,
        (0x17, &Operand::Byte(n)) => m.script.wait = (u32::from(n) * 3600).min(0xffff) as u16,
        (0x19, _) => m.script.wait = (random.word() & 0xff) + 0xf0,
        (0x1a, _) => m.script.wait = (random.word() & 0x7f) + 0x1e,
        (0x1b, _) => {
            let r = random.word();
            set_flags(m, r & 0x8000 != 0, r << 1, true);
        }
        (0x1c | 0x1d, _) => {
            let r = random.word() & if i.op == 0x1c { 3 } else { 7 };
            set_flags(m, r < 1, r.wrapping_sub(1), true);
        }
        (0x1e | 0x1f, _) => switch(world, id, i.op == 0x1e, events),
        (0x20 | 0x21, _) => jump(m, drives == (i.op == 0x20)),
        (0x22, &Operand::Byte(state)) => {
            push(m, &[program]);
            if let Some(change) = world.orders.enter_state(id.0, state) {
                events.push(Event::Program(change));
            }
        }
        (0x23, _) => {
            let state = pop(m, 1) as u8;
            if let Some(change) = world.orders.enter_state(id.0, state) {
                events.push(Event::Program(change));
            }
        }
        (0x24 | 0x25, &Operand::Byte(at)) => {
            let word = i.op == 0x25;
            let value = read(world, id, at, word).to_le_bytes();
            if let Some(m) = movement(world, id) {
                push(m, &value[..1 + usize::from(word)]);
            }
        }
        (0x26, _) => m.script.sp = m.script.sp.wrapping_add(1),
        (0x27, _) => m.script.sp = m.script.sp.wrapping_add(2),
        (0x28 | 0x29, &Operand::Byte(at)) => {
            let word = i.op == 0x29;
            let value = pop(m, 1 + u8::from(word));
            write(world, id, at, value, word);
        }
        (0x2a..=0x41, _) => arithmetic(world, id, i),
        (0x43..=0x4c, _) => {
            let taken = match i.op {
                0x43 | 0x4b => carry,
                0x44 | 0x4c => !carry,
                0x45 => carry || zero,
                0x46 => !carry && !zero,
                0x47 => zero,
                0x48 => !zero,
                0x49 => sign,
                _ => !sign,
            };
            jump(m, taken);
        }
        (0x4d, &Operand::Bytes(at, v)) => write(world, id, at, u16::from(v), false),
        (0x4e, &Operand::ByteWord(at, v)) => write(world, id, at, v, true),
        (0x4f, &Operand::Word(turn)) => m.target_heading = heading.wrapping_add(turn),
        (0x50, _) => m.target_heading = heading,
        // The interpreter keeps the script's flags: the routines' carry is lost.
        (0x51 | 0x52, _) => _ = shift(tables, kind, m, i.op == 0x51),
        (0x53, _) => {
            m.target_speed = 0;
            match_gear(tables, kind, m);
        }
        (0x54..=0x58, _) => {
            if let Some(k) = RobotKind::of(kind) {
                m.target_speed = tables.script_speeds[usize::from(i.op - 0x54)][k.index()];
                // The routines set their gear by number; the bounce routine finds the
                // same one, as each script speed is a gear's.
                match_gear(tables, kind, m);
            }
        }
        (0x59..=0x5c, _) => {
            let taken = match i.op {
                0x59 => m.target_speed == m.speed,
                0x5a => m.target_heading == heading,
                0x5b => m.target_speed != m.speed,
                _ => m.target_heading != heading,
            };
            jump(m, taken);
        }
        (0x5d, &Operand::Word(w)) => {
            let d = distance(world, id, goal_distance(world, id));
            if let Some(m) = movement(world, id) {
                set_flags(m, d < w, d.wrapping_sub(w), true);
            }
        }
        (0x5e, _) => {
            let d = distance(world, id, goal_distance(world, id));
            if let Some(m) = movement(world, id) {
                push(m, &d.to_le_bytes());
            }
        }
        (0x5f, _) => steer(world, id, tables, |b| b),
        (0x60 | 0x61, &Operand::Word(w)) => {
            if m.goal & 0x02 != 0 {
                let add = i.op == 0x60;
                steer(world, id, tables, |b| {
                    if add {
                        b.wrapping_add(w)
                    } else {
                        b.wrapping_sub(w)
                    }
                });
            }
        }
        (0x62, _) => {
            let turn = (random.word() & 0x3fff) + 0x4000;
            let to = (m.goal & 0x02 != 0).then(|| goal(world, id)).flatten();
            let side = to.map(|to| bearing(&tables.atan, position, to).wrapping_sub(heading));
            if let Some(m) = movement(world, id) {
                m.target_heading = match side {
                    Some(d) if (d as i16) >= 0 => m.target_heading.wrapping_add(turn),
                    Some(_) => m.target_heading.wrapping_sub(turn),
                    None => m.target_heading.wrapping_add(turn.wrapping_sub(0x6000)),
                };
            }
        }
        (0x63, _) => {
            let found = crate::sense::steer_to_nearest(world, id, tables);
            set_carry(world, id, !found);
        }
        (0x64, _) => {
            let turn = (random.word() & 0x7fff).wrapping_sub(0x4000);
            m.target_heading = m.target_heading.wrapping_add(turn);
        }
        (0x65, _) => {
            m.target_heading = heading;
            m.target_speed = 0;
        }
        (0x66, _) => {
            let (slot, offset) = (m.goal_object, m.destination);
            // shortcut: an object goal on an empty slot reads the original's null
            // object; the port takes it as none (every word 0xffff).
            world.point = object(world, slot).map_or(
                Position {
                    x: Fixed(-1),
                    y: Fixed(-1),
                },
                |at| Position {
                    x: Fixed(at.x.0.wrapping_add(offset.x.0)),
                    y: Fixed(at.y.0.wrapping_add(offset.y.0)),
                },
            );
        }
        (0x67, &Operand::Word(w)) => {
            let d = approx_distance(position, world.point).0 as u32;
            // Past 256 units the flags are those of the whole units less 1.
            let (a, b) = match d >> 24 {
                0 => ((d >> 8) as u16, w),
                _ => ((d >> 16) as u16, 1),
            };
            if let Some(m) = movement(world, id) {
                set_flags(m, a < b, a.wrapping_sub(b), true);
            }
        }
        (0x68, _) => {
            let heading = bearing(&tables.atan, position, world.point);
            if let Some(m) = movement(world, id) {
                m.target_heading = heading;
            }
        }
        (0x6d, _) => {
            let d = distance(world, id, Some(world.point));
            if let Some(m) = movement(world, id) {
                push(m, &d.to_le_bytes());
            }
        }
        (0x6e, _) => {
            let goal = std::mem::replace(&mut m.goal, 0x10);
            steer(world, id, tables, |b| b);
            if let Some(m) = movement(world, id) {
                m.goal = goal;
            }
        }
        (0x69, _) => {
            let side = world.get(id).map(|e| e.side);
            let d = distance(world, id, side.and_then(|s| base_position(world, s)));
            if let Some(m) = movement(world, id) {
                push(m, &d.to_le_bytes());
            }
        }
        (0x6a, _) => {
            let goal = std::mem::replace(&mut m.goal, 0x04);
            steer(world, id, tables, |b| b);
            if let Some(m) = movement(world, id) {
                m.goal = goal;
            }
        }
        (0x6b, _) => {
            let d = distance(world, id, target(world, id));
            if let Some(m) = movement(world, id) {
                push(m, &d.to_le_bytes());
            }
        }
        (0x6c, _) => {
            let goal = std::mem::replace(&mut m.goal, 0x08);
            let found = target(world, id).is_some();
            steer(world, id, tables, |b| b);
            if let Some(m) = movement(world, id) {
                m.goal = goal;
            }
            set_carry(world, id, !found);
        }
        (0x6f, _) => {
            m.destination = position;
            m.goal = 0x02;
            post(m, EVENT_HOLD);
        }
        (0x70, _) => join_nearest_path_point(world, id),
        (0x71, _) => {
            let found = crate::sense::look(world, id, tables, events);
            set_carry(world, id, !found);
        }
        (0x72, _) => {
            let damaged = matches!(world.get(id).map(|e| e.damage),
                Some(crate::world::Damage::Robot(c)) if c.targeting & 1 != 0);
            if let Some(m) = movement(world, id) {
                set_flags(m, damaged, u16::from(!damaged).wrapping_neg(), false);
            }
        }
        (0x73, _) => {
            let base = robot(world, id)
                .and_then(|(_, _, m)| world.get(m.target?))
                .is_some_and(|t| t.kind == xeno_formats::usg::ObjectKind::Base);
            set_carry(world, id, !base);
        }
        (0x74, &Operand::Word(text)) => m.panel.post(text),
        // shortcut: the base's message log (0x75, and 0x76's too) and 0x76's copy to
        // every robot of the side are T-129's; no stock script uses 0x76.
        (0x75 | 0x76, _) => {}
        (0x77, &Operand::Byte(command)) => match command {
            4 => m.path |= Movement::NEXT,
            8 => m.path |= 0x10,
            10 => {
                m.path_point = m.path_point.wrapping_sub(1);
                head_for_path_point(world, id, tables, random, events);
            }
            _ => {}
        },
        (0x78, _) => {
            let usable = (0..2).any(|w| {
                let bits = crate::sense::weapon_state(world, id, w, tables) as u8;
                bits & 0x01 != 0 && bits & 0x60 == 0
            });
            set_carry(world, id, !usable);
        }
        (0x79 | 0x7a, _) => {
            let state = crate::sense::weapon_state(world, id, usize::from(i.op - 0x79), tables);
            if let Some(m) = movement(world, id) {
                push(m, &state.to_le_bytes());
            }
        }
        (0x7b | 0x7c, _) => {
            let weapons = match world.get(id).map(|e| e.damage) {
                Some(crate::world::Damage::Robot(c)) => c.weapons,
                _ => 0,
            };
            if let Some(m) = movement(world, id) {
                crate::motion::select_weapon(tables, kind, weapons, id, m, i.op - 0x7b, events);
            }
        }
        // shortcut: firing (the fire request and the kinds' trigger routines) is
        // T-114's; until then the script goes on as if it fired.
        (0x7d, _) => {}
        (0x84, &Operand::Byte(bits)) => {
            let result = u16::from(m.goal & bits);
            set_flags(m, false, result, false);
        }
        // shortcut: the relay net (T-113) is not ported: the script stays here.
        _ => m.script.pointer = i.at,
    }
}

/// Starts every robot of `scenario` on its program given, in the file's order, as
/// the loader does.
pub fn start_all(world: &mut World, scenario: &xeno_formats::usg::Scenario, tables: &Tables) {
    for o in scenario.objects.iter().flatten() {
        let id = EntityId(o.slot);
        if let Some(r) = world.orders.get(o.slot) {
            restart(world, id, r.orders.program, tables);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
