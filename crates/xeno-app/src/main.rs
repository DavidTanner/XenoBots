//! The Xenobots game binary. For now it finds the user's game files and checks that
//! they are all there.

mod game_dir;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use game_dir::GameDir;

/// Xenobots / Ultrabots, reimplemented. Needs your own copy of the original game.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// Directory holding the original game files. Defaults to $XENOBOTS_GAME_DIR, then
    /// ./gameFiles.
    #[arg(long, value_name = "PATH")]
    game_dir: Option<PathBuf>,
}

/// The name and version shown to the user, e.g. `xeno-app 0.1.0`.
fn version_string() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    println!("{}", version_string());

    let game_dir = GameDir::resolve(cli.game_dir, std::env::var_os(game_dir::ENV_VAR));
    if let Err(e) = game_dir.check() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    println!(
        "Game files found in {} ({}).",
        game_dir.path.display(),
        game_dir.source
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_names_the_binary_and_version() {
        let s = version_string();
        assert!(s.starts_with("xeno-app "));
        assert!(s.ends_with(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
