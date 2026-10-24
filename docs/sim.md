# The simulation (T-050, T-051)

`crates/xeno-sim` holds the game's rules. It has no window, audio or clock, so it is
tested headless in CI. It depends only on `xeno-formats`.

## API

- **`Sim`** is the whole state: the retraces run, the random word, the held keys and
  mouse, and the `World`. `Sim::new(seed)` starts an empty world;
  `Sim::with_random(random)` starts one that carries on a random word already in use.
  `Sim::load(scenario, random)` starts a mission: the world from a `.usg` scenario
  (below, "Loading a scenario") and the run's random word.
- **`Sim::park()`** stops a mission for now and returns a `Parked` and the random word;
  `Parked::resume(random)` gives the word back and returns the `Sim`. A parked mission
  has no word, so it cannot tick or draw. **`Sim::end()`** ends a mission and returns
  the word. See "The run's random word".
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
  numbers, the fields of a `.usg` `DCBS` object. It also holds the `OrderBook`.
  `World::census(side)` counts one side's objects by kind.

## Loading a scenario

`World::from_scenario(&usg::Scenario)` (T-051) copies each `DCBS` object field for
field: its slot as the `EntityId`, its kind, its side (bit 3 of +0x16) and its x and y
as `Fixed`. The orders come from the same objects (`OrderBook::from_scenario`). Rules
for files the stock install does not have:

- An object too short to reach +0x16 has no side and is left out, as the orders leave
  out robots too short to hold them.
- Two objects in one slot: the later one is kept.
- No `DCBS` chunk: an empty world.

The bytes past +0x0c are not copied; they stay in the `usg::Scenario`, which
`xeno-app` keeps for the whole run (`mission::Assets::scenarios`). T-058 names them,
and the fields the sim needs then go into `Entity`.

**Units.** A unit is a robot: a humanoid, scorpion or scout. Those are the objects in
a side's unit list, which the consoles' Tab and the unit-name button step through
(`docs/screens.md`, "Switching views"), and the ones with orders. Bases, relays and
obstacles are not units. `Census::units()` counts the robots; the census also counts
bases, relays, obstacles and unnamed kinds. For `scen00a` the player has 5 units (3
humanoids, a scorpion and a scout), and the alien side 10.

The test `real_assets_every_scenario_loads_with_its_units` loads every stock scenario
and checks: every object is loaded, one per slot; each side's count of each kind is
the parser's; each side has one base (`docs/formats/usg.md`); every unit has orders;
a side has at most 38 units (its unit list); and the robots and bases fit the
40-object pool, the relays and obstacles the 200-object one (`usg.md`, `DCBS`).

## The run's random word

`xeno-app --seed N` sets the seed. Without it the seed comes from the system clock, as
the original's comes from the timer, and it is logged at start-up so a run can be
repeated.

The original has one random word for the whole run (`DS:0x1bf6`), which the campaign
(the alert rises and the calendar) and the mission both draw from. The port moves it
instead of copying it, so there is never a second one:

- A mission takes the word with `Sim::load` (a new mission) or `Parked::resume` (the
  loaded scenario played again).
- It gives it back with `Sim::end` (won or lost) or `Sim::park` (no result: the
  original keeps the mission's objects, and the map room's play resumes it).

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
