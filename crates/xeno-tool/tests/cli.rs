//! Runs the built binary on synthetic files.

use std::path::PathBuf;
use std::process::Command;

use xeno_formats::fixtures::{synthetic_bytes, synthetic_pcx};

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("xeno-tool-cli-{}-{name}", std::process::id()))
}

#[test]
fn inspect_prints_size_extension_and_preview() {
    let path = temp_path("sample.usg");
    std::fs::write(&path, synthetic_bytes(300, 3)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xeno-tool"))
        .arg("inspect")
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("size:      300 bytes (0x12c)\n"),
        "{stdout}"
    );
    assert!(stdout.contains("extension: usg\n"), "{stdout}");
    assert!(stdout.contains("first 64 bytes:\n00000000  "), "{stdout}");
}

#[test]
fn inspect_of_a_missing_file_fails_and_names_it() {
    let path = temp_path("missing.vcl");
    let output = Command::new(env!("CARGO_BIN_EXE_xeno-tool"))
        .arg("inspect")
        .arg(&path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
}

#[test]
fn dump_writes_a_png_into_the_out_dir() {
    let path = temp_path("drop.pcx");
    let out = temp_path("dump-out");
    let pixels = synthetic_bytes(6, 1);
    std::fs::write(
        &path,
        synthetic_pcx(3, 2, &pixels, &synthetic_bytes(768, 2)),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xeno-tool"))
        .arg("dump")
        .arg(&path)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let png = out.join(format!("xeno-tool-cli-{}-drop.png", std::process::id()));
    let written = std::fs::read(&png);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&out);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&png.display().to_string()), "{stdout}");
    assert!(written.unwrap().starts_with(b"\x89PNG"));
}
