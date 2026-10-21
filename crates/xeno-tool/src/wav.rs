//! A minimal RIFF WAVE writer: mono, unsigned 8-bit PCM, which is what `dsounds.bin`
//! holds. Small enough not to need a crate.

/// A complete `.wav` file holding `samples` (unsigned 8-bit, silence at 0x80) played at
/// `rate` Hz.
pub fn mono_u8(rate: u32, samples: &[u8]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len()).expect("sound fits in a WAV");
    let mut out = Vec::with_capacity(44 + samples.len() + 1);
    out.extend(b"RIFF");
    // RIFF chunks are padded to an even length; the pad byte counts in the RIFF size.
    out.extend((36 + data_len + (data_len & 1)).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes()); // fmt chunk size
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // channels
    out.extend(rate.to_le_bytes());
    out.extend(rate.to_le_bytes()); // bytes per second
    out.extend(1u16.to_le_bytes()); // block align
    out.extend(8u16.to_le_bytes()); // bits per sample
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    out.extend(samples);
    if data_len & 1 == 1 {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_describes_mono_u8_pcm() {
        let wav = mono_u8(7231, &[0x80, 0xff, 0x00]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 40);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 7231);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 3);
        assert_eq!(wav[44..], [0x80, 0xff, 0x00, 0]);
        assert_eq!(wav.len() - 8, 40);
    }
}
