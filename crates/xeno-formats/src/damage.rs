//! What the port reads from the user's `xeno.exe` to damage and destroy objects
//! (`docs/sim.md`, "Damage"): each robot kind's destroy threshold and the chance table
//! that picks which component a point of damage hits, with what each component
//! routine does; the base's and the relay's thresholds; and the explosions destroying
//! them leaves. Game data, read at run time through the instructions that use it,
//! after checking their bytes.

use crate::FormatError;
use crate::farewell::operand;
use crate::maproom::invalid;

/// `cmp al, threshold` then `ja` (destroy) in `sim_damage_humanoid`,
/// `sim_damage_scorpion` and `sim_damage_scout`, by kind.
pub(crate) const THRESHOLD_AT: [usize; 3] = [0xaf68, 0x94b7, 0xc6a1];
/// `mov bx, table` in the same routines: the chance table.
pub(crate) const TABLE_AT: [usize; 3] = [0xaf6f, 0x94be, 0xc6a8];
/// `cmp word [si + 0x2e], damage` in `sim_damage_base`.
pub(crate) const BASE_AT: (usize, &[u8]) = (0x62c3, &[0x81, 0x7c, 0x2e]);
/// `cmp byte [di + 0x33], damage` in `sim_damage_relay`.
pub(crate) const RELAY_AT: (usize, &[u8]) = (0xcfbf, &[0x80, 0x7d, 0x33]);
/// `mov ax, template` before `sim_spawn_explosion` in `sim_destroy_robot` and
/// `sim_damage_relay`.
pub(crate) const ROBOT_EXPLOSION_AT: usize = 0xde5c;
pub(crate) const RELAY_EXPLOSION_AT: usize = 0xcfc5;
/// `mov ax, kind` in `sim_spawn_explosion`: the object kind it makes.
pub(crate) const EXPLOSION_KIND_AT: usize = 0xe67d;
/// `mov ax, template` in `sim_step_base_end_script`'s routines: the explosions of a
/// destroyed base's end, steps 0 to 3, 4 to 7, 8 to 11, 12 to 19 and step 20.
pub(crate) const END_EXPLOSIONS_AT: [usize; 5] = [0xe6b3, 0xe6c3, 0xe6d3, 0xe6e3, 0xe6f3];
/// `mov word [si + 0x14], radius` for the small explosions
/// (`sim_spawn_scattered_explosion`) and the big one.
pub(crate) const END_RADII_AT: [usize; 2] = [0xe73b, 0xe6fc];
const RADIUS_OP: &[u8] = &[0xc7, 0x44, 0x14];
/// `cmp word [di + 0x30], step` in `sim_end_mission_with_result`: the step that ends
/// the mission.
pub(crate) const END_STEP_AT: (usize, &[u8]) = (0x68dd, &[0x83, 0x7d, 0x30]);

/// The most entries a chance table has before its chances pass 255 (the stock
/// tables have 10 and 12), and the most bits a component routine tries.
const MAX_ENTRIES: usize = 32;
const MAX_BITS: usize = 4;

/// What a component routine does to the robot hit, by its byte offsets in the
/// object (`docs/formats/usg.md`, "Object fields").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A fresh random word's low byte, masked, comes off the byte at `offset`,
    /// stopping at 0 (the helm, the power).
    Wear { offset: u16, mask: u8 },
    /// The first of these bits that is clear is set; all set, nothing (a weapon
    /// damaged, then destroyed).
    Bits(Vec<(u16, u8)>),
    /// Nothing.
    Nothing,
}

/// One robot kind's damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobotDamage {
    /// A hit of more points than this destroys the robot at once.
    pub destroy_over: u8,
    /// (chance out of 256, part) in table order; the chances add up past 255 at the
    /// last entry.
    pub parts: Vec<(u8, Part)>,
}

/// An explosion's template (`sim_init_explosion_from_template`): frames `first` to
/// `last`, each shown `ticks` ticks, and a heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Explosion {
    pub first: u8,
    pub last: u8,
    pub ticks: u8,
    pub heading: u16,
}

