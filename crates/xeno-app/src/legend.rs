//! The key legend (T-217), a port-only aid: Ctrl+F12 labels each widget of a robot
//! console or the situation room that has a key with that key's name (`docs/screens.md`,
//! "Keys that only the port uses"). The widgets and their rectangles are the view's,
//! read from the user's `xeno.exe`; which key each widget type stands for, and the key
//! names, are the port's own text, from the key tables traced in `docs/sim.md`, "The
//! cockpit keys (T-105)", and `docs/screens.md` (T-043, T-103, T-127, T-130 to T-132,
//! T-134, T-188).

use xeno_formats::maproom::Rect;
use xeno_formats::views::{
    CONSOLE_CORNER_TYPE, PROGRAM_PANEL_TYPE, ROSTER_TYPE, SITUATION_CORNER_TYPE, UNIT_BUTTON_TYPE,
    View, Widget,
};
use xeno_sim::orders::RobotKind;

use crate::console::PAN_TYPE;
use crate::mission::button;
use crate::{humancon as h, mainview, panels, scorpcon as c, scoutcon as s};

/// A label: its key names, shown joined by `/` (a left and a right release).
type Keys = &'static [&'static str];

/// Every console: the pan readout (a left release pans to 0, as Y), the program panel
/// (a left release, as F4), the unit-name button (Tab) and the corner (ESC).
const CONSOLE: [(u16, Keys); 4] = [
    (PAN_TYPE, &["Y"]),
    (PROGRAM_PANEL_TYPE, &["F4"]),
    (UNIT_BUTTON_TYPE, &["TAB"]),
    (CONSOLE_CORNER_TYPE, &["ESC"]),
];
/// Each console's second display buttons (T-188): pan less and more, zoom (a left
/// release zooms in, a right one out) and the camera, thermal and map modes.
fn display(pan: [u16; 2], zoom: u16, modes: [u16; 3]) -> [(u16, Keys); 6] {
    [
        (pan[0], &["Z"]),
        (pan[1], &["X"]),
        (zoom, &["=", "-"]),
        (modes[0], &["V"]),
        (modes[1], &["T"]),
        (modes[2], &["M"]),
    ]
}
const HUMANOID: [(u16, Keys); 10] = [
    (h::FIRE_TYPE, &["SPACE"]),
    (h::PILOT_TYPE, &["F2"]),
    (h::PROGRAM_TYPE, &["F3"]),
    (h::LOCK_TYPE, &["A", "R"]),
    (h::CLOAK_TYPE, &["F8"]),
    (h::ECM_TYPE, &["F7"]),
    (h::LEFT_TYPE, &["LEFT"]),
    (h::RIGHT_TYPE, &["RIGHT"]),
    (h::GEAR_TYPE, &["UP", "DOWN"]),
    (h::STATUS_TYPE, &["S"]),
];
const SCORPION: [(u16, Keys); 13] = [
    (c::FIRE_TYPE, &["SPACE"]),
    (c::PILOT_TYPE, &["F2"]),
    (c::PROGRAM_TYPE, &["F3"]),
    (c::LOCK_TYPE, &["A", "R"]),
    (c::FASTER_TYPE, &["UP"]),
    (c::STOP_TYPE, &["."]),
    (c::SLOWER_TYPE, &["DOWN"]),
    (c::LEFT_TYPE, &["LEFT"]),
    (c::RIGHT_TYPE, &["RIGHT"]),
    (c::CARGO_TYPE, &["C"]),
    (c::DUMP_TYPE, &["D"]),
    (c::LOAD_TYPE, &["L"]),
    (c::DEPLOY_TYPE, &["U"]),
];
const SCOUT: [(u16, Keys); 13] = [
    (s::FIRE_TYPE, &["SPACE"]),
    (s::PILOT_TYPE, &["F2"]),
    (s::PROGRAM_TYPE, &["F3"]),
    (s::LOCK_TYPE, &["A", "R"]),
    (s::CLOAK_TYPE, &["F8"]),
    (s::ECM_TYPE, &["F7"]),
    (s::STANCE_TYPES[0], &["E"]),
    (s::STANCE_TYPES[1], &["W"]),
    (s::STANCE_TYPES[2], &["Q"]),
    (s::LEFT_TYPE, &["LEFT"]),
    (s::RIGHT_TYPE, &["RIGHT"]),
    (s::GEAR_TYPE, &["UP", "DOWN"]),
    (s::STATUS_TYPE, &["S"]),
];
/// The weapon buttons' weapon `n` is F5 or F6; the drop buttons' `n` (0 to 3) is keys 1,
/// 4, 3 and 2 (`CS:0xd5df`).
const WEAPONS: [&str; 2] = ["F5", "F6"];
const DROPS: [&str; 4] = ["1", "4", "3", "2"];
/// The situation room (T-043, T-103, T-127): the roster (Tab steps its selection), the
/// buttons below it (view, launch, recall, end), the corner (the end), the map's zoom
/// and overlays (`mainview`'s toggles: U, I, B, N, G).
const SITUATION: [(u16, Keys); 12] = [
    (ROSTER_TYPE, &["TAB"]),
    (button::VIEW, &["SPACE"]),
    (button::LAUNCH, &["F2"]),
    (button::RECALL, &["F3"]),
    (button::END, &["ALT+Q"]),
    (SITUATION_CORNER_TYPE, &["ALT+Q"]),
    (mainview::ZOOM_TYPE, &["=", "-"]),
    (0x66, &["U"]),
    (0x68, &["I"]),
    (0x6a, &["B"]),
    (0x6c, &["N"]),
    (0x6e, &["G"]),
];
/// The mode buttons' mode (0, 2, 4, 6, 8) is O, S, M, R or C; a power row's (0, 2, 4)
/// left arrow is keys 1, 3, 5 and its right arrow 2, 4, 6.
const MODES: [&str; 5] = ["O", "S", "M", "R", "C"];
const ROWS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

