//! The original's scenario editor (`docs/screens.md`, "The scenario editor (T-048)"):
//! with unlock word 9 its Ctrl keys work in the situation room, on the mission's
//! scenario. The port keeps an editable copy of the scenario the mission loaded and
//! applies the keys that need neither the map view (placing, T-108) nor a file
//! dialog (T-110) to it; its saves go through `xeno_formats::usg::write` and
//! `map::write`. The status the original prints on its monochrome debug screen is
//! logged, built from the user's own template and names.

use xeno_formats::editor::Editor as Tables;
use xeno_formats::usg::{ObjectKind, PathTable, Scenario};

/// The handlers in the editor's key table (`DS:0x5bc3`), by their offset in segment
/// `1000`, which is how the port knows them.
mod handler {
    /// Ctrl+H, N, S, B, R, O, P: the kind to place.
    pub const KINDS: [(u16, i16); 7] = [
        (0x6ad8, 0),
        (0x6adf, 1),
        (0x6aed, 2),
        (0x6b08, 3),
        (0x6af4, 4),
        (0x6ae6, 16),
        (0x6afb, -1),
    ];
    pub const SIDE: u16 = 0x6b02;
    pub const HEADING_DOWN: u16 = 0x6bb3;
    pub const HEADING_UP: u16 = 0x6bba;
    pub const HEADING_ZERO: u16 = 0x6bc1;
    pub const HEADING_HALF: u16 = 0x6bc8;
    pub const NEW_PATH: u16 = 0x6b0f;
    pub const REMOVE_PATH: u16 = 0x6b31;
    pub const NEXT_PATH: u16 = 0x6b75;
    pub const PREVIOUS_PATH: u16 = 0x6b84;
    pub const NEXT_MISSION: u16 = 0x6b90;
    pub const DROP_POINT: u16 = 0x6ba6;
    pub const SHOW_PATHS: u16 = 0x6bdc;
    pub const REVEAL: u16 = 0x6be2;
    pub const REMOVE_RELAYS: u16 = 0x6ac8;
    pub const TERRAIN: u16 = 0x448c;
    pub const TOGGLE: u16 = 0x6bcf;
    /// Ctrl+D, Ctrl+C, Ctrl+Alt+O and B, Ctrl+Alt+arrows: not in the port yet.
    pub const LATER: [(u16, &str); 8] = [
        (0x6ab7, "destroys the roster's robot (T-108)"),
        (0x6901, "clears the base's messages (T-103)"),
        (0x6d67, "loads the orders from a file (T-110)"),
        (0x6dc3, "picks the sky from a file (T-110)"),
        (0x6dde, "scrolls the map (T-108)"),
        (0x6de4, "scrolls the map (T-108)"),
        (0x6dea, "scrolls the map (T-108)"),
        (0x6df0, "scrolls the map (T-108)"),
    ];
}

/// A heading step: a degree, 65,536 to a turn.
const DEGREE: u16 = 0xb6;

/// What a key asks the mission to do besides changing the scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Ctrl+M: the terrain editor, which the port runs as entering and leaving at
    /// once (Alt+Q), saving `BACKUP.MAP` (T-109 for the editor itself).
    SaveTerrain,
}

/// The editor's state (`DS:0x5c3f`..`0x5c47`), as `ui_reset_editor_state` leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Editor {
    /// The kind to place: 0 to 4 and 16, -1 for path points.
    kind: i16,
    alien: bool,
    heading: u16,
    path: usize,
    show_paths: bool,
    reveal: bool,
    flag: bool,
}

fn table(scenario: &mut Scenario, alien: bool) -> &mut PathTable {
    let table = if alien {
        &mut scenario.alien_paths
    } else {
        &mut scenario.human_paths
    };
    table.get_or_insert_with(PathTable::empty)
}

