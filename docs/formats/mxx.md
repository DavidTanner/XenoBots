# `.m00` / `.m08` / `.m16` / `.m32` unit models (`.MXX`)

## Status

**Confirmed** (T-015, T-016, T-029). The suffixes, the view header and the whole view
body are confirmed from the data: `xeno_formats::mxx` reads all 40 files of a stock
install, every byte of every view belongs to a table or an image, every file holds
exactly the facings its suffix predicts, and every part's image reference resolves.
T-029 traced the loader in `xeno.exe` and the drawing code in `xeno.exe` and
`mgavideo.dvr` ("Loading" and "Drawing" below; addresses at the end), which settled
the facing direction, the animations, the frame's depth byte, colour 0 and the
palette.

## Files

| Model | `.m00` | `.m08` | `.m16` | `.m32` |
|---|---|---|---|---|
| `basedome`, `cloak`, `explode`, `laser`, `netrelay`, `obst`, `plasma` | yes | yes | yes | yes |
| `humanoid`, `missile`, `scorpion`, `scout` | yes | yes | yes | no |

Sizes run from 18 bytes (most `.m00`) to 42 KB (`humanoid.m16`). Nothing is packed.

`mgavideo.dvr` names the models `HUMANOID.MXX`, `SCORPION.MXX`, ... in its file-id
table (ids 0x42 to 0x98, four consecutive even ids per model in the order humanoid,
scorpion, scout, missile, netrelay, explode, plasma, laser, basedome, cloak, obst).
The game only ever asks for the first id of each model and writes the suffix itself
(see "Loading"). The name builder (host slot `0x480`) also has code that would put a
suffix from the string `00081632` over the `XX`, but a jump skips it and its range
check would stop before `obst`: it is dead, and the other three ids of each model are
never requested.

## What the suffix means

Facings are measured in **64ths of a turn** (5.625 degrees). Each view stores one
facing `a` from 0 to 32 together with its mirror facing `(64 - a) % 64`, so 17 views
cover all 32 even facings. The suffixes split those views by angular resolution:

| Suffix | Facings in the file (each with its mirror) | Views | Directions once combined with the files above it |
|---|---|---|---|
| `.m00` | none (`-2`) | 1 | none: a direction-free view |
| `.m08` | 0, 8, 16, 24, 32 | 5 | 8 |
| `.m16` | 4, 12, 20, 28 | 4 | 16 |
| `.m32` | 2, 6, 10, ..., 30 | 8 | 32 |

So `.m08`, `.m16` and `.m32` are not three copies of a unit at different detail.
They are three disjoint sets of facings, and each file only adds the directions
between those of the file before it. The four models without a `.m32` are drawn in
16 directions.

**Facing 0 is the unit facing the viewer**, and facing 32 shows its back. A facing is
the unit's heading measured from the direction that points from the unit to the
viewer, in the same sense as the game's angles; facing 16 shows the unit side-on with
its front to the viewer's left (the scorpion's head points left). The stock art agrees:
`scorpion` facing 0 shows its head and claws, facing 32 its raised tail.

The `.m00` view of most models is an 18-byte file with no data. `netrelay.m00` (15 KB)
and `scout.m00` (216 bytes) are the only ones with content.

## Layout

A file is a chain of views back to back, ended by a u16 0. Nothing follows the 0.

Each view starts with a 16-byte header (`ViewHeader`). Offsets count from the start
of the view.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 LE | `size` | Bytes in this view, header included. 0 ends the file |
| 0x02 | i16 LE | `facing` | 0 to 32, or -2 for no facing (`.m00`) |
| 0x04 | i16 LE | `mirror_facing` | `(64 - facing) % 64`; -2 with no facing |
| 0x06 | u16 LE | `index_offset` | Two-level index for `facing`. 0 with no facing |
| 0x08 | u16 LE | `mirror_index_offset` | Two-level index for `mirror_facing`. 0 with no facing |
| 0x0a | u16 LE | `lists_offset` | Start of the lists `index_offset`'s second level points to. With no facing, an unknown value instead |
| 0x0c | u16 LE | `mirror_lists_offset` | The same for `mirror_facing`. With no facing, an unknown value instead |
| 0x0e | u16 LE | `data_offset` | Start of the data both facings share; runs to the end of the view |
| 0x10 | | body | Indexes, lists, then data |

Checks the parser applies (all hold in a stock install):

- `size` is at least 16 and the view fits in the file.
- `data_offset` is between 16 and `size`.
- With a facing: `mirror_facing` is `(64 - facing) % 64` and the four table offsets
  lie between 16 and `data_offset`.
- With no facing: both facings are -2 and both index offsets are 0.

