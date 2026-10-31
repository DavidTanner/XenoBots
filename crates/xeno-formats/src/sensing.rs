//! What the port reads from the user's `xeno.exe` for the robots' sensing
//! (`docs/sim.md`, "Sensing"): each kind's scanning and locking ranges, and where in
//! each kind's tick table the scan, the lock's upkeep (with its arc and range) and the
//! lock warning's decay run. Game data, read at run time through the instructions
//! that use it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{invalid, word};

/// `call [bx + table]` in each kind's tick (humanoid `1000:abe0`, scorpion `930a`,
/// scout `c485`): its tick table, 16 routines picked by +0x1a.
pub(crate) const TICKS_AT: [usize; 3] = [0xabfb, 0x9322, 0xc4a0];
const TICKS_OP: &[u8] = &[0xff, 0x97];
/// `mov ax, [bx + table]` in the scan (`1000:dcba`): the scanning range by kind.
pub(crate) const SCAN_RANGE_AT: usize = 0xdcfe;
/// `mov ax, [bx + table]` in the lock's upkeep (`1000:d66f`): the locking range by
/// kind, which looking for enemies (`1000:d757`) and the lock (`d8af`) use too.
pub(crate) const LOCK_RANGE_AT: usize = 0xd696;
const RANGE_OP: &[u8] = &[0x8b, 0x87];
/// The scan, the lock's upkeep and the lock warning's decay, by their first bytes:
/// `test byte [di + 0x1a], 0xe0`; `mov [arc], ax`, `mov [range], bx`;
/// `shr byte [di + 0xed], 1`.
pub(crate) const SCAN: (usize, &[u8]) = (0xdcba, &[0xf6, 0x45, 0x1a, 0xe0]);
pub(crate) const KEEP_LOCK: usize = 0xd66f;
pub(crate) const LOCKED_ON: (usize, &[u8]) = (0xdc95, &[0xd0, 0xad, 0xed, 0x00]);
/// The contact check (`1000:d464`, T-141), found in the tick tables by its address.
pub(crate) const CONTACT: usize = 0xd464;
/// Entries in a tick table.
pub const TICKS: usize = 16;
/// `mov si, message` before each of the lock's messages to the robot's panel, in
/// [`Messages`]' order: looking for enemies found none (`1000:d757`); the lock
/// (`d8af`) took a first target, replaced one, was refused with the targeting
/// computer damaged, out of range, on the target it has, on the robot itself, and
/// locked one of its own side; the upkeep (`d66f`) lost the target, and found it
/// destroyed; the R key (`d7f8`) dropped the target; the A key (`d812`) found none;
/// the consoles' map-mode press (T-133) set an object goal (`4685`) or a destination
/// (`46c7`).
pub(crate) const MESSAGES_AT: [usize; 14] = [
    0xd7f0, 0xd8f4, 0xd8fc, 0xd917, 0xd910, 0xd91e, 0xd909, 0xd8dd, 0xd6bd, 0xd6d0, 0xd7fe, 0xd89f,
    0x46bb, 0x46e7,
];
/// `mov ax, [bx + table]` in the A key's search (`1000:d812`): the locking range again.
pub(crate) const NEXT_RANGE_AT: usize = 0xd822;
/// `mov ax, 2 * event` before the lock's sound.
pub(crate) const LOCK_SOUND_AT: usize = 0xd902;
/// `call cs:[bx + table]` in the weapon state routines (`1000:d0e9`, `d10d`, opcodes
/// 0x78 to 0x7a): each kind's routine for its first and second weapon.
pub(crate) const WEAPON_STATE_AT: [usize; 2] = [0xd0ed, 0xd111];
const WEAPON_STATE_OP: &[u8] = &[0x2e, 0xff, 0x97];
/// The drive check those routines call (`cmp al, [di + 0x51]`).
pub(crate) const DRIVE_CHECK: (usize, &[u8]) = (0xd1c4, &[0x3a, 0x45, 0x51]);
/// `mov [point], ax` in opcode 0x66: the worked-out point (`DS:0x4da4`).
pub(crate) const POINT_AT: (usize, &[u8]) = (0x510e, &[0xa3]);

