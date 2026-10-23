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
(`xeno.exe` `DS:0x14e8` x, `DS:0x14ec` y, and a third value at `DS:0x14f0`). Loading
adds the origin to +0x04, +0x08 and +0x0c of every record, and saving subtracts it
from x and y. The `.usg` files' closing record suggests an origin of (128, 128).

The editor's save routine (driver offset `0x0711`, after the MZ header) writes:

1. A kind-0x14 circle for each circle placed, with the centre and the radius `r`
   (8.8) at +0x10, then `2r/3` at +0x12, `r/3` at +0x14, and `r/3` as 16.16 at +0x0c.
   That is the 3:2:1 ratio seen in `.usg` records.
2. A closing kind-0x12 record at the most negative 16.16 position. It is
   `backup.map`'s only record, and with the origin added it is the last `MAPP` record
   of every `.usg`.
3. The 0 end word.

So `backup.map` is a save with no circles. `default.map` does not fit this: it has
extra kind-0x12 records, a kind 0x16, a circle without the 3:2:1 ratio, and a closing
record offset by 128. It probably came from an earlier editor or was made by hand.
On import the editor treats kinds 0x14 and 0x16 as circles and skips the rest.

## How the game uses it

Only the editor touches `.map` files:

- **Load** (key handler `0x066f`) gets a name from host slot `0x2fe`, the host's
  file-open dialog (`docs/re/dvr-protocol.md`). Host slot `0x1506` (`xeno.exe` `1fe6:6a97`) then reads the file into the
  `MAPP` buffer (`DS:0x150e`) and adds the origin.
- **Save** (`0x0705`) gets a name from host slot `0x302`, the host's
  "SAVE GAME" file dialog. A path at `0x06dc`, which
  nothing in the driver calls directly, offers `DEFAULT.MAP`.
- **Apply** (`0x06b5`) writes `BACKUP.MAP`, loads it as above and deletes it.
  **Leave** (`0x06cf`) writes `BACKUP.MAP` and keeps it.

The game needs neither file to start or play, so neither is in
`xeno_formats::files::EXPECTED_FILES`.

## Open questions

- Kinds 0x12 and 0x16, the rest of the record and the third origin value (T-059).
- Whether a retail build can reach the scenario editor, whose key table (`DS:0x5bd8`)
  opens this one (`1000:448c`).

## Parser

`xeno_formats::map` (T-024): `parse` returns a `usg::Terrain`. `xeno-tool inspect`
lists the record kinds.
