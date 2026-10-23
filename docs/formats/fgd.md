# `.fgd` foreground layers and `fgarray.kyl`

## Status

**Confirmed** (T-021) for the layout and how the five `.fgd` files are drawn, from the
code in `vgaintro.dvr` that loads and draws them; the drop sequence's timing and end
positions traced in T-038. `xeno_formats::fgd` reads all six
files. `fgarray.kyl` has the same layout but nothing loads it.

The files are **not grids**: neither 80x24 nor 48x40. Comparing each byte with the
byte `w` places later gives the smallest difference at `w = 1` and grows steadily with
`w`, with no dip at 24, 40, 48 or 80. Each file is one smooth profile of 1920 columns.

## Files

| File | Size | Used for |
|---|---|---|
| `layer3.fgd` | 1920 | Near floor over `xenodrop.pcx` |
| `layer4.fgd` | 1920 | Far floor over `xenodrop.pcx` |
| `layer1.fgd` | 1920 | Near floor over `cavedrop.pcx` |
| `layer2.fgd` | 1920 | Far floor over `cavedrop.pcx` |
| `layer5.fgd` | 1920 | Cave ceiling over `cavedrop.pcx` |
| `fgarray.kyl` | 1920 | Nothing: no binary names it (see below) |

Not packed (no `@` in the extension).

## Layout

| Offset | Size | Type | Meaning |
|---|---|---|---|
| 0x000 | 1920 | u8 each | For column `x`, the backdrop row where the silhouette's edge is |

Rows count from the top of the drop backdrop (128 or 130 rows; `pcx.md`). Stock values
run from 0 to 130. A floor fills its column in colour 0 from that row down; a
ceiling fills it from row 0 down to that row, both inclusive. Whether a file is a
floor or a ceiling depends only on which buffer the driver loads it into.

## How the game uses them

All addresses are offsets in `vgaintro.dvr`'s load image (file offset minus its
0x200-byte MZ header), as Ghidra shows them in segment `1000`. The whole drop sequence
is described under "The drop sequence" below (T-038).

- **Loading.** A helper (`file_load_inline_name`, `0x2f17`) takes the file name inline
  after its call, opens it and reads it into the driver's own segment. There are three
  1920-byte buffers, back to back: A at `0x0a8e`, B at `0x120e`, C at `0x198e`. The
  first part loads `LAYER3`/`LAYER4` into A/B with `XENODROP.PCX` (`0x2af1`), the cave
  part `LAYER1`/`LAYER2`/`LAYER5` into A/B/C with `CAVEDROP.PCX` (`0x2c47`).
- **Scrolling** (`gfx_scroll_layers_and_view`, `0x3cf9`). Each buffer has a scroll
  offset (`0x210e`, `0x2111`, `0x2114`) with a fraction byte after it, and a speed in
  1/256 column per frame (`0x2117`, `0x2119`, `0x211b`); a fourth speed (`0x211d`,
  fraction `0x472`) moves the view (below). Each step adds the speed to the fraction
  and subtracts the carry from the offset, so the window moves from the end of the
  file towards its start. The offsets start at 1152 (`1920 - 768`) in each part
  (`gfx_reset_layers_and_floor`, `0x3a4d`); the fractions start at 0 and are not reset
  between the parts. Nothing clamps an offset.
- **Window.** 768 columns from the offset are drawn, the width of the backdrop and of
  the driver's 768-pixel virtual screen (`video-modes.md`).
- **Floor** (`gfx_combine_floor_layers`, `0x3d63`; `gfx_draw_floor_changes`, `0x3da1`).
  Per column, the smaller (higher) of A's and B's rows, so the nearer ridge hides the
  farther one; the previous frame's result is kept. Only the difference is drawn: where
  the floor rose, colour 0 on rows `new..=old`; where it fell, rows `old..=new` copied
  back from the clean copy of the backdrop, 128 rows further down video memory. The
  first draw of each part starts from an old row of 127 everywhere.
