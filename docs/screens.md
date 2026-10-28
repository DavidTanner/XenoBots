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
| R | 1st (8) | The review screen (`ui_run_review_screen`, `1000:796e`, nested: the map room comes back after it; below, "The review (T-044)") |
| N | 3rd (2) | Save `<NAME>.LOG` and log in again (`1000:7401`) |
| Q, Alt+Q | 5th (6) | Save and quit (`1000:741a`) |
| Right, Down, Tab | | Next selectable scenario (`ui_map_room_select_next`, `1000:7431`) |
| Left, Up, Alt+Tab | | Previous selectable scenario (`ui_map_room_select_previous`, `1000:7445`) |
| `=`, `-` | | Developer keys, only with unlock word 8 (`key.md`): progress up (`ui_map_room_raise_progress`, `1000:74e5`, to at most 16) or down (`ui_map_room_lower_progress`, `1000:7504`, to 0), then the campaign check |
| Ctrl+W, Ctrl+L | | Developer keys, the same word (`ui_map_room_dev_key_won`, `1000:752d`, and `_lost`, `75f3`): the selected scenario won, or the loaded one lost, by the same routines as a mission's end (below, "Campaign rules") |
| Ctrl+Space | | Run the mission at once (`ui_map_room_play_loaded`, `1000:73fa`, action 0), with no check and no load: whatever is loaded, the selection set to 12 |
| Alt+F1 | | Only with unlock word 9: toggles a flag nothing reads and prints `ON` or `OFF` on the debug screen (`ui_editor_key_toggle_flag`, `1000:6bcf`; below, "The scenario editor (T-048)"). It does **not** open an editor |

The ids index the button handler table (`DS:0x5f48`) and sprite table (`DS:0x5f52`).
ESC is in no map room table.

**Training menu** (`ui_run_training_menu`, `1000:6df6`): `TRAINMEN` fading in over 32
retraces, widget set `0x3842`, key table **`DS:0x5e9f`** (6 pairs): H, N, S and C load
`HTRAIN`, `NTRAIN`, `STRAIN` or `CTRAIN.USG` and run the mission; ESC and Alt+Q go
back; five buttons do the same. After a training mission it returns to the map room
with no cutscene. Details below, "The training menu".

**Mission** (`sim_run_mission_loop`, `1000:7e24`): runs the sim on the 60 Hz clock (at
most 10 ticks to catch up) until the quit flag `DS:0xe87e` is set. What it shows
depends on the object viewed: a table at `DS:0x6411` by the object's kind opens the
**humanoid**, **scorpion** or **scout console** (`HUMANCON`, `SCORPCON`, `SCOUTCON`)
or, for kind 3 (a base), the **situation room** (`SITROOM`, widget set `0x3822`). So the
situation room and the consoles are views inside a mission, not screens of their own;
the mission starts in the situation room. Details below, "The mission's views".

