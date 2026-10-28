//! A mission's views and the training menu (`docs/screens.md`, "The mission's views",
//! "The training menu"): the widget sets of the situation room and the three robot
//! consoles, their key tables, the situation room's sprite buttons and roster, the
//! console's program panel, and the training menu's buttons, keys and missions. They
//! are game data, read from the user's own `xeno.exe` at run time and never stored in
//! this repository.
//!
//! As in `crate::maproom`, each table is found from the instruction that uses it after
//! checking that instruction's bytes, and segments are taken only from words the
//! executable relocates. Key tables and sprite buttons are found through the host's
//! handler tables, by the shape of the handler a widget's type points at.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{Rect, invalid, rect, segment, string, word};
use crate::orders::RobotKind;
use crate::sequence;

/// `call [si + table]` in `ui_open_view_of_object` (`1000:7f0b`): the view opener of
/// each object kind, 0 to 3 (humanoid, scorpion, scout, base).
pub(crate) const VIEW_OPENERS: (usize, &[u8]) = (0x7f0b, &[0xff, 0x94]);
/// `call [di + table]` in the widget draw loop (`1000:2714`) and the key dispatch
/// (`1000:24ef`): the host's draw and key handler tables, by widget type.
pub(crate) const DRAW_HANDLERS: (usize, &[u8]) = (0x2714, &[0xff, 0x95]);
pub(crate) const KEY_HANDLERS: (usize, &[u8]) = (0x24ef, &[0xff, 0x95]);
/// Where the situation room loads its sprite bank (`mov dx, handle` at `1000:5525`).
pub(crate) const SITUATION_BANK: (usize, &[u8]) = (0x5525, &[0xba]);
/// The roster's rows: `mov si, [bx + table]` at `1000:5849`, `cmp bx, 2 * count` at
/// `1000:5863`.
pub(crate) const ROSTER_ROWS: (usize, &[u8]) = (0x5849, &[0x8b, 0xb7]);
pub(crate) const ROSTER_COUNT: (usize, &[u8]) = (0x5863, &[0x83, 0xfb]);
/// The program panel (`ui_draw_program_panel`): per robot kind, its font, the text's
/// x offset, its colours (fill, text) and its pressed recolouring; and the text colour
/// while the robot runs its program (`mov al, colour` at `1000:d2f6`).
pub(crate) const PANEL_X_OFFSETS: (usize, &[u8]) = (0xd2da, &[0x8b, 0x84]);
pub(crate) const PANEL_FONTS: (usize, &[u8]) = (0xd2df, &[0x8b, 0x84]);
pub(crate) const PANEL_COLOURS: (usize, &[u8]) = (0xd2ea, &[0x8b, 0x84]);
pub(crate) const PANEL_PRESSED: (usize, &[u8]) = (0xd31f, &[0x8b, 0x84]);
pub(crate) const PANEL_PILOT: (usize, u8) = (0xd2f6, 0xb0);
/// The message panel, as a console's opener sets it up (`1000:d1f3`): the font
/// (`mov ax, font` at `1000:d20d`), the colours by robot kind (`mov ax, [si + table]`
/// at `1000:d221`) and the colour of codes with bit 7 (`mov al, colour` at
/// `1000:d22c`).
pub(crate) const MESSAGE_FONT: (usize, &[u8]) = (0xd20d, &[0xb8]);
pub(crate) const MESSAGE_COLOURS: (usize, &[u8]) = (0xd221, &[0x8b, 0x84]);
pub(crate) const MESSAGE_HIGH: (usize, u8) = (0xd22c, 0xb0);
/// A console's second display (the colour-254 region): its view mode by display mode
/// (`mov al, [bx + table]` at `1000:dbf2`), the device each display mode needs
/// (`mov al, [bx + table]` at `1000:db6e`, tested against +0xf6), and per robot kind
/// the map's clear and hill colours (`mov byte [0x1457], clear; mov byte [0x1458],
/// hill` in the kinds' draw handlers).
pub(crate) const DISPLAY_VIEW_MODES: (usize, &[u8]) = (0xdbf2, &[0x8a, 0x87]);
pub(crate) const DISPLAY_DEVICES: (usize, &[u8]) = (0xdb6e, &[0x8a, 0x87]);
pub(crate) const DISPLAY_MAP_COLOURS: [usize; 3] = [0x9e14, 0x812d, 0xb5cc];
const SET_MAP_CLEAR: [u8; 4] = [0xc6, 0x06, 0x57, 0x14];
const SET_MAP_HILL: [u8; 4] = [0xc6, 0x06, 0x58, 0x14];
/// The pointer's shapes over the views' regions (T-106; `docs/screens.md`, "Shape and
/// visibility"): the situation room's main view takes the shape the word table names
/// by its mode byte (`mov ax, [bx + table]` at `1000:575b`); a humanoid console's
/// mouse handlers `mov ax, shape` over the 3D view (`1000:9dba`), the second display
/// (`9f77`), the second display in its map mode with Alt (`9f88`) and the third
/// display (`a02b`). The scorpion's and scout's handlers use the same numbers.
pub(crate) const MAIN_VIEW_SHAPES: (usize, &[u8]) = (0x575b, &[0x8b, 0x87]);
pub(crate) const CONSOLE_SHAPES: [usize; 4] = [0x9dba, 0x9f77, 0x9f88, 0xa02b];
/// Where the console's unit-name button prints (`mov ax, y; mov bx, x` at
/// `1000:7d5d`).
pub(crate) const UNIT_LABEL: (usize, &[u8]) = (0x7d5d, &[0xb8]);
/// The training menu (`ui_run_training_menu`): its widget set, the state block, the
/// key table, the action table and the action variable it dispatches on.
pub(crate) const TRAINING_WIDGETS: (usize, &[u8]) = (0x6e01, &[0xb8]);
pub(crate) const TRAINING_FIRST: (usize, &[u8]) = (0x6e0d, &[0xbb]);
pub(crate) const TRAINING_END: (usize, &[u8]) = (0x6e13, &[0xba]);
pub(crate) const TRAINING_KEYS: (usize, &[u8]) = (0x6eea, &[0xbb]);
pub(crate) const TRAINING_KEY_COUNT: (usize, &[u8]) = (0x6eed, &[0xb9]);
pub(crate) const TRAINING_ACTION: (usize, &[u8]) = (0x6e3d, &[0x8b, 0x1e]);
pub(crate) const TRAINING_ACTIONS: (usize, &[u8]) = (0x6e41, &[0xff, 0xa7]);

