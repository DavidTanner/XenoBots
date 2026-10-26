# `.usg` scenarios

## Status

**Layout confirmed** (T-020). `xeno_formats::usg` reads all 17 stock files to their last
byte. The chunk layout, and most of what each chunk is, comes from `xeno.exe`'s loader
and its chunk handlers. The object kinds and sides come from the game's built-in
scenario editor (below). The world, the terrain records and the paths are settled
(T-059); some object bytes are still unknown (T-107; the objects' layout is in
"Object fields").

## Files

17 files, 8 to 14 KB, not packed: `scen00a`..`scen12a` (the campaign) and `ctrain`,
`htrain`, `ntrain`, `strain` (training).

A `scendef.bin` record selects its file. The selection word (`DS:0x5f97`) holds the
record's `id` in its low byte and a variant in its high byte. The game writes `id / 2`
as two decimal digits and `'A'` plus the variant into the template `SCEN00A.USG`
(`DS:0x6006`, at `1000:739c`). So record `n` loads `scenNNa.usg`. Variants other than
`A` would load `B`, `C`, ... files, which a stock install does not have. The training
files are named directly (`DS:0x5e5c`). The same reader also loads the temporary save
`TEMPSCEN.USC`.

## Layout

A file is a chain of chunks, with no file header:

| Offset | Size | Field |
|---|---|---|
| 0x00 | 4 | tag, ASCII |
| 0x04 | 2 | size, u16 LE |
| 0x06 | size | data |

Every stock file has the same chunks in the same order: `SHDR`, `MAPP`, `DCBS`, `APTH`,
`HPTH`, `ORDR`, `BGND`, `TERM`. Every file ends right after `TERM`.

The reader (`1000:eefc`) opens the file and requires the first tag to be `SHDR`. It
then looks each tag up in a 9-entry table at `DS:0x7f26` (tag, handler) and calls the
handler, which reads the data from the open file. It skips the data of an unknown
tag, and it stops at `TERM`. The ninth entry, `SDEF`, is in no `.usg` file. It calls
the save-game loader for the `scendef.bin` table (`1000:76ab`, see `scendef.md`), so
save files use the same chunk format. `xeno_formats::usg` rejects a repeated known
tag, data after `TERM`, and a missing `TERM`. It keeps unknown chunks in
`Scenario::other_chunks`.

All values are little-endian. Positions are signed 16.16 fixed point: the high word is
the whole part. There is **no map grid** in the file.

### The world (T-059)

The world is an open plane with **no size and no edges**. Nothing in `xeno.exe`
limits or wraps a position: a robot's tick adds its velocity to x and y as 32-bit
numbers and then only tests the hills (below); no code compares a position with a
bound; the situation room's map scrolls by half a unit a step with no limit; and the
3D view and the maps work relative to the viewer. Only the 32-bit
values themselves end, at about ±32,768 units, far beyond anything stock files use.
The distance routine (`sim_approx_distance_to_point`, `1000:08f7`, and its entry
`1000:08ec` for two points) is `max(|dx|, |dy|) + min(|dx|, |dy|) / 2`, 16.16, and
gives 0x7fff.7fff when a difference overflows.

Scenarios are built around the map editor's origin, (128, 128) (`map.md`): stock
objects lie between about 76 and 175 in x and 112 and 163 in y, hills and path points
in about the same area. Height (z) is 0 on open ground.

### `SHDR` (16 bytes): header and start clock

`1000:f14b` reads 16 bytes, whatever the size field says.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `version` | 0x80 in every file. Any other value makes the game read only 0x16 bytes of each object and skip `APTH`/`HPTH` (an older layout). The parser accepts only 0x80 |
| 0x02 | 1 | `start.hour` | Copied to the game clock at `DS:0x7d46`. `1000:ed05` advances it: 60 ticks per second, then seconds, minutes, hours, days (from a days-per-month table) and months |
| 0x03 | 1 | `start.minute` | |
| 0x04 | 1 | `start.second` | |
| 0x05 | 1 | `start.tick` | Sixtieths of a second |
| 0x06 | 1 | `start.day` | From 0. The reader puts the campaign's day and month back after the file is read, so these two bytes do not take effect |
| 0x07 | 1 | `start.month` | 0 is January |
| 0x08 | 2 | `unknown_08` | 0xa0 in every file |
| 0x0a | 6 | `unknown_0a` | Zero |

