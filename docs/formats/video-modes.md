# Video modes

## Status

**Confirmed from the code** (T-012). The game uses **two** 256-colour display modes,
and some screens use a virtual width larger than the visible area so they can scroll in
hardware:

| Mode | Visible | Pixel aspect on a 4:3 CRT | Refresh | Memory layout | Set by | Used for |
|---|---|---|---|---|---|---|
| A | 320x200 | 5:6 (taller than wide; vertical factor 1.2) | 70 Hz | BIOS mode 13h, linear (chain-4) | `mgavideo.dvr` | The default mode: front end and most of the game |
| A' | 320x200 | 5:6 | 70 Hz | Mode 13h timing, unchained (planar), 768-pixel virtual width | `vgaintro.dvr`, with row tables from `vga240.mod` | The intro "drop" sequence over the 768-wide `xenodrop.pcx` / `cavedrop.pcx` backdrops |
| B | 320x240 | 1:1 (square) | 60 Hz | Unchained (planar, "Mode X" style), 640-pixel virtual width | `vga240.mod` | Three sequences in `xeno.exe` that switch into it and back (probably the `.vcl` cutscenes; see open questions) |

Neither a 320x400 mode nor any SVGA/VESA mode appears in any binary. `config.dvr`
offers a single graphics choice, "VGA/MCGA graphics".

So the port's logical framebuffer cannot be fixed at 320x200. It must support 320x200
and 320x240, each with its own aspect correction. Scrolling screens need a wider
virtual surface with a 320-wide viewport.

## Evidence

Offsets below are **file offsets**. For `xeno.exe` they are in the *unpacked* file
(SHA-256 `e57aec9d…6b36`, see `docs/re-setup.md`), and the matching Ghidra address is
`0x10000 + offset - 0xC00`. For the `.dvr` files the Ghidra address is
`0x10000 + offset - 0x200`. `vga240.mod` has no header, so file offset equals load
offset.

### `mgavideo.dvr`: 320x200, mode 13h

- Its mode-set routine stores a width of 320 and a height of 200 in its own data, then
  calls BIOS `int 10h` with `AX = 0013h` (file offset ~0x32e). Its shutdown path sets
  `AX = 0003h` (text mode, ~0x38d).
- It never touches the sequencer, graphics controller or CRTC, so its mode 13h is the
  stock linear 320x200 layout. It also writes the DAC (`3C8h`) for palette upload.
- It contains the 8.3 names `VGAIFF.DVR`, `LIGHT   .MOD` and `VGA240  .MOD`, and the
  names of every `.vcl` file. Its data segment starts 0x430 paragraphs into the load
  image; the three module names are at data offsets 0x2dc, 0x2e7 and 0x2f4.

### `vga240.mod`: 320x240 and the virtual-width services

`vga240.mod` is not an MZ file. It is a raw real-mode code image loaded at offset 0 of a
segment:

- Offset 0 is a stub entry table (`ret`, `retf`, short jumps to the init routines).
- Offset 0x0a is a slot that receives the module's code segment. It is followed by a
  table of `(host slot, module offset)` u16 pairs ending in a zero word. The host slots
  are far-pointer cells in the host's data segment. Once a slot is patched, the host and
  the other drivers reach the module through it.

`mgavideo.dvr` loads it lazily: its export table maps host slots `0x5f4`, `0x5fe` and
`0x606` to two small stubs. The first call through any of them loads `VGA240.MOD` into a
new segment, runs its init, and then retries the call. Init repoints slot `0x5f4` at the
module's own "enter 320x240" routine and passes the rest of its table to the host,
presumably so the host patches those slots too. If the load
fails, the stub returns with carry set and the display stays in mode 13h.

Routines exported, by host slot:

| Slot | What it does |
|---|---|
| `0x5f4` | Enter mode B: BIOS mode 12h (640x480 16-colour, for its 480-line 60 Hz timing), then reprogram to 256-colour unchained, and set a 640-pixel virtual width |
| `0x602` | Leave mode B: back to BIOS mode 13h, virtual width 320 |
| `0x606` | Set virtual width 640 without changing the mode |
| `0x60a` | Set virtual width 320 without changing the mode |
| `0x60e` | Set the display origin `(x, y)`: CRTC start address from a row table plus `x / 4`, and a pending pixel-panning value of `(x mod 4) * 2` that the host's retrace handler applies |
| `0x612` | Build the row table for a given virtual width (400 rows, `width / 4` bytes apart) |

