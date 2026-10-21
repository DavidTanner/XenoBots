# `.m00` / `.m08` / `.m16` / `.m32` unit models (`.MXX`)

## Status

**Partial** (T-015). The suffixes and the view header are confirmed from the data:
`xeno_formats::mxx` reads all 40 files of a stock install, every view header passes
the checks below, and every file holds exactly the facings its suffix predicts. The
two-level index inside each view is consistent in all 310 index tables. The meaning
of the lists and the pixel encoding are open (T-016). The code that loads and draws
models has not been traced (T-029).

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
  shared data. So the mirror facing gets its own placement even if its pixels are the
  same rows drawn flipped (guess).
- `index_offset` is 16 in every view.
- In the no-facing views, the four bytes at 0x0a to 0x0d are often one value
  repeated. In `basedome.m00` that value is the main shade of the dome's pixels in
  `basedome.m08`. They may be colours for a distant or map marker (guess).

## Inside a view (for T-016)

The body is not decoded yet. What the data shows so far:

- The **index** at `index_offset` has two levels of u16 offsets. The first word of the
  first level is where the second level starts, so the first level has
  `(first word - index_offset) / 2` entries. Each first-level entry points into the
  second level, and each second-level entry points into the lists area
  (`lists_offset` up to the next table or `data_offset`). All 310 index tables in a
  stock install satisfy these bounds.
- First-level entry counts: 1 for most models, 5 for `humanoid`, 6 for `netrelay`, 7
  for `scout`, 12 for `scorpion`. The second level has 1 to 32 entries per first-level
  entry (`explode` has 32, `cloak` 8, `plasma` 4). This looks like **animation
  sequences, then frames** (guess).
- The lists area holds short records that appear to be a count followed by
  (image number, signed x offset, signed y offset) entries. This does not hold for every
  list, so it is a guess.
- The data area is 8-bit pixels in row spans (a left-skip byte, a count byte, then
  `count` pixels), like the `.b@n` row-span sprites, preceded by a few words that
  are not understood yet.

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
- The list records, the words in front of the pixel rows, colour 0 transparency, and
  the meaning of the no-facing view's two unknown words (T-016).

## Parser

`xeno_formats::mxx::parse`, added by T-015. It checks every view header and keeps
each view's body as bytes. `xeno-tool inspect` lists the views as `facing/mirror`
pairs.
