//! The consoles' third displays, the radar in each console's colour-253 region
//! (`docs/screens.md`, "The third display (T-166)"): how each kind's frame routine
//! (humanoid `1fe6:7cff`, scorpion `7bdb`, scout `7e2b`) sets up view mode 0x12.
//! Read from the user's own `xeno.exe` at run time, never stored in this repository.
//!
//! Each routine is found from the `call far` in its draw handler, after checking the
//! handler's head, and decoded instruction by instruction: only the few forms the
//! three routines use are accepted, so a routine of another shape is refused.

use crate::FormatError;
use crate::farewell::operand;
use crate::lzexe::Unpacked;
use crate::maproom::{invalid, word};

/// The draw handlers (humanoid `1000:9f96`, scorpion `833a`, scout `b738`).
pub(crate) const HANDLERS: [usize; 3] = [0x9f96, 0x833a, 0xb738];
/// A third display's draw handler up to its `call far` (`mov di, [0x62f1]; mov si,
/// [bx + 2]; mov byte [bp + si], 3; mov al, [0x1450]; or al, al; jz; jns; jmp; nop;
/// ret`).
pub(crate) const HANDLER_HEAD: [u8; 23] = [
    0x8b, 0x3e, 0xf1, 0x62, 0x8b, 0x77, 0x02, 0xc6, 0x02, 0x03, 0xa0, 0x50, 0x14, 0x0a, 0xc0, 0x74,
    0x06, 0x79, 0x03, 0xeb, 0x08, 0x90, 0xc3,
];
/// The host's view words the routines set (`docs/render.md`, "The camera").
const MODE: u16 = 0x1451;
const ZOOM: u16 = 0x1452;
const SHIFT: u16 = 0x1454;
const NOISE: u16 = 0x1456;
const DOT: u16 = 0x1457;
const BACKGROUND: u16 = 0x1458;
/// The longest routine accepted, in instructions.
const MAX_STEPS: usize = 40;

/// One kind's third display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Radar {
    /// A frame is started once in this many chances (`DS:0x62ed`).
    pub frames: u8,
    /// The +0xf6 bit of the radar: while it is set, the damaged frame.
    pub device: u8,
    /// The view mode (0x12 in the stock build) and the damaged one (0x18).
    pub view_mode: u8,
    pub damaged_mode: u8,
    /// The objects' dots' colour (`DS:0x1457`).
    pub dot: u8,
    /// The sprite drawn over the buffer first (`DS:0x1458`).
    pub background: u16,
    /// The zoom (`DS:0x1452`) and zoom shift (`DS:0x1454`).
    pub zoom: u8,
    pub zoom_shift: u8,
    /// Taken off the robot's heading for the camera's (the scorpion's `sub ax`).
    pub heading_offset: u16,
    /// The damaged frame's noise colour (`DS:0x1456`).
    pub noise: u8,
}

/// What one path of a routine stores into the view words, up to its `retf`.
#[derive(Default)]
struct Stores {
    bytes: Vec<(u16, u16)>,
    heading_offset: u16,
}

impl Stores {
    fn get(&self, at: usize, address: u16) -> Result<u16, FormatError> {
        self.bytes
            .iter()
            .rev()
            .find(|(a, _)| *a == address)
            .map(|(_, v)| *v)
            .ok_or_else(|| invalid(at, format!("the routine does not set {address:#x}")))
    }
}

