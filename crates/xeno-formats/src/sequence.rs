//! Cutscene sequence entries: the 26-byte records in `xeno.exe`'s data segment that
//! drive the `.vcl` player (`docs/formats/vcl.md`, "How the game shows a picture").
//! They are game data, so they are read from the user's own `xeno.exe` at run time,
//! unpacked with [`crate::lzexe`], and never stored in this repository.

use binrw::{BinRead, binread};

use crate::FormatError;
use crate::lzexe::Unpacked;

/// One sequence entry. All words are u16 LE; times are in retraces of the 320x240 mode
/// (59.94 a second) unless noted.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// `.vcl` file id ([`vcl_name`]).
    pub file_id: u16,
    /// Window origin in picture pixels.
    pub x: u16,
    pub y: u16,
    /// Effect handler, a byte offset into the player's table ([`Entry::effect`]).
    pub effect: u16,
    /// Hold: 60 Hz ticks. Pan: one-pixel steps.
    pub length: u16,
    /// Caption: offset of a NUL-terminated string in the host's string segment.
    pub caption: u16,
    /// Caption position in picture pixels.
    pub caption_x: u16,
    pub caption_y: u16,
    /// The retrace count (reset when a song starts) the picture is held until.
    pub end_time: u16,
    pub fade_in: u16,
    /// Colours 0..=251 fade out over this many retraces, then all 256 over half.
    pub fade_out: u16,
    /// Retraces to wait after each caption character.
    pub caption_speed: u16,
    /// Retraces to wait before each pan step.
    pub pan_speed: u16,
}

/// What the picture does between its fade-in and its caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Wait [`Entry::length`] 60 Hz ticks.
    Hold,
    /// Move the window [`Entry::length`] times by one pixel.
    Pan { dx: i16, dy: i16 },
}

impl Entry {
    pub const SIZE: usize = 26;

    /// The effect, from the handler table at `1fe6:90e8`: 0 holds, 2 and 10 pan right,
    /// 4 left, 6 up and 8 down. `None` for anything else.
    pub fn effect(&self) -> Option<Effect> {
        let pan = |dx, dy| Some(Effect::Pan { dx, dy });
        match self.effect {
            0 => Some(Effect::Hold),
            2 | 10 => pan(1, 0),
            4 => pan(-1, 0),
            6 => pan(0, -1),
            8 => pan(0, 1),
            _ => None,
        }
    }

    /// The picture's file name.
    pub fn file_name(&self) -> Option<&'static str> {
        vcl_name(self.file_id)
    }
}

/// The `.vcl` names in `mgavideo.dvr`'s file-id order, ids 0x16, 0x18, ... 0x32
/// (`vcl.md`). `hlose.vcl` is not in a stock install.
pub const VCL_NAMES: [&str; 15] = [
    "win.vcl",
    "hlose.vcl",
    "nlose.vcl",
    "slose.vcl",
    "triumph.vcl",
    "ealogo.vcl",
    "presents.vcl",
    "detect.vcl",
    "arrival.vcl",
    "grndfall.vcl",
    "emerge.vcl",
    "war.vcl",
    "annihil.vcl",
    "aftermth.vcl",
    "return.vcl",
];

/// The `.vcl` file with id `id`, if it is one.
pub fn vcl_name(id: u16) -> Option<&'static str> {
    let index = id.checked_sub(0x16).filter(|d| d % 2 == 0)? / 2;
    VCL_NAMES.get(usize::from(index)).copied()
}

/// `DS` offset of the start-up chain (`1000:fd0a`): [`INTRO_ENTRIES`] entries back to
/// back, `EALOGO` first and `PRESENTS` last.
pub const INTRO_CHAIN: u16 = 0xecac;
pub const INTRO_ENTRIES: usize = 10;

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// Decodes one entry: exactly [`Entry::SIZE`] bytes, a known `.vcl` and effect.
pub fn parse(bytes: &[u8]) -> Result<Entry, FormatError> {
    if bytes.len() != Entry::SIZE {
        return Err(FormatError::WrongLength {
            expected: Entry::SIZE,
            actual: bytes.len(),
        });
    }
    let entry = Entry::read(&mut std::io::Cursor::new(bytes))?;
    if entry.file_name().is_none() {
        return Err(invalid(
            0,
            format!("{:#x} is not a .vcl file id", entry.file_id),
        ));
    }
    if entry.effect().is_none() {
        return Err(invalid(6, format!("unknown effect {:#x}", entry.effect)));
    }
    Ok(entry)
}

