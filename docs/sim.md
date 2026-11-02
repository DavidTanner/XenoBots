# The simulation (T-050, T-051, T-052, T-053, T-054, T-055, T-058, T-059, T-102, T-105, T-112, T-113, T-114, T-115, T-116, T-119, T-136, T-137, T-138, T-140, T-146, T-147, T-148, T-177)

`crates/xeno-sim` holds the game's rules. It has no window, audio or clock, so it is
tested headless in CI. It depends only on `xeno-formats`.

## API

- **`Sim`** is the whole state: the retraces run, the random word, the held keys and
  mouse, and the `World`. `Sim::new(seed)` starts an empty world;
  `Sim::with_random(random)` starts one that carries on a random word already in use.
  `Sim::load(scenario, random, tables)` starts a mission: the world from a `.usg`
  scenario (below, "Loading a scenario"), the run's random word and the movement
  tables read from the user's `xeno.exe` (`xeno_formats::motion`). Without tables nothing moves.
- **`Sim::park()`** stops a mission for now and returns a `Parked` and the random word;
  `Parked::resume(random)` gives the word back and returns the `Sim`. A parked mission
  has no word, so it cannot tick or draw. **`Sim::end()`** ends a mission and returns
  the word. See "The run's random word".
- **`World::save(&loaded, program_table)`** writes the running world into a copy of the
  loaded scenario, as a save does (see "Saving the world (T-116)").
- **`Sim::tick(events)`** runs one retrace with the input that arrived for it
  (`docs/game-loop.md`: the sim steps once per retrace, at the mode's rate).
  `Sim::run(log, n)` ticks until `n` retraces have run, each with its events from an
  `InputLog`. A tick counts the retrace, tracks held keys and buttons (nothing acts on
  them: the host turns the keys into orders and `Sim::pilot` calls) and runs every object's tick
  (`motion::tick`, below, "Movement"); it returns the `motion::Event`s it made,
  `MissionOver` among them once a destroyed base's end is over ("The mission's
  end").
- **`Sim::pilot(id, key)`** is a cockpit key's action on a robot (`Pilot`: turn left
  or right, a gear up or down, stop, a scout's stance; "The cockpit keys" below), as
  `Sim::change_orders` is the program panel's: the host calls it between ticks for
  the robot whose console is open, and it returns the events it made.
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
  `select_next`, `select_previous` and `set_program_drives`, each returning the change),
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
   negates the speed again). Otherwise its height is set to the ground's, which is
   always 0 there (below, "Heights").

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
`0x4000 - angle`. A speed of 63 moves a robot 63/65,536 of a unit a tick at most;
that is the original's scale (T-053, "Orders", "The speed scale").

**The program bit.** +0x23 bit 7 (T-043 named it "manual", the wrong way round) is
set when the robot's program drives: the script interpreter (`order_step_script`,
`1000:4899`) steps the script only with it set (otherwise it only runs event handlers,
and once every 64 ticks of `DS:0x62c6` drops an object goal that is gone; T-053,
"Orders"); recalling a robot, giving it a path's mission and the console's F3 set it
(`order_switch_to_automatic`, `1000:d42c`, posting event 0xa); the cockpit's drive
keys clear it first (`order_switch_to_manual`, `1000:d446`, event 0xb), as do F2 and
choosing a program on the console (T-111; `docs/screens.md`, "Orders"). Every stock
robot is saved with it set. Both Ghidra names were swapped in T-052; the port calls it
`Orders::program_drives` and `usg::Robot::PROGRAM` (T-111).

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
bearing routine; see "Orders" below.

### The port (T-052)

