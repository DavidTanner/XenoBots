//! `.b@n` banks: packed `.BIN` files (see `crate::packed`). Two kinds ship with the
//! game: sprite banks (`msprite0`..`msprite5`, the game's `MSPRITES.BIN` with the last
//! letter of the name replaced by the bank's digit), a list of 8-bit images, and sky
//! strips (`sky_a`..`sky_h`, named by a scenario's `BGND` chunk), one bare 342x54 image
//! each. Neither carries a palette. See `docs/formats/b@n.md`.

use binrw::{BinRead, binrw};

use crate::{FormatError, packed};

/// The six bytes in front of each sprite's pixels.
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteHeader {
    /// Bytes in this record, header included. 0 ends the bank.
    pub size: u16,
    pub width: u8,
    pub height: u8,
    /// [`SpriteHeader::ROW_SPANS`] (2): pixels are stored as row spans; 0: raw. The
    /// game's blitters test for exactly 2 and treat any other value as raw.
    pub flags: u8,
    /// 8 in every record of a stock install (perhaps bits per pixel). Nothing in the
    /// game reads it: the blitters use only the size, width, height and flags.
    pub unknown_05: u8,
}

impl SpriteHeader {
    /// Size of the header in bytes.
    pub const SIZE: usize = 6;
    /// Flag bit: each row is a left skip byte, a count byte and `count` pixels,
    /// instead of `width * height` raw pixels.
    pub const ROW_SPANS: u8 = 0x02;

    pub fn has_row_spans(&self) -> bool {
        self.flags & Self::ROW_SPANS != 0
    }
}

/// One image from a sprite bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub header: SpriteHeader,
    /// `width * height` palette indices, row by row. Pixels outside a row span are 0.
    pub pixels: Vec<u8>,
    /// Row spans only: `(skip, count)` for each row. Pixels outside a row's span are
    /// never drawn, while a 0 inside a span is drawn by the opaque blitters, so the
    /// renderer needs the spans as well as the pixels. Empty for raw sprites.
    pub spans: Vec<(u8, u8)>,
    /// Row spans only: the rows are followed by one more, empty span (`0, 0`), as if
    /// the encoder wrote `height + 1` rows. The game never reads it (`size` skips it).
    pub trailing_empty_span: bool,
}

impl Sprite {
    pub fn width(&self) -> usize {
        self.header.width.into()
    }

    pub fn height(&self) -> usize {
        self.header.height.into()
    }

    /// Whether the game's blitters write pixel (`x`, `y`): every pixel of a raw
    /// sprite, and only the pixels inside the row's span of a row-span sprite.
    pub fn covers(&self, x: usize, y: usize) -> bool {
        if !self.header.has_row_spans() {
            return x < self.width() && y < self.height();
        }
        self.spans.get(y).is_some_and(|&(skip, count)| {
            (usize::from(skip)..usize::from(skip) + usize::from(count)).contains(&x)
        })
    }
}

/// A decoded sprite bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteBank {
    /// 0 on disk. The game's loader writes here the sprite directory offset of the
    /// bank's first sprite (8 bytes per sprite), which it was asked to load the bank at.
    pub runtime_first_handle: u16,
    /// 0 on disk. The game's loader writes the number of sprites here.
    pub runtime_count: u16,
    pub sprites: Vec<Sprite>,
}

/// A decoded sky strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sky {
    /// `WIDTH * HEIGHT` palette indices, row by row.
    pub pixels: Vec<u8>,
}

impl Sky {
    pub const WIDTH: usize = 342;
    pub const HEIGHT: usize = 54;
}