/// Byte offset of `xeno.exe`'s data segment in its load image. The program starts by
/// loading `DS` from an immediate (`mov ax, seg` then `mov ds, ax`, with a relocation
/// on the immediate), so it is read from there instead of being assumed.
pub fn data_segment(exe: &Unpacked) -> Result<usize, FormatError> {
    let entry = Unpacked::linear(exe.cs, exe.ip);
    match exe.image.get(entry..entry + 5) {
        Some(&[0xb8, low, high, 0x8e, 0xd8])
            if exe.relocations.iter().any(|r| r.linear() == entry + 1) =>
        {
            Ok(usize::from(u16::from_le_bytes([low, high])) * 16)
        }
        _ => Err(invalid(
            entry,
            "the entry point does not load DS from a segment",
        )),
    }
}

/// The entry at image offset `at`, named `what` in errors, whose offsets are into the
/// image.
pub(crate) fn entry_at(image: &[u8], at: usize, what: &str) -> Result<Entry, FormatError> {
    let bytes = image
        .get(at..at + Entry::SIZE)
        .ok_or_else(|| invalid(at, format!("the image ends inside {what}")))?;
    parse(bytes).map_err(|e| match e {
        FormatError::Invalid { offset, reason } => {
            invalid(at + offset as usize, format!("{what}: {reason}"))
        }
        other => other,
    })
}

/// The start-up chain, read from an unpacked `xeno.exe`. Error offsets are into the
/// load image.
pub fn intro_chain(exe: &Unpacked) -> Result<Vec<Entry>, FormatError> {
    let start = data_segment(exe)? + usize::from(INTRO_CHAIN);
    (0..INTRO_ENTRIES)
        .map(|i| {
            entry_at(
                &exe.image,
                start + i * Entry::SIZE,
                &format!("intro entry {i}"),
            )
        })
        .collect()
}

/// The image offset of Ghidra's segment `1fe6`, the kernel.
const KERNEL: usize = 0xfe60;
/// The `DS` word the player reads the caption's segment from.
pub const STRINGS_WORD: u16 = 0x60;
/// At start-up the kernel points `DS:0x64` at a segment of fix-ups (`1fe6:00a3`, `mov
/// word [0x64], seg`), then installs the word table at offset `SI` in it
/// (`1fe6:00a9`, `xor bx, bx; mov si, offset`, then `drv_install_word_table`): a base
/// word, then (`DS` offset, segment) pairs up to a 0 offset. One of them sets
/// [`STRINGS_WORD`].
pub(crate) const FIXUPS_SEGMENT: (usize, &[u8]) = (KERNEL + 0xa3, &[0xc7, 0x06, 0x64, 0x00]);
pub(crate) const FIXUPS_TABLE: (usize, &[u8]) = (KERNEL + 0xa9, &[0x33, 0xdb, 0xbe]);
/// No fix-up table is longer than this, and no caption.
const MAX_FIXUPS: usize = 64;
pub const MAX_CAPTION: usize = 1024;

/// Image offset of the string segment that `DS:0x60` holds at run time, from the
/// start-up fix-ups. The segment must be a word the executable relocates.
pub fn string_segment(exe: &Unpacked) -> Result<usize, FormatError> {
    let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
    string_segment_image(&exe.image, &relocated)
}

pub(crate) fn string_segment_image(
    image: &[u8],
    relocated: &[usize],
) -> Result<usize, FormatError> {
    use crate::farewell::operand;
    use crate::maproom::{segment, word};
    let (at, op) = FIXUPS_TABLE;
    let table = segment(image, FIXUPS_SEGMENT, relocated)? + usize::from(operand(image, at, op)?);
    // The base is the load segment itself, so the segments are stored as they are.
    if word(image, table)? != 0 || !relocated.contains(&table) {
        return Err(invalid(
            table,
            "not a fix-up table based at the load segment",
        ));
    }
    for at in (table + 2..).step_by(4).take(MAX_FIXUPS) {
        match word(image, at)? {
            0 => break,
            STRINGS_WORD if relocated.contains(&(at + 2)) => {
                return Ok(usize::from(word(image, at + 2)?) * 16);
            }
            STRINGS_WORD => return Err(invalid(at + 2, "the string segment is not relocated")),
            _ => {}
        }
    }
    Err(invalid(table, "no fix-up sets the string segment"))
}

