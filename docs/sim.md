# The simulation (T-050, T-051, T-052, T-053, T-054, T-055, T-058, T-059, T-102, T-105)

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
`0x4000 - angle`. A speed of 63 moves a robot 63/65,536 of a unit a tick at most;
that is the original's scale (T-053, "Orders", "The speed scale").

**The program bit.** +0x23 bit 7, which T-043 named "manual", is set when the robot's
program drives: the script interpreter (`order_step_script`, `1000:4899`) steps the
script only with it set (otherwise it only runs event handlers, and once every 64 ticks
of `DS:0x62c6` drops an object goal that is gone; T-053, "Orders"); recalling a robot and
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
program bit set, kept as `Orders::manual` until T-111, or a script's state). After the
move each robot walks (`motion::walk`, `docs/render.md`, "Walking"): its step counter
and footstep legs step, and its footsteps are `Event::Sound`s (13 to 15) for it.

The scripts (T-053, below) call `head_for_path_point` and
`join_nearest_path_point`. Left out until their tasks: the scout's height at +0x1c
(T-101), the path census (`sim_count_units_on_paths`) and the base's other phases.
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
| 0x7e to 0x83 | The relay net: walk a line of grid cells from the robot to its destination (state at +0x10a..+0x114), the cell as the point, whether a relay stands there, deploy one |
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

Shortcuts, each named in the code: an opcode not ported (sensing and object goals,
T-112; weapons, T-114; the relay net, T-113)
**halts the script on itself**, so a robot keeps its speed and heading; opcode 0x74
puts its message in the robot's panel (T-104) and 0x75 and 0x76 do nothing (the base's
log and 0x76's copies are T-129); the 64-tick object-goal
check is skipped (no object goals yet); field opcodes
read 0 at other offsets; return addresses are pushed as offsets from the script
area, not the program table; the stagger starts at 0 each mission.
So in the stock missions the alien robots on PATROL drive point to point along their
paths and the human robots on ENGAGE halt at the weapons check (T-114), and an alien
scorpion on DENSE NET halts at once (T-113).

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
| F2, F3, F4 | `d446`, `d42c`, `d3a2` | the same | the same | The pilot or the program takes over; the next program ("Orders"; T-111 names these the right way round) |
| F5, F6 | `d131`, `d142`: weapon 0 or 1, by kind through `cs:0xd13c` (`b214`, `98a9`, `ca1d`) | | | The selected weapon (+0xdd) changes, with a message and sound 27 (`docs/formats/dsounds.md`) |
| F9 | `b20c` | | | The humanoid's weapon flips (+0xdd XOR 1), as F5/F6 |
| Space | `d0da`: fire, by kind through `cs:0xd0e3` (`b242`, `98f1`, `ca46`) | | | T-114 |
| F7 | `d92d` (humanoid, scout) | | | Launch a missile: with one left (+0xde) and the energy (`1000:d1b0`), one fewer, +0xdf = 20, +0x17 bit 1, a message; else a message |
| F8 | `d95e` (humanoid, scout) | | | With one left (+0xe0): one fewer, +0xe1 = 120, +0x17 bit 0, and a kind 0xe object made (`1000:f4b1`, `dfff`); a message |
| 1, 2, 3, 4 | | `d5e7`, `d609`, `d62b`, `d64d` | the same | Drop a mine or buoy: with one left (+0xe2, +0xe4, +0xe3, +0xe5), an object of kind 9, 12, 11 or 10 is made (`1000:f4b1`) and set up (`e5ab`, `e63d`, `e602`, `e5e8`); else a message |
| A | `d812` | the same | the same | The next target: from where the last search stopped (`DS:0x7205`), round the 240-slot object table, the first other unit of the other side (+0x16 bits 2 and 3) not gone (+0x17 bit 0) and within the kind's range (`DS:0x7079`); locked through `1000:d8af`, else a message |
| R | `d7f8` | the same | the same | The target is dropped (+0xe6 = 0) with a message |
| C, D, L, U | | `8d7f`, `9ab8`, `9b2d`, `9afa` | | The relays: C a message on the cargo (+0x118); D flips +0xfd bit 4 with a message; L apparently loads one (fewer than 12 carried, moving slower than 16) and U deploys one (+0x118 counts them, and picks the speed table; U sets +0xdc bit 3 and plays sound 25); not traced further (T-113) |
| S | `ddb0` | | the same | +0xfa = 0 (not traced) |
| X, Y, Z | `b389`, `b39b`, `b377` | `9bdc`, `9bee`, `9bca` | `cb61`, `cb73`, `cb4f` | The second display's pan (+0xf4; T-134) |
| F1 | `cff3` | the same | the same | A box over the view until a key (not traced) |
| Ctrl+P | `d193` | the same | the same | With unlock word 8 (`key.md`): power (+0x4e) 100 and +0x4f, +0x50, +0x51 (the drive) 0xff |
| Ctrl+Alt+S | `d177` | the same | the same | Flips `DS:0x62ec` bit 6 (a developer switch) |

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
it moves. The keys still to port are T-113 (relays), T-114 (fire), T-134 (pan) and
T-135 to T-139.

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
destroyed when that reaches its kind's limit (T-107, `docs/formats/usg.md`, "Object
fields"; not ported, T-140).

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
counts of `DS:0x374`. A base is never removed.

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