/// Where one robot kind's tick table runs the sensing routines: entry `n` runs on
/// the ticks when +0x1a, after its step, has `(tick & 0x1e) >> 1 == n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindTicks {
    /// The scan's entry (`1000:dcba`).
    pub scan: u8,
    /// The lock's upkeep's entry, and the arc (65,536 to a turn, either way of the
    /// heading) and range (8.8) it is called with.
    pub lock: u8,
    pub arc: u16,
    pub range: u16,
    /// The lock warning's decay (`1000:dc95`).
    pub locked_on: u8,
    /// The contact check (`1000:d464`).
    pub contact: u8,
}

/// The sensing tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensing {
    /// By kind (humanoid, scorpion, scout), in 8.8 units: how far a scan sees and a
    /// robot steers for the nearest enemy (`DS:0x707f`), and how far it looks for one
    /// to lock and keeps a lock (`DS:0x7079`).
    pub scan_range: [u16; 3],
    pub lock_range: [u16; 3],
    pub ticks: [KindTicks; 3],
    pub messages: Messages,
    /// Each kind's two weapons as the scripts' weapon state sees them.
    pub weapons: [[Weapon; 2]; 3],
    /// The worked-out point's x and y (16.16) in the image.
    pub point: (i32, i32),
    /// The sound event a lock plays for the robot whose console is open.
    pub lock_sound: u8,
}

/// One weapon in the weapon state (`docs/sim.md`, "Sensing"): its number (the state
/// word's high byte), the drive it needs (+0x51), and its damaged bits in +0xdc: the
/// weapon is present unless any is set, or with `all` unless all are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weapon {
    pub number: u8,
    pub drive: u8,
    pub damaged: u8,
    pub all: bool,
}

/// The lock's messages, string-segment offsets (see [`MESSAGES_AT`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Messages {
    pub none_found: u16,
    pub locked: u16,
    pub relocked: u16,
    pub targeting_damaged: u16,
    pub out_of_range: u16,
    pub already: u16,
    pub itself: u16,
    pub friendly: u16,
    pub lost: u16,
    pub destroyed: u16,
    pub dropped: u16,
    pub none_next: u16,
    pub goal_object: u16,
    pub destination: u16,
}

fn ranges(image: &[u8], ds: usize, at: usize) -> Result<[u16; 3], FormatError> {
    let table = ds + usize::from(operand(image, at, RANGE_OP)?);
    Ok([
        word(image, table)?,
        word(image, table + 2)?,
        word(image, table + 4)?,
    ])
}

fn starts(image: &[u8], at: usize, bytes: &[u8]) -> bool {
    image.get(at..at + bytes.len()) == Some(bytes)
}

/// A kind's tick table: the entries of the scan and the lock warning's decay, and of
/// the routine that calls the lock's upkeep (`mov ax, arc`, `mov bx, range`, `call`).
fn kind_ticks(image: &[u8], ds: usize, at: usize) -> Result<KindTicks, FormatError> {
    let table = ds + usize::from(operand(image, at, TICKS_OP)?);
    let (mut scan, mut lock, mut locked_on, mut contact) = (None, None, None, None);
    for n in 0..TICKS {
        let routine = usize::from(word(image, table + 2 * n)?);
        if routine == SCAN.0 {
            scan = scan.or(Some(n as u8));
        } else if routine == LOCKED_ON.0 {
            locked_on = locked_on.or(Some(n as u8));
        } else if routine == CONTACT {
            contact = contact.or(Some(n as u8));
        } else if let Some(&[0xb8, a0, a1, 0xbb, r0, r1, 0xe8, c0, c1]) =
            image.get(routine..routine + 9)
        {
            let callee = (routine as u16 + 9).wrapping_add(u16::from_le_bytes([c0, c1]));
            if usize::from(callee) == KEEP_LOCK && lock.is_none() {
                let w = |lo, hi| u16::from_le_bytes([lo, hi]);
                lock = Some((n as u8, w(a0, a1), w(r0, r1)));
            }
        }
    }
    match (scan, lock, locked_on, contact) {
        (Some(scan), Some((lock, arc, range)), Some(locked_on), Some(contact)) => Ok(KindTicks {
            scan,
            lock,
            arc,
            range,
            locked_on,
            contact,
        }),
        _ => Err(invalid(table, "a tick table without the sensing routines")),
    }
}

