//! The host's file dialogs (T-110; `docs/screens.md`, "The file dialogs (T-110)"), as
//! `ui_run_file_dialog` runs them: the open and SAVE GAME dialogs over the screen, the
//! name field, the FILES and DIRECTORIES lists with their scroll bars, the keys, the
//! mouse and the error box. Their scripts, widgets and patterns are read from the
//! user's `xeno.exe` (`xeno_formats::filedialog`).
//!
//! The original searches the DOS current directory; the port starts in a directory the
//! caller gives (`--save-dir`) and returns a path in it. The chosen name is returned as
//! typed (upper case, no extension added), or as the file found when one matches it in
//! any case.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xeno_formats::ban::Sprite;
use xeno_formats::filedialog::{Dialog, FileDialogs, List, Op, Part};
use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_render::{BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};
use xeno_sim::input::MouseButton;

use crate::mission::{fill, frame, xor_fill};
use crate::screens::KeyWord;

const ENTER: KeyWord = KeyWord(0x1c);
const ESC: KeyWord = KeyWord(0x01);
const BACKSPACE: KeyWord = KeyWord(0x0e);
/// The longest name the field takes, and how much of its end is shown
/// (`ui_file_dialog_on_key`, `ui_draw_file_dialog_name`).
const MAX_NAME: usize = 80;
const SHOWN: usize = 16;
/// The field's, the lists' and the directory line's font and colours: 0 on 12.
const FONT: u16 = 2;
const INK: u8 = 0;
const PAPER: u8 = 12;
const TRACK: u8 = 11;
/// The directory line's corner and width in characters (`1fe6:890a`).
const DIRECTORY_AT: (i32, i32) = (124, 55);
const DIRECTORY_WIDTH: usize = 20;
/// The lists: eight rows of 6 pixels from row 80, names padded to 12; the files' rows
/// end at x 134, the directories' 54 pixels right of their left edge less 2.
const ROWS: usize = 8;
const ROW_HEIGHT: u16 = 6;
const FIRST_ROW: u16 = 80;
const LIST_BOTTOM: u16 = 128;
const CELLS: usize = 12;
const FILES_RIGHT: u16 = 0x86;
const DIRECTORIES_WIDTH: u16 = 0x36;
/// `msprite0` handles: the up arrow, the thumb, the down arrow; plus 8 pressed.
const UP_SPRITE: u16 = 0x1d0;
const THUMB_SPRITE: u16 = 0x1e0;
const DOWN_SPRITE: u16 = 0x1f0;
const PRESSED: u16 = 8;
/// The thumb's travel and the row its sums start from (`1fe6:8624`, `8e48`).
const THUMB_TRAVEL: i32 = 24;
const DRAG_ORIGIN: i32 = 87;
/// A page of the track, and how far below the thumb's top a press still drags.
const PAGE: usize = 7;
const THUMB_HEIGHT: i32 = 10;
/// Game-clock counts (60 Hz): a held arrow's first repeat after 30, then every 4; a
/// second release within 30 (files) or 60 (directories) accepts.
const REPEAT_DELAY: u32 = 30;
const REPEAT_EVERY: u32 = 4;
const DOUBLE_CLICK: [u32; 2] = [30, 60];
/// The lists' buffer (`1fe6:8999`): 13-byte entries, files while the next starts below
/// 0x1435, then (after one empty entry) directories while below 0x1442.
const ENTRY: usize = 13;
const FILES_END: usize = 0x1435;
const DIRECTORIES_END: usize = 0x1442;

/// Which dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Open,
    Save,
}

/// How a dialog ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Chosen(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Entries {
    names: Vec<String>,
    first: usize,
    selected: Option<usize>,
    /// The game clock at the last release that selected an entry.
    released: Option<u32>,
}

