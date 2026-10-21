# `scendef.bin` scenario table

## Status

**Confirmed** (T-019). `xeno_formats::scendef` reads the stock file to its last byte.
The field meanings come from the code in `xeno.exe` that loads, indexes, draws and
hit-tests the table. The box positions also match the map room picture
(`maproom.m@l`): each box sits on the place its name gives.

## Files

One file, 410 bytes, not packed. It holds 13 records, one per campaign scenario
(`scen00a`..`scen12a`). The four training missions (`htrain`, `ntrain`, `strain`,
`ctrain`) are not in it: `xeno.exe` names their `.USG` files directly.

## Layout

Records follow each other with no header. A record whose first word is `0xffff` ends
the table, and the stock file ends right after it. All words are u16 little-endian.

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0x00 | 2 | `id` | Twice the record's position (0, 2, 4, ...). The game uses it as a byte offset into its table of record pointers (`DS:0x5fb5`), so it must match the position |
| 0x02 | 2 | `map_box.top` | Clickable box on the 320x200 map room screen, in pixels |
| 0x04 | 2 | `map_box.left` | |
| 0x06 | 2 | `map_box.bottom` | Inclusive |
| 0x08 | 2 | `map_box.right` | Inclusive |
| 0x0a | 2 | `unlock_level` | The scenario can be picked once the player's progress value (`DS:0x5f9b`) is at least this. 1, 2, 3, 4 or 6 in the stock file, rising with the record number |
| 0x0c | 2 | `state` | Runtime progress for this scenario; 0 in the file (see below) |
| 0x0e | to NUL | `name` | Place name, "TOWN, COUNTRY" style, NUL-terminated |

Every box in the stock file has `bottom = top + 9` and `right = left + 9`, so it covers
10x10 pixels.

### Names

Names use the game's text encoding: ASCII, with `0x0c` ending each line. Other strings
in `xeno.exe` use `0x0c` the same way, between two lines (for example a label above a
field). Every stock name is one line followed by `0x0c`, then the NUL.
`Scenario::name_lines` splits a name into its lines.

### `state`

The game keeps per-scenario progress in the loaded table itself. It writes 0, 8, 0xa
and 0xc to this word. The map room picks how to draw each box from a 7-entry table
indexed by `state` (`DS:0x5f5c`, so values are even), and refuses to select a
scenario whose `state` is 0xa. Which value means won, lost or in progress has not
been traced.

The save routine (`xeno.exe` `1000:7666`) writes 36 bytes of game state from
`DS:0x5f91` and then the whole loaded table, and the load routine (`1000:76ab`) reads
it back. So a save file carries every scenario's `state`. The save file is the
player's `<NAME>.LOG`, and this data is its `SDEF` chunk (T-024, `usg.md`, "Save
files").

## How the game uses it

All addresses are in the unpacked `xeno.exe` (`docs/re-setup.md`), Ghidra's segments.

- **Load** (`1000:712e`): reads `SCENDEF.BIN` (name at `DS:0x5ee0`) through the
  host's file loader into a buffer whose segment is stored at `DS:0x5edc`. The same
  buffer is used for other files, so the table is reloaded or restored when needed.
- **Index** (`1fe6:732d`): walks the records from offset 0. For each one it stores
  the record's offset in the pointer table at `DS:0x5fb5`, skips the 14 fixed bytes
  and scans past the NUL. It stops at a `0xffff` word or after 32 records, and stores
  the count at `DS:0x5ffd`. `xeno_formats::scendef` rejects more than 32 records,
  since the game would never see the rest.
- **Draw** (around `1000:7282`): for each record, draws its box with a look chosen by
  `state`, or a fixed "unavailable" look when `unlock_level` is above the player's
  progress. The record whose `id` equals the current selection (`DS:0x5f97`) gets a
  highlight.
- **Hit test** (`1fe6:72f8`): a click at (x, y) selects the first record with
  `left <= x <= right` and `top <= y <= bottom`. The click is refused when
  `unlock_level` is above the player's progress or `state` is 0xa.

## Scenario files

Record `n` loads `scenNNa.usg`. The game writes `id / 2` as two digits into the
template `SCEN00A.USG`, and the selection's high byte picks the letter (T-020, see
`usg.md`).

## Open questions

- What each `state` value means, and what sets the player's progress value
  `DS:0x5f9b` (T-042).
- What the draw values chosen by `state` look like (T-042).

## Parser

`xeno_formats::scendef` (T-019): `parse` returns a `ScenarioTable` of `Scenario`
records (`binrw`). `xeno-tool inspect scendef.bin` lists every record.
