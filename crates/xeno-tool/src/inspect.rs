//! `inspect`: a generic summary of any file, plus what the parser makes of it for
//! formats that have one (so far: pcx, b@n).

use std::fmt::Write;
use std::path::Path;

/// How many bytes the hex preview shows.
pub const PREVIEW_LEN: usize = 64;

const BYTES_PER_LINE: usize = 16;

/// The report printed for `file` with contents `bytes`.
pub fn report(file: &Path, bytes: &[u8]) -> String {
    let extension = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "(none)".into());
    let shown = bytes.len().min(PREVIEW_LEN);

    let mut out = String::new();
    writeln!(out, "file:      {}", file.display()).unwrap();
    writeln!(out, "size:      {0} bytes ({0:#x})", bytes.len()).unwrap();
    writeln!(out, "extension: {extension}").unwrap();
    match extension.as_str() {
        "pcx" => out.push_str(&pcx_details(bytes)),
        "b@n" => out.push_str(&ban_details(file, bytes)),
        _ => {}
    }
    if shown == 0 {
        return out;
    }
    writeln!(out, "first {shown} bytes:").unwrap();
    out.push_str(&hex_preview(&bytes[..shown]));
    out
}

/// What the PCX parser makes of `bytes`, or why it could not parse them.
fn pcx_details(bytes: &[u8]) -> String {
    match xeno_formats::pcx::parse(bytes) {
        Ok(pcx) => {
            let h = &pcx.header;
            format!(
                "pcx:       version {}, {}x{}, {} bpp, {} plane(s), {} bytes/line, 256-colour palette\n",
                h.version,
                pcx.width(),
                pcx.height(),
                h.bits_per_pixel,
                h.planes,
                h.bytes_per_line,
            )
        }
        Err(e) => format!("pcx:       cannot parse: {e}\n"),
    }
}

/// What the `.b@n` parsers make of `bytes`: a sky strip if the name starts with
/// `sky`, otherwise a sprite bank.
fn ban_details(file: &Path, bytes: &[u8]) -> String {
    use xeno_formats::{ban, packed};

    let decoded = match packed::decompress(bytes) {
        Ok(data) => data.len(),
        Err(e) => return format!("packed:    cannot decompress: {e}\n"),
    };
    let packed = format!("packed:    {decoded} bytes decoded\n");
    let is_sky = file
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("sky"));
    let details = if is_sky {
        match ban::parse_sky(bytes) {
            Ok(_) => format!(
                "b@n:       sky strip, {}x{}",
                ban::Sky::WIDTH,
                ban::Sky::HEIGHT
            ),
            Err(e) => format!("b@n:       cannot parse as a sky strip: {e}"),
        }
    } else {
        match ban::parse_sprites(bytes) {
            Ok(bank) => {
                let spans = bank
                    .sprites
                    .iter()
                    .filter(|s| s.header.has_row_spans())
                    .count();
                format!(
                    "b@n:       sprite bank, {} sprites ({spans} in row spans)",
                    bank.sprites.len()
                )
            }
            Err(e) => format!("b@n:       cannot parse as a sprite bank: {e}"),
        }
    };
    format!("{packed}{details}\n")
}

/// A classic hex dump: offset, 16 bytes in two groups of eight, then the printable
/// ASCII characters. The last line is padded so its ASCII column lines up.
pub fn hex_preview(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(BYTES_PER_LINE).enumerate() {
        write!(out, "{:08x} ", line * BYTES_PER_LINE).unwrap();
        for i in 0..BYTES_PER_LINE {
            if i % 8 == 0 {
                out.push(' ');
            }
            match chunk.get(i) {
                Some(b) => write!(out, "{b:02x} ").unwrap(),
                None => out.push_str("   "),
            }
        }
        out.push_str(" |");
        out.extend(chunk.iter().map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '.'
            }
        }));
        out.push_str("|\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_bytes, synthetic_pcx, synthetic_sprite_bank};

    #[test]
    fn report_of_a_synthetic_file() {
        let mut bytes = synthetic_bytes(100, 7);
        bytes[..12].copy_from_slice(b"XENO test \x00\xff");
        insta::assert_snapshot!(report(Path::new("dir/SCEN00A.USG"), &bytes));
    }

    #[test]
    fn short_file_pads_the_last_line() {
        insta::assert_snapshot!(report(Path::new("orders"), b"Hello, world!\r\n\x01\x02ab"));
    }

    #[test]
    fn empty_file_has_no_preview() {
        let out = report(Path::new("empty.m00"), &[]);
        assert_eq!(
            out,
            "file:      empty.m00\nsize:      0 bytes (0x0)\nextension: m00\n"
        );
    }

    #[test]
    fn preview_stops_at_64_bytes() {
        let out = report(Path::new("big.bin"), &synthetic_bytes(1000, 1));
        assert!(out.contains("first 64 bytes:\n"));
        assert_eq!(out.lines().filter(|l| l.starts_with("000000")).count(), 4);
    }

    #[test]
    fn pcx_files_show_their_header() {
        let bytes = synthetic_pcx(5, 3, &synthetic_bytes(15, 1), &synthetic_bytes(768, 2));
        let out = report(Path::new("drop.pcx"), &bytes);
        assert!(
            out.contains(
                "pcx:       version 5, 5x3, 8 bpp, 1 plane(s), 6 bytes/line, 256-colour palette\n"
            ),
            "{out}"
        );
        let out = report(Path::new("bad.PCX"), &bytes[..10]);
        assert!(out.contains("pcx:       cannot parse: "), "{out}");
    }

    #[test]
    fn sprite_banks_and_sky_strips_are_summarised() {
        let bank = synthetic_sprite_bank(&[(1, 1, &[1]), (2, 2, &[1, 2, 3, 4])]);
        let out = report(Path::new("MSPRITE0.B@N"), &bank);
        assert!(out.contains("packed:    23 bytes decoded\n"), "{out}");
        assert!(
            out.contains("b@n:       sprite bank, 2 sprites (0 in row spans)\n"),
            "{out}"
        );
        let out = report(Path::new("sky_a.b@n"), &bank);
        assert!(
            out.contains("b@n:       cannot parse as a sky strip: "),
            "{out}"
        );
        let out = report(Path::new("sky_a.b@n"), b"\x01");
        assert!(out.contains("packed:    cannot decompress: "), "{out}");
    }

    #[test]
    fn unusual_extensions_are_kept() {
        assert!(report(Path::new("login.M@L"), b"x").contains("extension: m@l\n"));
    }
}