### `MAPP`: terrain features

`1000:f373` allocates a buffer of the chunk's size (segment at `DS:0x150e`) and reads
the chunk into it. An empty chunk frees the buffer. The data is a list of records,
each starting with its own u16 size. A size of 0 ends the list. The rest of the chunk
is padding up to a whole 16-byte paragraph, and in stock files it holds leftover bytes.
Three files (`scen00a`, `scen04a`, `htrain`) have an empty chunk.

A record (`xeno_formats::usg::TerrainFeature`; the parser refuses one shorter than
0x16 bytes):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `size` | Of the record, this field included. 24 in every stock record |
| 0x02 | 1 | `kind` | 0x14 hill, 0x16 open hill or 0x12 block (below) |
| 0x03 | 1 | `point_slots` | 0x10 for a hill, 8 for a block (below) |
| 0x04 | 4 | `x` | Centre, 16.16 |
| 0x08 | 4 | `y` | |
| 0x0c | 4 | `z` | The height the shape is drawn from, 16.16 |
| 0x10 | 2 | `radius` | 8.8. A hill's radius; a block's half width |
| 0x12 | 2 | `diagonal` | 8.8. A hill's ring points on the diagonals; a block's half depth |
| 0x14 | 2 | `height` | 8.8. A hill's height above `z`; a block's half height |
| 0x16 | size - 0x16 | `shape` | The drawing program: op bytes ended by 0 |

`point_slots` is the number of 4-byte point slots the drawing code reserves on the
stack for the shape: 16 for a hill, 8 for a block.

**Kinds.** Each kind is also one of the ops of the shape programs (below). The robots'
hill test and the ground height test the kind byte; the 3D view and the maps go by the
record's `shape`, whose first op is the kind in every stock record:

- **0x14, hill.** An octagonal mound. A robot's tick (`sim_check_robot_against_hills`,
  `1000:d50f`, after the move; humanoid, scorpion and scout ticks all call it) tests
  whether the robot's centre is inside a hill's radius (`sim_test_point_in_hill`,
  `1000:4446`): if so its speed is negated, its target speed set to 0, a routine
  from its kind's table at `DS:0x7209` runs and it takes a hit of 0 points; otherwise its height is set
  to the ground's (`sim_set_height_to_ground`, `1000:436c`), which is 0 there. So
  hills are walls to robots, and robots stay at height 0. The maps outline a hill as
  an octagon with points at the radius on the axes and two thirds of it on both axes
  at the diagonals (`gfx_draw_hill_outline_on_map`, `1000:39fb`, which tests the
  shape's first op).
- **0x16, open hill.** Shots hit it, but robots drive through it. Op 0x16 runs the
  hill program too. Only `default.map` has one, and its `shape` is the hill's (0x14),
  so it looks like a hill in the 3D view and on the maps. The map editor reads it as a
  hill and saves it as one.
- **0x12, block.** A box of half width +0x10, half depth +0x12 and half height +0x14
  about (x, y, z). The map editor ends every save with a tiny one at the most negative
  position (`map.md`), so every scenario with terrain has one at about (-32640,
  -32640), where nothing ever sees it. Of the tests below, only the shots' sees it.

**The ground.** A point is inside a feature when the distance above, from the centre,
is less than +0x10 (the distance shifted to 8.8). The tests take the first record that
contains the point:

- `sim_set_height_to_ground` (hills only): inside, z = +0x0c + +0x14; outside every
  hill, z = 0.
- `sim_test_below_ground` (`1000:43d6`, every kind): whether a point is below the
  ground: inside a feature, below +0x0c + +0x14; elsewhere, below 0. The shots' ticks
  (`1000:e1ee` and the falling shot's `1000:e2f2`) end a shot there. So to a shot a
  feature is a cylinder of its radius up to that height.

**Drawing.** Each record becomes a render node of type 8 (`gfx_emit_terrain_render_node`,
`1000:41c0`, keeps the record's position and a pointer to it) and is drawn by
`gfx_draw_terrain_shape` (`1000:1b03`). That reads +0x10, +0x12 and +0x14 and runs
the record's `shape` from +0x16: each byte is an op, an even index into the handler
table at `DS:0x2988`, until a 0. Op 0x12 runs the built-in box program and ops 0x14
and 0x16 the built-in hill program, both in a table of shape programs in `xeno.exe`
(segment `3b53`); the other ops make points from the three sizes, take midpoints and
quarter points of projected points, and fill triangles and quads in a colour. Every
stock `.usg` record's `shape` is its kind and the 0. The hill program puts eight points on a
ring at height z - +0x14 (at the radius on the axes and at (±+0x12, ±+0x12) on the
diagonals) and one at the centre at z, and fills the facets between them in colours
57 to 61. With the editor's values (z and +0x14 both a third of the radius) the ring
lies on the ground. The exact facets and colours are for the 3D view (T-101).

### `DCBS`: objects

`1000:f198` resets the object pools, reads a u16 count, then reads that many records:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `size` | Size of the object in memory. 0x172 for robots and bases, 0x34 for relays and obstacles |
| 0x02 | 2 | `slot` | Index in the 240-entry object table at `DS:0xe2c4` (pointer, then the next field) |
| 0x04 | 2 | `ref_count` | Stored beside the pointer. The game drops it by one when an object that refers to this one is removed |
| 0x06 | 2 | `kind` | The object's first word in memory |
| 0x08 | size - 2 | object bytes +0x02 onwards | |

So a record is `size + 6` bytes. The allocator (`1000:f4ee`) takes a 0x172-byte
object (at most 40, from `DS:0xa8cc`) or a 0x34-byte one (at most 200, from `DS:0x7f64`)
by a flag per kind. It writes the kind and the number it allocated into the object's
first two words. The reader then overwrites everything from +0x02 with the file.

#### Loading (T-058)

For a version 0x80 file, the reader keeps every byte of an object as the file has it,
then (`scen_on_object_loaded`, `1000:7c6b`):

- An object with flag bit 5 (a unit: the robots) has its script pointer turned back
  into an address and is added to its side's **unit list**, 38 entries kept by the
  side's base (`sim_add_unit_to_side_list`, `1fe6:6e42`), in file order. The list
  follows whichever base was loaded last for that side, so a base must come before
  its robots (it does in every stock file).
- A base (kind 3) becomes its side's command object (`DS:0x568e` human, `DS:0x5690`
  alien; `scen_set_up_side_base`, `1000:626d`). Its height and the words at
  +0x42..=+0x4b are cleared.

After the last object, `scen_unpack_all_objects` (`1000:f109`) calls each kind's
"unpack" routine (table `DS:0xe83e`; the matching "pack" table at `DS:0xe81c` runs
before a save). For a robot it turns the script pointer into an address **again**: the
value is no longer in its saved form, so the robot is given its program anew
(`order_set_program` with +0x2e). So at load every robot runs the program it was given
(+0x30 = +0x2e), from its start, whatever the file saved as running. It also drops the
robot's target (+0xe6) and the reference count it held on it. The manual bit is kept.
The other kinds' routines do nothing.

Files with another version take a different path (`scen_on_old_layout_object_loaded`,
`1000:7c84`): only 0x16 bytes are read and the rest of the object is set up as the
editor does for a new one (`sim_init_new_object`, `1000:f812`, and the kind's routine
in `DS:0xe73f`).

#### Object fields

Offsets in the object, as in memory. `xeno_formats::usg::Object` names the first 0x18
bytes, and `Body` the rest by kind (`Robot`, `Base`, `Relay`; anything else stays
bytes). Unnamed bytes are kept as `unknown_XX` fields.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `kind` | 0 humanoid, 1 scorpion, 2 scout, 3 base, 4 relay, 16 obstacle. These are the editor's numbers and names (it calls them `HUMAN`, `SCORP`, `SCOUT`, `BASE`, `RELAY`, `OBST.`, with -1 for `PATH`). Kinds 5 to 15 are made during play: 6, 7 and 8 are shots (T-054's survey, `docs/sim.md`, "Weapons and shots"), 13 an explosion; the rest are not traced. Removing an object can turn it into kind 15 |
| 0x02 | 2 | `pool_index` | The object's place in its pool when it was saved. Some stock files repeat a number |
| 0x04 | 4 | `x` | Position, 16.16 |
| 0x08 | 4 | `y` | |
| 0x0c | 4 | `z` | Height, 16.16. Zero in every stock object. The line-of-sight code lifts it for a moment by a per-kind height (table `DS:0xe7a5`, 8.8: 10, 10, 6, 20, 6 for kinds 0 to 4, 6 for obstacles) |
| 0x10 | 2 | `heading` | 65,536 to a turn. The editor's arrow keys step it by 0xb6 (a degree). The 3D view's render node copies it (`mxx.md`) |
| 0x12 | 2 | `unknown_12` | Set per kind when an object is made (8.8: humanoid 10, scorpion 10.5, scout 12.5; a relay's from a table by its state). Not traced |
| 0x14 | 2 | `radius` | 8.8. Set per kind when an object is made: base 35, scorpion 18, humanoid and scout 10, obstacle 13, relay 5. Contact checks take both radii off the distance; the render node keeps the whole part |
| 0x16 | 1 | `flags` | Bits below |
| 0x17 | 1 | `unknown_17` | More state bits: the scanners skip an object with bit 0; a robot's targeting sets bit 6. Not traced |

The flags at +0x16 (`usg::flag`):

| Bit | Name | Meaning |
|---|---|---|
| 0 (0x01) | `GONE` | Destroyed or removed. Clear in every stock object |
| 1 (0x02) | `TARGETABLE` | A robot may take the object as its target. Robots, bases, relays |
| 2 (0x04) | `SCANNED` | The scanners list it. Robots and bases |
| 3 (0x08) | `ALIEN` | The side: the editor writes 0 for human, 8 for alien |
| 4 (0x10) | `RELAY_LINKED` | Relays: picks the relay's colour in the 3D view; a base's network check looks for it. The name is a guess |
| 5 (0x20) | `UNIT` | A robot: registered in its side's unit list at load. Set on every stock robot and on nothing else |
| 6 (0x40) | `IN_FIELD` | Shots and contact checks only see objects with it. A robot loses it inside its base and gets it back when launched. Robots, bases, obstacles and set-up relays |
| 7 (0x80) | | Not seen used |

**Robots** (0x172 bytes, `usg::Robot`):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x1a, 0x1b | 1, 1 | `tick`, `state` | A tick counter stepping by 2 (kept to 0..=0x1e for scorpions and scouts), and a state number; each picks a routine from the kind's table every tick. A scorpion steps its speed when the counter's low four bits are 0 (T-052, `docs/sim.md`, "Movement"). The state's routines are not traced |
| 0x1e, 0x20 | 2, 2 | `velocity_x`, `velocity_y` | Added, sign-extended, to `x` and `y` every tick (1/65,536ths). Worked out from the heading and speed only when either changes: `speed * sin(heading)`, `speed * cos(heading)` |
| 0x23 | 1 | `mode` | Bit 7: named manual by T-043, but set when the **program** drives (T-052: the script steps every tick only with it; starting a program sets it, the drive keys clear it; T-111); bit 3: recalled (every robot's next tick gives it program 4 and clears the bit); bits 4 and 5 take 0x40 off or add 0x40 to the target heading every tick (the pilot's turn keys); bit 0: the alien base gives it no path |
| 0x24 | 1 | `helm` | 0 to 100; a hit can take 0 to 7 off |
| 0x26 | 2 | `speed` | Steps by one towards `target_speed`, limited by the drive, the helm and (scorpions) a speed table: every tick, or every 8 ticks for a scorpion. In 1/65,536ths of a unit a tick |
| 0x28 | 2 | `target_speed` | |
| 0x2a | 2 | `target_heading` | Every tick `heading` turns towards it by at most the kind's step (read from `xeno.exe`; the same for the three kinds). The editor writes the same value here and at +0x10 |
| 0x2c | 1 | `path` | The robot's path: slot and flags ("How paths are used") |
| 0x2d | 1 | `path_point` | The point it heads for; 0xff its base |
| 0x2e | 1 | `program_given` | A number in `xeno.exe`'s program table (T-043) |
| 0x30 | 1 | `program_running` | That program, or a state the script entered by itself. Replaced by +0x2e at load (above) |
| 0x31 | 1 | | The script's stack pointer: 10 empty, stepping down by 1 or 2 a push; the stack is the bytes from +0x44 (T-053, `docs/sim.md`, "Orders") |
| 0x33 | 1 | | The script's flags: bits 0, 6 and 7 the carry, zero and sign of its last test; bit 5 runs the script every tick and holds events off (an event's handler sets it) |
| 0x38 | 2 | | The ticks the script waits before its next opcode; a program starts with a staggered wait (T-053) |
| 0x32 | 1 | `goal` | What the robot heads for: bit 1 the point at `destination` (0x22 from a path, 2 from the hold order), bit 0 an object (slot or pointer at +0x3a, a reference held), bit 2 its own base, bit 3 its target (+0xe6), bit 4 a point the script worked out; 0 nothing (T-053) |
| 0x34 | 2 | `script` | The script pointer (a data-segment offset in `DS:0x5034..0x5670`), saved as `0xa000` + its offset from the program table; flag bits in the top nibble restart the program given, or program 9, at load (`order_unpack_script_pointer`) |
| 0x36 | 2 | | Events waiting for the script, a bit each (event n is bit n): 5 a hold point set, 7 hit, 0xa and 0xb the program or the pilot took over, others not traced (T-053) |
| 0x44 | 10 | | The script's stack (T-053) |
| 0x3c | 8 | `destination` | Where it is heading, x and y (16.16): a path point or its base |
| 0x4e | 1 | `power` | Power systems, 0 to 100; a hit can take 0 to 15 off |
| 0x51 | 1 | `drive` | 0xff in nearly every stock robot. At 0 the robot cannot move; at or under a level read from `xeno.exe` it goes at half speed. A guessed name; +0x4f and +0x50 are reset to 0xff with it |
| 0xdc | 1 | `weapons` | Weapons damaged: bits 0, 1, 2 for the kind's first, second and third weapon (the third is the scorpion's tail missile). A humanoid's bit 7 is a fire request (script opcode 0x7d) and bits 5 and 6 its first and second weapon about to fire (T-054) |
| 0xdd | 1 | | A humanoid's selected weapon, 0 or 1 (script opcodes 0x7b, 0x7c; T-054) |
| 0xe6 | 2 | `target` | The robot's target: an object pointer in memory, its slot in a file (0xffff: none). The loader clears it |
| 0xec | 1 | `targeting` | Bit 0: the targeting computer is damaged. Bit 2 is set in stock robots; the other bits are not traced |
| 0xf6 | 1 | `systems` | Bits 0 visual camera, 1 thermal camera, 2 map, 3 radar, 4 EMI sensors damaged; 6 and 7 the first and second weapon destroyed |
| 0xfd | 1 | | Bit 7: a scorpion's relay deployer damaged (a hit can set it; with it the relay count reads 0). Bit 0: firing takes energy from +0x50; bit 5: the no-energy message was shown (T-054) |
| 0x108, 0x109 | 1, 1 | | A humanoid's first and second weapon's cool-down, counted down every 8 ticks (T-054) |
| 0x10b, 0x10c | 1, 1 | `stance`, `stance_target` | A scout's stance and the one it is rising or sinking to (0x30 standing; it moves only standing); a humanoid's speed gear (+0x10b, an index into its cockpit's speeds, updated when it hits a hill) |
| 0x10c, 0x127, 0x10d | 0x35, 0x45, 0x3c | | The humanoid, scorpion and scout console's widget state (T-043) |
| 0x118 | 1 | `speed_table` | A scorpion's speed goes through `xeno.exe`'s speed table `speed_table / 2` (0x0c, the last, in stock scorpions; other kinds hold other values and do not use it) |

**How robots take damage** (`sim_damage_object`, `1000:f84d`, calls the kind's routine
in the table at `DS:0xe783` with the damage in `AL`): a robot hit for more than 80 is
destroyed at once (`sim_destroy_robot`, `1000:de51`). Otherwise each point of damage
picks one of the kind's components by chance from a table (`DS:0x6856` humanoid,
`0x659a` scorpion, `0x6c06` scout; three-byte entries of a chance out of 256 and a
routine) and damages it: the helm or the power percentage drops, or a bit above is
set (a second hit on a damaged weapon destroys it). The message panel reports each.
Robots have no hit points: a robot hit for 80 or less survives. T-054 traced the
details, destruction and the explosions it leaves (`docs/sim.md`, "Damage").