### Weapons and shots (survey for T-114)

Read only far enough to scope T-114. A humanoid's weapon step
(`sim_step_humanoid_weapons`, `1000:b08a`) runs at tick phases 1 and 9 of +0x1a (every
8 ticks): it counts down two timers (+0xdf, +0xe1, clearing +0x17 bits 1 and 0 at
0) and the weapons' cool-downs (+0x108, +0x109); takes a fire request (+0xdc bit 7,
set by the scripts' opcode 0x7d) for the selected weapon (+0xdd, set by opcodes 0x7b
and 0x7c) when it is not damaged (setting +0x103 or +0x104, presumably the arm's
animation, which later sets +0xdc bit 5 or 6); and fires a weapon whose bit is set,
whose cool-down is 0 and with +0xec bit 2: with no target (+0xe6) a message; it needs
energy (+0x50, 10 or 22 taken when +0xfd bit 0 is set) and makes a shot of kind 7 or 8
at an offset by the heading's octant, with a sound, and a cool-down of 12 or 50 steps.
A shot of kind 6 (`1000:e2c8`) falls (its vertical speed drops by 3 a tick), lives
+0x20 ticks, explodes on the ground (`sim_test_below_ground`) or on hitting an object
(`sim_find_shot_hit`, `1000:e8ac`: the first object with +0x16 bit 6 within its
radius, and a side check) and then damages it by +0x22. A shot of kind 7
(`1000:e3ca`) turns towards its target by at most 0x2d8 every 8 ticks. The weapon
opcodes: 0x78 sets the carry unless a weapon is usable (present, enough drive, ...),
0x79 and 0x7a push a weapon's state word, 0x7b and 0x7c select a weapon, 0x7d fires
(by kind, through tables at `1000:d0e3`, `d107`, `d12b`, `d13c`). Scorpions and scouts
have their own routines, not read.

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
the explosion's height and +0x19 are not kept (T-100 draws it).

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
  run two counters fed by the situation room's sliders (11 and 12; not traced) and,
  for the alien base, launch the robots inside it (13); a second routine by +0x2d
  moves those three sliders (+0x39 to +0x3b).
  None of these decide a result; the port keeps only the path phase and the end.
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
losses; a training mission goes back to the map room. Ctrl+W and Ctrl+L in a mission
hit the alien or the player's base until it is destroyed (`Mission::destroy_base`,
through `Sim::hit`), standing in for the shots (T-114).

Each step's sound is a `motion::Event::Sound` for everyone (T-061), the events
written in `damage::step_base_end` rather than read from `DS:0x7342`.

A base's look (+0x18, `State::Base::look`, from the file and in the state hash) goes up
by one as the step becomes 4 and 14: the frame of its model in the 3D view
(`docs/render.md`).

Shortcuts, each named in the code: the explosions' height (the sim keeps no z yet;
T-101); with the small objects' pool full the original moves
whatever its allocator left, the port nothing; steps of 0x24 or more (only from a file
or a resumed ended mission) stay where they are, where the original's table runs into
code.

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

## The mission clock (T-102)

`DS:0x7d46` holds the game clock: hours, minutes, seconds and sixtieths (then the
campaign's day and month). The start-up sets it to 12:00:00.00 (`1000:ecc2`), and a
scenario's `SHDR` start replaces all but the date (`docs/formats/usg.md`). The mission
loop calls `sim_advance_clock` (`1000:ed05`) once per retrace, just before the object
tick, so it runs on every mission view. It adds **4** sixtieths, then carries into
seconds, minutes and hours by subtracting 60 (24 for hours); past midnight it also
moves the campaign's day on (`DS:0x7d4a`, by a days-per-month table at `DS:0x7d8f`,
and the month `0x7d4b`). So game time runs 15 times faster than the retrace count's
seconds: an hour is 54,000 retraces, about 12.8 minutes at 70 Hz.

It then calls `pal_tint_for_time_of_day` (`1000:ed70`), unless `DS:0x7d4d` is set,
for the 3D view's colours (`docs/formats/palette.md`, "The lists in the stock
drivers"): a weight of 128 before 06:00 and from 19:00, `0x78 - 2 * minute` over
06:xx, 0 from 07:00 to 17:59 and `2 * minute` over 18:xx.

The port: `xeno_sim::Clock` in `World::clock`, from the `SHDR` start, in the state
hash, advanced first in `Sim::tick`; `Clock::tint` is the weight. The date's
roll-over is not ported (T-124). `xeno-app` starts the records from it
(`Mission::tint`).

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
