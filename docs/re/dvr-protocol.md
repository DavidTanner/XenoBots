# Driver modules and the host slot protocol

How `xeno.exe` loads its six `.dvr` modules and `vga240.mod`, and how the host and the
modules call each other (T-025). Findings only; no listings.

Applies to the unpacked `xeno.exe` (181,836 bytes, SHA-256 `e57aec9d…6b36`) and the
module files whose hashes are in `docs/re-setup.md`.

Addresses: `xeno.exe` code is given as Ghidra `segment:offset` (`1000` is the main code
segment, `1fe6` the "kernel" segment of shared services; image paragraph = Ghidra
segment - 0x1000). `DS:x` is the host data segment (Ghidra `2901`), which every slot
lives in. Module addresses are load-image offsets (file offset minus the 0x200-byte MZ
header; Ghidra shows them in segment `1000`). `vga240.mod` has no header.

## Summary

| Module | Loaded by | When | What it provides |
|---|---|---|---|
| `config.dvr` | `drv_run_config` (`1fe6:08f6`), as a child program (DOS EXEC `4B00h`) | First thing at start-up | The setup program. It edits the host's configuration record in place (CPU class, video, music and sound devices, input) and exits; it is not resident |
| `sounddvr.dvr` | `drv_load_sounddvr` (`1fe6:47dc`) into block `DS:0x416` | Start-up | Music (`.ms3`, AdLib/MPU-401 back ends) and digitised effects (`dsounds.bin`, PC speaker/Sound Blaster/...); slots `0x424`–`0x460` |
| `mgavideo.dvr` | `drv_load_mgavideo` (`1000:1368`) into block `DS:0x30a` | Start-up | Mode 13h, palette and fades, full-screen pictures, file-id names, drawing primitives, fonts, the mouse pointer; slots `0x470`–`0x606` and `0x1496`–`0x14b2`. Loads `vgaiff.dvr`, `vga240.mod` and `LIGHT.MOD` on demand |
| `vgaintro.dvr` | `drv_run_vgaintro` (`1000:fc71`) into block `DS:0x1534` | Once, during the intro, then freed | The intro drop sequence over `xenodrop.pcx`/`cavedrop.pcx` (`fgd.md`); slots `0x1538` enter, `0x1540` run, `0x153c` leave |
| `mapdvr.dvr` | `ui_run_map_editor` (`1000:448c`) into block `DS:0x150a` | When the scenario editor opens the terrain editor (an entry in its key table at `DS:0x5bd8`), then freed | The terrain (`.map`) editor (`map.md`); slots `0x14f4` enter, `0x14f8` leave, `0x1500` input handler |
| `vgaiff.dvr` | `mgavideo.dvr`'s stubs at slots `0x1550`/`0x1558`, into block `DS:0x62e` | On demand | Slot `0x1558`: screenshot to `CAMERA00.LBM` (loaded, run, unloaded). Slots `0x155c`–`0x1570`: an IFF reader that nothing calls |
| `vga240.mod` | `mgavideo.dvr`'s stubs at slots `0x5f4`/`0x606` (raw file load, not EXEC) | First use of the 320x240 mode or of the row-table services | Mode B and virtual-width services (`video-modes.md`), the `.vcl` loader and a caption glyph renderer (`vcl.md`) |

`LIGHT.MOD` (not in the install) would be loaded by `mgavideo.dvr`'s stub at slot
`0x546`. Nothing calls that slot, so it is dead code. (The stub begins with a
breakpoint instruction, but that alone proves nothing: the host's file loader at slot
`0x2b0`, used for every file, starts with one too.)

## Slots

A **slot** is a fixed offset in the host data segment. Most slots hold a far pointer
(offset, segment). Everyone, host and modules, calls a service as `call far [slot]` with
`SS` (and normally `DS`) set to the host data segment; a module that points `DS` at
its own data uses an `SS:` override instead. A few slots hold a near offset instead,
for the far-to-near trampolines below, and a few cells next to slots are memory-block
descriptors (first word the block's segment, byte 2 flags; bit 7 of the flags is set
while the block is present).

