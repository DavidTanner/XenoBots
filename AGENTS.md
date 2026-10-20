# AGENTS.md

Guidance for any coding agent (Claude Code, Codex, Cursor, Copilot, etc.) working in
this repository. `CLAUDE.md` imports this file and adds Claude-specific notes.

## What this project is

A from-scratch Rust reimplementation of **Xenobots** (sold in North America as
**Ultrabots**, NovaLogic / Electronic Arts, 1993), a DOS mech-strategy game. The goals,
in priority order:

1. **Runs anywhere.** One `cargo run` on macOS, Linux and Windows. No DOSBox, no
   emulator, no platform-specific build steps.
2. **Faithful first.** The original 320x200 (or 320x240 Mode-X) 256-colour
   presentation and game rules must be reproduced before anything is "improved".
3. **4K without losing detail.** A resolution-independent renderer, and later optional
   higher-detail assets, so the game fills a 3840x2160 display cleanly instead of being a
   blurry stretch.

The project is at the **scaffold stage**: the Cargo workspace exists with a
placeholder `xeno-app` binary. Read `NEXT_STEPS.md` for the task backlog and pick
tasks from there.

## Hard rules

### 1. Never commit game data

The game is closed source and copyrighted. **Nothing derived from the original game
may be committed**, in any form:

- The original files in `gameFiles/` (already in `.gitignore`; keep it that way).
- Extracted, converted or re-encoded assets (PNG dumps, decoded sprites, WAVs, MIDI,
  palettes, level data, strings tables).
- Hex dumps or byte arrays of game files pasted into source, tests, docs or commit
  messages. Short *structural* notes ("offset 0x10 is a u16 LE frame count") are fine.
  Copying a run of bytes long enough to reconstruct content is not.
- Test fixtures built from game files. Tests use **synthetic** fixtures that the test
  itself constructs, or are gated behind an env var pointing at a local copy (see
  "Testing" below).
- Disassembly or decompiled output of `xeno.exe` / `*.dvr`. Write up what you *learned*
  (format layouts, algorithms, constants) in prose or Rust, not the listing itself.
- Any user-made HD asset pack. Those live outside the repo and are loaded at runtime.

Before every commit, run `git status` and check nothing under `gameFiles/`, `out/`,
`extracted/` or similar is staged. If `.gitignore` needs a new pattern, add it in the
same change.

### 2. Users bring their own copy

The program must locate game files at runtime from a directory the user supplies
(`--game-dir`, `XENOBOTS_GAME_DIR`, or `./gameFiles` as a default) and fail with a
clear, actionable message if they are missing. Never download, embed or ship them.

### 3. Small, reviewable changes

- One task from `NEXT_STEPS.md` per branch/PR. If a task grows, split it and add the
  remainder back to the backlog.
- A change should be readable in one sitting: roughly under 400 lines of diff excluding
  generated code and lockfiles.
- Update `NEXT_STEPS.md` in the same PR: mark the task done, add follow-ups you found.
- Don't refactor unrelated code in a feature PR.
- When a task is finished (checks pass, `NEXT_STEPS.md` updated), give the user a
  pull request description they can copy and paste, even if you don't open the PR.
  Put it in one fenced `markdown` block (use a four-backtick fence if it contains code
  fences) with:
  - **Title:** imperative, ending with the task id, e.g. `... (T-004)`.
  - **Summary:** what changed and why, in a few sentences.
  - **Changes:** the files or areas touched, one bullet each.
  - **Testing:** the commands you ran and their results, including whether the
    `real_assets` tests were run against a local copy.
  - **Notes for review:** guesses still to confirm, deviations from the task or from
    these rules (such as a diff over ~400 lines), and follow-up tasks you added.
  - **Checklist:** no game data or derived data included; `NEXT_STEPS.md` updated.

## Tech stack (pinned by policy, update deliberately)

Versions below were checked against crates.io on 2026-10-20. When bumping, bump in a
dedicated PR and note it in `NEXT_STEPS.md`.