/// Widget types (offsets into the host's handler tables) the port gives a meaning to.
pub const PROGRAM_PANEL_TYPE: u16 = 0x154;
pub const UNIT_BUTTON_TYPE: u16 = 0x156;
/// A console's message panel (`1000:d241`).
pub const MESSAGE_PANEL_TYPE: u16 = 0x152;
/// A 2 by 2 hot spot in the top-left corner: back to the situation room from a
/// console, the end of the mission from the situation room.
pub const CONSOLE_CORNER_TYPE: u16 = 0x158;
pub const SITUATION_CORNER_TYPE: u16 = 0x15a;
/// The situation room's roster and the training menu's buttons.
pub const ROSTER_TYPE: u16 = 0x52;
pub const TRAINING_BUTTON_TYPE: u16 = 0x32;

/// A widget's rectangle words are this when it has none.
const NO_AREA: u16 = 0x7fff;
const WIDGET_SIZE: usize = 12;
const MAX_ENTRIES: usize = 64;
/// The image offset of segment `1000`, where every handler lives.
const CODE: usize = 0;

/// A key-table handler: `test ax, 0x1800; jz; mov ax, [si + 2]; and al, 0x7f; mov bx,
/// table; mov cx, count`.
const KEY_HANDLER_HEAD: [u8; 10] = [0xa9, 0x00, 0x18, 0x74, 0x14, 0x8b, 0x44, 0x02, 0x24, 0x7f];
/// A sprite button's draw handler: `mov di, [object]; mov al, [di + pressed]; cbw;
/// add ax, handle`.
const SPRITE_BUTTON_HEAD: [u8; 8] = [0x8b, 0x3e, 0xf1, 0x62, 0x8a, 0x45, 0x40, 0x98];
const ADD_AX: u8 = 0x05;
/// A training key handler: `mov word [action], value; ret`.
const SET_ACTION: [u8; 2] = [0xc7, 0x06];
/// A training action: `mov si, file name` (then a jump to the mission), or `ret` (back
/// to the map room).
const LOAD_MISSION: u8 = 0xbe;
const BACK: u8 = 0xc3;

