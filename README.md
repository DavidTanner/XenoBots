# XenoBots

A Rust reimplementation of **Xenobots** (released in North America as **Ultrabots**,
NovaLogic / Electronic Arts, 1993), the DOS mech-strategy game where you program
squads of robots and watch them fight. The aim is a native, cross-platform build that
plays the original game from your own copy of its files, and that scales cleanly to 4K
displays instead of being a blurry stretch of 320x200.

> **Status: scaffolding.** The Cargo workspace builds, but the game is not playable
> yet. The task
> backlog in [NEXT_STEPS.md](NEXT_STEPS.md) describes how the code will come together.
> The commands below are the intended interface and will start working as those tasks
> land.

## You need your own copy of the game

Xenobots / Ultrabots is closed source and still under copyright. **This repository does
not contain, and will never contain, any game data.** You need an installed copy of the
game, the same ~130 files that the original `install.bat` placed in `C:\XENOBOT`
(`xeno.exe`, `*.dvr`, `*.usg`, `*.vcl`, `*.pcx`, `*.ms3`, ...), roughly 2.5 MB in all.

Place those files in a directory and point the program at it, in one of three ways:

| Method | Example |
|---|---|
| Default directory | copy the files into `./gameFiles/` (this path is gitignored) |
| Environment variable | `export XENOBOTS_GAME_DIR=/path/to/xenobot` |
| Command-line flag | `cargo run -p xeno-app -- --game-dir /path/to/xenobot` |

The flag wins over the environment variable, which wins over the default. The program
checks for the files at startup, ignoring the case of file names, and tells you which
ones are missing.

## Prerequisites

