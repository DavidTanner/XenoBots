//! Finding the user's copy of the original game and checking that it is complete.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use xeno_formats::files::EXPECTED_FILES;

/// Environment variable that names the game directory when `--game-dir` is not given.
pub const ENV_VAR: &str = "XENOBOTS_GAME_DIR";

/// Directory used when neither `--game-dir` nor [`ENV_VAR`] is set, relative to the
/// current directory.
pub const DEFAULT_DIR: &str = "gameFiles";

/// Where the game directory setting came from, so errors can say what to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Flag,
    EnvVar,
    Default,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Flag => write!(f, "set by --game-dir"),
            Source::EnvVar => write!(f, "set by {ENV_VAR}"),
            Source::Default => write!(f, "the default"),
        }
    }
}

/// The directory to load game files from, and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDir {
    pub path: PathBuf,
    pub source: Source,
}

impl GameDir {
    /// Picks the game directory: `--game-dir` wins, then [`ENV_VAR`], then
    /// [`DEFAULT_DIR`]. An empty environment variable counts as unset.
    pub fn resolve(flag: Option<PathBuf>, env: Option<OsString>) -> Self {
        if let Some(path) = flag {
            return GameDir {
                path,
                source: Source::Flag,
            };
        }
        match env {
            Some(value) if !value.is_empty() => GameDir {
                path: value.into(),
                source: Source::EnvVar,
            },
            _ => GameDir {
                path: DEFAULT_DIR.into(),
                source: Source::Default,
            },
        }
    }

    /// Checks that the directory exists and holds every file in [`EXPECTED_FILES`],
    /// and returns the lookup that loaders open files through.
    pub fn check(&self) -> Result<GameFiles, CheckError> {
        self.check_files(EXPECTED_FILES)
    }

    /// Checks for `expected` (lowercase names). Names match case-insensitively, since
    /// files copied from DOS media are often upper case and Linux file systems are not
    /// case-insensitive.
    fn check_files(&self, expected: &[&'static str]) -> Result<GameFiles, CheckError> {
        let error = |kind| CheckError {
            dir: self.clone(),
            kind,
        };
        if !self.path.exists() {
            return Err(error(CheckErrorKind::NotFound));
        }
        if !self.path.is_dir() {
            return Err(error(CheckErrorKind::NotADirectory));
        }
        let by_name = index_files(&self.path)
            .map_err(|e| error(CheckErrorKind::Unreadable(e.to_string())))?;
        let missing: Vec<_> = expected
            .iter()
            .copied()
            .filter(|name| !by_name.contains_key(*name))
            .collect();
        match missing.len() {
            0 => Ok(GameFiles {
                dir: self.clone(),
                by_name,
            }),
            n if n == expected.len() => Err(error(CheckErrorKind::Empty {
                expected: expected.len(),
            })),
            _ => Err(error(CheckErrorKind::MissingFiles(missing))),
        }
    }
}

/// The regular files directly inside `dir`, keyed by lowercased name. If two names
/// differ only in case (possible on Linux), the one that sorts first wins, so the
/// choice does not depend on directory order.
fn index_files(dir: &Path) -> io::Result<HashMap<String, PathBuf>> {
    let mut by_name = HashMap::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        by_name
            .entry(name.to_string_lossy().to_lowercase())
            .and_modify(|kept: &mut PathBuf| {
                if path < *kept {
                    kept.clone_from(&path);
                }
            })
            .or_insert(path);
    }
    Ok(by_name)
}

/// A checked game directory: finds files by their lowercase stock-install name, whatever
/// case they have on disk. Loaders open game files through this, never by joining a
/// name onto the directory themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFiles {
    dir: GameDir,
    by_name: HashMap<String, PathBuf>,
}

impl GameFiles {
    /// The directory the files are in, and how it was chosen.
    pub fn dir(&self) -> &GameDir {
        &self.dir
    }

    /// Every file's lowercase name, in no order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// The real path of `name` (matched case-insensitively), if the directory has it.
    pub fn path(&self, name: &str) -> Option<&Path> {
        self.by_name.get(&name.to_lowercase()).map(PathBuf::as_path)
    }

    /// Reads all of `name` (matched case-insensitively).
    pub fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        match self.path(name) {
            Some(path) => std::fs::read(path),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{name} is not in game directory {}",
                    self.dir.path.display()
                ),
            )),
        }
    }
}

/// Why the game directory cannot be used. Its `Display` is the message shown to the
/// user, including how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckError {
    pub dir: GameDir,
    pub kind: CheckErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckErrorKind {
    NotFound,
    NotADirectory,
    Unreadable(String),
    /// None of the expected files are there: probably the wrong directory.
    Empty {
        expected: usize,
    },
    /// Some files are missing, listed in [`EXPECTED_FILES`] order.
    MissingFiles(Vec<&'static str>),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.dir.path.display();
        let source = self.dir.source;
        match &self.kind {
            CheckErrorKind::NotFound => {
                writeln!(f, "game directory {path} ({source}) does not exist.")?
            }
            CheckErrorKind::NotADirectory => {
                writeln!(f, "game directory {path} ({source}) is not a directory.")?
            }
            CheckErrorKind::Unreadable(reason) => {
                writeln!(f, "cannot read game directory {path} ({source}): {reason}")?
            }
            CheckErrorKind::Empty { expected } => writeln!(
                f,
                "game directory {path} ({source}) contains none of the {expected} \
                 expected game files. Is this the right directory?"
            )?,
            CheckErrorKind::MissingFiles(missing) => {
                writeln!(
                    f,
                    "game directory {path} ({source}) is missing {} game file(s):",
                    missing.len()
                )?;
                for name in missing {
                    writeln!(f, "  {name}")?;
                }
            }
        }
        write!(
            f,
            "Xenobots needs your own copy of the original game. Copy every file from \
             your installed game (usually C:\\XENOBOT) into one directory, then either \
             put it at ./{DEFAULT_DIR}, pass --game-dir <path>, or set {ENV_VAR}=<path>."
        )
    }
}

