# `.vcl` cutscene pictures

## Status

**Confirmed** (T-018). `xeno_formats::vcl` reads all 14 files of a stock install. Each
one decodes to its last byte and gives a picture that looks right in its own palette.
The loader in `vga240.mod` has been read and matches the layout below.

A `.vcl` is **not** an animation. It holds one still picture and no frames, deltas or
timing. The cutscenes move by panning a 320x240 window over the picture and through
palette fades and captions, all driven by 26-byte sequence entries in `xeno.exe`
(see "How the game shows a picture", decoded in T-027).

## Files

| Rows | Files |
|---|---|
| 240 | `ealogo`, `presents`, `detect`, `war`, `annihil`, `triumph`, `win`, `nlose`, `slose` |
| 400 | `arrival`, `grndfall`, `emerge`, `aftermth`, `return` |

Sizes run from 5 KB (`ealogo`) to 133 KB (`nlose`). The files are not packed (they
have no `@` in the extension; `packed.md`).

`mgavideo.dvr`'s file-id table (`m@l.md`) gives the `.VCL` names ids 0x16 to 0x32, in
this order: `WIN`, `HLOSE`, `NLOSE`, `SLOSE`, `TRIUMPH`, `EALOGO`, `PRESENTS`,
`DETECT`, `ARRIVAL`, `GRNDFALL`, `EMERGE`, `WAR`, `ANNIHIL`, `AFTERMTH`, `RETURN`.
`HLOSE` (id 0x18) is not in a stock install, and the game never asks for it (see
below).

## Layout

| Offset | Size | Meaning |
|---|---|---|
| 0x000 | 768 | Palette: 256 `r, g, b` VGA DAC triples, 0..=63 each (`palette.md`) |
| 0x300 | to the end | The picture: PackBits, 640 pixels per row from the top, until the file ends |

There is no header and no stored size. The width is fixed by the display mode (a
640-pixel virtual line) and the height follows from the file length: the stream
expands to 153,600 bytes (240 rows) or 256,000 bytes (400 rows) in every stock file.
640x400 is all 256 KB of VGA memory, which is why `xeno_formats::vcl` refuses more
than 400 rows.

## Encoding

The same PackBits as `.m@l` (`m@l.md`): a control byte `n` of `0x00..=0x7f` copies the
next `n + 1` bytes, and `0x80..=0xff` repeats the next byte `257 - n` times. `0x80`
(129 repeats) is accepted by the loader but used by no stock file.

The encoder worked row by row: no run in any stock file crosses from one 640-pixel row
into the next. That is also the best evidence for the width from the data alone.

## How the game loads a picture

`vga240.mod`'s loader is at module offset 0x274. It is not in the module's first export
table. On entering the 320x240 mode (the routine slot `0x5f4` is repointed to), the
module hands a second table at offset 0x22 to a host installer (slot `0x26`). The
installer points **host slot `0x490`** at the loader, and slot `0x590` at offset 0xb8d,
next to the 1-bit glyph (caption text) renderer at ~0xc0a. `mgavideo.dvr` also exports slot
`0x490` (load offset 0x31d), but its version only loads the file into a memory block
(host slot `0x2b0`) and draws nothing, so outside the 320x240 mode a `.vcl` is never
drawn.

The loader:

1. Takes a file id in `AX` and a descriptor at `ES:BX` whose first word is the segment
   of a 768-byte palette buffer. It sets flags `0x22` in the descriptor's byte 2.
2. Builds the name through host slot `0x480` (the file-id name builder), opens the
   file (slot `0x2b4`) and reads the 768-byte palette into that buffer (slot `0x2bc`).
   It does not write the DAC itself.
3. Reads the rest through a 2 KB buffer, refilled as it empties, and decodes PackBits
   straight into video memory at `A000:0000`. It writes pixel by pixel, setting the
   sequencer map mask to 1, 2, 4, 8 in turn and stepping the address after every fourth
   pixel. So the stream is in plain left-to-right order and the planes are interleaved
   by the loader, not by the file.
4. Stops when the file runs out, with no length check, and closes the file (slot
   `0x2b8`).

## How the game shows a picture