- `DS:0000` holds the bytes of `ret` and `retf`, and slots `0x02` and `0x06` point at
  `1000:0000` and `1000:0001`, the same two instructions at the start of the main code
  segment. They are the "do nothing" targets. `sounddvr.dvr` checks the `DS:0000`
  bytes (and `DS == SS`) before it installs anything.
- Slots `0x3c`/`0x40` call the near routine in segment `1000` whose offset is in the
  word at `DS:0x3a` and return far; slots `0x44` and `0x1530` do the same for segment
  `1fe6`. The word slots (`0x22a`–`0x256`, `0x26a`–`0x274`, `0x30e`–`0x320`,
  `0x151c`–`0x152c`, ...) hold such near offsets.

### Export tables

All slot tables are lists that end in a zero word. The services that apply them take
`BX` = the table's segment (0 means the host's own export block) and `SI` = its offset:

| Slot | Kernel | Table entries after a leading segment word | Effect |
|---|---|---|---|
| `0x0a` | `1fe6:0144` | `(slot, offset)` | Word slots get `offset` adjusted to the host code segment |
| `0x0e` | `1fe6:0172` | `(slot, offset)` | Clears those word slots |
| `0x12` | `1fe6:0197` | `(slot, offset)` | **Install:** each far slot = `segment:offset` |
| `0x16` | `1fe6:01b5` | `(slot, offset)` | Clears those far slots to `0:0` |
| `0x1a` | `1fe6:01be` | `(slot, offset)` | Points every listed slot at `CX:DX` |
| `0x1e` / `0x22` | `1fe6:01e4` / `0209` | `(slot, offset, segment)` | Save the slots into the table / restore them from it |
| `0x26` | `1fe6:0227` | `(slot, saved offset, saved segment, offset)` | **Hook:** saves each slot's current pointer, then points it at `segment:offset` |
| `0x2a` | `1fe6:025e` | same | **Unhook:** restores the saved pointers |
| `0x2e` / `0x32` | `1fe6:0282` / `02b0` | code offsets | On a 386 or better (config CPU byte `'3'`+), overwrites each listed `rep movsw`/`rep stosw` in the module with `rep movsd`/`rep stosd` (`mgavideo.dvr` lists one site) |
| `0x36` | `1fe6:02db` | (not a table) | Allocates a block of `AX` bytes described at `DS:BX` and copies `DX:SI` into it |

The host installs its own services the same way: its export block (Ghidra segment
`3b6f`, 0x350 bytes, ending in the text `LAST SEGMENT`) holds 25 tables that each
subsystem's start-up code installs with `BX = 0`. At start-up the block is moved into a
managed memory block with slot `0x36`.

### Host services used by the modules

