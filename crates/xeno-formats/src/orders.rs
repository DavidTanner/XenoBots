//! A robot's orders (`docs/screens.md`, "Orders"): the program it was given, the one
//! it runs, and whether its pilot drives it by hand. The programs are scripts in
//! `xeno.exe` (a table of 31, each with a name and a byte saying which robot kinds may
//! be given it); a robot keeps the program it was given at +0x2e of its object, the one
//! it runs (that program, or a state it entered by itself) at +0x30, and the manual
//! switch in bit 7 of +0x23, so all three come from a `.usg` file's `DCBS` objects
//! (`docs/formats/usg.md`).
//!
//! The program table is game data (the names are game text), so [`Programs`] is read
//! from the user's own `xeno.exe` at run time, from the instructions that use it after
//! checking their bytes. [`OrderBook`] is the orders of every robot of a scenario: plain
//! data, deterministic, with no I/O, ready for the simulation (T-050) to take over.

use std::collections::BTreeMap;

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::sequence;
use crate::usg::{Object, ObjectKind, Scenario, Side};

/// Load-image offsets of the instructions read, and the bytes before each operand:
/// `mov bx, [bx + table]` in `order_find_program_name` (`1000:47d9`), its `cmp al,
/// count` (`1000:47ca`), and `mov al, [bx + masks]` in the program panel's mouse
/// handler (`1000:d37f`), which indexes the masks by the robot's kind.
pub(crate) const TABLE: (usize, &[u8]) = (0x47d9, &[0x8b, 0x9f]);
pub(crate) const COUNT: (usize, &[u8]) = (0x47ca, &[0x3c]);
pub(crate) const MASKS: (usize, &[u8]) = (0xd37f, &[0x8a, 0x87]);

/// A program's header items are three bytes (an opcode and a word) when it starts with
/// this byte; the name item's opcode is [`NAME_OP`].
const HEADER_LIST: u8 = 0x0f;
const NAME_OP: u8 = 0x11;
/// Flag bit 7: a state the robot enters by itself, never offered to the player
/// (`order_select_next_program` skips it).
pub const NOT_OFFERED: u8 = 0x80;
/// No header list or name is longer than this.
const MAX_HEADER: usize = 64;
const MAX_NAME: usize = 32;
const MAX_PROGRAMS: usize = 64;

/// In-memory object offsets: the program given, the program running and the manual
/// switch (bit 7).
pub const ORDERED_OFFSET: usize = 0x2e;
pub const PROGRAM_OFFSET: usize = 0x30;
pub const MODE_OFFSET: usize = 0x23;
pub const MANUAL: u8 = 0x80;

/// The robot kinds, which index the kind masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RobotKind {
    Humanoid,
    Scorpion,
    Scout,
}

impl RobotKind {
    pub const ALL: [RobotKind; 3] = [Self::Humanoid, Self::Scorpion, Self::Scout];

    /// The robot kind of an object, if it is a robot.
    pub fn of(kind: ObjectKind) -> Option<RobotKind> {
        match kind {
            ObjectKind::Humanoid => Some(Self::Humanoid),
            ObjectKind::Scorpion => Some(Self::Scorpion),
            ObjectKind::Scout => Some(Self::Scout),
            _ => None,
        }
    }

    /// The game's kind number (the object's first word).
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One program as the table describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    /// Bits 0 to 2: the robot kinds (by [`Programs::masks`]) that may be given it;
    /// bit 7 ([`NOT_OFFERED`]).
    pub flags: u8,
    /// Game text, without its NUL.
    pub name: Vec<u8>,
}

/// The program table and the kind masks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Programs {
    /// By program number.
    pub programs: Vec<Program>,
    /// A program is offered to a robot when its flags share a bit with the robot
    /// kind's mask and it is not [`NOT_OFFERED`].
    pub masks: [u8; 3],
}

impl Programs {
    pub fn get(&self, number: u8) -> Option<&Program> {
        self.programs.get(usize::from(number))
    }

    /// The name of program `number`; the game shows the last program for numbers
    /// past the table.
    pub fn name(&self, number: u8) -> &[u8] {
        let last = self.programs.len().saturating_sub(1);
        self.programs
            .get(usize::from(number).min(last))
            .map_or(&[], |p| &p.name)
    }

