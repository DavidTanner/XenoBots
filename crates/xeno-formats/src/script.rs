//! The robots' scripts (`docs/sim.md`, "Orders"): the bytecode the 31 programs are
//! written in, each opcode's time, and the handlers events start. Game data, read from
//! the user's `xeno.exe` at run time through the instructions of the interpreter
//! (`order_step_script`, `1000:4899`) that use it, after checking their bytes.
//!
//! A script is a run of opcodes, each followed by its operand ([`operand_kind`]). The
//! interpreter steps one opcode at a time and then waits that opcode's time
//! ([`Scripts::times`]) in ticks.

use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};
use crate::{FormatError, orders, sequence};

/// `call [bx + handlers]` and `mov ax, [bx + times]` in `order_step_script`: four
/// bytes an opcode, a handler and a time.
pub(crate) const HANDLERS: (usize, &[u8]) = (0x48ee, &[0xff, 0x97]);
pub(crate) const TIMES: (usize, &[u8]) = (0x48e7, &[0x8b, 0x87]);
/// `cmp si, end` and `cmp si, start`: the script pointer's bounds.
pub(crate) const END: (usize, &[u8]) = (0x48cf, &[0x81, 0xfe]);
pub(crate) const START: (usize, &[u8]) = (0x48d5, &[0x81, 0xfe]);
/// `mov si, table`: the handlers for events while the pilot drives, and while the
/// program drives but its header has none.
pub(crate) const PILOT_EVENTS: (usize, &[u8]) = (0x494e, &[0xbe]);
pub(crate) const PROGRAM_EVENTS: (usize, &[u8]) = (0x4977, &[0xbe]);

/// The header opcode: items of an event and a handler up to [`NAME`].
pub const HEADER: u8 = 0x0f;
pub const NAME: u8 = 0x11;
/// An event table ends at this word; no more than 16 events (a word of bits).
const TABLE_END: u16 = 0xffff;
const MAX_EVENTS: usize = 16;

/// The shape of an opcode's operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    None,
    Byte,
    Word,
    /// Two bytes: a field offset and a byte value.
    Bytes,
    /// A field offset byte and a word value.
    ByteWord,
    /// [`HEADER`]'s items up to [`NAME`].
    Header,
    /// 0x02: a byte (the program's flags), a length and that many bytes.
    Text,
}

/// The operand each opcode takes (`docs/sim.md`, "The opcodes").
pub fn operand_kind(op: u8) -> OperandKind {
    use OperandKind::*;
    match op {
        0x02 => Text,
        HEADER => Header,
        0x04
        | 0x05
        | 0x08
        | 0x20
        | 0x21
        | 0x2d
        | 0x2f
        | 0x33
        | 0x39
        | 0x3d
        | 0x3f
        | 0x4f
        | 0x59..=0x5d
        | 0x60
        | 0x61
        | 0x67
        | 0x74..=0x76 => Word,
        0x2a | 0x30 | 0x34 | 0x36 | 0x3a | 0x40 | 0x4d => Bytes,
        0x2b | 0x31 | 0x35 | 0x37 | 0x3b | 0x41 | 0x4e => ByteWord,
        0x13
        | 0x15
        | 0x17
        | 0x22
        | 0x24
        | 0x25
        | 0x28
        | 0x29
        | 0x2c
        | 0x2e
        | 0x32
        | 0x38
        | 0x3c
        | 0x3e
        | 0x42..=0x4c
        | 0x77
        | 0x84 => Byte,
        _ => None,
    }
}

/// A decoded operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    None,
    Byte(u8),
    Word(u16),
    Bytes(u8, u8),
    ByteWord(u8, u16),
    /// Each item: an event number and the handler's address.
    Header(Vec<(u8, u16)>),
    Text {
        flags: u8,
        text: Vec<u8>,
    },
}

/// One opcode and its operand, at a data-segment offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub at: u16,
    pub op: u8,
    pub operand: Operand,
    /// Where the next instruction starts.
    pub next: u16,
}

impl Instruction {
    /// Where a jump or call goes: relative to [`Instruction::next`] for jumps, an
    /// address for the calls 0x05 and 0x08.
    pub fn target(&self) -> Option<u16> {
        match (self.op, &self.operand) {
            (0x05 | 0x08, &Operand::Word(w)) => Some(w),
            (0x04 | 0x20 | 0x21 | 0x59..=0x5c, &Operand::Word(w)) => {
                Some(self.next.wrapping_add(w))
            }
            (0x42..=0x4c, &Operand::Byte(b)) => Some(self.next.wrapping_add(b as i8 as u16)),
            _ => None,
        }
    }

    /// Whether the next instruction can run after this one: not after a jump, a
    /// return (0x06, 0x09), a halt (0x0a) or a restart (0x03).
    pub fn falls_through(&self) -> bool {
        !matches!(self.op, 0x03 | 0x04 | 0x06 | 0x09 | 0x0a | 0x42)
    }
}

