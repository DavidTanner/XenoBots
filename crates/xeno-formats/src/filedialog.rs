//! The host's file dialogs (T-110; `docs/screens.md`, "The file dialogs (T-110)"): the
//! open and SAVE GAME dialogs and their error box, read from the user's own `xeno.exe`
//! at run time and never stored in this repository. Each is a widget set and a drawing
//! script; the name field's box and the two patterns (the open dialog's first name and
//! the directories' search) are read too.
//!
//! As in `crate::maproom`, each table is found from the instruction that uses it after
//! checking that instruction's bytes, and the widget set's segment only from a word the
//! executable relocates.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, segment, string, word};
use crate::sequence;

/// The image offset of Ghidra's segment `1fe6`, the kernel.
const KERNEL: usize = 0xfe60;
/// `ui_open_file_dialog` (`1fe6:83aa`): `mov ax, script`, then `mov di, field; mov si,
/// pattern`, and at +0x1b and +0x1e `mov bx, first; mov dx, end`.
pub(crate) const OPEN: usize = KERNEL + 0x83aa;
/// `ui_save_file_dialog` (`1fe6:83cd`): `mov ax, script`, then at +6 and +9 `mov bx,
/// first; mov dx, end`.
pub(crate) const SAVE: usize = KERNEL + 0x83cd;
/// `ui_run_file_dialog` (`1fe6:83db`): `mov ax, set`.
pub(crate) const RUN: usize = KERNEL + 0x83db;
/// The error box (`1fe6:8f1f`): `mov ax, script`, and at +0xb `mov ax, set; mov bx,
/// first; xor cx, cx; mov dx, end`.
pub(crate) const ERROR: usize = KERNEL + 0x8f1f;
/// The name field's box (`mov bx, rect` at `1fe6:88b9`) and the directories' pattern
/// (`mov dx, pattern` at `1fe6:8a41`).
pub(crate) const NAME_FIELD: (usize, &[u8]) = (KERNEL + 0x88b9, &[0xbb]);
pub(crate) const DIRECTORY_PATTERN: (usize, &[u8]) = (KERNEL + 0x8a41, &[0xba]);

/// Widget types (offsets into the host's handler tables).
const GROUP_TYPE: u16 = 0x0e;
const KEY_TYPE: u16 = 0x1a;
const BACKDROP_TYPE: u16 = 0x15c;
const OK_TYPE: u16 = 0x15e;
const CANCEL_TYPE: u16 = 0x160;
const UP_TYPE: u16 = 0x164;
const TRACK_TYPE: u16 = 0x166;
const DOWN_TYPE: u16 = 0x168;
const ERROR_OK_TYPE: u16 = 0x16a;
const FILES_TYPE: u16 = 0x16c;
const DIRECTORIES_TYPE: u16 = 0x16e;

const WIDGET_SIZE: usize = 12;
const MAX_WIDGETS: usize = 32;
/// No script is longer than this many bytes.
const MAX_SCRIPT: usize = 512;

/// One of a dialog's two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    Files,
    Directories,
}

/// What a widget of a dialog is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Ok,
    Cancel,
    Up(List),
    Track(List),
    Down(List),
    List(List),
    /// The error box's OK, drawn as this `msprite0` handle (plus 8 while pressed).
    ErrorOk(u16),
}

/// A drawing script's step (the backdrop's draw handler, `1fe6:8586`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Code 2: the rectangle filled with `colour` (slot `0x51a`).
    Fill { colour: u8, area: Rect },
    /// Code 4: the rectangle's one-pixel frame in colour 0 (slot `0x52a`).
    Frame(Rect),
    /// Code 6: `text` drawn transparent with its first glyph's corner at (`x`, `y`);
    /// the byte after the position, which the code skips, is kept.
    Text {
        x: u16,
        y: u16,
        unknown_skipped: u8,
        text: Vec<u8>,
    },
    /// Code 8: select the font.
    Font(u16),
}

/// One dialog: its script and its widgets that do something, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub script: Vec<Op>,
    pub parts: Vec<(Part, Rect)>,
}

/// The two dialogs, the error box, and the tables they share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogs {
    pub open: Dialog,
    pub save: Dialog,
    pub error: Dialog,
    /// What the open dialog puts in the name field: `*.USG`.
    pub open_pattern: Vec<u8>,
    /// What the name field holds at start-up: `*.*`.
    pub first_name: Vec<u8>,
    /// The pattern the directories are searched with: `*.*`.
    pub directory_pattern: Vec<u8>,
    /// The name field's box, filled before the name is drawn.
    pub name_field: Rect,
}

