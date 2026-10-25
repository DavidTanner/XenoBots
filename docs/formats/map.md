# `.map` editor terrain

## Status

**Layout confirmed** (T-024), from `mapdvr.dvr`'s save code and the `xeno.exe`
routine that loads a `.map`. These are **not** key or joystick mappings (`config.dvr` keeps its keyboard tables in
`XENO.CFG`, not in the install; `cfg.md`, `key.md`). They are the map
editor's terrain files, made of `.usg` `MAPP` records (`usg.md`).
`xeno_formats::map` reads both stock files to their last byte.

## Files

`default.map` (122 bytes, 5 records) and `backup.map` (26 bytes, 1 record).
`mapdvr.dvr` also names `WORKING.MAP`, which nothing uses.

## Layout

24-byte `MAPP` terrain records (`xeno_formats::usg::TerrainFeature`) follow each
other. A size word of 0 ends the list and the file, with no padding.

Positions (+0x04 x, +0x08 y, 16.16) are **relative to the editor's map origin**
(`xeno.exe` `DS:0x14e8` x, `DS:0x14ec` y and `DS:0x14f0` height), which start-up
(`1000:27e3`) sets to (128, 128) and height 0 (`xeno_formats::map::ORIGIN`). Loading
adds the origin to +0x04, +0x08 and +0x0c of every record, and saving subtracts it
from x and y. The `.usg` files' closing record is the editor's, moved by the origin.

The editor's save routine (`file_write_map`, driver offset `0x0711`, after the MZ
header) creates the file (`file_create_map`) and writes (T-048, exactly):

1. For each circle, in the editor's order (`file_write_map_circles`, `0x078c`), a hill
   from the template at driver data `0x18` (size 24, kind 0x14, 16 point slots, shape
   program 0x14, 0): +0x04/+0x08 the centre less the origin (32-bit), +0x10 the
   radius `r` (8.8), +0x12 `d = r * 0xaaaa >> 16` (two thirds, rounded down), +0x14
   `h = d >> 1`, and +0x0c `h` as 16.16 (`h << 8`). That is the 3:2:1 ratio seen in
   `.usg` records.
2. A closing block (the template at driver data `0x30`, written as it is): kind 0x12,
   8 point slots, x and y 0x8000_0000 (the most negative 16.16 position), z 0x2d00,
   +0x10 5, +0x12 0x14, +0x14 0x2d, shape 0x12, 0. It is `backup.map`'s only record,
   and with the origin added it is the last `MAPP` record of every `.usg`. It closes
   the list but nothing needs it: every list ends at its 0 word anyway.
3. The 0 end word (2 bytes).

The circles come from the terrain on entry (`scen_import_terrain_circles`, `0x080d`):
each record of kind 0x14 or 0x16, in order, becomes a circle of its centre and its
+0x10 radius (`scen_import_terrain_circle`, `0x0631`); everything else is dropped. A
circle drawn with the mouse gets the distance from its centre as its radius.

`xeno_formats::map::write` writes records and the end word, `to_world` adds the
origin as loading does, and `editor_save` gives what the editor writes for a world
terrain it imported and did not change. Both stock `.map` files round-trip byte for
byte, `backup.map` is `editor_save` of no terrain, and **every stock `.usg`'s terrain
is exactly `editor_save` of itself, loaded back** (`real_assets_maps_round_trip`): all
of them were made with this editor.

What each field means to the game is in `usg.md`, "`MAPP`: terrain features".

So `backup.map` is a save with no circles. `default.map` does not fit this: it has
extra blocks, an open hill (kind 0x16), a hill without the 3:2:1 ratio, and a closing
record offset by 128. It probably came from an earlier editor or was made by hand.
On import the editor treats kinds 0x14 and 0x16 as circles and skips the rest, so
saving turns an open hill into a hill and drops the blocks.

## How the game uses it

Only the editor touches `.map` files:

- **Load** (Alt+L, `ui_load_map`, `0x066f`) gets a name from host slot `0x2fe`, the host's
  file-open dialog (`docs/re/dvr-protocol.md`). Host slot `0x1506` (`xeno.exe` `1fe6:6a97`) then reads the file into the
  `MAPP` buffer (`DS:0x150e`) and adds the origin.
- **Save** (Alt+S, `ui_save_map_with_dialog`, `0x0705`) gets a name from host slot `0x302`, the host's
  "SAVE GAME" file dialog. A path at `0x06dc`, which
  nothing in the driver calls directly, offers `DEFAULT.MAP`.
- **Apply** (Alt+E, `0x06b5`) writes `BACKUP.MAP`, loads it as above and deletes it.
  **Leave** (Alt+Q, `0x06cf`) writes `BACKUP.MAP` and keeps it, leaving the terrain
  as it was.

The game needs neither file to start or play, so neither is in
`xeno_formats::files::EXPECTED_FILES`.

## Open questions

- None about the layout. The terrain editor itself (a 3D editor in the mission's view)
  is described in `docs/screens.md`, "The scenario editor (T-048)"; a retail build
  reaches it with unlock word 9 (Ctrl+M in the situation room).

## Parser

`xeno_formats::map` (T-024): `parse` returns a `usg::Terrain`; `write`, `to_world`,
`editor_save` and `closing_block` (T-048) as above. `xeno-tool inspect`
lists the record kinds.