/// With WASD on (T-216, `translate`'s swap), each letter and its arrow trade labels.
const WASD: [(&str, &str); 4] = [("W", "UP"), ("A", "LEFT"), ("S", "DOWN"), ("D", "RIGHT")];

fn swapped(key: &'static str) -> &'static str {
    WASD.iter()
        .find_map(|&(letter, arrow)| match key {
            k if k == letter => Some(arrow),
            k if k == arrow => Some(letter),
            _ => None,
        })
        .unwrap_or(key)
}

/// Each widget of `view` with an area and a key, and its label. `kind` is the console's
/// robot kind, `None` for the situation room; `byte` gives a widget's state's second
/// byte (a weapon, drop, mode or row number); `wasd` labels through T-216's swap (only
/// the consoles swap).
pub fn labels(
    view: &View,
    kind: Option<RobotKind>,
    byte: impl Fn(&Widget) -> Option<u8>,
    wasd: bool,
) -> Vec<(Rect, String)> {
    let (fixed, weapon, drop): (Vec<(u16, Keys)>, _, _) = match kind {
        None => (SITUATION.to_vec(), None, None),
        Some(RobotKind::Humanoid) => (
            [
                &CONSOLE[..],
                &HUMANOID,
                &display(h::PAN_TYPES, h::ZOOM_TYPE, h::MODE_TYPES),
            ]
            .concat(),
            Some(h::WEAPON_TYPE),
            None,
        ),
        Some(RobotKind::Scorpion) => (
            [
                &CONSOLE[..],
                &SCORPION,
                &display(c::PAN_TYPES, c::ZOOM_TYPE, c::MODE_TYPES),
            ]
            .concat(),
            Some(c::WEAPON_TYPE),
            Some(c::DROP_TYPE),
        ),
        Some(RobotKind::Scout) => (
            [
                &CONSOLE[..],
                &SCOUT,
                &display(s::PAN_TYPES, s::ZOOM_TYPE, s::MODE_TYPES),
            ]
            .concat(),
            Some(s::WEAPON_TYPE),
            Some(s::DROP_TYPE),
        ),
    };
    let numbered = |w: &Widget, table: &[&'static str], n: fn(u8) -> usize| {
        byte(w).and_then(|b| table.get(n(b)).copied())
    };
    let mut out = Vec::new();
    for w in view.widgets.iter().chain(&view.nested) {
        let Some(area) = w.area else { continue };
        let keys: Vec<&'static str> = match w.kind {
            k if Some(k) == weapon => numbered(w, &WEAPONS, usize::from).into_iter().collect(),
            k if Some(k) == drop => numbered(w, &DROPS, usize::from).into_iter().collect(),
            mainview::MODE_BUTTON_TYPE if kind.is_none() => {
                numbered(w, &MODES, |b| usize::from(b / 2))
                    .into_iter()
                    .collect()
            }
            panels::LOWER_TYPE if kind.is_none() => {
                numbered(w, &ROWS, usize::from).into_iter().collect()
            }
            panels::RAISE_TYPE if kind.is_none() => numbered(w, &ROWS, |b| usize::from(b) + 1)
                .into_iter()
                .collect(),
            k => fixed
                .iter()
                .find(|&&(t, _)| t == k)
                .map(|&(_, keys)| keys.to_vec())
                .unwrap_or_default(),
        };
        if keys.is_empty() {
            continue;
        }
        let keys: Vec<&str> = if wasd && kind.is_some() {
            keys.into_iter().map(swapped).collect()
        } else {
            keys
        };
        out.push((area, keys.join("/")));
    }
    out
}