**Bases** (0x172 bytes, `usg::Base`):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x2b | 1 | `losses` | The side's robots lost this mission (T-045) |
| 0x2c | 1 | `kills` | Enemy robots destroyed this mission |
| 0x2e | 2 | `damage` | Damage taken (`sim_damage_base`, `1000:62b7`). At 300 the base is destroyed (`GONE`) and the other side is told; below that its own side is told, at most once every 480 counts of `DS:0x374`. Playing a campaign scenario adds 0, 50, 100 or 150 to the player's base by the record's alert level (`sim_apply_scenario_alert`, table `DS:0x5a3f`) |
| 0x30 | 2 | `end_step` | Once the base is destroyed, the mission's end steps it every 16 ticks with explosions; at 0x24 the mission ends, won if the alien base was destroyed (`sim_end_mission_with_result`, T-046, T-055: `docs/sim.md`, "The mission's end"). 0 in every stock file |
| 0x34, 0x37 | 2, 1 | `typed`, `view_mode` | The orders' typing position and the main view's mode (0, 2, 4, 6, 8: O, S, M, R, C; T-044) |
| 0x41 | 1 | `tick` | Steps by one every tick; its low four bits pick the tick's routine (`DS:0x5aea`): at 2 the alien base hands out paths (T-052, `docs/sim.md`, "Movement") |
| 0x4d | 2 | | The side's unit list, set at load |
| 0xbd | 0x43 | `view_state` | The situation room's widget state (T-043) |

