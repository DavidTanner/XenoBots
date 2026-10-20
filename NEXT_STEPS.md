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
| T-001 | todo | Create the Cargo workspace: root `Cargo.toml` (`resolver = "3"`, edition 2024), `rust-toolchain.toml` (`channel = "stable"`), `rustfmt.toml`, and an empty `crates/xeno-app` binary that prints its version | | `cargo run -p xeno-app` prints a version string | Commit `Cargo.lock` (this is an application). |
| T-002 | todo | Repo hygiene: add `target/`, `.idea/`, `*.iml`, `.vscode/`, `out/`, `extracted/`, `dump/`, `*.png` outside `docs/` and a `# game data` section to `.gitignore`; add a `docs/formats/README.md` stub | | `git status` is clean after a build and an asset dump into `out/` | Keep the existing `gameFiles/` line. |
| T-003 | todo | GitHub Actions CI: fmt check, clippy `-D warnings`, `cargo test --workspace` on ubuntu-latest, macos-latest, windows-latest; cache cargo | T-001 | Green run on a PR | Install the Linux apt packages listed in README. |
| T-004 | todo | `xeno-app --game-dir` / `XENOBOTS_GAME_DIR` / `./gameFiles` resolution, plus a startup check that lists missing expected files by name | T-001 | Running without files prints a clear, actionable error and exits non-zero | Expected-file list lives in `xeno-formats` later; start with a const array. |
| T-005 | todo | Add `crates/xeno-formats` library with a `FormatError` type, a `Palette` type (256 x RGB), and the test-fixture helper `synthetic_bytes()` | T-001 | Crate compiles with one passing test | No parsers yet. |
| T-006 | todo | Add `crates/xeno-tool` CLI skeleton with `inspect <file>` that prints size, extension, and a 64-byte hex preview | T-005 | Works on any file | Generic; per-format output comes with each parser. |

## M1 Formats

Each parser task: `binrw` struct in `xeno-formats`, `parse()` entry point, synthetic
unit test, `insta` snapshot of the synthetic parse, an `inspect` branch in
`xeno-tool`, and `docs/formats/<ext>.md` with offsets and open questions.

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-010 | todo | Set up reverse-engineering workspace *outside the repo*: unpack `xeno.exe` and `config.dvr` with `unlzexe`, import into Ghidra; write `docs/re-setup.md` describing the steps (no binaries, no listings) | | Doc exists; a teammate can reproduce the setup | Ghidra MCP is available to Claude in this project. |
| T-011 | todo | PCX decoder (v3.0, 8-bit RLE, 256-colour palette trailer) | T-005 | Synthetic PCX round-trips; `xeno-tool dump` writes a PNG to an out dir | `image` crate has no PCX; ~80 lines. |
| T-012 | todo | Identify the video mode(s): is the logical framebuffer 320x200, 320x240 Mode-X (`vga240.mod`), or both? Record in `docs/formats/video-modes.md` | T-010 | Doc states the mode(s) with evidence | Drives T-030. |
| T-013 | todo | Palette extraction: find where the 256-colour palette(s) come from (PCX trailer, embedded in exe, or a separate file) | T-011, T-012 | `xeno-tool palette <source>` prints 256 RGB triples | |
| T-014 | todo | `.b@n` sprite banks (`msprite*`, `sky_*`): header, frame count, per-frame dimensions, pixel encoding | T-013 | Frames render to PNG in an out dir | Start with `msprite0.b@n` (618 bytes, likely few frames). |
| T-015 | todo | `.m00/.m08/.m16/.m32` unit models: establish what the four suffixes mean (LOD? zoom level? facing count?) and parse the header | T-013 | Doc + header struct; pixel data may remain `unknown` | Most `.m00` are exactly 18 bytes. |
| T-016 | todo | `.m00/.m08/.m16/.m32` pixel data | T-015 | Unit sprites render to PNG | |
| T-017 | todo | `.m@l` screen layouts (`login`, `sitroom`, `maproom`, ...) | T-013 | At least one screen renders to PNG | |
| T-018 | todo | `.vcl` cutscenes/animations: container structure and frame decoding | T-013 | `ealogo.vcl` (smallest) renders its frames | Likely delta-coded; expect 2 PRs. |
| T-019 | todo | `scendef.bin` scenario definition table | T-010 | Struct with names/indices of the 13+4 scenarios | 410 bytes; small. |
| T-020 | todo | `.usg` scenario files: header, map grid, unit placements, objectives | T-019 | `scen00a.usg` inspect prints map size and unit list | |
| T-021 | todo | `.fgd` layers and `fgarray.kyl` (all 1920 bytes): determine grid shape (80x24 vs 48x40) and meaning | T-010 | Doc + parser | |
| T-022 | todo | `.ms3` music format: identify (AdLib register stream? custom sequencer?) | T-010 | Doc; parser if feasible | Audio playback is M5. |
| T-023 | todo | `dsounds.bin` digitised SFX: directory structure, sample rate, encoding | T-010 | `xeno-tool dump` writes WAVs to an out dir | |
| T-024 | todo | `.map` key/joystick mapping, `state.rst` save state, `xenobot.1/.2` markers | T-010 | Doc for each; parsers for `.map` and `.rst` | Small. |
| T-025 | todo | `.dvr` modules: document the loader protocol between `xeno.exe` and the driver modules | T-010 | Doc explaining what each driver provides | Needed to know which behaviour lives where. |
| T-026 | todo | `xeno-tool inspect` runs over a whole directory (`inspect --all <dir>`) and summarises parse success per extension | T-006 | Table output; exit code 0 only if every file parsed | M1 exit criterion. |

