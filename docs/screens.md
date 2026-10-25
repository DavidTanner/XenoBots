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
| Alt+F1 | | The scenario editor, only with unlock word 9 (`ui_map_room_open_editor`, `1000:6bcf`) |

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
the developer keys' handlers past their unlock check. Who calls `1000:68c1` and who sets
those flags (presumably the player's base or command object) is not traced (T-055): no
instruction names it, so it is reached through a table. Nothing else writes 1 or 2 to
`DS:0x5fab`. Both routines first return at once if `DS:0x5fab` is 3 (the training
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
  one, as above, outside any mission and with no cutscene. Alt+F1 opens the scenario
  editor (`1000:4856`).

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
- **Not ported yet.** The situation room's main view picks its shape by the view mode
  (a word table at `DS:0x5a14`, by the mode byte) while the pointer is over the
  view's region, and 0 off it; each console does the same over its instruments
  (shapes 3, 4 or 6 by the unit's state). They need the views' regions (T-056).
  `ui_run_file_dialog` (`1fe6:83db`) shows shape 0 while a file dialog runs and puts
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
| `0x7e`, `0x60`, `0x62`, `0x7c` | (84, 17, 89, 65), (107, 16, 145, 83), (123, 162, 177, 240), (152, 103, 157, 143) | Panels drawn from the selected unit and the clock (status, detail); not traced |
| `0x72`, `0x76`-`0x7a`, `0x74` | three rows at 159, 167, 175 from x 103 to 146 | Left arrow, value, right arrow: while an arrow is held, the base's +0x2d is 1, 3 or 5 for the row (presumably the camera; not traced) |
| `0x04` x2 | (89, 284, 131, 318), (142, 254, 182, 306) | Groups of widgets (types `0x64`-`0x70`) shown in map mode only: zoom (+ and -, base +0x36, 0 to 6) and map overlays (bits of +0x32) |
| `0x5a`, `0x5c`, `0x5e` | (170, 29, 178, 66), 8 rows lower each | Sprite buttons: launch the selected unit from the base (`1000:6716`, only for a unit in the base: it is put at the base's position and marked in the field), recall it (sets its +0x23 bit 3), and end the mission (the quit flag `DS:0xe87e`) |

A sprite button is drawn opaque with handle `base + [+0x40]`: a press or release on
any of them flips the shared +0x40 between 0 and 8, so, as in the map room, all of
them show their pressed sprite (the next one) together. Their handles, in order, are
0x2a0, 0x2b0, 0x2c0, 0x3f8 and 0x408 (the arrows), 0x2d0, 0x2e0, 0x2f0. The release
acts. The corner hot spot (`0x15a`) ends the mission too.

Situation room keys (`DS:0x5b20`): Alt+Q ends the mission (ESC is listed but its
handler only acts on Alt+Q); Space and J view the selected unit; Tab and Alt+Tab step
the roster selection; O, S, M, R, C the main view's mode; F3 recalls the selected
unit; = and - (and keypad + and -) zoom the map; 1 to 6, B, G, I, N, U and F1, F2 act
on the mission (not traced); Ctrl+Alt+P, S and F are developer keys.

*The consoles* share most of their layout but not their positions. All three have:

| Type | Humanoid | Scorpion | Scout | What |
|---|---|---|---|---|
| `0x154` | (91, 126, 98, 194) | (154, 4, 161, 56) | (112, 18, 119, 59) | **The program panel**: the robot's program, see "Orders" |
| `0x156` | (185, 273, 200, 318) | the same | the same | The unit-name button: the robot's call sign in font 0, colour 0, at (279, 191); left or right release: next or previous unit (pressed look: slot `0x536`, the rectangle's frame XORed with 8) |
| `0x152` | (7, 7, 36, 102) | (20, 112, 51, 208) | (5, 5, 34, 100) | The robot's message panel (object +0x5e), framed and filled when it changes |
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
5 (mines and buoys), `=` and `-`, and per robot letters drive and fight (T-056);
Ctrl+P and Ctrl+Alt+S are developer keys.

### Orders (T-043)

A robot's **orders** are a program, one of 31 scripts in `xeno.exe`, and a switch
between the program (automatic) and the pilot (manual). There is no other order
parameter in the views: no waypoint or target is given in a console or the situation
room (the scripts that go to a unit or a place use the robot's destination and target
fields, set by the mission; T-053).

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
| +0x31 | 10 when a program starts |
| +0x33, +0x38 | 0, and a start phase from a counter (`DS:0x4db5`, stepping by 10 modulo 59) so robots given programs together do not run in step |
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

**The robot.** `ui_review_tick` (`1000:7aa2`) runs once per change of the host's tick
counter `DS:0x374`: it counts down the current script step and, at 0, runs the next one
(`ui_review_step_script`, `1fe6:7927`), then calls the step's per-tick routine and the
page's parts routine. A script is a list of words: an opcode (0 to 16, handlers by
opcode at `1fe6:7948`, per-tick routines at `1fe6:795c`) and its operands: 0 moves
(for `n` ticks, adding a step to the position `DS:0x61ac` each tick), 2 waits `n`
ticks, 4 jumps to an offset in the script (the loop), 6 and 8 turn one way or the
other (adding or subtracting a step to the facing `DS:0x61b6`), 10 and 12 switch the
parts routine between two per page (tables at `1fe6:79d8` and `1fe6:7a04`: presumably
walking and standing), 14 and 16 do nothing. The render hook (`1000:7a5f`) then adds one
render node with that type, model, position, facing and part bytes, and the 3D
renderer draws it as it draws a mission's units (`mxx.md`, "Drawing"). Drawing it
needs the renderer's projection and scaled model blit (T-056), so the port leaves it
to T-047.

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
| Review | `review.m@l`, the page's text panels, a placeholder for the robot (T-044) | The user's key table and buttons: Tab and the next button the next page, Alt+Tab the previous; ESC, Alt+Q and the exit button: map room |
| Mission | `sitroom.m@l` or a console's `.m@l`, by the object viewed (T-043) | The views' own key tables and widgets (below, "Mission views"); Ctrl+W won, Ctrl+L lost, Ctrl+C campaign won (placeholders) |
| Cutscene (won, lost, campaign won) | The original's cutscene in the 320x240 mode (T-045, `vcl.md`), played by the window | Any key but P skips it, P pauses; then the map room |

Keys that only the port uses (the mission's endings) are placeholders until the
mission runs (T-055). A training mission goes back to
the map room without a cutscene, as in the original.

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
  the mission views, moving into a corner picks shape 2 and moving out picks 0; a view
  that opens starts outside its corner. The main view's and the instruments' shapes
  wait for their regions (T-056).
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
  the scenario to 12. A mission ended with Ctrl+W or Ctrl+L (the placeholders for the
  mission's own end, T-055) runs the won or lost routine with 0 kills and 0 losses, so
  a win scores the bonus alone (1000) and the `WIN` caption shows those numbers; the
  cutscene is then chosen as `ui_map_room_run_mission` does (campaign won, lost, won,
  else none). Ctrl+C, the port's stand-in for the last win, wins the scenario and sets
  the campaign-won flag. The training menu sets the result to 3 and clears the
  selection and the loaded scenario. The developer keys (`=`, `-`, Ctrl+W, Ctrl+L in
  the map room) work only when the user's `XENO.KEY` gives unlock word 8 the value the
  user's `xeno.exe` checks; Alt+F1 with word 9 logs that the editor is not ported
  (T-048). Ctrl+Space runs the mission again with no check. The hover label follows
  the pointer over the boxes (none while a button is held, cleared off the boxes and
  by a release that selects nothing), placed, filled, framed and typed as described
  above in the user's font 2; the campaign-won box and message, and the prompt with
  nothing selected, are drawn with the panels. The port redraws the whole map room
  instead of saving and restoring the pixels under the label, and draws the label over
  the boxes every time. The random word is the original's shift register
  (`xeno_sim::Random`, T-050), seeded from the run's seed (`--seed`, else the clock)
  instead of the timer; there is one for the run, which a mission's sim takes and gives
  back and a second log-in carries on (T-051, `docs/sim.md`). The calendar's day advances but is not shown. Not done: the
  save (T-057).
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
  roster rows show each robot's number and program, the selected one framed; the
  unit-name button shows the robot's number. `--screen humancon`, `scorpcon` and
  `scoutcon` open the first training mission, in the menu's order, with one of the
  player's robots of that kind, on that robot; `--screen sit-room` the first training
  mission.
- **Briefing** (`crates/xeno-app/src/mission.rs`, T-044): as "The briefing (T-044)"
  above. A mission keeps its file's `ORDR` text and starts in the main view mode saved
  in the player's base (+0x37). O, S, M, R and C (the user's key table, by the
  original's handlers) set the mode; in the orders mode the main view's colour-255
  pixels are recoloured to 0x10 and the text is typed from (96, 30) in the user's font
  4, colour 0x1d, one byte per retrace, until its NUL or 0x1a, CR LF as the font's
  control codes. O, and coming back to the situation room, start it again. The other
  modes keep the T-056 placeholder. The speed (one byte a retrace) is the port's
  reading of "one per pass of the mission loop"; check it in DOSBox.
- **Review** (`crates/xeno-app/src/review.rs`, T-044): as "The review (T-044)" above,
  from the tables `xeno_formats::review` reads (kept in `maproom::Assets`). The map
  room's R opens it at once with its palette, on the first page; each page fills the
  three panels with colour 0 and draws its texts transparent in the user's font and
  colour; Tab, Alt+Tab and a left release on the next button turn the pages, wrapping;
  ESC, Alt+Q and a left release on the exit button go back to the map room, which fades
  in again. The buttons' frames are XORed with 9 on a press and back on a release over
  them (a release elsewhere leaves them XORed). The robot's view (the picture's
  colour-255 pixels) is hatched with a "T-047" label.
- **Cutscenes after a mission** (T-045): as `vcl.md`, "In the port". The lose picture
  is chosen by the robot console the mission last opened (`Screens::console`, reset
  when the map room plays a scenario); the win caption shows the numbers the won
  routine returns (T-046): 0 kills, 0 losses and the bonus as points until the
  mission counts them.
- **Not modelled**: the save file, the review's turning robots (T-047) and the music.
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

- What slot `0x5b4` does exactly (the orders view's clear, presumably filling the
  view's region with one colour), and how often the mission loop passes, which sets
  the orders' typing speed (T-044; check in DOSBox).
- The review's 3D view: the render hook's node layout, the due check `1fe6:39c3`, the
  parts routines and the scripts' steps in ticks (T-047, with T-056).
- What reaches `sim_end_mission_with_result` (`1000:68c1`, through a table) and sets
  the object's flags +0x16 bits 0 and 3 and its end script +0x30, which decide when a
  mission ends won or lost (T-055). The quit flag `DS:0xe87e` is also set at
  `1000:424b`, `5c25`, `5c50` and `7cf9` (not traced).
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
(`1000:73fa`, created), `ui_map_room_open_editor` (`1000:6bcf`, created),
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
