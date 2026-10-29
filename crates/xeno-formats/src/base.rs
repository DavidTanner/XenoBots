//! What the port reads from the user's `xeno.exe` for the bases' tick
//! (`docs/sim.md`, "The bases' phases"): which phase runs which routine, and the
//! numbers those routines use. Game data, read at run time through the instructions
//! that use it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{invalid, word};

/// `sim_tick_base` (`1000:6300`): `inc byte [di + 0x41]`, `mov bl, [di + 0x41]`,
/// `and bx, 0xf`, `shl bl, 1`, then `call [bx + table]`, the phases' routines.
pub(crate) const TICK_AT: usize = 0x6300;
const TICK: &[u8] = &[
    0xfe, 0x45, 0x41, 0x8a, 0x5d, 0x41, 0x83, 0xe3, 0x0f, 0xd0, 0xe3,
];
/// The phases (+0x41's low four bits) the port runs.
pub const COUNT_PHASE: u8 = 5;
pub const TAKE_IN_PHASE: u8 = 9;
pub const RELAY_PHASE: u8 = 11;
pub const REPAIR_PHASE: u8 = 12;
pub const LAUNCH_PHASE: u8 = 13;
/// Each routine's start, `??` for the numbers read and the jumps' offsets.
/// The count: `test byte [di + 0x41], mask`, `jnz`, `mov byte [di + 0x21], 0`.
const COUNT: &str = "f6 45 41 ?? 75 ?? c6 45 21";
/// The take-in: `test byte [di + 0x41], mask`, `jz`; at `REACH_AT`, `mov al, ah`,
/// `mov ah, dl`, `cmp ax, reach` (the distance as 8.8).
const TAKE_IN: &str = "f6 45 41 ?? 74";
const REACH_AT: usize = 0x4b;
const REACH: &str = "8a c4 8a e2 3d";
/// Relays made: `mov al, [di + 0x3b]`, `add [di + 0x19], al`, `jnc`, `inc word [di +
/// 0xb5]`, `cmp word [di + 0xb5], wait`, `jc`, `cmp byte [di + 0x20], most`, `jnc`.
const RELAYS: &str = "8a 45 3b 00 45 19 73 ?? ff 85 b5 00 81 bd b5 00 ?? ?? 72 ?? 80 7d 20 ?? 73";
/// Repairs: `mov al, [di + 0x3a]`, `xor ah, ah`, `add [di + 0x1c], ax`, `cmp word [di
/// + 0x1c], period`, `jnc`.
const REPAIRS: &str = "8a 45 3a 32 e4 01 45 1c 81 7d 1c ?? ?? 73";
/// `2000:7e5f` (image offset `0x17e5f`): `call` the step, `cmp byte [si + 0xf9], 0`,
/// `jz`, `clc`, `retf`, `stc`, `retf`; then the step's helm: `cmp byte [si + 0x24],
/// whole`, `jz`, `add byte [si + 0x24], step`, `and byte [si + 0x24], mask`.
pub(crate) const REPAIR_AT: usize = 0x1_7e5f;
const REPAIR: &str =
    "e8 0b 00 80 bc f9 00 00 74 02 f8 cb f9 cb 80 7c 24 ?? 74 ?? 80 44 24 ?? 80 64 24 fc";
/// The restock by kind (`1000:df87`): `mov bx, [si]`, `shl bx, 1`, `call [bx + table]`;
/// the scorpion's: `mov ax, carried`, `sub al, [si + 0x118]`, then at +0x15 `mov byte
/// [si + 0x122], missiles`.
pub(crate) const RESTOCK_AT: usize = 0xdf87;
const RESTOCK: &str = "8b 1c d1 e3 ff 97";
const SCORPION_RESTOCK: &str = "b8 ?? 00 2a 84 18 01 9a";
const MISSILES: &str = "c6 84 22 01";
/// The launch: `test byte [di + 0x16], 8` (alien), `jz`, the unit list's loop, and at
/// +0x25 `call` the launch itself (`1000:6716`: `mov ax, [di + 4]`, `mov [si + 4],
/// ax`), which at +0x37 `call`s the new orders (`1000:d2b6`: at +0x11 `mov al,
/// program`).
const LAUNCH: &str = "f6 45 16 08 74 ?? 8b 5d 4d b9 26 00";
const LAUNCH_CALL_AT: usize = 0x25;
const LAUNCH_ONE: &str = "8b 45 04 89 44 04";
const ORDERS_CALL_AT: usize = 0x37;
const ORDERS: &str = "c6 45 23 00 c6 45 24 64";
const PROGRAM_AT: usize = 0x11;
/// `call [bx + table]` after the phase's call: the sliders' routines by +0x2d. Slider
/// 1 lowers +0x39 (`cmp byte [di + 0x39], least`, `jc`, `sub byte [di + 0x39], step`,
/// `add byte [di + 0x38], step`); 2 raises it (`cmp byte [di + 0x39], most`, `ja`,
/// `add byte [di + 0x39], step`); 3 to 6 the same for +0x3a and +0x3b.
const SLIDERS_AT: usize = 0x16;
const LOWER: &str = "80 7d ?? ?? 72 ?? 80 6d ?? ?? 80 45 38";
const RAISE: &str = "80 7d ?? ?? 77 ?? 80 45";
/// `1000:66c5`, what a raise takes from the pool (+0x38) or the other two sliders
/// (`AL`, `AH`): the least at `LEAST`, the step at `STEP`, else one from each.
pub(crate) const POOL_AT: usize = 0x66c5;
const POOL: &str = "80 7d 38 ?? 72 05 80 6d 38 ?? c3 3c ?? 72 08 80 fc ?? 73 0c 2c ?? c3 80 fc ?? 72 03 80 ec ?? c3 fe c8 fe cc c3";
const LEAST: [usize; 4] = [3, 12, 17, 25];
const STEP: [usize; 3] = [9, 21, 30];

