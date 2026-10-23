//! The map room's tables in `xeno.exe` (`docs/screens.md`, "The map room"): its widget
//! set (the map area and the five buttons), the buttons' sprites and handlers, the key
//! table, the scenario boxes' looks by `state`, the panels and the alert-level words.
//! They are game data, so they are read from the user's own `xeno.exe` at run time and
//! never stored in this repository.
//!
//! Each table is found from the instruction that uses it, after checking that
//! instruction's bytes, so another build of the game is refused rather than misread.
//! Segments are taken only from words the executable relocates.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::sequence;

/// The states a scenario record can have: 0, 2, ..., 12, one entry each in the look
/// and alert-level tables.
pub const STATES: usize = 7;
/// Bytes per sprite directory entry: a handle plus this is the next sprite, which is a
/// button's pressed look (`xor [0x5f0d], 8` at `1000:7230`).
pub const HANDLE_STEP: u16 = 8;

/// The widget types (offsets into the host's handler tables) of the map area and of a
/// button (`docs/screens.md`, "Widgets").
const MAP_AREA_TYPE: u16 = 0x38;
const BUTTON_TYPE: u16 = 0x3c;
/// A widget: type, state offset, then its rectangle.
const WIDGET_SIZE: usize = 12;
/// The image offset of Ghidra's segment `1fe6`, the kernel.
const KERNEL: usize = 0xfe60;

/// Load-image offsets of the instructions read, and the bytes before each operand.
const WIDGETS_SEGMENT: (usize, &[u8]) = (0x6f4c, &[0xb8]);
const WIDGETS_FIRST: (usize, &[u8]) = (0x6f58, &[0xbb]);
const WIDGETS_END: (usize, &[u8]) = (0x6f5e, &[0xba]);
const BANK_BASE: (usize, &[u8]) = (0x71f5, &[0xba]);
const BUTTON_SPRITES: (usize, &[u8]) = (0x7204, &[0x8b, 0x87]);
const BUTTON_HANDLERS: (usize, &[u8]) = (0x7242, &[0xff, 0x97]);
const UNAVAILABLE: (usize, &[u8]) = (0x72ac, &[0xb8]);
const LOOKS: (usize, &[u8]) = (0x72b5, &[0x8b, 0x87]);
const HIGHLIGHT: (usize, &[u8]) = (0x72f2, &[0xb8]);
const KEYS: (usize, &[u8]) = (0x7384, &[0xbb]);
const KEY_COUNT: (usize, &[u8]) = (0x7387, &[0xb9]);
const STRINGS_SEGMENT: (usize, &[u8]) = (KERNEL + 0x4bed, &[0xb8]);
const NAME_PANEL: usize = KERNEL + 0x7416;
const SCORE_PANEL: usize = KERNEL + 0x7434;
const INFO_PANEL: usize = KERNEL + 0x749c;
const ALERT_LABEL: (usize, &[u8]) = (KERNEL + 0x74c7, &[0xbe]);
const ALERT_LEVELS: (usize, &[u8]) = (KERNEL + 0x74db, &[0x8b, 0xb7]);

/// No string the map room draws is longer than this.
const MAX_STRING: usize = 64;
/// No widget set or key table is longer than this.
const MAX_ENTRIES: usize = 64;

/// A rectangle as the code stores it. The widgets' bottom and right edges are outside
/// (the dispatcher's hit test), and the sprite blitters draw from the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub top: u16,
    pub left: u16,
    pub bottom: u16,
    pub right: u16,
}

impl Rect {
    /// Whether (`x`, `y`) is inside, the bottom and right edges excluded.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
    }
}

/// One of the buttons down the right edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// The id in the widget's state, a byte offset into the sprite and handler tables.
    pub id: u16,
    pub area: Rect,
    /// The sprite handle of its normal look; the next one is the pressed look.
    pub sprite: u16,
    /// The code offset its click calls; the key table names the same handlers.
    pub handler: u16,
}

/// A sprite drawn opaque with its top-left at the rectangle's, and the text over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    pub sprite: u16,
    pub area: Rect,
}

/// The map room's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRoom {
    /// The handle where the map room's sprite bank (`msprite5`) is loaded.
    pub bank_base: u16,
    /// Where clicks pick scenarios.
    pub map_area: Rect,
    /// In the widget set's order, which is top to bottom on screen.
    pub buttons: Vec<Button>,
    /// (key word, handler), searched in order (`DS:0x60bf`).
    pub keys: Vec<(u16, u16)>,
    /// A scenario box's sprite by `state / 2` (`DS:0x5f5c`).
    pub looks: [u16; STATES],
    /// The box of a scenario whose unlock level is above the player's progress.
    pub unavailable: u16,
    /// Drawn over the selected scenario's box.
    pub highlight: u16,
    /// The player's name, the score, and the selected scenario's name and alert level.
    pub name_panel: Panel,
    pub score_panel: Panel,
    pub info_panel: Panel,
    /// Drawn after the selected scenario's name, then its alert level's word.
    pub alert_label: Vec<u8>,
    /// By `state / 2`.
    pub alert_levels: [Vec<u8>; STATES],
}