/// The scripts and the interpreter's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scripts {
    /// The data-segment offset of `bytes[0]`; the script pointer stays in
    /// `start..start + bytes.len()`.
    pub start: u16,
    pub bytes: Vec<u8>,
    /// Where each program's script starts, by program number (the program table).
    pub programs: Vec<u16>,
    /// Each opcode's time: the ticks the script waits after it.
    pub times: Vec<u16>,
    /// The handler for event `n` while the program drives and its header has none.
    pub program_events: Vec<u16>,
    /// The handler for event `n` while the pilot drives.
    pub pilot_events: Vec<u16>,
}

impl Scripts {
    pub fn byte(&self, at: u16) -> Option<u8> {
        self.bytes
            .get(usize::from(at.wrapping_sub(self.start)))
            .copied()
    }

    fn word(&self, at: u16) -> Option<u16> {
        Some(u16::from_le_bytes([
            self.byte(at)?,
            self.byte(at.wrapping_add(1))?,
        ]))
    }

    /// Program `number`'s script; numbers past the table start the last
    /// (`order_set_program` keeps them under 31).
    pub fn program(&self, number: u8) -> u16 {
        let last = self.programs.len().saturating_sub(1);
        self.programs[usize::from(number).min(last)]
    }

    /// The handler program `number`'s header gives `event`, if it has one.
    pub fn header_handler(&self, number: u8, event: u8) -> Option<u16> {
        match self.decode(self.program(number)).ok()? {
            Instruction {
                operand: Operand::Header(items),
                ..
            } => items.iter().find(|&&(e, _)| e == event).map(|&(_, h)| h),
            _ => None,
        }
    }

    /// The instruction at `at`.
    pub fn decode(&self, at: u16) -> Result<Instruction, FormatError> {
        let bad = |a: u16| invalid(usize::from(a), "past the script area");
        let byte = |a: u16| self.byte(a).ok_or_else(|| bad(a));
        let word = |a: u16| self.word(a).ok_or_else(|| bad(a));
        let op = byte(at)?;
        let o = at.wrapping_add(1);
        let (operand, next) = match operand_kind(op) {
            OperandKind::None => (Operand::None, o),
            OperandKind::Byte => (Operand::Byte(byte(o)?), o + 1),
            OperandKind::Word => (Operand::Word(word(o)?), o + 2),
            OperandKind::Bytes => (Operand::Bytes(byte(o)?, byte(o + 1)?), o + 2),
            OperandKind::ByteWord => (Operand::ByteWord(byte(o)?, word(o + 1)?), o + 3),
            OperandKind::Header => {
                let mut items = Vec::new();
                let mut a = o;
                while byte(a)? != NAME {
                    items.push((byte(a)?, word(a + 1)?));
                    a += 3;
                }
                (Operand::Header(items), a + 1)
            }
            OperandKind::Text => {
                let length = byte(o + 1)?;
                let text = (0..u16::from(length))
                    .map(|i| byte(o + 2 + i))
                    .collect::<Result<_, _>>()?;
                let flags = byte(o)?;
                (Operand::Text { flags, text }, o + 2 + u16::from(length))
            }
        };
        Ok(Instruction {
            at,
            op,
            operand,
            next,
        })
    }

    /// Every instruction reachable from the programs and the event tables, by
    /// address: each decodes, its opcode has a time, and none overlaps another.
    pub fn walk(&self) -> Result<Vec<Instruction>, FormatError> {
        let mut seen = std::collections::BTreeMap::new();
        let mut todo: Vec<u16> = self.programs.clone();
        todo.extend(self.program_events.iter().chain(&self.pilot_events));
        while let Some(at) = todo.pop() {
            if seen.contains_key(&at) {
                continue;
            }
            let i = self.decode(at)?;
            if usize::from(i.op) >= self.times.len() {
                return Err(invalid(usize::from(at), "an opcode with no handler"));
            }
            if let Operand::Header(items) = &i.operand {
                todo.extend(items.iter().map(|&(_, h)| h));
            }
            todo.extend(i.target());
            if i.falls_through() {
                todo.push(i.next);
            }
            seen.insert(at, i);
        }
        let all: Vec<Instruction> = seen.into_values().collect();
        if let Some(w) = all.windows(2).find(|w| w[0].next > w[1].at) {
            return Err(invalid(usize::from(w[1].at), "instructions overlap"));
        }
        Ok(all)
    }
}

fn events(image: &[u8], at: usize) -> Result<Vec<u16>, FormatError> {
    let mut table = Vec::new();
    loop {
        let w = word(image, at + 2 * table.len())?;
        if w == TABLE_END {
            return Ok(table);
        }
        if table.len() == MAX_EVENTS {
            return Err(invalid(at, "an event table with no end"));
        }
        table.push(w);
    }
}

