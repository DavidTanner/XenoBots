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

Planned options (see `cargo run -p xeno-app -- --help` once implemented):

| Flag | Meaning |
|---|---|
| `--game-dir <path>` | Where the original game files are. |
| `--fullscreen` | Start in borderless fullscreen on the current monitor. |
| `--scale <n>` | Integer scale factor for windowed mode. Default picks the largest that fits. |
| `--aspect <auto\|square\|4:3>` | Pixel aspect correction. `auto` reproduces the 4:3 CRT look. |
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
parse; files with no parser (the executable, the driver modules, text) are counted, not
failed.

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
and 4:3 aspect correction by default, so a 3840x2160 display shows a crisp 6x image
rather than a smeared one. Later milestones add edge-aware scalers and an optional
user-supplied HD asset pack, with the sprite pipeline designed so higher-resolution
art can replace the originals one asset at a time.

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
