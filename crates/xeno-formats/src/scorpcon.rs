//! What the port reads from the user's `xeno.exe` for the scorpion console's
//! instruments (`docs/screens.md`, "The scorpion console's instruments (T-131)"): the
//! sprite bank's handle, the panels' texts, the compass's needle table and the speed
//! lever's sprites, the rectangles the handlers draw in, and the weapon and drop
//! buttons' sprites. Game data, read at run time through the instructions that use it.

use crate::FormatError;
use crate::farewell::operand;
use crate::humancon::NEEDLES;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, string, word};
use crate::sequence;

/// `mov dx, handle` at `1000:8496`: where the opener loads `msprite3`.
pub(crate) const BANK_AT: usize = 0x8496;
/// `mov si, text`: the speed's unlit digits (`1000:870e`), the heading's (`8823`), the
/// target panel's lines without a target (`8968`) and its rounds' line with both
/// barrels damaged (`899c`), the relay panel's count and its line with none (`8c76`,
/// `8c7f`).
pub(crate) const SPEED_UNLIT_AT: usize = 0x870e;
pub(crate) const HEADING_UNLIT_AT: usize = 0x8823;
pub(crate) const NO_TARGET_AT: usize = 0x8968;
pub(crate) const NO_ROUNDS_AT: usize = 0x899c;
pub(crate) const RELAYS_AT: usize = 0x8c76;
pub(crate) const NO_RELAYS_AT: usize = 0x8c7f;
/// The compass (`1000:87d6`): `mov bx, rect` for the dial (every quadrant's needle is
/// drawn in it too), `mov ax, [bx + table]` for the needles.
pub(crate) const DIAL_AT: usize = 0x87d6;
pub(crate) const NEEDLES_AT: usize = 0x87f3;
/// The speed lever (`1000:8760`): `mov ax, [bx + table]` going forward and in reverse.
pub(crate) const LEVER_AT: [usize; 2] = [0x8775, 0x878b];
/// The power panel (`1000:8fb9`): `mov bx, rect` for the two lights and the two bars.
pub(crate) const LIGHTS_AT: [usize; 2] = [0x8fbd, 0x8fe2];
pub(crate) const BARS_AT: [usize; 2] = [0x8fd8, 0x8ffd];
/// The damage panel (`1000:9065`): `mov bx, rect` for each of its six lights.
pub(crate) const DAMAGE_AT: [usize; DAMAGE_LIGHTS] =
    [0x9084, 0x9099, 0x90ae, 0x90c3, 0x90d8, 0x90ed];
/// The pilot and program switches (`1000:8b31`, `8b9c`): `mov bx, rect`.
pub(crate) const SWITCHES_AT: [usize; 2] = [0x8b31, 0x8b9c];
/// The weapon buttons (`1000:8a91`): `mov ax, [bx + table]` for a weapon not selected
/// and selected; the drop buttons (`8ae6`): `add ax, [bx + table]`.
pub(crate) const WEAPON_OFF_AT: usize = 0x8aa6;
pub(crate) const WEAPON_ON_AT: usize = 0x8aae;
pub(crate) const DROPS_AT: usize = 0x8af9;

/// The speed lever's looks each way: a look for every 8 of speed.
pub const LEVER_LOOKS: usize = 8;
/// The damage panel's lights.
pub const DAMAGE_LIGHTS: usize = 6;
/// The drop buttons.
pub const DROPS: usize = 4;

