# Palettes

## Status

**Partial** (T-013). Every image in the game is 8-bit indexed and drawn through one
256-colour VGA palette at a time. The game has no standalone palette file. Each picture
brings its own palette:

| Source | Encoding | Where in the file | Status | Read by |
|---|---|---|---|---|
| `.pcx` (`xenodrop`, `cavedrop`) | 8 bits per channel | 768-byte trailer after the `0x0c` marker (see `pcx.md`) | confirmed | `xeno_formats::pcx` |
| `.vcl` (all 14) | VGA DAC, 6 bits per channel | First 768 bytes of the file | confirmed from the data; the loader is not traced yet | `xeno_formats::vcl::palette` |
| `.m@l` screens (all 8) | VGA DAC, 6 bits per channel | First 768 bytes of the *decoded* screen; the file's outer encoding is not decoded yet | likely | not yet (T-017) |
| IFF ILBM/PBM `CMAP` | 8 bits per channel (IFF standard) | `CMAP` chunk | `vgaiff.dvr` only; no IFF file is in the install (see below) | no |
| Colour-animation records | VGA DAC, 6 bits per channel | Embedded in `mgavideo.dvr` and `vgaintro.dvr` data | partial | no |

Not palette sources, as far as the evidence goes:

- `xeno.exe` never writes the DAC, never calls `int 10h`, and holds no 768-byte run of
  values in the DAC range that varies like a palette. It leaves palettes to the drivers.
- The `.b@n` sprite banks and the `.m00`..`.m32` models are drawn with whatever
  palette is current. Their headers are far too short to hold one (guess, T-014/T-015).

`xeno-tool palette <file>` prints the palette of a `.pcx` or `.vcl` file, one colour
per line (index, `#rrggbb`, decimal channels), widened to 8 bits. For any other file,
`--offset <n>` (decimal, or hex with `0x`) reads a raw palette at that byte offset, and
`--encoding vga6|rgb8` says how it is stored (default `vga6`).

## Encodings

- **VGA DAC (6-bit)**: 256 `r, g, b` triples, one byte per channel, each 0..=63. This is
  what the hardware takes on port `0x3c9`. `Palette::from_vga6` widens each value to 8
  bits as `(c << 2) | (c >> 4)`, so 0 stays 0 and 63 becomes 255.
- **PCX trailer (8-bit)**: the same colours widened the same way. Every channel value in
  both game PCX files has its low two bits equal to 0 or 3, which is exactly what that
  widening produces. `vgaintro.dvr` shifts each byte right by 2 to get back to DAC
  values (file offset ~0x306a), so the 8-bit and 6-bit paths give identical colours.

## How the palette reaches the screen

Offsets are file offsets. For the `.dvr` files the Ghidra address is
`0x10000 + offset - 0x200`; for `xeno.exe` (unpacked) it is `0x10000 + offset - 0xC00`.
See `video-modes.md` for the driver slot mechanism.

`mgavideo.dvr` owns the DAC. It is the only module that writes the DAC (port `0x3c8`
then `0x3c9`). `vgaintro.dvr` hands its converted PCX palette over instead of writing
the DAC itself. `vgaiff.dvr` only *reads* the DAC (port `0x3c7` then `0x3c9`, near
offset ~0x5f0), just before it writes a `BODY` chunk from video memory at `A000`. With
its one file name, `CAMERA00.LBM`, that makes it a screenshot writer. It also has IFF
reading code, which may never run in normal play.

- **Shadow palette.** The driver keeps a 768-byte copy of the palette in its own
  segment (data word `0x690`). Code that changes colours writes the copy and sets a
  "dirty" byte (data `0x694`).
- **Upload on retrace.** Two vertical-retrace handlers (offsets ~0xb0f and ~0xbc3; one
  writes byte by byte, the other uses `rep outsb`) upload all 768 bytes from index 0
  when the dirty byte is set, then clear it. So a palette change takes effect at the next
  retrace and never tears mid-frame.
