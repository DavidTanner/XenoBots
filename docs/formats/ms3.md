# `.ms3` music

## Status

**Confirmed** (T-022) for the layout and the event stream, from the reader in
`sounddvr.dvr` that loads and plays them. `xeno_formats::ms3` reads all six files to
their end marker. The instrument bank the programs refer to is decoded in
[`sounddvr.md`](sounddvr.md) (T-063). The tick rate, the song table and the callers
are confirmed from `xeno.exe` and the driver (T-064, "When songs play" below); only a
DOSBox recording to check the timing by ear is left.

It is **not** an AdLib register dump and not a Standard MIDI File. It is a single
MIDI-like event stream with its own header, played by Kyle Freeman's "MS3" driver
(`sounddvr.dvr` carries "MS3 Copyright 1991 Kyle Freeman"; `KGF` in the signatures is
presumably his initials). The driver sends the same events to an AdLib (OPL2 at port
0x388) or to an MPU-401 MIDI interface (port 0x330), so instruments are program
numbers into a bank built into the driver, not OPL patches in the file.

## Files

| File | Size | Signature | Named by |
|---|---|---|---|
| `mainthem.ms3` | 13,165 | `MS3-KGF'91` | `xeno.exe` |
| `win1.ms3` | 1,465 | `MS3-KGF'91` | `xeno.exe` |
| `win2.ms3` | 471 | `MS3-KGF'91` | `xeno.exe` |
| `bigwin3.ms3` | 3,156 | `MS3-KGF'91` | `xeno.exe` |
| `lose.ms3` | 378 | `MS3-KGF'91` | nothing |
| `robot3.ms3` | 4,953 | `MS3 V2 - KGF` | nothing |

`xeno.exe` (unpacked) has the four names as strings next to each other, pointed to by
a four-word table at `DS:0xec4c`. No binary in the install (`xeno.exe`, the six `.dvr`
files, `vga240.mod`) names `LOSE.MS3` or `ROBOT3.MS3`, and the only song loader
(`1000:fc36`) takes its name from that table with a constant index, so the two are
never played (T-064). Not packed (no `@` in the extension).

## Layout

All values are bytes; there are no multi-byte fields except the delays.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 14 | `signature` | Text, ending in 0x1a (DOS end-of-file, so `type` stops there). Never read by the driver |
| 0x0e | 1 | `unknown_0e` | 0xff in every file. Never read by the driver |
| 0x0f | 1 | `unknown_0f` | 0x00, 0x01 or 0x80. Never read by the driver |
| 0x10 | 16 | `doubled_on` | Per channel: 0, or 1 + another channel that also plays every note of this one (the driver follows the chain). All zero in the stock files |
| 0x20 | 16 | `programs` | Per channel: the program it starts with |
| 0x30 | ... | events | Event, delay, event, delay, ... until `FF 2F` |

The stock files end with `FF 2F 00`, MIDI's end-of-track meta event; the driver stops
at `FF 2F` and never reads the last byte.

## Events

Each event is a status byte, whose high nibble is the kind and low nibble the channel,
then 0 to 2 data bytes, then a delay. There is no running status. The reader treats
the kinds as follows (all others included, so any byte sequence parses):

| Status | Data bytes | Effect |
|---|---|---|
| `9n` | note, velocity | Note on. MIDI note numbers (stock range 24..97) |
| `8n` | none | Note off for the channel's current note (MIDI's note and velocity bytes are dropped) |
| `Cn` | program | Program change |
| `En` | 1 (not MIDI's 2) | Pitch offset for the channel, added to its pitch every tick in half-semitones |
| `Bn` | 2 | Skipped |
| `FF 2F` | | End of the song |
| anything else (`00`..`7F`, `An`, `Dn`, `Fn`, `FF` not followed by `2F`) | 1 | Skipped |

The stock files use only `9n`, `8n`, `Cn` and the end marker, on channels 0 to 7
(`win1.ms3` also uses 8), and stop every note they start. Nine channels match the
OPL2's nine melodic voices; the driver keeps state for ten (0..9).

**Delay**: the ticks to wait after the event before reading the next one (0 = same
tick). One byte if bit 7 is clear; otherwise two bytes, `(first & 0x7f) << 7 | second`.
This is MIDI's variable-length quantity limited to two bytes, except that the driver
does not mask bit 7 of the second byte. Playback starts with the first event on the
first tick; there is no delay before it.

## How the driver plays it

Addresses are offsets in `sounddvr.dvr`'s load image (file offset minus its 0x200-byte
MZ header), as Ghidra shows them in segment `1000`. Data tables are in its data segment (`sounddvr.md`,
"Addresses").

- **Load** (`snd_queue_or_start_song`, `0x0b2e`, far entry `0x01e4`, slot `0x42c`,
  file in `ES:BX`, flags in `AX`): if bit 0 is set and a song is playing, it only
  remembers the song and its flags as the queued one. Otherwise (`snd_start_song_now`,
  `0x0b4a`) it keeps the flags as the current song's, copies `doubled_on` and
  `programs` into per-channel tables, starts reading at offset 0x30 and sets the delay
  to 1, so the first event runs on the next tick. The signature and bytes 0x0e/0x0f
  are not looked at, by the driver or by `xeno.exe`.
