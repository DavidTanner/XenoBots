# Reverse-engineering setup

How to build the local workspace used to study `xeno.exe` and the `*.dvr` modules
(T-010, and every T-090 pass after it). Everything this produces lives **outside the
repository**. Nothing in it may be committed: not the unpacked binaries, not the Ghidra
project, not listings. Only conclusions go into the repo (see `AGENTS.md`).

## Layout

One directory in your home folder, never inside the checkout:

```
~/xenobots-re/
  tools/bin/unlzexe        LZEXE decompressor (installed by cargo, below)
  unpacked/xeno.exe        unlzexe output of gameFiles/xeno.exe
  unpacked/config.dvr      unlzexe output of gameFiles/config.dvr
  ghidra/XenoBots.gpr      Ghidra project (+ XenoBots.rep/)
```

Any other location outside the repo works. Docs under `docs/re/` identify binaries by
SHA-256, not by path, so the location doesn't matter.

## Prerequisites

| Tool | Version used | Install |
|---|---|---|
| Rust / cargo | the repo's toolchain | rustup |
| Ghidra | 12.1.4 | macOS: `brew install ghidra` (pulls in `openjdk@21`). Elsewhere: a release zip from github.com/NationalSecurityAgency/ghidra plus JDK 21. |
| `lzexe` crate (`unlzexe` CLI) | 0.1.1 | `cargo install`, below |

The `lzexe` crate is a pure-Rust LZEXE 0.90/0.91/0.91e decompressor (MIT,
github.com/rahulsundaram/lzexe). It has no dependencies or build script, and it runs
the same on macOS, Linux and Windows, which the DOS-era `UNLZEXE.EXE` does not.

## 1. Install `unlzexe`

```bash
cargo install lzexe --version 0.1.1 --locked --root ~/xenobots-re/tools
```

**Always pass an output path.** Called with one argument, `unlzexe` overwrites its
input in place, which would replace your game copy with the unpacked file.

## 2. Unpack the two LZEXE-packed binaries