- **Full-screen pictures.** A routine near offset ~0x535 takes a loaded asset buffer,
  copies its first 768 bytes into the shadow palette, and then calls host slot `0x292`
  to unpack 64,000 bytes (one 320x200 screen) from buffer offset 0x300 straight to video
  memory at `A000:0000`. A sibling routine (~0x5ab) unpacks the picture only and keeps
  the current palette. The full-screen 320x200 assets `mgavideo.dvr` names are the
  `.MRL` screens (`SITROOM`, `LOGIN`, ...), stored on disk as `.m@l`, hence the "likely"
  in the table above.
- **Host slot `0x292`** is a PackBits decoder in `xeno.exe` (unpacked file offset
  ~0x14fcf, Ghidra `1fe6:456f`). A control byte `n` of 0..=127 copies the next `n + 1`
  bytes. Otherwise the next byte is repeated `257 - n` times. Decoding stops once the
  requested output length is reached. Slot `0x28e` (~0x14f57) is the matching encoder.
  Both are exported through a `(slot, offset)` table in `xeno.exe` (unpacked offset
  ~0x2c4f4).
- **Fades.** A routine near ~0x747 reads the current DAC back through port `0x3c7` into
  a second 768-byte buffer (data word `0x696`). A per-retrace routine (~0x7ba) then
  scales those colours through a 64-entry ramp into the shadow palette. By its shape it
  fades between black and the saved palette.

### Colour-animation records

The retrace handlers also walk a list of colour-animation records and write the colours
of any active record straight to the DAC. Records sit back to back. The same layout
appears in `mgavideo.dvr`'s and `vgaintro.dvr`'s data, so the drivers ship some
animations built in.

| Offset | Size | Meaning | Confidence |
|---|---|---|---|
| 0x00 | u16 LE | Size of this record in bytes; the next record follows it. 0 ends the list | confirmed |
| 0x02 | u8 | Flags. Bit 7: active. Bits 0-2: kind (0 = nothing, 1 = rotate, 2..=4 = step through frames) | confirmed for bit 7 and the kind dispatch; the meaning of each kind is inferred |
| 0x03 | u8 | Retrace countdown; the record is applied when it reaches 0 | confirmed |
| 0x04 | u8 | Probably the countdown reload value (the speed) | guess |
| 0x05 | u8 | First DAC index to write | confirmed |
| 0x06 | u8 | Number of colours | confirmed |
| 0x07 | u8 | Rotate only: rotation position. Colours from the data pointer go first, then that many from the start of the data | confirmed |
| 0x08 | u16 LE | Pointer to the colours to write now, relative to the driver's data. Helpers reset it to the start of the data, or to one frame past it | confirmed |
| 0x0a | `count * 3` per frame | The colours, VGA 6-bit | confirmed |

The built-in records seen so far have one record size per driver: `10 + 2 * count * 3`
bytes, which is two frames of colours. What advances the pointer and the rotation
between steps has not been traced. It is probably the routine the handlers call after
the list (mgavideo ~0xc66).

## Open questions

- The outer encoding of the `.m@l` files (T-017). Each starts with a u16 (1,464 to
  2,633 across the eight files), then control bytes in which `n < 0x80` introduces `n + 1`
  literal bytes, as in PackBits. The first literal run is already palette triples
  (6-bit, colour 0 black). Plain PackBits does not reproduce 768 + 64,000 bytes for
  most files, so the packets with the high bit set mean something else.
- Which code loads `.vcl` palettes and when they reach the DAC (T-018). `vga240.mod`
  contains a loader that reads a 768-byte palette and then RLE data into the four planes,
  which fits the `.vcl` layout (T-027).
- Whether `.vcl` files also carry colour-animation records or palette changes between
  frames (T-018).
- What triggers `vgaiff.dvr`'s screenshot, and whether its IFF reader is ever used.
- The countdown reload, and who advances the animation pointer.

## Parser

`xeno_formats::vcl::palette` (the `.vcl` palette only), the PCX trailer in
`xeno_formats::pcx`, and `Palette::from_vga6` / `Palette::from_rgb8` for raw palettes.
Added by T-013.