/// A widget: its type, where its state is in the view's state block, its rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widget {
    pub kind: u16,
    pub state: u16,
    pub area: Option<Rect>,
}

/// One view's widget set and key table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The top-level widgets, in order.
    pub widgets: Vec<Widget>,
    /// The widgets after them, up to a zero type: the members of the groups (type
    /// 0x04, whose state names its first and last member by their offsets in the set).
    pub nested: Vec<Widget>,
    /// Where the view's state block starts in the viewed object.
    pub state_offset: u16,
    /// (key word, handler offset in segment `1000`), searched in order.
    pub keys: Vec<(u16, u16)>,
}

impl View {
    pub fn find(&self, kind: u16) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.kind == kind)
    }

    /// Every widget of `kind`, top-level or nested, in order.
    pub fn all(&self, kind: u16) -> impl Iterator<Item = &Widget> {
        self.widgets
            .iter()
            .chain(&self.nested)
            .filter(move |w| w.kind == kind)
    }
}

/// A situation room button drawn from its sprite (opaque, at its rectangle's corner),
/// the next sprite while pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteButton {
    pub kind: u16,
    pub area: Rect,
    pub handle: u16,
}

/// How the console's program panel looks for one robot kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelStyle {
    pub font: u16,
    /// The name's x offset from the panel's left edge; it is one row below the top.
    pub x_offset: u16,
    pub fill: u8,
    /// The name's colour while the program drives.
    pub program_text: u8,
    /// While pressed, pixels of one of these colours become the other.
    pub pressed: [u8; 2],
}

/// The mission's views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionViews {
    pub humanoid: View,
    pub scorpion: View,
    pub scout: View,
    pub situation: View,
    /// The handle the situation room's sprite bank (`msprite1`) is loaded at.
    pub situation_bank: u16,
    pub situation_buttons: Vec<SpriteButton>,
    /// The roster's rows, top to bottom.
    pub roster_rows: Vec<Rect>,
    /// By robot kind.
    pub program_panel: [PanelStyle; 3],
    /// The name's colour while the pilot drives.
    pub pilot_text: u8,
    /// Where the unit-name button prints, (x, y).
    pub unit_label: (u16, u16),
    pub message_panel: MessageStyle,
    pub second_display: SecondDisplay,
    pub pointer_shapes: PointerShapes,
}

/// The pointer's shapes over the views' regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerShapes {
    /// The situation room's main view (its colour-255 region), by mode / 2 (the
    /// base's +0x37: 0, 2, 4, 6, 8).
    pub main_view: [u8; 5],
    /// A console's 3D view (colour 255), second display (254), the second display in
    /// its map mode with Alt held, and third display (253).
    pub console: [u8; 4],
}

/// A console's second display (`docs/screens.md`, "The second display (T-104)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondDisplay {
    /// The view mode (`docs/render.md`, "The step lists") by display mode (+0xef, its
    /// bit 2 set while the mode's device is damaged).
    pub view_modes: [u8; 8],
    /// The +0xf6 bit of the device each display mode needs (0: none).
    pub devices: [u8; 4],
    /// By robot kind: the map's clear and hill colours.
    pub map_colours: [[u8; 2]; 3],
}

/// How a console's message panel looks (`docs/screens.md`, "The message panel
/// (T-104)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageStyle {
    pub font: u16,
    /// By robot kind: the fill (and the text's background) and the text's colour.
    pub colours: [[u8; 2]; 3],
    /// The colour of codes with bit 7 set.
    pub high: u8,
}

