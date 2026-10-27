# AGENTS.md

Guidance for any coding agent (Claude Code, Codex, Cursor, Copilot, etc.) working in
this repository. `CLAUDE.md` imports this file and adds Claude-specific notes.

## What this project is

A from-scratch Rust reimplementation of **Xenobots** (sold in North America as
**Ultrabots**, NovaLogic / Electronic Arts, 1993), a DOS mech-strategy game. The goals,
in priority order:

1. **Runs anywhere.** One `cargo run` on macOS, Linux and Windows. No DOSBox, no
   emulator, no platform-specific build steps.
2. **Faithful first.** The original 256-colour presentation (320x200 mode 13h, plus a
   320x240 unchained mode for the `.vcl` cutscenes; see `docs/formats/video-modes.md`) and game rules must be reproduced before anything is "improved".
3. **4K without losing detail.** A resolution-independent renderer, and later optional
   higher-detail assets, so the game fills a 3840x2160 display cleanly instead of being a
   blurry stretch.

The project is at the **scaffold stage**: the Cargo workspace exists and `xeno-app`
checks the game files, then opens a window that plays the start-up cutscene from the
user's `xeno.exe` and `.vcl` pictures and the intro drop, with the game's music (`xeno-audio`), then steps through
placeholder screens in the original's order (`docs/screens.md`), run by a
fixed-timestep loop at the original's retrace rate (`docs/game-loop.md`). `xeno-sim`
holds the start of the deterministic simulation (`docs/sim.md`). Read `NEXT_STEPS.md` for the task backlog and pick
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
  xeno-render/             indexed framebuffer (320x200 or 320x240, optionally a wider virtual surface), palette, blits, scaling to any output size
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
| `xeno.exe` | 1 | Main executable, 84 KB, **LZEXE 0.91 compressed**; 181,836 bytes and 655 relocations once unpacked (see `docs/re-setup.md`). `xeno_formats::lzexe` unpacks it at run time, byte for byte as `unlzexe` does; `DS` is load-image paragraph 0x1901, read from the entry point's `mov ax, seg` / `mov ds, ax` (`DS:x` is unpacked file offset `0x19c10 + x`) | confirmed |
| `*.dvr` | 6 | DOS executables: `config`, `mapdvr`, `mgavideo`, `sounddvr`, `vgaiff`, `vgaintro`. `config.dvr` (LZEXE-packed, 16,160 bytes unpacked) is the setup program, run as a child process that fills in the host's configuration record (devices, and 16 unlock words from `XENO.KEY`) and saves its answers in `XENO.CFG` (see `docs/formats/cfg.md`). The other five are plain MZ modules loaded with DOS "load overlay" into memory blocks: each starts with `ret`, `retf`, an init and a shutdown entry, and exports routines by writing far pointers into fixed **slots** in `xeno.exe`'s data segment; everyone calls through the slots. `sounddvr` is music and sound effects, `mgavideo` mode 13h, palette (fades and colour-animation records, `docs/formats/palette.md`), pictures and drawing, `vgaintro` the intro drop (its frame script, times and colour records are read from the user's copy by `xeno_formats::vgaintro`, `docs/formats/fgd.md`), `mapdvr` the terrain editor, `vgaiff` a screenshot writer. See `docs/re/dvr-protocol.md` | confirmed; most `mgavideo` drawing slots unnamed |
| `vga240.mod` | 1 | Headerless real-mode code module that `mgavideo.dvr` loads on demand through stub slots (`docs/re/dvr-protocol.md`): 320x240 256-colour unchained mode at 60 Hz (from BIOS mode 12h, double-scanned), virtual-width row tables and hardware scroll. The default mode is 320x200 mode 13h (see `docs/formats/video-modes.md`) | confirmed |
| Fonts (no file) | 4 | Inside `mgavideo.dvr`: its fifth data block (host descriptor `DS:0x62a`) is a table of four fonts the host calls 0, 2, 4 and 6. A font is a 6-byte header (kind, unused, cell width, line height, x and y steps), a table of glyph offsets by character code (bit 7 masked off; bit 7 picks a second colour pair), then glyphs: a first byte (0 = control code; else the width) and `height - 1` rows. Kind 7 is a fixed-width bitmap (one byte a row, bit 7 leftmost: fonts 0 and 2, 6x6 and 4x6); kind 0 is proportional and shaded (one byte a pixel, drawn as `colour + (value ^ 3)`: fonts 4 and 6). Font 6 in colour 252 is the captions' and credits' font; the log-in name is font 0. Read from the user's driver by `xeno_formats::mgavideo` and `xeno_formats::font`, drawn by `xeno_render`'s `draw_text` (see `docs/formats/font.md`) | confirmed |
| `*.pcx` | 2 | `cavedrop`, `xenodrop`: PCX 3.0, 8-bit RLE with 256-colour trailer, 768x128 / 768x130. Decoded by `xeno_formats::pcx`. The intro drop's backdrops: `vgaintro.dvr` loads each into its 768-wide video memory (with a clean copy 128 rows lower) and scrolls it in hardware; only rows 0..=127 are on screen, and colour 0 is black in both (see `docs/formats/pcx.md`, `fgd.md`) | confirmed |
| `*.vcl` | 14 | Cutscene pictures (`ealogo`, `presents`, `arrival`, `emerge`, `win`, `nlose`, ...), 5 KB to 133 KB. Not animations: one still picture each, a 768-byte VGA 6-bit palette, then PackBits to the end of the file, 640 wide and 240 or 400 rows. `vga240.mod` loads it into the 320x240 mode's video memory, and the motion comes from 26-byte sequence entries in `xeno.exe` that pan a 320x240 window over it, fade the palette and type a caption, timed from the start of the music. Shown in three sequences: the intro, a mission won or lost (the lose picture by the robot console opened last), and the campaign won. Captions are NUL-terminated strings in the host's string segment (the word at `DS:0x60`, set by a start-up fix-up table), typed in font 6; the win caption gets the mission's kills, losses and points written into it first. `mgavideo.dvr` also names `hlose.vcl`, which nothing plays. Decoded by `xeno_formats::vcl`; the entries and captions by `xeno_formats::sequence` and `ending` from the user's `xeno.exe`, and `xeno-app` plays the ten-entry intro (`EALOGO` first, `PRESENTS` last) at start-up and the three after a mission, captions typed (see `docs/formats/vcl.md`) | confirmed; intro (T-036), endings and captions (T-045) played, timing not yet checked in DOSBox |
| `*.usg` | 17 | Scenarios: `scen00a`..`scen12a` (`scendef.bin` record `n` loads `scenNNa`) plus `ctrain`, `htrain`, `ntrain`, `strain` (training missions). ~8 to 14 KB each. Tagged chunks (tag, u16 size, data): `SHDR` start clock, `MAPP` terrain features, `DCBS` objects (kind, side, 16.16 position, heading, radius and flag bits; by kind, `usg::Body`: a robot's orders, speeds, helm, power and damage bits, a base's damage, kills and losses, a relay's damage; at load every robot restarts its given program; rest unknown), `APTH`/`HPTH` each side's paths (up to 8 of up to 16 points; a mission, a weight the alien base picks idle robots' paths by, the editor's kind mix, counts of the robots on them), `ORDR` briefing text (typed in the situation room's orders view), `BGND` sky file name, `TERM`. No map grid, and no world edges: an open plane around the editor's origin (128, 128). `MAPP` records are hills (0x14: octagonal mounds robots cannot enter), open hills (0x16: robots drive through them) and blocks (0x12: the editor's unseen closing record), each a position, three 8.8 sizes and a drawing program. Decoded by `xeno_formats::usg`; `usg::write` writes one as the game's writer does (every stock file round-trips byte for byte). A save, `<NAME>.LOG`, is the loaded scenario as it stands plus an `SDEF` chunk (the campaign: selection, loaded scenario, progress, score and counters, result, and the `scendef.bin` table with each `state`), parsed into `usg::SaveState`; the calendar is in its `SHDR` (`docs/formats/usg.md`, "Save files"). The game's built-in scenario editor works in a mission's situation room with unlock word 9, and the situation room's Ctrl+Alt+P saves the mission as a `.usg` (see `docs/formats/usg.md`, `docs/screens.md`). How robots move and walk paths, and the tables `xeno_formats::motion` reads for it from `xeno.exe`, are in `docs/sim.md`, "Movement" | layout, world, terrain, paths and writing confirmed; object fields partly read; movement traced (T-052) |
| `scendef.bin` | 1 | Campaign scenario table: 13 variable-length records (id, a clickable box on the map room screen, unlock level, a runtime state word, NUL-terminated place name), ended by `0xffff`. No training missions. The game also writes the table, with each scenario's state, into save files. Decoded by `xeno_formats::scendef` (see `docs/formats/scendef.md`). Record `n` loads `scenNNa.usg`; `state` is open (0, 2, 4, 6: alert levels), locked (8), won (0xa) or in play (0xc; an alert rise can push it to 0xe); the map room's tables and the campaign's rules are read from `xeno.exe` by `xeno_formats::maproom` and `xeno_formats::campaign` (`docs/screens.md`, "The map room", "Campaign rules") | confirmed |
| `*.m00` `*.m08` `*.m16` `*.m32` | 11 / 11 / 11 / 7 | Unit models (`mgavideo.dvr` calls them `.MXX`): `basedome`, `cloak`, `explode`, `humanoid`, `laser`, `missile`, `netrelay`, `obst`, `plasma`, `scorpion`, `scout`; `humanoid`, `missile`, `scorpion` and `scout` have no `.m32`. A chain of views, each for one facing (in 64ths of a turn) and its mirror. The suffix is the angular resolution: `.m08` holds facings 0, 8, ..., 32, `.m16` adds 4, 12, 20, 28, `.m32` adds 2, 6, ..., 30, and `.m00` is one view with no facing (most are 18 bytes, no data). Facing 0 faces the viewer. Each facing has animations (the unit's moving pieces, all drawn at once, each at its own frame); a frame places images (row spans like `.b@n`, colour 0 transparent, maybe mirrored, maybe from the `.m00`) at x/y offsets, sorted by a depth byte. `xeno.exe` loads them all at start-up (`.m00`+`.m08` for every model, `.m16`/`.m32` only for the directional ones and `netrelay`) and draws them scaled in the 3D view. Decoded by `xeno_formats::mxx`, drawn scaled by `xeno_render`'s `blit_model_image` (see `docs/formats/mxx.md`). Which objects become which render nodes, their frame numbers and the projection are in `docs/render.md` (T-100; tables read by `xeno_formats::view3d`) | confirmed |
| `*.m@l` | 8 | Full-screen 320x200 screens: `login`, `sitroom`, `maproom`, `review`, `trainmen`, `humancon`, `scorpcon`, `scoutcon` (`mgavideo.dvr` names them `.MRL`, file ids 0x00..0x14, and also names `TITLE`, `OPTIONS` and `WALL2`, which are not in the install). Packed (`xeno_formats::packed`); unpacked, a 768-byte 6-bit palette, then PackBits that expands to exactly 64,000 bytes. The game requests all 8 stock screens and never the other three. Decoded by `xeno_formats::mal` (see `docs/formats/m@l.md`) | confirmed |
| `*.b@n` | 14 | Packed `.BIN` files: the loader tries `.BIN`, `.B!N` (LZW) and `.B@N` (`docs/formats/packed.md`). `msprite0`..`msprite5` (`MSPRITES.BIN` with a digit) are sprite banks: records of width, height, flags and 8-bit pixels, raw or in row spans, numbered through one 390-entry directory; `mgavideo.dvr` draws them opaque or with colour 0 transparent, plain, flipped or mirrored. `sky_a`..`sky_h` are bare 342x54 horizon strips, named by a scenario's `BGND`, wrapped every 342 columns at 1024 columns per turn above the 3D view's horizon. No palette. Decoded by `xeno_formats::ban` (see `docs/formats/b@n.md`) | confirmed |
| `*.ms3` | 6 | Music: `mainthem`, `win1`, `win2`, `bigwin3` (named by `xeno.exe`), `lose`, `robot3` (named by nothing). Not OPL register dumps: a 48-byte header (signature, per-channel doubling and starting programs), then one MIDI-like event stream (`9n` note on, `8n` note off with no data, `Cn` program, `En` one-byte pitch offset; each event followed by a 1- or 2-byte delay in ticks) ending `FF 2F`. Played by `sounddvr.dvr`'s MS3 driver on AdLib or MPU-401; the programs index an instrument bank inside the driver (128 sixteen-byte OPL2 patches, a frequency table by half-semitone, per-program transpose, velocity factor and pitch envelope, a channel-to-voice map on which channel 8 has no AdLib voice), read from the user's `sounddvr.dvr` by `xeno_formats::sounddvr` (`docs/formats/sounddvr.md`). The music ticks once per vertical retrace (70 Hz in mode 13h, 60 Hz in the 320x240 mode). `xeno.exe` plays `mainthem` at start-up (stopped on entering the map room), `win1`/`win2` with a won/lost mission's picture and `bigwin3` with `TRIUMPH`, each once with no loop; `lose` and `robot3` are never played. Decoded by `xeno_formats::ms3` (see `docs/formats/ms3.md`); played by `xeno_audio::Music`, the driver's AdLib register writes on an emulated OPL2 (Nuked-OPL3; `docs/formats/sounddvr.md`, "Playback in the port") | layout, bank, callers and tick rate confirmed (T-064); played (T-062); DOSBox check to do |
| `dsounds.bin` | 1 | Digitised sound effects, 97 KB: 17 blocks of 16-byte paragraphs (u16 paragraph count, then the block; a zero count ends the file). Each block is a u16 end offset, a u16 step (1/65536ths of a sample per tick), then unsigned 8-bit mono samples. `sounddvr.dvr` mixes three voices on IRQ 0 at 1,193,182 / 165, about 7231 Hz, so a sound plays at `7231 * step / 65536` Hz (7231 Hz for all but one), to a PC speaker, AdLib, Disney Sound Source or Sound Blaster. `xeno.exe` plays them through 29 sound events (`DS:0x7e18`: block, voice, queue flag, step override), most only for the robot whose console is open; blocks 1, 4 and 15 are never heard. Decoded by `xeno_formats::dsounds`, the events by `xeno_formats::sounds`; `xeno-tool dump` writes WAVs; `xeno-app` plays the events the sim reaches (hits, robots destroyed, a base's end) on `xeno_audio::Mixer`, as the Sound Blaster (see `docs/formats/dsounds.md`, "Sound events") | confirmed; events named (T-065); played (T-061) |
| `*.fgd`, `fgarray.kyl` | 6 | All exactly 1920 bytes, but not grids: one byte per column of a 1920-column profile, the row where a black silhouette's edge is. `vgaintro.dvr` scrolls a 768-column window of each over the intro drop backdrops with parallax: `layer3`/`layer4` are the near and far floors over `xenodrop.pcx`, `layer1`/`layer2` the floors and `layer5` the ceiling over `cavedrop.pcx`. `fgarray.kyl` has the same layout but nothing loads it (an earlier draft of `layer3`?). The drop runs at 2 retraces a frame in mode 13h with the screen split after row 128 (credits below); a frame script accelerates, cruises and slows the layers (parallax about 2.6 : 1.9 : 1 against the backdrop), the first part ends at frame 448, the cave part stops at frame 701 and ends on the music's clock, so the first ~550 columns of the cave layers are never shown. Decoded by `xeno_formats::fgd` (see `docs/formats/fgd.md`, "The drop sequence") | confirmed; `fgarray.kyl` unused; DOSBox comparison to do |
| `*.map` | 2 | `default.map` (122 bytes), `backup.map` (26): terrain saved by the map editor in `mapdvr.dvr`, **not** key or joystick mapping (`config.dvr`'s keyboard tables are in `XENO.CFG`). The same 24-byte records as a `.usg` `MAPP` chunk, relative to the editor's map origin (128, 128), ended by a 0 word. Only the editor reads them (when the user loads one); it writes `BACKUP.MAP` itself. Decoded and written by `xeno_formats::map`; every stock `.usg`'s terrain is exactly what the editor writes (see `docs/formats/map.md`) | confirmed |
| `XENO.CFG`, `XENO.KEY` | 0 | Not in a stock install. `XENO.CFG` (48 bytes) is written and read only by `config.dvr`: its saved answers, then two players' 16-byte keyboard tables (set 1 codes for up-left, up, up-right, left, right and fire; `0x60` unused). Their defaults are a table in `config.dvr`. `xeno.exe` never reads the keys: its own keys are fixed tables. `XENO.KEY` is a hand-written text file of `n:code` lines that `config.dvr` hashes into 16 words that unlock developer features in `xeno.exe` (editor, screenshots, debug screen). Decoded by `xeno_formats::setup` (see `docs/formats/cfg.md`, `key.md`) | confirmed |
| `state.rst` | 1 | Not used by the game and not a save game: no binary names it. DOS text (a line of numbers, a `[FILES]` section naming a batch file on `C:`), apparently left by a developer's setup tool. Saves are `<NAME>.LOG`, `.usg` chunk files with an `SDEF` chunk (`docs/formats/usg.md`). Decoded by `xeno_formats::rst` (see `docs/formats/rst.md`) | confirmed unused |
| `xenobot.1`, `xenobot.2` | 2 | 4-byte floppy-disk markers (the disk's digit, a space, `CR LF`) that `install.bat` and `part2.bat` check for; the game never reads them (see `docs/formats/disk-markers.md`) | confirmed |
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
   nearest-neighbour scaling with letterboxing and VGA aspect correction (both 320x200 and
   320x240 were shown at 4:3, so 320x200 pixels are not square while 320x240 pixels
   are). Factors are whole numbers per axis, so 1.2 is exact only at 5x6 and 10x12;
   by default the largest pair within 5% of 4:3 is used: at 3840x2160, 320x200 is
   shown 8x10 (2560x2000, +4.2%) and 320x240 9x9 (2880x2160); at 1920x1080, 4x5 and
   4x4. `--aspect exact` and `--scale N` pick otherwise (see
   `docs/formats/video-modes.md`, "Presentation"). Later options: xBRZ-style scalers, CRT shader, and an *HD asset pack*
   loaded from a user directory that replaces individual sprites at higher resolution.

"Not lose details" means: never bilinear-blur the original art, keep aspect correct,
and make the sprite pipeline resolution-independent so HD replacements can drop in.

## Commands

Once the workspace exists, these are the expected commands (keep them true):

```bash
cargo build
cargo run -p xeno-app -- --game-dir ./gameFiles           # intro cutscene and drop (any key ends them, P pauses), then the placeholder screens (keys in the window title); ESC leaves fullscreen, then quits where no screen claims it, F11 fullscreen
cargo run -p xeno-app -- --game-dir ./gameFiles --no-intro  # straight to the log-in screen
cargo run -p xeno-app -- --game-dir ./gameFiles --drop    # the intro drop alone, then the log-in screen
cargo run -p xeno-app -- --game-dir ./gameFiles --screen map-room  # start at a screen (docs/screens.md); --screen test-card for the test card
cargo run -p xeno-app -- --game-dir ./gameFiles --fullscreen  # start borderless fullscreen
cargo run -p xeno-app -- --game-dir ./gameFiles --seed 1  # fix the random word's seed (logged at start-up; the clock's otherwise)
cargo run -p xeno-app -- --game-dir ./gameFiles --save-dir /tmp/saves  # where Ctrl+Alt+P (EDITED.USG), the editor's Ctrl+M (BACKUP.MAP) and the map room's N and Q (<NAME>.LOG, loaded at log-in) save; the game dir otherwise
cargo run -p xeno-app -- --game-dir ./gameFiles --check   # check the game files (and decode the intro, drop and screens) only, no window
cargo run -p xeno-app -- --game-dir ./gameFiles --view msprite1.b@n  # asset viewer (README): sprite banks, skies, screens, pictures, models
cargo run -p xeno-audio --example music -- ./gameFiles mainthem  # one song on the emulated OPL2, then exit
cargo run -p xeno-audio --example sfx -- ./gameFiles 6  # one dsounds.bin block through the effects mixer, then exit
cargo run -p xeno-tool -- inspect ./gameFiles/scen00a.usg
cargo run -p xeno-tool -- inspect ./gameFiles/xeno.exe     # unpacks it; lists the intro entries
cargo run -p xeno-tool -- inspect --all ./gameFiles
cargo run -p xeno-tool -- simulate ./gameFiles/scen00a.usg --ticks 21000 --every 7000  # robots' positions as their scripts drive them, no window (--drive SLOT:SPEED:HEADING starts one)
cargo run -p xeno-tool -- simulate ./gameFiles/scen00a.usg --ticks 120 --every 0 --hit 10:3:81  # hit slot 3 for 81 points before tick 10: "tick 10: 3 destroyed"
cargo run -p xeno-tool -- simulate ./gameFiles/ntrain.usg --ticks 2000 --every 0 --hit 1:1:255 --hit 1:1:45  # destroy the alien base (slot 1): "tick 520: mission won", and the run stops
cargo run -p xeno-audio --example tone  # a 440 Hz tone through the mixer, joined by a second voice after a second (no game files needed)
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
