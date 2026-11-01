//! The relay net (`docs/sim.md`, "Relay nets"): the grid of cells the net programs
//! walk, the line of cells from a robot to its destination (+0x10a..+0x114), and
//! whether a relay stands in a cell. The grids are read from the user's `xeno.exe`
//! (`xeno_formats::net`, in `Tables::net`).

use xeno_formats::net::Line;
use xeno_formats::usg::{self, ObjectKind, Side};

use crate::motion::{Event, Tables};
use crate::random::Random;
use crate::relay::Relay;
use crate::world::{Damage, Entity, EntityId, Fixed, Position, State, World};

/// +0xfd bit 7: the relay deployer is damaged.
const DEPLOYER_DAMAGED: u8 = 0x80;
/// +0xdc bit 3: a relay was deployed (the console's frames).
const DEPLOYED: u8 = 0x08;

/// A scorpion's line of cells (+0x10a..+0x114): the cell it is at, the line's two
/// lengths (+0x10e, +0x110), the error between them (+0x112), and the cell's step
/// with the signs of the two directions (+0x114: bits 1 and 2 the step, bit 7 and
/// bit 6 the first and second direction negative).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grid {
    pub cell: (i16, i16),
    pub lengths: (u16, u16),
    pub error: i16,
    pub step: u8,
}

impl Grid {
    /// The line as a file keeps it, 11 bytes from +0x10a.
    pub fn of(bytes: &[u8; 11]) -> Grid {
        let w = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        Grid {
            cell: (w(0) as i16, w(2) as i16),
            lengths: (w(4), w(6)),
            error: w(8) as i16,
            step: bytes[10],
        }
    }
}

/// The cell `at` is in on a grid of `mask` (`1000:49a1`): x and y rounded to the
/// nearest multiple of the mask's lowest bit (the low byte masked), then the bit
/// cleared in both where they differ in it, so cells lie on a diagonal lattice.
pub fn cell(mask: u8, at: Position) -> (i16, i16) {
    let step = mask & mask.wrapping_neg();
    let half = i16::from(step >> 1);
    let round = |v: i16| {
        let v = v.wrapping_add(half);
        (v as u16 & (0xff00 | u16::from(mask))) as i16
    };
    let (x, y) = (round(at.x.floor()), round(at.y.floor()));
    let differ = ((x ^ y) as u8 & step) as i16;
    (x & !differ, y & !differ)
}

/// A relay's cell (`1000:cca8`, and `2000:7faa` for every relay): x and y whole,
/// rounded up to even, and bit 1 cleared in both where they differ in it.
pub fn lattice(at: Position) -> (i16, i16) {
    let even = |v: i16| v.wrapping_add(1) & !1;
    let (x, y) = (even(at.x.floor()), even(at.y.floor()));
    let differ = (x ^ y) & 2;
    (x & !differ, y & !differ)
}

/// Whether a relay of either side stands in `cell` (`2000:7faa`).
pub fn relay_at(world: &World, cell: (i16, i16)) -> bool {
    world
        .iter()
        .any(|(_, e)| e.kind == ObjectKind::Relay && lattice(e.position) == cell)
}

/// The line of cells from `from` to `to` (opcode 0x7e, `1000:5258`): its start cell,
/// lengths, error and step, Bresenham's in the lattice's diagonal coordinates.
pub fn start(line: Line, from: Position, to: Position) -> Grid {
    let (start, end) = (cell(line.mask, from), cell(line.mask, to));
    let a = end.0.wrapping_sub(start.0) >> 1;
    let b = end.1.wrapping_sub(start.1) >> 1;
    let v = b.wrapping_sub(a);
    let mut u = a.wrapping_add(b);
    if !line.probe {
        u = u.wrapping_add(v);
    }
    let (mut u, mut v, mut step) = (u, v, 2u8);
    if line.mask & 2 == 0 {
        (u, v, step) = (u >> 1, v >> 1, 4);
    }
    if u < 0 {
        step |= 0x80;
    }
    if v < 0 {
        step |= 0x40;
    }
    let (u, v) = (u.unsigned_abs(), v.unsigned_abs());
    Grid {
        cell: start,
        lengths: (u, v),
        error: if u >= v {
            (u >> 1) as i16
        } else {
            ((v >> 1) as i16).wrapping_neg()
        },
        step,
    }
}

