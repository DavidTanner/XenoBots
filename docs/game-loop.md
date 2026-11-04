# Game loop and input (T-034)

How `xeno-app` keeps the original's timing and turns the host's input into the game's.
The code is `crates/xeno-app/src/clock.rs`, `translate.rs` and `window.rs`, and
`crates/xeno-sim/src/rate.rs` and `input.rs` (T-050).

## The clock

The original paces everything from the vertical retrace (`docs/re/dvr-protocol.md`,
"Hook chains"): the music (`ms3.md`), fades (`palette.md`), cutscene pans, captions and
hold times (`vcl.md`). So the port's fixed timestep is one **retrace** at the rate of
the current screen mode, not the host display's refresh.

| Mode | Dot clock | Dots a line | Lines a frame | Rate |
|---|---|---|---|---|
| 320x200, mode 13h | 25.175 MHz | 800 (CRTC horizontal total `5Fh` + 5 characters of 8 dots) | 449 (vertical total `1BFh` + 2) | 25,175,000 / 359,200 = **70.0863 Hz** |
| 320x240 (`vga240.mod`) | 25.175 MHz | 800 | 525 (mode 12h's vertical total `20Bh` + 2; `video-modes.md`) | 25,175,000 / 420,000 = **59.9405 Hz** |

The register values are the standard BIOS mode 13h and 12h tables (IBM VGA Technical
Reference); 25.175 MHz is the VGA's nominal 25 MHz clock.

- **Exact counts.** A rate is kept as the fraction above. The number of retraces due is
  recomputed each time from the elapsed time since the clock's epoch,
  `floor(elapsed_ns * num / (den * 10^9))` in 128-bit integers, never by adding
  periods, so it never drifts. A test runs an hour of uneven 1 to 40 ms waits and gets
  exactly 252,310 retraces at 70 Hz and 215,785 at 59.94 Hz.
- **Time source.** The clock is handed the time (a `Duration` since the window opened)
  rather than reading it, so the tests drive it with made-up times.
- **Catch-up cap.** One wake-up runs at most 10 retraces (`MAX_CATCH_UP`, about 140 ms
  at 70 Hz), so a short hiccup is caught up unseen. After a longer stall (a debugger, a
  blocked event loop, a sleeping laptop) the rest are **dropped**: the clock restarts
  from that moment, so the game resumes where it was, as if paused, instead of racing.
  Dropped retraces are counted and logged. At a game speed above 1x the cap is 10
  real retraces, each its N steps ("Game speed (T-215)" below).
- **The 60 Hz chain.** The original also runs a 60 Hz hook chain, advanced from each
  retrace by the frame period (it also drives the game clock `DS:0x374`). The port
  derives it the same way: after retrace `n` the 60 Hz count is
  `floor(n * 60 * den / num)`, so 0 or 1 ticks fire on a 70 Hz retrace and 1 or 2 on a
  59.94 Hz one. The original's 19,886 PIT ticks make its rate 60.0001 Hz; the port uses
  exactly 60. Each step reports the count, so its users (T-050's sim, perhaps) can run
  it.
- **Mode changes.** The original re-measures the frame period after every mode change
  (slot `0x348`), so the port restarts the clock at the new mode's rate, from the
  wake-up at which the change happened (T-036): the start-up cutscene runs at the
  320x240 rate, and when it ends the window goes on to the intro drop (T-038) and then
  the screens (T-040, `docs/screens.md`) or the test card, all in mode 13h at 70.09 Hz (the clock restarts at each change, as
  the drop's driver re-measures after entering and leaving its mode). A cutscene after
  a mission (T-045) switches to the 320x240 rate when the screens hand it over and back
  to 13h when it ends. Retraces still due at the old rate in that wake-up are dropped. The input
  log keeps counting across the change, and the rate log restarts. `--view` of a `.vcl`
  picture (T-035) runs at the 320x240 rate throughout, everything else at mode 13h's.

## Pacing

`window.rs` runs the retraces that are due after every batch of window events, then
sets `ControlFlow::WaitUntil` for the next one; if any ran, it requests a redraw. Each
frame shows the latest state.

- **Vsync.** The game rate comes from the clock, never from presenting frames, so it
  is the same on any display. On a 60 Hz monitor, about 10 of every 70 frames are never
  shown; on a 120 or 144 Hz monitor some frames are shown twice. The moving marker can
  then look slightly uneven, but it always covers 70 pixels a second. Matching the
  monitor exactly would need frame interpolation, which a pixel-exact port shouldn't do.
- **Timer precision.** A late wake-up only delays a retrace; the next one is still due
  on its exact time. On Windows winit uses a high-resolution waitable timer.
- **Hidden or minimised.** The game keeps running (music and cutscenes keep time);
  only the drawing is skipped while the window has no size. If the system throttles a
  hidden app's timers (macOS App Nap), wake-ups longer than the catch-up cap drop time.
- **Measuring.** With `RUST_LOG=xeno_app=debug`, `xeno-app` logs once a second the
  retraces run, the 60 Hz ticks and the frames drawn, per second and since the window
  opened, and any retraces dropped. The test card's white marker steps one pixel per
  retrace and its yellow one per 60 Hz tick.

## Game speed (T-215)

**Not in the original**, and off by default: a deliberate departure from "faithful
first", asked for by the user. `--speed N` (1, 2 or 4) or F12 in the window (1x, 2x,
4x, then 1x again) runs a mission in play N times as fast. The window title shows the
speed when it isn't 1x.

- **More steps, not shorter ones.** The clock (`Clock::at_speed`) still wakes once per
  real retrace and then runs N retrace steps, each exactly the step it is at 1x: the
  sim, the robots' programs, the game clock, the views' turns (T-209's passes count
  steps) and colour records advance N retraces for each real one. Only the last
  step's frame is shown, since the window draws after the batch.
- **Deterministic.** The steps are numbered on in the input log, and the host's events
  all go to the first step of each real retrace (`Step::extra` is set on the others),
  so a replay of the log gives the same state at any speed. Every step still takes
  the console arrows' own repeats (T-207), which therefore count game steps: a held
  arrow turns the robot, and changes gear, N times as fast, with the first repeat
  after 35 steps (125 ms at 4x). F12 is a window
  key, like F11: it never reaches the game or the log.
- **What follows it.** Only a mission in play (`Screens::in_play`: a mission screen,
  not paused by F1 and with no file dialog open). Inside it everything the retrace
  drives runs faster, the sound effects too (they play as the sim posts them, so more
  of them a second), and a view's fades. The music keeps real time (its driver is
  ticked once per real retrace), and so does the mouse. The cutscenes, the intro drop, the map
  room and every other screen stay 1x, so the chosen speed takes effect on entering a
  mission and stops on leaving it, pausing or opening a dialog. Each such change
  restarts the clock at the new speed from that wake-up (the log counting on), and the
  steps still due at the old speed in that wake-up are dropped, as at a mode change.
- **Catch-up cap.** One wake-up runs at most 10 real retraces, 10 N steps; after a
  longer stall the rest are dropped as at 1x, so a machine too slow for N steps a
  retrace runs the mission slower than N but never spirals.
- **Cost.** In a release build on the developer's machine (Apple silicon), four mission
  steps take about 0.5 ms a real retrace on a humanoid console and 0.6 ms in the
  situation room (worst 0.8 ms over a minute), against a 14.3 ms budget at 70 Hz: 4x
  fits, with the present step (under 2 ms at 5K) on top.
- **1x is unchanged.** At speed 1 no step is extra and the clock never restarts for
  the speed, so the loop is exactly what it was.

## Input

`translate.rs` turns winit events into `input::InputEvent`s and window actions; nothing
else sees winit.

- **Keys** are IBM PC scan codes, set 1 (`ScanCode`), as the original's input queue
  holds them (`vcl.md`: event 0x19 is P). They are mapped by physical position, from
  Microsoft's "Keyboard Scan Code Specification" (rev. 1.3a), appendix C. `E0`-prefixed
  keys (cursor block, right Ctrl and Alt, keypad Enter and /) are `0xE0xx`. Every key of
  a 101- or 102-key PC keyboard is mapped except Pause and Print Screen (multi-byte
  sequences); later keys, such as the Windows keys, are not. The original's own keys
  are fixed tables (`docs/formats/key.md`, "The game's keys").
- **Stick keys** (`keymap.rs`, T-039): the six keys per player that `config.dvr`
  remaps (up-left, up, up-right, left, right, fire), read at start-up from the user's
  `XENO.CFG` or else from `config.dvr`'s defaults. `KeyMap::slots` gives the players and
  slots a scan code drives; an `E0` key counts as its plain twin, as `config.dvr`
  records them. The original never reads these keys, so only the test card uses them,
  colouring them in its key grid.
- **Events**: `KeyDown { code, repeat }`, `KeyUp { code }` and `Text(char)` (printable
  characters with the layout and Shift applied, for T-041's name entry). A release is
  sent only for a key that went down, and losing focus releases every held key.
  `Keys` is the set of held keys. On a robot console the arrows' repeats are the
  port's own, not the system's (T-207, `docs/sim.md`, "The cockpit keys (T-105)").
- **Window keys** never reach the game. F11, or Alt+Enter (either Enter), toggles
  fullscreen; holding them does not repeat. ESC leaves fullscreen, or quits from a
  window. A screen that wants ESC itself sets `Translator::screen_claims_escape`
  (T-040: a screen claims it when its key table binds ESC, `docs/screens.md`); then ESC
  is an ordinary key, and F11, Alt+Enter or closing the window remain. The claim is
  updated after every retrace, so it applies from the next key. Closing the window
  quits, and so does a screen's quit key.
- **Screens** (T-040) turn each key going down into the original's key word (modifiers
  in the high byte, `E0` dropped) and look it up in their (key word, action) tables
  (`docs/screens.md`).
- **Per retrace.** Events that arrive between two retraces all belong to the next one.
  Each retrace records them in an `InputLog` and then reads them back from it, so the
  game sees input only through the log, and a log replays exactly. The log is in
  memory only for now.
- **Mouse** (T-040): `CursorMoved` (physical pixels, sent in fullscreen with the cursor
  hidden too) is mapped to a view pixel with the inverse of the presented layout
  (`xeno_render::Layout::view_point`: subtract the picture's corner, divide by the
  factors, round down), clamped to the view, so the black bars and points outside the
  window give the nearest edge, as a DOS mouse stops at the screen's edge. The game
  gets `MouseMove { x, y }` when that pixel changes (also when a resize moves the
  picture under a still cursor), `MouseDown` only on the picture and once the pointer
  is known, and `MouseUp` for a button it saw go down (so a drag released off the
  picture, or losing focus, never leaves one stuck). They are queued and logged per
  retrace like keys, and `input::Mouse` holds the pointer and buttons. While the
  screens are up the window draws the original's pointer (T-042: `mgavideo.dvr`'s
  shape, placed by its hot spot, `docs/screens.md`) into the framebuffer for each
  frame (saving and restoring the pixels under it, as the original's pointer slots
  do) and hides the system cursor over the picture; on the bars and outside the system
  cursor shows.

## Where it goes later

The input types (`ScanCode`, `InputEvent`, `Keys`, `Mouse`, `InputLog`) moved to
`xeno-sim` (T-050, `docs/sim.md`), since the sim is driven by retraces and replays
input logs, and so did the clock's `Rate`; the stepping, pacing and winit
translation stay in `xeno-app`.
