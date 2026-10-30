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
- It scrolls by setting the origin through slot `0x60e` once a frame (CRTC start
  address from the row table plus `x / 4`, and pixel panning `(x mod 4) * 2` applied
  by the host's retrace handler); the view's row is 72, where the backdrop starts.
  On exit it calls slot `0x60a` (width 320) and sets mode 13h again.
- It also sets the CRTC **line compare** to 255 scan lines and the attribute mode
  control's pixel-panning bit (bit 5): from screen row 128 down, the VGA shows video
  memory from address 0, unpanned. That band (rows 128..=199) shows the 72 rows above
  the backdrop, where the credits are drawn (T-038, `fgd.md`, "Screen"). No other
  screen of the game splits.

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
  precision. The intro drop also needs a split: rows from 128 on come from the top
  of the surface (T-038).
- **Aspect (T-032):** 320x200 needs the 1.2 vertical correction. 320x240 already fills
  4:3 with square pixels and must *not* be stretched. Switching between the two modes
  must not change the on-screen image size on a 4:3 frame.
- **Timing (T-034):** mode 13h refreshes at 25,175,000 / (800 x 449) = 70.0863 Hz and
  mode B at 25,175,000 / (800 x 525) = 59.9405 Hz. The music, fades and cutscenes are
  paced by vertical retrace, so their speed depends on the mode. The port's game loop
  steps once per retrace at the original rate of the current mode, not the host
  display's; see `docs/game-loop.md`.

## Presentation (T-032)

The port shows the view in a window by `xeno_render::present`: each view pixel becomes
an `sx` by `sy` block of window pixels (whole numbers, nearest neighbour, never
blended), centred, with black bars. Both modes filled a 4:3 frame on the original
monitor, so the target pixel shape is `3w / 4h` times taller than wide: 1.2 for
320x200, 1 for 320x240.

With whole-number factors, 1.2 is exact only at `sx` a multiple of 5 (5x6, 10x12, ...).
5x6 needs a 1600x1200 window, 10x12 a 3200x2400 one, so at 1920x1080 nothing exact fits
and at 3840x2160 exact means 5x6, a 1600x1200 picture on a 4K screen. Hence three
choices (`--aspect`):

- **`correct`** (default): the largest `sx` whose nearest vertical factor
  (`sy = round(1.2 * sx)`, never a tie) fits and is within **5%** of 1.2. Every `sx`
  from 4 up is (worst: 7x8, -4.8%), so from a 1280x1000 window up the error is at most
  5%. Below that, nothing may qualify; it then takes the smallest error, ties to the
  larger picture, which is never worse than 1x1's 16.7% too flat: 2x2 at 640x400,
  3x3 at 1280x720, 3x4 (+11.1%) at 1280x900.
- **`exact`**: always the smallest error, ties to the larger picture.
- **`off`**: square pixels, the largest equal factors that fit.

320x240 always gets equal factors, so its error is 0 in every setting.

| Window | 320x200 `correct` | Error | 320x240 | 320x200 `exact` |
|---|---|---|---|---|
| 640x400 | 2x2, 640x400 | -16.7% | 1x1, 320x240 | same |
| 800x600 (default window, 1x display) | 2x2, 640x400 | -16.7% | 2x2, 640x480 | same |
| 1280x720 | 3x3, 960x600 | -16.7% | 3x3, 960x720 | same |
| 1920x1080 | 4x5, 1280x1000 | +4.2% | 4x4, 1280x960 | same |
| 1600x1200 (default window, 2x display) | 5x6, 1600x1200 | 0 | 5x5, 1600x1200 | same |
| 2560x1440 | 6x7, 1920x1400 | -2.8% | 6x6, 1920x1440 | 5x6, 1600x1200 |
| 3840x2160 | 8x10, 2560x2000 | +4.2% | 9x9, 2880x2160 | 5x6, 1600x1200 |

A positive error is a picture taller than 4:3. The two modes come out the same width
up to 2560x1440, with heights within 4% (720p is the exception: 320x200 is 120 rows
shorter). At 3840x2160 they differ: 320x240 is 9x, while 320x200's 9x11 is 2200 rows
and 9x10 is 7.4% off, so it stays at 8x10, 320 columns narrower. The full table, with
`off`, is the `layout_table` snapshot in `xeno-render`.

Not chosen: nearest neighbour at a fractional vertical factor (rows of uneven height,
for example 10 and 11 at 4K for an exact 2880x2160). It stays sharp but shows as uneven
lines in fine patterns such as the test card's checkerboard; the edge-aware scalers of
T-071 are the place for it.

`--scale N` fixes `sx = N` (1 to 32) and takes `sy = round(N * 1.2)` for 320x200
(`N` with `--aspect off`, and for 320x240). The window is opened at the picture's size
in physical pixels; if it ends up smaller (a small screen, or the user shrinks it), the
picture is not rescaled but cropped equally on both sides, since a fixed scale is asked
for to see exact pixels. Under `auto` a window smaller than the view also shows the
middle of a 1x picture.

Window sizes and fullscreen (T-033). Every factor above is in physical pixels: the
picture buffer follows the window's real pixel size, so a high-DPI (Retina) display
gets a larger factor, never a blurred one. The default window is 800x600 points (2x2
on a 1x display, 5x6 on a 2x one) and keeps that size in points when it moves to a
monitor with another scale factor; a `--scale N` window keeps its size in pixels
instead, so the picture still fits it exactly. Fullscreen (`--fullscreen`, F11 or
Alt+Enter) is borderless on the current monitor and never changes the display mode:
the same layout rules apply to the monitor's size (a 3440x1440 monitor gives 6x7), and
`--scale N` keeps its factor there, centred, cropped if the screen is smaller.

## Open questions

- Confirm with a DOSBox capture (T-027 was done statically, and T-036 built the intro
  player from the same reading; no DOSBox was available to either, so this is still to
  do, as T-036's note in `docs/done-tasks.md` says). A capture should check:
  - the `.vcl` cutscenes are 320x240 at ~59.94 Hz and the front end and drop sequence
    320x200 at ~70 Hz (DOSBox's raw screenshots and its reported refresh rate);
  - a pan moves one pixel per retrace (`TRIUMPH`: one per two), captions type at two
    retraces per character, and fades take the lengths in the entries (`vcl.md`);
  - each intro picture ends at its music-relative time, and the intro lasts a little
    over two minutes;
  - which lose picture follows a defeat commanding each robot type (`DS:0x6012`);
  - P pauses a cutscene and any other key ends it, skipping the drop sequence.
  - frame by frame against `xeno-app`'s intro (T-036): when each picture's first
    non-black frame and each pan step appear, that a fade in ends one DAC level below
    the picture's palette, and that the next-to-last fade-out level holds for three
    retraces between the two fades out (`palette.md`, "Fade arithmetic").
- The purpose of the `xeno.exe` misc-output routine (~0x10d7b).