impl MapRoom {
    /// The index in the map room's sprite bank of `handle`.
    pub fn sprite_index(&self, handle: u16) -> Option<usize> {
        let offset = handle.checked_sub(self.bank_base)?;
        offset
            .is_multiple_of(HANDLE_STEP)
            .then_some(usize::from(offset / HANDLE_STEP))
    }

    /// The key words that do what `button` does: those whose handler is its handler.
    pub fn button_keys<'a>(&'a self, button: &'a Button) -> impl Iterator<Item = u16> + 'a {
        self.keys
            .iter()
            .filter(move |&&(_, handler)| handler == button.handler)
            .map(|&(word, _)| word)
    }
}

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// Reads the image at `start` as a little-endian word.
fn word(image: &[u8], at: usize) -> Result<u16, FormatError> {
    image
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

fn rect(image: &[u8], at: usize) -> Result<Rect, FormatError> {
    Ok(Rect {
        top: word(image, at)?,
        left: word(image, at + 2)?,
        bottom: word(image, at + 4)?,
        right: word(image, at + 6)?,
    })
}

/// A NUL-terminated string at `at`, without the NUL.
fn string(image: &[u8], at: usize) -> Result<Vec<u8>, FormatError> {
    let bytes = image
        .get(at..(at + MAX_STRING).min(image.len()))
        .ok_or_else(|| invalid(at, "the string is past the end of the image"))?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid(at, "no string ends here"))?;
    Ok(bytes[..end].to_vec())
}

/// The operand after `op` at `at`.
fn read_op(image: &[u8], (at, op): (usize, &[u8])) -> Result<u16, FormatError> {
    operand(image, at, op)
}

/// A segment operand, which must be one the executable relocates: its image offset.
fn segment(
    image: &[u8],
    (at, op): (usize, &[u8]),
    relocated: &[usize],
) -> Result<usize, FormatError> {
    let value = operand(image, at, op)?;
    if !relocated.contains(&(at + op.len())) {
        return Err(invalid(at, "the segment here is not relocated"));
    }
    Ok(usize::from(value) * 16)
}

/// A panel from `mov ax, sprite; mov bx, rect` at `at`.
fn panel(image: &[u8], ds: usize, at: usize) -> Result<Panel, FormatError> {
    let sprite = operand(image, at, &[0xb8])?;
    let area = rect(image, ds + usize::from(operand(image, at + 3, &[0xbb])?))?;
    Ok(Panel { sprite, area })
}

