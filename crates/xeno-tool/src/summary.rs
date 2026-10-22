//! `inspect --all`: runs every file in a directory through its parser and counts the
//! results per extension.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use anyhow::Context;

use crate::inspect::{self, Outcome};

/// The counts for one extension.
#[derive(Debug, Default)]
struct Row {
    files: usize,
    parsed: usize,
    failed: usize,
    no_parser: usize,
}

/// What the parsers made of a directory.
#[derive(Debug, Default)]
pub struct Summary {
    /// Keyed by lower-case extension, so sorted.
    rows: BTreeMap<String, Row>,
    /// File name and error, sorted by name ignoring case.
    failures: Vec<(String, String)>,
}

impl Summary {
    /// Runs every file directly in `dir` through its parser; subdirectories are not
    /// entered. A file that cannot be read counts as failed.
    pub fn of_dir(dir: &Path) -> anyhow::Result<Summary> {
        let entries =
            std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
        let mut summary = Summary::default();
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read {}", dir.display()))?
                .path();
            if !path.is_file() {
                continue;
            }
            let outcome = match std::fs::read(&path) {
                Ok(bytes) => inspect::outcome(&path, &bytes),
                Err(e) => Outcome::Failed(format!("cannot read: {e}")),
            };
            summary.add(&path, outcome);
        }
        summary
            .failures
            .sort_by_key(|(name, _)| (name.to_lowercase(), name.clone()));
        Ok(summary)
    }

    fn add(&mut self, path: &Path, outcome: Outcome) {
        let row = self.rows.entry(inspect::extension(path)).or_default();
        row.files += 1;
        match outcome {
            Outcome::Parsed => row.parsed += 1,
            Outcome::NoParser => row.no_parser += 1,
            Outcome::Failed(error) => {
                row.failed += 1;
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                self.failures.push((name.into_owned(), error));
            }
        }
    }

    /// How many files a parser rejected (or could not be read).
    pub fn failed(&self) -> usize {
        self.failures.len()
    }

    /// One row per extension, a total, then one line per failure.
    pub fn table(&self) -> String {
        let width = self.rows.keys().map(String::len).max().unwrap_or(0).max(9);
        let mut total = Row::default();
        let mut out = format!("{:width$}  files  parsed  failed  no parser\n", "extension");
        let mut line = |name: &str, r: &Row| {
            writeln!(
                out,
                "{name:width$}  {:5}  {:6}  {:6}  {:9}",
                r.files, r.parsed, r.failed, r.no_parser
            )
            .unwrap();
        };
        for (extension, row) in &self.rows {
            line(extension, row);
            total.files += row.files;
            total.parsed += row.parsed;
            total.failed += row.failed;
            total.no_parser += row.no_parser;
        }
        line("total", &total);
        if !self.failures.is_empty() {
            out.push_str("\nfailed:\n");
            for (name, error) in &self.failures {
                writeln!(out, "  {name}: {error}").unwrap();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use xeno_formats::fixtures::{synthetic_bytes, synthetic_pcx, synthetic_usg};

    /// A fresh directory holding `files`, plus an empty subdirectory.
    fn temp_dir(name: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xeno-tool-summary-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SUBDIR.PCX")).unwrap();
        for (file, bytes) in files {
            std::fs::write(dir.join(file), bytes).unwrap();
        }
        dir
    }

    fn summary_of(name: &str, files: &[(&str, Vec<u8>)]) -> Summary {
        let dir = temp_dir(name, files);
        let summary = Summary::of_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        summary.unwrap()
    }

    fn good_files() -> Vec<(&'static str, Vec<u8>)> {
        let pcx = synthetic_pcx(2, 2, &[1, 2, 3, 4], &synthetic_bytes(768, 2));
        vec![
            ("DROP.PCX", pcx),
            ("scen00a.usg", synthetic_usg()),
            ("LAYER1.FGD", vec![40; 1920]),
            ("XENO.EXE", synthetic_bytes(100, 1)),
            ("xenobot.1", b"1 \r\n".to_vec()),
            ("README", b"text".to_vec()),
        ]
    }

    #[test]
    fn a_failure_is_counted_and_listed() {
        let mut files = good_files();
        files.push(("bad.pcx", synthetic_bytes(10, 3)));
        files.push(("Layer2.Fgd", vec![40; 5]));
        let summary = summary_of("failure", &files);
        assert_eq!(summary.failed(), 2);
        insta::assert_snapshot!(summary.table());
    }

    #[test]
    fn files_without_a_parser_do_not_fail() {
        let summary = summary_of("clean", &good_files());
        assert_eq!(summary.failed(), 0);
        let table = summary.table();
        let row = |name: &str| {
            let line = table.lines().find(|l| l.starts_with(&format!("{name} ")));
            line.map(|l| l.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
        };
        assert_eq!(row("exe").as_deref(), Some("1 0 0 1"), "{table}");
        assert_eq!(row("total").as_deref(), Some("6 3 0 3"), "{table}");
        assert!(!table.contains("failed:"), "{table}");
    }

    #[test]
    fn a_missing_dir_is_an_error() {
        let dir = std::env::temp_dir().join("xeno-tool-summary-no-such-dir");
        assert!(Summary::of_dir(&dir).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR pointing at a real install"]
    fn real_assets_inspect_all_parses_a_full_install() {
        let Some(path) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory, so a relative path such as
        // `./gameFiles` is taken relative to the workspace root, where it was typed.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let summary = Summary::of_dir(&workspace_root.join(path)).unwrap();
        assert_eq!(summary.failed(), 0, "{}", summary.table());
        assert!(summary.rows.values().any(|r| r.parsed > 0));
    }
}
