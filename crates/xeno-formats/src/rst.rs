//! `state.rst`: DOS text that the game never opens and that is not a save game (saves
//! are `<NAME>.LOG`). See `docs/formats/rst.md`. A text format, so no `binrw`.

use crate::FormatError;

/// The decoded file: its lines without their `CR LF`, blank lines included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restart {
    pub lines: Vec<String>,
    /// Whether the text ended with the DOS end-of-file byte [`Restart::EOF`].
    pub has_eof: bool,
}

impl Restart {
    /// The DOS end-of-file byte (Ctrl-Z).
    pub const EOF: u8 = 0x1a;

    /// The names of the INI-style `[NAME]` section headings.
    pub fn sections(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| l.strip_prefix('[')?.strip_suffix(']'))
            .collect()
    }
}

/// Decodes `state.rst`: ASCII lines ending in `CR LF` (a bare `LF` is accepted),
/// optionally ended by [`Restart::EOF`] with nothing after it.
pub fn parse(bytes: &[u8]) -> Result<Restart, FormatError> {
    let invalid = |at: usize, reason: String| FormatError::Invalid {
        offset: at as u64,
        reason,
    };
    let eof = bytes.iter().position(|&b| b == Restart::EOF);
    if let Some(end) = eof.filter(|&end| end + 1 != bytes.len()) {
        let reason = format!("{} bytes after the end-of-file byte", bytes.len() - end - 1);
        return Err(invalid(end + 1, reason));
    }
    let text = &bytes[..eof.unwrap_or(bytes.len())];
    if let Some(at) = text.iter().position(|b| !b.is_ascii()) {
        return Err(invalid(at, "not ASCII text".into()));
    }
    let mut lines: Vec<String> = String::from_utf8_lossy(text)
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(Restart {
        lines,
        has_eof: eof.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up text with the stock file's shape.
    const SAMPLE: &[u8] = b"2 0 1\r\n\r\n[ONE]\r\nA:\\X.BAT\r\n\x1a";

    #[test]
    fn synthetic_file_decodes() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.lines, ["2 0 1", "", "[ONE]", "A:\\X.BAT"]);
        assert_eq!(r.sections(), ["ONE"]);
        assert!(r.has_eof);
        let r = parse(b"7\n[S]\nlast").unwrap();
        assert_eq!((r.lines.len(), r.has_eof), (3, false));
        assert!(parse(b"").unwrap().lines.is_empty());
    }

    #[test]
    fn restart_snapshot() {
        insta::assert_debug_snapshot!(parse(SAMPLE).unwrap());
    }

    #[test]
    fn malformed_files_are_rejected() {
        assert!(matches!(
            parse(b"1\r\n\x1a\r\n"),
            Err(FormatError::Invalid { offset: 4, reason }) if reason.contains("2 bytes after")
        ));
        assert!(matches!(
            parse(b"1 \xff\r\n"),
            Err(FormatError::Invalid { offset: 2, .. })
        ));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_state_rst_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir)
            .join("state.rst");
        let r = parse(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(r.sections(), ["FILES"]);
        assert!(r.has_eof);
    }
}