    /// Whether the player may give program `number` to a robot of `kind`.
    pub fn offered(&self, kind: RobotKind, number: u8) -> bool {
        self.get(number)
            .is_some_and(|p| p.flags & self.masks[kind.index()] != 0 && p.flags & NOT_OFFERED == 0)
    }

    /// The programs offered to `kind`, in order.
    pub fn offered_to(&self, kind: RobotKind) -> impl Iterator<Item = u8> + '_ {
        (0..self.programs.len() as u8).filter(move |&n| self.offered(kind, n))
    }

    /// The next offered program after `current`, wrapping (`order_select_next_program`);
    /// `None` if `kind` is offered none.
    pub fn next(&self, kind: RobotKind, current: u8) -> Option<u8> {
        let count = self.programs.len();
        (1..=count)
            .map(|step| ((usize::from(current) + step) % count) as u8)
            .find(|&n| self.offered(kind, n))
    }

    /// The previous offered program before `current`, wrapping
    /// (`order_select_previous_program`).
    pub fn previous(&self, kind: RobotKind, current: u8) -> Option<u8> {
        let count = self.programs.len();
        let current = usize::from(current).min(count - 1);
        (1..=count)
            .map(|step| ((current + count - step) % count) as u8)
            .find(|&n| self.offered(kind, n))
    }
}

/// A robot's orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orders {
    /// The program it was given (+0x2e).
    pub ordered: u8,
    /// The program it runs (+0x30): the one given, or a state it entered by itself.
    /// The console shows this one, and the next and previous programs are counted from
    /// it.
    pub program: u8,
    /// Its pilot drives it; the program waits.
    pub manual: bool,
}

impl Orders {
    /// Program `program`, just given, in automatic.
    pub fn given(program: u8) -> Orders {
        Orders {
            ordered: program,
            program,
            manual: false,
        }
    }

    /// A robot object's orders (+0x2e, +0x30 and bit 7 of +0x23), if the object is
    /// that big.
    pub fn of(object: &Object) -> Option<Orders> {
        Some(Orders {
            ordered: object.byte_at(ORDERED_OFFSET)?,
            program: object.byte_at(PROGRAM_OFFSET)?,
            manual: object.byte_at(MODE_OFFSET)? & MANUAL != 0,
        })
    }
}

/// A robot in an [`OrderBook`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Robot {
    pub kind: RobotKind,
    pub side: Side,
    pub orders: Orders,
}

/// An order changed: which robot, and its orders before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub slot: u16,
    pub before: Orders,
    pub after: Orders,
}

/// The orders of every robot in a mission, by object slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderBook {
    robots: BTreeMap<u16, Robot>,
}

impl OrderBook {
    /// The robots of `scenario` with their orders as the file sets them. Objects too
    /// small to hold orders are left out.
    pub fn from_scenario(scenario: &Scenario) -> OrderBook {
        let robots = scenario
            .objects
            .iter()
            .flatten()
            .filter_map(|o| {
                let robot = Robot {
                    kind: RobotKind::of(o.kind)?,
                    side: o.side()?,
                    orders: Orders::of(o)?,
                };
                Some((o.slot, robot))
            })
            .collect();
        OrderBook { robots }
    }

    pub fn get(&self, slot: u16) -> Option<&Robot> {
        self.robots.get(&slot)
    }

    /// The robots of `side`, by slot.
    pub fn side(&self, side: Side) -> impl Iterator<Item = (u16, &Robot)> {
        self.robots
            .iter()
            .filter(move |(_, r)| r.side == side)
            .map(|(&slot, r)| (slot, r))
    }

    fn change(&mut self, slot: u16, f: impl FnOnce(&Robot) -> Option<Orders>) -> Option<Change> {
        let robot = self.robots.get_mut(&slot)?;
        let before = robot.orders;
        let after = f(robot)?;
        robot.orders = after;
        (after != before).then_some(Change {
            slot,
            before,
            after,
        })
    }

