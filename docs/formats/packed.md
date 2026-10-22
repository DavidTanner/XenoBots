# Packed files (`@` in the extension)

## Status

**Confirmed** (T-014, T-028). The decoder was traced in `xeno.exe`, and
`xeno_formats::packed::decompress` reads all 22 packed files of a stock install, each
to its last byte and to the size its header gives. T-028 found where the `@` comes
from and settled the second packet loop.

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

### Where the `@` comes from

The host's whole-file loader (slot `0x2b0`, `file_load_block_any_form` at Ghidra
`1fe6:2827`) and its open-for-reading service (slot `0x2b4`, `1fe6:285f`) take a name
in the padded 8.3 form (`SKY_B   .BIN`) and try the extension's middle letter (byte
10 of the name) in turn as:

| Letter | Form | Loader |
|---|---|---|
| as given (`I`, `R`, ...) | Plain file, read as is | `1fe6:2631` |
| `!` | LZW: 9- to 12-bit codes, code 0x100 resets the table and 0x101 ends; not decoded further | `1fe6:29aa` |
| `@` | This packet format | `1fe6:2dd9` |

The first that exists (DOS "get attributes") is loaded, and the name is restored. So a
plain `SKY_B.BIN` placed next to `SKY_B.B@N` would win. If none exists the loader
returns with carry clear and allocates nothing, so callers find the block absent
rather than an error (`b@n.md`, `BACKLAND.BIN`). The streaming services (read, close)
dispatch the same way; write and seek fail for the two compressed forms. No stock file
uses `!`.

Both `.MRL` screens (through `mgavideo.dvr`'s slot `0x490`) and the `.BIN` files load
through slot `0x2b0`. Its first instruction is a breakpoint (`int 3`), which is
harmless without a debugger; the same is true of other routines that start with one.

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
- A second loop of the same shape sits at ~`1fe6:2e4c`, in a routine starting at
  `1fe6:2e1a` (named `mem_unpack_packed_variant_unused`). It decodes memory to memory,
  counts packets and normalises both pointers for buffers over 64 KB. Its copy packets
  differ: it adds 1 to `c & 0x7f` before splitting, so the length is
  `((c & 0x7f) + 1) >> 2` (0 to 32) and the distance's low two bits come from the
  incremented value, and it never clears the high byte of the distance register, which
  still holds an earlier pointer. Nothing calls it: no loader table, slot table or call
  in the code reaches it, nor `1fe6:2db7`, which would decode a packed file into a
  caller's buffer. It is dead code for an older or unfinished variant; every packed file
  uses the file decoder above, which `file_load_packed_block` (`1fe6:2dd9`) calls.

## Open questions

None about the format. `MSPRITES.BIN` and `BACKLAND.BIN` are mapped to the files in
`b@n.md`.

## Parser

`xeno_formats::packed::decompress`, added by T-014.
`xeno_formats::fixtures::synthetic_packed` is a test-only compressor that writes the
same stream format.