Only `xeno.exe` and `config.dvr` are packed (`file` reports "LZEXE v0.91
compressed"). The other four `.dvr` files are plain MZ executables. `vga240.mod` is not
an MZ executable at all.

```bash
mkdir -p ~/xenobots-re/unpacked
~/xenobots-re/tools/bin/unlzexe gameFiles/xeno.exe   ~/xenobots-re/unpacked/xeno.exe
~/xenobots-re/tools/bin/unlzexe gameFiles/config.dvr ~/xenobots-re/unpacked/config.dvr
shasum -a 256 gameFiles/xeno.exe gameFiles/config.dvr ~/xenobots-re/unpacked/*
```

Check against the reference copy. If your inputs hash differently, you have a
different release; addresses in `docs/re/` may not apply.

| File | Size (bytes) | SHA-256 |
|---|---|---|
| `gameFiles/xeno.exe` (packed) | 83,832 | `f25619a3c4b349fb9d37b5f645af68984f2ee9bd0e294a08e2a5fc757beaf6ca` |
| `unpacked/xeno.exe` | 181,836 | `e57aec9dd3d3d3d5f7962efd12f121b97f5c58499c4159236f263d5e43d66b36` |
| `gameFiles/config.dvr` (packed) | 9,400 | `df76f56f38a8b9433dbaebe79da7d50941ae204d3acb8c5367ed863e3663bcc8` |
| `unpacked/config.dvr` | 16,160 | `76aa26d15dc6372e0aa9236609702e2bdf1aa0db5f157f0ba464aa867691b8af` |
| `gameFiles/mapdvr.dvr` | 3,708 | `78baa12f1cd4168525edc3f6e823da6ababcf84ecfacb4c89a3b805ec79ac34b` |
| `gameFiles/mgavideo.dvr` | 33,436 | `47ac06c4059c646ab85718afb7796c43080e2c0399b10dbe2d4498ad470fd4ef` |
| `gameFiles/sounddvr.dvr` | 17,260 | `c29cb1118ea2c427c056fa88a311e72ef6789b1270b28262d3998f0ee33d4734` |
| `gameFiles/vgaiff.dvr` | 7,452 | `462b1d3137bca92d46dfa31f497255a6429a0c319994b26f5f01f1ee9ee59183` |
| `gameFiles/vgaintro.dvr` | 86,129 | `0581ed3b1f43861d7d2925d1032aa537a0f9390a578c52e7b284aa95839a6482` |
| `gameFiles/vga240.mod` | 4,654 | `275247c45b9bfd94e285af599723c691aeb90cdb02113ede54327677909d0144` |

Sanity checks on the unpacked MZ headers:

- `xeno.exe`: 655 relocations, 192-paragraph header, entry `CS:IP = 0000:0002`,
  `SS:SP = 27DE:0400`. Its strings now include the scenario file names and order
  names, which the packed file hides.
- `config.dvr`: 13 relocations, 32-paragraph header, entry `0000:0000`.

## 3. Create the Ghidra project

1. Start Ghidra (`ghidraRun`). **File > New Project > Non-Shared Project**, directory
   `~/xenobots-re/ghidra`, name `XenoBots`.
2. **File > Import File** for each binary, setting *Destination Folder* as below.
   Ghidra detects *Old-style DOS Executable (MZ)* and language `x86:LE:16:Real Mode`;
   keep both.

   | Project folder | Import from |
   |---|---|
   | `/unpacked` | `~/xenobots-re/unpacked/xeno.exe`, `~/xenobots-re/unpacked/config.dvr` |
   | `/dvr` | `gameFiles/mapdvr.dvr`, `mgavideo.dvr`, `sounddvr.dvr`, `vgaiff.dvr`, `vgaintro.dvr` |
   | `/mod` | `gameFiles/vga240.mod` (see below) |

   `vga240.mod` has no header, so Ghidra imports it as a *Raw Binary*. Set the language
   to `x86:LE:16:Real Mode` yourself and keep base address `0000:0000`; the module is
   loaded at offset 0 of its own segment. Skip auto-analysis: it finds nothing from
   offset 0. Disassemble from the jump targets in the stub table at offset 0 instead
   (see `docs/formats/video-modes.md`).

3. Open each program and run auto-analysis with the default analysers. Then
   **File > Save All**.

Never import the *packed* `xeno.exe` or `config.dvr` for analysis. Ghidra loads them
without complaint, but all it can see is the LZEXE stub (two functions).

Expected function counts after default auto-analysis, as a quick check that the right
files went in:

| Program | Functions |
|---|---|
| `/unpacked/xeno.exe` | 256 |
| `/unpacked/config.dvr` | 60 |
| each `/dvr/*.dvr` | 1 or 2 |
| `/mod/vga240.mod` | 0 (not auto-analysed) |

The driver modules analyse to almost nothing because the code that matters is not
reached from the MZ entry point: `xeno.exe` loads them and calls in through slots.
`docs/re/dvr-protocol.md` (T-025) lists the real entry points.

## 4. Optional: Ghidra MCP for Claude Code

Claude Code drives Ghidra through an MCP bridge to a running Ghidra GUI. The setup
used here is `bethington/ghidra-mcp` (GitHub): install its Ghidra extension, enable
the plugin in the CodeBrowser, and register its `bridge-mcp-ghidra` command as an MCP
server named `ghidra` (for example with `uv run --directory <checkout>
bridge-mcp-ghidra`). With Ghidra open on the `XenoBots` project, the
`mcp__ghidra__*` tools see the programs above. Any other Ghidra MCP server also
works; the docs only depend on the program names and addresses.

## After setup

- Analysis work is T-090. Record findings in `docs/re/<binary>.md`, headed with the
  SHA-256 of the *unpacked* binary from the table above.
- Back up `~/xenobots-re/ghidra/` yourself if your labels matter to you. The repo
  never holds it, so the doc tables are what other contributors get.
