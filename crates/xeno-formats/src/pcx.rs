//! ZSoft PCX images, version 3.0 with 8 bits per pixel, run-length encoding and a
//! 256-colour palette after the pixel data. The game ships two (`cavedrop.pcx`,
//! `xenodrop.pcx`); see `docs/formats/pcx.md`.

use binrw::{BinRead, binrw};

use crate::{FormatError, Palette};

/// The fixed 128-byte header at the start of every PCX file.
#[binrw]
#[brw(little, magic = 0x0au8)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcxHeader {
    /// 5 for PCX 3.0, the only version with a 256-colour palette trailer.
    pub version: u8,
    /// 1 for run-length encoding, the only encoding PCX defines.
    pub encoding: u8,
    pub bits_per_pixel: u8,
    pub x_min: u16,
    pub y_min: u16,
    /// Inclusive, so the width is `x_max - x_min + 1`.
    pub x_max: u16,
    pub y_max: u16,
    pub h_dpi: u16,
    pub v_dpi: u16,
    /// 16-colour palette for images of 4 bits or fewer; unused at 8 bits.
    pub ega_palette: [u8; 48],
    pub reserved_40: u8,
    pub planes: u8,
    /// Decoded bytes per scanline per plane. At least the width; may be padded.
    pub bytes_per_line: u16,
    pub palette_info: u16,
    pub h_screen_size: u16,
    pub v_screen_size: u16,
    pub reserved_4a: [u8; 54],
}

impl PcxHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 128;

    pub fn width(&self) -> usize {
        usize::from(self.x_max) - usize::from(self.x_min) + 1
    }

    pub fn height(&self) -> usize {
        usize::from(self.y_max) - usize::from(self.y_min) + 1
    }
}

/// A decoded 8-bit PCX image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcx {
    pub header: PcxHeader,
    /// `width * height` palette indices, row by row, without scanline padding.
    pub pixels: Vec<u8>,
    pub palette: Palette,
}

impl Pcx {
    /// Byte that introduces the palette trailer, 769 bytes before the end of the file.
    pub const PALETTE_MARKER: u8 = 0x0c;

    pub fn width(&self) -> usize {
        self.header.width()
    }

    pub fn height(&self) -> usize {
        self.header.height()
    }
}

/// Decodes a PCX 3.0 file with one 8-bit plane and a 256-colour palette trailer.
pub fn parse(bytes: &[u8]) -> Result<Pcx, FormatError> {
    let header = PcxHeader::read(&mut std::io::Cursor::new(bytes))?;
    let invalid = |offset: u64, reason: String| FormatError::Invalid { offset, reason };
    let expect = |offset: u64, field: &str, actual: u8, wanted: u8| {
        if actual == wanted {
            Ok(())
        } else {
            Err(invalid(
                offset,
                format!("{field} is {actual}, expected {wanted}"),
            ))
        }
    };
    expect(0x01, "version", header.version, 5)?;
    expect(0x02, "encoding", header.encoding, 1)?;
    expect(0x03, "bits per pixel", header.bits_per_pixel, 8)?;
    expect(0x41, "plane count", header.planes, 1)?;
    if header.x_max < header.x_min || header.y_max < header.y_min {
        return Err(invalid(0x04, "image window is inverted".into()));
    }
    let (width, height) = (header.width(), header.height());
    let stride = usize::from(header.bytes_per_line);
    if stride < width {
        return Err(invalid(
            0x42,
            format!("{stride} bytes per line is less than the width {width}"),
        ));
    }

    let Some(trailer_start) = bytes.len().checked_sub(Palette::BYTES + 1) else {
        return Err(invalid(0, "too short for a palette trailer".into()));
    };
    if trailer_start < PcxHeader::SIZE || bytes[trailer_start] != Pcx::PALETTE_MARKER {
        return Err(invalid(
            trailer_start as u64,
            "no 256-colour palette trailer".into(),
        ));
    }
    let palette = Palette::from_rgb8(&bytes[trailer_start + 1..])?;

    let rows =
        decode_rle(&bytes[PcxHeader::SIZE..trailer_start], stride * height).map_err(|at| {
            invalid(
                (PcxHeader::SIZE + at) as u64,
                "pixel data ends early".into(),
            )
        })?;
    let pixels = rows
        .chunks_exact(stride)
        .flat_map(|row| &row[..width])
        .copied()
        .collect();
    Ok(Pcx {
        header,
        pixels,
        palette,
    })
}