/// The damage tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    /// Humanoid, scorpion, scout.
    pub robots: [RobotDamage; 3],
    /// A base is destroyed once its damage reaches this.
    pub base_destroyed: u16,
    /// A relay is removed once its damage reaches this (or passes 255).
    pub relay_removed: u8,
    /// The kind of object an explosion is.
    pub explosion_kind: u16,
    /// The explosions a destroyed robot and a removed relay leave.
    pub robot_explosion: Explosion,
    pub relay_explosion: Explosion,
    /// The mission's end once a base is destroyed.
    pub base_end: BaseEnd,
}

/// What a destroyed base's end script makes (`docs/sim.md`, "The mission's end").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseEnd {
    /// The explosion of steps 0 to 3, 4 to 7, 8 to 11 and 12 to 19, then of step 20.
    pub explosions: [Explosion; 5],
    /// The radius (8.8) of the scattered explosions and of step 20's.
    pub radii: [u16; 2],
    /// The step at which the mission ends.
    pub last_step: u8,
}

fn byte(image: &[u8], at: usize) -> Result<u8, FormatError> {
    image
        .get(at)
        .copied()
        .ok_or_else(|| invalid(at, "past the end of the image"))
}

/// The part the component routine at `at` is: `ret`; `call` (the random word), `and
/// al, mask`, `sub [di + offset], al`; or `test [di + offset], bit`, `jnz`, `or [di +
/// offset], bit`, where the `jnz` goes to a `ret` or to the next such test.
fn part(image: &[u8], at: usize) -> Result<Part, FormatError> {
    let bytes = image
        .get(at..at + 12)
        .ok_or_else(|| invalid(at, "past the end of the image"))?;
    match *bytes {
        [0xc3, ..] => Ok(Part::Nothing),
        [0xe8, _, _, 0x24, mask, 0x28, 0x45, offset, ..] => Ok(Part::Wear {
            offset: offset.into(),
            mask,
        }),
        _ => {
            let mut bits = Vec::new();
            let mut at = at;
            while bits.len() < MAX_BITS {
                match *image.get(at..at + 12).unwrap_or_default() {
                    [
                        0xf6,
                        0x85,
                        lo,
                        hi,
                        bit,
                        0x75,
                        jump,
                        0x80,
                        0x8d,
                        lo2,
                        hi2,
                        bit2,
                    ] if (lo, hi, bit) == (lo2, hi2, bit2) => {
                        bits.push((u16::from_le_bytes([lo, hi]), bit));
                        at = (at + 7).wrapping_add_signed(isize::from(jump as i8));
                    }
                    _ => return Err(invalid(at, "not a component routine")),
                }
                if byte(image, at)? == 0xc3 {
                    return Ok(Part::Bits(bits));
                }
            }
            Err(invalid(at, "too many bits in a component routine"))
        }
    }
}

fn robot(image: &[u8], ds: usize, kind: usize) -> Result<RobotDamage, FormatError> {
    let at = THRESHOLD_AT[kind];
    let destroy_over = match image.get(at..at + 3) {
        Some(&[0x3c, n, 0x77]) => n,
        _ => return Err(invalid(at, "not the destroy threshold")),
    };
    let mut table = ds + usize::from(operand(image, TABLE_AT[kind], &[0xbb])?);
    let (mut parts, mut sum) = (Vec::new(), 0u8);
    loop {
        let chance = byte(image, table)?;
        let routine = crate::maproom::word(image, table + 1)?;
        parts.push((chance, part(image, usize::from(routine))?));
        let carry;
        (sum, carry) = sum.overflowing_add(chance);
        if carry {
            return Ok(RobotDamage {
                destroy_over,
                parts,
            });
        }
        if parts.len() == MAX_ENTRIES {
            return Err(invalid(table, "the chances never pass 255"));
        }
        table += 3;
    }
}

pub(crate) fn explosion(image: &[u8], ds: usize, at: usize) -> Result<Explosion, FormatError> {
    let t = ds + usize::from(operand(image, at, &[0xb8])?);
    Ok(Explosion {
        first: byte(image, t)?,
        last: byte(image, t + 1)?,
        ticks: byte(image, t + 2)?,
        heading: crate::maproom::word(image, t + 3)?,
    })
}