- **Rust** stable, 1.88 or newer. Install with [rustup](https://rustup.rs):

  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

  The repo's `rust-toolchain.toml` selects the stable channel automatically.
- **Linux only:** development headers for windowing and audio. On Debian/Ubuntu:

  ```bash
  sudo apt install libasound2-dev libxkbcommon-dev libwayland-dev libx11-dev
  ```

- **macOS and Windows:** nothing beyond Rust.

No GPU is required. The first renderer is a plain CPU framebuffer.

## Build and run

```bash
cargo build --release
```

```bash
cargo run --release -p xeno-app -- --game-dir ./gameFiles
```

For now this checks the game files, then opens a window and plays the opening
cutscene, as the original does at start-up: `ealogo.vcl`, eight story pictures and
`presents.vcl`, about two minutes, timed and faded from the sequence data
in your `xeno.exe`. Then comes the intro drop, as in the original: a scroll over
`xenodrop.pcx` and then the cave (`cavedrop.pcx`), with the `.fgd` ridges moving in
front at their own speeds and the palette cross-faded, about 45 seconds, run from the
numbers in your `vgaintro.dvr`, with the credits in the game's own font from your
`mgavideo.dvr`. Each picture's caption is typed in the game's font, as in the
original; there is no music yet. Any
key ends the cutscene early with a fade and skips the drop; any key ends the drop at
once; P pauses either until the next key. `--no-intro` skips both and `--drop` plays
the drop alone. Then come the game's screens in the original's order: the log-in, then
the map room, from which you play a mission, open the training menu or the review, log
in again or quit. The log-in works as the original's: type a name of up to eight
letters and digits (Backspace corrects, the cursor blinks), Enter logs in, ESC quits
(after N, ESC goes back to the map room). Your name is remembered and a new campaign
starts from `scendef.bin`; saves are neither read nor written yet. The map room works
as the original's too: each scenario's box in the look of its state (scenarios beyond
your progress shown as unavailable), your name and score, and the five buttons; pick
a scenario by clicking its box (on the release; the right button also plays it) or
with Tab and the arrow keys, and its name and alert level show at the bottom. Playing
loads that scenario's `scenNNa.usg` and opens the mission in the situation room; the
training menu's buttons and keys load its training missions the same way. The mission
itself does not run yet: its views work as the original's (the situation room, the
view of your base, and a console for each of your robots), and you can set each
robot's orders on its console, but the 3D views, scanners, instruments and the
roster's contents are placeholders marked "T-056". The situation room's orders view
(O) types the scenario's briefing from your `.usg` file, as the original does. The
review is the original's reference of the robots, with its pages, texts, keys and
buttons from your `xeno.exe`, and each page's robot walks and turns in its view by
the original's scripts.
A mission ends won or lost when a base is destroyed, after the base's end (about 7.5
seconds of explosions): the alien base won, yours lost. Robots lock on and fire at each
other (Space fires the viewed robot's selected weapon), and at bases, which take 300
points. To win `NTRAIN` by play, for example: leave the first scorpion to its program
for about 26 minutes, then open its console, Up a few times for speed, F6 for the tail
gun, steer at the alien base with Left and Right, stop near it with `.`, lock what comes
in range with A and fire with Space, and near the base press A until the base is locked
(about 31 minutes in all). To lose `HTRAIN`, park each of your units just behind your
base, away from the aliens, with its console's arrows, Up and `.`: the aliens' shots at
them hit the base (about 15 minutes). Robots off their side's relay net run on their
batteries, so aliens that attack your units on your net run dry. Ctrl+C ends a mission
with the campaign won (a placeholder). Then the original's cutscene plays from your files, `win.vcl`,
`slose.vcl` or `nlose.vcl` (by the robot console you opened last), or `triumph.vcl`,
with its caption and the mission's kills, losses and points; after a training mission,
the map room. Any key but P skips it, P pauses it, and it goes back to the map room. The
window title names the screen and its keys. The mouse pointer is the original's, from
your `mgavideo.dvr` (the system cursor is hidden over the picture). See
`docs/screens.md`.

| Screen | Keys |
|---|---|
| Log-in | Letters and digits: the name. Backspace: delete. Enter: log in (map room). ESC: quit, or back to the map room after N |
| Map room | Click a box, or Tab, Right or Down / Alt+Tab, Left or Up: select the next / previous scenario. P, Enter or Space (or a right click on a box): play it. T: training menu. R: review. N: log-in. Q or Alt+Q: quit. The buttons do what their keys do |
| Training menu | Click a button, or H, N, S or C: training mission. ESC or Alt+Q (or the last button): back |
| Review | Tab or the next button (left click): next page. Alt+Tab: previous page. ESC, Alt+Q or the exit button: back to the map room |
| Mission, situation room | Click a roster row (left) or Tab / Alt+Tab: select a unit. Space or J, the third button below the roster, or a right click on a row: that unit's console (not while it is in the base). F3 or the middle of the three buttons at the bottom left: recall the unit to its base, which takes it in and repairs it. F2 or the top one: launch it from the base. The arrow buttons scroll the roster. O: the orders view, which types the scenario's briefing (S, M, R, C pick the other views, placeholders). Alt+Q, the bottom button or the top-left corner: end the mission (back to the map room). A destroyed base's explosions are heard (T-061), and about 7.5 seconds later the mission ends won or lost and plays its cutscene (a training mission goes straight back). Ctrl+C: end it with the campaign won (a placeholder) |
| Mission, robot console | Click the program panel: left the next program, right the previous (and the robot back to its program). F4: next program. F2: automatic (the program drives). F3: manual (the pilot drives). Space: fire. A: lock the next enemy in range; R: drop the target. In the 3D view or the second display, a left click on a robot locks it; a right click on one of yours opens its console. With the second display on the map (M), Alt+click on an object makes it the unit's goal, and on empty ground sets its destination. Tab / Alt+Tab or the bottom-right button (left / right): next / previous unit, skipping units in the base; a unit taken in turns its console to the situation room. ESC, Alt+Q or the top-left corner: situation room. Ctrl+C as above |
| Cutscene after a mission | Any key but P (ESC too): skip it, then the map room. P: pause until the next key |

`--screen test-card` shows a test card (a synthetic pattern) instead. Its white marker moves one pixel per frame of the
original 70 Hz display and its yellow one at 60 Hz, whatever your monitor's refresh
rate; the blue grid lights a cell for each key held, green or red for player 1's or
2's stick keys (the defaults in your `config.dvr`, or the keys you set with the original
setup program, saved in `XENO.CFG`; the original game itself never uses them). Close the window to
quit, and press F11 or Alt+Enter to switch between a window and fullscreen.