/// The scorpion console's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScorpCon {
    /// The first handle of the console's sprite bank (`msprite3`).
    pub bank: u16,
    /// Drawn before the speed and the heading: unlit digits, then the cursor back over
    /// them.
    pub speed_unlit: Vec<u8>,
    pub heading_unlit: Vec<u8>,
    /// The target panel: the lines without a target, the rounds' line with both barrels
    /// damaged.
    pub no_target: Vec<u8>,
    pub no_rounds: Vec<u8>,
    /// The relay panel: its text after the count's two digits (the first two bytes
    /// are overwritten), and its text with none carried.
    pub relays: Vec<u8>,
    pub no_relays: Vec<u8>,
    /// The compass: the dial's rectangle and each heading's needle (low byte the
    /// quadrant times 2, high byte the needle).
    pub dial: Rect,
    pub needles: [u16; NEEDLES],
    /// The speed lever's sprites, forward and in reverse.
    pub lever: [[u16; LEVER_LOOKS]; 2],
    /// The power panel: the two lights' and the two bars' rectangles.
    pub lights: [Rect; 2],
    pub bars: [Rect; 2],
    /// The damage panel's lights: the helm, the barrels, the missile, the deployer,
    /// the sensors.
    pub damage: [Rect; DAMAGE_LIGHTS],
    /// The pilot and program switches' sprites' rectangles.
    pub switches: [Rect; 2],
    /// The weapon buttons' sprites by weapon: not selected, selected.
    pub weapon_sprites: [[u16; 2]; 2],
    /// The drop buttons' sprites by drop.
    pub drops: [u16; DROPS],
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<ScorpCon, FormatError> {
    let at_ds = |at, op: &[u8]| -> Result<usize, FormatError> {
        Ok(ds + usize::from(operand(image, at, op)?))
    };
    let text = |at| string(image, at_ds(at, &[0xbe])?);
    let area = |at| rect(image, at_ds(at, &[0xbb])?);
    let words = |at, op: &[u8], n: usize| -> Result<Vec<u16>, FormatError> {
        let table = at_ds(at, op)?;
        (0..n).map(|i| word(image, table + 2 * i)).collect()
    };
    let table = |at, op: &[u8]| -> Result<[u16; 2], FormatError> {
        let w = words(at, op, 2)?;
        Ok([w[0], w[1]])
    };
    let lever = |at| -> Result<[u16; LEVER_LOOKS], FormatError> {
        words(at, &[0x8b, 0x87], LEVER_LOOKS)?
            .try_into()
            .map_err(|_| invalid(at, "short table"))
    };
    let damage = DAMAGE_AT
        .map(area)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| invalid(DAMAGE_AT[0], "short table"))?;
    Ok(ScorpCon {
        bank: operand(image, BANK_AT, &[0xba])?,
        speed_unlit: text(SPEED_UNLIT_AT)?,
        heading_unlit: text(HEADING_UNLIT_AT)?,
        no_target: text(NO_TARGET_AT)?,
        no_rounds: text(NO_ROUNDS_AT)?,
        relays: text(RELAYS_AT)?,
        no_relays: text(NO_RELAYS_AT)?,
        dial: area(DIAL_AT)?,
        needles: words(NEEDLES_AT, &[0x8b, 0x87], NEEDLES)?
            .try_into()
            .map_err(|_| invalid(NEEDLES_AT, "short table"))?,
        lever: [lever(LEVER_AT[0])?, lever(LEVER_AT[1])?],
        lights: [area(LIGHTS_AT[0])?, area(LIGHTS_AT[1])?],
        bars: [area(BARS_AT[0])?, area(BARS_AT[1])?],
        damage,
        switches: [area(SWITCHES_AT[0])?, area(SWITCHES_AT[1])?],
        weapon_sprites: [
            table(WEAPON_OFF_AT, &[0x8b, 0x87])?,
            table(WEAPON_ON_AT, &[0x8b, 0x87])?,
        ],
        drops: words(DROPS_AT, &[0x03, 0x87], DROPS)?
            .try_into()
            .map_err(|_| invalid(DROPS_AT, "short table"))?,
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<ScorpCon, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x20000;

    /// Made-up tables: bank 0x100; texts "t0".."t5" at DS + 0x100 on; needles 0..31;
    /// lever sprites 0x10.. and 0x20..; rectangles (n, n + 1, n + 2, n + 3) for the
    /// dial (1), lights (2, 3), bars (4, 5), damage lights (6..11) and switches (12,
    /// 13); weapon sprites 0x30, 0x31 and 0x40, 0x41; drops 0x50..0x53.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x1000];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        let table = |first: u16, n: u16| -> Vec<u8> {
            (first..first + n).flat_map(u16::to_le_bytes).collect()
        };
        put(BANK_AT, &op(&[0xba], 0x100));
        let texts = [
            SPEED_UNLIT_AT,
            HEADING_UNLIT_AT,
            NO_TARGET_AT,
            NO_ROUNDS_AT,
            RELAYS_AT,
            NO_RELAYS_AT,
        ];
        for (i, at) in texts.into_iter().enumerate() {
            let value = 0x100 + 8 * i as u16;
            put(at, &op(&[0xbe], value));
            put(DS + usize::from(value), format!("t{i}\0").as_bytes());
        }
        put(NEEDLES_AT, &op(&[0x8b, 0x87], 0x200));
        put(DS + 0x200, &table(0, NEEDLES as u16));
        for (i, at) in LEVER_AT.into_iter().enumerate() {
            let value = 0x280 + 0x20 * i as u16;
            put(at, &op(&[0x8b, 0x87], value));
            put(DS + usize::from(value), &table(0x10 * (i as u16 + 1), 8));
        }
        let rects = [DIAL_AT]
            .into_iter()
            .chain(LIGHTS_AT)
            .chain(BARS_AT)
            .chain(DAMAGE_AT)
            .chain(SWITCHES_AT);
        for (i, at) in rects.enumerate() {
            let n = i as u16 + 1;
            let value = 0x300 + 8 * n;
            put(at, &op(&[0xbb], value));
            put(DS + usize::from(value), &table(n, 4));
        }
        put(WEAPON_OFF_AT, &op(&[0x8b, 0x87], 0x400));
        put(WEAPON_ON_AT, &op(&[0x8b, 0x87], 0x404));
        put(DS + 0x400, &[0x30, 0, 0x31, 0, 0x40, 0, 0x41, 0]);
        put(DROPS_AT, &op(&[0x03, 0x87], 0x410));
        put(DS + 0x410, &table(0x50, 4));
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let con = read_image(&synthetic(), DS).unwrap();
        assert_eq!((con.bank, con.speed_unlit.as_slice()), (0x100, &b"t0"[..]));
        assert_eq!(con.no_relays, b"t5");
        assert_eq!((con.needles[31], con.lever[1][7]), (31, 0x27));
        assert_eq!((con.damage[5].top, con.switches[1].right), (11, 16));
        assert_eq!(con.weapon_sprites, [[0x30, 0x31], [0x40, 0x41]]);
        assert_eq!(con.drops, [0x50, 0x51, 0x52, 0x53]);
        insta::assert_debug_snapshot!(con);
    }

    #[test]
    fn another_build_is_refused() {
        let mut checked = vec![BANK_AT, SPEED_UNLIT_AT, NO_TARGET_AT, NO_RELAYS_AT];
        checked.extend([
            DIAL_AT,
            NEEDLES_AT + 1,
            LEVER_AT[1],
            LIGHTS_AT[1],
            BARS_AT[0],
        ]);
        checked.extend([DAMAGE_AT[5], SWITCHES_AT[0], WEAPON_ON_AT + 1, DROPS_AT]);
        for at in checked {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/screens.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_scorpcon_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let con = read(&exe).unwrap();
        assert_eq!(con.bank, 0x678);
        // Two and three unlit digits, then the cursor back over them.
        assert_eq!(con.speed_unlit[2..], [8, 8]);
        assert_eq!(con.heading_unlit[3..], [8, 8, 8]);
        // The humanoid's needles, drawn in the dial.
        let human = crate::humancon::read(&exe).unwrap();
        assert_eq!(con.needles, human.needles);
        assert_eq!((con.dial.top, con.dial.left), (137, 62));
        // The lever's looks step on with the speed, never back.
        for looks in con.lever {
            assert!(looks.windows(2).all(|w| w[0] <= w[1]));
        }
        assert_eq!(con.weapon_sprites, [[0x728, 0x738], [0x730, 0x740]]);
        assert_eq!(con.drops, [0x6e8, 0x6f8, 0x708, 0x718]);
        assert_eq!(con.bars[0].right - con.bars[0].left, 31);
        // Every light is inside the damage panel's widget (3, 235, 53, 314).
        assert!(con.damage.iter().all(|r| r.top >= 3 && r.left >= 235));
        assert!(con.relays.len() > 2 && con.relays.iter().all(u8::is_ascii));
    }
}
