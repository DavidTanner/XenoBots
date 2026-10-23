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
the widgets chose (`DS:0x5f20`, handlers at `DS:0x5f44`: 0 plays the loaded mission,
2 opens the training menu). Its widget set (segment `0x384c`) is a full-screen
`MAPROOM` picture, the map area with one box per `scendef.bin` record, five buttons
down the right edge, and a key handler with the table at **`DS:0x60bf`** (20 key-word
pairs). The details are below, "The map room (T-042)"; the keys and buttons:

| Keys | Button (id) | Action |
|---|---|---|
| P, Enter, Space | 2nd (0) | Play the selected scenario (`ui_map_room_play_selected`, `1000:739c`): nothing without a selection; else load its `scenNNa.usg` if it is not the one loaded, then the mission (action 0) |
| T | 4th (4) | The training menu (`1000:740c`, action 2), only while `DS:0x5fab` is not 0 |
| R | 1st (8) | The review screen (`ui_run_review_screen`, `1000:796e`, nested: the map room comes back after it) |
| N | 3rd (2) | Save `<NAME>.LOG` and log in again (`1000:7401`) |
| Q, Alt+Q | 5th (6) | Save and quit (`1000:741a`) |
| Right, Down, Tab | | Next selectable scenario (`ui_map_room_select_next`, `1000:7431`) |
| Left, Up, Alt+Tab | | Previous selectable scenario (`ui_map_room_select_previous`, `1000:7445`) |
| `=`, `-` | | Developer keys, only with unlock word 8 (`key.md`): progress up (to at most 16) or down |
| Ctrl+W, Ctrl+L | | Developer keys, the same word: the selected scenario won, or the loaded one lost (below, "Campaign rules") |
| Ctrl+Space | | Play the loaded mission at once (`1000:73fa`, action 0), with no check |
| Alt+F1 | | The scenario editor, only with unlock word 9 (`1000:6bcf`) |

The ids index the button handler table (`DS:0x5f48`) and sprite table (`DS:0x5f52`).
ESC is in no map room table.

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

**After a mission** (`1000:6f9e`, the map room's action 0): sets `DS:0x5fab` to 0 and
the selected scenario's `state` to 12 ("in play"), runs the mission, fades out over 32 retraces, then plays `TRIUMPH` if the
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
with its table and calls the first match. Mouse events carry the driver's bits (1
moved, 2 and 4 left button down and up, 8 and 0x10 right button down and up); the
dispatcher (`1000:2512` on) gives a press to the widget under the pointer and captures
the pointer there until the same button comes up, adding 0x80 while the pointer is
over the capturing widget and 0x100 when an event goes to it from elsewhere, and sends
0x200 and 0x400 to the widgets the pointer enters and leaves. Mouse handlers are in
the table at `DS:0x3608`, draw handlers at `DS:0x3328`. No separate briefing screen was found; the
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
| `0x001c` (Enter) | `ui_name_entry_accept` (`1000:790b`) | Nothing if the name is empty. Else copies it into the save name template `DS:0x613b` (`NAME    .LOG`, padded with spaces to 8) and into the exit message at `DS:0x618c` (a thank-you line: fixed text, padded so the name lands at `DS:0x61a0`, then a full stop and a NUL written by `1000:7927`; shown on exit with slot `0x54`, from `mov si, 0x618c` at `1000:7946`), and sets the done flag to 1 |
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

### The map room (T-042)

From a Ghidra pass over the map room's widgets, their draw and mouse handlers, the key
handlers and the campaign routines in `kernel` segment `1fe6`. Rectangles are (top,
left, bottom, right) as the code stores them; the dispatcher's hit test leaves the
bottom and right edges out.

**Widgets** (segment `0x384c`, 13 widgets from offset 0x10 to 0xac; each one's state
is in a 25-byte block at `DS:0x5f78`, copied from a template in segment `0x38e9`).
Each widget type is an offset into three handler tables: draw (`DS:0x3328`), mouse
(`DS:0x3608`) and key (`DS:0x3778`).

