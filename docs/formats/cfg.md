# The setup program, its record and `XENO.CFG`

## Status

**Confirmed** (T-039) from the unpacked `config.dvr` (16,160 bytes, SHA-256 in
`docs/re-setup.md`) and from the code in `xeno.exe` that reads what it hands over.
Neither `XENO.CFG` nor `XENO.KEY` is in a stock install. Keys and `XENO.KEY` are in
`key.md`. `xeno_formats::setup` parses both files.

Addresses in `config.dvr` are load-image offsets (Ghidra segment `1000`); its code and
data share one segment. `DS:x` is `xeno.exe`'s data segment, as in
`docs/re/dvr-protocol.md`.

## What runs when

1. `xeno.exe` copies a built-in default record into `DS:0xde` and runs `CONFIG.DVR`
   with its own command tail and an environment holding only `!=` and the record's
   address (`dvr-protocol.md`, "Loading"). `drv_find_host_record` (`0x02d4`) finds it.
   Run from DOS without it, `config.dvr` fills a record of its own instead.
2. `ui_init_setup` (`0x1c1e`) finds its strings and two key tables, reads the command
   tail, notes whether `XENO.KEY` exists, and unless the tail holds `!` reads
   `XENO.CFG` (`file_load_saved_config`, `0x319a`). The stock `setup.bat` runs
   `XENO !`, so it always asks every question again.
3. It measures the machine: CPU and FPU class, display adapter (mono, EGA, VGA, Tandy,
   an Amstrad BIOS), sound hardware (it loads `SOUNDDVR.DVR` as an overlay and asks
   its device check, slot `0x440`, about each letter: `snd_detect_devices`, `0x0c64`),
   joysticks and a mouse.
4. `ui_run_setup_menu` (`0x1d63`) walks a list of menu entries. Entry `n` writes its
   answer into byte `n` of the record. An answer comes from the command tail, from
   `XENO.CFG`, or from the user. Esc goes back; "Return to DOS (Y/N)?" exits with code
   0xff, and the game then exits too.
5. `file_load_unlock_codes` (`0x3c26`) hashes `XENO.KEY` into 16 words (`key.md`).
6. `drv_write_config_record` (`0x0790`) writes the record into the host.
7. `file_save_config_if_changed` (`0x31e7`) rewrites `XENO.CFG` if the answers differ
   from the saved ones (`file_write_saved_config`, `0x3221`).

Command-tail letters `V`, `D`, `A`, `Y`, `I` and `N` preselect answers (VGA; Disney,
AdLib, Tandy, IBM or no sound); `!` ignores `XENO.CFG`; anything else shows a help
screen and exits with code 0xfe.

## The configuration record

The answers are ASCII, one byte per menu entry, then optional text, a CR and 16 words:

| Byte | Host copy | Menu entry | Values |
|---|---|---|---|
| 0 | `DS:0x220` | none | `B` if `XENO.KEY` exists, else a space. Sets how the words are rotated (`key.md`) |
| 1 | `DS:0x221` | none | `M` if the BIOS reports a monochrome adapter, else a space. `xeno.exe` only opens its debug screen on one (`dbg_init_mono_monitor`) |
| 2 | `DS:0x222` | CPU Type | `0` 8088/8086, `1` 80188/80186, `2` 80286, `3` 80386, `4` 80486. Above `2` turns on the 386 patches |
| 3 | `DS:0x223` | FPU Type | space for none, `0` 8087, `2` 80287, `3` 80387, `4` 80486. Not read by the game |
| 4 | `DS:0x224` | Graphics Type | `V` VGA/MCGA, the only choice. Any other letter replaces the `M` of `MGAVIDEO.DVR` |
| 5 | `DS:0x225` | Music Type | `R` Roland, `A` AdLib, `C` Sound Blaster, `Y` Tandy, `I` IBM, `N` none; for `sounddvr.dvr`'s music init |
| 6 | `DS:0x226` | Sound Type | `D` Disney Sound Source, `A` AdLib, `C` Sound Blaster, `I` IBM, `N` none; for the digitised-sound init |
| 7 | `DS:0x227` | No. of Players | `1` or `2` |
| 8 | `DS:0x228` | Input Device | `M` mouse, `J` joystick 1, `G` joystick 2, `K` keyboard, `F` Amstrad joystick (offered only with an Amstrad BIOS) |
| 9 | not copied | Player 1 | The keyboard remap (`key.md`); bit 0 flips each time the keys are remapped |
| 10 | not copied | Joystick Alignment | A status byte (a space until calibrated); see "Joysticks" |
| 11 | not copied | Switched stick detect | A status byte |
| 12 | `DS:0x229` | P2 Input Device | As byte 8, plus `S` switched joystick |
| 13, 14, 15 | not copied | Player 2, P2 Joystick Alignment, P2 Switch stick detect | As bytes 9 to 11 |
| 16 | not copied | Memory check | Warns when the choices need more memory than is free |

