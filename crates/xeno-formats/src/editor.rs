//! What the port reads from the user's `xeno.exe` for the scenario editor: its key
//! table, the names and template of its status display, and each path mission's
//! defaults (`docs/screens.md`, "The scenario editor (T-048)"). Game data, read at run
//! time; each table is found through an instruction that uses it, after checking that
//! instruction's bytes.

use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{string, word};
use crate::{FormatError, sequence};

/// `mov bx, table; mov cx, count` in `ui_dispatch_editor_key` (`1000:6a99`).
const KEYS_AT: usize = 0x6a99;
/// `mov si, [bx + names]` in `ui_draw_editor_status`: the kinds' names (by kind, the
/// path's in the word before), the sides', the path display's (by 0 or 2) and the
/// missions'.
const KINDS_AT: usize = 0x6963;
const SIDES_AT: usize = 0x6977;
const SHOWS_AT: usize = 0x699a;
const MISSIONS_AT: usize = 0x69e7;
const MOV_SI_BX: [u8; 2] = [0x8b, 0xb7];
/// `mov al, [bx + table]`: a mix's tenths as a percentage, by tenths.
const MIX_AT: usize = 0x6a09;
/// `mov si, template` in `ui_reset_editor_state` (`1000:692f`).
const TEMPLATE_AT: usize = 0x692f;
/// `mov ax, [bx + table]` in `1000:6ce3`: the four default header bytes by mission.
const DEFAULTS_AT: usize = 0x6cf1;

/// No status template is longer (the stock one is under 200 bytes).
const MAX_TEMPLATE: usize = 512;
/// Kinds with a name slot: the path (-1), then 0 to 16.
pub const KINDS: usize = 18;
/// The missions Ctrl+Space cycles through (the names table has one more, unused).
pub const MISSIONS: usize = 7;

/// The scenario editor's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    /// (key word, handler offset in segment `1000`), in the table's order.
    pub keys: Vec<(u16, u16)>,
    /// The status display's name for each kind, by kind + 1 (the path first); `None`
    /// where the table has no string.
    pub kind_names: Vec<Option<Vec<u8>>>,
    /// Human, alien.
    pub side_names: [Vec<u8>; 2],
    /// The path display off, on.
    pub show_names: [Vec<u8>; 2],
    /// By mission number, 0 to 7.
    pub mission_names: Vec<Vec<u8>>,
    /// Weight and the three mix bytes a new path or mission gets, by mission.
    pub mission_defaults: Vec<[u8; 4]>,
    /// A mix byte (tenths, 0 to 10) as the percentage shown.
    pub mix_percent: Vec<u8>,
    /// The status rows' labels: CR between rows, printed from row 20, column 1.
    pub status: Vec<u8>,
}

fn table(image: &[u8], ds: usize, at: usize, op: &[u8]) -> Result<usize, FormatError> {
    Ok(ds + usize::from(operand(image, at, op)?))
}

/// The strings `count` words from `at` point to (offsets in the data segment).
fn strings(image: &[u8], ds: usize, at: usize, count: usize) -> Result<Vec<Vec<u8>>, FormatError> {
    (0..count)
        .map(|i| string(image, ds + usize::from(word(image, at + 2 * i)?)))
        .collect()
}