| Slots | Kernel | What |
|---|---|---|
| `0x0a`–`0x36` | `1fe6:0144`–`02db` | Table services above |
| `0x54` | `1fe6:02fb` | Put an error message (`ES:SI`) in the buffer shown on exit |
| `0x68`–`0xd8` | `1fe6:0d72`–`1d0b` | Memory manager. `0xb0` allocate `AX` bytes, `0xb4` allocate `AX` paragraphs, `0xb8` resize to `AX` paragraphs, `0xbc` free; the block's descriptor is at `CX:BX` |
| `0x276`–`0x27e` | `1fe6:4435`–`443d` | Copy a NUL-terminated name from `DS:SI`, `ES:SI` or the host's string segment into the name buffer `DS:0x64e` |
| `0x282`–`0x28a` | `1fe6:445c`–`44a4` | Hook chains (below): append, insert, remove |
| `0x28e` / `0x292` | `1fe6:44f7` / `456f` | PackBits encode / decode (`palette.md`) |
| `0x298`–`0x2bc` | `1fe6:253a`–`28e8` | File I/O: `0x2b0` loads a whole file (padded 8.3 name at `DS:DX`) into the block at `CX:BX`, `0x2b4` open, `0x2b8` close, `0x2bc` read. `0x2b0` and `0x2b4` also try the name with `!` (LZW) and `@` (packed) as the extension's middle letter (`packed.md`) |
| `0x2e4`–`0x2f8` | `1fe6:45d6`–`47f5` | Module loader (below) |
| `0x2fe` / `0x302` | `1fe6:83aa` / `83cd` | The host's file dialogs: open (its name field starts as `*.USG`) and "SAVE GAME" (FILENAME, DIRECTORY, OK, CANCEL); the name chosen comes back in `SI` |
| `0x324`–`0x340` | `1fe6:357c`–`39b5` | Input event queue (`0x324` posts an event with the time; `0x330` takes the next one matching a mask) |
| `0x348` | `1fe6:30b3` | Re-measure the frame period and re-arm the retrace timer; called after every mode change |
| `0x34c` | `1fe6:3144` | Recompute the derived tick rates from the frame period |
| `0x358`/`0x35c`, `0x360`/`0x364` | `1fe6:33e9`–`3424` | Add/remove a per-retrace hook; add/remove a 60 Hz tick hook |
| `0x38a`–`0x40e` | `1fe6:489c`–`4cbc` | Text: font select and glyph drawing (fonts live in `mgavideo.dvr`'s data) |
| `0x1506` | `1fe6:6a97` | Load a `.map` into the `MAPP` buffer (`map.md`) |
| `0x30e`–`0x320` (word slots) | `1000:13be`–`14d8` | Colour-animation records (`palette.md`): select a list, stop all, hold, release, start, turn on, turn off, read flags, set direction, set speed. `vgaintro.dvr` calls `0x30e` and `0x316` through slot `0x40` |

## Module layout

Every module, and `xeno.exe` itself, starts with the same stub:

| Offset | Contents |
|---|---|
| 0x00 | `ret` (near null target) |
| 0x01 | `retf` (far null target) |
| 0x02 | **Init**: a jump to the init routine, then `retf` at 0x05 |
| 0x06 | **Shutdown**: a jump, then `retf` at 0x09 (in `mgavideo.dvr` it goes straight to a `retf`) |
| 0x0a | `sounddvr`, `mapdvr`, `vgaiff`, `vga240`: the export table: a segment word (fixed up by the DOS relocation or written by init), then `(slot, offset)` pairs ending in a zero word |

Init installs the table at 0x0a with slot `0x12`, does its own set-up and usually
shrinks its block with slot `0xb8` to the resident part. Shutdown clears or unhooks its
slots (`0x16`, `0x2a`). Init returns with carry set on failure.

`mgavideo.dvr` and `vgaintro.dvr` are different: their tables are in data segments. At
init `mgavideo.dvr` copies five data segments into blocks of their own with slot `0x36`
(descriptors at `DS:0x61a`, `0x61e`, `0x622`, `0x626`, `0x62a`; the file-id name table of
`m@l.md` is the first), shrinks itself to its 0x4300-byte code, and installs a one-entry
table: slot `0x470`, "enter graphics". That routine sets mode 13h, re-arms the timer
(`0x348`), installs the full table (90 slots), the 386 patch list and the mouse-pointer
table. `vgaintro.dvr` installs a table at its segment `0x41a`, offset 0 (its shutdown
entry installs the same table again instead of clearing it, which is harmless because
the host never calls through those slots after freeing the block). Its MZ entry point
only sets text mode and exits, for anyone who runs it from DOS.

## Loading

**`drv_load_module`** (`1fe6:463d`, slot `0x2ec`; `BX` = descriptor, `DX` = file name):
opens the file to get its size, allocates that many paragraphs at the descriptor (slot
`0xb4`), loads it there with DOS EXEC `4B03h` ("load overlay", which applies the MZ
relocations with the block's segment) and far-calls offset 0x02, the module's init. It
returns with carry set on error, and `1fe6:4747` (slot `0x2f4`) turns the DOS error into
a message such as "Insufficient memory for MGAVIDEO.DVR".

**`drv_unload_module`** (`1fe6:4713`, slot `0x2f0`; `BX` = descriptor): far-calls
offset 0x06, the shutdown, then marks the block free.

**Lazy modules.** `mgavideo.dvr` exports small stubs for `vga240.mod` (slots `0x5f4`,
`0x606`), `vgaiff.dvr` (`0x1550`, `0x1558`) and `LIGHT.MOD` (`0x546`). The first call
loads the module, whose init repoints the slots at the real routines, and then repeats
the call. `vga240.mod` has no MZ header, so it is read with slot `0x2b0` into a block
whose descriptor overlaps slot `0x5f4` (the segment word is shared), and the stub sets
the slot's offset to 2 to call its init. On shutdown `vga240.mod` points its slots back
at the stub saved in `DS:0x5fe` (slot `0x1a`). Slot `0x2f8` frees a lazy module and
restores its slot (`BX`) from the stub pointer kept 10 bytes after it; nothing calls
the `vga240.mod` routine that uses it, so the module stays loaded. `vgaiff.dvr` hooks `0x1550`/`0x1558` with slot
`0x26`, so its shutdown restores the stubs.

The screenshot stub (`0x1558`) does nothing unless the word at `DS:0x618` is `0xe263`.
That word is decoded from the configuration record (`1fe6:0a57` onwards), so it is a
hidden switch set through `config.dvr` (T-039). `xeno.exe` calls `0x1558` from three
places (one is a key handler) and `mapdvr.dvr` from one.

Slots `0x2e4` (advance a load cursor) and `0x2e8` (`1fe6:45e7`, load an overlay at the
cursor) are exported but never called.

**`config.dvr`** is not loaded this way. `drv_run_config` copies a default record into
`DS:0xde`, builds an environment whose only string is `!=` followed by the record's
offset and segment as eight hex digits, and runs `CONFIG.DVR` with the game's own
command tail. `config.dvr` looks for that string (without it, it edits a record of its
own), measures the CPU and FPU, lets the user choose, and writes the record. If the
program can't be run, the game carries on with the default record. If it exits with a
code other than 0, the game exits. The host then spreads the record over `DS:0x220`–
`0x229` and decodes the words after it into flags (`DS:0x616`, `0x618`, ...). From how
the host uses them: `0x222` is the CPU class (`'1'`, `'2'`, `'3'`; `'3'` enables the
386 patches, `'2'` is needed for digitised sound), `0x224` the first letter of the
video driver's name (`'V'` keeps `MGAVIDEO.DVR`; any other letter replaces the `M`),
`0x225` the music device letter for slot `0x424` and `0x226` the digitised-sound device
letter for slot `0x438`. The rest is T-039.

Start-up order (`1000:0002` onwards): `config.dvr`, `sounddvr.dvr`, `mgavideo.dvr`;
later (`1000:fb1c`) the music (`MAINTHEM.MS3`), the `.vcl` intro (`vcl.md`),
`vgaintro.dvr`'s drop sequence, and digitised sound with `dsounds.bin`.

## Hook chains

A hook chain is a far jump that can be patched. A chain node is an optional `nop` and a
5-byte `jmp far`, and its handler code follows it directly. Insert (slot `0x286`,
chain head at `DS:SI`, node at `ES:BX`) points the head's jump at the code after the
node and gives the node the head's old target; the handler ends by jumping back to its
own node, which continues down the chain. Remove (slot `0x28a`) unlinks the node.

The host's retrace interrupt (`1fe6:31d8`) is IRQ 0 with the PIT re-armed every frame
to fire just before vertical retrace (`gfx_sync_retrace_timer`, slot `0x348`, measures
the frame period). Its per-frame routine `gfx_on_retrace` (`1fe6:32a3`) applies the
pending pixel panning and the screen shake (`video-modes.md`), calls `mgavideo.dvr`'s
retrace handler (slot `0x4f4`, the palette upload), the mouse pointer (slot `0x14b2`),
then runs two chains:

- **Per retrace** (head at `1fe6:3312`, slots `0x358` add, `0x35c` remove): every
  frame, so about 70.09 Hz in mode 13h and 59.94 Hz in the 320x240 mode.
- **60 Hz tick** (head at `1fe6:3369`, slots `0x360`/`0x364`): an accumulator advanced
  each frame by the frame period over 19,886 PIT ticks fires it 60 times a second,
  whatever the refresh rate. The same tick advances the game clock (`DS:0x374`). It is
  skipped while `DS:0x264` is set.

`sounddvr.dvr`'s music hook (node `0x01d0`, handler `0x01d6`) is added to the
**per-retrace** chain by its music init (slot `0x424`) unless the device is `'N'`, and
removed again with `0x35c`. The music and digitised-sound inits also hook slot `0x348`
(table at 0x52, slot `0x26`), replacing the host's frame measurement with the driver's
own, since the driver reprograms the PIT (7.2 kHz for digitised sound).

## Exports per module

**`sounddvr.dvr`.** Init installs the table at 0x0a; the music and digitised inits each
install one more (0x2a and 0x3a).

| Slot | Offset | What |
|---|---|---|
| `0x424` | `0x0151` | Music init; `AL` = device letter (`N` none) |
| `0x428` | `0x019d` | Music shutdown |
| `0x42c` | `0x01e4` | Start a song (`ES:BX` = loaded `.ms3`, `AX` flags; `ms3.md`) |
| `0x438` | `0x01f7` | Digitised-sound init; `AL` = device letter; needs a 286 |
| `0x43c` | `0x028e` | Digitised-sound shutdown |
| `0x440` | `0x0100` | Check a device letter |
| `0x444` | `0x02d7` | Load `dsounds.bin` (`dsounds.md`) |
| `0x448` | `0x03af` | No-op |
| `0x44c` | `0x01df` / `0x03b4` | Play a sound effect: the music table points it at an FM routine; the digitised table, installed after it at start-up, at the mixer's play routine |
| `0x450` | `0x0506` | Toggle a driver flag |
| `0x454` / `0x458` | `0x01ea` / `0x01f0` | Hold / release the driver (a counter); callers wrap BIOS mode sets and disk loads in it |
| `0x45c` / `0x460` | `0x0541` / `0x0542` | No-op / return the driver's segment in `ES` |

**`mgavideo.dvr`** (the 90-slot table is at offset 8 of its second data block,
`DS:0x61e`; the one-entry table is at offset 0):

| Slots | What |
|---|---|
| `0x470` / `0x474` | Enter mode 13h and install everything / back to text mode |
| `0x478`, `0x47c` | Null `ret` / `retf` |
| `0x480` | File-id name builder (`m@l.md`, `mxx.md`): copies the name for id `AX` to the name buffer `DS:0x64e`. Its code for the `MSPRITES` digit and the `.MXX` suffix is skipped by a jump; the host writes both itself |
| `0x484`, `0x488`, `0x48c` | Select the draw and text surfaces; set the row stride |
| `0x490` | Load file id `AX` into the block at `CX:BX` (no drawing; `vga240.mod` replaces it in mode B with the `.vcl` loader) |
| `0x494`, `0x49c` | Show a loaded full-screen picture with / without its palette |
| `0x498` | Show a loaded picture, fading its palette in at speed `AX` |
| `0x4a0`–`0x4e8` | Palette: set, blank, commit, fades to a colour or to a palette (`palette.md`; `0x4d8` is the `.vcl` fade) |
| `0x4ec`, `0x4f0` | Colour-animation blends (`pal_blend_anim_from_palette`, `pal_blend_anim_frames`): build a record's frame between the palette and frame 0, or between frames 0 and 1 (`palette.md`) |
| `0x4f4` | Per-retrace handler (palette upload); repointed to a second version on some configurations |
| `0x4fa`–`0x570`, `0x590` | Drawing: pixels, rectangles and frames, probably lines, and fonts (`0x590` selects the glyph renderer) |
| `0x546` | Stub for `LIGHT.MOD` (dead) |
| `0x574`–`0x58c` | Scaled model images (`mxx.md`, "Drawing"): `0x574` target, row table and clip rectangle, `0x578` anchor, `0x57c`/`0x580` get/select the scale pattern, `0x584` scale (8.8), `0x588` draw a prepared list of images (not traced), `0x58c` draw one row-span image, colour 0 transparent, mirrored if `DL` bit 0 |
| `0x59c`–`0x5bc` | Screen-region helpers: build a run-length region of one colour from a loaded picture and compile it into copy code, then copy through it; not traced further |
| `0x5c0`–`0x5ec` | Sprite blitters by directory handle (`AX`) into a rectangle (`DS:BX`): `0x5c0` anchor mode, `0x5c4`–`0x5d4` opaque, `0x5d8`–`0x5e8` colour 0 transparent, `0x5ec` a window of a raw sprite (`b@n.md`) |
| `0x5f0` | Remap a buffer through a 256-byte table, not traced further |
| `0x5f4`, `0x5fe`, `0x606` | Stubs for `vga240.mod` |
| `0x1550`, `0x1558` | Stubs for `vgaiff.dvr` |
| `0x1496`–`0x14b2` | Mouse pointer: show/hide, shape, save/restore under it, per-frame update |

**`vgaintro.dvr`** (T-038; table at its segment `0x41a`, offset 0): `0x1538` enter
(`0x2a52`), `0x153c` leave (`0x2a90`), `0x1540` run (`0x2ab4`), and `0x1544`/`0x1548`
(`0x2d33`, `0x2d3b`), which only load its data segment and return. The run routine
also calls the host's word slots `0x22a` (`1fe6:0432`, set the cursor on a text-mode
debug screen) and `0x240` (`1fe6:054f`, print a number there) with the frame number
every frame; nothing of that reaches the game's screen. See `fgd.md`, "The drop
sequence".

**`vga240.mod`**, **`mapdvr.dvr`**, **`vgaiff.dvr`**: see the summary
table and `video-modes.md`, `vcl.md`, `fgd.md` and `map.md`. `mapdvr.dvr`'s slot `0x14fc`
points at its `ret` and is unused.

## Screens requested

`.MRL` screens are loaded by file id with slot `0x490` and shown with `0x494` or `0x498`:
`SITROOM` (0x04, `1000:5405`), `TRAINMEN` (0x06, `1000:6e8a`), `MAPROOM` (0x08,
`1000:71de`), `SCORPCON` (0x0c, `1000:7fbe`), `HUMANCON` (0x0e, `1000:9cc2`),
`SCOUTCON` (0x10, `1000:b47e`), `REVIEW` (0x12, `1fe6:7837`) and `LOGIN` (0x14,
through `1000:76fe` from `1000:70ec`). The only other callers pass `.vcl` ids (the
sequence player). `TITLE` (0x00), `OPTIONS` (0x02) and
`WALL2` (0x0a) are never requested.

## Open questions

- Names for most of `mgavideo.dvr`'s drawing slots below `0x5c0` and the host's
  `0x38a`–`0x40e` text services (T-031 and later T-090 passes).
- The configuration record's full layout and the flag words (T-039).

## Symbols

Named in the Ghidra project in this pass (T-025); all `confirmed` unless marked.

| Address | Name | Purpose |
|---|---|---|
| `1000:1368` | `drv_load_mgavideo` | Load `?GAVIDEO.DVR` at start-up |
| `1000:448c` | `ui_run_map_editor` | Load `mapdvr.dvr`, run the editor, unload |
| `1000:fc71` | `drv_run_vgaintro` | Load, run and free `vgaintro.dvr` |
| `1fe6:0144` | `drv_install_word_table` | Slot `0x0a` |
| `1fe6:0197` | `drv_install_far_table` | Slot `0x12` |
| `1fe6:01b5` | `drv_clear_far_table` | Slot `0x16` |
| `1fe6:01be` | `drv_fill_far_table` | Slot `0x1a` |
| `1fe6:0227` | `drv_hook_far_table` | Slot `0x26` |
| `1fe6:025e` | `drv_unhook_far_table` | Slot `0x2a` |
| `1fe6:02b0` | `drv_patch_rep_stosd` | Slot `0x32` |
| `1fe6:02db` | `mem_alloc_copy_block` | Slot `0x36` |
| `1fe6:08f6` | `drv_run_config` | Run `CONFIG.DVR` and read its record |
| `1fe6:30b3` | `gfx_sync_retrace_timer` | Slot `0x348` |
| `1fe6:32a3` | `gfx_on_retrace` | Per-frame work and the two hook chains |
| `1fe6:33e9`–`3424` | `drv_add_retrace_hook`, `drv_remove_retrace_hook`, `drv_add_tick_hook`, `drv_remove_tick_hook` | Slots `0x358`–`0x364` |
| `1fe6:4477` / `44a4` | `drv_hook_chain_insert` / `drv_hook_chain_remove` | Slots `0x286` / `0x28a` |
| `1fe6:45e7` | `drv_load_overlay_at_cursor` | Slot `0x2e8` (unused) |
| `1fe6:463d` | `drv_load_module` | Slot `0x2ec` |
| `1fe6:4713` | `drv_unload_module` | Slot `0x2f0` |
| `1fe6:47dc` | `drv_load_sounddvr` | Load `SOUNDDVR.DVR` at start-up |
| `1fe6:47f5` | `drv_unload_lazy_module` | Slot `0x2f8` |
| `1fe6:83aa` / `83cd` | `ui_open_file_dialog` / `ui_save_file_dialog` | Slots `0x2fe` / `0x302` |

In the modules: `drv_init` / `drv_shutdown` at their stub targets (`mapdvr.dvr`
`0x1e`/`0x54`, `sounddvr.dvr` `0x7c`/`0xc5`, `vgaiff.dvr` `0x35e`, `vgaintro.dvr`
`0x2a3a`, `mgavideo.dvr` `0x0a`), and `gfx_enter_mode13h` (`mgavideo.dvr` `0xf5`).

Named in T-028 (sprite banks and the sky; details in `b@n.md` and `packed.md`): in
`xeno.exe` `file_load_block_any_form` (`1fe6:2827`, slot `0x2b0`), `file_open_any_form`
(`1fe6:285f`, slot `0x2b4`), `file_load_lzw_block` (`1fe6:29aa`),
`file_load_packed_block` (`1fe6:2dd9`), `mem_unpack_packed_variant_unused`
(`1fe6:2e1a`), `file_unpack_packed_stream` (`1fe6:2efb`), and the `gfx_*` sprite and sky
routines listed in `b@n.md`; in `mgavideo.dvr` the sprite blitters at slots
`0x5c0`–`0x5ec`.

Named in T-029 (`.MXX` models; details in `mxx.md`): in `xeno.exe` the loaders
`gfx_load_all_models` (`1000:7b55`), `gfx_load_model` (`1000:17e3`),
`gfx_load_model_single_view` (`1000:1652`), `gfx_load_model_quarter_turn` (`1000:16e7`),
`file_load_model_views` (`1000:195f`) and their helpers, and the 3D view's model
drawing (`gfx_draw_render_list` `1000:2e5b`, `gfx_draw_model` `1000:2fda` and its
variants, `gfx_get_model_facing_index`, `sim_get_bearing`); in `mgavideo.dvr`
`file_build_name_from_id` (`0x01a4`, slot `0x480`) and the model image routines at
slots `0x574`–`0x58c` (`0x3c2c`–`0x419f`).

Named in T-037 (colour-animation records and fades; details in `palette.md`): in
`xeno.exe` `pal_install_anim_slots` (`1000:13a4`), `pal_select_anim_list` (`13be`),
`pal_stop_all_anims` (`13d8`), `pal_hold_anims` (`13f2`), `pal_release_anims` (`13fe`),
`pal_start_anim` (`140b`), `pal_activate_anim` (`14a1`), `pal_deactivate_anim` (`14ac`),
`pal_get_anim_flags` (`14b7`), `pal_set_anim_direction` (`14c3`), `pal_set_anim_speed`
(`14d8`), `pal_find_anim` (`14e2`), `ui_show_screen_fading_in` (`76fe`),
`ui_edit_name_with_blinking_cursor` (`77ce`) and `pal_tint_for_time_of_day` (`ed70`);
in `mgavideo.dvr` `pal_start_fade_out_all` (`0x4ec`), `pal_start_fade_out` (`0x505`),
`pal_step_fade_out` (`0x5ba`), `pal_step_fade_in` (`0x736`), `pal_restore_saved`
(`0x7bd`), `pal_blend_anim_from_palette` (`0x854`), `pal_blend_anim_frames` (`0x8ac`),
`pal_on_retrace` (`0x90f`), `pal_on_retrace_outsb` (`0x9c3`, with `rep outsb`; slot
`0x4f4` uses it when the configuration's CPU class is `'1'` or more and `DS:0x616` is
not `0x5981`) and `pal_advance_colour_anims` (`0xa66`).

Named in T-038 (the intro drop; details in `fgd.md`), all in `vgaintro.dvr`, load
offsets: `gfx_enter_drop_mode` (`0x2a52`), `gfx_leave_drop_mode` (`0x2a90`),
`gfx_run_intro_drop` (`0x2ab4`), `file_load_pcx_to_video` (`0x2d43`),
`file_load_inline_name` (`0x2f17`), `gfx_set_unchained_768_split` (`0x39a4`),
`gfx_set_drop_origin` (`0x39eb`), `gfx_reset_layers_and_floor` (`0x3a4d`),
`gfx_draw_ceiling_full` (`0x3a80`), `gfx_reset_drop_script` (`0x3ada`),
`ui_step_credits_script` (`0x3af7`), the credits steps `ui_credits_op_draw` (`0x3b05`),
`ui_credits_op_wait_frame` (`0x3b11`), `ui_credits_op_fade_in` (`0x3b29`),
`ui_credits_op_fade_out` (`0x3b49`), `ui_credits_op_clear` (`0x3b64`),
`pal_show_credits_colour` (`0x3b24`), `pal_hide_credits_colour` (`0x3b44`),
`gfx_step_drop_frame` (`0x3b6d`) and `gfx_step_drop_frame_cave` (`0x3b7f`), the states
`gfx_drop_state_accelerate` (`0x3b91`), `gfx_drop_state_cruise` (`0x3bdd`),
`gfx_drop_state_slow` (`0x3c30`), `gfx_drop_state_hold_first` (`0x3c7e`),
`gfx_drop_state_hold_second` (`0x3c98`), `gfx_drop_state_end` (`0x3cb2`),
`pal_switch_to_cave_light` (`0x3cb9`), `dbg_print_frame_number` (`0x3cdc`),
`gfx_select_drop_state` (`0x3cea`), `gfx_scroll_layers_and_view` (`0x3cf9`),
`gfx_combine_floor_layers` (`0x3d63`), `gfx_draw_floor_changes` (`0x3da1`),
`gfx_draw_ceiling_changes` (`0x3e16`), `gfx_set_line_compare` (`0x3ea8`),
`gfx_put_pixel_and_clean_copy` (`0x3f1c`), `gfx_clear_split_area` (`0x3ff6`),
`ui_draw_text_list` (`0x4011`), `sim_wait_counter_reaches` (`0x40c5`),
`pal_copy_buffer_to_shadow` (`0x40d9`, slot `0x4a8`), `pal_mark_shadow_for_upload`
(`0x40eb`, slot `0x4b0`), `pal_copy_anim_colours_to_buffer` (`0x410b`),
`pal_start_anim_via_slot` (`0x4121`, word slot `0x316`), `sim_wait_retrace`
(`0x4153`) and `dbg_print_number_at` (`0x415f`). All `confirmed`.