Then, only when `XENO.KEY` exists, any text that followed a `\` in the command tail;
then a CR; then the 16 words of `key.md`. The host (`drv_decode_config_record`,
`1fe6:09fe`) copies bytes 0 to 8 and 12, reads decimal digits from byte 17 up to the CR
into a number it never stores, and rotates the words into flag words.

The built-in default record in `xeno.exe` is shorter than this layout (it has no CPU or
FPU byte). If `CONFIG.DVR` cannot be run, the game would read its graphics letter from
the wrong byte and ask for a driver other than `MGAVIDEO.DVR`. Not checked in DOSBox.

**What `xeno.exe` uses.** Only bytes 0 to 2 and 4 to 6 (above), plus the words. No code
in `xeno.exe` reads the FPU byte, the number of players or either input device (a scan
of every function in the Ghidra project; the keyboard interrupt handler and some key
handlers lie outside functions and were read separately). The `.dvr` modules share the
host's data segment and were not scanned for it.

## `XENO.CFG`

48 bytes, written only by `config.dvr`, read only by `config.dvr` and only if exactly
48 bytes long:

| Offset | Size | Contents |
|---|---|---|
| 0x00 | 1 | CPU class measured on the run that wrote it (record byte 2's digits) |
| 0x01 | 1 | FPU class measured (record byte 3's values) |
| 0x02 | 14 | Record bytes 2 to 15, as above |
| 0x10 | 16 | Player 1's keys (`key.md`, "Key tables") |
| 0x20 | 16 | Player 2's keys |

The record bytes are the saved answers: an entry whose saved answer is still possible
(listed among its choices and found on this machine) is answered with it. The compare
before saving starts at offset 1 against record byte 1, one field apart, so in practice
the file is written on most runs. T-038's DOSBox smoke run saw a 48-byte `XENO.CFG`
appear, which fits.

The port does not need it to run: the devices it chooses do not apply to a port, and
its key tables have no reader in `xeno.exe` (`key.md`). `xeno_formats::setup::
parse_saved_config` decodes it for the key map.

## Joysticks

- **Devices.** Joystick 1 and 2 are the game port's sticks A and B. Switched joystick
  (player 2 only) and Amstrad joystick are offered as letters; the switched-stick
  screen asks to hold the stick lower right.
- **Alignment** asks to hold the stick upper left, centre and lower right and reads
  both axes each time (the timing loop on port 0x201). The six values go into
  `config.dvr`'s own stick reader (at `0x0e68` for stick A, `0x0e74` for B), which is
  freed when it exits. They are not saved in `XENO.CFG` or handed to the game; only a
  status byte is (record bytes 10 and 14).
- **In the game** `xeno.exe` never reads the axes. Once per retrace (`gfx_on_retrace`,
  `1fe6:32a3`) it reads stick A's two buttons from port 0x201 into a byte at
  `DS:0x268`; who reads that byte is not traced.

The port implements no joystick input; this is a record of what the original offers.

## Open questions

- What reads `DS:0x268`, the joystick buttons (perhaps as mouse buttons).
- The status values of the alignment and switched-stick entries (`0x238d`, `0x25e7`).
- What the four build-switch bytes at `0x1b82` change (the second skips the "Keys are
  used by Player 1!" check and lets both players pick the same input device).
