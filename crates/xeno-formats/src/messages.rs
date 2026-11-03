//! What the port reads from the user's `xeno.exe` for the bases' message logs and the
//! messages sent to a whole side (`docs/screens.md`, "The message log (T-129)"): the
//! senders' names, the base's two messages and the header a side's robots are sent.
//! Read at run time through the instructions that use them, after checking their
//! bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::{Rect, invalid, rect};

/// `1fe6:7f38`'s fixed names, each three `mov word [0x1bfe + 2n], imm`: a base's, a
/// relay's, kind 0xa's and the alert's (`DI` = 0xffff, through `1000:65ae`).
pub(crate) const NAMES_AT: [usize; 4] = [0x17dd1, 0x17e03, 0x17dea, 0x17e1c];
const NAME_OPS: [&[u8]; 3] = [
    &[0xc7, 0x06, 0xfe, 0x1b],
    &[0xc7, 0x06, 0x00, 0x1c],
    &[0xc7, 0x06, 0x02, 0x1c],
];
/// `mov ax, [bx + letters]`: a robot's two letters by kind.
pub(crate) const LETTERS_AT: usize = crate::sitroom::KIND_LETTERS_AT;
/// `sim_damage_base` (`1000:62b7`): `mov si, text` for the base destroyed and under
/// attack, and `cmp ax, interval`, the game clock's counts between the second.
pub(crate) const DESTROYED_AT: usize = 0x62d0;
pub(crate) const ATTACKED_AT: usize = 0x62f6;
pub(crate) const INTERVAL_AT: usize = 0x62e9;
/// `1000:d54c`: `mov cx, length` (the name's), the header's first and last bytes
/// written in the string segment (`mov byte es:[header], b`, `mov byte es:[bx +
/// header + 1], b`) and `mov si, header` before each robot's copy.
pub(crate) const NAME_LENGTH_AT: usize = 0xd55e;
pub(crate) const HEADER_START_AT: usize = 0xd573;
pub(crate) const HEADER_END_AT: usize = 0xd579;
pub(crate) const HEADER_AT: usize = 0xd5a9;
/// The scorpion's damage panel's mouse handler (`1000:90f9`) on a release: six `mov
/// si, rect`, `call 1000:cfdc` (the point in the rectangle, edges included), `jc
/// line`, the first hit winning.
pub(crate) const STATUS_TESTS_AT: usize = 0x911b;
pub(crate) const STATUS_LINES: usize = 6;
const RECT_TEST: usize = 0xcfdc;
const PANEL_POST: usize = 0xd53f;
const FORMAT: usize = 0x02b7;
/// `1000:02b7`'s digits (five, leading zeros), the last three copied from here.
const DIGITS: u16 = 0x1bfe;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Messages {
    /// The names a base, a relay and an object of kind 0xa sign with, and the alerts
    /// (a mine's, a buoy's).
    pub base: [u8; 6],
    pub relay: [u8; 6],
    pub other: [u8; 6],
    pub alert: [u8; 6],
    /// A robot's name's letters, by kind (humanoid, scorpion, scout).
    pub letters: [[u8; 2]; 3],
    /// The texts (string-segment offsets) a destroyed base sends the other side and a
    /// base under attack its own.
    pub destroyed: u16,
    pub attacked: u16,
    /// The game clock's counts (`DS:0x374`) a base waits past before it says it is
    /// under attack again.
    pub interval: u16,
    /// The header each of a side's robots is sent first (a string-segment offset),
    /// and the two bytes that bracket the name's place in it.
    pub header: u16,
    pub header_bytes: [u8; 2],
    /// The name's place's length.
    pub name_length: u16,
    /// The scorpion's damage panel's status lines, in the order its release tests
    /// them.
    pub status: Vec<StatusLine>,
}

