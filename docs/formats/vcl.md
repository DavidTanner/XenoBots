# `.vcl` cutscene pictures

## Status

**Confirmed** (T-018). `xeno_formats::vcl` reads all 14 files of a stock install. Each
one decodes to its last byte and gives a picture that looks right in its own palette.
The loader in `vga240.mod` has been read and matches the layout below.

A `.vcl` is **not** an animation. It holds one still picture and no frames, deltas or
timing. The cutscenes move by panning a 320x240 window over the picture and through
palette effects and captions, all driven by 26-byte sequence entries in `xeno.exe`
(see "How the game shows a picture" and T-027).

## Files

| Rows | Files |
|---|---|
| 240 | `ealogo`, `presents`, `detect`, `war`, `annihil`, `triumph`, `win`, `nlose`, `slose` |
| 400 | `arrival`, `grndfall`, `emerge`, `aftermth`, `return` |

Sizes run from 5 KB (`ealogo`) to 133 KB (`nlose`). The files are not packed (they
have no `@` in the extension; `packed.md`).

`mgavideo.dvr`'s file-id table (`m@l.md`) gives the `.VCL` names ids 0x16 to 0x32, in
this order: `WIN`, `HLOSE`, `NLOSE`, `SLOSE`, `TRIUMPH`, `EALOGO`, `PRESENTS`,
`DETECT`, `ARRIVAL`, `GRNDFALL`, `EMERGE`, `WAR`, `ANNIHIL`, `AFTERMTH`, `RETURN`.
`HLOSE` (id 0x18) is not in a stock install, and the game never asks for it (see
below).

## Layout

| Offset | Size | Meaning |
|---|---|---|
| 0x000 | 768 | Palette: 256 `r, g, b` VGA DAC triples, 0..=63 each (`palette.md`) |
| 0x300 | to the end | The picture: PackBits, 640 pixels per row from the top, until the file ends |

There is no header and no stored size. The width is fixed by the display mode (a
640-pixel virtual line) and the height follows from the file length: the stream
expands to 153,600 bytes (240 rows) or 256,000 bytes (400 rows) in every stock file.
640x400 is all 256 KB of VGA memory, which is why `xeno_formats::vcl` refuses more
than 400 rows.

## Encoding

The same PackBits as `.m@l` (`m@l.md`): a control byte `n` of `0x00..=0x7f` copies the
next `n + 1` bytes, and `0x80..=0xff` repeats the next byte `257 - n` times. `0x80`
(129 repeats) is accepted by the loader but used by no stock file.

The encoder worked row by row: no run in any stock file crosses from one 640-pixel row
into the next. That is also the best evidence for the width from the data alone.

## How the game loads a picture

`vga240.mod`'s loader is at module offset 0x274. It is not in the module's first export
table. On entering the 320x240 mode (the routine slot `0x5f4` is repointed to), the
module hands a second table at offset 0x22 to a host installer (slot `0x26`). The
installer points **host slot `0x490`** at the loader, and slot `0x590` at offset 0xb8d,
next to the 1-bit glyph (caption text) renderer at ~0xc0a. `mgavideo.dvr` also exports slot
`0x490` (load offset 0x31d), but its version only builds the file name and returns, so
outside the 320x240 mode a `.vcl` is never drawn.

The loader:

1. Takes a file id in `AX` and a descriptor at `ES:BX` whose first word is the segment
   of a 768-byte palette buffer. It sets flags `0x22` in the descriptor's byte 2.
2. Builds the name through host slot `0x480` (the file-id name builder), opens the
   file (slot `0x2b4`) and reads the 768-byte palette into that buffer (slot `0x2bc`).
   It does not write the DAC itself.
3. Reads the rest through a 2 KB buffer, refilled as it empties, and decodes PackBits
   straight into video memory at `A000:0000`. It writes pixel by pixel, setting the
   sequencer map mask to 1, 2, 4, 8 in turn and stepping the address after every fourth
   pixel. So the stream is in plain left-to-right order and the planes are interleaved
   by the loader, not by the file.
4. Stops when the file runs out, with no length check, and closes the file (slot
   `0x2b8`).

## How the game shows a picture

`xeno.exe`'s sequence player (unpacked file offset ~0x19a24) takes one 26-byte entry.
What it does with the fields so far:

| Offset | Size | Use |
|---|---|---|
| 0x00 | u16 | `.vcl` file id, passed to slot `0x490` |
| 0x02, 0x04 | u16, u16 | `x`, `y` origin of the 320x240 window, passed to slot `0x60e` (`video-modes.md`) |
| 0x06 | u16 | Index into a near jump table in the player's code segment; the handler runs after the load. Presumably the effect (scroll, fade) |
| 0x0a | u16 | Pointer to caption text, drawn one character at a time through slot `0x594`; byte 0 ends it and 9 is handled apart |
| 0x0c, 0x0e | u16, u16 | Passed to a routine just before the text is drawn; presumably the caption position |
| 0x12, 0x16, 0x18 | u16 | Copied into variables of the player and the host; meaning unknown |

After the load the player passes the palette buffer's segment to host slot `0x4d8`.
`mgavideo.dvr`'s routine there runs a colour ramp over indices 0..=255, which by its
shape fades the picture's palette in (guess).

Every call of the player is between an "enter 320x240" (slot `0x5f4`) and a "leave"
(slot `0x602`), so every `.vcl` is shown in the 320x240 mode with square pixels. The
entries live in `xeno.exe`'s data segment (file offset `0x19c10` plus the `DS` offset):

- **Intro** (`xeno.exe` ~0x1091d): ten entries at `DS:0xecac`, played in order:
  `EALOGO`, `DETECT`, `ARRIVAL`, `GRNDFALL`, `EMERGE`, `WAR`, `ANNIHIL`, `AFTERMTH`,
  `RETURN`, `PRESENTS`. Any of them can end the chain early (carry set).
- **`TRIUMPH`** (~0x109b2): one entry at `DS:0xedb0`.
- **Win and lose** (~0x109f0, with the entry chosen by its caller): `WIN` from
  `DS:0x606a` (caller ~0x7cbd). A lose entry comes from a three-word pointer table at
  `DS:0x6016`, indexed by a word at `DS:0x6012`: `SLOSE`, `NLOSE`, `SLOSE`. A fourth
  entry for `HLOSE` sits at `DS:0x601c`, but no code or data in `xeno.exe` refers to
  it, and the table's two `SLOSE` slots suggest `HLOSE` was cut and its slot pointed
  at `SLOSE` instead. So a stock install never asks for `hlose.vcl`, and the port
  doesn't need it.

## Open questions

- The remaining entry fields, the effect handlers behind 0x06, and the timing of each
  sequence (T-027). Confirm the panning and fades against a DOSBox capture (T-036).
- What the word at `DS:0x6012` that picks the lose cutscene means (a side or robot
  type, by the names).
- Whether slot `0x4d8` really fades in, and over how many retraces (T-037).
- The flags the loader sets in the descriptor (`0x22`), and what slot `0x590`
  (offset 0xb8d) does.

## Parser

`xeno_formats::vcl` (T-013 for the palette, T-018 for the picture): `parse` returns a
`Picture` with the palette, the row count and the pixels, and `runs` the PackBits runs.
`xeno-tool inspect` prints the size and run counts, and `xeno-tool dump` writes the
whole picture as one PNG.
