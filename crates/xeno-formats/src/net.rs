//! What the port reads from the user's `xeno.exe` for the relay-net orders
//! (`docs/sim.md`, "Relay nets"): the grid each net program walks, and what deploying
//! a relay posts, plays and makes. Game data, read at run time through the
//! instructions that use it, after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::invalid;

/// `1000:49cc`, the grid's choice by the running program (+0x30): `call`, `mov al,
/// [di + 0x30]`, `mov dx, mask`, `cmp al, dense`, `jz`, `mov dl, mask`, `cmp al,
/// sparse`, `jz`, `mov dh, 0xff`.
pub(crate) const LINES_AT: usize = 0x49cc;
/// `sim_deploy_relay` (`1000:9b01`): the deployer's test, the cargo's, the call that
/// makes the relay, then `mov ax, 2 * sound`; `mov si, message` when it fails.
pub(crate) const DEPLOY_AT: usize = 0x9b01;
pub(crate) const DEPLOY_SOUND_AT: usize = 0x9b1d;
pub(crate) const NO_RELAY_AT: usize = 0x9b25;
/// `1000:cc69`: `push di`, `mov ax, kind`, the slot's allocation; then the set-up
/// (`1000:cc2a`: `mov word [si + 0x14], radius` at `cc2e`) and `mov si, message`.
pub(crate) const MAKE_AT: usize = 0xcc69;
pub(crate) const RADIUS_AT: usize = 0xcc2e;
pub(crate) const DEPLOYED_AT: usize = 0xcc9d;
/// The scorpion's L (`1000:9b34`, T-149): the deployer's test, `cmp byte [di + 0x118],
/// full`, the speed's size `cmp ax, speed`, `mov ax, 4` (relays), `mov cx`, the call
/// to the nearest test (`1000:f703`), `cmp ax, reach`; then `mov si, message`: loaded,
/// the deployer damaged, none near, full, too fast.
pub(crate) const LOAD_AT: usize = 0x9b34;
pub(crate) const NEAREST_AT: usize = 0xf703;
pub(crate) const LOAD_MESSAGES_AT: [usize; 5] = [0x9b6b, 0x9b70, 0x9b75, 0x9b7a, 0x9b7f];
/// The scorpion's C (`1000:8d7f`): `mov si, message` (relays carried),
/// `cmp byte [di + 0x118], 0`, `jnz`, `mov si, message` (none).
pub(crate) const CARGO_AT: usize = 0x8d7f;
/// The scorpion's D (`1000:9ab8`): `xor byte [di + 0xfd], bit`, `test`, `jnz`, `mov si,
/// message` (stopped), the log call, `ret`, `mov byte [di + 0x11c], wait`, `mov si,
/// message` (started).
pub(crate) const DUMP_AT: usize = 0x9ab8;
/// The dump's step (`1000:9a50`, in the scorpion's tick table): `test byte [di +
/// 0xfd], bit`, `jz`, `dec byte [di + 0x11c]`, `jnz`, `mov byte [di + 0x11c], wait`,
/// `call sim_deploy_relay`, `jnc`, `call` D's routine.
pub(crate) const DUMP_STEP_AT: usize = 0x9a50;

/// How a net program walks its line of cells: the grid's mask (cells are whole
/// numbers with the mask's bits) and whether it steps along both diagonals (PROBE
/// NET) or along one diagonal and across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub mask: u8,
    pub probe: bool,
}

/// The relay-net tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    /// DENSE NET's and SPARSE NET's program numbers.
    pub programs: [u8; 2],
    /// Their lines, then every other program's (PROBE NET's).
    pub lines: [Line; 3],
    /// The sound event deploying a relay plays for the robot whose console is open.
    pub sound: u8,
    /// The messages: no relay to deploy (none carried, or the deployer damaged), and
    /// one deployed (string-segment offsets).
    pub no_relay: u16,
    pub deployed: u16,
    /// A new relay's radius (+0x14, 8.8).
    pub radius: u16,
    /// The scorpion's L, C and D (T-149).
    pub load: Load,
    /// C's messages: relays carried, and none.
    pub cargo: [u16; 2],
    pub dump: Dump,
}

