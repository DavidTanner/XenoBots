//! LZEXE 0.91 decompression, so `xeno.exe`'s data (the cutscene sequence entries,
//! `sequence`) can be read from the user's own copy at run time. Written from the
//! published description of the format (`docs/re-setup.md`, "LZEXE"); no code is
//! taken from any unpacker.
//!
//! A packed file is an MZ executable whose code is a small loader. The loader's segment
//! starts with the original entry point and stack, the size of the compressed image
//! in paragraphs, and (at loader offset 0x158) the packed relocation table. The
//! compressed image sits in the paragraphs just before the loader. It is one stream
//! of 16-bit flag words, read lowest bit first, interleaved with the bytes the flags
//! describe: a literal byte, or a copy of earlier output.

use binrw::{BinRead, binread};

use crate::FormatError;

/// The MZ header fields LZEXE uses. Offsets are into the file.
#[binread]
#[br(little, magic = b"MZ")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MzHeader {
    pub last_page_bytes: u16,
    pub pages: u16,
    pub relocation_count: u16,
    /// Header size in 16-byte paragraphs: the load image starts after it.
    pub header_paragraphs: u16,
    pub min_alloc: u16,
    pub max_alloc: u16,
    pub ss: u16,
    pub sp: u16,
    pub checksum: u16,
    pub ip: u16,
    /// The loader's segment, relative to the load image.
    pub cs: u16,
    pub relocation_offset: u16,
    pub overlay: u16,
    /// `LZ91` for LZEXE 0.91, `LZ09` for 0.90, at file offset 0x1c.
    pub signature: [u8; 4],
}

/// The record at the start of the loader's segment.
#[binread]
#[br(little)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderInfo {
    /// The original entry point and stack.
    pub ip: u16,
    pub cs: u16,
    pub sp: u16,
    pub ss: u16,
    /// Paragraphs of compressed image, ending where the loader starts.
    pub compressed_paragraphs: u16,
    /// Extra paragraphs the loader asks for (unlzexe takes it off the minimum
    /// allocation; not needed to decompress).
    pub unknown_0a: u16,
    /// The loader's size in bytes.
    pub loader_size: u16,
}

/// One relocation: the image word at `segment * 16 + offset` holds a segment, to which
/// DOS adds the load segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relocation {
    pub segment: u16,
    pub offset: u16,
}

impl Relocation {
    /// Byte offset of the relocated word in the load image.
    pub fn linear(self) -> usize {
        usize::from(self.segment) * 16 + usize::from(self.offset)
    }
}

/// An unpacked executable: its load image as DOS would see it before relocation (load
/// segment 0), and the header fields of the original program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    pub image: Vec<u8>,
    pub relocations: Vec<Relocation>,
    pub cs: u16,
    pub ip: u16,
    pub ss: u16,
    pub sp: u16,
}

impl Unpacked {
    /// The size of the MZ file unlzexe writes for it: a 0x1c-byte header and the
    /// relocation table, padded to a multiple of 512 bytes, then the image.
    pub fn exe_size(&self) -> usize {
        (0x1c + 4 * self.relocations.len()).next_multiple_of(512) + self.image.len()
    }

    /// Byte offset of `segment:offset` in the image.
    pub fn linear(segment: u16, offset: u16) -> usize {
        usize::from(segment) * 16 + usize::from(offset)
    }
}

/// Where the relocation table starts in an LZEXE 0.91 loader.
const RELOCATIONS_IN_LOADER: usize = 0x158;
/// No real-mode program is larger than the 1 MB address space.
const MAX_IMAGE: usize = 0x10_0000;

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// Whether `bytes` is an LZEXE 0.91 executable.
pub fn is_packed(bytes: &[u8]) -> bool {
    bytes.starts_with(b"MZ") && bytes.get(0x1c..0x20) == Some(b"LZ91")
}