impl MissionViews {
    pub fn console(&self, kind: RobotKind) -> &View {
        match kind {
            RobotKind::Humanoid => &self.humanoid,
            RobotKind::Scorpion => &self.scorpion,
            RobotKind::Scout => &self.scout,
        }
    }
}

/// What a training menu button or key does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrainingAction {
    /// Load this `.USG` file (padded 8.3 name) and run the mission.
    Mission(Vec<u8>),
    /// Back to the map room.
    Back,
}

/// The training menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingMenu {
    pub widgets: Vec<Widget>,
    /// Each button's rectangle and its action number.
    pub buttons: Vec<(Rect, u16)>,
    /// (key word, action number), searched in order.
    pub keys: Vec<(u16, u16)>,
    /// By action number / 2.
    pub actions: Vec<TrainingAction>,
}

impl TrainingMenu {
    pub fn action(&self, number: u16) -> Option<&TrainingAction> {
        self.actions.get(usize::from(number / 2))
    }
}

struct Exe<'a> {
    image: &'a [u8],
    ds: usize,
    relocated: &'a [usize],
}

impl Exe<'_> {
    fn op(&self, (at, op): (usize, &[u8])) -> Result<u16, FormatError> {
        operand(self.image, at, op)
    }

    /// The DS word at the table the instruction at `entry` names, plus `index`.
    fn table_word(&self, entry: (usize, &[u8]), index: usize) -> Result<u16, FormatError> {
        word(self.image, self.ds + usize::from(self.op(entry)?) + index)
    }

    fn bytes(&self, at: usize, len: usize) -> Result<&[u8], FormatError> {
        self.image
            .get(at..at + len)
            .ok_or_else(|| invalid(at, "past the end of the image"))
    }

    /// Widgets from `first` to `end` in the set at image offset `set`.
    fn widgets(&self, set: usize, first: u16, end: u16) -> Result<Vec<Widget>, FormatError> {
        let (first, end) = (usize::from(first), usize::from(end));
        if first > end
            || (end - first) % WIDGET_SIZE != 0
            || end - first > MAX_ENTRIES * WIDGET_SIZE
        {
            return Err(invalid(set, "not a list of widgets"));
        }
        (set + first..set + end)
            .step_by(WIDGET_SIZE)
            .map(|at| {
                let area = rect(self.image, at + 4)?;
                Ok(Widget {
                    kind: word(self.image, at)?,
                    state: word(self.image, at + 2)?,
                    area: (area.top != NO_AREA).then_some(area),
                })
            })
            .collect()
    }

    /// The key table of the first widget whose key handler is a table lookup.
    fn keys(&self, widgets: &[Widget]) -> Result<Vec<(u16, u16)>, FormatError> {
        for widget in widgets.iter().filter(|w| w.area.is_none()) {
            let handler = usize::from(self.table_word(KEY_HANDLERS, usize::from(widget.kind))?);
            let at = CODE + handler;
            if self.bytes(at, KEY_HANDLER_HEAD.len())? != KEY_HANDLER_HEAD {
                continue;
            }
            let table = usize::from(operand(self.image, at + 10, &[0xbb])?);
            let count = usize::from(operand(self.image, at + 13, &[0xb9])?);
            if count > MAX_ENTRIES {
                return Err(invalid(at, "too many keys"));
            }
            return (0..count)
                .map(|i| {
                    let entry = self.ds + table + 4 * i;
                    Ok((word(self.image, entry)?, word(self.image, entry + 2)?))
                })
                .collect();
        }
        Err(invalid(0, "no key table among the widgets"))
    }

    /// The view opened for object kind `kind`.
    fn view(&self, kind: usize) -> Result<View, FormatError> {
        let at = CODE + usize::from(self.table_word(VIEW_OPENERS, 2 * kind)?);
        let set = segment(self.image, (at, &[0xb8]), self.relocated)?;
        let first = operand(self.image, at + 3, &[0xbb])?;
        let state_offset = operand(self.image, at + 6, &[0x81, 0xc1])?;
        let end = operand(self.image, at + 10, &[0xba])?;
        let widgets = self.widgets(set, first, end)?;
        let keys = self.keys(&widgets)?;
        let nested = (set + usize::from(end)..)
            .step_by(WIDGET_SIZE)
            .take(MAX_ENTRIES)
            .map_while(|at| match word(self.image, at) {
                Ok(0) | Err(_) => None,
                Ok(_) => self.widgets(at, 0, WIDGET_SIZE as u16).ok()?.pop(),
            })
            .collect();
        Ok(View {
            widgets,
            nested,
            state_offset,
            keys,
        })
    }

    /// The widgets of `view` drawn as sprite buttons.
    fn sprite_buttons(&self, view: &View) -> Result<Vec<SpriteButton>, FormatError> {
        let mut buttons = Vec::new();
        for widget in &view.widgets {
            let Some(area) = widget.area else { continue };
            let at = CODE + usize::from(self.table_word(DRAW_HANDLERS, usize::from(widget.kind))?);
            let head = self.bytes(at, SPRITE_BUTTON_HEAD.len())?;
            if head == SPRITE_BUTTON_HEAD {
                let handle = operand(self.image, at + head.len(), &[ADD_AX])?;
                buttons.push(SpriteButton {
                    kind: widget.kind,
                    area,
                    handle,
                });
            }
        }
        Ok(buttons)
    }

    fn panel(&self) -> Result<([PanelStyle; 3], u8), FormatError> {
        let pilot = match self.bytes(PANEL_PILOT.0, 2)? {
            &[op, colour] if op == PANEL_PILOT.1 => colour,
            _ => return Err(invalid(PANEL_PILOT.0, "not the program panel's colour")),
        };
        let mut styles = [PanelStyle {
            font: 0,
            x_offset: 0,
            fill: 0,
            program_text: 0,
            pressed: [0; 2],
        }; 3];
        for (kind, style) in styles.iter_mut().enumerate() {
            let [fill, program_text] = self.table_word(PANEL_COLOURS, 2 * kind)?.to_le_bytes();
            let [a, b] = self.table_word(PANEL_PRESSED, 2 * kind)?.to_le_bytes();
            *style = PanelStyle {
                font: self.table_word(PANEL_FONTS, 2 * kind)?,
                x_offset: self.table_word(PANEL_X_OFFSETS, 2 * kind)?,
                fill,
                program_text,
                pressed: [a, b],
            };
        }
        Ok((styles, pilot))
    }

    fn second_display(&self) -> Result<SecondDisplay, FormatError> {
        let table = |entry, len| -> Result<&[u8], FormatError> {
            self.bytes(self.ds + usize::from(self.op(entry)?), len)
        };
        let mut map_colours = [[0; 2]; 3];
        for (colours, at) in map_colours.iter_mut().zip(DISPLAY_MAP_COLOURS) {
            match self.bytes(at, 10)? {
                &[a, b, c, d, clear, e, f, g, h, hill]
                    if [a, b, c, d] == SET_MAP_CLEAR && [e, f, g, h] == SET_MAP_HILL =>
                {
                    *colours = [clear, hill]
                }
                _ => return Err(invalid(at, "not the second display's map colours")),
            }
        }
        Ok(SecondDisplay {
            view_modes: table(DISPLAY_VIEW_MODES, 8)?.try_into().expect("8 bytes"),
            devices: table(DISPLAY_DEVICES, 4)?.try_into().expect("4 bytes"),
            map_colours,
        })
    }

    fn pointer_shapes(&self) -> Result<PointerShapes, FormatError> {
        let mut shapes = PointerShapes {
            main_view: [0; 5],
            console: [0; 4],
        };
        for (i, shape) in shapes.main_view.iter_mut().enumerate() {
            *shape = self.table_word(MAIN_VIEW_SHAPES, 2 * i)?.to_le_bytes()[0];
        }
        for (shape, at) in shapes.console.iter_mut().zip(CONSOLE_SHAPES) {
            *shape = operand(self.image, at, &[0xb8])?.to_le_bytes()[0];
        }
        Ok(shapes)
    }

    fn message_panel(&self) -> Result<MessageStyle, FormatError> {
        let high = match self.bytes(MESSAGE_HIGH.0, 2)? {
            &[op, colour] if op == MESSAGE_HIGH.1 => colour,
            _ => return Err(invalid(MESSAGE_HIGH.0, "not the message panel's colour")),
        };
        let mut colours = [[0; 2]; 3];
        for (kind, c) in colours.iter_mut().enumerate() {
            *c = self.table_word(MESSAGE_COLOURS, 2 * kind)?.to_le_bytes();
        }
        Ok(MessageStyle {
            font: self.op(MESSAGE_FONT)?,
            colours,
            high,
        })
    }
}

