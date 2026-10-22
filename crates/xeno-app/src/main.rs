//! The Xenobots game binary. For now it finds the user's game files, checks that they
//! are all there, then opens a window showing a test card.

mod game_dir;
mod test_pattern;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::builder::TypedValueParser;
use xeno_render::{Aspect, Scale};

use game_dir::GameDir;

/// Xenobots / Ultrabots, reimplemented. Needs your own copy of the original game.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// Directory holding the original game files. Defaults to $XENOBOTS_GAME_DIR, then
    /// ./gameFiles.
    #[arg(long, value_name = "PATH")]
    game_dir: Option<PathBuf>,

    /// Only check the game directory, then exit without opening a window.
    #[arg(long)]
    check: bool,

    /// Picture size: `auto` (the largest that fits the window) or a horizontal factor
    /// from 1 to 32, with the vertical one set by --aspect. A fixed scale sizes the
    /// window to fit; if the window is still too small, its middle is shown.
    #[arg(long, value_name = "auto|N", default_value = "auto", value_parser = parse::<Scale>)]
    scale: Scale,

    /// Pixel shape. `correct`: fill a 4:3 frame as the original monitor did (320x200
    /// pixels 1.2 times taller than wide), within 5% so the picture can be larger.
    /// `exact`: the smallest aspect error, even if the picture is smaller. `off`:
    /// square pixels. Scaling is always by whole pixels, never blurred.
    #[arg(
        long,
        default_value = "correct",
        value_parser = clap::builder::PossibleValuesParser::new(["correct", "exact", "off"])
            .map(|s| s.parse::<Aspect>().expect("listed values parse")),
    )]
    aspect: Aspect,

    /// Start in borderless fullscreen on the current monitor (the display mode is not
    /// changed). F11 or Alt+Enter toggles it; ESC quits.
    #[arg(long)]
    fullscreen: bool,
}

/// Parses a flag value with its `FromStr`.
fn parse<T: std::str::FromStr<Err = String>>(s: &str) -> Result<T, String> {
    s.parse()
}

/// The name and version shown to the user, e.g. `xeno-app 0.1.0`.
fn version_string() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,xeno_app=info"),
    )
    .init();
    println!("{}", version_string());

    let game_dir = GameDir::resolve(cli.game_dir, std::env::var_os(game_dir::ENV_VAR));
    let files = match game_dir.check() {
        Ok(files) => files,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Game files found in {} ({}).",
        files.dir().path.display(),
        files.dir().source
    );
    if cli.check {
        return ExitCode::SUCCESS;
    }
    match window::run(cli.scale, cli.aspect, cli.fullscreen) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
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

    #[test]
    fn scale_aspect_and_fullscreen_flags_parse() {
        let cli = Cli::try_parse_from(["xeno-app"]).unwrap();
        assert_eq!((cli.scale, cli.aspect), (Scale::Auto, Aspect::Correct));
        assert!(!cli.fullscreen);
        let cli = Cli::try_parse_from(["xeno-app", "--scale", "3", "--aspect", "off"]).unwrap();
        assert_eq!((cli.scale, cli.aspect), (Scale::Fixed(3), Aspect::Off));
        assert!(
            Cli::try_parse_from(["xeno-app", "--fullscreen"])
                .unwrap()
                .fullscreen
        );
        for bad in [["--scale", "0"], ["--scale", "big"], ["--aspect", "4:3"]] {
            assert!(Cli::try_parse_from(["xeno-app", bad[0], bad[1]]).is_err());
        }
    }
}