fn byte(image: &[u8], at: usize) -> Result<u8, FormatError> {
    image
        .get(at)
        .copied()
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

/// Decodes from `at` to the `retf`, keeping the immediates stored.
fn stores(image: &[u8], mut at: usize) -> Result<Stores, FormatError> {
    let start = at;
    let mut out = Stores::default();
    for _ in 0..MAX_STEPS {
        let w = |off| word(image, at + off);
        let len = match [byte(image, at)?, byte(image, at + 1)?] {
            [0xcb, _] => return Ok(out),
            // mov byte [addr], imm8; mov word [addr], imm16
            [0xc6, 0x06] => {
                out.bytes.push((w(2)?, u16::from(byte(image, at + 4)?)));
                5
            }
            [0xc7, 0x06] => {
                out.bytes.push((w(2)?, w(4)?));
                6
            }
            // sub ax, imm16 (after the heading's load)
            [0x2d, _] => {
                out.heading_offset = w(1)?;
                3
            }
            // mov [addr], di / dx
            [0x89, 0x3e | 0x16] => 4,
            // mov ax, [di + d8]; add ax, [di + d8]; adc dx, [di + d8]
            [0x8b, 0x45] | [0x03, 0x45] | [0x13, 0x55] => 3,
            // mov [addr], ax
            [0xa3, _] => 3,
            // cwd
            [0x99, _] => 1,
            _ => return Err(invalid(at, "not a third display's routine")),
        };
        at += len;
    }
    Err(invalid(start, "the routine does not end"))
}

/// The routine at `at`: `dec byte [counter]; jnz; mov byte [counter], frames; test
/// byte [di + 0xf6], device; jnz damaged`, then the view's set-up and `retf`.
fn routine(image: &[u8], at: usize) -> Result<Radar, FormatError> {
    let head = image
        .get(at..at + 18)
        .ok_or_else(|| invalid(at, "past the end of the image"))?;
    let (frames, device, jump) = match *head {
        [
            0xfe,
            0x0e,
            a,
            b,
            0x75,
            _,
            0xc6,
            0x06,
            c,
            d,
            frames,
            0xf6,
            0x85,
            0xf6,
            0x00,
            device,
            0x75,
            jump,
        ] if [a, b] == [c, d] => (frames, device, jump),
        _ => return Err(invalid(at, "not a third display's frame routine")),
    };
    let set_up = stores(image, at + 18)?;
    let damaged = stores(image, at + 18 + usize::from(jump))?;
    let low = |s: &Stores, a| s.get(at, a).map(|v| v as u8);
    Ok(Radar {
        frames,
        device,
        view_mode: low(&set_up, MODE)?,
        damaged_mode: low(&damaged, MODE)?,
        dot: low(&set_up, DOT)?,
        background: set_up.get(at, BACKGROUND)?,
        zoom: low(&set_up, ZOOM)?,
        zoom_shift: low(&set_up, SHIFT)?,
        heading_offset: set_up.heading_offset,
        noise: low(&damaged, NOISE)?,
    })
}

/// The three kinds' third displays (humanoid, scorpion, scout) from a load image.
pub fn read_image(image: &[u8]) -> Result<[Radar; 3], FormatError> {
    let one = |handler: usize| {
        if image.get(handler..handler + HANDLER_HEAD.len()) != Some(&HANDLER_HEAD[..]) {
            return Err(invalid(handler, "not a third display's draw handler"));
        }
        let call = handler + HANDLER_HEAD.len();
        let offset = operand(image, call, &[0x9a])?;
        let segment = word(image, call + 3)?;
        routine(image, usize::from(segment) * 16 + usize::from(offset))
    };
    Ok([one(HANDLERS[0])?, one(HANDLERS[1])?, one(HANDLERS[2])?])
}

/// The three kinds' third displays from the user's unpacked `xeno.exe`.
pub fn read(exe: &Unpacked) -> Result<[Radar; 3], FormatError> {
    read_image(&exe.image)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A routine like the stock ones with made-up values: counter at 0x100, frames 3,
    /// device bit 0x20, mode 0x22, dot 0x44, background 0x1234, zoom 4, shift 2,
    /// heading offset 0x0102 (scorpion only); damaged: mode 0x28, noise 0x30.
    fn routine_bytes(offset: bool) -> Vec<u8> {
        let mut r = vec![0xfe, 0x0e, 0x00, 0x01, 0x75, 0x00];
        r.extend([
            0xc6, 0x06, 0x00, 0x01, 3, 0xf6, 0x85, 0xf6, 0x00, 0x20, 0x75, 0,
        ]);
        let jump_at = r.len() - 1;
        let set = |a: u16, v: u8| [0xc6, 0x06, a as u8, (a >> 8) as u8, v];
        let start = r.len();
        r.extend(set(MODE, 0x22));
        r.extend([0xc7, 0x06, 0x5a, 0x14, 0x4a, 0x68, 0x89, 0x3e, 0x5c, 0x14]);
        r.extend(set(DOT, 0x44));
        r.extend([0xc7, 0x06, 0x58, 0x14, 0x34, 0x12]);
        r.extend(set(SHIFT, 2));
        r.extend([0x8b, 0x45, 0x10]);
        if offset {
            r.extend([0x2d, 0x02, 0x01]);
        }
        r.extend([0xa3, 0x60, 0x14]);
        r.extend(set(ZOOM, 4));
        r.extend([0x8b, 0x45, 0x1c, 0x99, 0x03, 0x45, 0x0c, 0x13, 0x55, 0x0e]);
        r.extend([0x89, 0x16, 0x6c, 0x14, 0xcb]);
        r[jump_at] = (r.len() - start) as u8;
        r.extend(set(MODE, 0x28));
        r.extend(set(NOISE, 0x30));
        r.push(0xcb);
        r
    }

    /// An image with the three handlers calling routines at 0x1_0000 on (segment
    /// 0x1000), the scorpion's with a heading offset.
    fn synthetic() -> Vec<u8> {
        let mut image = vec![0x90; 0x1_0400];
        for (i, &h) in HANDLERS.iter().enumerate() {
            let at = 0x1_0000 + 0x100 * i;
            image[h..h + HANDLER_HEAD.len()].copy_from_slice(&HANDLER_HEAD);
            let call = h + HANDLER_HEAD.len();
            image[call..call + 5].copy_from_slice(&[0x9a, 0, i as u8, 0x00, 0x10]);
            let r = routine_bytes(i == 1);
            image[at..at + r.len()].copy_from_slice(&r);
        }
        image
    }

    #[test]
    fn reads_each_kind() {
        let radars = read_image(&synthetic()).unwrap();
        let want = Radar {
            frames: 3,
            device: 0x20,
            view_mode: 0x22,
            damaged_mode: 0x28,
            dot: 0x44,
            background: 0x1234,
            zoom: 4,
            zoom_shift: 2,
            heading_offset: 0,
            noise: 0x30,
        };
        assert_eq!(radars[0], want);
        assert_eq!(radars[1].heading_offset, 0x0102);
        assert_eq!(radars[2], want);
        insta::assert_debug_snapshot!(radars[1]);
    }

    #[test]
    fn refuses_other_shapes() {
        let corrupt = |at: usize| {
            let mut image = synthetic();
            image[at] = 0x90;
            read_image(&image).is_err()
        };
        // The handler's head, its call, the routine's head, an unknown instruction.
        assert!(corrupt(HANDLERS[2]));
        assert!(corrupt(HANDLERS[0] + HANDLER_HEAD.len()));
        assert!(corrupt(0x1_0000));
        assert!(corrupt(0x1_0100 + 18));
        // A routine that never ends.
        let mut image = synthetic();
        let r = routine_bytes(false);
        let end = r.iter().rposition(|&b| b == 0xcb).unwrap();
        image[0x1_0000 + end] = 0x99;
        assert!(read_image(&image).is_err());
    }

    /// The stock humanoid's (`1fe6:7cff`): every 8 chances, the radar bit 3, view mode
    /// 0x12 (0x18 damaged), dots 0x7f over sprite 0x670, zoom 8 shift 1, noise 0x10.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_third_displays_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let [h, s, c] = read(&exe).unwrap();
        assert_eq!(
            (h.frames, h.device, h.view_mode, h.damaged_mode),
            (8, 8, 0x12, 0x18)
        );
        assert_eq!(
            (h.dot, h.background, h.zoom, h.zoom_shift),
            (0x7f, 0x670, 8, 1)
        );
        assert_eq!((h.heading_offset, h.noise), (0, 0x10));
        // The scorpion's (`1fe6:7bdb`, T-170): the humanoid's but every 5 chances, its
        // own dots and background, and the heading turned about 25 degrees.
        assert_eq!((s.frames, s.dot, s.background), (5, 0x57, 0x8c8));
        assert_eq!((s.device, s.view_mode, s.damaged_mode), (8, 0x12, 0x18));
        assert_eq!((s.zoom, s.zoom_shift, s.noise), (8, 1, 0x10));
        assert_eq!(s.heading_offset, 0x11c6);
        // The scout's (T-173): every 5, its own dots and background.
        assert_eq!((c.frames, c.dot, c.background), (5, 0x63, 0xba0));
    }
}
