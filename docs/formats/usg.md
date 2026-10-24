# `.usg` scenarios

## Status

**Layout confirmed** (T-020). `xeno_formats::usg` reads all 17 stock files to their last
byte. The chunk layout, and most of what each chunk is, comes from `xeno.exe`'s loader
and its chunk handlers. The object kinds and sides come from the game's built-in
scenario editor (below). Inside the objects, terrain records and path headers, most
bytes are still unknown (T-059, T-107; the objects' layout is in "Object fields").

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
the whole part. The stock files place objects between about 76 and 175 in x and 112
and 163 in y. The world's bounds have not been traced (T-059). There is **no map grid**
in the file.

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

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `size` | Of the record, this field included. 24 in every stock record |
| 0x02 | 1 | `kind` | 0x14 (a circle) or 0x12 |
| 0x03 | 1 | `unknown_03` | 0x10 with kind 0x14, 0x08 with kind 0x12 |
| 0x04 | size - 4 | `data` | Kept as is |

For **kind 0x14**, the ground-height code (`1000:436c`, `1000:43d9`) takes the
distance from a point to (+0x04, +0x08) (16.16) and compares it with +0x10 (8.8). Inside
that radius it works out a height from +0x0c (16.16) and +0x14. These records are
probably hills or craters. +0x12 and +0x16 are unknown, and +0x10, +0x12 and +0x14
have about a 3:2:1 ratio in every record.

**Kind 0x12**: every file that has terrain ends with one record of this kind, the same
in all of them. Its position words look like a sentinel, not a place. Not traced.

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
| 0x00 | 2 | `kind` | 0 humanoid, 1 scorpion, 2 scout, 3 base, 4 relay, 16 obstacle. These are the editor's numbers and names (it calls them `HUMAN`, `SCORP`, `SCOUT`, `BASE`, `RELAY`, `OBST.`, with -1 for `PATH`). Kinds 5 to 15 are made during play and not traced (shots and explosions, presumably); removing an object can turn it into kind 15 |
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
| 0x1a, 0x1b | 1, 1 | | A tick counter stepping by 2, and a state number; each picks a routine from a table. Not traced |
| 0x1e, 0x20 | 2, 2 | `velocity_x`, `velocity_y` | Added, sign-extended, to `x` and `y` every tick (1/65,536ths) |
| 0x23 | 1 | `mode` | Bit 7: manual, the pilot drives (T-043, `docs/screens.md`, "Orders"); bit 3: recalled (the scorpion's next tick gives it program 4 and clears the bit); bits 4 and 5 turn the target heading by 0x40 one way or the other every tick (the pilot's turn keys) |
| 0x24 | 1 | `helm` | 0 to 100; a hit can take 0 to 7 off |
| 0x26 | 2 | `speed` | Every 16 ticks it steps by one towards `target_speed` (limited by a value at +0x118). Units not traced |
| 0x28 | 2 | `target_speed` | |
| 0x2a | 2 | `target_heading` | Every tick `heading` turns towards it by at most 0x5b (the scorpion's step; the others not read). The editor writes the same value here and at +0x10 |
| 0x2e | 1 | `program_given` | A number in `xeno.exe`'s program table (T-043) |
| 0x30 | 1 | `program_running` | That program, or a state the script entered by itself. Replaced by +0x2e at load (above) |
| 0x31, 0x33, 0x38 | | | The script's timing (T-043) |
| 0x34 | 2 | `script` | The script pointer, saved as `0xa000` + its offset from the program table |
| 0x36 | | | Event bits, which the message panel reports |
| 0x4e | 1 | `power` | Power systems, 0 to 100; a hit can take 0 to 15 off |
| 0xdc | 1 | `weapons` | Weapons damaged: bits 0, 1, 2 for the kind's first, second and third weapon (the third is the scorpion's tail missile) |
| 0xe6 | 2 | `target` | The robot's target: an object pointer in memory, its slot in a file (0xffff: none). The loader clears it |
| 0xec | 1 | `targeting` | Bit 0: the targeting computer is damaged |
| 0xf6 | 1 | `systems` | Bits 0 visual camera, 1 thermal camera, 2 map, 3 radar, 4 EMI sensors damaged; 6 and 7 the first and second weapon destroyed |
| 0x10c, 0x127, 0x10d | 0x35, 0x45, 0x3c | | The humanoid, scorpion and scout console's widget state (T-043) |

**How robots take damage** (`sim_damage_object`, `1000:f84d`, calls the kind's routine
in the table at `DS:0xe783` with the damage in `AL`): a robot hit for more than 80 is
destroyed at once (`sim_destroy_robot`, `1000:de51`). Otherwise each point of damage
picks one of the kind's components by chance from a table (`DS:0x6856` humanoid,
`0x659a` scorpion, `0x6c06` scout; three-byte entries of a chance out of 256 and a
routine) and damages it: the helm or the power percentage drops, or a bit above is
set (a second hit on a damaged weapon destroys it). The message panel reports each.
Robots have no hit points: a robot hit for 80 or less survives.