| Concern | Choice | Version | Notes |
|---|---|---|---|
| Toolchain | stable Rust via rustup | 1.99 (MSRV 1.88) | `rust-toolchain.toml` pins `channel = "stable"`; MSRV set by `pixels`. |
| Edition | 2024 | | |
| Windowing/input | `winit` | 0.30 | Stay on 0.30 until `softbuffer`/`pixels` support 0.31 (0.31 is beta). |
| Framebuffer to screen (phase 1) | `softbuffer` | 0.4 | CPU framebuffer, zero GPU deps. Simplest "runs anywhere". |
| Framebuffer to screen (phase 2) | `pixels` (wgpu 29) | 0.17 | Swap in when shaders (CRT / xBRZ-style scalers) are wanted. |
| Audio | `rodio` | 0.23 | |
| Binary parsing | `binrw` | 0.15 | Declarative struct parsing for asset formats. |
| Images (PNG export in tools only) | `image` | 0.25 | `image` has no PCX decoder; we write our own. |
| CLI | `clap` (derive) | 4.6 | |
| Errors | `anyhow` (bins), `thiserror` (libs) | 1 / 2 | |
| Logging | `log` + `env_logger` | 0.4 / 0.11 | |
| Serialisation (config, saves) | `serde` + `toml` | 1 | |
| Tests | `insta` (snapshots), `proptest` | 1.49 / 1.12 | Snapshots of *synthetic* data only. |
| Benchmarks | `criterion` | 0.8 | |

Avoid adding a game engine (bevy, macroquad, ggez). The original is a fixed-function
2D game; a framebuffer plus our own loop keeps the port auditable and portable.

## Intended layout

A Cargo workspace. Crates are created by backlog tasks; don't pre-create empty ones.

```
Cargo.toml                 workspace root
rust-toolchain.toml
crates/
  xeno-formats/            pure parsers for every original file type; bytes in, structs out; no I/O
  xeno-sim/                deterministic game rules; headless; no rendering or audio
  xeno-render/             320x200 indexed framebuffer, palette, blit, scaling to any output size
  xeno-audio/              music/SFX decoding and playback
  xeno-app/                the binary: window, input, timing, wiring of the above
  xeno-tool/               developer CLI: inspect / dump / convert assets into a gitignored out dir
docs/
  formats/                 one markdown file per file type: what is known, what is guessed
gameFiles/                 GITIGNORED. User's copy of the installed game.
```

Dependency direction is strictly downward: `xeno-app` -> `xeno-render`/`xeno-audio`/
`xeno-sim` -> `xeno-formats`. `xeno-formats` and `xeno-sim` must not depend on winit,
wgpu, rodio or anything with a native backend, so they can be tested headless in CI.

## The original game's files (reference only)

A stock installation (`install.bat` copies both floppies into `\XENOBOT`) contains about
130 files, roughly 2.5 MB. Observed types and current best guesses. Confirmed facts are
marked; everything else is a hypothesis to verify and record under `docs/formats/`.

| Pattern | Count | What it is | Status |
|---|---|---|---|
| `xeno.exe` | 1 | Main executable, 84 KB, **LZEXE 0.91 compressed**; 181,836 bytes and 655 relocations once unpacked (see `docs/re-setup.md`) | confirmed |
| `*.dvr` | 6 | DOS executables: `config`, `mapdvr`, `mgavideo`, `sounddvr`, `vgaiff`, `vgaintro`. Loadable driver/overlay modules; `config.dvr` is also LZEXE-packed (16,160 bytes unpacked), the other five are plain MZ | confirmed executables |
| `vga240.mod` | 1 | Probably the 320x240 Mode-X video module | guess |
| `*.pcx` | 2 | `cavedrop`, `xenodrop`: PCX 3.0, 8-bit, 768x128 / 768x130. Drop-sequence sprite strips or scrolling backgrounds | format confirmed |
| `*.vcl` | 14 | Cutscenes / animations (`ealogo`, `presents`, `arrival`, `emerge`, `win`, `nlose`, ...). 5 KB to 133 KB | guess |
| `*.usg` | 17 | Scenarios: `scen00a`..`scen12a` plus `ctrain`, `htrain`, `ntrain`, `strain` (training missions). ~8 to 14 KB each | guess |
| `scendef.bin` | 1 | Scenario definition table (name appears in exe strings) | likely |
| `*.m00` `*.m08` `*.m16` `*.m32` | 11 / 11 / 11 / 7 | Per-unit model data at four sizes/LODs: `basedome`, `cloak`, `explode`, `humanoid`, `laser`, `missile`, `netrelay`, `obst`, `plasma`, `scorpion`, `scout`. `humanoid`, `missile`, `scorpion` and `scout` have no `.m32` (confirmed in the reference copy). Most `.m00` are 18 bytes (header only?) | guess |
| `*.m@l` | 8 | Full-screen layouts: `login`, `sitroom`, `maproom`, `review`, `trainmen`, `humancon`, `scorpcon`, `scoutcon` | guess |
| `*.b@n` | 14 | `msprite0`..`msprite5`, `sky_a`..`sky_h`: sprite banks / sky strips | guess |
| `*.ms3` | 6 | Music: `mainthem`, `win1`, `win2`, `bigwin3`, `lose`, `robot3`. Likely AdLib/OPL2 sequences | guess |
| `dsounds.bin` | 1 | Digitised sound effects, 97 KB | likely |
| `*.fgd`, `fgarray.kyl` | 6 | All exactly 1920 bytes = 80x24. Text-mode screen layouts or 48x40 grids | guess |
| `*.map` | 2 | `default.map`, `backup.map`: tiny (26 / 122 bytes). Key or joystick mapping | guess |
| `state.rst` | 1 | Saved game / resume state | guess |
| `xenobot.1`, `xenobot.2` | 2 | 4-byte floppy-disk markers used by the installer | confirmed from `install.bat` |
| `orders.txt` | 1 | Placeholder text (an EBS test message) | confirmed |