| Type | Rectangle | What |
|---|---|---|
| `0x36` | the screen | The picture: loads `msprite5` at handle 0x200 and `MAPROOM`, shows it fading in, starts colour-animation record 21 (`1000:7176`) |
| `0x38` | (10, 5, 192, 288) | The map area: the boxes, the selection and the panels (`ui_draw_map_area`, `1000:7255`; mouse `ui_map_area_on_mouse`, `1000:72fa`) |
| `0x3a` | none | The hover label (`ui_draw_map_tooltip`, `1000:773c`); no mouse handler |
| `0x3c` x5 | (137, 271, 147, 311), then 9 rows lower each | The buttons (`ui_draw_map_room_button`, `1000:71fc`; mouse `ui_map_room_button_on_mouse`, `1000:721c`). Their ids, from the template, are 8, 0, 2, 4, 6 top to bottom: review, play, log-in, training, quit, as `MAPROOM`'s own art labels them |
| `0x3e` | none | The key table (`1000:737f`) |
| `0x148`, `0x14c`, `0x14e` | none | Keys every screen has (the screenshot key `1000:7d3f`, and `f3ef`, `f405`, not traced) |

**Sprites.** All from `msprite5` (handles 0x200 on, 8 per sprite: `b@n.md`). The
buttons are drawn **opaque** (slot `0x5c4`) at their rectangle's corner with the
handle from `DS:0x5f52 + id` (40 by 10 pixels); a pressed button uses the next
sprite. The boxes are drawn **colour 0 transparent** (slot `0x5d8`) at each record's
`map_box` corner (9 by 9): with handle 0x250 when `unlock_level` is above the
progress `DS:0x5f9b`, else with the handle for `state` from the 7-word table
`DS:0x5f5c`. The selected record (its `id` equal to `DS:0x5f97`) then gets handle
0x268 over its box, also transparent; the pixels under that rectangle are saved first
and put back before the next draw. Looks by `state`, as sprite numbers in the bank:

| `state` | Meaning | Box (sprite) | Hover label | Alert level |
|---|---|---|---|---|
| 0 | Open | 14 | ready | level 1 of 4 |
| 2 | Open, alert raised once | 15 | ready | level 2 |
| 4 | Open, raised twice | 16 | ready | level 3 |
| 6 | Open, raised three times (no further) | 16 | ready | level 4 |
| 8 | Locked: its `unlock_level` is above the progress | 10 | not yet | under way |
| 10 | Won; can never be selected again | 11 | won | under way |
| 12 | In play: a mission of it was started and not won | 12 | fighting | under way |
| unlock above progress | | 10 | | |

The columns give the sense in our words; the words themselves are game text in the host's string segment
(Ghidra `38fa`), read through `DS:0x5f6a + state` (hover) and `DS:0x60b1 + state`
(alert level).

**Panels**, drawn by `ui_draw_map_room_status` (`1fe6:7406`) after the boxes every
time the map area is drawn: in font 0, colour 0xe0, transparent text,
- sprite 17 opaque at the rectangle `DS:0x5f2c` = (25, 10, 32, 58), then the player's
  name at its corner;
- sprite 18 opaque at `DS:0x5f34` = (25, 254, 32, 284), then the score `DS:0x5f9d` as
  five decimal digits (`1fe6:4a6c`; leading zeros presumably kept, as the scenario
  number code reads the last two of the five);
- then in font 4 (`ui_draw_selection_info`, `1fe6:749c`): sprite 19 opaque at
  `DS:0x5f24` = (172, 100, 190, 220) and, if a scenario is selected, its name from
  `scendef.bin` (whose line break goes back to the left edge and down), the label at
  `DS:0x60a4` and the alert word for its `state`, all at that corner;
- once the campaign is won (`DS:0x5fac`), a box (68, 90, 112, 230) filled with colour
  0xa0, framed in 0xe0 (slots `0x51a`, `0x52a`), and a message from the string segment.