/// One light's status line (`1000:9153` to `926c`): its fields written into the
/// string segment, then `message` (or `alternate`'s) to the robot's panel
/// (`1000:d53f`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub rect: Rect,
    pub fields: Vec<Field>,
    pub message: u16,
    /// The robot's byte at an offset, a bit and the message sent instead while it is
    /// set.
    pub alternate: Option<(u16, u8, u16)>,
}

/// Three bytes the status line writes into the string segment at `at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// `ok`, or `out` while the robot's byte at `offset` has `bit`.
    Flag {
        at: u16,
        offset: u16,
        bit: u8,
        ok: [u8; 3],
        out: [u8; 3],
    },
    /// The robot's byte at `offset` masked, as the last three of five digits.
    Number { at: u16, offset: u16, mask: u8 },
}

fn near_target(image: &[u8], at: usize) -> Result<usize, FormatError> {
    let rel = operand(image, at, &[0xe8])?;
    Ok((at + 3 + usize::from(rel)) & 0xffff)
}

/// The status line routine at `at`, `jmp`s followed.
fn status_line(
    image: &[u8],
    ds: usize,
    rect_at: u16,
    mut at: usize,
) -> Result<StatusLine, FormatError> {
    let get = |at: usize, n: usize| {
        image
            .get(at..at + n)
            .ok_or_else(|| invalid(at, "past the end"))
    };
    let le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let mut ok = std::collections::BTreeMap::new();
    let mut fields = Vec::new();
    let post = |call: usize, message, fields, alternate| {
        if near_target(image, call)? != PANEL_POST {
            return Err(invalid(call, "not the panel's post"));
        }
        Ok(StatusLine {
            rect: rect(image, ds + usize::from(rect_at))?,
            fields,
            message,
            alternate,
        })
    };
    for _ in 0..32 {
        let b = get(at, 26).or_else(|_| get(at, 6))?;
        match b {
            [0xe9, lo, hi, ..] => {
                at = (at + 3).wrapping_add(usize::from(u16::from_le_bytes([*lo, *hi]))) & 0xffff
            }
            [0x26, 0xc7, 0x06, a0, a1, w0, w1, ..] => {
                ok.insert(le(&[*a0, *a1]), [*w0, *w1]);
                at += 7;
            }
            [0x26, 0xc6, 0x06, a0, a1, v, ..] => {
                ok.insert(le(&[*a0, *a1]), [*v, 0]);
                at += 6;
            }
            [
                0xf6,
                0x85,
                o0,
                o1,
                bit,
                0x74,
                0x0d,
                0x26,
                0xc7,
                0x06,
                a0,
                a1,
                w0,
                w1,
                0x26,
                0xc6,
                0x06,
                c0,
                c1,
                v,
                ..,
            ] => {
                let field = le(&[*a0, *a1]);
                let (Some(w), Some(v0)) = (ok.get(&field), ok.get(&field.wrapping_add(2))) else {
                    return Err(invalid(at, "a field with no default"));
                };
                if le(&[*c0, *c1]) != field.wrapping_add(2) {
                    return Err(invalid(at, "not one field"));
                }
                fields.push(Field::Flag {
                    at: field,
                    offset: le(&[*o0, *o1]),
                    bit: *bit,
                    ok: [w[0], w[1], v0[0]],
                    out: [*w0, *w1, *v],
                });
                at += 20;
            }
            [0x8a, 0x45, offset, 0x24, mask, 0x98, ..] => {
                let field = operand(image, at + 15, &[0x26, 0xa2])?;
                let checks = [
                    (at + 6, &[0xbe][..], DIGITS),
                    (at + 12, &[0xa0], DIGITS + 2),
                    (at + 19, &[0xa1], DIGITS + 3),
                    (at + 22, &[0x26, 0xa3], field.wrapping_add(1)),
                ];
                for (op_at, op, value) in checks {
                    if operand(image, op_at, op)? != value {
                        return Err(invalid(op_at, "not the number's copy"));
                    }
                }
                if near_target(image, at + 9)? != FORMAT {
                    return Err(invalid(at + 9, "not the number's format"));
                }
                fields.push(Field::Number {
                    at: field,
                    offset: u16::from(*offset),
                    mask: *mask,
                });
                at += 26;
            }
            // `mov si, message`, then another while the robot's bit is set.
            [
                0xbe,
                m0,
                m1,
                0xf6,
                0x85,
                o0,
                o1,
                bit,
                0x74,
                0x03,
                0xbe,
                n0,
                n1,
                ..,
            ] => {
                let alternate = Some((le(&[*o0, *o1]), *bit, le(&[*n0, *n1])));
                return post(at + 13, le(&[*m0, *m1]), fields, alternate);
            }
            [0xbe, m0, m1, ..] => return post(at + 3, le(&[*m0, *m1]), fields, None),
            _ => return Err(invalid(at, "not a status line")),
        }
    }
    Err(invalid(at, "a status line too long"))
}