/// A weapon state routine: `mov al, number`, `xor ah, ah`, `push ax`, `mov al, drive`,
/// a call to the drive check, `pop ax`, `jnc`, `or ah, 0x20`, then the damaged test:
/// `test byte [di + 0xdc], bits`, `jnz`, or `mov al, [di + 0xdc]`, `and al, bits`,
/// `cmp al, bits`, `jz`.
fn weapon(image: &[u8], at: usize) -> Result<Weapon, FormatError> {
    let bad = || invalid(at, "not a weapon state routine");
    let b = image.get(at..at + 26).ok_or_else(bad)?;
    let call = (at as u16 + 10).wrapping_add(u16::from_le_bytes([b[8], b[9]]));
    let head = b[0] == 0xb0
        && b[2..6] == [0x32, 0xe4, 0x50, 0xb0]
        && b[7] == 0xe8
        && usize::from(call) == DRIVE_CHECK.0
        && b[10..16] == [0x58, 0x73, 0x03, 0x80, 0xcc, 0x20];
    let (damaged, all) = match b[16..] {
        [0xf6, 0x85, 0xdc, 0x00, bits, 0x75, ..] => (bits, false),
        [0x8a, 0x85, 0xdc, 0x00, 0x24, bits, 0x3c, again, 0x74, _] if bits == again => (bits, true),
        _ => return Err(bad()),
    };
    if !head || damaged == 0 {
        return Err(bad());
    }
    Ok(Weapon {
        number: b[1],
        drive: b[6],
        damaged,
        all,
    })
}