**The hover label** (`ui_draw_map_tooltip`): while the pointer is over a scenario's
box with no button held (selectable or not), a box in colour 0xa0 framed in 0x80
above the scenario's box (its bottom 3 rows above the box's top, or at the top for
states 8 to 12), sized to its text (4 pixels a character plus 3, by 9 rows), centred
on the box and kept between `x` 10 and 285 (265 below row 120, clear of the buttons),
with the hover word for its `state` in font 2, colour 0xb6 on 0xa0, opaque. The pixels
under it are saved and put back when it goes.

**Selection.** `DS:0x5f97` holds the selected record's `id` (0xffff: none; a new
campaign starts with none) and `DS:0x5ff5` its offset. A record is **selectable**
when its `unlock_level` is at most the progress and its `state` is not 10.
`ui_map_room_set_selection` (`1000:74c4`) then accepts it only if `DS:0x5fab` is not 0,
or the record is in play (12); otherwise it calls `1000:0270` (presumably a beep) and
keeps the old selection. So after a mission that ended with no result, only the
scenario in play can be picked.
- **Click**: the map area acts on a button's **release** (the press only captures the
  pointer), hit-testing the pointer then (`scen_hit_test_map_box`, `1fe6:72f8`, the
  first record whose `map_box` holds it, edges included). The left button selects; the
  right button selects and plays at once. A miss, or a record that is not selectable,
  changes nothing.
- **Keys**: next and previous step through the table by one record, wrapping, skipping
  those that are not selectable, for at most one round, and set the result. With no
  selection, both pick the first selectable record (without the `DS:0x5fab` check).
- **Hover**: pointer movement with no button held sets `DS:0x5f0f` to the record
  under the pointer (selectable or not, a hit-test only) for the hover label; leaving
  the map area clears it.

**Buttons.** A press (left or right) on a button captures the pointer, remembers the
button in `DS:0x5f0b` and flips `DS:0x5f0d` between 0 and 8, the offset added to the
remembered button's sprite handle; a release of the same mouse button over it flips it
again and runs the button's handler. A release elsewhere does nothing, so the button
stays drawn pressed until the next press flips the offset (a quirk of the original).

**Progress** `DS:0x5f9b`: a new campaign (`scen_start_new_campaign`, `1fe6:72a8`) sets
it to 1, the selection and loaded scenario to none, the score, win, loss and other
counters (`DS:0x5f9d`..`0x5fa7`) to 0, `DS:0x5fab` to 0xff and the campaign-won flag
`DS:0x5fac` to 0. Then `scen_update_campaign_progress` (`1fe6:7366`, also run after a
save is loaded and after each result) sets the campaign-won flag when every record
has been won (`DS:0x5f9f` equal to the count), and otherwise walks the table: a record
whose `unlock_level` is above the progress becomes 8, a record at 8 that is now within
it becomes 0, and for the open records (not won) it notes the lowest and highest
`unlock_level` (`DS:0x5ff7`, `0x5ff9`, their difference in `DS:0x5ffb`) and their
number (`DS:0x5fff`). The progress rises by one after a won scenario if fewer than
three records were open; the developer keys move it by hand.