Observed but not enforced:

- When the facing is its own mirror (0 and 32), the mirror offsets equal the plain
  ones: one index serves both. Every other view has a separate index per facing over
  the same images, and the mirror facing's parts mostly draw those images flipped.
- In the no-facing views, the four bytes at 0x0a to 0x0d are often one value
  repeated. In `basedome.m00` that value is the main shade of the dome's pixels in
  `basedome.m08`. The game never reads them, nor `lists_offset` and
  `mirror_lists_offset` in any view: it only uses `size`, the facings, the two index
  offsets and `data_offset`.

## Inside a view

Offsets are from the start of the view and all words are u16 LE. With a facing, the
body is, in order and with no gaps:

1. the index for one facing, then its frame lists;
2. the index for the other facing, then its frame lists (absent for facings 0 and 32);
3. the images, from `data_offset` to the end of the view.

The first index is at offset 16, and each run of frame lists ends where the next table
starts. A view with no facing has only the images, starting at 16.

### Index: animations, then frames

At `index_offset` there is one word per **animation**, pointing at that animation's
list of **frame** words; the first of them therefore also marks where the first level
ends. An animation's frame words run to the next animation's, and the last
animation's to `lists_offset`. Each frame word points at a frame list. The frame lists
are stored in the same order as the frame words, back to back from `lists_offset`, and
none is shared.

Every view of a model has the same animations and frame counts:

| Model | Frames per animation |
|---|---|
| `basedome` | 3 |
| `cloak` | 8 |
| `explode` | 32 |
| `laser`, `obst` | 1 |
| `missile` | 2 |
| `plasma` | 4 |
| `humanoid` | 1, 7, 7, 5, 5 |
| `netrelay` | 1, 4, 4, 4, 4, 3 |
| `scorpion` | 1, 6, 6, 6, 6, 6, 6, 5, 5, 2, 3, 3 |
| `scout` | 1, 12, 12, 2, 2, 12, 12 |

The animations are not alternatives but the moving pieces of the unit: the game
draws one frame of **every** animation at once, each animation with its own frame
number (see "Drawing"). For `humanoid`, animation 0 is the body and the others are the
legs and arms; a frame with no parts hides that piece.

### Frame list

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u8 | count | Number of parts; 0 is allowed |
| 0x01 | u8 | `depth` | Draw-order key: a multiple of 5 from 0 to 250. The frames of all a model's animations are drawn in increasing order of this key, so higher keys end up on top |
| 0x02 | 4 x count | parts | See below |

Each part is an image drawn at an offset from the unit's position:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 LE | `image` | Bits 0-13: image number. Bit 14: the image is in the model's `.m00` view, not this one. Bit 15: draw it mirrored left to right |
| 0x02 | i8 | `x` | Left edge relative to the unit |
| 0x03 | i8 | `y` | Top edge relative to the unit |

A flipped part keeps `x` as its left edge: `humanoid`'s body is a left half at
`x = -27` and the same image flipped at `x = -1`. Only `netrelay` (every file) and one
part in `scout.m16` use bit 14; they draw from `netrelay.m00` (51 images) and
`scout.m00` (one 27x16 image).

Models that look the same from every side (`basedome`, `cloak`, `explode`, `obst`,
`plasma`) keep their images in facing 0 only. Their other views have the same
animations, but every frame has no parts.

### Images

At `data_offset`, one word per image gives its offset; the first one therefore also
marks the end of the table. The images follow back to back, each running to the next
(the last to the end of the view). An image is:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u8 | `width` | |
| 0x01 | u8 | `height` | |
| 0x02 | | rows | Per row: a skip byte, a count byte, then `count` 8-bit pixels |

This is the `.b@n` row-span encoding without the flags. Details:

- An empty row (count 0) has an arbitrary skip byte, sometimes wider than the image
  (0xfd to 0xff, 15, 27, ...). Rows with pixels always fit in the width.
- One image stores fewer rows than its height: image 12 of `humanoid.m08` facing 24
  is 2x1 with no rows at all, although a frame draws it. Its counterparts in the
  other facings are 35x11 and 40x9, so it is probably a cut that was left in. The game
  reads the next image's first bytes as its row; that span starts past the image's
  width, so nothing is drawn.
- About 8% of the pixels inside spans are 0. The game skips them: **colour 0 is
  transparent**, inside spans as well as outside them.
- No trailing empty span follows the last row (unlike some `.b@n` banks).

### The `.m00` view

`.m00` has one view with no facing. `netrelay.m00` and `scout.m00` hold images (in the
layout above) that the other files' parts reach with bit 14; the other nine have none.
The four bytes at 0x0a are not offsets and remain unknown (see "Layout").

