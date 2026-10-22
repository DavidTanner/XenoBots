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
  right by `DL` and re-centred). `AL = 0xff` stops the voice.
- **Mixer** (four routines, one per output device, chosen at init; one drives the PC
  speaker, another a Sound Blaster at DSP port 0x22c): sums the three voices' bytes.
  The PC-speaker version adds a term from a small history buffer, divides by four and
  drives the speaker with the top bit.

`xeno.exe` plays effects through a table of 29 sound events at `DS:0x7e18` (4 bytes
each: slot, voice/flags, step override). Every entry has step override 0 and volume
0 is passed, so the stock game plays each sample at its file rate and full volume.
Blocks 4 and 15 are not referenced by that table.

## Open questions

- Which game action triggers each of the 29 events (and so what each sound is).
- The other mixer back ends (device ids and ports) and the exact volume table at
  `0x1b75`; the PC-speaker path is the only one read in full.
- Whether anything plays blocks 4 and 15.

## Parser

`xeno_formats::dsounds` (T-023): `parse` returns a `Bank` of `Sound`s, each with the
`binrw` `Header` (`end`, `step`), its samples and its `unknown_tail`, plus any
bytes after the terminator. `xeno-tool inspect dsounds.bin` lists each sound's
length, rate and duration; `xeno-tool dump dsounds.bin --out <dir>` writes
`dsounds_NN.wav` (8-bit unsigned mono at the sound's rate).
