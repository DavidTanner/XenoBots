# The 3D view: render list, projection and frame numbers (T-100)

How `xeno.exe` turns the mission's objects into the robot's-eye view: who builds the
list of **render nodes**, in what order, what each object kind becomes, where each
animation's frame number comes from, and the projection behind a model's anchor and
scale. Drawing a node's images (facing, depth sort, scaled blit) is in
`docs/formats/mxx.md`, "Drawing". Addresses are Ghidra's, in the unpacked `xeno.exe`
of `docs/re-setup.md` (`DS:x` is Ghidra `2901:x`). The tables the port needs are read
from the user's `xeno.exe` by `xeno_formats::view3d` (below, "In the port").

## The step lists

The 3D views are drawn by a list of **steps** that the mission loop runs a few at a
time (`gfx_run_view_steps`, `1000:28cf`: `DS:0x372` is set to 3 and counted down by
the retrace hook, and the steps run until it reaches 0, so a frame's drawing is spread
over several passes of the loop). `DS:0x38ea` points at the next step; each step adds
2 to it when it is done. `DS:0x38ec` is the start, a list of one step,
`gfx_start_view_frame` (`1000:2914`), which sizes the view's buffer, resets the
pointer probe (below) and moves on to the list for the **view mode**, the byte at
`DS:0x1451`: list `[DS:0x38ec + mode]`.

| Mode | List | Steps | Used by |
|---|---|---|---|
| 4 | `DS:0x391a` | camera (`DS:0x4d2a`), **build**, ground, sky, **draw**, static, end | A console's main view (`1000:dba0`) |
| 6 | `DS:0x3928` | camera, reuse, ground, sky, draw, static, end | A console's second display, mode 0 |
| 8 | `DS:0x3936` | camera, build, ground, sky, draw, `1000:4164`, slot `0x5f0`, static, end | The second display, mode 3 |
| 0xa | `DS:0x3948` | as 6, plus `4164` and slot `0x5f0` | The second display, mode 1 |
| 0xc, 0xe | `DS:0x395a`, `0x3964` | camera (`DS:0x4d32`), map steps | The second display, mode 2; the situation room's map (T-103); 0xe the terrain editor (T-109) |
| 0x10 to 0x16 | `DS:0x3970`, `0x397c` | camera (`DS:0x4d2e`), build or reuse, map steps | The situation room (T-103) |
| 0x18 | `DS:0x3988` | other | Not the 3D view |
| 0x1a | `DS:0x398c` | camera, clear, one node at a time, end | The review (below, "The review's view") |

The steps of the 3D lists:

- **camera** (`gfx_set_view_camera`, `1000:29a3`): the focal factor and the camera's
  sine and cosine (below, "Projection").
- **build** (`gfx_build_render_list`, `1000:2a2b`): the render list (below).
- **reuse** (`1000:2b1a`): takes the list the last build made (`DS:0x39e4`) again.
- **ground** (`1000:2bc3`): fills the lower half of the buffer, from byte `W * H / 2`,
  with colour 0x3f.
- **sky** (`gfx_draw_sky`, `1000:2bfb`; `docs/formats/b@n.md`).
- **draw** (`gfx_draw_render_list`, `1000:2e5b`): every node, far to near (below).
- **static** (`1000:3c91`): when `DS:0x1455` is not 0, noise over the buffer (a
  damaged camera; T-104).
- **end** (`1000:2982`): marks the frame ready (`DS:0x1450` bit 7) and goes back to the
  start step.

**Two views a frame.** A console draws its main view first: its routine (humanoid
`1000:b507`, scorpion `1000:9d4a`, scout and the others alike) sets the target to the
main view's rectangle (`DS:0x145a`, for the scorpion `DS:0x6832`), `DS:0x1450` to 1
and calls `1000:dba0` (mode 4, so it builds the list). When that frame is ready
(`DS:0x1450` = 0x81) it copies the buffer to the screen through the picture's
colour-255 region (slot `0x5a4`, `docs/screens.md`) and reads the pointer probe. Then
it draws the second display (`DS:0x1450` = 2, rectangle `DS:0x683e` for the
scorpion, `1000:dbe8`): the mode is `DS:0x70ae[+0xef]` (the console's display mode:
0, 1, 2, 3 give 6, 0xa, 0xc, 8), from the same camera position, so modes 6 and 0xa
draw the main view's list again. That display, its zoom and pan, and its map are in
`docs/screens.md`, "The second display (T-104)".