    /// The next program offered to the robot; choosing a program also hands the robot
    /// back to it (`ui_program_panel_on_mouse`, F4), and gives it both as the program
    /// given and the one running (`order_set_program`).
    pub fn select_next(&mut self, slot: u16, programs: &Programs) -> Option<Change> {
        self.change(slot, |r| {
            let program = programs.next(r.kind, r.orders.program)?;
            Some(Orders::given(program))
        })
    }

    /// The previous program offered to the robot, as [`OrderBook::select_next`].
    pub fn select_previous(&mut self, slot: u16, programs: &Programs) -> Option<Change> {
        self.change(slot, |r| {
            let program = programs.previous(r.kind, r.orders.program)?;
            Some(Orders::given(program))
        })
    }

    /// Hands the robot to its pilot or back to its program (F3, F2).
    pub fn set_manual(&mut self, slot: u16, manual: bool) -> Option<Change> {
        self.change(slot, |r| Some(Orders { manual, ..r.orders }))
    }
}

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// One program's flags and name, from its header at `at` (`order_find_program_name`):
/// either a list of three-byte items up to the name item (opcode [`NAME_OP`], a byte,
/// the flags), then a length byte; or, without the list, a byte, the flags and the
/// length byte. The name follows, NUL-terminated.
fn program(image: &[u8], at: usize) -> Result<Program, FormatError> {
    let byte = |i: usize| {
        image
            .get(i)
            .copied()
            .ok_or_else(|| invalid(i, "past the end of the image"))
    };
    let name_at = if byte(at)? == HEADER_LIST {
        let mut item = at + 1;
        while byte(item)? != NAME_OP {
            item += 3;
            if item - at > MAX_HEADER {
                return Err(invalid(at, "the program has no name"));
            }
        }
        item + 4
    } else {
        at + 3
    };
    let flags = byte(name_at - 2)?;
    let bytes = image
        .get(name_at..(name_at + MAX_NAME).min(image.len()))
        .ok_or_else(|| invalid(name_at, "past the end of the image"))?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid(name_at, "the program's name has no end"))?;
    Ok(Program {
        flags,
        name: bytes[..end].to_vec(),
    })
}

/// Reads the program table from a load image whose data segment starts at `ds`. Error
/// offsets are into the load image.
pub fn read_image(image: &[u8], ds: usize) -> Result<Programs, FormatError> {
    let table = ds + usize::from(operand(image, TABLE.0, TABLE.1)?);
    let count = match image.get(COUNT.0..COUNT.0 + 2) {
        Some(&[op, count]) if op == COUNT.1[0] => usize::from(count),
        _ => return Err(invalid(COUNT.0, "not the program count")),
    };
    if count == 0 || count > MAX_PROGRAMS {
        return Err(invalid(COUNT.0, "not a program count"));
    }
    let programs = (0..count)
        .map(|n| {
            let at = table + 2 * n;
            let offset = operand(image, at, &[])?;
            program(image, ds + usize::from(offset))
        })
        .collect::<Result<_, _>>()?;
    let masks_at = ds + usize::from(operand(image, MASKS.0, MASKS.1)?);
    let masks = image
        .get(masks_at..masks_at + 3)
        .ok_or_else(|| invalid(masks_at, "past the end of the image"))?;
    Ok(Programs {
        programs,
        masks: [masks[0], masks[1], masks[2]],
    })
}

