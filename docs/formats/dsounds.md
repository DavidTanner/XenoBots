# `dsounds.bin` digitised sound effects

## Status

**Confirmed** (T-023) for the layout, the sample encoding and the playback rate, from
the loader and the timer-interrupt mixer in `sounddvr.dvr`. `xeno_formats::dsounds`
reads the stock file to its terminator; `xeno-tool dump` writes one WAV per sound.

## Files

| File | Size | Named by |
|---|---|---|
| `dsounds.bin` | 97,268 | `sounddvr.dvr` (as `DSOUNDS .BIN`, padded to 8.3 fields) |

Not packed (no `@` in the extension), and no directory: just blocks back to back.

## Layout

The file is a list of blocks, read in order until a zero count:

| Offset | Size | Type | Meaning |
|---|---|---|---|
| 0 | 2 | u16 LE | `paragraphs`: block size in 16-byte paragraphs; 0 ends the file |
| 2 | `paragraphs * 16` | | The block |

Each block:

| Offset | Size | Type | Meaning |
|---|---|---|---|
| 0x00 | 2 | u16 LE | `end`: offset in the block where playback stops (exclusive) |
| 0x02 | 2 | u16 LE | `step`: play-position increment per driver tick, in 1/65536ths of a sample |
| 0x04 | `end - 4` | u8 each | Samples |
| `end` | to the block end | | `unknown_tail`: never played |

The stock file has 17 blocks, the most the driver accepts (its slot table has 17
entries), and nothing after the terminator. 16 blocks have `step` 0xffff and one
(block 3) has 0x4000. Most tails are under 16 bytes of padding that repeat the
block's own header and first samples, like a reused buffer. Block 9's tail is about
10 KB of smooth audio that continues the sample: the sound was apparently cut short by
lowering `end` without removing the data.

## Encoding

Unsigned 8-bit mono PCM, silence at 0x80.

The driver programs PIT channel 0 with divisor 165 (mode 3) and puts its mixer on
IRQ 0, so it ticks at 1,193,182 / 165, about **7231.4 Hz**. Each tick, for each of its
three voices, it adds `step` to a 16-bit fraction, advances the position by the carry,
fetches the byte there, and ends the voice once the position reaches `end`. A sound
therefore plays at `7231.4 * step / 65536` samples per second: **7231 Hz** for
0xffff, and 1808 Hz for block 3. `Sound::sample_rate` gives this, rounded.

## How the driver uses it

Addresses are offsets in `sounddvr.dvr`'s load image (file offset minus its 0x200-byte
MZ header), as Ghidra shows them in segment `1000`.

- **Load** (`0x02d7`, exported for host slot `0x444`): opens the file through host
  services, then for each block reads the count, allocates that many paragraphs,
  reads the block into the new segment at offset 0 and records the segment in the
  host's 17-slot table (pointer at host `DS:0x41a`, entries of segment and flags).
  It fails if a read comes up short or a 18th block appears.
- **Play** (`0x03b4`, host slot `0x44c` in the same export table, which the
  digitised-sound init installs; before that the slot points at an FM routine,
  `docs/re/dvr-protocol.md`): `AL` is the slot, `AH` the voice (0..2;
  bit 7 queues the sound to start when the voice's current sound ends), `CX` a step
  that overrides the file's when non-zero, `DL` a volume (each sample is shifted
  right by `DL` and re-centred). `AL = 0xff` silences the voice (at once, or queued
  with bit 7).
- **Mixer** (four routines, one per output device, chosen at init: PC speaker, AdLib,
  Disney Sound Source, Sound Blaster): sums the three voices' bytes with the previous
  output, divides by four and sends the result to the device (T-065).

Details of the mixer and its four back ends are in [`sounddvr.md`](sounddvr.md),
"The effects mixer".

## Sound events

**Confirmed** (T-065) from every caller in `xeno.exe`. The game plays effects through
a table of 29 **sound events** at `DS:0x7e18`, 4 bytes each: the block (`AL`; 0xff
plays nothing), the voice and flags (`AH`: voice 0 to 2, bit 7 queued), and a step
override (`CX`). Callers pass **twice the event number** in `AX` (so the `0x2a` and `4`
in `docs/sim.md` are events 21 and 2); `AX = 0xffff` plays nothing. Three routines
use the table:

- `snd_play_sound_effect` (`1000:ee90`): plays the event (`DL` = 0, full volume) and
  records it in `DS:0x7e14` (written, never read).
- `snd_play_sound_effect_if_viewed` (`1000:eec6`): the same, but only when `DI` is
  the object whose console is open (`DS:0x7e16`, set by `ui_open_view_of_object`). Most
  robot sounds go through it, so only the robot being watched is heard.
- `1000:eeb1` (not a function in the project): "stops" the event's voice by playing
  block 0xff with the event's flags; event 28's flags queue, so its sound finishes.

Every entry's step override is 0, so each block plays at its file's rate.
`xeno_formats::sounds` reads the table from the user's `xeno.exe`.