## M2 Window and framebuffer

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-030 | todo | `crates/xeno-render`: `Framebuffer` (indexed 8-bit, logical size from T-012), `Palette`, `blit_sprite`, `fill_rect`, `to_rgba()` | T-005, T-012 | Unit tests on synthetic sprites; snapshot of a small rendered scene | No windowing here. |
| T-031 | todo | Window via `winit` 0.30 + `softbuffer` 0.4 in `xeno-app`; show a test pattern; ESC quits | T-001 | Window opens on macOS, Linux, Windows (CI builds; manual run check) | |
| T-032 | todo | Presentation scaler: integer nearest-neighbour scale, letterbox, 4:3 aspect correction (vertical 1.2x); `--scale`, `--aspect` | T-030, T-031 | Test pattern is crisp and aspect-correct at 1080p and 4K | Aspect correction via non-uniform integer scale (e.g. 5x horizontal, 6x vertical) to avoid blur. |
| T-033 | todo | Fullscreen toggle (`--fullscreen`, F11) and high-DPI handling | T-032 | Correct on a Retina/HiDPI display | |
| T-034 | todo | Fixed-timestep game loop with frame pacing and an input abstraction (keyboard, later mouse) | T-031 | Loop runs at a stable rate independent of vsync | Sim will tick in this loop. |
| T-035 | todo | `xeno-app --view <file>` asset viewer: renders any decoded sprite/screen/animation through the real framebuffer and scaler | T-032, T-011 | Can page through `msprite1.b@n` frames in the window | Doubles as a visual test of parsers. |
| T-036 | todo | Show the title/logo sequence (`ealogo.vcl`, `presents.vcl`) in the window | T-018, T-035 | Matches DOSBox frame for frame | M2 exit criterion. |

## M3 Front end

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-040 | todo | Screen/state machine (`Logo -> Login -> SitRoom -> MapRoom -> Briefing -> Mission -> Review`) with placeholder screens | T-034 | Can step through with keys | |
| T-041 | todo | Login screen from `login.m@l` with text input | T-017, T-040 | Looks like the original | |
| T-042 | todo | Situation room and map room screens | T-041 | Scenario list from `scendef.bin` shown and selectable | |
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
| T-057 | todo | Save/resume compatible with `state.rst` where feasible, else own format | T-055 | Round-trip test | |

## M5 Audio

| Id | Status | Task | Depends on | Done when | Notes |
|---|---|---|---|---|---|
| T-060 | todo | `crates/xeno-audio` with `rodio` 0.23 output and a mixer; play a synthetic tone | T-001 | Tone audible | |
| T-061 | todo | Digitised SFX playback from `dsounds.bin` | T-023, T-060 | Weapon sounds play in a mission | |
| T-062 | todo | OPL2 emulation or re-synthesis for `.ms3` music | T-022, T-060 | Main theme plays | Evaluate existing OPL crates first; write up choice. |

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
| T-080 | todo | Release workflow: tag => build `xeno-app` for macOS (universal), Linux x86_64, Windows x86_64 and attach to a GitHub release | T-003 | Artifacts downloadable | |
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
