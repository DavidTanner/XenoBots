# Next steps

The backlog for the Xenobots Rust port. Every entry is sized to be one branch, one
pull request, reviewable in one sitting. Pick the lowest-numbered `todo` task whose
dependencies are `done`, unless something else is more urgent.

## How to use this file

- **Claim** a task by setting its status to `doing` in your branch's first commit.
- **Finish** it by setting `done`, and add any follow-up work you discovered as new
  tasks at the end of the relevant milestone (next free id in that block).
- **Split** rather than grow: if a task is turning into more than ~400 lines of diff,
  land what works, mark the task `done`, and file the rest.
- Status values: `todo`, `doing`, `done`, `blocked` (say why in the notes column),
  and `ongoing` for standing work that is never finished (see "Ongoing" below).
- Ids are stable; never renumber. Gaps are fine.
- Nothing here may involve committing game files or data derived from them. See
  `AGENTS.md`.

## Milestones

| Milestone | Goal | Exit criterion |
|---|---|---|
| M0 Scaffold | Workspace builds, CI is green, repo hygiene in place | `cargo test` passes on three OSes in CI with no game files |
| M1 Formats | Every original file type has a parser or a documented "unknown" | `xeno-tool inspect` runs over a full install without erroring |
| M2 Window | A window shows a scaled logical framebuffer at 4K-ready quality | Title screen art visible, pixel-exact, aspect-correct |
| M3 Front end | Menus, scenario select, screens drive real data | Can navigate to a scenario and see its map |
| M4 Simulation | Robots move, fight and follow orders as in the original | A training mission can be won |
| M5 Audio | Music and sound effects | Main theme plays, weapons make noise |
| M6 HD | Detail-preserving upscaling and HD asset hooks | 4K fullscreen with optional xBRZ and HD pack |
| M7 Release | Packaged builds for the three OSes | Tagged release with downloadable binaries |

## M0 Scaffold

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-001 | done | Create the Cargo workspace: root `Cargo.toml` (`resolver = "3"`, edition 2024), `rust-toolchain.toml` (`channel = "stable"`), `rustfmt.toml`, and an empty `crates/xeno-app` binary that prints its version | | `cargo run -p xeno-app` prints a version string | Commit `Cargo.lock` (this is an application). |
| T-002 | done | Repo hygiene: add `target/`, `.idea/`, `*.iml`, `.vscode/`, `out/`, `extracted/`, `dump/`, `*.png` outside `docs/` and a `# game data` section to `.gitignore`; add a `docs/formats/README.md` stub | | `git status` is clean after a build and an asset dump into `out/` | Keep the existing `gameFiles/` line. Replaced the C template; the game-data section also ignores every original extension anywhere in the tree. |
| T-003 | done | GitHub Actions CI: fmt check, clippy `-D warnings`, `cargo test --workspace` on ubuntu-latest, macos-latest, windows-latest; cache cargo | T-001 | Green run on a PR | Install the Linux apt packages listed in README. Green on PR #2 and on `main`. |
| T-004 | done | `xeno-app --game-dir` / `XENOBOTS_GAME_DIR` / `./gameFiles` resolution, plus a startup check that lists missing expected files by name | T-001 | Running without files prints a clear, actionable error and exits non-zero | Const list of 118 files in `xeno-app/src/game_dir.rs`; names match case-insensitively. Excludes installer files, `orders.txt`, and `state.rst` / `backup.map` (assumed user state). |
| T-005 | done | Add `crates/xeno-formats` library with a `FormatError` type, a `Palette` type (256 x RGB), and the test-fixture helper `synthetic_bytes()` | T-001 | Crate compiles with one passing test | No parsers yet. `Palette::from_rgb8` (PCX trailer form) and `from_vga6` (DAC form, widened to 8 bits). `fixtures::synthetic_bytes(len, seed)` is SplitMix64; other crates get it via the `test-fixtures` feature in `[dev-dependencies]`. |
| T-006 | done | Add `crates/xeno-tool` CLI skeleton with `inspect <file>` that prints size, extension, and a 64-byte hex preview | T-005 | Works on any file | Generic; per-format output comes with each parser. Report built in `xeno-tool/src/inspect.rs` (`insta` snapshots of synthetic files); ran without error over all 126 files of a local install. |
| T-007 | done | Move the expected-file list from `xeno-app` into `xeno-formats`, and give `xeno-app` a `GameDir` lookup (lowercase name -> real path) that loaders use to open files | T-004, T-005 | Upper-case installs load on Linux; `xeno-app` has no file list of its own | List is `xeno_formats::files::EXPECTED_FILES`; `GameDir::check()` returns `GameFiles` (`path`/`read` by case-insensitive name). A string search of every binary finds no `state.rst`; `backup.map` is named only by `mapdvr.dvr`, with `default.map` and a `working.map` not in the install. Names are often built at run time, so read-vs-write moved to T-024. |

## M1 Formats

