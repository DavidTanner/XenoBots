//! The Xenobots game binary. For now it finds the user's game files, checks that they
//! are all there, then opens a window that plays the start-up cutscene and the intro
//! drop and steps through the front end's screens and the cutscenes after a mission
//! (the mission itself is still a placeholder), or with `--view` one asset.

mod campaign;
mod clock;
mod console;
mod dialog;
mod drop;
mod editor;
mod game_dir;
mod humancon;
mod intro;
mod keymap;
mod login;
mod mainview;
mod maproom;
mod mission;
mod music;
mod panels;
mod pointer;
mod review;
mod save;
mod scene;
mod scorpcon;
mod scoutcon;
mod screens;
mod status;
mod terrain;
mod test_pattern;
mod translate;
mod viewer;
mod window;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::builder::TypedValueParser;
use xeno_render::{Aspect, Scale};

use game_dir::GameDir;
// The game's input types live in the sim; `crate::input` keeps naming them.
use screens::{Ending, Screen, View};
use xeno_sim::input;

/// Xenobots / Ultrabots, reimplemented. Needs your own copy of the original game.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// Directory holding the original game files. Defaults to $XENOBOTS_GAME_DIR, then
    /// ./gameFiles.
    #[arg(long, value_name = "PATH")]
    game_dir: Option<PathBuf>,

    /// Where the situation room's Ctrl+Alt+P (`EDITED.USG`), the scenario editor's
    /// terrain key (`BACKUP.MAP`) and the map room's N and Q (`<NAME>.LOG`, read back at
    /// log-in) save. Defaults to the game directory, as the original saves in its own.
    #[arg(long, value_name = "PATH")]
    save_dir: Option<PathBuf>,

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
    /// changed). F11 or Alt+Enter toggles it; ESC leaves it.
    #[arg(long)]
    fullscreen: bool,

    /// Skip the start-up cutscene and the drop that follows it (any key but P ends
    /// either early, and a key in the cutscene also skips the drop; P pauses).
    #[arg(long)]
    no_intro: bool,

    /// Play the intro drop (vgaintro.dvr: the scroll over xenodrop.pcx and
    /// cavedrop.pcx) alone, then go on to the log-in screen.
    #[arg(long, conflicts_with_all = ["no_intro", "view", "screen"])]
    drop: bool,

    /// Start at this screen, skipping the cutscene and the drop (for development).
    /// `won`, `lost` and `triumph` play the cutscene after a mission, then go to the
    /// map room. `test-card` shows the synthetic test card instead of the screens.
    #[arg(long, value_enum, value_name = "NAME", conflicts_with = "view")]
    screen: Option<StartScreen>,

    /// Show one file instead of the cutscene and screens: a sprite bank or sky strip (.b@n), a
    /// screen (.m@l), a cutscene picture (.vcl) or a backdrop (.pcx). A bare name is
    /// looked up in the game directory, in any case; anything else is a path.
    #[arg(long, value_name = "FILE")]
    view: Option<PathBuf>,

    /// With --view: take the colours from this .m@l, .vcl or .pcx (found the same way)
    /// instead of the file's own or, for files without one, a stock screen's.
    #[arg(long, value_name = "FILE", requires = "view")]
    palette: Option<PathBuf>,

    /// The seed of the game's random word (the campaign's alert rises and calendar).
    /// The original seeds it from the timer, and so does the port by default; the seed
    /// is logged at start-up, and passing it again repeats the run.
    #[arg(long, value_name = "N")]
    seed: Option<u32>,
}

/// `--screen`'s values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum StartScreen {
    Login,
    MapRoom,
    Training,
    Review,
    SitRoom,
    Humancon,
    Scorpcon,
    Scoutcon,
    Won,
    Lost,
    Triumph,
    TestCard,
}

impl StartScreen {
    /// The screen, or `None` for the test card.
    fn screen(self) -> Option<Screen> {
        use StartScreen::*;
        Some(match self {
            Login => Screen::Login,
            MapRoom => Screen::MapRoom,
            Training => Screen::Training,
            Review => Screen::Review,
            SitRoom => Screen::Mission(View::SitRoom),
            Humancon => Screen::Mission(View::Humanoid),
            Scorpcon => Screen::Mission(View::Scorpion),
            Scoutcon => Screen::Mission(View::Scout),
            Won => Screen::Cutscene(Ending::Won),
            Lost => Screen::Cutscene(Ending::Lost),
            Triumph => Screen::Cutscene(Ending::Triumph),
            TestCard => return None,
        })
    }
}

/// Parses a flag value with its `FromStr`.
fn parse<T: std::str::FromStr<Err = String>>(s: &str) -> Result<T, String> {
    s.parse()
}