## Loading

All models are loaded once at start-up, after the intro, by `gfx_load_all_models`.
It sets each model's size factor (see "Drawing") and walks a 25-entry table in
`xeno.exe`'s data (`DS:0x6443`): a file group, the model's first file id, its model
number and which of three loaders to call. The model number is `(id - 0x42) / 4`, so
humanoid 0, scorpion 2, scout 4, missile 6, netrelay 8, explode 10, plasma 12, laser 14,
basedome 16, cloak 18, obst 20. The table makes three passes, so every model gets its
coarse directions before any gets finer ones:

| Pass | Files | Models asked for |
|---|---|---|
| 1 | `.m00` then `.m08` | all 11 |
| 2 | `.m16` | all but `cloak` and `obst` |
| 3 | `.m32` | `netrelay`, `explode`, `plasma`, `laser`, `basedome` |

| Loader | Models | What it keeps |
|---|---|---|
| `gfx_load_model` | `humanoid`, `scorpion`, `scout`, `missile`, `laser` | Every facing it loaded: 16 directions, 32 for `laser` |
| `gfx_load_model_quarter_turn` | `netrelay` | Facings 0 to 14 only. Facing `f` draws facing `f mod 16`, never mirrored, so the relay looks the same every quarter turn |
| `gfx_load_model_single_view` | `explode`, `plasma`, `basedome`, `cloak`, `obst` | Facing 0 only, drawn for every direction. It ignores passes 2 and 3 |

So the files actually read are every `.m00` and `.m08`, the `.m16` of the five
directional models and `netrelay`, and the `.m32` of `laser` and `netrelay`: 30 of the
40. The `.m16` and `.m32` of `explode`, `plasma`, `basedome`, `cloak` and `obst` are
never opened. Where a model has no
picture for a direction, the game shows another facing's (below); a facing whose frames
have no parts is never chosen, so it would draw nothing.

Reading a file (`file_load_model_views`): the name comes from slot `0x480` for the
model's first id, the loader writes `00`, `08`, `16` or `32` over the `XX` (bytes 10
and 11 of the padded 8.3 name), and opens it through slot `0x2b4` (so a `.M!8` or
`.M@8` form would also be found; `packed.md`). It then reads view after view until a
`size` of 0, each into a memory block of its own, so every offset in a view stays
relative to the start of that block. Each model has a table of 32 view pointers, one
per even facing, and a view with facing `a` fills the entries for `a` and its mirror.
The model's animation count comes from the last view read. If a file is missing or
memory runs out, that pass's views are dropped; if the `.m08` pass fails the model has
no animations and is never drawn. The start-up loop ignores errors.

Directions without a view of their own take one that is loaded, filled in after each
pass:

- after `.m08`: facings `8j - 2`, `8j + 2` and `8j + 4` show facing `8j` (so a facing
  halfway between two `.m08` facings shows the lower one);
- after `.m16`: facings `4j + 2` show facing `4j`;
- after `.m32`: every even facing has its own view.

The quarter-turn and single-view loaders then overwrite the table as described above,
and for the views they keep set the mirror index offset equal to the plain one, so
those views are drawn unflipped from every side. The views they don't use are
flagged in their memory blocks' descriptors as no longer needed.

## Drawing

Models are drawn only into the 3D view, by `gfx_draw_render_list`, one step of the
view's per-frame step list. It walks the frame's list of render nodes and calls a
routine by the node's type byte (table at `DS:0x3994`):

| Type | Routine | What it draws |
|---|---|---|
| 2 | `gfx_draw_model` | A unit at its world position |
| 4 | `gfx_draw_model_with_turret` | The same, but animation 0 turns separately: it is always frame 0, drawn at the facing given by the node's byte +0x1e (an angle's high byte), and the other animations' frame numbers start at +0x1f |
| 6 | `gfx_draw_model_sized` | Type 2 with a size factor taken from the word after the node's frame numbers |