/// The bases' numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bases {
    /// The count runs when the tick's other bits under this mask are clear (once
    /// every 256 ticks in the stock build).
    pub count_mask: u8,
    /// The take-in runs likewise (once every 128 ticks).
    pub take_in_mask: u8,
    /// How near (8.8 units) a robot comes to its base to be taken in.
    pub reach: u16,
    /// Relays made: one each `relay_wait` carries, while fewer than `relays_most` are
    /// in store.
    pub relay_wait: u16,
    pub relays_most: u8,
    /// A repair step each time the repairs' clock reaches `repair_period`; a step
    /// adds `repair_step` to the helm or the power, up to `whole`.
    pub repair_period: u16,
    pub repair_step: u8,
    pub whole: u8,
    /// The scorpion's restock: relays carried and tail missiles.
    pub restock: (u8, u8),
    /// The program a launched robot is given.
    pub launch_program: u8,
    /// The sliders: lowered only from `least`, raised only up to `most`, by `step`.
    pub slider_least: u8,
    pub slider_most: u8,
    pub slider_step: u8,
}

/// The bytes at `at` match `shape` (hex bytes, `??` any), else an error naming `what`.
fn shape(image: &[u8], at: usize, shape: &str, what: &str) -> Result<(), FormatError> {
    let ok = shape.split(' ').enumerate().all(|(i, b)| {
        image
            .get(at + i)
            .is_some_and(|&v| b == "??" || u8::from_str_radix(b, 16) == Ok(v))
    });
    ok.then_some(())
        .ok_or_else(|| invalid(at, format!("not the {what}")))
}

