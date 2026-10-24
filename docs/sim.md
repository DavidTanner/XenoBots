# The simulation (T-050)

`crates/xeno-sim` holds the game's rules. It has no window, audio or clock, so it is
tested headless in CI. It depends only on `xeno-formats`.

## API

- **`Sim`** is the whole state: the retraces run, the random word, the held keys and
  mouse, and the `World`. `Sim::new(seed)` starts an empty world;
  `Sim::with_random(random)` starts one that carries on a random word already in use.
- **`Sim::tick(events)`** runs one retrace with the input that arrived for it
  (`docs/game-loop.md`: the sim steps once per retrace, at the mode's rate).
  `Sim::run(log, n)` ticks until `n` retraces have run, each with its events from an
  `InputLog`. For now a tick only counts the retrace and tracks held keys and
  buttons; the rules come with T-051 on.
- **`Sim::state_hash()`** is a 64-bit hash of everything in the state.
- **`input`**: `ScanCode`, `InputEvent`, `Keys`, `Mouse` and `InputLog`, moved from
  `xeno-app` (`docs/game-loop.md`, "Input"). `xeno-app` still names them
  `crate::input`.
- **`Random`** is the original's random word (`docs/screens.md`, "Campaign rules":
  four shift registers with taps 0xb400). The original seeds it from the timer chip;
  the port seeds it from the run's seed through a small linear congruential step that
  makes four odd words. The words after that follow the original exactly.
- **`Rate`**: the retrace rates as exact fractions (`docs/game-loop.md`, "The
  clock"), moved from `xeno-app`'s `clock.rs`. The host's `Clock` steps with it.
  `Rate::hz` is a float for logs only.
- **`orders`**: `OrderBook` (the robots' orders by object slot, changed only through
  `select_next`, `select_previous` and `set_manual`, each returning the change),
  `Orders`, `Robot` and `Change`, moved unchanged from `xeno_formats::orders`. They
  are mission state the player changes, so they belong to the sim, and the world
  holds an `OrderBook` in its hash. The program table, `Programs`, stays in
  `xeno_formats::orders`: it is read from the user's `xeno.exe` by a parser, and
  `xeno-tool` and the views' tables use it too. `xeno_sim::orders` re-exports it
  with `RobotKind`, so `xeno-app` takes the orders' types from the sim and only
  calls the parser in `xeno-formats`. The
  mission still keeps its own `OrderBook`; it moves into the mission's `Sim` with
  T-051.
- **`World`** holds the mission's objects, by `EntityId` (the object's slot in the
  original's 240-entry object table). An `Entity` is a kind and side
  (`xeno_formats::usg::{ObjectKind, Side}`) and a `Position` of two 16.16 `Fixed`
  numbers, the fields of a `.usg` `DCBS` object, so T-051 can fill it from
  `usg::Object` field for field. It also holds the `OrderBook`.

## The run's seed

`xeno-app --seed N` sets the seed. Without it the seed comes from the system clock, as
the original's comes from the timer, and it is logged at start-up so a run can be
repeated. The original has one random word for the whole run. The port's campaign
holds it today (the alert rises and the calendar); a mission's `Sim` should take it
over with `Sim::with_random` and hand it back, rather than start a second one.

## Determinism rules

The same seed, world and input log must give the same state and hash on every
machine, OS and toolchain. So, in `xeno-sim`:

- **No hash-map iteration.** Collections that are walked are `BTreeMap`s or `Vec`s.
  The world is a `BTreeMap` by slot, so objects are always visited in slot order,
  whatever order they were added in.
- **No wall-clock time.** Time is the retrace count. The host decides when a retrace
  is due (`xeno-app`'s `Clock`) and passes in its input.
- **No floats in state.** Positions are 16.16 fixed point, as in the original. Floats
  may appear only for display, outside the sim.
- **One source of chance**, the random word, seeded by the host.
- **A stable hash.** `state_hash` is FNV-1a over each field written in a fixed order as
  little-endian bytes (`hash::StateHasher`), not `std::hash`, whose output may change
  between Rust versions. New state goes into the hash through `hash::StateHash`.

The property test `crates/xeno-sim/tests/determinism.rs` runs random worlds and input
logs from `proptest`: the same seed and log give equal states and hashes, and another
seed, one more key press or one more retrace gives another hash.

## Not settled

- **The world's size and edges.** The `.usg` files have no map grid, and the world's
  extent and what happens at its edge are open until T-059. The sim invents neither:
  there is no grid, and positions are not limited. Movement (T-052) and the mission
  render (T-056) need T-059.
- What the original keeps per object beyond kind, side and position is mostly
  unknown (T-058); entities grow as those fields are named.
