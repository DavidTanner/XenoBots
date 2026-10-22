//! Runs the built binary to check what a user sees when the game files are missing,
//! and that `--check` stops before opening a window.

use std::process::Command;

#[test]
fn missing_game_dir_fails_with_an_actionable_message() {
    let missing = std::env::temp_dir().join(format!("xeno-app-cli-missing-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_xeno-app"))
        .arg("--game-dir")
        .arg(&missing)
        .env_remove("XENOBOTS_GAME_DIR")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&missing.display().to_string()), "{stderr}");
    assert!(stderr.contains("does not exist"), "{stderr}");
    assert!(stderr.contains("XENOBOTS_GAME_DIR"), "{stderr}");
}

#[test]
fn check_flag_verifies_the_game_dir_without_opening_a_window() {
    let dir = std::env::temp_dir().join(format!("xeno-app-cli-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in xeno_formats::files::EXPECTED_FILES {
        std::fs::write(dir.join(name), b"").unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_xeno-app"))
        .arg("--game-dir")
        .arg(&dir)
        .arg("--check")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Game files found in"), "{stdout}");
}

#[test]
fn check_with_view_decodes_the_file_by_name_in_any_case() {
    let dir = std::env::temp_dir().join(format!("xeno-app-cli-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in xeno_formats::files::EXPECTED_FILES {
        std::fs::write(dir.join(name), b"").unwrap();
    }
    // Synthetic, not the game's: a grey palette and a flat picture.
    let palette: Vec<u8> = (0..=255u8).flat_map(|i| [i / 4; 3]).collect();
    let screen = xeno_formats::fixtures::synthetic_screen(&palette, &[7; 64_000]);
    std::fs::write(dir.join("login.m@l"), screen).unwrap();
    let run = |view: &str| {
        Command::new(env!("CARGO_BIN_EXE_xeno-app"))
            .arg("--game-dir")
            .arg(&dir)
            .args(["--check", "--view", view])
            .output()
            .unwrap()
    };
    let (good, empty, unknown) = (run("LOGIN.M@L"), run("sitroom.m@l"), run("scen00a.usg"));
    std::fs::remove_dir_all(&dir).unwrap();

    assert!(good.status.success(), "{good:?}");
    assert!(!empty.status.success());
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(!unknown.status.success());
    assert!(
        stderr.contains("scen00a.usg") && stderr.contains(".vcl"),
        "{stderr}"
    );
}
