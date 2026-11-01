# Palettes

## Status

**Confirmed** for the sources, the upload, the fades and the colour-animation records
(T-013, T-036, T-037). Every image in the game is 8-bit indexed and drawn through one
256-colour VGA palette at a time. The game has no standalone palette file. Each picture
brings its own palette:

| Source | Encoding | Where in the file | Status | Read by |
|---|---|---|---|---|
| `.pcx` (`xenodrop`, `cavedrop`) | 8 bits per channel | 768-byte trailer after the `0x0c` marker (see `pcx.md`) | confirmed | `xeno_formats::pcx` |
| `.vcl` (all 14) | VGA DAC, 6 bits per channel | First 768 bytes of the file (`vcl.md`) | confirmed; `vga240.mod`'s loader reads it into a buffer of the caller's | `xeno_formats::vcl` |
| `.m@l` screens (all 8) | VGA DAC, 6 bits per channel | First 768 bytes of the screen after unpacking (`m@l.md`) | confirmed | `xeno_formats::mal` |
| IFF ILBM/PBM `CMAP` | 8 bits per channel (IFF standard) | `CMAP` chunk | `vgaiff.dvr` only; no IFF file is in the install (see below) | no |
| Colour-animation records | VGA DAC, 6 bits per channel | Lists in `mgavideo.dvr`'s and `vgaintro.dvr`'s data (see below) | confirmed (T-037) | `xeno_render::anim` runs them; `xeno_formats::mgavideo` reads `mgavideo.dvr`'s list (T-041), `xeno_formats::vgaintro` the drop's |

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
  slot `0x4f4`: upload the shadow if dirty, run the colour-animation records (below),
  then call the fade routine, which writes the next shadow and marks it dirty. So a
  step shows one retrace after it is computed.
