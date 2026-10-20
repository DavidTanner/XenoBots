//! `inspect`: a generic summary of any file. Formats with a parser will add their own
//! details here as their tasks land.

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
    if shown == 0 {
        return out;
    }
    writeln!(out, "first {shown} bytes:").unwrap();
    out.push_str(&hex_preview(&bytes[..shown]));
    out
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
    use xeno_formats::fixtures::synthetic_bytes;

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
    fn unusual_extensions_are_kept() {
        assert!(report(Path::new("login.M@L"), b"x").contains("extension: m@l\n"));
    }
}
