# The simulation (T-050, T-051, T-058, T-059)

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
  buttons; the rules come with T-052 on.
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
  calls the parser in `xeno-formats`. `xeno-app`'s mission keeps no orders of its
  own: they are in its `Sim`'s world, and it changes them only through the sim.
- **`World`** holds the mission's objects, by `EntityId` (the object's slot in the
  original's 240-entry object table). An `Entity` is a kind and side
  (`xeno_formats::usg::{ObjectKind, Side}`), a `Position` of two 16.16 `Fixed`
  numbers, a heading (65,536 to a turn), a radius (8.8) and its `Damage`: a robot's
  `Components` (helm and power percentages, the weapon, targeting and system damage
  bits), the damage a base or relay has taken, or none (obstacles take none). These
  are fields of a `.usg` `DCBS` object (`docs/formats/usg.md`, "Object fields"), all
  in the state hash. It also holds the `OrderBook`.
  `World::census(side)` counts one side's objects by kind. The world also holds the
  `terrain` and both sides' `paths` (below), also in the hash.
- **`terrain`** (T-059): `Terrain` is the `MAPP` records in file order as `Feature`s
  (kind, 16.16 centre and `z`, 8.8 radius, diagonal and height). `approx_distance` is
  the original's `max + min/2` distance, saturating to 0x7fff.7fff on overflow;
  `Feature::contains` drops the distance's low byte and compares it with the radius;
  `Terrain::hill_at` is the robots' test (hills only, the first that contains the
  point), `ground_height` the height it gives (the hill's `z + height`, else 0) and
  `below_ground` the shots' test (any kind). `Paths` holds each side's paths in use
  in slot order: mission, pick weight and points (`docs/formats/usg.md`, "How paths
  are used").

## Loading a scenario

`World::from_scenario(&usg::Scenario)` (T-051, T-058) copies each `DCBS` object
field for field: its slot as the `EntityId`, its kind, its side (bit 3 of +0x16), its
x and y as `Fixed`, its heading and radius, and the damage its kind's layout records;
and `Terrain::from_scenario`, `Paths::from_scenario` take the `MAPP` records and the
`APTH`/`HPTH` paths in use (none for a missing chunk).
The orders come from the same objects (`OrderBook::from_scenario`), as loading leaves
them: the original gives every robot its program (+0x2e) anew, so each robot runs the
program it was given, whatever the file saved as running; the manual bit is kept
(`usg.md`, "Loading"). Rules for files the stock install does not have:

- An object shorter than 0x18 bytes is refused by the parser. A robot without the
  stock 0x172-byte layout has no orders and no recorded damage.
- Two objects in one slot: the later one is kept.
- No `DCBS` chunk: an empty world.

The other named bytes stay in the `usg::Scenario`, which `xeno-app` keeps for the
whole run (`mission::Assets::scenarios`), until a rule needs them.

**The alert.** Playing a campaign scenario that is not the loaded one adds the alert
level's damage to the player's base (`sim_apply_scenario_alert`): `World::apply_alert`
adds it to the human base's damage, and `xeno-app` passes the value for the record's
state before play, read from the user's `xeno.exe`
(`xeno_formats::campaign::Campaign::alert_damage`). Training missions and a resumed
mission take none.

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
- It gives it back with `Sim::park`, keeping its world, or `Sim::end`, dropping it.

In `xeno-app` the campaign holds the word between missions
(`Campaign::take_random`, `give_random`). The mission's sim takes it when the mission
starts or resumes, and every action but play parks the mission, so the word is back
before the campaign draws for a result. The app parks at every end, won, lost or with
no result, because the original keeps the objects in memory until another scenario
loads: playing the loaded scenario, or Ctrl+Space, runs them again. A second log-in
hands the word on to the new player's campaign instead of seeding a new one.

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

- **The world has no size and no edges** (T-059, `usg.md`, "The world"): the original
  limits no position, so the sim does not either. Movement (T-052) should add
  velocities with wrapping 32-bit adds, as the original does; stock scenarios stay
  near (128, 128) and never come near the ends of the 16.16 range.
- What the original keeps per object beyond kind, side and position is mostly
  unknown (T-058); entities grow as those fields are named.