**Bases** (0x172 bytes, `usg::Base`):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x2b | 1 | `losses` | The side's robots lost this mission (T-045) |
| 0x2c | 1 | `kills` | Enemy robots destroyed this mission |
| 0x2e | 2 | `damage` | Damage taken (`sim_damage_base`, `1000:62b7`). At 300 the base is destroyed (`GONE`) and the other side is told; below that its own side is told, at most once every 480 counts of `DS:0x374`. Playing a campaign scenario adds 0, 50, 100 or 150 to the player's base by the record's alert level (`sim_apply_scenario_alert`, table `DS:0x5a3f`) |
| 0x30 | 2 | `end_step` | Once the base is destroyed, the mission's end steps it; at 0x24 the mission ends, won if the alien base was destroyed (`sim_end_mission_with_result`, T-046) |
| 0x34, 0x37 | 2, 1 | `typed`, `view_mode` | The orders' typing position and the main view's mode (0, 2, 4, 6, 8: O, S, M, R, C; T-044) |
| 0x4d | 2 | | The side's unit list, set at load |
| 0xbd | 0x43 | `view_state` | The situation room's widget state (T-043) |

**Relays** (0x34 bytes, `usg::Relay`): +0x33 `damage`, at 20 the relay is removed
(`sim_damage_relay`, `1000:cfba`). +0x1f and +0x20 are its set-up state, and a new
relay sets the words at +0x25, +0x29, +0x2d and +0x31 to 0xffff; not traced.
**Obstacles** (0x34 bytes) have no damage routine, so shots do not harm them; nothing
past +0x18 is known.

Every stock file has exactly one base per side, which supports the side bit. Relays
sit on whole-number positions in chains near their own side's base.

### `APTH` and `HPTH` (1122 bytes each): paths

`APTH` (alien) is read into `DS:0x7372` and `HPTH` (human) into `DS:0x77da`, as one
block each. Older-version files skip both. The block is a u16 count of paths in use,
then 8 path slots of 140 bytes:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 1 | `point_count` | In every stock path whose later points are empty, this is the number of points before them. Not traced in code |
| 0x01 | 1 | `mission` | The debug display names it from `DS:0x5cf5`: `NET DENSE`, `NET SPARSE`, `MAINTAIN`, `RECON`, `PATROL`, `SKIRMISH`, `WATCH`, `----------`. Stock files use 0, 3 and 4 |
| 0x02 | 10 | `unknown_02` | The debug display prints +0x04 as a number, +0x05..+0x07 as tenths (0 to 100 %) and +0x08..+0x0b as numbers. +0x04..+0x07 go with `mission` in stock files |
| 0x0c | 128 | `points` | 16 points of x, y (16.16) |

Slots past the count, and points past `point_count`, often hold leftovers from the
editor. Human paths are empty in the training files and in `scen00a` and `scen04a`.

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

`xeno.exe` has a scenario editor behind a debug word (`DS:0x5bc1` must hold a magic
value). Its key table at `DS:0x5bc3` maps keys to handlers: one key per object kind
(`H`, `N`, `S`, `B`, `R`, `O`, and `P` for paths), `T` to toggle the side, and more.
Its status line (`1000:6940`) shows the kind, the side (`HUMAN`/`ALIEN`) and the
current path's fields. That is where the names above come from. `DS:0xe6a7` holds its
object labels (`SIZE`, `LOC-X`, `LOC-Y`, `LOC-Z`, `#DCBS`, ...).

## Save files

Found in T-024. Progress is saved to `<NAME>.LOG`, where `NAME` is the login name, up
to eight characters, space-padded into the template at `DS:0x613b` (`1000:7912`). At
login `1000:70ec` loads it through the chunk reader, or `scendef.bin` if there is
none. `1000:f01b` saves the same chunks as the scenario writer (`1000:efde`) with
`SDEF` after `SHDR`. `state.rst` is not a save file (`rst.md`).

`SDEF` (written by `1000:7666`, read by `1000:76ab` only while loading a save, a flag
at `DS:0x7ea1`) holds 30 bytes of state from `DS:0x5f97` (+0x00 the selected
scenario, +0x04 the player's progress `DS:0x5f9b`, +0x12 the table's size in bytes,
the rest not traced), then the loaded `scendef.bin` buffer rounded up to whole
paragraphs, with each scenario's `state` (`scendef.md`).

## Open questions

- The object bytes still unnamed: +0x12 and +0x17, most of a robot's 0x172 bytes
  (sensors, weapons, the script's working bytes), the relay's set-up state and links,
  and anything in an obstacle past +0x18 (T-107).
- The world's size and edges, and the terrain record fields not listed above. Also how
  `point_count` and the rest of the path header are used (T-059).
- Whether `pool_index` matters once loaded, given that stock files repeat some.

## Parser

`xeno_formats::usg` (T-020): `parse` returns a `Scenario` with the header, terrain,
objects, both path tables, briefing and background, and keeps any other chunks.
`Object` names the common header and `Body` each kind's layout (T-058; robots, bases
and relays of the stock sizes). `xeno_sim::orders::Orders::of` reads a robot's orders
(T-043).
`xeno-tool inspect <file>.usg` prints the clock, the sky, the extent of the object
positions, the terrain features, one line per object, both sides' paths and the
briefing's first line.