/// A running dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialog {
    kind: Kind,
    dir: PathBuf,
    name: Vec<u8>,
    lists: [Entries; 2],
    error: bool,
    /// The part the left button went down on, until it comes up.
    captured: Option<Part>,
    /// A held arrow and the clock its repeats count from.
    repeat: Option<(Part, u32)>,
    /// A thumb drag: the list, the press's row and the thumb's top then. Only the next
    /// press on a track ends it, so the thumb follows the pointer over the track after.
    drag: Option<(List, i32, i32)>,
    retraces: u32,
    dirty: bool,
}

fn index(list: List) -> usize {
    match list {
        List::Files => 0,
        List::Directories => 1,
    }
}

/// Whether `name` matches the DOS pattern `pattern` (`*` the rest of the part, `?`
/// any one character or none), the name and extension apart.
fn matches(pattern: &str, name: &str) -> bool {
    fn part(p: &str, n: &str) -> bool {
        let mut n = n.chars();
        for c in p.chars() {
            match c {
                '*' => return true,
                '?' => {
                    n.next();
                }
                c => {
                    if n.next() != Some(c) {
                        return false;
                    }
                }
            }
        }
        n.next().is_none()
    }
    let split = |s: &'_ str| -> (String, String) {
        match s.rsplit_once('.') {
            Some((a, b)) => (a.into(), b.into()),
            None => (s.into(), String::new()),
        }
    };
    let ((pn, pe), (nn, ne)) = (split(pattern), split(name));
    part(&pn, &nn) && part(&pe, &ne)
}

/// A host name as DOS would list it: upper case, if it fits 8.3.
/// shortcut: longer names and hidden files are left out, where DOSBox shows long names
/// by an alias; upgrade if players keep long names in the save directory.
fn dos_name(name: &str) -> Option<String> {
    let (base, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let ok = name.is_ascii()
        && !name.starts_with('.')
        && (1..=8).contains(&base.len())
        && ext.len() <= 3
        && !base.contains('.');
    ok.then(|| name.to_ascii_uppercase())
}

/// The files matching `pattern` and the directories in `dir`, sorted (DOS gives them
/// in directory order), each list cut where the original's buffer ends.
fn list(dir: &Path, pattern: &str) -> [Vec<String>; 2] {
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(name) = entry.file_name().to_str().and_then(dos_name) else {
            continue;
        };
        match entry.file_type() {
            Ok(t) if t.is_dir() => dirs.push(name),
            Ok(_) if matches(pattern, &name) => files.push(name),
            _ => {}
        }
    }
    files.sort();
    dirs.sort();
    files.truncate(FILES_END.div_ceil(ENTRY));
    // shortcut: no drives; `.` and `..` as DOS lists them below a root.
    if dir.parent().is_some() {
        dirs.splice(0..0, [".".into(), "..".into()]);
    }
    let start = (files.len() + 1) * ENTRY;
    dirs.truncate(DIRECTORIES_END.saturating_sub(start).div_ceil(ENTRY));
    [files, dirs]
}

/// The entry in `dir` named `name` in any case, else `name` as it is.
fn host_name(dir: &Path, name: &str) -> PathBuf {
    let found = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
    found.map_or_else(|| dir.join(name), |e| e.path())
}

impl FileDialog {
    /// Opens a dialog in `dir`. `name` is the field the last dialog left (the
    /// original's one buffer); the open dialog sets it to its pattern first.
    pub fn open(kind: Kind, dir: PathBuf, name: Vec<u8>, tables: &FileDialogs) -> Self {
        let name = match kind {
            Kind::Open => tables.open_pattern.clone(),
            Kind::Save => name,
        };
        let mut dialog = FileDialog {
            kind,
            dir,
            name,
            lists: Default::default(),
            error: false,
            captured: None,
            repeat: None,
            drag: None,
            retraces: 0,
            dirty: true,
        };
        dialog.relist();
        dialog
    }

    /// The name field, for the next dialog to start from.
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// The game clock, from mode 13h's 70 Hz retraces as `crate::clock` derives it.
    fn clock(&self) -> u32 {
        self.retraces * 6 / 7
    }

