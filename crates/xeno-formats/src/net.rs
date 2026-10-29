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

/// Reads the tables from a load image.
pub fn read_image(image: &[u8]) -> Result<Net, FormatError> {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; 0x10000];
        crate::fixtures::synthetic_net(&mut image);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let net = read_image(&synthetic()).unwrap();
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
        ] {
            let mut image = synthetic();
            image[at] = value;
            assert!(read_image(&image).is_err(), "{at:#x}");
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
        let net = read_image(&exe.image).unwrap();
        assert_eq!(net.programs, [13, 12]);
        let masks = net.lines.map(|l| (l.mask, l.probe));
        assert_eq!(masks, [(0xfe, false), (0xfc, false), (0xfc, true)]);
        assert_eq!((net.sound, net.radius), (25, 0x500));
        let motion = crate::motion::read(&exe).unwrap();
        for at in [net.no_relay, net.deployed] {
            let text: Vec<u8> = (at..)
                .map(|o| motion.strings.byte(o))
                .take_while(|&b| b != 0)
                .collect();
            assert!(text.len() > 2, "{at:#x}");
            assert!(
                text.iter()
                    .all(|&b| b == 0x0c || (0x20..0x7f).contains(&(b & 0x7f)))
            );
        }
    }
}