**Campaign rules** (from the developer keys' handlers `1000:752d` and `75f3`, which
are presumably what a mission's end does too; the mission code is not traced):
- **Won**: the record becomes 10, the selection clears, the win count rises,
  `DS:0x5fab` = 1, the score rises (`1000:7626` from the mission's values, plus 1000),
  progress as above; then, if `DS:0x5ffb` is not 0, each open record that is not at 6
  raises its alert by 2 with a chance from the table `DS:0x6094` by its
  `unlock_level - DS:0x5ff7` (0, 1/2, 3/16, 1/16 for 0 to 3; from the host's random
  word `1000:0283`); then the first selectable record is selected. A record at 12 is
  not excluded, so it could reach 14, past the tables' end (not seen; to check).
- **Lost**: the loaded scenario is forgotten (`DS:0x5f99` = 0xffff, so play reloads
  it), `DS:0x5fab` = 2, the loss count and score as above. Its `state` stays 12.
- **Training gate** `DS:0x5fab`: 0xff for a new campaign, 0 from the start of a campaign
  mission until its result, 1 won, 2 lost, 3 while in the training menu. T opens the
  training menu whenever it is not 0, so only right after a campaign mission that
  ended with no result is training refused.
- **Alert**: when a scenario is played its `state` is passed to `1000:68a0`, which adds
  a value from the table `DS:0x5a3f` by `state` to a field of the mission's command
  object (`[DS:0x568e] + 0x2e`), so a higher alert makes the mission harder in some way
  (T-051, T-058).

**Scenario files.** Play writes `id / 2` as two decimal digits into the template
`SCEN00A.USG` (`DS:0x6006`) and `'A'` plus the high byte of the `id` (always 0 in a
stock table) as the letter, so record `n` loads `scenNNa.usg`, `n` counted from 0.

**The pointer** is `mgavideo.dvr`'s (slots `0x1496`–`0x14b2`, `docs/re/dvr-protocol.md`):
eight shapes in its fourth data block (`DS:0x626`): a word offset each, then per shape
its width, height, hot spot `x` and `y` bytes and the offsets of an image and a mask of
`width * height` bytes. The driver's set-up picks shape 0 (an arrow, 9 by 11, hot spot
(1, 1)) and colour 12; shape 1 is shown while the map room loads its picture. Each
frame `gfx_update_pointer` clamps the position to the screen, puts back the pixels
under the old pointer and draws the new one at the position minus the hot spot,
clipped to the screen: where the mask is 0 it writes the image byte AND the colour
(so 0xff gives colour 12 and 0 black), elsewhere it leaves the screen alone. The
sprite blitters hide it while they draw over it.

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
| Log-in | `login.m@l`, the name entry (T-041) | Letters and digits: the name. Backspace. Enter: log in, map room. ESC: quit, or the map room after N |
| Map room | `maproom.m@l`, the boxes, panels and buttons (T-042) | Click or Tab/Right/Down, Alt+Tab/Left/Up: select. P, Enter, Space: play the selection. T: training. R: review. N: log-in. Q, Alt+Q: quit (the original's keys); the buttons as their keys |
| Training menu | `trainmen.m@l` | H, N, S, C: training mission. ESC, Alt+Q: map room (the original's keys) |
| Review | `review.m@l` | ESC, Enter: map room |
| Mission | `sitroom.m@l`, or a console's `.m@l` | F1 to F4: situation room, humanoid, scorpion, scout. W won, L lost, C campaign won |
| Cutscene (won, lost, campaign won) | A synthetic card: green, red or yellow block | Enter, Space, ESC: map room |

Keys that only the port uses (review, mission, cutscenes) are placeholders until the
screens are built (T-043, T-044, T-056). A training mission goes back to
the map room without a cutscene, as in the original.

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
  while fading out are dropped. The colour records are `mgavideo.dvr`'s own list, read
  from the user's copy; the log-in holds them over its fade in.
- **Log-in** (`crates/xeno-app/src/login.rs`, T-041): as "The name entry" above. Typed
  characters arrive as `InputEvent::Text`; Enter, ESC and Backspace as key words with
  no modifier. Keys typed during the fade in count at once (the original queues them
  until its fade is over; the result is the same). Record 23 blinks colour 254 from the
  end of the fade in until the name is done, then stops where it stands. The field is
  drawn after each change, so there is no cursor before the first key, as we read the
  original. A name logs in: the port remembers it, whether `<NAME>.LOG` is in the game
  directory, and the campaign table from `scendef.bin`, and fades to the map room. It
  does not read the save (T-057) and never writes one (the original saves on N and Q
  from the map room; the port logs that it would). The original's goodbye, a thank-you line
  with the name in it, is printed when the window closes after a log-in. It is game
  text, so `xeno_formats::farewell` reads it from the user's `xeno.exe` (the three
  instructions above give its start, the name's place and the closing character); if
  it cannot, the game exits without it.
- **Labels.** The window title names the screen and its keys.
- **Mouse.** The pointer and buttons reach the screens through the log
  (`docs/game-loop.md`, "Input"), and the original's pointer
  (`crates/xeno-app/src/pointer.rs`) is drawn at it on every screen. Only the map room
  acts on clicks.
- **Map room** (`crates/xeno-app/src/maproom.rs`, T-042): as "The map room (T-042)"
  above. It draws over `maproom.m@l` the boxes (keyed: the look for each `state`, or
  unavailable above the progress), the highlight on the selection, the name, score
  (always 0 so far) and selection panels (the scenario's name, the label and its alert
  word, in the user's fonts 0 and 4, colour 0xe0) and the buttons (opaque, the next
  sprite while pressed, with the original's quirk after a release off the button). A
  new campaign (`maproom::Campaign`) starts at progress 1 with the locked scenarios at
  8; nothing is selected. Clicks act on the release: on the map area the left button
  selects the selectable box under the pointer and the right one also plays it; on a
  button, released over it, it runs the action of the first key whose handler is the
  button's (`MapRoom::button_keys`). Tab and the arrows step through the selectable
  scenarios. P, Enter, Space and the play button start the mission placeholder for the
  selection, remembered in `Screens::scenario` and logged with its `scenNNa.usg`, and
  do nothing without one; T is refused while a campaign mission has no result (never,
  so far). With `--screen map-room` and no log-in, a guest's new campaign is shown.
  Not done (T-046): the hover label, the campaign-won box, the developer keys, the
  busy pointer shape during loads, and any change to the campaign on play or after a
  mission.
- **Map room tables** (T-042): `xeno_formats::maproom` reads the map room's tables
  from the user's `xeno.exe`, each from the instruction that uses it after checking
  its bytes, with segments only from relocated words: the widget set (map area, and
  each button's rectangle and id from the state template), the buttons' sprites and
  handlers, the key table, the box looks by `state`, the unavailable and highlight
  handles, the three panels' sprites and rectangles, the alert label and the alert
  words (from the host's string segment). `xeno_formats::mgavideo` reads the pointer
  the driver sets up (shape and colour from the set-up's two calls, the shape from its
  data block). The fonts, colours and blit modes the drawing code passes are kept as
  constants in the port: they are short, and they are code, not tables. `xeno-tool
  inspect` shows the map room's and the pointer's shape.
- **Not modelled**: the save file, the widgets of the screens other than the map room,
  the real cutscenes (they need the sequence entries at `DS:0x606a`, `0x6016` and
  `0xedb0`), the 320x240 mode of the cutscenes (the placeholders are 320x200), and the
  music.
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

- The review screen's keys (T-044).
- How a mission ends (who sets `DS:0xe87e` and `DS:0x5fab`), whether it applies the
  same won and lost rules as the map room's developer keys, and how the player
  switches between the situation room and the consoles (T-043, T-046, T-056).
- Whether a scenario in play (12) can have its alert raised to 14 (T-046).
- What the keys `f3ef` and `f405` that every screen has do, and what `1000:0270`
  (presumably a beep) does.

T-042 named, in `xeno.exe`: `ui_draw_map_area` (`1000:7255`), `ui_map_area_on_mouse`
(`1000:72fa`), `ui_map_room_play_selected` (`1000:739c`), `ui_map_room_select_next`
(`1000:7431`), `ui_map_room_select_previous` (`1000:7445`), `ui_map_room_set_selection`
(`1000:74c4`), `ui_draw_map_room_button` (`1000:71fc`), `ui_map_room_button_on_mouse`
(`1000:721c`), `ui_draw_map_tooltip` (`1000:773c`), `scen_start_new_campaign`
(`1fe6:72a8`), `scen_update_campaign_progress` (`1fe6:7366`), `scen_hit_test_map_box`
(`1fe6:72f8`), `scen_index_scenario_table` (`1fe6:732d`), `ui_draw_map_room_status`
(`1fe6:7406`) and `ui_draw_selection_info` (`1fe6:749c`); in `mgavideo.dvr` the pointer
routines listed in `docs/re/dvr-protocol.md`. All `confirmed`, all saved.

T-041 named the name entry's routines (`ui_draw_name_entry`, `ui_name_entry_on_key`,
`ui_name_entry_type_char`, `ui_name_entry_backspace`, `ui_name_entry_accept`,
`ui_name_entry_cancel`) and the text services; the list is in
`docs/re/dvr-protocol.md`.
