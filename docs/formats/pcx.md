# PCX images (`*.pcx`)

## Status

**Confirmed.** Standard ZSoft PCX 3.0, 8 bits per pixel, one plane, with a 256-colour
palette trailer. `xeno_formats::pcx::parse` decodes both files in a stock install
(checked by the `real_assets_pcx_files_decode` test), consuming every byte of the
compressed data exactly.

## Files

| File | Size (bytes) | Image |
|---|---|---|
| `cavedrop.pcx` | 107,618 | 768x128 |
| `xenodrop.pcx` | 46,421 | 768x130 |

## Layout

128-byte header, little-endian:

| Offset | Size | Field | Value in the game's files |
|---|---|---|---|
| 0x00 | u8 | magic | 0x0a |
| 0x01 | u8 | version | 5 (PCX 3.0) |
| 0x02 | u8 | encoding | 1 (RLE) |
| 0x03 | u8 | bits per pixel | 8 |
| 0x04 | 4 x u16 | `x_min`, `y_min`, `x_max`, `y_max` (inclusive) | origin 0, 0 |
| 0x0c | u16 | horizontal DPI | 640 |
| 0x0e | u16 | vertical DPI | 400 |
| 0x10 | 48 bytes | 16-colour EGA palette | unused at 8 bpp; non-zero in `cavedrop`, zero in `xenodrop` |
| 0x40 | u8 | reserved | 0 |
| 0x41 | u8 | planes | 1 |
| 0x42 | u16 | bytes per line | 768 (equal to the width; no padding) |
| 0x44 | u16 | palette info | 0 |
| 0x46 | 2 x u16 | screen size | 0 |
| 0x4a | 54 bytes | reserved | |

Then the RLE pixel data, then the palette trailer: the byte 0x0c followed by 768 bytes
of 8-bit `r, g, b`, occupying the last 769 bytes of the file.

## Encoding

A byte with its top two bits set (`0xc0..=0xff`) is a run: repeat the next byte
`byte & 0x3f` times. Any other byte is a single literal pixel. Decoding stops after
`bytes_per_line * planes * height` bytes. In the game's files no run crosses the end
of a scanline, but the parser accepts runs that do, since other encoders produce them.

## Open questions

- Settled (T-038): both are the intro drop's backdrops, loaded straight into
  `vgaintro.dvr`'s 768-pixel-wide video memory (with a clean copy 128 rows lower) and
  scrolled in hardware under the `.fgd` silhouettes (`fgd.md`, "The drop sequence").
  The driver's loader checks the first 8 header bytes, takes the size from the header
  and every row it gives, and shifts the trailer right by 2 into its palette buffer.
  Only rows 0..=127 are ever on screen: the screen splits after row 128, so
  `xenodrop`'s last two rows are never seen (they overwrite the clean copy of its first
  two). Colour 0 is black in both trailers.
- The DPI fields hold 640 and 400, which look like a screen size written by the
  paint program rather than real DPI. Not used by the parser.
- Settled (T-038): the trailer is uploaded as is when each part starts. `xenodrop`'s is
  nearly all black, and colour-animation blends then bring the scene in
  (`palette.md`); `cavedrop`'s shows at once.

## Parser

`xeno_formats::pcx` (`PcxHeader`, `Pcx`, `parse`), added in T-011. It rejects
anything but version 5, RLE, 8 bpp, one plane, and a file without a palette trailer.
`xeno-tool inspect` prints the header summary; `xeno-tool dump <file> --out <dir>`
writes a PNG.
