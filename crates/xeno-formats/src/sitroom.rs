//! What the port reads from the user's `xeno.exe` for the situation room's main view
//! (`docs/screens.md`, "The situation room's main view"): the map's robot colours and
//! sprites and its zoom labels, the message log's heading, rule and time, the robot
//! view's at-base text, the panels' bar widths, clock and detail texts (T-127), and the
//! string segment the log's messages point into. Game data, read at run time through
//! the instructions that use it.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, string, word};
use crate::sequence;

/// `mov ah, [bx + table]` in the robots' map routine: a dot's colour by model / 2.
pub(crate) const DOTS_AT: usize = 0xf9a3;
/// `mov bx, table` there: the sprites' first handles by model, at a zoom of 8 or more
/// and below it.
pub(crate) const LARGE_AT: usize = 0xf9b5;
pub(crate) const SMALL_AT: usize = 0xf9bf;
/// The map's zoom label (`1fe6:6cb9`): `mov si, prefix`, then `mov si, [bx + names]`.
pub(crate) const ZOOM_PREFIX_AT: usize = 0x16b40;
pub(crate) const ZOOM_NAMES_AT: usize = 0x16b53;
/// Mode C (`1000:56a7`): `mov si, heading`, `mov bx, rule`.
pub(crate) const LOG_HEADING_AT: usize = 0x56e2;
pub(crate) const LOG_RULE_AT: usize = 0x56ec;
/// A message's time (`1fe6:6d5b`): `mov [hours], ax`, `mov [minutes], ax`, `mov si,
/// template`.
pub(crate) const HOURS_AT: usize = 0x16bc0;
pub(crate) const MINUTES_AT: usize = 0x16bc9;
pub(crate) const TIME_AT: usize = 0x16bcc;
/// Mode R (`1000:5630`): `mov si, text` in the string segment, for a unit at its base.
pub(crate) const AT_BASE_AT: usize = 0x5699;

/// The roster's rows (`1fe6:6d87`): `mov ax, [bx + table]` for the status icons and
/// the kind icons at the base and in the field, `mov bx, rect` for the icons' places,
/// `mov si, text` for an empty row.
pub(crate) const STATUS_ICONS_AT: usize = 0x16c2e;
pub(crate) const BASE_ICONS_AT: usize = 0x16c45;
pub(crate) const FIELD_ICONS_AT: usize = 0x16c4b;
pub(crate) const STATUS_RECT_AT: usize = 0x16c32;
pub(crate) const KIND_RECT_AT: usize = 0x16c4f;
pub(crate) const EMPTY_ROW_AT: usize = 0x16c6c;
/// `mov si, [bx + table]` in `1fe6:7fd5`: the call signs, by +0xdb.
pub(crate) const CALL_SIGNS_AT: usize = 0x17e3d;
/// `mov ax, [bx + table]` in the mode buttons' draw (`1000:5efd`): labels by mode.
pub(crate) const MODE_LABELS_AT: usize = 0x5f05;

/// The power rows and the reserve bar (`1000:6047` to `61a4`): `mov al, [bx + table]`
/// at `1000:60c2`, a bar's width by its value over 8.
pub(crate) const BAR_WIDTHS_AT: usize = 0x60c2;
/// The clock (`1000:61a5`): `mov si, text` at `1000:61c6`; `1000:edb6` writes into it
/// the month's name (`mov ax, [bx + names]` at `edc0`, `mov [text], ax` at `edc4`),
/// then the day, hours and minutes (`mov [field], ax` at `eddf`, `ede8`, `edf1`).
pub(crate) const CLOCK_AT: usize = 0x61c6;
pub(crate) const MONTHS_AT: usize = 0xedc0;
pub(crate) const MONTH_AT: usize = 0xedc4;
pub(crate) const CLOCK_FIELDS_AT: [usize; 3] = [0xeddf, 0xede8, 0xedf1];

/// The detail panel (`1000:5a0e`): `mov si, text` for its heading, the texts without
/// a unit (string segment), at the base and in the field, in repair, waiting for
/// repair, ready and with the pilot driving; `mov bx, rect` for the line it clears;
/// `mov al, [bx + table]` for the speed shown by the speed's size. `mov ax, [bx +
/// table]` in `1fe6:7f38`: a robot's two letters by kind.
pub(crate) const DETAIL_HEADING_AT: usize = 0x5a29;
pub(crate) const NO_UNIT_AT: [usize; 2] = [0x5a3d, 0x5a50];
pub(crate) const PLACES_AT: [usize; 2] = [0x5a82, 0x5a8c];
pub(crate) const STATUS_CLEAR_AT: usize = 0x5a96;
pub(crate) const REPAIR_STATES_AT: [usize; 3] = [0x5abe, 0x5ac5, 0x5acf];
pub(crate) const MANUAL_AT: usize = 0x5ada;
pub(crate) const SPEEDS_AT: usize = 0x5b2a;
pub(crate) const KIND_LETTERS_AT: usize = 0x17dbe;
/// F1's pause (`1000:cff3`, in the situation room and every console): `mov bx, rect`
/// for its box, `mov si, text` (string segment) for what it says.
pub(crate) const PAUSE_BOX_AT: usize = 0xcffd;
pub(crate) const PAUSE_TEXT_AT: usize = 0xd03f;
/// The most speeds read: the table rises by 0 or 1 a step to its end.
const MAX_SPEEDS: usize = 0x100;

