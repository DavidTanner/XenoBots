# `.m@l` screens

## Status

**Confirmed** (T-017). `xeno_formats::mal` reads all 8 files of a stock install. Each
one decodes to its last byte and gives a 320x200 picture that looks right in its own
palette. The driver routine that shows them has been read (see `palette.md`). The code
in `xeno.exe` that asks for each screen has not been traced.

## Files

Each `.m@l` is a packed `.MRL` file (see `packed.md`). Decompress it first; every
offset below is into the decoded data.

| File | Shown as (guess from the art) |
|---|---|
| `login` | Operator log-in |
| `sitroom` | Situation room: roster, unit status, main view |
| `maproom` | Map room |
| `review` | Mission review |
| `trainmen` | Training menu |
| `humancon`, `scorpcon`, `scoutcon` | Order screens for the humanoid, scorpion and scout robots |

Decoded sizes run from 23 KB to 42 KB.

`mgavideo.dvr` keeps a file-id table in its data segment (driver data offset 0, file
offset ~0x4500): one u16 per id, each pointing at a 13-byte name (8.3, space padded,
NUL terminated). A file id is a byte offset into that table, so ids are even. The
`.MRL` names come first, ids 0x00 to 0x14, in this order: `TITLE`, `OPTIONS`,
`SITROOM`, `TRAINMEN`, `MAPROOM`, `WALL2`, `SCORPCON`, `HUMANCON`, `SCOUTCON`,
`REVIEW`, `LOGIN`. `TITLE`, `OPTIONS` and `WALL2` are not in a stock install. The
`.VCL`, `.BIN` and `.MXX` names follow (`mxx.md` uses the same ids).

## Layout

| Offset | Size | Meaning |
|---|---|---|
| 0x000 | 768 | Palette: 256 `r, g, b` VGA DAC triples, 0..=63 each |
| 0x300 | to the end | The picture: PackBits that expands to exactly 64,000 bytes, 320 per row from the top |

The PackBits stream ends exactly at the end of the data in every stock file.

## Encoding

PackBits, as the host's decoder at slot `0x292` reads it (`palette.md`). Each packet
starts with a control byte `n`:

| `n` | Bytes that follow | Output |
|---|---|---|
| `0x00..=0x7f` | `n + 1` | Those bytes |
| `0x80..=0xff` | 1 | That byte, repeated `257 - n` times (2 to 129) |

Classic PackBits skips `0x80`. The game's decoder doesn't, but no stock file uses
`0x80`, so the difference never shows. The decoder stops once it has written the
length it was asked for (64,000 here).

The encoder worked row by row: no run in any stock file crosses from one row into the
next. The decoder does not rely on this, and `xeno_formats::mal` accepts runs that
cross rows.

## How the game shows a screen

`mgavideo.dvr`'s routine at ~0x535 copies the palette into its shadow palette, unpacks
the picture straight into video memory at `A000:0000` and marks the palette dirty, so
the new colours arrive at the next vertical retrace (`palette.md`). So the screens are
mode 13h, 320x200, shown at 4:3 with non-square pixels (`video-modes.md`).

## Open questions

- When each screen is shown (T-040). T-025 found the callers: `xeno.exe` loads ids
  0x04, 0x06, 0x08, 0x0c, 0x0e, 0x10, 0x12 and 0x14 through slot `0x490`
  (`docs/re/dvr-protocol.md`, "Screens requested"). `TITLE`, `OPTIONS` and `WALL2` are
  never requested, so the port doesn't need them.
- In `sitroom`, `scorpcon` and `scoutcon` index 255 is pure green and fills the large
  panels (in `sitroom`, the main view). It is presumably a placeholder that the game
  draws over, or a key colour. Find out from the code that draws into those panels
  (T-042, T-043).
- Which of the screens' colours the sprite banks and unit models are drawn with
  (T-028, T-029).

## Parser

`xeno_formats::mal::parse` (packed file) and `parse_unpacked`, added by T-017.
`mal::runs` and `mal::expand` are the PackBits decoder, split so the runs can be
checked. `xeno-tool inspect` counts the runs, `xeno-tool dump` writes one PNG per
screen in its own palette, and `xeno-tool palette` prints the screen's palette. A
screen can also be passed to `dump --palette` for files that have no palette.
`xeno_formats::fixtures::synthetic_screen` builds test screens.
