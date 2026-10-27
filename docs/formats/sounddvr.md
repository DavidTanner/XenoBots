# `sounddvr.dvr`'s instrument bank

## Status

**Confirmed** (T-063) from the music driver's code in `sounddvr.dvr`: the program
loader, the per-tick pitch pass and the AdLib back end's seven routines.
`xeno_formats::sounddvr` reads every table below from the user's copy at run time; the
bank is game data and is never stored in the repository. This is the groundwork for
T-062 (OPL2 music); the `.ms3` files themselves are in [`ms3.md`](ms3.md).

## Addresses

Code addresses are load-image offsets (file offset minus the 0x200-byte MZ header),
Ghidra segment `1000`. The tables are in the driver's **data segment**: every music
routine starts with `mov ds, cs:[0x86b]`, and the word at load-image 0x86b (relocated
at load) is `DS`'s paragraph, 0x2a8 in the stock driver, so `DS:x` is load-image offset
`0x2a80 + x` (Ghidra `12a8:x`). The table offsets in T-063's notes and in `ms3.md`
(`0x15b4`, `0x0fc9`, ...) are `DS` offsets.

The parser finds each table through the instruction that indexes it (it checks the
instruction's bytes first, so another build is refused rather than misread):

| Table | `DS` offset (stock) | Found at (code) | Entries |
|---|---|---|---|
| OPL2 patches | 0x01dd | `add bx, imm` at 0x0fed (`opl_set_program`) | 128 x 16 bytes |
| OPL2 frequencies | 0x09dd | `add bx, imm` at 0x10f7 (`opl_set_pitch`) | 256 words |
| Modulator operator offsets | 0x0bef | `mov dl, [si+imm]` at 0x1000 | 9 bytes |
| Carrier operator offsets | 0x0bf8 | `mov dh, [si+imm]` at 0x112c | 9 bytes |
| Channel to OPL2 voice | 0x0c01 | `mov ch, [bx+imm]` at 0x111d | 10 bytes |
| MPU-401 program map | 0x0c53 | `mov cl, [bx+imm]` at 0x0e91 | 128 bytes |
| Pitch envelope pointers | 0x15b4 | `mov dx, [bx+imm]` at 0x0d49 | 130 words |
| Tandy attack pointers | 0x0fc9 | `mov ax, [bx+imm]` at 0x0d59 | 130 words |
| Tandy release pointers | 0x10d4 | `mov ax, [bx+imm]` at 0x0d61 | 130 words |
| Velocity factor | 0x152e | `mov al, ds:[bp+imm]` at 0x0d69 | 130 bytes |
| Transpose | 0x14ac | `mov al, ds:[bp+imm]` at 0x0d72 | 130 bytes |

The counts are the gaps between the tables in the stock driver (the code never checks
a program number): programs 0 to 129 have per-program entries, but only 0 to 127 have
an OPL2 patch or a MIDI program (128 and 129 would read the next table). The stock
`.ms3` files use programs up to 117.

Other `DS` data near the bank, not parsed: runtime shadows of each OPL2 voice's
carrier level byte (0x0bdd) and `B0` register (0x0be6); the MPU-401 channel map
(0x0c11) and its timeout (0x0c31); the PC speaker's 256 PIT divisors by pitch (0x0cd3);
the Tandy back end's default volume script (0x0ed3) and 256 tone dividers by pitch
(0x11e2); the FM sound-effect table (0x13e4, see T-066); the default pitch envelope
(0x15b0).

## The per-program tables

When a channel's program changes (see "When a note starts"), `snd_load_channel_program`
(`0x0d35`) copies the program's entries into the channel's state and then calls the
back end's program routine:

- **Pitch envelope pointer** (word, a `DS` offset): the start of a byte script, saved
  as the channel's envelope start and current position.
- **Tandy attack and release pointers** (words, `DS` offsets): volume scripts the Tandy
  back end walks (see "Other back ends"). Not used by the AdLib.
- **Velocity factor** (byte): a note's velocity becomes
  `((2 * velocity) & 0xff) * factor >> 8`, so 128 leaves a velocity under 128 as it is.
  Every stock program has 128.
- **Transpose** (signed byte, half-semitones): added to twice the note number. Every
  stock program has 0.
- **MPU-401 program map** (byte): the MIDI program sent is the entry minus 1.

### Pitch envelope scripts

One byte per tick, read in the per-tick pitch pass (`0x0a62`):

| Byte | Effect |
|---|---|
| `0x80` | End: no offset this tick or any later one; the position stays here |
| `0x81` | Loop: go back to the script's start and read again (same tick) |
| anything else | A signed offset in half-semitones for **this tick only**, then advance |

The offset is not accumulated: each tick's pitch is the note's base pitch plus the `En`
offset plus that tick's byte. A note start resets the position to the start; the
script keeps running after the note stops. In the stock driver every program points at
the same script, a single 0 then the end, so no stock program bends its pitch.

## How a note is played (all back ends)

A channel (0 to 9) keeps: its program and the program last loaded, its base pitch,
its `En` offset, its envelope start and position, the pitch last sent, its scaled
velocity, an "active" flag, and a "busy with a sound effect" countdown. The driver's
per-retrace tick (`ms3.md`, "How the driver plays it") does, in order:

1. **Events** that are due (`snd_run_due_events`, `0x0c73`). For a note on (`9n`) or
   off (`8n`), on the channel and every channel its `doubled_on` chain names:
   - skip the channel if it is playing a driver sound effect;
   - if its program differs from the one loaded, load it (tables above, then the back
     end's **program** routine);
   - note on: base pitch = `2 * note + transpose` (8-bit, wrapping), velocity scaled,
     envelope position reset, active set, the "pitch last sent" forgotten (so step 2
     always sends it), then the back end's **note** routine with the base pitch and
     the scaled velocity;
   - note off (or note number 0): active cleared, then the **note** routine with
     pitch 0. (A base pitch that wraps to 0 reaches the AdLib as a note off.)

   `Cn` only records the channel's program (loaded at its next note); `En` sets the
   offset.
2. **Pitch pass**, channels 9 down to 0 (every channel, playing or not): pitch = base +
   `En` offset + envelope step (8-bit, wrapping); if it differs from the pitch last
   sent, the back end's **pitch** routine.
3. The back end's **tick** routine.

## The AdLib back end

Device letters `A` and `C` select device 3 (`I` is device 1, the PC speaker; `T` is
2, the Tandy/PCjr sound chip; `R` is 4, the MPU-401; anything else turns music off).
Init (`0x08ac`) copies the device's entry of seven routine tables (`DS:0x17b`,
`0x189`, `0x197`, `0x1a5`, `0x1b3`, `0x1c1`, `0x1cf`; seven words each: the slot
called through, then devices 0 to 5) into each table's slot.

All writes go through `opl_write_register` (`0x0f5b`): the register number to port
0x388, a short delay, the value to 0x389, a longer delay (both delays are busy loops
calibrated against the PIT; an emulator can drop them). Below, `v` is the channel's
OPL2 voice from the channel table (a channel whose entry is 9 or more is ignored by
every routine), `car` and `mod` are the voice's operator offsets, and the patch bytes
are numbered 0 to 15.

| Slot | Routine | What it writes |
|---|---|---|
| `0x1cf` | `opl_detect_adlib` (`0x0f82`) | The usual timer test: 0x04 = 0x60, 0x04 = 0x80, read status; 0x02 = 0xff, 0x04 = 0x21, wait, read status; 0x04 = 0x60, 0x04 = 0x80. Present if the statuses' top three bits are 0 and then 0xc0 |
| `0x17b` | `opl_init` (`0x1089`) | 0x01 = 0x20 (waveform select on), 0x08 = 0x00, 0xbd = 0xc0 (deep tremolo and vibrato, no rhythm mode); then `opl_silence_all`; then the `B0` shadows of voices 0 to 8 = 0x1f, and program 0 loaded through `opl_set_program` on channels 0 to 8 |
| `0x197` | `opl_silence_all` (`0x10bc`) | For voices 0 to 8: `0x40 + car` = 0x3f, `0xb0 + v` = 0 (the shadows are left as they were). Called when the driver re-initialises a device |
| `0x1c1` | `opl_set_program` (`0x0fd3`) | `0xb0 + v` = shadow & 0xdf (key off; the shadow keeps its key bit); `0x20 + car` = byte 0; `0x60 + car` = byte 2; `0x80 + car` = byte 3; `0xe0 + car` = byte 4; `0x20 + mod` = byte 5; `0x40 + mod` = byte 6 XOR 0x3f; `0x60 + mod` = byte 7; `0x80 + mod` = byte 8; `0xe0 + mod` = byte 9; `0xc0 + v` = byte 10. Byte 1 is kept as the voice's carrier level byte. Bytes 11 to 15 are never read (they are not zero in the stock bank) |
| `0x1a5` | `opl_note_on_off` (`0x111d`) | Note on (pitch not 0): level = `min(velocity / 4 + 0x20, 0x3f)` (8-bit `velocity`); `0x40 + car` = (carrier level byte OR level) XOR 0x3f; shadow \|= 0x20; `0xb0 + v` = shadow. Note off: shadow &= 0xdf; `0xb0 + v` = shadow |
| `0x1b3` | `opl_set_pitch` (`0x10e0`) | f = frequency word for the pitch byte; shadow = (shadow & 0xe0) \| (f >> 8); `0xa0 + v` = f & 0xff; `0xb0 + v` = shadow (so a sounding note stays keyed on) |
| `0x189` | (`0x10df`) | Nothing: the AdLib has no per-tick work |

So the carrier's total level comes from the velocity alone: with the key-scale bits in
the patch's byte 1 (its low six bits are 0 in every stock patch), attenuation is
`0x1f - velocity / 4` steps of 0.75 dB for velocities under 128: 0 at velocity 124 and
up, 23.25 dB at velocity 0. The modulator's level is fixed by the patch.

### The frequency table

One word per pitch byte (half-semitones, so 24 entries an octave): the OPL2 block in
bits 10 to 12 and the F-number in bits 0 to 9, ready for registers `A0`/`B0`. Pitch
`2 * note` is MIDI note `note`.

### Voices and operators

The operator offsets are the standard OPL2 layout (voice `v`'s modulator and carrier
are the usual operator 1 and 2 slots). The channel table maps channels 0 to 7 to
voices 0 to 7 and channel 9 to voice 8; **channel 8 has no voice**, so on an AdLib the
notes `win1.ms3` puts on channel 8 are not heard. Voice 8 gets no patch at init
(init loads program 0 through channels 0 to 8, and channel 8 maps to nothing), only
when channel 9 first plays.

### The order of writes when a note starts

Within one tick: a program change if needed (key off on `B0`, then the eleven patch
registers), then key on (carrier level, then `B0` with the key bit and the voice's
**previous** frequency), then, in the pitch pass of the same tick, `A0` and `B0` with
the new frequency. On real hardware the two writes are microseconds apart; an
emulator that applies a tick's writes at one sample time can write them in that order
or put the frequency first, with the same result.

## Other back ends

- **MPU-401** (device 4, port 0x330/0x331): MIDI channel from the channel map (0 =
  not sent), note on as `9n note/2 velocity` (velocity capped at 127), note off as
  `8n note 0x40`, pitch changes as pitch bend relative to the base pitch, program
  change through the program map.
- **Tandy / PCjr** (device 2, port 0xc0, SN76489: channels 0 to 2 on its tone voices, channel 9 on its noise voice): tone dividers by
  pitch; each tick walks a volume script per voice from the program's attack pointer
  (note on) or release pointer (note off while the current byte is not 0). A script
  byte with bit 7 set holds, 0 is silence, otherwise the byte times the doubled
  velocity sets the attenuation.
- **PC speaker** (device 1): channel 0 only, PIT channel 2 divisors by pitch.

## Open questions

- Bytes 11 to 15 of each patch: never read by this driver; perhaps fields of the
  authoring tool's patch format.
- The FM sound-effect path (the music export's slot `0x44c` target, `0x09a0`, table at
  `DS:0x13e4`), used when no digitised device is set up: see T-066.

## Parser

`xeno_formats::sounddvr` (T-063): `parse` returns a `Bank` with the 128 `binrw`
`Patch`es (named registers, `unknown_0b`), the 256 frequency words, each program's
`Envelope` (signed steps and whether it loops), the Tandy pointers, velocity factors,
transposes, MIDI program map, channel-to-voice map and the operator offsets.
`xeno-tool inspect sounddvr.dvr` prints the counts.

## Symbols

Named in Ghidra (`sounddvr.dvr`, T-063): `snd_read_ms3_event` (`0x0b97`),
`snd_run_due_events` (`0x0c73`), `snd_start_or_stop_note` (`0x0ae1`),
`snd_load_channel_program` (`0x0d35`), `snd_stop_all_notes` (`0x0ce3`). The AdLib
routine names above (`opl_*`) and the tick `0x0a62` are not functions in the project
yet: the project's save stopped answering after the first renames (see T-063's note).