**Decoded** (T-027) from `xeno.exe`'s sequence player, `1fe6:8fc4` (unpacked file
offset 0x19a24). Addresses are Ghidra `segment:offset` and `DS` offsets as in
`docs/re/dvr-protocol.md`. One call plays one 26-byte **sequence entry** (`DS:SI`),
always in the 320x240 mode (see "Sequences" below). All words are u16 LE.

| Offset | Meaning |
|---|---|
| 0x00 | `.vcl` file id, passed to slot `0x490` (the loader above) |
| 0x02, 0x04 | Window origin `x`, `y` in picture pixels, passed to slot `0x60e` as `BX`, `AX` (`video-modes.md`) |
| 0x06 | Effect: a byte offset into a six-word table of handlers in the player's segment (`1fe6:90e8`). 0 holds; 2 and 10 pan right (`x + 1` per step; 10 is a duplicate whose extra setup is overwritten); 4 pans left; 6 pans up (`y - 1`); 8 pans down |
| 0x08 | Effect length. Hold: 60 Hz ticks to wait (`sim_wait_ticks_polled`, `1000:71c3`: poll the abort hook, then wait until the game clock `DS:0x374` moves, which in the 320x240 mode it does every retrace; 0 loops 65,536 times). Pan: number of 1-pixel steps, so the window ends at `origin + length * step` |
| 0x0a | Caption: offset of a NUL-terminated string in the host's string segment (the segment word at `DS:0x60`; Ghidra `38fa`) |
| 0x0c, 0x0e | Caption `x`, `y`, in **picture** coordinates (the glyph renderer addresses the 640-wide virtual screen through the row table), so a caption placed after a pan sits at a fixed spot of the final window |
| 0x10 | End time: after the caption, the player waits until the retrace counter `DS:0x370` reaches this value. `1000:fc36` resets that counter to 0 whenever it starts a song, so this is a time since the music started, which keeps the pictures in step with it |
| 0x12 | Fade-in length in retraces (slot `0x4d8`; runs in the background during the effect and caption). Also copied to `DS:0x376`, a word that only the developer's debug display (`1fe6:3482`, writing to `B000`) shows |
| 0x14 | Fade-out length in retraces |
| 0x16 | Caption speed: retraces to wait after each character |
| 0x18 | Pan speed: retraces to wait before each step. Slot `0x60e` also waits until the previous origin has been applied by the retrace handler, so a step never takes less than one retrace (0 means one pixel per retrace). In order (`1fe6:9132`): a step, a wait, then `length - 2` times a poll, a wait and a step, then a wait and a step; so with a speed, two waits come before the second step |

What the player (`gfx_play_vcl_sequence_entry`) does, in order:

1. Blank the palette (slot `0x4b4`: shadow palette to black, wait for the upload).
2. Load the picture with slot `0x490`, its palette going to the buffer whose
   descriptor is `DS:0xec90` (the abort hook is polled before and after). Set the
   origin to (`0x02`, `0x04`); it is applied at the next retrace.
3. Start a fade from black to the picture's palette over `0x12` retraces (slot
   `0x4d8`: per retrace, every colour is scaled by a level that rises linearly from 0
   to 1, through a 64-entry table). It sets the colours to black, waits one retrace for
   that upload, then starts the fade, whose steps each show a retrace after they are
   computed. It ends one DAC level short of the picture's palette (`palette.md`).
4. Run the effect.
5. Draw the caption: font 6, colour 252, cursor at (`0x0c`, `0x0e`) (host text services
   `1fe6:489c`, `4959`, `491f`), one glyph at a time through slot `0x594`, waiting `0x16`
   retraces after each. Byte 9 is followed by a byte `n`: move the cursor `n` pixels
   right. A glyph with no bitmap is a control code in `vga240.mod`'s renderer; the
   captions use 0x0c, a new line back to the caption's `x`.
6. Hold until the retrace counter reaches `0x10`, polling the abort hook each time.
7. Fade colours 0..=251 to black over `0x14` retraces (slot `0x4c8`, at most `0x14`
   steps), then all 256 over `0x14 / 2` (slot `0x4c0`). The caption, in colour 252,
   outlasts the picture. Each fade first drops any pending upload, waits a retrace and
   reads the DAC back, so the first fade's last (black) step is never shown and the
   second starts from its next-to-last; the second's own black step is shown only by
   whatever comes next (`palette.md`).

