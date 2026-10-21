# `.m00` / `.m08` / `.m16` / `.m32` unit models (`.MXX`)

## Status

**Layout confirmed** (T-015, T-016). The suffixes, the view header and the whole view
body are confirmed from the data: `xeno_formats::mxx` reads all 40 files of a stock
install, every byte of every view belongs to a table or an image, every file holds
exactly the facings its suffix predicts, and every part's image reference resolves.
Composed frames look right (a humanoid built from a half-body image and its mirror,
the scorpion turning through its facings). Not yet known: the palette units are drawn
with, two unknown bytes, and how the game draws and animates them (T-029).

## Files

| Model | `.m00` | `.m08` | `.m16` | `.m32` |
|---|---|---|---|---|
| `basedome`, `cloak`, `explode`, `laser`, `netrelay`, `obst`, `plasma` | yes | yes | yes | yes |
| `humanoid`, `missile`, `scorpion`, `scout` | yes | yes | yes | no |

Sizes run from 18 bytes (most `.m00`) to 42 KB (`humanoid.m16`). Nothing is packed.

`mgavideo.dvr` names the models `HUMANOID.MXX`, `SCORPION.MXX`, ... in its file-id
table (ids 0x42 to 0x98, four consecutive even ids per model in the order humanoid,
scorpion, scout, missile, netrelay, explode, plasma, laser, basedome, cloak, obst). Its
name builder (export at host slot `0x480`) has code that replaces `XX` from the string
`00081632`: id `0x42 + 8n + 2k` gets the k-th suffix, so each model's four ids are its
`.m00`, `.m08`, `.m16` and `.m32`.

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
between those of the file before it. The four models without a `.m32` presumably
face only 16 ways.

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
  `basedome.m08`. They may be colours for a distant or map marker (guess).

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

Animation 0 of the robots is a single standing pose (guess). The others are not
identified; some draw only a limb or a lying body, so they may be layered over or
swapped in for the pose.

### Frame list

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u8 | count | Number of parts; 0 is allowed |
| 0x01 | u8 | `unknown_01` | A multiple of 5 from 0 to 250; 0 for every one-animation model |
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
  other facings are 35x11 and 40x9, so it is probably a cut that was left in.
- About 8% of the pixels inside spans are 0, so 0 is not simply "transparent". Pixels
  outside the spans are not drawn.
- No trailing empty span follows the last row (unlike some `.b@n` banks).

### The `.m00` view

`.m00` has one view with no facing. `netrelay.m00` and `scout.m00` hold images (in the
layout above) that the other files' parts reach with bit 14; the other nine have none.
The four bytes at 0x0a are not offsets and remain unknown (see "Layout").

## Open questions

- The suffix code in `mgavideo.dvr`'s name builder can't be reached as the module sits
  on disk. Short jumps that run before it go straight to the exit, and nothing in the
  module patches them. Who reaches it (the host patching the module, another entry
  point, or nothing, which would mean these files are not loaded this way) is T-029.
  The same code only handles ids up to 0x90, which leaves out `obst` (0x92 to 0x98).
- Whether the game always loads all four files of a model, or chooses how many
  facings to load (memory or speed).
- Which facing is 0 (facing the viewer, or north on the map), and which way the
  facings turn.
- What the game draws for a facing whose frames have no parts (the nearest facing
  that has some, presumably).
- What the animations are, how they combine, and what the frame's `unknown_01`
  means (a duration or a height?).
- Whether colour 0 inside a span is drawn, and the draw order of parts (the dump
  draws them in list order, later on top).
- The palette: models carry none, so the mission screen's palette presumably applies.
- The no-facing view's two unknown words.

## Parser

`xeno_formats::mxx::parse`, added by T-015 and extended by T-016. Each `View` holds
its header, its animations (`Vec<Vec<Frame>>`) for `facing`, those for
`mirror_facing` when they are separate, and its images as row spans
(`Image::to_indexed` expands them). It rejects anything that leaves a byte of a view
unaccounted for. `xeno-tool inspect` lists the views and counts images, animations and
frames. `xeno-tool dump` writes each frame of each facing as a PNG with a transparent
background (`<model>_f<facing>_a<animation>_<frame>.png`), reading the model's `.m00`
from the same directory for bit-14 parts, and each `.m00` image on its own.