/// Reads the program table from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Programs, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_mission_usg, synthetic_programs};

    const DS: usize = 0x20000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x400];
        synthetic_programs(&mut image, DS);
        image
    }

    #[test]
    fn a_synthetic_program_table_reads() {
        let programs = read_image(&synthetic(), DS).unwrap();
        assert_eq!(programs.programs.len(), 5);
        assert_eq!(programs.name(1), b"ONE");
        // A header without a list.
        assert_eq!(programs.name(2), b"TWO");
        // Past the table, the game shows the last one.
        assert_eq!(programs.name(200), b"FOUR");
        insta::assert_debug_snapshot!(programs);
    }

    #[test]
    fn each_kind_is_offered_its_own_programs() {
        let programs = read_image(&synthetic(), DS).unwrap();
        let offered = |kind| programs.offered_to(kind).collect::<Vec<_>>();
        // 0 is never offered; 3 only to the scorpion; 4 to nobody.
        assert_eq!(offered(RobotKind::Humanoid), [1, 2]);
        assert_eq!(offered(RobotKind::Scorpion), [1, 2, 3]);
        assert_eq!(offered(RobotKind::Scout), [1, 2]);
        // Next and previous wrap and skip.
        assert_eq!(programs.next(RobotKind::Humanoid, 2), Some(1));
        assert_eq!(programs.next(RobotKind::Scorpion, 2), Some(3));
        assert_eq!(programs.previous(RobotKind::Humanoid, 1), Some(2));
        assert_eq!(programs.previous(RobotKind::Scorpion, 1), Some(3));
        assert_eq!(programs.next(RobotKind::Humanoid, 0), Some(1));
        assert_eq!(programs.previous(RobotKind::Scout, 30), Some(2));
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let mut image = synthetic();
            image[at] = value;
            read_image(&image, DS)
        };
        assert!(corrupt(TABLE.0, 0x90).is_err());
        assert!(corrupt(COUNT.0, 0x90).is_err());
        assert!(corrupt(COUNT.0 + 1, 0).is_err());
        assert!(corrupt(MASKS.0 + 1, 0x90).is_err());
        // A name that never ends.
        let mut image = synthetic();
        image[DS + 0x100..DS + 0x200].fill(b'X');
        assert!(read_image(&image, DS).is_err());
        assert!(read_image(&synthetic()[..0x5000], DS).is_err());
    }

    #[test]
    fn orders_are_set_and_kept_per_robot() {
        let programs = read_image(&synthetic(), DS).unwrap();
        let scenario = crate::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut book = OrderBook::from_scenario(&scenario);
        // Slots 2 (humanoid, program 1, manual), 5 (scorpion, program 2) and 7 (scout,
        // alien); the base and the relay are not robots.
        let slots: Vec<_> = book.side(Side::Human).map(|(s, _)| s).collect();
        assert_eq!(slots, [2, 5]);
        assert_eq!(
            book.get(2).unwrap().orders,
            Orders {
                ordered: 1,
                program: 1,
                manual: true
            }
        );
        // Choosing a program hands the robot back to it.
        let change = book.select_next(2, &programs).unwrap();
        assert_eq!(change.after, Orders::given(2));
        assert_eq!(book.select_next(5, &programs).unwrap().after.program, 3);
        assert_eq!(book.select_previous(5, &programs).unwrap().after.program, 2);
        assert!(book.set_manual(5, true).unwrap().after.manual);
        assert_eq!(book.set_manual(5, true), None, "no change, no report");
        // Other robots keep theirs; unknown slots change nothing.
        assert_eq!(book.get(7).unwrap().orders.program, 2);
        assert_eq!(book.select_next(99, &programs), None);
        assert_eq!(book.get(2).unwrap().orders.program, 2);
    }

    /// Reads the program table from the user's `xeno.exe`, checking only its shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_programs_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let programs = read(&exe).unwrap();
        // `docs/screens.md`, "Orders": 31 programs; the humanoid and scout are offered
        // the same ten, the scorpion three more.
        assert_eq!(programs.programs.len(), 31);
        assert!(programs.programs.iter().all(|p| !p.name.is_empty()));
        let count = |kind| programs.offered_to(kind).count();
        assert_eq!(count(RobotKind::Humanoid), 10);
        assert_eq!(count(RobotKind::Scout), 10);
        assert_eq!(count(RobotKind::Scorpion), 13);
        // Every stock robot runs a program in the table.
        for name in ["htrain.usg", "scen00a.usg"] {
            let scenario = crate::usg::parse(&std::fs::read(dir.join(name)).unwrap()).unwrap();
            let book = OrderBook::from_scenario(&scenario);
            assert!(book.side(Side::Human).next().is_some(), "{name}");
            for side in [Side::Human, Side::Alien] {
                for (_, robot) in book.side(side) {
                    assert!(programs.get(robot.orders.program).is_some(), "{name}");
                }
            }
        }
    }
}