Nothing in `xeno.exe` makes a type 4 node. `gfx_draw_model_on_screen` (`1000:3424`,
a model at a fixed point, its facing the node's heading itself) is not in this table:
T-029 read it at types 0x22 and 0x26, but those run past the 3D table into the maps'
(`DS:0x39ae`), where it is types 0x1c and 0x20 (T-103).

Who makes the render nodes, in what order, which type and frame numbers each object
kind gets, and the projection behind the anchor and scale are in `docs/render.md`
(T-100). A node holds, among others, the object (+0x02), the type (+0x04), the model
number (+0x05), the heading (+0x08, 65,536 to a turn), the position (+0x0a), the
distance from the camera (+0x16, 16.16) and, from +0x1e, one frame number per
animation.

**Choosing the facing.** With `b` the bearing from the camera to the unit and `h` the
unit's heading (both 65,536 to a turn), the game computes, in 16-bit arithmetic,
`g = (((h - b) >> 10) + 1) & 0x3e` and draws facing `f = g ^ 0x20` (64ths of a turn;
`f` above 32 is the mirror facing of a view). That is the relative heading rounded to
the nearest even 64th (a tie rounds up), turned half way round: the unit's heading
measured from the direction pointing at the viewer (above). `sim_get_bearing` measures
angles from the +y axis towards +x.

**Culling and scale** (type 2). The viewer's own object is skipped, as are units
outside a coarse cone of two octants either side of the view direction, models with
no animations and units 125 or more units away. The scale `s` (8.8 fixed point, 256 = 1:1) is the model's size factor times the
projection's factor for the distance (`docs/render.md`, "Projection"). Size factors
set at start-up: humanoid 0x44, scorpion 0x80, scout 0x40, missile 0x80, netrelay
0x28, plasma 0x28, laser 0x20, basedome 0x180, obst 0x58. `explode` and `cloak` keep
0x100, which stands for 128 (the float at `DS:0x4d52`), and type 6 nodes bring their
own. The unit's position is projected to the anchor, the screen point that part
offsets are measured from.

**Composing a frame.** For each animation `k` (0 to the model's count - 1) the game
takes frame number `node[0x1e + k]` of animation `k` in the facing's index, with no
range check. It sorts those frames by `depth`, lower first, keeping animation order
for equal keys, and draws each frame's parts in list order, so later parts and higher
keys end up on top. A part's image is in this view, or with bit 14 in the model's
`.m00`; bit 15 mirrors it.

**Drawing an image** (`mgavideo.dvr` slot `0x58c`, with the target and clip rectangle
from slot `0x574`, the anchor from `0x578` and the scale from `0x584`):

- The image's top-left is at (`x`, `y`) in unit pixels from the anchor. A mirrored
  image covers the same columns, with source column `c` at `x + width - 1 - c`.
- Unit pixels are scaled through a table of 256 repeat counts `r[u]` for unit columns
  (and rows) `u` = -128..127: every count is `ceil(s / 256)`, and when `s mod 256` is
  not 0, the first `256 - s mod 256` entries of a fixed order get one less. In that
  order the `k`-th entry is the column with `u + 128 = 255 - bitrev8(k)` (`bitrev8`
  reverses the 8 bits). Unit column `u` covers `r[u]` screen columns, starting at
  the anchor plus the sum of `r` over `0..u` (minus the sum over `u..0` when `u` is
  negative); a count of 0 drops it. Rows work the same way. So `s = 256` is 1:1,
  smaller scales drop evenly spread rows and columns, and larger ones repeat them.
  (Slot `0x580` can pick one of six such orders, but order 0, the one above, is
  always used: no binary calls slot `0x580` or `0x588`, and inside `mgavideo.dvr` the
  routine of `0x580` is reached only from that of `0x588`, which draws a list of
  images with a pattern each; T-100.)
- Only pixels inside the row spans are drawn, and **colour 0 is skipped**. Each row is
  limited to the image's width.
- Unlike the sprite blitters (`b@n.md`), the image is clipped to the 3D view's
  rectangle rather than dropped when it does not fit.

Details confirmed in `mgavideo.dvr` (T-056):

- The scale slot keeps the last scale and rebuilds the table only when it changes.
  The counts are bytes: the fill value is the high byte of `s + 0xff` in 16 bits, so
  scales above 0xff00 would wrap to 0. The game never asks for one; the port does not
  reproduce the wrap.
- The image's rectangle on screen runs from the anchor plus the start of unit `x` (and
  `y`) to the end of unit `x + width - 1` (and `y + height - 1`), summed straight
  through the table. Units outside -128..127 would read past the table's 256 bytes;
  every part of a stock model stays inside it (`x >= -96`, `x + width <= 98`,
  `y >= -43`, `y + height <= 66`), and the port draws nothing there.
- The clip rectangle (slot `0x574`: a row-address table, and top, left, bottom and right
  words) includes its left and top edges and excludes its right and bottom ones. The
  image's rectangle is cut to it first; a unit cut part way keeps its visible columns
  or rows.
- Each drawn row stops at the rectangle's right edge, which is what limits a row to
  the image's width.
- There are three row routines, picked in this order: mirrored (any scale), enlarged
  (`s` above 256) and shrunk (`s` up to 256, every count 0 or 1). They follow the same
  rule; a row whose count is 0 is skipped, and a larger one is drawn that many times.
  A mirrored image is drawn right to left from the rectangle's right edge, reading the
  table backwards from unit `x + width - 1`.

**Palette.** Models carry none and are drawn with whatever palette is current. In a
mission that is the cockpit screen's (`humancon`, `scorpcon` or `scoutcon`, shown with
its own palette, `m@l.md`). The model pixels use indices 1, 5 to 12, 49 to 63 and 128
to 254, and the three cockpits agree on all of them except 253 and 254, which only
`netrelay.m00` uses (its lights come out in a different colour in each cockpit).