**Relays** (0x34 bytes, `usg::Relay`): +0x33 `damage`, at 20 the relay is removed
(`sim_damage_relay`, `1000:cfba`). +0x1f and +0x20 are its set-up state, and a new
relay sets the words at +0x25, +0x29, +0x2d and +0x31 to 0xffff. Its tick
(`sim_tick_relay`, T-055) counts +0x1e and every 16 ticks runs a routine by +0x1f
(set-up stage +0x20, linked from +0x21, its colour +0x1d); not traced further (T-113).
**Obstacles** (0x34 bytes) have no damage routine and a tick that does nothing, so
shots do not harm them and they never change; nothing past +0x18 is known.

Every stock file has exactly one base per side, which supports the side bit. Relays
sit on whole-number positions in chains near their own side's base.

### `APTH` and `HPTH` (1122 bytes each): paths

`APTH` (alien) is read into `DS:0x7372` and `HPTH` (human) into `DS:0x77da`, as one
block each. Older-version files skip both. The block is a u16 count of paths in use,
then 8 path slots of 140 bytes:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 1 | `point_count` | The points in use. The editor's add-point key writes the point under the pointer at `points[point_count]` and counts it (at most 16); its remove key drops the last. A path with none is skipped |
| 0x01 | 1 | `mission` | The editor names it from `DS:0x5cf5`: `NET DENSE`, `NET SPARSE`, `MAINTAIN`, `RECON`, `PATROL`, `SKIRMISH`, `WATCH` (it cycles 0 to 6; 7 is `----------`). Stock files use 0, 3 and 4 |
| 0x02 | 2 | `unknown_02` | Zero in every stock path. Nothing in the code read reads or writes it |
| 0x04 | 1 | `weight` | How often the alien base picks the path (below) |
| 0x05 | 3 | `mix` | Humanoids, scorpions and scouts the mission wants, in tenths; the editor shows them as percentages (table `DS:0x7d35`). Only the editor's display reads them |
| 0x08 | 1 | `on_path` | Alien robots on the path |
| 0x09 | 3 | `on_path_by_kind` | Of those, humanoids, scorpions and scouts |
| 0x0c | 128 | `points` | 16 points of x, y (16.16) |