impl Editor {
    /// Runs the key's `handler` on `scenario`.
    pub fn key(&mut self, handler: u16, scenario: &mut Scenario, tables: &Tables) -> Effect {
        let alien = self.alien;
        let path = self.path;
        match handler {
            h if handler::KINDS.iter().any(|&(k, _)| k == h) => {
                self.kind = handler::KINDS
                    .iter()
                    .find(|&&(k, _)| k == h)
                    .map_or(0, |k| k.1);
            }
            handler::SIDE => self.alien = !self.alien,
            handler::HEADING_DOWN => self.heading = self.heading.wrapping_sub(DEGREE),
            handler::HEADING_UP => self.heading = self.heading.wrapping_add(DEGREE),
            handler::HEADING_ZERO => self.heading = 0,
            handler::HEADING_HALF => self.heading = 0x8000,
            handler::NEW_PATH => {
                let t = table(scenario, alien);
                if usize::from(t.count) < PathTable::SLOTS {
                    self.path = usize::from(t.count);
                    t.count += 1;
                    let p = &mut t.paths[self.path];
                    (p.point_count, p.mission) = (0, 0);
                    set_defaults(p, tables);
                }
            }
            handler::REMOVE_PATH => {
                let t = table(scenario, alien);
                if t.count > 0 {
                    let old = usize::from(t.count);
                    t.count -= 1;
                    if path == usize::from(t.count) {
                        self.path = path.saturating_sub(1);
                    } else {
                        // The later paths move down a slot (the original copies one
                        // slot past the count too).
                        for i in path..old.min(PathTable::SLOTS - 1) {
                            t.paths[i] = t.paths[i + 1];
                        }
                    }
                }
            }
            // The original compares with the alien table's count whichever side.
            handler::NEXT_PATH if path + 1 < usize::from(table(scenario, true).count) => {
                self.path += 1;
            }
            handler::PREVIOUS_PATH => self.path = path.saturating_sub(1),
            handler::NEXT_MISSION | handler::DROP_POINT => {
                let t = table(scenario, alien);
                if t.count > 0 {
                    let p = &mut t.paths[path.min(PathTable::SLOTS - 1)];
                    if handler == handler::DROP_POINT {
                        p.point_count = p.point_count.saturating_sub(1);
                    } else {
                        p.mission = (p.mission + 1) % xeno_formats::editor::MISSIONS as u8;
                        set_defaults(p, tables);
                    }
                }
            }
            handler::SHOW_PATHS => self.show_paths = !self.show_paths,
            handler::REVEAL => self.reveal = !self.reveal,
            handler::REMOVE_RELAYS => {
                let objects = scenario.objects.get_or_insert_with(Vec::new);
                objects.retain(|o| o.kind != ObjectKind::Relay);
            }
            handler::TERRAIN => return Effect::SaveTerrain,
            handler::TOGGLE => {
                self.flag = !self.flag;
                log::info!("editor: Alt+F1 flag {} (nothing reads it)", self.flag);
            }
            h => match handler::LATER.iter().find(|&&(l, _)| l == h) {
                Some((_, what)) => log::info!("editor: this key {what}, not in the port yet"),
                None => log::debug!("editor: handler {h:#06x} not known"),
            },
        }
        Effect::None
    }