/// A label placed: its box's top-left corner in the view, and its text, one pixel in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub x: usize,
    pub y: usize,
    pub text: String,
}

/// Places each label's box (`cell` pixels a character plus 2, `rows` plus 2 high) in a
/// `width` by `height` view: on its widget's centre, else above, below, left or right
/// of it, the first that overlaps no box placed before; on the centre if none is free.
/// Every box is kept inside the view.
// The window places them in T-217's part 2.
#[allow(dead_code)]
pub fn place(
    labels: &[(Rect, String)],
    cell: usize,
    rows: usize,
    (width, height): (usize, usize),
) -> Vec<Placed> {
    let mut boxes: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut out = Vec::new();
    for (area, text) in labels {
        let (w, h) = (cell * text.len() + 2, rows + 2);
        let (top, left) = (i32::from(area.top), i32::from(area.left));
        let (bottom, right) = (i32::from(area.bottom), i32::from(area.right));
        let (w32, h32) = (w as i32, h as i32);
        let (cx, cy) = ((left + right - w32) / 2, (top + bottom - h32) / 2);
        let clamp = |(x, y): (i32, i32)| {
            (
                x.clamp(0, (width - w.min(width)) as i32) as usize,
                y.clamp(0, (height - h.min(height)) as i32) as usize,
            )
        };
        let candidates = [
            (cx, cy),
            (cx, top - h32),
            (cx, bottom),
            (left - w32, cy),
            (right, cy),
        ]
        .map(clamp);
        let free = |&(x, y): &(usize, usize)| {
            boxes
                .iter()
                .all(|&(bx, by, bw, bh)| x + w <= bx || bx + bw <= x || y + h <= by || by + bh <= y)
        };
        let (x, y) = candidates
            .iter()
            .copied()
            .find(free)
            .unwrap_or(candidates[0]);
        boxes.push((x, y, w, h));
        out.push(Placed {
            x,
            y,
            text: text.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(kind: u16, state: u16, top: u16, left: u16) -> Widget {
        Widget {
            kind,
            state,
            area: Some(Rect {
                top,
                left,
                bottom: top + 5,
                right: left + 7,
            }),
        }
    }

    /// A view of `kinds`, each widget's state its index and its rectangle unlike any
    /// real one; the last one nested.
    fn view(kinds: &[u16]) -> View {
        let mut widgets: Vec<Widget> = (0..)
            .zip(kinds)
            .map(|(i, &k)| widget(k, i * 3, 1 + 6 * i, 300 - 9 * i))
            .collect();
        let nested = widgets.split_off(widgets.len() - 1);
        View {
            widgets,
            nested,
            state_offset: 0x40,
            keys: vec![],
        }
    }

    fn texts(labels: &[(Rect, String)]) -> Vec<&str> {
        labels.iter().map(|(_, t)| t.as_str()).collect()
    }

    /// The state's second byte by the widget's index: weapon, drop, mode or row numbers.
    fn bytes(table: &[u8]) -> impl Fn(&Widget) -> Option<u8> + '_ {
        |w| table.get(usize::from(w.state / 3)).copied()
    }

    #[test]
    fn each_console_labels_its_widgets_with_their_keys() {
        let humanoid = view(&[
            0x0e,
            h::WEAPON_TYPE,
            h::WEAPON_TYPE,
            h::FIRE_TYPE,
            h::LOCK_TYPE,
            h::GEAR_TYPE,
            h::AHEAD_TYPE,
            PAN_TYPE,
            h::MODE_TYPES[1],
        ]);
        let byte = bytes(&[0, 1, 0]);
        let labels = super::labels(&humanoid, Some(RobotKind::Humanoid), &byte, false);
        assert_eq!(
            texts(&labels),
            ["F6", "F5", "SPACE", "A/R", "UP/DOWN", "Y", "T"]
        );
        // Each label goes with its widget's rectangle (the straight button has no key).
        assert_eq!(labels[2].0, humanoid.widgets[3].area.unwrap());
        assert_eq!(labels[6].0, humanoid.nested[0].area.unwrap());
        let scorpion = view(&[
            c::DROP_TYPE,
            c::DROP_TYPE,
            c::DROP_TYPE,
            c::DROP_TYPE,
            c::STOP_TYPE,
            c::DUMP_TYPE,
            c::ZOOM_TYPE,
        ]);
        let byte = bytes(&[1, 0, 3, 2]);
        let labels = super::labels(&scorpion, Some(RobotKind::Scorpion), &byte, false);
        assert_eq!(texts(&labels), ["4", "1", "2", "3", ".", "D", "=/-"]);
        let scout = view(&[
            s::STANCE_TYPES[0],
            s::STANCE_TYPES[1],
            s::STANCE_TYPES[2],
            s::STATUS_TYPE,
            UNIT_BUTTON_TYPE,
            CONSOLE_CORNER_TYPE,
            s::LEFT_TYPE,
        ]);
        let none = bytes(&[]);
        let labels = super::labels(&scout, Some(RobotKind::Scout), &none, false);
        assert_eq!(texts(&labels), ["E", "W", "Q", "S", "TAB", "ESC", "LEFT"]);
        // Another kind's types mean nothing: the scorpion's fire button on a scout, and
        // a weapon button with no number.
        let other = view(&[c::FIRE_TYPE, s::WEAPON_TYPE, 0]);
        assert!(super::labels(&other, Some(RobotKind::Scout), &none, false).is_empty());
    }

    #[test]
    fn the_situation_room_labels_its_modes_rows_and_controls() {
        let room = view(&[
            ROSTER_TYPE,
            panels::LOWER_TYPE,
            panels::RAISE_TYPE,
            button::END,
            mainview::MODE_BUTTON_TYPE,
            button::SCROLL_UP,
            mainview::MODE_BUTTON_TYPE,
        ]);
        // Rows 0 and 4, modes 2 and 8; WASD leaves the situation room's S alone.
        let byte = bytes(&[0, 0, 4, 0, 2, 0, 8]);
        let labels = super::labels(&room, None, &byte, true);
        assert_eq!(texts(&labels), ["TAB", "1", "6", "ALT+Q", "S", "C"]);
    }

    #[test]
    fn wasd_swaps_the_arrows_and_the_letters_on_the_consoles() {
        let none = bytes(&[]);
        let scorpion = view(&[
            c::FASTER_TYPE,
            c::SLOWER_TYPE,
            c::LEFT_TYPE,
            c::RIGHT_TYPE,
            c::LOCK_TYPE,
            c::DUMP_TYPE,
            c::CARGO_TYPE,
        ]);
        let on = super::labels(&scorpion, Some(RobotKind::Scorpion), &none, true);
        assert_eq!(texts(&on), ["W", "S", "A", "D", "LEFT/R", "RIGHT", "C"]);
        let scout = view(&[s::STANCE_TYPES[1], s::STATUS_TYPE, s::GEAR_TYPE, 0]);
        let on = super::labels(&scout, Some(RobotKind::Scout), &none, true);
        assert_eq!(texts(&on), ["UP", "DOWN", "W/S"]);
    }

    #[test]
    fn labels_stay_inside_the_view_and_apart() {
        let r = |top, left, bottom, right| Rect {
            top,
            left,
            bottom,
            right,
        };
        // Two buttons side by side, one in each corner.
        let labels = [
            (r(50, 50, 57, 57), "LEFT".to_string()),
            (r(50, 57, 57, 64), "RIGHT".to_string()),
            (r(0, 0, 2, 2), "ESC".to_string()),
            (r(190, 310, 200, 320), "SPACE".to_string()),
        ];
        let placed = place(&labels, 6, 6, (320, 200));
        let boxes: Vec<_> = placed
            .iter()
            .map(|p| (p.x, p.y, 6 * p.text.len() + 2, 8))
            .collect();
        for (i, &(x, y, w, h)) in boxes.iter().enumerate() {
            assert!(x + w <= 320 && y + h <= 200, "{:?}", placed[i]);
            for &(bx, by, bw, bh) in &boxes[..i] {
                assert!(x + w <= bx || bx + bw <= x || y + h <= by || by + bh <= y);
            }
        }
        // The first on its widget's centre.
        assert_eq!((placed[0].x, placed[0].y), (53 - 13, 53 - 4));
        // The second overlaps the first on its centre and above it, so goes below.
        assert_eq!((placed[1].x, placed[1].y), (60 - 16, 57));
        assert_eq!((placed[2].x, placed[2].y), (0, 0));
        assert_eq!((placed[3].x, placed[3].y), (320 - 32, 191));
    }
}
