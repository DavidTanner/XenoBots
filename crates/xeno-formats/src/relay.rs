//! What the port reads from the user's `xeno.exe` for the relays' tick and the net's
//! signal (`docs/sim.md`, "The relay's tick (T-148)"): the relay's looks and heights
//! by stage, its colours, the reaches of its fold and its links, and the signal and
//! drive a robot takes from the net. Game data, read at run time through the
//! instructions that use it, after checking their bytes.

use crate::FormatError;
use crate::base::shape;
use crate::farewell::operand;
use crate::maproom::{invalid, word};
use crate::sensing::TICKS_AT;

/// `sim_tick_relay` (`1000:ccd9`): `inc byte [di + 0x1e]`, `test byte [di + 0x1e],
/// mask`, `jz`, `ret`, `mov bl, [di + 0x1f]`, `xor bh, bh`, `call [bx + states]`,
/// `call` the look, `ret`.
pub(crate) const TICK_AT: usize = 0xccd9;
const TICK: &str = "fe 45 1e f6 45 1e ?? 74 01 c3 8a 5d 1f 32 ff ff 97 ?? ?? e8 ?? ?? c3";
/// The look (`1000:ccf0`): the colour by +0x16 bits 4 and 3 (`mov al, colour` three
/// times) into +0x1d, the frames `[bx + looks]` by stage into +0x19..=+0x1c, the
/// height `[bx + heights]` (words) into +0x12.
const LOOK: &str = "b0 ?? f6 45 16 10 74 0a b0 ?? f6 45 16 08 74 02 b0 ?? 88 45 1d 8a 5d 20 32 ff \
                    8a 87 ?? ?? 88 45 19 88 45 1a 88 45 1b 88 45 1c d1 e3 8b 87 ?? ?? 89 45 12 c3";
/// The states' routines, by state / 2. Folded: out of the field and unlinked, stage 0,
/// then `mov ax, kind`, `mov cx, reach`, `call` the nearest, `jc`, rise (state 2).
const FOLDED: &str = "80 65 16 af c6 45 20 00 b8 ?? ?? b9 ?? ?? e8 ?? ?? 72 04 c6 45 1f 02 c3";
/// Rising: unlinked, stage up, `cmp byte [di + 0x20], top`, `jz`, up (state 4).
const RISING: &str = "80 65 16 ef fe 45 20 80 7d 20 ?? 74 01 c3 c6 45 1f 04 c3";
/// Up: `call` the links, in the field, linked while +0x21 is not 0, then the nearest
/// scorpion as the folded state's, and fold (state 6) on the carry.
const UP: &str = "e8 ?? ?? 80 4d 16 40 80 65 16 ef 80 7d 21 00 74 04 80 4d 16 10 \
                  b8 ?? ?? b9 ?? ?? e8 ?? ?? 73 04 c6 45 1f 06 c3";
/// Folding: unlinked, stage down, at 0 folded (state 0).
const FOLDING: &str = "80 65 16 ef fe 4d 20 80 7d 20 00 74 01 c3 c6 45 1f 00 c3";
/// The links (`1000:cd88`): only when +0x1e is 0, then `call far` the base's power;
/// at `REACH_AT`, `cmp cx, reach` (8.8) and the direction from the bearing's top two
/// bits.
const LINKS: &str = "f6 45 1e ff 74 01 c3 9a";
const LINK_REACH_AT: usize = 0x8b;
const LINK_REACH: &str = "81 f9 ?? ?? 77 0f d1 c0 d1 c0 d1 c0 25 06 00";
/// The base's power (`2000:6978`): the relay's side's base, its +0x39 shifted right
/// twice.
const BASE_POWER: &str = "8b 36 ?? ?? f6 45 16 08 74 04 8b 36 ?? ?? 8a 44 39 d0 e8 d0 e8 cb";
/// The signal and drive (`2000:8087`, image offset `0x18087`): `call` the signal,
/// into +0x4f; `cmp al, battery`: over it off the battery and +0x50 up one (to
/// 0xff), else `ON_BATTERY`; the drive +0x51 the signal, or +0x50 on the battery,
/// ANDed with `[bx + masks]` by +0x4e >> 4.
pub(crate) const DRIVE_AT: usize = 0x1_8087;
const DRIVE: &str = "0e e8 ?? ?? 90 88 45 4f 3c ?? 76 10 80 a5 fd 00 fe 80 7d 50 ff 74 0a fe 45 50 \
                     eb 05 80 8d fd 00 01 8a 45 4f f6 85 fd 00 01 74 03 8a 45 50 88 45 51 8a 5d 4e \
                     d0 eb d0 eb d0 eb d0 eb 32 ff 8a 87 ?? ?? 20 45 51 cb";