/// Unpacks an LZEXE 0.91 executable.
pub fn unpack(bytes: &[u8]) -> Result<Unpacked, FormatError> {
    let header = MzHeader::read(&mut std::io::Cursor::new(bytes))?;
    match &header.signature {
        b"LZ91" => {}
        b"LZ09" => return Err(invalid(0x1c, "LZEXE 0.90 is not supported, only 0.91")),
        _ => return Err(invalid(0x1c, "not an LZEXE 0.91 file (no LZ91 signature)")),
    }
    let loader = (usize::from(header.header_paragraphs) + usize::from(header.cs)) * 16;
    let info_bytes = bytes
        .get(loader..)
        .ok_or_else(|| invalid(loader, "the loader is past the end of the file"))?;
    let info = LoaderInfo::read(&mut std::io::Cursor::new(info_bytes))?;
    let start = (usize::from(header.header_paragraphs) * 16)
        .checked_add(usize::from(header.cs) * 16)
        .and_then(|end| end.checked_sub(usize::from(info.compressed_paragraphs) * 16))
        .filter(|&s| s >= usize::from(header.header_paragraphs) * 16)
        .ok_or_else(|| invalid(loader, "the compressed image starts before the load image"))?;
    let image = decompress(&bytes[start..loader]).map_err(|e| match e {
        FormatError::Invalid { offset, reason } => invalid(start + offset as usize, reason),
        other => other,
    })?;
    let relocations = relocations(bytes, loader + RELOCATIONS_IN_LOADER)?;
    Ok(Unpacked {
        image,
        relocations,
        cs: info.cs,
        ip: info.ip,
        ss: info.ss,
        sp: info.sp,
    })
}

/// Reads the compressed stream: bytes, and 16-bit flag words whose bits are taken
/// lowest first. A new flag word is read as soon as the last bit of the old one is
/// taken, which is where the format puts it.
struct Stream<'a> {
    data: &'a [u8],
    pos: usize,
    flags: u16,
    left: u8,
}

impl<'a> Stream<'a> {
    fn new(data: &'a [u8]) -> Result<Self, FormatError> {
        let mut s = Stream {
            data,
            pos: 0,
            flags: 0,
            left: 16,
        };
        s.flags = s.word()?;
        Ok(s)
    }

    fn byte(&mut self) -> Result<u8, FormatError> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| invalid(self.pos, "compressed image ends before its end mark"))?;
        self.pos += 1;
        Ok(b)
    }

    fn word(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes([self.byte()?, self.byte()?]))
    }

    fn bit(&mut self) -> Result<bool, FormatError> {
        let bit = self.flags & 1 == 1;
        self.left -= 1;
        if self.left == 0 {
            self.flags = self.word()?;
            self.left = 16;
        } else {
            self.flags >>= 1;
        }
        Ok(bit)
    }
}

/// Decompresses the stream. Flag 1 is a literal byte. Flag 0 is a copy from `distance`
/// bytes back: then flag 0 is a short copy (two flags give the length 2 to 5, a byte
/// the distance 1 to 256), and flag 1 a long one (two bytes give a 13-bit distance,
/// 1 to 8192, and a 3-bit length 3 to 9; length bits 0 mean a length byte follows:
/// 0 ends the stream, 1 is a no-op the loader uses to move to the next segment, and
/// anything else is the length minus 1).
fn decompress(data: &[u8]) -> Result<Vec<u8>, FormatError> {
    let mut s = Stream::new(data)?;
    let mut out = Vec::new();
    loop {
        if s.bit()? {
            out.push(s.byte()?);
            continue;
        }
        let at = s.pos;
        let (length, distance) = if !s.bit()? {
            let length = usize::from(s.bit()?) << 1 | usize::from(s.bit()?);
            (length + 2, 256 - usize::from(s.byte()?))
        } else {
            let low = usize::from(s.byte()?);
            let high = usize::from(s.byte()?);
            let distance = 0x2000 - ((high & 0xf8) << 5 | low);
            let length = match high & 7 {
                0 => match s.byte()? {
                    0 => break,
                    1 => continue,
                    n => usize::from(n) + 1,
                },
                n => n + 2,
            };
            (length, distance)
        };
        if distance > out.len() {
            return Err(invalid(
                at,
                format!("a copy reaches {distance} bytes back, before the start"),
            ));
        }
        if out.len() + length > MAX_IMAGE {
            return Err(invalid(at, "the image grows past 1 MB"));
        }
        for _ in 0..length {
            out.push(out[out.len() - distance]);
        }
    }
    Ok(out)
}