fn base_end(image: &[u8], ds: usize) -> Result<BaseEnd, FormatError> {
    let e = |i: usize| explosion(image, ds, END_EXPLOSIONS_AT[i]);
    let last_step = match image.get(END_STEP_AT.0..END_STEP_AT.0 + 4) {
        Some(b) if b[..3] == *END_STEP_AT.1 => b[3],
        _ => return Err(invalid(END_STEP_AT.0, "not the end script's last step")),
    };
    Ok(BaseEnd {
        explosions: [e(0)?, e(1)?, e(2)?, e(3)?, e(4)?],
        radii: [
            operand(image, END_RADII_AT[0], RADIUS_OP)?,
            operand(image, END_RADII_AT[1], RADIUS_OP)?,
        ],
        last_step,
    })
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Damage, FormatError> {
    let relay_removed = match image.get(RELAY_AT.0..RELAY_AT.0 + 4) {
        Some(b) if b[..3] == *RELAY_AT.1 => b[3],
        _ => return Err(invalid(RELAY_AT.0, "not the relay's threshold")),
    };
    Ok(Damage {
        robots: [
            robot(image, ds, 0)?,
            robot(image, ds, 1)?,
            robot(image, ds, 2)?,
        ],
        base_destroyed: operand(image, BASE_AT.0, BASE_AT.1)?,
        relay_removed,
        explosion_kind: operand(image, EXPLOSION_KIND_AT, &[0xb8])?,
        robot_explosion: explosion(image, ds, ROBOT_EXPLOSION_AT)?,
        relay_explosion: explosion(image, ds, RELAY_EXPLOSION_AT)?,
        base_end: base_end(image, ds)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0x9c00];
        crate::fixtures::synthetic_damage(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let damage = read_image(&synthetic(), DS).unwrap();
        assert_eq!(damage.robots.each_ref().map(|r| r.destroy_over), [0x50; 3]);
        let humanoid = &damage.robots[0].parts;
        assert_eq!(
            humanoid[0],
            (
                0x80,
                Part::Wear {
                    offset: 0x24,
                    mask: 7
                }
            )
        );
        assert_eq!(humanoid[1].1, Part::Bits(vec![(0xdc, 1), (0xf6, 0x40)]));
        assert_eq!(damage.robots[1].parts[0].1, Part::Nothing);
        assert_eq!((damage.base_destroyed, damage.relay_removed), (300, 20));
        assert_eq!(damage.explosion_kind, 13);
        assert_eq!(damage.robot_explosion.last, 9);
        let end = &damage.base_end;
        assert_eq!(
            (end.explosions[4].last, end.radii, end.last_step),
            (5, [0x2800, 0x4000], 0x24)
        );
        insta::assert_debug_snapshot!(damage);
    }

    #[test]
    fn another_build_is_refused() {
        for at in [
            THRESHOLD_AT[1],
            THRESHOLD_AT[2] + 2,
            TABLE_AT[0],
            BASE_AT.0 + 1,
            RELAY_AT.0,
            ROBOT_EXPLOSION_AT,
            EXPLOSION_KIND_AT,
            END_EXPLOSIONS_AT[3],
            END_RADII_AT[1] + 2,
            END_STEP_AT.0 + 1,
            // A component routine's `or` of another bit, and an unknown routine.
            0x803b,
            0x8020,
        ] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
    }

    /// The user's tables have the shapes `docs/sim.md` describes: every robot is
    /// destroyed over 80, every table's last chance always picks, and every part
    /// touches a component the sim keeps.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_damage_reads() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let damage = crate::motion::read(&exe).unwrap().damage;
        let (mut wear, mut bits) = (0, 0);
        for robot in &damage.robots {
            assert_eq!(robot.destroy_over, 80);
            assert_eq!(robot.parts.last().unwrap().0, 0xff);
            for (_, part) in &robot.parts {
                match part {
                    Part::Wear { offset, .. } => {
                        wear += 1;
                        assert!([0x24, 0x4e].contains(offset));
                    }
                    Part::Bits(b) => {
                        bits += 1;
                        assert!(b.iter().all(|(o, _)| [0xdc, 0xec, 0xf6, 0xfd].contains(o)));
                    }
                    Part::Nothing => {}
                }
            }
        }
        assert!(wear > 0 && bits > 0);
        assert_eq!((damage.base_destroyed, damage.relay_removed), (300, 20));
        let e = damage.robot_explosion;
        assert!(e.first < e.last && e.ticks > 0);
        let end = &damage.base_end;
        assert!(
            end.explosions
                .iter()
                .all(|e| e.first < e.last && e.ticks > 0)
        );
        assert_eq!(end.last_step, 0x24);
        assert!(end.radii[0] < end.radii[1]);
    }
}