impl std::error::Error for CheckError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A uniquely named directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("xeno-app-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn touch(&self, name: &str) {
            std::fs::write(self.0.join(name), b"").unwrap();
        }

        fn game_dir(&self) -> GameDir {
            GameDir {
                path: self.0.clone(),
                source: Source::Flag,
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const FEW: &[&str] = &["xeno.exe", "scendef.bin", "login.m@l"];

    #[test]
    fn flag_wins_over_env_and_default() {
        let dir = GameDir::resolve(Some("from-flag".into()), Some("from-env".into()));
        assert_eq!(dir.path, PathBuf::from("from-flag"));
        assert_eq!(dir.source, Source::Flag);
    }

    #[test]
    fn env_wins_over_default() {
        let dir = GameDir::resolve(None, Some("from-env".into()));
        assert_eq!(dir.path, PathBuf::from("from-env"));
        assert_eq!(dir.source, Source::EnvVar);
    }

    #[test]
    fn default_when_nothing_set_or_env_empty() {
        for env in [None, Some(OsString::new())] {
            let dir = GameDir::resolve(None, env);
            assert_eq!(dir.path, PathBuf::from(DEFAULT_DIR));
            assert_eq!(dir.source, Source::Default);
        }
    }

    #[test]
    fn complete_directory_passes() {
        let tmp = TempDir::new("complete");
        for name in FEW {
            tmp.touch(name);
        }
        let files = tmp.game_dir().check_files(FEW).unwrap();
        assert_eq!(files.dir(), &tmp.game_dir());
        assert_eq!(
            files.path("xeno.exe"),
            Some(tmp.0.join("xeno.exe").as_path())
        );
    }

    #[test]
    fn upper_case_install_is_found_and_read_by_lowercase_name() {
        let tmp = TempDir::new("upper");
        for name in FEW {
            tmp.touch(&name.to_uppercase());
        }
        std::fs::write(tmp.0.join("SCENDEF.BIN"), b"table").unwrap();
        let files = tmp.game_dir().check_files(FEW).unwrap();
        // Compare the file name as stored, not the whole path: on a case-insensitive
        // file system a lowercase join would point at the same file anyway.
        let real = files.path("scendef.bin").unwrap();
        assert_eq!(real.file_name().unwrap(), "SCENDEF.BIN");
        assert_eq!(files.read("scendef.bin").unwrap(), b"table");
        assert_eq!(files.read("ScenDef.Bin").unwrap(), b"table");
    }

    #[test]
    fn reading_an_absent_file_names_it() {
        let tmp = TempDir::new("absent");
        for name in FEW {
            tmp.touch(name);
        }
        let files = tmp.game_dir().check_files(FEW).unwrap();
        assert_eq!(files.path("dsounds.bin"), None);
        let err = files.read("dsounds.bin").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("dsounds.bin"), "{err}");
    }

    #[test]
    fn missing_files_are_listed_by_name_in_order() {
        let tmp = TempDir::new("partial");
        tmp.touch("scendef.bin");
        let err = tmp.game_dir().check_files(FEW).unwrap_err();
        assert_eq!(
            err.kind,
            CheckErrorKind::MissingFiles(vec!["xeno.exe", "login.m@l"])
        );
        let message = err.to_string();
        assert!(message.contains("missing 2 game file(s)"));
        assert!(message.contains("  xeno.exe\n  login.m@l\n"));
        assert!(message.contains("--game-dir"));
        assert!(message.contains(ENV_VAR));
    }

    #[test]
    fn subdirectory_does_not_count_as_a_file() {
        let tmp = TempDir::new("subdir");
        tmp.touch("xeno.exe");
        tmp.touch("login.m@l");
        std::fs::create_dir(tmp.0.join("scendef.bin")).unwrap();
        let err = tmp.game_dir().check_files(FEW).unwrap_err();
        assert_eq!(err.kind, CheckErrorKind::MissingFiles(vec!["scendef.bin"]));
    }

    #[test]
    fn directory_with_none_of_the_files_says_so() {
        let tmp = TempDir::new("empty");
        let err = tmp.game_dir().check_files(FEW).unwrap_err();
        assert_eq!(err.kind, CheckErrorKind::Empty { expected: 3 });
        assert!(err.to_string().contains("Is this the right directory?"));
    }

    #[test]
    fn nonexistent_directory_names_path_and_source() {
        let tmp = TempDir::new("gone");
        let dir = GameDir {
            path: tmp.0.join("nope"),
            source: Source::EnvVar,
        };
        let err = dir.check().unwrap_err();
        assert_eq!(err.kind, CheckErrorKind::NotFound);
        let message = err.to_string();
        assert!(message.contains(&dir.path.display().to_string()));
        assert!(message.contains("set by XENOBOTS_GAME_DIR"));
    }

    #[test]
    fn file_instead_of_directory_is_rejected() {
        let tmp = TempDir::new("file");
        tmp.touch("xeno.exe");
        let dir = GameDir {
            path: tmp.0.join("xeno.exe"),
            source: Source::Flag,
        };
        assert_eq!(dir.check().unwrap_err().kind, CheckErrorKind::NotADirectory);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR pointing at a real install"]
    fn real_assets_game_dir_is_complete() {
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        // `cargo test` runs in the crate directory, so a relative path such as
        // `./gameFiles` is taken relative to the workspace root, where it was typed.
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = GameDir::resolve(None, Some(workspace_root.join(path).into()));
        if let Err(e) = dir.check() {
            panic!("{e}");
        }
    }
}