/// The next cell of the line towards `to` (opcode 0x7f, `1000:52d8`). Returns `None`
/// at the end (the cell is `to`'s: the carry set), else the last coordinate the step
/// wrote, which sets the zero and sign flags.
pub fn advance(grid: &mut Grid, line: Line, to: Position) -> Option<i16> {
    if grid.cell == cell(line.mask, to) {
        return None;
    }
    let signed = |step: u8, negative: bool| {
        let s = i16::from(step & 6);
        if negative { -s } else { s }
    };
    let first = signed(grid.step, grid.step & 0x80 != 0);
    let second = signed(grid.step, grid.step & 0x40 != 0);
    let (x, y) = (&mut grid.cell.0, &mut grid.cell.1);
    if grid.error >= 0 {
        grid.error = grid.error.wrapping_sub(grid.lengths.1 as i16);
        *x = x.wrapping_add(first);
        *y = y.wrapping_add(first);
        Some(*y)
    } else if line.probe {
        grid.error = grid.error.wrapping_add(grid.lengths.0 as i16);
        *y = y.wrapping_add(second);
        *x = x.wrapping_sub(second);
        Some(*x)
    } else {
        grid.error = grid.error.wrapping_add(grid.lengths.0 as i16);
        *x = x.wrapping_sub(second.wrapping_mul(2));
        Some(*x)
    }
}

/// The relays robot `id` can deploy (`1000:9aec`): those it carries (+0x118), none
/// while its deployer is damaged.
pub fn relays(world: &World, id: EntityId) -> u8 {
    match world.get(id) {
        Some(Entity {
            damage: Damage::Robot(c),
            ..
        }) if c.equipment & DEPLOYER_DAMAGED != 0 => 0,
        Some(Entity {
            state: State::Robot(m),
            ..
        }) => m.relays,
        _ => 0,
    }
}

/// Robot `id` deploys a relay where it stands (`sim_deploy_relay`, `1000:9b01`; the
/// scorpion's U and opcode 0x83): with one to deploy and room for it, a relay is made
/// (`1000:cc69`), the "deployed" message posted, one fewer carried, +0xdc bit 3 set
/// and the sound played for the robot; else the "no relay" message. Returns whether
/// one was deployed.
pub fn deploy(
    world: &mut World,
    id: EntityId,
    tables: &Tables,
    random: &mut Random,
    events: &mut Vec<Event>,
) -> bool {
    let net = &tables.net;
    let at = world.get(id).map(|e| (e.position, e.z));
    let made = relays(world, id) > 0
        && at.is_some_and(|(at, z)| make(world, at, z, tables, random).is_some());
    let Some(e) = world.get_mut(id) else {
        return false;
    };
    if let (true, Damage::Robot(c)) = (made, &mut e.damage) {
        c.weapons |= DEPLOYED;
    }
    let State::Robot(m) = &mut e.state else {
        return false;
    };
    if !made {
        m.panel.post(net.no_relay);
        return false;
    }
    m.panel.post(net.deployed);
    m.relays -= 1;
    events.push(Event::Sound {
        event: net.sound,
        only_for: Some(id),
    });
    true
}

