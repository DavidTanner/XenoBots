//! The keyboard "stick" `config.dvr` lets each player set up: up-left, up, up-right,
//! left, right and fire (`docs/formats/key.md`, "Key tables"). The defaults are read
//! from the user's `config.dvr`; a `XENO.CFG` in the game directory, which `config.dvr`
//! writes when it saves its settings, replaces them.
//!
//! The original game never reads these keys (its own keys are fixed tables), so nothing
//! plays with them yet; the test card colours them. With `xeno-sim` (T-050) this moves
//! next to `input`.

use std::io;

use xeno_formats::setup::{self, KeyTable, NO_KEY, Slot};

use crate::game_dir::GameFiles;
use crate::input::ScanCode;

/// Where a [`KeyMap`]'s keys came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The defaults in `config.dvr`.
    ConfigDvr,
    /// The user's `XENO.CFG`.
    SavedConfig,
    /// Neither could be read: no keys.
    Nothing,
}

/// Each player's stick keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMap {
    tables: [KeyTable; 2],
    pub source: Source,
}

impl KeyMap {
    /// A map with no keys.
    pub fn empty() -> KeyMap {
        let table = KeyTable {
            slots: [NO_KEY; setup::KEY_SLOTS],
            unknown_0c: [0; 4],
        };
        KeyMap {
            tables: [table; 2],
            source: Source::Nothing,
        }
    }

    /// The keys from the game directory: `XENO.CFG` if it is there and valid, else
    /// `config.dvr`'s defaults. Problems are logged, never fatal, since nothing needs
    /// the keys yet.
    pub fn load(files: &GameFiles) -> KeyMap {
        KeyMap::from_files(files.read("xeno.cfg"), files.read("config.dvr"))
    }

    pub(crate) fn from_files(
        xeno_cfg: io::Result<Vec<u8>>,
        config_dvr: io::Result<Vec<u8>>,
    ) -> KeyMap {
        match xeno_cfg.map(|bytes| setup::parse_saved_config(&bytes)) {
            Ok(Ok(saved)) => {
                return KeyMap {
                    tables: saved.keys,
                    source: Source::SavedConfig,
                };
            }
            Ok(Err(e)) => log::warn!("XENO.CFG: {e}; using config.dvr's keys"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read XENO.CFG: {e}; using config.dvr's keys"),
        }
        let defaults = config_dvr
            .map_err(|e| e.to_string())
            .and_then(|bytes| setup::default_keys(&bytes).map_err(|e| e.to_string()));
        match defaults {
            Ok(tables) => KeyMap {
                tables,
                source: Source::ConfigDvr,
            },
            Err(e) => {
                log::warn!("config.dvr: cannot read its default keys: {e}");
                KeyMap::empty()
            }
        }
    }

    /// The players (0 or 1) and slots `code` drives. A key sent with an `E0` prefix
    /// counts as its plain twin, as `config.dvr` drops the prefix when it records keys.
    pub fn slots(&self, code: ScanCode) -> impl Iterator<Item = (usize, Slot)> + '_ {
        let plain = match code.0 >> 8 {
            0 | 0xe0 => Some((code.0 & 0xff) as u8),
            _ => None,
        };
        (0..2).flat_map(move |player| {
            Slot::ALL
                .into_iter()
                .filter(move |&slot| plain.is_some() && self.tables[player].code(slot) == plain)
                .map(move |slot| (player, slot))
        })
    }

    /// Each player's keys, for the log.
    pub fn describe(&self) -> String {
        let player = |t: &KeyTable| {
            Slot::ALL
                .iter()
                .map(|&s| match t.code(s) {
                    Some(c) => format!("{s:?} {c:#04x}"),
                    None => format!("{s:?} none"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "player 1: {}; player 2: {}",
            player(&self.tables[0]),
            player(&self.tables[1])
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::synthetic_config_dvr;

    /// Made-up keys: player 1 uses codes 0x10.., player 2 0x20.. in the six slots.
    fn tables() -> [[u8; setup::KEY_SLOTS]; 2] {
        [0x10, 0x20].map(|first| {
            let mut slots = [NO_KEY; setup::KEY_SLOTS];
            for (i, slot) in Slot::ALL.iter().enumerate() {
                slots[*slot as usize] = first + i as u8;
            }
            slots
        })
    }

    fn not_found() -> io::Result<Vec<u8>> {
        Err(io::ErrorKind::NotFound.into())
    }

    #[test]
    fn defaults_come_from_config_dvr() {
        let map = KeyMap::from_files(not_found(), Ok(synthetic_config_dvr(&tables())));
        assert_eq!(map.source, Source::ConfigDvr);
        assert_eq!(
            map.slots(ScanCode(0x11)).collect::<Vec<_>>(),
            [(0, Slot::Up)]
        );
        assert_eq!(
            map.slots(ScanCode(0x25)).collect::<Vec<_>>(),
            [(1, Slot::Fire)]
        );
        // E0 keys count as their plain twin; unused slots never match.
        assert_eq!(
            map.slots(ScanCode(0xe013)).collect::<Vec<_>>(),
            [(0, Slot::Left)]
        );
        assert_eq!(map.slots(ScanCode(NO_KEY.into())).count(), 0);
        assert_eq!(map.slots(ScanCode(0x1013)).count(), 0);
        assert!(map.describe().starts_with("player 1: UpLeft 0x10, Up 0x11"));
    }

    #[test]
    fn a_saved_config_replaces_them() {
        let mut cfg = b"2032VAC2K!  K   ".to_vec();
        let mut keys = tables();
        keys[0][Slot::Fire as usize] = 0x39;
        keys[1][Slot::Fire as usize] = 0x39;
        for t in keys {
            cfg.extend(t);
            cfg.extend([0; 4]);
        }
        let map = KeyMap::from_files(Ok(cfg), Ok(synthetic_config_dvr(&tables())));
        assert_eq!(map.source, Source::SavedConfig);
        // Both players may share a key if the user accepted the warning.
        assert_eq!(
            map.slots(ScanCode(0x39)).collect::<Vec<_>>(),
            [(0, Slot::Fire), (1, Slot::Fire)]
        );
        // A file of the wrong size is ignored, as config.dvr ignores it.
        let map = KeyMap::from_files(Ok(vec![0; 47]), Ok(synthetic_config_dvr(&tables())));
        assert_eq!(map.source, Source::ConfigDvr);
    }

    #[test]
    fn unreadable_defaults_leave_no_keys() {
        let map = KeyMap::from_files(not_found(), Ok(b"MZ".to_vec()));
        assert_eq!(map, KeyMap::empty());
        assert_eq!(map.slots(ScanCode(0x11)).count(), 0);
        assert_eq!(
            KeyMap::from_files(not_found(), not_found()),
            KeyMap::empty()
        );
    }
}