**After a mission** (`ui_map_room_run_mission`, `1000:6f9e`, the map room's action
0): sets `DS:0x5fab` to 0 and the selected scenario's `state` to 12 ("in play"), runs
the mission, fades out over 32 retraces, then plays `TRIUMPH` if the campaign is
complete (`DS:0x5fac`), else the lose sequence (outcome `DS:0x5fab` = 2, the picture
chosen by the robot console opened last, `DS:0x6012`) or the win sequence (1, its
caption showing the mission's kills, losses and points), and goes back to the map
room. The result itself is applied inside the mission, when it ends (below, "Campaign
rules"); a mission left with no result keeps 0. The cutscenes, their captions and the win caption's numbers are in `vcl.md`,
"Sequences" (T-045).

**Saving**: `<NAME>.LOG` is written when the player logs in again or quits from the map
room (`ui_map_room_save_if_loaded`, `1000:7423`, only if a game is loaded,
`DS:0x6005`), and read at log-in (`usg.md`, "Save files").

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
the table at `DS:0x3608`, draw handlers at `DS:0x3328`. There is no separate briefing
screen: the scenario's `ORDR` text (`usg.md`) is typed into the situation room's main
view in its orders mode (below, "The briefing (T-044)").

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
  `DS:0x60a4` and the alert word for its `state`, all at that corner; with nothing
  selected, a prompt from the string segment instead (`1fe6:74e5`): one before the
  campaign is won, another after (T-046);
- once the campaign is won (`DS:0x5fac`), the box at `DS:0x5f3c` = (68, 90, 112, 230)
  filled with colour 0xa0 (slot `0x51a`), framed in 0xe0 (slot `0x52a`), and a message
  from the string segment in font 4, colour 0xe0, transparent, from (left + 5, top + 4);
  its line breaks (0x0c) go back to that left edge (T-046).

The fill and frame slots (`mgavideo.dvr`'s `gfx_fill_rect`, 0x0e6e, and
`gfx_frame_rect`, 0x0fb8, T-046) take the same rectangle as the widgets: the fill
covers rows `top..bottom` and columns `left..right` with the bottom and right edges
out, and the frame is the one-pixel border just inside them (rows `top` and
`bottom - 1`, columns `left` and `right - 1`).

**The hover label** (`ui_draw_map_tooltip`, T-046): while the pointer is over a
scenario's box with no button held (selectable or not), a label above the box.
`ui_place_map_tooltip` (`1fe6:74fe`) sizes and places it from the hover word for the
record's `state` (the string segment, through the word table `DS:0x5f6a`):
- width `w` = 4 x (the word's length) + 3, height 9;
- bottom 3 rows above the box's `top` while `state` is at most 6, else at the box's
  `top` (so the label of a locked, won or in-play scenario covers 3 rows less of the
  map), top 9 rows above that;
- left = box `left` + 5 - `w / 2`, then at least 10 (a signed compare), then at most
  285 - `w`, or 265 - `w` when the label's top is at row 120 or below (clear of the
  buttons); right = left + `w`.

It saves the pixels under that rectangle (slots `0x50a`, `0x50e`), fills it with 0xa0
and frames it in 0x80 (as above), and types the word in font 2, colour 0xb6 on 0xa0,
**opaque**, from (left + 2, top + 2). `ui_restore_under_map_tooltip` (`1000:77b1`) puts
the pixels back (slot `0x512`) before the label is drawn again or goes. The map area's
mouse handler sets the hovered record (`DS:0x5f0f`, its `id`) on a move with no button
held, hit-testing every record, and marks the label for redrawing when it changes; a
move over no box clears it; leaving the map area (0x400) and a release that selects
nothing (a miss, or a record that cannot be selected) clear it and redraw the map area.
Moves with a button held change nothing. A won scenario's routine clears the saved-label
flag `DS:0x5f11` as the map area is redrawn anyway.

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
  the map area, a move over no box and a release that selects nothing clear it.

**Buttons.** A press (left or right) on a button captures the pointer, remembers the
button in `DS:0x5f0b` and flips `DS:0x5f0d` between 0 and 8, the offset added to the
remembered button's sprite handle; a release of the same mouse button over it flips it
again and runs the button's handler. A release elsewhere does nothing, so the button
stays drawn pressed until the next press flips the offset (a quirk of the original).

**Progress** `DS:0x5f9b`: a new campaign (`scen_start_new_campaign`, `1fe6:72a8`) sets
it to 1, the selection and loaded scenario to none, the score, win, loss and other
counters (`DS:0x5f9d`..`0x5fa7`) to 0, `DS:0x5fab` to 0xff, the campaign-won flag
`DS:0x5fac` to 0, the game-loaded flag `DS:0x6005` to 0, and the campaign's calendar
(`DS:0x7d4a`, a day, and `DS:0x7d4b`, a month index; the mission clock's) to day 0 of
month 3. Then `scen_update_campaign_progress` (`1fe6:7366`, also run after a save is
loaded and after each result) sets the campaign-won flag and stops when the win count
`DS:0x5f9f` equals the number of records; otherwise it walks the table in order:
- a record whose `unlock_level` is above the progress becomes 8, **whatever its
  `state`** (a won record too: only the developer key `-` can do that);
- else a won record (10) is skipped;
- else a record at 8 becomes 0, and the record (any other `state`, 12 included) counts
  as open: the lowest and highest open `unlock_level` (`DS:0x5ff7`, `0x5ff9`, their
  difference `DS:0x5ffb`) and the number open (`DS:0x5fff`).

**Campaign rules** (T-046). The won and lost routines are shared by the developer keys
and the mission's end: `sim_end_mission_with_result` (`1000:68c1`) runs, for an object
(`DI`) whose flags +0x16 have bit 0 set, the end-of-mission script that +0x30 counts
through (`1000:e691`, a jump table by +0x30 / 4: messages); once +0x30 reaches 0x24 it
calls `scen_win_selected_scenario` (`1000:7536`) if bit 3 of +0x16 is set, else
`scen_lose_loaded_scenario` (`1000:75fc`), and sets the quit flag `DS:0xe87e`. These are
the developer keys' handlers past their unlock check. It is entry 15 of the bases'
phase table (`DS:0x5aea`), so each base runs it every 16 ticks; a base destroyed by
damage (its `GONE` bit) steps the end script and its side bit picks the result (T-055,
`docs/sim.md`, "The mission's end"). Nothing else writes 1 or 2 to `DS:0x5fab`. Both routines first return at once if `DS:0x5fab` is 3 (the training
menu's value), so a training mission changes nothing.
- **Won** (`scen_win_selected_scenario`): nothing without a selection (`DS:0x5ff5`).
  The selected record becomes 10; the selection clears (`DS:0x5f97`, `0x5ff5`);
  `DS:0x5ef2` (not traced) and the saved-label flag `DS:0x5f11` are cleared and the map
  area and a button marked for redrawing; the win count `DS:0x5f9f` rises; `DS:0x5fab`
  = 1; `scen_score_mission_result` (`1000:7626`) sets the mission's points `DS:0x6014`
  (`vcl.md`, "The win caption"), the bonus at `1000:757d` (1000) is added to **those
  points**, and the points are added to the score `DS:0x5f9d` (16-bit, wrapping). Then
  the campaign check; then, if fewer than 3 records are open (`1000:758f`), the progress
  rises by one and the check runs again. Then, if the open levels differ
  (`DS:0x5ffb` not 0), for each record in table order whose `unlock_level` is within
  the progress and whose `state` is neither 10 nor 6, `sim_random_below`
  (`1000:772a`) draws a random word and the `state` rises by 2 if the word is below the
  chance at `DS:0x6094` indexed by `unlock_level - DS:0x5ff7`: 0x8000, 0x3000, 0x1000
  and then 0 (about 1/2, 3/16 and 1/16 for the lowest open level and the next two, never
  above that; the table's zeros run on to the alert label at `DS:0x60a4`, and the stock
  levels never index past them). Then `ui_map_room_select_first` (`1000:745b`) selects
  the first record within the progress and not won (with no check of `DS:0x5fab`), and
  `scen_advance_campaign_day` (`1000:7732`) adds a random 0 to 3 to the calendar's day.
- **Lost** (`scen_lose_loaded_scenario`): the campaign check; the loaded scenario is
  forgotten (`DS:0x5f99` = 0xffff, so play loads it again); `DS:0x5fab` = 2; the loss
  count `DS:0x5fa1` rises; the points are scored as above (no bonus) and added to the
  score; the calendar's day advances as above. The record's `state` stays 12 and the
  selection stays.
- **Alert past 6.** A record at 12 (lost earlier, or left with no result) is neither 10
  nor 6, so a later win can raise it to 14 (and on by 2). Then its box look, hover word
  and alert word are read past their tables (the hover word table's first word, the
  widget state block, the key table's first word as a string offset), it stays
  selectable while `DS:0x5fab` is not 0, and playing it sets 12 again.
- **Random word** (`sim_random_word`, `1000:0283`): four 16-bit words at `DS:0x1bf6`
  used in turn (the index `DS:0x1bf4` steps by 2 and wraps); each call shifts the next
  word right by one and, if a 1 fell out, XORs its high byte with 0xb4; it returns the
  new word less one (0 to 0xfffe), also kept at `DS:0x296`. `sim_seed_random_from_timer`
  (`1fe6:42e8`) seeds the four from the timer chip's counter (`1fe6:4301`, made odd), so
  the original is not repeatable.
- **Training gate** `DS:0x5fab`: 0xff for a new campaign, 0 from the start of a campaign
  mission until its result, 1 won, 2 lost, 3 once the training menu opens
  (`ui_map_room_open_training`, `1000:6ff8`, which also clears the selection and the
  loaded scenario). T opens the training menu whenever it is not 0, so only right after
  a campaign mission that ended with no result is training refused.
- **Play** (`ui_map_room_play_selected`): when the selection is already the loaded
  scenario (`DS:0x5f99`), nothing is loaded and the mission goes on as it was left (its
  objects and counts kept); otherwise its `scenNNa.usg` is loaded (a failure forgets the
  loaded scenario and plays nothing), `sim_apply_scenario_alert` (`1000:68a0`) adds a
  value from the table `DS:0x5a3f` by the record's `state` **before** it becomes 12 to a
  field of the mission's command object (`[DS:0x568e] + 0x2e`): 0, 50, 100 or 150 damage
  to the player's base, which is destroyed at 300 (T-058, `usg.md`), and
  `sim_reset_mission_counts`
  (`1000:68ae`) zeroes its kills and losses and sets the situation room's view mode to
  0. Then the game counts as loaded (`DS:0x6005`, so N and Q save) and action 0 runs.
- **Developer keys**: `=` raises the progress by one unless it is 16 (`1000:74ee`); `-`
  lowers it unless it is 0; both run the check, so `-` can lock won records and `=`
  then reopens them at 0. Ctrl+W wins the selected scenario and Ctrl+L loses the loaded
  one, as above, outside any mission and with no cutscene. Alt+F1 only toggles an
  unused flag (`dbg_toggle_unused_editor_flag`, `1000:4856`; T-048).

`xeno_formats::campaign` reads the bonus, the open limit, the progress cap, the chances,
the two unlock values, the hover words, the campaign-won box and message and the two
prompts from the user's `xeno.exe`, from the instructions that use them after checking
their bytes; the colours, fonts, the label's geometry and the random word's taps are
code constants.

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

**Shape and visibility (T-049).** The driver keeps the shape in `DS:0x14b6` and a
hide counter in `DS:0x14d8` (both in the host's data segment):
- `0x1496` (`gfx_set_pointer_shape`): a number from 8 on becomes 0; the old shape
  comes back in `AL`. It only stores the shape and its size and hot spot; the next
  retrace redraws the pointer (the retrace handler calls `0x14b2` after the palette
  upload), so a shape set before a long load is on screen for the whole load.
- `0x149e` (`gfx_hide_pointer`) adds one to the counter and, on its way from 0xff to
  0, puts back the pixels under the pointer. `0x14a2` (`gfx_show_pointer`) draws it
  if the counter is 0 and takes one away. The pointer is drawn only while the counter
  is 0xff, so hides nest, and a show with nothing hidden hides it until a hide.
  The counter starts at 0: hidden.
- `0x14a6` / `0x14aa` (`gfx_hide_pointer_over_rect`, `gfx_clear_pointer_rect`): a
  rectangle, grown by the pointer's size, that the pointer is not drawn in until it
  is cleared. `vga240.mod` wraps its drawing in them. The driver's own copies and
  blitters hide it the same way while they draw.
- `0x14ae` (`gfx_post_mouse_events`) is the mouse driver's callback; it posts the
  move and button bits through host slot `0x324`.

Every caller in `xeno.exe`, read from Ghidra's listing of both code segments:
- **Start-up.** The entry point shows it once the drivers are loaded;
  `ui_run_startup_and_game` hides it again before the intro and sets colour 12 after
  the loads. So the intro, the drop and the **first log-in have no pointer**. The map
  room's loop (`ui_run_map_room_loop`, `1000:6f37`) shows it after the first log-in.
  The map room's N calls the log-in from inside the loop, so a second log-in shows the
  pointer. Nothing hides it again but the cutscenes.
- **Cutscenes after a mission.** `ui_play_lose_cutscene`, `ui_play_win_cutscene` and
  `ui_play_campaign_won_cutscene` hide it before the song and show it after the
  wait. `vga240.mod`'s enter routine hides it again and its leave routine shows it.
- **Picture loads (shape 1, busy).** Each of these sets shape 1, loads the full-screen
  picture and draws it, then sets shape 0: `ui_load_map_room_picture` (`1000:71d3`,
  `MAPROOM`), `ui_load_training_picture` (`1000:6e7f`), `ui_load_sitroom_picture`
  (`1000:53fa`), `ui_load_scorpion_console_picture` (`1000:7fb3`),
  `ui_load_humanoid_console_picture` (`1000:9cb7`), `ui_load_scout_console_picture`
  (`1000:b473`) and `ui_load_review_picture` (`1fe6:782c`). The busy shape is seen
  only where the old picture is still on screen with its palette: the map room's R
  (the review is shown at once), the way back from the review, and a mission's view
  switches. After a fade out the palette is black, so it cannot be seen.
  `gfx_load_all_models` (`1000:7b55`) also shows shape 1 and puts the old shape back,
  but the pointer is hidden then.
- **Corners.** A mission view's corner (`ui_sitroom_corner_on_mouse`, `1000:5c30`, and
  `ui_console_corner_on_mouse`, `1000:7cbf`) picks shape 2 when the dispatcher says
  the pointer entered it (0x200) and shape 0 when it left (0x400).
- **The views' regions (T-106).** The mouse dispatcher (`1000:2509`) hands a move to
  one widget: the last in the set whose rectangle holds it (a group, type 4, is
  searched inside), through the mouse handler table `DS:0x3608` by type; a widget
  that loses the pointer gets 0x400. Four kinds of handler set a shape on each move
  (0x1) by testing the point against a colour region with slot `0x5bc` (`mgavideo.dvr`
  0x226e: the screen offset of the point against the region's run list; carry when
  inside), and set 0 when it is outside or on 0x400:
  - The situation room's main view (type 0x50, `1000:571f`, region `DS:0x5680`, its
    colour-255 pixels): the word at `DS:0x5a14 + mode`, the mode being the base's
    +0x37 (0, 2, 4, 6, 8). The stock table gives 3 in modes M and R and 0 in O, S
    and C. `DS:0x594c` remembers whether it is over the region.
  - A console's 3D view (colour 255; humanoid type 0x84 `1000:9d83` region
    `DS:0x6832`, scorpion 0xbc `8081` `0x6540`, scout 0x106 `b540` `0x6bbc`): 3.
  - Its second display (colour 254; 0x86 `9eea` `0x683e`, 0xbe `828e` `0x654c`, 0x108
    `b68c` `0x6bc8`): 4, or 6 when the display mode (+0xef, bit 2 set while damaged)
    is exactly 2, the map, and the event's word at +2 has 0x2000. The posting
    routine (`1fe6:357c`) stores there the keyboard's last key word (`DS:0x1bf0`)
    with the mouse buttons ORed into its high byte, so 0x2000 is presumably Alt, as
    in the key words; the writer of the high byte, `DS:0x1bf1`, is not traced.
  - Its third display (colour 253; 0xaa `9fcc` `0x684a`, 0xd8 `8370` `0x6558`, 0x126
    `b76e` `0x6bd4`): 4.

  The consoles' handlers keep which region the pointer is over in `DS:0x6a7e`
  (humanoid; 1 the 3D view, 2 the second display, 3 the third), scorpion `0x65e9`,
  scout `0x6e16`; their clicks are T-133. All three kinds use the same shapes. On the
  stock pictures the last widget over a region is its own handler's for all but a few
  dozen pixels at the regions' edges (row 107 of the humanoid's second display, rows
  47 to 49 of the scorpion's 3D view), where another widget takes the move.
- **Elsewhere.** `ui_run_file_dialog` (`1fe6:83db`) shows shape 0 while a file dialog runs and puts
  the old one back (the editor and saves, T-048). Pixel and text drawing straight to
  the screen (`1000:376c`, `40a0`–`41b2`, `1fe6:4d54`) and a screenshot routine
  (`1fe6:4ce9`) hide and show it around their writes. The terrain editor in
  `mapdvr.dvr` sets shapes and hides it too (T-048).

### The mission's views (T-043)

From a Ghidra pass over the view switch, the four widget sets, their key tables and
the handlers that choose views and orders. Rectangles are (top, left, bottom, right),
the bottom and right edges outside, as in the map room.

**Which view.** `sim_run_mission_loop` starts on the player's base (`DS:0x568e`; the
alien base is `DS:0x5690`) and `ui_open_view_of_object` (`1000:7ef2`) opens a view for
any object: it remembers the object in `DS:0x62f1` and calls the opener for its kind
from the word table `DS:0x6411`: kind 0 the humanoid console
(`ui_open_humanoid_console`, `1000:9c94`), 1 the scorpion's (`1000:7f90`), 2 the
scout's (`1000:b450`) and 3, a **base**, the situation room (`1000:5457`). So the
situation room is the view of the base, and a console the view of one robot. Each
opener opens its widget set with the **state block inside the viewed object**:

| View | Widget set | Widgets | State in the object | Picture (file id) | Key table |
|---|---|---|---|---|---|
| Situation room | `0x3822` | 30 | +0xbd, 0x43 bytes | `SITROOM` (0x04) | `DS:0x5b20`, 32 pairs |
| Humanoid console | `0x388b` | 27 | +0x10c, 0x35 bytes | `HUMANCON` (0x0e) | `DS:0x6b07`, 33 |
| Scorpion console | `0x3863` | 37 | +0x127, 0x45 bytes | `SCORPCON` (0x0c) | `DS:0x676a`, 38 |
| Scout console | `0x38aa` | 39 | +0x10d, 0x3c bytes | `SCOUTCON` (0x10) | `DS:0x6efa`, 40 |

So each robot and base remembers its own view's state (pressed buttons, modes,
scroll), and it is saved with the object in a `.usg` file (`usg.md`, `DCBS`).

**Switching views** (all through `ui_open_view_of_object`):
- From a console, ESC and Alt+Q (`1000:7ee7`), and a release on the 2 by 2 hot spot in
  the top-left corner (type `0x158`, which shows pointer shape 2 while the pointer is
  over it), go to the robot's side's base: the situation room.
- Tab and Alt+Tab (`ui_view_next_unit` / `ui_view_previous_unit`, `1000:7db2` /
  `7dd9`), and a left or right release on the unit-name button (type `0x156`, the
  bottom-right corner, (185, 273, 200, 318)), step through the side's list of 38 unit
  slots (`DS:0x62f5` human, `DS:0x6341` alien), wrapping and skipping empty slots and
  units in the base (object +0xfd bit 2).
- A right click on a robot in a console's scanner (`1000:7eac`) opens that robot's
  console, if it is the player's (otherwise it refuses with a message).
- In the situation room, Space, J and the third button below the roster
  (`1000:6852`) open the console of the robot selected in the roster; a right release
  on a roster row does the same (`ui_sitroom_roster_on_mouse`, `1000:582d`).

**Widget sets.** Every view starts with the same first widgets: type `0x0e` (no area),
its picture (the whole screen), a corner hot spot (`0x158` in the consoles, `0x15a`
in the situation room), the developer clock keys (`0x146`: Ctrl+Alt+H, D, M add an
hour, a day, five minutes), the screenshot key (`0x148`) and the view's key table. The
pictures are loaded with slot `0x490` and shown through screen regions: each opener
builds a run-length region of the pixels of colour 255 (and, in the consoles, 254 and
253) with slot `0x59c`, and the picture is copied around them with slot `0x5b8`; the
3D view and the scanner are drawn into those regions. That is what the pure-green
colour 255 of `m@l.md` is.

*The situation room* (sprite bank `msprite1` loaded at handle 0x2a0 by the picture's
draw handler, which also starts colour records 0, 1, 16 and 17):

| Type | Rectangle | What |
|---|---|---|
| `0x50` | (26, 91, 130, 292) | The main view, colour 255: one of five modes by the base's +0x37, chosen with O (0), S (2), M (4), R (6) and C (8); mode 8 is the units' message log (time and message per unit), modes 4 and 6 look through the selected unit (mode 4 with the map controls below) |
| `0x52` | (14, 2, 70, 78) | The roster: five rows (rectangles from the table at `DS:0x5957`) of units from the base's list at +0x42, scrolled by +0x4c. A left press selects the row's unit (+0x3e), a right release opens its console |
| `0x54`, `0x56` | (73, 13, 81, 20), (73, 21, 81, 28) | Sprite buttons: scroll the roster up and down |
| `0x58` | (73, 29, 81, 69) | Sprite button: open the selected unit's console |
| `0x7e`, `0x60`, `0x62`, `0x7c` | (84, 17, 89, 65), (107, 16, 145, 83), (123, 162, 177, 240), (152, 103, 157, 143) | The clock (`1000:61a5`), the status counts (`5979`: the base's +0x20 to +0x27 as two digits each, font 2, colour 0x1d), the selected unit's detail (`5a0e`) and the reserve bar (`614e`, +0x38); T-127 |
| `0x72`, `0x76`-`0x7a`, `0x74` | three rows at 159, 167, 175 from x 103 to 146 | The power rows (NET, REP, CON): left arrow (sprite 0x3f8 plus the pressed look), bar (`6047`, `60a0`, `60f7`: the base's +0x39, +0x3a, +0x3b over 8), right arrow (0x408); while an arrow is held the base's +0x2d is 1, 3 or 5 (left) or 2, 4 or 6 (right) for the row; T-127 |
| `0x04` x2 | (89, 284, 131, 318), (142, 254, 182, 306) | Groups: their members are the widgets after the list, up to a zero type (the group's state names its first and last); the map controls and the mode buttons (below, "The roster, map controls and mode buttons (T-103)") |
| `0x5a`, `0x5c`, `0x5e` | (170, 29, 178, 66), 8 rows lower each | Sprite buttons: launch the selected unit from the base (`1000:6716`, only for a unit in the base: it is put at the base's position and marked in the field), recall it (sets its +0x23 bit 3), and end the mission (the quit flag `DS:0xe87e`) |

A sprite button is drawn opaque with handle `base + [+0x40]`: a press or release on
any of them flips the shared +0x40 between 0 and 8, so, as in the map room, all of
them show their pressed sprite (the next one) together. Their handles, in order, are
0x2a0, 0x2b0, 0x2c0, 0x3f8 and 0x408 (the arrows), 0x2d0, 0x2e0, 0x2f0. The release
acts. The corner hot spot (`0x15a`) ends the mission too.

Situation room keys (`DS:0x5b20`): Alt+Q ends the mission (ESC is listed but its
handler only acts on Alt+Q); Space and J view the selected unit; Tab and Alt+Tab step
the roster selection; O, S, M, R, C the main view's mode; F3 recalls the selected
unit (`1000:675e`: +0x23 bit 3; `docs/sim.md`, "The cockpit keys"); F2 launches it from the base (`6704`; T-135); = and - (and keypad + and -) zoom the map; 1 to 6, B, G, I, N, U and F1, F2 act
on the mission (not traced); Ctrl+Alt+P, S and F are developer keys.

*The consoles* share most of their layout but not their positions. All three have:

| Type | Humanoid | Scorpion | Scout | What |
|---|---|---|---|---|
| `0x154` | (91, 126, 98, 194) | (154, 4, 161, 56) | (112, 18, 119, 59) | **The program panel**: the robot's program, see "Orders" |
| `0x156` | (185, 273, 200, 318) | the same | the same | The unit-name button (`1000:7d55`): the robot's call sign (`1fe6:7fd5`, as in the roster) in font 0, colour 0, at (279, 191), transparent; left or right release: next or previous unit (pressed look: slot `0x536`, the rectangle's frame XORed with 8) |
| `0x152` | (7, 7, 36, 102) | (20, 112, 51, 208) | (5, 5, 34, 100) | The robot's message panel (object +0x5e; below, "The message panel (T-104)") |
| `0x158` | (0, 0, 2, 2) | the same | the same | Back to the situation room |

The rest are the robot's instruments, weapons, cargo and scanner, drawn from the
running simulation: the 3D view (colour 255) and scanner (colour 254 or 253) regions,
the scanner (humanoid `0x86`, (108, 110, 158, 207): a left click locks the target
under the pointer, a right click opens that robot's console), damage, speed and
weapon panels. Their handlers live in `1000:7f90`-`9c94` (scorpion),
`9c94`-`b450` (humanoid) and `b450`-`cbff` (scout) and are T-056's.

Console keys (the three tables share most entries): F2 the robot's program
(automatic), F3 its pilot (manual), F4 automatic and the next program (see "Orders");
ESC and Alt+Q the situation room; Tab and Alt+Tab the next and previous unit; F1, F5
to F8 (and F9 on the humanoid), Space, `.`, A, R and the arrows, the number keys 1 to
4 (mines and buoys), `=` and `-`, and per robot letters drive and fight (`docs/sim.md`,
"The cockpit keys (T-105)"); Ctrl+P and Ctrl+Alt+S are developer keys.

### The message panel (T-104)

Every robot has a panel of text (object +0x52 to +0xda, `usg::Robot::panel` and the
fields after it) whether or not its console is open.

**Messages.** `1000:d2a2` puts a message (a string-segment offset) in the first free
of the four words at +0x54 (a fifth, +0x5c, is only the queue's end); with none free
it is dropped. The scripts' opcode 0x74 does it for the robot itself (for a base,
`1000:d53f` writes the base's log instead); 0x75 writes the side's base log
(`1000:65a6`: the sender's name from `1fe6:7f38`, the clock from `DS:0x7d46`, the text;
the oldest of seven lines dropped when full; +0xb4 set, the "new message" mode button);
0x76 writes the log and then queues, for every robot of the sender's side, a header of
the sender's name between `-` and `:` with bit 7 set on every byte (so drawn in the high
colour), built in the string segment at 0x1714, and then the text. No stock script uses
0x76. Messages are also queued by `1000:8d8c`, `9b28`, `9b82` and `d801`, and through
`d53f` and `65a6` by other routines (hits, the bases); those are T-129.

**Typing** (`1fe6:826d`, every tick right after the robot moves, all three kinds): with
a message waiting, one byte of it from the string segment (`ES` = the word at
`DS:0x6fec`, the same segment as `DS:0x60`):
- If +0x52 bit 6 is set (a line ended), a new line starts first (`1fe6:8142`): the bit
  is cleared and, with fewer than five lines counted (+0x52 bits 0 to 3), the count goes
  up by one (the whole byte is written, so bit 7 goes too); at five, the first line is
  dropped: bit 7 is set, the length (+0x53) loses the first line's bytes up to and with
  its 0x0c, and 100 bytes from after it are copied to +0x5e. So the panel holds six
  lines.
- A NUL ends the message: a 0x0c (the font's new line) and a NUL are written at the
  length (one byte back if the length is 0x7c or more), the length moves past the
  0x0c, bit 6 is set and the queue moves up a word, +0x5c becoming 0.
- A CR or 0x0c is written as 0x0c and sets bit 6; any other byte is written as it is,
  bit 7 included; a NUL follows it and the length moves on.

Whenever a byte was typed or a message ended, the routine returns with carry and the
kind's tick (`1000:b2b7` humanoid, `997d` scorpion, `cac0` scout) sets the panel
widget's state to 3, so the open console redraws it on the next two draw passes: the
text appears a byte a tick.

**Drawing.** A console's opener calls `1000:d1f3`: the panel's text state is the block
at `DS:0x7188` (selected with slot `0x488`), font 2, the colours from the word table
`DS:0x71b0` by kind (the high byte the text's colour, the low byte the fill and the
text's background), the high colour (codes with bit 7) 0xff, the typed count `DS:0x71ae`
0 and +0x52 bit 7 set. The panel's draw handler (`1000:d241`) fills the rectangle in the
fill colour when bit 7 is set (clearing it) and moves the cursor to the rectangle's
top-left corner, then types the text from `DS:0x71ae` to its NUL through slot `0x594`
(transparent) and remembers where it stopped. So the panel always shows the whole
text. The port fills the rectangle and types the whole text every frame
(`xeno-app`'s `mission`), which gives the same pixels. Font 2's six lines run a few
rows below the rectangle in the original too.

### The second display (T-104)

Each console has a second display in its colour-254 region (built with slot `0x59c`
into `DS:0x683e` for the humanoid, `0x654c` for the scorpion, `0x6bc8` for the scout;
the colour-253 region, `0x684a`, `0x6558`, `0x6bd4`, is a third display of each kind's
own, T-130 to T-132). Its draw handler (humanoid `0x86`, `1000:9df2`; scorpion `0xbe`,
`80ee`; scout `0x108`, `b5ad`) starts a frame (`DS:0x1450` = 2) with the map's clear
and hill colours of its kind (`DS:0x1457`, `0x1458`) and `1000:dbe8` (the scorpion sets
the same up itself, then calls through `DS:0x6722` by mode):

- The view mode is `DS:0x70ae[+0xef]`: display modes 0, 1, 2, 3 give view modes 6
  (the camera, reusing the main view's render list), 0xa (the same, then slot `0x5f0`
  remaps the buffer through a 256-byte table: thermal), 0xc (the map) and 8 (the
  scorpion's missile camera); modes 4 to 7 (bit 2, the device damaged) give 0x18, which
  is no 3D view (the scorpion's damaged modes go to `1000:81d0`). A map frame is drawn
  only every few passes (`DS:0x62ee`).
- The camera: the robot's position and eye height as the main view's, a zoom shift of 1
  (`DS:0x1454`, halving the focal factor), and in the camera modes the heading plus the
  pan (+0xf4) at a zoom of `1 << +0xf1`; in the map mode heading 0, a zoom of
  `1 << +0xf2` and the map's objects filter `DS:0x62ea` = 0x0f (robots as sprites, mines,
  relays; no grid). The 3D modes set the clear colour to 0x20.
- When the frame is done: the overlay of the mode (`DS:0x6abf` for the humanoid: the
  target's name blinking in modes 0 and 1 while +0x17 bit 6, the zoom's name from
  `DS:0x70e7` or `0x7105` while +0xf3 is not 0), the copy through the region, the
  pointer probe into the console's own word (`DS:0x6a7a` humanoid), the colour records
  of the mode (`1000:d9ca`: `DS:0x70b6[+0xef]`, started with `1000:140b` after stopping
  the last), and the noise check below.

`1000:db5c` (every tick, in each robot's tick, and when the mode is set) clears +0xef bit
2 and sets it again when `DS:0x70be[+0xef]` (the camera, thermal camera and map bits of
+0xf6; none for the missile camera) is damaged; it leaves a mode of 7 alone.

**Keys.** V, T and M (each kind's table: humanoid `1000:b415`, `b41b`, `b421`; scorpion
`9c5a`, `9c60`, `9c66`; scout `cbed`, `cbf3`, `cbf9`) set +0xef to 0, 1 or 2 (through
`b3e9`, `99c7`, `cbc1`): the mode buttons and their widgets are marked for a redraw,
+0xf3 = 6, `db5c` runs and sound event 10 plays, whatever the mode was. = and keypad +
(`1000:da0e`), - and keypad - (`da37`) step +0xf2 in the map mode (exactly 2) and +0xf1
otherwise, up to 4 or down to 0 (a value out of range steps on and wraps), set +0xf3 = 6
and clear +0x17 bit 6. Each console's first widget group also holds the mode buttons
(humanoid `0x8e`, `0x90`, `0x92`: sprite 0x4e0 and on, the next one for the current mode;
a release sets the mode) and the pan buttons (`0x88`: held, +0x1b = 1), T-134.

**Noise** (`1000:daa2`, after each frame): when the mode byte (`DS:0x70ac`, bit 2
included) differs from the last (`DS:0x70ad`), it is kept, and event 28 plays if bit 2
is set; otherwise `1000:eeb1` gives the event's voice silence, queued, so the noise
finishes. Leaving the console (`1000:dac0`) does the same and sets `DS:0x70ad` to 0.

**Under the pointer** (the mouse handlers `1000:9d83` for the main view and `9eea` for
the second display, the others alike): a move over the region shows pointer shape 3 (4
over the second display; 6 there in the map mode with the key flag 0x2000) and marks
the view; a left release with the probe's object (`1000:41b7`, read after the frame)
locks it as the target (`1000:d8a7`); a right release opens that object's console
(`1000:7eac`). In the map mode with 0x2000 a left press sets +0x23 bit 2, which the next
frame acts on (`1000:466b`, presumably moving the robot's destination; T-133).

**In the port** (`xeno-app`'s `mission`): each robot's display (mode, zoom steps and pan)
starts as its file has it and changes with V, T, M, =, -, keypad + and -; the camera
mode draws the 3D view from `scene::second_camera`, the map mode the situation room's
map (`mainview::draw_map` without its zoom label) in the kind's colours from
`mainview::console_map`; the thermal and missile camera modes and a damaged device keep
a "T-134" placeholder. Event 10 plays on a mode key; event 28 and its silence follow the
noise check after each retrace and on leaving the console. Not ported (T-133, T-134): the
overlays, the colour records, the remap, the static, the frame-skipping of the map, the
target lock and the right click, the buttons and the zoom's name.

### The situation room's main view (T-103)

From a Ghidra pass over the main view's routines (`ui_draw_sitroom_main_view`,
`1000:5551`, which calls the mode's routine from the word table `DS:0x5ae0` by the
base's +0x37 and then clears bit 14 of +0x32). The base's fields: +0x32 the map's
overlay bits, +0x36 its zoom (0 to 6), +0x37 the mode, +0x3c the selected unit, +0x4f
and +0x53 the map's centre (16.16), +0x59 the message log (`usg::Base`). Modes M and R
draw a 3D-view frame into the view's buffer (`DS:0x1470`, the colour-255 region of
`DS:0x5680`, as a console does) and copy it through the region when it is ready (at
most every 4 ticks for the map); modes O, S and C draw on the screen.

| Mode | Key | Routine | What it draws |
|---|---|---|---|
| 0 | O | `1fe6:6fc1` | The briefing (T-044) |
| 2 | S | `1fe6:704a` | The selected unit's status: no time-of-day tint; on a redraw (+0x32 bit 14) the view cleared to 0, the kind's picture (slot `0x5d8`, handles from `DS:0x5694`/`0x569a`) and `1fe6:6b4b`; then its name at (0x70, 0x7b) and the kind's routine from `DS:0x5858`. `NO UNITS` (string segment 0x80d) without one. Not traced further (T-126) |
| 4 | M | `1000:5590`, frame `1fe6:6c05` | The map (below) |
| 6 | R | `1000:55d6`, frame `1fe6:6b84` | The selected unit's own 3D view: the console's camera (`1000:dbe8`'s: eye height, zoom `1 << +0xf1`, heading plus the pan +0xf4), view mode `DS:0x59b5[+0xef]` (4, 8, 4, 4 for the console's display modes; 0x18 when mode 4 and its camera is damaged, +0xf6 bit 0). Over it (`1000:5630`) its heading in degrees in font 0 at (0x0c, 0x60) of the buffer, three digits (`1000:02b7` prints five) and code 0x17; a unit at its base (+0xfd bit 2) gets the buffer cleared to 0 and the string-segment text at 0x855 at (0x30, 0x28) instead. No unit: the view cleared to 0 |
| 8 | C | `1000:56a7` | The log: the view cleared to 0x10; font 0, colours 0x14 / 0xff, `UNIT TIME MESSAGE` (`DS:0x5a83`) at (0x60, 0x1e) and a rule in 0x14 (slot `0x558`, `DS:0x5a95`: (36, 95) to (36, 288)); then (`1fe6:6d02`) for each of the base's seven 13-byte messages up to the first with no text: in font 2, colour 0x14 on 0x10, the unit's name at (0x60, y) and its time (`DS:0x5a9d`, `"  00:00  "`, hours and minutes from bytes 7 and 8), then the text (a string-segment offset at byte 11) in 0x13 at (0x9c, y); y from 0x28, 12 a row |

Texts go through slot `0x594` (transparent). The map's and mode R's labels set no
colour: they use whatever the last text left in the text state (`DS:0x644` +0x16).

**The map** (`1fe6:6c05`): view mode 0xc, or 0xe with the GRID overlay (+0x32 bit 4,
step `1000:3760`, not traced; not drawn by the port yet); clear colour 0x43
(`DS:0x1457`), hill colour 0x42 (`DS:0x1458`); zoom `1 << +0x36`; heading 0; the
camera on the centre +0x4f/+0x53 while +0x32 bit 15 is set (a left press on the map
off a unit sets it and moves the centre to the pointer, `1000:57d2`; every stock base
has it set), else on the selected unit unless it is at its base, else on the base. The
steps (`DS:0x395a`): the camera with the focal factor `DS:0x4d32` (327, exponent -16),
the clear, the pointer probe and `1000:3877`, which asks the producers for one node at
a time (no sorting) and draws each through `DS:0x39ae` by type, with the sprite anchor
centred (slot `0x5c0`). A position becomes a pixel by `1000:3eb9`: the offset from the
camera turned as the 3D view turns it (`1000:12b9`), each 48-bit sum to a float by
whole bytes (`1000:0ced`), times 2^-24 and the focal factor, to an integer;
`1000:3eb9` returns `X` in `CX` and `Z` in `AX`, and the column is `W/2 + X`, the row
`H/2 - Z` (`1000:3ad8`), so at heading 0 x runs right and y up, about 1.28 pixels a
unit at zoom 0 (T-103 had the two swapped; T-108 found it from the pointer's
conversion below, which puts columns on x and rows on y). The zoom label (`1fe6:6cb9`): `DS:0x5ada` (a space, three
scale-bar glyphs and a new line) then the zoom's name (`DS:0x7140`: `3.2 K` to `50 M`)
in font 0 at (8, 0x5a) of the buffer.

The nodes, in order:
- **Terrain** (type 8, `1000:39fb`): only records whose program starts with op 0x14,
  as type 0x12 (`1000:3a27`): an octagon filled in the hill colour by the polygon fill
  (`1fe6:4d7d`, the 3D view's), corners at the radius (+0x10, 8.8) on the axes and two
  thirds of it on the diagonals, each length scaled as an offset (`(len, len)`'s `Z`).
- The editor's path points (type 0xe lines, `1000:6d27`, with the path display; not in
  the port yet).
- **The selected robot's goal** (`1000:f914`, `4714`): with its +0x32 bit 1, sprite
  0x198 at its destination (+0x3c); with bit 0, sprite 0x1a0 on its target object
  (+0x3a).
- **The objects** in slot order by kind (`DS:0xe7fa`; type 0xc a pixel, type 0xa a
  sprite from `msprite0` then a pixel of the node's colour unless 0):

| Kind | Routine | Shown |
|---|---|---|
| Robots 0 to 2 (10 after `1000:e5fe`) | `1000:f976` | Not alien and undetected (+0x17 bit 2, unless the editor's Ctrl+E), not +0x17 bit 0, and only with overlay bit 0. Without bit 1 a pixel in the model's colour (`DS:0xe86c`: 0x30, 0x0e, 0x0d); with it a sprite, the eight headings a handle step apart from `DS:0xe866` (zoom factor under 8: 0xc0, 0x100, 0x140) or `DS:0xe860` (0, 0x40, 0x80), octant `(heading + 0x1000) >> 13`, with a 0xfd pixel for an alien |
| Base 3 | `f9e5` | Always: sprite 0x1b8; with bit 1, 0x1a8 (human) or 0x1b0 (alien) |
| Relay 4 | `fa04` | With overlay bit 3: a pixel 0xfe (0xfd alien), 7 if not linked (+0x16 bit 4); with bit 1 sprite 0x188 (0x190 alien), 0x180 if not linked |
| Shots 5 to 8 | `fa4d` | Only with the developer's Ctrl+Alt+S (`DS:0x62ec` bit 6): a 0x0f pixel |
| 9, 12 / 11 | `fa6a` / `fa5c` | A 0x0f pixel / 0x0f, or 0xff with +0x16 bit 4 |
| Obstacle 16 | `fa47` | A pixel in colour 1 |
| 13 to 15 | | Nothing |

The pointer over the main view takes shape `DS:0x5a14[mode]` (3 in M and R; T-106).

### The roster, map controls and mode buttons (T-103)

**The roster** (`1000:57e3`): font 0 with an x step of 3 (`1fe6:4908`), then five rows
from the base's +0x42 (`1fe6:6d87`), each 11 pixels below the last. A unit's row: its
call sign at (0x18, 0x12), opaque (slot `0x598`), colour 0x0f on 0x42 or 0x0c for the
selected unit (+0x3c); its status icon (`DS:0x5991` by +0xf9 bits 4 to 6: 0x430 to
0x468) at (5, 0x10) and its kind icon (`DS:0x59a9` in the field, `0x59af` at its base,
+0xfd bit 2: 0x480 to 0x4a8) at (0x0e, 0x10), both keyed. An empty row: eight spaces
(`DS:0x5a4f`) at x 6 and sprite 0x478 at both icons. A call sign (`1fe6:7fd5`) is six
bytes from the string segment at the word `DS:0x7245[+0xdb]`; a new robot takes a free
index 0 to 63 at random (`1000:dfaf`, the used flags at `DS:0x72c5`) and gives it back
when destroyed.

**The map controls** (the first group: `1000:5c59` to `5f4b`), live only in mode M; in
other modes each shows its idle sprite and ignores the mouse. All draw opaque a pixel in
from their corner:

| Type | Place | Sprite (off, on / idle) | Mouse and keys |
|---|---|---|---|
| `0x64` | (91, 288) | 0x300 plus the pressed look / 0x310 | A press or release flips the shared look (+0x40); a left release zooms in (+0x36 up to 6), a right release out (down to 0). `=` and keypad + (`1000:5cc7`), `-` and keypad - (`5cd7`) |
| `0x68` | (91, 303) | 0x318, 0x320 / 0x328 | Icons (+0x32 bit 1): a release, left or right (after `1000:dfd5`), flips it; key I |
| `0x66` | (99, 288) | 0x330, 0x338 / 0x340 | UNITS (bit 0); key U |
| `0x6a` | (107, 288) | 0x348, 0x350 / 0x358 | MINES (bit 2); key B |
| `0x6c` | (115, 288) | 0x360, 0x368 / 0x370 | NET (bit 3, the relays); key N |
| `0x6e` | (123, 288) | 0x378, 0x380 / 0x388 | GRID (bit 4); key G |

**The mode buttons** (the second group, five type `0x70`, `1000:5efd`, `5f4b`): each
one's mode is its state's second byte (0, 2, 4, 6, 8 top to bottom in every stock
base). Its label `DS:0x5a0a[mode]` (0x3a0 to 0x3e0, the next sprite for the current
mode; 0x3f0 for C while the base's +0xb4 says a message is new) at (+0x0e, +2), then
the frame 0x390 plus the pressed look at its corner, both opaque. A press or release
flips the look; a left release sets the mode (`ui_sitroom_set_view_mode`).

### Orders (T-043)

A robot's **orders** are a program, one of 31 scripts in `xeno.exe`, and a switch
between the program (automatic) and the pilot (manual). There is no other order
parameter in the views: no waypoint or target is given in a console or the situation
room (the scripts that go to a unit or a place use the robot's destination and target
fields, set by the mission; the scripts are traced in `docs/sim.md`, "Orders").

**The program table** (`DS:0x4ff4`, 31 word offsets in the data segment, then 0xffff)
points at the scripts. A script starts with a header: either a list of three-byte
items (an opcode and a word) up to the name item, introduced by a byte 0x0f, or the
name item alone. The name item is opcode 0x11, a byte, the **flags** and a length,
then the name (NUL-terminated). `order_find_program_name` (`1000:47ca`) finds a
program's name and flags; numbers past the table show the last program. The flags'
bits 0 to 2 say which robot kinds may be given the program, through the kind masks at
`DS:0x7183` (1 humanoid, 2 scorpion, 4 scout), and bit 7 marks a program the robot
only enters by itself (states such as sentrying or retreating, the faults, and the
"no program" entry).

| Programs | Offered to |
|---|---|
| 1 to 10 (hold, the two sentry orders, return to base, go to a unit, the two shadow orders, patrol, engage, go to a place) | Every robot kind |
| 12 to 14 (the three relay-net orders) | The scorpion only (it carries the relays) |
| 0, 11, 15 to 30 (`SENTRY`ING and the other states, `FIRE ALL`, `ZIGZAG`, `DETECT`, ...) | Nobody: bit 7 |

The words in brackets are ours; the names shown are game text, read from the user's
`xeno.exe`.

**Where they are stored**, in the robot's object (and so in a `.usg` file's `DCBS`
record):

| Offset | What |
|---|---|
| +0x23 bit 7 | Manual: the pilot drives and the program waits. Bits 4 and 5 are cleared when switching to manual; bit 3 is the situation room's recall |
| +0x2e | **The program given** |
| +0x30 | **The program running**: the one given, or a state the script entered by itself (in `scen12a` three human robots given program 9 are saved running 24). The console shows this one, and the next and previous programs are counted from it |
| +0x31 | The script's stack pointer, 10 (empty) when a program starts (T-053, `docs/sim.md`, "Orders") |
| +0x33, +0x38 | The script's flags, cleared; and its wait, started from a counter (`DS:0x4db5`, down 10 a start, up 49 more when it runs out) so robots given programs together do not run in step |
| +0x34 | The script pointer; in a file, `0xa000` + its offset from the table (`1000:4826`, `4833`) |

`order_set_program` (`1000:476f`) sets all of these from a program number.

**Choosing a program** is done on the console's program panel (type `0x154`,
`ui_draw_program_panel` `1000:d2d1`, `ui_program_panel_on_mouse` `1000:d34c`), for the
robot viewed:
- While the pointer is over the panel, a press or release (left or right) marks it
  pressed (+0x22 bit 7, bit 0 flipped); a **left release** switches the robot to
  automatic (`order_switch_to_automatic`, `1000:d446`) and gives it the **next**
  program offered to its kind (`order_select_next_program`, `1000:4794`), wrapping past
  30 to 0; a **right release** the **previous** one (`order_select_previous_program`,
  `1000:47b1`), wrapping below 0 to 30. Programs not offered are skipped.
- F4 does what a left release does. F2 switches to automatic and F3 to manual
  (`order_switch_to_manual`, `1000:d42c`), each only if it changes something; both
  post an event to the robot (+0x36 bits from `DS:0x1c56`: events 0xa and 0xb, which
  the message panel reports).
- **T-052 found the two names the wrong way round**: `1000:d42c` *sets* +0x23 bit 7,
  and the script steps every tick only with that bit, recalling a robot and giving it
  a path's program call `1000:d42c`, and the cockpit's drive keys call `1000:d446`
  (which clears it) before they steer. So bit 7 is "the program drives", `1000:d42c`
  is `order_switch_to_automatic` and `1000:d446` `order_switch_to_manual` (renamed in
  Ghidra); which F key does which, and the panel's colours, follow from that and are
  left to T-111 to check and fix here and in the port.
- **Drawn**: the panel filled with the kind's fill colour (slot `0x51a`), then the
  program's name, transparent, in the kind's font, one row below the top and the
  kind's x offset in from the left, in the kind's text colour if the pilot drives
  and colour 6 if the program does (tables by kind at `DS:0x7177` font, `0x717d` x
  offset, `0x71b8` fill and text, `0x71c0` pressed). While pressed, the panel is
  recoloured instead (slot `0x522`: pixels of one of the two pressed colours become the
  other; which way flips with +0x22 bit 0) and the name is left as it is.

In the stock `.usg` files nearly every robot, of either side, is saved with +0x23 bit
7 set (one human robot in `scen06a` is not); the human robots are given program 9 (or
1 in `scen08a`), the alien ones 8 or, for scorpions, 13. Loading keeps the manual bit,
but gives every robot its program (+0x2e) anew, so +0x30 is the given program when a
mission starts (T-058, `usg.md`, "Loading"). The
paths of `APTH`/`HPTH` are each side's missions for the computer (`usg.md`); no view
sets them.

### The training menu (T-043)

`ui_run_training_menu` (`1000:6df6`) clears its action (`DS:0x5e50` = 0xffff), opens
widget set `0x3842` with its state at `DS:0x5e88` (a 23-byte template), and loops
until a widget sets the action; then it jumps through the table at `DS:0x5e52`.

| Type | Rectangle | What |
|---|---|---|
| `0x30` | the screen | `TRAINMEN` (file id 0x06), fading in over 32 retraces (slot `0x498`) |
| `0x32` x5 | (14, 200, 25, 275), then 10 rows lower each | The buttons, drawn by the picture; their actions, from the template, are 0, 2, 4, 6 and 8 top to bottom (`ui_training_button_on_mouse`, `1000:6e98`) |
| `0x34` | none | The key table `DS:0x5e9f` (`ui_training_menu_on_key`, `1000:6ee5`): ESC and Alt+Q action 8; H 0, N 2, S 4, C 6 |
| `0x14c`, `0x14e`, `0x148` | none | The keys every screen has |

A button shows its press by XORing its rectangle's frame with 9 (slot `0x536`, on the
press and again on the release, so the release restores it) and acts on a release over
it. Actions 0 to 6 load `HTRAIN`, `NTRAIN`, `STRAIN` and `CTRAIN.USG`
(`ui_run_training_mission`, `1000:6ecf`, from the names at `DS:0x5e5c` on): the scenario
reader (`1000:eefc`) loads the file; if that fails nothing happens; otherwise the
digitised sound is started (slot `0x438`), the mission runs and the sound is stopped.
Action 8 returns. Either way the menu returns to the map room.

### The review (T-044)

From a Ghidra pass over `ui_run_review_screen`, its widgets and handlers and the page
set-up code in segment `1fe6`. The review is **not a record of the campaign**: it is a
reference of the three robot kinds. Each of its three pages shows one robot walking
and turning in a 3D view, with three panels of text about it (a description and its
specifications, in the game's words). Nothing on it comes from the campaign table,
the player or a save file; its texts are in the host's string segment (Ghidra `38fa`)
and its models are the `.MXX` files the mission draws (`mxx.md`).

**The loop** (`ui_run_review_screen`, `1000:796e`, called straight from the map room's
R handler `1000:7408`): resets the 3D view's state (`1000:28bd`) and the review's
(`ui_review_reset`, `1000:7ae4`: the model's position and facing to 0, page 0), copies a
12-byte state template (segment `0x38ec`) to `DS:0x61ed`, opens widget set `0x385c`
(eight widgets, 0x10 to 0x70) keeping the map room's to put back, points the 3D
renderer's object hook `DS:0x152a` at `1000:7a5f` (it adds the page's robot to the
render list) and clears the done flag `DS:0x61e8`. Then it loops as the mission does:
draw the widgets whose redraw bits are set; when the 3D view is due (`1fe6:39c3`, not
traced) run `ui_review_tick` and render; wait for an input event and hand it to the
widgets; until the done flag is set. Then it restores the hook and reopens the map
room's widget set, whose picture loads `MAPROOM` and fades it in again.

| Type | Rectangle | What |
|---|---|---|
| `0x0e`, `0x148` | none | As on every screen; `0x148` is the screenshot key |
| `0x1a` | none | The key handler (`ui_review_on_key`, `1000:7a19`, from the state template) |
| `0x40` | the screen | The picture (`ui_draw_review_picture`, `1fe6:77f7`): loads `REVIEW` (file id 0x12) with pointer shape 1 shown, builds the run-length region of its colour-255 pixels once (`DS:0x6238`, flag `DS:0x6237`), copies the picture around it and shows it **at once with its palette** (slot `0x494`, no fade). The picture's own art labels the buttons |
| `0x42` | the screen | The text panels (`ui_draw_review_text_panels`, `1fe6:7874`) |
| `0x44` | (14, 161, 115, 313) | The robot's view: `1fe6:78df` copies the 3D renderer's frame through the colour-255 region (slot `0x5a4`) when the renderer has one ready (`DS:0x1450`) |
| `0x46` | (187, 9, 196, 52) | Button: the next page (`ui_review_next_button_on_mouse`, `1000:79e3`) |
| `0x48` | (187, 56, 196, 99) | Button: back to the map room (`ui_review_exit_button_on_mouse`, `1000:79fe`) |

**Buttons.** They are part of the picture. Any press or release while the pointer is
over the capturing button (not a move) XORs its rectangle's frame with 9 (`1000:dfd5`,
slot `0x536`), exactly as the training menu's buttons do; a **left release** then acts.
A right press and release only XOR the frame twice; a release off the button does
nothing, so the frame stays XORed.

**Keys** (`DS:0x6244`, 4 pairs; events with bit `0x2000` are ignored and bit 7 of the
key word's low byte is masked off, as in the name entry):

| Key word | Handler | Effect |
|---|---|---|
| `0x0001` (ESC), `0x2010` (Alt+Q) | `ui_review_leave` (`1000:7a3b`) | Sets the done flag: back to the map room |
| `0x000f` (Tab) | `ui_review_next_page` (`1000:7a41`) | The next page, as the next button |
| `0x200f` (Alt+Tab) | `ui_review_previous_page` (`1000:7a4c`) | The previous page |

**Pages.** The page number `DS:0x61aa` counts 0, 2, 4. `ui_review_set_up_page`
(`1fe6:7a12`) wraps it (past the last, `cmp word [0x61aa], 4`, it subtracts 6; the
previous key wraps below 0 to 4), resets the model's position, facing and parts, marks
the text panels for redrawing and jumps through a table of three set-ups at
`1fe6:7a52`. Each set-up stores: the model (`DS:0x61b9`: 0, 2, 4, whose texts are about
the humanoid, the scorpion and the scout; `DS:0x61b8` = 2 is the render node's type),
how many part bytes the render node carries (`DS:0x61ba`: 5, 12, 7), a height
(`DS:0x61b3`), the facing 0, the three texts' offsets in the string segment
(`DS:0x61cc`, `0x61ce`, `0x61d0`), the page's parts animation (a far routine in
`DS:0x61dc`) and its motion script (`DS:0x6254`, `0x627c`, `0x629e`, started by
`1fe6:7b0c`).

**Text panels** (`ui_draw_review_text_panels`): font 4, colour 0x7d (shades 0x7d to
0x7f) on 0; for each panel, the rectangle at `DS:0x61f9`, `0x6201` and `0x6209` =
(27, 12, 89, 153), (113, 12, 181, 153) and (132, 168, 180, 309) is filled with colour 0
(slot `0x51a`), the cursor put at its top-left corner and the page's text drawn
transparent from the string segment (`1fe6:4a47`, as the map room's alert word). The
texts carry their own layout: 0x0c starts a new line, 0x0e moves one pixel right, and
code 0x7f is a glyph. There is no wrapping, scrolling or paging inside a panel.

**The robot** (T-047). `ui_review_tick` (`1000:7aa2`) runs once per change of the
host's game clock `DS:0x374` (the 60 Hz chain), when the loop finds the 3D view due
(`1fe6:39c3`: whenever no input event is waiting): it counts the review's ticks
(`DS:0x61d2`, never reset), counts down the current script step (`DS:0x61d8`) and at 0
runs the next ones (`ui_review_step_script`, `1fe6:7927`), then applies the step's
per-tick change (`DS:0x61da`) and calls the page's parts routine (far, `DS:0x61dc`).

A page's set-up (`ui_review_set_up_page`) zeroes the heading `DS:0x61ac`, the walking
counter `DS:0x61e4` and the sixteen frame bytes from `DS:0x61bc`, and stores: the model
(`DS:0x61b9`), how many frame bytes the node carries (`DS:0x61ba`: 5, 12, 7), the middle
word of the model's y (`DS:0x61b3`: 0, -3, 16, the **height** in pixels above the
view's centre), the **scale** `DS:0x61b6` = 0, the three texts, the standing parts
routine and the script (`DS:0x6254`, `0x627c`, `0x629e`: the next step its first, due
at the next tick).

**Scripts** are words: an opcode (handlers `1fe6:7948`, per-tick changes `1fe6:795c`,
both by opcode) and its operands.

| Opcode | Operands | Effect |
|---|---|---|
| 0 | ticks, step | For that many ticks, add the step to the heading (65,536 to a turn) |
| 2 | ticks | Wait |
| 4 | byte offset | Go on at that step at once |
| 6, 8 | ticks, step | For that many ticks, add the step to the scale, or subtract it |
| 10, 12 | none | Switch to the page's walking (table `1fe6:79d8`) or standing (`1fe6:7a04`) parts routine and go on at once |
| 14, 16 | none | Nothing; the count is left at 0, so the next step comes 65,536 ticks later (no script uses them) |

A step with ticks has its change applied on each of them, the first on the tick it
starts. The three stock scripts are the same: grow from 0 to 1:1 in 32 ticks; then,
looping, a quarter turn in 16 ticks standing, walk, wait 128 ticks walking, a whole
turn in 128 ticks walking, stand, wait 64 ticks. After the loop's jump an unreachable
shrink follows.

**Parts routines** (segment `1fe6`), one standing and one walking per page:

- **Walking** (`7568`, `762a`, `7763`): the counter `DS:0x61e4` goes up by 6 every tick
  and bits 4 to 7 of its low byte pick an entry of 16-byte tables written into the
  frame bytes: the humanoid's legs (frames 3, 4) and arms (1, 2) from four tables, the
  scorpion's six legs from six, the scout's two leg pairs (1 and 5, 2 and 6) from two.
- **Standing** (`75dd`, `76cf`, `77ae`): on ticks whose count is a multiple of 4, each
  part not at frame 0 settles: the humanoid's legs step up to 5 and wrap to 0, its arms
  step down to 0; the scorpion's legs step up to 6 and wrap; the scout's pairs step
  together to 6 and wrap.

**Drawing.** The view's widget (`1fe6:78df`) asks for view mode 0x1a with background
colour `DS:0x1457` = 0, zoom 1, and the region of the picture's colour-255 pixels
(`DS:0x6238`) as the target. That mode's steps (`docs/render.md`, "The review's view")
clear the buffer and draw the one node the render hook (`1000:7a5f`) makes: no camera,
no perspective. The model's anchor is the buffer's centre (`W / 2`, `H / 2`, halves
rounded down) plus the middle word of the node's x (0) across and less that of its y
(the height) down; the scale is the node's z low word (`DS:0x61b6`, 256 = 1:1); the
facing comes from the heading alone, `((((h + 0x8000) >> 8) >> 2) + 1) & 0x3e` (as
`mxx.md`'s rule with a bearing of 0, so heading 0 shows facing 32); the frames are the
node's, one per animation of the model, drawn as `mxx.md`, "Drawing". The frame is
copied through the region when ready. The region's rectangle (slot `0x59c`) is the
bounding box of the colour-255 pixels, its left edge moved down to an even column and
its right edge up when its width is odd; in `REVIEW` that is 148 by 97 pixels from
(163, 16), inside the widget's (14, 161, 115, 313).

### The briefing (T-044)

A scenario's `ORDR` chunk is read into the string segment at `38fa:04ed` (`usg.md`).
Its only reader found is **the situation room's main view in mode 0**, the orders
view (key O). There is no briefing screen, nothing at the start of a mission, and the
map room's selection panel shows only the scenario's name and alert level.

**Modes.** O, S, M, R and C call `ui_sitroom_set_view_mode` (`1000:676e`) with 0, 2, 4, 6
and 8: it stores the mode in the base's +0x37, sets +0x32 bit `0x4000` (a full
redraw), sets +0x34 (the orders' typing position) to 0 and marks the main view and
its neighbours for redrawing. The main view's draw handler (`ui_draw_sitroom_main_view`,
`1000:5551`) calls the mode's routine from the table `DS:0x5ae0`; mode 0's is
`ui_sitroom_type_orders_text` (`1fe6:6fc1`). The situation room's picture draw (when the
view is opened: at a mission's start, and on coming back from a console) also sets
+0x34 to 0 (`1000:54fc`). The mode itself is kept: a mission starts in the mode saved
in its file's base. In the stock files the human base starts in mode 0 in `scen00a`,
`scen05a`, `scen09a` and the four training files, and in mode 4 (map) or 6 in the
others, where the player presses O to read the orders.

**Typing** (`ui_sitroom_type_orders_text`), each time the main view is drawn, on the
main view's text surface (`DS:0x59e4`):
- If +0x34 is 0, it clears the view (slot `0x5b4`, a screen-region helper, with colour
  0x10 and the view's region `DS:0x5680`: presumably the view's colour-255 pixels
  filled with 0x10) and puts the cursor at (96, 30), five columns and four rows inside
  the main view's rectangle (26, 91, 130, 292).
- It selects font 4, colour 0x1d (shades 0x1d to 0x1f) and colour 0x17 for codes with
  bit 7 set, and draws **one byte** of the text, the one at +0x34, transparent (slot
  `0x594`), then adds one to +0x34.
- At a NUL or 0x1a (the DOS end-of-file byte every stock text ends with) it stops;
  otherwise it marks the main view for redrawing (+0xc6 = 3), so the next pass of the
  mission loop draws the next byte.

So the orders type themselves out, one character per pass of the mission loop (the
loop draws its widgets on every pass; about once a frame, to check in DOSBox). The
text's CR LF line ends are the font's control codes: CR back to the left margin (96),
LF down one line (font 4's 6 rows). There is **no wrapping and no paging**: the stock
texts have 4 to 15 lines of at most 46 characters, which fit the view. No key acts on
the text except the mode keys, which start it again (O) or replace it.

### The scenario editor (T-048)

From a Ghidra pass over the editor's key table and handlers, its status display, the
scenario writer and `mapdvr.dvr`. There is **no editor screen**: with unlock word 9
(`DS:0x5bc1` = 0xab65, `key.md`) the editor's keys simply work in the **situation
room** of a running mission, on the mission's own objects and paths, and the
scenario is saved from there. The situation room's widget set (`0x3822`) has a widget
of type `0x14a` whose key handler is `ui_dispatch_editor_key` (`1000:6a8d`); no console
and no front-end screen has one. It compares the event's whole key word (presses only:
a release has bit 7 set) with the 31 pairs at `DS:0x5bc3`, runs the match, redraws the
status and calls `1000:0270` (the beep).

**Alt+F1.** The map room's Alt+F1 and the editor's both run `ui_editor_key_toggle_flag`
(`1000:6bcf`), which with word 9 toggles `DS:0x4db4` and prints `ON` or `OFF` on the
debug screen (`dbg_toggle_unused_editor_flag`, `1000:4856`). Nothing in `xeno.exe`,
`mgavideo.dvr` or `sounddvr.dvr` reads `DS:0x4db4`, so the key does nothing visible: the
editor needs no opening.

**The status display** goes to the **monochrome debug screen** only (the host's word
slots `0x22a`–`0x256`, which `dbg_init_mono_monitor` installs only with unlock word 0 and
a monochrome adapter; otherwise they do nothing), so a stock set-up never shows it.
At start-up `ui_reset_editor_state` (`1000:690d`) clears the kind, side and path and
prints a template (`DS:0x5d7d`: the title `CONSTRUCTION SET`, then four rows from row
20 of an 80-column text screen) that `ui_draw_editor_status` (`1000:6940`) fills in:

| Row | Left | Middle | Right |
|---|---|---|---|
| 20 | `TYPE:` the kind's name (`DS:0x5c56` by kind, `DS:0x5c54` for the path: `PATH `, `HUMAN`, `SCORP`, `SCOUT`, `BASE `, `RELAY`, `OBST.`) | `PATHS:` the side's path count | `PRIOR.:` the path's weight; `T#:` its robots |
| 21 | `SIDE:` `HUMAN` or `ALIEN` (`DS:0x5ca4`) | `PATH #:` the current path | `H-MIX%:` the mix's humanoids as a percentage (table `DS:0x7d35`); `H#:` humanoids on it |
| 22 | `HEAD:` the heading in degrees (`1000:0320`) | `POINTS:` its point count | `N-MIX%`, `N#:` scorpions |
| 23 | `PSHOW:` the path display, `OFF` or `ON ` (`DS:0x5c48`) | the path's mission name (`DS:0x5cf5`) | `S-MIX%`, `S#:` scouts |

The path fields always show the **alien** table (its count `DS:0x7372` and slot list
`DS:0x7c3c`), whichever side is picked, and only while it has a path; the keys act on
the picked side's table (`DS:0x5cd1`). The state
is `DS:0x5c3f` kind (-1 path), `0x5c41` side (0 human, 1 alien), `0x5c43` heading,
`0x5c45` current path, `0x5c47` path display (0 or 2).

**The keys** (`DS:0x5bc3`, all with Ctrl):

| Key | Handler | Effect |
|---|---|---|
| Ctrl+H, N, S, B, R, O, P | `1000:6ad8`..`6b08` | Pick the kind to place: humanoid 0, scorpion 1, scout 2, base 3, relay 4, obstacle 16, path -1 |
| Ctrl+T | `1000:6b02` | Toggle the side |
| Ctrl+Left, Ctrl+Right | `1000:6bb3`, `6bba` | Heading for new robots down or up by 0xb6 (a degree) |
| Ctrl+Up, Ctrl+Down | `1000:6bc1`, `6bc8` | Heading 0 or 0x8000 |
| Ctrl+Ins | `1000:6b0f` | New path for the side (at most 8): it becomes the current one, with no points, mission 0 and that mission's weight and mix (four bytes a mission at `DS:0x7cf8`, `1000:6ce3`) |
| Ctrl+Del | `1000:6b31` | Remove the current path: the count drops by one and the paths after it move down a slot; if it was the last, the previous one becomes current |
| Ctrl+PgUp, Ctrl+PgDn | `1000:6b75`, `6b84` | Next or previous path, within the count |
| Ctrl+Space | `1000:6b90` | The current path's next mission (0 to 6, wrapping), with its weight and mix |
| Ctrl+Backspace | `1000:6ba6` | Drop the current path's last point |
| Ctrl+Z | `1000:6bdc` | Toggle the path display on the maps (`DS:0x5c47`; T-059) |
| Ctrl+E | `1000:6be2` | Toggle `DS:0x6ffa`, which makes the maps' robot routine (`1000:f976`) skip its check that an alien robot has been detected (+0x17 bit 2): the maps show every alien robot (T-108) |
| Ctrl+D | `1000:6ab7` | Destroy the selected unit (`sim_destroy_robot`): base +0x3c is the selected unit's object, which the roster's selection writes (`1fe6:6f31`, with +0xc7 and +0xcd = 3 for redraws; T-108) |
| Ctrl+C | `1000:6901` | Clear the base's seven message slots (`1fe6:6b2e`, as `sim_init_base` does) |
| Ctrl+Alt+R | `1000:6ac8` | Remove every relay (`1000:f60f` with kind 4) |
| Ctrl+M | `ui_run_map_editor` (`1000:448c`) | The terrain editor (`mapdvr.dvr`, below) |
| Ctrl+Alt+O | `1000:6d67` | Open-file dialog, then read up to 800 bytes of the chosen text file into the briefing buffer (`38fa:04ed`) and switch the main view to the orders (mode 0). An unused string at `DS:0x5d61` names a developer's `ORDERS1.TXT` |
| Ctrl+Alt+B | `1000:6dc3` | Open-file dialog, then the chosen file's base name becomes the sky (`scen_set_sky_name`) and is loaded |
| Ctrl+Alt+arrows | `1000:6dde`..`6df0` | Scroll the situation room's map by half a unit (base +0x4f x, +0x53 y; `1fe6:7243`..`727a`) |
| Alt+F1 | `1000:6bcf` | As above |

**Placing** (`ui_editor_place_at_pointer`, `1000:6be8`, T-108): the situation room's
main view's mouse handler (`1000:571f`) calls it on a left press whose event word has
Ctrl (0x1000) while the view is in **mode 4 (the map)** (`1000:5781`); the press then
does nothing else, so without unlock word 9 (checked first) a Ctrl+press on the map
does nothing at all. Every kind is followed by `1000:0261`/`0266` (beeps through
`2000:4189`/`4191`, not traced).

The pointer becomes a world point in `ui_map_pointer_to_world` (`1000:3fd3`, the
inverse of the drawing above): the row offset is the target rectangle's (`DS:0x145a`:
top +4, left +6, bottom +8, right +0xa) middle row less the pointer's row
(`DS:0x14ba`), the column offset the pointer's column (`DS:0x14bc`) less the middle
column, each middle the halved 17-bit sum. Each offset, a 16-bit whole number of
pixels (`m`, exponent 0, sign), is divided by the focal factor the last frame used
(`DS:0x4d24`, which `gfx_set_view_camera` writes; `1000:0e3c`), turned into a 16.16 by
`1000:0d6f` (`m << (e + 8)` by whole bytes, `m >> 8` at `e + 8 = -8`, 0 below,
0x6fff6fff or 0x90019001 when it does not fit), and added to the camera's position:
the column's to x (`DS:0x1462`), the row's to y (`DS:0x1466`). The map's heading is 0,
so nothing is turned back. Both ways truncate, so a point placed off a whole number of
pixels a unit is drawn up to a pixel short of the pointer.

By the picked kind (`DS:0x5c3f`):

- **Robots** (0, 1, 2): `1000:7e99` allocates the object (`sim_alloc_object_slot`),
  writes the side's flag byte (`DS:0x5ca2` by side: 0 or 8) to +0x16, sets it up as new
  (`sim_init_new_object`, `1000:f812`, below) and counts it (`DS:0x62f3`); then the
  point, the heading and target heading (`DS:0x5c43`), and the side's unit list
  (`sim_add_unit_to_side_list`: the first free entry of 38, and base +0xc7 = 3).
  In the port since T-108 part 2 (`xeno_sim::Sim::place`).
- **Bases** (3): none is made; the side's command object (`DS:0x568e` human, `0x5690`
  alien) moves to the point (x and y only).
- **Relays** (4): the point is worked out into a scratch object at `DS:0x5cb4`, then
  `1000:49a1` and the relay check `1000:d15b`; only if it allows (carry) is a relay
  allocated, its x, y and z copied from the scratch and set up (`1000:cc2a`, `cca8`).
  Partly traced (T-142): `1000:49a1` rounds the point's whole x and y by the mask
  `DS:0x4db2` (left by the last robot through `1000:49cc`: 0xfe for program 0xd,
  else 0xfc) and the check is `2000:7faa` (not traced); the set-up `1000:cc2a`
  clears +0x18 and +0x1f, sets radius 0x500 and `TARGETABLE`, a random byte at +0x1e,
  +0x21/+0x22 = 0, the four links' words (+0x25, +0x29, +0x2d, +0x31) 0xffff, then
  `1000:ccf0`; `1000:cca8` puts the relay on the lattice: x and y whole and even
  (rounded up), and if their bit 1 differs, cleared in both. Not in the port yet.
- **Obstacles** (16): allocated (`sim_alloc_object_slot`, `1000:f4b1`: the lowest free
  slot of the 240, the reference count 1, the lowest free entry of its pool, the bytes
  from +4 cleared, 0x30 of them for the small pool), then radius 13 (0x0d00) and
  `IN_FIELD` (`1000:e9c3`), and the point: so side human, heading 0, and every other
  byte 0, exactly as every stock obstacle is saved.
- **Path points** (-1, `1000:6cac`): the picked side's table (`DS:0x5cd1`) needs at
  least one path (`1000:6d06`), the current path (`DS:0x5c45`, in use or not) fewer
  than 16 points; the point is written after its last and the count goes up, and the
  status is redrawn.

`sim_init_new_object` (`1000:f812`) for robots runs `1000:d2b6` (+0x23 = 0, +0x24 =
100, +0x26 = 0, +0x25 = 0, program 9 through `order_set_program`, then +0x23 bit 7),
`d17d` (+0x4e = 100, +0xfd bit 0 cleared, +0x4f..+0x51 = 0xff), `d1cd` (+0x52 = 1,
+0x53..+0x5e cleared), `d533` (a call sign: a random number, then the next of the 64
not in use, `DS:0x72c5`, into +0xdb), `d5c4` (+0xdc, +0xdd, +0xe6 cleared), `d9ab`
(+0xef, +0xf1, +0xf3 = 0, +0xf2 = 2, and two display globals), `dc7e` (+0xf6, +0xf4
cleared) and `dd79` (+0xf9 = 0), clears +0x1b, sets +0x16 bits 0x66 and runs the
kind's routine (`DS:0xe73f`): a serial (`random & 0x1ff`, +0x18), then the
humanoid's (`1000:ab8a`) radius 0x0a00, +0x10b = 4, +0x103/+0x104 = 0, eye height
0x0f80, model height 0x0a00, 10 cloaks, ECM 0xff; the scorpion's (`9293`) radius
0x1200, 2000 rounds (+0x120), 6 missiles, +0xdd = 0, ECM 0xff, mines 20, alarm and
ECM buoys 8, decoys 4, +0x123 = 0xffff, 12 relays, +0x119 = 0, model height 0x0a80, eye
0x0b80; the scout's (`c40e`) radius 0x0a00, +0x105/+0x106 = 0, 80 shells (+0x107), ECM
0xff, 4 cloaks, mines 6, alarm and ECM buoys 2, decoys 6, gear 4, stance 0x2f, +0x10c
= 0x30, eye 0x0a00, model height 0x0c80. The humanoid and scorpion then work out
their velocity (`1000:d41c`, 0 at speed 0), and each copies a block of the load image
over its parts (`1000:2485`: the block named by `mov ax, seg` has the data's paragraph
at +4 and its length at +8; humanoid 53 bytes to +0x10c, scorpion 69 to +0x127, scout
60 to +0x10d; the same blocks are the consoles' widget sets, `ui_open_widget_set`),
read from the user's `xeno.exe` by `xeno_formats::editor`.

The call signs (`1000:dfaf`): `DS:0x72c5` holds a flag for each of the 64, all
cleared when a scenario's objects are read (`scen_read_dcbs`), so loaded robots hold
none; a new robot takes, from a random word's low byte plus one, the next free one (mod
64), and a destroyed robot frees its own (`1000:dfc9`). A save writes a made robot as
any other (`scen_pack_all_objects`, `DS:0xe81c`): its script pointer as `(pointer -
0x4ff4) | 0xa000` (`1000:4826`, the program table's address read by
`xeno_formats::editor`) and no target as 0xffff.

**Scrolling.** Ctrl+Alt+Left and Right add 0x7fff and -0x7fff to the base's map
centre x (+0x4f), Up and Down -0x7fff and 0x7fff to its y (+0x53) (`1fe6:7243`,
`7254`, `7267`, `727a`, on the viewed object `DS:0x62f1`), whatever the mode and the
map's bit 15; the camera uses the centre only while bit 15 is set.

**Saving.** The situation room's own key table (`DS:0x5b20`) has **Ctrl+Alt+P**, with
**no unlock check**: `ui_save_scenario_with_dialog` (`1000:efd5`) opens the "SAVE GAME"
file dialog (`ui_call_save_file_dialog`, `1000:fb78`, host slot `0x302`) and writes the
running mission with the scenario writer (`file_write_scenario`, `1000:efde`; layout in
`usg.md`, "Writing"). So any player can save a mission as a `.usg`. The matching load
(`ui_load_scenario_with_dialog`, `1000:eeec`: open dialog, then the chunk reader) has no
caller in either code segment or in the data below `DS:0x6e4d`. The same table's
Ctrl+Alt+S toggles bit 6 of `DS:0x62ec` and Ctrl+Alt+F toggles `DS:0x5ba0` (not traced).

**The terrain editor** (`mapdvr.dvr`, `docs/formats/map.md`; T-109 from a Ghidra pass
over the whole driver). `ui_run_map_editor` loads the driver into block `DS:0x150a`,
calls its enter slot `0x14f4` (`ui_enter_map_editor`, `0x006e`), points the render
list's first producer (`DS:0x1528`) at `1000:44ed`, then loops: draw the widgets
(`1000:26cd`), run the view's steps (`1000:28cf`), wait for an input event and hand it
to the widgets (`1000:24a9`), until the driver sets the quit flag `DS:0x5a`; then it
puts the producer back, calls the leave slot `0x14f8` (`ui_exit_map_editor`, `0x011a`)
and frees the block. The loop never ticks the mission: **the sim stands still** while
the editor is open.

**Its data.** The driver's init hands slot `0x68` 0x104 bytes of its data segment
from offset 0xde (`ES:SI`, `AX`, descriptor `DS:0x1514`), and every routine then
reads them through the host's `DS` at the same offsets (slot `0x68` itself is not
traced), so its variables, strings and key table live in the host's data segment (the key table "at driver data
`0x186`" is `DS:0x186`). Its widget set is a block of its own (four words, then six
12-byte widgets; slot `0x151c`, `ui_open_widget_set`, keeps the situation room's to
put back), whose 24-byte state template slot `0x1522` (`1000:2485`) copies to
`DS:0xde`: a draw widget (type `0x20`) over the whole screen whose routine,
`gfx_start_map_editor_frame` (`0x0164`), starts each frame; a key widget (`0x26`,
`ui_map_editor_on_key`, `0x032a`); a mouse widget (`0x24`) over the whole screen whose
routine is a word in its state (`DS:0xed`: `ui_map_editor_on_mouse`, `0x0563`, or
`ui_map_editor_on_drag`, `0x05af`, while a circle is dragged); and a draw widget
(`dbg_draw_map_editor_status`, `0x086b`) for the debug screen. A key routine sets
`DS:0xe2` (the view's redraw) and `DS:0xf1` (the status's) to 3.

| `DS:` | Size | What |
|---|---|---|
| `0xf6` | 4 | The circle buffer's block (segment, flags) |
| `0xfa`, `0xfe` | 4, 4 | The camera's x and y (16.16) |
| `0x102`, `0x106` | 4, 4 | The ground point under the pointer |
| `0x10a`, `0x10e` | 4, 4 | The dragged circle's centre |
| `0x112` | 1 | 0xff while a circle is dragged |
| `0x114` | 2 | The selected circle (its offset in the buffer) |
| `0x116` | 14 | The view's buffer descriptor: segment 0, so the frame is drawn on the screen |
| `0x124` | 12 | The view's rectangle: (0, 0, 200, 320), the whole screen |
| `0x130`, `0x131` | 1, 1 | The zoom factor (`DS:0x1452`) and zoom shift (`DS:0x1454`) |
| `0x142`, `0x144`, `0x146` | | The host's `DS:0x374`, `0x146e` and widget set, kept to put back |
| `0x14e` | 1 | The pointer shape before entering |

**Entering** (`0x006e`): the camera starts at the map origin (`DS:0x14e8`, `0x14ec`:
(128, 128)), the zoom at 2 with no shift, the view's buffer is `DS:0x116`
(`DS:0x146e`), the widget set opens, the pointer takes shape 7 (slot `0x1496`, which
returns the old one) and the circle buffer is allocated **empty**
(`mem_reset_circle_buffer`, `0x0839`). Entering does **not** import the terrain: the
editor starts with no circles, and the first circle slot selected. **Leaving**
(`0x011a`): the buffer freed, the old pointer shape, the situation room's widget set
reopened (it redraws), `DS:0x1450` = 0x80 and `DS:0x146e`, `0x374` put back.

**The circle buffer**: 0x2000 bytes; a header of three (start, end) pairs (0xc, 0xc),
(0x100c, 0x100c), (0x180c, 0x180c), of which only the first is used, so 341 circles
fit (from 0xc up to 0x100c). A circle is 12 bytes: x, y and radius, all 16.16; one
imported from a hill gets its 8.8 radius shifted left 8.

**The view** (`gfx_start_map_editor_frame`, `0x0164`, when the last frame is done:
`DS:0x1450` = 0x7f): view mode **0xe**, the map with the grid step (`1000:3760`, not
traced: T-143), not the 3D view; zoom and shift from `DS:0x130`, `0x131`; no static;
the rectangle `DS:0x124`; no viewer (0xffff); heading 0; clear colour 0x1f
(`DS:0x1457`); the camera's x and y from `DS:0xfa`, `0xfe` (z unchanged). The render
list (`1000:44ed`): the driver's producers first (slot `0x1500`, `0x01c9`, through the
code-segment table `CS:0x1e2`), then the mission's objects (`DS:0x152a`) as the
situation room's map shows them; the terrain's own producer is skipped, so the hills
are not drawn, only the circles. The driver's nodes, in order:

1. Each circle in buffer order (`0x022a`): type 0x12 (the maps' octagon, `1000:3a27`)
   in colour **0x0e** for the selected circle, **0x0c** for the others; +0x08 0x0e;
   +0x0a the circle's x, y and radius; +0x1e its radius (16.16).
2. While a circle is dragged (`0x02e0`): a type 0x12 node in 0x0e at the drag's
   centre, its radius the distance (`sim_approx_distance_between`, slot `0x1504`) from
   the centre to the ground point under the pointer.
3. The origin (`0x01f5`): a type 0xa node with colour 0 at the map origin, sprite
   0x1a0 (`msprite0`, the maps' target mark).

Two other producers (`0x0287`, a node at `DS:0x132` that swaps two words each time,
and `0x02b4`, the drag as a type 0xc node) are in the code but not in the table.

**The mouse** (`ui_map_editor_on_mouse`, `0x0563`): on a left press (event bit 2)
whose key word's high byte is exactly 1 (no Shift, Ctrl or Alt; the 1 is presumably
the left button) the camera moves to the ground point under the pointer
(`ui_map_pointer_to_world`, slot `0x152c`, with the frame's rectangle and focal
factor); with 0x11 (Ctrl) the point becomes the dragged circle's centre and the
handler switches to `ui_map_editor_on_drag`. Any event then puts the ground point
under the pointer at `DS:0x102` and redraws the status, not the view. While dragging
(`0x05af`) only moves and the left release count: a move updates `DS:0x102` and
redraws the view; the release ends the drag and adds the circle
(`ui_map_editor_add_dragged_circle`, `0x05e4`) at the centre with the radius from the
centre to `DS:0x102` (the last move's point, not the release's), clamped to 0xff.ffff
when it is 256 units or more; it becomes the selected circle. A full buffer adds
nothing and beeps twice (slot `0x26c`).

**The keys** (`ui_map_editor_on_key`, `0x032a`): key events only (event bits 0x1800;
0x800 is a press); the key word, release bit masked off, is written into the
sentinel at the table's end (`DS:0x1de`) and the 22 pairs at `DS:0x186` are scanned;
the sentinel's handler (`0x0346`) passes the key on. One unit is 0x10000.

| Key | Handler | Effect |
|---|---|---|
| Up, Down | `0x03a5`, `03c1` | The camera's y down, up a unit (so the map moves up, down) |
| Left, Right | `0x03dd`, `03f9` | The camera's x up, down a unit |
| Z | `0x0356` | Zoom in: the shift down one; at 0 the factor doubles instead, up to 0x80 (at 0x80 nothing, not even a redraw) |
| X | `0x037e` | Zoom out: the factor halves; at 1 the shift goes up instead, up to 0xff |
| Ctrl+Up, Down | `0x0415`, `0440` | The selected circle's y up, down a unit |
| Ctrl+Left, Right | `0x046b`, `0496` | Its x down, up a unit |
| Ctrl+Z, Ctrl+X | `0x04c1`, `04ec` | Its radius up, down a unit (nothing stops it going negative) |
| Ctrl+`,`, Ctrl+`.` | `0x0517`, `053d` | The previous, next circle slot |
| Space | `0x0348` | The next pointer shape (a debug key) |
| Alt+L | `0x066f` | Open dialog (slot `0x2fe`, T-110); the file loaded as the terrain (slot `0x1506`), then the circles cleared and imported from it |
| Alt+S | `0x0705` | "SAVE GAME" dialog (slot `0x302`); the circles written there (`file_write_map`) |
| Alt+N | `0x0694` | Clear the circles |
| Alt+I | `0x06a3` | Clear the circles and import them from the terrain (`scen_import_terrain_circles`, `0x080d`; with no terrain buffer, a beep) |
| Alt+E | `0x06b5` | **Apply**: write `BACKUP.MAP`; if that worked, load it as the terrain (slot `0x1506`: the `MAPP` buffer replaced, the origin added) and delete it. The circles stay and the editor stays open |
| Alt+Q | `0x06cf` | **Leave**: write `BACKUP.MAP` (kept) and quit; the terrain is not changed |
| Shift+`*` | `0x06f1` | Screenshot (slot `0x1558`, pointer hidden around it), then a beep |

The selection checks only its lower bound: the selected slot must be at or after the
first, so Ctrl+`.` walks past the last circle (and past the buffer, with nothing to
stop it) and Ctrl+arrows and Ctrl+Z/X then change bytes no node or save reads; a new
circle overwrites all 12 bytes of its slot. Writing a `.map` beeps once on success
(slot `0x26c`) and once the other way on failure (`0x26e`). A routine at `0x06dc`
(save as `DEFAULT.MAP` through the dialog if `DS:0x14f` is set, then quit) has no
caller.

**The status** (`dbg_draw_map_editor_status`) goes to the monochrome debug screen
only, under the title `MAP EDITOR` that entering prints: row 12 the pointer's ground
point, row 13 the camera, rows 14 to 16 the buffer's three (start, end) pairs.

### Which parts need the running simulation (T-056)

The 3D views, scanners and map (the colour-255, 254 and 253 regions), the message
panels and log, the unit status panels (damage, speed, weapons, cargo), the
situation room's roster contents and detail panels, the unit call signs, the map
controls and camera arrows, and every key that drives, fights or deploys. The views,
their switching and the orders (program and manual switch) are not.

### Where it differs from T-040's list

The backlog guessed `Logo -> Login -> SitRoom -> MapRoom -> Briefing -> Mission ->
Review`. In the original the map room, not the situation room, is the hub after the
log-in; the situation room is one of the mission's views; there is no briefing screen
(the orders are typed in the situation room's orders view); and the review, a reference
of the robots rather than of the campaign, is opened from the map room rather than
following each mission, which ends in a cutscene instead.

## In the port

`crates/xeno-app/src/screens.rs` is a state machine stepped once per retrace with that
retrace's `InputEvent`s from the `InputLog`, so a log replays across screens. After the
intro and the drop (or straight away with `--no-intro`) it starts at the log-in.

| State | Shows | Keys |
|---|---|---|
| Log-in | `login.m@l`, the name entry (T-041) | Letters and digits: the name. Backspace. Enter: log in, map room. ESC: quit, or the map room after N |
| Map room | `maproom.m@l`, the boxes, panels, buttons, hover label and campaign-won box (T-042, T-046) | Click or Tab/Right/Down, Alt+Tab/Left/Up: select. P, Enter, Space: play the selection. Ctrl+Space: run the mission again. T: training. R: review. N: log-in. Q, Alt+Q: quit (the original's keys); the buttons as their keys. With the user's `XENO.KEY`: `=`, `-` progress, Ctrl+W won, Ctrl+L lost |
| Training menu | `trainmen.m@l`, the buttons' pressed frames (T-043) | The user's key table and buttons: H, N, S, C and the first four buttons: training mission; ESC, Alt+Q and the last button: map room |
| Review | `review.m@l`, the page's text panels, the walking and turning robot (T-044, T-047) | The user's key table and buttons: Tab and the next button the next page, Alt+Tab the previous; ESC, Alt+Q and the exit button: map room |
| Mission | `sitroom.m@l` or a console's `.m@l`, by the object viewed (T-043) | The views' own key tables and widgets (below, "Mission views"); the port's Ctrl+W and Ctrl+L destroy the alien or the player's base (the mission then ends won or lost after its end script, T-055), Ctrl+C campaign won (a placeholder) |
| Cutscene (won, lost, campaign won) | The original's cutscene in the 320x240 mode (T-045, `vcl.md`), played by the window | Any key but P skips it, P pauses; then the map room |

Keys that only the port uses: Ctrl+W and Ctrl+L stand in for the shots that would
destroy a base (T-114), Ctrl+C for the campaign's last win. A mission ends with a
result when a destroyed base's end script is over (`docs/sim.md`, "The mission's
end"). A training mission goes back to the map room without a cutscene, as in the
original.

- **Key words.** Each key going down is turned into the original's key word (Shift
  0x08, Ctrl 0x10, Alt 0x20 in the high byte, the set 1 code with `E0` dropped in the
  low byte, `screens::key_word`) and looked up in the screen's (key word, action)
  table; the first match acts. Auto-repeat counts as a press.
- **ESC.** A screen claims ESC exactly when its table binds it (log-in, training menu,
  the mission's views, review; and a cutscene, whose player takes any key; for the training menu and the views, the
  user's tables); then `Translator::screen_claims_escape` is set and ESC reaches the
  game. On the map room ESC still leaves fullscreen or quits, as the original binds no
  ESC there.
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
  original. A name logs in: the port loads `<NAME>.LOG` or starts a campaign from
  `scendef.bin` (below, "Saves"), and fades to the map room. The original's goodbye, a thank-you line
  with the name in it, is printed when the window closes after a log-in. It is game
  text, so `xeno_formats::farewell` reads it from the user's `xeno.exe` (the three
  instructions above give its start, the name's place and the closing character); if
  it cannot, the game exits without it.
- **Labels.** The window title names the screen and its keys.
- **Mouse.** The pointer and buttons reach the screens through the log
  (`docs/game-loop.md`, "Input"), and the original's pointer
  (`crates/xeno-app/src/pointer.rs`) is drawn at it where the screens show it. Only the
  map room acts on clicks.
- **Pointer** (T-049): as "Shape and visibility" above. `pointer::State` keeps the
  shape and the driver's hide counter; all eight shapes are read from the user's
  `mgavideo.dvr`. It is hidden at the first log-in (also with `--screen login`) and
  shown when the map room first opens; a log-in from the map room's N shows it; each
  cutscene after a mission hides it and shows it again. Every other `--screen` starts
  with it shown. The window keeps the system cursor hidden over the picture while the
  screens are up, so the first log-in shows no pointer at all, as the original.
  **The busy shape is shown for one retrace.** Before the map room, the training menu,
  the review or a mission view draws its picture, the screens set shape 1 and wait one
  retrace (the frame shows the old picture with the busy pointer), then draw the new
  picture and set shape 0. The original shows it for as long as its disk load takes,
  which depends on the machine and is never under a retrace (the pointer is redrawn
  only on the retrace). The port's own load is far shorter than a retrace, so one
  retrace is the least the original would show; a longer, made-up span would only add
  lag. Input in that retrace is dropped, as during a fade out. After a fade the
  palette is black, so the busy retrace only delays those screens by one retrace. In
  the mission views, each move picks the shape the widget it lands on would set
  (T-106, `MissionState::pointer_shape`): the last widget whose rectangle holds it,
  its region tested on the view's picture (the pixel's colour, which is what slot
  `0x5bc`'s region holds), Alt from the keys held, the mode and display mode as they
  are; 2 on a corner, 0 elsewhere. A view opens with the arrow. As in the original, a
  mode key or Alt changes the shape only at the next move. The shapes are read from
  the user's `xeno.exe` (`views::PointerShapes`).
- **Map room** (`crates/xeno-app/src/maproom.rs`, T-042): as "The map room (T-042)"
  above. It draws over `maproom.m@l` the boxes (keyed: the look for each `state`, or
  unavailable above the progress), the highlight on the selection, the name, score
  and selection panels (the scenario's name, the label and its alert
  word, in the user's fonts 0 and 4, colour 0xe0) and the buttons (opaque, the next
  sprite while pressed, with the original's quirk after a release off the button). A
  new campaign (`maproom::Campaign`) starts at progress 1 with the locked scenarios at
  8; nothing is selected. Clicks act on the release: on the map area the left button
  selects the selectable box under the pointer and the right one also plays it; on a
  button, released over it, it runs the action of the first key whose handler is the
  button's (`MapRoom::button_keys`). Tab and the arrows step through the selectable
  scenarios. P, Enter, Space and the play button start the mission placeholder for the
  selection, remembered in `Screens::scenario` and logged with its `scenNNa.usg`, and
  do nothing without one; T is refused while a campaign mission has no result. With
  `--screen map-room` and no log-in, a guest's new campaign is shown.
- **Campaign** (`crates/xeno-app/src/campaign.rs` and `maproom.rs`, T-046): as
  "Campaign rules" above, with the user's values (`xeno_formats::campaign`, kept in
  `maproom::Assets::rules`). Play loads the selection's `scenNNa.usg` only when it is
  not the loaded scenario (else the mission and its orders resume; a file that cannot
  be read forgets the loaded scenario and plays nothing), then sets the result to 0 and
  the scenario to 12. A mission that ends won or lost (its sim's
  `MissionOver`, T-055) runs the won or lost routine with the player's base's kills and
  losses, which score the points and fill the `WIN` caption; the cutscene is then chosen as `ui_map_room_run_mission` does (campaign won, lost, won,
  else none). Ctrl+C, the port's stand-in for the last win, wins the scenario and sets
  the campaign-won flag. The training menu sets the result to 3 and clears the
  selection and the loaded scenario. The developer keys (`=`, `-`, Ctrl+W, Ctrl+L in
  the map room) work only when the user's `XENO.KEY` gives unlock word 8 the value the
  user's `xeno.exe` checks; Alt+F1 with word 9 logs that it toggles a flag nothing
  reads (T-048). Ctrl+Space runs the mission again with no check. The hover label follows
  the pointer over the boxes (none while a button is held, cleared off the boxes and
  by a release that selects nothing), placed, filled, framed and typed as described
  above in the user's font 2; the campaign-won box and message, and the prompt with
  nothing selected, are drawn with the panels. The port redraws the whole map room
  instead of saving and restoring the pixels under the label, and draws the label over
  the boxes every time. The random word is the original's shift register
  (`xeno_sim::Random`, T-050), seeded from the run's seed (`--seed`, else the clock)
  instead of the timer; there is one for the run, which a mission's sim takes and gives
  back and a second log-in carries on (T-051, `docs/sim.md`). The calendar's day advances but is not shown; a save
  keeps it (below, "Saves").
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
- **Review tables** (T-044): `xeno_formats::review` reads the review's widget set (the
  view and both buttons), its key table, the panels' font, colour and rectangles, and
  each page's model and three texts (from the string segment) from the user's
  `xeno.exe`, from the instructions that use them after checking their bytes; the
  handlers the keys name and the widget types are constants. `xeno-tool inspect` shows
  their shape.
- **Training menu** (T-043): its buttons and keys are the user's (`xeno_formats::views`,
  `read_training`): a press on a button XORs its frame with 9, a release over it XORs it
  back and acts; released elsewhere the frame stays XORed, as in the original. A
  mission loads its `?TRAIN.USG` with `xeno_formats::usg` (all four, and the
  campaign's `scenNNa.usg`, are parsed when the screens load; one that cannot be read
  is logged and does nothing when chosen, as the original's failed load does) and goes
  to the mission; the map room's play loads `scenNNa.usg` the same way.
- **Mission views** (`crates/xeno-app/src/mission.rs`, T-043): a mission holds the
  loaded file, its `xeno_sim::Sim` (T-051: the file's `DCBS` objects and the robots'
  orders, which the views change only through the sim; `docs/sim.md`) and the views'
  state, for the whole mission, so orders persist across views. It starts on the situation room. The widget sets, key tables, sprite buttons,
  roster rows, program names and panel styles are the user's; what a key does is
  chosen by the original handler it calls (a short list of handler offsets in the
  port). Switching views draws the new picture at once with its palette, as
  `ui_open_view_of_object` does. In the situation room: the six sprite buttons (one
  shared pressed look), a roster of the player's robots (left press selects; right
  release, Space, J and the view button open its console; the scroll buttons scroll;
  Tab and Alt+Tab step the selection), and Alt+Q, the end button and the corner end the
  mission with no result (back to the map room). In a console: the program panel
  (filled in the kind's colour, the running program's name in the kind's font, in the
  manual colour or colour 6; pressed, recoloured; a left release the next program
  offered to the kind, a right release the previous, both back to automatic), F2, F3
  and F4, the unit-name button and Tab / Alt+Tab (the next or previous of the player's
  robots, by object slot: the order of the original's unit list is not traced), and
  ESC, Alt+Q and the corner back to the situation room. A release away from the
  pressed widget does nothing and leaves its pressed look, as in the original. Each
  change of orders is logged. Placeholders, the port's own: the pixels of colour 255 (and 253, 254 in the
  consoles) where the mission would draw are hatched black with a "T-056" label; the
  roster rows show each robot's number and program, the selected one framed. The
  unit-name button shows the robot's call sign and the message panel the robot's text
  (T-104, "The message panel (T-104)"). `--screen humancon`, `scorpcon` and
  `scoutcon` open the first training mission, in the menu's order, with one of the
  player's robots of that kind, on that robot; `--screen sit-room` the first training
  mission.
- **Briefing** (`crates/xeno-app/src/mission.rs`, T-044): as "The briefing (T-044)"
  above. A mission keeps its file's `ORDR` text and starts in the main view mode saved
  in the player's base (+0x37). O, S, M, R and C (the user's key table, by the
  original's handlers) set the mode; in the orders mode the main view's colour-255
  pixels are recoloured to 0x10 and the text is typed from (96, 30) in the user's font
  4, colour 0x1d, one byte per retrace, until its NUL or 0x1a, CR LF as the font's
  control codes. O, and coming back to the situation room, start it again. Mode S keeps
  the placeholder (T-126). The speed (one byte a retrace) is the port's
  reading of "one per pass of the mission loop"; check it in DOSBox.
- **Main view** (`crates/xeno-app/src/mainview.rs`, T-103 part 1): modes M, R and C
  as "The situation room's main view (T-103)" above, from the tables
  `xeno_formats::sitroom` reads and `msprite0`. The map starts with the overlays, zoom
  and centre saved in the player's base and is drawn each frame (not every 4 ticks);
  the pixel scale is `xeno_render::view3d::Camera::map_offset`. Known gaps, each a
  `shortcut:` in the code: the labels' colour (0x0f, a guess), the detection and relay
  link bits are the scenario's as loaded, a target object's marker and the grid and
  path overlays are not drawn, the log is the base's as loaded (the sim posts no
  messages), the rule's line routine (slot `0x558`) is not traced. Mode R uses the
  console's 3D view with zoom 1 and no pan, and ignores a damaged camera (mode 0x18).
- **Roster and controls** (`crates/xeno-app/src/mainview.rs`, T-103 part 2): as "The
  roster, map controls and mode buttons (T-103)" above, from `xeno_formats::sitroom`
  and the view's nested widgets (`views::View::nested`). Shortcuts: a robot's call sign
  and repair state are the scenario's as loaded (robots made since show call sign 0);
  the C button's new-message look is not shown (the sim posts no messages). Not yet:
  the status, detail, reserve and clock panels and the power rows (T-127), and a left
  press on the map to move its centre or select a unit (T-128).
- **Review** (`crates/xeno-app/src/review.rs`, T-044): as "The review (T-044)" above,
  from the tables `xeno_formats::review` reads (kept in `maproom::Assets`). The map
  room's R opens it at once with its palette, on the first page; each page fills the
  three panels with colour 0 and draws its texts transparent in the user's font and
  colour; Tab, Alt+Tab and a left release on the next button turn the pages, wrapping;
  ESC, Alt+Q and a left release on the exit button go back to the map room, which fades
  in again. The buttons' frames are XORed with 9 on a press and back on a release over
  them (a release elsewhere leaves them XORed). The robot (T-047) is `ReviewState`'s
  `Robot`: one tick per 60 Hz tick of the 70 Hz retraces, its script and parts routines
  read from the user's `xeno.exe`, the page's model loaded as the start-up loads it
  (`xeno_render::LoadedModel`) and drawn by `Framebuffer::draw_model` into a buffer the
  size of the colour-255 region, copied through it.
- **Cutscenes after a mission** (T-045): as `vcl.md`, "In the port". The lose picture
  is chosen by the robot console the mission last opened (`Screens::console`, reset
  when the map room plays a scenario); the win caption shows the numbers the won
  routine returns (T-046): 0 kills, 0 losses and the bonus as points until the
  mission counts them.
- **The scenario editor** (T-048, `crates/xeno-app/src/editor.rs`): with the user's
  `XENO.KEY` giving unlock word 9, the editor's keys, read from the user's `xeno.exe`
  (`xeno_formats::editor`), act in the situation room before its own keys, on a copy
  of the scenario the mission loaded: the kind, side and heading, the paths (new,
  remove, next and previous, mission with its defaults, drop the last point), the path
  display, Ctrl+E's flag and Ctrl+Alt+R (remove the relays). After each key the status
  rows are logged, built from the user's template and names as the original prints them
  on its debug screen. Ctrl+M opens the terrain editor (below). Ctrl+Alt+P (no
  unlock, as in the original) writes the scenario with `usg::write` as `EDITED.USG`, a
  fixed name until the file dialogs (T-110). Both go to `--save-dir`, by default the
  game directory, where the original writes them. Not done: placing on the map,
  Ctrl+D, the map scroll (T-108), Ctrl+Alt+O and B (T-110), Ctrl+C (T-103). The edits
  change the scenario copy, not the sim's world (which does not run yet).
- **The terrain editor** (T-109, `crates/xeno-app/src/terrain.rs`): Ctrl+M, with the
  key table read from the user's `mapdvr.dvr` (`xeno_formats::mapdvr`; without it
  Ctrl+M does nothing), opens it over the whole screen and the sim stands still. The
  view is `mainview`'s map at the editor's camera, zoom and shift, cleared to 0x1f,
  with the circles as octagons (0x0e the selected one and the drag's, 0x0c the
  others), the origin's sprite and the mission's objects as the situation room's map
  shows them. A left press moves the camera to the point under the pointer, a
  Ctrl+left drag makes a circle; the arrows, Z and X, Ctrl+arrows, Ctrl+Z and X,
  Ctrl+`,` and `.`, Space, Alt+N and Alt+I work as traced; **Alt+E** makes the circles
  the mission's terrain through the sim (`Sim::set_terrain`; the scenario copy, so the
  views and a save, follows) and **Alt+Q** writes `BACKUP.MAP` to `--save-dir` and
  closes it. The status is logged at debug level. Not done: Alt+L and Alt+S (the file
  dialogs, T-110), Shift+`*` (a screenshot), the grid (T-143); the point under the
  pointer is worked out from the camera now rather than the last frame's.
- **Saves** (T-057): the log-in loads `<NAME>.LOG` from `--save-dir` (else the game
  directory), any case, into the campaign, the map room's selection and the loaded
  mission (parked, so playing the loaded scenario resumes it); a file that does not
  read as a save starts a new campaign, logged. The map room's N and Q write it there
  once a save was loaded or a scenario played (`xeno-app/src/save.rs`). The world
  written is the loaded scenario as loaded, not the running sim's (T-116).
- **Not modelled**: the music.
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
| `1000:3877` | `gfx_draw_map_nodes` | The maps' draw step: one node at a time through `DS:0x39ae` (T-103, created) |
| `1000:3e6c` | `gfx_project_map_offset` | A position less the camera's, scaled to map pixels (T-103) |
| `1000:3eb9` | `gfx_scale_map_offset` | An offset turned and scaled by the focal factor to map pixels (T-103) |
| `1000:f976` | `gfx_emit_robot_map_node` | A robot's map node: a dot or a sprite by heading (T-103, created) |
| `1000:5630` | `ui_label_sitroom_robot_view` | Mode R's heading or at-base text (T-103) |
| `1fe6:6b84` | `ui_start_sitroom_robot_view_frame` | Mode R's camera on the selected unit (T-103) |
| `1fe6:6c05` | `ui_start_sitroom_map_frame` | Mode M's camera, zoom and colours (T-103) |
| `1fe6:6cb9` | `ui_draw_map_zoom_label` | The map's zoom label (T-103) |

## Open questions

- What slot `0x5b4` does exactly (the orders view's clear, presumably filling the
  view's region with one colour), and how often the mission loop passes, which sets
  the orders' typing speed (T-044; check in DOSBox).
- The review's robot against DOSBox: its speed (one step a 60 Hz tick, when the loop
  has no input waiting) and its place in the view (T-047).
- What runs the step list at `DS:0x38ea` whose last routine sets the quit flag at
  `1000:424b` (T-055 found it touches no object; `docs/sim.md`, "The mission's end").
- The situation room's panels, spinners and remaining keys, the consoles'
  instruments, and the keys that drive and fight (T-056).
- What +0x2e is used for once a state replaces +0x30 during a mission (T-053).
- What `DS:0x5ef2` is (cleared by the won routine and the `-` key), and where the
  campaign's calendar (`DS:0x7d4a`, `0x7d4b`) is shown (T-056).
- Whether a picture loader always reads the file: the map room's draw handler skips
  `ui_load_map_room_picture` when bit 7 of the picture descriptor's flags
  (`DS:0x5efd`) is set (presumably: still in memory), which would mean no busy shape (T-049; check in DOSBox how long
  the busy shape shows on R, on the way back from the review and on view switches).
  Whether the dispatcher sends a corner its enter bit when a view opens with the
  pointer already over it (the port waits for the next move into it).
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

T-043 named, in `xeno.exe`: `ui_open_view_of_object` (`1000:7ef2`),
`ui_open_situation_room` (`1000:5457`), `ui_open_humanoid_console` (`1000:9c94`),
`ui_open_scorpion_console` (`1000:7f90`), `ui_open_scout_console` (`1000:b450`),
`ui_view_next_unit` (`1000:7db2`), `ui_view_previous_unit` (`1000:7dd9`),
`sim_get_side_base` (`1000:6845`), `ui_sitroom_roster_on_mouse` (`1000:582d`),
`ui_sitroom_on_key` (`1000:61e3`), `ui_unit_name_button_on_mouse` (`1000:7d81`),
`ui_draw_program_panel` (`1000:d2d1`), `ui_program_panel_on_mouse` (`1000:d34c`),
`ui_humanoid_console_on_key` (`1000:ab64`), `ui_scorpion_console_on_key` (`1000:926d`),
`ui_scout_console_on_key` (`1000:bade`), `order_set_program` (`1000:476f`),
`order_select_next_program` (`1000:4794`), `order_select_previous_program`
(`1000:47b1`), `order_find_program_name` (`1000:47ca`), `order_switch_to_manual`
(`1000:d42c`), `order_switch_to_automatic` (`1000:d446`), `ui_training_button_on_mouse`
(`1000:6e98`), `ui_training_menu_on_key` (`1000:6ee5`) and `ui_run_training_mission`
(`1000:6ecf`); in `mgavideo.dvr` `gfx_recolour_rect` (`0x0ef1`, slot `0x522`) and
`gfx_xor_rect_frame` (`0x106d`, slot `0x536`). All `confirmed`, all saved.

T-044 named, in `xeno.exe`: `ui_draw_review_picture` (`1fe6:77f7`, created),
`ui_draw_review_text_panels` (`1fe6:7874`, created), `ui_review_set_up_page`
(`1fe6:7a12`), `ui_review_step_script` (`1fe6:7927`), `ui_review_on_key` (`1000:7a19`,
created), `ui_review_leave` (`1000:7a3b`, created), `ui_review_next_page` (`1000:7a41`,
created), `ui_review_previous_page` (`1000:7a4c`, created),
`ui_review_next_button_on_mouse` (`1000:79e3`, created), `ui_review_exit_button_on_mouse`
(`1000:79fe`, created), `ui_review_tick` (`1000:7aa2`), `ui_review_reset` (`1000:7ae4`),
`ui_draw_sitroom_main_view` (`1000:5551`, created), `ui_sitroom_set_view_mode`
(`1000:676e`) and `ui_sitroom_type_orders_text` (`1fe6:6fc1`). All `confirmed`, all
saved.

T-045 named, in `xeno.exe`: `ui_play_win_cutscene` (`1000:7059`),
`ui_play_lose_cutscene` (`1000:702d`), `ui_play_campaign_won_cutscene` (`1000:70d0`),
`ui_play_ending_sequence_entry` (`1000:fde8`), `ui_play_triumph_sequence`
(`1000:fd9f`), `sim_get_mission_kills_and_losses` (`1fe6:6b0d`),
`scen_score_mission_result` (`1000:7626`), `txt_print_u16_five_digits` (`1fe6:4a6c`)
and `txt_format_u16_to_buffer` (`1000:02b7`); details in `docs/formats/vcl.md`. All
`confirmed`, all saved.

T-046 named, in `xeno.exe`: `sim_end_mission_with_result` (`1000:68c1`, created),
`ui_map_room_dev_key_won` (`1000:752d`, created), `scen_win_selected_scenario`
(`1000:7536`), `ui_map_room_dev_key_lost` (`1000:75f3`, created),
`scen_lose_loaded_scenario` (`1000:75fc`), `ui_map_room_raise_progress` (`1000:74e5`,
created), `ui_map_room_lower_progress` (`1000:7504`, created),
`ui_map_room_select_first` (`1000:745b`), `ui_map_room_run_mission` (`1000:6f9e`,
created), `ui_map_room_open_training` (`1000:6ff8`, created), `ui_map_room_play_loaded`
(`1000:73fa`, created), `ui_map_room_open_editor` (`1000:6bcf`, created; renamed
`ui_editor_key_toggle_flag` in T-048),
`sim_apply_scenario_alert` (`1000:68a0`), `sim_reset_mission_counts` (`1000:68ae`),
`sim_random_word` (`1000:0283`), `sim_random_below` (`1000:772a`),
`scen_advance_campaign_day` (`1000:7732`), `sim_seed_random_from_timer` (`1fe6:42e8`),
`ui_place_map_tooltip` (`1fe6:74fe`) and `ui_restore_under_map_tooltip` (`1000:77b1`);
in `mgavideo.dvr` `gfx_alloc_rect_save` (0x0cf7, slot `0x50a`), `gfx_save_rect`
(0x0d17, `0x50e`), `gfx_restore_rect` (0x0d99, `0x512`), `gfx_fill_rect` (0x0e6e,
`0x51a`) and `gfx_frame_rect` (0x0fb8, `0x52a`), all created. All `confirmed`, all saved.

T-058 named, in `xeno.exe`: `scen_on_object_loaded` (`1000:7c6b`),
`scen_on_old_layout_object_loaded` (`1000:7c84`), `scen_unpack_all_objects`
(`1000:f109`), `scen_set_up_side_base` (`1000:626d`), `order_pack_script_pointer`
(`1000:4826`), `order_unpack_script_pointer` (`1000:4833`), `sim_init_base`
(`1000:6209`), `sim_init_new_object` (`1000:f812`), `sim_add_unit_to_side_list`
(`1fe6:6e42`), `sim_destroy_robot` (`1000:de51`), and created `sim_damage_object`
(`1000:f84d`), `sim_damage_humanoid` (`1000:af51`), `sim_damage_scorpion`
(`1000:94a0`), `sim_damage_scout` (`1000:c68a`), `sim_damage_base` (`1000:62b7`) and
`sim_damage_relay` (`1000:cfba`); details in `docs/formats/usg.md`. All `confirmed`, all
saved.

T-059 named, in `xeno.exe`: `sim_approx_distance_to_point` (`1000:08f7`),
`gfx_draw_terrain_shape` (`1000:1b03`), `scen_reset_path_tables` (`1000:e9e0`),
`sim_head_for_path_point` (`1000:eb05`), `order_start_path_mission_program`
(`1000:ec4d`), `sim_set_robot_destination` (`1000:46f3`), `sim_run_path_end_routine`
(`1000:ec18`), and created `sim_set_height_to_ground` (`1000:436c`),
`sim_test_below_ground` (`1000:43d6`), `sim_test_point_in_hill` (`1000:4446`),
`gfx_emit_terrain_render_node` (`1000:41c0`), `gfx_draw_hill_outline_on_map`
(`1000:39fb`), `sim_check_robot_against_hills` (`1000:d50f`),
`sim_assign_random_path` (`1000:ea98`), `sim_advance_path_point` (`1000:eae4`),
`sim_join_nearest_path_point` (`1000:eb66`), `sim_assign_paths_to_idle_units`
(`1000:ea3a`) and `sim_count_units_on_paths` (`1fe6:8326`); details in
`docs/formats/usg.md`. All `confirmed`, all saved.

T-052 named, in `xeno.exe`: `sim_move_humanoid` (`1000:ac29`), `sim_move_scorpion`
(`1000:9377`), `sim_move_scout` (`1000:c4f5`), `sim_steer_humanoid` (`1000:ac7a`),
`sim_steer_scorpion` (`1000:93c8`), `sim_steer_scout` (`1000:c549`),
`sim_step_humanoid_speed` (`1000:aca2`), `sim_step_scorpion_speed` (`1000:93f0`),
`sim_step_scout_speed` (`1000:c571`), `sim_step_scout_stance` (`1000:c5a3`),
`sim_limit_speed_by_drive_and_helm` (`1000:d3b2`), `sim_map_scorpion_speed`
(`1000:d3f8`), `sim_update_robot_velocity` (`1000:d41c`), `sim_scale_by_heading`
(`1000:03ad`), `sim_sine_magnitude` (`1000:05e6`), `sim_cosine_magnitude`
(`1000:05e1`), `sim_tick_all_objects` (`1000:f631`), `sim_on_robot_hit_hill`
(`1000:d45c`), `sim_run_path_return_routine` (`1000:ec22`), and created
`sim_tick_humanoid` (`1000:abe0`), `sim_tick_scorpion` (`1000:930a`), `sim_tick_scout`
(`1000:c485`), `sim_tick_base` (`1000:6300`), `sim_run_alien_base_path_phase`
(`1000:ea0e`), `sim_advance_units_asking_next_point` (`1000:ea69`; a 1-byte body, as
Ghidra holds an older instruction inside its first one), `sim_turn_round_on_path`
(`1000:ecb4`) and `sim_take_new_random_path` (`1000:ecbd`); and swapped
`order_switch_to_manual` and `order_switch_to_automatic` (above, "Orders"). Details in
`docs/sim.md`, "Movement". All `confirmed`, all saved.

T-054 named, in `xeno.exe`: `sim_spawn_explosion` (`1000:e67b`),
`sim_init_explosion_from_template` (`1000:e79a`), `sim_alloc_object_slot`
(`1000:f4b1`), `sim_free_object_slot` (`1000:f5cd`), `sim_count_kill_and_loss`
(`1fe6:6af6`), `sim_remove_unit_from_list` (`1fe6:6e71`), and created
`sim_tick_explosion` (`1000:e7e3`), `sim_damage_shot_target` (`1000:e954`),
`sim_find_shot_hit` (`1000:e8ac`), `sim_damage_objects_in_radius` (`1000:e8eb`) and
`sim_step_humanoid_weapons` (`1000:b08a`). Details in `docs/sim.md`, "Damage". All
`confirmed`, all saved.

T-055 named, in `xeno.exe`: `sim_step_base_end_script` (`1000:e691`),
`sim_spawn_scattered_explosion` (`1000:e714`), `sim_lower_explosion_randomly`
(`1000:e751`), `snd_play_base_end_step_sound` (`1000:e708`), `snd_play_sound_effect`
(`1000:ee90`), and created `sim_tick_relay` (`1000:ccd9`; a 1-byte body, as Ghidra
holds an older instruction inside it). Details in `docs/sim.md`, "The mission's end".
All `confirmed`, all saved.

T-041 named the name entry's routines (`ui_draw_name_entry`, `ui_name_entry_on_key`,
`ui_name_entry_type_char`, `ui_name_entry_backspace`, `ui_name_entry_accept`,
`ui_name_entry_cancel`) and the text services; the list is in
`docs/re/dvr-protocol.md`.

T-049 named, in `xeno.exe`: `ui_load_map_room_picture` (`1000:71d3`),
`ui_load_training_picture` (`1000:6e7f`), `ui_load_sitroom_picture` (`1000:53fa`),
`ui_load_review_picture` (`1fe6:782c`) and `ui_run_file_dialog` (`1fe6:83db`), and
created `ui_load_scorpion_console_picture` (`1000:7fb3`),
`ui_load_humanoid_console_picture` (`1000:9cb7`), `ui_load_scout_console_picture`
(`1000:b473`), `ui_sitroom_corner_on_mouse` (`1000:5c30`) and
`ui_console_corner_on_mouse` (`1000:7cbf`); in `mgavideo.dvr` the pointer routines
listed in `docs/re/dvr-protocol.md`. All `confirmed`, all saved.

T-048 named, in `xeno.exe`: `file_write_scenario` (`1000:efde`), `scen_write_shdr`
(`1000:f05b`), `scen_write_mapp` (`1000:f32e`), `scen_write_dcbs` (`1000:f09a`),
`scen_count_dcbs_size` (`1000:f255`), `scen_pack_all_objects` (`1000:f0f4`),
`scen_write_apth` (`1000:f11e`), `scen_write_hpth` (`1000:f12d`), `scen_write_ordr`
(`1000:f2da`), `scen_write_bgnd` (`1000:f3b3`), `scen_write_term` (`1000:f13c`),
`sim_find_object_slot` (`1000:f656`), `ui_call_open_file_dialog` (`1000:fb6e`),
`ui_call_save_file_dialog` (`1000:fb78`), `ui_map_pointer_to_world` (`1000:3fd3`),
`ui_editor_get_current_path` (`1000:6d06`), `dbg_toggle_unused_editor_flag`
(`1000:4856`), `ui_reset_editor_state` (`1000:690d`), `dbg_print_number_at`
(`1000:6a79`), renamed `ui_map_room_open_editor` to `ui_editor_key_toggle_flag`
(`1000:6bcf`), and created `ui_save_scenario_with_dialog` (`1000:efd5`),
`ui_load_scenario_with_dialog` (`1000:eeec`) and `ui_editor_place_at_pointer`
(`1000:6be8`); in `mapdvr.dvr` `ui_save_map_with_dialog` (`0x0705`),
`file_write_map_circles` (`0x078c`), `scen_import_terrain_circle` (`0x0631`),
`mem_reset_circle_buffer` (`0x0839`), `file_create_map` (`0x074e`), and created
`ui_enter_map_editor` (`0x006e`), `ui_exit_map_editor` (`0x011a`),
`gfx_emit_map_editor_render_nodes` (`0x01c9`) and `ui_map_editor_on_key` (`0x032a`).
All `confirmed`, all saved.

T-057 named, in `xeno.exe`: `ui_map_room_save_if_loaded` (`1000:7423`),
`scen_load_scenario_table` (`1000:712e`, the log-in's new campaign: `scendef.bin`
through slot `0x2b0`, then the index) and `mem_get_block_paragraphs` (`1fe6:1c7b`).
All `confirmed`, all saved.
