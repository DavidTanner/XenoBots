# Screens and their flow (T-040)

How `xeno.exe` moves between its screens, from a Ghidra pass over the unpacked
executable (SHA-256 in `docs/re/dvr-protocol.md`), and how `xeno-app` models it.
Findings only; no listings. Addresses are Ghidra `segment:offset` and `DS:x` as in
`docs/re/dvr-protocol.md`.

## The original's flow

```
start-up -> MAINTHEM, intro cutscene -> drop -> log-in -> map room (the hub)
                                                  ^          |
   win / lose / triumph cutscene  <- mission  <---+----------+  play
   (not after training)               ^          |
                    training menu ----+          +-> training menu, review,
                                                     log-in again, quit
```

**Start-up** (`ui_run_startup_and_game`, `1000:fafc`, called from the entry point after
the drivers load): starts `MAINTHEM`, plays the intro cutscene (`ui_play_intro_cutscene`)
and, unless a key aborted it, the drop (`drv_run_vgaintro`); then loads the digitised
sounds, the models and a sprite bank, and runs the main loop.

**Log-in** (`ui_login_and_load_save_game`, `1000:70ec`): shows `LOGIN` fading in over
64 retraces (`ui_show_screen_fading_in`) and runs the name entry
(`ui_edit_name_with_blinking_cursor`). If that returns carry (ESC), the start-up caller
exits the game; the map room's N ignores it. Otherwise it loads `<NAME>.LOG` if there
is one (and marks a game as loaded, `DS:0x6005`), or starts a new campaign from
`scendef.bin` (`1000:712e`), then runs the campaign-won check `1fe6:7366`
(`vcl.md`, "TRIUMPH"). Details below, "The name entry".

