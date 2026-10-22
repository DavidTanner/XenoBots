# Video modes

## Status

**Confirmed from the code** (T-012). The game uses **two** 256-colour display modes,
and some screens use a virtual width larger than the visible area so they can scroll in
hardware:

| Mode | Visible | Pixel aspect on a 4:3 CRT | Refresh | Memory layout | Set by | Used for |
|---|---|---|---|---|---|---|
| A | 320x200 | 5:6 (taller than wide; vertical factor 1.2) | 70 Hz | BIOS mode 13h, linear (chain-4) | `mgavideo.dvr` | The default mode: front end and most of the game |
| A' | 320x200 | 5:6 | 70 Hz | Mode 13h timing, unchained (planar), 768-pixel virtual width | `vgaintro.dvr`, with row tables from `vga240.mod` | The intro "drop" sequence over the 768-wide `xenodrop.pcx` / `cavedrop.pcx` backdrops |
| B | 320x240 | 1:1 (square) | 60 Hz | Unchained (planar, "Mode X" style), 640-pixel virtual width | `vga240.mod` | The `.vcl` cutscenes: three sequences in `xeno.exe` switch into it, play `.vcl` pictures and switch back (`vcl.md`) |

Neither a 320x400 mode nor any SVGA/VESA mode appears in any binary. `config.dvr`
offers a single graphics choice, "VGA/MCGA graphics".

## Screens and their modes

From the code (T-027): only three `xeno.exe` routines enter mode B, only `vgaintro.dvr`
uses mode A', and nothing else in any binary sets a mode or a virtual width (every far
call through slots `0x5f4`–`0x612`, and every `int 10h`, was checked). Not yet confirmed
against a DOSBox capture (see the checklist at the end).

| Screen | Mode | Where |
|---|---|---|
| `config.dvr` setup | Text (BIOS mode 3, or 7 on a monochrome adapter) | Runs before `mgavideo.dvr` is loaded |
| Intro cutscene: `EALOGO`, `DETECT`, `ARRIVAL`, `GRNDFALL`, `EMERGE`, `WAR`, `ANNIHIL`, `AFTERMTH`, `RETURN`, `PRESENTS` | B | `xeno.exe` `1000:fd0a` (`vcl.md`) |
| Intro drop over `xenodrop.pcx`, then `cavedrop.pcx` | A' | `vgaintro.dvr` (`fgd.md`) |
| Front end and game: `LOGIN`, `SITROOM`, `MAPROOM`, `REVIEW`, `TRAINMEN`, the three robot consoles, the battle view, the scenario and terrain editors, the file dialogs | A | `mgavideo.dvr`'s mode 13h, set once at start-up (slot `0x470`) and restored after each B or A' sequence |
| Mission won: `WIN` | B | `1000:fde8` from `1000:7059` |
| Mission lost: `NLOSE` or `SLOSE` (`HLOSE` is never shown) | B | `1000:fde8` from `1000:702d` |
| Campaign won: `TRIUMPH`, instead of `WIN` | B | `1000:fd9f` from `1000:70d0` |

Mode B is therefore used for exactly the `.vcl` cutscenes, and every `.vcl` is shown in it.

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
  `VGAIFF.DVR` is the screenshot writer; `LIGHT.MOD`'s loader (slot `0x546`) is never
  called (`docs/re/dvr-protocol.md`).

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
which the host installs (`docs/re/dvr-protocol.md`). If the load
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

The module also contains the `.vcl` loader (offset 0x274), which it installs at host
slot `0x490` on entering mode B. It reads a 768-byte palette, then PackBits pixels in
plain linear order, spread across the four planes by cycling the map mask. See
`vcl.md`.

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
- Three routines (`1000:fd0a`, `fd9f`, `fde8`; their slot calls are at unpacked file
  offsets ~0x1091d, ~0x109b2, ~0x109f0) call slot `0x5f4` (enter mode B), run one or
  more calls of the sequence player (`1fe6:8fc4`, ~0x19a24) on 26-byte entries, and
  finish with slot `0x602` (back to mode 13h). The player positions the window with
  slot `0x60e` and pans it one pixel per step; it is paced by the retrace counter
  `DS:0x370`, so in mode B by 59.94 Hz retraces (entry layout and timing in `vcl.md`).
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

- Confirm with a DOSBox capture (T-027 was done statically; no DOSBox was available).
  A capture should check:
  - the `.vcl` cutscenes are 320x240 at ~59.94 Hz and the front end and drop sequence
    320x200 at ~70 Hz (DOSBox's raw screenshots and its reported refresh rate);
  - a pan moves one pixel per retrace (`TRIUMPH`: one per two), captions type at two
    retraces per character, and fades take the lengths in the entries (`vcl.md`);
  - each intro picture ends at its music-relative time, and the intro lasts a little
    over two minutes;
  - which lose picture follows a defeat commanding each robot type (`DS:0x6012`);
  - P pauses a cutscene and any other key ends it, skipping the drop sequence.
- The purpose of the `xeno.exe` misc-output routine (~0x10d7b).