- **Reader** (`0x0b97`): decodes one event and its delay as above.
- **Tick** (`snd_music_tick`, `0x0a62`, through a hook node at `0x01d0` whose handler is at `0x01d6`;
  music init, slot `0x424`, adds it to the host's per-retrace chain with host slot
  `0x358`, and `0x35c` removes it; `docs/re/dvr-protocol.md`): counts down the delay and runs every event that
  is due (only while a song is playing; the pitch pass and the back end's tick run
  every retrace regardless). A note on a channel that is playing a driver sound effect (`0x09a0`) is
  dropped. Before a note, the channel's program is loaded if it changed.
- **End** (`FF 2F`, in `snd_run_due_events`): the delay is set to 1, every note is
  stopped and the song marked not playing. Then, if a song is queued, it starts (as
  above, its flags becoming the current ones); else, if the ending song's flags have
  bit 1, it starts again from offset 0x30; else the music stays silent. A repeat or a
  queued song's first event therefore runs one tick after the tick that read `FF 2F`,
  so a looping song's period is its total delay plus 1 tick.
- **Pitch**: internally twice the note number, plus a per-program transpose, plus the
  `En` offset, plus that tick's byte of a per-program pitch envelope (a byte script
  advanced every tick: each byte is the offset for that tick only; 0x80 ends it, 0x81
  jumps back to the start; `sounddvr.md`).
- **Velocity** is scaled by a per-program factor: `velocity * 2 * factor / 256`.
- **Programs** index tables of 130 entries in the driver's data segment (envelope
  pointers, Tandy volume-script pointers, transpose, velocity factor) and 128 OPL2
  patches and MIDI program numbers (`sounddvr.md`). Stock files use programs up to 117.
- **Back ends**: device 1 to 5 picks a set of seven routines (init, note on, pitch,
  program, ...). The MPU-401 back end sends real MIDI note on/off, program change
  (through the map) and pitch bend; the AdLib one writes OPL2 registers.

## When songs play

`xeno.exe` has one song routine, `snd_start_song` (`1000:fc36`, song `BX`): it loads the
file named by the word at `DS:0xec4c + BX` (slot `0x2a4`), calls the music init (slot
`0x424`, the device letter at `DS:0x225`), which adds the driver's hook to the
per-retrace chain, then starts the song (slot `0x42c`) and resets the retrace counter
`DS:0x370` that the cutscenes' end times count from (`vcl.md`). The flags are what the
init leaves in `AX`, which is always 0 (the host's table-hook routine, slot `0x26`, the
init's last call, returns with `AX` = 0): **no queue and no loop**. Every song plays
once, from its first event, and the music is silent after its `FF 2F` until the next
start. With device `N` the init returns at once and slot `0x42c` is not the driver's,
so nothing plays.

`snd_stop_song` (`1000:fc60`) calls the music shutdown (slot `0x428`), which silences
every voice and removes the hook, so the music stops at once, mid-note.

| Song (`BX`) | File | Started | Stopped |
|---|---|---|---|
| 0 | `MAINTHEM.MS3` | `ui_run_startup_and_game` (`1000:fb1c`), just before the intro cutscene | On entering the map room's loop (`1000:6f34`), if it has not ended. It is 11,053 ticks; the intro cutscene's first two minutes or so are at 59.94 Hz and the rest at 70.09 Hz, so unskipped it ends nearly 3 minutes in, during the drop or the log-in screen |
| 2 | `WIN1.MS3` | `ui_play_win_cutscene` (`1000:70b7`), before the mode change | After the `WIN` picture (`1000:70c0`) |
| 4 | `WIN2.MS3` | `ui_play_lose_cutscene` (`1000:703b`), the same way | After the lose picture (`1000:7049`) |
| 6 | `BIGWIN3.MS3` | `ui_play_triumph_sequence` (`1000:fdb9`), after the mode change | After the 90-tick wait that follows `TRIUMPH` (`1000:fdd4`) |

These are the only call sites of slots `0x424`, `0x428` (apart from the driver's own
shutdown) and `0x42c` in any binary. Each start is preceded by a stop or by nothing
playing; the init adds the hook again every time.

**Tick rate.** The hook is on the host's per-retrace chain (slot `0x358` adds it,
`0x35c` removes it; `docs/re/dvr-protocol.md`), so one tick per retrace interrupt:
about 70.09 Hz in mode 13h and 59.94 Hz while the 320x240 mode is up (the intro
cutscene, the endings). The chain is never paused: the interrupt keeps running during
file loads and mode changes (it gives up waiting for the retrace bit after a timeout
and runs the chain anyway, at the old period until slot `0x348` re-measures it), and
the driver's own slot `0x348` hook only measures the frame and chains to the host's.
Only a retrace whose interrupt arrives while the previous one's chain is still running
is dropped. So a port can tick the music once per presented frame of whichever mode is
up, and keep ticking across a mode change.

## Open questions

- Why `robot3.ms3` has a `V2` signature and what `unknown_0f` meant to the authoring
  tool. Nothing reads either.
- A DOSBox recording to check the timing by ear (T-062 can do it when it plays music).

## Parser

`xeno_formats::ms3` (T-022): `parse` returns a `Song` with the `binrw` `Header`, the
events as `Step`s (an `Event` and its delay) and the bytes after the end marker;
`Song::ticks` gives the length. `xeno-tool inspect` prints the signature, event counts,
length in ticks and each used channel's starting program.
