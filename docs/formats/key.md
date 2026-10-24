# Keys: `config.dvr`'s key tables, `XENO.KEY` and the game's own keys

## Status

**Confirmed** (T-039) from the unpacked `config.dvr` and `xeno.exe`. The short answer:
`config.dvr` lets each player remap a keyboard "stick" and saves it in `XENO.CFG`, but
**`xeno.exe` never reads those keys**. `XENO.KEY` is not a key map at all: it holds
developer unlock codes. The game's real keys are fixed tables in `xeno.exe`. The record
and `XENO.CFG` are in `cfg.md`; `xeno_formats::setup` parses both files and reads the
default tables.

## Key tables

Each player has a 16-byte table. Bytes 0 to 11 are slots in the remap screen's 3x4
grid, row by row; bytes 12 to 15 are never touched (zero in the defaults). Each slot
holds a set 1 make code. 0x60 marks a slot the grid does not use.

| Slot | Label on the remap screen |
|---|---|
| 0 | L UP |
| 1 | UP |
| 2 | R UP |
| 3 | LEFT |
| 5 | RIGHT |
| 10 | FIRE |
| 4, 6, 7, 8, 9, 11 | unused (0x60); the third row is blank |

There is no down, back or second button: a keyboard stand-in for a joystick with one
button.

**Defaults.** Player 1 uses the numeric keypad (7, 8 and 9 for the top row, 4 and 6,
Ins to fire); player 2 uses Q, W, E, A and D, and left Shift to fire. They are data in
`config.dvr`: the last 32 bytes of the first entry of its menu list, whose head the
menu walker (`0x1e59`) loads with `mov si, 0x1130`, right after the label `FIRE`.
`ui_init_setup` copies them to `0x1b4e` (player 1) and `0x1b5e` (player 2), and
`XENO.CFG` replaces them when present. `xeno_formats::setup::default_keys` reads them
from the user's `config.dvr` the same way, checking the walker's instruction, the label
and that every slot holds a code.

**Remapping** (`0x20ff`, reached from the "Player 1" and "Player 2" entries):

- The grid shows each used slot's key name from one of three 128-entry name tables,
  chosen by the machine type `config.dvr` detected (one is for an Amstrad BIOS).
- "Remap keyboard (Y/N)?"; on Y, "(Press key selection.)" and a key per used slot, in
  slot order. `config.dvr` hooks the keyboard interrupt and takes the next byte from
  port 0x60 without bit 7: a make code, with any `E0` prefix dropped, so a grey arrow
  key and its keypad twin give the same code.
- **Refused:** F1 (0x3b) and F10 (0x44) beep and wait for another key. Esc (0x01)
  abandons the remap.
- **Duplicate check:** if a code is in two slots of one table: "Keys are used multiple
  times!", "Use selection anyway (Y/N)?"; N remaps again. For player 2 with player 1 on
  the keyboard, a code in player 1's table gives "Keys are used by Player 1!" the same
  way, unless a build switch skips it (it is set in this build).
- Bit 0 of the entry's record byte flips, so the answers differ from the saved ones and
  `XENO.CFG` is written again.

**No consumer.** Nothing in `xeno.exe` opens `XENO.CFG`, and the record carries no key
codes, so the remapped keys never reach the game. Its keyboard interrupt handler reads
its key-state table at fixed codes only for Shift, Ctrl and Alt, and no code indexes it
with these slots (`cfg.md`, "What `xeno.exe` uses"). The tables look like the setup
program of a NovaLogic engine shared with games that are steered with a stick.

## The game's keys

`dos_install_keyboard_handler` (`1fe6:3cb8`) installs an INT 9 handler at `1fe6:3d53`
(outside any Ghidra function). It keeps a table of the last byte seen per code at
`DS:0x1af0` (indexed by the code without bit 7; `E0` keys share their plain key's
entry), builds a modifier byte (0x08 Shift, 0x10 Ctrl, 0x20 Alt) and posts an event
whose key word is `modifiers << 8 | raw byte`, with an event type for press, repeat or
release.

Fixed keys, in every screen:

| Key | Effect |
|---|---|
| Ctrl+Alt+Del | Reboots the machine |
| Ctrl+Alt+` | Raises interrupt 5 (BIOS print screen) |
| Shift+F1 | Calls `sounddvr.dvr`'s slot `0x450` (toggles a driver flag) |

Screens dispatch the key words through tables of (key word, handler) pairs: a key
widget masks the release bit (bit 7 of the low byte) off the event's word and calls
the first entry that matches (T-040, `docs/screens.md`). Those found so far are the
**map room's** at `DS:0x60bf` (20 pairs) and the **training menu's** at `DS:0x5e9f` (6
pairs), both listed in `docs/screens.md`; the **robot consoles'** at `DS:0x6b07`
(humanoid, 33 pairs), `DS:0x676a` (scorpion, 38) and `DS:0x6efa` (scout, 40); the
**situation room's** at `DS:0x5b20` (32 pairs; T-043, `docs/screens.md`, "The mission's
views"); and the scenario editor's table at `DS:0x5bc3`
(`ui_dispatch_editor_key`). The mission views' keys that choose views and orders are
decoded (T-043); the rest of their handlers (weapons, mines and buoys, the camera) are
work for the mission (T-056). The
port's input layer already delivers these keys as scan codes with the modifiers held
(`docs/game-loop.md`).

## `XENO.KEY`

Not a key map: a text file of up to 16 lines `n:code`, read by `config.dvr` only
(`file_load_unlock_codes`, `0x3c26`) and only if it exists. Each code is hashed into
word `n - 1` of the 16 words at the end of the record (`cfg.md`); `xeno.exe` compares
some of them with fixed values to turn on developer features.

**Reading** (`file_hash_unlock_line`, `0x3c6e`), byte by byte, at most 16 lines:

- LF is skipped; a line ends after its CR (which is hashed) or at the end of the file.
- Until the first `:`, digits build the slot number `n` (16-bit, wrapping). Slots
  outside 1 to 16, and lines without digits, are ignored.
- Every other byte is hashed. Bytes `a` to `y` are upper-cased first (`z` is not).
- The hash starts at 0xffff. Each byte is **OR**ed into the low half, then eight times:
  rotate left by one, and if the bit that came round is 1, XOR with 0x480. Because of
  the OR, the first byte of a code never changes the hash.
- Slots without a line keep a value `config.dvr` copies from its own code bytes.

**In the game.** The host rotates each word left by `((0x62 XOR byte 0) - 0x20) AND 3`:
0 when byte 0 is `B` (the file exists), 2 otherwise, so without the file nothing can
match. The words land in flag words:

| Word | Flag | Compared in `xeno.exe` | Unlocks |
|---|---|---|---|
| 0 | `DS:0x1594` | equal to 0xcfc0, and record byte 1 is `M` (`dbg_init_mono_monitor`) | Debug text on a monochrome adapter |
| 2 | `DS:0x1aba` | equal to 0xd0ca (`1fe6:3674`, the event reader) | A screenshot key (Shift with code 0x37, the keypad `*` / Print Screen key), Ctrl+Alt+F11 (a toggle) and Ctrl+Alt+F12 (a timing benchmark that runs while F12 is held, `1fe6:3900`) |
| 6 | `DS:0x616` | not 0x5981 | When equal, `mgavideo.dvr` keeps its plain palette upload rather than the `rep outsb` one (`dvr-protocol.md`) |
| 8 | `DS:0x1598` | equal to 0xba17 | Ctrl+P in the cockpit: sets some of the robot's values to full (not decoded further). In the map room (T-042): `=` and `-` move the progress, Ctrl+W wins the selected scenario and Ctrl+L loses the loaded one (`docs/screens.md`); `xeno_formats::campaign` reads the value from the user's `xeno.exe` |
| 9 | `DS:0x5bc1` | equal to 0xab65 (`ui_dispatch_editor_key`) | The scenario editor's Ctrl keys, including the terrain editor; Alt+F1 in the map room opens the editor (`ui_map_room_open_editor`, `1000:6bcf`; value read by `xeno_formats::campaign`) |
| 15 | `DS:0x618` | equal to 0xe263 (`mgavideo.dvr`'s stub at slot `0x1558`) | Screenshots to `CAMERA00.LBM` |
| 1, 3, 4, 5, 7, 10 to 14 | `DS:0x15a4`, `0x16f6`, `0x1676`, `0x16f8`, `0x1596`, `0x159a`–`0x15a2` | not traced | |

`xeno_formats::setup::parse_unlock_codes` reads a file the same way and gives each
slot's hash; nothing in the port uses them yet.

## Open questions

- The handlers of the mission views' keys that act on the running mission (T-056).
- What the untraced unlock words gate.
