//! What the port reads from the user's `xeno.exe` to make the 3D view's render nodes
//! (`docs/render.md`): each object kind's model, frame bytes and node type, the focal
//! factors, the models' size factors and the distance limit. Game data, read at run
//! time through the instructions that use it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};
use crate::sequence;

/// Object kinds 0 to 16 (`usg::ObjectKind`).
pub const KINDS: usize = 17;
/// Models 0, 2, ... 20 (`mxx.md`, "Loading"), by number / 2.
pub const MODELS: usize = 11;

/// `mov dl, [bx + table]` in `gfx_emit_object_render_nodes`: the kinds' models.
pub(crate) const KIND_MODELS_AT: usize = 0xf8ec;
/// `add si, [bx + table]` in the type 2 node writer: the kinds' frame bytes.
pub(crate) const FRAME_OFFSETS_AT: usize = 0xfa98;
/// `call [bx + table]`: the kinds' 3D node routines.
pub(crate) const NODE_ROUTINES_AT: usize = 0xf904;
/// `mov bx, factor` in the camera steps: the 3D view's, the situation room's and the
/// map's focal factors.
pub(crate) const FOCAL_AT: [usize; 3] = [0x29a3, 0x2999, 0x299e];
/// `cmp word [si + table], 0x100` in `gfx_get_model_scale`: the size factors.
pub(crate) const SIZES_AT: usize = 0x3fb3;
/// The first `mov word [table + model], size` in `gfx_load_all_models`.
pub(crate) const SIZE_MOVS_AT: usize = 0x7b61;
/// `mov ax, [float]` in `gfx_draw_model`: the size a factor of 0x100 stands for.
pub(crate) const DEFAULT_SIZE_AT: usize = 0x303f;
/// `cmp dx, limit` in `gfx_draw_model`: the distance's whole part must be below it.
pub(crate) const MAX_DISTANCE_AT: usize = 0x301e;

/// A number in the game's floating-point format (`docs/render.md`, "Projection"):
/// `mantissa * 2^exponent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Float {
    pub mantissa: u16,
    pub exponent: i8,
}

/// The 3D view's tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View3d {
    /// Each kind's model number (`DS:0xe70c`).
    pub kind_models: [u8; KINDS],
    /// Where each kind's frame bytes start in the object (`DS:0xe71d`).
    pub frame_offsets: [u16; KINDS],
    /// Each kind's node type in the 3D view (2 or 6), or none.
    pub node_types: [Option<u8>; KINDS],
    /// The focal factors at `DS:0x4d2a` (3D view), `0x4d2e` and `0x4d32`.
    pub focal: [Float; 3],
    /// The models' size factors once loaded (0x100: [`View3d::default_size`]).
    pub model_sizes: [u16; MODELS],
    pub default_size: Float,
    /// Models are drawn only nearer than this many units.
    pub max_distance: u8,
}

