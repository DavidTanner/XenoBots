# `state.rst`

## Status

**Confirmed unused** (T-024). No binary names it: `xeno.exe` (packed and unpacked), the
`.dvr` modules and `vga240.mod` were searched for `RST`, `STATE` and `.RST`. It is not
a save game. Saves are `<NAME>.LOG` (`usg.md`, "Save files").

## Layout

61 bytes of DOS text: ASCII lines ending in `CR LF`, then the end-of-file byte `0x1a`.
First comes a line of five small numbers and some blank lines. Then an INI-style
`[FILES]` section has a path to a `.bat` file on `C:` and a line of two numbers. The
path is under the same `\RGAME` directory as the `ORDERS1.TXT` path in `xeno.exe`, so
this looks like state left by a setup or build tool on the developers' machines.

## Parser

`xeno_formats::rst` (T-024): `parse` returns the lines and whether the end-of-file
byte was there. `Restart::sections` lists the section names. A text format, so no
`binrw`. `xeno-tool inspect state.rst` prints a summary.
