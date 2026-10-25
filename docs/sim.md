# The simulation (T-050, T-051, T-052, T-058, T-059)

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

## Movement (T-052)

A T-090 pass on `xeno.exe` read the mission loop, the object tick, each robot kind's
tick and the routines they share. Addresses are in Ghidra's `xeno.exe`
(`docs/screens.md`, "Symbols"); the tables are read from the user's `xeno.exe` by
`xeno_formats::motion`.

**The tick.** The mission loop (`sim_run_mission_loop`) counts the retraces since its
last pass and, for each one (catching up at most 9 behind), advances the game clock
and runs `sim_tick_all_objects` (`1000:f631`): it adds one to a global tick count
(`DS:0x62c6`, which the scripts read), then calls every object's tick routine in slot
order (0 to 239), by kind from the table at `DS:0xe6e8`. So the sim ticks once per
retrace, objects in slot order, each seeing the objects before it already moved.

**A robot's tick** (`sim_tick_humanoid` `1000:abe0`, `sim_tick_scorpion` `1000:930a`,
`sim_tick_scout` `1000:c485`) runs, in order: a cool-down, the **movement** (below),
sensors and weapons, then steps +0x1a by 2 and calls a routine by its value, a routine
by the state at +0x1b, and the walking animation. The movement routine
(`sim_move_humanoid` `1000:ac29`, `sim_move_scorpion` `1000:9377`, `sim_move_scout`
`1000:c4f5`) is the same for the three kinds except where noted:

1. **Recall.** With +0x23 bit 3, the robot is given program 4, switched to its program
   (below, "The program bit"), the bit is cleared and a kind's routine runs (orders,
   T-053).
2. **Turn keys.** +0x23 bit 4 takes 0x40 off the target heading (+0x2a), bit 5 adds
   0x40, wrapping.
3. **Steering** (`sim_steer_*`). The difference target heading less heading, as a
   signed 16-bit number, is limited to the kind's turn step (`Motion::turn_step`, the
   same for the three kinds in the stock build) and added to the heading. The limit
   compares the difference's magnitude unsigned, so a difference of exactly half a turn
   (0x8000) turns by the step the negative way. A change recomputes the velocity.
4. **Speed** (`sim_step_*_speed`). The speed asked for (+0x28) is first limited
   (`sim_limit_speed_by_drive_and_helm`, `1000:d3b2`): 0 if the drive (+0x51) is 0,
   halved (arithmetic shift) if the drive is at or under `Motion::weak_drive`; then by
   the helm (+0x24): under 16, 0; 16 to 47, a quarter (two arithmetic shifts); 48 to
   63, half; 64 to 95, the speed less a quarter of it; 96 and over, unchanged (a jump
   table by `(helm >> 3) & 0xfe`; helms over 111 would read past it, and none can be).
   The scorpion's then goes through one of `Motion::scorpion_speeds`
   (`sim_map_scorpion_speed`, `1000:d3f8`): table `+0x118 / 2`, by the speed's
   magnitude, the sign kept. The speed (+0x26) then steps by one towards the result
   (a signed comparison) and a change recomputes the velocity. A humanoid steps every
   tick. A scorpion steps only when +0x1a's low four bits are 0: +0x1a steps by 2 and
   is kept to 0..=0x1e, so every 8 ticks. A scout steps every tick, but while it is
   asked to move and its stance (+0x10b) is not 0x30 (standing) it does not step and
   sets its target stance (+0x10c) to 0x30 instead; while its speed is 0, the stance
   steps by one towards the target stance (`sim_step_scout_stance`), and while it moves
   both are set to 0x30. (The stance also sets a height at +0x1c for the 3D view.)
5. The script runs (T-053).
6. **The move.** The velocity (+0x1e, +0x20), sign-extended, is added to x and y as
   32-bit numbers, wrapping. There are no world edges.
7. **Hills** (`sim_check_robot_against_hills`, `1000:d50f`). If the robot's centre is
   inside a hill (`Terrain::hill_at`), its speed is negated and its target speed set
   to 0, its kind's bounce routine runs (`sim_on_robot_hit_hill`, `DS:0x7209`: the
   humanoid's and scout's update their console's speed gear, the scorpion's does
   nothing), and it takes a hit of 0 points (`sim_damage_object` with AL 0, AH 1).
   The velocity is **not** recomputed, so the robot keeps going the same way until its
   speed next steps (the next tick, or up to 8 ticks for a scorpion; each tick inside
   negates the speed again). Otherwise its height is set to the ground's, which is 0
   outside a hill.

