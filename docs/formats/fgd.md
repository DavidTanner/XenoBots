# `.fgd` foreground layers and `fgarray.kyl`

## Status

**Confirmed** (T-021) for the layout and how the five `.fgd` files are drawn, from the
code in `vgaintro.dvr` that loads and draws them. `xeno_formats::fgd` reads all six
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
0x200-byte MZ header), as Ghidra shows them in segment `1000`.

- **Loading.** A helper (`0x2f17`) takes the file name inline after its call, opens it
  and reads it into the driver's own segment. There are three 1920-byte buffers, back
  to back: A at `0x0a8e`, B at `0x120e`, C at `0x198e`. The intro drop loads
  `LAYER3`/`LAYER4` into A/B with `XENODROP.PCX` (`0x2af1`), and later
  `LAYER1`/`LAYER2`/`LAYER5` into A/B/C with `CAVEDROP.PCX` (`0x2c47`).
- **Scrolling.** Each buffer has a scroll offset (`0x210e`, `0x2111`, `0x2114`), a
  fraction byte after it, and a speed in 1/256 column per frame (`0x2117`, `0x2119`,
  `0x211b`). All three offsets start at 1152 (`1920 - 768`, set at `0x3a50`) and the
  step routine (`0x3cf9`) *subtracts* the speed, so the window moves from the end of
  the file towards its start. Nothing clamps the offset.
- **Window.** 768 columns from the offset are drawn, the width of the backdrop and of
  the driver's 768-pixel virtual screen (`video-modes.md`).
- **Floor.** `0x3d63` takes, per column, the smaller (higher) of A's and B's rows, so
  the nearer ridge hides the farther one, and keeps the previous frame's result.
  `0x3da1` then redraws only the difference: colour 0 from the new row down to the old
  one where the floor rose, and pixels copied back from a clean copy of the backdrop
  (128 rows further on in video memory) where it fell. The first draw starts from an
  old row of 127 everywhere (`0x3a62`).
- **Ceiling.** `0x3a80` draws C once, colour 0 from each column's row up to row 0.
  `0x3e16` then redraws the difference each frame, against C at the previous offset.
  Only the cave sequence draws it.
- **Speeds.** Both drop loops run one step per frame and wait for at least two
  ticks of the host's timer counter between frames. A frame-counted script (counter
  `0x0a7c`, state table at `0x0a82`, stepped by `0x3b6d` and `0x3b7f`) speeds the
  layers up for 16 frames, holds, then slows to a stop. A and C reach 0x290 (about
  2.56 columns per frame) and B 0x1f0 (about 1.94), so B is the far layer: classic parallax. B speeds up
  to 0x3c0 at frame 0x291. At frame 0x1c0 the script raises the flag (`0x047b`) that
  ends the `XENODROP` part; by our reading that leaves the window near offset 0, so
  the xenodrop layers are used almost end to end. The cave part restarts the offsets
  at 1152 but not the frame count, so it stops earlier and does not reach the start of
  its layers (see open questions).
- The draw routines write to segment `0xa360`, so the backdrop's row 0 sits 72
  rows (0x3600 bytes) into the 768-wide virtual screen.

## `fgarray.kyl`

No binary in the install names it, packed or unpacked (`xeno.exe`, all six `.dvr`
files, `vga240.mod`). Its layout and range match the layers, and it starts like
`layer3.fgd` (the same run of row-0 columns, then a similar rise) and matches it in
about a fifth of its bytes, so it looks like an earlier draft of the xenodrop near
floor. The extension is likely the author's initials: `sounddvr.dvr` carries a Kyle
Freeman copyright and `vgaintro.dvr` credits him.

## Open questions

- The exact end positions: when each drop part stops and how far the cave layers
  scroll, and the length of a frame in time (T-038). By our estimate the cave
  scroll stops near offset 500 to 650, so roughly the first quarter to third of
  `layer1`/`layer2`/`layer5` is never on screen.
- Whether colour 0 is black in both backdrop palettes (it is drawn as the silhouette).
- What rows past 127 do: the clean copy starts 128 rows below the backdrop, so a
  floor edge at 128..130 (and the last rows of the 130-row `xenodrop.pcx`) fall in
  the copy's area.

## Parser

`xeno_formats::fgd` (T-021): `parse` returns a `Layer` (`binrw`) of 1920 rows,
`Layer::window` gives the 768 columns at a scroll offset, `Layer::floor` combines two
floor layers as the game does, and `Role::of` names a file's role and backdrop.
`xeno-tool inspect` prints a layer's row range and role.
