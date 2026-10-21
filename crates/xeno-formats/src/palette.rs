//! The 256-colour palette that every indexed image in the game is drawn with.

use std::ops::Index;

use crate::FormatError;

/// One palette entry, 8 bits per channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// 256 colours, indexed by the pixel values of the game's 8-bit images.
///
/// The game stores palettes in two encodings: 8-bit triples in a PCX file's trailer,
/// and the VGA DAC's 6-bit triples everywhere else (`.vcl` files, the screens). See
/// `docs/formats/palette.md` for every source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub colors: [Rgb; 256],
}

impl Palette {
    /// Number of colours.
    pub const LEN: usize = 256;

    /// Size of a palette stored as one byte per channel.
    pub const BYTES: usize = Self::LEN * 3;

    /// Reads 256 `r, g, b` triples with 8 bits per channel.
    pub fn from_rgb8(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::from_triples(bytes, Ok)
    }

    /// Reads 256 `r, g, b` triples in the VGA DAC's 6-bit range (0..=63) and widens
    /// them to 8 bits, so 0 stays 0 and 63 becomes 255.
    pub fn from_vga6(bytes: &[u8]) -> Result<Self, FormatError> {
        Self::from_triples(bytes, |c| {
            if c > 63 {
                Err(format!("VGA DAC value {c} is above 63"))
            } else {
                Ok((c << 2) | (c >> 4))
            }
        })
    }

    fn from_triples(
        bytes: &[u8],
        channel: impl Fn(u8) -> Result<u8, String>,
    ) -> Result<Self, FormatError> {
        if bytes.len() != Self::BYTES {
            return Err(FormatError::WrongLength {
                expected: Self::BYTES,
                actual: bytes.len(),
            });
        }
        let mut colors = [Rgb::default(); Self::LEN];
        for (i, (color, &[r, g, b])) in colors.iter_mut().zip(bytes.as_chunks().0).enumerate() {
            let read = |k: usize, value| {
                channel(value).map_err(|reason| FormatError::Invalid {
                    offset: (i * 3 + k) as u64,
                    reason,
                })
            };
            *color = Rgb {
                r: read(0, r)?,
                g: read(1, g)?,
                b: read(2, b)?,
            };
        }
        Ok(Palette { colors })
    }
}

impl Index<u8> for Palette {
    type Output = Rgb;

    fn index(&self, index: u8) -> &Rgb {
        &self.colors[usize::from(index)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::synthetic_bytes;

    #[test]
    fn rgb8_keeps_every_byte_in_order() {
        let bytes = synthetic_bytes(Palette::BYTES, 1);
        let palette = Palette::from_rgb8(&bytes).unwrap();
        for (i, &[r, g, b]) in bytes.as_chunks().0.iter().enumerate() {
            let expected = Rgb { r, g, b };
            assert_eq!(palette[i as u8], expected);
        }
    }

    #[test]
    fn vga6_widens_to_the_full_8_bit_range() {
        let mut bytes = vec![0u8; Palette::BYTES];
        bytes[3..6].copy_from_slice(&[63, 32, 1]);
        let palette = Palette::from_vga6(&bytes).unwrap();
        assert_eq!(palette[0], Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            palette[1],
            Rgb {
                r: 255,
                g: 130,
                b: 4
            }
        );
    }

    #[test]
    fn vga6_rejects_values_above_63_with_their_offset() {
        let mut bytes: Vec<u8> = synthetic_bytes(Palette::BYTES, 2)
            .into_iter()
            .map(|b| b & 0x3f)
            .collect();
        assert!(Palette::from_vga6(&bytes).is_ok());
        bytes[100] = 64;
        match Palette::from_vga6(&bytes) {
            Err(FormatError::Invalid { offset, .. }) => assert_eq!(offset, 100),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn wrong_length_is_reported() {
        for len in [0, Palette::BYTES - 1, Palette::BYTES + 1] {
            match Palette::from_rgb8(&vec![0; len]) {
                Err(FormatError::WrongLength { expected, actual }) => {
                    assert_eq!((expected, actual), (Palette::BYTES, len));
                }
                other => panic!("expected WrongLength, got {other:?}"),
            }
        }
    }
}