/// A seed from the system clock, as the original seeds its random word from the timer.
fn timer_seed() -> u32 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    now.map_or(0, |t| t.subsec_nanos() ^ t.as_secs() as u32)
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
    // With --check, a --view file is decoded but not shown.
    let viewer = match cli.view {
        Some(view) => match viewer::load(&files, &view, cli.palette.as_deref()) {
            Ok(viewer) => Some(viewer),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let no_intro = cli.no_intro || cli.screen.is_some();
    // The intro is read from xeno.exe and decoded with --check too.
    let intro = if viewer.is_none() && !no_intro && !cli.drop {
        match intro::load(&files) {
            Ok(intro) => Some(intro),
            Err(e) => {
                eprintln!("error: cannot play the intro: {e} (--no-intro skips it)");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    // So is the drop, from vgaintro.dvr, the layers and the backdrops.
    let drop = if viewer.is_none() && !no_intro {
        match drop::load(&files) {
            Ok(drop) => Some(drop),
            Err(e) => {
                eprintln!("error: cannot play the intro drop: {e} (--no-intro skips it)");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    // Then the screens, from the .m@l files, or the test card.
    let start = cli.screen.map_or(Some(Screen::Login), StartScreen::screen);
    let seed = cli.seed.unwrap_or_else(timer_seed);
    log::info!("random seed {seed} (--seed {seed} repeats it)");
    let front = match start {
        // Not shown after --view.
        _ if viewer.is_some() => window::FrontEnd::TestCard(keymap::KeyMap::empty()),
        Some(start) => match screens::load(&files).and_then(|p| {
            let mut login = login::load(&files, seed)?;
            let mut missions = mission::load(&files, login.campaign.scenarios.len())?;
            let endings = intro::load_endings(&files)?;
            let map = maproom::load(&files)?;
            missions.editor_unlocked = map.editor;
            if let Some(dir) = &cli.save_dir {
                missions.save_dir = dir.clone();
                login.save_dir = dir.clone();
            }
            Ok((p, login, map, missions, endings))
        }) {
            Ok((pictures, login, map, missions, endings)) => {
                let mut screens = screens::Screens::new(pictures, login, map, missions, start);
                screens.set_endings(endings);
                window::FrontEnd::Screens(Box::new(screens))
            }
            Err(e) => {
                eprintln!("error: cannot show the screens: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            // config.dvr's keyboard stick (the original game never reads it).
            let keymap = keymap::KeyMap::load(&files);
            log::info!("stick keys from {:?}: {}", keymap.source, keymap.describe());
            window::FrontEnd::TestCard(keymap)
        }
    };
    if cli.check {
        return ExitCode::SUCCESS;
    }
    let mut music = match viewer {
        Some(_) => music::Jukebox::default(),
        None => music::Jukebox::open(&files),
    };
    // The game's own start (intro, drop, log-in) plays the main theme from its first
    // retrace; `--screen` starts elsewhere, in silence.
    if viewer.is_none() && cli.screen.is_none() {
        music.cue(music::Cue::Start(music::MAIN_THEME));
    }
    match window::run(
        cli.scale,
        cli.aspect,
        cli.fullscreen,
        viewer,
        intro,
        drop,
        front,
        music,
    ) {
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
        assert!(!cli.fullscreen && !cli.no_intro);
        assert!(
            Cli::try_parse_from(["xeno-app", "--no-intro"])
                .unwrap()
                .no_intro
        );
        assert!(Cli::try_parse_from(["xeno-app", "--drop"]).unwrap().drop);
        assert!(Cli::try_parse_from(["xeno-app", "--drop", "--no-intro"]).is_err());
        assert!(Cli::try_parse_from(["xeno-app", "--drop", "--view", "x.pcx"]).is_err());
        let screen = |name| Cli::try_parse_from(["xeno-app", "--screen", name]);
        assert_eq!(
            screen("map-room").unwrap().screen,
            Some(StartScreen::MapRoom)
        );
        assert_eq!(StartScreen::TestCard.screen(), None);
        assert_eq!(
            StartScreen::Scoutcon.screen(),
            Some(Screen::Mission(View::Scout))
        );
        assert!(screen("briefing").is_err());
        assert!(Cli::try_parse_from(["xeno-app", "--screen", "login", "--drop"]).is_err());
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

    #[test]
    fn view_and_palette_flags_parse() {
        let cli = Cli::try_parse_from(["xeno-app"]).unwrap();
        assert_eq!((cli.view, cli.palette), (None, None));
        let cli = Cli::try_parse_from([
            "xeno-app",
            "--view",
            "msprite1.b@n",
            "--palette",
            "./x/login.m@l",
        ])
        .unwrap();
        assert_eq!(cli.view, Some(PathBuf::from("msprite1.b@n")));
        assert_eq!(cli.palette, Some(PathBuf::from("./x/login.m@l")));
        // A palette without a file to view is refused.
        assert!(Cli::try_parse_from(["xeno-app", "--palette", "login.m@l"]).is_err());
        let seed =
            |args: &[&str]| Cli::try_parse_from([&["xeno-app"], args].concat()).map(|c| c.seed);
        assert_eq!(seed(&[]).unwrap(), None);
        assert_eq!(seed(&["--seed", "7"]).unwrap(), Some(7));
        assert!(seed(&["--seed", "-1"]).is_err());
    }
}
