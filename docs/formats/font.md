# Fonts

## Status

**Confirmed** (T-041) from the code that selects and draws them: the host's text
services in `xeno.exe` (`1fe6:489c`–`4cbc`, slots `0x38a`–`0x40e`), `mgavideo.dvr`'s
glyph renderers (slots `0x590`–`0x598`) and `vga240.mod`'s planar versions.
`xeno_formats::font` reads all four stock fonts, and `xeno_render`'s
`Framebuffer::draw_text` draws them. Not yet compared with a DOSBox capture.

## Files

The fonts are not a file. They live in `mgavideo.dvr`'s data: the fifth of the five
data blocks its init copies into memory blocks of their own (descriptor `DS:0x62a`,
0x1c0c bytes in the stock driver). `vga240.mod` has none of its own; its renderers
read the same block through `DS:0x62a`. Nothing else in the install holds a font.

| Id | Kind | Cell | Codes | Used for |
|---|---|---|---|---|
| 0 | 7, bitmap | 6 wide, 6 high | 128 | The log-in name (`docs/screens.md`); the host's default font |
| 2 | 7, bitmap | 4 wide, 6 high | 128 | Not traced |
| 4 | 0, shaded | 6 high, proportional | 128 | Not traced |
| 6 | 0, shaded | 9 high, proportional | 97 | Cutscene captions (`vcl.md`) and the intro drop's credits (`fgd.md`), in colour 252 |

The two bitmap fonts map lower case to the upper-case glyphs. Font 6 has no lower case
at all: its table stops after code 0x60.

## Layout

All words are u16 little-endian, offsets from the start of the block.

**The block** starts with four words: the offsets of fonts 0, 2, 4 and 6. A font id
is a byte offset into this table: the host's font select (slot `0x38a`, `AX` = id)
turns an odd id or one of 8 or more into font 0 and returns carry.

**A font**:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 1 | `kind` | Picks the renderer: 0 and 1 shaded, 7 bitmap, 2 to 6 a `retf` (nothing drawn, cursor not moved) |
| 0x01 | 1 | `unknown_01` | Not read by the font select; 0 in every stock font |
| 0x02 | 1 | `width` | The cell width: a bitmap glyph's advance, and how far a backspace moves back |
| 0x03 | 1 | `height` | The line height. A glyph has `height - 1` rows; the last row of a line is always empty |
| 0x04 | 1 | `step_x` | Extra pixels after every glyph (0 in every stock font) |
| 0x05 | 1 | `step_y` | Pixels every glyph moves the cursor down (0 in every stock font) |
| 0x06 | 2 each | glyph table | Glyph offsets by character code, up to where the first glyph starts (128 entries, or 97 for font 6) |

A character code indexes the table with bit 7 masked off (the renderers double the
code in a byte register): codes 0x80 and up draw the same glyph as the code without
bit 7, in the second colour pair (below). A code past the table's end reads glyph data
as an offset; the port draws nothing for it.

**A glyph** starts with one byte. 0 makes the code a control code (below). Otherwise:

- **Bitmap** (kind 7): the byte is non-zero (the cell width in the stock fonts; only
  the host's text measuring reads it). Then `height - 1` bytes, one per row, top row
  first, the leftmost pixel in bit 7. A set bit is the colour.
- **Shaded** (kinds 0 and 1): the byte is the glyph's width `w` in pixels. Then
  `w * (height - 1)` bytes, row by row, one per pixel: 0 transparent, and 1, 2 or 3 a
  shade drawn as `colour + (value XOR 3)`. So 3 is the colour itself, 2 the next colour
  and 1 the one after: font 6 in colour 252 uses colours 252, 253 and 254, exactly the
  colours `vgaintro.dvr`'s credits record blends (`palette.md`).

Many codes share a glyph (all control codes one empty glyph, lower case the upper
case's). In fonts 2 and 6 code 9 (tab) shares the space's glyph.

## Text state

The host keeps one text state, pointed to by `DS:0x644` (the text surface chosen with
`mgavideo.dvr`'s slot `0x488`). Fields that matter here:

| Offset | Set by | Meaning |
|---|---|---|
| +0x00 | slot `0x488` | Surface segment (`0xa000` for the screen; then the mouse pointer is hidden around each glyph) |
| +0x04 | `0x38a` | Font id |
| +0x06, +0x08, +0x0a, +0x0c | `0x38a` | Glyph table offset, `kind`, `width`, `height` |
| +0x0e, +0x10 | `0x38a` (to 0), `1fe6:48f1` | Extra x after each glyph, extra y after each line |
| +0x12, +0x14 | `0x38a` (`step_x`, `step_y`), `0x38e` | Per-glyph x and y steps |
| +0x16 | `0x39e` | Colour (low byte) and background (high byte) |
| +0x18 | `0x3a6` | The same for codes with bit 7 set |
| +0x1a | `0x396` | Left margin: the cursor's x when it was last set |
| +0x1c, +0x1e | `0x396` (`AX` = y, `BX` = x), `0x39a` reads them | Cursor: the next glyph's top-left corner |
| +0x20, +0x22 | `0x396` and every glyph | The cursor plus the cell's height and width (a box for hiding the pointer) |

At start-up (`1fe6:4872`) the host selects font 0, puts the cursor at (0, 0) and sets
the colours to 12 on 0 and 255 on 0.

## Drawing

A glyph is drawn with its top-left corner at the cursor, through the row table of the
current surface (stride 320 in mode 13h; `vga240.mod`'s row tables, 640 or 768 pixels
wide, in the unchained modes). Nothing is clipped. Then the cursor moves:

| Style | Renderer | Pixels | Then |
|---|---|---|---|
| Bitmap | slot `0x594` (`gfx_draw_glyph_1bit`, `mgavideo.dvr` 0x178a; planar `0x0bd8` in `vga240.mod`) | Set bits in the colour, up to the last set bit of each row | `x += width + extra_x + step_x`, `y += step_y` |
| Bitmap, opaque | slot `0x598` (`gfx_draw_glyph_1bit_opaque`, 0x1938; none in `vga240.mod`) | `width` pixels a row: the colour on the background | The same |
| Shaded | slot `0x594` (`gfx_draw_glyph_shaded`, 0x1aff; planar `0x0f19`) | Non-zero bytes as `colour + (value XOR 3)` | `x += w + 1 + extra_x + step_x`, `y += step_y` |
| Shaded, opaque | slot `0x598` (`gfx_draw_glyph_shaded_opaque`, 0x1c69; none in `vga240.mod`) | Zero bytes in the background, and one more background column after the glyph | The same |

Slot `0x590` (`gfx_select_glyph_renderer`, 0x173f; `vga240.mod` 0x0b8d) points slots
`0x594` and `0x598` at the renderers for the font's kind; the font select calls it. In
the 320x240 mode slot `0x598` is a `retf` for every kind.

**Control codes** (a glyph whose first byte is 0), by code with bit 7 masked off:

| Code | Effect |
|---|---|
| 0x07 | Calls the host's word slot `0x26c` (presumably a bell; not traced) |
| 0x08 | Back one cell: `x -= width + extra_x + step_x`, `y -= step_y` |
| 0x0a | Down one line: `y += height + extra_y + step_y` |
| 0x0c | New line: `x` back to the left margin, and down one line |
| 0x0d | `x` back to the left margin |
| 0x0e, 0x0f | `x` one pixel right, left |
| Others | Nothing |

**Strings** (slots `0x3b2`/`0x3b6` at `DS:SI`, `0x3ba`/`0x3be` at `ES:SI`, `0x3c2`/`0x3c6`
in the host's string segment, Ghidra `38fa`; the first of each pair transparent, the
second opaque) are drawn code by code up to a NUL, or up to a `$` followed by a NUL,
whose `$` is not drawn. Slot `0x3ae` draws the two codes in `AH` then `AL`; the slots
from `0x3ca` print numbers (decimal and hexadecimal, through the glyph slot) and, with
slot `0x406`, into a buffer instead of the screen.

The host's character width (`1fe6:497c`) and string width (`1fe6:499c`) add each
glyph's first byte plus one, for every kind, so for a bitmap font they measure one
pixel a character more than is drawn.

### Who draws what

- **Captions** (`vcl.md`, step 5): font 6, colour 252, transparent, one code at a time
  through slot `0x594`; the player handles code 9 itself (the next byte moves the cursor
  that many pixels right), so the font's own tab glyph is never drawn there.
- **The intro drop's credits** (`fgd.md`, "Credits"): each text-list item sets the
  cursor (`x`, `y`), the font and the colour, then draws its string through slot
  `0x3ba`, transparent, on the 768-wide surface (`vgaintro.dvr`'s enter routine sets
  `vga240.mod`'s row tables to 768 pixels).
- **The log-in name** (`docs/screens.md`, "Log-in"): font 0, opaque.

## In the port

`xeno_formats::mgavideo` finds the font block from the instructions that copy it in
`mgavideo.dvr`'s init (load offset 0x50, checked byte by byte, `mov ax, size; mov bx,
0x62a; xor si, si; mov dx, paragraph; call far [0x36]`) and `xeno_formats::font`
parses the four fonts, refusing kinds 2 to 6, heights under 2 (the renderers would
loop 65,536 rows) and bitmap widths over 8. `xeno_render::Framebuffer::draw_char` and
`draw_text` draw with a `TextCursor` (cursor, left margin, both colour pairs, extra
spacing) in `TextMode::Transparent` or `Opaque`, clipping to the surface.
`xeno-tool inspect mgavideo.dvr` lists the fonts, and `xeno-tool dump mgavideo.dvr`
writes a sheet of each font's 128 codes.

## Open questions

- What calls `1fe6:48f1` (extra spacing), and what fonts 2 and 4 are used for.
- What the host's word slot `0x26c` does for code 7.
- A DOSBox capture of a caption, the credits and the log-in name, to check spacing and
  shades.

## Parser

`xeno_formats::font` (the block and its fonts) and `xeno_formats::mgavideo` (reading
the block, and the colour-animation list, from the driver), T-041.
