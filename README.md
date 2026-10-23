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
numbers in your `vgaintro.dvr`. There is no music, caption or credits text yet. Any
key ends the cutscene early with a fade and skips the drop; any key ends the drop at
once; P pauses either until the next key. `--no-intro` skips both and `--drop` plays
the drop alone. Then the window shows a test card (a synthetic pattern). Its white marker moves one pixel per frame of the
original 70 Hz display and its yellow one at 60 Hz, whatever your monitor's refresh
rate; the blue grid lights a cell for each key held. Press ESC or close the window to
quit, and F11 or Alt+Enter to switch between a window and fullscreen.

| Flag | Meaning |
|---|---|
| `--game-dir <path>` | Where the original game files are. |
| `--check` | Only check the game directory (and decode the opening cutscene and the drop), then exit without opening a window. |
| `--no-intro` | Skip the opening cutscene and the drop. |
| `--drop` | Play the intro drop alone (from the point the cutscene would have ended), then the test card. |
| `--scale <auto\|n>` | `auto` (default): the largest picture that fits the window. `n` (1 to 32): that horizontal factor, the window sized to fit. |
| `--aspect <correct\|exact\|off>` | `correct` (default): the 4:3 CRT shape within 5%. `exact`: the closest shape, even if smaller. `off`: square pixels. |
| `--fullscreen` | Start in borderless fullscreen on the current monitor (the display mode is not changed). With `--scale n` the picture keeps that scale, centred. |
| `--view <file>` | Show one file instead of the cutscene and test card (see "Asset viewer" below). A bare name such as `msprite1.b@n` is looked up in the game directory, in any case; anything else is read as a path. With `--check`, the file is decoded but no window opens. |
| `--palette <file>` | With `--view`: take the colours from this `.m@l`, `.vcl` or `.pcx`, found the same way. |

| Key | Action |
|---|---|
| ESC | Leave fullscreen; in a window, quit. |
| Any other key, P | During the cutscene: end it (fading out) and skip the drop, or pause it until the next key. During the drop: end it at once, or pause it. |
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

| Key | Action |
|---|---|
| Right or Page Down, Left or Page Up | Next or previous sprite (stops at the ends) |
| Home, End | First or last sprite; for a picture, the left or right edge |
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
`xeno.exe` the opening cutscene's sequence entries are listed.

```bash
cargo run -p xeno-tool -- inspect --all ./gameFiles
```

```bash
cargo run -p xeno-tool -- dump ./gameFiles/xenodrop.pcx --out /tmp/xeno-out
```

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