## Open questions

None about loading, drawing or facing direction. Which render type and frame numbers
each object gets, and the projection, are in `docs/render.md` (T-100).

## In the executables

`xeno.exe` (Ghidra `segment:offset`, the unpacked binary of `re-setup.md`):

| Address | Name | Purpose |
|---|---|---|
| `1000:059e` | `gfx_get_model_facing_index` | Facing table index from heading and bearing |
| `1000:0735` | `sim_get_bearing` | Bearing between two positions, 65,536 to a turn |
| `1000:1652` | `gfx_load_model_single_view` | Loader for direction-free models |
| `1000:16e7` | `gfx_load_model_quarter_turn` | Loader for `netrelay` |
| `1000:17e3` | `gfx_load_model` | Loads a model's `.m00`+`.m08`, `.m16` or `.m32` (`CX` 8, 0x10, 0x20) |
| `1000:1861` | `gfx_unload_model_files` | Drops one pass's views after a failure |
| `1000:18f2` | `gfx_reset_model_facings` | Points every facing at one view |
| `1000:190d` / `1930` | `gfx_fill_model_facings_from_m08` / `_m16` | Fill-in after a pass |
| `1000:195f` | `file_load_model_views` | Reads one file, a block per view |
| `1000:1a5e` | `gfx_set_model_size` | Sets a model's size factor |
| `1000:2e5b` | `gfx_draw_render_list` | 3D view step: draws every render node |
| `1000:2fda` | `gfx_draw_model` | Render type 2 |
| `1000:318f` | `gfx_draw_model_with_turret` | Render type 4 |
| `1000:3402` | `gfx_draw_model_sized` | Render type 6 |
| `1000:3424` | `gfx_draw_model_on_screen` | Render types 0x22, 0x26 |
| `1000:3f49` | `gfx_project_render_node` | Projects a node to the anchor |
| `1000:3fac` | `gfx_get_model_scale` | Size factor times projection |
| `1000:7b55` | `gfx_load_all_models` | Start-up loading table |

`mgavideo.dvr` (load-image offsets): `file_build_name_from_id` `0x01a4` (slot `0x480`),
`gfx_set_model_target` `0x3c2c` (slot `0x574`), `gfx_set_model_anchor` `0x3c6d`
(`0x578`), `gfx_get_scale_pattern` `0x3c7c` (`0x57c`), `gfx_select_scale_pattern`
`0x3c92` (`0x580`), `gfx_set_model_scale` `0x3cc0` (`0x584`),
`gfx_draw_model_image_list` `0x3d18` (`0x588`, never called),
`gfx_draw_model_image` `0x3d6c` (`0x58c`), `gfx_draw_model_image_scaled` `0x3d70`,
`gfx_place_model_image_rect` `0x3dab`, `gfx_key_model_rows_shrunk` `0x3f12`,
`gfx_key_model_rows_enlarged` `0x4046`, `gfx_key_model_rows_mirrored` `0x419f`.

## Parser

`xeno_formats::mxx::parse`, added by T-015 and extended by T-016. Each `View` holds
its header, its animations (`Vec<Vec<Frame>>`) for `facing`, those for
`mirror_facing` when they are separate, and its images as row spans
(`Image::to_indexed` expands them). It rejects anything that leaves a byte of a view
unaccounted for. `xeno-tool inspect` lists the views and counts images, animations and
frames. `xeno-tool dump` writes each frame of each facing as a PNG with a transparent
background (`<model>_f<facing>_a<animation>_<frame>.png`; colour 0 is left
transparent, as in the game), reading the model's `.m00` from the same directory for
bit-14 parts, and each `.m00` image on its own. It draws each animation's frames
separately; combining them by `depth` is for the renderer.

## Drawing in the port

`xeno_render::ModelScale` is the repeat table and `Framebuffer::blit_model_image` the
image blit of slot `0x58c`, with a `ModelTarget` holding the clip rectangle, the
anchor and the scale. `Framebuffer::draw_model_frames` composes one frame of each
animation by `depth`, as above (T-056).