/// Reads the map room's tables from a load image whose data segment starts at `ds`;
/// `relocated` lists the image offsets of the words the executable relocates. Error
/// offsets are into the load image.
pub fn read_image(image: &[u8], ds: usize, relocated: &[usize]) -> Result<MapRoom, FormatError> {
    let table = |entry: (usize, &[u8])| -> Result<usize, FormatError> {
        Ok(ds + usize::from(read_op(image, entry)?))
    };

    // The widget set: a header (its size, its own segment, the segment and size of
    // the state template), then 12-byte widgets from `first` to `end`.
    let widgets = segment(image, WIDGETS_SEGMENT, relocated)?;
    let first = usize::from(read_op(image, WIDGETS_FIRST)?);
    let end = usize::from(read_op(image, WIDGETS_END)?);
    if first > end || (end - first) % WIDGET_SIZE != 0 || end - first > MAX_ENTRIES * WIDGET_SIZE {
        return Err(invalid(WIDGETS_END.0, "not a list of widgets"));
    }
    if !relocated.contains(&(widgets + 4)) {
        return Err(invalid(
            widgets + 4,
            "the state template's segment is not relocated",
        ));
    }
    let template = usize::from(word(image, widgets + 4)?) * 16;
    let template_size = usize::from(word(image, widgets + 8)?);

    let sprites = table(BUTTON_SPRITES)?;
    let handlers = table(BUTTON_HANDLERS)?;
    let mut map_area = None;
    let mut buttons = Vec::new();
    for at in (widgets + first..widgets + end).step_by(WIDGET_SIZE) {
        let (kind, state) = (word(image, at)?, usize::from(word(image, at + 2)?));
        let area = rect(image, at + 4)?;
        match kind {
            MAP_AREA_TYPE if map_area.is_none() => map_area = Some(area),
            MAP_AREA_TYPE => return Err(invalid(at, "two map areas")),
            BUTTON_TYPE => {
                // The button's state: a redraw byte, then its id.
                if state + 3 > template_size {
                    return Err(invalid(at, "the button's state is past the template"));
                }
                let id = word(image, template + state + 1)?;
                if id % 2 != 0 || usize::from(id) >= 2 * MAX_ENTRIES {
                    return Err(invalid(at, "not a button id"));
                }
                buttons.push(Button {
                    id,
                    area,
                    sprite: word(image, sprites + usize::from(id))?,
                    handler: word(image, handlers + usize::from(id))?,
                });
            }
            _ => {}
        }
    }
    let map_area = map_area.ok_or_else(|| invalid(widgets, "no map area"))?;
    if buttons.is_empty() {
        return Err(invalid(widgets, "no buttons"));
    }

    let keys_at = table(KEYS)?;
    let count = usize::from(read_op(image, KEY_COUNT)?);
    if count > MAX_ENTRIES {
        return Err(invalid(KEY_COUNT.0, "too many keys"));
    }
    let keys = (0..count)
        .map(|i| {
            Ok((
                word(image, keys_at + 4 * i)?,
                word(image, keys_at + 4 * i + 2)?,
            ))
        })
        .collect::<Result<_, FormatError>>()?;

    let looks_at = table(LOOKS)?;
    let levels_at = table(ALERT_LEVELS)?;
    let strings = segment(image, STRINGS_SEGMENT, relocated)?;
    let mut looks = [0; STATES];
    let mut alert_levels: [Vec<u8>; STATES] = Default::default();
    for state in 0..STATES {
        looks[state] = word(image, looks_at + 2 * state)?;
        let offset = word(image, levels_at + 2 * state)?;
        alert_levels[state] = string(image, strings + usize::from(offset))?;
    }

    Ok(MapRoom {
        bank_base: read_op(image, BANK_BASE)?,
        map_area,
        buttons,
        keys,
        looks,
        unavailable: read_op(image, UNAVAILABLE)?,
        highlight: read_op(image, HIGHLIGHT)?,
        name_panel: panel(image, ds, NAME_PANEL)?,
        score_panel: panel(image, ds, SCORE_PANEL)?,
        info_panel: panel(image, ds, INFO_PANEL)?,
        alert_label: string(image, table(ALERT_LABEL)?)?,
        alert_levels,
    })
}