/// The signal (`2000:7d16`), checked in pieces: the robot's side (`and al, 8` on its
/// +0x16); relays and bases (`cmp word [si], 4`); linked (`test` of their +0x16 bit
/// 4); the distance (`call far 1000:cfd8`); 0xff within `full`, else the last under
/// `none` (`mov al, ah`, `mov ah, dl`, `cmp ax, full`, `jbe`, `cmp ax, none`, `jnc`);
/// then `sub ax, full`, `cmp ax, none - full`, `jnc`, `shl al, 1` twice, `neg al`.
const SIGNAL: [(usize, &str); 6] = [
    (0, "8a 45 16 24 08 a2"),
    (0x26, "83 3c 04 74 05 83 3c 03"),
    (0x3b, "f6 44 16 10"),
    (0x47, "9a d8 cf 00 00"),
    (0x53, "8a c4 8a e2 3d ?? ?? 76 ?? 3d ?? ?? 73"),
    (0x69, "2d ?? ?? 3d ?? ?? 73 ?? d0 e0 d0 e0 f6 d8"),
];
const DISTANCE_AT: usize = 0xcfd8;
const DISTANCE: &str = "e8 11 39 cb";
/// Each kind's tick-table routine that runs the signal (humanoid `1000:b2fc`, scorpion
/// `99f8`, scout `cad9`): `call far` it, mark the power panel, then once every
/// `recharge + 1` passes (`test word [0x62c6], mask`) +0x50 up one, to 0xff.
pub(crate) const SIGNAL_STEPS: [usize; 3] = [0xb2fc, 0x99f8, 0xcad9];
const SIGNAL_STEP: &str =
    "9a ?? ?? ?? ?? c6 85 ?? 01 03 f7 06 c6 62 ?? ?? 75 09 80 7d 50 ff 74 03 fe 45 50";

/// The relays' and the signal's numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relays {
    /// The relay's state runs when its clock's bits under this mask are 0.
    pub state_mask: u8,
    /// The colour frame (+0x1d): unpowered, powered by the human net, by the alien.
    pub colours: [u8; 3],
    /// The look (+0x19..=+0x1c) and model height (+0x12) by stage.
    pub looks: [u8; 4],
    pub heights: [u16; 4],
    /// The stage a rising relay is up at.
    pub top: u8,
    /// A relay folds (or stays folded) while an object of kind `folder` is within
    /// `fold_reach` (8.8).
    pub folder: u16,
    pub fold_reach: u16,
    /// A relay links to the nearest relay or base each quarter turn within this (8.8).
    pub link_reach: u16,
    /// A robot's signal is 0xff within `full` (8.8) of a linked relay or base of its
    /// side, falling by 4 a 1/256th to 0 at `none`.
    pub full: u16,
    pub none: u16,
    /// Over this signal a robot runs off the net; at or under it, on its battery.
    pub battery: u8,
    /// The drive's mask by power (+0x4e) >> 4 (the table's 7 entries cover power 0 to
    /// 111; the rest is what the original reads after it).
    pub drive_masks: [u8; 16],
    /// The battery gains one on passes (`DS:0x62c6`) with none of these bits.
    pub recharge: u16,
    /// The entry of each kind's tick table (humanoid, scorpion, scout) that runs the
    /// signal.
    pub signal_steps: [u8; 3],
}

/// The image offset of the `call far` at `at` (segment and offset as the image keeps
/// them).
fn far(image: &[u8], at: usize) -> Result<usize, FormatError> {
    let offset = operand(image, at, &[0x9a])?;
    let segment = word(image, at + 3)?;
    Ok(usize::from(segment) * 16 + usize::from(offset))
}

/// The target of the near `call` at `at` (a 16-bit offset in `at`'s 64 KB).
fn callee(image: &[u8], at: usize) -> Result<usize, FormatError> {
    let rel = operand(image, at, &[0xe8])?;
    Ok(((at + 3) as isize + isize::from(rel as i16)) as usize)
}