/// Mode S (`1fe6:704a`, on a redraw): `mov ax, [di + pictures]`, `mov bx, [di +
/// places]` and `mov si, [di + captions]` by the selected unit's kind times two; `mov
/// si, text` (string segment) without a unit. `mov al, [di + records]` in `1000:686a`:
/// the colour record mode S starts, by kind.
pub(crate) const STATUS_PICTURES_AT: usize = 0x16f34;
pub(crate) const STATUS_PLACES_AT: usize = 0x16f38;
pub(crate) const STATUS_CAPTIONS_AT: usize = 0x16f41;
pub(crate) const NO_UNITS_AT: usize = 0x16f55;
pub(crate) const STATUS_RECORDS_AT: usize = 0x6897;
/// Mode S's `call [bx + table]`: each kind's figure routine, an offset in segment
/// `1fe6`. `mov si, text` twice in `1000:de12`: a flag's texts, clear and set.
pub(crate) const FIGURES_AT: usize = 0x16f0b;
pub(crate) const FLAG_TEXTS_AT: [usize; 2] = [0xde13, 0xde1a];
/// Segment `1fe6`'s start in the load image.
const KERNEL: usize = 0xfe60;
/// The far calls a figure routine's entries end with: a flag, a percentage.
const FLAG_CALL: u16 = 0x5583;
const PERCENT_CALL: u16 = 0x5576;
/// The most entries read from a figure routine before its `ret` (the stock has 11).
const MAX_FIGURES: usize = 32;
/// The robot kinds mode S shows.
pub const KINDS: usize = 3;
/// The most caption ops read for a kind before its end word (the stock has 21).
const MAX_CAPTIONS: usize = 64;

/// The bars' widths, by value / 8.
pub const BAR_STEPS: usize = 32;
/// The months' names, four bytes each.
pub const MONTHS: usize = 12;
const MONTH_LEN: usize = 4;

/// The call signs' count and length.
pub const CALL_SIGNS: usize = 64;
pub const CALL_SIGN_LEN: usize = 6;
/// The main view's modes, 0 to 8 by two.
pub const MODES: usize = 5;

/// The map's zooms, 0 to 6 (the zoom keys' range).
pub const ZOOMS: usize = 7;
/// How much of the string segment is kept for the messages.
const STRINGS_BYTES: usize = 0x10000;

/// The situation room's main view tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitRoom {
    /// A robot's dot colour on the map by model / 2 (humanoid, scorpion, scout).
    pub robot_dots: [u8; 3],
    /// A robot's first sprite handle on the map by model / 2, below a zoom of 8 and
    /// from it; the eight headings follow, a handle step apart.
    pub robot_sprites: [[u16; 3]; 2],
    /// The zoom label: a prefix, then the zoom's name.
    pub zoom_prefix: Vec<u8>,
    pub zoom_names: Vec<Vec<u8>>,
    /// Mode C's heading and the rule under it (top, left, bottom, right).
    pub log_heading: Vec<u8>,
    pub log_rule: [u16; 4],
    /// A message's time: the template and where its hours and minutes go.
    pub time: Vec<u8>,
    pub time_digits: [usize; 2],
    /// Mode R's text for a unit at its base.
    pub at_base: Vec<u8>,
    /// The roster's status icons (by +0xf9 bits 4 to 6) and kind icons at the base
    /// and in the field; the icons' left edges (status, kind) and an empty row's text.
    pub status_icons: [u16; 8],
    pub kind_icons: [[u16; 3]; 2],
    pub icon_lefts: [u16; 2],
    pub empty_row: Vec<u8>,
    /// The call signs: string segment offsets, by +0xdb.
    pub call_signs: Vec<u16>,
    /// The mode buttons' labels, by mode / 2 (the next sprite is the current mode's).
    pub mode_labels: [u16; MODES],
    /// A power row's or the reserve's bar width by its value / 8 (`cbw`: signed).
    pub bar_widths: [i8; BAR_STEPS],
    /// The clock's template, the months' names (four bytes each) and where the month,
    /// day, hours and minutes go in it.
    pub clock: Vec<u8>,
    pub months: Vec<u8>,
    pub clock_fields: [usize; 4],
    pub detail: Detail,
    pub status: Status,
    /// F1's pause box and its text.
    pub pause_box: Rect,
    pub pause_text: Vec<u8>,
    /// The string segment, from its start.
    pub strings: Vec<u8>,
}

/// The detail panel's texts and tables (`docs/screens.md`, "The situation room's
/// panels (T-127)").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub heading: Vec<u8>,
    /// Without a unit: after the heading, then the lines below.
    pub no_unit: [Vec<u8>; 2],
    /// In the base (+0xfd bit 2), in the field.
    pub places: [Vec<u8>; 2],
    /// The status line, cleared to 0 before its text.
    pub status_clear: Rect,
    /// In the base: in repair, waiting for repair, ready.
    pub repair_states: [Vec<u8>; 3],
    /// In the field with the pilot driving.
    pub manual: Vec<u8>,
    /// A robot's name's letters, by kind.
    pub kind_letters: [[u8; 2]; 3],
    /// The speed shown, by the speed's size.
    pub speeds: Vec<u8>,
}

/// Mode S's pictures and captions (`docs/screens.md`, "The status view (T-126)").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Each kind's picture (a sprite handle) and the rectangle whose top-left corner
    /// it is drawn at, colour 0 transparent.
    pub pictures: [u16; KINDS],
    pub places: [Rect; KINDS],
    /// The colour record each kind's picture starts.
    pub records: [u8; KINDS],
    /// Each kind's labels and lines, in order (`1fe6:6b4b`).
    pub captions: [Vec<Caption>; KINDS],
    /// The text without a unit.
    pub no_units: Vec<u8>,
    /// Each kind's figures, in order.
    pub figures: [Vec<Figure>; KINDS],
    /// A flag's texts: its masked byte 0, and not.
    pub flags: [Vec<u8>; 2],
}

