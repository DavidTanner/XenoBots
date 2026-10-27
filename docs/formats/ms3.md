# `.ms3` music

## Status

**Confirmed** (T-022) for the layout and the event stream, from the reader in
`sounddvr.dvr` that loads and plays them. `xeno_formats::ms3` reads all six files to
their end marker. The instrument bank the programs refer to is decoded in
[`sounddvr.md`](sounddvr.md) (T-063); the tick rate is still to confirm (see open questions).

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

`xeno.exe` (unpacked) has the four names as strings next to each other. No binary in
the install (`xeno.exe`, the six `.dvr` files, `vga240.mod`) names `LOSE.MS3` or
`ROBOT3.MS3`, so they may be unused, or their names may be built at run time. Not
packed (no `@` in the extension).

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

- **Load** (`0x0b2e`, far entry `0x01e4`, file in `ES:BX`): copies `doubled_on` and
  `programs` into per-channel tables and starts reading at offset 0x30. Bit 0 of `AX`
  queues the song to start when the current one ends; bit 1 makes it restart at its
  end. The signature and bytes 0x0e/0x0f are not looked at.
- **Reader** (`0x0b97`): decodes one event and its delay as above.
- **Tick** (`0x0a62`, through a hook node at `0x01d0` whose handler is at `0x01d6`;
  music init, slot `0x424`, adds it to the host's per-retrace chain with host slot
  `0x358`, and `0x35c` removes it; `docs/re/dvr-protocol.md`): counts down the delay and runs every event that
  is due. A note on a channel that is playing a driver sound effect (`0x09a0`) is
  dropped. Before a note, the channel's program is loaded if it changed.
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

## Open questions

- **Tick rate.** Settled by T-025: the music ticks once per vertical retrace (the
  host's per-retrace hook chain, not its 60 Hz tick), so about 70.09 Hz in mode 13h
  and 59.94 Hz while the 320x240 mode is up. At 70 Hz `mainthem.ms3` lasts about 2
  minutes 38 seconds. Still to confirm against a DOSBox recording (T-064).
- What `unknown_0f` means (the authoring tool's flags?), and why `robot3.ms3` has a
  `V2` signature. The driver ignores both.
- The queue/loop flags each song is started with. From T-025: `xeno.exe` `1000:fc36`
  loads and starts song `BX` from a name table at `DS:0xec4c` (0 `MAINTHEM` at
  start-up, 2 `WIN1` before the win cutscene, 4 `WIN2` before a lose cutscene, 6
  `BIGWIN3` for `TRIUMPH`). Nothing names `lose.ms3` or `robot3.ms3`.

## Parser

`xeno_formats::ms3` (T-022): `parse` returns a `Song` with the `binrw` `Header`, the
events as `Step`s (an `Event` and its delay) and the bytes after the end marker;
`Song::ticks` gives the length. `xeno-tool inspect` prints the signature, event counts,
length in ticks and each used channel's starting program.