/// Reads the mission's views from a load image whose data segment starts at `ds`;
/// `relocated` lists the image offsets of the words the executable relocates.
pub fn read_image(
    image: &[u8],
    ds: usize,
    relocated: &[usize],
) -> Result<MissionViews, FormatError> {
    let exe = Exe {
        image,
        ds,
        relocated,
    };
    let situation = exe.view(3)?;
    let situation_buttons = exe.sprite_buttons(&situation)?;
    let rows = match exe.bytes(ROSTER_COUNT.0, 3)? {
        &[a, b, count] if [a, b] == *ROSTER_COUNT.1 => usize::from(count / 2),
        _ => return Err(invalid(ROSTER_COUNT.0, "not the roster's row count")),
    };
    let roster_rows = (0..rows)
        .map(|i| rect(image, ds + usize::from(exe.table_word(ROSTER_ROWS, 2 * i)?)))
        .collect::<Result<_, _>>()?;
    let (program_panel, pilot_text) = exe.panel()?;
    Ok(MissionViews {
        humanoid: exe.view(0)?,
        scorpion: exe.view(1)?,
        scout: exe.view(2)?,
        situation,
        situation_bank: exe.op(SITUATION_BANK)?,
        situation_buttons,
        roster_rows,
        program_panel,
        pilot_text,
        unit_label: (
            operand(image, UNIT_LABEL.0 + 3, &[0xbb])?,
            exe.op(UNIT_LABEL)?,
        ),
        message_panel: exe.message_panel()?,
        second_display: exe.second_display()?,
        pointer_shapes: exe.pointer_shapes()?,
    })
}

