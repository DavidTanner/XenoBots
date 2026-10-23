//! The intro drop sequence's numbers and lists in `vgaintro.dvr` (`docs/formats/fgd.md`,
//! "The drop sequence"). They are game data, so they are read from the user's own copy
//! at run time and never stored in this repository.
//!
//! `vgaintro.dvr` is a plain MZ file. Its frame script is code: each number (a frame,
//! a speed, a retrace count) is the immediate operand of one instruction. So each is read
//! at its documented load-image offset, after checking that the bytes before it are
//! the instruction expected there, which rejects any other build of the driver instead
//! of misreading it. The colour-animation list, the credits script and the credits text
//! are data and are read where the code points.

use crate::FormatError;
use crate::lzexe::MzHeader;

fn invalid(offset: usize, reason: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        offset: offset as u64,
        reason: reason.into(),
    }
}

/// The value of the immediate at `at` in `image`, of `width` bytes, if the bytes just
/// before it are `op`.
fn immediate(image: &[u8], at: usize, op: &[u8], width: usize) -> Result<u16, FormatError> {
    let start = at
        .checked_sub(op.len())
        .ok_or_else(|| invalid(at, "no room for the instruction"))?;
    match image.get(start..at + width) {
        Some(bytes) if &bytes[..op.len()] == op => Ok(match width {
            1 => u16::from(bytes[op.len()]),
            _ => u16::from_le_bytes([bytes[op.len()], bytes[op.len() + 1]]),
        }),
        Some(_) => Err(invalid(
            at,
            format!("not the expected instruction {op:02x?}"),
        )),
        None => Err(invalid(at, "past the end of the image")),
    }
}