fn byte(image: &[u8], at: usize) -> Result<u8, FormatError> {
    image
        .get(at)
        .copied()
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

fn float(image: &[u8], at: usize) -> Result<Float, FormatError> {
    Ok(Float {
        mantissa: word(image, at)?,
        exponent: byte(image, at + 2)? as i8,
    })
}

/// The node type a kind's routine writes: `stc` for none, or a call of a writer
/// (`mov ah, dl`, `mov al, type`, `call`), maybe after `mov ah, x` or
/// `mov dl, x` / `xor dh, dh`.
fn node_type(image: &[u8], at: usize) -> Result<Option<u8>, FormatError> {
    let call = match *image.get(at..at + 7).unwrap_or_default() {
        [0xf9, 0xc3, ..] => return Ok(None),
        [0xe8, lo, hi, ..] => (at + 3, lo, hi),
        [0xb4, _, 0xe8, lo, hi, ..] => (at + 5, lo, hi),
        [0xb2, _, 0x32, 0xf6, 0xe8, lo, hi] => (at + 7, lo, hi),
        _ => return Err(invalid(at, "not a render node routine")),
    };
    let writer = call
        .0
        .wrapping_add(usize::from(u16::from_le_bytes([call.1, call.2])))
        & 0xffff;
    match *image.get(writer..writer + 5).unwrap_or_default() {
        [0x8a, 0xe2, 0xb0, t, 0xe8] => Ok(Some(t)),
        _ => Err(invalid(writer, "not a render node writer")),
    }
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<View3d, FormatError> {
    let models = ds + usize::from(operand(image, KIND_MODELS_AT, &[0x8a, 0x97])?);
    let offsets = ds + usize::from(operand(image, FRAME_OFFSETS_AT, &[0x03, 0xb7])?);
    let routines = ds + usize::from(operand(image, NODE_ROUTINES_AT, &[0xff, 0x97])?);
    let mut view = View3d {
        kind_models: [0; KINDS],
        frame_offsets: [0; KINDS],
        node_types: [None; KINDS],
        focal: [Float {
            mantissa: 0,
            exponent: 0,
        }; 3],
        model_sizes: [0; MODELS],
        default_size: float(
            image,
            ds + usize::from(operand(image, DEFAULT_SIZE_AT, &[0xa1])?),
        )?,
        max_distance: match image.get(MAX_DISTANCE_AT..MAX_DISTANCE_AT + 3) {
            Some(&[0x83, 0xfa, n]) => n,
            _ => return Err(invalid(MAX_DISTANCE_AT, "not the distance limit")),
        },
    };
    for kind in 0..KINDS {
        view.kind_models[kind] = byte(image, models + kind)?;
        view.frame_offsets[kind] = word(image, offsets + 2 * kind)?;
        view.node_types[kind] = node_type(image, usize::from(word(image, routines + 2 * kind)?))?;
    }
    for (f, at) in view.focal.iter_mut().zip(FOCAL_AT) {
        *f = float(image, ds + usize::from(operand(image, at, &[0xbb])?))?;
    }
    let sizes = usize::from(operand(image, SIZES_AT, &[0x81, 0xbc])?);
    for (m, size) in view.model_sizes.iter_mut().enumerate() {
        *size = word(image, ds + sizes + 2 * m)?;
    }
    // The start-up `mov word [sizes + model], size`s, up to the next instruction.
    let mut at = SIZE_MOVS_AT;
    while let Some(&[0xc7, 0x06, lo, hi, s0, s1]) = image.get(at..at + 6) {
        let m = usize::from(u16::from_le_bytes([lo, hi])).wrapping_sub(sizes);
        if m % 2 != 0 || m / 2 >= MODELS {
            return Err(invalid(at, "a size for no model"));
        }
        view.model_sizes[m / 2] = u16::from_le_bytes([s0, s1]);
        at += 6;
    }
    if at == SIZE_MOVS_AT {
        return Err(invalid(at, "not the models' sizes"));
    }
    Ok(view)
}

/// Reads the tables from an unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<View3d, FormatError> {
    read_image(&exe.image, sequence::data_segment(exe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    /// Made-up tables: kind `k`'s model is `2 * k`, its frames at `0x18 + k`; kinds 0
    /// and 16 make type 2 nodes (16 through `mov dl` / `xor dh, dh`), kind 13 type 6,
    /// the rest none; focal factors 230, 655 and 327; sizes 0x100 but for models 0
    /// (0x44) and 20 (0x58); the stand-in 0x8000 / -8; a limit of 0x7d.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0xf000];
        let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
        let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
        put(KIND_MODELS_AT, &op(&[0x8a, 0x97], 0xe70c));
        put(FRAME_OFFSETS_AT, &op(&[0x03, 0xb7], 0xe71d));
        put(NODE_ROUTINES_AT, &op(&[0xff, 0x97], 0xe7d8));
        // Routines: 0x9000 none, 0x9010 type 2, 0x9020 type 6, 0x9030 type 2 with `mov dl`.
        put(0x9000, &[0xf9, 0xc3]);
        put(0x9010, &op(&[0xe8], 0x9100 - 0x9013));
        put(0x9020, &op(&[0xe8], 0x9110 - 0x9023));
        put(
            0x9030,
            &[&[0xb2, 0x14, 0x32, 0xf6][..], &op(&[0xe8], 0x9100 - 0x9037)].concat(),
        );
        put(0x9100, &[0x8a, 0xe2, 0xb0, 2, 0xe8]);
        put(0x9110, &[0x8a, 0xe2, 0xb0, 6, 0xe8]);
        for k in 0..KINDS {
            let routine: u16 = match k {
                0 => 0x9010,
                13 => 0x9020,
                16 => 0x9030,
                _ => 0x9000,
            };
            put(DS + 0xe70c + k, &[2 * k as u8]);
            put(DS + 0xe71d + 2 * k, &(0x18 + k as u16).to_le_bytes());
            put(DS + 0xe7d8 + 2 * k, &routine.to_le_bytes());
        }
        for (at, (addr, mantissa, exponent)) in FOCAL_AT.into_iter().zip([
            (0x4d2a, 230u16, 0),
            (0x4d2e, 655, 0xf0),
            (0x4d32, 327, 0xf0),
        ]) {
            put(at, &op(&[0xbb], addr));
            put(
                DS + usize::from(addr),
                &[&mantissa.to_le_bytes()[..], &[exponent]].concat(),
            );
        }
        put(SIZES_AT, &op(&[0x81, 0xbc], 0x2316));
        for m in 0..MODELS {
            put(DS + 0x2316 + 2 * m, &0x100u16.to_le_bytes());
        }
        put(
            SIZE_MOVS_AT,
            &[&op(&[0xc7, 0x06], 0x2316)[..], &op(&[], 0x44)].concat(),
        );
        put(
            SIZE_MOVS_AT + 6,
            &[&op(&[0xc7, 0x06], 0x232a)[..], &op(&[], 0x58)].concat(),
        );
        put(DEFAULT_SIZE_AT, &op(&[0xa1], 0x4d52));
        put(DS + 0x4d52, &[0, 0x80, 0xf8]);
        put(MAX_DISTANCE_AT, &[0x83, 0xfa, 0x7d]);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let view = read_image(&synthetic(), DS).unwrap();
        assert_eq!(view.kind_models[3], 6);
        assert_eq!(view.frame_offsets[16], 0x28);
        let mut types = [None; KINDS];
        (types[0], types[13], types[16]) = (Some(2), Some(6), Some(2));
        assert_eq!(view.node_types, types);
        assert_eq!(
            view.focal[0],
            Float {
                mantissa: 230,
                exponent: 0
            }
        );
        assert_eq!(view.focal[1].exponent, -16);
        assert_eq!(
            (
                view.model_sizes[0],
                view.model_sizes[1],
                view.model_sizes[10]
            ),
            (0x44, 0x100, 0x58)
        );
        assert_eq!(
            view.default_size,
            Float {
                mantissa: 0x8000,
                exponent: -8
            }
        );
        assert_eq!(view.max_distance, 0x7d);
        insta::assert_debug_snapshot!(view);
    }

    #[test]
    fn another_build_is_refused() {
        for at in [
            KIND_MODELS_AT,
            FRAME_OFFSETS_AT + 1,
            NODE_ROUTINES_AT,
            FOCAL_AT[2],
            SIZES_AT + 1,
            SIZE_MOVS_AT,
            SIZE_MOVS_AT + 2,
            DEFAULT_SIZE_AT,
            MAX_DISTANCE_AT + 1,
            // A routine that is neither, and a writer of another shape.
            0x9010,
            0x9101,
        ] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables are those of `docs/render.md`.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_view3d_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let view = read(&exe).unwrap();
        assert_eq!(view.kind_models[..5], [0, 2, 4, 0x10, 8]);
        assert_eq!(view.kind_models[16], 0x14);
        assert_eq!(view.frame_offsets[..4], [0xfe, 0xfe, 0xfe, 0x18]);
        let two = Some(2);
        assert_eq!(
            view.node_types,
            [
                two,
                two,
                two,
                two,
                two,
                two,
                None,
                two,
                two,
                None,
                None,
                None,
                None,
                Some(6),
                Some(6),
                None,
                two
            ]
        );
        assert_eq!(
            view.focal[0],
            Float {
                mantissa: 230,
                exponent: 0
            }
        );
        assert_eq!(
            view.model_sizes,
            [
                0x44, 0x80, 0x40, 0x80, 0x28, 0x100, 0x28, 0x20, 0x180, 0x100, 0x58
            ]
        );
        assert_eq!(
            view.default_size,
            Float {
                mantissa: 0x8000,
                exponent: -8
            }
        );
        assert_eq!(view.max_distance, 0x7d);
    }
}