- **Slots** (T-037, from the driver's slot table): `0x4c0` / `0x4c4` fade all 256 out
  and wait / start it; `0x4c8` / `0x4cc` the same for `BX..=CX`; `0x4d4` / `0x4d8` fade
  all 256 in from the palette at `ES` (`0x4d8` takes it in `DX`) and wait / start it;
  `0x4dc` / `0x4e0` the same for `BX..=CX`. The "start" forms of a fade in still wait
  for the black upload. `0x4a8` copies a palette into the shadow without marking it,
  `0x4ac` does the same and marks it, `0x4a4` drops a pending upload, `0x4b0` marks
  the shadow for upload.
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
- **Colour range.** The routines take a first and last colour (`BX`, `CX`) and
  handle the bytes of their range correctly when they copy or read back, but the step
  loops from byte `first * 3` up to `count * 3`, `count` being the number of colours.
  A fade in copies colours `first..=last` of the target into the shadow and the saved
  buffer and blackens them, but scales only `first..=last - first`; the rest stay
  black. A fade out reads colours `first..=last` back into the start of the saved
  buffer, so colour `c` fades from what colour `c + first` showed, again only for
  `first..=last - first`. When `last - first < first` the loop count wraps and the step
  runs 65,536 times over the memory after the buffers (the port scales nothing). Only a
  first colour of 0 works as intended, which is all the `.vcl` player uses (colours
  0..=251, then 0..=255). `xeno_render::Dac` keeps these quirks.
- **The saved buffer** (`DS:0x696`) persists between fades: `0x4e8` copies all 768
  bytes of it back into the shadow while any fade runs, so after a partial fade in,
  colours outside its range get whatever an earlier fade left there.
- **Back to back** (the player's two fades out): the second stops the first in the same
  retrace its black step was computed, dropping that upload, so the second starts from
  the first's next-to-last levels. The DAC shows those for three retraces before the
  second's first step.

### Colour-animation records (T-037)

Read in `mgavideo.dvr` (load offsets: the retrace handlers `pal_on_retrace` 0x90f and
`pal_on_retrace_outsb` 0x9c3, `pal_advance_colour_anims` 0xa66, the blends at slots
`0x4ec`/`0x4f0`) and in `xeno.exe` (`1000:13a4`–`14f9`). Reproduced by
`xeno_render::anim::ColourAnims`, run by `xeno_render::Dac`.

**The list.** Records sit back to back from offset 0 of one segment, each starting with
its size; a size of 0 ends the list. Which list runs is a host word, `DS:0x322`, that
points at a memory-block descriptor whose first word is the list's segment.
`pal_install_anim_slots` (`1000:13a4`, at start-up) selects the driver's own list:
`mgavideo.dvr`'s third data block (descriptor `DS:0x622`). Colour pointers are offsets
from the start of the list, so a record only works at the place it was built for.

| Offset | Size | Meaning |
|---|---|---|
| 0x00 | u16 LE | Size of this record in bytes; the next record follows it. 0 ends the list |
| 0x02 | u8 | Flags: bit 7 active; bit 6 direction; bit 5 stop at the end of this pass; bit 4 loop; bit 3 chain (start the next record when this one stops); bits 0-2 the kind |
| 0x03 | u8 | Countdown: the record fires when it reaches 0 |
| 0x04 | u8 | Speed: the countdown's reload, so retraces between steps (0 is 256) |
| 0x05 | u8 | First DAC index to write |
| 0x06 | u8 | Number of colours written (`count`) |
| 0x07 | u8 | Rotate only: how many colours wrap round to the start of the data (`pos`) |
| 0x08 | u16 LE | Offset (in the list) of the colours to write next |
| 0x0a | to the end | The colours, VGA 6-bit `r, g, b` triples |

**Each retrace**, in the driver's handler: the dirty shadow palette is uploaded, then,
unless the records are held (`DS:0x695` non-zero) or the host's pause byte `DS:0x264`
is set, two passes run over the list, then the fade step (`0x4f8`) runs:

1. **Fire.** Every active record's countdown drops by one (wrapping). At 0 its colours
   go **straight to the DAC**, starting at its first index (the DAC's index wraps
   after 255): `count` colours from the pointer, or for a rotate `count - pos` colours
   from the pointer and then `pos` from the start of the data.
2. **Advance.** Every active record whose countdown is 0 (those that just fired)
   reloads it from the speed byte and moves on by its kind:
   - **1, rotate.** The data is a ring of `N = (size - 10) / 3` colours and the record
     shows a window of `count` of them starting at ring position `s` (pointer = data
     + 3s; `pos = max(0, s + count - N)` is the part that wraps). Bit 6 clear moves
     `s` up by one, bit 6 set down by one, wrapping at the ends, so colour `first + i`
     shows ring colour `(s + i) mod N`. With `N == count` it is a plain rotation.
   - **2, frames.** The data is frames of `count` colours. Bit 6 clear steps back a
     frame, bit 6 set forward. Past either end the pointer wraps to the other end; then
     the record keeps going if bit 4 is set and bit 5 clear, and otherwise stops
     (clearing bits 7 and 5) and, if bit 3 is set, activates the next record with this
     record's speed as its countdown.
   - **3 and 4, blends.** Turn themselves off: they show one prepared frame once.
   - 0 and 5..7: only the countdown runs.

**Starting a record** (word slot `0x316`, `pal_start_anim` `1000:140b`; `AL` the index
in the list, `AH` a weight): the record is turned off, then by kind: a rotate goes to
ring position 0; frames go to the first frame if bit 6 is set, else to the last; kind
3 builds frame 1 from the current palette (the shadow, through `DS:0x68e`) towards
frame 0 and kind 4 builds frame 2 from frame 0 towards frame 1, each byte moved
`(|distance| * 2 * weight) >> 8` levels (so rounding towards the start), a weight of
128 or more giving the end frame itself (and for kind 4, 0 the start frame); the
pointer is set to the built frame. Then the countdown is set to 1 and the record
turned on, so it shows at the next retrace. Kind 0 is accepted but left off; kinds
5..7 return carry set.

The other host services, all near routines in `xeno.exe` exported as word slots
(`docs/re/dvr-protocol.md`): `0x30e` select a list (`AX` a segment, or 0 for the
driver's own; returns the previous one), `0x310` turn every record off, `0x312` /
`0x314` hold and release the records (a counter that saturates both ways), `0x318` /
`0x31a` turn record `AL` on or off where it stands, `0x31c` read its flags, `0x31e` set
its direction from `AH` bit 6, `0x320` set its speed to `AH`.

**With fades.** Neither stops the other. Records write the DAC after the upload, so on a
retrace when a record fires its colours win, unfaded; on the retraces between, every
upload (every retrace while a fade runs) puts the shadow's values for those colours
back. Once nothing is uploaded the DAC keeps the record's last colours. A fade out
reads the DAC back, animated colours included. The game avoids the mix by **holding the
records** (`0x312`/`0x314`) around its cutscenes (`ui_play_intro_cutscene` and the
won/lost and campaign-won routines at `1000:fde8` and `1000:fd9f`) and around
`ui_show_screen_fading_in` (`1000:76fe`, the log-in screen's fade in).

#### The lists in the stock drivers

Contents are the user's to read, never committed; this is the shape only.

- **`mgavideo.dvr`'s own list**: 24 records in a 0xddf-byte block, every one off at
  start: blinks and frame cycles of single colours (253, 254, 255), two-frame blinks
  of 23..25 and 230..255, frame cycles and one-frame loads over 32..47, rotates over
  16..23, 24..31, 16..31 and 32..47, one-frame loads of 128..254, and blends (kind 3)
  over 49..63, 128..252 and 16..63. `xeno.exe` starts these directly:
  - record 23 (colour 254, on and off every 32 retraces) by
    `ui_edit_name_with_blinking_cursor` (`1000:77ce`), the log-in screen's name entry,
    which turns it off again when the name is done: the text cursor's blink;
  - records 5 and 6 (blends over 49..63 and 128..252) by `pal_tint_for_time_of_day`
    (`1000:ed70`), called from `sim_advance_clock` (unless `DS:0x7d4d` is set) and
    restarted whenever the weight changes: 128 (frame 0, night) before 06:00 and after
    18:59, `0x78 - 2 * minute` from 06:00 to 06:59 (so 120 at 06:00, down to 2), 0
    (the palette itself) from 07:00 to 17:59, `2 * minute` from 18:00 to 18:59 (0 up
    to 118, then 128 at 19:00). So the 3D view's colours darken at dusk and lighten
    at dawn with the mission clock (`DS:0x7d46` hours, `0x7d47` minutes), which moves
    4 sixtieths a retrace (`docs/sim.md`, "The mission clock"): once a retrace, but a
    new weight only every 900 retraces over the two fading hours. The last weight is
    kept at `DS:0x7d4c`; 0xff forces a restart, set at start-up (`1000:ecc2`) and by
    `1000:ecff`, which the situation room's and the consoles' view draws call after
    starting their own records (below) and `ui_sitroom_set_view_mode` calls on every
    mode key. `DS:0x7d4d` (no tint) is cleared by every mission view's draw (the
    situation room's at `1000:550a`, the three consoles' at `845f`, `a0c3`, `bb86`) and
    by the situation room's main-view routines for modes 0, 4, 6 and 8 (`DS:0x5ae0`);
    **mode 2's routine (`1fe6:704a`) sets it** and resets `0x7d4c`. So the tint runs
    on every mission view, the situation room included (its picture's colours in
    those ranges are tinted too), except while the situation room's main view is in
    mode 2 (S). A blend shows its frame once; the next upload of the shadow puts the
    palette back until the weight changes, which is why each view's draw forces a
    restart. The port (T-102): `xeno_sim::Clock::tint`, started by `xeno-app` on
    records 5 and 6 of the screens' list once no fade is pending or running.
  - **The views' records** (T-125). Each mission view's picture is a widget whose
    draw handler (table `DS:0x3328`) loads and draws the picture, sets its palette
    (slot `0x494`) and then starts the view's records with `pal_start_anim`, before
    restarting the tint (`1000:ecff`): the situation room (type 0x4e, `1000:5484`)
    records 0, 1, 0x10 and 0x11; the humanoid console (type 0x82) 0xa, 0xb, 2, 0x10
    and 0x11; the scorpion console (type 0xba) 0xd, 3, 0x10 and 0x11; the scout
    console (type 0x104) 0xc, 4, 0x10 and 0x11. The three consoles hold the records
    (`1000:13f2`) before the first start and release them (`13fe`) after the tint's
    restart, so no retrace runs a half-started set; the situation room does not. All
    of them are rotates or frames (none a blend), so the weight in `AH` (whatever
    the picture's palette call left there) is unused. What they animate: record 0, 2,
    3 or 4 blinks colour 255 between two colours (each record its own pair, every 32
    retraces), record 1 blinks 23..25 (every 16), record 0x10 cycles colour 253
    through 14 colours (every 3) and 0x11 colour 254 through 16 (every 2), all looping;
    record 0xa rotates 16..23 and 0xb 24..31 (every 5), 0xc and 0xd 16..31 (every 4).
    In the stock pictures 253 and 254 are only in the consoles' art and 16..31 only in
    the situation room's, so several records start on screens with no pixel of their
    colours (until the code draws some); colour 255 is every view's region for the 3D
    view and the second display (`docs/screens.md`), drawn over each frame. The picture widget's close handler (table
    `DS:0x31b8`, run on every widget of the set when the mission switches to another
    view's set, `1000:2690`) turns the same records off where they stand (slot
    `0x31a`) and switches the second display's record off (`1000:d9ef` with 0xff:
    `0x31a` on `DS:0x70d0`; the record by display mode, `docs/screens.md`, "The display's other modes (T-134)"); the next view's draw restarts the shared
    0x10 and 0x11 from their beginning. When the mission ends, `ui_map_room_run_mission`
    holds the records, turns every one off (slot `0x310`, its only caller) and fades
    out over 32 retraces, then releases them. The port (T-125):
    `xeno_app::mission::view_records`, started by `xeno-app` with the tint once no fade
    is pending or running, turned off as the view is left, all off when the mission is
    left. It starts them on showing a view only: the original restarts them whenever
    the picture widget is drawn again, which the port does not trace (T-180).
  T-037 missed the views' records (it saw only direct callers: the views call
  `1000:140b` near, not through slot `0x316`). Nothing in the `1fe6` segment calls
  through the word slots `0x30e`..`0x320` (a search of its code for each slot word
  found only other operands); in segment `1000` only `0x310` is called through its
  slot, every other service is called near. `mapdvr.dvr` is not searched.
- **`vgaintro.dvr`'s list** (its data at load offset 0x13090, 0x1de1 bytes): six
  records, all off at start. Records 0..=4 are blends from the palette (kind 3) over
  colours 0..=251 (two frames of 252 colours each, speed 5); record 5 is a blend between
  two frames (kind 4) over colours 252..=254, the credits text colour. Colour 0 is
  black in every record's target. The drop sequence (T-038, `fgd.md`, "The drop
  sequence") selects the list with `0x30e`, starts records 0, 1 and 2 before its frames
  and record 3 at the end, each with weights 0, 2, ..., 128 a retrace apart (65
  steps): cross-fades from the shadow palette to the record's colours. Between them it
  copies colours from the list into its palette buffer and the shadow without an
  upload (slot `0x4a8`), so the next blend starts from them: the first 192 colours
  of records 0 and 1, and at the cave's change of light 255 colours from record 4's
  start (its 252 colours and the first 3 of its second frame, as the list holds them
  then) after record 4 is started at full weight. Record 5 is started at a weight that
  rises or falls by 4 a frame (`fgd.md`, "Credits"). Then the previous list is put
  back.
- The `.m@l` screens and `.vcl` pictures carry no records; what animates on a screen is
  whatever the code starts.

**Reading them from the user's copy.** `mgavideo.dvr` is a plain MZ file: its load image
starts after the header (header paragraphs at MZ offset 8). Its init (load offset 0x0a)
copies five data blocks with slot `0x36`, each as `mov ax, size; mov bx, descriptor;
xor si, si; mov dx, paragraph; call far [0x36]`; the list is the one whose descriptor is
`0x622`, `size` bytes from load offset `paragraph * 16` (relocated paragraphs: subtract
the load segment). Reading the size and paragraph out of that sequence, rather than
hardcoding them, keeps it working on other releases. `vgaintro.dvr` passes its list's
segment in `AX` before calling `0x30e` from its run routine (slot `0x1540`);
`xeno_formats::vgaintro` reads the list from there (T-038). `xeno_formats::mgavideo` reads
`mgavideo.dvr`'s own list this way (T-041), with the copy's bytes checked at load
offset 0x28; the same routine finds the fonts (`font.md`).

## Open questions

- Settled (T-027): a `.vcl` palette reaches the DAC through the fade-in of slot
  `0x4d8`, over the number of retraces in the sequence entry (`vcl.md`). The cutscenes
  use no colour animation records.
- Settled (T-037): the countdown's reload is byte 4, and `pal_advance_colour_anims`
  moves the pointer (above).
- What sets the hidden switch (`DS:0x618 = 0xe263`) that `vgaiff.dvr`'s screenshot
  needs (T-039). Its IFF reader is never called (`docs/re/dvr-protocol.md`).
- Whether anything outside segment `1000` of `xeno.exe` starts the other built-in
  records. A DOSBox check of the dusk tint and the log-in cursor blink. (Settled,
  T-102: `DS:0x7d4d` is set only by the situation room's mode 2, above.)
- `mgavideo.dvr` code at load offset 0x450 (not in the slot table that was read) dims
  the shadow into a third buffer and points `DS:0x68e` at it while it waits for an
  upload, so a blend started then would mix against the dimmed colours. Who calls it
  is not known.

## Parser

`xeno_formats::vcl` (the `.vcl` palette and picture, T-018), the PCX trailer in
`xeno_formats::pcx`, the screen palette in `xeno_formats::mal` (T-017), and `Palette::from_vga6` / `Palette::from_rgb8` for raw palettes.
Added by T-013. The driver's shadow palette, fades and colour-animation records are
`xeno_render::Dac` and `xeno_render::anim` (T-036, T-037).