Each step polls an abort hook (far pointer at `DS:0x154c`). The intro and `TRIUMPH`
routines set it to `1000:fccf` and nothing else writes it, so every cutscene uses that:
an input event with code 0x19 (the P key's scan code, so presumably pause) waits for the
next event, which it uses up (and puts the game clock `DS:0x374` back), and any other
event aborts. The retrace counter and any fade keep running during a pause. On abort
the player fades everything out over 32 retraces and returns with carry set; an abort
during a pan first calls slot `0x4e8` (put a running fade-in's target in the shadow),
but the fade out drops that upload at once, so it has no visible effect.

### Sequences

Every call of the player is between an "enter 320x240" (slot `0x5f4`) and a "leave"
(slot `0x602`), so every `.vcl` is shown in the 320x240 mode with square pixels, at
59.94 retraces a second. The entries live in `xeno.exe`'s data segment (file offset
`0x19c10` plus the `DS` offset).

| Routine | When | Music | Entries |
|---|---|---|---|
| `1000:fd0a` (`ui_play_intro_cutscene`) | Start-up, right after `MAINTHEM` starts | `MAINTHEM` (already playing) | Blanks the palette, enters the mode, waits 30 ticks (it ignores the abort hook's answer there, so a key pressed then is lost), then ten entries back to back at `DS:0xecac`: `EALOGO`, `DETECT`, `ARRIVAL`, `GRNDFALL`, `EMERGE`, `WAR`, `ANNIHIL`, `AFTERMTH`, `RETURN`, `PRESENTS`. An abort ends the chain and also skips the drop sequence that follows (`fgd.md`) |
| `ui_play_win_cutscene` (`1000:7059`) | A mission is won (`DS:0x5fab` = 1) | `WIN1` (song 2), started before the mode change | Fills the caption's three number fields (below, "The win caption"), starts the song, then `ui_play_ending_sequence_entry` (`1000:fde8`): hold the colour records, blank, enter the mode, play `WIN` (the entry at `DS:0x606a`), leave the mode, release the records. Then it stops the music and waits 15 ticks in mode 13h |
| `ui_play_lose_cutscene` (`1000:702d`) | A mission is lost (`DS:0x5fab` = 2) | `WIN2` (song 4), the same way | As the win, with the entry pointed to by the word at `DS:0x6016 + [DS:0x6012]` |
| `ui_play_triumph_sequence` (`1000:fd9f`), from `ui_play_campaign_won_cutscene` (`1000:70d0`) | Instead of either, whenever `DS:0x5fac` is set | `BIGWIN3` (song 6), started inside, after the mode change | Sets the abort hook, holds the records, blanks, enters the mode, starts the song, waits 120 ticks (2 s), `TRIUMPH` at `DS:0xedb0`, waits 90 ticks, stops the music, waits 15 ticks, leaves the mode |

After a mission (`1000:6f9e`) the map room's action sets `DS:0x5fab` to 0, the lose
selector `DS:0x6012` to 0 and the scenario's `state` to 12, runs the mission, and fades
everything out over 32 retraces with the colour records held. Then it checks
`DS:0x5fac` **first**, so `TRIUMPH` replaces whatever the result was once the campaign
is complete, then 2 (lose) and 1 (win); any other value (0, no result) plays nothing.
Each routine returns at once if `DS:0x5fab` is 3 (the training menu's value), and
hides the mouse pointer around the cutscene (slots `0x149e`, `0x14a2`). The waits are
`sim_wait_ticks_polled` (60 Hz ticks, the abort hook polled before each): a key ends a
wait early. Nobody looks at the player's carry, so an aborted cutscene goes on to the
waits that follow it. The win and lose routines do not set the abort hook; the intro,
which always runs first, has set it already. The music is not modelled in the port
(T-062), but each song's start resets the retrace counter `DS:0x370` the end times count
from: the win and lose songs start just before the palette blank and the mode change,
`BIGWIN3` just after, so `TRIUMPH`'s end time counts from before its 120-tick wait.

The picture sizes and the entries agree: every vertical pan on a 400-row picture covers
the 160 rows below the window, and every pan from `x` = 320 ends at 0.

- **Intro.** `EALOGO`, `DETECT`, `WAR` and `PRESENTS` hold; `ARRIVAL` and `RETURN` pan up
  over the whole picture, `GRNDFALL` and `AFTERMTH` pan down, `EMERGE` pans left over
  its full width and `ANNIHIL` pans right part of the way. Every entry but the two logos
  has a caption. The last entry ends a little over two minutes into the music.
- **Lose.** `DS:0x6012` is reset to 0 before a mission starts and set to 0, 2 or 4 by
  the humanoid, scorpion and scout consoles' routines (`1000:a0ce`, `8471`, `bb9b`), so
  it is the type of the robot console the player opened last; a mission in which no
  console was opened counts as the humanoid's. The table gives `SLOSE` for 0, `NLOSE`
  for 2 and `SLOSE` for 4. The `HLOSE` entry at `DS:0x601c` is referred to by nothing;
  the humanoid's slot, the first, points at `SLOSE` instead, so `HLOSE` was presumably
  cut and a stock install never asks for `hlose.vcl`. The three lose entries share one
  caption.
- **`TRIUMPH`.** `DS:0x5fac` is set by `scen_update_campaign_progress` (`1fe6:7366`)
  when the count of won scenarios `DS:0x5f9f` equals the number of `scendef.bin` records
  (`DS:0x5ffd`), so when the campaign is complete (`docs/screens.md`).

### Captions

The player reads a caption from the segment whose word is `DS:0x60`. That word is 0 in
the image; at start-up the kernel (`1fe6:00a3`) points `DS:0x64` at a small segment of
fix-ups and installs its first table with `drv_install_word_table` (`1fe6:0144`, slot
`0x0a`): a base word (the load segment), then (`DS` offset, segment) pairs up to a 0
offset, each segment relocated. One pair sets `DS:0x60` to the host's string segment
(Ghidra `38fa`, image paragraph `0x28fa`), the same segment that slot `0x3c2` loads
directly and the map room's and review's texts come from. A caption with no text points
at an empty string (the two logos).

Typing (`1fe6:9032` on): select font 6, colour 252, cursor at the entry's (`0x0c`,
`0x0e`). Then for each code: poll the abort hook (a key aborts as anywhere else), read
the code; NUL ends the caption; 9 takes the next byte `n` and moves the cursor `n`
pixels right (`1fe6:4946`, the margin unchanged) without drawing or waiting; any other
code goes to the glyph slot `0x594`, which draws a glyph or runs a control code (the
stock captions use 0x0c for a new line and 0x0e to move one pixel right), and is then
followed by a wait of `caption_speed` retraces of `DS:0x370` (0: none). So control codes
cost a wait like a glyph; 9 and its byte do not.

### The win caption

Before playing `WIN`, `ui_play_win_cutscene` writes three numbers into its caption in
the string segment, so the stock caption holds placeholders at fixed offsets:

1. `sim_get_mission_kills_and_losses` (`1fe6:6b0d`) returns two bytes of the player's
   command object (`DS:0x568e`): its kills, enemy robots it destroyed (+0x2c), and its
   losses, its own robots killed (+0x2b).
2. `txt_format_u16_to_buffer` (`1000:02b7`) formats a number into a buffer at
   `DS:0x1bfe` by pointing the glyph slot at a byte store and calling
   `txt_print_u16_five_digits` (`1fe6:4a6c`): five decimal digits **with** leading
   zeros (its zero-suppression flag starts set), then a NUL.
3. The last two digits of each count are copied into the caption (`mov es, [0x60]`,
   `mov es:[field], ax`), and all five digits and the NUL of the mission's points
   `DS:0x6014`. The points field ends the caption.

The points come from `scen_score_mission_result` (`1000:7626`): the kills times the
word at `DS:0x5ed8` less the losses times the word at `DS:0x5eda` (16-bit products), 0 if
that would be negative; it also adds the counts to the campaign's totals
(`DS:0x5fa3`, `0x5fa5`). In the stock game the weights are 50 and 25. A won mission then
adds a bonus (1000 in the stock game) to `DS:0x6014` itself before adding it to the score
(`scen_win_selected_scenario`, T-046, `docs/screens.md`), and the caption is filled after
that, so the `WIN` caption's points include the bonus. A count of 100 or more shows only
its last two digits.

### In the port (T-036, T-045)

`xeno-app` plays the intro chain at start-up (`--no-intro` skips it, `--view` too),
from the user's `xeno.exe` (unpacked by `xeno_formats::lzexe`, entries read by
`xeno_formats::sequence`), in a 320x240 view on a 640x400 surface at 59.94 retraces a
second, then returns to mode 13h at 70.09. `crates/xeno-app/src/intro.rs` turns each
entry into the steps above and runs them between retraces, with every retrace doing
what the interrupt does first (count, apply the origin, upload the palette, step the
fade, count the tick); the palette is `xeno_render::Dac`. Holds, pans, fades, the
abort and pause hook and the end times are reproduced; still missing:

- **Captions** (T-045): read from the string segment (`xeno_formats::sequence`) and
  typed as above, in the user's font 6 from `mgavideo.dvr`, colour 252, transparent,
  at the entry's picture coordinates on the 640-wide surface: each code after a poll,
  then `caption_speed` retraces; 9 and its byte move the cursor with no wait; the
  font's control codes (0x0c, 0x0e) cost a wait. A key while typing aborts.
- **Music**: the counter `DS:0x370` is reset when `MAINTHEM` starts, a few retraces
  before the chain (the palette blank, the mode set and the frame re-measure come
  between). Without audio the port starts it at 0 with the chain.
- Time spent loading pictures from disk and setting the mode is not modelled.

The cutscenes after a mission (T-045) use the same player (`intro::Cutscene`, kinds
`Ending` and `Triumph`): the screens fade out over 32 retraces and hand the window the
cutscene (`Screens::take_cutscene`), which switches to the 320x240 mode with the clock
restarted, plays it as its routine does, then goes back to mode 13h with the clock
restarted and the map room fading in from black. The lose picture follows the robot
console opened last in the mission (reset to the humanoid's when a mission starts), as
`DS:0x6012` does. The win caption's numbers come from the campaign's won routine
(T-046): the port's missions do not count kills or losses yet, so they are 0 and the
points are the bonus alone (1000 in the stock game). Without music the counter starts at
0 with the cutscene (`TRIUMPH`: one retrace later, where `BIGWIN3` would start). One
difference: the 15 ticks the routines wait after leaving the mode are waited before
it, still in the 320x240 mode (the screen is black either way). `--screen won`, `lost` and `triumph` play one alone, then the map room; a base
destroyed in play, or Ctrl+C, ends a mission in one.

With the stock entries the chain lasts about 7,690 retraces (2 min 8 s) when not
aborted: `PRESENTS` is held until counter 7,640.

## Open questions

- A DOSBox capture to confirm the timing frame by frame (T-036's note in
  `NEXT_STEPS.md` lists what to check, with the checklist in `video-modes.md`).
- The descriptor flags the loader sets (`0x22`), and what slot `0x590` (offset 0xb8d)
  does beyond selecting the glyph routine.

## Parser

`xeno_formats::vcl` (T-013 for the palette, T-018 for the picture): `parse` returns a
`Picture` with the palette, the row count and the pixels, and `runs` the PackBits runs.
`xeno-tool inspect` prints the size and run counts, and `xeno-tool dump` writes the
whole picture as one PNG.

`xeno_formats::sequence` (T-036): `parse` reads one 26-byte entry (checking its file
id and effect), `data_segment` finds `DS` in an unpacked `xeno.exe` and `intro_chain`
the ten start-up entries. `xeno-tool inspect xeno.exe` lists them. T-045 added
`string_segment` (from the start-up fix-ups, checking the two instructions and that
every segment word is relocated) and `captions`.

`xeno_formats::ending` (T-045) reads the cutscenes after a mission from the
instructions that use them, after checking their bytes: the `WIN`, three lose and
`TRIUMPH` entries, the selector word, the score's two weights, the win caption's five
copies (value, word of the number, caption offset; each must land before the caption's
NUL) and the captions; `Endings::win_caption` fills the fields as the game does and
`points` scores a result. `xeno-tool inspect xeno.exe` shows their shape, never their
text.
