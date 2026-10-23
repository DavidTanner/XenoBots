//! The cutscenes after a mission (`docs/formats/vcl.md`, "Sequences"): the `WIN`
//! entry, the three lose entries chosen by the robot console last opened, `TRIUMPH`,
//! their captions, and the number fields the win routine writes into its caption.
//! They are game data, so they are read from the user's own `xeno.exe` at run time and
//! never stored in this repository.
//!
//! As in [`crate::maproom`], each table is found from the instruction that uses it,
//! after checking that instruction's bytes, so another build of the game is refused
//! rather than misread.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};
use crate::sequence::{self, Entry};

/// Load-image offsets of the instructions read, and the bytes before each operand.
/// `ui_play_win_cutscene` (`1000:7059`): `mov si, entry` before the player.
pub(crate) const WIN_ENTRY: (usize, &[u8]) = (0x70ba, &[0xbe]);
/// `ui_play_lose_cutscene` (`1000:702d`): `mov bx, [selector]; mov si, [bx + table]`.
pub(crate) const LOSE_SELECTOR: (usize, &[u8]) = (0x703e, &[0x8b, 0x1e]);
pub(crate) const LOSE_TABLE: (usize, &[u8]) = (0x7042, &[0x8b, 0xb7]);
/// `ui_play_triumph_sequence` (`1000:fd9f`): `mov si, entry` before the player.
pub(crate) const TRIUMPH_ENTRY: (usize, &[u8]) = (0xfdc4, &[0xbe]);
/// `scen_score_mission_result` (`1000:7626`): `mul word [per kill]`, `mov [points],
/// ax`, `mul word [per loss]`.
pub(crate) const PER_KILL: (usize, &[u8]) = (0x7637, &[0xf7, 0x26]);
pub(crate) const POINTS: (usize, &[u8]) = (0x763b, &[0xa3]);
pub(crate) const PER_LOSS: (usize, &[u8]) = (0x7640, &[0xf7, 0x26]);
/// The win routine formats each number into a buffer (`mov si, buffer`), then points
/// `ES` at the string segment (`mov es, [0x60]`) and copies words of it into the
/// caption (`mov ax, [buffer + n]; mov es:[field], ax`). The points are read from
/// `mov ax, [points]`.
pub(crate) const BUFFERS: [(usize, &[u8]); 3] =
    [(0x706f, &[0xbe]), (0x7081, &[0xbe]), (0x7095, &[0xbe])];
pub(crate) const STRINGS_LOADS: [usize; 3] = [0x7075, 0x7087, 0x709b];
pub(crate) const LOAD_STRINGS: &[u8] = &[0x8e, 0x06, 0x60, 0x00];
pub(crate) const WIN_POINTS: (usize, &[u8]) = (0x7092, &[0xa1]);
/// (value, `mov ax, [buffer + n]`, `mov es:[field], ax`) for each word copied.
pub(crate) const COPIES: [(Value, usize, usize); 5] = [
    (Value::Kills, 0x7079, 0x707c),
    (Value::Losses, 0x708b, 0x708e),
    (Value::Points, 0x709f, 0x70a2),
    (Value::Points, 0x70a6, 0x70a9),
    (Value::Points, 0x70ad, 0x70b0),
];
const LOAD: &[u8] = &[0xa1];
const STORE: &[u8] = &[0x26, 0xa3];

/// The robot types that pick a lose entry: the consoles store 0, 2 or 4 in the
/// selector word (`DS:0x6012`).
pub const LOSE_ENTRIES: usize = 3;
/// A number is formatted as five digits and a NUL (`txt_format_u16_to_buffer`).
pub const NUMBER_BYTES: usize = 6;

/// What a field of the win caption shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// The enemy robots the player destroyed in the mission (a byte of the player's
    /// command object, `sim_get_mission_kills_and_losses`).
    Kills,
    /// The player's own robots killed (another byte of it).
    Losses,
    /// The mission's points (`DS:0x6014`, from `scen_score_mission_result`).
    Points,
}

/// One word the win routine copies into its caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub value: Value,
    /// Byte offset of the word in the number's [`NUMBER_BYTES`]-byte buffer.
    pub from: u16,
    /// Offset in the string segment it is written to.
    pub to: u16,
}