/// The scorpion's L (`1000:9b34`): the nearest relay within `reach` (8.8) is taken in,
/// with fewer than `full` carried and the speed's size under `speed`; else a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    pub full: u8,
    pub speed: u16,
    pub reach: u16,
    /// The messages: loaded, the deployer damaged, none near, full, too fast.
    pub loaded: u16,
    pub damaged: u16,
    pub none_near: u16,
    pub too_full: u16,
    pub too_fast: u16,
}

/// The cargo dump (D, `1000:9ab8`, and its step `9a50`): +0xfd's `bit` flips with a
/// message; while it is set, the step at tick-table entry `step` deploys a relay every
/// `wait` runs (+0x11c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dump {
    pub bit: u8,
    pub wait: u8,
    pub step: u8,
    /// The messages: started, stopped.
    pub on: u16,
    pub off: u16,
}

impl Net {
    /// The line program `program` walks.
    pub fn line(&self, program: u8) -> Line {
        match self.programs.iter().position(|&p| p == program) {
            Some(i) => self.lines[i],
            None => self.lines[2],
        }
    }
}

fn starts(image: &[u8], at: usize, bytes: &[u8]) -> bool {
    image.get(at..at + bytes.len()) == Some(bytes)
}

fn load(image: &[u8]) -> Result<Load, FormatError> {
    let at = LOAD_AT;
    let call = (NEAREST_AT as u16).wrapping_sub(at as u16 + 0x24);
    let shape = starts(image, at, &[0xf6, 0x85, 0xfd, 0x00, 0x80, 0x75])
        && starts(image, at + 7, &[0x80, 0xbd, 0x18, 0x01])
        && starts(
            image,
            at + 0xe,
            &[0x8b, 0x45, 0x26, 0x99, 0x33, 0xc2, 0x2b, 0xc2, 0x3d],
        )
        && starts(image, at + 0x1b, &[0xb8, 0x04, 0x00, 0xb9])
        && operand(image, at + 0x21, &[0xe8])? == call
        && starts(image, at + 0x29, &[0xfe, 0x85, 0x18, 0x01]);
    if !shape {
        return Err(invalid(at, "not the scorpion's L"));
    }
    let [loaded, damaged, none_near, too_full, too_fast] = LOAD_MESSAGES_AT
        .map(|m| operand(image, m, &[0xbe]))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .expect("five messages");
    Ok(Load {
        full: image[at + 0xb],
        speed: operand(image, at + 0x16, &[0x3d])?,
        reach: operand(image, at + 0x24, &[0x3d])?,
        loaded,
        damaged,
        none_near,
        too_full,
        too_fast,
    })
}