/// One entry of a kind's figure routine: the object's byte at `field` ANDed with
/// `mask` (0xff without an `and`), drawn opaque at (`x`, `y`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Figure {
    pub x: u16,
    pub y: u16,
    pub field: u16,
    pub mask: u8,
    /// `1000:5583`'s flag, or `1000:5576`'s percentage.
    pub percent: bool,
}

/// A figure routine at `at`: entries of `mov ax, y`, `mov bx, x`, `mov cl, [di +
/// field]` (an 8- or 16-bit displacement), an optional `and cl, mask` and a far call to
/// the flag or percentage routine, up to a `ret`.
fn figures(image: &[u8], mut at: usize) -> Result<Vec<Figure>, FormatError> {
    let mut entries = Vec::new();
    let byte = |at: usize| {
        image
            .get(at)
            .copied()
            .ok_or_else(|| invalid(at, "past the end"))
    };
    while byte(at)? != 0xc3 {
        if entries.len() == MAX_FIGURES {
            return Err(invalid(at, "too many figures"));
        }
        let (y, x) = (
            operand(image, at, &[0xb8])?,
            operand(image, at + 3, &[0xbb])?,
        );
        at += 6;
        let field = match (byte(at)?, byte(at + 1)?) {
            (0x8a, 0x4d) => {
                at += 3;
                u16::from(byte(at - 1)?)
            }
            (0x8a, 0x8d) => {
                at += 4;
                word(image, at - 2)?
            }
            _ => return Err(invalid(at, "not a figure's field")),
        };
        let mask = match (byte(at)?, byte(at + 1)?) {
            (0x80, 0xe1) => {
                at += 3;
                byte(at - 1)?
            }
            _ => 0xff,
        };
        let percent = match operand(image, at, &[0x9a])? {
            FLAG_CALL => false,
            PERCENT_CALL => true,
            _ => return Err(invalid(at, "not a figure's call")),
        };
        at += 5;
        entries.push(Figure {
            x,
            y,
            field,
            mask,
            percent,
        });
    }
    Ok(entries)
}

/// One op of a kind's captions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caption {
    /// Op 0: a string-segment text at (`x`, `y`), drawn transparent.
    Text { x: u16, y: u16, text: Vec<u8> },
    /// Op 2: a line in `colour` (slot `0x558`) between the rows and columns `[y0, x0,
    /// y1, x1]`.
    Line { colour: u8, ends: [u16; 4] },
}

/// The captions at `at`: words of op 0 (x, y, a string-segment offset) or op 2 (a
/// colour byte, an offset of four words), up to a word 0xffff.
fn captions(
    image: &[u8],
    ds: usize,
    strings: usize,
    at: usize,
) -> Result<Vec<Caption>, FormatError> {
    let mut ops = Vec::new();
    let mut at = at;
    loop {
        match word(image, at)? {
            0xffff => return Ok(ops),
            _ if ops.len() == MAX_CAPTIONS => return Err(invalid(at, "too many captions")),
            0 => {
                let text = strings + usize::from(word(image, at + 6)?);
                ops.push(Caption::Text {
                    x: word(image, at + 2)?,
                    y: word(image, at + 4)?,
                    text: string(image, text)?,
                });
                at += 8;
            }
            2 => {
                let colour = *image
                    .get(at + 2)
                    .ok_or_else(|| invalid(at, "past the end"))?;
                let line = ds + usize::from(word(image, at + 3)?);
                let end = |i| word(image, line + i);
                ops.push(Caption::Line {
                    colour,
                    ends: [end(0)?, end(2)?, end(4)?, end(6)?],
                });
                at += 5;
            }
            _ => return Err(invalid(at, "not a caption op")),
        }
    }
}

impl SitRoom {
    /// The NUL-terminated string at `offset` in the string segment.
    pub fn string(&self, offset: u16) -> &[u8] {
        let s = self.strings.get(usize::from(offset)..).unwrap_or_default();
        &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
    }

    /// Call sign `index` (+0xdb): six bytes, `1fe6:7fd5` copies no more.
    pub fn call_sign(&self, index: u8) -> &[u8] {
        let at = self
            .call_signs
            .get(usize::from(index))
            .map_or(usize::MAX, |&o| usize::from(o));
        let s = self.strings.get(at..).unwrap_or_default();
        let s = &s[..s.len().min(CALL_SIGN_LEN)];
        &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
    }

    /// A message's time as the log shows it.
    pub fn time_of(&self, hour: u8, minute: u8) -> Vec<u8> {
        let mut time = self.time.clone();
        for (at, n) in self.time_digits.into_iter().zip([hour, minute]) {
            // `1fe6:6d76`: tens counted up from '0', then the units.
            if let Some(d) = time.get_mut(at..at + 2) {
                d.copy_from_slice(&[b'0' + n / 10, b'0' + n % 10]);
            }
        }
        time
    }

