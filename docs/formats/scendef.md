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
| 0x0a | 2 | `unlock_level` | The scenario can be picked once the player's progress value (`DS:0x5f9b`) is at least this. 1, 2, 3, 4 or 6 in the stock file, rising with the record number. A new campaign starts at progress 1 |
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

The game keeps per-scenario progress in the loaded table itself (T-042,
`docs/screens.md`, "The map room"):

| `state` | Meaning |
|---|---|
| 0, 2, 4, 6 | Open, at alert level 1 to 4. After each won scenario, open records may rise by 2 (at most to 6), more likely the higher their `unlock_level` above the lowest open one |
| 8 | Locked: `unlock_level` is above the progress. Set when the campaign is checked, and back to 0 once the progress reaches it |
| 0xa | Won. Never selectable again |
| 0xc | In play: a mission of it was started and not won (lost, or left with no result) |

The map room picks each box's sprite from a 7-entry table indexed by `state`
(`DS:0x5f5c`), and its hover and alert words from two more. A scenario can be selected
when `unlock_level` is at most the progress and `state` is not 0xa; while the last
campaign mission has no result, only the one at 0xc.

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
- **Draw** (`ui_draw_map_area`, `1000:7255`): for each record, draws its box with a
  look chosen by `state`, or a fixed "unavailable" look when `unlock_level` is above
  the player's progress, colour 0 transparent at `map_box`'s top-left corner. The
  record whose `id` equals the current selection (`DS:0x5f97`) gets a highlight.
- **Hit test** (`scen_hit_test_map_box`, `1fe6:72f8`): a click at (x, y) selects the
  first record with `left <= x <= right` and `top <= y <= bottom`. The click is
  refused when `unlock_level` is above the player's progress or `state` is 0xa.
- **Campaign check** (`scen_update_campaign_progress`, `1fe6:7366`): after a new
  campaign, a loaded save and each result; sets locked records to 8 and unlocked ones
  from 8 to 0 (above).

## Scenario files

Record `n` (counted from 0) loads `scenNNa.usg`, `NN` in decimal: playing the
selection writes `id / 2` as two decimal digits into the template `SCEN00A.USG`
(`DS:0x6006`) and `'A'` plus the `id`'s high byte, 0 in every stock record, as the
letter (`ui_map_room_play_selected`, `1000:739c`, confirmed in T-042; see also
`usg.md`).

## Open questions

- Settled in T-042 (`docs/screens.md`, "The map room"): the `state` values, the
  progress, the looks.
- Whether a mission's end applies the same rules as the map room's developer keys,
  and whether a record at 0xc can be raised to 0xe (T-046).

## Parser

`xeno_formats::scendef` (T-019): `parse` returns a `ScenarioTable` of `Scenario`
records (`binrw`). `xeno-tool inspect scendef.bin` lists every record.