    fn field(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    /// The lists built again from the field (`1fe6:850c`): no selections, at the top.
    fn relist(&mut self) {
        let [files, dirs] = list(&self.dir, &self.field());
        self.lists = [files, dirs].map(|names| Entries {
            names,
            ..Entries::default()
        });
        self.dirty = true;
    }

    /// Enter, OK or a double click (`ui_file_dialog_accept`).
    fn accept(&mut self) -> Option<Outcome> {
        let field = self.field();
        if let Some((path, rest)) = field.rsplit_once('\\') {
            match self.change_directory(path) {
                Some(dir) => {
                    self.dir = dir;
                    self.name = rest.as_bytes().to_vec();
                }
                None => {
                    self.error = true;
                    self.dirty = true;
                    return None;
                }
            }
        }
        let field = self.field();
        if field.is_empty() || field.contains(['*', '?']) {
            self.relist();
            return None;
        }
        Some(Outcome::Chosen(host_name(&self.dir, &field)))
    }

    /// The directory `path` names from the current one, if it exists.
    /// shortcut: drives and paths from a root are refused (the error box); upgrade if a
    /// player needs to leave the save directory's tree by name.
    fn change_directory(&self, path: &str) -> Option<PathBuf> {
        if path.contains(':') || path.is_empty() {
            return None;
        }
        let mut dir = self.dir.clone();
        for part in path.split('\\') {
            match part {
                "." => {}
                ".." => dir = dir.parent()?.to_path_buf(),
                name => dir = host_name(&dir, name),
            }
        }
        dir.is_dir().then_some(dir)
    }

    /// A key word going down (`ui_file_dialog_on_key`; with the error box up, any key
    /// closes it).
    pub fn key(&mut self, word: KeyWord) -> Option<Outcome> {
        if std::mem::take(&mut self.error) {
            self.dirty = true;
            return None;
        }
        match word {
            ESC => Some(Outcome::Cancelled),
            ENTER => self.accept(),
            BACKSPACE => {
                self.dirty |= self.name.pop().is_some();
                None
            }
            _ => None,
        }
    }

    /// A character typed: lower case made upper, appended while the field is at most
    /// 80 long.
    pub fn typed(&mut self, c: char) {
        if !self.error && (' '..='~').contains(&c) && self.name.len() < MAX_NAME {
            self.name.push(c.to_ascii_uppercase() as u8);
            self.dirty = true;
        }
    }

    fn part_at(&self, tables: &FileDialogs, at: (u16, u16)) -> Option<Part> {
        let dialog = self.dialog(tables);
        let parts = if self.error { &tables.error } else { dialog };
        parts
            .parts
            .iter()
            .rev()
            .find(|(_, r)| r.contains(at.0, at.1))
            .map(|&(p, _)| p)
    }

    fn dialog<'a>(&self, tables: &'a FileDialogs) -> &'a Dialog {
        match self.kind {
            Kind::Open => &tables.open,
            Kind::Save => &tables.save,
        }
    }

    fn track(&self, tables: &FileDialogs, list: List) -> Option<Rect> {
        let parts = &self.dialog(tables).parts;
        parts
            .iter()
            .find(|(p, _)| *p == Part::Track(list))
            .map(|&(_, r)| r)
    }

    /// Where the thumb's top is drawn: `top - 1 + 24 * first / (count - 1)`.
    fn thumb_top(&self, track: Rect, list: List) -> i32 {
        let e = &self.lists[index(list)];
        let step = match e.names.len() {
            0 | 1 => 0,
            n => THUMB_TRAVEL as usize * e.first / (n - 1),
        };
        i32::from(track.top) - 1 + step as i32
    }

    /// One step of an arrow: up to the first entry, down to the last.
    fn scroll(&mut self, list: List, down: bool) {
        let e = &mut self.lists[index(list)];
        if down && e.first + 1 < e.names.len() {
            e.first += 1;
        } else if !down && e.first > 0 {
            e.first -= 1;
        }
        self.dirty = true;
    }

    /// The row under `y` selected; above or below the list it scrolls one first.
    fn select(&mut self, list: List, y: u16) {
        let e = &mut self.lists[index(list)];
        let row = if y < FIRST_ROW {
            e.first = e.first.saturating_sub(1);
            0
        } else if y - FIRST_ROW >= ROW_HEIGHT * ROWS as u16 {
            if e.first + 1 < e.names.len() {
                e.first += 1;
            }
            ROWS - 1
        } else {
            usize::from((y - FIRST_ROW) / ROW_HEIGHT)
        };
        e.selected = (!e.names.is_empty()).then(|| (e.first + row).min(e.names.len() - 1));
        self.dirty = true;
    }

    fn drag_to(&mut self, list: List, y: i32) {
        let Some((_, press, thumb)) = self.drag else {
            return;
        };
        let e = &mut self.lists[index(list)];
        let Some(last) = e.names.len().checked_sub(1) else {
            return;
        };
        let at = ((y - press + thumb - DRAG_ORIGIN) * last as i32 / THUMB_TRAVEL).max(0);
        e.first = (at as usize).min(last);
        self.dirty = true;
    }

    /// The left button went down at `at`.
    pub fn mouse_down(&mut self, button: MouseButton, at: (u16, u16), tables: &FileDialogs) {
        if button != MouseButton::Left {
            return;
        }
        self.captured = self.part_at(tables, at);
        match self.captured {
            Some(part @ (Part::Up(_) | Part::Down(_))) => {
                self.repeat = Some((part, self.clock()));
                self.dirty = true;
            }
            Some(Part::List(list)) => self.select(list, at.1),
            Some(Part::Track(list)) => {
                self.drag = None;
                let Some(track) = self.track(tables, list) else {
                    return;
                };
                let (y, thumb) = (i32::from(at.1), self.thumb_top(track, list));
                let e = &mut self.lists[index(list)];
                if y < thumb {
                    e.first = e.first.saturating_sub(PAGE);
                } else if y - thumb > THUMB_HEIGHT {
                    e.first = match e.first + PAGE {
                        f if f <= e.names.len() => f,
                        _ => e.names.len().saturating_sub(1),
                    };
                } else {
                    self.drag = Some((list, y, thumb));
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// The pointer moved to `at`: a held list selects as it goes, a drag moves the
    /// thumb while the pointer is over its track.
    pub fn mouse_move(&mut self, at: (u16, u16), tables: &FileDialogs) {
        if let Some(Part::List(list)) = self.captured {
            self.select(list, at.1);
        }
        if let Some((list, ..)) = self.drag
            && (self.captured == Some(Part::Track(list))
                || self.part_at(tables, at) == Some(Part::Track(list)))
        {
            self.drag_to(list, i32::from(at.1));
        }
    }

    /// The left button came up at `at`.
    pub fn mouse_up(
        &mut self,
        button: MouseButton,
        at: (u16, u16),
        tables: &FileDialogs,
    ) -> Option<Outcome> {
        if button != MouseButton::Left {
            return None;
        }
        let captured = self.captured.take();
        self.repeat = None;
        self.dirty = true;
        if let Some(Part::Track(list)) = captured {
            self.drag_to(list, i32::from(at.1));
        }
        if captured != self.part_at(tables, at) {
            return None;
        }
        match captured? {
            Part::Ok => self.accept(),
            Part::Cancel => Some(Outcome::Cancelled),
            Part::ErrorOk(_) => {
                self.error = false;
                None
            }
            Part::Up(list) => {
                self.scroll(list, false);
                None
            }
            Part::Down(list) => {
                self.scroll(list, true);
                None
            }
            Part::List(list) => self.release_on(list, tables),
            Part::Track(_) => None,
        }
    }

    /// A release on a list: its entry into the field, and a second soon enough accepts.
    fn release_on(&mut self, list: List, tables: &FileDialogs) -> Option<Outcome> {
        let now = self.clock();
        let i = index(list);
        let e = &mut self.lists[i];
        let selected = e.selected?;
        let quick = e
            .released
            .is_some_and(|t| now.wrapping_sub(t) <= DOUBLE_CLICK[i]);
        e.released = Some(now);
        let name = e.names[selected].clone();
        self.lists[1 - i].selected = None;
        self.name = match list {
            List::Files => name.into_bytes(),
            List::Directories => [name.as_bytes(), b"\\", &tables.directory_pattern].concat(),
        };
        if quick { self.accept() } else { None }
    }

    /// One retrace: a held arrow repeats every 4 counts after 30.
    pub fn retrace(&mut self) {
        self.retraces += 1;
        let now = self.clock();
        while let Some((part, since)) = self.repeat
            && now.wrapping_sub(since) > REPEAT_DELAY
        {
            self.repeat = Some((part, since + REPEAT_EVERY));
            if let Part::Up(list) | Part::Down(list) = part {
                self.scroll(list, matches!(part, Part::Down(_)));
            }
        }
    }

    /// The directory line: `C:` and the directory, cut to `..\` and its end past 19
    /// characters, then spaces to 20.
    fn directory_line(&self) -> Vec<u8> {
        // shortcut: the host path's parts stand for DOS's directory, on drive C.
        let parts: Vec<String> = self
            .dir
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().to_uppercase()),
                _ => None,
            })
            .collect();
        let path = parts.join("\\");
        let mut line = b"C:".to_vec();
        if path.len() < DIRECTORY_WIDTH {
            line.extend(path.as_bytes());
            line.resize(2 + DIRECTORY_WIDTH - 1, b' ');
        } else {
            let cut = path
                .match_indices('\\')
                .map(|(i, _)| &path[i + 1..])
                .find(|rest| rest.len() <= 17)
                .unwrap_or(&path[path.len() - 17..]);
            let shown = format!("..\\{cut}");
            line.extend(shown.as_bytes());
            line.resize(2 + DIRECTORY_WIDTH, b' ');
        }
        line
    }

    /// Whether anything changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Draws the dialog (and the error box over it): the script, the field, the
    /// directory line, the scroll bars and the lists.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        tables: &FileDialogs,
        fonts: &BTreeMap<u16, Font>,
        sprites: &[Sprite],
    ) {
        let mut current = 0;
        run_script(fb, &self.dialog(tables).script, fonts, &mut current);
        let Some(text) = fonts.get(&FONT) else {
            return;
        };
        current = FONT;
        let opaque = |fb: &mut Framebuffer, x, y, bytes: &[u8]| {
            let mut cursor = TextCursor::at(x, y, INK);
            cursor.background = PAPER;
            fb.draw_text(text, &mut cursor, bytes, TextMode::Opaque);
            cursor
        };
        let field = tables.name_field;
        fill(fb, field, PAPER);
        let shown = &self.name[self.name.len().saturating_sub(SHOWN)..];
        let mut cursor = opaque(fb, field.left.into(), field.top.into(), shown);
        fb.draw_char(text, &mut cursor, 0x7f, TextMode::Transparent);
        opaque(fb, DIRECTORY_AT.0, DIRECTORY_AT.1, &self.directory_line());

        let sprite = |fb: &mut Framebuffer, handle: u16, x: i32, y: i32| {
            if let Some(s) = sprites.get(usize::from(handle / 8)) {
                let blit = SpriteBlit {
                    mode: BlitMode::Opaque,
                    ..SpriteBlit::default()
                };
                fb.blit_sprite(s, x, y, blit);
            }
        };
        for &(part, area) in &self.dialog(tables).parts {
            let pressed = if self.captured == Some(part) {
                PRESSED
            } else {
                0
            };
            let corner = (i32::from(area.left), i32::from(area.top));
            match part {
                Part::Up(_) => sprite(fb, UP_SPRITE + pressed, corner.0, corner.1),
                Part::Down(_) => sprite(fb, DOWN_SPRITE + pressed, corner.0, corner.1),
                Part::Track(list) => {
                    fill(fb, area, TRACK);
                    let top = self.thumb_top(area, list);
                    sprite(fb, THUMB_SPRITE + pressed, corner.0, top);
                }
                Part::List(list) => self.draw_list(fb, text, list, area.left),
                _ => {}
            }
        }
        if self.error {
            run_script(fb, &tables.error.script, fonts, &mut current);
            for &(part, area) in &tables.error.parts {
                if let Part::ErrorOk(handle) = part {
                    let pressed = if self.captured == Some(part) {
                        PRESSED
                    } else {
                        0
                    };
                    sprite(fb, handle + pressed, area.left.into(), area.top.into());
                }
            }
        }
    }

    fn draw_list(&self, fb: &mut Framebuffer, font: &Font, list: List, x: u16) {
        let e = &self.lists[index(list)];
        let (left, right) = match list {
            List::Files => (x - 2, FILES_RIGHT),
            List::Directories => (x - 2, x - 2 + DIRECTORIES_WIDTH),
        };
        let mut cursor = TextCursor::at(x.into(), FIRST_ROW.into(), INK);
        cursor.background = PAPER;
        let rows = e.names.iter().skip(e.first).take(ROWS);
        let mut drawn = 0;
        for name in rows {
            let cell = format!("{name:<CELLS$}");
            fb.draw_text(font, &mut cursor, cell.as_bytes(), TextMode::Opaque);
            fb.draw_char(font, &mut cursor, 0x0c, TextMode::Transparent);
            drawn += 1;
        }
        if drawn < ROWS as u16 {
            let top = FIRST_ROW + ROW_HEIGHT * drawn;
            fill(
                fb,
                Rect {
                    top,
                    left,
                    bottom: LIST_BOTTOM,
                    right,
                },
                PAPER,
            );
        }
        if let Some(row) = e.selected.and_then(|s| s.checked_sub(e.first))
            && row < ROWS
        {
            let top = FIRST_ROW - 1 + ROW_HEIGHT * row as u16;
            xor_fill(
                fb,
                Rect {
                    top,
                    left,
                    bottom: top + 7,
                    right,
                },
                PAPER,
            );
        }
    }
}