/// The cutscenes after a mission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endings {
    pub win: Entry,
    /// By the selector word / 2 (`DS:0x6016`).
    pub lose: [Entry; LOSE_ENTRIES],
    pub triumph: Entry,
    /// `DS` offset of the selector word.
    pub lose_selector: u16,
    /// The win caption's number fields, in the order written.
    pub fields: Vec<Field>,
    /// Points for each kill and off for each loss.
    pub per_kill: u16,
    pub per_loss: u16,
    /// Each caption's bytes from the string segment, with its NUL: win, lose by index,
    /// triumph.
    win_bytes: Vec<u8>,
    lose_bytes: [Vec<u8>; LOSE_ENTRIES],
    triumph_bytes: Vec<u8>,
}

/// What the win caption's fields show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Numbers {
    pub kills: u8,
    pub losses: u8,
    pub points: u16,
}

/// `n` as `txt_format_u16_to_buffer` writes it: five decimal digits with leading zeros
/// (the host prints them all), then a NUL.
pub fn digits(n: u16) -> [u8; NUMBER_BYTES] {
    let mut out = [0; NUMBER_BYTES];
    for (i, power) in [10000u16, 1000, 100, 10, 1].into_iter().enumerate() {
        out[i] = b'0' + (n / power % 10) as u8;
    }
    out
}

impl Endings {
    /// The mission's points (`scen_score_mission_result`): kills times their weight
    /// less losses times theirs, in 16 bits, and 0 if that would go below 0.
    pub fn points(&self, kills: u8, losses: u8) -> u16 {
        let gained = u16::from(kills).wrapping_mul(self.per_kill);
        let lost = u16::from(losses).wrapping_mul(self.per_loss);
        gained.saturating_sub(lost)
    }

    /// The win caption with its fields filled in, up to its NUL.
    pub fn win_caption(&self, numbers: Numbers) -> Vec<u8> {
        let mut bytes = self.win_bytes.clone();
        for copy in &self.fields {
            let value = match copy.value {
                Value::Kills => u16::from(numbers.kills),
                Value::Losses => u16::from(numbers.losses),
                Value::Points => numbers.points,
            };
            let from = usize::from(copy.from);
            let to = usize::from(copy.to - self.win.caption);
            bytes[to..to + 2].copy_from_slice(&digits(value)[from..from + 2]);
        }
        sequence::caption_text(&bytes).to_vec()
    }

    /// The lose caption for selector index `index` (0 to 2).
    pub fn lose_caption(&self, index: usize) -> Vec<u8> {
        sequence::caption_text(&self.lose_bytes[index]).to_vec()
    }

    pub fn triumph_caption(&self) -> Vec<u8> {
        sequence::caption_text(&self.triumph_bytes).to_vec()
    }
}

/// Reads the endings from a load image whose data segment starts at `ds` and whose
/// string segment starts at `strings`. Error offsets are into the load image.
pub fn read_image(image: &[u8], ds: usize, strings: usize) -> Result<Endings, FormatError> {
    let op = |(at, op): (usize, &[u8])| operand(image, at, op);
    let entry = |offset: u16, what: &str| sequence::entry_at(image, ds + usize::from(offset), what);
    let raw = |e: &Entry| {
        let text = sequence::caption_text(sequence::caption_bytes(image, strings, e.caption)?);
        Ok::<_, FormatError>([text, &[0]].concat())
    };

    let win = entry(op(WIN_ENTRY)?, "the win entry")?;
    let triumph = entry(op(TRIUMPH_ENTRY)?, "the triumph entry")?;
    let table = op(LOSE_TABLE)?;
    let mut lose = [win; LOSE_ENTRIES];
    for (i, slot) in lose.iter_mut().enumerate() {
        let at = table.wrapping_add(2 * i as u16);
        *slot = entry(
            word(image, ds + usize::from(at))?,
            &format!("lose entry {i}"),
        )?;
    }

    let points = op(POINTS)?;
    if op(WIN_POINTS)? != points {
        return Err(invalid(
            WIN_POINTS.0,
            "the win caption does not show the points",
        ));
    }
    let win_bytes = raw(&win)?;
    let mut fields = Vec::with_capacity(COPIES.len());
    for (i, &(value, load, store)) in COPIES.iter().enumerate() {
        // The first two numbers are copied once each, the points three times.
        let number = i.min(2);
        let buffer = op(BUFFERS[number])?;
        let es = STRINGS_LOADS[number];
        if image.get(es..es + LOAD_STRINGS.len()) != Some(LOAD_STRINGS) {
            return Err(invalid(es, "ES is not the string segment"));
        }
        let from = op((load, LOAD))?.wrapping_sub(buffer);
        let to = op((store, STORE))?;
        // A word of the buffer, into the caption before its NUL.
        let text = &win_bytes[..win_bytes.len() - 1];
        let within = to
            .checked_sub(win.caption)
            .and_then(|at| text.get(usize::from(at)..usize::from(at) + 2));
        if usize::from(from) + 2 > NUMBER_BYTES || within.is_none() {
            return Err(invalid(store, "not a field of the win caption"));
        }
        fields.push(Field { value, from, to });
    }

    let lose_bytes = [raw(&lose[0])?, raw(&lose[1])?, raw(&lose[2])?];
    Ok(Endings {
        win,
        lose,
        triumph,
        lose_selector: op(LOSE_SELECTOR)?,
        fields,
        per_kill: word(image, ds + usize::from(op(PER_KILL)?))?,
        per_loss: word(image, ds + usize::from(op(PER_LOSS)?))?,
        win_bytes,
        lose_bytes,
        triumph_bytes: raw(&triumph)?,
    })
}

