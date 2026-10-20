//! Runs the built binary to check what a user sees when the game files are missing.

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