fn byte(image: &[u8], at: usize) -> Result<u8, FormatError> {
    image
        .get(at)
        .copied()
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

/// The target of the near `call` at `at`.
fn callee(image: &[u8], at: usize) -> Result<usize, FormatError> {
    let rel = operand(image, at, &[0xe8])?;
    Ok(usize::from((at as u16).wrapping_add(3).wrapping_add(rel)))
}

/// Reads the numbers from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Bases, FormatError> {
    if image.get(TICK_AT..TICK_AT + TICK.len()) != Some(TICK) {
        return Err(invalid(TICK_AT, "not the base's tick"));
    }
    let table = |at: usize| operand(image, at, &[0xff, 0x97]).map(|t| ds + usize::from(t));
    let phases = table(TICK_AT + TICK.len())?;
    let routine = |phase: u8| word(image, phases + 2 * usize::from(phase)).map(usize::from);
    let count = routine(COUNT_PHASE)?;
    shape(image, count, COUNT, "base's count")?;
    let take_in = routine(TAKE_IN_PHASE)?;
    shape(image, take_in, TAKE_IN, "base's take-in")?;
    shape(image, take_in + REACH_AT, REACH, "take-in's reach")?;
    let relays = routine(RELAY_PHASE)?;
    shape(image, relays, RELAYS, "relays made")?;
    let repairs = routine(REPAIR_PHASE)?;
    shape(image, repairs, REPAIRS, "repairs")?;
    shape(image, REPAIR_AT, REPAIR, "repair step")?;
    shape(image, RESTOCK_AT, RESTOCK, "restock")?;
    let scorpion = usize::from(word(image, table(RESTOCK_AT + 4)? + 2)?);
    shape(image, scorpion, SCORPION_RESTOCK, "scorpion's restock")?;
    let launch = routine(LAUNCH_PHASE)?;
    shape(image, launch, LAUNCH, "launch")?;
    let one = callee(image, launch + LAUNCH_CALL_AT)?;
    shape(image, one, LAUNCH_ONE, "launch of a robot")?;
    let orders = callee(image, one + ORDERS_CALL_AT)?;
    shape(image, orders, ORDERS, "new robot's orders")?;
    shape(image, orders + PROGRAM_AT, "b0", "new robot's program")?;
    shape(image, scorpion + 0x15, MISSILES, "scorpion's missiles")?;
    let sliders = table(TICK_AT + SLIDERS_AT)?;
    let slider = |n: usize| word(image, sliders + 2 * n).map(usize::from);
    for (n, at) in [(1, 0x39), (3, 0x3a), (5, 0x3b)] {
        let (lower, raise) = (slider(n)?, slider(n + 1)?);
        shape(image, lower, LOWER, "slider's lower")?;
        shape(image, raise, RAISE, "slider's raise")?;
        if [image[lower + 2], image[lower + 8], image[raise + 2]] != [at; 3] {
            return Err(invalid(lower, "another slider"));
        }
    }
    let (lower, raise) = (slider(1)?, slider(2)?);
    shape(image, POOL_AT, POOL, "sliders' pool")?;
    let pool = |at: &[usize], v: u8| at.iter().all(|&i| image[POOL_AT + i] == v);
    if !pool(&LEAST, image[lower + 3]) || !pool(&STEP, image[lower + 9]) {
        return Err(invalid(POOL_AT, "the pool's numbers are not the sliders'"));
    }
    Ok(Bases {
        count_mask: image[count + 3],
        take_in_mask: image[take_in + 3],
        reach: operand(image, take_in + REACH_AT + 4, &[0x3d])?,
        relay_wait: word(image, relays + 0x10)?,
        relays_most: byte(image, relays + 0x17)?,
        repair_period: word(image, repairs + 0xb)?,
        whole: byte(image, REPAIR_AT + 0x11)?,
        repair_step: byte(image, REPAIR_AT + 0x17)?,
        restock: (byte(image, scorpion + 1)?, byte(image, scorpion + 0x19)?),
        launch_program: byte(image, orders + PROGRAM_AT + 1)?,
        slider_least: image[lower + 3],
        slider_most: image[raise + 3],
        slider_step: image[lower + 9],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x1_0000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1_0000];
        crate::fixtures::synthetic_bases(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_numbers_read() {
        let bases = read_image(&synthetic(), DS).unwrap();
        assert_eq!(
            (bases.count_mask, bases.take_in_mask, bases.reach),
            (0xf0, 0x70, 0x3a)
        );
        assert_eq!((bases.relay_wait, bases.relays_most), (0x30, 9));
        assert_eq!(
            (bases.repair_period, bases.repair_step, bases.whole),
            (0x100, 5, 90)
        );
        assert_eq!((bases.restock, bases.launch_program), ((10, 4), 7));
        assert_eq!(
            (bases.slider_least, bases.slider_most, bases.slider_step),
            (6, 0xf0, 3)
        );
        insta::assert_debug_snapshot!(bases);
    }

    #[test]
    fn another_build_is_refused() {
        let table = DS + 0x5aea;
        for (at, value) in [
            (TICK_AT + 2, 0x42),
            (TICK_AT + TICK.len() + 1, 0x87),
            (table + 2 * 5, 0x00),
            (table + 2 * 9 + 1, 0x00),
            (0x6344 + 4, 0x74),
            (0x63dd + 4, 0x75),
            (0x63dd + REACH_AT + 4, 0x3c),
            (table + 2 * 11, 0x00),
            (0x648b + 2, 0x3a),
            (0x64af + 9, 0x7c),
            (REPAIR_AT + 4, 0xbd),
            (RESTOCK_AT + 4, 0x90),
            (DS + 0x723f + 2, 0x00),
            (0x9b97 + 0x16, 0x85),
            (0x6515 + 3, 0x10),
            (0x6515 + 0x25, 0xe9),
            (0x6716 + 0x38, 0x00),
            (0xd2b6 + 0x11, 0xb1),
            (TICK_AT + 0x18, 0x5c),
            (0x6656 + 2, 0x39),
            (0x6694 + 4, 0x76),
            (POOL_AT + 12, 0x05),
            (POOL_AT + 30, 0x01),
            (REPAIR_AT + 0x1b, 0xf8),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's numbers (`docs/sim.md`, "The bases' phases").
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_bases_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let ds = crate::sequence::data_segment(&exe).unwrap();
        let bases = read_image(&exe.image, ds).unwrap();
        assert_eq!(
            (bases.count_mask, bases.take_in_mask, bases.reach),
            (0xf0, 0x70, 0x3a)
        );
        assert_eq!((bases.relay_wait, bases.relays_most), (600, 70));
        assert_eq!(
            (bases.repair_period, bases.repair_step, bases.whole),
            (0x1800, 4, 100)
        );
        assert_eq!((bases.restock, bases.launch_program), ((12, 6), 9));
        assert_eq!(
            (bases.slider_least, bases.slider_most, bases.slider_step),
            (4, 0xfd, 2)
        );
    }
}