`xeno_sim::motion` does the above. `World` keeps each robot's movement fields in
`Entity::state` (`State::Robot(Movement)`: +0x1a, the velocity, +0x23 without bit 7,
the speeds, the target heading, +0x2c, +0x2d, +0x32, the destination, +0x51, +0x10b,
+0x10c and +0x118) and each base's +0x41 (`State::Base`), all hashed; `World::units`
holds each side's unit list in file order and `World::bases` each side's command
object, both filled at load. `motion::tick` runs the objects in slot order: robots
move (`steer_and_speed`, the script, then `travel`), and the alien base's path phase
runs at its count's phase 2. Its events are `HillHit` (the 0-point hit, T-054's `Destroyed`),
`PathTaken` and `Program` (orders changed: the mission's program given and the
program bit set, `Orders::program_drives`, or a script's state). After the
move each robot walks (`motion::walk`, `docs/render.md`, "Walking"): its step counter
and footstep legs step, and its footsteps are `Event::Sound`s (13 to 15) for it.

The scripts (T-053, below) call `head_for_path_point` and
`join_nearest_path_point`. Left out until their tasks: the path census (`sim_count_units_on_paths`) and the base's other phases.
Where the original reads past a table (a scorpion speed table or entry past the
last, a path slot over 7, a mission over 6) the port takes the last entry or does
nothing; `advance_path_point` gives up after 16 tries where the original would loop
forever. None of these happen in the stock files.

`xeno-app` ticks the mission's sim once a retrace on every mission view, with that
retrace's input, and logs the events (debug level). `xeno-tool simulate <file.usg>
[--ticks N] [--every K] [--drive SLOT:SPEED:HEADING]` runs one with no window and
prints the robots' positions; `--drive` gives a robot a speed and heading to start
with (its script may change them).

## Orders (T-053)

A T-090 pass on `xeno.exe` read the script interpreter, every opcode's handler, the
event dispatch, the bearing routine and the speed routines the scripts call. The
scripts and the interpreter's tables are read from the user's `xeno.exe` by
`xeno_formats::script` (`Scripts`) and `xeno_formats::motion` (`Motion::atan`,
`Motion::script_speeds`).

**Where they are.** The 31 programs (`docs/screens.md`, "Orders") are bytecode in the
data segment, `DS:0x5034..0x5670`, which also holds two tables of event handlers. The
opcode table at `DS:0x4dd4` has 136 entries of two words, a handler and a **time**; it
ends where the program table (`DS:0x4ff4`) starts. A script is a run of opcodes, each
followed by its operand: none, a byte, a word, a field offset and a byte, a field
offset and a word, a header (0x0f) or a name (0x02). `xeno_formats::script::operand_kind`
gives each opcode's.

**A program's start.** `order_set_program` (`1000:476f`) writes the program to both
+0x2e and +0x30, points +0x34 at its script, clears the flags (+0x33), empties the
stack (+0x31 = 10) and sets the wait (+0x38) from a global stagger counter
(`DS:0x4db5`): the counter drops by 10 each start, and when that borrows or reaches 0
it then gains 49 more (59 added and 10 taken again); it starts at 0, so the first robots started wait
39, 29, 19, 9, 48, 38, ... ticks. Program numbers over 30 start program 30. The
script's first opcode is the header (0x0f), which skips its items up to the byte after
0x11; then 0x02 skips the flags byte and the name. So a program's first real opcode
runs two steps after its wait.

**The step** (`order_step_script`, `1000:4899`), called from each robot's move routine
after the speed step and before the move (step 5 of "A robot's tick"):

1. Nothing while the global pause byte `DS:0x4db4` is set (a developer key).
2. If +0x33 bit 5 is set the script steps. Otherwise, if events wait (+0x36), the
   highest one is dispatched (below) and the tick ends. Otherwise the script steps only
   with the program bit (+0x23 bit 7); without it, only every 64th tick of `DS:0x62c6`
   runs the object-goal check (step 4).
3. Stepping: the wait (+0x38) drops by one; only when it was already 0 does an opcode
   run. Before it, a stack pointer over 10 starts program 30 (`CORE DUMP`) and a script
   pointer outside the script area starts program 29 (`BUS FAULT`), each setting +0x33
   bit 5. The opcode is read, the wait set to its time, and its handler runs. So an
   opcode of time `t` is followed by the next one `t + 1` ticks later; most have time 0
   and the scripts run an opcode a tick.
4. Then, if +0x32 bit 0 (an object goal) is set and the object is gone, the goal is
   dropped.

**Events.** `order_post_event` (`1000:4818`) sets bit `n` of +0x36 for event `n` (through
a table of bits at `DS:0x1c56`, the identity). Posted so far: 5 by the hold order
(opcode 0x6f), 7 by every hit (`sim_damage_humanoid` and the other kinds' damage
routines; the hill bounce's 0-point hit too), 0xa and 0xb when the program or the pilot
takes over. Dispatch pushes the script pointer (as its offset from the program table,
with +0x33 bits 0, 5, 6, 7 in the word's top nibble), sets +0x33 bit 5 and the wait to
0, and clears the event's bit. The handler is the running program's own, from its
header's items (event, address), when the program drives; otherwise the program
table's default for the event (`DS:0x5623`); while the pilot drives, the pilot table
(`DS:0x55e4`). Both tables have 13 entries; most handlers only show a message and
return. Return (opcode 0x06) restores the pointer and the flags, clearing bit 5 again.

**The opcodes**, by number; times are the stock ones (in ticks):

| Opcodes | What they do |
|---|---|
| 0x00, 0x01, 0x10, 0x11, 0x85 | Nothing (0x01 is a debugger break) |
| 0x02 | Skip a flags byte, a length and that many bytes (the program's name) |
| 0x03 | Restart the running program (`order_set_program` with +0x30, so it becomes the program given too), then wait 30 to 61 ticks at random |
| 0x04 | Jump by a word, relative to the next opcode (time 1) |
| 0x05, 0x06 | Call an address, pushing the return and the flags (wait 1); return, restoring both (time 1) |
| 0x07 | Drop a return address (wait 1) |
| 0x08, 0x09 | Call and return without the flags |
| 0x0a | Halt: run itself again (time 255), until an event or a new program |
| 0x0b, 0x0c | Set or clear the carry flag |
| 0x0d, 0x0e | Set or clear +0x33 bit 5 (step every tick, hold events off) |
| 0x0f | The header: skip its items up to 0x11 |
| 0x12, 0x14, 0x16, 0x18 | Wait 1, 60, 3600, 255 ticks (by their times) |
| 0x13, 0x15, 0x17 | Wait a byte's ticks, seconds (x 60) or minutes (x 3600, at most 0xffff) |
| 0x19, 0x1a | Wait 240 to 495 or 30 to 157 ticks at random |
| 0x1b, 0x1c, 0x1d | Flags from the random word: the carry is its top bit, or set one time in 4 or 8 |
| 0x1e, 0x1f | Switch to automatic or manual (`order_switch_to_*`; time 1 for manual) |
| 0x20, 0x21 | Jump by a word if the program drives, or if the pilot does |
| 0x22, 0x23 | Enter a state: push +0x30 and set it to a byte; leave it: pop +0x30 (the console's program panel is redrawn) |
| 0x24, 0x25, 0x28, 0x29 | Push a byte or word field of the robot; pop one into a field |
| 0x26, 0x27 | Drop a byte or a word |
| 0x2a to 0x41 | Add, subtract, compare, test, or, and: on a field (offset byte) or the top of the stack, byte or word, with a byte or word value, setting the flags |
| 0x42 | Jump by a signed byte |
| 0x43 to 0x4c | Jump by a signed byte if carry, no carry, below or equal, above, zero, not zero, sign, no sign, carry, no carry |
| 0x4d, 0x4e | Set a byte or word field |
| 0x4f, 0x50 | Turn: target heading = heading + a word; stop turning: target heading = heading |
| 0x51, 0x52 | Speed up or slow down a gear: a humanoid's (+0x10b) or scout's (+0x109) gear by one through its gear table (`DS:0x68ec`, `0x6df8`), a scorpion's speed by 21 within -42..63; time 30 |
| 0x53 | Stop: speed asked for 0, then the kind's bounce routine (the console's gear); time 30 |
| 0x54 to 0x58 | Set a speed by kind: top, high, low, slow reverse, fast reverse (95, 56, 28, -23, -43 for a humanoid; 63, 42, 21, -21, -42 for a scorpion; 113, 85, 39, -35, -71 for a scout); time 30. `Motion::script_speeds` |
| 0x59 to 0x5c | Jump by a word if the speed has reached the speed asked for, if the heading has reached the heading asked for, or if not (time 1) |
| 0x5d | Compare the distance to the goal (8.8 units, 0xffff past 256) with a word |
| 0x5e | Push that distance |
| 0x5f | Steer to the goal: target heading = the bearing to it (no flags) |
| 0x60, 0x61, 0x62 | Steer to the goal plus or minus a word; or zigzag: the target heading turned a random 90 to 180 degrees more towards the side the goal is on (-45 to +45 without a goal) |
| 0x63 | Pick a target and steer to it (sensing) |
| 0x64 | Turn by a random -90 to +90 degrees |
| 0x65 | Stop turning and stop (time 60) |
| 0x66 to 0x68, 0x6d, 0x6e | A point the script works out (`DS:0x4da4`): an object's position plus +0x3c as an offset; compare the distance to it; steer to it; push the distance; steer to it as goal 0x10 |
| 0x69, 0x6a | Distance to, and steer to, the robot's own base |
| 0x6b, 0x6c | Distance to, and steer to, the target (+0xe6) |
| 0x6f | Hold here: drop an object goal, destination = position, goal 2, message, post event 5 |
| 0x70 | Join the nearest path point (`sim_join_nearest_path_point`; time 60) |
| 0x71 | Look for enemies (sensing, `1000:d757`; time 30) |
| 0x72 | Carry if the targeting computer is damaged (time 15) |
| 0x73 | Carry unless the target is a kind 3 object (a base) (time 30) |
| 0x74, 0x75, 0x76 | Messages (a word: the text's string-segment offset): to the robot's message panel; to its side's base log; to the log and every robot of the side (`docs/screens.md`, "The message panel (T-104)") |
| 0x77 | A path command, a byte (`order_run_path_command`, table `DS:0x7d14`): 4 ask for the next point (+0x2c bit 6), 8 set +0x2c bit 4, 10 step a point back and head for it; 0, 2 and 6 do nothing |
| 0x78 to 0x7d | Weapons: their state as flags or pushed; fire (time 30) |
| 0x7e to 0x83 | The relay net ("Relay nets" below): start a line of grid cells from the robot to its destination (+0x10a..+0x114); step to its next cell (carry at the end); the cell as the worked-out point; carry unless a relay stands in the cell; carry with no relay carried; deploy one (carry when none was) |
| 0x84 | Test +0x32 with a byte |
| 0x86, 0x87 | Debug counters |

The flags are the x86 flags of the opcode's operation, kept in +0x33 (`order_save_script_flags`).

**What the stock programs do.** The human robots are given ENGAGE (9): with the
targeting computer or the weapons damaged it retreats (RETREATING, 25); otherwise it runs ENGAGING (24), which looks for
an enemy, closes on it and fires, then stops and waits at random. The alien robots
are given PATROL (8): while they head for a point (+0x32 bit 1), they set a speed,
steer to the point and wait 5 seconds, until within 0x32 (0.2 units); then they stop,
ask for the next point (path command 4) and recheck every 3600 ticks, so the alien
base moves them on within 16 ticks and they set off again a minute later. Without a
point PATROL shows a message and halts. HOLD POS (1) holds the point it was given or,
without one, holds where it is. RET BASE (4) joins the nearest path point and walks
the path backwards, a point at a time, to its base. Alien scorpions are given
DENSE NET (13): they walk a line of grid cells to their point, apparently laying
relays where none stand. Events turn
patrols into fights: a hit (7) or event 1 runs ENGAGING. Event 9 (posted by nothing
traced yet) runs AVOIDANCE: stop and wait, or back off turning 45 degrees, then drive
on slowly.

**The bearing** (`sim_get_bearing`, `1000:0735`): from the robot's position to the
goal, as 32-bit differences. If x is negative, both are negated (half a turn); if y is
then negative, it is negated and the two swapped (a quarter turn); the octant count is
doubled. Then, if x >= y, the octant count goes up by one and the ratio is
`(x - y) / (x + y)`, else `x / y` (y = 0 with x > 0 adds another octant, giving a
quarter turn; both 0 give 0). Numerator and denominator are shifted left together
until the denominator's top word has its top bit set, and the top word divides the
numerator (one less, and 0xffff in the low word, when the numerator's top word is not
smaller). The scripts call it through `sim_get_bearing_coarse` (`1000:04cc`), which
takes `Motion::atan[ratio >> 8]` with no interpolation (otherwise the next entry is
interpolated as the sine is); the angle is the octants and the table entry shifted
right 3 together, rounded: `(octants << 13) | (atan >> 3)`, plus the last bit shifted
out. Heading 0 is +y and a quarter turn +x, as for the velocity.

### The fine bearing and the true distance (T-146)

**The coarse flag.** `DS:0x1cb2` is a byte that starts at 0 and is only ever
incremented before and decremented after a call, so it is a nesting count: the
coarse wrappers (`1000:04cc` bearing, `04dd` bearing between two 16-bit points,
`04f8` climb, `0530` bearing and rough distance, `0550` the same with the climb,
`0727`, `074d`, `0768`, and the facing for the models, `05a4`; and the sine's,
`05d3`) raise it around their call, and everything else reaches the angle routine
(`1000:0782`) with it at 0: the **fine** path. The sine (`1000:05e6`) and the cosine
(`067e`) read it the same way.

**The fine path** (`1000:07fa`): with the ratio `r` (16 bits) from the division
above, `i = r >> 8` and `f = r & 0xff`; `lo = atan[i]`, and the step is
`atan[i + 1] - lo` in 16 bits. Only the step's low byte and bit 8 are used:
`add = ((step & 0xff) * f + 0x80) >> 8`, plus `f` when the step's high byte is
exactly 1 (the stock table's steps run from 0xa3 to 0x146, so this is exact for it).
The entry `lo + add` is then shifted with the octants as the coarse one is. For
`i = 255` the next entry is the word after the 256-entry table at `DS:0x20ba`, which
is 0 in the stock build: the step wraps to 0x10000 less the last entry, the right
one. `xeno_formats::motion` reads that word as `Motion::atan[256]`.
Against the exact angle, the fine bearing is off by under one 65,536th of a turn
all round with the user's table; the coarse one by up to about 40.

**The true distance** (`1000:0942`) is **level**: the squares of x and y only (the
T-112 survey's z was wrong; the routine moves its pointers on to y and back, never
to z). Each axis goes through `1000:0b8c`: the 32-bit difference (wrapping, from SI
to DI) as its magnitude `d`, squared from its top 16 significant bits:

| `d` | square |
|---|---|
| under 0x1_0000 | `d * d` (32 bits) |
| under 0x100_0000 | `(d >> 8)^2 << 16` |
| from 0x100_0000 | `(d >> 16)^2 << 32` |

The two are added into a five-word scratch at `DS:0x1ca6` (cleared first; the second
add carries into the fifth word). Each square is at most 2^62, so the sum `s` fits 64
bits and the fifth word stays 0; the branch that would handle it (`1000:09c7`, which
assumes it is exactly 1) is never taken. Then, by `s`'s size, a 32-bit root at a
scale:

| `s` | distance (16.16) |
|---|---|
| under 2^32 | `root(s)` |
| under 2^40 | `root(s >> 8) << 4` |
| under 2^48 | `root(s >> 16) << 8` |
| from 2^48 | `root(s >> 32) << 16` |

`root` (`1000:0bca`) is the digit-by-digit square root, two bits a step (8 steps for
a value under 2^16, 16 otherwise), exact: the **floor** of the root, 16 bits, in AX
with DX 0. So the distance is never more than the exact one and loses under 1/256 of
it (the truncated squares). `1000:0ab4`, a three-axis copy of the same sum, is called
by nothing traced.

**Callers.** In the disassembled code, the fine path is used by:

- the lock's upkeep (`1000:d6e6`, through `1000:0545`: the fine bearing to the target
  into +0xea and the true distance's middle word into +0xe8) and the lock's range check
  (`1000:d730`, also through `0545`, the middle word against `DS:0x7079[kind]`);
- the lead (`1000:4549`, the fine angle at `1000:45ed`);
- the missile's launch climb (`1000:e98d` through `1000:0519`: the fine angle of the
  target's height over the true distance);
- the three consoles' compasses (`1000:04eb`, the fine bearing between two 16-bit
  points: `889c`, `a6dd`, `b97c`), not yet ported (T-165, T-169, T-172).

`1000:0545`'s sibling `057c` (with the climb), the bare `04da`, `03a8` and `042a` are
called by nothing found. Everything else (the scripts' steering, the scan, the
missile in flight, the contact check, the models' facing) is coarse. The listing was
searched by a linear disassembly of segment `1000`; a call from code reached only
through a table could still be missed.

**The port.** `xeno_sim::script::fine_bearing` (and `angle` with `fine`) and
`xeno_sim::sense::true_distance` (`u32::isqrt` is the same floor root). The lock's
upkeep and its range check take both; the lead and the missile's launch climb take
the fine angle and the climb the true distance. The stock closing test
(`real_assets_two_stock_robots_find_and_close_on_each_other`) now uses seed 3: the
finer aim changes when the fight's hits land, and seed 7's human turns back (most
seeds' do, with either aim). `xeno-app`'s
`real_assets_a_training_mission_is_won_and_lost_by_play` now takes over the `ntrain`
scorpion at 30,000 retraces and locks only the base: the escort's finer aim cost the
old play's scorpion its targeting computer. Not checked in DOSBox (T-199).

**The speed scale.** Speeds asked for are small: the scripts' and the cockpit's speeds
are at most 95 (humanoid), 63 (scorpion) and 113 (scout), and the velocity is
`speed * sin(heading)` in 1/65,536ths of a unit a tick (T-052's port is right). So a
humanoid at top speed covers 95 x 70 / 65,536, about 0.1 units a second: a 5-unit
leg between path points takes about 50 seconds and the 40 units between the stock
bases nearly 7 minutes. The scripts agree: they call a point reached at 0.2 units and
recheck every 5 seconds (0.4 units at most). Objects' radii (+0x14, 10 for a robot)
are not in position units (robots stand 3 units apart); hills' are.

**Destinations on no path.** Stock alien robots are saved heading for points on no
path in the file (scen00a's slot 3 for (142.7, 146.9)). PATROL just drives there: it
reads +0x32 and +0x3c, not the path, then asks for the next point, and the alien
base moves it to point 1 of its path. They look like points of an earlier edit of the
paths.

### The port (T-053)

`xeno_sim::script` runs the scripts. `Movement::script` (`Script`) keeps +0x34, +0x38,
+0x31 and the stack, +0x33 and +0x36, and `World::stagger` the stagger, all hashed;
the program running is the `OrderBook`'s. `Sim::load` starts every robot's program in
the file's order (`script::start_all`), so their waits are staggered, keeping the
events the file saved; `script::step` runs between the speed step and the move, as
the original does; a path's mission starts its program through `script::set_program`
and `script::switch` (`order_set_program`, `order_switch_to_automatic`, which posts
event 0xa); `Sim::change_orders` does the same for the player's choices (the program
panel, F2, F3). `script::bearing` is the coarse bearing. Ported: the flow, call and
return, halt, flags, waits, random, switching, states, the stack and field opcodes
(the fields the stock scripts touch: the kind, +0x23, the speeds, target heading,
path, point and goal), the jumps, turning, the set-speed and gear opcodes (T-105), the goal's distance
and bearing (the destination and the robot's own base), zigzag, random turns, stop,
hold here, joining the nearest path point, the targeting check and the path commands;
events dispatch to the header's or the tables' handlers.

Shortcuts, each named in the code: an opcode not ported (the relay net until T-113)
**halted the script on itself**, so a robot kept its speed and heading; opcode 0x74
puts its message in the robot's panel (T-104), 0x75 writes the side's base log and 0x76
the log and copies to the side's robots (T-129; `docs/screens.md`, "The message log
(T-129)"); firing (0x7d) does nothing until T-114; field
opcodes read 0 at other offsets; return addresses are pushed as offsets from the
script area, not the program table; the stagger starts at 0 each mission.
So in the stock missions the alien robots on PATROL drive point to point along their
paths until a scan finds enemies, the human robots on ENGAGE look for enemies and
close on them (T-112), and an alien scorpion on DENSE NET lays relays along its line
(T-113).

## Sensing (T-112)

A T-090 pass on `xeno.exe` read the sensing routines, the opcodes that use them, the
lock and its upkeep, and the object goal. The ranges and where each kind's tick table
runs the routines are read from the user's `xeno.exe` by `xeno_formats::sensing`
(`Motion::sensing`).

**The tick tables.** After the move (step 6 of "A robot's tick") a robot's tick steps
+0x1a by 2 and calls entry `(+0x1a & 0x1e) / 2` of its kind's table of 16 routines
(humanoid `DS:0x6acf`, scorpion `0x6732`, scout `0x6ebf`, through the `call` at
`1000:abfb`, `9322`, `c4a0`). Every kind has the scan at entry 15, the lock's upkeep at
entry 5 (scorpion 6) and the lock warning's decay right after it; the contact check
(T-141), the weapons (T-114) and the console's routines fill the rest. So each runs
once every 16 ticks.

**Ranges** (8.8 units, by kind; humanoid, scorpion, scout in the stock build): the
*scanning* range (`DS:0x707f`: 8.6, 12.5, 9.8) and the *locking* range (`DS:0x7079`:
9.8, 13.7, 10.6). Distances are the original's `max + min/2` (`1000:08ec`, "Movement")
unless named otherwise, counted only under 256 units, and in range at or under the
range.

**The scan** (`1000:dcba`). Only while +0x1a's top three bits are clear (so every
time for a scorpion or scout, whose counter stays under 0x20, and one pass in eight
for a humanoid), it steps the 4-bit count in +0x17 (`SCAN_COUNT`: bit 7 lowest, then
5, 4, 3). When the count comes round to 0, every **unit** (+0x16 bit 5, the robots) of
the other side within the scanning range gets `SPOTTED` (+0x17 bit 2), and their
number goes to `contacts` (+0xf7). There is no check for a cloak or the field. If the
number changed: from none, **event 1** is posted; to none, **event 2**. So a
scorpion or scout scans every 256 ticks and a humanoid every 2048.

**Looking for enemies** (opcode 0x71, `1000:d757`; time 30). The nearest object with
+0x16 bit 2 (`SCANNED`: robots and bases) of the other side, not the robot itself, not
`CLOAKED` (+0x17 bit 0), within the **locking** range (strictly nearer wins, slot
order) is locked (below), and the carry is cleared; with none, the "no enemies"
message goes to the robot's panel and the carry is set. A refused lock still clears
the carry.

**Steering for the nearest enemy** (opcode 0x63, `1000:d165` calling `2000:8005`,
then `1000:49fd`). The same search with the **scanning** range, without the self and
cloak checks (`2000:8005` returns the nearest or none). With one, the target heading
(+0x2a) becomes the coarse bearing to it and the carry is cleared; with none, the
carry is set. It does not lock the object.

**The lock** (`sim_lock_target`, `1000:d8af`; the A key and T-133's pointer reach it
too), with the object to lock:

1. Targeting computer damaged (+0xec bit 0): a message, nothing else.
2. +0x17 bit 6 (`TRACKING`) is set.
3. The robot itself, or its target already: a message, nothing else.
4. Not `TARGETABLE` (+0x16 bit 1), or `CLOAKED`: nothing.
5. The true distance (below) at or past the locking range: an "out of range" message.
6. Of its own side: a message, and the lock goes on.
7. The upkeep's bearing, distance and bits are worked out (with the last arc and range
   any robot's upkeep used, `DS:0x7201`, `0x7203`), the object gets `SPOTTED`, it
   becomes the target (+0xe6), a kind routine marks the console's target panels for
   redrawing (`DS:0x7239`), a message ("target locked", or another when it replaces
   one) and sound event 26 for the robot whose console is open.

**The lock's upkeep** (`1000:d66f`, from each kind's tick table entry with that kind's
arc and range; humanoid 30 degrees and 9.8 units, scorpion 40 and 13.7, scout 35 and
10.6, either side of the heading). With no target, +0xec bit 2 is set. A target that
is `GONE` (+0x16 bit 0) is dropped with a "destroyed" message. With the targeting
computer damaged, or the last distance (+0xe8) past the locking range, or a unit
target whose ECM time (+0xdf) is running, the target is dropped with a "lost"
message and **event 4** is posted. Otherwise a unit target gets `locked_on` (+0xed)
0xff, and the bearing and distance are worked out (`1000:d6e6`): +0xec bits 1 and 2
are cleared; the bearing (`1000:0545`, the fine one) goes to +0xea, and bit 2 is set
when it less the heading is under the arc either way (unsigned compares with the arc
and its negation); the true distance (`1000:0942`: the square root of the sum of the
squares of x and y, "The fine bearing and the true distance") as 8.8 (its middle word, the top byte dropped) goes to +0xe8,
and bit 1 is set when it is under the range. When it drops the target the kind's
routine marks the console's panels (not the sim's concern).

**The lock warning** (`1000:dc95`): `locked_on` is halved each pass; while it is not 0,
`systems` bit 5 is set, and setting it posts **event 3**; at 0 the bit is cleared. So
a lock warns its target within 16 ticks and the warning fades 128 ticks after the
lock goes.

**The target opcodes.** 0x6b pushes the distance to the target (as 0x5e does, 0xffff
with none or past 256 units). 0x6c steers to it: +0x32 is set to 8 for the steering
and put back, so the target heading becomes the coarse bearing to the target, carry
clear; with none, carry set. 0x73 sets the carry unless the target is a base (kind 3).
The scripts test the target as the word field +0xe6 (opcode 0x37 with 0xffff): 0 is
none.

**Goals, bit by bit.** The distance (`order_push_goal_distance`, `1000:4f80`, opcodes
0x5d, 0x5e) and the steering (`order_steer_to_goal`, `1000:4ff1`, 0x5f to 0x61) take
the first of: bit 4, the worked-out point; bit 3, the target; bit 1, the destination
(+0x3c); bit 0, the object goal (+0x3a). Then they differ: with none of those, the
distance takes the destination if bit 2 is set and the robot's own base otherwise;
the steering takes the own base if bit 2 is set and nothing otherwise (carry set).
An object goal whose slot is empty, a target of 0, or a goal object of 0xffff count
as nothing (distance 0xffff).

**Object goals.** +0x3a is an object's **slot** (0xffff none), not a pointer
(`1000:f691` turns a slot into the object, `f6a6` and `f6bb` take and release a
reference on it). `1000:4685` sets one: the old one is dropped, the slot taken, the
destination becomes the robot's position **less** the object's (an offset), +0x32
becomes 1, a message and event 5. Only the consoles' map-mode press reaches it
(`1000:466b`, T-133). `sim_drop_object_goal` (`1000:473d`) clears +0x32 and +0x3a.
After every opcode, and every 64th tick without the program bit, the step drops an
object goal (+0x32 = 0) whose slot is 0xffff.

**The worked-out point** (`DS:0x4da4`, one for every robot): opcode 0x66 sets it to
the goal object's position plus the offset in +0x3c, or to 0xffff in every word
with no goal object; 0x67 compares the distance to it with a word (past 256 units,
the flags of the distance's whole units less 1), 0x68 steers to it, 0x6d pushes the
distance to it, 0x6e steers to it as goal bit 4 (carry with none, as 0x6c). It starts
at 0 in the image.

**Events.** Posted by sensing: 1 and 2 by the scan, 3 by the lock warning, 4 when the
upkeep loses a lock, 5 by an object goal (and the hold order). The others: 7 by
hits, 9 by the contact check (`1000:d4df`, "Contacts (T-141)"), 0xa and 0xb by the switches, 0xc
when crippled (the damage totals, T-140). Nothing posts 0, 6 or 8 (FIRE ALL's
handler for 8 is never run).

**The weapon state** (opcodes 0x78 to 0x7a, survey for T-114). A kind's routine for
each weapon (tables at `1000:d107`, `d12b`) gives a word: the weapon's number, and
bits for present (not damaged), cooling down, selected, short of drive, the
scorpion's cannon out of rounds; `1000:d0e9` and `d10d` add +0xec bits 2 and 1 (in arc,
in range). 0x78 clears the carry if either weapon is present with neither short of
drive nor out of rounds; 0x79 and 0x7a push the first's or the second's word.

**What the stock programs do with it.** ENGAGING (24) checks the weapons (0x78),
looks for an enemy if it has no target, steers for the nearest in scanning range if
it still has none, and gives up if there is none; with a target it steers to it,
drives at high speed while 2 units or more away, stops closer and backs off slowly
under 0.8 units, fires what is ready (T-114) and loops. PATROL's and the nets' event 1
(a scan found enemies) and event 7 (hit) run ENGAGING; ENGAGE's event 4 (lock lost)
restarts it. DETECT steers for the nearest enemy every 10 seconds. SHADOWING keeps
within 0.2 units of the worked-out point off its goal object. COMMANDO holds, looks
and steers for enemies and fires at bases.

### The port (T-112)

`xeno_sim::sense::step` runs after each robot's move, its panel's typing and before
its walk, at the entries of its kind's tick table that `Motion::sensing` names.
`Movement` keeps +0xf7 (`contacts`), loaded from the file and hashed; +0x16 and +0x17
are every object's since T-147 ("The flags and status bytes"). Part 1 (`t-112`): the
scan, with its events 1 and 2.

Part 2 (`t-112-target`): the lock (`sense::lock`), its upkeep and the warning at
their tick-table entries, looking for enemies (0x71, `sense::look`) and steering for
the nearest (0x63), the target opcodes 0x6b, 0x6c and 0x73, goal bit 3, and the field
+0xe6 for the scripts' test. `Movement` keeps the target (+0xe6, cleared at load),
its distance and bearing (+0xe8, +0xea) and `locked_on` (+0xed); `World::lock_window`
keeps `DS:0x7201` and `0x7203`; the targeting and warning bits are the robot's
`Components`. All hashed. Messages go to the robot's panel and the lock's sound is a
`motion::Event::Sound` for the robot. Shortcuts, each named in the code (the true
distance and the upkeep's fine bearing are ported since T-146): a locked base is not marked spotted; the
field +0xe6 reads the target's slot plus one, not a pointer; the lock window starts
at 0 each mission; a target is gone when its slot is empty or holds another kind.

Part 3 (`t-112-goals`): the goal bits as the distance and the steering each read
them (`script::goal_distance`, `goal`; T-053's port took bit 2 as the base for both
and gave no goal the distance 0xffff), the goal object (+0x3a, `Movement::goal_object`,
loaded and hashed) and the step's object check, the worked-out point
(`World::point`, from the image at load, hashed) and opcodes 0x66 to 0x68, 0x6d and
0x6e; the weapon state (`sense::weapon_state`, the routines' numbers, drives and
damaged bits read by `xeno_formats::sensing`) and opcodes 0x78 to 0x7c, 0x7b and 0x7c
selecting through the weapon keys' routine (`motion::select_weapon`). So ENGAGE,
ENGAGING, GO TO UNIT, SHADOW, PURSUE, DETECT and COMMANDO run; `sense`'s
`real_assets_two_stock_robots_find_and_close_on_each_other` shows scen00a's first
human humanoid and first alien robot, 7 units apart, lock each other and stand under
2 units apart within two minutes. Shortcuts, each named in the code: firing (0x7d)
did nothing and a weapon was never cooling down or out of rounds (both done by T-114); an
object goal on an empty slot gives the point 0xffff; the point carries over no
mission. The consoles' map-mode press sets one (T-133, below).

**Goals from a console's map (T-133).** `script::set_destination` (`1000:46c7`) and
`script::set_goal_object` (`1000:4685`), by `Sim::pilot` (`Pilot::Destination`,
`Pilot::GoalObject`), as `docs/screens.md`, "Under the pointer (T-133)", traces them:
the old object goal dropped (`sim_drop_object_goal`, only with bit 0), +0x32 written
whole (2 or 1), the destination the point or the robot's offset from the object, the
slot kept, a message read from `xeno.exe` with the lock's (`sensing::Messages::
destination`, `goal_object`) and event 5. The object's reference count is not kept (as
nowhere in the port). No new state: the state hash is unchanged.

**The target keys (T-138).** A (`1000:d812`, the same in every console's key table)
keeps the robot (`DS:0x71f7`), its side bit (+0x16 bit 3, `DS:0x709c`) and its kind's
**locking** range (`DS:0x7079`, `DS:0x7085`), then walks the 240-slot object table
(`DS:0xe2c4`, four bytes an entry: the object, its reference count) once round from
where the last search stopped: `DS:0x7205` the entry after the last one found, and
`DS:0x7207` the entries left before the search wraps to slot 0 (so one number, the
next slot, says both; a search that finds nothing ends where it began). It takes the
first object that is not the robot itself, has `SCANNED` (+0x16 bit 2: robots and
bases), is not `CLOAKED` (+0x17 bit 0), has the other side bit, and is within the
locking range by `1000:08ec` (under 256 units, the middle word at most the range),
stores the entry after it and locks it through `1000:d8af`, carry clear; with none,
the "none found" message opcode 0x71 posts, carry set. So A steps in slot order, not
by distance, and comes back round to the target it has (the lock's "already" message);
the lock's own checks (true distance, damaged computer) can still refuse it. R
(`1000:d7f8`) sets +0xe6 to 0 and posts its message; nothing else (the upkeep sets
+0xec bit 2 with no target). Neither checks who drives: both work under the program
too. `1000:d805` puts the search back at slot 0; each kind's console set-up calls it
(humanoid at `1000:a0e5`, scorpion `8489`, scout `bbb3`), so the stop position is one
for all robots, starts again whenever a console opens, and is not saved. A console
widget reaches A and R too: the lock buttons (`docs/screens.md`, T-130 to T-132); the humanoid's handler at
`1000:a3f1` flips +0x22 bit 3, then with AX bits 2 and 7 calls A (and sets +0x120 to
3) and with bits 4 and 7 calls R; the scorpion's and scout's call them at `1000:8a5c`,
`8a52` and `be37`, `be2d` the same way (T-162, done).

`sense::next_target` and `sense::drop_target`, by `Sim::pilot` (`Pilot::NextTarget`,
`Pilot::DropTarget`; `xeno-app` maps `1000:d812` and `d7f8`). `World::target_search`
keeps the next slot; it is hashed, not saved (a load opens the situation room, and
opening a console, `Sim::open_console`, starts it at 0). The R and A messages are read
from `xeno.exe` with the lock's (`xeno_formats::sensing::Messages::dropped`,
`none_next`).

## Contacts (T-141)

The contact check (`1000:d464`) is in every kind's tick table, so it runs once every
16 ticks; its entry is found by its address (`xeno_formats::sensing::KindTicks::contact`:
12, 8 and 7 in the stock build). In order:

1. A robot in its base (+0xfd bit 2) clears `BUMPED` (+0x23 bit 1) and is not checked.
2. While `bump_wait` (+0xfb) is not 0 it counts down by one and nothing is checked.
3. Every other object in the field (`IN_FIELD`), not its own side's base, whose
   distance (`1000:08ec`, 16.16) less both radii (+0x14, taken in the distance's low
   word's units, so a robot's 0x0a00 is 10/256 of a unit) is under 0x6400 (about 0.4
   units) is a contact. One ahead (`1000:45f2`) posts **event 9**. One closer than both
   radii (the difference negative) **bumps** the robot: `BUMPED` is set, the speed is
   negated and the speed asked for set to 0 (the velocity waits for the speed's next
   step, as for a hill), the kind's bounce routine runs (`DS:0x7209`, "The cockpit
   keys"), it takes a hit of 1 point (`sim_damage_object`, event 7 and the hit's
   sound) and `bump_wait` becomes 4, so the next four checks (64 ticks) are skipped.

Nothing in the check clears `BUMPED` but the base. Only robots run it, so the other
object bumps on its own check.

### The port (T-141)

`xeno_sim::contact::step`, after the sensing step in the robot's tick; `Movement::
bump_wait` is loaded from and saved to +0xfb and hashed; a bump reports
`Event::Bumped` (`xeno-tool simulate` prints "bumped into"). Shortcuts, each named in
the code (T-183): "ahead" (`1000:45f2`) is not traced, and is taken as the coarse
bearing within a quarter turn of the heading; the check stops at the first bump, and
whether the original goes on through the field, and whether a bump needs the object
ahead, is untraced. The stock scripts' event 9 handlers now run, and `ntrain`
plays out differently: at 110,000 retraces the first human scorpion is 27 units from
the alien base instead of 9 (it never bumps), so `xeno-app`'s
`real_assets_a_training_mission_is_won_and_lost_by_play` takes it over at 60,000.

## The cockpit keys (T-105)

A pass over the consoles' key tables (`docs/formats/key.md`; humanoid `DS:0x6b07`,
scorpion `DS:0x676a`, scout `DS:0x6efa`). Each handler runs with DI on the viewed
robot. Keys are set 1 make codes; the arrows' `E0` prefix is dropped, so the keypad's
arrows act the same, and the keyboard's repeat repeats them.

| Key | Humanoid | Scorpion | Scout | What it does |
|---|---|---|---|---|
| Left, Right | `1000:acc0`, `accd` | `9420`, `942d` | `c5e3`, `c5f0` | The pilot takes over (`order_switch_to_manual`, `1000:d446`: +0x23 bit 7 cleared, event 0xb), then the heading asked for is the **heading** less or plus `Gears::turn` (0x100, 1/256 of a turn, in the stock build). A held key repeats from the heading as it turns |
| Up, Down | `acda`, `ace0` | `943a`, `9440` | `c5fd`, `c603` | The pilot takes over, then a gear up or down (below) |
| `.` | `d0a0` | the same | the same | Opcode 0x53's routine: the speed asked for 0 and the bounce routine (the gear matches). The program keeps driving |
| Q, W, E | | | `c33d`, `c2f8`, `c2b3` | The stance the scout rises or sinks to (+0x10c): 0x60, 0x30 (standing), 0 |
| F2, F3, F4 | `d446`, `d42c`, `d3a2` | the same | the same | F2 the pilot takes over (bit 7 cleared), F3 the program (bit 7 set; the turn bits 4 and 5 cleared); F4 the pilot takes over and the next program is given ("Orders", T-111) |
| F5, F6 | `d131`, `d142`: weapon 0 or 1, by kind through `cs:0xd13c` (`b214`, `98a9`, `ca1d`) | | | The selected weapon (+0xdd) changes, with a message and sound 27 (`docs/formats/dsounds.md`) |
| F9 | `b20c` | | | The humanoid's weapon flips (+0xdd XOR 1), as F5/F6 |
| Space | `d0da`: fire, by kind through `cs:0xd0e3` (`b242`, `98f1`, `ca46`) | | | T-114 |
| F7 | `d92d` (humanoid, scout) | | | The ECM (not a missile): "The ECM and the cloak (T-136)" |
| F8 | `d95e` (humanoid, scout) | | | The sparkle cloak: "The ECM and the cloak (T-136)" |
| 1, 2, 3, 4 | | `d5e7`, `d609`, `d62b`, `d64d` | the same | Drop a mine, ECM buoy, alarm buoy or decoy: "Mines and buoys (T-137)" |
| A | `d812` | the same | the same | The next target: from where the last search stopped (`DS:0x7205`), round the 240-slot object table, the first other unit of the other side (+0x16 bits 2 and 3) not gone (+0x17 bit 0) and within the kind's range (`DS:0x7079`); locked through `1000:d8af`, else a message |
| R | `d7f8` | the same | the same | The target is dropped (+0xe6 = 0) with a message |
| C, D, L, U | | `8d7f`, `9ab8`, `9b2d`, `9afa` | | The relays ("Relay nets"): U deploys one where the scorpion stands (`sim_deploy_relay`, then the cargo redrawn); L loads the nearest within 0.08 units (the deployer whole, fewer than 12 carried, moving slower than 16); C a message on the cargo (+0x118); D flips +0xfd bit 4, the dump: a relay every 10 runs of tick entry 5 (T-149) |
| S | `ddb0` | | the same | +0xfa = 0: the status panel turns its page at the next count (T-139, below) |
| X, Y, Z | `b389`, `b39b`, `b377` | `9bdc`, `9bee`, `9bca` | `cb61`, `cb73`, `cb4f` | The second display's pan (+0xf4): X adds the pan step of the zoom (`DS:0x70d1[+0xf1]`), Z takes it off, Y sets 0; each clears `TRACKING` (`docs/screens.md`, "The display's other modes (T-134)") |
| F1 | `cff3` | the same | the same | The pause (also in the situation room, T-139, below) |
| Ctrl+P | | `d193` | the same | With unlock word 8 (`key.md`): power (+0x4e) 100, the signal (+0x4f), battery (+0x50) and drive (+0x51) 0xff, a beep (T-139, below) |
| Ctrl+Alt+S | `d177` | the same | the same | Flips `DS:0x62ec` bit 6: the shots on the maps (T-139, below) |

The V, T, M, `=` and `-` keys are the second display's (`docs/screens.md`, T-104);
Tab, Alt+Tab, ESC and Alt+Q switch views (T-043).

**Gears.** The up and down keys and the scripts' opcodes 0x51 and 0x52 call the same
routines, by kind (`DS:0x722d`, `0x7233`; `xeno_formats::motion::Gears`):

- A humanoid's gear (+0x10b) and a scout's (+0x109) is twice an index into a table of
  speeds asked for (`DS:0x68ec`, `0x6df8`; six gears, from fast reverse through stop to
  top speed, the same speeds as the opcodes 0x54 to 0x58). Up adds 2 unless it is at the
  top (10), down takes 2 unless it is 0; the speed asked for becomes the new gear's.
  The opcodes 0x54 to 0x58 set their gear by number with its speed.
- A scorpion has no gear: up adds 21 to the speed asked for, at most 63; down takes 21,
  at least -42 (refused at the limits). Its routines start with an `int 3`.
- At a limit a routine sets the carry, but the interpreter does not keep the opcode's
  flags, so the scripts never see it.
- **The bounce routine** (`DS:0x7209`), run by the hill bounce and the stop (opcode
  0x53, `.`): a humanoid's or scout's gear becomes the first whose speed is at or over
  the speed asked for, the top gear past them all; a scorpion's does nothing. The
  console redraws its speed panel (+0x12b, +0x132).

**Recall.** The kind routines run with the recall (`1000:b365`, `9b87`, `cb3d`) only
mark the console's panels for redrawing.

### The port (T-105)

`Sim::pilot` does the arrows, `.` and the scout's stances; `motion::shift` and
`motion::match_gear` the gears, used by the keys, the opcodes 0x51 to 0x58 and the hill
bounce. `Movement::gear` is loaded from +0x10b (humanoid) or +0x109 (scout) and hashed.
`xeno-app` maps each console's handlers to `Pilot` (`mission::handler::PILOT`) and
calls the sim for the viewed robot. The opcodes 0x54 to 0x58 find their gear with the
bounce routine, which gives the routines' gear because every script speed is a gear's.

Part 2: F5, F6 and F9 are `Pilot::Weapon` and `Pilot::OtherWeapon`: a change of
`Movement::weapon` (+0xdd) posts the kind's message to the robot's panel and plays the
sound (`Event::Sound`, only for the robot) read with the messages from the routines
(`xeno_formats::motion::WeaponKeys`). The scorpion's message is the damaged one when
its weapon's bit (+0xdc) is set, else the loaded one: its weapon counts are T-114's,
so the empty one is never posted. `Sim::recall` sets +0x23 bit 3 (the situation
room's F3 and recall button, for the selected unit); `motion::tick` gives a robot with
it the recall's program (`Tables::recall_program`) and its program takes over, before
it moves. U is `Pilot::Deploy` (T-113), A and R `Pilot::NextTarget` and `Pilot::DropTarget`
(T-138, "Sensing"). F7 and F8 are `Pilot::Ecm` and `Pilot::Cloak` (T-136). Keys 1 to 4
are `Pilot::Drop` (T-137), Ctrl+P `Pilot::Charge` (T-139, below), L, C and D
`Pilot::Load`, `Cargo` and `Dump` (T-149; mapped in part 2). The key still to port
is S (T-167).

## The developer and leftover keys (T-139)

From a Ghidra pass over the four handlers, the beep they call and the routines that
read what they change. F1 and Ctrl+Alt+S are also in the situation room's table
(`DS:0x5b20`), with the same handlers; Ctrl+P is in the scorpion's and scout's tables
only, and S in the humanoid's and scout's.

- **F1, the pause** (`1000:cff3`): pushes the game clock (`DS:0x374`), holds the
  colour records (`pal_hold_anims`, `1000:13f2`), beeps (below), then saves the pixels
  under the box at `DS:0x6ff2` (top, left, bottom, right: 64, 138, 76, 181 in the stock
  build; slots `0x50a`, `0x50e`), fills it with colour 0 (`0x51a`), frames it in 0x0f
  (`0x52a`) and types a string from the string segment (`mov si` at `1000:d03f`) in
  font 0, colour 0x0f, transparent (slot `0x3c2`) from (left + 4, top + 4). It empties
  the event queue (slot `0x338`, `1fe6:398c`), then waits in slot `0x340`
  (`1fe6:39b5`) for an event of mask 0x82a: a key press (0x800) or a mouse button
  press. That event is used up. It then puts the pixels back (`0x512`), marks the
  view for a redraw (`DS:0x6ff0` |= 0x22), releases the records (`1000:13fe`) and pops
  the clock, so no game time passes. Nothing ticks while it waits.
- **The beep** (`1000:0261`, far `1fe6:4329` → `1fe6:439a`): only while the game clock
  is not 0, the PC speaker at PIT divisor 0x4b0 (1,193,182 / 1200, about 994 Hz) for 4
  ticks of the clock, then 4 ticks of silence. F1 and Ctrl+P both call it.
- **S** (`1000:ddb0`): +0xfa = 0, the same as a press on the status panel (`1000:dd7f`).
  The tick's entry 4 steps the status page (humanoid +0x10a, scout +0x108) when
  +0xfa's low five bits are 0, then steps +0xfa (T-130, T-132), so the page turns at
  the next entry 4 (within 16 ticks) and then every 32 calls from there: S turns the
  page now rather than up to 32 calls later. The only other `[DI + 0xfa]` accesses
  are the humanoid's and scout's steps (`1000:b34a`, `cb22`), the scorpion's bit-7
  flip (`9a45`) and a write of 3 in `ui_sitroom_set_view_mode` (`1000:678f`, on the
  object it is given; not traced further).
- **Ctrl+P** (`1000:d193`): with `DS:0x1598` = 0xba17 (unlock word 8, `key.md`) power
  (+0x4e) 100, the signal (+0x4f), battery (+0x50) and drive (+0x51) 0xff, then the
  beep; without it, nothing. Unlike the new robot's set-up (`1000:d17d`) it leaves
  +0xfd bit 0 alone. The signal and drive are the relay net's (`relay::signal_step`,
  "The relay's tick (T-148)"), which writes them again at its next step; the battery
  is what firing draws on ("Nothing else recharges a robot").
- **Ctrl+Alt+S** (`1000:d177`): `DS:0x62ec` XOR 0x40. Bit 6 is read only by the map's
  shot routine (`1000:fa4d`, kinds 5 to 8; `docs/screens.md`, "The situation room's
  main view (T-103)"): with it, each shot is a 0x0f pixel on the situation room's map and on a
  console's map display; without it, none. No other code touches the byte. It is
  never cleared, so it lasts the run.

### The port (T-139)

`Pilot::Charge` sets the four bytes (part of the state hash and saves); `xeno-app`
sends it on Ctrl+P only with unlock word 8 from the user's `XENO.KEY`
(`Assets::developer_unlocked`, the map room's check). F1 pauses the mission in the
app: no ticks, the briefing stops typing, the colour records are held, the box and
its text (`xeno_formats::sitroom::SitRoom::pause_box`, `pause_text`, read from the
user's `xeno.exe`) are drawn over the view, and the next key or button press only
ends it. Ctrl+Alt+S flips `MapState::shots`. Not ported: the beep (no PC speaker in
`xeno-audio`; T-193), and S, whose +0xfa is worked out from the passes in the app
until T-167 keeps it in the sim (S and the status panel's press then set it to 0).
The shots flag is kept by the mission, not the run (a shortcut).

## The ECM and the cloak (T-136)

Traced with Ghidra. Both keys' routines are shared by the humanoid's and scout's key
tables (`DS:0x6b21`, `0x6f14` hold `d92d` and `d95e`) and by those consoles' cloak and
ECM buttons on a release (`docs/screens.md`, T-130, T-132). Neither checks the kind,
the robot's damage or whether it is in the field; neither plays a sound. Their
messages go through `1000:d53f` (`xeno_sim::log::say`: the robot's panel). The numbers
and messages below are read from the user's `xeno.exe` (`xeno_formats::weapons::Devices`).

**F7, the ECM** (`d92d` is an `int 3`, which DOS returns from; the routine is `d92e`).
With no charge left (+0xde is 0) it posts the "none left" message. Else it takes 1 of
energy through the energy check (`1000:d1b0`: only on the battery, +0xfd bit 0; short,
the energy goes to 0 and the check fails); short, it posts the weapons' "no power"
message (the humanoid gun's, `Messages::no_power`) and keeps the charge. Otherwise one
charge fewer, +0xdf = 20, +0x17 bit 1 (`ECM`) set, and the "ECM on" message.

**F8, the cloak** (`d95e`). With no cloak left (+0xe0 is 0) its own message. Else it
takes 2 of energy, **ignoring the check's answer** (short, the energy goes to 0 and it
cloaks anyway), one cloak fewer, +0xe1 = 120, +0x17 bit 0 (`CLOAKED`) set, then makes
an object of kind 14 (`sim_alloc_object_slot`, `1000:f4b1`, small pool) and, if a slot
was free, sets it up as the sparkle (`dfff`); the "cloak on" message either way.

**The timers.** They count weapon steps, not ticks: each kind's weapon step (two
tick-table entries in 16 ticks, so about every 8 ticks; humanoid `1000:b08a`, scout
`c8ab`) first takes one off a running +0xdf, clearing `ECM` when it reaches 0, then the
same for +0xe1 and `CLOAKED`. The scorpion's step (`972e`) counts only +0xdf. So the ECM
lasts about 160 ticks and the cloak about 960. Nothing else ends them.

**What they do.** `CLOAKED` hides the robot from the scanners, the map, the A key and
locks ("Sensing"). The ECM breaks locks on the robot: the lock's upkeep (`1000:d66f`),
after the damaged and range checks, loses a robot target (+0x16 bit 5) whose +0xdf is
not 0 (the time, not the status bit), as a lost lock: the lost message, event 4, +0xe6
cleared. A lock can still be taken on it; the next upkeep loses it.

**The sparkle** (kind 14). Set-up (`dfff`): the robot's x, y and z (32-bit each),
+0x18 (frame), +0x19 (ticks), +0x1a (age) and the word +0x1b (stage) 0, the heading
(+0x10) 0x10 and the radius (+0x14) 0x2000. It does not follow the robot. Its tick
(`e03b`, from `DS:0xe6e8[14]`, every tick): +0x19 one more; when its low three bits are
0 (every 8 ticks), the frame steps (0 to 7, wrapping) and the age one more; at age 40
and again at 80 the stage word adds 2; then the stage's routine from `CS:0xe073` runs:
stage 0 adds 6 to the heading and 0xcc to the radius, stage 1 nothing, stage 2 takes
them off. At age 120 or more it is removed (`f656`, `f5cd`). So it grows for 39
frames, holds for 40 and shrinks for 41, and lasts 960 ticks, as the cloak. The 3D
view draws it as the `cloak` model with the frame from +0x18 and, as a node type 6, its
heading as its size (`docs/render.md`).

### The port (T-136)

`Pilot::Ecm` and `Pilot::Cloak` (`weapons::ecm`, `weapons::cloak`); the weapon step
counts the times down; `sense`'s upkeep loses a lock on a robot with `Guns::ecm_time`
running. `Guns` keeps +0xde to +0xe1 (loaded, hashed, saved); `State::Sparkle` keeps
the sparkle's frame, ticks, age and stage (hashed, saved, read back from a save) and
`weapons::tick_sparkle` runs it. Shortcut: the sparkle is removed at once, as the
other kinds (T-054). Part 2 (`t-136-app`): `xeno-app` maps F7 and F8 by their handlers
(`d92d`, `d95e`) and runs them from the humanoid's and scout's cloak and ECM buttons on a
release (the look flipped while held), draws the sparkle with its frame, and the
consoles' cloak and ECM counts are the sim's. Left to check in DOSBox: T-191.

## Mines and buoys (T-137)

Traced with Ghidra. Keys 1 to 4 (`1000:d5e7`, `d609`, `d62b`, `d64d`) are in the
scorpion's and scout's key tables; their consoles' four drop buttons (scorpion `0xd4`,
`1000:8b06`; scout `0x12e`, `b88d`) run `1000:d5d5` with the button's number on a
release, which calls through the word table at `CS:0xd5df`: buttons 0 to 3 run keys 1,
4, 3 and 2. The numbers and messages below are read from the user's `xeno.exe`
(`xeno_formats::weapons::Drops`); the messages are string-segment offsets.

**The keys.** Each tests one count of the robot and nothing else (no kind, energy,
damage or field check; the humanoid has none of any, so its keys only say so):

| Key | Count | Kind made | Set-up | Tick |
|---|---|---|---|---|
| 1 | +0xe2 mines | 9, a proximity mine | `e5ab` | `e5b5` |
| 2 | +0xe4 ECM buoys | 12, an ECM buoy | `e63d` | `e64b` |
| 3 | +0xe3 alarm buoys | 11, an alarm buoy | `e602` | `e60c` |
| 4 | +0xe5 EMI decoys | 10, a decoy | `e5e8` | none (`e5fd` is a `ret`) |

With the count at 0, the key's "none left" message goes to the robot's panel
(`1000:d53f`). Otherwise an object is allocated (`sim_alloc_object_slot`, `1000:f4b1`,
the small pool); if none is free, keys 2 to 4 post the same "none left" message and the
mine's key says nothing; either way the count stays. Else the kind's set-up runs, which
posts its "dropped" message to the panel, and the count goes down by one. No sound.

**The set-up** (`1000:e801`, shared): x is the robot's plus a random word (`1000:0283`)
shifted right by 2 as a signed number (so within 1/8 of a unit), then y the same with
a second word; z (32-bit) the robot's; the side bit (+0x16 bit 3) the robot's, and no
other flag (not `IN_FIELD`, `TARGETABLE`, `SCANNED` or `UNIT`: shots, blasts, scanners
and locks never see them); +0x18 bit 0 when the robot is on its battery (+0xfd bit 0;
nothing reads it). The allocation cleared the rest: heading, radius and model height
0. Then by kind: the ECM buoy's +0x1b = 10 (its charges); the decoy's word +0x19 the
byte `DS:0xe70c[dropper's kind]`, the dropper's model (humanoid 0, scorpion 2, scout
4).

**The search** (`1000:e84f`, the range in `AX` as 8.8): the 240 slots in order, the
first object with `UNIT` (+0x16 bit 5: robots) of the other side within the range by
`1000:08ec` (under 256 units, the middle word at most the range). It does not look at
`GONE`, `CLOAKED` or `IN_FIELD`: a cloaked robot, or one in its base, sets them off.

**The ticks** (every tick, from `DS:0xe6e8` by kind). Each counts +0x19 (a byte) up and
acts only when it wraps to 0, every 256 ticks:

- **Mine** (9): with an enemy within 0x3c, the alert (below) that a mine went off, an
  explosion at the mine (`sim_spawn_explosion`, its own template), sound event 4 for
  everyone (`snd_play_sound_effect`), a blast of 50 points in radius 100 at the mine's
  x, y and z (`sim_damage_objects_in_radius`, `1000:e8eb`, as the tail missile's: every
  object in the field of either side, the dropper's too), and the mine is removed.
- **Alarm buoy** (11): +0x1a halves; +0x16 bit 4 is set while it is not 0, cleared at 0
  (the map shows the buoy in colour 0xff instead of 0x0f, `docs/screens.md`). Then with
  an enemy within 0x96 and +0x1a at 0: +0x1a = 0x10 and the alert that the alarm buoy was
  triggered. So an enemy that stays in range is reported every five looks (1,280
  ticks), and bit 4 shows from the look after an alert until +0x1a halves to 0.
- **ECM buoy** (12): +0x1a halves (no flag). With an enemy within 0x96 and +0x1a at 0:
  +0x1a = 0x40, the alert that the ECM buoy was triggered, and one charge (+0x1b)
  fewer; at 0 the buoy is removed (`1000:f656`, `f5cd`). So an enemy in range is
  reported every seven looks (1,792 ticks), ten times.
- **Decoy** (10): no tick.

**The alerts** go through `1000:d53b`, which is `1000:65ae`: a line in the log of the
base of the object's side (the dropper's), signed with the fixed alert name
(`1fe6:7f38` with `DI` = 0xffff, `1fe6:7fbc`; `xeno_formats::messages::Messages::alert`)
instead of the sender's. Nothing is sent to the robots' panels.

**What they do to the enemy.** The mine's blast is the only effect on another object.
No code was found that reads a buoy's +0x1a or looks for kinds 11 and 12 (a byte search
of the image for compares with 0xc and for `mov ax, 0xc` before a call finds only key
2): a triggered ECM buoy does nothing to the enemy beyond its alert, and the alarm
buoy's level only picks its colour on the map. The decoy's model is read back only by
the map's routine for kind 10 (`1000:fa70`: `1000:e5fe` puts +0x19 in `DX`, then the
robots' map routine `f976` draws it as a robot of the dropper's kind, with the robots'
checks: an alien one only once `SPOTTED`). In the 3D view kinds 9 to 12 make no node
(`docs/render.md`). Nothing marks a decoy `SCANNED`, `TARGETABLE` or `UNIT`, so the
other side's scans, locks and scripts never see it; it fools only the map.