fn weapons(image: &[u8]) -> Result<[[Weapon; 2]; 3], FormatError> {
    if !starts(image, DRIVE_CHECK.0, DRIVE_CHECK.1) {
        return Err(invalid(DRIVE_CHECK.0, "not the drive check"));
    }
    let routine = |w: usize, kind: usize| -> Result<Weapon, FormatError> {
        let table = usize::from(operand(image, WEAPON_STATE_AT[w], WEAPON_STATE_OP)?);
        weapon(image, usize::from(word(image, table + 2 * kind)?))
    };
    let kind = |k| Ok::<_, FormatError>([routine(0, k)?, routine(1, k)?]);
    Ok([kind(0)?, kind(1)?, kind(2)?])
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Sensing, FormatError> {
    if !starts(image, SCAN.0, SCAN.1) || !starts(image, LOCKED_ON.0, LOCKED_ON.1) {
        return Err(invalid(SCAN.0, "not the scan or the lock warning"));
    }
    if !starts(image, KEEP_LOCK, &[0xa3]) || !starts(image, KEEP_LOCK + 3, &[0x89, 0x1e]) {
        return Err(invalid(KEEP_LOCK, "not the lock's upkeep"));
    }
    if operand(image, NEXT_RANGE_AT, RANGE_OP)? != operand(image, LOCK_RANGE_AT, RANGE_OP)? {
        return Err(invalid(NEXT_RANGE_AT, "not the A key's locking range"));
    }
    let m = |i: usize| operand(image, MESSAGES_AT[i], &[0xbe]);
    let sound = operand(image, LOCK_SOUND_AT, &[0xb8])?;
    if sound % 2 != 0 || sound > 0x1ff {
        return Err(invalid(LOCK_SOUND_AT, "not a sound event"));
    }
    Ok(Sensing {
        messages: Messages {
            none_found: m(0)?,
            locked: m(1)?,
            relocked: m(2)?,
            targeting_damaged: m(3)?,
            out_of_range: m(4)?,
            already: m(5)?,
            itself: m(6)?,
            friendly: m(7)?,
            lost: m(8)?,
            destroyed: m(9)?,
            dropped: m(10)?,
            none_next: m(11)?,
            goal_object: m(12)?,
            destination: m(13)?,
        },
        lock_sound: (sound / 2) as u8,
        weapons: weapons(image)?,
        point: {
            let at = ds + usize::from(operand(image, POINT_AT.0, POINT_AT.1)?);
            let w = |i: usize| word(image, at + 2 * i);
            (
                i32::from(w(0)?) | i32::from(w(1)?) << 16,
                i32::from(w(2)?) | i32::from(w(3)?) << 16,
            )
        },
        scan_range: ranges(image, ds, SCAN_RANGE_AT)?,
        lock_range: ranges(image, ds, LOCK_RANGE_AT)?,
        ticks: [
            kind_ticks(image, ds, TICKS_AT[0])?,
            kind_ticks(image, ds, TICKS_AT[1])?,
            kind_ticks(image, ds, TICKS_AT[2])?,
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x9c00];
        crate::fixtures::synthetic_sensing(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let sensing = read_image(&synthetic(), DS).unwrap();
        assert_eq!(sensing.scan_range, [0x500, 0x600, 0x700]);
        assert_eq!(sensing.lock_range, [0x800, 0x900, 0xa00]);
        let t = sensing.ticks;
        assert_eq!(
            (t[0].scan, t[0].lock, t[0].locked_on, t[0].contact),
            (15, 5, 6, 8)
        );
        assert_eq!((t[1].lock, t[1].arc, t[1].range), (6, 0x1100, 0x310));
        let m = sensing.messages;
        assert_eq!(
            (m.none_found, m.destroyed, sensing.lock_sound),
            (0x40, 0x49, 26)
        );
        insta::assert_debug_snapshot!(sensing);
    }

    #[test]
    fn another_build_is_refused() {
        for (at, value) in [
            (SCAN.0 + 3, 0xf0),
            (LOCKED_ON.0, 0xd1),
            (KEEP_LOCK, 0xa1),
            (SCAN_RANGE_AT + 1, 0x86),
            (LOCK_RANGE_AT, 0x8a),
            (NEXT_RANGE_AT + 2, 0x60),
            (TICKS_AT[2] + 1, 0x96),
            (MESSAGES_AT[4], 0xbf),
            (LOCK_SOUND_AT + 1, 0x35),
            (DRIVE_CHECK.0 + 2, 0x50),
            (WEAPON_STATE_AT[1] + 2, 0x96),
            (0xa300 + 7, 0xe9),
            (0xa320 + 16, 0x8b),
            (POINT_AT.0, 0xa1),
            // The scorpion's lock routine calls elsewhere, and its scan is gone.
            (0xa217, 0x00),
            (DS + 0x9720 + 30, 0x00),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables: every kind scans and keeps its lock in its tick table, a
    /// lock reaches further than a scan for the humanoid (`docs/sim.md`, "Sensing"),
    /// and the arcs are under a quarter turn.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_sensing_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let sensing = read_image(&exe.image, crate::sequence::data_segment(&exe).unwrap()).unwrap();
        assert!(sensing.scan_range.iter().all(|&r| r > 0x100));
        assert!(sensing.scan_range[0] < sensing.lock_range[0]);
        for (t, range) in sensing.ticks.iter().zip(sensing.lock_range) {
            assert_eq!(t.scan, 15);
            assert!(t.arc > 0 && t.arc < 0x4000);
            assert!(t.range > 0 && t.range <= range);
        }
        // Each kind has two weapons with damaged bits; only the scorpion's first takes
        // all its bits (its two cannons).
        for (k, kind) in sensing.weapons.iter().enumerate() {
            assert!(kind.iter().all(|w| w.damaged != 0));
            assert_eq!(kind[0].all, k == 1);
        }
        // The messages are text in the string segment (0x0c breaks a line).
        let motion = crate::motion::read(&exe).unwrap();
        let m = sensing.messages;
        for at in [
            m.none_found,
            m.locked,
            m.relocked,
            m.targeting_damaged,
            m.out_of_range,
            m.already,
            m.itself,
            m.friendly,
            m.lost,
            m.destroyed,
            m.dropped,
            m.none_next,
            m.goal_object,
            m.destination,
        ] {
            let text: Vec<u8> = (at..)
                .map(|o| motion.strings.byte(o))
                .take_while(|&b| b != 0)
                .collect();
            assert!(
                text.len() > 2
                    && text
                        .iter()
                        .all(|&b| b == 0x0c || (0x20..0x7f).contains(&(b & 0x7f))),
                "{at:#x}"
            );
        }
    }
}