/// Reads the training menu from a load image whose data segment starts at `ds`.
pub fn read_training_image(
    image: &[u8],
    ds: usize,
    relocated: &[usize],
) -> Result<TrainingMenu, FormatError> {
    let exe = Exe {
        image,
        ds,
        relocated,
    };
    let set = segment(image, TRAINING_WIDGETS, relocated)?;
    let widgets = exe.widgets(set, exe.op(TRAINING_FIRST)?, exe.op(TRAINING_END)?)?;
    if !relocated.contains(&(set + 4)) {
        return Err(invalid(
            set + 4,
            "the state template's segment is not relocated",
        ));
    }
    let template = usize::from(word(image, set + 4)?) * 16;
    let template_size = usize::from(word(image, set + 8)?);
    let mut buttons = Vec::new();
    for widget in widgets.iter().filter(|w| w.kind == TRAINING_BUTTON_TYPE) {
        // The button's state: a redraw byte, then its action.
        let state = usize::from(widget.state);
        let (Some(area), true) = (widget.area, state + 3 <= template_size) else {
            return Err(invalid(set, "not a training button"));
        };
        buttons.push((area, word(image, template + state + 1)?));
    }

    let variable = exe.op(TRAINING_ACTION)?;
    let count = exe.op(TRAINING_KEY_COUNT)?;
    if usize::from(count) > MAX_ENTRIES {
        return Err(invalid(TRAINING_KEY_COUNT.0, "too many keys"));
    }
    let keys = (0..usize::from(count))
        .map(|i| {
            let key = exe.table_word(TRAINING_KEYS, 4 * i)?;
            let handler = CODE + usize::from(exe.table_word(TRAINING_KEYS, 4 * i + 2)?);
            let action = operand(image, handler, &SET_ACTION)?;
            if action != variable {
                return Err(invalid(handler, "not a training key"));
            }
            Ok((key, operand(image, handler + 4, &[])?))
        })
        .collect::<Result<Vec<_>, FormatError>>()?;

    let last = buttons
        .iter()
        .map(|&(_, a)| a)
        .chain(keys.iter().map(|&(_, a)| a))
        .max()
        .ok_or_else(|| invalid(set, "the training menu does nothing"))?;
    if last % 2 != 0 || usize::from(last) >= 2 * MAX_ENTRIES {
        return Err(invalid(set, "not a training action"));
    }
    let actions = (0..=usize::from(last) / 2)
        .map(|i| {
            let at = CODE + usize::from(exe.table_word(TRAINING_ACTIONS, 2 * i)?);
            match exe.bytes(at, 1)?[0] {
                LOAD_MISSION => {
                    let name = ds + usize::from(operand(image, at, &[LOAD_MISSION])?);
                    Ok(TrainingAction::Mission(string(image, name)?))
                }
                BACK => Ok(TrainingAction::Back),
                _ => Err(invalid(at, "not a training action")),
            }
        })
        .collect::<Result<_, FormatError>>()?;
    Ok(TrainingMenu {
        widgets,
        buttons,
        keys,
        actions,
    })
}