- **Ceiling** (`gfx_draw_ceiling_full`, `0x3a80`; `gfx_draw_ceiling_changes`,
  `0x3e16`). C is drawn once per cave part, colour 0 on rows `0..=row`, then each frame
  the difference against C at the previous offset: lowered, colour 0 on rows
  `old..=new`; raised, rows `new..=old` copied back from the clean copy.
- **The edge row after a retreat.** Both restore loops include the new edge row
  itself, so where a silhouette has just retreated, its edge row shows the backdrop,
  not colour 0, until the edge moves again. A full redraw from the rows would not do
  that; the port draws the differences the same way.
- The draw routines address video memory from segment `0xa360`, so the backdrop's row
  0 sits 72 rows (0x3600 bytes) into the 768-wide virtual screen, and its clean copy at
  row 200.

## The drop sequence (T-038)

`xeno.exe` runs it once at start-up, right after the intro cutscene unless that was
aborted (`drv_run_vgaintro`, `1000:fc71`, called from `1000:fb2d`): load
`VGAINTRO.DVR`, blank the palette (slot `0x4b4`), enter (slot `0x1538`), run (`0x1540`),
leave (`0x153c`), unload. The music (`MAINTHEM`) is still playing, and the retrace
counter `DS:0x370`, reset when it started, keeps counting through the drop.

### Screen

