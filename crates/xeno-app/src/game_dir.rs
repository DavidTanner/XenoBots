//! Finding the user's copy of the original game and checking that it is complete.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Environment variable that names the game directory when `--game-dir` is not given.
pub const ENV_VAR: &str = "XENOBOTS_GAME_DIR";

/// Directory used when neither `--game-dir` nor [`ENV_VAR`] is set, relative to the
/// current directory.
pub const DEFAULT_DIR: &str = "gameFiles";

/// Files the game needs, as named in a stock install (lowercase).
///
/// This is every file the installer copies except its own (`*.bat`, the `xenobot.1` /
/// `xenobot.2` disk markers), the placeholder `orders.txt`, and `state.rst` /
/// `backup.map`, which look like user state the game writes rather than reads. Which
/// files the game really opens is still to be confirmed from the executable; the list
/// moves into `xeno-formats` once that crate exists (T-007).
pub const EXPECTED_FILES: &[&str] = &[
    // Executable, driver modules and video mode module.
    "xeno.exe",
    "config.dvr",
    "mapdvr.dvr",
    "mgavideo.dvr",
    "sounddvr.dvr",
    "vgaiff.dvr",
    "vgaintro.dvr",
    "vga240.mod",
    // Tables, sound effects, key mapping and the 1920-byte layer files.
    "scendef.bin",
    "dsounds.bin",
    "default.map",
    "fgarray.kyl",
    "layer1.fgd",
    "layer2.fgd",
    "layer3.fgd",
    "layer4.fgd",
    "layer5.fgd",
    // Drop-sequence images.
    "cavedrop.pcx",
    "xenodrop.pcx",
    // Cutscenes.
    "aftermth.vcl",
    "annihil.vcl",
    "arrival.vcl",
    "detect.vcl",
    "ealogo.vcl",
    "emerge.vcl",
    "grndfall.vcl",
    "nlose.vcl",
    "presents.vcl",
    "return.vcl",
    "slose.vcl",
    "triumph.vcl",
    "war.vcl",
    "win.vcl",
    // Scenarios and training missions.
    "scen00a.usg",
    "scen01a.usg",
    "scen02a.usg",
    "scen03a.usg",
    "scen04a.usg",
    "scen05a.usg",
    "scen06a.usg",
    "scen07a.usg",
    "scen08a.usg",
    "scen09a.usg",
    "scen10a.usg",
    "scen11a.usg",
    "scen12a.usg",
    "ctrain.usg",
    "htrain.usg",
    "ntrain.usg",
    "strain.usg",
    // Screen layouts.
    "humancon.m@l",
    "login.m@l",
    "maproom.m@l",
    "review.m@l",
    "scorpcon.m@l",
    "scoutcon.m@l",
    "sitroom.m@l",
    "trainmen.m@l",
    // Sprite banks and sky strips.
    "msprite0.b@n",
    "msprite1.b@n",
    "msprite2.b@n",
    "msprite3.b@n",
    "msprite4.b@n",
    "msprite5.b@n",
    "sky_a.b@n",
    "sky_b.b@n",
    "sky_c.b@n",
    "sky_d.b@n",
    "sky_e.b@n",
    "sky_f.b@n",
    "sky_g.b@n",
    "sky_h.b@n",
    // Music.
    "bigwin3.ms3",
    "lose.ms3",
    "mainthem.ms3",
    "robot3.ms3",
    "win1.ms3",
    "win2.ms3",
    // Unit models. humanoid, missile, scorpion and scout have no `.m32`.
    "basedome.m00",
    "basedome.m08",
    "basedome.m16",
    "basedome.m32",
    "cloak.m00",
    "cloak.m08",
    "cloak.m16",
    "cloak.m32",
    "explode.m00",
    "explode.m08",
    "explode.m16",
    "explode.m32",
    "humanoid.m00",
    "humanoid.m08",
    "humanoid.m16",
    "laser.m00",
    "laser.m08",
    "laser.m16",
    "laser.m32",
    "missile.m00",
    "missile.m08",
    "missile.m16",
    "netrelay.m00",
    "netrelay.m08",
    "netrelay.m16",
    "netrelay.m32",
    "obst.m00",
    "obst.m08",
    "obst.m16",
    "obst.m32",
    "plasma.m00",
    "plasma.m08",
    "plasma.m16",
    "plasma.m32",
    "scorpion.m00",
    "scorpion.m08",
    "scorpion.m16",
    "scout.m00",
    "scout.m08",
    "scout.m16",
];

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

    /// Checks that the directory exists and holds every file in [`EXPECTED_FILES`].
    pub fn check(&self) -> Result<(), CheckError> {
        self.check_files(EXPECTED_FILES)
    }

    /// Checks for `expected` (lowercase names). Names match case-insensitively, since
    /// files copied from DOS media are often upper case and Linux file systems are not
    /// case-insensitive.
    fn check_files(&self, expected: &[&'static str]) -> Result<(), CheckError> {
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
        let present = lowercase_file_names(&self.path)
            .map_err(|e| error(CheckErrorKind::Unreadable(e.to_string())))?;
        let missing: Vec<_> = expected
            .iter()
            .copied()
            .filter(|name| !present.contains(*name))
            .collect();
        match missing.len() {
            0 => Ok(()),
            n if n == expected.len() => Err(error(CheckErrorKind::Empty {
                expected: expected.len(),
            })),
            _ => Err(error(CheckErrorKind::MissingFiles(missing))),
        }
    }
}

/// The lowercased names of the regular files directly inside `dir`.
fn lowercase_file_names(dir: &Path) -> io::Result<HashSet<String>> {
    let mut names = HashSet::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().is_file() {
            names.insert(entry.file_name().to_string_lossy().to_lowercase());
        }
    }
    Ok(names)
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
    fn expected_files_are_lowercase_and_unique() {
        let unique: HashSet<_> = EXPECTED_FILES.iter().collect();
        assert_eq!(unique.len(), EXPECTED_FILES.len());
        for name in EXPECTED_FILES {
            assert_eq!(*name, name.to_lowercase());
        }
    }

    #[test]
    fn complete_directory_passes() {
        let tmp = TempDir::new("complete");
        for name in FEW {
            tmp.touch(name);
        }
        assert_eq!(tmp.game_dir().check_files(FEW), Ok(()));
    }

    #[test]
    fn names_match_case_insensitively() {
        let tmp = TempDir::new("upper");
        for name in FEW {
            tmp.touch(&name.to_uppercase());
        }
        assert_eq!(tmp.game_dir().check_files(FEW), Ok(()));
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