fn status_lines(image: &[u8], ds: usize) -> Result<Vec<StatusLine>, FormatError> {
    (0..STATUS_LINES)
        .map(|i| {
            let at = STATUS_TESTS_AT + 8 * i;
            let rect_at = operand(image, at, &[0xbe])?;
            if near_target(image, at + 3)? != RECT_TEST {
                return Err(invalid(at + 3, "not the rectangle test"));
            }
            let line = match image.get(at + 6..at + 8) {
                Some(&[0x72, j]) => (at + 8).wrapping_add_signed(isize::from(j as i8)),
                _ => return Err(invalid(at + 6, "not the test's jc")),
            };
            status_line(image, ds, rect_at, line)
        })
        .collect()
}

pub fn read_image(image: &[u8], ds: usize) -> Result<Messages, FormatError> {
    let byte = |at: usize| {
        image
            .get(at)
            .copied()
            .ok_or_else(|| invalid(at, "past the end"))
    };
    let name = |at: usize| -> Result<[u8; 6], FormatError> {
        let mut name = [0; 6];
        for (i, op) in NAME_OPS.into_iter().enumerate() {
            let word = operand(image, at + 6 * i, op)?;
            name[2 * i..2 * i + 2].copy_from_slice(&word.to_le_bytes());
        }
        Ok(name)
    };
    let letters = ds + usize::from(operand(image, LETTERS_AT, &[0x8b, 0x87])?);
    let pair = |kind: usize| {
        Ok::<_, FormatError>([byte(letters + 2 * kind)?, byte(letters + 2 * kind + 1)?])
    };
    let header = operand(image, HEADER_AT, &[0xbe])?;
    let name_length = operand(image, NAME_LENGTH_AT, &[0xb9])?;
    let start = &[0x26, 0xc6, 0x06][..];
    if operand(image, HEADER_START_AT, start)? != header {
        return Err(invalid(HEADER_START_AT, "not the header's first byte"));
    }
    if operand(image, HEADER_END_AT, &[0x26, 0xc6, 0x87])? != header.wrapping_add(1) {
        return Err(invalid(HEADER_END_AT, "not the header's last byte"));
    }
    Ok(Messages {
        base: name(NAMES_AT[0])?,
        relay: name(NAMES_AT[1])?,
        other: name(NAMES_AT[2])?,
        alert: name(NAMES_AT[3])?,
        letters: [pair(0)?, pair(1)?, pair(2)?],
        destroyed: operand(image, DESTROYED_AT, &[0xbe])?,
        attacked: operand(image, ATTACKED_AT, &[0xbe])?,
        interval: operand(image, INTERVAL_AT, &[0x3d])?,
        header,
        header_bytes: [byte(HEADER_START_AT + 5)?, byte(HEADER_END_AT + 5)?],
        name_length,
        status: status_lines(image, ds)?,
    })
}