`gfx_enter_drop_mode` (`0x2a52`) keeps mode 13h's timing (320x200 at 70.09 Hz) and
makes video memory planar with a 768-pixel line (`video-modes.md`, mode A'), has
`vga240.mod` build a row table for width 768 (slots `0x606`, `0x612`), clears the top
72 rows, and sets the CRTC **line compare** so the screen splits after row 128
(`gfx_set_line_compare`, `0x3ea8`, value `2 * 128 - 1` scan lines), with pixel
panning switched off below the split (attribute mode bit 5). So:

- **Rows 0..=127** of the screen show 320 columns of the 768-wide screen from (`x`,
  72), where the backdrop is: the drop proper. `x` starts at 448 (the backdrop's right
  end) and is set through slot `0x60e` once a frame (`gfx_set_drop_origin`, `0x39eb`),
  which moves the CRTC start address and the pixel panning. So the backdrop scrolls in
  hardware; the silhouettes are redrawn.
- **Rows 128..=199** show video memory from address 0, the 72 rows above the backdrop,
  never scrolled: the **credits area**, black but for the credits text.
- Video memory rows 200 and below hold the clean copy, never on screen.

The backdrop is loaded straight into video memory (`file_load_pcx_to_video`, `0x2d43`,
with the destination 0x3600 inline before the name): every pixel goes both to its row
and 128 rows lower (`gfx_put_pixel_and_clean_copy`, `0x3f1c`), in order, and the
palette trailer, shifted to 6 bits, to the driver's palette buffer (`0x16a`). Only
backdrop rows 0..=127 are ever on screen. `xenodrop.pcx`'s rows 128 and 129 land on
rows 200 and 201, over the clean copy of its rows 0 and 1, so where the floor retreats
from rows 0 or 1 the backdrop's bottom rows are copied there instead. Floor edges of
128..=130 (the stock layers have some) only draw below the split, on the clean copy's
first rows: invisible, but they can change what a later retreat copies back.

### Steps

`gfx_run_intro_drop` (`0x2ab4`) selects its own colour-animation list (slot `0x30e`;
the list is at load offset 0x13090, `palette.md`) and puts the previous one back at the
end. In order, with the stock numbers:

1. View at (448, 72); load `LAYER3`, `LAYER4` and `XENODROP.PCX`; draw the floor; the
   backdrop's palette into the shadow, marked for upload. It is nearly all black.
2. Wait until the counter reaches 7,648 (already past when the intro ran to its end,
   about 7,690), then **blend 1**: record 0 started with weights 0, 2, ..., 128, one a
   retrace (65 retraces): a cross-fade from the shadow palette to the record's colours
   0..=251. Then its first 192 colours go into the palette buffer and the shadow
   (without an upload), the copyright line is drawn in the credits area, and the
   credits colour (record 5) is set to full.
3. Wait for 7,936, **blend 2** (record 1), copy its first 192 colours likewise; wait for
   8,192, **blend 3** (record 2); the credits colour to 0.
4. Wait for 8,352, then the **first part's frames** until the frame script ends it.
5. Load `LAYER1`, `LAYER2`, `LAYER5` and `CAVEDROP.PCX`; offsets back to 1152, view back
   to (448, 72); draw the floor and the ceiling; the cave's palette into the shadow,
   marked: it shows at the next retrace, with no fade.
6. The **cave part's frames** until the counter reaches 10,768.
7. **Blend 4** (record 3, apparently black: a fade out), then return.

`gfx_leave_drop_mode` (`0x2a90`) then sets the width back to 320 and BIOS mode 13h.

### Frames and pacing

A frame (`0x2c16` and `0x2cc9`): note the counter; poll the abort hook; the frame
script (`gfx_step_drop_frame`, `0x3b6d`: count the frame, print it on the debug
screen through host word slots `0x22a`/`0x240`, which a stock set-up never shows, run
the current state, set the view); in the cave part, the ceiling; the credits script;
then wait for the counter to move and to be **at least 2 past** the noted value. So a
frame lasts two retraces (35 frames a second), longer only if its drawing took more
than two retraces. It is the retrace counter, not the 60 Hz tick.

The frame count (`0x0a7c`) starts at 0 before the first part and is **not** reset
for the cave part, nor is the script's state.

### The frame script

States, from the table at `0x0a82` (`gfx_select_drop_state`, `0x3cea`), each run with
the frame number `f` (from 1). Speeds are A, B, C and the view.

| State | Handler | What it does |
|---|---|---|
| 0, accelerate | `0x3b91` | Until `f` = 16, add 0x29, 0x1f, 0x29, 0x10 to the speeds, then step. At 16, set them to 0x290, 0x1f0, 0x290, 0x100 and go on as state 1 in the same frame |
| 1, cruise | `0x3bdd` | At 685, go on as state 2 in the same frame. At 448, end the first part (no step). At 602, move the view 180 columns left and change the light (below). At 657, B's speed becomes 0x3c0. Then step |
| 2, slow | `0x3c30` | Until 701, take 0x29, 0x3c, 0x29, 0x10 off the speeds and step. At 701, all speeds 0 (no step) |
| 3, 4 | `0x3c7e`, `0x3c98` | Step (at speed 0) until 906, then until 960 |
| 5 | `0x3cb2` | Raise the end flag (`0x047b`), which only the first part looks at |

Every step also redraws the floor. So in a frame the near layers (A, and C in the cave)
move 2.56 columns across video memory, the far one (B) 1.94, and the view 1 column the
other way: on screen the backdrop drifts right at 1 pixel a frame, the far ridge at
2.94 and the near one at 3.56. The parallax ratio of the layers to the backdrop is
0x290 : 0x1f0 : 0x100, about 2.6 : 1.9 : 1.

### Where each part starts and stops

- **First part** (frames 1..=448, about 12.8 s at 35 frames a second): accelerates for
  15 frames and cruises. Frame 448 only raises the flag, so the last step is frame 447.
  It ends with A at offset 26, B at 301 and the view at x = 9: from the right end of
  the backdrop almost to its left end, and through `layer3` from its last column to
  column 35 (`layer4`: 1,600 down to 310).
- **Cave part** (from frame 449, with the frame count running on): still cruising, so
  it starts at full speed with no acceleration. At frame 602 the view jumps 180
  columns (from 295 to 115) and the light changes; at 657 the far layer speeds up;
  frames 685..=700 slow down; from 701 nothing moves. It stops with A and C at offset
  528, B at 616 and the view at x = 24. **The first 552 columns of `layer1` and
  `layer5`, and 640 of `layer2`, are never on screen** (the window starts at
  `offset + x`). The script reaches its end state at frame 960, but nothing watches
  the flag in this part.
- The cave part ends on the **music's clock**, at counter 10,768: with frames of two
  retraces and no time lost to loading, that is frame 1,208, about 22 s after the
  first part ended, so the scene stands still for the last 14 s. On a slower machine
  the first part takes longer (448 frames, however long each takes), and the cave part
  is shorter to keep the end in time with the music.
- **Hand-over.** Between the parts nothing fades: the cave's files load over the
  first part's last picture, and its palette is uploaded at once.

### Palette

The palette never fades through the driver's fade routines; it changes by blends of
the drop's own colour-animation records (`palette.md`, "The lists in the stock
drivers"): records 0, 1 and 2 before the frames (steps 2 and 3), record 3 at the
end, all over colours 0..=251, each started at the next weight once a retrace for 65
retraces. At frame 602 of the cave, record 4 is started at full weight (a one-step
change of light, shown at the next retrace) and 255 colours from its list bytes go into
the palette buffer and the shadow (`pal_switch_to_cave_light`, `0x3cb9`). Record 5 is the
credits colour (colours 252..=254, kind 4, blended between its two frames).

Colour 0, the silhouettes' colour, is black in both backdrops' palettes and in every
blend record's target, so the silhouettes are black throughout.

### Credits

The driver's data segment (paragraph 0x41a) holds a credits script that runs once a
frame (`ui_step_credits_script`, `0x3af7`) from its offset 0x18: pairs of a handler and
an argument. Handlers: `0x3b64` clear the credits area, `0x3b11` wait until frame `n`,
`0x3b05` draw a text list, `0x3b29` raise the credits colour's weight by 4 a frame to
128 (32 frames), `0x3b49` lower it by 4 to 0. The stock script draws seven lists in
turn, each faded in, held and faded out, and ends with a wait for frame 0xffff. A text
list is a count, then per string `x`, `y`, a font (6) and a colour (252) and the
string's offset; the host's text services draw it (slots `0x396`, `0x38a`, `0x39e`,
`0x3ba`) into the credits area. The copyright line drawn in step 2 is a list of its
own.

### Input

The abort hook (`DS:0x154c`, the intro's `1000:fccf`, `vcl.md`) is polled before each
frame and after each blend step, not during the waits for the counter (a key pressed
then is acted on at the next poll). P pauses until the next key (the counter, and so
the music's clock, runs on: a pause in the cave part shortens it). Any other key ends
the drop at once, with no fade: the run returns, and the leave routine sets mode 13h.
What follows, aborted or not, is the rest of start-up: digitised sound, `dsounds.bin`,
the models, then the log-in screen.

### Reading it from the user's copy

`xeno_formats::vgaintro` reads every number above from the instruction that holds it,
at the load-image offsets in its `NUMBERS` table, after checking the instruction's
bytes, so another build of the driver is refused rather than misread. It also checks
the state table, and reads the colour-animation list (from the paragraph in the `mov`
before slot `0x30e`), the credits script and the text lists. `xeno-tool inspect
vgaintro.dvr` prints a summary.

## `fgarray.kyl`

No binary in the install names it, packed or unpacked (`xeno.exe`, all six `.dvr`
files, `vga240.mod`). Its layout and range match the layers, and it starts like
`layer3.fgd` (the same run of row-0 columns, then a similar rise) and matches it in
about a fifth of its bytes, so it looks like an earlier draft of the xenodrop near
floor. The extension is likely the author's initials: `sounddvr.dvr` carries a Kyle
Freeman copyright and `vgaintro.dvr` credits him.

## Open questions

- Settled (T-038): the end positions, the frame length and the parts' timing (above),
  colour 0 (black in both palettes), and rows 128..=130 (never on screen; see
  "Screen"). Still to do: compare with the running game in DOSBox (`NEXT_STEPS.md`,
  T-038's note).
- The credits text needs the game's font (T-041).

## Parser

`xeno_formats::fgd` (T-021): `parse` returns a `Layer` (`binrw`) of 1920 rows,
`Layer::window` gives the 768 columns at a scroll offset, `Layer::floor` combines two
floor layers as the game does, and `Role::of` names a file's role and backdrop.
`xeno-tool inspect` prints a layer's row range and role. `xeno_formats::vgaintro`
(T-038) reads the drop's numbers and lists from `vgaintro.dvr`.
