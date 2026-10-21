# Packed files (`@` in the extension)

## Status

**Confirmed** (T-014). The decoder was traced in `xeno.exe`, and
`xeno_formats::packed::decompress` reads all 22 packed files of a stock install, each
to its last byte and to the size its header gives.

## Files

The game's code names its data files with ordinary extensions, `.BIN` and `.MRL`
(`mgavideo.dvr` names `MSPRITES.BIN`, `BACKLAND.BIN` and the `.MRL` screens). The
copies on disk are compressed, and their extension has `@` in place of its middle
letter:

| On disk | Count | Decoded content |
|---|---|---|
| `.b@n` | 14 | Sprite banks and sky strips (`b@n.md`) |
| `.m@l` | 8 | Full-screen pictures: 768-byte 6-bit palette, then a PackBits 320x200 screen (`m@l.md`) |

No unpacked `.bin` or `.mrl` copy of these is in the install. The other `.bin` files
(`scendef.bin`, `dsounds.bin`) have no `@` and are not in this format.

## Layout

| Offset | Size | Meaning |
|---|---|---|
| 0x00 | u16 LE | Decoded size in 16-byte paragraphs, rounded up (what the loader allocates) |
| 0x02 | to the end | Packet stream; the last byte is the end marker `0x80` |

Each packet starts with a control byte `c`:

| `c` | Bytes that follow | Output |
|---|---|---|
| `0x00..=0x7f` | `c + 1` | Those bytes, as literals |
| `0x80` | none | End of stream |
| `0x81..=0xff` | 1 (`b`) | Copy `((c & 0x7f) >> 2) + 1` bytes (1 to 32) starting `d` bytes back in the output |

The distance is `d = (b << 2 | (c & 1) << 1 | (c >> 1) & 1) + 1`, so 1 to 1024: the
high eight bits come from `b`, and the two low bits from `c` in swapped order. The copy
goes forward one byte at a time, so a copy longer than its distance repeats a pattern.
That happens in every file.

The stock files never use copies shorter than 4 bytes. No copy reaches back before the
start of the output.

## In the executable

Offsets are in the `unlzexe` output of `xeno.exe` (SHA-256 in `re-setup.md`).

- The file decoder starts at Ghidra `1fe6:2efb` (file offset ~0x1395b). It decodes
  while it reads the open file in 4 KB blocks, refilling once the read pointer passes
  0xf7f. The packet loop at ~`1fe6:2f13` does exactly what the table above describes.
- A second loop of the same shape sits at ~`1fe6:2e4c`. It works on memory, counts
  packets in a register and normalises segments for buffers over 64 KB. It adds 1 to
  the length bits *before* splitting them, so if it is a decoder, it is for a variant
  with a different bit layout. No file in the install needs that variant.

## Open questions

- What the second loop at ~`1fe6:2e4c` is for, and whether any data uses it.
- Where the file names get their `@`. No code was found that writes `@` into a name,
  and no binary names the packed files themselves (T-025, T-090).
- How the code maps `MSPRITES.BIN` and `BACKLAND.BIN` to `msprite0`..`5` and
  `sky_a`..`h` (T-028).

## Parser

`xeno_formats::packed::decompress`, added by T-014.
`xeno_formats::fixtures::synthetic_packed` is a test-only compressor that writes the
same stream format.