Making a path, or changing its mission, sets +0x04..+0x07 from the mission's defaults
(four bytes a mission at `DS:0x7cf8`), and every stock path has its mission's. The
counts at +0x08..+0x0b are worked out during play, for the alien paths only
(`sim_count_units_on_paths`, `1fe6:8326`: cleared, then one for each robot in the
alien base's unit list that has a path); stock files keep what they were when saved.

Slots past the count, and points past `point_count`, often hold leftovers from the
editor. Human paths are empty in the training files and in `scen00a` and `scen04a`.

#### How paths are used (T-059)

`scen_reset_path_tables` (`1000:e9e0`) empties both tables and makes the slot pointer
lists (`DS:0x7c3c` alien, `0x7c4c` human). A robot keeps its path in two bytes
(`usg::Robot`): +0x2c holds the slot (bits 0 to 3) and flags (bit 5 walking back,
bit 6 the next point asked for, bit 7 on a path), +0x2d the point it heads for.

- **Picking a path** (`sim_assign_random_path`, `1000:ea98`). Only the alien base does
  this (`sim_assign_paths_to_idle_units`, `1000:ea3a`, runs for a base with the
  alien bit): for each robot in its unit list without a path (and without +0x23
  bit 0), it lists each path in use `weight + 1` times, draws the random word's low
  byte modulo the list's length, gives the robot that path from point 0, heads it
  there, and starts the path's mission program (`order_start_path_mission_program`,
  `1000:ec4d`): a program number by mission and robot kind from `DS:0x7d20` (three
  bytes a mission: humanoid, scorpion, scout; humanoids and scouts get 8 on every
  mission, scorpions 13, 12, 16 on the first three and 13 on the rest).