/// Decodes a packed sprite bank. Error offsets are into the decompressed data.
pub fn parse_sprites(bytes: &[u8]) -> Result<SpriteBank, FormatError> {
    let data = packed::decompress(bytes)?;
    let invalid = |offset: usize, reason: String| FormatError::Invalid {
        offset: offset as u64,
        reason,
    };

    let mut cursor = std::io::Cursor::new(&data);
    let runtime_first_handle = u16::read_le(&mut cursor)?;
    let runtime_count = u16::read_le(&mut cursor)?;
    let mut sprites = Vec::new();
    loop {
        let at = cursor.position() as usize;
        let size = u16::read_le(&mut cursor)?;
        if size == 0 {
            break;
        }
        cursor.set_position(at as u64);
        let header = SpriteHeader::read(&mut cursor)?;
        let body = data
            .get(at + SpriteHeader::SIZE..at + usize::from(size))
            .ok_or_else(|| invalid(at, format!("record of {size} bytes runs past the end")))?;
        if header.flags & !SpriteHeader::ROW_SPANS != 0 {
            return Err(invalid(
                at + 4,
                format!("unknown sprite flags {:#04x}", header.flags),
            ));
        }
        let (width, height) = (usize::from(header.width), usize::from(header.height));
        let (pixels, spans, trailing_empty_span) = if header.has_row_spans() {
            row_spans(body, width, height).map_err(|reason| invalid(at, reason))?
        } else if body.len() == width * height {
            (body.to_vec(), Vec::new(), false)
        } else {
            return Err(invalid(
                at,
                format!(
                    "{width}x{height} sprite needs {} pixel bytes, record has {}",
                    width * height,
                    body.len()
                ),
            ));
        };
        sprites.push(Sprite {
            header,
            pixels,
            spans,
            trailing_empty_span,
        });
        cursor.set_position((at + usize::from(size)) as u64);
    }

    let end = cursor.position() as usize;
    if end != data.len() {
        return Err(invalid(
            end,
            format!("{} bytes after the end of the bank", data.len() - end),
        ));
    }
    Ok(SpriteBank {
        runtime_first_handle,
        runtime_count,
        sprites,
    })
}

/// Pixels, `(skip, count)` per row, and whether an empty span follows the last row.
type RowSpans = (Vec<u8>, Vec<(u8, u8)>, bool);

/// Expands row spans: per row, a left skip, a count, then `count` pixels. The spans
/// must use the whole body, apart from an optional empty span after the last row,
/// which is reported in the returned flag.
fn row_spans(body: &[u8], width: usize, height: usize) -> Result<RowSpans, String> {
    let mut pixels = vec![0; width * height];
    let mut spans = Vec::with_capacity(height);
    let mut i = 0;
    for y in 0..height {
        let (Some(&skip), Some(&count)) = (body.get(i), body.get(i + 1)) else {
            return Err("row spans end early".into());
        };
        spans.push((skip, count));
        let (skip, count) = (usize::from(skip), usize::from(count));
        if skip + count > width {
            return Err(format!(
                "row span {skip}+{count} is wider than the sprite ({width})"
            ));
        }
        let span = body
            .get(i + 2..i + 2 + count)
            .ok_or("row spans end early")?;
        let row = y * width + skip;
        pixels[row..row + count].copy_from_slice(span);
        i += 2 + count;
    }
    match &body[i..] {
        [] => Ok((pixels, spans, false)),
        [0, 0] => Ok((pixels, spans, true)),
        rest => Err(format!("{} bytes after the last row span", rest.len())),
    }
}