/// Declares [`Numbers`] and [`NUMBERS`] together: each field, its type, the load-image
/// offset of the immediate and the instruction bytes before it.
macro_rules! numbers {
    ($($(#[$doc:meta])* $name:ident: $ty:ident at $at:literal after [$($op:literal),*];)*) => {
        /// The numbers of the drop sequence, each read from one instruction of the driver.
        /// Frames count from 1; speeds are in 1/256 column a frame; counts of the
        /// retrace counter `DS:0x370` run from the start of the music.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Numbers {
            $($(#[$doc])* pub $name: $ty,)*
        }

        /// Where each of [`Numbers`]'s fields is read: name, load-image offset of the
        /// immediate, the instruction bytes before it, and its width in bytes.
        pub const NUMBERS: &[(&str, usize, &[u8], usize)] = &[
            $((stringify!($name), $at, &[$($op),*], std::mem::size_of::<$ty>()),)*
        ];

        impl Numbers {
            fn read(image: &[u8]) -> Result<Numbers, FormatError> {
                Ok(Numbers {
                    $($name: immediate(image, $at, &[$($op),*], std::mem::size_of::<$ty>())?
                        as $ty,)*
                })
            }

            /// Every field in [`NUMBERS`] order, widened to u16.
            pub fn values(&self) -> Vec<u16> {
                vec![$(u16::from(self.$name)),*]
            }
        }
    };
}

numbers! {
    /// Paragraph of the colour-animation list in the image (`mov ax, seg` before slot
    /// `0x30e`, relocated).
    anim_paragraph: u16 at 0x2ac3 after [0xb8];
    /// Paragraph of the driver's data segment, which holds the credits script and text.
    data_paragraph: u16 at 0x3aec after [0xb8];
    /// The view's row on the 768-wide screen: the backdrop's first row.
    view_y: u16 at 0x39f5 after [0xb8];
    /// Rows above the split line (the line compare register; the rest of the screen
    /// shows the top of video memory, the credits area).
    split_row: u16 at 0x39e2 after [0xb8];
    /// The view's first x in each part.
    view_x: u16 at 0x2ae5 after [0x2e, 0xc7, 0x06, 0x73, 0x04];
    cave_view_x: u16 at 0x2cb1 after [0x2e, 0xc7, 0x06, 0x73, 0x04];
    /// Where every layer's 768-column window starts, in each part.
    layer_start: u16 at 0x3a52 after [0x2e, 0xc7, 0x06, 0x0e, 0x21];
    /// Retrace counts: the first blend waits for `start`, the next two for
    /// `second_blend` and `third_blend`, the frames for `frames`; the cave part runs
    /// until `cave_end`.
    start: u16 at 0x2b3c after [0xb8];
    second_blend: u16 at 0x2b7e after [0xb8];
    third_blend: u16 at 0x2bb4 after [0xb8];
    frames: u16 at 0x2bde after [0xb8];
    cave_end: u16 at 0x2cfa after [0x3d];
    /// Retraces a frame takes at least, in each part.
    frame_retraces: u16 at 0x2c3b after [0x3d];
    cave_frame_retraces: u16 at 0x2cf1 after [0x3d];
    /// The four blends: their steps (weights 0, 2, 4, ...) and records.
    blend_steps: u16 at 0x2b47 after [0xb9];
    blend_record: u8 at 0x2b4c after [0xb0];
    second_blend_steps: u16 at 0x2b86 after [0xb9];
    second_blend_record: u8 at 0x2b8b after [0xb0];
    third_blend_steps: u16 at 0x2bbc after [0xb9];
    third_blend_record: u8 at 0x2bc1 after [0xb0];
    last_blend_steps: u16 at 0x2d04 after [0xb9];
    last_blend_record: u8 at 0x2d09 after [0xb0];
    /// After the first two blends, bytes of the list copied into the palette buffer:
    /// from list offset, to buffer offset, length.
    first_copy_from: u16 at 0x2b66 after [0xbe];
    first_copy_to: u16 at 0x2b69 after [0xbf];
    first_copy_len: u16 at 0x2b6c after [0xb9];
    second_copy_from: u16 at 0x2ba5 after [0xbe];
    second_copy_to: u16 at 0x2ba8 after [0xbf];
    second_copy_len: u16 at 0x2bab after [0xb9];
    /// The cave's change of light: a record started at a weight, then list bytes
    /// copied into the palette buffer (`to` is an address: the buffer is at 0x16a).
    light_weight: u8 at 0x3cba after [0xb4];
    light_record: u8 at 0x3cbc after [0xb0];
    light_from: u16 at 0x3cc2 after [0xbe];
    light_to: u16 at 0x3cca after [0xbf];
    light_len: u16 at 0x3cd2 after [0xb9];
    /// The frame script, state 0 (accelerate): until this frame, each speed grows.
    accelerate_until: u8 at 0x3b96 after [0x2e, 0x83, 0x3e, 0x7c, 0x0a];
    accelerate_a: u8 at 0x3b9e after [0x2e, 0x83, 0x06, 0x17, 0x21];
    accelerate_b: u8 at 0x3ba4 after [0x2e, 0x83, 0x06, 0x19, 0x21];
    accelerate_c: u8 at 0x3baa after [0x2e, 0x83, 0x06, 0x1b, 0x21];
    accelerate_view: u8 at 0x3bb0 after [0x2e, 0x83, 0x06, 0x1d, 0x21];
    /// Then the speeds are set to these.
    cruise_a: u16 at 0x3bbd after [0x2e, 0xc7, 0x06, 0x17, 0x21];
    cruise_b: u16 at 0x3bc4 after [0x2e, 0xc7, 0x06, 0x19, 0x21];
    cruise_c: u16 at 0x3bcb after [0x2e, 0xc7, 0x06, 0x1b, 0x21];
    cruise_view: u16 at 0x3bd2 after [0x2e, 0xc7, 0x06, 0x1d, 0x21];
    /// State 1 (cruise): at `slow_from` it moves to state 2; at `first_part_end` it
    /// ends the first part (no step that frame); at `jump_at` the view jumps
    /// `jump_by` columns left and the light changes; at `boost_at` layer B's speed
    /// becomes `boost_b`.
    slow_from: u16 at 0x3be2 after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    first_part_end: u16 at 0x3beb after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    jump_at: u16 at 0x3bfb after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    jump_by: u16 at 0x3c0b after [0x2e, 0x81, 0x2e, 0x73, 0x04];
    boost_at: u16 at 0x3c15 after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    boost_b: u16 at 0x3c1e after [0x2e, 0xc7, 0x06, 0x19, 0x21];
    /// State 2 (slow): until `stop_at`, each speed drops; then all stop.
    stop_at: u16 at 0x3c35 after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    slow_a: u8 at 0x3c3e after [0x2e, 0x83, 0x2e, 0x17, 0x21];
    slow_b: u8 at 0x3c44 after [0x2e, 0x83, 0x2e, 0x19, 0x21];
    slow_c: u8 at 0x3c4a after [0x2e, 0x83, 0x2e, 0x1b, 0x21];
    slow_view: u8 at 0x3c50 after [0x2e, 0x83, 0x2e, 0x1d, 0x21];
    /// States 3 and 4 wait for these frames; state 5 ends the part.
    hold_until: u16 at 0x3c83 after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    hold_again_until: u16 at 0x3c9d after [0x2e, 0x81, 0x3e, 0x7c, 0x0a];
    /// The credits: the script's offset in the data segment, the copyright text shown
    /// after the first blend, the colour record and its weights.
    credits_script: u16 at 0x3af4 after [0xc7, 0x06, 0xb0, 0x00];
    copyright: u16 at 0x2b75 after [0xbe];
    credits_record: u8 at 0x3b33 after [0xb0];
    credits_show_from: u8 at 0x3b28 after [0xc6, 0x06, 0xb2, 0x00];
    credits_step: u8 at 0x3b2d after [0x80, 0x06, 0xb2, 0x00];
    credits_full: u8 at 0x3b3b after [0x80, 0x3e, 0xb2, 0x00];
    credits_hide_from: u8 at 0x3b48 after [0xc6, 0x06, 0xb2, 0x00];
    credits_down: u8 at 0x3b4d after [0x80, 0x2e, 0xb2, 0x00];
}

/// The frame script's state handlers, in the order of the state table at load offset
/// 0x0a82 (accelerate, cruise, slow, two holds, end).
pub const STATE_TABLE: usize = 0x0a82;
pub const STATE_HANDLERS: [u16; 6] = [0x3b91, 0x3bdd, 0x3c30, 0x3c7e, 0x3c98, 0x3cb2];

/// The palette buffer's address in the driver (`light_to` counts from it).
pub const PALETTE_BUFFER: u16 = 0x016a;

/// One step of the credits script, run once a frame (`ui_step_credits_script`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditOp {
    /// Clear the credits area (`0x3b64`).
    Clear,
    /// Wait until the frame count reaches this (`0x3b11`).
    WaitFrame(u16),
    /// Draw the text list at this data-segment offset (`0x3b05`).
    Draw(u16),
    /// Raise the credits colour's weight a step a frame up to full (`0x3b29`).
    FadeIn,
    /// Lower it a step a frame down to 0 (`0x3b49`).
    FadeOut,
}

/// The handlers' code offsets, by [`CreditOp`].
pub const CREDIT_HANDLERS: [(u16, &str); 5] = [
    (0x3b64, "clear"),
    (0x3b11, "wait"),
    (0x3b05, "draw"),
    (0x3b29, "fade in"),
    (0x3b49, "fade out"),
];

/// One string of a text list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextItem {
    pub x: u16,
    pub y: u16,
    pub font: u16,
    pub colour: u8,
    /// The bytes up to the NUL, in the game's font's code page.
    pub text: Vec<u8>,
}

/// What the drop reads from `vgaintro.dvr`.
#[derive(Clone, PartialEq, Eq)]
pub struct IntroDrop {
    pub numbers: Numbers,
    /// The colour-animation list, up to and including its end marker.
    pub anims: Vec<u8>,
    /// The credits script, up to its last wait (one that never ends).
    pub credits: Vec<CreditOp>,
    /// The driver's data segment, for [`IntroDrop::text_list`].
    data: Vec<u8>,
}

/// The numbers and steps in full; the list and the data segment by size.
impl std::fmt::Debug for IntroDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntroDrop")
            .field("numbers", &self.numbers)
            .field("anims", &format_args!("{} bytes", self.anims.len()))
            .field("credits", &self.credits)
            .field("data", &format_args!("{} bytes", self.data.len()))
            .finish()
    }
}