### The port (T-137)

`Pilot::Drop(n)` (n 0 to 3 for keys 1 to 4) runs `drops::drop`; `drops::tick` runs the
three ticks from `motion::tick`; the alerts are `log::alert`. `Guns::drops` keeps +0xe2
to +0xe5 (loaded, hashed, saved) and `State::Drop` the object's +0x18 to +0x1b (hashed,
saved, read back from a save); the alarm's +0x16 bit 4 is the entity's (T-147). The search walks the world
in slot order for any robot of the other side. Removal is at once, as the other kinds
(T-054). Part 2 (`t-137-app`): `xeno-app` maps keys 1 to 4 and both consoles' drop
buttons, draws the decoy on the map with its dropper's model and the alarm's colour
from the sim, and the consoles' counts are the sim's. Left to check in DOSBox: T-192.

## Relay nets (T-113)

A T-090 pass on `xeno.exe` read the net opcodes, the grid they walk, the relay check,
the relay count and the deployment. The programs' grids and what deploying posts,
plays and makes are read from the user's `xeno.exe` by `xeno_formats::net`
(`Motion::net`).

**The grid.** `1000:49cc` picks a mask by the running program (+0x30): DENSE NET (13)
0xfe, SPARSE NET (12) 0xfc, any other (PROBE NET, 14) 0xfc, and a flag at
`DS:0x4db3` set for the last (the probe). It leaves the mask at `DS:0x4db2`; every
opcode that calls it puts 0xfe back, so between them the mask is always 0xfe (which
the editor's relay placement reads, T-142). A point's **cell** (`1000:49a1`, on x and
y's whole parts): each is rounded to the nearest multiple of the mask's lowest bit
(the step: 2 or 4) by adding half the step and masking the low byte, then, where x
and y differ in the step's bit, that bit is cleared in both. So cells lie on a
diagonal lattice: dense cells two units apart along the diagonals, sparse and probe
cells four.

**The line** (opcode 0x7e, `1000:5258`): the robot's cell goes to +0x10a and +0x10c
(words), and the destination's (+0x3c) is worked out. With `a` and `b` half the
differences in x and y, the line runs in the lattice's own coordinates: `u = a + b`
and `v = b - a` for the probe, `u = 2b` and `v = b - a` otherwise; on a grid of four
both are halved again. Their sizes go to +0x10e and +0x110, the step (2 or 4) with
bit 7 for `u` negative and bit 6 for `v` negative to +0x114, and the error to +0x112:
`|u| / 2` when `|u| >= |v|`, else `-(|v| / 2)`. The opcode keeps no flags.

**The next cell** (0x7f, `1000:52d8`): if the cell is the destination's (worked out
again), the carry is set and nothing moves. Otherwise, with `s` the step signed by
bit 7 and `t` by bit 6: an error at 0 or over loses +0x110 and the cell moves `s` in
both x and y (a diagonal step); else the error gains +0x10e and the cell moves `-2t`
in x (dense and sparse), or `t` in y and `-t` in x (the probe). The carry is cleared.
It is Bresenham's line in the diagonal coordinates, so every cell stays on the
lattice and the last is the destination's. Zero and sign are those of the last
coordinate written (y after a diagonal step, x otherwise).

**The other opcodes.** 0x80 (`1000:53ae`) makes the cell the worked-out point
(`DS:0x4da4`: the cell's x and y as whole numbers, no flags kept). 0x81 (`53c5`) is
the **relay check** (`1000:d15b`, far `2000:7faa`): every object of kind 4 in the 240
slots, of either side and in any state, is put on the relays' lattice (`1000:cca8`:
x and y's whole parts plus one with bit 0 cleared, then bit 1 cleared in both where
they differ) and compared with the cell; one there clears the carry, none sets it
(zero set either way). 0x82 (`53d3`) is the **relay count** (`1000:9aec`: +0x118, or
0 while the deployer is damaged, +0xfd bit 7) compared with 1: carry with none.
0x83 (`53dc`) **deploys** one, the carry set when it could not.

**Deploying** (`sim_deploy_relay`, `1000:9b01`; the scorpion's U key runs it too,
then redraws the console's cargo): with the deployer damaged or no relay carried, a
message ("no relay", `Net::no_relay`) to the robot's panel and the carry set.
Otherwise `1000:cc69` makes the relay: a slot of kind 4 (`sim_alloc_object_slot`; a
full small pool of 200 sets the carry, as above), set up by `1000:cc2a` (state 0,
folded; radius 5 (0x500); `TARGETABLE`; +0x18 and the stage 0; the clock +0x1e a
random byte (one random word drawn); power and its side 0; the four links' words
0xffff; the look and height for stage 0 by `1000:ccf0`), with the robot's x, y and z
copied: **not snapped to the lattice**, and with the flags' side bit clear (a new
relay is human until a net powers it). Then the "deployed" message (`Net::deployed`;
to the robot's panel, or a base's log if a base made it, `1000:d53f`), one fewer
relay carried, +0xdc bit 3 set (the console's frames; cleared when the base restocks
the cargo, `1000:9b97`), and sound 25 for the robot whose console is open.

**What the net programs do.** SPARSE, DENSE and PROBE NET share one script after
their headers (event 1 and a hit run ENGAGING; 5 and 9 have handlers of their own). Without a
destination (+0x32 bit 1) they post a message, stop and wait. Otherwise they start
the line, then cell by cell: the cell becomes the point; while more than about 1 unit
(0xfa) away they steer to it at high speed and recheck every 8 seconds; with a relay
in the cell they step to the next; else within 0.4 units (0x64), with relays carried
(else RET BASE, through state 4 and a restart), they creep to it at low speed,
rechecking every second, until within 0x0a (0.04 units), stop and run DEPLOYING
(state 15: wait 3 seconds, deploy if the cell is still empty, wait 3 more, leave the
state). A deployment that fails sends it home (RET BASE, through state 4 and a
restart), a cell still empty takes it back to the cell, and none left sends it home. At the line's end they drive to the destination within 0x32, stop,
post a message to the panel and the log, ask for the next path point (path command
4) and halt. So an alien scorpion on DENSE NET lays a relay in every empty cell
between its path points, one every few units, until its 12 are gone.

**The relay's tick** (`sim_tick_relay`, `1000:ccd9`; survey for T-148): +0x1e steps
every tick, and when its low four bits are 0 the state's routine runs (`DS:0x6fc7`),
then the look (`1000:ccf0`: colour +0x1d 1 unpowered, 0 human net, 2 alien by +0x16
bits 4 and 3; frames +0x19..=+0x1c from `DS:0x6fbb[stage]`, height +0x12 from
`DS:0x6fbf[stage]`). State 0 (folded): out of the field and unlinked, stage 0; it
rises (state 2) once no scorpion is within 0x32 (`1000:f703`, the nearest object of
a kind and its distance; carry when under the limit). State 2: stage up by one; at 3
up (state 4). State 4: the links (`1000:cd88`, not traced), `IN_FIELD`,
`RELAY_LINKED` while powered; a scorpion within 0x32 folds it (state 6). State 6:
stage down; at 0 folded. State 8 does nothing; state 10 is `1000:cf7e` (not traced).
So a relay a scorpion lays rises only after the scorpion moves on.

**The other relay keys** (T-149; the numbers and messages are read from the user's
`xeno.exe` by `xeno_formats::net`, `Net::load`, `cargo` and `dump`):

- **L** (`1000:9b2d`: `9b34`, then the cargo redrawn, `9bbe`). With the deployer
  damaged (+0xfd bit 7) the "no relay" message (the same string as deploying's);
  with 12 or more carried (+0x118, `cmp` with 0xc) a "full" message; with the speed's
  size (+0x26, `cwd`, `xor`, `sub`, compared unsigned) 16 or more a "too fast" one.
  Otherwise the nearest relay (`1000:f703` with kind 4: below) under 0x14 (0.08
  units) is taken in: +0x118 one more, the relay's slot freed (`1000:f656`, then
  `sim_free_object_slot`, `f5cd`; Ghidra mis-decodes the bytes from `9b6a`, which are
  `pop di`, `mov si, message`, `jmp`), and a "loaded" message; with none that near, a
  "none near" one. Every message goes to the robot's panel (`1000:d2a2`). The
  relay's state, side and power do not matter.
- **The nearest test** (`1000:f703`, kind in `AX`, a limit in `CX`): the nearest
  object of the kind (strictly nearer wins, so the first in slot order on a tie), by
  `1000:08ec`'s distance under 256 units, as 8.8; none gives 0xffff. L ignores the
  carry the limit (0x32) sets and compares the distance itself.
- **C** (`1000:8d7f`): one message to the robot's panel while +0x118 is not 0,
  another when it is (the deployer's damage is not looked at). The first holds a
  carriage return, so it is two lines.
- **D** (`1000:9ab8`): +0xfd bit 4 (`DUMPING`) flips. Set, +0x11c (`dump_wait`)
  becomes 10 and a "dump started" message; cleared, a "dump stopped" one; both through
  `1000:d53f` (the robot's panel).
- **The dump's step** (`1000:9a50`, entry 5 of the scorpion's tick table, so once
  every 16 ticks): with bit 4 set, +0x11c steps down; at 0 it is 10 again and
  `sim_deploy_relay` runs (a relay where the scorpion stands, one fewer carried, the
  "deployed" message and sound 25 for the open console); when that fails (none
  carried, the deployer damaged, or no room) D's routine runs, which clears bit 4
  with its "stopped" message (after deploying's "no relay"). So a dump lays a relay
  every 160 ticks while it moves or stands, until the cargo is empty. The step then
  marks the cargo for a redraw (`9bbe`: +0x138 and +0x165) and sets the gauges
  (T-131). Nothing checks the speed or the cell, and the net programs' own deploying
  is unaffected.
- **The console's buttons** C, D and L (`0xfa`, `0xfc`, `0xfe`; mouse `1000:8d53`,
  `8dad`, `8cbf`) are U's (`8d0b`): a press or release (`AX` bit 7) flips the
  shared look (+0x22 bit 3) and marks the button; a release (`AX` bits 2 or 4) then
  runs `8d7f`, `9ab8` or `9b34` and `9bbe`.

### The port (T-113)

`xeno_sim::net` has the grid: `cell` (`1000:49a1`), `lattice` (`1000:cca8`),
`relay_at` (`2000:7faa`), and the line, `start` (0x7e) and `advance` (0x7f), on
`net::Grid`, which `Movement::grid` keeps (+0x10a..+0x114, loaded from the file's
scorpions and hashed: the state hash changes). Part 1 (`t-113`) is the trace and these
checks.

Part 2 (`t-113-deploy`): the opcodes 0x7e to 0x83 in `xeno_sim::script` with the flags
traced above, `net::relays` (the count) and `net::deploy` (`sim_deploy_relay`: the
relay in the lowest free slot of the small pool, the messages, +0x118 one fewer,
+0xdc bit 3, sound 25), which the scorpion's U reaches through `Sim::pilot`
(`Pilot::Deploy`; `xeno-app` maps `1000:9afa` to it). `Movement::relays` (+0x118, was
`speed_table`) is the count, so a scorpion that lays relays speeds up as before. So
SPARSE, DENSE and PROBE NET and DEPLOYING run;
`script`'s `real_assets_a_stock_scorpion_on_dense_net_lays_relays` shows scen00a's
alien scorpion, the other robots taken out, lay at least five relays in 20 minutes,
each in a DENSE NET cell of its own within 0.04 units of the cell's point. Shortcuts,
each named in the code: a laid relay had no tick until T-148 (below, "The relay's
tick"); the net state
+0x10a..+0x114 is kept for every kind (a humanoid or scout running a net opcode would
overwrite its gear and light in the original; none is given one). Saves still write
the scenario as loaded, so laid relays are not saved.

### The port (T-149)

`xeno_sim::net` has L (`load`), C (`cargo`), D (`dump`) and the dump's step
(`dump_step`, from `motion::tick` after the signal step, at `Dump::step`), which the
scorpion's keys reach through `Sim::pilot` as `Pilot::Load`, `Cargo` and `Dump`
(the keys and the console's buttons in `xeno-app` are part 2, `t-149-console`). The nearest test is `relay::nearest`, which the relay's
fold uses too; L frees the relay's slot with `World::remove`, and the dump deploys
through `net::deploy`. `Movement::dump_wait` is +0x11c, loaded, hashed and saved;
`DUMPING` is in `Components::equipment` (+0xfd), as before. The cargo's redraw marks
are not kept: the console draws the gauges every frame (T-131). `relay`'s
`real_assets_a_stock_scorpion_dumps_its_cargo` shows ctrain's scorpion lay two
relays in 320 ticks of a dump and none after D again.

## The relay's tick (T-148)

A T-090 pass on `xeno.exe` read the relay's tick, its states, its look, its links and
power, and the signal and drive a robot takes from the net. The numbers are read from
the user's `xeno.exe` by `xeno_formats::relay` (`Motion::relays`).

**The tick** (`sim_tick_relay`, `1000:ccd9`, from the object pass in slot order): the
clock +0x1e steps by one; when its low four bits are then 0 the state's routine runs
by +0x1f (`DS:0x6fc7`, a word each for states 0, 2, 4, 6 and 8), then the look. A new
relay's clock is a random byte, so relays laid together do not run together.

| State | Routine | What it does |
|---|---|---|
| 0 folded | `1000:cd25` | clears `IN_FIELD` and `RELAY_LINKED` (+0x16 `and 0xaf`), stage 0; rises (state 2) unless a scorpion is within 0x32 (`1000:f703`, below) |
| 2 rising | `cd3d` | clears `RELAY_LINKED`, stage up one; at 3 up (state 4) |
| 4 up | `cd50` | the links (`1000:cd88`, below), sets `IN_FIELD`, `RELAY_LINKED` only while +0x21 (the power) is not 0; folds (state 6) when a scorpion is within 0x32 |
| 6 folding | `cd75` | clears `RELAY_LINKED`, stage down one; at 0 folded |
| 8 | `cd24` | a bare `ret` |

Nothing sets another state. The table's next word, `DS:0x6fd1`, starts the links'
direction table, so a state 10 would run its first routine; odd states would jump into
the middle of an address. A folded relay keeps its power, side and links as its last
up run left them (only the flags are cleared).

**The nearest test** (`1000:f703`, kind in `AX`, limit in `CX`): every slot of the 240,
any side and state, of that kind, its distance from the relay (`1000:08ec`) under 256
units, as 8.8; the carry is set when the nearest is under the limit. A robot in its base
has its position's top bits flipped, so it is never near. The limit is 8.8, so 0x32 is
about a fifth of a unit: a scorpion laying a relay stands on it, and the relay rises the
16-tick run after the scorpion moves away.

**The look** (`1000:ccf0`, after every state run, and when the relay is made): the
colour +0x1d is 1 without `RELAY_LINKED`, else 0 on the human side and 2 on the alien
(+0x16 bit 3); +0x19..=+0x1c all take `DS:0x6fbb[stage]` (3, 2, 1, 0 in the stock
build: the model's look per stage); the model height +0x12 takes the word
`DS:0x6fbf[stage]` (rising with the stage). A new relay's model height is stage 0's,
set by the set-up's look.

**The links** (`1000:cd88`, from the up state): only when the clock is 0, so once every
256 ticks. First the side's base power (`2000:6978`): the human base's +0x39 (the net's
power slider) shifted right twice, or the alien base's when the relay's side bit is set,
into `DS:0x6fba`. The four links' state bytes are cleared. Then every slot of kind 4 or
3 (relay or base) but the relay itself, its bearing and distance from the relay
(`1000:0530`: the bearing from the other to the relay plus a half turn, so from the
relay to it), under 256 units and within 0x640 (8.8: 6.25 units), is kept as the
nearest of its quarter turn (the bearing's top two bits: 0 is +y to +x), strictly
nearer replacing it (`1000:cf7e`..`cfab`, a routine each through `DS:0x6fd1`). Further
than 256 units the original jumps back into its loop with two words still pushed (its
counter and pointer trashed); no stock relay or base is that far from another.

Then, with `AL` the relay's power side (+0x22) and `CL` 0, each quarter turn's nearest,
in order 0 to 3:
- a **base**: its side (1 human, 2 alien, by +0x16 bit 3) must match `AL` (or `AL` is 0,
  and becomes it); then that link is 1 (fed) and the power offered is `DS:0x6fba`, the
  base power of the relay's own side, not the base's;
- a **relay**: its link the opposite way (`(n + 2) & 3`) must be 2 (open) and its +0x22
  must match `AL` (or `AL` 0 becomes it); then the link is 1 and the power offered is
  its +0x21;
- the largest power offered goes to `CL`.

+0x22 takes `AL`; the side bit is cleared and set again if `AL` is 2 (so the relay
changes sides with its power); +0x21 takes `CL - 1` (not below 0). Powered, every
link not fed is opened (2); unpowered, +0x22 is cleared and the links stay 0. So power
spreads one hop per 256 ticks from a base, falling by one a hop; a relay keeps the side
of the first net it took until it loses power for a run. A link's other byte and word
are never read.

**The signal and drive** (`2000:8087`, from each robot kind's tick table at entry 3:
humanoid `1000:b2fc`, scorpion `99f8`, scout `cad9`, so once every 16 ticks): the
signal (`2000:7d16`) goes to +0x4f. It looks at every relay and base of the robot's
side (+0x16 bit 3) with `RELAY_LINKED`, their distance (`1000:cfd8`, `1000:08ec`)
under 256 units as 8.8: one within 0x578 gives 0xff at once; otherwise the **last**
in slot order under 0x5b8 (not the nearest) is kept, and the signal is `-4 * (d -
0x578)` as a byte (0xfc down to 4), or 0 with none. No base ever has `RELAY_LINKED`:
nothing in play sets it on one and no stock base has it, so only relays carry the
signal. Then over 0x40 the robot is off its battery (+0xfd bit 0 cleared) and +0x50
gains one (to 0xff); at or under, `ON_BATTERY` is set. The drive +0x51 is the signal,
or +0x50 on the battery, ANDed with `DS:0x70a5[power >> 4]` (+0x4e, 0 to 100: 0, 0x17,
0x3f, 0x60, 0x7f, 0xf0, 0xff). After it, on passes (`DS:0x62c6`) with no bit of 0x1f0
set (16 of every 512), +0x50 gains one more (to 0xff), whatever the signal; the tick
also marks the console's power panel.

What the drive does is already ported: no drive, no speed; at or under the weak level,
half (`sim_limit_speed_by_drive_and_helm`, "Movement"); a weapon that needs more drive
than the robot has does not fire (`sense`, opcodes 0x78 to 0x7c); on the battery,
firing and the devices take energy (`1000:d1b0`, "Weapons and shots"). So a robot away
from its net runs on its battery: it moves at full speed while +0x50 masked by its
power is over the weak level, fires while it has energy, and recharges slowly (one a
512 ticks, 16 at a time); on a powered net it recharges one every 16 ticks more.

### The port (T-148)

Part 1 (`t-148`): `xeno_formats::relay` reads the state mask, colours, looks, heights,
top stage, the fold's kind and reach, the links' reach, the signal's reaches, the
battery level, the drive masks, the recharge mask and each kind's signal entry, after
checking every routine above. `xeno_sim::relay` keeps a relay's state
(`State::Relay`: frames, clock, state, stage, power, side and links; its
`IN_FIELD`/`RELAY_LINKED` flags are the entity's since T-147), loaded from the file, hashed and packed into saves (a
relay made in play gets the set-up's `TARGETABLE` and link words), and runs the tick:
the states, the nearest scorpion, the look and the model height. A relay a scorpion lays
is made with its clock, look and height (`net::make`); shots see a relay only while it
is in the field. `xeno-app`'s 3D view draws a relay with its frames. The state hash
changes.

Part 2 (`t-148-signal`): the links and power in the up state (`relay::links`), and
the signal step (`relay::signal_step`, at each kind's tick-table entry; `relay::signal`):
`Movement::signal` (+0x4f) is kept (loaded, hashed, packed), and the drive, the
battery bit and the energy follow the net. So a robot away from its side's net runs
on its battery: firing costs energy, it recharges one every 512 ticks, and its drive
falls with its energy (none at 0: it stops); a robot whose power (+0x4e) is damaged
gets its drive masked (`DS:0x70a5`), which with signals of 0x80 and over can leave
almost none. `xeno-app` shows the sim's signal in the consoles' power panels and the
detail panel, and the sim's link bits for relays on the maps. Shortcuts: an object 256
units or more from a relay is skipped by the links (the original's loop is left with
its stack unbalanced); a base's flags are not kept (none is ever linked).

`relay`'s tests: the states, look, reach and clock on synthetic relays; the links (a
base feeds the nearest relay, power falls a hop, the side follows it, the other side's
net is refused, a link run every 256 ticks); the signal (full reach, the last in slot
order, the fall, side and link); the signal step (battery level, drive mask, the
recharge). `real_assets_a_relay_a_stock_scorpion_lays_rises_once_it_moves_on`:
scen00a's alien scorpion lays a relay that stays folded under it, rises a stage every
16 ticks once it moves on, links within 256 ticks (alien, from the stock net beside
it) and gives full signal to a robot at the full reach from it that no other relay
reaches. `real_assets_stock_relay_nets_are_steady`: in every stock scenario a run of
the links reproduces every relay's side and links exactly (over 350 relays), and its
power but for `ntrain` and `strain`, whose relays are saved 68 higher than any base
gives. With the battery, T-150's play test no longer held: `htrain`'s player's base is
never destroyed with nothing pressed, and `ntrain`'s scorpion reached the alien base
with its weapons wrecked (T-177, below).

### Checked against the trace (T-177)

A second T-090 pass re-read every step above against `relay.rs` and found the port
faithful:
- **Nothing gives signal near a base.** The writers of +0x16 bit 4 are the relay's up
  state (`1000:cd61`) and another kind's tick (`1000:e61f`, not a base's); bases are made with
  `or [di+0x16], 0x46` (`1000:6237`) and every stock base is saved without it, so the
  signal's base test (`2000:7d16`, kinds 4 and 3 both reach the side and bit 4 tests)
  never passes for a base. The last-in-slot-order reading stands (`mov [0x6fd9], ax`
  for every reach under 0x5b8, no compare with the kept one).
- **The battery.** `cmp al, 0x40` / `jbe` (on the battery at 0x40 and under); the
  extra +1 on passes with no bit of 0x1f0 (`DS:0x62c6`, stepped before the object
  pass, `1000:f631`); entry 3 of each kind's tick table holds `b2fc`, `99f8` and
  `cad9`, as `xeno_formats::relay` checks. Firing on
  the battery takes 10 (humanoid weapon 0, scout plasma) or 22 (humanoid missile) or 1
  (scorpion cannon and tail, scout cannon), as `weapons.rs` reads them.
- **The drive** (`1000:d3b2`): none at 0, half at 0x28 and under, then the helm, as
  `motion::limit_speed`.
- **Nothing else recharges a robot.** +0x50 is written only by the signal step, the
  energy check (`1000:d1b0`), the new robot's set-up (`1000:d17d`, from `f812`) and
  Ctrl+P (`d193`); no script opcode reads +0x4f, +0x50 or +0xfd bit 0, so no alien
  program goes home to recharge: it does when its path ends, as in the port.
- **Missed, not about power:** entry 14 of every kind's table (`1000:df79`) calls
  `2000:7f10`, a repair on the net: with the signal at 0x80 or over and the damage
  total +0xf9 not 0, +0xfc counts each run; at 0xf0 it restarts and mends the first of
  the first weapon (+0xdc bit 0, unless +0xf6 bit 6), the second (bit 1, unless bit 7),
  the targeting computer (+0xec bit 0) and +0xf6 bits 0 to 3, with a message
  (`DS:0x18b3`). The port has no such step (T-179).

So in `htrain` with nothing pressed the alien attackers fight the human units on the
human net, run their batteries dry and lose, and the port's mission is never lost; the
original may be the same (T-175). Without units to shoot at they drive to the base and
home again without firing: no stock program fires at a base. T-150's test now loses
`htrain` by parking every unit (its console's arrows, Up and `.`) half a unit behind
the base, away from the aliens, so their shots hit the base (lost at retrace 62,782),
and wins `ntrain` by leaving the scorpion to its program for 110,000 retraces before
taking it to the alien base (its tail missiles cost 1 energy each, so the battery
never runs low; won at retrace 129,912; waits from 80,000 to 140,000 win, and 40,000
to 60,000, but not 70,000 or under 40,000).

## Damage (T-054)

A T-090 pass on `xeno.exe` read the damage routines, the destruction of robots and
relays, the explosions they leave and, as a survey for T-114, the humanoid's weapons
and two shot kinds. The tables are read from the user's `xeno.exe` by
`xeno_formats::damage` (`Motion::damage`).

**A hit** (`sim_damage_object`, `1000:f84d`) takes the object in `SI` (copied to `DI`)
and the points in `AL` (`AH` is 1 from the callers seen, and unused), and calls the
routine for its kind from the table at `DS:0xe783`: humanoid, scorpion, scout, base,
relay; every other kind (shots, explosions, obstacles) has a bare `ret`. Callers:
the hill bounce (0 points, `docs/sim.md`, "Movement"), a shot's direct hit
(`sim_damage_shot_target`, `1000:e954`, the shot's damage at its +0x22) and the blast
(`sim_damage_objects_in_radius`, `1000:e8eb`), both T-114.

**Robots** (`sim_damage_humanoid` `1000:af51`, `sim_damage_scorpion` `1000:94a0`,
`sim_damage_scout` `1000:c68a`, the same code with the kind's table):

1. If the robot is the one on view (`DS:0x62f1`), the screen flashes (`DS:0x64c` and
   +0xf8 set to 20; +0xf8 counts down each tick).
2. 0 points: nothing more. Over the kind's threshold (80 in each kind): the robot is
   destroyed (below) and nothing more, no event.
3. Otherwise, once for each point: a random word is drawn; its high byte picks an
   entry of the kind's chance table (`DS:0x6856` humanoid, `0x659a` scorpion,
   `0x6c06` scout; three bytes an entry, a chance and a routine): the chances are
   added up in a byte, and the first entry where the sum carries, or reaches the high
   byte or more, is taken. The last stock entry is 255, so one always is. The entry's
   component routine runs.
4. Event 7 (hit) is posted to the robot's script, also for 0 points, and a sound
   plays (event 21, `AX` = 0x2a, `docs/formats/dsounds.md`, "Sound events"; only if
   the robot's console is open).

The component routines come in three shapes, and `xeno_formats::damage::Part`
decodes each by its instructions: **wear**, a second random word's low byte masked
(7 for the helm at +0x24, 15 for the power at +0x4e) comes off the byte, stopping at
0; **bits**, the first clear bit of a list is set (a weapon at +0xdc damaged, or if it
already was, destroyed at +0xf6 bit 6 or 7; the targeting computer at +0xec bit 0;
the cameras, map, radar at +0xf6 bits 0 to 3; the scorpion's relay deployer at +0xfd
bit 7; the scout's weapons have no second bit); **nothing** (the last entry of each
table, 255, which takes the rest of the chance). Each part that changes something
shows a message in the robot's message panel (T-104). A robot hit for 80 or less
survives the hit, worn down; but every tick it adds its damage up (+0xf9) and is
destroyed when that reaches its kind's limit ("The damage total (T-140)" below).

**Destroying a robot** (`sim_destroy_robot`, `1000:de51`; also the developer key
Ctrl+D):

1. An object it was heading for is dropped (`sim_drop_object_goal`).
2. Its side's base counts a loss (+0x2b) and the other side's base a kill (+0x2c)
   (`sim_count_kill_and_loss`, `1fe6:6af6`; the `WIN` caption's numbers).
3. Its call sign's slot is freed (`DS:0x72c5` by +0xdb).
4. An explosion is made where it stood (`sim_spawn_explosion`, below), and a sound
   plays (event 2, `AX` = 4, heard everywhere).
5. It is taken out of its side's unit list, the rest moving up in order
   (`sim_remove_unit_from_list`, `1fe6:6e71`), the roster is marked for a redraw and,
   if it was the roster's selection, another is picked; if it was on view, the view
   changes (`1000:7ee7`).
6. Its slot is freed (`sim_free_object_slot`, `1000:f5cd`): the object's +0x16 bit 0
   (`GONE`) is set, the slot's pointer cleared, its reference count and its pool's
   count lowered. A slot is reused only once its reference count is 0 (references
   are taken by robots that target the object, T-112).

**Bases** (`sim_damage_base`, `1000:62b7`): a base already destroyed (+0x16 bit 0)
takes nothing. Otherwise the points are added to +0x2e; at 300 or more the base sets
+0x16 bit 0 and the other side is told (a message), and the mission's end runs from
the base's +0x30 (T-055); below that its own side is told at most once every 480
counts of `DS:0x374` (both through `1000:d54c`; `docs/screens.md`, "The message log
(T-129)"). A base is never removed.

**Relays** (`sim_damage_relay`, `1000:cfba`): the points are added to the byte at
+0x33; when that carries or reaches 20, an explosion is made and the slot freed. No
kill or loss is counted.

**Explosions** are objects of kind 13 (`sim_spawn_explosion`, `1000:e67b`): a slot is
taken (`sim_alloc_object_slot`, `1000:f4b1`: the lowest of the 240 slots with no
pointer and no references; the object comes from the small pool of 200, and if that
is full there is no explosion) and filled from a five-byte template
(`sim_init_explosion_from_template`, `1000:e79a`): the first frame (+0x18), the last
(+0x1a), the ticks a frame (+0x1b, and the countdown +0x1c), and a heading (+0x10);
+0x19 keeps the template's address's low byte, the radius is 8 and the position
(x, y and z) is the destroyed object's. A robot leaves frames 0 to 9 at 10 ticks
each, a relay 0 to 9 at 8. Its tick (`sim_tick_explosion`, `1000:e7e3`) counts +0x1c
down; at 0 it reloads it and steps the frame, and after the last frame frees its
slot. The 3D view draws it with the `explode` models (T-100). The base's end (T-055)
and shots (T-114) make explosions from other templates.

The new slot can be above or below the slot ticking: `sim_tick_all_objects` walks the
slots in order, so an explosion in a higher slot ticks in the same pass and one in a
lower slot from the next.

### The damage total (T-140)

The last call of each robot kind's tick (`1000:ac0a` humanoid, `9331` scorpion,
`c4af` scout, after the tick-table entry and the routine picked by +0x1b) adds its damage up into +0xf9
(humanoid `1000:afee`, scorpion `94e9`, scout `c6fd`; the same code with the kind's
numbers), in a byte, with wrapping adds:

1. A quarter of the power lost and of the helm lost: `(100 - byte) >> 2` for +0x4e
   and +0x24, worked out in a byte (a byte over 100 wraps to a large loss; the
   repairs never go past 100 in the stock build).
2. A weight for each damaged part whose bit is set, in this order. Humanoid: the
   weapons (+0xdc bits 0 and 1) 10 each, the targeting computer (+0xec bit 0) 10, the
   cameras, map and radar (+0xf6 bits 0 to 3) 5 each. Scorpion: the relay deployer
   (+0xfd bit 7) 7, +0xdc bit 3 1 (an `inc`), +0xdc bits 0 to 2 8 each, +0xec bit 0
   8, +0xf6 bits 0 to 3 5 each. Scout: +0xdc bits 0 and 1 12 each, +0xec bit 0 10,
   +0xf6 bits 0 to 3 5 each. Nothing counts the sensors (+0xf6 bit 4) or a weapon's
   destroyed bits (+0xf6 bits 6 and 7): they come with a damaged bit that does.
3. Capped at 100 and written to +0xf9.
4. At 50 (humanoid) or 40 (scorpion, scout) or more: if `CRIPPLED` (+0xfd bit 6) is
   clear it is set and **event 12** posted; below that it is cleared.
5. At the kind's limit or more the robot is destroyed (`sim_destroy_robot`, above):
   humanoid 70, scorpion 75, scout 55 for a human robot; 35, 40 and 30 for an alien
   one (+0x16 bit 3). So an alien robot is destroyed before it is ever crippled, and
   a human one is crippled first (in the fight below with seed 7, the crippled human
   humanoid drives home and is repaired).

The total is also cleared for a new object (`1000:dd79`), read by the repairs and the
alien base's launch (below, "The bases' phases"), which see it as the robot's last
tick left it, and by the roster's icon and the situation room's detail panel.

**The port.** `xeno_formats::damage::Total` reads each kind's numbers through the
call at the end of its tick, checking the routine's bytes (the two quarters, each
weighted test, the cap, the crippled bit and event, the side test and the call to
`sim_destroy_robot`). `Components::total` keeps +0xf9 (loaded, hashed and saved, so
the state hash changes); `damage::total` runs last in a robot's tick in
`motion::tick`. `weapons`' `real_assets_two_stock_robots_fight` (seed 5): scen00a's two
robots fight and the alien humanoid is worn to 35 and destroyed at tick 3838; with
the limits out of reach the same fight leaves it standing at 35.

### Weapons and shots (survey for T-114)

Superseded by "Weapons and shots (T-114)" below. The survey's "side check" after the
hit search is the kinds' hit test: shots hit either side.

### The port (T-054)

`xeno_sim::damage` does the above with `Motion::damage`. `damage::hit` is
`sim_damage_object`: a robot's points pick parts through its chance table (`pick`)
and wear its `Components` (which keep +0x24, +0x4e, +0xdc, +0xec, +0xf6 and +0xfd, the
bytes the stock routines touch; a routine on any other byte does nothing), then event
7 is posted; over the threshold `damage::destroy` counts the loss and the kill in the
bases' `State::Base` (`kills`, `losses`, and `destroyed` for +0x16 bit 0, loaded from
the file), makes the explosion, and takes the robot out of its unit list, the
`OrderBook` and the world. Bases and relays follow their routines. An explosion is an
`Entity` of kind 13 with `State::Explosion` (frame, last, ticks, countdown), in the
lowest free slot unless the 240 slots or the small pool of 200 are full;
`motion::tick` steps it and removes it after its last frame, and walks the slots so
that an object made during the pass in a higher slot ticks in it. The hill bounce
calls `hit` with 0 points, so it posts event 7. All of it is in the state hash.
`motion::Event::Destroyed` reports a robot or relay gone, or a base destroyed.
`Sim::hit` gives a hit between ticks (the shots are T-114), and `xeno-tool simulate
--hit TICK:SLOT:POINTS` uses it.

The sounds (T-061) are `motion::Event::Sound { event, only_for }`: event 21 with the
robot for a hit (the hill bounce too, not when it is destroyed), event 2 for everyone
when a robot is destroyed. They are not in the state hash (the original's sound calls
change nothing the sim reads).

Shortcuts, each named in the code: the screen flash and the messages (T-104); the call sign's slot, the roster's redraw and selection and the view's
change (T-103, T-104); a freed slot is reused
at once (the original waits for the robots targeting the object to let go, T-112);
+0x19 is not kept (T-100 draws it); the explosion's z is the destroyed object's
(T-119).

## Weapons and shots (T-114)

A T-090 pass on `xeno.exe` read each kind's weapon step and trigger, the guns, the
shots' set-ups and ticks, the hit search, the hit tests and the blast. The numbers are
read from the user's `xeno.exe` by `xeno_formats::weapons` (`Motion::weapons`).

**Firing.** Space and the scripts' opcode 0x7d (time 30) run `1000:d0da`, the kind's
trigger through `cs:0xd0e3`: the humanoid's (`b242`) and the scout's (`ca46`) set the
fire request (+0xdc bit 7); the scorpion's (`98f1`) sets its burst (+0x11e) to 5 and
the request. Opcode 0x7d sets no flags.

**The weapon steps** run from the kinds' tick tables (entries 1 and 9 for the
humanoid, `1000:b08a`; 1 and 12 for the scorpion, `9713`; 1 and 11 for the scout,
`c8ab`), so twice in 16 ticks. Each counts down the ECM's and cloak's times (+0xdf,
+0xe1, clearing +0x17 bits 1 and 0 at 0; the scorpion has no cloak) and the cool-downs,
then:

- **Humanoid**: the request (bit 7) is taken (cleared) and raises the selected
  weapon's arm (+0x103 or +0x104 = 1) unless the weapon is damaged (+0xdc bit 0 or 1);
  the walking animation (`1000:ae23`, `ae8a`, every tick after the legs) steps a
  raised arm every 4 passes (`DS:0x62c6 & 3 = 0`): state 1 lifts its frame (+0xff,
  +0x100) to 5, then sets +0xdc bit 5 or 6 (ready) and goes to 2; state 2 waits for
  the bit to clear, then 3; state 3 lowers the frame to 0, then 0. Otherwise the arm
  swings by `CS:0xaf11`/`0xaf21[step]` while walking and settles by one a tick when
  still; a destroyed weapon (+0xf6 bit 6 or 7) shows frame 6. Back in the step, a
  ready weapon whose cool-down (+0x108, +0x109) is 0 and with the target in the arc
  (+0xec bit 2) fires: the ready bit is cleared; the cannon needs a target ("no
  target" otherwise); both take energy (below); a shot is made (kind 7 the cannon,
  kind 8 the missile) at the muzzle offset for the heading (an eighth of a quarter
  turn from `DS:0x6a9f` or `0x6a7f`, turned by the quarter through `DS:0x708f`),
  0xe80 up; sound 1 or 0 for the robot whose console is open; cool-down 12 or 50
  steps. The cannon's shot does 4.
- **Scorpion**: the burst counts down, each step setting the request; the cool-down
  (+0x11f) counts down; a tail missile in flight (+0x123, a referenced slot) that is
  gone is let go (+0x123 = 0xffff). The request is taken: weapon 0 the cannon
  (`982d`): only in the arc, with rounds (+0x120, a word) and the cool-down 0 (each
  failure silent), and energy; +0xdc bit 6 flips to pick a barrel, and the barrel's
  damaged bit (0 or 1) stops it silently; +0x11d records the barrel that fired; a
  shell (kind 6) of 1 point, sound 5 for everyone, one round fewer, cool-down 2. So a
  burst of 5 requests fires 3 shells. Weapon 1 the tail missile (`978d`): with
  missiles (+0x122), its reload (+0x125) at 0 (counted down at entry 9), not damaged
  (+0xdc bit 2), a target, and energy (each failure a message); the last missile is
  let go, a kind-5 object is made and referenced in +0x123, sound 12 for the robot,
  the second display switches to the missile camera (+0xf0 keeps the mode, +0xef 3,
  T-134), "missile launched", +0xfd bit 5, one fewer, reload 400 ticks.
- **Scout**: weapon 0 the plasma gun (`c8ff`): a target ("no target"), the arc, its
  cool-down (+0x105) 0, not damaged (bit 0), energy; a kind-7 shot of 2 points at the
  muzzle (`DS:0x6c2c`) from the eye height (+0x1c) less 0x400; sound 1 for the robot;
  cool-down 8. Weapon 1 the cannon (`c990`): the arc, shells (+0x107, "out of shells"),
  cool-down (+0x106), not damaged (bit 1, a message), energy; a shell of 2 points
  (the muzzle offsets from `DS:0x6c4c` are worked out and then not used), sound 5 for
  the robot, a shell fewer, cool-down 3.

**Energy** (`1000:d1b0`, with the amount: 10 and 22 the humanoid's, 1 the scorpion's
and the scout's cannon, 10 the plasma): only on the battery (+0xfd bit 0) it comes off
+0x50; short, the energy goes to 0 and the gun fails with "insufficient power". The
weapons' messages are posted only while +0xfd bit 5 is clear, which they then set; a
shot fired clears it.

**The weapon state** (opcodes 0x78 to 0x7a): bit 1 (cooling) is the weapon's
cool-down not 0 (the scorpion's missile: its reload); bit 6 (none left) the
scorpion's rounds or missiles, or the scout's shells, at 0. Opcode 0x78 ignores bit 1.

**Shot kinds** (each kind's tick in `DS:0xe6e8`; the set-up runs with the maker in DI):

- **Kind 6, the shell** (`1000:e235`, `e2c8`): its damage; arming 15 steps; speed
  0x900; at the maker's x and y, 0x900 up. With a target (+0xe6) the heading is the
  lead's (below, speed 0x1200) plus a random spread of `(random & 0x1ff) - 0x100`, and
  it climbs 0x80; without, the maker's heading plus `(random & 0x7ff) - 0x400` and a
  random climb `random & 0x7f`. The level velocity is `sim_scale_by_heading`'s. Each
  tick it steps twice: x, y and z by the velocity, the climb less 3; below the ground
  (`sim_test_below_ground`) it explodes (template `DS:0x7315`); once armed, a hit
  explodes it (`0x732e`) and does its damage to the object hit. A shell of 1 point
  explodes without the sound.
- **Kind 7, the homing shot** (`e34a`, `e3ca`): its damage; arming 10 ticks; speed
  0x400; at the muzzle; no velocity; the maker's heading; the maker's target
  referenced (none: the shot is freed at once, after its sound and cool-down). Each
  tick its age steps; at 300 it is freed. On the ticks its age is a multiple of 8 it
  turns: to the target's coarse bearing by at most 0x2d8, climbing at the coarse
  bearing of (the target's z plus its kind's height, `DS:0xe7a5` shifted left 8, less
  its own; the level distance) (`1000:e95e`); the 3D velocity follows (`1000:045d`).
  A gone target frees it. Then it moves; on the ground it explodes (`0x732e`)
  quietly; once armed a hit explodes it (`0x7333`, sound 4) with its damage.
- **Kind 8, the missile** (`e48a`, `e537`): arming 10; speed 0x300; life 960 ticks; at
  the muzzle. A random spread `(random & 0x7f) - 0x40`; with a target the lead's
  heading (speed 0x300) plus the spread and a climb at the bearing of (the height to
  the target's top; the true distance, `1000:0519`); without, the maker's heading plus
  the spread, level. It moves once when made and keeps the heading 0x40 (its model's).
  Each tick its frame (+0x18) steps 0 to 3, its life counts down (freed at 0), it
  moves; on the ground it explodes quietly, once armed a hit explodes it with sound 4
  and 8 points. It does not turn.
- **Kind 5, the tail missile** (`e08e`, `e0e8`): 0x19c0 above the scorpion, its
  heading, the target referenced (none: freed), turn limit 0x16c. Phase 0 waits 20
  ticks on the tail; phase 2 boosts for 20 ticks: speed from 0x100, 0x80 more a tick
  up to 0x800, climb 0x16c, no ground or hit test; phase 4 homes every tick: the turn
  limit grows by 0x16, it turns to the target's coarse bearing by the limit, and within
  0x3c (8.8) of it the climb becomes the climb to it clamped to ±0xe38. On the ground
  it explodes (`0x7338`, sound 4); on a hit, or with its target gone, a **blast** of
  100 points in radius 100 (0x64, 8.8), then the explosion. It is removed as kind 15
  (`1000:f5a4`).

**The lead** (`1000:4549`, with the shot's speed): no target, the robot's heading; a
target not a unit, the lock's bearing (+0xea); a unit: the offset to it in 8.8 (the
middle words) and a quarter of its velocity are added up to five times, while a
quarter of the speed as many times does not yet cover the offset in x and y (signed;
the quarter speed is left in `DS:0x4da4`, the worked-out point's first word); then
half a quarter of the velocity is taken back and the heading is the (fine) bearing to
the result.

**The hit search** (`sim_find_shot_hit`, `1000:e8ac`): the 240 slots in order; the
first object in the field (+0x16 bit 6), not the shot, whose x and y are each within
its radius of the shot's (`1000:0ec3`: the 16.16 difference's high word 0 and the low
at most the radius) and whose kind's **hit test** (`DS:0xe761` through `1000:f6d0`)
passes: robots of every kind but the humanoid, bases, relays and mines and buoys
always; the humanoid when the shot is over its z by more than 0 and at most 0x1300;
an obstacle when the shot's z is 0 to 0x8ff; shots, explosions, sparkles and removed
objects never. There is no side check: shots hit friends. The object hit is kept
(`DS:0x7313`) for the damage (`sim_damage_shot_target`, `1000:e954`, the points in AL,
AH 1).

**The blast** (`sim_damage_objects_in_radius`, `1000:e8eb`): every object in the
field whose distance in three axes (`1000:0a11`: the largest axis, plus a quarter of
the other level axis and a quarter of the smaller of the larger level axis and the
height) is at most the radius (in 8.8, under 256 units) takes the points, halved for
each step past half the radius (the step grows by half each time: past r/2 half,
past 3r/4 a quarter, down to 0 points, which still is a hit). No side check, and the
maker is not spared.

### The port (T-114)

Part 1 (`t-114`): `xeno_formats::weapons` reads the guns, the shots' numbers and
explosions, the steps' tick-table entries, the hit tests, the kinds' heights and the
messages through the instructions above. `xeno_sim::shot` makes and flies the four
shot kinds (`shot::fire` with a gun, `State::Shot` ticked by `motion::tick` in slot
order), the lead, the hit search and the blast through `damage::hit`, and the
explosions from their templates. A shot is freed after the damage it does, so an
explosion that damage makes takes another slot. The shot's state is hashed. Shortcuts,
each named in the code: (until T-147, in the field by kind); (until T-146, the coarse bearing for
the lead and an exact level distance for the missile); a gone target is an empty slot or one a shot or explosion took; removal
frees the slot at once.

Part 2 (`t-114-fire`): `xeno_sim::weapons` has the trigger (`weapons::trigger`, from
opcode 0x7d and `Pilot::Fire`, which `xeno-app` maps to Space, `1000:d0da`), the three
weapon steps at their tick-table entries after the sensing ones (`weapons::step`), the
humanoid's arms after its walk (`weapons::arms`, with the swing tables now in
`Motion::walk`), the energy check, the weapons' messages and their once-only bit, and
the weapon state's cooling and none-left bits (`weapons::state_bits`, read by
`sense::weapon_state`); the scorpion's weapon key posts its empty message by the
count's low byte. `Movement::guns` keeps the energy, cool-downs, rounds or shells,
missiles, burst, barrels fired, the missile in flight, and the humanoid's arm frames and
raise, loaded from the file and hashed (the state hash changes). The guns' sounds are
`Event::Sound`, for the robot unless the gun's is heard by all. `weapons`'
`real_assets_two_stock_robots_fight`: scen00a's two robots of the closing test fire
and both are hit and worn (since T-140, with seed 5, until the alien is destroyed by
its damage total 55 s in). Shortcuts, each named in the
code: the missile camera's
switch is the console's (T-134); the arms' swing step is the legs'. (Until T-148 nothing recharged
the energy or set `ON_BATTERY`, so stock robots fired freely; now a robot off its net
pays for its shots, "The relay's tick (T-148)".) A robot
worn by many small hits is destroyed by its damage total (T-140).

Part 3 (`t-114-scene`): the scout's plasma gun fires from its eye height (+0x1c, kept
since T-119); `xeno-app`'s 3D view draws a
shot at its own height with its frame, and the humanoid's arms with their frames.
A base takes 300 points, which no robot's program shoots at in a stock mission's first
minutes, but a player does: `xeno-app`'s `real_assets_a_training_mission_is_won_and_lost_by_play`
wins `ntrain` with one scorpion through its console's keys (T-150).

## The mission's end (T-055)

A T-090 pass on `xeno.exe` read how a mission ends, every writer of the quit flag
`DS:0xe87e`, and the ticks of the structures. The values are read from the user's
`xeno.exe` by `xeno_formats::damage` (`Damage::base_end`).

**Only a destroyed base ends a mission with a result.** The base's tick
(`sim_tick_base`, `1000:6300`) calls a routine by the low four bits of +0x41 from the
table at `DS:0x5aea`, and entry 15 is `sim_end_mission_with_result` (`1000:68c1`). So
once every 16 ticks each base runs it, destroyed or not; it does nothing unless the
base's +0x16 bit 0 (`GONE`) is set, which only `sim_damage_base` sets (damage at 300
or more, "Damage" above). Then:

1. `sim_step_base_end_script` (`1000:e691`) takes +0x30 (the end step, 0 in every
   stock file) and returns the next step, by a jump table of 9 entries at `1000:e69d`
   indexed by `step / 4`:
   - steps 0 to 19: a sound by the step (`snd_play_base_end_step_sound`, `1000:e708`,
     the word at `DS:0x7342 + 2 * step`) and a **scattered explosion**
     (`sim_spawn_scattered_explosion`, `1000:e714`): one from a template
     (`sim_spawn_explosion`) at the base's position, then two random words move it
     in x and y by `(word & 0x3fff) - 0x2000` (sign-extended, in 1/65,536ths: within
     an eighth of a unit), and its radius (+0x14) is set to a value read from the
     instruction (40.0 in the stock build); then a third random word lowers it
     (`sim_lower_explosion_randomly`, `1000:e751`: `z -= word & 0x7ff`). The template
     is one for steps 0 to 3, another for 4 to 7, 8 to 11, and 12 to 19. The step
     goes up by one;
   - step 20: the sound, a **big explosion** from a fifth template at the base's
     position (no scatter), lowered the same way, its radius set from another
     instruction (64.0); the step goes up by **four**, to 24;
   - steps 24 to 35: the step goes up by one, nothing else.
   The sounds play through `snd_play_sound_effect` (`1000:ee90`, slot `0x44c`); they
   draw no random word. By step (T-065): event 3 at steps 0, 5, 12 and 17, event 4
   (a shot's explosion) at 3, 8 and 15, event 5 at 10 and 16, event 2 at 20, none
   at the others.
2. If the new step is 4 or 14, the base's +0x18 goes up by one (its look in the 3D
   view: a damaged frame).
3. Once the step is 0x24 or more (read from the `cmp` at `1000:68dd`): won
   (`scen_win_selected_scenario`) if the base's +0x16 bit 3 is set (an **alien** base
   destroyed), else lost (`scen_lose_loaded_scenario`); then the quit flag is set and
   the mission loop stops.

So a mission ends 33 runs, about 520 ticks (7.5 s at 70 Hz), after the base is
destroyed, with 21 explosions on it. The step is never reset; the table's entries past
the ninth are the code that follows it, which only a step of 0x24 or more read from a
file, or a resumed mission whose end already ran, would reach.

**No other result.** Nothing else sets a base's `GONE` bit or writes 1 or 2 to the
result (T-046). There is no timer, no check of units left (a side with no robots goes
on), and nothing specific to the training missions: their base is destroyed the same
way. The training menu's result 3 makes the won and lost routines return at once, so
a training mission's end changes nothing in the campaign; the quit flag is still set,
the mission stops, and the training menu returns to the map room with no cutscene
(`docs/screens.md`, "The training menu").

**The other writers of the quit flag** end the mission with no result: `1000:5c25`
the situation room's end button (a release on the sprite button that also flips the
buttons' shared look +0x40), `1000:5c50` its corner (`ui_sitroom_corner_on_mouse`),
`1000:7cf9` its key handler for Alt+Q (key word 0x2010). `1000:424b` is the last of a
group of short routines at `1000:4224`-`42a9` stepped through `DS:0x38ea` (each adds 2
to it) that pick random points on the 320x200 screen; it touches no object, so it is
no win or loss; what runs it is not traced.

**Structures.**
- A **base** ticks as above: the path phase (T-052) and phase 15's end; the other
  phases count each side's robots by kind and state for the situation room, take in
  robots that come home (phase 9: within a short distance they leave the field),
  run two counters fed by the situation room's sliders (11 and 12) and,
  for the alien base, launch the robots inside it (13); a second routine by +0x2d
  moves those three sliders (+0x39 to +0x3b). None of these decide a result; T-115
  traced them ("The bases' phases" below).
- A **relay** (`sim_tick_relay`, `1000:ccd9`, created) counts +0x1e and every 16
  ticks runs a routine by its state +0x1f (table `DS:0x6fc7`: states 0, 2, 4, 6, 8,
  10): it is set up in stages (+0x20, 0 to 3; out of the field until up), joins the
  net (+0x16 bit 4, its colour +0x1d by side) and can fold up again; its look and
  height follow its stage. It never touches a base or the result: the relay net is
  T-113's.
- An **obstacle**'s tick (`1000:e9cd`) is a bare `ret`, and it takes no damage. It is
  seen only by shots and by contact checks (both radii off the distance), through
  `IN_FIELD`: the scripts' sensing (T-112) and shots (T-114). Movement never tests it
  (T-052).

### The port (T-055)

`damage::step_base_end` is one run of the end script with `Damage::base_end`:
`motion::tick` calls it from a destroyed base's tick when the low four bits of its
count are 15, and `State::Base` keeps the step (+0x30, loaded from the file, in the
state hash). The explosions and the random words are the original's; at the last step
it reports `motion::Event::MissionOver { won }` (won for the alien base), and again at
each later run, as the step stays. `xeno-tool simulate` stops there. In `xeno-app`
the mission's tick returns it, and the screens end the mission with that result
through the campaign rules and the cutscenes, passing the player's base's kills and
losses; a training mission goes back to the map room. A base falls to shots in play;
the screens' flow tests end missions through `Sim::hit` (`mission::tests::destroy_base`),
as `xeno-tool simulate --hit` does; the port's Ctrl+W and Ctrl+L that did it in a mission
are gone (T-150).

Each step's sound is a `motion::Event::Sound` for everyone (T-061), the events
written in `damage::step_base_end` rather than read from `DS:0x7342`.

A base's look (+0x18, `State::Base::look`, from the file and in the state hash) goes up
by one as the step becomes 4 and 14: the frame of its model in the 3D view
(`docs/render.md`).

Shortcuts, each named in the code: with the small objects' pool full the original moves
whatever its allocator left, the port nothing; steps of 0x24 or more (only from a file
or a resumed ended mission) stay where they are, where the original's table runs into
code.

## The bases' phases (T-115)

A T-090 pass read the rest of `sim_tick_base` (`1000:6300`). Each tick it adds one to
+0x41 and calls the routine its low four bits pick from the table at `DS:0x5aea`
(read through the `call [bx + table]` at `1000:630b`), then the slider routine its
+0x2d picks from the table at `DS:0x5b0a` (`1000:6316`). By phase:

| Phase | Routine | Runs when | What it does |
|---|---|---|---|
| 0 | `1000:631b` | every 16 | Marks situation room panels for a redraw (the +0xc7..+0xcd bytes); every 128, two more |
| 1 | `1000:ecae` (far `2000:8186`) | every 16, alien base | Counts the robots on each alien path into the path records (+8 to +0xb; `usg.md`, "How paths are used"); the editor's display only |
| 2 | `1000:ea0e` | every 16 | The alien base's path phase (T-052) |
| 4 | `1000:6331` | every 256, in mode S | A redraw mark |
| 5 | `1000:6344` | every 256 (+0x41 & 0xf0 clear) | **The count**, below |
| 6 | `1000:6542` | every 16 | The roster's five rows from its scroll (+0x4c) into +0x42 (T-103's roster) |
| 9 | `1000:63dd` | every 128 (+0x41 & 0x70 clear) | **The take-in**, below |
| 11 | `1000:648b` | every 16 | **Relays made**, below |
| 12 | `1000:64af` | every 16 | **Repairs**, below |
| 13 | `1000:6515` | every 16, alien base | **The launch**, below |
| 15 | `1000:68c1` | every 16 | The mission's end (T-055) |
| 3, 7, 8, 10, 14 | `1000:63dc` | | `ret` |

**The count** (phase 5) clears +0x21..+0x27 and walks the whole object table: every
object of the base's side (+0x16 bit 3) adds one by kind: a relay to +0x21, a humanoid
to +0x22 if it is in the base (+0xfd bit 2) else +0x23, a scorpion to +0x24 / +0x25, a
scout to +0x26 / +0x27. A deployed relay has the side bit clear (T-113), so the human
base counts every deployed relay and the alien base none. +0x20 is not touched: it is
the relays in store (25 or 26 in stock files; phase 11). The situation room's status
panel shows +0x20..+0x27 (`docs/screens.md`, "The situation room's panels (T-127)").

**The take-in** (phase 9) remembers the base's side, then walks the object table in
slot order. For each robot (+0x16 bit 5) of that side it works out the distance from
the base (`1000:08ec`, the `max + min/2` distance, "Movement"): 256 units or more (`DH`
not 0, which includes every robot already in a base) it skips; out of reach (the
distance's 8.8 value over 0x3a, read from the `cmp ax` at `1000:642c`, a little under
a quarter of a unit) it clears +0xfd bit 1 and goes on; in reach it calls `1000:6445`
and **stops**, whatever that did. `1000:6445` does nothing if +0xfd bit 1 is set (the
robot was taken in, or launched, and has not left the reach since). Otherwise it sets
+0xfd bits 1 (`ENTERING_BASE`) and 2 (`IN_BASE`), clears +0x16 bit 6 (`IN_FIELD`),
turns the console to the base if the robot is on view (`1000:7ee7`), flips the top bit
of the position's x and y (`xor byte [di + 7], 0x80` and `[di + 0xb]`: half the 16.16
range away, so no distance, scan, shot or contact reaches it), asks for speed 0 (+0x28)
and hands it to its pilot (`order_switch_to_manual`, `1000:d446`: the program bit
cleared and event 0xb if it was set). So one robot is taken in per run, every 128
ticks. A robot comes this near only when sent there: the recall (F3, program 4, RET
BASE) drives it to the base's position.

**Relays made** (phase 11): +0x19 += +0x3b (the third slider); on a carry the word
+0xb5 goes up by one, and once it reaches 600 (`cmp` at `1000:6497`) with fewer than
70 relays in store (+0x20, `cmp` at `649f`), +0x20 goes up by one and +0xb5 restarts.
At the stock 0x40 that is a relay every 600 * 4 * 16 ticks, about 9 minutes at 70 Hz.

**Repairs** (phase 12): +0x1c += +0x3a (the second slider); once it reaches 0x1800 it
restarts and one repair step is made on the robot whose slot is +0x1a (0xffff none in
stock files). With no robot there (the slot empty, `1000:f691`; an `int3` the DOS
handler ignores precedes the search), it takes the first robot in its unit list (+0x4d,
38 entries) that is in the base (+0xfd bit 2) and damaged (+0xf9 not 0), takes a
reference on its slot (`f656`, `f6a6`) and keeps the slot in +0x1a; none, 0xffff.
Otherwise `2000:7e5f` makes one step (`2000:7e6d`) and then, if the robot's damage
total +0xf9 is 0, releases it (`f6bb`) and clears +0x1a. A step mends the first of, in
order: the helm (+0x24 up 4 and rounded down to a multiple of 4, until 100), the power
(+0x4e, the same), the first weapon (+0xdc bit 0, and +0xf6 bit 6 its destroyed bit),
the second (+0xdc bit 1, +0xf6 bit 7), +0xdc bit 2, the targeting computer (+0xec bit
0), the visual and thermal cameras, the map, the radar and the sensors (+0xf6 bits 0
to 4); with nothing of those, the kind's restock (`1000:df87` by `DS:0x723f`: nothing
for humanoids and scouts; a scorpion takes relays from the store up to 12 carried,
+0x118 from +0x20, clears +0xdc bit 3, gets 6 tail missiles (+0x122) and its deployer
mended, +0xfd bit 7). At the stock 0x40 a step is every 96 runs (0x1800 / 0x40), about
22 s.

**The launch** (phase 13, alien base only, +0x16 bit 3): every robot in the unit list
that is in the base with +0xf9 at 0 (whole) is launched by `1000:6716`: its x and y
become the base's (+4..+0xb copied), z 0, +0xfd bit 2 cleared (bit 1 stays, so the
take-in leaves it until it has driven out of reach), `IN_FIELD` set, +0x2c bit 7 (on a
path) cleared, +0xf7 (contacts) 0, then the new robot's orders (`1000:d2b6`: +0x23 = 0,
helm 100, speed 0, +0x25 = 0, program 9 through `order_set_program`, the program bit
set without an event). The alien path phase then gives it a random path and its
mission's program (T-052). The situation room's F2 and launch button do the same for
the player's selected unit (`1000:6704`, `5ba0`; `docs/screens.md`, "Launching"),
with no check of its damage.

**The sliders** (`DS:0x5b0a` by +0x2d, every tick while it is not 0): 1 and 2 lower and
raise +0x39 (the net's power, T-113: a base powers relays to +0x39 / 4), 3 and 4 +0x3a
(repairs), 5 and 6 +0x3b (relays made); 0 and 7 nothing. Lowering takes 2 off (only
from 4 up) and adds it to the pool +0x38; raising adds 2 (only up to 0xfd), taken from
the pool if it holds 4 or more, else 2 from one of the other two sliders (the first if
it is 4 or more and the second is not, the second likewise) or 1 from each. The four
add up to 0x100 (stock: 0, 0x80, 0x40, 0x40). Then the power rows are marked for a
redraw. What sets +0x2d is the situation room's arrows; its keys 1 to 6 call the
routines once (`docs/screens.md`, "The situation room's panels (T-127)").

### The port (T-115)

Part 1 (`t-115`): `xeno_formats::base` reads the phase table through the tick's
call, checks the count's and take-in's routines, and reads their masks and the reach. `xeno_sim::base::tick` is
the base's tick: the count of +0x41 and the path (2), count (5), take-in (9) and end
(15) phases. `State::Base::counts` holds +0x21..+0x27 (loaded, in the state hash, so
the hash changes). A robot taken in gets bits 1 and 2 in `Components::equipment`, its
position flipped as the original's, speed 0 asked for and its pilot driving, and
`motion::Event::TakenIn`; `xeno-tool simulate` prints "N taken in". Shots and blasts
skip a robot in its base, as `IN_FIELD` is cleared and set with bit 2 (on the entity
since T-147). Phases 0, 1, 4
and 6 are display only and not ported. `base`'s `real_assets_a_recalled_robot_is_taken_in`:
scen00a's slot 2, recalled, drives home and is taken in (tick 3234 with seed 1).

Part 2 (`t-115-launch`): `xeno_formats::base` also reads relays made (the wait and the
most in store), the repairs' period, the repair step's size and whole value, the
scorpion's restock, the launch's program and the sliders' least, most and step (with
the pool routine's numbers checked against them). `State::Base::depot` (`Depot`)
holds +0x20, +0x19, +0xb5, +0x1a, +0x1c, +0x38..+0x3b and +0x2d, loaded and hashed.
`base::tick` runs phases 11, 12 and 13 and, every tick, the slider held, which
`Sim::hold_slider(side, n)` sets (the situation room's arrows, T-127; its keys 1 to 6
run one step with `Sim::step_slider`). A robot
launched gets `motion::Event::Launched` ("N launched" in `xeno-tool simulate`), and
`base::launch` is the routine the situation room's F2 and launch button will call
(T-135: `Sim::launch`, below). In scen00a with seed 1 alien slot 34 comes home on its path at tick 11971 and
is launched four ticks later (`real_assets_the_alien_base_launches_a_robot_it_took_in`).
Shortcuts, named in the code: the repaired robot's slot holds no reference. The
damage total +0xf9 the repairs and the launch go by is kept since T-140 (a repaired
robot is released, and an alien robot launched, once its own tick has added its total
up to 0).

### The player's units in the base (T-135)

A T-090 pass read the situation room's F2 (`1000:6704`) and launch button (`5ba0`):
both launch the selected unit with `1000:6716` if it is in the base, and check nothing
else (`docs/screens.md`, "Launching"). `Sim::launch(id)` is that call: a robot in its
base (+0xfd bit 2) leaves at its side's base's position, whatever its damage total or
repairs; any other robot is left alone. The take-in (`1000:6445`) also turns the view
to the base when the robot taken in is the one on view (`1000:6460`, `7ee7`):
`xeno-app`'s `Mission::tick` answers `motion::Event::TakenIn` for the robot on view
with the situation room. A unit in the base cannot be viewed from the situation room
(Space, J, the view button, a roster row's right release) and the consoles' Tab and
unit-name button skip it; the situation room's Tab steps through it
(`docs/screens.md`, "Switching views"). No stock scenario starts with a unit in its
base (`real_assets_every_scenario_loads_with_its_units`). With seed 1, scen00a's slot
2, hit for 6 points and recalled, is taken in at tick 3106 with a total of 5, repaired
to 0 by tick 5989 and launched by `Sim::launch`
(`real_assets_a_robot_taken_in_and_repaired_is_launched_by_f2`); in the net-builder
training the first unit Tab selects (slot 21), recalled, is taken in about 1,900
retraces later, its console turns to the situation room and F2 launches it
(`xeno-app`'s `real_assets_f2_launches_the_selected_unit_taken_in`). The state hash
does not change for a mission played without F2.

## Heights (T-119)

A T-090 pass on `xeno.exe` read who writes an object's z (+0x0c), its model height
(+0x12, where its model's anchor sits above z in the 3D view, `docs/render.md`) and a
robot's eye height (+0x1c, the camera above z). All three are in 1/65,536ths of a unit
(a z is 16.16; the other two are words added to its low word).

**z.** `sim_alloc_object_slot` (`1000:f4b1`) clears every new object from +4, so an
object starts at 0 unless its set-up copies one: an explosion (`1000:e79a`) and a relay
(`1000:cc69`) take x, y and z from the object they come from; a shot is made at its
maker's z plus its gun's height (`docs/sim.md`, "Weapons and shots"). A robot's moves
end in `sim_check_robot_against_hills` (`1000:d50f`): `sim_test_point_in_hill`
(`1000:4446`) and, only when that fails, `sim_set_height_to_ground` (`1000:436c`). Both
walk the `MAPP` records for the first hill (kind 0x14) whose radius holds the point, so
the second never finds one: **a robot's z is set to 0 on every move that does not end
in a hill**, and a move into a hill leaves it as it was. Open hills (0x16) are tested by
neither (no stock file has one), so robots drive through them at 0; the hills a robot
stands on top of in the 3D view would need a z the code never writes. The launch
(`1000:6716`) sets z to 0. Nothing else writes a robot's z.

**The explosions' drop.** The base's end script (`docs/sim.md`, "The mission's end")
calls `sim_lower_explosion_randomly` (`1000:e751`) after each of its explosions: a
random word, `& 0x7ff`, negated and sign-extended, is added to the 32-bit z, so each
sinks by 0 to 0x7ff below the base's z. When the explosion could not be made, the
original lowers whatever the allocator left in `SI`.

**The model height** is set when an object is made (humanoid 0x0a00 `1000:aba7`,
scorpion 0x0a80 `1000:92df`, scout 0x0c80 `1000:c458`; 0 for shots and explosions; a
relay's by its stage, T-148) and then by the walking animations (`docs/render.md`,
"Walking"), each run at the end of the robot's move:

- the humanoid's (`1000:ad67`) on **every** tick, moving or not: `0x0a00 +
  CS:0xaf31[step]` (words), the step being the counter's (+0x105) after the speed is
  added and before a still robot's counter is cleared, so the tick it stops still
  shows its last step; its eye height is set to 0x0f80 there too;
- the scout's (`1000:c799`): moving, `0x0c80 + CS:0xc880[step]` (words); still and
  crouched (heading reached, stance not 0x30), `0x0c80 - 8 * stance` (+0x10b);
  turning or standing still, unchanged.

The scorpion's walk leaves both alone, so a scorpion keeps 0x0a80 and 0x0b80.

**The eye height** is set when a robot is made (above) and by the humanoid's walk
(0x0f80, every tick); the scout's stance step (`sim_step_scout_stance`, `1000:c5a3`)
sets it to `0x0d00 - 8 * stance` (the stance sign-extended) each time the stance moves
one towards its target, and nothing else does: a scout made at 0x0a00 keeps it until it
first crouches or rises. (The same step sets +0x13e to 3, the state of the console's stance gauge, so it is
redrawn: `docs/screens.md`, "The scout console's instruments (T-132)".)
Every stock scout is saved at 0x0b80, its standing height.

### The port (T-119)

Part 1 (`t-119`): `Entity::z` (+0x0c) and `Entity::model_height` (+0x12) for every
object and `Movement::eye_height` (+0x1c) for robots, loaded from the file, in the state
hash and packed into saves (`World::save`; a shot's z is its object's, no longer a field
of `Shot`). `motion::travel` sets a robot's z to `Terrain::ground_height` when it does
not hit a hill; the launch sets it to 0; an explosion takes the z of the object it
replaces (`damage::spawn_explosion`), a relay its deployer's, a shot its maker's.
`xeno-app`'s `scene` takes the camera's z and eye height and each node's z and model
height from the sim (`scene::heights` is gone). The state hash changes.

Part 2 (`t-119-walk`): `xeno_formats::motion` reads the bob tables and the bases (`Walk`:
humanoid and scout model heights and bobs, the scout's crouched base, the humanoid's eye
height and the scout's stance eye base) through the instructions above, with byte
checks. `motion::walk` sets the model height and the humanoid's eye height as traced;
`motion::steer_and_speed` sets a scout's eye height on each stance step; the base's end
lowers each explosion by its random word; the scout's plasma gun fires from its kept
eye height. `real_assets_stock_robots_bob_as_they_walk`: four stock scenarios' robots,
driven for two minutes, stand at z 0 and bob by their steps. The state hash changes
again (every humanoid's height moves with its step; a still one settles to step 0's).

## The flags and status bytes (T-147)

A T-090 pass over segment 1000 (and the far routines in 2000) found every instruction
on an object's +0x16 or +0x17: byte tests, ORs, ANDs, XORs and moves through SI, DI
and BX, by a byte search as well as Ghidra's listing (several routines are not
disassembled in the project), and the words at +0x16 (none: the only `mov [reg+0x16]`
words are in drawing code on other structures). The object maker (`1000:f4b1`) clears
every byte from +4, so a new object starts with both bytes 0; each maker then sets what
its kind needs. Nothing else writes the bytes, apart from the loader and the editor's
copy (`1000:f565`), which copy whole objects.

**+0x16, the flags.**

| Bit | Set by | Cleared by | Read by |
|---|---|---|---|
| 0 `GONE` | a base's damage reaching 300 (`1000:62ca`); freeing a slot (`1000:f5dc`) and removal to kind 15 (`f5b7`) | robots' tick-table entry 13 (12 for the scout) at `1000:dc8a` clears it if set; nothing sets it on a live robot, so it does nothing | the base's damage (`62b8`, none once set), the mission's end (`68c1`), the lock's upkeep on its target (`d685`: the target destroyed), the scorpion's missile check (`974f`) |
| 1 `TARGETABLE` | a made robot (`1000:f812`), a base (`sim_init_base`, `6237`: 0x46), a relay's set-up (`cc33`) | nothing | the lock (`d8c6`) |
| 2 `SCANNED` | made robots and bases (as bit 1) | nothing | the nearest-enemy searches (`1000:d797`, far `2000:8005`), the A key (`d85b`) |
| 3 `ALIEN` | the editor's new robot (`7e9f`, the side byte), the new scenario's alien base (`7c0d`), a drop takes its maker's (`e83f`), a relay follows its power's side (`ce40`, `ce48`); `sim_damage_base` flips it around a message to the other side (`62d3`, `62da`) | as set | every side check (dozens: scans, searches, contact, unit lists, kills, paths, the map) |
| 4 `RELAY_LINKED` | an up relay with power (`cd61`); an alarm buoy while its level is not 0 (`e61f`) | a relay's states (`cd25`, `cd3d`, `cd57`, `cd75`); the alarm buoy's look (`e615`) | the signal (`2000:7d16`: relays and bases of the side; nothing sets a base's), the relay's look (`ccf2`), the map's colours (`fa1e`, `fa38`, `fa5e`) |
| 5 `UNIT` | made robots (`f812`) | nothing | the loader (`7c6b`, `7c84`: the unit list and the program), the scan (`dd1c`), the lock's upkeep (`d6a0`: the warning and the ECM apply to units), the lead (`4551`: a unit's velocity is led), the mines' and buoys' search (`1000:e84f`, at `e875`), and `57aa` on the object at `DS:0x594a` (not traced) |
| 6 `IN_FIELD` | made robots (`f812`), bases (`6237`), the editor's obstacle (`e9c8`), an up relay (`cd53`), a launch (`673d`) | the take-in (`645c`), a folded relay (`cd25`) | the contact check (`d4a0`), the hit search (`e8c0`), the blast (`e90a`) |
| 7 | nothing | | nothing |

Shots (kinds 5 to 8), explosions (13) and the sparkle (14) keep the maker's zeros: no
side bit, not in the field, so nothing of theirs is seen by shots, scans or the map's
side checks. Drops (9 to 12) have the side bit alone (`1000:e801`). A relay laid in play
has `TARGETABLE` alone until it rises.

**+0x17, the status.**

| Bit | Set by | Cleared by | Read by |
|---|---|---|---|
| 0 `CLOAKED` | F8 (`1000:d975`) | the humanoid's and scout's weapon steps when the cloak's time runs out (`b0a8`, `c8c9`) | the searches (`d79d`), the A key (`d861`), the lock (`d8cc`), the map (`f989`) |
| 1 `ECM` | F7 (`d945`) | the weapon steps at the ECM's end (`b097`, `973b`, `c8b8`) | nothing: the upkeep reads the time (+0xdf) instead |
| 2 `SPOTTED` | the scan on units of the other side in range (`dd47`), the lock on its target, whatever it is (`d8e7`) | nothing | the map (`f983`) |
| 3, 4, 5, 7 `SCAN_COUNT` | the scan's count (`dcc0` to `dce2`) | the same | the same |
| 6 `TRACKING` | the lock (`d8b6`) | the consoles' pan keys and buttons (humanoid `b384`, `b396`, `b3af`, `b3c8`; scorpion `9bd7`, `9be9`, `9c02`, `9c1b`; scout `cb5c`, `cb6e`, `cb87`, `cba0`) and the second display's zoom (`da5b`) | the camera's pan in each kind's tick (`9352`, `b2d7`, `c4d0`); `9e80`, `81dd` and `b604` (with the target, on the clock's bit 5: a blink, not traced) |

Removal (`1000:f5a4`) sets a kind-15 object's +0x17 to 2 and its tick (`f5c1`) counts
it down to free the slot.

### The port (T-147)

`Entity` keeps both bytes, `flags` and `status`, for every object: loaded from the file
(`World::from_scenario`), hashed, and written whole into a save (`pack`, which no
longer patches single bits). They replace the copies the sim kept before:
`Movement::status` (robots only), `State::Base`'s `destroyed` (now `GONE`), the relay's
`IN_FIELD`/`RELAY_LINKED` (`Relay::flags`) and the alarm buoy's bit 4 (`Drop::alarm`).
`Entity::side` stays as the side's convenient form; the relay's tick sets `ALIEN` with
it. The readers read the bits where the original does: the scan counts units by `UNIT`
and spots them, the searches and the A key take `SCANNED`, the lock `TARGETABLE` and
marks its target `SPOTTED` (a base too), the upkeep drops a target that is `GONE` (or
no longer `TARGETABLE`, its slot freed and taken), shots, blasts and contacts see
`IN_FIELD`, which the take-in clears and the launch sets, the mines' and buoys' search
looks for `UNIT`, the signal for linked relays and bases. The makers set the
original's bytes: shots, explosions and sparkles 0 (and the port's side human, as the
file would say), drops the side bit, a laid relay `TARGETABLE`, the editor's obstacle
`IN_FIELD`, robots from their record. Not ported: entry 13's clear of `GONE` on a robot
(nothing sets it). The state hash changes (each object's two bytes; the robot's, base's,
relay's and drop's copies are gone). Tests: `weapons`'
`objects_made_in_play_carry_the_original_s_flags`, `sense`'s
`a_locked_base_is_spotted_but_the_scan_spots_only_units`, the take-in's and launch's
`IN_FIELD` in `base`'s tests, the save round trip (a status byte on each shot), and
`real_assets_every_scenario_loads_with_its_units` (every stock object's bytes load and
save unchanged) and `real_assets_a_stock_scan_spots_an_alien_robot_in_play`.

Part 2 (`t-147-maps`): `xeno-app`'s maps (`mainview::draw_map`, `draw_objects`, the
terrain editor's) read each object's bytes from the sim, not the scenario's file (the
`mainview::flags` table is gone). `real_assets_an_alien_robot_spotted_in_play_shows_on_the_console_map`:
in the humanoid training's console map, an alien robot 3 units away is not drawn until
the scan spots it, and then is.

## Loading a scenario

`World::from_scenario(&usg::Scenario)` (T-051, T-058) copies each `DCBS` object
field for field: its slot as the `EntityId`, its kind, its side (bit 3 of +0x16), its
x and y as `Fixed`, its heading and radius, and the damage its kind's layout records;
and `Terrain::from_scenario`, `Paths::from_scenario` take the `MAPP` records and the
`APTH`/`HPTH` paths in use (none for a missing chunk).
The orders come from the same objects (`OrderBook::from_scenario`), as loading leaves
them: the original gives every robot its program (+0x2e) anew, so each robot runs the
program it was given, whatever the file saved as running; the program bit is kept
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

## Saving the world (T-116)

A T-090 pass on `xeno.exe` read what a save writes of the running mission
(`file_write_save_game`, `1000:f01b`; `usg.md`, "Save files"): every object in the
object table, in slot order, with **nothing skipped**: robots, bases, relays,
obstacles, shots, explosions, mines and removed objects (kind 15) alike, each record
`0x172` or `0x34` bytes by its kind's pool flag (`DS:0xe7c7` bit 0), its slot and
reference count from the table. Before writing, `scen_pack_all_objects` (`1000:f0f4`)
runs each kind's pack routine (`DS:0xe81c`): only the three robot kinds have one
(humanoid `1000:b427`, scorpion `9c6c`, scout `cbff`; the base's `690b` and every other
kind's `f848` are a bare `ret`). A robot's routine turns its script pointer into the
saved form (`1000:4826`: less the program table's address `DS:0x4ff4`, ORed with
0xa000) and its target (+0xe6, a pointer) into its slot (`1000:f656`; 0xffff for none),
counting one more reference on the target (`1000:f6a6`). After the write
`scen_unpack_all_objects` puts both back (`1000:4833` restores a pointer marked 0xa0
without restarting; `1000:f6bb` drops the extra reference). Every other reference an
object holds (a goal object, a shot's or tail missile's target, the scorpion's missile,
the base's robot being repaired) is a slot already and is written as it is; the base's
selected unit (+0x3c) is a pointer written as it is. The path tables and the clock
(`DS:0x7d46`, hour to month) are written whole.

So a save resumes a mission as it stood, except what loading itself redoes
(`usg.md`, "Loading"): every robot restarts the program it was given (the scripts'
pointers, waits, stacks and flags start over) and drops its target, and what the
original carries from mission to mission (the stagger, the walking count, the lock
window, the worked-out point, the call signs taken) is not in the file.

**The port.** `World::save(&loaded, program_table)` gives `loaded` (the scenario the host
keeps: terrain, briefing, sky and each object's bytes as loaded) with the world written
over it: `SHDR`'s hour to sixtieths from `World::clock` (the day and month stay
`loaded`'s, the campaign's calendar), every object in the world over the loaded object
of the same slot and kind (bytes the sim does not keep stay as loaded), and both path
tables (count, missions, weights and points; the editor's mix, the counts of robots on
a path and leftover points stay). Robots get their movement, orders, script (packed as
above), panel, guns and kind's parts; every object its +0x16 and +0x17 (T-147); bases
their tick, counts, kills, losses, depot and damage; relays their damage; shots and
explosions their fields by kind (`usg.md`, "Kinds made during play"). An object made in
play starts from zeros, a reference count of 1 and the lowest index free in its pool.
`Sim::load` reads shots and explosions back (`pack::unpack_made`; a missile's damage is
the kind's). The shots no longer keep what their objects do not: a tail missile's
damage (its blast has its own points) and a missile's climb (used once, when fired), so
the state hash changes for a mission with those shots in flight.

`pack`'s tests run the synthetic mission with a relay deployed, a lock, a recall, one of
each shot and an explosion, save and load it: the world comes back equal to the running
one with the loader's resets applied, and saving and loading again changes no byte and
no hash. `real_assets_stock_missions_save_and_load_back` does the same for every stock
scenario after 4,500 ticks (fights, shots and relays in play), then runs both copies
2,000 ticks more to the same hash. Shortcuts: the reference counts are the loaded ones
(T-154); a shot or explosion made in play has no bytes beyond what the sim keeps (a
relay's are all kept since T-148).

## The mission clock (T-102)

`DS:0x7d46` holds the game clock: hours, minutes, seconds and sixtieths (then the
campaign's day and month). The start-up sets it to 12:00:00.00 (`1000:ecc2`), and a
scenario's `SHDR` start replaces all but the date (`docs/formats/usg.md`). The mission
loop calls `sim_advance_clock` (`1000:ed05`) once per retrace, just before the object
tick, so it runs on every mission view. It adds **4** sixtieths, then carries into
seconds, minutes and hours by subtracting 60 (24 for hours); past midnight it also
moves the campaign's date on (below). So game time runs 15 times faster than the
retrace count's seconds: an hour is 54,000 retraces, about 12.8 minutes at 70 Hz.

**The date (T-124).** The clock's last two bytes are the campaign's calendar: the day
from 0 (`DS:0x7d4a`) and the month from 0, January (`0x7d4b`), the same bytes the map
room's results move on (`scen_advance_campaign_day` adds 0 to 3 days with no month
check) and a save keeps in `SHDR`. When the hour carries past 23, the day goes up by
one and is compared, unsigned, with the month's length: a byte table of 12 days per
month at `DS:0x7d8f`, indexed by the month (`cmp al, [bx + table]` at `1000:ed53`,
which is how the port finds it; `xeno_formats::calendar`). At or past the length the
day goes back to 0 and the month up by one. That is all: no leap years, no year, and
no wrap past December (month 12 and on index the bytes after the table; a campaign
starting in April at a few days a result never gets there). Nothing else happens at
midnight: no message, no alert, no change to the scenarios' states. As the
comparison is made only at midnight, a day the map room has pushed past the month's
end stays there until the next midnight in play, which then starts the next month.

It then calls `pal_tint_for_time_of_day` (`1000:ed70`), unless `DS:0x7d4d` is set,
for the 3D view's colours (`docs/formats/palette.md`, "The lists in the stock
drivers"): a weight of 128 before 06:00 and from 19:00, `0x78 - 2 * minute` over
06:xx, 0 from 07:00 to 17:59 and `2 * minute` over 18:xx.

The port: `xeno_sim::Clock` in `World::clock`, from the `SHDR` start, in the state
hash, advanced first in `Sim::tick` with the table from `Tables::calendar` (without
tables only the day counts); `Clock::tint` is the weight. `xeno-app` starts the
records from it (`Mission::tint`). The original has one calendar; the port's campaign
keeps its own day and month between missions: `Mission::set_date` puts them in the
clock when a mission is played, `Mission::park` (every way a mission stops) gives the
clock's back, and `World::save` writes the clock's date into `SHDR`, so Ctrl+Alt+P
and the map room's saves carry a midnight passed in play. A month past the table
never ends in the port.

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
  limits no position, so the sim does not either. Movement (T-052) adds velocities with
  wrapping 32-bit adds, as the original does; stock scenarios stay near (128, 128) and
  never come near the ends of the 16.16 range.
- What the original keeps per object beyond kind, side and position is mostly
  unknown (T-058); entities grow as those fields are named.
