# CLAUDE.md

@AGENTS.md

Claude-specific additions to the shared agent guidance above.

## Start here

1. Read `NEXT_STEPS.md` and pick the lowest-numbered task that is `todo` and whose
   dependencies are `done`, unless the user names one.
2. Say which task you are taking, do only that task, and update its status in
   `NEXT_STEPS.md` in the same change.
3. Before any commit: `git status` and confirm nothing from `gameFiles/` or any
   dump/extract directory is staged. This is the one rule that must never be broken.

## Tools available to you here

- **Ghidra MCP** (`mcp__ghidra__*`) is available for analysing `xeno.exe` and the
  `*.dvr` modules. `xeno.exe` and `config.dvr` are LZEXE 0.91 packed; unpack with
  `unlzexe` into a path outside the repo (for example the session scratchpad or
  `~/xenobots-re/`) before importing. Keep the Ghidra project outside the repo.
  Write *conclusions* (format layouts, constants, algorithms) into `docs/formats/`;
  never paste decompiled listings into the repo.
- The game files live in `./gameFiles/` locally. They are gitignored. Read them freely
  with `xxd`, `file`, `strings` and `xeno-tool`; never copy them anywhere tracked.
- `cargo search` / `cargo info` work for checking crate versions. Prefer the versions
  in the AGENTS.md table; bump only in a dedicated task.

## Working style for this repo

- Keep PRs to a single `NEXT_STEPS.md` task. If you discover more work, add new
  tasks to the backlog instead of expanding the current one.
- When you confirm or refute a format guess, edit the table in `AGENTS.md`
  ("The original game's files") and the matching `docs/formats/*.md` so the next
  agent starts from facts.
- Prefer `binrw` structs with explicitly named `unknown_*` fields over hand-rolled
  byte slicing; it documents the format as it parses it.
- Use the session scratchpad for asset dumps and experiments, not the repo.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` before declaring a task done. Report failures verbatim.
- For a branch-wide bug pass, run `/code-review`; for cleanup, `/simplify`.

## Things that are easy to get wrong

- 320x200 VGA pixels are not square. Presentation scaling must apply the 4:3 aspect
  correction (vertical factor 1.2) or the game looks squashed on modern displays.
- The game may switch to a 320x240 Mode-X mode (`vga240.mod`). Confirm the actual
  mode(s) before hardcoding the logical framebuffer size.
- `.m00` files are mostly 18 bytes. Treat them as headers, not empty models.
- Do not reach for a game engine; see "Tech stack" in AGENTS.md.