/// Reads the map room's tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<MapRoom, FormatError> {
    let relocated: Vec<usize> = exe.relocations.iter().map(|r| r.linear()).collect();
    read_image(&exe.image, sequence::data_segment(exe)?, &relocated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x20000;
    const WIDGETS: usize = 0x1e000;
    const TEMPLATE: usize = 0x1e100;
    const STRINGS: usize = 0x1f000;

    fn put(image: &mut [u8], at: usize, bytes: &[u8]) {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn put_words(image: &mut [u8], at: usize, words: &[u16]) {
        for (i, w) in words.iter().enumerate() {
            put(image, at + 2 * i, &w.to_le_bytes());
        }
    }

    fn op(image: &mut [u8], (at, op): (usize, &[u8]), value: u16) {
        put(image, at, op);
        put(image, at + op.len(), &value.to_le_bytes());
    }

    /// A made-up image: a map area and two buttons (ids 2 and 0, in that order), two
    /// keys, made-up handles, rectangles and words.
    fn synthetic() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; DS + 0x200];
        op(&mut image, WIDGETS_SEGMENT, (WIDGETS / 16) as u16);
        op(&mut image, WIDGETS_FIRST, 0x10);
        op(&mut image, WIDGETS_END, 0x10 + 4 * 12);
        put_words(
            &mut image,
            WIDGETS,
            &[0x40, 0, (TEMPLATE / 16) as u16, 0, 9],
        );
        #[rustfmt::skip]
        put_words(&mut image, WIDGETS + 0x10, &[
            0x36, 0, 0, 0, 200, 320,
            MAP_AREA_TYPE, 1, 10, 5, 190, 280,
            BUTTON_TYPE, 3, 150, 270, 160, 310,
            BUTTON_TYPE, 6, 160, 270, 170, 310,
        ]);
        put(&mut image, TEMPLATE, &[0, 0, 0, 0, 2, 0, 0, 0, 0]);
        op(&mut image, BANK_BASE, 0x100);
        op(&mut image, BUTTON_SPRITES, 0x10);
        op(&mut image, BUTTON_HANDLERS, 0x20);
        put_words(&mut image, DS + 0x10, &[0x100, 0x110]);
        put_words(&mut image, DS + 0x20, &[0x7000, 0x7100]);
        op(&mut image, KEYS, 0x30);
        op(&mut image, KEY_COUNT, 2);
        put_words(&mut image, DS + 0x30, &[0x19, 0x7000, 0x2010, 0x7100]);
        op(&mut image, LOOKS, 0x40);
        put_words(
            &mut image,
            DS + 0x40,
            &[0x120, 0x128, 0x130, 0x130, 0x138, 0x140, 0x148],
        );
        op(&mut image, UNAVAILABLE, 0x138);
        op(&mut image, HIGHLIGHT, 0x150);
        for (at, sprite, rect) in [(NAME_PANEL, 0x158, 0x60), (SCORE_PANEL, 0x160, 0x68)]
            .into_iter()
            .chain([(INFO_PANEL, 0x168, 0x70)])
        {
            op(&mut image, (at, &[0xb8]), sprite);
            op(&mut image, (at + 3, &[0xbb]), rect);
        }
        put_words(
            &mut image,
            DS + 0x60,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        );
        op(&mut image, ALERT_LABEL, 0x80);
        put(&mut image, DS + 0x80, b"LEVEL\0");
        op(&mut image, ALERT_LEVELS, 0x90);
        put_words(&mut image, DS + 0x90, &[0, 2, 4, 6, 8, 8, 8]);
        put(&mut image, STRINGS, b"A\0B\0C\0D\0E\0");
        op(&mut image, STRINGS_SEGMENT, (STRINGS / 16) as u16);
        let relocated = vec![WIDGETS_SEGMENT.0 + 1, WIDGETS + 4, STRINGS_SEGMENT.0 + 1];
        (image, relocated)
    }

    #[test]
    fn a_synthetic_map_room_reads() {
        let (image, relocated) = synthetic();
        let room = read_image(&image, DS, &relocated).unwrap();
        assert_eq!(
            room.map_area,
            Rect {
                top: 10,
                left: 5,
                bottom: 190,
                right: 280
            }
        );
        assert_eq!(
            room.buttons
                .iter()
                .map(|b| (b.id, b.sprite))
                .collect::<Vec<_>>(),
            [(2, 0x110), (0, 0x100)]
        );
        assert_eq!(
            room.button_keys(&room.buttons[0]).collect::<Vec<_>>(),
            [0x2010]
        );
        assert_eq!(room.sprite_index(0x138), Some(7));
        assert_eq!(room.sprite_index(0x13c), None);
        assert_eq!(room.sprite_index(0xf8), None);
        assert_eq!(room.alert_levels[6], b"E");
        assert!(room.map_area.contains(5, 10) && !room.map_area.contains(280, 10));
        insta::assert_debug_snapshot!(room);
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let (mut image, relocated) = synthetic();
            image[at] = value;
            read_image(&image, DS, &relocated)
        };
        // Not the instructions.
        for (at, _) in [WIDGETS_FIRST, BUTTON_SPRITES, LOOKS, KEYS, ALERT_LEVELS] {
            assert!(corrupt(at, 0x90).is_err(), "{at:#x}");
        }
        assert!(corrupt(INFO_PANEL + 3, 0x90).is_err());
        // Widgets that are not a whole list, an odd button id, two map areas, a key
        // table too long, a string with no end.
        assert!(corrupt(WIDGETS_END.0 + 1, 0x15).is_err());
        assert!(corrupt(TEMPLATE + 4, 3).is_err());
        assert!(corrupt(WIDGETS + 0x28, BUTTON_TYPE as u8 - 4).is_err());
        assert!(corrupt(KEY_COUNT.0 + 1, 0x80).is_err());
        let (mut image, relocated) = synthetic();
        image[STRINGS..STRINGS + MAX_STRING].fill(b'X');
        assert!(read_image(&image, DS, &relocated).is_err());
        // A segment that is not relocated.
        let (image, relocated) = synthetic();
        assert!(read_image(&image, DS, &relocated[1..]).is_err());
        assert!(read_image(&image, DS, &relocated[..2]).is_err());
        // Too short.
        assert!(read_image(&image[..0x7000], DS, &relocated).is_err());
    }

    /// Reads the map room from the user's `xeno.exe`, checking only its shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_map_room_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let room = read(&exe).unwrap();
        // `docs/screens.md`: five buttons, each with a key that does the same; 20 keys;
        // every sprite in the 20-sprite bank at handle 0x200.
        assert_eq!(room.buttons.len(), 5);
        assert_eq!(room.keys.len(), 20);
        for button in &room.buttons {
            assert!(room.button_keys(button).next().is_some(), "{button:?}");
            assert!(room.sprite_index(button.sprite + HANDLE_STEP).unwrap() < 20);
        }
        let panels = [room.name_panel, room.score_panel, room.info_panel];
        for handle in room.looks.iter().chain(&[room.unavailable, room.highlight]) {
            assert!(room.sprite_index(*handle).unwrap() < 20);
        }
        for panel in panels {
            assert!(room.sprite_index(panel.sprite).unwrap() < 20);
            assert!(panel.area.right <= 320 && panel.area.bottom <= 200);
        }
        assert!(room.alert_levels.iter().all(|w| !w.is_empty()));
    }
}