/// The dump's key and step, and the step's entry in the scorpion's tick table.
fn dump(image: &[u8], ds: usize) -> Result<Dump, FormatError> {
    let bad = |at| invalid(at, "not the scorpion's cargo dump");
    let key = image
        .get(DUMP_AT..DUMP_AT + 0x1f)
        .ok_or_else(|| bad(DUMP_AT))?;
    let bit = key[4];
    let shape = key[..4] == [0x80, 0xb5, 0xfd, 0x00]
        && key[5..10] == [0xf6, 0x85, 0xfd, 0x00, bit]
        && key[10] == 0x75
        && key[0x13..0x17] == [0xc6, 0x85, 0x1c, 0x01];
    if !shape || bit == 0 {
        return Err(bad(DUMP_AT));
    }
    let at = DUMP_STEP_AT;
    let wait = key[0x17];
    let step = image.get(at..at + 0x1a).ok_or_else(|| bad(at))?;
    let calls = |i: usize, to: usize| {
        step[i] == 0xe8
            && u16::from_le_bytes([step[i + 1], step[i + 2]])
                == (to as u16).wrapping_sub((at + i + 3) as u16)
    };
    let shape = step[..5] == [0xf6, 0x85, 0xfd, 0x00, bit]
        && step[7..11] == [0xfe, 0x8d, 0x1c, 0x01]
        && step[13..18] == [0xc6, 0x85, 0x1c, 0x01, wait]
        && calls(18, DEPLOY_AT)
        && step[21] == 0x73
        && calls(23, DUMP_AT);
    if !shape {
        return Err(bad(at));
    }
    let ticks = ds + usize::from(operand(image, crate::sensing::TICKS_AT[1], &[0xff, 0x97])?);
    let found: Vec<u8> = (0..crate::sensing::TICKS as u8)
        .filter(|&n| {
            crate::maproom::word(image, ticks + 2 * usize::from(n)).ok() == Some(at as u16)
        })
        .collect();
    let [step] = found[..] else {
        return Err(invalid(
            ticks,
            "the dump not once in the scorpion's tick table",
        ));
    };
    Ok(Dump {
        bit,
        wait,
        step,
        on: operand(image, DUMP_AT + 0x18, &[0xbe])?,
        off: operand(image, DUMP_AT + 0xc, &[0xbe])?,
    })
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Net, FormatError> {
    let b = image
        .get(LINES_AT..LINES_AT + 21)
        .ok_or_else(|| invalid(LINES_AT, "past the end of the image"))?;
    let shape = b[0] == 0xe8
        && b[3..7] == [0x8a, 0x45, 0x30, 0xba]
        && [b[9], b[11], b[13], b[15], b[17], b[19]] == [0x3c, 0x74, 0xb2, 0x3c, 0x74, 0xb6];
    if !shape || b[7] == 0 || b[14] == 0 {
        return Err(invalid(LINES_AT, "not the net programs' grid"));
    }
    let deploy = starts(image, DEPLOY_AT, &[0xf6, 0x85, 0xfd, 0x00, 0x80, 0x75])
        && starts(image, DEPLOY_AT + 7, &[0x80, 0xbd, 0x18, 0x01, 0x00, 0x74])
        && operand(image, DEPLOY_AT + 14, &[0xe8])? == (MAKE_AT as u16).wrapping_sub(0x9b12);
    if !deploy || !starts(image, MAKE_AT, &[0x57, 0xb8, 0x04, 0x00, 0xe8]) {
        return Err(invalid(DEPLOY_AT, "not the relay's deployment"));
    }
    let sound = operand(image, DEPLOY_SOUND_AT, &[0xb8])?;
    if sound % 2 != 0 || sound > 0x1ff {
        return Err(invalid(DEPLOY_SOUND_AT, "not a sound event"));
    }
    Ok(Net {
        programs: [b[10], b[16]],
        lines: [
            Line {
                mask: b[7],
                probe: b[8] != 0,
            },
            Line {
                mask: b[14],
                probe: b[8] != 0,
            },
            Line {
                mask: b[14],
                probe: b[20] != 0,
            },
        ],
        sound: (sound / 2) as u8,
        no_relay: operand(image, NO_RELAY_AT, &[0xbe])?,
        deployed: operand(image, DEPLOYED_AT, &[0xbe])?,
        radius: operand(image, RADIUS_AT, &[0xc7, 0x44, 0x14])?,
        load: load(image)?,
        cargo: [
            operand(image, CARGO_AT, &[0xbe])?,
            match image.get(CARGO_AT + 3..CARGO_AT + 10) {
                Some([0x80, 0xbd, 0x18, 0x01, 0x00, 0x75, 0x03]) => {
                    operand(image, CARGO_AT + 10, &[0xbe])?
                }
                _ => return Err(invalid(CARGO_AT, "not the scorpion's C")),
            },
        ],
        dump: dump(image, ds)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x1_0000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; 0x2_0000];
        crate::fixtures::synthetic_net(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let net = read_image(&synthetic(), DS).unwrap();
        assert_eq!(net.programs, [13, 12]);
        assert_eq!(
            net.line(13),
            Line {
                mask: 0xfe,
                probe: false
            }
        );
        assert_eq!(
            net.line(12),
            Line {
                mask: 0xfc,
                probe: false
            }
        );
        assert_eq!(
            net.line(14),
            Line {
                mask: 0xfc,
                probe: true
            }
        );
        assert_eq!((net.sound, net.no_relay, net.deployed), (25, 0x50, 0x51));
        let l = net.load;
        assert_eq!((l.full, l.speed, l.reach), (10, 0x20, 0x30));
        assert_eq!(
            [l.loaded, l.damaged, l.none_near, l.too_full, l.too_fast],
            [0x52, 0x53, 0x54, 0x55, 0x56]
        );
        assert_eq!(net.cargo, [0x57, 0x58]);
        let d = net.dump;
        assert_eq!(
            (d.bit, d.wait, d.step, d.on, d.off),
            (0x10, 3, 5, 0x59, 0x5a)
        );
        insta::assert_debug_snapshot!(net);
    }

    #[test]
    fn another_build_is_refused() {
        for (at, value) in [
            (LINES_AT + 3, 0x8b),
            (LINES_AT + 7, 0),
            (LINES_AT + 19, 0xb2),
            (DEPLOY_AT + 2, 0xfc),
            (DEPLOY_AT + 15, 0x58),
            (MAKE_AT + 2, 0x05),
            (DEPLOY_SOUND_AT + 1, 0x33),
            (NO_RELAY_AT, 0xbf),
            (DEPLOYED_AT, 0xbf),
            (RADIUS_AT + 2, 0x16),
            (LOAD_AT + 0x1c, 0x05),
            (LOAD_AT + 0x22, 0x00),
            (LOAD_MESSAGES_AT[2], 0xbf),
            (CARGO_AT + 8, 0x74),
            (DUMP_AT + 9, 0x20),
            (DUMP_AT + 0x18, 0xbf),
            (DUMP_STEP_AT + 4, 0x20),
            (DUMP_STEP_AT + 19, 0x00),
            (DUMP_STEP_AT + 24, 0x00),
            (DS + 0x9720 + 10, 0x00),
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables: DENSE NET (13) on even cells, SPARSE NET (12) on cells of
    /// four, both stepping along one diagonal and across, PROBE NET along both
    /// (`docs/sim.md`, "Relay nets"); the messages are text.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_net_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let ds = crate::sequence::data_segment(&exe).unwrap();
        let net = read_image(&exe.image, ds).unwrap();
        assert_eq!(net.programs, [13, 12]);
        let masks = net.lines.map(|l| (l.mask, l.probe));
        assert_eq!(masks, [(0xfe, false), (0xfc, false), (0xfc, true)]);
        assert_eq!((net.sound, net.radius), (25, 0x500));
        let l = net.load;
        assert_eq!((l.full, l.speed, l.reach), (12, 0x10, 0x14));
        // The deployer's refusal is deploying's.
        assert_eq!(l.damaged, net.no_relay);
        let d = net.dump;
        assert_eq!((d.bit, d.wait, d.step), (0x10, 10, 5));
        let motion = crate::motion::read(&exe).unwrap();
        let texts = [
            net.no_relay,
            net.deployed,
            l.loaded,
            l.none_near,
            l.too_full,
        ];
        let texts = texts.into_iter().chain([l.too_fast, d.on, d.off]);
        for at in texts.chain(net.cargo) {
            let text: Vec<u8> = (at..)
                .map(|o| motion.strings.byte(o))
                .take_while(|&b| b != 0)
                .collect();
            assert!(text.len() > 2, "{at:#x}");
            assert!(
                text.iter()
                    .all(|&b| b == 0x0c || b == 0x0d || (0x20..0x7f).contains(&(b & 0x7f))),
                "{at:#x}: {:?}",
                text.iter()
                    .filter(|&&b| (b & 0x7f) < 0x20)
                    .collect::<Vec<_>>()
            );
        }
    }
}
