# `.b@n` sprite banks and sky strips

## Status

**Confirmed** (T-014, T-028). The layout is confirmed from the data: `xeno_formats::ban`
reads all 14 files of a stock install, and every record and row-span list accounts for
every byte. T-028 traced the code that loads and draws both kinds (addresses at the
end), so every field's meaning is known.

## Files

Each `.b@n` is a packed `.BIN` file (see `packed.md`). Decompress it first; every
offset below is into the decoded data. The file name decides which of two layouts it
has:

| Files | Decoded size | Layout | Name in the code |
|---|---|---|---|
| `msprite0`..`msprite5` | 2.4 KB to 32 KB | Sprite bank | `MSPRITES.BIN` (file id 0x36) with its eighth letter replaced by the digit |
| `sky_a`..`sky_h` | 18,468 bytes each | Sky strip | The name in a scenario's `BGND` chunk (`SKY_B   .BIN`, ...) |

The game asks for `xxx.BIN` and the loader finds the packed `xxx.B@N` (`packed.md`).
`mgavideo.dvr`'s file-id table names `MSPRITES.BIN` six times (ids 0x36..0x40) and
`BACKLAND.BIN` once (id 0x34). The bank loader asks the name builder for id 0x36 and
writes `'0' + (id - 0x36) / 2` over the `S` itself, so ids 0x36, 0x38, ..., 0x40 are
`msprite0`..`msprite5`. `BACKLAND.BIN` is only the sky's default name (below).

Neither layout holds a palette. They are drawn with whatever palette is current.

## Sprite bank

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 LE | `runtime_first_handle` | 0 on disk. The loader writes the bank's first sprite handle here |
| 0x02 | u16 LE | `runtime_count` | 0 on disk. The loader writes the number of sprites here |
| 0x04 | ... | records | One per sprite, back to back |
| end | u16 LE | | 0: no more records. Nothing follows it |

Each record:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 LE | `size` | Bytes in the record, these six included. The loader steps from record to record with it |
| 0x02 | u8 | `width` | Pixels |
| 0x03 | u8 | `height` | Pixels |
| 0x04 | u8 | `flags` | 2: row spans (below). Anything else is drawn as raw; the stock files use 0 and 2 |
| 0x05 | u8 | `unknown_05` | 8 in every record (perhaps bits per pixel). Nothing reads it |
| 0x06 | `size - 6` | pixels | Raw or row spans, by `flags` |

- **Raw** (`flags` = 0): `width * height` palette indices, row by row. `size` is
  always `6 + width * height`.
- **Row spans** (`flags` = 2): for each of the `height` rows, a left skip byte, a
  count byte, then `count` pixels. Pixels outside the span are not stored. Some records
  in `msprite3` and `msprite4` end with one extra empty span (`0, 0`), as if the
  encoder had written `height + 1` rows. The game never reads it: the blitters stop
  after `height` rows and the loader steps over it with `size`. The parser keeps it as
  `trailing_empty_span`. A stock install has 45 row-span sprites, all in `msprite2`,
  `msprite3` and `msprite4`.

Sprite counts per bank: 64, 66, 57, 103, 80 and 20. Widths run from 1 to 121 pixels
and heights from 2 to 96. The banks hold interface art: buttons and labels, gauges,
robot pictures, and pieces of the cockpit panels.

### Sprite numbers

The game draws a sprite by its **handle**, an offset into one global sprite
directory of 390 entries of 8 bytes (allocated at start-up, one more entry set to
`0xffff` after the end). The bank loader is called with the handle where the bank's
first sprite goes. It walks the records, and for sprite `n` fills the entry at
`first + 8n` with a far pointer to its pixels (record + 6), the offset just past
`width * height` bytes and `width * height`. It then writes the first handle and the
count into the bank's first four bytes, and registers a callback that moves the
pointers if the memory manager moves the bank. So a sprite is numbered by its record
index in its bank plus the bank's base, which the code fixes:

| Bank | First handle | Entries | Loaded by the code for |
|---|---|---|---|
| `msprite0` | 0x000 | 0..63 | Start-up (always loaded) |
| `msprite5` | 0x200 | 64..83 | The map room |
| `msprite1` | 0x2a0 | 84..149 | The situation room |
| `msprite2` | 0x4b0 | 150..206 | The Humanoid cockpit |
| `msprite3` | 0x678 | 207..309 | The Scorpion cockpit |
| `msprite4` | 0x9b0 | 310..389 | The Scout cockpit |

The ranges don't overlap and add up to exactly the directory's 390 entries. The
screen for each bank is from where its load call sits (next to that screen's `.m@l`
load); the handles are certain.

### Drawing (`mgavideo.dvr`)

All sprite blitters take the handle in `AX` and a rectangle (`y0, x0, y1, x1`, words)
at `DS:BX`, and draw into the current surface (slot `0x484`). They look up the pixels
through the directory and read the width and height at pixel pointer - 4 and the flags
at - 2. Common rules:

- **Anchor.** Slot `0x5c0` sets the anchor mode from `AL` (0 or -1) and returns the
  old one. With -1, half the sprite's width and height (rounded down) is subtracted
  from the rectangle's corners first, so the sprite is centred on the point.
- **No clipping.** If the anchored rectangle's top or left is negative, or its bottom
  `y1` or right `x1` lies past the surface, nothing is drawn (carry set).
- **Raw** sprites copy `width` pixels on each of `height` rows. **Row spans** copy
  only the span pixels; everything outside the spans is left as it was.
- **Opaque** blitters copy every pixel they visit, colour 0 included. **Keyed**
  blitters skip pixels of colour 0, in raw sprites and inside spans alike.
- On the visible screen the mouse pointer is hidden around the rectangle while
  drawing.

| Slot | Mode | Placement |
|---|---|---|
| `0x5c4` | Opaque | Top-left at (`x0`, `y0`) |
| `0x5c8` | Opaque | Flipped vertically: first row at `y1 - 1`, going up |
| `0x5cc` | Opaque | Mirrored: each row written leftwards from `x1 - 1` |
| `0x5d0` | Opaque | Rotated 180 degrees (both of the above) |
| `0x5d4` | Opaque | Centred on the surface, offset by (`x0`, `y0`) |
| `0x5d8` | Keyed | Top-left at (`x0`, `y0`) |
| `0x5dc` | Keyed | Flipped vertically |
| `0x5e0` | Keyed | Mirrored |
| `0x5e4` | Keyed | Rotated 180 degrees |
| `0x5e8` | Keyed | Centred on the surface, offset by (`x0`, `y0`) |
| `0x5ec` | Keyed | A window: the rectangle's size of a raw sprite, starting at source column `DX` and row `CX` (`DX + CX * width`), drawn at (`x0`, `y0`) without bounds checks |

In the mirrored keyed blitter a span's skip counts from the right edge, as expected.
The mirrored *opaque* one moves the skip to the right and then writes leftwards, so a
span with a skip lands too far right; which callers mirror a row-span sprite opaquely
is not traced (T-056).

The centred blitters (`0x5d4`, `0x5e8`) check the anchored rectangle like the others
but position the sprite at the surface's centre plus (`x0`, `y0`) minus half its size.

`xeno_render::Framebuffer::blit_sprite` and `blit_sprite_window` (T-030) follow these
rules, with three deliberate differences: a mirrored opaque row span is placed as the
keyed blitter places it (an exact mirror image), the centred blitters check the
rectangle they draw rather than the un-centred one, and the window blitter draws only
when the window lies inside the sprite and on the surface. All three keep every write
on the surface.

## Sky strip

18,468 bytes of palette indices with no header: one image, 342 pixels wide and 54
high, row by row.

**Loading.** A scenario's `BGND` chunk names the strip (`usg.md`). Its handler
(`scen_set_sky_name`) starts from `BACKLAND.BIN` (file id 0x34), puts the chunk's base
name (up to 8 characters, space padded) over the first 8 characters, and frees the old
sky; an empty or longer name keeps the previous sky. `gfx_load_sky` loads the named
file when the sky is not present.

