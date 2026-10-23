# `.usg` scenarios

## Status

**Layout confirmed** (T-020). `xeno_formats::usg` reads all 17 stock files to their last
byte. The chunk layout, and most of what each chunk is, comes from `xeno.exe`'s loader
and its chunk handlers. The object kinds and sides come from the game's built-in
scenario editor (below). Inside the objects, terrain records and path headers, most
bytes are still unknown (T-058, T-059).

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

Object fields known so far (offsets in the object, as in memory):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `kind` | 0 humanoid, 1 scorpion, 2 scout, 3 base, 4 relay, 16 obstacle. These are the editor's numbers and names (it calls them `HUMAN`, `SCORP`, `SCOUT`, `BASE`, `RELAY`, `OBST.`, with -1 for `PATH`) |
| 0x02 | 2 | `pool_index` | The object's place in its pool when it was saved. Some stock files repeat a number |
| 0x04 | 4 | `x` | Position, 16.16 |
| 0x08 | 4 | `y` | |
| 0x0c | 4 | | Zero in every stock object. Possibly the third coordinate (the debug labels include `LOC-Z`) |
| 0x16 | 1 | flags | Bit 3 is the side: the editor writes 0 for human and 8 for alien. The game also uses bit 0 (removed), 0x20 and others |
| 0x23 | 1 | mode (robots) | Bit 7: manual, the pilot drives (T-043, `docs/screens.md`, "Orders"); bit 3: recalled |
| 0x2e | 1 | program given (robots) | A number in `xeno.exe`'s program table (T-043) |
| 0x30 | 1 | program running (robots) | That program, or a state the script entered by itself |
| 0x31, 0x33, 0x34, 0x38 | | the script's state (robots) | +0x34 is the script pointer, saved as `0xa000` + its offset from the program table |
| 0xbd | 0x43 | view state (bases) | The situation room's widget state (T-043) |
| 0x10c, 0x127, 0x10d | 0x35, 0x45, 0x3c | view state (robots) | The humanoid, scorpion and scout console's widget state (T-043) |
| rest | | `data` | Unknown (T-058). The editor writes one value to both +0x10 and +0x2a for robots (a heading?) |

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

- The object bytes after +0x0c: heading, damage, and the rest of the 0x172-byte robot
  layout (T-058). The orders and the views' state blocks are placed (T-043).
- The world's size and edges, and the terrain record fields not listed above. Also how
  `point_count` and the rest of the path header are used (T-059).
- Whether `pool_index` matters once loaded, given that stock files repeat some.

## Parser

`xeno_formats::usg` (T-020): `parse` returns a `Scenario` with the header, terrain,
objects, both path tables, briefing and background, and keeps any other chunks.
`xeno_formats::orders::Orders::of` reads a robot's orders (T-043).
`xeno-tool inspect <file>.usg` prints the clock, the sky, the extent of the object
positions, the terrain features, one line per object, both sides' paths and the
briefing's first line.