How mode B is reached from mode 12h, in register terms:

- Attribute mode control `0x41`: 8-bit colour, pixels clocked at half rate, so the
  640-dot line becomes 320 pixels.
- Graphics mode register `0x40`: 256-colour shift.
- CRTC max scan line `0x41`: each row is scanned twice, so mode 12h's 480 scanlines
  give **240 rows**. The vertical timing registers are left at mode 12h's values
  (25 MHz clock, 525 total lines: **~59.94 Hz**).
- CRTC offset `0x50`, underline `0x00`, mode control `0xe3` (byte mode): 160 bytes per
  row per plane, which is a 640-pixel virtual line.
- Sequencer memory mode: chain-4 off, odd/even off. GC misc: chain odd/even off.
  Memory is planar, four pixels per address.
- The attribute palette is reset to identity (0..15).

It also writes sequencer index `0x5c` with `0x02`. That is not a standard VGA register
and is presumably a chipset-specific write or a leftover; it has no effect on standard
VGA.

The module also contains a loader that reads a file into planar video memory: a
768-byte palette, then RLE pixel data split across the four planes through the map-mask
register. It has not been matched to a file type yet.

### `vgaintro.dvr`: 320x200 unchained, 768-pixel virtual width

- It names `XENODROP.PCX` and `CAVEDROP.PCX`, the two 768-pixel-wide backdrops.
- Its setup leaves mode 13h's timing alone: no BIOS mode set, no change to max scan
  line, vertical registers or misc output. So the visible area stays 320x200 at 70 Hz.
  It turns chain-4 and odd/even off, sets CRTC offset `0x60` (192 bytes per row per
  plane, a **768-pixel virtual line**) and selects byte mode. Then it calls host slots
  `0x606` and `0x612` (with width 768) to have `vga240.mod` build the matching row table.
- It scrolls by writing the CRTC start address (`x / 4` plus a base) and pixel panning
  `(x mod 4) * 2` during vertical retrace. On exit it calls slot `0x60a`
  (width 320) and sets mode 13h again.

### `xeno.exe`

- It never calls `int 10h` itself. Mode changes go through the driver slots.
- Three routines (unpacked file offsets ~0x1091d, ~0x109b2, ~0x109f0) call slot
  `0x5f4` (enter mode B), run a chain of calls to one sequence-player routine
  (~0x19a24) that takes a table of 26-byte entries, and finish with slot `0x602` (back
  to mode 13h). The player calls slot `0x60e` to position the display for each entry.
- Its vertical-retrace handler (~0x13d2a) applies the pending pixel-panning value set by
  slot `0x60e`. It also runs a countdown that alternates the CRTC start address between
  two values for a number of frames, which by its shape is a screen-shake effect.
- A small far routine (~0x10d7b) holds the sequencer in reset, writes misc output
  `0xe3` (25 MHz clock, 480-line sync polarity) and restarts it. Nothing calls it
  directly; it is presumably reached through a slot. Mode B gets the same misc value from
  BIOS mode 12h, so its purpose is open.

## Implications for the port

- **Framebuffer (T-030):** the logical size is per screen: 320x200 or 320x240. Screens
  that scroll in hardware draw into a wider virtual surface (640 wide for mode B, 768
  for the intro drop) and present a 320-wide window at `(x, y)` with 1-pixel horizontal
  precision.
- **Aspect (T-032):** 320x200 needs the 1.2 vertical correction. 320x240 already fills
  4:3 with square pixels and must *not* be stretched. Switching between the two modes
  must not change the on-screen image size on a 4:3 frame.
- **Timing (T-034):** mode 13h refreshes at ~70.09 Hz and mode B at ~59.94 Hz. If any
  animation is paced by vertical retrace (the retrace handler above suggests some is),
  its speed depends on the mode. Pace those by the original rate, not the host display's.

## Open questions

- Exactly which screens the three mode-B sequences in `xeno.exe` show. The VCL names
  live in `mgavideo.dvr`, and the 26-byte entry tables have not been decoded yet
  (T-018, T-027). Confirm with a DOSBox capture.
- `LIGHT.MOD` is named by `mgavideo.dvr` (data offset 0x2e7) but is not in the install.
  Find out whether anything reaches its loader and what happens when it is missing.
- The purpose of the `xeno.exe` misc-output routine (~0x10d7b).
- The file type read by `vga240.mod`'s planar loader.
- What `vgaiff.dvr` does; `mgavideo.dvr` loads it by name (T-025).
