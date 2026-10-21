# File formats

One markdown file per original file type, named after its extension
(`pcx.md`, `usg.md`, `b@n.md`, ...). Each one records what is known about the format
so the next person starts from facts rather than guesses. The summary table of all
types and their status lives in `AGENTS.md` ("The original game's files"); keep the
two in step.

Documented so far: [`pcx.md`](pcx.md). Parser tasks in `NEXT_STEPS.md` (milestone
M1) add the rest here.

## What goes in a format doc

- **Status**: `confirmed`, `partial` or `guess`, and the evidence (a parser that
  reads every file in a stock install, code in `xeno.exe`, a matching DOSBox capture).
- **Files**: which files in the install use the format, and their sizes.
- **Layout**: a table of offset, size, type, endianness and meaning. Unknown fields
  are listed as `unknown_XX` with whatever has been observed about them.
- **Encoding**: compression, pixel or sample encoding, palette source.
- **Open questions**: what is still unexplained.
- **Parser**: the `xeno-formats` module that reads it and the backlog task that
  added it.

## What never goes in a format doc

Nothing derived from the game data itself: no hex dumps, byte arrays, decoded
images, palettes, strings tables or disassembly listings. Short structural notes
("offset 0x10 is a u16 LE frame count") are fine. See `AGENTS.md`, "Never commit game
data".
