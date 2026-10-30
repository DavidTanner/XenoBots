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
the save-game loader for the campaign (`1000:76ab`, "Save files" below), so save files
use the same chunk format. `xeno_formats::usg` rejects a repeated known
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
| 0x02 | 1 | `start.hour` | Copied to the game clock at `DS:0x7d46`. `1000:ed05` advances it 4 sixtieths a retrace (`docs/sim.md`, "The mission clock"), then seconds, minutes, hours, days (from a days-per-month table) and months |
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
  hill, z = 0. Its one caller (`sim_check_robot_against_hills`, `1000:d50f`) runs it
  only when `sim_test_point_in_hill`, the same test, failed, so a robot's z is set to 0
  on every move that does not end in a hill (T-119; `docs/sim.md`, "Heights").
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
lies on the ground.

**The shape ops (T-101, T-122).** `gfx_draw_terrain_shape` reserves the record's
`point_slots` four-byte slots below its frame; an op's point operands are signed byte
offsets of slots (multiples of 4, -0x04 down to -0x40), each holding a
projected screen point (y, then x). It keeps the sine and cosine of the node's heading
less the bearing (`DS:0x2976`, `0x297a`, from `1000:05c0` and `05bb`; a terrain node's
heading is 0 in play, below) and reads the three sizes, the third as the
**current height** (8.8). Then, by op:

- **0x26** (`1000:1cb0`, the hill's first op): the current height is negated, and
  eight points are made on the ring at it: in slots -0x1c to -0x38, offsets (0, r),
  (-d, d), (-r, 0), (-d, -d), (0, -r), (d, -d), (r, 0), (d, d) from the centre
  (r the radius, d the diagonal size, both turned to 16.16; anticlockwise from +y).
  The current height is then set to 0 for good and the centre itself goes in slot -0x18.
- **Projection** of an offset (`1000:20f4`): the centre plus the offset, relative to
  the camera and turned as a model's anchor is (`docs/render.md`, "Projection"),
  giving the lateral `X` and depth `Z`, which are kept as the **base** (`DS:0x1c80`,
  `0x1c84`; before any op, the centre's, which the cone test has just left there).
  Then `1000:2147`, at the current height (shifted to 16.16) above the centre's z: in
  front (`Z` not negative) divided by the **larger of |X| and |Z|** (not the
  approximate distance): `x = W/2 + int(float(X) * F / m)`, `y = H/2 - int(float(z) * F
  / m)`, each 0x7fff if the addition overflows. Behind, `m = |X|` and `x = W/2 ±
  int(F)` by the sign of `X` (the focal factor's width to that side), `y` as before,
  with no overflow check. It returns with `CX` the point's x and `BX` the divisor's
  mantissa halved (the division's `shr bx, 1`; as it is if 0 or 1).
- **0x02** (`1000:1b86`): with the current height negated, five points: at twice the
  diagonal size on the axes, (0, 2d), (-2d, 0), (0, -2d), (2d, 0), in slots -0x04 to
  -0x10, and the centre in -0x14; then, the height restored, the centre in -0x18. The
  offsets are on the world's axes: nothing turns them.
- **0x04**, **0x06**, **0x08** (`1000:1d8d`, `1dd7`, `1e1d`): pairs of points through
  `1000:205a`: an offset `(x, y)` across and in depth is turned by the angle above
  (`1000:2233`, each term a size times the sine or cosine magnitude, 8.24, signed by
  the trig sign and the call's flag byte, summed in 32 bits and shifted right 8 to a
  16.16: `x cos - y sin` across, `x sin + y cos` in depth), and the base plus it goes
  in the first slot at the current height, the base less it in the second at the
  negated height (`1000:2147` directly: camera space, not the world's). With r the
  radius and d the diagonal size, the first points, anticlockwise:
  - 0x04, a box: (d, r), (-d, r), (-d, -r), (d, -r) in -0x04 to -0x10, the opposites in
    -0x20, -0x1c, -0x18, -0x14;
  - 0x06, a diamond: (0, r), (-d, 0), (0, -r), (d, 0), the same slots;
  - 0x08, an octagon: (r, 0), (d, d), (0, r), (-d, d) in -0x04 to -0x10 with the
    opposites in -0x30 to -0x24, then (-r, 0), (-d, -d), (0, -r), (d, -d) in -0x14 to
    -0x20 with the opposites in -0x40 to -0x34 (16 slots).
- **0x22** (`1000:1c20`): `h` the current height taken unsigned (8.8 to 16.16); with
  the height negated, the base plus (-h, h) across and in depth in -0x20, then a point
  meant to be the base plus (-h, -h) in -0x28, but `BX` and `CX` are never set for its
  depth, which is what the last projection left (`CX:BX`, so about the previous point's
  x in whole units) less `h`; then (h, -h) in -0x30, (h, h) in -0x38 and the base in
  -0x14; with the height restored, the base in -0x18. Not turned.
- **0x24** a b c (`1000:2008`): slot a = b extended by half the way from c, each
  coordinate `(3b - c) >> 1` (32 bits, kept to 16).
- **0x1c** a b c (`1000:1fc1`): slot a = the midpoint of b and c, each coordinate
  `(b + c) >> 1` without overflow.
- **0x1e** a b c (`1000:1f75`): slot a = b plus a quarter of the way to c, `b + ((c -
  b) >> 2)`.
- **0x0e** a b c colour and **0x10** a b c d colour (`1000:1ee6`, `1f2e`): the triangle or
  quad of those slots, in that order, filled in the colour by `1fe6:4d7d` (below).
- **0x12**, **0x14**, **0x16**: run the built-in block or hill program (`3b53:0` and
  `3b53:0x16c`), then go on.
- **0x0a**: skips 8 bytes. **0x00**, **0x0c**, **0x18**, **0x1a**: end the whole shape.
  **0x20** sets a flag that moves a triangle's first corner and breaks into the
  debugger (`int 3`): a developer's aid. An odd op indexes the handler table
  (`DS:0x2988`) between two entries.

No stock record uses the point ops but the hill's 0x26; the block program uses 0x04,
and its one record in each file is where nothing sees it. A slot below the record's
`point_slots` is below the stack pointer in the original, where later calls'
return addresses can overwrite it; the port keeps it.

The node's heading (+0x08, `gfx_emit_terrain_render_node`) is `[DS:0x1486] << 4`. Only
the event reader's developer timing keys write `DS:0x1486` (unlock word 2's Ctrl+Alt+F11
and F12, `key.md`: the retraces a run of polygon fills took, shown on the debug
screen); it is 0 from load, so in play the angle is minus the bearing.

The hill program is op 0x26; midpoints of ring points 0, 2, 4 and 6 with the centre;
then for each quarter of the ring, two triangles (ring point, the next, the nearer
midpoint) and a quad (midpoint, the ring point between, the next midpoint, the centre),
in colours 57 to 61. The block program makes its points with op 0x04 and fills six
quads (the box's faces) in colours 7 to 14. The built-in table holds more programs
after the block's that no op runs.

**The polygon fill** (`gfx_fill_polygon`, `1fe6:4d7d`): the corners' bounding box is
tested against the buffer's clip rectangle, then the polygon is drawn only if it faces
the viewer: from the top corner `t` (smallest y; on a tie the smallest x, the later
corner on a full tie), with `n` the next corner and `p` the one before, only when `(n.x
- t.x) * (p.y - t.y) < (n.y - t.y) * (p.x - t.x)` (corners anticlockwise on screen).
Edges are walked into a table of a left and a right x per row
(`gfx_walk_polygon_edge`, `1fe6:60ea`, clipped), and each row in the clip rectangle is
filled from the left x up to, not including, the right x. The pixel rules are in
`docs/render.md`, "The polygon fill (T-121)".

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
robot's target (+0xe6) and the reference count it held on it. The program bit (+0x23 bit 7) is kept.
The other kinds' routines do nothing.

Files with another version take a different path (`scen_on_old_layout_object_loaded`,
`1000:7c84`): only 0x16 bytes are read and the rest of the object is set up as the
editor does for a new one (`sim_init_new_object`, `1000:f812`, and the kind's routine
in `DS:0xe73f`).

#### Object fields

Offsets in the object, as in memory. `xeno_formats::usg::Object` names the first 0x18
bytes, and `Body` the rest by kind (`Robot`, with the kind's `Parts` from +0xfe,
`Base`, `Relay`; anything else stays bytes). Unnamed bytes are kept as `unknown_XX` fields.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `kind` | 0 humanoid, 1 scorpion, 2 scout, 3 base, 4 relay, 16 obstacle. These are the editor's numbers and names (it calls them `HUMAN`, `SCORP`, `SCOUT`, `BASE`, `RELAY`, `OBST.`, with -1 for `PATH`). Kinds 5 to 15 are made during play (below, "Kinds made during play"): 5 the scorpion's tail missile, 6 to 8 shots, 9 to 12 mines and buoys, 13 an explosion, 14 the cloak's sparkle. Removing an object can turn it into kind 15 |
| 0x02 | 2 | `pool_index` | The object's place in its pool when it was saved. Some stock files repeat a number |
| 0x04 | 4 | `x` | Position, 16.16 |
| 0x08 | 4 | `y` | |
| 0x0c | 4 | `z` | Height, 16.16. Zero in every stock object, and a robot's is set to 0 on every move (`docs/sim.md`, "Heights"). The line-of-sight code lifts it for a moment by a per-kind height (table `DS:0xe7a5`, 8.8: 10, 10, 6, 20, 6 for kinds 0 to 4, 6 for obstacles) |
| 0x10 | 2 | `heading` | 65,536 to a turn. The editor's arrow keys step it by 0xb6 (a degree). The 3D view's render node copies it (`mxx.md`) |
| 0x12 | 2 | `model_height` | The model's height above z in the 3D view, added to z's low word (`docs/render.md`): set per kind when an object is made (0x0a00 humanoid, 0x0a80 scorpion, 0x0c80 scout, 0 shots and explosions; a relay's from a table by its stage); humanoids set it every tick and scouts while they walk or crouch (`docs/render.md`, "Walking"). Stock humanoids hold 0x0a40 to 0x0b00, scouts 0x0c80 to 0x0d00 |
| 0x14 | 2 | `radius` | Set per kind when an object is made: base 35, scorpion 18, humanoid and scout 10, obstacle 13, relay 5 (as 8.8 numbers; the 3D view takes them as 1/65,536ths of a unit, like a z's low word, so a robot's radius is 10/256 of a unit there, `docs/render.md`). Contact checks take both radii off the distance; the render node keeps the whole part |
| 0x16 | 1 | `flags` | Bits below |
| 0x17 | 1 | `status` | State bits below (T-107) |

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

The status bits at +0x17 (`usg::status`, T-107):

| Bit | Name | Meaning |
|---|---|---|
| 0 (0x01) | `CLOAKED` | A robot's sparkle cloak (F8, `1000:d95e`) is on while +0xe1 counts down: the scanners, the map (`1000:f989`), the A key and target locks (`1000:d8cc`) skip it. A removed object (kind 15) instead counts its last ticks here: removal sets the byte to 2 and the kind's tick (`1000:f5c1`) frees the slot at 0 |
| 1 (0x02) | `ECM` | A robot's ECM (F7, `1000:d92e`) is on while +0xdf counts down: a lock on it is lost (`1000:d6a6`) |
| 2 (0x04) | `SPOTTED` | The other side has seen it: set on enemy units within a robot's scanning range at each scan (`1000:dcba`) and on an object a robot locks (`1000:d8e7`). The map shows an object of the other side only with it (`1000:f983`, unless the developer flag `DS:0x6ffa`). Nothing found clears it |
| 3, 4, 5, 7 (0xb8) | `SCAN_COUNT` | A robot's count to its next scan, a 4-bit counter with bit 7 lowest, then 5, 4, 3, stepped when the kind's tick table reaches `1000:dcba` (and +0x1a's top three bits are 0); when it comes round to 0 the robot scans: every unit of the other side within the kind's range (`DS:0x707f`) gets `SPOTTED`, and their number goes to +0xf7 |
| 6 (0x40) | `TRACKING` | The robot's camera follows its target: set by a lock (`1000:d8b6`), cleared by the console's pan keys (X, Y, Z) and `1000:da32`; while set the humanoid's tick points the pan (+0xf4) at the target (`1000:b2d0`) |

Bases and relays show only bit 2 in stock files.

**Robots** (0x172 bytes, `usg::Robot`). Every routine named below takes the robot in
`DI`. The kind's set-up routine (`DS:0xe73f`: humanoid `1000:ab8a`, scorpion `9293`,
scout `c40e`) runs after the common one (`sim_init_new_object`, `1000:f812`), and
gives the values called "when made". Up to +0xfd the kinds share a layout:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x18 | 2 | `serial` | A random number 0 to 511 drawn when made. The robot's name is the kind's two letters, `-`, and its last three digits (`1fe6:7f38`); nothing else reads it |
| 0x1a, 0x1b | 1, 1 | `tick`, `state` | A tick counter stepping by 2 (kept to 0..=0x1e for scorpions and scouts), and a state number; each picks a routine from the kind's table every tick (the tables at `DS:0x6ac0` humanoid, `0x6720` scorpion, `0x6eb0` scout hold the scan, the lock warning and the contact check below). A scorpion steps its speed when the counter's low four bits are 0 (T-052, `docs/sim.md`, "Movement"). The state's routines are not traced |
| 0x1c | 2 | `eye_height` | The camera's height above z (`docs/render.md`, "The camera"): when made 0x0f80 humanoid (set again every tick), 0x0b80 scorpion, 0x0a00 scout (set from its stance each time the stance steps). Every stock scout holds 0x0b80, its standing height |
| 0x1e, 0x20 | 2, 2 | `velocity_x`, `velocity_y` | Added, sign-extended, to `x` and `y` every tick (1/65,536ths). Worked out from the heading and speed only when either changes: `speed * sin(heading)`, `speed * cos(heading)` |
| 0x22 | 1 | `program_panel` | The console's program panel (`1000:d2d1`, `d34c`): bit 7 while a program is being picked, bit 0 flips which half of the kind's programs it shows; cleared once one is picked (`DS:0x7183` by kind) |
| 0x23 | 1 | `mode` | Bit 7 (`Robot::PROGRAM`): the **program** drives (T-052, T-111: the script steps every tick only with it; recall, a path's program and F3 set it, the drive keys, F2 and choosing a program on the console clear it; T-043 named it manual); bit 3: recalled (every robot's next tick gives it program 4 and clears the bit); bits 4 and 5 take 0x40 off or add 0x40 to the target heading every tick (the pilot's turn keys); bit 1 (`BUMPED`): it ran into something (below); bit 0: the alien base gives it no path |
| 0x24 | 1 | `helm` | 0 to 100; a hit can take 0 to 7 off |
| 0x25 | 1 | | Cleared when made (`1000:d2b6`); nothing reads it |
| 0x26 | 2 | `speed` | Steps by one towards `target_speed`, limited by the drive, the helm and (scorpions) a speed table: every tick, or every 8 ticks for a scorpion. In 1/65,536ths of a unit a tick |
| 0x28 | 2 | `target_speed` | |
| 0x2a | 2 | `target_heading` | Every tick `heading` turns towards it by at most the kind's step (read from `xeno.exe`; the same for the three kinds). The editor writes the same value here and at +0x10 |
| 0x2c | 1 | `path` | The robot's path: slot and flags ("How paths are used") |
| 0x2d | 1 | `path_point` | The point it heads for; 0xff its base |
| 0x2e | 1 | `program_given` | A number in `xeno.exe`'s program table (T-043) |
| 0x2f | 1 | | Nothing reads or writes it |
| 0x30 | 1 | `program_running` | That program, or a state the script entered by itself. Replaced by +0x2e at load (above) |
| 0x31 | 1 | `stack_pointer` | The script's stack pointer: 10 empty, stepping down by 1 or 2 a push (T-053, `docs/sim.md`, "Orders") |
| 0x32 | 1 | `goal` | What the robot heads for: bit 1 the point at `destination` (0x22 from a path, 2 from the hold order), bit 0 an object (`goal_object`, a reference held), bit 2 its own base, bit 3 its target (+0xe6), bit 4 a point the script worked out (`DS:0x4da4`); 0 nothing (T-053). The distance and the steering read bits 2 and 0 differently (`docs/sim.md`, "Sensing", T-112) |
| 0x33 | 1 | `script_flags` | Bits 0, 6 and 7 the carry, zero and sign of the script's last test; bit 5 runs the script every tick and holds events off (an event's handler sets it) |
| 0x34 | 2 | `script` | The script pointer (a data-segment offset in `DS:0x5034..0x5670`), saved as `0xa000` + its offset from the program table; flag bits in the top nibble restart the program given, or program 9, at load (`order_unpack_script_pointer`) |
| 0x36 | 2 | `events` | Events waiting for the script, a bit each (event n is bit n): 1 a scan found enemies where there were none, 2 none left, 3 a lock on it (below), 4 its lock lost, 5 a hold point or object goal set, 7 hit, 9 something close ahead (the contact check), 0xa and 0xb the program or the pilot took over, 0xc crippled; nothing posts 0, 6 or 8 (T-053, T-112) |
| 0x38 | 2 | `wait` | The ticks the script waits before its next opcode; a program starts with a staggered wait (T-053) |
| 0x3a | 2 | `goal_object` | The object of `goal` bit 0: its slot, in memory as in a file (0xffff none); `destination` then holds the robot's offset from it (T-112) |
| 0x3c | 8 | `destination` | Where it is heading, x and y (16.16): a path point or its base |
| 0x44 | 10 | `stack` | The script's stack (T-053) |
| 0x4e | 1 | `power` | Power systems, 0 to 100; a hit can take 0 to 15 off |
| 0x4f | 1 | `signal` | The net's signal (`2000:8087`, the kind's tick-table entry 3, every 16 ticks): 0xff within 0x578 (in 1/256ths of a unit) of any relay or base of its side with flag bit 4, else falling by 4 a 1/256th to 0 at 0x5b8 from the **last** such in slot order under 0x5b8, not the nearest (`2000:7d16`, through `1000:cfd8`); no base ever has the bit, so only relays carry it. Over 0x40 the robot runs off the net and `energy` gains one each run; at or under it, it is on its battery (`docs/sim.md`, "The relay's tick (T-148)") |
| 0x50 | 1 | `energy` | The battery, 0 to 0xff: recharged off the net (above; the humanoid's tick also adds one now and then, `1000:b306`), drained on the battery by firing (10 or 22) and the devices (ECM 1, cloak 2, `1000:d1b0`); "insufficient power" when short |
| 0x51 | 1 | `drive` | The power the drive gets (`2000:8087`): `signal`, or `energy` on the battery, ANDed with `DS:0x70a5[power >> 4]`. At 0 the robot cannot move; at or under a level read from `xeno.exe` it goes at half speed |
| 0x52..0xda | 0x89 | `panel`, `panel_length`, `panel_queue`, `panel_text` | The message panel (`docs/screens.md`, "The message panel (T-104)"): +0x52 its line count (bits 0 to 3) and bits 6 (a line ended) and 7 (redraw from the top); +0x53 the text's length; +0x54 five words, the messages waiting as string-segment offsets; +0x5e the text, NUL-terminated. Stock robots have empty texts and nothing waiting |
| 0xdb | 1 | `call_sign` | An index into the 64 call signs (T-103) |
| 0xdc | 1 | `weapons` | Weapons damaged: bits 0, 1, 2 for the kind's first, second and third weapon (the third is the scorpion's tail missile). Bit 7 a fire request (script opcode 0x7d, the Space key; the scorpion's burst repeats it); bits 5 and 6 a humanoid's first and second weapon about to fire (T-054); bit 3 set when a scorpion deploys a relay |
| 0xdd | 1 | `weapon` | The weapon selected, 0 or 1 (script opcodes 0x7b, 0x7c; F5, F6; T-054); a scorpion's 1 is its tail missile |
| 0xde, 0xdf | 1, 1 | `ecm`, `ecm_time` | ECM charges (0xff when made) and the ticks it stays on: F7 (`1000:d92e`) takes one and the energy, sets 20 and `ECM`; each tick counts it down and clears `ECM` at 0 (humanoid `1000:b08a`, scorpion `972e`, scout `c8ab`) |
| 0xe0, 0xe1 | 1, 1 | `cloaks`, `cloak_time` | Sparkle cloaks (humanoid 10, scout 4 when made; scorpions none) and the ticks one lasts: F8 (`1000:d95e`) takes one and the energy, sets 120 and `CLOAKED`, and makes the sparkle (kind 14); counted down as `ecm_time` |
| 0xe2..0xe5 | 1 each | `mines`, `alarm_buoys`, `ecm_buoys`, `decoys` | What keys 1 to 4 drop (`1000:d5e7`..`d64d`): proximity mines (kind 9), alarm buoys (11), ECM buoys (12) and EMI decoys (10) left. Scorpion 20, 8, 8, 4; scout 6, 2, 2, 6; humanoid none |
| 0xe6 | 2 | `target` | The robot's target: an object pointer in memory (0 none; the scripts test it), its slot in a file (0xffff: none). The loader clears it. Set by a lock (`1000:d8af`), dropped by the upkeep (T-112) |
| 0xe8 | 2 | `target_distance` | The target's true distance (`1000:0942`, a square root) as 8.8 (the middle word of the 16.16), worked out every 16 ticks while the lock holds (`1000:d6e6`, from the kind's tick table through `1000:d66f` with the kind's arc and range) and at the lock |
| 0xea | 2 | `target_bearing` | The bearing to the target (`1000:0545`, the fine bearing) |
| 0xec | 1 | `targeting` | Bit 0: the targeting computer is damaged (no lock holds). Bit 1: the target is closer than the range given (`DS:0x7203`); bit 2: within the arc (`DS:0x7201`) of the heading, or no target (stock robots have bit 2). The weapons fire only with bit 2 |
| 0xed | 1 | `locked_on` | Another robot holds a lock on this one: its tick sets 0xff (`1000:d6ad`) unless this robot's ECM is on (then the lock is lost); this robot's tick halves it (`1000:dc95`), setting `systems` bit 5 and posting event 3 when it starts, clearing bit 5 at 0. The humanoid's console sounds an alarm while it is not 0 |
| 0xee | 1 | `blink` | Steps by 2 (`1000:b318`); times the console's blinking and the lock alarm |
| 0xef..0xf5 | 7 | `display`, `display_before`, `zoom`, `map_zoom`, `zoom_label`, `pan` | The console's second display (`docs/screens.md`, "The second display (T-104)"): +0xef the display mode (0 the camera, 1 thermal, 2 the map, 3 the scorpion's missile camera; bit 2 set every tick while that mode's device is damaged), +0xf0 the mode a scorpion's display goes back to once its missile is gone (saved when it fires one), +0xf1 and +0xf2 the camera's and the map's zoom steps (0 to 4), +0xf3 the ticks left (in 16s) to show the zoom's name, +0xf4 the camera's pan (a word added to the heading) |
| 0xf6 | 1 | `systems` | Bits 0 visual camera, 1 thermal camera, 2 map, 3 radar, 4 EMI sensors damaged; 6 and 7 the first and second weapon destroyed; bit 5 while `locked_on` |
| 0xf7 | 1 | `contacts` | Enemy units found by the last scan (`1000:dd58`; see `SCAN_COUNT`); cleared when launched from the base (`1000:6745`). The consoles show it |
| 0xf8 | 1 | `flash` | Ticks left of the screen's flash after a hit (20; `1000:d16b` counts it down) |
| 0xf9 | 1 | `damage` | The damage added up, 0 to 100, at the end of every tick (humanoid `1000:afee`, scorpion `94e9`, scout `c6fd`): a quarter of the power lost and of the helm lost, plus weights for each damaged part. At 50 (humanoid; 40 the others) `CRIPPLED` is set and event 12 posted; at 70 / 35 (humanoid, human / alien), 75 / 40 (scorpion), 55 / 30 (scout) the robot is destroyed (`sim_destroy_robot`; `docs/sim.md`, "The damage total (T-140)", ported as `Components::total`). Bits 4 to 6 pick the roster's status icon, and the situation room's detail panel its repair state |
| 0xfa | 1 | `clock` | Steps by one (`1000:b35f`, scout `cb37`); a humanoid's or scout's console light (+0x10a, +0x108) steps each time its low five bits are 0. S sets it to 0 (T-139). A scorpion keeps only bit 7, flipped every 16 ticks by its tick-table entry 4 (`1000:9a45`): its console's damage panel shows its lights while it is set (T-131) |
| 0xfb | 1 | `bump_wait` | The contact check (`1000:d464`, from the tick tables): while it counts down nothing is checked; otherwise every other object in the field (not its own base) whose distance less both radii is under 0x6400 in the distance routine's units (`1000:08ec`) is a contact: one ahead (`1000:45f2`) posts event 9, and one closer than both radii makes it bump: `BUMPED`, the speed reversed, the target speed 0, the kind's bump routine (`DS:0x7209`), 1 point of damage, and 4 here. A robot in its base clears `BUMPED` and is not checked |
| 0xfc | 1 | | The repair on the net's count (`2000:7f10`, from tick-table entry 14, `1000:df79`): up one each run while the signal is 0x80 or over and `damage` is not 0; at 0xf0 back to 0 and one damaged part mended (`docs/sim.md`, "Checked against the trace (T-177)"; not ported, T-179) |
| 0xfd | 1 | `equipment` | Bit 0 `ON_BATTERY` (the signal is 0x40 or under; firing and the devices then take `energy`); bits 1 and 2 entering and in the base (set together when the base takes the robot in, `1000:6445`; bit 2 cleared by the launch, `6716`, which the player calls with the situation room's F2 and launch button only while bit 2 is set, T-135; bit 1 once the robot is out of the base's reach, `6439`; T-115, `docs/sim.md`, "The bases' phases"); bit 4 `DUMPING` (a scorpion's cargo dump, D: a relay every 10 steps of +0x11c); bit 5 the no-energy message was shown, and a scorpion's missile is out; bit 6 `CRIPPLED`; bit 7 a scorpion's relay deployer is damaged (a hit can set it; with it the relay count reads 0) |

From +0xfe each kind has its own layout (`usg::Parts`). The frame bytes from +0xfe are
the model's animations (`docs/render.md`, "Frame numbers"); the widget state is the
console's (T-043).

| Offset | Humanoid (`HumanoidParts`) | Scorpion (`ScorpionParts`) | Scout (`ScoutParts`) |
|---|---|---|---|
| 0xfe.. | `frames`, 5: body, arms, legs | `frames`, 12: body, legs, tail, claws | `frames`, 7: body, legs |
| 0x103, 0x104 | `arms_raised`: counted up while a weapon fires (`1000:ae23`, `ae8a`) | frames | frames |
| 0x105, 0x106 | +0x105 `step`, the walking counter; +0x106 unused | frames (the claws, stepped every 4 ticks unless damaged) | `cooldowns`: plasma gun 8, cannon 3 after a shot (the weapon step from `1000:c8ab`) |
| 0x107 | unused | frames | `shells`: cannon shells, 80 when made ("out of cannon shells") |
| 0x108, 0x109 | `cooldowns` of the two weapons, counted down every 8 ticks (T-054) | frames (the weapons' flashes from +0x11d) | +0x108 `light`; +0x109 `gear` (4 when made) |
| 0x10a | `light`: the console light's step, 0 to 3 (`1000:a93e`) | `grid` to +0x114, the relay-net orders' line of cells (`docs/sim.md`, "Relay nets"): +0x10a, +0x10c the cell (words), +0x10e, +0x110 the line's two lengths, +0x112 the error, +0x114 the step (2 or 4) with bits 7 and 6 for the directions negative | `step`, the walking counter |
| 0x10b | `gear` (4 when made) | | `stance` (0x2f when made) |
| 0x10c | widget state to +0x140 | | `stance_target` (0x30) |
| 0x10d | | | widget state to +0x148 |
| 0x115 | | unused | |
| 0x116 | | `step`, the walking counter | |
| 0x117 | | `static_time`: 5 ticks of static once the missile camera's missile is gone (`1000:99a6`) | |
| 0x118 | | `relays` carried, 12 when made and in stock files: the cargo bay. U deploys one (`1000:9b01`), L loads one within reach (`9b34`, under 12, moving slower than 16), D dumps them; `relays / 2` picks the speed table, so a full bay is slowest | |
| 0x119 | | Cleared when made and added into the cargo gauge (`1000:9a6d`); nothing else | |
| 0x11a, 0x11b | | `gauges`: the cargo gauges' last drawn values | |
| 0x11c | | `dump_wait`, 10 between the cargo dump's relays | |
| 0x11d | | `fired`: bits 0 and 1 a weapon fired, shown in the frames (`1000:96ab`) | |
| 0x11e | | `burst`: the trigger (`1000:98f1`) sets 5; each tick sets the fire request and counts it down | |
| 0x11f | | `cooldown`, the cannon's | |
| 0x120 | | `rounds`, 2 bytes: cannon rounds, 2000 when made ("rounds exhausted") | |
| 0x122 | | `missiles`: tail missiles, 6 when made | |
| 0x123 | | `missile`, 2 bytes: the missile in flight (kind 5); 0xffff none, and when it is gone (`1000:9743`) | |
| 0x125 | | `reload`, 2 bytes: ticks before the next missile, 400 after a launch (`1000:978e`) | |
| 0x127 | | widget state to +0x16b | |

The bytes after each kind's widget state (+0x141, +0x16c, +0x149) and the humanoid's
+0x106 and +0x107 are never used; stock files have them 0.

**How robots take damage** (`sim_damage_object`, `1000:f84d`, calls the kind's routine
in the table at `DS:0xe783` with the damage in `AL`): a robot hit for more than 80 is
destroyed at once (`sim_destroy_robot`, `1000:de51`). Otherwise each point of damage
picks one of the kind's components by chance from a table (`DS:0x6856` humanoid,
`0x659a` scorpion, `0x6c06` scout; three-byte entries of a chance out of 256 and a
routine) and damages it: the helm or the power percentage drops, or a bit above is
set (a second hit on a damaged weapon destroys it). The message panel reports each.
A robot hit for 80 or less is worn down, and destroyed when the damage it has
added up (+0xf9) reaches its kind's limit (above, T-107). T-054 traced the
details, destruction and the explosions it leaves (`docs/sim.md`, "Damage").

**Bases** (0x172 bytes, `usg::Base`):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x18 | 1 | `look` | Its look in the 3D view, the frame of its model; the end script raises it (T-055) |
| 0x19 | 1 | `relay_clock` | Phase 11's clock: +0x3b is added every 16 ticks; a carry counts +0xb5 (T-115, `docs/sim.md`, "The bases' phases") |
| 0x1a | 2 | `repairing` | Phase 12: the slot of the robot being repaired, 0xffff none (as in every stock file) |
| 0x1c | 2 | `repair_clock` | Phase 12's clock: +0x3a is added every 16 ticks; at 0x1800 a repair step is made |
| 0x20 | 1 | `relays` | Relays in store (25 or 26 in stock files): phase 11 adds one every 600 carries of +0x19, up to 70; a scorpion's repair restocks from it |
| 0x21 | 7 | `counts` | The count phase 5 makes every 256 ticks: relays in the field (deployed relays count for the human side), then humanoids, scorpions and scouts, each in the base and in the field; the situation room's status panel |
| 0x2b | 1 | `losses` | The side's robots lost this mission (T-045) |
| 0x2c | 1 | `kills` | Enemy robots destroyed this mission |
| 0x2d | 1 | `slider` | The slider held in the situation room's power rows (1 to 6, 0 none): every tick the routine at `DS:0x5b0a` moves +0x39..+0x3b by it |
| 0x2e | 2 | `damage` | Damage taken (`sim_damage_base`, `1000:62b7`). At 300 the base is destroyed (`GONE`) and the other side is told; below that its own side is told, at most once every 480 counts of `DS:0x374`. Playing a campaign scenario adds 0, 50, 100 or 150 to the player's base by the record's alert level (`sim_apply_scenario_alert`, table `DS:0x5a3f`) |
| 0x30 | 2 | `end_step` | Once the base is destroyed, the mission's end steps it every 16 ticks with explosions; at 0x24 the mission ends, won if the alien base was destroyed (`sim_end_mission_with_result`, T-046, T-055: `docs/sim.md`, "The mission's end"). 0 in every stock file |
| 0x34, 0x37 | 2, 1 | `typed`, `view_mode` | The orders' typing position and the main view's mode (0, 2, 4, 6, 8: O, S, M, R, C; T-044) |
| 0x38 | 4 | `power` | The power pool, then the three sliders: +0x39 the net's power (a base powers relays to +0x39 / 4), +0x3a repairs, +0x3b relays made. They add up to 0x100; stock 0, 0x80, 0x40, 0x40 |
| 0x3c | 2 | | The situation room's selected unit (a pointer; the launch button's) |
| 0x41 | 1 | `tick` | Steps by one every tick; its low four bits pick the tick's routine (`DS:0x5aea`): at 2 the alien base hands out paths (T-052, `docs/sim.md`, "Movement"); 5 the count, 9 the take-in, 11 relays made, 12 repairs, 13 the alien base's launch, 15 the end (T-115, `docs/sim.md`, "The bases' phases") |
| 0x4d | 2 | | The side's unit list, set at load |
| 0xb5 | 2 | `relay_wait` | Phase 11: carries of +0x19 towards the next relay made (600) |
| 0xbd | 0x43 | `view_state` | The situation room's widget state (T-043) |

**Relays** (0x34 bytes, `usg::Relay`, T-107). Set up by `1000:cc2a`; a scorpion's
deployment (U and the net opcode 0x83, `1000:cc69`) puts it at the scorpion's x, y and
z exactly, with the side bit clear, and the editor (T-142) snaps it to the lattice
(`1000:cca8`); the relay check (`2000:7faa`) takes every relay's cell on the lattice
(`docs/sim.md`, "Relay nets"). Ticked by `sim_tick_relay` (`1000:ccd9`):

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x18 | 6 | `frames` | The model's frames: +0x18 the body (0); +0x19..=+0x1c the stage's look (`DS:0x6fbb[stage]`); +0x1d the colour: 1 unpowered, 0 powered by the human net, 2 by the alien net (`1000:ccf0`, every time the state runs) |
| 0x1e | 1 | `clock` | Steps every tick from a random start; the state runs when its low four bits are 0, and an up relay looks for links when it is 0 |
| 0x1f | 1 | `state` | By `DS:0x6fc7`: 0 folded (out of the field and unlinked, its power and links kept; it rises once no scorpion is within 0x32 (8.8) of it, `1000:f703`), 2 rising (stage up by one a run; at 3 it is up), 4 up (in the field; links as below; folds when a scorpion comes within 0x32, the scorpion's L loading it), 6 folding (stage down; at 0 folded), 8 nothing. Nothing sets another (`docs/sim.md`, "The relay's tick (T-148)") |
| 0x20 | 1 | `stage` | 0 to 3; the look and the model height (+0x12, `DS:0x6fbf[stage]`) follow it |
| 0x21 | 1 | `power` | The power's reach: the largest of its feeding neighbours' less one (a base gives its +0x39 / 4, `2000:6978`, the situation room's first slider); 0 unpowered. Flag bit 4 (`RELAY_LINKED`) is set while it is not 0, and robots take their signal only from such relays and bases |
| 0x22 | 1 | `power_side` | Whose power: 0 none, 1 human, 2 alien. A neighbour of the other side's power is not taken; the relay's side bit (+0x16 bit 3) is set to follow it, so a relay can change sides |
| 0x23 | 16 | `links` | Four 4-byte links, one per quarter turn of bearing from the relay (`1000:cd88`: the nearest relay or base within 0x640 of it that way, 1/256ths): a state byte (0 none; 1 fed from that neighbour, which needs the neighbour's opposite link at 2, or is a base of a compatible side; 2 open, offering power: every link not fed, once the relay is powered), a byte nothing uses, and a word set to 0xffff when made that nothing reads (0 in stock files) |
| 0x33 | 1 | `damage` | At 20 the relay is removed (`sim_damage_relay`, `1000:cfba`) |

**Obstacles** (0x34 bytes) have no set-up, damage or tick routine (their entries are
bare `ret`s), so they never change and nothing reads or writes past +0x18: those bytes
are 0 in every stock file. A shot hits one only below 0x900 of height (z under 9,
`1000:e9ce`, through the kinds' hit tests at `DS:0xe761`).

**Kinds made during play** (5 to 15, T-107). None is in a stock file, but a save can
hold them; `usg::Body::Other` keeps their bytes. Each kind's set-up routine is in
`DS:0xe73f` (called with the maker in `DI` and the new object in `SI`), its tick in
`DS:0xe6e8`, its hit test in `DS:0xe761` (carry: a shot hits it):

| Kind | What | Set up, tick | Fields from +0x18 |
|---|---|---|---|
| 5 | The scorpion's tail missile | `1000:e08e`, `e0e8` | +0x19 the target (a referenced slot); +0x1b its speed (0x100, +0x80 a tick to 0x800), +0x1d its climb (clamped to ±0xe38), +0x1f, +0x21, +0x23 its velocity; +0x25 ticks in the phase; +0x27 its turn limit (0x16c, then +0x16 a tick); +0x29 the phase (0 launch for 20 ticks, 2, 4 homing). Starts 0x19c0 above the scorpion; within 0x3c of the target it climbs or dives at it; on the ground or a hit, a blast of 100 points in radius 100 |
| 6 | A shell (the scorpion's cannon, the scout's) | `1000:e235`, `e2c8` | +0x18 the speed (0x900); +0x1a, +0x1c its velocity; +0x1e the vertical speed (0x80 with a target, else random to 0x7f; less 3 a step); +0x20 steps before it can hit (15); +0x22 the damage (a 1-point shell explodes silently). It moves two steps a tick; the heading is the lead's, spread at random (`docs/sim.md`, "Weapons and shots") |
| 7 | A homing shot (the humanoid's first weapon, the scout's plasma) | `1000:e34a`, `e3ca` | +0x19 the target; +0x1b the speed (0x400); +0x1d the climb; +0x1f..+0x23 the velocity (none until its first turn); +0x25 ticks before it can hit (10); +0x27 its age (freed at 300); +0x29 the damage. It turns to its target by at most 0x2d8 every 8 ticks |
| 8 | The humanoid's missile (its second weapon) | `1000:e48a`, `e537` | +0x18 a spinning frame (0 to 3); +0x1b the speed (0x300); +0x1d..+0x21 the velocity, aimed with a lead (`1000:4549`) and a climb to the target (`1000:e98d`); +0x23 ticks before it can hit (10); +0x25 ticks it lives (0x3c0). Damage 8 (`1000:e587`) |
| 9 | A proximity mine | `1000:e5ab`, `e5b5` | +0x19 counts ticks; every 256 it looks for an enemy within 0x3c (`1000:e84f`): a blast of 50 points in radius 100 |
| 10 | An EMI decoy | `1000:e5e8`, none | +0x19 a word from `DS:0xe70c` by the dropping robot's kind, read back by `1000:e5fe` (not traced) |
| 11 | An alarm buoy | `1000:e602`, `e60c` | +0x19 counts ticks; every 256 +0x1a halves (flag bit 4 while it is not 0, the alarm showing) and an enemy within 0x96 sets it to 0x10 with a message |
| 12 | An ECM buoy | `1000:e63d`, `e64b` | As the alarm buoy, with +0x1a set to 0x40, and +0x1b its charges (10): one is spent each time; at 0 it is removed. What it does to the enemy is not traced (T-137) |
| 13 | An explosion | `1000:e79a`, `e7e3` | `docs/sim.md`, "Damage": +0x18 the frame, +0x1a the last, +0x1b ticks a frame, +0x1c the countdown |
| 14 | The sparkle (a cloak, F8) | `1000:dfff`, `e03b` | At the robot's position, heading 0x10, radius 0x2000. +0x18 the frame (0 to 7, one every 8 ticks of +0x19); +0x1a its age in frames: at 40 and 80 +0x1b (a word) steps to the next of the routines at `CS:0xe073`, which shrink the heading and radius; removed at 120 |
| 15 | Removed | none, `f5c1` | Removal (`1000:f5a4`) turns an object into kind 15 with +0x17 = 2; its tick counts +0x17 down and frees the slot at 0 |

Every drop (9 to 12, `1000:e801`) is put at the robot's position with a small random
offset, takes its side, and sets +0x18 bit 0 when the robot was on its battery.

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
typing position, and +0x37 the mode the mission starts in. The other main view modes
read more of the base (`usg::Base`, T-103): +0x32 the map's overlay bits, +0x36 its
zoom, +0x4f its centre (16.16 x and y) and +0x59 seven 13-byte log messages (name,
hour, minute, two unknown bytes, a string-segment text offset); stock bases save their
logs, with the texts of the build that wrote them (`docs/screens.md`, "The situation
room's main view (T-103)").

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
`SHDR`. `xeno_formats::usg::write` writes a `Scenario` this way (its `save` as `SDEF`
after `SHDR`, unknown chunks dropped, a missing chunk is
written as the game's empty state), and **every stock `.usg` comes back byte for
byte** (`real_assets_usg_round_trip`): stock objects are in slot order, every `MAPP`
is whole paragraphs and every `ORDR` ends at its NUL.

## Save files

Found in T-024, fields named in T-057. Progress is saved to `<NAME>.LOG`, where `NAME`
is the login name, up to eight characters, space-padded into the template at
`DS:0x613b` (`        .LOG`, filled by `1000:7912`). DOS reads the padded 8.3 name with
its trailing spaces dropped, so the file is `NAME.LOG`. `state.rst` is not a save file
(`rst.md`).

**Writing.** The map room's N (log in again) and Q (quit) both run
`ui_map_room_save_if_loaded` (`1000:7423`): only if a game is loaded (`DS:0x6005`),
`file_write_save_game` (`1000:f01b`) creates the file and writes the same chunks as the
scenario writer (`1000:efde`, above) with `SDEF` after `SHDR`. So a save is the
**whole loaded scenario as it stands in memory** plus the campaign: the clock, terrain,
objects (a mission left with no result included, so playing it again resumes it),
paths, briefing and sky of the scenario whose file is loaded (`DS:0x5f99`), which may
be a training mission or one already won or lost. A game counts as loaded once a save
has been read at log-in or a scenario has been played from the map room
(`ui_map_room_play_selected`, `1000:739c`); a new campaign that has not played one
yet writes no save.

**Reading.** At log-in `ui_login_and_load_save_game` (`1000:70ec`) calls the chunk
reader (`scen_read_chunks`, `1000:eefc`) on the name with `AL` = 0xff, which it keeps
at `DS:0x7ea1` (the map room's play and the training menu pass 0). If that fails (no
file, or no `SHDR` first) a new campaign starts from `scendef.bin`; otherwise the game
counts as loaded and the campaign check runs (`scen_update_campaign_progress`,
`docs/screens.md`, "Campaign rules"). With the flag 0 `SDEF`'s handler returns carry and
the reader skips the chunk, and the reader puts the campaign's calendar day and month
(`DS:0x7d4a`, `0x7d4b`) back after the file; with 0xff they stay as the save's `SHDR`
clock left them. **So the campaign's calendar is saved in `SHDR`.**

**`SDEF`** (`scen_write_sdef`, `1000:7666`) is one write of 36 bytes from `DS:0x5f91`
(the tag, the size word and the 30 bytes of state below) and one of the scenario table
buffer. The size word is 30 plus the table's size; the reader (`scen_read_sdef`,
`1000:76ab`) ignores it and reads 30 bytes, then +0x12 bytes of table into a buffer of
that size, indexes the records and sets the selection's record offset (`DS:0x5ff5`).

| Offset | Size | Field (`SaveState`) | Meaning |
|---|---|---|---|
| 0x00 | 2 | `selected` | The map room's selection `DS:0x5f97`: a record id (twice its position) or 0xffff |
| 0x02 | 2 | `loaded` | The scenario whose file is loaded `DS:0x5f99`, an id or 0xffff (training, a lost mission, a new campaign) |
| 0x04 | 2 | `progress` | `DS:0x5f9b`, compared with each record's `unlock_level` |
| 0x06 | 2 | `score` | `DS:0x5f9d` |
| 0x08 | 2 | `wins` | `DS:0x5f9f` |
| 0x0a | 2 | `losses` | `DS:0x5fa1` |
| 0x0c | 2 | `kills` | `DS:0x5fa3`, every mission's kills added up |
| 0x0e | 2 | `robots_lost` | `DS:0x5fa5`, the player's robots lost |
| 0x10 | 2 | `unknown_10` | `DS:0x5fa7`: a new campaign clears it; no other instruction names it |
| 0x12 | 2 | (table size) | `DS:0x5fa9`: the table buffer's size in bytes, its paragraphs times 16 (`mem_get_block_paragraphs`, `1fe6:1c7b`) |
| 0x14 | 1 | `result` | `DS:0x5fab`: 0xff new campaign, 0 no result, 1 won, 2 lost, 3 training |
| 0x15 | 1 | `campaign_won` | `DS:0x5fac` |
| 0x16 | 8 | `unknown_16` | `DS:0x5fad`..`0x5fb4`: no instruction names them (a search of every absolute operand); leftover memory |
| 0x1e | size | `table` | The scenario table as `scendef.bin` lays it out, each record's `state` as it stands, then the end marker and whatever else the buffer holds to its last paragraph (`scendef.md`) |

**The running world (T-116).** The objects are written as `scen_write_dcbs` writes
them (above, "Writing"): every slot of the object table in use, whatever its kind
(shots, explosions, mines and removed objects included), after the pack routines,
which only robots have (`DS:0xe81c`: the script pointer to its saved form and the
target to a slot, one more reference counted on it while saved). The reference count
written is the table's (`DS:0xe2c6`); stock files hold 0, 1 and once 0xfff2.
`xeno_sim::World::save` packs the port's world this way (`docs/sim.md`, "Saving the
world (T-116)").

The game-loaded flag (`DS:0x6005`), the selection's offset and the open levels
(`DS:0x5ff5`..`0x5fff`) are not saved; the reader and the campaign check rebuild them.

`xeno_formats::usg` parses `SDEF` into `Scenario::save` (`SaveState`; it rejects a
chunk whose size is not 30 plus the table's, or whose table does not decode) and
`write` writes it after `SHDR`; `SaveState::set_scenarios` pads a table with zeros to a
paragraph as the writer's buffer is. `xeno-tool inspect` prints a save's campaign.

## Open questions

- The object bytes still unnamed are never used (T-107): a robot's +0x25, +0x2f,
  +0xfc, the humanoid's +0x106 and +0x107, a scorpion's +0x115 and +0x119 and the
  tails after each console's widget state; a relay link's second byte and word; an
  obstacle's bytes past +0x18. The kinds made during play (5 to 15) are described
  above but kept as bytes; an EMI decoy's +0x19 and what an ECM buoy does are not
  traced.
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