- **Heading for a point** (`sim_head_for_path_point`, `1000:eb05`). The robot's side
  picks the table. A path with no points fails. Point 0xff is the robot's own base.
  A point at or past `point_count` runs the end-of-path routine (by kind and mission,
  `DS:0x7c92`): every one turns the robot round (bit 5 flips, bit 6 set). Point 0xfe
  (past the base, coming back) runs another (`DS:0x7cb4`): humanoids and scouts pick
  a new path, scorpions turn round. Otherwise the point's x and y become the robot's
  destination (+0x3c, `sim_set_robot_destination`, `1000:46f3`) and bit 7 is set.
- **Next point** (`sim_advance_path_point`, `1000:eae4`): when the script has asked for
  the next point (bit 6), the point number steps forward, or back with bit 5, and the
  robot heads for it. So a robot walks from its base along the points, back to point
  0, to its base, and then (if not a scorpion) along a new path.
- **Joining a path** (`sim_join_nearest_path_point`, `1000:eb66`, reached through a
  table): the robot takes the nearest point of any of its side's paths, or its base
  if that is nearer; only points less than 256 units away count.

The script's path commands (which set bits 4 and 6 or step back a point, table
`DS:0x7d14`) are part of the orders (T-053).

T-052 read the rest (`docs/sim.md`, "Movement", "Paths"): only the alien base walks
robots along paths, every 16 ticks; the slot pointer lists are fixed (slot `i` is the
`i`th 140-byte slot, in use or not); the end-of-path and return routines, and a
picked path's program, follow the mission of the **alien** table's slot whatever the
robot's side; and a humanoid or scout that takes a new path at its base heads for
that path's point 2.

### `ORDR`: briefing

`1000:f315` reads the chunk into the text segment at `38fa:04ed`. It is ASCII with
CR LF line ends. Every stock file ends it with two DOS end-of-file bytes (0x1a) and a
NUL. `Scenario::briefing_lines` returns the lines. The file holds no other mission
objectives; win and lose conditions are in the code (T-055).

The game shows it in one place only: the situation room's main view in its orders
mode (key O, mode 0 in the base's +0x37), which types it one byte per pass from (96,
30) in font 4, CR LF taken as the font's control codes, until a NUL or 0x1a, with no
wrapping or paging (`docs/screens.md`, "The briefing (T-044)"). The base's +0x34 is the
typing position, and +0x37 the mode the mission starts in.

### `BGND` (64 bytes): sky picture

`1000:f3d7` reads a file name into `DS:0x7eda` and hands it to `scen_set_sky_name`
(`1000:2822`). The name is in padded 8.3 form (`SKY_B   .BIN`), then NULs; only the
base name (up to 8 characters) is used, with the extension `.BIN`. Every campaign and
training file names a `SKY_x.BIN` (the packed `sky_x.b@n` files) except `scen06a`, which
names `BACKLAND.BIN`. A stock install has no such file in any form, so that mission has
no sky picture: the 3D view's upper half is a flat colour instead (`b@n.md`, T-028).

### `TERM` (0 bytes)

Ends the file.

## The scenario editor

`xeno.exe`'s scenario editor works in a running mission's situation room once unlock
word 9 matches (`DS:0x5bc1` = 0xab65): its key table at `DS:0x5bc3` picks a kind (`H`,
`N`, `S`, `B`, `R`, `O`, and `P` for paths, all with Ctrl), the side (Ctrl+T), a
heading, and edits the paths; Ctrl and a left press on the map places the kind. Its
status goes to the debug screen. That is where the names above come from. Details in
`docs/screens.md`, "The scenario editor (T-048)".

## Writing (T-048)

The situation room's Ctrl+Alt+P (no unlock needed) saves the running mission through
`file_write_scenario` (`1000:efde`), which creates the file and writes, in this order
and always all of them:

1. `SHDR` (`scen_write_shdr`): 16 bytes from a template at `DS:0x7ea8`: version 0x80,
   the game clock as it is (`DS:0x7d46`, day and month included), 0xa0, and six zero
   bytes nothing writes.