## The camera

`1000:dba0` (the main view) sets, from the viewed robot (`DS:0x62f1`):

| Field | Value |
|---|---|
| `DS:0x1451` | 4 (the view mode) |
| `DS:0x1452` | 1 (zoom) |
| `DS:0x1454` | 0 (zoom shift) |
| `DS:0x145c` | the robot: its own node is never drawn |
| `DS:0x1462`, `0x1466` | its x and y (16.16) |
| `DS:0x146a` | its z plus its **eye height** +0x1c (a signed word added to the 16.16 z, carrying) |
| `DS:0x1460` | its heading (+0x10) |

The second display (`1000:dbe8`) uses the same position, a zoom of `1 << [+0xf1]`
(heading 0 and `1 << [+0xf2]` in mode 2), a zoom shift of 1 and the heading plus the
console's pan (+0xf4).

Eye heights (+0x1c, in 1/65,536ths of a unit like a z's low word): humanoid 0x0f80
(set when it is made, `1000:aba7`, and on every walking step, `1000:adb1`), scorpion
0x0b80 (`1000:92df`), scout 0x0a00 when made (`1000:c458`), then
`0x0d00 - 8 * stance` (+0x10b) as its stance steps (`1000:c5c4`), so 0x0b80 standing.

**The view's buffer.** The view draws into a buffer of its own, `W` by `H` pixels, the
width and height of the target rectangle (`DS:0x145a`: top +4, left +6, bottom +8,
right +0xa; `W = right - left`, `H = bottom - top`), described at `DS:0x1470` (segment
+0; the clip rectangle at +6: 0, 0, `H`, `W`; `H` at +0xa, `W` at +0xc). Anchors are in
the buffer's pixels; the buffer is then copied through the picture's colour-255 region.

## The render list

**Producers.** The build step calls the word slot `DS:0x1528` (a near routine; the
host's export block points it at `1000:41c0`) with `SI` = 0 the first time and `DI` =
where to write; each call writes one node and returns `SI` (where it got to) or carry
when there are no more. The chain:

1. **Terrain** (`gfx_emit_terrain_render_node`, `1000:41c0`): one type 8 node per
   `MAPP` record, in file order, while the record's size word is not 0. When done it
   sets `DS:0x152e` and jumps to the word slot `DS:0x152a`.
2. `DS:0x152a` is set by whoever runs the view: the mission (`1000:f437`) points it at
   **the objects** (`gfx_emit_object_render_nodes`, `1000:f85b`); the review points it
   at its one node (`1000:7a5f`, T-047); the terrain editor (`ui_run_map_editor`) points
   `DS:0x1528` at `1000:44ed` instead: the driver's circles and origin mark (slot
   `0x1500`), then `DS:0x152a`, with no terrain nodes (`docs/screens.md`, "The
   scenario editor (T-048)", T-109).

The objects' producer is a small state machine (state `DS:0xe873`, routines at
`DS:0xe875`). Starting, it points at the object table (`DS:0xe86f` = `0xe2c4`, 240
entries of 4 bytes, the first word the object or 0) and readies the paths. Then:

1. the alien paths' points, then the human paths', each as a type 0xe node, only while
   the scenario editor's path display is on (`1000:6d27`; nothing draws type 0xe in the
   3D view);
2. in view modes 0xc, 0xe and 0x12 only, one node for the viewed robot (`1000:f914`,
   the maps);
3. every object in slot order, 0 to 239, skipping empty slots: the kind's model is
   `DS:0xe70c[kind]` (a byte), and the kind's routine (`DS:0xe7d8[kind]`, or
   `DS:0xe7fa[kind]` in view modes 0xc and 0xe) writes the node or skips the object.