/// The raw bytes from `offset` in the string segment at image offset `strings`: up to
/// [`MAX_CAPTION`] bytes (fewer at the image's end), with a NUL among them.
pub fn caption_bytes(image: &[u8], strings: usize, offset: u16) -> Result<&[u8], FormatError> {
    let at = strings + usize::from(offset);
    let bytes = image
        .get(at..(at + MAX_CAPTION).min(image.len()))
        .filter(|b| b.contains(&0))
        .ok_or_else(|| invalid(at, "no caption ends here"))?;
    Ok(bytes)
}

/// A caption up to its NUL, without it: the codes the player types.
pub fn caption_text(bytes: &[u8]) -> &[u8] {
    bytes.split(|&b| b == 0).next().unwrap_or_default()
}

/// Each entry's caption, read from the string segment.
pub fn captions(exe: &Unpacked, entries: &[Entry]) -> Result<Vec<Vec<u8>>, FormatError> {
    let strings = string_segment(exe)?;
    entries
        .iter()
        .map(|e| Ok(caption_text(caption_bytes(&exe.image, strings, e.caption)?).to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lzexe::Relocation;

    /// A made-up entry: `presents.vcl`, panning up from (0, 160) in 160 steps.
    fn entry_bytes() -> Vec<u8> {
        [0x22u16, 0, 160, 6, 160, 0x100, 10, 20, 1000, 32, 64, 2, 0]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    #[test]
    fn a_synthetic_entry_decodes() {
        let entry = parse(&entry_bytes()).unwrap();
        assert_eq!(entry.file_name(), Some("presents.vcl"));
        assert_eq!(entry.effect(), Some(Effect::Pan { dx: 0, dy: -1 }));
        assert_eq!(
            (entry.end_time, entry.fade_in, entry.fade_out),
            (1000, 32, 64)
        );
        insta::assert_debug_snapshot!(entry);
    }

    #[test]
    fn file_ids_and_effects() {
        assert_eq!(vcl_name(0x16), Some("win.vcl"));
        assert_eq!(vcl_name(0x20), Some("ealogo.vcl"));
        assert_eq!(vcl_name(0x32), Some("return.vcl"));
        for id in [0, 0x15, 0x17, 0x34] {
            assert_eq!(vcl_name(id), None, "{id:#x}");
        }
        let with = |effect| Entry {
            effect,
            ..parse(&entry_bytes()).unwrap()
        };
        assert_eq!(with(0).effect(), Some(Effect::Hold));
        assert_eq!(with(10).effect(), with(2).effect());
        assert_eq!(with(4).effect(), Some(Effect::Pan { dx: -1, dy: 0 }));
        assert_eq!(with(8).effect(), Some(Effect::Pan { dx: 0, dy: 1 }));
        assert_eq!(with(3).effect(), None);
    }

    #[test]
    fn bad_entries_are_rejected() {
        assert!(matches!(
            parse(&[0; 25]),
            Err(FormatError::WrongLength { expected: 26, .. })
        ));
        let mut bad = entry_bytes();
        bad[0] = 0x17;
        assert!(matches!(
            parse(&bad),
            Err(FormatError::Invalid { offset: 0, .. })
        ));
        let mut bad = entry_bytes();
        bad[6] = 12;
        assert!(matches!(
            parse(&bad),
            Err(FormatError::Invalid { offset: 6, .. })
        ));
    }

    /// An image whose entry point at 0010:0002 loads DS = 0x20, with the chain at
    /// DS:0xecac.
    fn exe() -> Unpacked {
        let ds = 0x20usize;
        let mut image = vec![0u8; ds * 16 + usize::from(INTRO_CHAIN) + 10 * 26];
        image[0x102..0x107].copy_from_slice(&[0xb8, ds as u8, 0, 0x8e, 0xd8]);
        let chain = ds * 16 + usize::from(INTRO_CHAIN);
        for i in 0..INTRO_ENTRIES {
            image[chain + i * 26..][..26].copy_from_slice(&entry_bytes());
        }
        Unpacked {
            image,
            relocations: vec![Relocation {
                segment: 0x10,
                offset: 3,
            }],
            cs: 0x10,
            ip: 2,
            ss: 0,
            sp: 0,
        }
    }

    #[test]
    fn the_chain_is_found_through_the_data_segment() {
        let mut exe = exe();
        assert_eq!(data_segment(&exe).unwrap(), 0x200);
        let chain = intro_chain(&exe).unwrap();
        assert_eq!(chain.len(), INTRO_ENTRIES);
        assert!(chain.iter().all(|e| e.file_name() == Some("presents.vcl")));

        let last = exe.image.len() - 26;
        exe.image[last] = 0x99;
        assert!(
            matches!(intro_chain(&exe), Err(FormatError::Invalid { offset, reason }) if offset as usize == last && reason.starts_with("intro entry 9"))
        );
        exe.image.truncate(last + 10);
        assert!(intro_chain(&exe).is_err());
        // Without the relocation the immediate is not a segment.
        exe.relocations.clear();
        assert!(data_segment(&exe).is_err());
    }

    /// An image whose start-up fix-ups (table at 0x3000:0x10) set `DS:0x5c` and then
    /// `DS:0x60` to segment 0x2000, which holds two captions at 0x10 and 0x20.
    pub(crate) fn strings_image() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; 0x30100];
        let (seg, table) = (FIXUPS_SEGMENT.0 + 4, 0x30010);
        image[FIXUPS_SEGMENT.0..seg].copy_from_slice(FIXUPS_SEGMENT.1);
        image[seg..seg + 2].copy_from_slice(&0x3000u16.to_le_bytes());
        image[FIXUPS_TABLE.0..FIXUPS_TABLE.0 + 3].copy_from_slice(FIXUPS_TABLE.1);
        image[FIXUPS_TABLE.0 + 3] = 0x10;
        for (i, w) in [0u16, 0x5c, 0x1000, 0x60, 0x2000, 0]
            .into_iter()
            .enumerate()
        {
            image[table + 2 * i..][..2].copy_from_slice(&w.to_le_bytes());
        }
        image[0x20010..0x20015].copy_from_slice(b"AB\x09\x03C");
        image[0x20020..0x20023].copy_from_slice(b"D\x0cE");
        (image, vec![seg, table, table + 4, table + 8])
    }

    #[test]
    fn the_string_segment_and_captions_are_found() {
        let (image, relocated) = strings_image();
        let strings = string_segment_image(&image, &relocated).unwrap();
        assert_eq!(strings, 0x20000);
        let text = |offset| caption_text(caption_bytes(&image, strings, offset).unwrap());
        assert_eq!(text(0x10), b"AB\x09\x03C");
        assert_eq!(text(0x20), b"D\x0cE");
        assert_eq!(text(0x30), b"");
        // A caption with no NUL before the image ends.
        let mut open = image.clone();
        open[0x20030..].fill(b'X');
        assert!(caption_bytes(&open, strings, 0x30).is_err());
        // The segment word, the table's base and the string segment must be relocated.
        for skip in 0..3 {
            let mut fewer = relocated.clone();
            fewer.remove(if skip == 2 { 3 } else { skip });
            assert!(string_segment_image(&image, &fewer).is_err(), "{skip}");
        }
        // Another instruction, or a table without the word.
        let mut other = image.clone();
        other[FIXUPS_TABLE.0] = 0x90;
        assert!(string_segment_image(&other, &relocated).is_err());
        let mut other = image;
        other[0x30010 + 2 + 4] = 0x62;
        assert!(string_segment_image(&other, &relocated).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_intro_chain_is_found() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        // `docs/re-setup.md`: DS is file offset 0x19c10 less the 0xc00-byte header.
        assert_eq!(data_segment(&exe).unwrap(), 0x19010);
        let chain = intro_chain(&exe).unwrap();
        let names: Vec<_> = chain.iter().map(|e| e.file_name().unwrap()).collect();
        assert_eq!(names.first(), Some(&"ealogo.vcl"));
        assert_eq!(names.last(), Some(&"presents.vcl"));
        // Every picture is in a stock install, and the end times follow the music.
        assert!(
            names
                .iter()
                .all(|n| crate::files::EXPECTED_FILES.contains(n))
        );
        assert!(chain.windows(2).all(|w| w[0].end_time < w[1].end_time));
        for logo in [chain[0], chain[9]] {
            assert_eq!((logo.x, logo.y, logo.effect()), (0, 0, Some(Effect::Hold)));
        }
        // `DS:0x60` is the string segment the other tables use (`docs/re-setup.md`), and
        // only the logos have no caption.
        let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
        assert_eq!(
            string_segment(&exe).unwrap(),
            crate::maproom::segment(&exe.image, crate::maproom::STRINGS_SEGMENT, &relocated)
                .unwrap()
        );
        let captions = captions(&exe, &chain).unwrap();
        assert!(captions[0].is_empty() && captions[9].is_empty());
        assert!(captions[1..9].iter().all(|c| !c.is_empty()));
    }
}