2. `MAPP` (`scen_write_mapp`): the **whole terrain buffer**, whose size the memory
   block gives in paragraphs (`1fe6:1c7b`), so the chunk is a multiple of 16 bytes:
   the records, the 0 word and whatever lies after it in the block. With no buffer,
   an empty chunk. A buffer read from a `.usg` has the chunk's own size; one loaded
   from a `.map` (the terrain editor's Apply) is the file rounded up to a paragraph,
   with leftover memory after the end word.
3. `DCBS` (`scen_write_dcbs`): `scen_pack_all_objects` first runs each kind's pack
   routine (`DS:0xe81c`; a robot's script pointer to its saved form, its target to a
   slot), then the count and the objects **in slot order** (the object table
   `DS:0xe2c4`, empty slots skipped): each record's size is 0x172 or 0x34 by the
   kind's flag (`DS:0xe7c7` bit 0), its slot and reference count come from the table
   (`sim_find_object_slot`), and the object follows from its kind word. Then
   `scen_unpack_all_objects` turns everything back.
4. `APTH`, `HPTH` (`scen_write_apth`, `scen_write_hpth`): each table's 1122 bytes as
   they are, slots past the count and points past `point_count` included.
5. `ORDR` (`scen_write_ordr`): the briefing buffer up to and including its first NUL.
6. `BGND` (`scen_write_bgnd`): the 64-byte sky name buffer (`DS:0x4c90`): the
   default name the video driver gives for `BACKLAND.BIN`'s file id with the
   scenario's base name written over its first 8 characters, then NULs.
7. `TERM`.

The save game (`file_write_save_game`, `1000:f01b`) is the same with `SDEF` after
`SHDR`. `xeno_formats::usg::write` writes a `Scenario` this way (an `SDEF` among
`other_chunks` goes after `SHDR`, other unknown chunks are dropped, a missing chunk is
written as the game's empty state), and **every stock `.usg` comes back byte for
byte** (`real_assets_usg_round_trip`): stock objects are in slot order, every `MAPP`
is whole paragraphs and every `ORDR` ends at its NUL.

## Save files

Found in T-024. Progress is saved to `<NAME>.LOG`, where `NAME` is the login name, up
to eight characters, space-padded into the template at `DS:0x613b` (`1000:7912`). At
login `1000:70ec` loads it through the chunk reader, or `scendef.bin` if there is
none. `1000:f01b` saves the same chunks as the scenario writer (`1000:efde`, above)
with `SDEF` after `SHDR`. `state.rst` is not a save file (`rst.md`).

`SDEF` (written by `1000:7666`, read by `1000:76ab` only while loading a save, a flag
at `DS:0x7ea1`) holds 30 bytes of state from `DS:0x5f97` (+0x00 the selected
scenario, +0x04 the player's progress `DS:0x5f9b`, +0x12 the table's size in bytes,
the rest not traced), then the loaded `scendef.bin` buffer rounded up to whole
paragraphs, with each scenario's `state` (`scendef.md`).

## Open questions

- The object bytes still unnamed: +0x12 and +0x17, most of a robot's 0x172 bytes
  (sensors, weapons, the script's working bytes), the relay's set-up state and links,
  and anything in an obstacle past +0x18 (T-107).
- Whether `pool_index` matters once loaded, given that stock files repeat some.

## Parser

`xeno_formats::usg` (T-020): `parse` returns a `Scenario` with the header, terrain,
objects, both path tables, briefing and background, and keeps any other chunks.
`Object` names the common header and `Body` each kind's layout (T-058; robots, bases
and relays of the stock sizes). `TerrainFeature` names every field of a terrain record,
`Path` the path header, and `Robot` the path and destination fields (T-059).
`xeno_sim::orders::Orders::of` reads a robot's orders
(T-043).
`write` encodes a `Scenario` as the game's writer does (T-048, "Writing" above).
`xeno-tool inspect <file>.usg` prints the clock, the sky, the extent of the object
positions, the terrain features, one line per object (with its heading, radius, flags
and its kind's orders and damage), both sides' paths and the briefing's first line.
`xeno_sim::World::from_scenario` copies the heading, radius and damage into each
`Entity`, and the terrain and paths into `xeno_sim::terrain` (`docs/sim.md`). The
terrain lines show each record's kind, position, sizes and shape program, the path
lines each path's weight, mix and counts.