    /// The clock as `1000:edb6` writes it: the month's name (none past the table), the
    /// day from 1 with no leading zero (its units first, then a space), and the hours
    /// and minutes as two digits.
    pub fn clock_of(&self, month: u8, day: u8, hour: u8, minute: u8) -> Vec<u8> {
        let mut clock = self.clock.clone();
        let [at_month, at_day, at_hour, at_minute] = self.clock_fields;
        let m = usize::from(month) * MONTH_LEN;
        if let (Some(name), Some(to)) = (
            self.months.get(m..m + MONTH_LEN),
            clock.get_mut(at_month..at_month + MONTH_LEN),
        ) {
            to.copy_from_slice(name);
        }
        // `1000:edf5`: tens counted up from '0', then the units.
        let digits = |n: u8| [b'0'.wrapping_add(n / 10), b'0' + n % 10];
        let day = match digits(day.wrapping_add(1)) {
            [b'0', units] => [units, b' '],
            d => d,
        };
        for (at, d) in [
            (at_day, day),
            (at_hour, digits(hour)),
            (at_minute, digits(minute)),
        ] {
            if let Some(to) = clock.get_mut(at..at + 2) {
                to.copy_from_slice(&d);
            }
        }
        clock
    }
}

/// Reads the tables from a load image whose data and string segments start at `ds`
/// and `strings`.
pub fn read_image(image: &[u8], ds: usize, strings: usize) -> Result<SitRoom, FormatError> {
    let at_ds = |at, op: &[u8]| -> Result<usize, FormatError> {
        Ok(ds + usize::from(operand(image, at, op)?))
    };
    let dots = at_ds(DOTS_AT, &[0x8a, 0xa7])?;
    let sprites = |at| -> Result<[u16; 3], FormatError> {
        let table = at_ds(at, &[0xbb])?;
        Ok([
            word(image, table)?,
            word(image, table + 2)?,
            word(image, table + 4)?,
        ])
    };
    let names = at_ds(ZOOM_NAMES_AT, &[0x8b, 0xb7])?;
    let rule = at_ds(LOG_RULE_AT, &[0xbb])?;
    let time = operand(image, TIME_AT, &[0xbe])?;
    let words = |table: usize, n: usize| -> Result<Vec<u16>, FormatError> {
        (0..n).map(|i| word(image, table + 2 * i)).collect()
    };
    let table = |at| at_ds(at, &[0x8b, 0x87]);
    let left = |at| -> Result<u16, FormatError> { word(image, at_ds(at, &[0xbb])? + 2) };
    let digit = |at| -> Result<usize, FormatError> {
        let d = operand(image, at, &[0xa3])?.wrapping_sub(time);
        (d < 8)
            .then_some(usize::from(d))
            .ok_or_else(|| invalid(at, "not in the time template"))
    };
    let bars = at_ds(BAR_WIDTHS_AT, &[0x8a, 0x87])?;
    let clock = at_ds(CLOCK_AT, &[0xbe])?;
    let months = at_ds(MONTHS_AT, &[0x8b, 0x87])?;
    let field = |at| -> Result<usize, FormatError> {
        let d = at_ds(at, &[0xa3])?.wrapping_sub(clock);
        (d < 16)
            .then_some(d)
            .ok_or_else(|| invalid(at, "not in the clock's template"))
    };
    let bytes = |at: usize, n: usize| {
        image
            .get(at..at + n)
            .ok_or_else(|| invalid(at, "past the end"))
    };
    let text = |at| string(image, at_ds(at, &[0xbe])?);
    let in_strings = |at| string(image, strings + usize::from(operand(image, at, &[0xbe])?));
    let letters = at_ds(KIND_LETTERS_AT, &[0x8b, 0x87])?;
    let letter_pair = |kind: usize| -> Result<[u8; 2], FormatError> {
        let b = bytes(letters + 2 * kind, 2)?;
        Ok([b[0], b[1]])
    };
    let by_kind = |at, op: &[u8]| -> Result<[u16; KINDS], FormatError> {
        let table = at_ds(at, op)?;
        Ok([
            word(image, table)?,
            word(image, table + 2)?,
            word(image, table + 4)?,
        ])
    };
    let places = by_kind(STATUS_PLACES_AT, &[0x8b, 0x9d])?;
    let place = |kind: usize| rect(image, ds + usize::from(places[kind]));
    let scripts = by_kind(STATUS_CAPTIONS_AT, &[0x8b, 0xb5])?;
    let script = |kind: usize| captions(image, ds, strings, ds + usize::from(scripts[kind]));
    let records = bytes(at_ds(STATUS_RECORDS_AT, &[0x8a, 0x85])?, KINDS)?;
    let routines = by_kind(FIGURES_AT, &[0xff, 0x97])?;
    let routine = |kind: usize| figures(image, KERNEL + usize::from(routines[kind]));
    let speeds = at_ds(SPEEDS_AT, &[0x8a, 0x87])?;
    let speeds = image
        .get(speeds..)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(speeds, "past the end"))?
        .iter()
        .take(MAX_SPEEDS)
        .scan(None, |last: &mut Option<u8>, &b| {
            let rises = last.is_none_or(|l| b == l || b == l.wrapping_add(1));
            *last = Some(b);
            rises.then_some(b)
        })
        .collect();
    Ok(SitRoom {
        robot_dots: [
            *image
                .get(dots)
                .ok_or_else(|| invalid(dots, "past the end"))?,
            *image
                .get(dots + 1)
                .ok_or_else(|| invalid(dots, "past the end"))?,
            *image
                .get(dots + 2)
                .ok_or_else(|| invalid(dots, "past the end"))?,
        ],
        robot_sprites: [sprites(SMALL_AT)?, sprites(LARGE_AT)?],
        zoom_prefix: string(image, at_ds(ZOOM_PREFIX_AT, &[0xbe])?)?,
        zoom_names: (0..ZOOMS)
            .map(|z| string(image, ds + usize::from(word(image, names + 2 * z)?)))
            .collect::<Result<_, _>>()?,
        log_heading: string(image, at_ds(LOG_HEADING_AT, &[0xbe])?)?,
        log_rule: [
            word(image, rule)?,
            word(image, rule + 2)?,
            word(image, rule + 4)?,
            word(image, rule + 6)?,
        ],
        time: string(image, ds + usize::from(time))?,
        time_digits: [digit(HOURS_AT)?, digit(MINUTES_AT)?],
        at_base: string(
            image,
            strings + usize::from(operand(image, AT_BASE_AT, &[0xbe])?),
        )?,
        status_icons: words(table(STATUS_ICONS_AT)?, 8)?
            .try_into()
            .unwrap_or_default(),
        kind_icons: [
            words(table(FIELD_ICONS_AT)?, 3)?
                .try_into()
                .unwrap_or_default(),
            words(table(BASE_ICONS_AT)?, 3)?
                .try_into()
                .unwrap_or_default(),
        ],
        icon_lefts: [left(STATUS_RECT_AT)?, left(KIND_RECT_AT)?],
        empty_row: string(image, at_ds(EMPTY_ROW_AT, &[0xbe])?)?,
        call_signs: words(at_ds(CALL_SIGNS_AT, &[0x8b, 0xb7])?, CALL_SIGNS)?,
        mode_labels: words(table(MODE_LABELS_AT)?, MODES)?
            .try_into()
            .unwrap_or_default(),
        bar_widths: {
            let widths = bytes(bars, BAR_STEPS)?;
            std::array::from_fn(|i| widths[i] as i8)
        },
        clock: string(image, clock)?,
        months: bytes(months, MONTHS * MONTH_LEN)?.to_vec(),
        clock_fields: [
            field(MONTH_AT)?,
            field(CLOCK_FIELDS_AT[0])?,
            field(CLOCK_FIELDS_AT[1])?,
            field(CLOCK_FIELDS_AT[2])?,
        ],
        detail: Detail {
            heading: text(DETAIL_HEADING_AT)?,
            no_unit: [in_strings(NO_UNIT_AT[0])?, in_strings(NO_UNIT_AT[1])?],
            places: [text(PLACES_AT[0])?, text(PLACES_AT[1])?],
            status_clear: rect(image, at_ds(STATUS_CLEAR_AT, &[0xbb])?)?,
            repair_states: [
                text(REPAIR_STATES_AT[0])?,
                text(REPAIR_STATES_AT[1])?,
                text(REPAIR_STATES_AT[2])?,
            ],
            manual: text(MANUAL_AT)?,
            kind_letters: [letter_pair(0)?, letter_pair(1)?, letter_pair(2)?],
            speeds,
        },
        status: Status {
            pictures: by_kind(STATUS_PICTURES_AT, &[0x8b, 0x85])?,
            places: [place(0)?, place(1)?, place(2)?],
            records: [records[0], records[1], records[2]],
            captions: [script(0)?, script(1)?, script(2)?],
            no_units: in_strings(NO_UNITS_AT)?,
            figures: [routine(0)?, routine(1)?, routine(2)?],
            flags: [text(FLAG_TEXTS_AT[0])?, text(FLAG_TEXTS_AT[1])?],
        },
        pause_box: rect(image, at_ds(PAUSE_BOX_AT, &[0xbb])?)?,
        pause_text: in_strings(PAUSE_TEXT_AT)?,
        strings: image
            .get(strings..(strings + STRINGS_BYTES).min(image.len()))
            .ok_or_else(|| invalid(strings, "no string segment"))?
            .to_vec(),
    })
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<SitRoom, FormatError> {
    read_image(
        &exe.image,
        sequence::data_segment(exe)?,
        sequence::string_segment(exe)?,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const DS: usize = 0x20000;
    pub(crate) const STRINGS: usize = 0x30000;

    /// Made-up tables: dots 1, 2, 3; sprites 0x10.. small and 0x40.. large; zoom
    /// labels "Z" and "z0".."z6"; heading "LOG", rule (5, 6, 5, 60); the time template
    /// "[00-00]" with hours at 1 and minutes at 4; "HOME" at base.
    pub(crate) fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; STRINGS + 0x100];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        put(DOTS_AT, &op(&[0x8a, 0xa7], 0x100));
        put(DS + 0x100, &[1, 2, 3]);
        put(SMALL_AT, &op(&[0xbb], 0x110));
        put(LARGE_AT, &op(&[0xbb], 0x118));
        for (i, h) in [0x10u16, 0x18, 0x20, 0x40, 0x48, 0x50]
            .into_iter()
            .enumerate()
        {
            put(DS + 0x110 + 2 * i + 2 * (i / 3), &h.to_le_bytes());
        }
        put(ZOOM_PREFIX_AT, &op(&[0xbe], 0x200));
        put(DS + 0x200, b"Z\0");
        put(ZOOM_NAMES_AT, &op(&[0x8b, 0xb7], 0x210));
        for z in 0..ZOOMS {
            let at = 0x220 + 4 * z as u16;
            put(DS + 0x210 + 2 * z, &at.to_le_bytes());
            put(DS + usize::from(at), &[b'z', b'0' + z as u8, 0]);
        }
        put(LOG_HEADING_AT, &op(&[0xbe], 0x300));
        put(DS + 0x300, b"LOG\0");
        put(LOG_RULE_AT, &op(&[0xbb], 0x310));
        put(DS + 0x310, &[5, 0, 6, 0, 5, 0, 60, 0]);
        put(HOURS_AT, &op(&[0xa3], 0x321));
        put(MINUTES_AT, &op(&[0xa3], 0x324));
        put(TIME_AT, &op(&[0xbe], 0x320));
        put(DS + 0x320, b"[00-00]\0");
        put(STATUS_ICONS_AT, &op(&[0x8b, 0x87], 0x400));
        put(FIELD_ICONS_AT, &op(&[0x8b, 0x87], 0x410));
        put(BASE_ICONS_AT, &op(&[0x8b, 0x87], 0x416));
        put(MODE_LABELS_AT, &op(&[0x8b, 0x87], 0x420));
        for i in 0..21u16 {
            put(
                DS + 0x400 + 2 * usize::from(i),
                &(0x100 + 8 * i).to_le_bytes(),
            );
        }
        put(STATUS_RECT_AT, &op(&[0xbb], 0x440));
        put(KIND_RECT_AT, &op(&[0xbb], 0x448));
        put(DS + 0x442, &[5, 0]);
        put(DS + 0x44a, &[14, 0]);
        put(EMPTY_ROW_AT, &op(&[0xbe], 0x450));
        put(DS + 0x450, b"  \0");
        put(CALL_SIGNS_AT, &op(&[0x8b, 0xb7], 0x500));
        put(DS + 0x502, &0x50u16.to_le_bytes());
        put(STRINGS + 0x50, b"AB-123XYZ\0");
        put(AT_BASE_AT, &op(&[0xbe], 0x40));
        put(STRINGS + 0x40, b"HOME\0HELLO\0");
        // Bars 0, 1, .. 31 wide (the last -1); the clock "mmmm/dd/hh/nn" with
        // months "M00 ".."M11 ".
        put(BAR_WIDTHS_AT, &op(&[0x8a, 0x87], 0x600));
        for i in 0..BAR_STEPS {
            put(DS + 0x600 + i, &[if i == 31 { 0xff } else { i as u8 }]);
        }
        put(CLOCK_AT, &op(&[0xbe], 0x640));
        put(DS + 0x640, b"mmmm/dd/hh/nn\0");
        put(MONTHS_AT, &op(&[0x8b, 0x87], 0x660));
        for m in 0..MONTHS {
            put(DS + 0x660 + 4 * m, format!("M{m:02} ").as_bytes());
        }
        let [day, hour, minute] = CLOCK_FIELDS_AT;
        for (at, field) in [
            (MONTH_AT, 0x640),
            (day, 0x645),
            (hour, 0x648),
            (minute, 0x64b),
        ] {
            put(at, &op(&[0xa3], field));
        }
        // The detail panel: "D:" heading, "-0-" and "~" without a unit, "IN"/"OUT",
        // "R1".."R3", "MAN", letters "AA" "BB" "CC", speeds 0, 1, 1, 2 then a jump.
        for (at, value, text) in [
            (DETAIL_HEADING_AT, 0x700, &b"D:\0"[..]),
            (PLACES_AT[0], 0x704, b"IN\0"),
            (PLACES_AT[1], 0x708, b"OUT\0"),
            (REPAIR_STATES_AT[0], 0x70c, b"R1\0"),
            (REPAIR_STATES_AT[1], 0x710, b"R2\0"),
            (REPAIR_STATES_AT[2], 0x714, b"R3\0"),
            (MANUAL_AT, 0x718, b"MAN\0"),
        ] {
            put(at, &op(&[0xbe], value));
            put(DS + usize::from(value), text);
        }
        put(NO_UNIT_AT[0], &op(&[0xbe], 0x60));
        put(NO_UNIT_AT[1], &op(&[0xbe], 0x64));
        put(STRINGS + 0x60, b"-0-\0~\0");
        put(STATUS_CLEAR_AT, &op(&[0xbb], 0x720));
        put(DS + 0x720, &[1, 0, 2, 0, 3, 0, 4, 0]);
        put(KIND_LETTERS_AT, &op(&[0x8b, 0x87], 0x730));
        put(DS + 0x730, b"AABBCC");
        put(SPEEDS_AT, &op(&[0x8a, 0x87], 0x740));
        put(DS + 0x740, &[0, 1, 1, 2, 9]);
        // Mode S: pictures 0x10, 0x18, 0x20 at rectangles from (1, 2), (3, 4), (5, 6);
        // kind 0 captioned "LBL" at (5, 6) and a line in 9, the others not; "nil"
        // without a unit; records 7, 8, 9.
        put(STATUS_PICTURES_AT, &op(&[0x8b, 0x85], 0x800));
        put(DS + 0x800, &[0x10, 0, 0x18, 0, 0x20, 0]);
        put(STATUS_PLACES_AT, &op(&[0x8b, 0x9d], 0x810));
        put(DS + 0x810, &[0x20, 8, 0x28, 8, 0x30, 8]);
        for (i, at) in [0x820, 0x828, 0x830].into_iter().enumerate() {
            let n = 2 * i as u8 + 1;
            put(DS + at, &[n, 0, n + 1, 0, 50, 0, 60, 0]);
        }
        put(STATUS_CAPTIONS_AT, &op(&[0x8b, 0xb5], 0x840));
        put(DS + 0x840, &[0x50, 8, 0x70, 8, 0x80, 8]);
        put(
            DS + 0x850,
            &[0, 0, 5, 0, 6, 0, 0x70, 0, 2, 0, 9, 0x60, 8, 0xff, 0xff],
        );
        put(DS + 0x860, &[1, 0, 2, 0, 1, 0, 9, 0]);
        put(DS + 0x870, &[0xff, 0xff]);
        put(DS + 0x880, &[0xff, 0xff]);
        put(STRINGS + 0x70, b"LBL\0nil\0");
        put(NO_UNITS_AT, &op(&[0xbe], 0x74));
        put(STATUS_RECORDS_AT, &op(&[0x8a, 0x85], 0x890));
        put(DS + 0x890, &[7, 8, 9]);
        // Kind 0's figures: +0x24 as a percentage at (0x20, 0x10), +0xdc bit 1 as a
        // flag at (0x21, 0x11); the others none. Flag texts "ok" and "bad".
        put(FIGURES_AT, &op(&[0xff, 0x97], 0x8a0));
        put(DS + 0x8a0, &[0, 1, 0x20, 1, 0x30, 1]);
        put(
            KERNEL + 0x100,
            &[0xb8, 0x10, 0, 0xbb, 0x20, 0, 0x8a, 0x4d, 0x24],
        );
        put(
            KERNEL + 0x109,
            &[0x9a, 0x76, 0x55, 0, 0, 0xb8, 0x11, 0, 0xbb, 0x21, 0],
        );
        put(
            KERNEL + 0x114,
            &[0x8a, 0x8d, 0xdc, 0, 0x80, 0xe1, 2, 0x9a, 0x83, 0x55],
        );
        put(KERNEL + 0x120, &[0xc3]);
        put(KERNEL + 0x130, &[0xc3]);
        put(KERNEL + 0x11e, &[0, 0]);
        put(FLAG_TEXTS_AT[0], &op(&[0xbe], 0x8b0));
        put(FLAG_TEXTS_AT[1], &op(&[0xbe], 0x8b4));
        put(DS + 0x8b0, b"ok\0\0bad\0");
        // F1's box (10, 20, 22, 60) saying "wait".
        put(PAUSE_BOX_AT, &op(&[0xbb], 0x8c0));
        put(DS + 0x8c0, &[10, 0, 20, 0, 22, 0, 60, 0]);
        put(PAUSE_TEXT_AT, &op(&[0xbe], 0x90));
        put(STRINGS + 0x90, b"wait\0");
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let room = read_image(&synthetic(), DS, STRINGS).unwrap();
        assert_eq!(room.robot_sprites, [[0x10, 0x18, 0x20], [0x40, 0x48, 0x50]]);
        assert_eq!(room.zoom_names[6], b"z6");
        assert_eq!(room.time_of(9, 45), b"[09-45]");
        assert_eq!(room.string(0x45), b"HELLO");
        assert_eq!(room.string(0xffff), b"");
        // Call sign 1 is six bytes; 0 points at the start of the segment (empty).
        assert_eq!(room.call_sign(1), b"AB-123");
        assert_eq!(room.call_sign(0), b"");
        assert_eq!(room.call_sign(200), b"");
        assert_eq!(room.status_icons[7], 0x138);
        assert_eq!(
            room.kind_icons,
            [[0x140, 0x148, 0x150], [0x158, 0x160, 0x168]]
        );
        assert_eq!(room.mode_labels, [0x180, 0x188, 0x190, 0x198, 0x1a0]);
        assert_eq!(room.icon_lefts, [5, 14]);
        assert_eq!((room.bar_widths[7], room.bar_widths[31]), (7, -1));
        assert_eq!(room.clock_fields, [0, 5, 8, 11]);
        // Day 4 is the 5th: no leading zero; past the months the template's name stays.
        assert_eq!(room.clock_of(11, 4, 7, 9), b"M11 /5 /07/09");
        assert_eq!(room.clock_of(12, 10, 23, 59), b"mmmm/11/23/59");
        let d = &room.detail;
        assert_eq!(
            (d.no_unit[1].as_slice(), d.status_clear.right),
            (&b"~"[..], 4)
        );
        assert_eq!(
            (d.kind_letters[2], d.speeds.as_slice()),
            (*b"CC", &[0, 1, 1, 2][..])
        );
        let status = &room.status;
        assert_eq!(
            (status.pictures, status.places[2].left, status.records),
            ([0x10, 0x18, 0x20], 6, [7, 8, 9])
        );
        assert_eq!(
            status.captions[0],
            [
                Caption::Text {
                    x: 5,
                    y: 6,
                    text: b"LBL".to_vec()
                },
                Caption::Line {
                    colour: 9,
                    ends: [1, 2, 1, 9]
                }
            ]
        );
        assert!(status.captions[1].is_empty());
        assert_eq!(status.no_units, b"nil");
        let figure = |x, y, field, mask, percent| Figure {
            x,
            y,
            field,
            mask,
            percent,
        };
        assert_eq!(
            status.figures[0],
            [
                figure(0x20, 0x10, 0x24, 0xff, true),
                figure(0x21, 0x11, 0xdc, 2, false)
            ]
        );
        assert!(status.figures[2].is_empty());
        assert_eq!(status.flags, [b"ok".to_vec(), b"bad".to_vec()]);
        insta::assert_debug_snapshot!(SitRoom {
            strings: Vec::new(),
            ..room
        });
    }

    #[test]
    fn another_build_is_refused() {
        for at in [
            DOTS_AT,
            SMALL_AT,
            LARGE_AT,
            ZOOM_PREFIX_AT,
            ZOOM_NAMES_AT + 1,
            LOG_HEADING_AT,
            LOG_RULE_AT,
            HOURS_AT,
            MINUTES_AT + 2,
            TIME_AT,
            AT_BASE_AT,
            STATUS_ICONS_AT,
            BASE_ICONS_AT + 1,
            FIELD_ICONS_AT,
            STATUS_RECT_AT,
            KIND_RECT_AT,
            EMPTY_ROW_AT,
            CALL_SIGNS_AT + 1,
            MODE_LABELS_AT,
            BAR_WIDTHS_AT + 1,
            CLOCK_AT,
            MONTHS_AT,
            MONTH_AT,
            CLOCK_FIELDS_AT[2],
            DETAIL_HEADING_AT,
            NO_UNIT_AT[1],
            PLACES_AT[0],
            STATUS_CLEAR_AT,
            REPAIR_STATES_AT[2],
            MANUAL_AT,
            SPEEDS_AT + 1,
            KIND_LETTERS_AT,
            STATUS_PICTURES_AT,
            STATUS_PLACES_AT + 1,
            STATUS_CAPTIONS_AT,
            NO_UNITS_AT,
            STATUS_RECORDS_AT,
            FIGURES_AT + 1,
            FLAG_TEXTS_AT[1],
            PAUSE_BOX_AT,
            PAUSE_TEXT_AT,
        ] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS, STRINGS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/screens.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_sitroom_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let room = read(&exe).unwrap();
        assert_eq!(room.robot_dots, [0x30, 0x0e, 0x0d]);
        assert_eq!(room.robot_sprites, [[0xc0, 0x100, 0x140], [0, 0x40, 0x80]]);
        assert_eq!(room.zoom_names.len(), ZOOMS);
        assert!(
            room.zoom_names
                .iter()
                .all(|n| n.ends_with(b"M") || n.ends_with(b"K"))
        );
        assert_eq!(room.log_rule, [36, 95, 36, 288]);
        assert_eq!(room.time_digits, [2, 5]);
        assert_eq!(room.time_of(14, 7)[2..7], *b"14:07");
        assert!(room.at_base.starts_with(b"UNIT IS AT BASE"));
        assert_eq!(room.status_icons[0], 0x430);
        // F1's box: 12 rows of 43 pixels, with room for its text in font 0.
        let b = room.pause_box;
        assert_eq!((b.top, b.left, b.bottom, b.right), (64, 138, 76, 181));
        assert!(!room.pause_text.is_empty());
        assert_eq!(
            room.kind_icons,
            [[0x480, 0x488, 0x490], [0x498, 0x4a0, 0x4a8]]
        );
        assert_eq!(room.icon_lefts, [5, 14]);
        assert_eq!(room.mode_labels, [0x3a0, 0x3b0, 0x3c0, 0x3d0, 0x3e0]);
        assert_eq!(room.empty_row, b"        ");
        // The bars fill 23 pixels at the most, and grow with the value.
        assert!(room.bar_widths.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(room.bar_widths[BAR_STEPS - 1], 23);
        assert_eq!(room.clock_fields, [0, 4, 7, 10]);
        assert_eq!(room.clock_of(0, 8, 6, 5)[4..], *b"9  06:05");
        // The detail panel's status line is one row of font 4 under the place's.
        let d = &room.detail;
        assert_eq!(d.status_clear.bottom - d.status_clear.top, 5);
        assert!(
            d.places
                .iter()
                .chain(&d.repair_states)
                .all(|t| !t.is_empty())
        );
        // Every robot's letters are two capitals; the speeds rise by 0 or 1.
        assert!(
            d.kind_letters
                .as_flattened()
                .iter()
                .all(u8::is_ascii_uppercase)
        );
        assert!(d.speeds.len() > 100 && d.speeds[0] == 0);
        // Mode S: each kind's picture from msprite1's, the records after the views',
        // captions of texts and lines on the screen.
        let status = &room.status;
        assert_eq!(status.pictures, [0x418, 0x420, 0x428]);
        assert_eq!(status.records, [0x12, 0x13, 0x14]);
        assert!(!status.no_units.is_empty());
        for kind in &status.captions {
            assert!(kind.iter().any(|c| matches!(c, Caption::Line { .. })));
            for c in kind {
                match c {
                    Caption::Text { text, .. } => assert!(!text.is_empty()),
                    Caption::Line {
                        ends: [y0, x0, y1, x1],
                        ..
                    } => {
                        assert!(*y0.max(y1) < 200 && *x0.max(x1) < 320, "{c:?}")
                    }
                }
            }
        }
        // The figures: the helm and power as percentages, the parts damage wears as
        // flags; a flag set shows in the second colour pair.
        for kind in &status.figures {
            let percent: Vec<_> = kind.iter().filter(|f| f.percent).map(|f| f.field).collect();
            assert_eq!(percent, [0x24, 0x4e]);
            assert!(kind.iter().all(|f| f.percent
                || [0xdc, 0xec, 0xf6, 0xfd].contains(&f.field) && f.mask.count_ones() == 1));
        }
        assert_eq!(status.figures.each_ref().map(Vec::len), [9, 11, 9]);
        assert!(status.flags[0].iter().all(|&b| b < 0x80));
        assert!(status.flags[1].iter().any(|&b| b >= 0x80));
        // Every call sign is six printable bytes (names like `EAGLE `).
        for i in 0..CALL_SIGNS as u8 {
            let sign = room.call_sign(i);
            assert!(
                sign.len() == CALL_SIGN_LEN
                    && sign.iter().all(|&b| b == b' ' || b.is_ascii_graphic()),
                "{i}: {sign:?}"
            );
        }
    }
}