| Event (`AX`) | Block | Voice | Queued | Trigger | Played by (`1000:`) | Heard |
|---|---|---|---|---|---|---|
| 0 (0x00) | 9 | 0 | | A humanoid fires its missile (a kind-8 shot, cool-down 50) | `sim_fire_humanoid_missile` (`b16e`) | viewed |
| 1 (0x02) | 10 | 0 | | A kind-7 shot is fired: a humanoid's cannon (cool-down 12), a scout's plasma cannon | `sim_fire_humanoid_cannon` (`b0f6`), scout `c965` | viewed |
| 2 (0x04) | 6 | 1 | | A robot is destroyed; a destroyed base's big explosion (end step 20) | `sim_destroy_robot` (`de65`); base end | all |
| 3 (0x06) | 6 | 1 | | A destroyed base's end, steps 0, 5, 12, 17 | `snd_play_base_end_step_sound` (`e708`) | all |
| 4 (0x08) | 11 | 1 | | A shot or mine explodes: kind 5 (`e20d`), kind 6 (`e318`, `e33d`), kind 7 (`e464`), kind 8 (`e584`), a mine, kind 9 (`e5d3`, "MINE DETONATED"); base end steps 3, 8, 15 | the kinds' ticks; base end | all |
| 5 (0x0a) | 3 | 0 | | A kind-6 shell is fired: a scorpion's rapid-fire cannon (heard by all), a scout's cannon (viewed); base end steps 10, 16 | scorpion `9884`, scout `c9ee`; base end | see trigger |
| 6, 7, 8 | none | 2 | yes | Nothing | never played | |
| 9 (0x12) | 13 | 2 | | Lock warning: another robot keeps a target lock on this one (`1000:d6ad` sets its +0xed to 0xff; it halves each pass, and the warning sounds on some passes while it is not 0) | scorpion `9a39`, humanoid `b347`, scout `cb1f` | viewed |
| 10 (0x14) | 0 | 2 | | The console's display mode (+0xef) changes: the keys pick 0, 1 or 2, the scorpion's missile camera is 3 and falls back on its own when the missile is gone | `ui_set_humanoid_display_mode` (`b3e9`), scorpion `99c7`, scout `cbc1` | all (only consoles call it) |
| 11 | 1 | 2 | | Nothing | never played | |
| 12 (0x18) | 2 | 0 | | A scorpion fires its tail missile (a kind-5 shot, one at a time) | scorpion `97da` | viewed |
| 13 (0x1a) | 8 | 1 | | Humanoid footstep: a leg's frame (+0x101, +0x102) wraps to 0 | `ae11` | viewed |
| 14 (0x1c) | 7 | 1 | | Scorpion footstep: when it moves, its six legs' frames are set from a table by the step, and an even step plays it | `sim_step_scorpion_leg_frames` (`95dd`) | viewed |
| 15 (0x1e) | 7 | 1 | | Scout footstep: a leg's frame (+0x103, +0x104) wraps to 0 | `snd_play_scout_footstep` (`c84e`) | viewed |
| 16 to 19 | none | 0 | | Nothing | never played | |
| 20 | 11 | 2 | | Nothing | never played | |
| 21 (0x2a) | 11 | 2 | | A robot is hit (also for 0 points, so the hill bounce) | `sim_damage_humanoid`, `sim_damage_scorpion`, `sim_damage_scout` | viewed |
| 22, 23, 24 | none | 0 | | Nothing | never played | |
| 25 (0x32) | 5 | 2 | | A scorpion deploys a relay from its cargo bay | scorpion `9b20` | viewed |
| 26 (0x34) | 12 | 2 | | A target lock is taken ("TARGET ACQUIRED") | `sim_lock_target` (`d8af`) | viewed |
| 27 (0x36) | 14 | 2 | | The selected weapon (+0xdd) changes | `sim_select_scorpion_weapon` (`98a9`), humanoid `b23d`, scout `ca37` | viewed |
| 28 (0x38) | 16 | 2 | yes | Display noise: the open console's display mode has bit 2 set (the mode's device, by a table at `DS:0x70be`, is damaged in +0xf6, or the scorpion's missile camera was just lost); played when the bit sets, the voice "stopped" (silence queued) when it clears or the console closes | `daa2`, `dac0` | the open console |

"Viewed" means `snd_play_sound_effect_if_viewed`. Shot kinds are `usg.md`'s object
kinds (5 to 9 have their ticks at `DS:0xe6e8`); deploying mines and buoys plays
nothing.

**Blocks 4 and 15 are never played**: no event names them, and nothing else calls
slot `0x44c`. Block 1 is named only by event 11, which nothing plays, so it is never
heard either. The other 14 blocks are.

## Open questions

- Which of the 0, 1, 2 display modes (+0xef) is the camera, map or radar.

## Parser

`xeno_formats::dsounds` (T-023): `parse` returns a `Bank` of `Sound`s, each with the
`binrw` `Header` (`end`, `step`), its samples and its `unknown_tail`, plus any
bytes after the terminator. `xeno-tool inspect dsounds.bin` lists each sound's
length, rate and duration; `xeno-tool dump dsounds.bin --out <dir>` writes
`dsounds_NN.wav` (8-bit unsigned mono at the sound's rate).
