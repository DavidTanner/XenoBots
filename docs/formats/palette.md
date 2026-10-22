# Palettes

## Status

**Partial** (T-013). Every image in the game is 8-bit indexed and drawn through one
256-colour VGA palette at a time. The game has no standalone palette file. Each picture
brings its own palette:

| Source | Encoding | Where in the file | Status | Read by |
|---|---|---|---|---|
| `.pcx` (`xenodrop`, `cavedrop`) | 8 bits per channel | 768-byte trailer after the `0x0c` marker (see `pcx.md`) | confirmed | `xeno_formats::pcx` |
| `.vcl` (all 14) | VGA DAC, 6 bits per channel | First 768 bytes of the file (`vcl.md`) | confirmed; `vga240.mod`'s loader reads it into a buffer of the caller's | `xeno_formats::vcl` |
| `.m@l` screens (all 8) | VGA DAC, 6 bits per channel | First 768 bytes of the screen after unpacking (`m@l.md`) | confirmed | `xeno_formats::mal` |
| IFF ILBM/PBM `CMAP` | 8 bits per channel (IFF standard) | `CMAP` chunk | `vgaiff.dvr` only; no IFF file is in the install (see below) | no |
| Colour-animation records | VGA DAC, 6 bits per channel | Embedded in `mgavideo.dvr` and `vgaintro.dvr` data | partial | no |

Not palette sources, as far as the evidence goes:

- `xeno.exe` never writes the DAC, never calls `int 10h`, and holds no 768-byte run of
  values in the DAC range that varies like a palette. It leaves palettes to the drivers.
- The `.b@n` sprite banks and sky strips hold no palette (confirmed, T-014; see
  `b@n.md`). The `.m00`..`.m32` models hold none either (confirmed, T-029;
  `mxx.md`). Both are drawn with whatever palette is current.

`xeno-tool palette <file>` prints the palette of a `.pcx`, `.vcl` or `.m@l` file, one colour
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
  `.MRL` screens (`SITROOM`, `LOGIN`, ...), stored on disk packed as `.m@l`. Unpacked
  (`packed.md`), every one is 768 bytes of palette followed by a PackBits stream that
  expands to exactly 64,000 bytes (`m@l.md`). Decoded that way, every screen looks right
  in its own palette.
- **Host slot `0x292`** is a PackBits decoder in `xeno.exe` (unpacked file offset
  ~0x14fcf, Ghidra `1fe6:456f`). A control byte `n` of 0..=127 copies the next `n + 1`
  bytes. Otherwise the next byte is repeated `257 - n` times. Decoding stops once the
  requested output length is reached. Slot `0x28e` (~0x14f57) is the matching encoder.
  Both are exported through a `(slot, offset)` table in `xeno.exe` (unpacked offset
  ~0x2c4f4).
- **Fades.** A routine near ~0x747 reads the current DAC back through port `0x3c7` into
  a second 768-byte buffer (data word `0x696`). A per-retrace routine (~0x7ba) then
  scales those colours through a 64-entry ramp into the shadow palette. By its shape it
  fades between black and the saved palette. From the `.vcl` player (T-027; load-image
  offsets, see `docs/re/dvr-protocol.md`): the fade is a routine pointer (data word
  `0x4f8`) that the retrace handler calls once per retrace, and `AX` is the length in
  retraces (a 16-bit level moves by `0x10000 / AX` each time). Slot `0x4d8` fades from
  black to the palette in segment `DX` and returns at once; `0x4dc` fades colours
  `BX..=CX` in from the palette at `ES` and waits. `0x4c8` fades colours `BX..=CX` to
  black (at most `DX` retraces) and `0x4c0` all 256, both waiting. `0x4b4` sets the
  whole palette to black and waits for the upload, `0x4e4` returns carry set while a
  fade runs, and `0x4e8` stops it and puts the saved palette back (a fade-in's target).

### Fade arithmetic (T-036)

Read in `mgavideo.dvr` (load offsets: `pal_start_fade_in` 0x664 and its step 0x736;
the fade out at 0x505 and its step 0x5ba; the retrace handler 0x90f). Reproduced by
`xeno_render::Dac`.

- **Order each retrace.** The host's retrace interrupt counts `DS:0x370`, then calls
  slot `0x4f4`: upload the shadow if dirty, then call the fade routine, which writes the
  next shadow and marks it dirty. So a step shows one retrace after it is computed.
- **Start.** Every fade first stops the running one and clears the dirty flag (a
  pending upload is dropped). The step is `0x10000 / length` (a length of 0 counts as
  1; a length of 1 overflows the division, a fault in the original).
- **Fade in** (`0x4d8`/`0x4dc`): copies the target into the shadow and a saved buffer,
  sets those shadow colours to black, waits for that upload, then starts. Each step the
  16-bit level rises by the step, saturating at 0xffff, and every byte becomes
  `ramp[saved]`, where `ramp[i] = i * level >> 16` for `i` in 0..64. At 0xffff the ramp
  is `i - 1`, and the fade ends there: **a fade in leaves every non-zero level one below
  its target**, until something puts the target back (`0x4e8`, only while it runs).
- **Fade out** (`0x4c8`/`0x4c0`): waits for the counter to move (one retrace), reads the
  DAC back through port `0x3c7`, then starts. The level rises the same way but wraps to
  0 on overflow, and the ramp uses `0x10000 - level`, so the last step is black. It
  also stops after `DX` steps (`0x4c0` passes 0xffff).
- **Colour range.** The routines take a first and last colour, but they read the DAC
  into the start of their buffer and then index it from `first * 3`, and loop to
  `count * 3`; only a first colour of 0 works as intended, which is all the `.vcl`
  player uses (colours 0..=251, then 0..=255).
- **Back to back** (the player's two fades out): the second stops the first in the same
  retrace its black step was computed, dropping that upload, so the second starts from
  the first's next-to-last levels. The DAC shows those for three retraces before the
  second's first step.

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

- Settled (T-027): a `.vcl` palette reaches the DAC through the fade-in of slot
  `0x4d8`, over the number of retraces in the sequence entry (`vcl.md`). The cutscenes
  use no colour animation records.
- What sets the hidden switch (`DS:0x618 = 0xe263`) that `vgaiff.dvr`'s screenshot
  needs (T-039). Its IFF reader is never called (`docs/re/dvr-protocol.md`).
- The countdown reload, and who advances the animation pointer.

## Parser

`xeno_formats::vcl` (the `.vcl` palette and picture, T-018), the PCX trailer in
`xeno_formats::pcx`, the screen palette in `xeno_formats::mal` (T-017), and `Palette::from_vga6` / `Palette::from_rgb8` for raw palettes.
Added by T-013.