/// The status template, up to its NUL (longer than [`string`] reads).
fn template(image: &[u8], at: usize) -> Result<Vec<u8>, FormatError> {
    let bytes = image
        .get(at..(at + MAX_TEMPLATE).min(image.len()))
        .unwrap_or_default();
    let end = bytes.iter().position(|&b| b == 0);
    end.map(|end| bytes[..end].to_vec())
        .ok_or_else(|| crate::maproom::invalid(at, "no status template ends here"))
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Editor, FormatError> {
    let keys_at = table(image, ds, KEYS_AT, &[0xbb])?;
    let count = usize::from(operand(image, KEYS_AT + 3, &[0xb9])?);
    let keys = (0..count)
        .map(|i| {
            Ok((
                word(image, keys_at + 4 * i)?,
                word(image, keys_at + 4 * i + 2)?,
            ))
        })
        .collect::<Result<_, FormatError>>()?;
    let kinds = table(image, ds, KINDS_AT, &MOV_SI_BX)? - 2;
    let kind_names = (0..KINDS)
        .map(|i| match word(image, kinds + 2 * i)? {
            0 => Ok(None),
            at => string(image, ds + usize::from(at)).map(Some),
        })
        .collect::<Result<_, FormatError>>()?;
    let pair = |at| -> Result<[Vec<u8>; 2], FormatError> {
        let [a, b] = <[Vec<u8>; 2]>::try_from(strings(image, ds, at, 2)?).expect("two");
        Ok([a, b])
    };
    let side_names = pair(table(image, ds, SIDES_AT, &MOV_SI_BX)?)?;
    let show_names = pair(table(image, ds, SHOWS_AT, &MOV_SI_BX)?)?;
    let missions = table(image, ds, MISSIONS_AT, &MOV_SI_BX)?;
    let mission_names = strings(image, ds, missions, MISSIONS + 1)?;
    let mix = table(image, ds, MIX_AT, &[0x8a, 0x87])?;
    let defaults = table(image, ds, DEFAULTS_AT, &[0x8b, 0x87])?;
    let bytes = |at: usize, n: usize| {
        image
            .get(at..at + n)
            .ok_or_else(|| crate::maproom::invalid(at, "past the end of the image"))
    };
    let mission_defaults = bytes(defaults, 4 * MISSIONS)?
        .chunks(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    Ok(Editor {
        keys,
        kind_names,
        side_names,
        show_names,
        mission_names,
        mission_defaults,
        mix_percent: bytes(mix, 11)?.to_vec(),
        status: template(image, table(image, ds, TEMPLATE_AT, &[0xbe])?)?,
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Editor, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x8000;

    /// Writes `bytes` at `at`.
    fn put(image: &mut [u8], at: usize, bytes: &[u8]) {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// A made-up editor: two keys, names for the path and kind 0 only, made-up
    /// strings and tables in the data segment.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x400];
        put(&mut image, KEYS_AT, &[0xbb, 0x00, 0x01, 0xb9, 2, 0]);
        put(
            &mut image,
            DS + 0x100,
            &[0x30, 0x10, 0x08, 0x6b, 0x14, 0x10, 0x02, 0x6b],
        );
        put(&mut image, KINDS_AT, &[0x8b, 0xb7, 0x22, 0x01]);
        put(&mut image, DS + 0x120, &[0x80, 0x01, 0x84, 0x01]);
        put(&mut image, DS + 0x180, b"WAY\0MAN\0");
        put(&mut image, SIDES_AT, &[0x8b, 0xb7, 0x50, 0x01]);
        put(&mut image, SHOWS_AT, &[0x8b, 0xb7, 0x50, 0x01]);
        put(&mut image, DS + 0x150, &[0x80, 0x01, 0x84, 0x01]);
        put(&mut image, MISSIONS_AT, &[0x8b, 0xb7, 0x60, 0x01]);
        for i in 0..8 {
            image[DS + 0x160 + 2 * i] = 0x84;
            image[DS + 0x161 + 2 * i] = 0x01;
        }
        put(&mut image, MIX_AT, &[0x8a, 0x87, 0x00, 0x02]);
        put(
            &mut image,
            DS + 0x200,
            &[0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 99],
        );
        put(&mut image, DEFAULTS_AT, &[0x8b, 0x87, 0x20, 0x02]);
        put(&mut image, DS + 0x220, &[1, 2, 3, 5]);
        put(&mut image, TEMPLATE_AT, &[0xbe, 0x00, 0x03]);
        put(&mut image, DS + 0x300, b"KIND:\rSIDE:\0");
        image
    }

    #[test]
    fn a_synthetic_editor_reads() {
        let editor = read_image(&synthetic(), DS).unwrap();
        assert_eq!(editor.keys, [(0x1030, 0x6b08), (0x1014, 0x6b02)]);
        assert_eq!(editor.kind_names.len(), KINDS);
        assert_eq!(editor.kind_names[0].as_deref(), Some(&b"WAY"[..]));
        assert_eq!(editor.kind_names[1].as_deref(), Some(&b"MAN"[..]));
        assert!(editor.kind_names[2..].iter().all(Option::is_none));
        assert_eq!(editor.side_names, [b"WAY".to_vec(), b"MAN".to_vec()]);
        assert_eq!(editor.mission_names.len(), MISSIONS + 1);
        assert_eq!(editor.mission_defaults[0], [1, 2, 3, 5]);
        assert_eq!(editor.mix_percent[10], 99);
        assert_eq!(editor.status, b"KIND:\rSIDE:");
        insta::assert_debug_snapshot!(editor);
    }

    #[test]
    fn another_build_is_refused() {
        let mut image = synthetic();
        image[KEYS_AT] = 0x90;
        assert!(read_image(&image, DS).is_err());
        let mut image = synthetic();
        image[MIX_AT + 1] = 0x86;
        assert!(read_image(&image, DS).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_editor_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let editor = read(&exe).unwrap();
        assert_eq!(editor.keys.len(), 31);
        // Every key is a Ctrl (or Alt+F1) key, and the names are the ones the format
        // docs list (kinds 0 to 4 and 16, and the path).
        assert!(editor.keys.iter().all(|&(k, _)| k & 0x3000 != 0));
        let named: Vec<usize> = (0..KINDS)
            .filter(|&i| editor.kind_names[i].is_some())
            .collect();
        assert_eq!(named, [0, 1, 2, 3, 4, 5, 17]);
        assert_eq!(editor.mission_defaults.len(), MISSIONS);
        assert_eq!(editor.mix_percent[0], 0);
        assert_eq!(editor.status.iter().filter(|&&b| b == b'\r').count(), 4);
    }
}
