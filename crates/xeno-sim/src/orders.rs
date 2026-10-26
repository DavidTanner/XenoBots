//! The robots' orders (`docs/screens.md`, "Orders"): the program each was given, the
//! one it runs, and whether its pilot drives it by hand. All three are in a `.usg`
//! file's `DCBS` objects (`docs/formats/usg.md`, `usg::Robot`): the program given at
//! +0x2e, the one running at +0x30 and the manual switch in bit 7 of +0x23.
//!
//! [`OrderBook`] is mission state the player changes, so it lives in the sim. The
//! program table it consults is game data read from the user's `xeno.exe`, so
//! [`Programs`] stays a parser's output in `xeno_formats::orders` and is re-exported
//! here with [`RobotKind`].

use std::collections::BTreeMap;

pub use xeno_formats::orders::{Program, Programs, RobotKind};
use xeno_formats::usg::{self, Object, Scenario, Side};

use crate::hash::{StateHash, StateHasher};
use crate::world::side_code;

/// A robot's orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orders {
    /// The program it was given (+0x2e).
    pub ordered: u8,
    /// The program it runs (+0x30): the one given, or a state it entered by itself.
    /// The console shows this one, and the next and previous programs are counted from
    /// it.
    pub program: u8,
    /// Its pilot drives it; the program waits.
    pub manual: bool,
}

impl Orders {
    /// Program `program`, just given, in automatic.
    pub fn given(program: u8) -> Orders {
        Orders {
            ordered: program,
            program,
            manual: false,
        }
    }

    /// A robot object's orders as saved (+0x2e, +0x30 and bit 7 of +0x23), if it is a
    /// robot with the stock layout.
    pub fn of(object: &Object) -> Option<Orders> {
        let robot = object.robot()?;
        Some(Orders {
            ordered: robot.program_given,
            program: robot.program_running,
            manual: robot.mode & usg::Robot::MANUAL != 0,
        })
    }
}

/// A robot in an [`OrderBook`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Robot {
    pub kind: RobotKind,
    pub side: Side,
    pub orders: Orders,
}

/// An order changed: which robot, and its orders before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub slot: u16,
    pub before: Orders,
    pub after: Orders,
}

/// The orders of every robot in a mission, by object slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderBook {
    robots: BTreeMap<u16, Robot>,
}

impl OrderBook {
    /// The robots of `scenario` with their orders as loading leaves them: the saved
    /// program given and manual switch, and the program given running (the loader
    /// gives each robot its program anew, `docs/formats/usg.md`, "Loading"). Robots
    /// without the stock layout are left out.
    pub fn from_scenario(scenario: &Scenario) -> OrderBook {
        let robots = scenario
            .objects
            .iter()
            .flatten()
            .filter_map(|o| {
                let saved = Orders::of(o)?;
                let robot = Robot {
                    kind: RobotKind::of(o.kind)?,
                    side: o.side(),
                    orders: Orders {
                        program: saved.ordered,
                        ..saved
                    },
                };
                Some((o.slot, robot))
            })
            .collect();
        OrderBook { robots }
    }

    #[cfg(test)]
    pub(crate) fn insert(&mut self, slot: u16, robot: Robot) {
        self.robots.insert(slot, robot);
    }

    pub fn get(&self, slot: u16) -> Option<&Robot> {
        self.robots.get(&slot)
    }

    /// The robots of `side`, by slot.
    pub fn side(&self, side: Side) -> impl Iterator<Item = (u16, &Robot)> {
        self.robots
            .iter()
            .filter(move |(_, r)| r.side == side)
            .map(|(&slot, r)| (slot, r))
    }

    fn change(&mut self, slot: u16, f: impl FnOnce(&Robot) -> Option<Orders>) -> Option<Change> {
        let robot = self.robots.get_mut(&slot)?;
        let before = robot.orders;
        let after = f(robot)?;
        robot.orders = after;
        (after != before).then_some(Change {
            slot,
            before,
            after,
        })
    }

    /// The next program offered to the robot; choosing a program also hands the robot
    /// back to it (`ui_program_panel_on_mouse`, F4), and gives it both as the program
    /// given and the one running (`order_set_program`).
    pub fn select_next(&mut self, slot: u16, programs: &Programs) -> Option<Change> {
        self.change(slot, |r| {
            let program = programs.next(r.kind, r.orders.program)?;
            Some(Orders::given(program))
        })
    }

    /// The previous program offered to the robot, as [`OrderBook::select_next`].
    pub fn select_previous(&mut self, slot: u16, programs: &Programs) -> Option<Change> {
        self.change(slot, |r| {
            let program = programs.previous(r.kind, r.orders.program)?;
            Some(Orders::given(program))
        })
    }