/// A dialog's drawing script (`ui_draw_file_dialog`); text in colour 0, transparent,
/// in the font last selected (`current`).
fn run_script(fb: &mut Framebuffer, script: &[Op], fonts: &BTreeMap<u16, Font>, current: &mut u16) {
    for op in script {
        match op {
            Op::Fill { colour, area } => fill(fb, *area, *colour),
            Op::Frame(area) => frame(fb, *area, 0),
            Op::Font(id) => *current = *id,
            Op::Text { x, y, text, .. } => {
                if let Some(f) = fonts.get(current) {
                    let mut cursor = TextCursor::at((*x).into(), (*y).into(), INK);
                    fb.draw_text(f, &mut cursor, text, TextMode::Transparent);
                }
            }
        }
    }
}

/// The synthetic dialogs' tables (`xeno_formats::fixtures::synthetic_file_dialogs`).
#[cfg(test)]
pub fn test_tables() -> FileDialogs {
    const DS: usize = 0x19010;
    let mut image = vec![0u8; DS + 0x10000];
    let relocated = xeno_formats::fixtures::synthetic_file_dialogs(&mut image, DS);
    xeno_formats::filedialog::read_image(&image, DS, &relocated).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_render::Size;

    fn tables() -> FileDialogs {
        test_tables()
    }

    /// A fresh directory holding `files` and the directory `SUB`.
    fn dir(test: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xeno-dialog-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SUB")).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        dir
    }

    fn type_in(d: &mut FileDialog, text: &str) {
        text.chars().for_each(|c| d.typed(c));
    }

    #[test]
    fn dos_patterns_match_name_and_extension_apart() {
        assert!(matches("*.USG", "SCEN00A.USG"));
        assert!(!matches("*.USG", "SCEN00A.MAP"));
        assert!(matches("*.*", "README"));
        assert!(!matches("*", "A.USG"));
        // `?` takes one character, or none at the end.
        assert!(matches("SCEN0??.*", "SCEN0A.USG"));
        assert!(!matches("SCEN?A.USG", "SCEN10A.USG"));
        assert_eq!(dos_name("scen00a.usg").as_deref(), Some("SCEN00A.USG"));
        assert_eq!(dos_name("a long name.usg"), None);
        assert_eq!(dos_name(".hidden"), None);
    }

    #[test]
    fn the_open_dialog_lists_its_pattern_and_returns_the_file_found() {
        let tables = tables();
        let dir = dir("open", &["b.usg", "A.USG", "c.map"]);
        let mut d = FileDialog::open(Kind::Open, dir.clone(), b"OLD".to_vec(), &tables);
        assert_eq!(d.name(), b"*.USG");
        assert_eq!(d.lists[0].names, ["A.USG", "B.USG"]);
        assert_eq!(d.lists[1].names, [".", "..", "SUB"]);
        // Enter on a pattern lists again; on a name, the host's file in any case.
        d.name.clear();
        type_in(&mut d, "*.map");
        assert_eq!(d.key(ENTER), None);
        assert_eq!(d.lists[0].names, ["C.MAP"]);
        d.key(BACKSPACE);
        d.name.clear();
        type_in(&mut d, "b.usg");
        assert_eq!(d.key(ENTER), Some(Outcome::Chosen(dir.join("b.usg"))));
        // A new name comes back as typed, upper case, no extension added.
        d.name.clear();
        type_in(&mut d, "new");
        assert_eq!(d.key(ENTER), Some(Outcome::Chosen(dir.join("NEW"))));
        assert_eq!(d.key(ESC), Some(Outcome::Cancelled));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_save_dialog_keeps_the_field_and_takes_eighty_characters() {
        let tables = tables();
        let dir = dir("save", &[]);
        let mut d = FileDialog::open(Kind::Save, dir.clone(), b"OLD".to_vec(), &tables);
        assert_eq!(d.name(), b"OLD");
        type_in(&mut d, &"x".repeat(100));
        assert_eq!(d.name().len(), MAX_NAME);
        d.typed('é');
        assert_eq!(d.name().len(), MAX_NAME);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_path_changes_directory_or_shows_the_error_box() {
        let tables = tables();
        let dir = dir("path", &[]);
        let mut d = FileDialog::open(Kind::Save, dir.clone(), Vec::new(), &tables);
        type_in(&mut d, "sub\\a.usg");
        let chosen = dir.join("SUB").join("A.USG");
        assert_eq!(d.key(ENTER), Some(Outcome::Chosen(chosen)));
        assert_eq!(d.name(), b"A.USG");

        d.name.clear();
        type_in(&mut d, "nope\\a.usg");
        assert_eq!(d.key(ENTER), None);
        assert!(d.error);
        // Any key closes the box; ESC does not cancel the dialog then.
        assert_eq!(d.key(ESC), None);
        assert!(!d.error);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clicks_select_scroll_and_a_second_click_accepts() {
        let tables = tables();
        let names: Vec<String> = (0..12).map(|i| format!("F{i:02}.USG")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let dir = dir("mouse", &refs);
        let mut d = FileDialog::open(Kind::Open, dir.clone(), Vec::new(), &tables);
        let left = MouseButton::Left;
        // The files list is at (81, 78)..(135, 129): its third row.
        d.mouse_down(left, (90, 93), &tables);
        assert_eq!(d.lists[0].selected, Some(2));
        assert_eq!(d.mouse_up(left, (90, 93), &tables), None);
        assert_eq!(d.name(), b"F02.USG");
        // A drag below the list scrolls one and picks the bottom row.
        d.mouse_down(left, (90, 93), &tables);
        d.mouse_move((90, 129), &tables);
        assert_eq!((d.lists[0].first, d.lists[0].selected), (1, Some(8)));
        for _ in 0..40 {
            d.retrace();
        }
        d.mouse_up(left, (90, 300), &tables);
        // Two releases within 30 counts accept; 31 apart (37 retraces) they do not.
        d.mouse_down(left, (90, 81), &tables);
        d.mouse_up(left, (90, 81), &tables);
        (0..37).for_each(|_| d.retrace());
        d.mouse_down(left, (90, 81), &tables);
        assert_eq!(d.mouse_up(left, (90, 81), &tables), None);
        d.mouse_down(left, (90, 81), &tables);
        let chosen = d.mouse_up(left, (90, 81), &tables);
        assert_eq!(chosen, Some(Outcome::Chosen(dir.join("F01.USG"))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn arrows_step_and_repeat_and_the_track_pages() {
        let tables = tables();
        let dir = dir("scroll", &[]);
        for i in 0..20 {
            std::fs::create_dir(dir.join(format!("D{i:02}"))).unwrap();
        }
        let mut d = FileDialog::open(Kind::Save, dir.clone(), Vec::new(), &tables);
        let left = MouseButton::Left;
        // The save dialog's directories: up arrow (135, 78), track (135, 87)..(144, 119).
        d.lists[1].first = 3;
        d.mouse_down(left, (140, 80), &tables);
        assert_eq!(d.mouse_up(left, (140, 80), &tables), None);
        assert_eq!(d.lists[1].first, 2);
        // Held: the first repeat after 30 counts (35 retraces at 70 Hz), then every 4.
        d.mouse_down(left, (140, 80), &tables);
        (0..37).for_each(|_| d.retrace());
        assert_eq!(d.lists[1].first, 1);
        // The down arrow stops at the last entry (the 23rd: `.`, `..`, `SUB`, 20 more).
        d.lists[1].first = 22;
        d.scroll(List::Directories, true);
        assert_eq!(d.lists[1].first, 22);
        // Below the thumb (at row 86 for the first of 23): a page of 7 down.
        d.mouse_up(left, (0, 0), &tables);
        d.lists[1].first = 0;
        d.mouse_down(left, (140, 110), &tables);
        assert_eq!(d.lists[1].first, 7);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_directory_line_cuts_long_paths() {
        let tables = tables();
        let mut d = FileDialog::open(Kind::Save, PathBuf::from("/xeno"), Vec::new(), &tables);
        assert_eq!(d.directory_line(), b"C:XENO               ");
        d.dir = PathBuf::from("/users/someone/games/xenobots/saves");
        assert_eq!(d.directory_line(), b"C:..\\XENOBOTS\\SAVES   ");
    }

    #[test]
    fn the_dialog_draws_its_script_field_and_highlight() {
        let tables = tables();
        let fonts =
            xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block()).unwrap();
        let fonts: BTreeMap<u16, Font> = [(0, fonts[0].clone()), (2, fonts[1].clone())].into();
        let dir = dir("draw", &["A.USG"]);
        let mut d = FileDialog::open(Kind::Open, dir.clone(), Vec::new(), &tables);
        d.lists[0].selected = Some(0);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(9);
        d.draw(&mut fb, &tables, &fonts, &[]);
        // The box's fill, the field's, the selected row XORed, and outside untouched.
        assert_eq!(fb.get(100, 30), Some(12));
        assert_eq!(fb.get(190, 45), Some(PAPER));
        assert_eq!(fb.get(80, 80), Some(PAPER ^ PAPER));
        assert_eq!(fb.get(80, 87), Some(PAPER));
        assert_eq!(fb.get(10, 10), Some(9));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