/// Reads the scripts from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Scripts, FormatError> {
    let op = |(at, op): (usize, &[u8])| operand(image, at, op);
    let handlers = op(HANDLERS)?;
    if op(TIMES)? != handlers.wrapping_add(2) {
        return Err(invalid(TIMES.0, "not the opcode times"));
    }
    // The opcode table runs up to the program table.
    let table = op(orders::TABLE)?;
    let count = usize::from(table.wrapping_sub(handlers));
    if count % 4 != 0 || !(4..=0x400).contains(&count) {
        return Err(invalid(
            HANDLERS.0,
            "the opcode table does not end at the programs",
        ));
    }
    let times = (0..count / 4)
        .map(|n| word(image, ds + usize::from(handlers) + 4 * n + 2))
        .collect::<Result<_, _>>()?;
    let (start, end) = (op(START)?, op(END)?);
    let bytes = image
        .get(ds + usize::from(start)..ds + usize::from(end))
        .filter(|b| !b.is_empty())
        .ok_or_else(|| invalid(START.0, "no script area"))?
        .to_vec();
    // As many programs as `order_find_program_name` allows (`cmp al, count`).
    let count = match image.get(orders::COUNT.0..orders::COUNT.0 + 2) {
        Some(&[op, n]) if op == orders::COUNT.1[0] && n > 0 => usize::from(n),
        _ => return Err(invalid(orders::COUNT.0, "not the program count")),
    };
    let programs = (0..count)
        .map(|n| match word(image, ds + usize::from(table) + 2 * n)? {
            p if (start..end).contains(&p) => Ok(p),
            _ => Err(invalid(usize::from(table), "a program outside the scripts")),
        })
        .collect::<Result<_, _>>()?;
    Ok(Scripts {
        start,
        bytes,
        programs,
        times,
        program_events: events(image, ds + usize::from(op(PROGRAM_EVENTS)?))?,
        pilot_events: events(image, ds + usize::from(op(PILOT_EVENTS)?))?,
    })
}

/// Reads the scripts from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Scripts, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_scripts;

    const DS: usize = 0x20000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x9c00];
        synthetic_scripts(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_scripts_read_and_decode() {
        let scripts = read_image(&synthetic(), DS).unwrap();
        assert_eq!(scripts.times.len(), 0x88);
        assert_eq!(scripts.times[0x14], 60);
        assert_eq!(scripts.programs.len(), 5);
        assert_eq!(scripts.header_handler(1, 9), Some(0x9a76));
        assert_eq!(scripts.header_handler(1, 7), None);
        assert_eq!(scripts.header_handler(2, 9), None, "no header");
        assert_eq!(scripts.header_handler(9, 9), None, "the last program");
        let all = scripts.walk().unwrap();
        // A conditional jump back lands on an instruction.
        let jump = all.iter().find(|i| i.op == 0x43).unwrap();
        assert!(all.iter().any(|i| Some(i.at) == jump.target()));
        insta::assert_debug_snapshot!((scripts, all));
    }

    #[test]
    fn another_build_or_bad_scripts_are_refused() {
        for (at, value) in [
            (HANDLERS.0, 0x90),
            (TIMES.0 + 2, 0x00),
            (START.0 + 1, 0xff),
            (PILOT_EVENTS.0, 0xbf),
            (DS + 0x9b9b, 0x00),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
        // An opcode with no handler, then a jump into the middle of an instruction.
        for (at, value) in [(0x9a6f, 0xee), (0x9a74, 0xfc)] {
            let mut image = synthetic();
            image[DS + at] = value;
            let scripts = read_image(&image, DS).unwrap();
            assert!(scripts.walk().is_err(), "{at:#x}");
        }
    }

    /// The user's scripts: 31 programs, 13 events in each table, every instruction
    /// reachable decodes and the program names are where `xeno_formats::orders`
    /// finds them.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_program_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let scripts = read(&exe).unwrap();
        let names = orders::read(&exe).unwrap();
        assert_eq!(scripts.programs.len(), 31);
        assert_eq!(
            (scripts.program_events.len(), scripts.pilot_events.len()),
            (13, 13)
        );
        let all = scripts.walk().unwrap();
        for (n, &p) in scripts.programs.iter().enumerate() {
            let i = all.iter().position(|i| i.at == p).unwrap();
            // A program starts with its header or its name.
            let text = if all[i].op == HEADER {
                &all[i + 1]
            } else {
                &all[i]
            };
            let Operand::Text { text, .. } = &text.operand else {
                panic!("program {n} has no name");
            };
            assert_eq!(
                &text[..text.len() - 1],
                names.programs[n].name,
                "program {n}"
            );
        }
        assert!(all.len() > 400);
    }
}