/// A wait for this frame never ends: the credits script's last step.
pub const FOREVER: u16 = 0xffff;
/// More credit steps than this is not a script.
const MAX_CREDIT_OPS: usize = 256;

impl IntroDrop {
    /// The text list at data-segment `offset`: a u16 count, then per string its `x`,
    /// `y`, font (u16 each), colour (u8) and the offset of its NUL-terminated text.
    pub fn text_list(&self, offset: u16) -> Result<Vec<TextItem>, FormatError> {
        let word = |at: usize| -> Result<u16, FormatError> {
            self.data
                .get(at..at + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or_else(|| invalid(at, "text list past the data segment"))
        };
        let at = usize::from(offset);
        (0..usize::from(word(at)?))
            .map(|i| {
                let item = at + 2 + 9 * i;
                let colour = *self
                    .data
                    .get(item + 6)
                    .ok_or_else(|| invalid(item, "text list past the data segment"))?;
                let text = usize::from(word(item + 7)?);
                let bytes = self.data.get(text..).unwrap_or_default();
                let end = bytes
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or_else(|| invalid(text, "text without its NUL"))?;
                Ok(TextItem {
                    x: word(item)?,
                    y: word(item + 2)?,
                    font: word(item + 4)?,
                    colour,
                    text: bytes[..end].to_vec(),
                })
            })
            .collect()
    }
}

/// The load image of a plain (not packed) MZ file: the bytes after its header, up to
/// the size the header gives.
pub fn load_image(bytes: &[u8]) -> Result<&[u8], FormatError> {
    let header = mz_header(bytes)?;
    let size = match header.last_page_bytes {
        0 => usize::from(header.pages) * 512,
        last => usize::from(header.pages).saturating_sub(1) * 512 + usize::from(last),
    };
    let start = usize::from(header.header_paragraphs) * 16;
    bytes
        .get(start..size.min(bytes.len()))
        .filter(|image| !image.is_empty())
        .ok_or_else(|| invalid(0, "the header leaves no load image"))
}

/// Reads the drop's numbers and lists from the bytes of `vgaintro.dvr`.
pub fn parse(bytes: &[u8]) -> Result<IntroDrop, FormatError> {
    let image = load_image(bytes)?;
    let numbers = Numbers::read(image)?;
    for (i, &handler) in STATE_HANDLERS.iter().enumerate() {
        let at = STATE_TABLE + 2 * i;
        if immediate(image, at, &[], 2)? != handler {
            return Err(invalid(at, "not the frame script's state table"));
        }
    }
    let anims = anim_list(image, usize::from(numbers.anim_paragraph) * 16)?;
    let data = image
        .get(usize::from(numbers.data_paragraph) * 16..)
        .ok_or_else(|| invalid(0x3aec, "the data segment is past the image"))?
        .to_vec();
    let credits = credit_ops(&data, usize::from(numbers.credits_script))?;
    Ok(IntroDrop {
        numbers,
        anims,
        credits,
        data,
    })
}

/// The records from `start`, each starting with its size, up to the zero size that
/// ends the list (`palette.md`, "Colour-animation records").
fn anim_list(image: &[u8], start: usize) -> Result<Vec<u8>, FormatError> {
    let mut at = start;
    loop {
        let size = usize::from(immediate(image, at, &[], 2)?);
        if size == 0 {
            return Ok(image[start..at + 2].to_vec());
        }
        if size < 10 {
            return Err(invalid(
                at,
                format!("a {size}-byte colour-animation record"),
            ));
        }
        at += size;
    }
}

/// The credits script from `start`: (handler, argument) word pairs.
fn credit_ops(data: &[u8], start: usize) -> Result<Vec<CreditOp>, FormatError> {
    let mut ops = Vec::new();
    for i in 0..MAX_CREDIT_OPS {
        let at = start + 4 * i;
        let handler = immediate(data, at, &[], 2)?;
        let arg = immediate(data, at + 2, &[], 2)?;
        let op = match CREDIT_HANDLERS.iter().position(|&(h, _)| h == handler) {
            Some(0) => CreditOp::Clear,
            Some(1) => CreditOp::WaitFrame(arg),
            Some(2) => CreditOp::Draw(arg),
            Some(3) => CreditOp::FadeIn,
            Some(4) => CreditOp::FadeOut,
            _ => return Err(invalid(at, format!("unknown credits step {handler:#06x}"))),
        };
        ops.push(op);
        if op == CreditOp::WaitFrame(FOREVER) {
            return Ok(ops);
        }
    }
    Err(invalid(start, "the credits script does not end"))
}

/// The header of any MZ file: LZEXE's four signature bytes are read but not checked.
fn mz_header(bytes: &[u8]) -> Result<MzHeader, FormatError> {
    use binrw::BinRead;
    let mut header = bytes.get(..0x20).unwrap_or(bytes).to_vec();
    header.resize(0x20, 0);
    Ok(MzHeader::read(&mut std::io::Cursor::new(header))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{synthetic_numbers, synthetic_vgaintro};

    #[test]
    fn synthetic_driver_parses() {
        let numbers = synthetic_numbers();
        let file = synthetic_vgaintro(&numbers);
        let drop = parse(&file).unwrap();
        assert_eq!(drop.numbers, numbers);
        // Two records and the end marker.
        assert_eq!(drop.anims.len(), 2 * (10 + 6) + 2);
        assert_eq!(
            drop.credits,
            [
                CreditOp::Clear,
                CreditOp::WaitFrame(3),
                CreditOp::Draw(0x40),
                CreditOp::FadeIn,
                CreditOp::FadeOut,
                CreditOp::WaitFrame(FOREVER)
            ]
        );
        let text = drop.text_list(0x40).unwrap();
        assert_eq!(
            text,
            [TextItem {
                x: 9,
                y: 8,
                font: 6,
                colour: 252,
                text: b"HI".to_vec()
            }]
        );
        assert!(drop.text_list(0xfff0).is_err());
    }

    #[test]
    fn numbers_snapshot() {
        insta::assert_debug_snapshot!(parse(&synthetic_vgaintro(&synthetic_numbers())).unwrap());
    }

    #[test]
    fn another_build_is_refused() {
        let file = synthetic_vgaintro(&synthetic_numbers());
        // An instruction that is not the one expected before an immediate.
        let (_, at, op, _) = NUMBERS[10];
        let mut bad = file.clone();
        bad[0x20 + at - op.len()] ^= 0xff;
        assert!(parse(&bad).is_err());
        // A state table that does not list the handlers.
        let mut bad = file.clone();
        bad[0x20 + STATE_TABLE] ^= 1;
        assert!(parse(&bad).is_err());
        // Cut short, or not an MZ file.
        assert!(parse(&file[..0x1000]).is_err());
        assert!(parse(b"ZM").is_err());
    }

    #[test]
    fn credits_and_records_must_end() {
        let numbers = synthetic_numbers();
        let mut file = synthetic_vgaintro(&numbers);
        // The script's last wait no longer waits forever.
        let last = 0x20 + usize::from(numbers.data_paragraph) * 16 + 0x18 + 5 * 4 + 2;
        file[last] = 0;
        assert!(parse(&file).is_err());
        // A record too short to have a header.
        let mut file = synthetic_vgaintro(&numbers);
        file[0x20 + usize::from(numbers.anim_paragraph) * 16] = 4;
        assert!(parse(&file).is_err());
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_vgaintro_parses() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let drop = parse(&std::fs::read(dir.join("vgaintro.dvr")).unwrap()).unwrap();
        let n = &drop.numbers;
        // Structure only: the frames come in order, the times rise, every blend and
        // the credits record are in the list, and the credits draw real text lists.
        assert!(n.accelerate_until < 0x80);
        assert!(u16::from(n.accelerate_until) < n.first_part_end);
        assert!(n.first_part_end < n.jump_at && n.jump_at < n.boost_at);
        assert!(n.boost_at < n.slow_from && n.slow_from < n.stop_at);
        assert!(n.stop_at < n.hold_until && n.hold_until < n.hold_again_until);
        assert!(n.start < n.second_blend && n.second_blend < n.third_blend);
        assert!(n.third_blend < n.frames && n.frames < n.cave_end);
        let records = xeno_count(&drop.anims);
        for r in [
            n.blend_record,
            n.second_blend_record,
            n.third_blend_record,
            n.last_blend_record,
            n.light_record,
            n.credits_record,
        ] {
            assert!(usize::from(r) < records);
        }
        assert_eq!(drop.credits.last(), Some(&CreditOp::WaitFrame(FOREVER)));
        for op in &drop.credits {
            if let CreditOp::Draw(list) = op {
                assert!(!drop.text_list(*list).unwrap().is_empty());
            }
        }
        assert!(!drop.text_list(n.copyright).unwrap().is_empty());
    }

    /// Records in a list.
    fn xeno_count(list: &[u8]) -> usize {
        let mut at = 0;
        let mut n = 0;
        while u16::from_le_bytes([list[at], list[at + 1]]) != 0 {
            at += usize::from(u16::from_le_bytes([list[at], list[at + 1]]));
            n += 1;
        }
        n
    }
}