**Drawing** (`gfx_draw_sky`, into the 3D view's buffer, which is `w` by `h`):

- If the sky failed to load, the upper `h / 2` rows are filled with palette index
  `0x3e` and the rest with `0x3f`: a flat sky and ground.
- Otherwise the strip fills the upper `ceil(h / 2)` rows, so its last row sits just
  above the horizon in the middle of the view, using strip rows `54 - ceil(h / 2)` to
  53 (for `h` up to 108). The view has no pitch: the horizon never moves.
- Horizontally the strip wraps with a period of 342. The first column is
  `(heading / 64 - w / 2) mod 342`, where the heading is the 16-bit angle at host
  `DS:0x1460` (presumably 65,536 to a turn), so one strip column is 1/1024 of a turn. The strip
  covers about 120 degrees and repeats almost exactly three times around (3 x 342 =
  1026). Turning moves the sky one column per 1/1024 turn.
- The detail setting `s` (`DS:0x1452` shifted right by `DS:0x1454`) picks the
  variant: 1 is the above; 0 samples every second column and row; 2 or more draws
  each sample as an `n` x `n` block, `n` the largest power of two not above `s`.

### `BACKLAND.BIN`

`scen06a.usg` names `BACKLAND.BIN`, and a stock install has no `BACKLAND.BIN`,
`BACKLAND.B!N` or `BACKLAND.B@N`. The loader then returns without allocating the sky,
so that mission shows the flat `0x3e` / `0x3f` view. That is presumably what the
original shows too (not yet checked in DOSBox; T-056).

## Parser

`xeno_formats::ban::parse_sprites` and `xeno_formats::ban::parse_sky`, added by T-014.
`Sprite::spans` keeps each row's `(skip, count)` and `Sprite::covers(x, y)` says
whether a blitter writes a pixel, since a 0 inside a span is opaque and outside is not.
`xeno-tool dump <file> [--palette <pcx|vcl|m@l>]` writes one PNG per sprite
(`msprite0_000.png`, ...) or one per sky strip. Without `--palette` the colours are
grey levels of the index.

## In the executables

`xeno.exe` (Ghidra `segment:offset`, unpacked binary of `re-setup.md`):

| Address | Name | Purpose |
|---|---|---|
| `1000:14fa` | `gfx_init_sprite_directory` | Allocates the 390-entry directory (`DS:0x412`) |
| `1000:155c` | `gfx_load_sprite_bank` | `AX` file id, `CX:BX` block, `DX` first handle |
| `1000:15e2` | `gfx_on_sprite_bank_event` | Memory-manager callback that rebases the directory |
| `1000:163d` | `gfx_get_sprite_header` | Width/height and flags of a handle (no caller found) |
| `1000:2822` | `scen_set_sky_name` | `BGND` name into the sky descriptor (`DS:0x4c8a`, name at `0x4c90`) |
| `1000:2894` | `gfx_load_sky` | Load the sky if absent |
| `1000:2bfb` | `gfx_draw_sky` | Draw the strip, or the flat fallback |

`mgavideo.dvr` (load-image offsets): `gfx_set_sprite_anchor` `0x22a3`,
`gfx_place_sprite_rect` `0x22ab`, `gfx_blit_sprite_vflip` `0x22ff`, `gfx_blit_sprite`
`0x2340`, `gfx_copy_sprite_rows` `0x237d`, `gfx_blit_sprite_rot180` `0x23f8`,
`gfx_blit_sprite_hflip` `0x243d`, `gfx_copy_sprite_rows_mirrored` `0x247e`,
`gfx_blit_sprite_centred` `0x24d8`, `gfx_blit_sprite_keyed_vflip` `0x25ac`,
`gfx_blit_sprite_keyed` `0x25ed`, `gfx_key_sprite_rows` `0x262a`,
`gfx_blit_sprite_keyed_rot180` `0x2694`, `gfx_blit_sprite_keyed_hflip` `0x26d9`,
`gfx_key_sprite_rows_mirrored` `0x271a`, `gfx_blit_sprite_keyed_centred` `0x2789`,
`gfx_blit_sprite_window_keyed` `0x284c`.