fn byte(image: &[u8], at: usize) -> Result<u8, FormatError> {
    image
        .get(at)
        .copied()
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

/// Reads the numbers from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Relays, FormatError> {
    shape(image, TICK_AT, TICK, "relay's tick")?;
    let look = callee(image, TICK_AT + 0x13)?;
    shape(image, look, LOOK, "relay's look")?;
    let states = ds + usize::from(word(image, TICK_AT + 0x11)?);
    let routine = |state: usize| word(image, states + 2 * state).map(usize::from);
    let (folded, rising, up, folding) = (routine(0)?, routine(1)?, routine(2)?, routine(3)?);
    shape(image, folded, FOLDED, "folded relay's state")?;
    shape(image, rising, RISING, "rising relay's state")?;
    shape(image, up, UP, "up relay's state")?;
    shape(image, folding, FOLDING, "folding relay's state")?;
    shape(image, routine(4)?, "c3", "idle relay's state")?;
    let folder = word(image, folded + 9)?;
    let fold_reach = word(image, folded + 0xc)?;
    let nearest = callee(image, folded + 0xe)?;
    if [word(image, up + 0x16)?, word(image, up + 0x19)?] != [folder, fold_reach]
        || callee(image, up + 0x1b)? != nearest
    {
        return Err(invalid(up, "the fold's test is not the rise's"));
    }
    let links = callee(image, up)?;
    shape(image, links, LINKS, "relay's links")?;
    shape(image, far(image, links + 7)?, BASE_POWER, "base's power")?;
    shape(image, links + LINK_REACH_AT, LINK_REACH, "links' reach")?;

    shape(image, DRIVE_AT, DRIVE, "signal and drive")?;
    let signal = callee(image, DRIVE_AT + 1)?;
    for (at, bytes) in SIGNAL {
        shape(image, signal + at, bytes, "signal")?;
    }
    shape(image, DISTANCE_AT, DISTANCE, "signal's distance")?;
    let full = word(image, signal + 0x58)?;
    let none = word(image, signal + 0x5d)?;
    if word(image, signal + 0x6a)? != full || word(image, signal + 0x6d)? != none.wrapping_sub(full)
    {
        return Err(invalid(signal, "the signal's fall is not its reach"));
    }
    let masks = ds + usize::from(word(image, DRIVE_AT + 0x40)?);
    let mut drive_masks = [0; 16];
    for (i, m) in drive_masks.iter_mut().enumerate() {
        *m = byte(image, masks + i)?;
    }

    let table = |at: usize| word(image, at + 2).map(|t| ds + usize::from(t));
    let mut signal_steps = [0; 3];
    let mut recharge = None;
    for (k, &at) in SIGNAL_STEPS.iter().enumerate() {
        shape(image, at, SIGNAL_STEP, "signal's step")?;
        let mask = word(image, at + 0xe)?;
        if far(image, at)? != DRIVE_AT || recharge.is_some_and(|r| r != mask) {
            return Err(invalid(at, "another signal step"));
        }
        recharge = Some(mask);
        let ticks = table(TICKS_AT[k])?;
        let found: Vec<usize> = (0..crate::sensing::TICKS)
            .filter(|&n| word(image, ticks + 2 * n).is_ok_and(|w| usize::from(w) == at))
            .collect();
        signal_steps[k] = match *found {
            [n] => n as u8,
            _ => {
                return Err(invalid(
                    TICKS_AT[k],
                    "the signal not once in its tick table",
                ));
            }
        };
    }
    let words = |at: usize| -> Result<[u16; 4], FormatError> {
        let mut out = [0; 4];
        for (i, w) in out.iter_mut().enumerate() {
            *w = word(image, at + 2 * i)?;
        }
        Ok(out)
    };
    let looks = ds + usize::from(word(image, look + 0x1c)?);
    Ok(Relays {
        state_mask: byte(image, TICK_AT + 6)?,
        colours: [
            byte(image, look + 1)?,
            byte(image, look + 9)?,
            byte(image, look + 0x11)?,
        ],
        looks: std::array::from_fn(|i| image[looks + i]),
        heights: words(ds + usize::from(word(image, look + 0x2e)?))?,
        top: byte(image, rising + 0xa)?,
        folder,
        fold_reach,
        link_reach: word(image, links + LINK_REACH_AT + 2)?,
        full,
        none,
        battery: byte(image, DRIVE_AT + 9)?,
        drive_masks,
        recharge: recharge.unwrap_or(0),
        signal_steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x1_0000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1_0000];
        crate::fixtures::synthetic_relays(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_numbers_read() {
        let r = read_image(&synthetic(), DS).unwrap();
        assert_eq!((r.state_mask, r.colours, r.top), (0x0f, [1, 0, 2], 3));
        assert_eq!(
            (r.looks, r.heights),
            ([3, 2, 1, 0], [0x100, 0x200, 0x300, 0x400])
        );
        assert_eq!((r.folder, r.fold_reach, r.link_reach), (1, 0x32, 0x640));
        assert_eq!((r.full, r.none, r.battery), (0x578, 0x5b8, 0x40));
        assert_eq!((r.recharge, r.signal_steps), (0x1f0, [3, 4, 2]));
        insta::assert_debug_snapshot!(r);
    }

    #[test]
    fn another_build_is_refused() {
        for (at, value) in [
            (TICK_AT + 1, 0x46),
            (0xccf0 + 2, 0xf7),
            (0xcd25 + 3, 0xae),
            (0xcd50 + 0x17, 0x02),
            (0xcd88 + LINK_REACH_AT, 0x83),
            (0x1_6978 + 0x10, 0x3a),
            (DRIVE_AT + 5, 0x89),
            (0x1_7d16 + 0x6b, 0x00),
            (DISTANCE_AT, 0x90),
            (SIGNAL_STEPS[1] + 0xe, 0xf1),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's numbers (`docs/sim.md`, "The relay's tick (T-148)").
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_relays_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let motion = crate::motion::read(&exe).unwrap();
        let r = &motion.relays;
        assert_eq!(
            (r.state_mask, r.colours, r.looks, r.top),
            (0x0f, [1, 0, 2], [3, 2, 1, 0], 3)
        );
        assert!(r.heights.windows(2).all(|h| h[0] < h[1]), "{:?}", r.heights);
        assert_eq!((r.folder, r.fold_reach, r.link_reach), (1, 0x32, 0x640));
        assert_eq!(
            (r.full, r.none, r.battery, r.recharge),
            (0x578, 0x5b8, 0x40, 0x1f0)
        );
        assert_eq!(r.signal_steps, [3; 3]);
        // Full power keeps the whole drive.
        assert_eq!(r.drive_masks[100 >> 4], 0xff);
    }
}