/// Decodes a packed sky strip.
pub fn parse_sky(bytes: &[u8]) -> Result<Sky, FormatError> {
    let pixels = packed::decompress(bytes)?;
    if pixels.len() != Sky::WIDTH * Sky::HEIGHT {
        return Err(FormatError::WrongLength {
            expected: Sky::WIDTH * Sky::HEIGHT,
            actual: pixels.len(),
        });
    }
    Ok(Sky { pixels })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_bytes, synthetic_packed, synthetic_sprite_bank};

    /// A bank with one raw 3x2 sprite and one 4x3 sprite in row spans.
    fn mixed_bank() -> Vec<u8> {
        let mut bank = vec![0, 0, 0, 0];
        bank.extend([12, 0, 3, 2, 0, 8, 1, 2, 3, 4, 5, 6]);
        bank.extend([15, 0, 4, 3, SpriteHeader::ROW_SPANS, 8]);
        bank.extend([1, 2, 7, 8, 0, 0, 3, 1, 9]);
        bank.extend([0, 0]);
        bank
    }

    #[test]
    fn raw_and_row_span_sprites_decode() {
        let bank = parse_sprites(&synthetic_packed(&mixed_bank())).unwrap();
        assert_eq!(bank.sprites.len(), 2);
        assert_eq!(bank.sprites[0].pixels, [1, 2, 3, 4, 5, 6]);
        let spans = &bank.sprites[1];
        assert!(spans.header.has_row_spans());
        assert_eq!((spans.width(), spans.height()), (4, 3));
        assert_eq!(spans.pixels, [0, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 9]);
        assert_eq!(spans.spans, [(1, 2), (0, 0), (3, 1)]);
        // Inside a span is drawn, even where the pixel is 0; outside is not.
        assert!(spans.covers(1, 0) && spans.covers(3, 2));
        assert!(!spans.covers(0, 0) && !spans.covers(0, 1) && !spans.covers(2, 2));
        assert!(!spans.covers(0, 3));
        let raw = &bank.sprites[0];
        assert!(raw.spans.is_empty() && raw.covers(2, 1) && !raw.covers(3, 0));
    }

    #[test]
    fn bank_snapshot() {
        insta::assert_debug_snapshot!(parse_sprites(&synthetic_packed(&mixed_bank())).unwrap());
    }

    #[test]
    fn an_empty_span_after_the_last_row_is_recorded() {
        let mut bank = mixed_bank();
        bank[16] += 2;
        bank.splice(31..31, [0, 0]);
        let sprites = parse_sprites(&synthetic_packed(&bank)).unwrap().sprites;
        assert!(sprites[1].trailing_empty_span);
        assert!(!sprites[0].trailing_empty_span);
        assert_eq!(sprites[1].pixels[11], 9);

        bank[16] += 2;
        bank.splice(31..31, [0, 0]);
        match parse_sprites(&synthetic_packed(&bank)) {
            Err(FormatError::Invalid { reason, .. }) => {
                assert!(
                    reason.contains("4 bytes after the last row span"),
                    "{reason}"
                )
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn synthetic_bank_round_trips() {
        let big = synthetic_bytes(40 * 30, 1);
        let sprites: [(u8, u8, &[u8]); 3] = [(1, 1, &[5]), (40, 30, &big), (0, 0, &[])];
        let bank = parse_sprites(&synthetic_sprite_bank(&sprites)).unwrap();
        assert_eq!((bank.runtime_first_handle, bank.runtime_count), (0, 0));
        for (sprite, (width, height, pixels)) in bank.sprites.iter().zip(sprites) {
            assert_eq!((sprite.header.width, sprite.header.height), (width, height));
            assert_eq!(sprite.pixels, pixels);
        }
    }

    #[test]
    fn malformed_banks_are_rejected_with_their_offset() {
        let good = mixed_bank();
        let cases: [(usize, u8, u64, &str); 5] = [
            (4, 13, 4, "needs 6 pixel bytes, record has 7"),
            (4, 200, 4, "runs past the end"),
            (20, 0x03, 20, "unknown sprite flags 0x03"),
            (22, 3, 16, "wider than the sprite"),
            (16, 14, 16, "row spans end early"),
        ];
        for (index, value, offset, reason) in cases {
            let mut bank = good.clone();
            bank[index] = value;
            match parse_sprites(&synthetic_packed(&bank)) {
                Err(FormatError::Invalid {
                    offset: at,
                    reason: r,
                }) => {
                    assert_eq!(at, offset, "{r}");
                    assert!(r.contains(reason), "{r}");
                }
                other => panic!("expected Invalid for byte {index} = {value}, got {other:?}"),
            }
        }

        let mut trailing = good.clone();
        trailing.push(0);
        assert!(matches!(
            parse_sprites(&synthetic_packed(&trailing)),
            Err(FormatError::Invalid { offset: 33, .. })
        ));
        let mut unterminated = good;
        unterminated.truncate(unterminated.len() - 2);
        assert!(matches!(
            parse_sprites(&synthetic_packed(&unterminated)),
            Err(FormatError::Binrw(_))
        ));
    }

    #[test]
    fn sky_needs_exactly_one_strip() {
        let pixels = synthetic_bytes(Sky::WIDTH * Sky::HEIGHT, 4);
        assert_eq!(
            parse_sky(&synthetic_packed(&pixels)).unwrap().pixels,
            pixels
        );
        assert!(matches!(
            parse_sky(&synthetic_packed(&pixels[1..])),
            Err(FormatError::WrongLength { .. })
        ));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_bank_decodes() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let mut banks: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("b@n")))
            .collect();
        banks.sort();
        assert_eq!(banks.len(), 14, "a stock install has 14 .b@n files");
        let mut sprites = 0;
        for path in banks {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            let bytes = std::fs::read(&path).unwrap();
            if name.starts_with("sky") {
                parse_sky(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            } else {
                let bank = parse_sprites(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(!bank.sprites.is_empty(), "{name}");
                assert_eq!((bank.runtime_first_handle, bank.runtime_count), (0, 0));
                sprites += bank.sprites.len();
            }
        }
        // The game's sprite directory holds 390 entries, all six banks at once.
        assert_eq!(sprites, 390);
    }
}