| Flag | Meaning |
|---|---|
| `--game-dir <path>` | Where the original game files are. |
| `--check` | Only check the game directory (and decode the opening cutscene, the drop and the screens), then exit without opening a window. |
| `--no-intro` | Skip the opening cutscene and the drop. |
| `--drop` | Play the intro drop alone (from the point the cutscene would have ended), then the log-in screen. |
| `--screen <name>` | For development: skip the cutscene and drop and start at a screen: `login`, `map-room`, `training`, `review`, `sit-room`, `humancon`, `scorpcon`, `scoutcon`, `won`, `lost`, `triumph` (the cutscene after a mission, then the map room), or `test-card` for the test card instead of the screens. |
| `--scale <auto\|n>` | `auto` (default): the largest picture that fits the window. `n` (1 to 32): that horizontal factor, the window sized to fit. |
| `--aspect <correct\|exact\|off>` | `correct` (default): the 4:3 CRT shape within 5%. `exact`: the closest shape, even if smaller. `off`: square pixels. |
| `--fullscreen` | Start in borderless fullscreen on the current monitor (the display mode is not changed). With `--scale n` the picture keeps that scale, centred. |
| `--view <file>` | Show one file instead of the cutscene and screens (see "Asset viewer" below). A bare name such as `msprite1.b@n` is looked up in the game directory, in any case; anything else is read as a path. With `--check`, the file is decoded but no window opens. |
| `--palette <file>` | With `--view`: take the colours from this `.m@l`, `.vcl` or `.pcx`, found the same way. |
| `--seed <n>` | The seed of the game's random word (the campaign's alert rises and calendar). By default it comes from the clock, as the original's comes from the timer; it is logged at start-up, and passing it again repeats the run. |