struct Exe<'a> {
    image: &'a [u8],
    ds: usize,
    set: usize,
}

impl Exe<'_> {
    fn ds_word(&self, at: usize, op: &[u8]) -> Result<usize, FormatError> {
        Ok(self.ds + usize::from(operand(self.image, at, op)?))
    }

    /// A length byte, then that many bytes (the field's and the patterns' form).
    fn counted(&self, at: usize) -> Result<Vec<u8>, FormatError> {
        let len = usize::from(
            *self
                .image
                .get(at)
                .ok_or_else(|| invalid(at, "past the end"))?,
        );
        let bytes = self.image.get(at + 1..at + 1 + len);
        Ok(bytes.ok_or_else(|| invalid(at, "past the end"))?.to_vec())
    }

    fn byte(&self, at: usize) -> Result<u8, FormatError> {
        self.image
            .get(at)
            .copied()
            .ok_or_else(|| invalid(at, "past the end"))
    }

    fn script(&self, at: usize) -> Result<Vec<Op>, FormatError> {
        let mut ops = Vec::new();
        let mut i = at;
        while i < at + MAX_SCRIPT {
            let op = match self.byte(i)? {
                0 => return Ok(ops),
                2 => {
                    i += 10;
                    Op::Fill {
                        colour: self.byte(i - 9)?,
                        area: rect(self.image, i - 8)?,
                    }
                }
                4 => {
                    i += 9;
                    Op::Frame(rect(self.image, i - 8)?)
                }
                6 => {
                    let (at, text) = (i, string(self.image, i + 6)?);
                    i += 7 + text.len();
                    Op::Text {
                        y: word(self.image, at + 1)?,
                        x: word(self.image, at + 3)?,
                        unknown_skipped: self.byte(at + 5)?,
                        text,
                    }
                }
                8 => {
                    i += 3;
                    Op::Font(word(self.image, i - 2)?)
                }
                _ => return Err(invalid(i, "not a dialog script code")),
            };
            ops.push(op);
        }
        Err(invalid(at, "the dialog script does not end"))
    }

    /// The widgets from `first` to `end` that act, by type and state.
    fn parts(&self, first: u16, end: u16) -> Result<Vec<(Part, Rect)>, FormatError> {
        let (first, end) = (usize::from(first), usize::from(end));
        if first > end
            || (end - first) % WIDGET_SIZE != 0
            || end - first > MAX_WIDGETS * WIDGET_SIZE
        {
            return Err(invalid(self.set, "not a list of widgets"));
        }
        let mut parts = Vec::new();
        for at in (self.set + first..self.set + end).step_by(WIDGET_SIZE) {
            let state = self.ds + usize::from(word(self.image, at + 2)?);
            // Which list an arrow or track scrolls: 0 the files, else the directories.
            let list = || -> Result<List, FormatError> {
                Ok(match self.byte(state + 1)? {
                    0 => List::Files,
                    _ => List::Directories,
                })
            };
            let part = match word(self.image, at)? {
                GROUP_TYPE | KEY_TYPE | BACKDROP_TYPE => continue,
                OK_TYPE => Part::Ok,
                CANCEL_TYPE => Part::Cancel,
                UP_TYPE => Part::Up(list()?),
                TRACK_TYPE => Part::Track(list()?),
                DOWN_TYPE => Part::Down(list()?),
                FILES_TYPE => Part::List(List::Files),
                DIRECTORIES_TYPE => Part::List(List::Directories),
                ERROR_OK_TYPE => Part::ErrorOk(word(self.image, state + 3)?),
                _ => return Err(invalid(at, "not a file dialog widget")),
            };
            parts.push((part, rect(self.image, at + 4)?));
        }
        Ok(parts)
    }

    fn dialog(&self, script: usize, (first, end): (u16, u16)) -> Result<Dialog, FormatError> {
        Ok(Dialog {
            script: self.script(script)?,
            parts: self.parts(first, end)?,
        })
    }
}