    /// The robot is given `program`, which runs (`order_set_program`; the script
    /// starts with `xeno_sim::script::set_program`).
    pub fn set_program(&mut self, slot: u16, program: u8) -> Option<Change> {
        self.change(slot, |r| {
            Some(Orders {
                manual: r.orders.manual,
                ..Orders::given(program)
            })
        })
    }

    /// The robot's script enters (or leaves for) `state`: the program running only.
    pub fn enter_state(&mut self, slot: u16, state: u8) -> Option<Change> {
        self.change(slot, |r| {
            Some(Orders {
                program: state,
                ..r.orders
            })
        })
    }

    /// Hands the robot to its pilot or back to its program (F3, F2).
    pub fn set_manual(&mut self, slot: u16, manual: bool) -> Option<Change> {
        self.change(slot, |r| Some(Orders { manual, ..r.orders }))
    }
}

impl StateHash for OrderBook {
    fn state_hash(&self, h: &mut StateHasher) {
        h.u64(self.robots.len() as u64);
        for (&slot, robot) in &self.robots {
            let Orders {
                ordered,
                program,
                manual,
            } = robot.orders;
            h.u16(slot);
            h.u8(robot.kind.index() as u8);
            h.u8(side_code(robot.side));
            h.bytes(&[ordered, program, u8::from(manual)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_mission_usg, synthetic_programs};

    const DS: usize = 0x20000;

    fn programs() -> Programs {
        let mut image = vec![0u8; DS + 0x9c00];
        synthetic_programs(&mut image, DS);
        xeno_formats::orders::read_image(&image, DS).unwrap()
    }

    #[test]
    fn orders_are_set_and_kept_per_robot() {
        let programs = programs();
        let scenario = xeno_formats::usg::parse(&synthetic_mission_usg()).unwrap();
        let mut book = OrderBook::from_scenario(&scenario);
        // Slots 2 (humanoid, program 1, manual), 5 (scorpion, program 2) and 7 (scout,
        // alien); the base and the relay are not robots.
        let slots: Vec<_> = book.side(Side::Human).map(|(s, _)| s).collect();
        assert_eq!(slots, [2, 5]);
        assert_eq!(
            book.get(2).unwrap().orders,
            Orders {
                ordered: 1,
                program: 1,
                manual: true
            }
        );
        // A robot saved running a state starts on the program it was given.
        let mut running = scenario.clone();
        let objects = running.objects.as_mut().unwrap();
        let usg::Body::Robot(r) = &mut objects[1].body else {
            panic!("slot 2 is a robot")
        };
        r.program_running = 24;
        assert_eq!(Orders::of(&objects[1]).unwrap().program, 24);
        let loaded = OrderBook::from_scenario(&running);
        assert_eq!(loaded.get(2).unwrap().orders, book.get(2).unwrap().orders);
        // Choosing a program hands the robot back to it.
        let change = book.select_next(2, &programs).unwrap();
        assert_eq!(change.after, Orders::given(2));
        assert_eq!(book.select_next(5, &programs).unwrap().after.program, 3);
        assert_eq!(book.select_previous(5, &programs).unwrap().after.program, 2);
        assert!(book.set_manual(5, true).unwrap().after.manual);
        assert_eq!(book.set_manual(5, true), None, "no change, no report");
        // Other robots keep theirs; unknown slots change nothing.
        assert_eq!(book.get(7).unwrap().orders.program, 2);
        assert_eq!(book.select_next(99, &programs), None);
        assert_eq!(book.get(2).unwrap().orders.program, 2);
        // The orders are part of the state's hash.
        let hash = |book: &OrderBook| {
            let mut h = StateHasher::default();
            book.state_hash(&mut h);
            h.finish()
        };
        let before = hash(&book);
        book.set_manual(2, true);
        assert_ne!(hash(&book), before);
    }

    /// Every stock robot runs a program in the user's table.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_stock_robots_run_known_programs() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = std::fs::read(dir.join("xeno.exe")).unwrap();
        let exe = xeno_formats::lzexe::unpack(&exe).unwrap();
        let programs = xeno_formats::orders::read(&exe).unwrap();
        for name in ["htrain.usg", "scen00a.usg"] {
            let file = std::fs::read(dir.join(name)).unwrap();
            let book = OrderBook::from_scenario(&xeno_formats::usg::parse(&file).unwrap());
            assert!(book.side(Side::Human).next().is_some(), "{name}");
            for side in [Side::Human, Side::Alien] {
                for (_, robot) in book.side(side) {
                    assert!(programs.get(robot.orders.program).is_some(), "{name}");
                }
            }
        }
    }
}