**Velocity** (`sim_update_robot_velocity`, `1000:d41c`, through
`sim_scale_by_heading`, `1000:03ad`). It is worked out only when the heading or the
speed changes: `vx = speed * sin(heading)`, `vy = speed * cos(heading)` (heading 0 is
+y, a quarter turn +x), each the high word of a 16 by 16 unsigned multiply, signed by
the sine's sign and the speed's. The sine (`sim_sine_magnitude`, `1000:05e6`) takes an
angle (65,536 to a turn): its top bit is the sign; the rest, folded into the first
quarter (`0x8000 - a` above 0x4000), indexes `Motion::sine` by `a >> 6` and
interpolates to the next entry by `(a & 0x3f) << 2` in 256ths, rounding: the low byte
of the difference times the fraction, plus 0x80, shifted right 8, plus the fraction
itself if the difference's high byte is 1; then plus the entry. A quarter turn exactly,
or a sum that carries, is 1.0: then the component is the speed itself, unmultiplied.
The table's last entry (index 256) is 0, not 1.0, so between 255 and 256 the
difference wraps to 1 and the carry gives 1.0. The cosine is the sine of
`0x4000 - angle`. A speed of 63 moves a robot 63/65,536 of a unit a tick at most.

**The program bit.** +0x23 bit 7, which T-043 named "manual", is set when the robot's
program drives: the script interpreter (`1000:4899`) steps the script every tick only
with it set (otherwise once every 64 ticks of `DS:0x62c6`); recalling a robot and
giving it a path's mission set it (`order_switch_to_automatic`, `1000:d42c`, posting
event 0xa); and the cockpit's drive keys clear it first (`order_switch_to_manual`,
`1000:d446`, event 0xb). Every stock robot is saved with it set. Both Ghidra names
were swapped in this pass; T-111 renames the port's `manual`.

**Paths.** Only the alien base walks robots along paths. A base's tick (`sim_tick_base`,
`1000:6300`) adds one to +0x41 and calls a routine by its low four bits (table
`DS:0x5aea`); at 2, the alien base (`sim_run_alien_base_path_phase`, `1000:ea0e`)
gives every robot in its unit list without a path a random one
(`sim_assign_paths_to_idle_units`) and then moves on every robot in the list that asked
for its next point (`sim_advance_units_asking_next_point`, `1000:ea69`); at 1 it counts
the robots on each path (`sim_count_units_on_paths`, only the editor shows it). So
both happen once every 16 ticks, robots in unit-list order (the file's order, not the
slots'). Robots with +0x23 bit 0 are skipped by both. The details, in the order the
code runs (`docs/formats/usg.md`, "How paths are used", has the overview):

- *Picking* (`sim_assign_random_path`): if the alien table has paths in use, a list
  holds each in-use slot `weight + 1` times (a weight of 255 gives 256); the random
  word's low byte, reduced by the list's length until it is smaller, picks one. The
  robot's +0x2c becomes the slot alone (every flag cleared), +0x2d 0, it heads for
  point 0, and the mission's program starts (`order_start_path_mission_program`,
  `1000:ec4d`: `Motion::mission_programs` by mission and kind, then the program bit
  set). The alien table is used whatever the robot's side.
- *Heading for a point* (`sim_head_for_path_point`): the robot's side picks the table;
  slot +0x2c & 0xf is used whether or not it is in use (slots past the count hold what
  the file left there). A path of no points fails. Point 0xff is the robot's side's
  base (the last base loaded for it). Point 0xfe runs the kind's return routine
  (`sim_run_path_return_routine`, `1000:ec22`) and fails; a point at or past the count
  runs the end routine (`sim_run_path_end_routine`) and fails. Otherwise the point
  becomes the destination: `sim_set_robot_destination` copies x and y to +0x3c and
  sets +0x32 to 0x22 (dropping an object it was heading for, bit 0); then +0x2c bit 6
  is cleared and bit 7 set. The end and return routines are picked by kind and the
  mission of the path in the **alien** table's slot, whatever the robot's side; in the
  stock build every end routine turns the robot round (`sim_turn_round_on_path`,
  `1000:ecb4`: +0x2c bit 5 flipped, bit 6 set), and the return routine takes a new
  random path for humanoids and scouts (`sim_take_new_random_path`, `1000:ecbd`) and
  turns scorpions round.
- *Next point* (`sim_advance_path_point`): forwards (bit 5 clear) the point steps up
  one and the robot heads for it; if that fails the point steps back and it tries
  again from the top (now perhaps backwards, as the routine may have turned it). Backwards
  the same the other way. So at the last point a robot turns and heads for the one
  before it; from point 0 backwards it heads for its base (0xff); from there, 0xfe
  turns a scorpion round to point 0; a humanoid or scout gets a new path at point 0,
  and the step back and forth that follows leaves it heading for **point 2** of the new
  path (or turned round, if it has fewer than three points). A path with no points
  makes this loop forever in the original; the port stops it (none in stock files).
- *Joining a path* (`sim_join_nearest_path_point`, `1000:eb66`, a script command,
  T-053): with no paths in its side's table the robot heads for its base; otherwise
  the nearest of its base and every point of every path in use, by
  `approx_distance` truncated to 8.8 and counted only under 256 units (strictly
  nearer wins, base first, then slots and points in order), becomes the destination,
  and +0x2c and +0x2d become that slot and point with no flags (the base gives slot 0,
  point 0).

The scripts set +0x2c bit 6 (ask for the next point), bit 4, or step a point back and
head for it (the commands at `DS:0x7d14`), and steer towards the destination with the
bearing routine (`sim_get_bearing`, `1000:0735`, an arctangent through a table at
`DS:0x20ba`); both are T-053's.

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