| Kind | Object | 3D node | Model |
|---|---|---|---|
| 0, 1, 2 | humanoid, scorpion, scout | type 2 | humanoid, scorpion, scout |
| 3 | base | type 2 | basedome |
| 4 | relay | type 2 | netrelay |
| 5 | shot | type 2 | missile |
| 6 | shell (T-114) | none | |
| 7, 8 | shots | type 2 | laser, plasma |
| 9 to 12, 15 | | none | |
| 13 | explosion | type 6 | explode |
| 14 | | type 6 | cloak |
| 16 | obstacle | type 2 | obst |

Kind 17 has a tick routine but its model byte would be read past the table (0xfe);
nothing makes one in the stock build as far as traced. Nothing in `xeno.exe` makes a
node of type 4 (`gfx_draw_model_with_turret`): every producer is above, and none
writes a 4.

**Sorting.** The first node goes in alone; each later one is inserted before the first
node whose near distance (+0x1a) is smaller than its own (unsigned 32-bit), so the list
runs from far to near and nodes at the same distance keep the order they were made in.
The head is kept in `DS:0x39e2` and `DS:0x39e4`.

### A node

Written by `gfx_write_render_node_header` (`1000:fac5`) from the object at `SI`, then
completed by the build step. Nodes are written back to back, each at an even offset,
in the buffer from `DS:0x39e6`; the build stops making them once one ends past
`DS:0x4bde` (about 4.6 KB: some 130 robots' nodes).

| Offset | Size | Field |
|---|---|---|
| 0x00 | 2 | Next node, 0 at the end |
| 0x02 | 2 | The object (its offset in `DS`); 0 for terrain, 0xffff for the review's |
| 0x04 | 1 | Type: 2 model, 6 sized model, 8 terrain, 0xa, 0xc, 0xe map marks |
| 0x05 | 1 | Model number (`mxx.md`, "Loading": 0, 2, ... 20) |
| 0x06 | 2 | The object's radius (+0x14) high byte; a terrain record's radius word |
| 0x08 | 2 | Heading: the object's +0x10 |
| 0x0a | 4 | x, 16.16: the object's +0x04 |
| 0x0e | 4 | y: +0x08 |
| 0x12 | 4 | z: +0x0c, with the object's **model height** +0x12 added to its low word (16 bits, no carry) |
| 0x16 | 4 | Distance from the camera, 16.16 (`sim_approx_distance_between`, x and y only: `max + min / 2`) |
| 0x1a | 4 | Near distance: the distance less `radius_high << 8`, 0 if that would be negative |
| 0x1e | n | One frame number per animation of the model (`n` = its animation count, `DS:0x2300[model]`) |
| 0x1e + n | 2 | Type 6 only: the size factor (the object's +0x10) |

The model height (+0x12, set per kind when the object is made: humanoid 0x0a00,
scorpion 0x0a80, scout 0x0c80, shots 0; a relay's by its stage, table `DS:0x6fbf`) is
where the model's anchor sits above the object's z; humanoids and scouts bob it as
they walk (below). Heights, eye heights and the objects' radii are all in 1/65,536ths
of a unit (a 16.16's low word: a radius of 0x0a00 is 10/256 of a unit), while a
terrain record's radius is 8.8 in units.

A terrain node (`1000:41c0`): +0x04 type 8, +0x05 0, +0x06 the record's radius, +0x08
`[DS:0x1486] << 4`, +0x0a to +0x15 the record's x, y and z, +0x1e the record's offset
in the `MAPP` buffer. Its near distance takes the whole radius off (+0x06 is shifted
left 8 for every node, so an object's radius high byte is 1/256 of a unit there and a
record's radius a whole one).

**Type 8** (`1000:3595`): the viewer test (a terrain node's object is 0, so it never
matches), the coarse cone, then the centre's depth (`Z` of the rotation) must not be
negative; then `gfx_draw_terrain_shape` (`docs/formats/usg.md`, "The shape ops") with
the node's heading less the bearing to it, and the pointer probe. No distance limit.

### Frame numbers

A node's frame bytes are copied from the object, from `DS:0xe71d[kind]` (a word: +0xfe
for robots, +0x18 for the others, 0 for the kinds that make no node), as many as the
model has animations. Who writes them:

| Kind | Bytes | Animations |
|---|---|---|
| Humanoid | +0xfe to +0x102 | 0 body (1 frame), 1 and 2 arms (7), 3 and 4 legs (5) |
| Scorpion | +0xfe to +0x109 | 0 body, 1 to 6 legs (6 each), 7 to 11 tail and claws |
| Scout | +0xfe to +0x104 | 0 body, 1, 2, 5, 6 legs (12 each), 3 and 4 (2 each) |
| Base | +0x18 | its damaged look, 0 to 2 |
| Relay | +0x18 to +0x1d | its stage's look (`sim_tick_relay`, T-113) |
| Explosion, cloak | +0x18 | the frame its tick steps (`docs/sim.md`, "Damage") |
| Shots | +0x18 | the missile's (kind 8) spinning frame, 0 to 3; 0 for the others (T-114) |
| Obstacle | +0x18 | 0 (one frame) |

**Walking.** Each robot's move routine ends, after the hills test, with its walking
animation (humanoid `sim_animate_humanoid_walk` `1000:ad67`, scorpion `1000:95a2`,
scout `1000:c799`). Each keeps a step counter, adds the speed (+0x26) shifted right
arithmetically by 3 to it (the low byte, wrapping) and takes `step = counter >> 4`:

| Kind | Counter | While moving (speed not 0) | While still |
|---|---|---|---|
| Humanoid | +0x105 | legs +0x101, +0x102 = `CS:0xaef1[step]`, `CS:0xaf01[step]`; model height = `0x0a00 + CS:0xaf31[step]` (words); eye height 0x0f80 | counter 0; every 8 ticks (`DS:0x62c6 & 7 = 0`) each leg not at 0 steps up one, 5 wrapping to 0 |
| Scorpion | +0x116 | legs +0xff, +0x101, +0x103, +0x100, +0x102, +0x104 from the six 16-byte tables at `CS:0x964b`, `0x965b`, `0x966b`, `0x967b`, `0x968b`, `0x969b` | every 8 ticks each leg not at 0 steps up one, 6 wrapping to 0 |
| Scout | +0x10a | +0xff and +0x103 = `CS:0xc860[step]`, +0x100 and +0x104 = `CS:0xc870[step]`; model height `0x0c80 + CS:0xc880[step]` (words) | turning, or standing (stance 0x30): counter and the four 0; otherwise (crouched, still) all four `CS:0xc8a0[stance >> 4]`, model height `0x0c80 - 8 * stance` |

Every branch of the scout's walk sets all four of its legs, so +0xff and +0x100 are
always +0x103's and +0x104's frames after its first tick. Its +0x101 and +0x102
(frames 3 and 4) are written by nothing in `xeno.exe` (no store to either offset in
the scout's code): they stay as loaded. The scorpion's six tables are read in the
order of the stores, `+0xff, +0x101, +0x103, +0x100, +0x102, +0x104`, so the port reads
each table with the leg its `mov [di + leg], al` names (T-120). The step BX the
humanoid's arms swing by is the legs' (`1000:ad80` sets it; the legs' routine and the
footstep, which pushes it, leave it alone), so `CS:0xaf11` and `0xaf21` are read with the
same step as the legs.

The scorpion's routine (`1000:95a2`) first sets +0x107 (frame 9, part of the tail) to 0
when it has missiles (+0x122) and its reload (+0x125) is 0, else 1, then the legs, then
`1000:96ab` (T-151's).

The tables are in the code segment (`CS:x` is image offset `x`). The humanoid's arms
(+0xff, +0x100) follow its weapons (`1000:ae23`, `ae8a`: raised through +0x103 and
+0x104 while firing, otherwise swung by `CS:0xaf11[step]` while walking and settled to
0 when still); a destroyed arm (+0xf6 bit 6, bit 7) shows frame 6. The scorpion's
other animations are stepped by `1000:96ab` (T-114).

**Footsteps** (`docs/formats/dsounds.md`, "Sound events", all for the viewed robot
only): a humanoid plays event 13 when a leg's new frame from the table is 0 and its
old one was not (`1000:ae11`, for each leg); a scout event 15 the same way for +0x103
and +0x104 (`snd_play_scout_footstep`, `1000:c84e`); a scorpion event 14 on **every**
tick it moves with an even step. Settling while still makes no sound.

## Drawing the list

`gfx_draw_render_list` sets the target (slot `0x574`: the buffer, its clip rectangle
and row table `DS:0x69c`), then calls each node's routine by its type byte from the
table at `DS:0x3994` (`[DS:0x3994 + type]`: 2 `gfx_draw_model`, 4
`gfx_draw_model_with_turret`, 6 `gfx_draw_model_sized`, 8 `1000:3595`, 0xc
`1000:361f`, every other type up to 0x18 nothing), far to near, then the pointer probe
(below). The maps use the table at `DS:0x39ae` (`1000:3877`): the overlapping entries
are why `mxx.md` once listed types 0x22 and 0x26 for `gfx_draw_model_on_screen`; that
routine is the map table's types 0x1c and 0x20 (T-103).

**`gfx_draw_model`** (type 2), in order; a failed test skips the node:

1. The node is not the viewer's (`+0x02` against `DS:0x145c`).
2. **Coarse cone.** `o` is the octant of the vector from the camera to the node
   (`sim_get_octant`, `1000:084a`: quadrant by the signs of x and y, then whether
   `|y| >= |x|` in that quadrant, so octant 0 is from +x towards +y, counting
   anticlockwise; a vector on a diagonal takes the higher octant, one on an axis the
   lower but for +x, which is octant 0, and a zero vector is octant 0) and `c` the camera's,
   the top three bits of `0x4000 - heading` (`1000:0378`, in `DS:0x4d28`). With
   `d = |o - c|` (as bytes), the node is kept if `(d + 2) & 7 <= 4`: within two
   octants either way. The camera's octant holds its view direction, so the cone
   always covers the half-plane in front: it only drops nodes behind or exactly
   abeam (depth 0, which the depth test below would keep).
3. The model has animations (`DS:0x2300[model]` not 0).
4. The distance's whole part (+0x18) is under 0x7d (125 units).
5. `P = F / float(distance)` (below), kept in `DS:0x4d4e`.
6. **Scale.** `s = int(float(size) * P * 256)`, where size is the model's size factor
   (`DS:0x2316[model]`, `mxx.md`); a factor of 0x100 stands for the float at
   `DS:0x4d52` (0x8000 with exponent -8: 128). `s` 0 skips. Slot `0x584` sets it.
7. **Anchor** (`gfx_project_render_node`, `1000:3f49`): the camera rotation below
   gives the lateral offset `X` and the depth `Z`; a negative depth (its top bit)
   skips; `ax = int(float(X) * P) + W / 2`, `ay = H / 2 - int(float(z_node - z_cam) *
   P)` (halves rounded down; `z_node - z_cam` the 32-bit difference of the 16.16s).
   Slot `0x578` sets it.
8. The facing, its view (loaded on demand if its memory block was dropped) and the
   frames, drawn as `mxx.md` describes.

`gfx_draw_model_sized` (type 6) swaps the node's size word into `DS:0x2316[model]`
for the call and puts the old value back. After each node, the **pointer probe**
(`1000:40bc`): if `DS:0x4c86` holds a buffer offset (not 0xffff) and the byte there is
no longer 0xff, the node's object goes into `DS:0x4c84` and the byte is set back to
0xff. So the probe ends up naming the nearest object drawn over that pixel; the
console reads it after the frame (`1000:41b7`) for the scanner's lock (T-104).

## Projection

**Numbers.** The projection uses the game's own floating-point format
(`1000:0cd8` to `0e9d`): a 16-bit magnitude `m`, an exponent byte `e` and a sign
byte; the value is `m * 2^e` (exponents are multiples of 8). The operations, exactly:

- `float(v)` of a 16.16 `v`, as the 32-bit integer `V` (`1000:0d2d`): sign of `V`; with
  `a = |V|`: `(a, 0)` if `a < 2^16`, `(a >> 8, 8)` if `a < 2^24`, else `(a >> 16, 16)`.
  So the value is the 16.16's raw integer, 65,536 times the number.
- `a * b` (`1000:0e11`): `p = ma * mb` (32 bits), exponent `ea + eb`, signs XORed;
  `(p, e)` if `p < 2^16`, `(p >> 8, e + 8)` if `p < 2^24`, else `(p >> 16, e + 16)`.
- `a / b` (`1000:0e3c`): exponent `ea - eb`, signs XORed. `mb = 1` gives `(ma, e)`,
  `mb = 0` gives `(0x6fff, 0x38)`. Otherwise the dividend is `ma << k`, with `k` the
  first of 0, 8, 16 that fits: 0 if `ma >> 8 >= mb`; else 8 if `ma >= mb`; else 16
  if `ma >= 256` or `ma << 8 >= mb`; else 24. The result is
  `((ma << k) / mb, e - k)`, the quotient truncated (it fits 16 bits).
- `int(a)` (`1000:0dcf`): `e = 0`: `m`; `e > 0`: `m << 8` if `m < 256` and `e < 16`;
  `e < 0`: `m >> 8` if `e > -16`, else 0; then negated for a negative sign. A
  magnitude of 0x8000 or more, or a larger value, gives 0x6fff (with the sign).

**Focal factor** (`gfx_set_view_camera`, `1000:29a3`, from the word and exponent byte
at `DS:0x4d2a`: 230 and 0 for the 3D view; `DS:0x4d2e` and `0x4d32` for the maps):
`V = word * zoom` (zoom `DS:0x1452`, used when above 1) as 32 bits, shifted left by
`n` until its top bit is set (whole bytes first, then bits); with `t = n + shift`
(`DS:0x1454`), it is shifted right by `t & 7` and `F = (V >> 16, 16 - (t & !7) +
exponent)`. So `F = word * zoom * 2^(exponent - shift)`, to 16 bits: **230** for the
main view.

**Rotation** (`1000:11c7`, `12b9`, `133c`). With `dx`, `dy` the node's position less
the camera's (32 bits), `c = cos(heading)` and `s = sin(heading)` (`sim_sine_magnitude`,
`docs/sim.md`, "Velocity": a 16-bit magnitude and a sign, or exactly 1):

- `X = dx * c - dy * s` (lateral, positive to the right),
- `Z = dx * s + dy * c` (depth, along the heading).

Each product is of `|a|` (a 16.16) and a magnitude `t` (1/65,536ths), as 48 bits:
`a_lo * t` when `|a| < 1`, `(|a| >> 8) * t << 8` when `|a| < 256` (the lowest 8
fraction bits dropped), `(|a| >> 16) * t << 16` otherwise; a factor of exactly 1 passes
`|a| << 16` through. The two signed products are added in 48 bits and bits 16 to 47
kept: a 16.16.

So, in real numbers, with `d` the node's approximate distance:
`ax = W/2 + F * X / d`, `ay = H/2 - F * (z_node - z_cam) / d`, and the scale
`s = 256 * size * F / (65,536 * d)`: at the main view's F = 230, a humanoid (size 0x44)
is drawn 1:1 about 0.24 units away. Perspective divides by the approximate distance,
not by the depth `Z`, which only decides what is behind.

## The review's view (T-047)

View mode 0x1a's list is camera, `1000:374e` (fills the buffer with colour
`DS:0x1457`, 0 for the review, and empties the list `DS:0x39e2`), `1000:2ea4` and end.
`1000:2ea4` draws without a list: it sets the target, then asks the producers for one
node at a time into a single node at `DS:0x4cd0` and draws each at once through the
table `DS:0x39c8` by type (2 and 6: `gfx_draw_model_on_screen`, `1000:3424`; the
others nothing), until they have no more. In the review the producers are the terrain
(`MAPP` records, if any are loaded; type 8 draws nothing here) and the review's hook.

`gfx_draw_model_on_screen` has no camera, culling or perspective: the scale is the
node's +0x12 word (slot `0x584`), the anchor (`W / 2 + [node + 0x0b]`, `H / 2 -
[node + 0x0f]`), the words in the middle of x and y (slot `0x578`), the facing from
the node's heading alone (`docs/screens.md`, "The review (T-044)"), then the frames as
`gfx_draw_model`. Its table entries are the maps' types 0x1c and 0x20 (`DS:0x39ae`),
so `mxx.md`'s note on those holds too.

`xeno_render::LoadedModel` (a model's facings as `gfx_load_model` fills them),
`model_facing` and `Framebuffer::draw_model` (a model at a facing with its frame bytes)
are shared with the mission's 3D view (T-101), which still needs the projection above.

## Not traced here

- The ground's `GROUND.BIN` (`1000:435c`, drawn by `1fe6:67d3`; not in a stock
  install). The map marks (types 0xa, 0xc, 0xe, 0x12) and the map table are in
  `docs/screens.md`, "The situation room's main view (T-103)".
- The second display's extra steps (`1000:4164`, slot `0x5f0`) and the static (T-104).

## In the port

`xeno_formats::view3d::read` reads from the user's `xeno.exe`, after checking the
instructions that use them: the kinds' models (`DS:0xe70c`) and frame offsets
(`DS:0xe71d`), each kind's 3D node type (following the routines of `DS:0xe7d8` to the
header writer), the focal factors (`DS:0x4d2a`, `0x4d2e`, `0x4d32`), the models' size
factors (the `mov` instructions at `1000:7b61`) and the stand-in size at `DS:0x4d52`,
and the distance limit (`1000:301e`). Drawing the view is T-101; the model drawing it
needs is in `xeno_render` (above, "The review's view").

**The 3D view (T-101).** `xeno_render::view3d` has the float operations
(`GameFloat`), the focal factor, the rotation (`Camera::rotate`), the octants and cone,
the sort (`sort_far_to_near`) and the culling and projection (`place`), and
`Framebuffer::draw_view` runs the ground, sky and draw steps into the view's buffer.
`xeno-app`'s `scene` builds the camera and the nodes from the sim's world (distance
and bearing by `xeno_sim`'s `approx_distance` and `bearing`, the coarse one
`1000:059e` uses) and the console copies the buffer through its picture's colour-255
region, the bounding box `review::region` measures. `LoadedModel::single_view` and
`quarter_turn` are the other two loaders. `xeno_render::shape` runs the terrain
nodes' programs (the hill and block programs are read from the user's `xeno.exe` by
`xeno_formats::view3d`), with a scan fill that is not yet the original's edge walker.
Each object's z and model height and the viewed robot's eye height are the sim's
(`Entity::z`, `Entity::model_height`, `Movement::eye_height`; `docs/sim.md`,
"Heights"). Not yet: only the frames the sim keeps are set (the humanoid's and scout's footstep legs,
a base's look, an explosion's frame), the rest are 0; the sky is detail 1 only (the
main view's).

`xeno_sim::motion::walk` runs each robot's walking animation after its move with the
leg tables `xeno_formats::motion` reads (`Motion::walk`, through the `mov al,
cs:[bx + table]` of each routine): the step counter and the two legs that make
footsteps (`Movement::stride`, `legs`, loaded from the file and in the state hash, as
is the pass count `World::ticks`), and it reports the footsteps as
`motion::Event::Sound` for the robot (`xeno-app` plays them on its console). It also
sets the model height (+0x12) by the step (the humanoid every tick, the scout while it
moves or crouches) and the humanoid's eye height, from the bob tables and bases read
through the `add ax, cs:[bx + table]` of each routine (`Walk::humanoid_bob`,
`scout_bob`; `docs/sim.md`, "Heights", T-119). The humanoid's arms swing by the same
step (`xeno_sim::weapons::arms`). The scout's four legs walk and crouch (T-120); the
scorpion's six are read (`Walk::scorpion_legs`) but not yet stepped (T-120 part 2).