/// Expands PCX run-length data until `len` bytes are produced. A byte with its top
/// two bits set repeats the following byte `byte & 0x3f` times; any other byte is a
/// literal. Runs may cross scanline ends, and a run past `len` is cut short.
///
/// On truncated input, returns the offset in `data` where more bytes were needed.
fn decode_rle(data: &[u8], len: usize) -> Result<Vec<u8>, usize> {
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while out.len() < len {
        let &byte = data.get(i).ok_or(i)?;
        i += 1;
        if byte & 0xc0 == 0xc0 {
            let &value = data.get(i).ok_or(i)?;
            i += 1;
            let count = usize::from(byte & 0x3f).min(len - out.len());
            out.resize(out.len() + count, value);
        } else {
            out.push(byte);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_bytes, synthetic_pcx};

    #[test]
    fn synthetic_image_round_trips() {
        for (width, height) in [(1, 1), (2, 3), (7, 5), (320, 200)] {
            let pixels = synthetic_bytes(width * height, width as u64);
            let palette = synthetic_bytes(Palette::BYTES, 9);
            let pcx = parse(&synthetic_pcx(width, height, &pixels, &palette)).unwrap();
            assert_eq!((pcx.width(), pcx.height()), (width, height));
            assert_eq!(pcx.pixels, pixels);
            assert_eq!(pcx.palette, Palette::from_rgb8(&palette).unwrap());
        }
    }

    #[test]
    fn long_runs_and_values_that_need_escaping_round_trip() {
        // A 100-pixel run needs two run bytes; 0xc0..=0xff can't be bare literals.
        let mut pixels = vec![5u8; 100];
        pixels.extend([0xc0, 0xff, 0xbf, 0xc1, 0xc1]);
        let pcx = parse(&synthetic_pcx(105, 1, &pixels, &[0; Palette::BYTES])).unwrap();
        assert_eq!(pcx.pixels, pixels);
    }

    #[test]
    fn header_snapshot() {
        let pcx = synthetic_pcx(3, 2, &[1, 2, 3, 4, 5, 6], &[0; Palette::BYTES]);
        insta::assert_debug_snapshot!(parse(&pcx).unwrap().header);
    }

    #[test]
    fn runs_crossing_scanlines_and_padding_are_handled() {
        // 3x2 image, 4 bytes per line: one run of 8 fills both padded rows.
        let mut bytes = synthetic_pcx(3, 2, &[0; 6], &[0; Palette::BYTES]);
        bytes[0x42] = 4;
        let trailer = bytes.split_off(bytes.len() - Palette::BYTES - 1);
        bytes.truncate(PcxHeader::SIZE);
        bytes.extend([0xc8, 7]);
        bytes.extend(trailer);
        assert_eq!(parse(&bytes).unwrap().pixels, [7; 6]);
    }

    #[test]
    fn truncated_pixel_data_is_reported_where_it_ends() {
        let mut bytes = synthetic_pcx(4, 4, &synthetic_bytes(16, 1), &[0; Palette::BYTES]);
        let trailer = bytes.split_off(bytes.len() - Palette::BYTES - 1);
        bytes.truncate(PcxHeader::SIZE + 3);
        bytes.extend(trailer);
        match parse(&bytes) {
            Err(FormatError::Invalid { offset, reason }) => {
                assert!(offset >= PcxHeader::SIZE as u64, "{offset}");
                assert!(reason.contains("ends early"), "{reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn missing_palette_trailer_is_rejected() {
        let mut bytes = synthetic_pcx(2, 2, &[1, 2, 3, 4], &[0; Palette::BYTES]);
        let marker = bytes.len() - Palette::BYTES - 1;
        bytes[marker] = 0;
        assert!(matches!(parse(&bytes), Err(FormatError::Invalid { .. })));
        assert!(matches!(
            parse(&bytes[..PcxHeader::SIZE]),
            Err(FormatError::Invalid { .. })
        ));
    }

    #[test]
    fn unsupported_headers_are_rejected_with_their_offset() {
        let good = synthetic_pcx(2, 2, &[1, 2, 3, 4], &[0; Palette::BYTES]);
        for (offset, value) in [(0x01, 4), (0x02, 0), (0x03, 4), (0x41, 3)] {
            let mut bytes = good.clone();
            bytes[offset] = value;
            match parse(&bytes) {
                Err(FormatError::Invalid { offset: at, .. }) => assert_eq!(at, offset as u64),
                other => panic!("expected Invalid at {offset:#x}, got {other:?}"),
            }
        }
        let mut bad_magic = good;
        bad_magic[0] = 0;
        assert!(matches!(parse(&bad_magic), Err(FormatError::Binrw(_))));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_pcx_files_decode() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for (name, size) in [("cavedrop.pcx", (768, 128)), ("xenodrop.pcx", (768, 130))] {
            let path = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.file_name().unwrap().eq_ignore_ascii_case(name))
                .unwrap_or_else(|| panic!("{name} not in {}", dir.display()));
            let pcx = parse(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!((pcx.width(), pcx.height()), size, "{name}");
        }
    }
}