fn relocations(exe: &Unpacked) -> Vec<usize> {
    exe.relocations.iter().map(|r| r.linear()).collect()
}

/// Reads the mission's views from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<MissionViews, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?, &relocations(exe))
}

/// Reads the training menu from an unpacked `xeno.exe`.
pub fn read_training(exe: &Unpacked) -> Result<TrainingMenu, FormatError> {
    read_training_image(&exe.image, sequence::data_segment(exe)?, &relocations(exe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{SyntheticViews, synthetic_views};

    const DS: usize = 0x20000;

    fn synthetic() -> (Vec<u8>, Vec<usize>) {
        let mut image = vec![0u8; DS + 0x800];
        let relocated = synthetic_views(&mut image, DS);
        (image, relocated)
    }

    #[test]
    fn synthetic_views_read() {
        let (image, relocated) = synthetic();
        let views = read_image(&image, DS, &relocated).unwrap();
        // Each console has the program panel and its own key table.
        for kind in RobotKind::ALL {
            let view = views.console(kind);
            assert!(view.find(PROGRAM_PANEL_TYPE).is_some(), "{kind:?}");
            assert_eq!(view.keys[0].0, 0x3c);
        }
        assert_eq!(views.humanoid.state_offset, SyntheticViews::STATE);
        assert_eq!(views.scout.state_offset, SyntheticViews::STATE + 2);
        // The situation room's one sprite button and its roster.
        assert_eq!(views.situation_buttons.len(), 1);
        assert_eq!(views.situation_buttons[0].handle, 0x2a0);
        assert_eq!(views.roster_rows.len(), 2);
        insta::assert_debug_snapshot!(views);
    }

    #[test]
    fn a_synthetic_training_menu_reads() {
        let (image, relocated) = synthetic();
        let menu = read_training_image(&image, DS, &relocated).unwrap();
        assert_eq!(menu.buttons.len(), 2);
        assert_eq!(
            menu.action(menu.buttons[0].1),
            Some(&TrainingAction::Mission(b"ATRAIN.USG".to_vec()))
        );
        assert_eq!(menu.action(menu.buttons[1].1), Some(&TrainingAction::Back));
        assert_eq!(menu.keys, [(0x01, 2), (0x1e, 0)]);
        insta::assert_debug_snapshot!(menu);
    }

    #[test]
    fn other_code_or_data_is_refused() {
        let corrupt = |at: usize, value: u8| {
            let (mut image, relocated) = synthetic();
            image[at] = value;
            (
                read_image(&image, DS, &relocated).is_err(),
                read_training_image(&image, DS, &relocated).is_err(),
            )
        };
        for (at, _) in [
            VIEW_OPENERS,
            DRAW_HANDLERS,
            KEY_HANDLERS,
            SITUATION_BANK,
            PANEL_FONTS,
        ] {
            assert!(corrupt(at, 0x90).0, "{at:#x}");
        }
        assert!(corrupt(ROSTER_COUNT.0, 0x90).0);
        assert!(corrupt(PANEL_PILOT.0, 0x90).0);
        assert!(corrupt(MESSAGE_HIGH.0, 0x90).0);
        assert!(corrupt(MESSAGE_COLOURS.0, 0x90).0);
        assert!(corrupt(DISPLAY_DEVICES.0, 0x90).0);
        assert!(corrupt(DISPLAY_MAP_COLOURS[2] + 5, 0x90).0);
        assert!(corrupt(MAIN_VIEW_SHAPES.0, 0x90).0);
        assert!(corrupt(CONSOLE_SHAPES[3], 0x90).0);
        for (at, _) in [
            TRAINING_WIDGETS,
            TRAINING_KEYS,
            TRAINING_ACTION,
            TRAINING_ACTIONS,
        ] {
            assert!(corrupt(at, 0x90).1, "{at:#x}");
        }
        // A key handler that is not a table lookup leaves the view without keys.
        assert!(corrupt(SyntheticViews::KEY_HANDLER, 0x90).0);
        // A training action that is neither a mission nor back.
        assert!(corrupt(SyntheticViews::BACK_ACTION, 0x90).1);
        // Segments that are not relocated.
        let (image, relocated) = synthetic();
        assert!(read_image(&image, DS, &[]).is_err());
        assert!(read_training_image(&image, DS, &relocated[..1]).is_err());
    }

    /// Reads the views and the training menu from the user's `xeno.exe`, checking only
    /// their shape.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_views_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let views = read(&exe).unwrap();
        // `docs/screens.md`: 27, 37 and 39 widgets in the consoles, 30 in the
        // situation room; 33, 38, 40 and 32 keys.
        let counts = |v: &View| (v.widgets.len(), v.keys.len());
        assert_eq!(counts(&views.humanoid), (27, 33));
        // The situation room's two groups: the map controls and the mode buttons.
        let nested: Vec<u16> = views.situation.nested.iter().map(|w| w.kind).collect();
        assert_eq!(
            nested,
            [
                0x64, 0x68, 0x66, 0x6a, 0x6c, 0x6e, 0x70, 0x70, 0x70, 0x70, 0x70
            ]
        );
        assert_eq!(counts(&views.scorpion), (37, 38));
        assert_eq!(counts(&views.scout), (39, 40));
        assert_eq!(counts(&views.situation), (30, 32));
        // T-106: shape 3 over modes M and R; 3, 4, 6, 4 in the consoles.
        assert_eq!(views.pointer_shapes.main_view, [0, 0, 3, 3, 0]);
        assert_eq!(views.pointer_shapes.console, [3, 4, 6, 4]);
        for kind in RobotKind::ALL {
            let view = views.console(kind);
            for t in [
                PROGRAM_PANEL_TYPE,
                UNIT_BUTTON_TYPE,
                CONSOLE_CORNER_TYPE,
                MESSAGE_PANEL_TYPE,
            ] {
                assert!(view.find(t).is_some(), "{kind:?} {t:#x}");
            }
        }
        // The message panel: one of the four fonts (0, 2, 4, 6).
        assert!(views.message_panel.font <= 6 && views.message_panel.font.is_multiple_of(2));
        // Six buttons and three rows of left and right arrows.
        assert_eq!(views.situation_buttons.len(), 12);
        assert_eq!(views.roster_rows.len(), 5);
        let menu = read_training(&exe).unwrap();
        assert_eq!(menu.buttons.len(), 5);
        assert_eq!(menu.keys.len(), 6);
        let missions = menu
            .actions
            .iter()
            .filter(|a| matches!(a, TrainingAction::Mission(_)))
            .count();
        assert_eq!(missions, 4);
        for (_, action) in &menu.buttons {
            assert!(menu.action(*action).is_some());
        }
    }
}