impl Messages {
    /// The header as the first copy leaves it in the string segment `strings`: its
    /// two bytes around the name's place. The original copies the name into the data
    /// segment instead (`mov [bx + header + 1], al` with no `es:`), so the place
    /// stays as the image has it.
    pub fn write_header(&self, strings: &mut [u8]) {
        let at = usize::from(self.header);
        let end = at + 1 + usize::from(self.name_length);
        if let Some(b) = strings.get_mut(at) {
            *b = self.header_bytes[0];
        }
        if let Some(b) = strings.get_mut(end) {
            *b = self.header_bytes[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    #[test]
    fn synthetic_messages_read() {
        let mut image = vec![0u8; DS + 0xa000];
        crate::fixtures::synthetic_messages(&mut image, DS);
        let m = read_image(&image, DS).unwrap();
        assert_eq!(
            (m.base, m.relay, m.other, m.alert),
            (*b"BB    ", *b"RR    ", *b"??    ", *b"AA    ")
        );
        assert_eq!(m.letters, [*b"HU", *b"SC", *b"ST"]);
        assert_eq!((m.destroyed, m.attacked, m.interval), (0x30, 0x40, 0x1e0));
        assert_eq!(
            (m.header, m.header_bytes, m.name_length),
            (0x50, [0xad, 0xba], 6)
        );
        assert_eq!(m.status.len(), STATUS_LINES);
        assert_eq!(
            m.status[0],
            StatusLine {
                rect: Rect {
                    top: 0,
                    left: 0,
                    bottom: 5,
                    right: 5
                },
                fields: vec![Field::Flag {
                    at: 0x60,
                    offset: 0xdc,
                    bit: 1,
                    ok: *b"GO ",
                    out: *b"BAD"
                }],
                message: 0x100,
                alternate: None,
            }
        );
        assert_eq!(
            m.status[1].fields,
            [Field::Number {
                at: 0x70,
                offset: 0x24,
                mask: 0x7f
            }]
        );
        assert_eq!(
            (m.status[2].message, m.status[2].alternate),
            (0x102, Some((0xfd, 0x80, 0x200)))
        );
        assert_eq!(m.status[5].rect.top, 50);
        let mut strings = vec![0; 0x60];
        m.write_header(&mut strings);
        assert_eq!(&strings[0x50..0x59], b"\xad\0\0\0\0\0\0\xba\0");
        image[HEADER_AT] = 0x90;
        assert!(read_image(&image, DS).is_err());
        crate::fixtures::synthetic_messages(&mut image, DS);
        // Another build's panel post.
        image[0x100 + 0x40 + 30] ^= 1;
        assert!(read_image(&image, DS).is_err());
    }

    #[test]
    #[ignore]
    fn real_assets_messages_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let m = read_image(&exe.image, crate::sequence::data_segment(&exe).unwrap()).unwrap();
        assert_eq!((m.destroyed, m.attacked, m.interval), (0x3ce, 0x3e4, 0x1e0));
        assert_eq!((m.header, m.name_length), (0x1714, 6));
        // Bit 7 set: drawn in the high colour.
        assert!(m.header_bytes.iter().all(|b| b & 0x80 != 0));
        // The damage panel's six lights, the last (the helm's) tested last.
        let fields: Vec<_> = m.status.iter().map(|l| l.fields.len()).collect();
        assert_eq!(fields, [1, 1, 1, 0, 3, 1]);
        assert!(matches!(
            m.status[5].fields[0],
            Field::Number { offset: 0x24, .. }
        ));
        assert_eq!(
            m.status[3].alternate.map(|a| (a.0, a.1)),
            Some((0xfd, 0x80))
        );
        assert_eq!(
            m.status.iter().map(|l| l.rect.top).collect::<Vec<_>>(),
            [25, 36, 21, 25, 29, 13]
        );
        assert!(
            [m.base, m.relay, m.other, m.alert]
                .iter()
                .all(|n| n.is_ascii())
        );
    }
}