| Key | Action |
|---|---|
| ESC | Leave fullscreen; in a window, quit. Screens that use ESC themselves (log-in, training menu, the mission's views, review, the cutscenes after a mission) take it instead. |
| Any other key, P | During the opening cutscene: end it (fading out) and skip the drop, or pause it until the next key. During a cutscene after a mission: the same, then the map room. During the drop: end it at once, or pause it. |
| F11, Alt+Enter | Switch between a window and fullscreen. On a Mac, F11 may need Fn; the green window button also works. |

Run with `RUST_LOG=xeno_app=debug` to log the measured game rate once a second.

### Asset viewer

`--view` shows a decoded file through the game's own framebuffer and scaler, as a visual
check of the parsers:

```bash
cargo run --release -p xeno-app -- --game-dir ./gameFiles --view msprite1.b@n
```

| File | Shown as | Palette |
|---|---|---|
| `msprite0`..`msprite5.b@n` | One sprite at a time, centred on a 320x200 checkerboard of the palette's darkest and brightest colours; the window title gives its number and size | The screen the bank is loaded with: `msprite0` and `msprite1` the situation room, `msprite2`..`4` the Humanoid, Scorpion and Scout cockpits, `msprite5` the map room |
| `sky_a`..`sky_h.b@n` | The 342x54 strip on a 342x200 surface, panned | The Humanoid cockpit |
| `.m@l` | The 320x200 screen | Its own |
| `.vcl` | The 640-wide picture in the 320x240 mode (59.94 Hz), panned | Its own |
| `.pcx` | The 768-wide backdrop on a 768x200 surface, panned | Its own |
| `.m08`, `.m16`, `.m32` | A unit model: one frame of one facing (or of every animation at once, by depth, as the game draws a unit), at the centre of the checkerboard, drawn with the game's scaled model blit; parts from the model's `.m00` are found next to the file | The model's own cockpit: `scorpion` the Scorpion's, `scout` the Scout's, the rest the Humanoid's |
| `.m00` | A model's direction-free images, one at a time, centred | As above |

| Key | Action |
|---|---|
| Right or Page Down, Left or Page Up | Next or previous sprite (stops at the ends) |
| Home, End | First or last sprite; for a picture, the left or right edge |
| Page Down, Page Up | A model's next or previous facing (64ths of a turn, in the title) |
| Down, Up | A model's next or previous animation; past the last, all of them at once |
| Right, Left | A model's next or previous frame (an animation with fewer stays on its last), or a `.m00`'s next or previous image |
| `=`, `-`, Home | A model larger or smaller by 32/256, or back to 1:1 (32/256 to 1024/256) |
| Space | Draw sprites opaque, or with colour 0 transparent so the checkerboard shows through |
| Arrows | Pan a wide picture one pixel per retrace while held |
| F | Fade the palette out to black over 32 retraces |
| I | Fade the palette back in from black over 32 retraces |
| R | Start or stop rotating colours 16..31 through the file's own colours, one place every 4 retraces |

ESC, F11 and Alt+Enter work as above.

On a high-DPI (Retina) display the picture is drawn in the display's real pixels, so
the default window gets a larger scale there, not a blurred one.

Planned options (see `cargo run -p xeno-app -- --help`):

| Flag | Meaning |
|---|---|
| `--filter <nearest\|xbrz>` | Upscaling filter. `nearest` is pixel-exact; `xbrz` smooths edges without blurring. |
| `--hd-pack <path>` | Optional directory of higher-resolution replacement assets (not provided here). |

## Developer tool

`xeno-tool` is a command-line helper for examining the original files while the
formats are being reverse engineered. Output goes to a directory you choose; keep it
outside the repo or under a gitignored path.

```bash
cargo run -p xeno-tool -- inspect ./gameFiles/scen00a.usg
```

`inspect --all` runs every file in a directory through its parser and prints a table of
results per extension. It exits non-zero only if a file that has a parser fails to
parse; files with no parser (the unpacked driver modules, text) are counted, not
failed. The LZEXE-packed `xeno.exe` and `config.dvr` are unpacked in memory, and for
`xeno.exe` the opening cutscene's sequence entries are listed; `vgaintro.dvr` and
`mgavideo.dvr` (its fonts and colour-animation list) have parsers of their own.

```bash
cargo run -p xeno-tool -- inspect --all ./gameFiles
```

```bash
cargo run -p xeno-tool -- dump ./gameFiles/xenodrop.pcx --out /tmp/xeno-out
```

`dump ./gameFiles/mgavideo.dvr` writes one sheet per font (codes 0 to 127, rows of 16).

```bash
cargo run -p xeno-tool -- palette ./gameFiles/ealogo.vcl
```

## Tests and checks

```bash
cargo test --workspace
```

Tests run without game files. Tests that need real assets are ignored by default and
only run when `XENOBOTS_GAME_DIR` is set:

```bash
XENOBOTS_GAME_DIR=./gameFiles cargo test --workspace real_assets -- --ignored
```

Lint and format:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo fmt --all -- --check
```

## How 4K is handled

The game draws into a logical framebuffer at the original resolution with the original
256-colour palette, exactly as the DOS version did. A separate presentation step scales
that framebuffer to your window: integer nearest-neighbour scaling with letterboxing
and 4:3 aspect correction by default, so a 3840x2160 display shows a crisp 8x10 image
(each 320x200 pixel 8 wide and 10 tall) rather than a smeared one. Later milestones
add edge-aware scalers and an optional user-supplied HD asset pack, with the sprite
pipeline designed so higher-resolution art can replace the originals one asset at a
time.

## Project layout

```
AGENTS.md        rules and context for coding agents (and humans)
CLAUDE.md        Claude Code specifics; imports AGENTS.md
NEXT_STEPS.md    task backlog, one small reviewable task per entry
docs/done-tasks.md  finished tasks, moved out of the backlog
docs/formats/    notes on each original file format as it is decoded
crates/          Cargo workspace members (created as tasks land)
gameFiles/       your copy of the game; gitignored, never committed
```

## Contributing

Pick a task from [NEXT_STEPS.md](NEXT_STEPS.md), keep the change to that one task, and
update the task's status in the same pull request. Read [AGENTS.md](AGENTS.md) first;
the rule about never committing game-derived data applies to everyone.

## License

The code in this repository is licensed under the terms in [LICENSE](LICENSE).
Xenobots / Ultrabots and all of its assets remain the property of their rights holders;
nothing here grants any rights to them.
