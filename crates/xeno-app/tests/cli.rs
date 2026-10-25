//! Runs the built binary to check what a user sees when the game files are missing,
//! and that `--check` stops before opening a window, having read the intro, the drop
//! and the screens.

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
fn check_flag_verifies_the_game_dir_the_intro_the_drop_and_screens_without_a_window() {
    let dir = std::env::temp_dir().join(format!("xeno-app-cli-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in xeno_formats::files::EXPECTED_FILES {
        std::fs::write(dir.join(name), b"").unwrap();
    }
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xeno-app"))
            .arg("--game-dir")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap()
    };
    // Empty files: the intro cannot be read from xeno.exe, unless it is skipped; the
    // test card needs no screens.
    let (empty, card) = (
        run(&["--check"]),
        run(&["--check", "--screen", "test-card"]),
    );
    // Synthetic, not the game's: an LZEXE-packed image whose entry point loads DS =
    // 0x20, with ten made-up entries for `ealogo.vcl` at DS:0xecac, and a flat picture.
    // Also made-up map room tables (`synthetic_map_room`), mission views, a training
    // menu and programs (`synthetic_views`, `synthetic_programs`) and a review
    // (`synthetic_review`), the campaign's rules (`synthetic_campaign`), and the
    // cutscenes after a mission and the captions' string segment (`synthetic_endings`),
    // and the movement tables (`synthetic_motion`).
    let chain = 0x200 + 0xecac;
    let mut image = vec![0u8; 0x1c000];
    image[0x10..0x15].copy_from_slice(&[0xb8, 0x20, 0, 0x8e, 0xd8]);
    for i in 0..10 {
        let entry = [0x20u16, 0, 0, 0, 1, 0, 0, 0, 100 * (i + 1), 32, 64, 2, 0];
        let bytes: Vec<u8> = entry.iter().flat_map(|w| w.to_le_bytes()).collect();
        image[chain + usize::from(i) * 26..][..26].copy_from_slice(&bytes);
    }
    let mut relocations = vec![0x11];
    relocations.extend(xeno_formats::fixtures::synthetic_map_room(
        &mut image, 0x200,
    ));
    relocations.extend(xeno_formats::fixtures::synthetic_views(&mut image, 0x200));
    relocations.extend(xeno_formats::fixtures::synthetic_review(&mut image, 0x200));
    relocations.extend(xeno_formats::fixtures::synthetic_endings(&mut image, 0x200));
    xeno_formats::fixtures::synthetic_programs(&mut image, 0x200);
    xeno_formats::fixtures::synthetic_campaign(&mut image, 0x200);
    xeno_formats::fixtures::synthetic_motion(
        &mut image,
        0x200,
        xeno_formats::fixtures::SYNTHETIC_MAP_ROOM_STRINGS,
    );
    relocations.sort_unstable();
    relocations.dedup();
    let exe = xeno_formats::fixtures::synthetic_lzexe(&image, (1, 0), &relocations);
    std::fs::write(dir.join("xeno.exe"), exe).unwrap();
    let picture = xeno_formats::fixtures::synthetic_vcl(&[0; 768], &[1; 640 * 240]);
    for name in ["ealogo", "win", "nlose", "slose", "triumph"] {
        std::fs::write(dir.join(format!("{name}.vcl")), &picture).unwrap();
    }
    // And a made-up mgavideo.dvr for the captions' and the credits' fonts.
    use xeno_formats::fixtures::{
        synthetic_mgavideo, synthetic_numbers, synthetic_pcx, synthetic_scendef, synthetic_vgaintro,
    };
    std::fs::write(dir.join("mgavideo.dvr"), synthetic_mgavideo()).unwrap();
    // The drop's files are still empty.
    let no_drop = run(&["--check"]);
    // Synthetic, not the game's: a made-up vgaintro.dvr, flat backdrops and layers.
    let driver = synthetic_vgaintro(&synthetic_numbers());
    std::fs::write(dir.join("vgaintro.dvr"), driver).unwrap();
    for name in ["xenodrop.pcx", "cavedrop.pcx"] {
        let backdrop = synthetic_pcx(768, 128, &[3; 768 * 128], &[0; 768]);
        std::fs::write(dir.join(name), backdrop).unwrap();
    }
    for i in 1..=5 {
        std::fs::write(dir.join(format!("layer{i}.fgd")), [100; 1920]).unwrap();
    }
    // The screens are still empty.
    let no_screens = run(&["--check"]);
    // Synthetic, not the game's: a black palette and a flat picture for each screen.
    let screen = xeno_formats::fixtures::synthetic_screen(&[0; 768], &[2; 64_000]);
    for name in [
        "login", "maproom", "trainmen", "review", "sitroom", "humancon", "scorpcon", "scoutcon",
    ] {
        std::fs::write(dir.join(format!("{name}.m@l")), &screen).unwrap();
    }
    // And a one-scenario campaign for the log-in, and a map room sprite bank.
    let campaign = synthetic_scendef(&[((0, 0, 9, 9), 1, b"HERE")]);
    std::fs::write(dir.join("scendef.bin"), campaign).unwrap();
    let bank = xeno_formats::fixtures::synthetic_sprite_bank(&[(1, 1, &[5][..]); 20]);
    std::fs::write(dir.join("msprite5.b@n"), &bank).unwrap();
    std::fs::write(dir.join("msprite1.b@n"), &bank).unwrap();
    let (good, drop_alone) = (run(&["--check"]), run(&["--check", "--drop"]));
    let skipped = run(&["--check", "--no-intro"]);
    std::fs::remove_dir_all(&dir).unwrap();

    let stderr = String::from_utf8_lossy(&empty.stderr);
    assert!(!empty.status.success());
    assert!(
        stderr.contains("cannot play the intro") && stderr.contains("--no-intro"),
        "{stderr}"
    );
    let stderr = String::from_utf8_lossy(&no_drop.stderr);
    assert!(!no_drop.status.success());
    assert!(stderr.contains("cannot play the intro drop"), "{stderr}");
    let stderr = String::from_utf8_lossy(&no_screens.stderr);
    assert!(!no_screens.status.success());
    assert!(stderr.contains("cannot show the screens"), "{stderr}");
    for output in [card, skipped, good, drop_alone] {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{output:?}");
        assert!(stdout.contains("Game files found in"), "{stdout}");
    }
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
