//! `scendef.bin`: the campaign's scenario table. Each record places one scenario on the
//! map room's world map as a clickable box, says when it becomes available, and names
//! the place. The training missions are not in it. See `docs/formats/scendef.md`.

use binrw::{BinRead, NullString, binrw};

use crate::FormatError;

/// A scenario's clickable box on the 320x200 map room screen, in pixels, edges
/// inclusive (the game's hit test accepts `top..=bottom` and `left..=right`).
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapBox {
    pub top: u16,
    pub left: u16,
    pub bottom: u16,
    pub right: u16,
}

/// One record of the table.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    /// Twice the record's position in the table. The game uses it as a byte offset into
    /// its table of record pointers, so it must match the position; see
    /// [`Scenario::index`].
    pub id: u16,
    pub map_box: MapBox,
    /// The scenario can be picked once the player's progress value (`xeno.exe`
    /// `DS:0x5f9b`) is at least this; until then the map shows it as unavailable.
    /// 1, 2, 3, 4 or 6 in a stock install.
    pub unlock_level: u16,
    /// Per-scenario progress the game keeps while playing: it writes 0, 8, 0xa and 0xc
    /// here, picks the box's look from it, and refuses to select a scenario at 0xa. The
    /// whole table goes into save games, so this is how progress is stored. 0 in the
    /// file for every record.
    pub state: u16,
    /// The place name, in the game's text encoding: ASCII with
    /// [`Scenario::LINE_BREAK`] ending each line. Every stock name is one line.
    pub name: NullString,
}

impl Scenario {
    /// Bytes before the name.
    pub const FIXED_SIZE: usize = 14;
    /// Ends a line in the game's strings (other strings in `xeno.exe` use it between
    /// two lines, such as a label above a field).
    pub const LINE_BREAK: u8 = 0x0c;

    /// The record's position in the table, from its id.
    pub fn index(&self) -> usize {
        usize::from(self.id / 2)
    }

    /// The name's lines, without the line breaks.
    pub fn name_lines(&self) -> Vec<String> {
        let mut lines: Vec<_> = self
            .name
            .split(|&b| b == Self::LINE_BREAK)
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect();
        if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }
}

/// The decoded table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioTable {
    pub scenarios: Vec<Scenario>,
}

impl ScenarioTable {
    /// The word in place of an id that ends the table.
    pub const END_MARKER: u16 = 0xffff;
    /// The game's loader stops after this many records, end marker or not.
    pub const MAX_SCENARIOS: usize = 32;
}

/// Decodes `scendef.bin`: records until the end marker, which must end the file.
pub fn parse(bytes: &[u8]) -> Result<ScenarioTable, FormatError> {
    let invalid = |offset: usize, reason: String| FormatError::Invalid {
        offset: offset as u64,
        reason,
    };

    let mut cursor = std::io::Cursor::new(bytes);
    let mut scenarios = Vec::new();
    loop {
        let at = cursor.position() as usize;
        if u16::read_le(&mut cursor)? == ScenarioTable::END_MARKER {
            break;
        }
        if scenarios.len() == ScenarioTable::MAX_SCENARIOS {
            return Err(invalid(
                at,
                format!(
                    "more than {} scenarios; the game reads no more",
                    ScenarioTable::MAX_SCENARIOS
                ),
            ));
        }
        cursor.set_position(at as u64);
        scenarios.push(Scenario::read(&mut cursor)?);
    }

    let end = cursor.position() as usize;
    if end != bytes.len() {
        return Err(invalid(
            end,
            format!("{} bytes after the end marker", bytes.len() - end),
        ));
    }
    Ok(ScenarioTable { scenarios })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_scendef;

    /// Two made-up places, the second with a two-line name.
    fn two_scenarios() -> Vec<u8> {
        synthetic_scendef(&[
            ((10, 20, 19, 29), 1, b"FIRST PLACE\x0c"),
            ((100, 200, 109, 209), 3, b"SECOND\x0cPLACE\x0c"),
        ])
    }

    #[test]
    fn synthetic_table_decodes() {
        let table = parse(&two_scenarios()).unwrap();
        assert_eq!(table.scenarios.len(), 2);
        let second = &table.scenarios[1];
        assert_eq!((second.id, second.index()), (2, 1));
        assert_eq!(
            second.map_box,
            MapBox {
                top: 100,
                left: 200,
                bottom: 109,
                right: 209
            }
        );
        assert_eq!(second.unlock_level, 3);
        assert_eq!(second.state, 0);
        assert_eq!(second.name_lines(), ["SECOND", "PLACE"]);
        assert_eq!(table.scenarios[0].name_lines(), ["FIRST PLACE"]);
    }

    #[test]
    fn table_snapshot() {
        insta::assert_debug_snapshot!(parse(&two_scenarios()).unwrap());
    }

    #[test]
    fn an_empty_table_is_just_the_end_marker() {
        assert!(parse(&[0xff, 0xff]).unwrap().scenarios.is_empty());
    }

    #[test]
    fn malformed_tables_are_rejected() {
        let mut trailing = two_scenarios();
        trailing.push(0);
        assert!(matches!(
            parse(&trailing),
            Err(FormatError::Invalid { offset, reason })
                if offset as usize == trailing.len() - 1 && reason.contains("1 bytes after")
        ));

        let mut unterminated = two_scenarios();
        unterminated.truncate(unterminated.len() - 2);
        assert!(matches!(parse(&unterminated), Err(FormatError::Binrw(_))));

        let mut unnamed = two_scenarios();
        unnamed.truncate(unnamed.len() - 4);
        assert!(matches!(parse(&unnamed), Err(FormatError::Binrw(_))));

        let records: Vec<_> = (0..33).map(|_| ((0, 0, 1, 1), 1, &b"X"[..])).collect();
        let too_many = synthetic_scendef(&records);
        match parse(&too_many) {
            Err(FormatError::Invalid { offset, reason }) => {
                assert_eq!(offset as usize, 32 * (Scenario::FIXED_SIZE + 2));
                assert!(reason.contains("more than 32"), "{reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_scendef_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir)
            .join("scendef.bin");
        let table = parse(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(table.scenarios.len(), 13, "scen00a..scen12a");
        for (i, s) in table.scenarios.iter().enumerate() {
            assert_eq!(s.index(), i, "id of record {i}");
            assert_eq!(s.state, 0, "state of record {i}");
            assert!(
                s.map_box.bottom < 200 && s.map_box.right < 320,
                "box of {i}"
            );
            assert!(s.map_box.top <= s.map_box.bottom && s.map_box.left <= s.map_box.right);
            assert_eq!(s.name.last(), Some(&Scenario::LINE_BREAK), "name of {i}");
            assert_eq!(s.name_lines().len(), 1, "name of {i}");
        }
    }
}