Strings in the exe mention robot order names (`SENTRY`, `DETECT`, `ZIGZAG`,
`AVOIDANCE`, `FIRE ALL`...), which matches the game's programmable-robot mechanic.

## Working on the formats

- Run `xeno-tool inspect <file>` (once it exists) before guessing. Record findings in
  `docs/formats/<ext>.md` with offsets, types, endianness and what is still unknown.
- Reverse-engineering the executable is allowed for interoperability; keep the
  unpacked exe and any Ghidra project **outside** the repo or under a gitignored path.
- Every parser gets: a `binrw` struct, a `parse(&[u8]) -> Result<T, FormatError>`
  entry point, a unit test on a synthetic buffer, and an `insta` snapshot of the
  *debug-printed struct* from that synthetic buffer.
- Parsers must be total: unknown fields are kept as named `unknown_XX` fields, never
  silently skipped, so later tasks can identify them.

## Rendering and the 4K goal

Two layers, always kept separate:

1. **Logical framebuffer** at the original resolution with the original 256-colour
   palette. All game drawing targets this. Pixel-exact fidelity is checkable here.
2. **Presentation**: scale the logical framebuffer to the window. Default is integer
   nearest-neighbour scaling with letterboxing and VGA aspect correction (320x200 was
   shown at 4:3, so pixels are not square). 4K gets a 6x integer scale with correct
   aspect. Later options: xBRZ-style scalers, CRT shader, and an *HD asset pack*
   loaded from a user directory that replaces individual sprites at higher resolution.

"Not lose details" means: never bilinear-blur the original art, keep aspect correct,
and make the sprite pipeline resolution-independent so HD replacements can drop in.

## Commands

Once the workspace exists, these are the expected commands (keep them true):

```bash
cargo build
cargo run -p xeno-app -- --game-dir ./gameFiles
cargo run -p xeno-tool -- inspect ./gameFiles/scen00a.usg
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Testing

- Unit tests and snapshots use synthetic data only.
- Tests that need real game files must be `#[ignore]` by default and read
  `XENOBOTS_GAME_DIR`; they should `return` (not fail) when it is unset. Name them
  `real_assets_*` so they can be run with `cargo test real_assets -- --ignored`.
  `cargo test` runs in the crate directory, so resolve a relative `XENOBOTS_GAME_DIR`
  against the workspace root (see `xeno-app`'s `real_assets_game_dir_is_complete`).
- CI runs fmt, clippy and tests on ubuntu, macos and windows without game files.

## Style

- `cargo fmt` defaults, `clippy` clean with `-D warnings`.
- No `unsafe` outside `xeno-render`'s hot blit paths, and each use gets a `// SAFETY:` comment.
- Prefer small, pure functions; the sim must be deterministic for a given seed and input log.
- Commit messages: imperative subject, body explains *why*, reference the task id
  (`T-012`) from `NEXT_STEPS.md`.