    /// The four status rows the original prints from row 20 of its debug screen
    /// (`ui_draw_editor_status`), with the user's template and names.
    pub fn status(&self, scenario: &Scenario, tables: &Tables) -> Vec<String> {
        let mut rows = vec![vec![b' '; 80]; 4];
        let mut put = |row: usize, col: usize, text: &[u8]| {
            for (i, &b) in text.iter().enumerate() {
                if let Some(cell) = rows[row].get_mut(col + i) {
                    *cell = b;
                }
            }
        };
        for (row, line) in tables.status.split(|&b| b == b'\r').take(4).enumerate() {
            put(row, 1, line);
        }
        let number = |n: u16| format!("{n:05}").into_bytes()[2..].to_vec();
        let kind = usize::try_from(self.kind + 1).unwrap_or(0);
        put(
            0,
            6,
            tables
                .kind_names
                .get(kind)
                .cloned()
                .flatten()
                .as_deref()
                .unwrap_or_default(),
        );
        put(1, 6, &tables.side_names[usize::from(self.alien)]);
        put(2, 6, &number(degrees(self.heading)));
        put(3, 7, &tables.show_names[usize::from(self.show_paths)]);
        // The path fields show the alien table whichever side is picked.
        let alien = scenario.alien_paths.as_ref();
        let count = alien.map_or(0, |t| t.count);
        put(0, 20, &number(count));
        if let Some(p) = alien
            .filter(|_| count > 0)
            .map(|t| t.paths[self.path.min(7)])
        {
            put(1, 20, &number(self.path as u16));
            put(2, 20, &number(p.point_count.into()));
            let mission = tables.mission_names.get(usize::from(p.mission));
            put(3, 13, mission.map_or(&[][..], Vec::as_slice));
            put(0, 33, &number(p.weight.into()));
            let percent = |tenths: u8| tables.mix_percent.get(usize::from(tenths)).copied();
            for (row, &mix) in p.mix.iter().enumerate() {
                put(row + 1, 33, &number(percent(mix).unwrap_or(0).into()));
            }
            put(0, 42, &number(p.on_path.into()));
            for (row, &n) in p.on_path_by_kind.iter().enumerate() {
                put(row + 1, 42, &number(n.into()));
            }
        }
        rows.iter()
            .map(|r| {
                r.iter()
                    .map(|&b| cp437(b))
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }
}

/// A new path's or mission's weight and mix (`1000:6ce3`).
fn set_defaults(p: &mut xeno_formats::usg::Path, tables: &Tables) {
    if let Some(d) = tables.mission_defaults.get(usize::from(p.mission)) {
        p.weight = d[0];
        p.mix = [d[1], d[2], d[3]];
    }
}

/// A heading in whole degrees as the status shows it (`1000:0320`): the heading less
/// a 4096th of itself, divided by 0xb6 and rounded, 360 shown as 0.
pub fn degrees(heading: u16) -> u16 {
    let a = u32::from(heading) - (u32::from(heading) >> 12);
    let d = a / u32::from(DEGREE) + u32::from(a % u32::from(DEGREE) > u32::from(DEGREE / 2));
    if d >= 360 { 0 } else { d as u16 }
}

/// The debug screen's characters: ASCII, and the template's line and degree signs.
fn cp437(b: u8) -> char {
    match b {
        0xb3 => '│',
        0xf8 => '°',
        0x20..0x7f => char::from(b),
        _ => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::synthetic_usg;
    use xeno_formats::usg;

    fn tables() -> Tables {
        let name = |s: &str| s.as_bytes().to_vec();
        Tables {
            keys: Vec::new(),
            kind_names: (0..18)
                .map(|i| (i < 2).then(|| name(["PTH", "HUM"][i])))
                .collect(),
            side_names: [name("HUM"), name("ALI")],
            show_names: [name("NO"), name("YES")],
            mission_names: (0..8).map(|i| name(&format!("M{i}"))).collect(),
            mission_defaults: (0..7).map(|i| [i, 1, 2, 7]).collect(),
            mix_percent: (0..11).map(|i| i * 10).collect(),
            status: name("KIND:\rSIDE:\rHEAD:\rSHOW:\r"),
        }
    }

    #[test]
    fn paths_are_made_cycled_and_removed() {
        let mut s = usg::parse(&synthetic_usg()).unwrap();
        let t = tables();
        let mut e = Editor::default();
        e.key(handler::SIDE, &mut s, &t);
        assert!(e.alien);
        let before = s.alien_paths.as_ref().unwrap().count;
        e.key(handler::NEW_PATH, &mut s, &t);
        let alien = s.alien_paths.as_ref().unwrap();
        assert_eq!(alien.count, before + 1);
        let new = alien.paths[e.path];
        assert_eq!(
            (new.point_count, new.mission, new.weight, new.mix),
            (0, 0, 0, [1, 2, 7])
        );
        e.key(handler::NEXT_MISSION, &mut s, &t);
        e.key(handler::NEXT_MISSION, &mut s, &t);
        let p = s.alien_paths.as_ref().unwrap().paths[e.path];
        assert_eq!((p.mission, p.weight), (2, 2));
        // Removing the last path makes the one before it current.
        e.key(handler::REMOVE_PATH, &mut s, &t);
        assert_eq!((s.alien_paths.as_ref().unwrap().count, e.path), (before, 0));
        // The human side had no paths; a new one goes in slot 0.
        e.key(handler::SIDE, &mut s, &t);
        e.key(handler::NEW_PATH, &mut s, &t);
        assert_eq!(s.human_paths.as_ref().unwrap().count, 1);
    }

    #[test]
    fn the_status_shows_the_editor_state() {
        let mut s = usg::parse(&synthetic_usg()).unwrap();
        let t = tables();
        let mut e = Editor::default();
        e.key(handler::KINDS[0].0, &mut s, &t);
        e.key(handler::HEADING_UP, &mut s, &t);
        e.key(handler::SHOW_PATHS, &mut s, &t);
        let rows = e.status(&s, &t);
        assert!(rows[0].starts_with(" KIND:HUM"), "{rows:?}");
        assert!(rows[2].starts_with(" HEAD:001"), "{rows:?}");
        assert!(rows[3].starts_with(" SHOW: YES"), "{rows:?}");
        // The synthetic scenario has one alien path, a RECON (3) with mix 0, 3, 7.
        assert!(rows[0].contains("001"), "{rows:?}");
        assert!(rows[3].contains("M3"), "{rows:?}");
        assert!(rows[3].contains("070"), "{rows:?}");
    }

    #[test]
    fn headings_turn_into_degrees_as_the_original_rounds() {
        assert_eq!(degrees(0), 0);
        assert_eq!(degrees(DEGREE), 1);
        assert_eq!(degrees(0x4000), 90);
        assert_eq!(degrees(0x8000), 180);
        assert_eq!(degrees(0xffff), 0);
    }

    #[test]
    fn relays_are_removed() {
        let mut s = usg::parse(&synthetic_usg()).unwrap();
        Editor::default().key(handler::REMOVE_RELAYS, &mut s, &tables());
        let objects = s.objects.unwrap();
        assert!(objects.iter().all(|o| o.kind != ObjectKind::Relay) && objects.len() == 1);
    }
}