/// Reads the file dialogs from a load image whose data segment starts at `ds`;
/// `relocated` lists the image offsets of the words the executable relocates. Error
/// offsets are into the load image.
pub fn read_image(
    image: &[u8],
    ds: usize,
    relocated: &[usize],
) -> Result<FileDialogs, FormatError> {
    let set = segment(image, (RUN, &[0xb8]), relocated)?;
    let exe = Exe { image, ds, set };
    if segment(image, (ERROR + 0xb, &[0xb8]), relocated)? != set {
        return Err(invalid(
            ERROR,
            "the error box is not in the dialogs' widget set",
        ));
    }
    let op = |at, code: &[u8]| operand(image, at, code);
    let open = exe.dialog(
        exe.ds_word(OPEN, &[0xb8])?,
        (op(OPEN + 0x1b, &[0xbb])?, op(OPEN + 0x1e, &[0xba])?),
    )?;
    let save = exe.dialog(
        exe.ds_word(SAVE, &[0xb8])?,
        (op(SAVE + 6, &[0xbb])?, op(SAVE + 9, &[0xba])?),
    )?;
    let error = exe.dialog(
        exe.ds_word(ERROR, &[0xb8])?,
        (op(ERROR + 0xe, &[0xbb])?, op(ERROR + 0x13, &[0xba])?),
    )?;
    Ok(FileDialogs {
        open,
        save,
        error,
        open_pattern: exe.counted(exe.ds_word(OPEN + 9, &[0xbe])?)?,
        first_name: exe.counted(exe.ds_word(OPEN + 6, &[0xbf])?)?,
        directory_pattern: string(
            image,
            exe.ds_word(DIRECTORY_PATTERN.0, DIRECTORY_PATTERN.1)?,
        )?,
        name_field: rect(image, exe.ds_word(NAME_FIELD.0, NAME_FIELD.1)?)?,
    })
}

/// Reads the file dialogs from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<FileDialogs, FormatError> {
    let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
    read_image(&exe.image, sequence::data_segment(exe)?, &relocated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_file_dialogs;

    const DS: usize = 0x19010;

    fn synthetic() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; DS + 0x10000];
        let relocated = synthetic_file_dialogs(&mut image, DS);
        (image, relocated)
    }

    #[test]
    fn synthetic_dialogs_read() {
        let (image, relocated) = synthetic();
        let dialogs = read_image(&image, DS, &relocated).unwrap();
        assert_eq!(dialogs.open_pattern, b"*.USG");
        assert_eq!(dialogs.first_name, b"*.*");
        let parts: Vec<Part> = dialogs.save.parts.iter().map(|p| p.0).collect();
        assert_eq!(
            parts,
            [
                Part::Ok,
                Part::Cancel,
                Part::Up(List::Directories),
                Part::Track(List::Directories),
                Part::List(List::Directories)
            ]
        );
        assert_eq!(dialogs.error.parts[0].0, Part::ErrorOk(0x1d0));
        assert!(matches!(&dialogs.save.script[3], Op::Text { text, .. } if text == b"SAVE"));
        insta::assert_debug_snapshot!(dialogs);
    }

    #[test]
    fn a_wrong_instruction_or_code_is_refused() {
        let (mut image, relocated) = synthetic();
        image[RUN] = 0x90;
        assert!(read_image(&image, DS, &relocated).is_err());

        let (mut image, relocated) = synthetic();
        let script = DS + usize::from(operand(&image, SAVE, &[0xb8]).unwrap());
        image[script] = 3;
        assert!(read_image(&image, DS, &relocated).is_err());

        // The set's segment must be relocated.
        let (image, _) = synthetic();
        assert!(read_image(&image, DS, &[]).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_file_dialogs_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let dialogs = read(&exe).unwrap();
        assert_eq!(dialogs.open_pattern, b"*.USG");
        assert_eq!(dialogs.first_name, b"*.*");
        assert_eq!(dialogs.directory_pattern, b"*.*");
        // The open dialog has both lists with their scroll bars, the save dialog the
        // directories only; the error box its OK.
        let count = |d: &Dialog, list| {
            d.parts
                .iter()
                .filter(|(p, _)| matches!(p, Part::Up(l) | Part::Track(l) | Part::Down(l) | Part::List(l) if *l == list))
                .count()
        };
        assert_eq!(count(&dialogs.open, List::Files), 4);
        assert_eq!(count(&dialogs.open, List::Directories), 4);
        assert_eq!(count(&dialogs.save, List::Files), 0);
        assert_eq!(count(&dialogs.save, List::Directories), 4);
        assert!(matches!(dialogs.error.parts[..], [(Part::ErrorOk(_), _)]));
        let texts = |d: &Dialog| {
            d.script
                .iter()
                .filter(|op| matches!(op, Op::Text { .. }))
                .count()
        };
        assert_eq!(
            (
                texts(&dialogs.open),
                texts(&dialogs.save),
                texts(&dialogs.error)
            ),
            (7, 6, 1)
        );
    }
}