Each parser task: `binrw` struct in `xeno-formats`, `parse()` entry point, synthetic
unit test, `insta` snapshot of the synthetic parse, an `inspect` branch in
`xeno-tool`, and `docs/formats/<ext>.md` with offsets and open questions.

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-010 | done | Set up reverse-engineering workspace *outside the repo*: unpack `xeno.exe` and `config.dvr` with `unlzexe`, import into Ghidra; write `docs/re-setup.md` describing the steps (no binaries, no listings) | | Doc exists; a teammate can reproduce the setup | `unlzexe` from the `lzexe` crate 0.1.1 (pure Rust, cross-platform). Doc lists SHA-256 of packed and unpacked binaries. All six `.dvr` imported too; the four unpacked ones analyse to 1-2 functions (see T-025). |
| T-011 | done | PCX decoder (v3.0, 8-bit RLE, 256-colour palette trailer) | T-005 | Synthetic PCX round-trips; `xeno-tool dump` writes a PNG to an out dir | `xeno_formats::pcx`; `fixtures::synthetic_pcx` encodes test images. `xeno-tool dump <file> --out <dir>` (default `out/`) and a pcx line in `inspect`. Both game files decode; `docs/formats/pcx.md`. |
| T-012 | done | Identify the video mode(s): is the logical framebuffer 320x200, 320x240 Mode-X (`vga240.mod`), or both? Record in `docs/formats/video-modes.md` | T-010 | Doc states the mode(s) with evidence | Both. `mgavideo.dvr` runs mode 13h (320x200, 70 Hz). It loads `vga240.mod` on demand for 320x240 unchained (60 Hz, square pixels, 640-wide virtual screen), which `xeno.exe` enters for three sequences. `vgaintro.dvr` keeps 320x200 but goes unchained with a 768-wide virtual line for the drop backdrops. Follow-up: T-027. |
| T-013 | done | Palette extraction: find where the 256-colour palette(s) come from (PCX trailer, embedded in exe, or a separate file) | T-011, T-012 | `xeno-tool palette <source>` prints 256 RGB triples | No palette file and none in `xeno.exe`: each picture brings its own. PCX trailer (8-bit); first 768 bytes of every `.vcl` (6-bit, `xeno_formats::vcl::palette`); first 768 bytes of a decoded `.m@l` screen (T-017). `mgavideo.dvr` owns the DAC: shadow palette uploaded on retrace, fades, colour-animation records. `xeno-tool palette` also takes `--offset`/`--encoding` for raw palettes. `docs/formats/palette.md`; follow-up T-037. |
| T-014 | done | `.b@n` sprite banks (`msprite*`, `sky_*`): header, frame count, per-frame dimensions, pixel encoding | T-013 | Frames render to PNG in an out dir | Every `@` file is packed with a small LZ77 (1 KB window, `0x80` end marker; decoder in `xeno.exe` at `1fe6:2efb`); `xeno_formats::packed`, `docs/formats/packed.md`. Unpacked, `msprite*` are sprite banks (records of size, width, height, flags, `unknown_05`, then raw or row-span pixels) and `sky_*` are bare 342x54 images; `xeno_formats::ban`, `docs/formats/b@n.md`. `xeno-tool dump` writes one PNG per sprite, with `--palette <pcx\|vcl>` (grey without). Follow-up: T-028. |
| T-015 | done | `.m00/.m08/.m16/.m32` unit models: establish what the four suffixes mean (LOD? zoom level? facing count?) and parse the header | T-013 | Doc + header struct; pixel data may remain `unknown` | Not LODs. A file is a chain of views, each for a facing in 64ths of a turn plus its mirror; `.m08` holds 0..32 step 8, `.m16` adds 4..28 step 8, `.m32` adds 2..30 step 4, `.m00` is one view with no facing. `xeno_formats::mxx` checks every header; `inspect` lists facings; `docs/formats/mxx.md`. `mgavideo.dvr` builds the names from `00081632` (file ids 0x42..0x98). Follow-up: T-029. |
| T-016 | done | `.m00/.m08/.m16/.m32` pixel data | T-015 | Unit sprites render to PNG | Each facing has an index of animations, then frames; a frame lists parts (image number, flip bit, `.m00` bit, x, y) over the view's row-span images. Every byte of every view is accounted for and every reference resolves. `xeno_formats::mxx` decodes it, `inspect` counts it, `dump` writes one composed PNG per frame. Open questions (palette, `unknown_01`, empty facings, colour 0) went to T-029. |
| T-017 | done | `.m@l` screen layouts (`login`, `sitroom`, `maproom`, ...) | T-013, T-014 | At least one screen renders to PNG | Unpacked, a screen is a 768-byte 6-bit palette, then PackBits (`0x80` repeats 129, as the game's decoder does; unused) that ends exactly at 64,000 pixels and at the end of the data. All 8 decode and look right; no run crosses a row. `xeno_formats::mal`; `inspect`, `dump` and `palette` in `xeno-tool`; `docs/formats/m@l.md`. Loader: `mgavideo.dvr`'s file-id table names 11 `.MRL` screens (ids 0x00..0x14), including `TITLE`, `OPTIONS` and `WALL2`, which a stock install lacks; who requests them went to T-025. Index 255 is a pure-green placeholder in three screens (T-042, T-043). |
| T-018 | done | `.vcl` cutscenes/animations: container structure and frame decoding | T-013 | `ealogo.vcl` (smallest) renders its frames | One PR, not two: a `.vcl` has no frames. It is one still picture, a 768-byte palette and then PackBits to the end of the file, 640 wide and 240 or 400 rows (the 320x240 mode's 640-wide virtual screen; 400 rows is all of VGA memory). All 14 decode exactly to EOF, no run crosses a row, and they look right; `ealogo` renders as one PNG. The reader is `vga240.mod`'s loader (0x274), installed at host slot `0x490` in mode B; it writes linear pixels across the planes. Cutscene motion comes from 26-byte entries in `xeno.exe` (origin, effect, caption), now partly read for T-027. `hlose.vcl` is never played: its entry exists but nothing refers to it, and the lose table points at `slose` instead. `xeno_formats::vcl`, `inspect`/`dump`, `docs/formats/vcl.md`. |
| T-019 | done | `scendef.bin` scenario definition table | T-010 | Struct with names/indices of the 13+4 scenarios | 13 records, not 13+4: the training missions are not in it (`xeno.exe` names their `.USG` files itself). A record is id (twice its position), a clickable box on the map room screen (top, left, bottom, right), an unlock level compared with the player's progress, a runtime `state` word (0 in the file) and a NUL-terminated name with `0x0c` line breaks; `0xffff` ends the table and the game reads at most 32. Read from the loader, indexer, draw loop and hit test in `xeno.exe`; boxes match `maproom.m@l`. `xeno_formats::scendef`, `inspect`, `docs/formats/scendef.md`. Leads on T-020, T-024, T-042, T-057. |
| T-020 | done | `.usg` scenario files: header, map grid, unit placements, objectives | T-019 | `scen00a.usg` inspect prints map size and unit list | Record `n` loads `scenNNa.usg` (template `SCEN00A.USG`, letter from the selection's high byte). A file is tagged chunks (tag, u16 size, data) read by `xeno.exe` `1000:eefc` through a tag table: `SHDR` (version 0x80 and start clock), `MAPP` (terrain feature records), `DCBS` (objects: kind, side bit, 16.16 x/y, rest raw), `APTH`/`HPTH` (8 path slots per side), `ORDR` (briefing), `BGND` (sky file name), `TERM`; save files add `SDEF`. **No map grid**, so `inspect` prints the extent of the object positions instead; objectives are only briefing text. Kinds and sides read from the built-in scenario editor. All 17 parse; one base per side in each. `xeno_formats::usg`, `inspect`, `docs/formats/usg.md`. Follow-ups: T-058, T-059; `BACKLAND.BIN` lead on T-028. |
| T-021 | done | `.fgd` layers and `fgarray.kyl` (all 1920 bytes): determine grid shape (80x24 vs 48x40) and meaning | T-010 | Doc + parser | Neither: each file is a 1920-column profile, one byte per column giving the backdrop row of a black silhouette's edge. `vgaintro.dvr` loads them into three buffers (A/B/C) and scrolls a 768-column window of each from offset 1152 down, with parallax speeds: `layer3`/`layer4` are the near/far floors over `xenodrop.pcx`; `layer1`/`layer2` floors and `layer5` ceiling over `cavedrop.pcx`. Floors combine by taking the higher edge; draws are incremental (colour 0 down, or restore from a clean backdrop copy). `fgarray.kyl` is named by no binary; it resembles `layer3` (a draft?). `xeno_formats::fgd`, `inspect`, `docs/formats/fgd.md`. Follow-up: T-038. |
| T-022 | done | `.ms3` music format: identify (AdLib register stream? custom sequencer?) | T-010 | Doc; parser if feasible | Audio playback is M5. A custom sequencer, not a register stream: a 48-byte header (signature, per-channel doubling, starting programs), then a MIDI-like event stream (note on/off, program, a one-byte pitch offset; 1- or 2-byte delays in ticks) ending `FF 2F`, read by `sounddvr.dvr` (`0x0b97`) and played on AdLib or MPU-401. Programs index an instrument bank inside the driver. `lose.ms3` and `robot3.ms3` are named by no binary. `xeno_formats::ms3`, `inspect`, `docs/formats/ms3.md`. Follow-ups: T-063, T-064. |
| T-023 | done | `dsounds.bin` digitised SFX: directory structure, sample rate, encoding | T-010 | `xeno-tool dump` writes WAVs to an out dir | `docs/formats/dsounds.md`. Unsigned 8-bit mono at about 7231 Hz (PIT divisor 165), per-sound step in 1/65536ths of a sample per tick. Follow-up: T-065. |
| T-024 | done | `.map` key/joystick mapping, `state.rst` save state, `xenobot.1/.2` markers | T-010 | Doc for each; parsers for `.map` and `.rst` | None of the guesses held. `.map` files are the map editor's (`mapdvr.dvr`) terrain: `MAPP` records relative to the editor's origin, ended by a 0 word. Only the editor reads one (when the user loads it, through `xeno.exe` `1fe6:6a97`); it writes `BACKUP.MAP`, loads it and deletes it, and offers `DEFAULT.MAP` when saving. Keys and joysticks are `config.dvr`'s `XENO.KEY` (not in the install). `state.rst` is named by no binary: DOS text from a developer's setup tool. Saves are `<NAME>.LOG` (login name), written by `1000:f01b` as `.usg` chunks with an `SDEF` chunk (`1000:7666`: 30 bytes of state from `DS:0x5f97`, then the `scendef.bin` table). The markers are only checked by `install.bat` / `part2.bat`. `default.map` left `EXPECTED_FILES`. `xeno_formats::map`, `xeno_formats::rst`, `inspect`, `docs/formats/map.md`, `rst.md`, `disk-markers.md`; save files in `usg.md`. Follow-up: T-039; notes on T-057, T-059. |
| T-025 | done | `.dvr` modules: document the loader protocol between `xeno.exe` and the driver modules | T-010 | Doc explaining what each driver provides | `docs/re/dvr-protocol.md`. Modules are MZ files loaded with DOS EXEC `4B03h` into memory blocks (`1fe6:463d`); offset 2 is init, 6 shutdown, and they install `(slot, offset)` tables into far-pointer slots in the host `DS` through host slot `0x12` (hook/unhook with save: `0x26`/`0x2a`). `config.dvr` runs as a child process and edits the host's configuration record, found through an `!=` environment string. `vgaiff.dvr`, `vga240.mod` and `LIGHT.MOD` load lazily through `mgavideo.dvr` stubs; `LIGHT.MOD` and the IFF reader are unreachable. Two host hook chains: per retrace and a 60 Hz tick. Settled: music tick rate (T-064), slots `0x2fe`/`0x302` (file dialogs, `map.md`), and the `.MRL` requests (`TITLE`, `OPTIONS`, `WALL2` never). |
| T-026 | done | `xeno-tool inspect` runs over a whole directory (`inspect --all <dir>`) and summarises parse success per extension | T-006 | Table output; exit code 0 only if every file parsed | M1 exit criterion, now met: a stock install gives 112 parsed, 0 failed, 14 with no parser (`xeno.exe`, the `.dvr` modules, `vga240.mod`, `orders.txt`, the `.bat` files, the disk markers). Files with no parser are counted, not failed; only a parser rejecting a file (or a file that cannot be read) gives a non-zero exit. Subdirectories are not entered. |
| T-027 | todo | Identify which screens run in 320x240: decode the 26-byte sequence tables used by the three `xeno.exe` routines that enter mode B (`docs/formats/video-modes.md`), and confirm with a DOSBox capture | T-012 | `video-modes.md` lists every screen with its mode | Also settle whether `LIGHT.MOD` (named by `mgavideo.dvr`, absent from the install) is ever loaded. From T-018 (`docs/formats/vcl.md`): the three routines play only `.vcl` entries; the intro chain is at `DS:0xecac`, `TRIUMPH` at `DS:0xedb0`, win/lose at `DS:0x601c..0x606a`. Known fields: file id (+0), window origin (+2, +4), effect handler index (+6), caption text pointer (+0xa) and probably its position (+0xc, +0xe). Still to decode: the effect handlers (scroll? fade?), +0x08, +0x12, +0x16, +0x18, timing, and what the word at `DS:0x6012` that picks the lose cutscene means. |
| T-028 | todo | Trace the `.b@n` consumers: the sprite blitter (colour 0 transparent? row spans), how sprites are numbered when drawn, what `unknown_00`, `unknown_05` and the trailing empty span mean, how `MSPRITES.BIN` / `BACKLAND.BIN` map to the files, and how the 342-wide sky strip is placed and scrolled | T-014, T-025 | `docs/formats/b@n.md` has no open questions about the layout's fields | A T-090 pass on `mgavideo.dvr` (it names both `.BIN` files) and `xeno.exe`. Also settle what the second packet loop at ~`1fe6:2e4c` decodes (`docs/formats/packed.md`). Lead from T-017: `mgavideo.dvr`'s file-id table has `BACKLAND.BIN` once (id 0x34) and `MSPRITES.BIN` six times (ids 0x36..0x40), matching `msprite0`..`5`. From T-020: `scen06a.usg`'s sky (`BGND`) is `BACKLAND.BIN`, which a stock install lacks; find what the game does then. |
| T-029 | todo | Trace how `.MXX` models are loaded and drawn: the name builder's suffix code in `mgavideo.dvr` can't be reached on disk (short jumps skip it; nothing in the module patches them) and its id range check stops before `obst`. Find who requests ids 0x42..0x98, whether all four suffixes are always loaded, which way facing 0 points, and the drawing routine | T-015, T-025 | `docs/formats/mxx.md` has no open questions about loading, drawing or facing direction | A T-090 pass on `mgavideo.dvr` and `xeno.exe`. Name builder export: host slot `0x480`. From T-016, settle in the drawing code: what the animations are, the frame's `unknown_01`, what a facing with empty frames shows, part draw order, whether colour 0 in a span is drawn, and which palette units use. |

## M2 Window and framebuffer

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-030 | todo | `crates/xeno-render`: `Framebuffer` (indexed 8-bit, logical size from T-012), `Palette`, `blit_sprite`, `fill_rect`, `to_rgba()` | T-005, T-012 | Unit tests on synthetic sprites; snapshot of a small rendered scene | No windowing here. Size is per screen: 320x200 or 320x240. Support a virtual width wider than the visible 320 (640 and 768 are used) with a 1-pixel-precise viewport origin, for the hardware-scrolled screens. |
| T-031 | todo | Window via `winit` 0.30 + `softbuffer` 0.4 in `xeno-app`; show a test pattern; ESC quits | T-001 | Window opens on macOS, Linux, Windows (CI builds; manual run check) | |
| T-032 | todo | Presentation scaler: integer nearest-neighbour scale, letterbox, 4:3 aspect correction (vertical 1.2x); `--scale`, `--aspect` | T-030, T-031 | Test pattern is crisp and aspect-correct at 1080p and 4K | Aspect correction via non-uniform integer scale (e.g. 5x horizontal, 6x vertical) to avoid blur. The factor depends on the mode: 1.2 for 320x200, 1.0 for 320x240 (square pixels). Both modes should fill the same 4:3 frame. |
| T-033 | todo | Fullscreen toggle (`--fullscreen`, F11) and high-DPI handling | T-032 | Correct on a Retina/HiDPI display | |
| T-034 | todo | Fixed-timestep game loop with frame pacing and an input abstraction (keyboard, later mouse) | T-031 | Loop runs at a stable rate independent of vsync | Sim will tick in this loop. The original refreshes at ~70.09 Hz in mode 13h and ~59.94 Hz in 320x240, and `xeno.exe` has a vertical-retrace handler, so retrace-paced effects need the original rate of their mode. |
| T-035 | todo | `xeno-app --view <file>` asset viewer: renders any decoded sprite/screen/animation through the real framebuffer and scaler | T-032, T-011 | Can page through `msprite1.b@n` frames in the window | Doubles as a visual test of parsers. |
| T-036 | todo | Show the title/logo sequence (`ealogo.vcl`, `presents.vcl`) in the window | T-018, T-027, T-035 | Matches DOSBox frame for frame | M2 exit criterion. Needs T-027's effect handlers: the file is one still picture, the sequence entry supplies the motion and fades. Both logos sit in the picture's left 320 columns and start at origin (0, 0). |
| T-037 | todo | Palette effects in `xeno-render`: a shadow palette applied once per (original-rate) retrace, fades through a 64-step ramp, and the colour-animation records (rotate, step through frames) from `mgavideo.dvr` | T-030, T-034 | Unit tests on synthetic records; a fade and a rotation visible through `--view` | Layout and open questions in `docs/formats/palette.md`. First trace what advances the animation pointer and reloads the countdown (a T-090 pass on `mgavideo.dvr` ~0xc66). |
| T-038 | todo | Reproduce the intro drop sequence (`vgaintro.dvr`): backdrop scroll through the CRTC start address, the `.fgd` layers' frame script (state table at driver offset `0x0a82`: accelerate, hold, slow, stop) and frame pacing, and where each part stops | T-030, T-034, T-021 | `docs/formats/fgd.md` has no open questions about timing or end positions; the drop plays through `--view` or the intro | From T-021: frames wait at least two host timer ticks; the cave part keeps the xenodrop frame count, so by our reading it stops well before the start of its layers. Also settle whether colour 0 is black in both backdrop palettes and what rows 128..130 do over the 128-row `cavedrop.pcx`. |
| T-039 | todo | Decode the key and joystick setup in `config.dvr` and its `XENO.KEY` file (not in a stock install): the file's layout, the default keys, and which game action each key slot drives | T-025, T-034 | `docs/formats/key.md` and a parser tested on synthetic data; T-034's input layer uses the default keys | From T-025: `config.dvr` edits a record at host `DS:0xde` (`docs/re/dvr-protocol.md`); the host copies it to `DS:0x220`-`0x229` (CPU class, video, music and sound device letters) and decodes rotated flag words after it (`DS:0x616`, `0x618`, the screenshot switch). Document the whole record here too. From T-024: the `.map` files are not key maps. `config.dvr` offers keyboard, joystick 1 and 2, switched and Amstrad joysticks, joystick alignment and a keyboard remap, and checks for keys used twice. A T-090 pass on the unpacked `config.dvr`. |

## M3 Front end

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-040 | todo | Screen/state machine (`Logo -> Login -> SitRoom -> MapRoom -> Briefing -> Mission -> Review`) with placeholder screens | T-034 | Can step through with keys | |
| T-041 | todo | Login screen from `login.m@l` with text input | T-017, T-040 | Looks like the original | |
| T-042 | todo | Situation room and map room screens | T-041 | Scenario list from `scendef.bin` shown and selectable | From T-019 (`docs/formats/scendef.md`): the map room draws each record's box with a look chosen by its `state` (table at `DS:0x5f5c`) or an "unavailable" look when `unlock_level` is above the player's progress (`DS:0x5f9b`), and hit-tests clicks against the boxes. Still to trace: what each `state` value means, what raises the progress value, and what the draw values look like. |
| T-043 | todo | Unit control/order screens (`humancon`, `scorpcon`, `scoutcon`, `trainmen`) | T-042 | Orders can be set and are stored in sim state | |
| T-044 | todo | Mission briefing and review screens | T-042 | Navigable | |

## M4 Simulation

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-050 | todo | `crates/xeno-sim`: world, grid, unit entities, deterministic RNG, tick function; headless | T-020 | Property test: same seed + inputs => same state hash | |
| T-051 | todo | Load a `.usg` scenario into the sim | T-050 | `scen00a` loads with correct unit count | |
| T-052 | todo | Movement and pathing | T-051 | Units move as ordered | Compare against DOSBox recordings. |
| T-053 | todo | Orders: `SENTRY`, `DETECT`, `ZIGZAG`, `AVOIDANCE`, `FIRE ALL` and the rest found in exe strings | T-052 | Each order has a test | Likely several PRs; one order group each. |
| T-054 | todo | Weapons: laser, plasma, missile; damage; `explode` animation hook | T-053 | A unit can destroy another | |
| T-055 | todo | Structures: `basedome`, `netrelay`, obstacles; win/lose conditions | T-054 | Training mission `ntrain` can be won and lost | M4 exit criterion. |
| T-056 | todo | Mission render: map, units, effects through `xeno-render` | T-050, T-030 | Mission visible and playable | |
| T-057 | todo | Save/resume compatible with the original `<NAME>.LOG` saves where feasible, else own format | T-055 | Round-trip test | From T-024 (`docs/formats/usg.md`, "Save files"): a save is a `.usg` chunk file with an `SDEF` chunk (30 bytes of state, then the `scendef.bin` table with per-scenario `state`). `xeno_formats::usg` keeps `SDEF` unparsed in `other_chunks`; name its fields here. `state.rst` is not a save. |
| T-058 | todo | Decode the `.usg` object bytes past +0x0c (`docs/formats/usg.md`, `DCBS`): heading, orders, damage, the robot layout (0x172 bytes) and the relay/obstacle layout (0x34), and the flag bits at +0x16 | T-020 | `xeno_formats::usg::Object` names the fields the sim needs; `usg.md` lists the rest as unknown | A T-090 pass on `xeno.exe`: start from the per-kind handler tables at `DS:0xe81c` and `DS:0xe83e`, the object-loaded hook `1000:7c84`, and the editor (it writes one value to +0x10 and +0x2a for robots). Needed by T-051 and T-053. |
| T-059 | todo | Settle the rest of the `.usg` world: its size and edges (positions are 16.16; stock objects lie within about 76..175 x 112..163), the `MAPP` record fields (kind 0x14 height and radii, kind 0x12), and how paths use `point_count` and their header bytes | T-020 | `usg.md` has no open questions about the world, terrain or paths | Ground-height code at `1000:436c` and `1000:43d9`; path debug display at `1000:6940`. Needed by T-052 and T-056. From T-024 (`docs/formats/map.md`): the map editor writes a circle as radius `r` at +0x10, `2r/3` at +0x12, `r/3` at +0x14 and `r/3` as 16.16 at +0x0c; `default.map` also has a kind 0x16, which the editor treats as a circle. |

## M5 Audio

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-060 | todo | `crates/xeno-audio` with `rodio` 0.23 output and a mixer; play a synthetic tone | T-001 | Tone audible | |
| T-061 | todo | Digitised SFX playback from `dsounds.bin` | T-023, T-060 | Weapon sounds play in a mission | Mix three voices as `sounddvr.dvr` does (`docs/formats/dsounds.md`): per-voice step, queue flag, volume shift. Needs T-065 to know which event each action plays. |
| T-062 | todo | OPL2 emulation or re-synthesis for `.ms3` music | T-022, T-060 | Main theme plays | Evaluate existing OPL crates first; write up choice. Needs T-063's bank and T-064's tick rate. |
| T-063 | todo | Decode `sounddvr.dvr`'s instrument bank for `.ms3` programs: the per-program tables (pitch-envelope pointers at `0x15b4`, words at `0x0fc9` and `0x10d4`, transpose at `0x14ac`, velocity factor at `0x152e`, MIDI program map at `0x0c53`), the envelope script bytes, and the OPL2 register writes of the AdLib back end's seven routines | T-022 | `docs/formats/ms3.md` (or a new `sounddvr` doc) describes each table; a parser reads the bank from the user's `sounddvr.dvr` at run time | A T-090 pass. Bank data is game data: read it from the user's copy, never commit it. Offsets are load-image offsets (`docs/formats/ms3.md`). Needed by T-062. |
| T-064 | todo | Settle the `.ms3` tick rate and callers: what host slot `0x35c` registers the driver's music hook (`0x01d6`) with (likely the per-frame retrace callback, so 70 or 60 Hz), which `xeno.exe` code plays each song with which queue/loop flags (load entry `0x01e4`, `AX` bits 0 and 1), and whether `lose.ms3` and `robot3.ms3` are ever played | T-022, T-025 | `docs/formats/ms3.md` has no open questions about timing or callers | A T-090 pass on `xeno.exe` from the four `.MS3` name strings. Also check what header byte 0x0f means if anything reads it. From T-025: the hook runs on the per-retrace chain (slot `0x358` adds it, `0x35c` removes it), so 70.09 / 59.94 Hz; `1000:fc36` plays song `BX` from the table at `DS:0xec4c` (`ms3.md`). Left: the `AX` flags and a DOSBox check. |
| T-065 | todo | Name the 29 sound events in `xeno.exe`'s table at `DS:0x7e18` (slot, voice/flags, step override; played through host slot `0x44c` from `1000:ee90`): which game action triggers each, and whether `dsounds.bin` blocks 4 and 15 are ever played | T-023 | `docs/formats/dsounds.md` lists each event with its trigger | A T-090 pass from the callers of `1000:ee90`, `1000:eeb1` and `1000:eec6`. Also read the other three mixer back ends and the volume table at `sounddvr.dvr` `0x1b75`. |

## M6 HD and 4K

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-070 | todo | Make the sprite pipeline resolution-independent: sprites carry a logical size and a scale factor; renderer composes at output resolution when an HD source exists | T-032, T-056 | Original art still pixel-exact; architecture doc written | Prerequisite for everything below. |
| T-071 | todo | xBRZ (or similar edge-aware) scaler, `--filter xbrz` | T-032 | Selectable at runtime; benchmarked with `criterion` at 4K | Pure Rust; CPU cost must stay under frame budget or move to T-073. |
| T-072 | todo | HD asset pack loader: `--hd-pack <dir>` overlays PNGs named by asset id/frame; falls back per-asset | T-070 | A single replaced sprite shows at HD while all others stay original | No pack is ever committed. |
| T-073 | todo | Optional GPU presentation path with `pixels` 0.17 (wgpu 29) behind a cargo feature, enabling shader scalers and a CRT filter | T-032 | Feature builds on all three OSes; `softbuffer` path remains default | Only if T-071 is too slow on CPU or shaders are wanted. |
| T-074 | todo | Vector/hi-res UI text and font so menus are sharp at 4K while art stays original | T-070 | Toggle `--hd-ui` | |

## M7 Release

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-080 | doing | Release workflow, manual `workflow_dispatch` only: run the CI workflow first (via `workflow_call`), build `xeno-app` for macOS (universal), Linux x86_64, Windows x86_64 and attach them to a GitHub release tagged `v<Cargo.toml version>` | T-003 | Artifacts downloadable from a release made by a real run | Releases only from `main`; drafts by default. Linux binary links the runner's glibc (ubuntu-latest); revisit if older distros matter. Workflow merged in PR #3; stays `doing` until a real `workflow_dispatch` run publishes a release with all three binaries. |
| T-081 | todo | First-run experience: if no game dir is found, open a dialog or print instructions pointing at the README section | T-004 | Tested on a clean machine | |
| T-082 | todo | User docs: keybindings, options, troubleshooting | T-081 | README section complete | |

## Ongoing

Standing work that runs alongside the milestones. These tasks are never marked
`done`; each pass is its own small branch/PR. They are not picked by the "lowest
`todo`" rule: take one when the user names it, or when the current task needs
evidence from the executable (for example a format whose layout is only clear from
the code that reads it).

| Id | Status | Task | Depends on | Each pass is done when | Notes |
|---|---|---|---|---|---|
| T-090 | ongoing | Analyse `xeno.exe` and the `*.dvr` modules in Ghidra: identify and rename functions, globals and data structures, and record what was learned in `docs/re/<binary>.md` (e.g. `docs/re/xeno-exe.md`) | T-010 | The Ghidra project is saved, the doc's symbol table has the new entries, and any format facts found are in `docs/formats/` and the `AGENTS.md` table | See "How to do a T-090 pass" below. |

### How to do a T-090 pass

1. **Pick a focus** tied to real work: the subsystem the current or next backlog task
   needs (file loader for a format, video mode setup, palette upload, order
   interpreter, sound driver calls). Start from string xrefs (file names like
   `scendef.bin`, order names like `SENTRY`) and DOS interrupt calls (`int 21h`
   file I/O, `int 10h` video, `int 33h` mouse).
2. **Label in Ghidra** (via the Ghidra MCP): rename functions and globals, set
   prototypes and struct types where clear, and tag each function `confirmed` or
   `guess`. Naming convention: lowercase `subsystem_verb_noun`, with prefixes
   `dos_`, `mem_`, `file_`, `gfx_`, `pal_`, `snd_`, `ui_`, `scen_`, `sim_`, `order_`,
   `drv_` (driver/overlay interface), `rt_` (compiler runtime library). Leave a
   function as `FUN_*` rather than guess a name with no evidence.
3. **Save the program** in the Ghidra project, which lives *outside* the repo
   (`docs/re-setup.md` from T-010 says where).
4. **Record in the repo**, in `docs/re/<binary>.md`:
   - a header naming the unpacked binary it applies to (size and SHA-256 of the
     `unlzexe` output, so addresses are unambiguous);
   - a symbol table: `segment:offset`, name, one-line purpose, confidence, and the
     task id that motivated it;
   - prose notes on structures, globals and algorithms in our own words.
   Never paste decompiled or disassembled listings (see `AGENTS.md`).
5. **Feed the backlog**: confirmed format facts go into `docs/formats/*.md` and the
   `AGENTS.md` table; new work discovered becomes a task in its milestone.
6. Add a row to the pass log below.

#### T-090 pass log

| Date | Binary | Focus | Outcome |
|---|---|---|---|
| 2026-10-21 | `xeno.exe` | `.usg` loader (T-020) | Named the chunk reader and its handlers (`scen_read_*`), `sim_alloc_object`, `sim_advance_clock` and the editor's `ui_draw_editor_status` / `ui_dispatch_editor_key`; layout in `docs/formats/usg.md`. No `docs/re/xeno-exe.md` symbol table yet; addresses are in the format docs. |
| 2026-10-21 | `xeno.exe`, `mapdvr.dvr` | `.map`, `state.rst` and save files (T-024) | Named `file_load_map_terrain`, `scen_write_sdef`, `scen_read_sdef`, `file_write_save_game`, `file_load_save_game` in `xeno.exe`, and `file_write_map`, `ui_load_map`, `ui_apply_map_via_backup`, `ui_leave_map_editor`, `scen_import_terrain_circles` in `mapdvr.dvr`. Findings in `docs/formats/map.md`, `rst.md` and `usg.md`. |
| 2026-10-21 | `xeno.exe`, all `.dvr` | Driver loader protocol (T-025) | Named the slot-table services, module loader, hook-chain and retrace routines in `xeno.exe` (`drv_*`, `gfx_sync_retrace_timer`, `gfx_on_retrace`, `ui_open_file_dialog`, `ui_save_file_dialog`) and each module's `drv_init`/`drv_shutdown`; symbol table in `docs/re/dvr-protocol.md`. `mgavideo.dvr`'s drawing slots `0x4fa`-`0x5f0` and the host text slots `0x38a`-`0x40e` are still unnamed. |

## Parking lot

Ideas that are not yet tasks. Promote to a task with an id when ready.

- Multiplayer / network relay (`netrelay` exists as a structure; original likely had none).
- Replay recording via the deterministic input log.
- Accessibility: remappable keys, colour-blind palettes layered on the original.
- Compare `vgaintro.dvr` and `mgavideo.dvr` to see whether a non-VGA (MCGA?) path exists.

## Changelog of this file

| Date | Change |
|---|---|
| 2026-10-20 | Created with milestones M0 to M7 and initial tasks. |
| 2026-10-20 | Added the `ongoing` status and T-090 (continuous Ghidra analysis and labelling). |
| 2026-10-20 | T-080 changed from tag-triggered to manual-only, gated on the CI workflow. |
| 2026-10-20 | T-002 done. T-003 done (CI green). T-080 still `doing`: merged, awaiting its first real release run. |
| 2026-10-20 | T-004 done; added T-007 (shared file list and case-insensitive file lookup). |
| 2026-10-20 | T-010 done: `docs/re-setup.md`; noted `.dvr` entry-point finding on T-025. |
| 2026-10-20 | T-005 done: `xeno-formats` crate with `FormatError`, `Palette` and `fixtures`. |
| 2026-10-20 | T-006 done: `xeno-tool inspect`. |
| 2026-10-20 | T-007 done: file list in `xeno-formats`, `GameFiles` lookup; moved the `state.rst` / `backup.map` question to T-024. |
| 2026-10-20 | T-011 done: PCX decoder and `xeno-tool dump`. |
| 2026-10-20 | T-012 done: `docs/formats/video-modes.md` (320x200 and 320x240); added T-027; mode notes on T-025, T-030, T-032, T-034. |
| 2026-10-20 | T-013 done: `docs/formats/palette.md`, `xeno-tool palette`; added T-037; palette notes on T-017, T-018. |
| 2026-10-20 | T-014 done: `xeno_formats::packed` and `ban`, `.b@n` in `xeno-tool dump`/`inspect`; added T-028; T-017 now depends on T-014. |
| 2026-10-21 | T-015 done: `xeno_formats::mxx` (view headers), `docs/formats/mxx.md`; added T-029; notes on T-016. |
| 2026-10-21 | T-016 done: `.mXX` animations, frames and images in `xeno_formats::mxx`; `xeno-tool dump` for models; open questions added to T-029. |
| 2026-10-21 | T-017 done: `xeno_formats::mal` (`.m@l` screens), `docs/formats/m@l.md`; notes on T-025 and T-028. |
| 2026-10-21 | T-018 done: `xeno_formats::vcl` decodes the picture, `docs/formats/vcl.md`; `.vcl` in `inspect`/`dump`; sequence-entry leads on T-027; T-036 now depends on T-027. |
| 2026-10-21 | T-019 done: `xeno_formats::scendef`, `docs/formats/scendef.md`; leads on T-020, T-024, T-042, T-057. |
| 2026-10-21 | T-020 done: `xeno_formats::usg`, `docs/formats/usg.md`; added T-058, T-059; `BACKLAND.BIN` lead on T-028; T-090 pass logged. |
| 2026-10-21 | T-023 done: `xeno_formats::dsounds`, `docs/formats/dsounds.md`, WAV output in `xeno-tool dump`; added T-065; notes on T-061. |
| 2026-10-21 | T-024 done: `xeno_formats::map`, `xeno_formats::rst`, `docs/formats/map.md`, `rst.md`, `disk-markers.md`; `default.map` left `EXPECTED_FILES`; added T-039; notes on T-057, T-059. |
| 2026-10-21 | T-025 done: `docs/re/dvr-protocol.md`; settled questions in `ms3.md`, `map.md`, `m@l.md`, `vcl.md`, `palette.md`; notes on T-039, T-064; T-090 pass logged. |
| 2026-10-21 | T-026 done: `xeno-tool inspect --all`; the M1 exit criterion holds on a stock install. |