**Map room** (`ui_run_map_room_loop`, `1000:6f26`): the hub. After the log-in it loops:
draw, wait for an input event, hand it to the screen's widgets, and run the action
the widgets chose (`DS:0x5f20`, handlers at `DS:0x5f44`). Its widget set (segment
`0x384c`) is a full-screen `MAPROOM` picture, the map area (`5..288 x 10..192`) with
one box per `scendef.bin` record (click to select, `scendef.md`), five buttons down the
right edge (button `n`'s handler at `DS:0x5f48 + 2n`, sprite handle at `DS:0x5f52`),
and a key handler with the table at **`DS:0x60bf`** (20 key-word pairs):

| Keys | Action |
|---|---|
| P, Enter, Space; button 0 | Play the selected scenario: load its `scenNNa.usg` if it is not the one loaded (`1000:739c`), then the mission |
| T; button 2 | The training menu (`1000:740c`), only once `DS:0x5fab` is non-zero (0 in the image; what sets it before a mission is not traced) |
| R; button 4 | The review screen (`ui_run_review_screen`, `1000:796e`, nested: the map room comes back after it) |
| N; button 1 | Save `<NAME>.LOG` and log in again (`1000:7401`) |
| Q, Alt+Q; button 3 | Save and quit (`1000:741a`) |
| Right, Down, Tab / Left, Up, Alt+Tab | Two handlers (`1000:7431`, `7445`) that start from the selection, presumably next / previous scenario (guess) |
| `=`, `-`, Ctrl+W, Ctrl+L, Ctrl+Space, Alt+F1 | Not traced |

The buttons' order on screen is not checked; their ids are 0 play, 2 log-in, 4
training, 6 quit, 8 review in the handler table. ESC is in no map room table.

**Training menu** (`ui_run_training_menu`, `1000:6df6`): `TRAINMEN` fading in over 32
retraces, widget set `0x3842`, key table **`DS:0x5e9f`** (6 pairs): H, N, S and C load
`HTRAIN`, `NTRAIN`, `STRAIN` or `CTRAIN.USG` and run the mission; ESC and Alt+Q go
back. After a training mission it returns to the map room with no cutscene.

**Mission** (`sim_run_mission_loop`, `1000:7e24`): runs the sim on the 60 Hz clock (at
most 10 ticks to catch up) until the quit flag `DS:0xe87e` is set. What it shows
depends on the object viewed: a table at `DS:0x6411` by the object's kind opens the
**humanoid**, **scorpion** or **scout console** (`HUMANCON`, `SCORPCON`, `SCOUTCON`)
or, for kind 3, the **situation room** (`SITROOM`, widget set `0x3822`). So the
situation room and the consoles are views inside a mission, not screens of their own.

**After a mission** (`1000:6f9e`, the map room's action 0): sets the scenario's `state`
to 12, runs the mission, fades out over 32 retraces, then plays `TRIUMPH` if the
campaign is complete (`DS:0x5fac`), else the lose sequence (outcome `DS:0x5fab` = 2)
or the win sequence (1), and goes back to the map room. The cutscenes are in `vcl.md`,
"Sequences".

**Saving**: `<NAME>.LOG` is written when the player logs in again or quits from the map
room (`1000:7423`, only if a game is loaded, `DS:0x6005`), and read at log-in
(`usg.md`, "Save files").

**Widgets.** Every screen is a list of 12-byte widgets (type word, state offset,
rectangle) opened with `ui_open_widget_set` (`1000:2343`); events go to them with
`ui_dispatch_event_to_widgets` (`1000:24a9`): mouse events to the widget under the
pointer, key events to the focused chain. Each type has an event handler in a table at
`DS:0x3778`; a key handler compares the event's key word (release bit masked off)
with its table and calls the first match. No separate briefing screen was found; the
scenario's `ORDR` text (`usg.md`) is presumably shown inside the mission (T-044).

### The name entry (T-041)

`ui_edit_name_with_blinking_cursor` (`1000:77ce`) starts colour-animation record 23
(colour 254, `palette.md`), opens a widget set of its own (segment `0x3857`, state at
`DS:0x6149`, copied from a 9-byte template), empties the name, and loops: draw the
widgets whose redraw bits are set, wait for an input event, hand it to the widgets,
until the done flag `DS:0x6148` is set. Then it turns record 23 off where it stands
(the colour keeps its last value), puts the previous widget set back, and returns carry
if the flag is `0xff`. The `LOGIN` picture itself carries the "Operator / Log in:"
label and the black field; the game draws only the name.

The widgets: a key handler (`ui_name_entry_on_key`, `1000:7882`), the name field (its
draw handler `ui_draw_name_entry`, `1000:7833`), a box with no drawing, and the
screenshot key (`1000:7d3f`, Shift or Ctrl with scan code 0x37, only with the hidden
switch of `key.md`). The key handler ignores events with bit `0x2000` and compares the
key word (bit 7 of the low byte masked off) with a table at `DS:0x616a`:

| Key word | Handler | Effect |
|---|---|---|
| `0x001c` (Enter) | `ui_name_entry_accept` (`1000:790b`) | Nothing if the name is empty. Else copies it into the save name template `DS:0x613b` (`NAME    .LOG`, padded with spaces to 8) and into the exit message at `DS:0x618c` ("THANKS FOR PLAYING, NAME.", put in the buffer shown on exit with slot `0x54`), and sets the done flag to 1 |
| `0x0001` (ESC) | `ui_name_entry_cancel` (`1000:794e`) | Puts the previous name back from the template and sets the done flag to `0xff` |
| `0x000e` (Backspace) | `ui_name_entry_backspace` (`1000:78f1`) | If the name is not empty, drops its last character and marks the field for redrawing |
| Anything else | `ui_name_entry_type_char` (`1000:78a7`) | If the name is shorter than 8, turns the key word into a character (`1fe6:3e86`, not traced); accepts `0`-`9` and `A`-`Z`, and `a`-`z` turned into upper case; appends it and marks the field for redrawing |

So a key with Shift, Ctrl or Alt never matches the table: Shift+Enter is typed as a
character and refused. The name is at most 8 characters, upper-case letters and digits.

**Drawing** (`ui_draw_name_entry`, when the field's redraw bits are set, so not before
the first change): font 0, cursor at (`DS:0x6164` + 5, `DS:0x6162` + 3) = (50, 69),
the field's top-left corner being (45, 66), all **opaque**: the name in colour 251 on
0; then, if it is shorter than 7, the string at `DS:0x6187` (two spaces and two
backspaces: clears the two cells after it and comes back); then, if it is shorter than
8, an underscore in colour 254 on 0: the cursor, which record 23 blinks. Nothing draws
the field before the first key is accepted, so the original shows no cursor until then
(to confirm in DOSBox).

**After the name** (`ui_login_and_load_save_game`): the save `NAME    .LOG` is loaded
with `1000:eefc` if it exists (`usg.md`, "Save files"); otherwise `1fe6:72a8` and the
`scendef.bin` load `1000:712e` start a campaign (on failure the game exits with a
message). The map room's N (`1000:7401`) saves first, then runs the log-in again;
there ESC (carry) goes back to the map room with the old name.

### Where it differs from T-040's list

The backlog guessed `Logo -> Login -> SitRoom -> MapRoom -> Briefing -> Mission ->
Review`. In the original the map room, not the situation room, is the hub after the
log-in; the situation room is one of the mission's views; there is no briefing screen;
and the review is opened from the map room rather than following each mission, which
ends in a cutscene instead.

## In the port

`crates/xeno-app/src/screens.rs` is a state machine stepped once per retrace with that
retrace's `InputEvent`s from the `InputLog`, so a log replays across screens. After the
intro and the drop (or straight away with `--no-intro`) it starts at the log-in.

| State | Shows | Keys |
|---|---|---|
| Log-in | `login.m@l` | Enter: map room. ESC: quit |
| Map room | `maproom.m@l` | P, Enter, Space: mission. T: training. R: review. N: log-in. Q, Alt+Q: quit (the original's keys) |
| Training menu | `trainmen.m@l` | H, N, S, C: training mission. ESC, Alt+Q: map room (the original's keys) |
| Review | `review.m@l` | ESC, Enter: map room |
| Mission | `sitroom.m@l`, or a console's `.m@l` | F1 to F4: situation room, humanoid, scorpion, scout. W won, L lost, C campaign won |
| Cutscene (won, lost, campaign won) | A synthetic card: green, red or yellow block | Enter, Space, ESC: map room |

Keys that only the port uses (log-in Enter, review, mission, cutscenes) are placeholders
until the screens are built (T-041 to T-044, T-056). A training mission goes back to
the map room without a cutscene, as in the original; T is not gated on `DS:0x5fab`.

- **Key words.** Each key going down is turned into the original's key word (Shift
  0x08, Ctrl 0x10, Alt 0x20 in the high byte, the set 1 code with `E0` dropped in the
  low byte, `screens::key_word`) and looked up in the screen's (key word, action)
  table; the first match acts. Auto-repeat counts as a press.
- **ESC.** A screen claims ESC exactly when its table binds it (log-in, training menu,
  review, cutscenes); then `Translator::screen_claims_escape` is set and ESC reaches the
  game. On the map room and in a mission ESC still leaves fullscreen or quits, as the
  original binds no ESC there.
- **Fades.** Leaving a screen fades the palette out over 32 retraces (`Dac::fade_out`);
  the next screen is drawn once the black is uploaded and fades in over 32 retraces
  (64 for the log-in), ending one level short as the driver's fade does. Keys pressed
  while fading out are dropped. The colour animations a screen starts are not run yet
  (the log-in cursor blink, T-041).
- **Labels.** There is no font yet, so the window title names the screen and its keys.
- **Mouse.** The pointer and buttons reach the screens through the log
  (`docs/game-loop.md`, "Input"), and a placeholder arrow is drawn at the pointer, but
  no screen acts on a click yet (the widgets are T-042 on).
- **Not modelled**: the name entry, the save file, scenario selection, the widgets, the
  real cutscenes (they need the sequence entries at `DS:0x606a`, `0x6016`
  and `0xedb0`), the 320x240 mode of the cutscenes (the placeholders are 320x200), and
  the music.
- `--screen <name>` starts at a screen (skipping the intro and drop); `--screen
  test-card` shows the synthetic test card instead of the screens.

## Symbols

Named in the Ghidra project in this pass (T-040), all `confirmed` from the code read:

| Address | Name | Purpose |
|---|---|---|
| `1000:fafc` | `ui_run_startup_and_game` | Music, intro, drop, sounds, models, then the main loop |
| `1000:70ec` | `ui_login_and_load_save_game` | Log-in screen, then load the player's save or start a campaign (was `file_load_save_game`, T-024) |
| `1000:6f26` | `ui_run_map_room_loop` | The map room hub and its action loop |
| `1000:6df6` | `ui_run_training_menu` | Training menu |
| `1000:7e24` | `sim_run_mission_loop` | A mission: sim ticks, views and input until the quit flag |
| `1000:796e` | `ui_run_review_screen` | Review, nested in the map room |
| `1000:2343` | `ui_open_widget_set` | Opens a screen's widget list |
| `1000:24a9` | `ui_dispatch_event_to_widgets` | Hands an input event to the widgets |

## Open questions

- The map room's untraced keys, what sets `DS:0x5fab` before the first mission (the
  training key's gate), and the review screen's keys (T-042, T-044).
- How a mission ends (who sets `DS:0xe87e`) and how the player switches between the
  situation room and the consoles (T-043, T-056).
- The mouse: the widget dispatch's event bits (`0x80` mouse, bit 0 release, `0x14`
  click) and the pointer slots `0x1496`–`0x14b2`.

T-041 named the name entry's routines (`ui_draw_name_entry`, `ui_name_entry_on_key`,
`ui_name_entry_type_char`, `ui_name_entry_backspace`, `ui_name_entry_accept`,
`ui_name_entry_cancel`) and the text services; the list is in
`docs/re/dvr-protocol.md`.