/// Reads the packed relocation table at `at`. Each entry is a byte, the distance from
/// the previous relocation; a 0 byte is followed by a word: 0 moves on by 0xfff0
/// bytes, 1 ends the table, and anything else is the distance. Positions are kept
/// normalised, with the offset below 16.
fn relocations(bytes: &[u8], at: usize) -> Result<Vec<Relocation>, FormatError> {
    let mut pos = at;
    let mut next = || -> Result<u8, FormatError> {
        let b = *bytes
            .get(pos)
            .ok_or_else(|| invalid(pos, "relocation table ends before its end mark"))?;
        pos += 1;
        Ok(b)
    };
    let (mut segment, mut offset) = (0u16, 0u16);
    let mut out = Vec::new();
    loop {
        let mut span = u16::from(next()?);
        if span == 0 {
            span = u16::from_le_bytes([next()?, next()?]);
            match span {
                0 => {
                    segment = segment.wrapping_add(0x0fff);
                    continue;
                }
                1 => break,
                _ => {}
            }
        }
        let sum = u32::from(offset) + u32::from(span);
        segment = segment.wrapping_add((sum >> 4) as u16);
        offset = (sum & 0x0f) as u16;
        out.push(Relocation { segment, offset });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a compressed stream from tokens, packing the flag bits into words the way
    /// the format interleaves them: a flag word is written before the bytes that follow
    /// its first bit, and the next one as soon as its sixteenth bit is used.
    #[derive(Default)]
    struct Encoder {
        out: Vec<u8>,
        /// Where the current flag word goes, and its bits so far.
        slot: usize,
        flags: u16,
        used: u8,
    }

    enum Token {
        Literal(u8),
        Short { length: usize, distance: usize },
        Long { length: usize, distance: usize },
        Segment,
        End,
    }

    impl Encoder {
        fn new() -> Self {
            Encoder {
                out: vec![0, 0],
                ..Default::default()
            }
        }

        fn bit(&mut self, bit: bool) {
            self.flags |= u16::from(bit) << self.used;
            self.used += 1;
            if self.used == 16 {
                self.out[self.slot..self.slot + 2].copy_from_slice(&self.flags.to_le_bytes());
                self.slot = self.out.len();
                self.out.extend([0, 0]);
                (self.flags, self.used) = (0, 0);
            }
        }

        fn token(&mut self, token: Token) {
            match token {
                Token::Literal(b) => {
                    self.bit(true);
                    self.out.push(b);
                }
                Token::Short { length, distance } => {
                    self.bit(false);
                    self.bit(false);
                    self.bit((length - 2) & 2 != 0);
                    self.bit((length - 2) & 1 != 0);
                    self.out.push((256 - distance) as u8);
                }
                Token::Long { length, distance } => {
                    self.bit(false);
                    self.bit(true);
                    let field = 0x2000 - distance;
                    let bits = if length <= 9 { length - 2 } else { 0 };
                    self.out.push(field as u8);
                    self.out.push(((field >> 5) & 0xf8) as u8 | bits as u8);
                    if bits == 0 {
                        self.out.push((length - 1) as u8);
                    }
                }
                Token::Segment | Token::End => {
                    self.bit(false);
                    self.bit(true);
                    self.out
                        .extend([0, 0, u8::from(matches!(token, Token::Segment))]);
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            self.out[self.slot..self.slot + 2].copy_from_slice(&self.flags.to_le_bytes());
            self.out
        }
    }

    fn encode(tokens: impl IntoIterator<Item = Token>) -> Vec<u8> {
        let mut e = Encoder::new();
        for t in tokens {
            e.token(t);
        }
        e.finish()
    }

    /// A whole LZEXE 0.91 file: a 2-paragraph header, the compressed image padded to
    /// paragraphs, then the loader with its info record and relocation table.
    fn packed_exe(stream: &[u8], relocation_table: &[u8]) -> Vec<u8> {
        let compressed = stream.len().div_ceil(16);
        let mut file = vec![0u8; 32];
        file[0..2].copy_from_slice(b"MZ");
        file[0x08..0x0a].copy_from_slice(&2u16.to_le_bytes()); // header paragraphs
        file[0x16..0x18].copy_from_slice(&(compressed as u16).to_le_bytes()); // cs
        file[0x14..0x16].copy_from_slice(&0x0eu16.to_le_bytes()); // ip
        file[0x1c..0x20].copy_from_slice(b"LZ91");
        file.extend(stream);
        file.resize(32 + compressed * 16, 0);
        let mut loader = vec![0u8; RELOCATIONS_IN_LOADER];
        for (i, word) in [
            0x0002u16,
            0x0010,
            0x0400,
            0x0020,
            compressed as u16,
            0,
            0x158,
        ]
        .iter()
        .enumerate()
        {
            loader[i * 2..i * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
        loader.extend(relocation_table);
        file.extend(loader);
        file
    }

    #[test]
    fn flag_words_are_read_lowest_bit_first_and_refilled_eagerly() {
        // 16 flags 1010..., then the next word is read on the sixteenth bit.
        let mut s = Stream::new(&[0b0101_0101, 0b0101_0101, 0xff, 0x00]).unwrap();
        let bits: Vec<bool> = (0..16).map(|_| s.bit().unwrap()).collect();
        assert_eq!(bits, [true, false].repeat(8));
        assert_eq!(s.pos, 4);
        assert!(s.bit().unwrap());
        let mut short = Stream::new(&[0, 0]).unwrap();
        for _ in 0..15 {
            short.bit().unwrap();
        }
        assert!(short.bit().is_err());
    }

    #[test]
    fn literals_and_copies_round_trip() {
        let stream = encode([
            Token::Literal(b'a'),
            Token::Literal(b'b'),
            Token::Short {
                length: 5,
                distance: 2,
            }, // ababa: an overlapping copy
            Token::Literal(b'c'),
            Token::Segment,
            Token::Long {
                length: 9,
                distance: 8,
            },
            Token::Long {
                length: 40,
                distance: 3,
            },
            Token::End,
        ]);
        let out = decompress(&stream).unwrap();
        let mut expected = b"abababac".to_vec();
        for _ in 0..49 {
            expected.push(expected[expected.len() - if expected.len() < 17 { 8 } else { 3 }]);
        }
        assert_eq!(out, expected);
    }

    #[test]
    fn long_copies_reach_8192_bytes_back() {
        let mut tokens: Vec<Token> = (0..8192).map(|i| Token::Literal(i as u8)).collect();
        tokens.push(Token::Long {
            length: 3,
            distance: 8192,
        });
        tokens.push(Token::Short {
            length: 2,
            distance: 256,
        });
        tokens.push(Token::End);
        let out = decompress(&encode(tokens)).unwrap();
        assert_eq!(out.len(), 8197);
        assert_eq!(out[8192..], [0, 1, 2, 3, 4]);
    }

    #[test]
    fn bad_streams_are_rejected() {
        let early = encode([
            Token::Literal(1),
            Token::Short {
                length: 2,
                distance: 2,
            },
        ]);
        assert!(
            matches!(decompress(&early), Err(FormatError::Invalid { reason, .. }) if reason.contains("before the start"))
        );
        let unterminated = encode([Token::Literal(1)]);
        assert!(
            matches!(decompress(&unterminated), Err(FormatError::Invalid { reason, .. }) if reason.contains("end mark"))
        );
    }

    #[test]
    fn relocations_are_unpacked_and_normalised() {
        // +0x23, +0x05, then a word span 0x1234, a 0xfff0-byte skip, +1, end.
        let table = [0x23, 0x05, 0, 0x34, 0x12, 0, 0, 0, 0x01, 0, 1, 0];
        let r = relocations(&table, 0).unwrap();
        let linear: Vec<usize> = r.iter().map(|r| r.linear()).collect();
        assert_eq!(linear, [0x23, 0x28, 0x125c, 0x125c + 0xfff0 + 1]);
        assert!(r.iter().all(|r| r.offset < 16));
        assert!(relocations(&table[..4], 0).is_err());
    }

    #[test]
    fn a_synthetic_file_unpacks() {
        let stream = encode([
            Token::Literal(0xb8),
            Token::Literal(0x34),
            Token::Literal(0x12),
            Token::Short {
                length: 4,
                distance: 3,
            },
            Token::End,
        ]);
        let file = packed_exe(&stream, &[0x01, 0x03, 0, 1, 0]);
        assert!(is_packed(&file));
        let unpacked = unpack(&file).unwrap();
        assert_eq!(unpacked.image, [0xb8, 0x34, 0x12, 0xb8, 0x34, 0x12, 0xb8]);
        assert_eq!((unpacked.cs, unpacked.ip), (0x10, 0x02));
        assert_eq!((unpacked.ss, unpacked.sp), (0x20, 0x400));
        assert_eq!(
            unpacked.relocations,
            [
                Relocation {
                    segment: 0,
                    offset: 1
                },
                Relocation {
                    segment: 0,
                    offset: 4
                }
            ]
        );
        // Two relocations fit the minimum 512-byte header.
        assert_eq!(unpacked.exe_size(), 512 + 7);
        insta::assert_debug_snapshot!(unpacked);
    }

    #[test]
    fn headers_are_checked() {
        let stream = encode([Token::End]);
        let mut file = packed_exe(&stream, &[0, 1, 0]);
        assert!(unpack(&file).unwrap().image.is_empty());
        file[0x1c..0x20].copy_from_slice(b"LZ09");
        assert!(!is_packed(&file));
        assert!(
            matches!(unpack(&file), Err(FormatError::Invalid { offset: 0x1c, reason }) if reason.contains("0.90"))
        );
        file[0x1c..0x20].copy_from_slice(b"PKLT");
        assert!(unpack(&file).is_err());
        assert!(matches!(unpack(b"ZM"), Err(FormatError::Binrw(_))));
        // A compressed image larger than everything before the loader.
        let mut file = packed_exe(&stream, &[0, 1, 0]);
        let loader = 32 + 16;
        file[loader + 8..loader + 10].copy_from_slice(&9u16.to_le_bytes());
        assert!(unpack(&file).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_xeno_exe_and_config_dvr_unpack() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        // The documented facts (`docs/re-setup.md`): file size once unpacked,
        // relocations, entry point and stack.
        for (name, size, count, entry, stack) in [
            ("xeno.exe", 181_836, 655, (0, 2), Some((0x27de, 0x400))),
            ("config.dvr", 16_160, 13, (0, 0), None),
        ] {
            let bytes = std::fs::read(dir.join(name)).unwrap();
            assert!(is_packed(&bytes), "{name}");
            let u = unpack(&bytes).unwrap();
            assert_eq!(u.exe_size(), size, "{name}");
            assert_eq!(u.relocations.len(), count, "{name}");
            assert_eq!((u.cs, u.ip), entry, "{name}");
            if let Some(stack) = stack {
                assert_eq!((u.ss, u.sp), stack, "{name}");
            }
            assert!(
                u.relocations
                    .iter()
                    .all(|r| r.linear() + 2 <= u.image.len())
            );
        }
    }
}
