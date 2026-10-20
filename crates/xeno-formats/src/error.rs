//! The error type shared by every parser in this crate.

/// Why some bytes could not be parsed as the format that was asked for.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FormatError {
    /// The input has the wrong size for a fixed-size structure.
    #[error("expected {expected} bytes, found {actual}")]
    WrongLength { expected: usize, actual: usize },

    /// A field holds a value the format does not allow.
    #[error("invalid data at offset {offset:#x}: {reason}")]
    Invalid { offset: u64, reason: String },

    /// A `binrw` struct failed to read: short input, a bad magic number, or a failed
    /// assertion.
    #[error(transparent)]
    Binrw(#[from] binrw::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_sizes_and_offsets() {
        let e = FormatError::WrongLength {
            expected: 768,
            actual: 10,
        };
        assert_eq!(e.to_string(), "expected 768 bytes, found 10");

        let e = FormatError::Invalid {
            offset: 0x2a,
            reason: "too big".into(),
        };
        assert_eq!(e.to_string(), "invalid data at offset 0x2a: too big");
    }

    #[test]
    fn binrw_errors_convert() {
        use binrw::BinRead;
        let mut short = std::io::Cursor::new([0u8; 1]);
        let e: FormatError = u32::read_le(&mut short).unwrap_err().into();
        assert!(matches!(e, FormatError::Binrw(_)));
    }
}
