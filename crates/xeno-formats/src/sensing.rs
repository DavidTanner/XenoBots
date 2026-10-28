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
/// Entries in a tick table.
pub const TICKS: usize = 16;

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
    let (mut scan, mut lock, mut locked_on) = (None, None, None);
    for n in 0..TICKS {
        let routine = usize::from(word(image, table + 2 * n)?);
        if routine == SCAN.0 {
            scan = scan.or(Some(n as u8));
        } else if routine == LOCKED_ON.0 {
            locked_on = locked_on.or(Some(n as u8));
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
    match (scan, lock, locked_on) {
        (Some(scan), Some((lock, arc, range)), Some(locked_on)) => Ok(KindTicks {
            scan,
            lock,
            arc,
            range,
            locked_on,
        }),
        _ => Err(invalid(table, "a tick table without the sensing routines")),
    }
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Sensing, FormatError> {
    if !starts(image, SCAN.0, SCAN.1) || !starts(image, LOCKED_ON.0, LOCKED_ON.1) {
        return Err(invalid(SCAN.0, "not the scan or the lock warning"));
    }
    if !starts(image, KEEP_LOCK, &[0xa3]) || !starts(image, KEEP_LOCK + 3, &[0x89, 0x1e]) {
        return Err(invalid(KEEP_LOCK, "not the lock's upkeep"));
    }
    Ok(Sensing {
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
        assert_eq!((t[0].scan, t[0].lock, t[0].locked_on), (15, 5, 6));
        assert_eq!((t[1].lock, t[1].arc, t[1].range), (6, 0x1100, 0x310));
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
            (TICKS_AT[2] + 1, 0x96),
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
    }
}
