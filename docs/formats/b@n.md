# `.b@n` sprite banks and sky strips

## Status

**Partial** (T-014). The layout is confirmed from the data: `xeno_formats::ban` reads all
14 files of a stock install, and every record and row-span list accounts for every
byte. The code that draws these files has not been traced, so the meaning of the
unknown fields below is still open.

## Files

Each `.b@n` is a packed `.BIN` file (see `packed.md`). Decompress it first; every
offset below is into the decoded data. The file name decides which of two layouts it
has:

| Files | Decoded size | Layout |
|---|---|---|
| `msprite0`..`msprite5` | 2.4 KB to 32 KB | Sprite bank |
| `sky_a`..`sky_h` | 18,468 bytes each | Sky strip |

`mgavideo.dvr` names `MSPRITES.BIN` and `BACKLAND.BIN`. The sprite banks are probably
`MSPRITES` with a digit appended at run time; the sky strips may be `BACKLAND` (guess,
T-028).

Neither layout holds a palette. They are drawn with whatever palette is current.

## Sprite bank

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u32 LE | `unknown_00` | 0 in every bank. Possibly space for a pointer the loader fills in |
| 0x04 | ... | records | One per sprite, back to back |
| end | u16 LE | | 0: no more records. Nothing follows it |

Each record:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 LE | `size` | Bytes in the record, these six included |
| 0x02 | u8 | `width` | Pixels |
| 0x03 | u8 | `height` | Pixels |
| 0x04 | u8 | `flags` | Bit 1: row spans (below). No other bit is ever set |
| 0x05 | u8 | `unknown_05` | 8 in every record. Perhaps bits per pixel |
| 0x06 | `size - 6` | pixels | Raw or row spans, by bit 1 of `flags` |

- **Raw** (`flags` = 0): `width * height` palette indices, row by row. `size` is
  always `6 + width * height`.
- **Row spans** (`flags` = 2): for each of the `height` rows, a left skip byte, a
  count byte, then `count` pixels. Pixels outside the span are not stored. Some records
  end with one extra empty span (`0, 0`) after the last row, and others don't. The
  parser keeps it as `trailing_empty_span`. A stock install has 45 row-span sprites,
  all in `msprite2`, `msprite3` and `msprite4`.

Sprite counts per bank: 64, 66, 57, 103, 80 and 20. Widths run from 1 to 121 pixels
and heights from 2 to 96. Raw sprites fill their background with colour 0. Colour 0
is probably transparent (guess). The banks include interface art: buttons and labels,
gauges, robot pictures, and pieces of the cockpit panels.

## Sky strip

18,468 bytes of palette indices with no header: one image, 342 pixels wide and 54
high, row by row. The width comes from the data: a strong repeat at a 342-byte
stride, and 342 x 54 is exactly the decoded size. Rendered that way, every strip is a
horizon with sky, clouds and mountains. 342 is 22 more than the screen width, which
suggests the strip scrolls sideways or wraps as the view turns (guess).

## Open questions

- Which code draws the sprites, whether it treats colour 0 as transparent, and what
  `unknown_00`, `unknown_05` and the trailing empty span are for (T-028).
- How sprites are numbered when the game asks for one: by record index in the bank,
  or through a table elsewhere.
- Which palette each bank is drawn with: probably that of the `.m@l` screen shown at
  the time.
- How the sky strip is placed and scrolled in the mission view, and which strip goes
  with which scenario.

## Parser

`xeno_formats::ban::parse_sprites` and `xeno_formats::ban::parse_sky`, added by T-014.
`xeno-tool dump <file> [--palette <pcx|vcl|m@l>]` writes one PNG per sprite
(`msprite0_000.png`, ...) or one per sky strip. Without `--palette` the colours are
grey levels of the index.