/// `1000:cc69`: a relay in the lowest free slot at `at`, height `z`, set up
/// (`1000:cc2a`) folded, unpowered, on no side (+0x16 only `TARGETABLE`), its clock a random
/// byte, with its look. `None` with no room.
pub(crate) fn make(
    world: &mut World,
    at: Position,
    z: Fixed,
    tables: &Tables,
    random: &mut Random,
) -> Option<EntityId> {
    let slot = crate::damage::alloc_small(world)?;
    let (state, model_height) = Relay::made(random.word() as u8, tables);
    let relay = Entity {
        kind: ObjectKind::Relay,
        side: Side::Human,
        position: at,
        z,
        model_height,
        heading: 0,
        radius: tables.net.radius,
        damage: Damage::Taken(0),
        // `1000:cc2a`: `TARGETABLE`, on no side.
        flags: usg::flag::TARGETABLE,
        status: 0,
        state: State::Relay(state),
    };
    world.insert(slot, relay);
    Some(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Damage, Entity, EntityId, Fixed, State};
    use xeno_formats::usg::Side;

    fn at(x: f64, y: f64) -> Position {
        Position {
            x: Fixed((x * 65536.0) as i32),
            y: Fixed((y * 65536.0) as i32),
        }
    }

    const DENSE: Line = Line {
        mask: 0xfe,
        probe: false,
    };
    const SPARSE: Line = Line {
        mask: 0xfc,
        probe: false,
    };
    const PROBE: Line = Line {
        mask: 0xfc,
        probe: true,
    };

    #[test]
    fn cells_lie_on_a_diagonal_lattice() {
        // Rounded to even, then bit 1 cleared in both where x and y differ in it.
        assert_eq!(cell(0xfe, at(130.4, 128.0)), (128, 128));
        assert_eq!(cell(0xfe, at(131.0, 129.9)), (132, 128));
        assert_eq!(cell(0xfe, at(126.9, 126.9)), (126, 126));
        // Cells of four: rounded to fours, then bit 2 cleared where they differ.
        assert_eq!(cell(0xfc, at(129.0, 133.0)), (128, 128));
        assert_eq!(cell(0xfc, at(134.0, 134.0)), (136, 136));
        // The low byte only is masked: the high byte carries the add.
        assert_eq!(cell(0xfe, at(255.5, 255.5)), (256, 256));
        // A relay's lattice: whole numbers rounded up to even.
        assert_eq!(lattice(at(127.9, 130.0)), (128, 128));
        assert_eq!(lattice(at(129.0, 130.5)), (130, 130));
        assert_eq!(lattice(at(129.0, 131.0)), (128, 132));
        assert_eq!(lattice(at(130.0, 130.0)), (130, 130));
    }

    #[test]
    fn a_relay_stands_in_its_lattice_cell() {
        let mut world = World::default();
        assert!(!relay_at(&world, (128, 128)));
        let relay = Entity {
            kind: ObjectKind::Relay,
            side: Side::Alien,
            position: at(128.6, 129.0),
            z: Fixed(0),
            model_height: 0,
            heading: 0,
            radius: 0x500,
            damage: Damage::Taken(0),
            flags: 0,
            status: 0,
            state: State::None,
        };
        world.insert(EntityId(9), relay.clone());
        assert!(relay_at(&world, (128, 128)));
        assert!(!relay_at(&world, (130, 130)));
        // Only relays count.
        world.insert(
            EntityId(9),
            Entity {
                kind: ObjectKind::Obstacle,
                ..relay
            },
        );
        assert!(!relay_at(&world, (128, 128)));
    }

    /// Every cell from `from` to `to`, the start's included.
    fn walk(line: Line, from: Position, to: Position) -> Vec<(i16, i16)> {
        let mut g = start(line, from, to);
        let mut cells = vec![g.cell];
        while advance(&mut g, line, to).is_some() {
            cells.push(g.cell);
            assert!(cells.len() < 64, "{cells:?}");
        }
        cells
    }

    #[test]
    fn lines_walk_cell_by_cell_to_the_destination() {
        let from = at(128.0, 128.0);
        // DENSE NET: a diagonal step of two, or a step of four across.
        assert_eq!(
            walk(DENSE, from, at(128.0, 132.0)),
            [(128, 128), (130, 130), (132, 132), (128, 132)]
        );
        assert_eq!(
            walk(DENSE, from, at(132.0, 128.0)),
            [(128, 128), (132, 128)]
        );
        assert_eq!(
            walk(DENSE, from, at(134.0, 136.0)),
            [
                (128, 128),
                (130, 130),
                (132, 132),
                (134, 134),
                (130, 134),
                (132, 136)
            ]
        );
        // SPARSE NET: the same on cells of four.
        assert_eq!(
            walk(SPARSE, from, at(136.0, 136.0)),
            [(128, 128), (132, 132), (136, 136)]
        );
        // PROBE NET: diagonal steps of four either way.
        assert_eq!(
            walk(PROBE, from, at(136.0, 128.0)),
            [(128, 128), (132, 132), (136, 128)]
        );
        // Every step keeps the cell on its lattice.
        for (line, to) in [(DENSE, at(120.0, 141.0)), (PROBE, at(101.0, 115.0))] {
            let cells = walk(line, from, to);
            assert_eq!(*cells.last().unwrap(), cell(line.mask, to));
            for c in cells {
                assert_eq!(cell(line.mask, at(f64::from(c.0), f64::from(c.1))), c);
            }
        }
    }

    #[test]
    fn a_file_s_line_reads_as_words() {
        let bytes = [0x80, 0, 0x82, 0, 4, 0, 2, 0, 0xff, 0xff, 0x42];
        let g = Grid::of(&bytes);
        assert_eq!(
            g,
            Grid {
                cell: (128, 130),
                lengths: (4, 2),
                error: -1,
                step: 0x42,
            }
        );
    }
}