/// Reads the endings from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<Endings, FormatError> {
    read_image(
        &exe.image,
        sequence::data_segment(exe)?,
        sequence::string_segment(exe)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x20000;
    const STRINGS: usize = 0x30000;

    fn put(image: &mut [u8], at: usize, bytes: &[u8]) {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn op(image: &mut [u8], (at, op): (usize, &[u8]), value: u16) {
        put(image, at, op);
        put(image, at + op.len(), &value.to_le_bytes());
    }

    /// A made-up entry for `file_id` with its caption at `caption`.
    fn entry(file_id: u16, caption: u16) -> Vec<u8> {
        [file_id, 0, 0, 0, 1, caption, 10, 20, 100, 4, 4, 2, 0]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    /// Win at `DS:0x100`, two lose entries at 0x120 and 0x13a (the first used twice),
    /// triumph at 0x180, the lose table at 0x1c0, weights 7 and 3, the points at
    /// 0x204 and the number buffer at 0x300. The win caption has three fields.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; STRINGS + 0x100];
        put(&mut image, DS + 0x100, &entry(0x16, 0x10));
        put(&mut image, DS + 0x120, &entry(0x1c, 0x40));
        put(&mut image, DS + 0x13a, &entry(0x1a, 0x48));
        put(&mut image, DS + 0x180, &entry(0x1e, 0x50));
        for (i, at) in [0x120u16, 0x13a, 0x120].into_iter().enumerate() {
            put(&mut image, DS + 0x1c0 + 2 * i, &at.to_le_bytes());
        }
        put(&mut image, DS + 0x200, &[7, 0, 3, 0]);
        put(&mut image, STRINGS + 0x10, b"K=00\x0cL=00\x0cP=      \0");
        put(&mut image, STRINGS + 0x40, b"LOST\0");
        put(&mut image, STRINGS + 0x48, b"GONE\0");
        put(&mut image, STRINGS + 0x50, b"\x09\x05ALL\0");
        op(&mut image, WIN_ENTRY, 0x100);
        op(&mut image, TRIUMPH_ENTRY, 0x180);
        op(&mut image, LOSE_SELECTOR, 0x6012);
        op(&mut image, LOSE_TABLE, 0x1c0);
        op(&mut image, PER_KILL, 0x200);
        op(&mut image, PER_LOSS, 0x202);
        op(&mut image, POINTS, 0x204);
        op(&mut image, WIN_POINTS, 0x204);
        for (buffer, es) in BUFFERS.into_iter().zip(STRINGS_LOADS) {
            op(&mut image, buffer, 0x300);
            put(&mut image, es, LOAD_STRINGS);
        }
        for ((_, load, store), (from, to)) in
            COPIES
                .into_iter()
                .zip([(3, 0x12), (3, 0x17), (0, 0x1c), (2, 0x1e), (4, 0x20)])
        {
            op(&mut image, (load, LOAD), 0x300 + from);
            op(&mut image, (store, STORE), to);
        }
        image
    }

    #[test]
    fn synthetic_endings_read() {
        let endings = read_image(&synthetic(), DS, STRINGS).unwrap();
        assert_eq!(endings.win.file_name(), Some("win.vcl"));
        let lose: Vec<_> = endings
            .lose
            .iter()
            .map(|e| e.file_name().unwrap())
            .collect();
        assert_eq!(lose, ["slose.vcl", "nlose.vcl", "slose.vcl"]);
        assert_eq!(endings.triumph.file_name(), Some("triumph.vcl"));
        assert_eq!(endings.lose_caption(1), b"GONE");
        assert_eq!(endings.triumph_caption(), b"\x09\x05ALL");
        insta::assert_debug_snapshot!(endings);
    }

    #[test]
    fn the_win_caption_shows_two_digits_of_each_count_and_five_of_the_points() {
        let endings = read_image(&synthetic(), DS, STRINGS).unwrap();
        assert_eq!(
            endings.win_caption(Numbers::default()),
            b"K=00\x0cL=00\x0cP=00000"
        );
        let numbers = Numbers {
            kills: 103,
            losses: 2,
            points: endings.points(103, 2),
        };
        assert_eq!(numbers.points, 715);
        assert_eq!(endings.win_caption(numbers), b"K=03\x0cL=02\x0cP=00715");
        assert_eq!(digits(65535), *b"65535\0");
        // Losses outweighing kills give 0; the products wrap at 16 bits.
        assert_eq!(endings.points(1, 3), 0);
        assert_eq!(endings.points(255, 0), 1785);
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let mut image = synthetic();
            image[at] = value;
            read_image(&image, DS, STRINGS)
        };
        let ops = [
            WIN_ENTRY,
            TRIUMPH_ENTRY,
            LOSE_TABLE,
            PER_KILL,
            POINTS,
            WIN_POINTS,
        ];
        for (at, _) in ops.into_iter().chain(BUFFERS) {
            assert!(corrupt(at, 0x90).is_err(), "{at:#x}");
        }
        for at in STRINGS_LOADS.into_iter().chain(COPIES.map(|c| c.2)) {
            assert!(corrupt(at, 0x90).is_err(), "{at:#x}");
        }
        // Points that are not the scored ones, a field past the caption's NUL or
        // outside the buffer, an entry that is not one, a caption with no end.
        assert!(corrupt(WIN_POINTS.0 + 1, 0x06).is_err());
        assert!(corrupt(COPIES[4].2 + 2, 0x22).is_err());
        assert!(corrupt(COPIES[4].1 + 1, 0x05).is_err());
        assert!(corrupt(DS + 0x13a, 0x17).is_err());
        let mut image = synthetic();
        image[STRINGS + 0x40..].fill(b'X');
        assert!(read_image(&image, DS, STRINGS).is_err());
    }

    /// Reads the endings from the user's `xeno.exe`, checking only their shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_endings_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let endings = read(&exe).unwrap();
        // `docs/formats/vcl.md`, "Sequences".
        let name = |e: &Entry| e.file_name().unwrap();
        assert_eq!(name(&endings.win), "win.vcl");
        assert_eq!(
            endings.lose.map(|e| name(&e)),
            ["slose.vcl", "nlose.vcl", "slose.vcl"]
        );
        assert_eq!(name(&endings.triumph), "triumph.vcl");
        assert_eq!(endings.lose_selector, 0x6012);
        let values = endings.fields.iter().map(|f| f.value).collect::<Vec<_>>();
        assert_eq!(values[..2], [Value::Kills, Value::Losses]);
        assert_eq!((endings.per_kill, endings.per_loss), (50, 25));
        // The fields are filled in, and the points end the caption.
        let numbers = Numbers {
            kills: 12,
            losses: 34,
            points: 56789,
        };
        let caption = endings.win_caption(numbers);
        assert!(caption.ends_with(b"56789"));
        assert!(
            caption
                .windows(2)
                .filter(|w| w == b"12" || w == b"34")
                .count()
                >= 2
        );
        for i in 0..LOSE_ENTRIES {
            assert!(!endings.lose_caption(i).is_empty());
        }
        assert!(!endings.triumph_caption().is_empty());
    }
}
