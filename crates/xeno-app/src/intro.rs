//! The start-up cutscene (T-036): the ten-entry intro chain of `xeno.exe`, `EALOGO` to
//! `PRESENTS`, played in the 320x240 mode as the original's sequence player
//! (`1fe6:8fc4`) and `mgavideo.dvr`'s palette driver do it (`docs/formats/vcl.md`,
//! `palette.md`). The entries are read from the user's own `xeno.exe`.
//!
//! The original is busy-waiting code run between retrace interrupts. Here each entry
//! becomes a list of [`Op`]s, and every retrace first does what the interrupt does
//! (count, apply the pending origin, upload the palette and step a fade, count a
//! 60 Hz tick) and then runs ops until one has to wait. So changes show at the same
//! retrace as in the original, if the code between retraces took no time (loading a
//! picture from disk did take time, which is not modelled).
//!
//! Not yet: the captions (they need the game's font and glyph renderer; no entry's
//! timing depends on them) and the music, whose start resets the retrace counter
//! `DS:0x370` that the end times count from. Without music the counter starts with the
//! sequence.

use std::collections::VecDeque;

use xeno_formats::sequence::{self, Effect, Entry};
use xeno_formats::{lzexe, vcl};
use xeno_render::{DAC_BYTES, Dac, Fade, Framebuffer, Palette, Size};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, ScanCode};

/// P pauses until the next key (the abort hook `1000:fccf`).
const PAUSE: ScanCode = ScanCode(0x19);
/// Video memory in the 320x240 mode: 640 wide, 400 rows (`video-modes.md`).
pub const SURFACE: Size = Size::new(640, 400);
/// Ticks the chain waits before its first entry (`1000:fd0a`).
const LEAD_IN: u32 = 30;
/// Retraces of the fade after an abort.
const ABORT_FADE: u16 = 32;

/// A loaded picture and its palette in DAC levels.
#[derive(Debug, Clone)]
pub struct Picture {
    pub name: &'static str,
    pub dac: [u8; DAC_BYTES],
    /// `640 * rows` pixels.
    pub pixels: Vec<u8>,
}

/// Reads the intro chain from `xeno.exe` and loads its pictures.
pub fn load(files: &GameFiles) -> Result<Intro, String> {
    let exe = files
        .read("xeno.exe")
        .map_err(|e| format!("cannot read xeno.exe: {e}"))?;
    let exe = lzexe::unpack(&exe).map_err(|e| format!("xeno.exe: cannot unpack it: {e}"))?;
    let chain = sequence::intro_chain(&exe)
        .map_err(|e| format!("xeno.exe: cannot find the intro sequence: {e}"))?;
    let pictures = chain
        .iter()
        .map(|entry| {
            let name = entry
                .file_name()
                .expect("sequence::parse checks the file id");
            let bytes = files
                .read(name)
                .map_err(|e| format!("cannot read {name}: {e}"))?;
            let picture = vcl::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
            let mut dac = [0; DAC_BYTES];
            dac.copy_from_slice(&bytes[vcl::PALETTE_OFFSET..][..DAC_BYTES]);
            Ok(Picture {
                name,
                dac,
                pixels: picture.pixels,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    log::info!(
        "intro: {} entries from xeno.exe ({})",
        chain.len(),
        pictures
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(Intro::new(&chain, pictures))
}

/// What a poll of the input does when a key other than P is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnKey {
    /// The lead-in's caller ignores the result, so the key is lost.
    Ignore,
    Abort,
    /// A pan's poll first puts a fading-in palette back (slot `0x4e8`).
    RestoreAndAbort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Entry `n` begins (for the log).
    Begin(usize),
    /// Slot `0x4b4`: the palette to black.
    Blank,
    /// Until the shadow palette has been uploaded.
    WaitUpload,
    Poll(OnKey),
    /// Until the 60 Hz clock moves.
    WaitTick,
    /// Until the retrace counter has moved this many times.
    WaitRetraces(u16),
    /// Slot `0x490`: picture `n` into video memory from the top.
    Load(usize),
    /// Slot `0x60e`: once the last origin has been applied, set this one; it is
    /// applied at the next retrace.
    Origin(usize, usize),
    Step(i16, i16),
    /// Slot `0x4d8`: black, then (after [`Op::WaitUpload`]) [`Op::StartFade`] up to
    /// picture `n`'s palette.
    PrepareFadeIn(usize, u16),
    StartFade,
    /// Poll each retrace until the counter reaches the end time.
    HoldUntil(u16),
    /// Slots `0x4c8`/`0x4c0`: stop any fade; one retrace later, fade the DAC out.
    Stop,
    FadeOut {
        last: u8,
        length: u16,
        max: u16,
    },
    WaitFade,
    End,
}

/// A loop count as `LOOP` sees it: 0 runs 65,536 times.
fn times(n: u16) -> u32 {
    if n == 0 { 0x10000 } else { u32::from(n) }
}

/// The ops for a chain of entries, picture `i` being entry `i`'s.
fn compile(entries: &[Entry]) -> Vec<Op> {
    let mut ops = vec![Op::Blank, Op::WaitUpload];
    for _ in 0..LEAD_IN {
        ops.extend([Op::Poll(OnKey::Ignore), Op::WaitTick]);
    }
    for (i, e) in entries.iter().enumerate() {
        ops.extend([
            Op::Begin(i),
            Op::Blank,
            Op::WaitUpload,
            Op::Poll(OnKey::Abort),
            Op::Load(i),
            Op::Poll(OnKey::Abort),
            Op::Origin(usize::from(e.x), usize::from(e.y)),
            Op::PrepareFadeIn(i, e.fade_in),
            Op::WaitUpload,
            Op::StartFade,
        ]);
        match e.effect() {
            Some(Effect::Hold) | None => {
                for _ in 0..times(e.length) {
                    ops.extend([Op::Poll(OnKey::Abort), Op::WaitTick]);
                }
            }
            Some(Effect::Pan { dx, dy }) => {
                // `1fe6:9132`: a step, then `length - 2` polled steps, then a last one,
                // each after the pan speed's wait.
                let wait = (e.pan_speed > 0).then_some(Op::WaitRetraces(e.pan_speed));
                ops.push(Op::Step(dx, dy));
                ops.extend(wait);
                for _ in 0..times(e.length.wrapping_sub(2)) {
                    ops.push(Op::Poll(OnKey::RestoreAndAbort));
                    ops.extend(wait);
                    ops.push(Op::Step(dx, dy));
                }
                ops.extend(wait);
                ops.push(Op::Step(dx, dy));
            }
        }
        // The caption is not drawn yet; its poll stays.
        ops.extend([
            Op::Poll(OnKey::Abort),
            Op::HoldUntil(e.end_time),
            Op::Stop,
            Op::WaitRetraces(1),
            Op::FadeOut {
                last: 251,
                length: e.fade_out,
                max: e.fade_out,
            },
            Op::WaitFade,
            Op::Stop,
            Op::WaitRetraces(1),
            Op::FadeOut {
                last: 255,
                length: e.fade_out / 2,
                max: u16::MAX,
            },
            Op::WaitFade,
        ]);
    }
    ops.push(Op::End);
    ops
}

/// The player's state.
#[derive(Debug, Clone)]
pub struct Intro {
    entries: Vec<Entry>,
    pictures: Vec<Picture>,
    ops: Vec<Op>,
    pc: usize,
    dac: Dac,
    prepared: Option<Fade>,
    /// The retrace counter `DS:0x370`.
    counter: u16,
    /// Retraces and 60 Hz ticks so far, and the value of one when a wait began.
    retraces: u64,
    ticks: u64,
    since: Option<u64>,
    /// The origin last set, and one waiting for the next retrace.
    origin: (usize, usize),
    pending: Option<(usize, usize)>,
    keys: VecDeque<ScanCode>,
    paused: bool,
    /// The entry playing.
    entry: usize,
    aborted: bool,
    finished: bool,
}

impl Intro {
    pub fn new(entries: &[Entry], pictures: Vec<Picture>) -> Intro {
        assert_eq!(entries.len(), pictures.len(), "one picture an entry");
        Intro {
            entries: entries.to_vec(),
            pictures,
            ops: compile(entries),
            pc: 0,
            dac: Dac::default(),
            prepared: None,
            counter: 0,
            retraces: 0,
            ticks: 0,
            since: None,
            origin: (0, 0),
            pending: None,
            keys: VecDeque::new(),
            paused: false,
            entry: 0,
            aborted: false,
            finished: false,
        }
    }

    /// The 320x240 view on video memory.
    pub fn framebuffer() -> Framebuffer {
        Framebuffer::with_surface(Size::MODE_240, SURFACE).expect("the surface holds the view")
    }

    pub fn palette(&self) -> Palette {
        self.dac.palette()
    }

    /// One retrace, with its input; `ticked` is whether the 60 Hz clock moved. Returns
    /// whether the intro is over (the mode can go back to 13h).
    pub fn retrace<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a InputEvent>,
        ticked: bool,
        fb: &mut Framebuffer,
    ) -> bool {
        if self.finished {
            return true;
        }
        // The interrupt (`gfx_on_retrace`): count, origin, palette, then the tick.
        self.counter = self.counter.wrapping_add(1);
        self.retraces += 1;
        if let Some((x, y)) = self.pending.take()
            && let Err(e) = fb.set_origin(x, y)
        {
            log::warn!("intro: origin ({x}, {y}): {e}");
        }
        self.dac.retrace();
        self.ticks += u64::from(ticked);
        for event in events {
            if let InputEvent::KeyDown { code, .. } = *event {
                self.keys.push_back(code);
            }
        }
        while !self.finished && self.exec(fb) {}
        self.finished
    }

    /// The input hook: `None` while paused, else whether to abort. P pauses until the
    /// next key, which is used up; any other key aborts.
    fn poll(&mut self) -> Option<bool> {
        loop {
            let key = self.keys.pop_front();
            if self.paused {
                key?;
                self.paused = false;
                log::debug!("intro: resumed at counter {}", self.counter);
                return Some(false);
            }
            match key {
                None => return Some(false),
                Some(PAUSE) => {
                    self.paused = true;
                    log::debug!("intro: paused at counter {}", self.counter);
                }
                Some(_) => return Some(true),
            }
        }
    }

    /// Runs the op at `pc`; returns whether to go on to the next.
    fn exec(&mut self, fb: &mut Framebuffer) -> bool {
        match self.ops[self.pc] {
            Op::Begin(i) => {
                self.entry = i;
                let e = &self.entries[i];
                log::debug!(
                    "intro: entry {i} ({}) begins at counter {}, {:?} {} from ({}, {}), \
                     fade in {}, held until {}, fade out {} + {}",
                    self.pictures[i].name,
                    self.counter,
                    e.effect(),
                    e.length,
                    e.x,
                    e.y,
                    e.fade_in,
                    e.end_time,
                    e.fade_out,
                    e.fade_out / 2
                );
            }
            Op::Blank => self.dac.blank(),
            Op::WaitUpload => return self.wait(!self.dac.dirty()),
            Op::Poll(on_key) => match self.poll() {
                None => return false,
                Some(true) if on_key != OnKey::Ignore => {
                    if on_key == OnKey::RestoreAndAbort {
                        self.dac.restore();
                    }
                    return self.abort();
                }
                Some(_) => {}
            },
            Op::WaitTick => {
                let start = *self.since.get_or_insert(self.ticks);
                return self.wait(self.ticks != start);
            }
            Op::WaitRetraces(n) => {
                let start = *self.since.get_or_insert(self.retraces);
                return self.wait(self.retraces - start >= u64::from(n));
            }
            Op::Load(i) => {
                let pixels = &self.pictures[i].pixels;
                fb.pixels_mut()[..pixels.len()].copy_from_slice(pixels);
            }
            Op::Origin(x, y) => return self.set_origin(x, y),
            Op::Step(dx, dy) => {
                let (x, y) = self.origin;
                let (x, y) = (
                    x.saturating_add_signed(dx.into()),
                    y.saturating_add_signed(dy.into()),
                );
                return self.set_origin(x, y);
            }
            Op::PrepareFadeIn(i, length) => {
                let dac = self.pictures[i].dac;
                self.prepared = Some(self.dac.prepare_fade_in(&dac, 0, 255, length));
            }
            Op::StartFade => {
                if let Some(fade) = self.prepared.take() {
                    self.dac.start(fade);
                }
            }
            Op::HoldUntil(end) => match self.poll() {
                None => return false,
                Some(true) => return self.abort(),
                Some(false) if self.counter < end => return false,
                Some(false) => log::debug!(
                    "intro: entry {} held until counter {} (end time {end})",
                    self.entry,
                    self.counter
                ),
            },
            Op::Stop => self.dac.stop(),
            Op::FadeOut { last, length, max } => self.dac.fade_out(0, last, length, max),
            Op::WaitFade => return self.wait(!self.dac.fading()),
            Op::End => {
                self.finished = true;
                log::debug!(
                    "intro: {} at counter {} ({} retraces)",
                    if self.aborted { "aborted" } else { "ended" },
                    self.counter,
                    self.retraces
                );
                return false;
            }
        }
        self.pc += 1;
        true
    }

    /// Moves on if `done`, else waits for the next retrace.
    fn wait(&mut self, done: bool) -> bool {
        if done {
            self.since = None;
            self.pc += 1;
        }
        done
    }

    fn set_origin(&mut self, x: usize, y: usize) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.origin = (x, y);
        self.pending = Some((x, y));
        self.pc += 1;
        true
    }

    /// The player's abort path: everything fades out over 32 retraces, and the chain
    /// ends.
    fn abort(&mut self) -> bool {
        log::debug!(
            "intro: key at counter {}, entry {}: fading out",
            self.counter,
            self.entry
        );
        self.aborted = true;
        self.ops = vec![
            Op::Stop,
            Op::WaitRetraces(1),
            Op::FadeOut {
                last: 255,
                length: ABORT_FADE,
                max: u16::MAX,
            },
            Op::WaitFade,
            Op::End,
        ];
        self.pc = 0;
        self.since = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hold of one tick, fades of 4 retraces, held until counter 50.
    fn hold() -> Entry {
        Entry {
            file_id: 0x20,
            x: 0,
            y: 0,
            effect: 0,
            length: 1,
            caption: 0,
            caption_x: 0,
            caption_y: 0,
            end_time: 50,
            fade_in: 4,
            fade_out: 4,
            caption_speed: 2,
            pan_speed: 0,
        }
    }

    fn picture(rows: usize) -> Picture {
        Picture {
            name: "test.vcl",
            dac: [63; DAC_BYTES],
            pixels: vec![7; 640 * rows],
        }
    }

    fn down(code: u16) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat: false,
        }
    }

    /// Plays to the end (at most 10,000 retraces) with `keys` at their retraces; returns
    /// each retrace's DAC colour 0 and view origin.
    fn play(entries: &[Entry], keys: &[(u64, u16)]) -> Vec<(u8, (usize, usize))> {
        let pictures = entries.iter().map(|_| picture(400)).collect();
        let mut intro = Intro::new(entries, pictures);
        let mut fb = Intro::framebuffer();
        let mut seen = Vec::new();
        for r in 1..10_000 {
            let events: Vec<_> = keys
                .iter()
                .filter(|(at, _)| *at == r)
                .map(|&(_, k)| down(k))
                .collect();
            let done = intro.retrace(&events, true, &mut fb);
            seen.push((intro.dac.hardware()[0], fb.origin()));
            if done {
                return seen;
            }
        }
        panic!("the intro never ends");
    }

    fn levels(seen: &[(u8, (usize, usize))], from: usize, to: usize) -> Vec<u8> {
        seen[from - 1..to].iter().map(|s| s.0).collect()
    }

    #[test]
    fn a_hold_fades_in_waits_for_its_end_time_and_fades_out() {
        let seen = play(&[hold()], &[]);
        // Black for the 30-tick lead-in; the entry blanks at 32 and prepares its fade
        // at 33; black is uploaded at 34 and the steps follow, each shown a retrace
        // later, ending one level short.
        assert_eq!(levels(&seen, 1, 35), [0; 35]);
        assert_eq!(levels(&seen, 36, 39), [15, 31, 47, 62]);
        // Held until counter 50; one retrace later the fade out of 0..=251 starts
        // from the DAC. Its last (black) step is dropped by the next fade's start, which
        // takes two retraces from 15, and the chain ends before its black is shown.
        assert_eq!(levels(&seen, 50, 58), [62, 62, 62, 46, 31, 15, 15, 15, 7]);
        assert_eq!(seen.len(), 58);
    }

    #[test]
    fn keys_abort_except_in_the_lead_in() {
        // A key in the lead-in is lost; one during the hold fades out over 32
        // retraces and skips the rest of the chain.
        assert_eq!(play(&[hold(), hold()], &[(10, 0x1c), (40, 0x1c)]).len(), 73);
        assert_eq!(play(&[hold()], &[(10, 0x1c)]).len(), 58);
    }

    #[test]
    fn p_pauses_until_the_next_key_while_the_counter_runs() {
        let seen = play(&[hold()], &[(40, 0x19), (100, 0x1e)]);
        // The resuming key is used up; the counter passed 50 meanwhile, so the fade out
        // starts at once.
        assert_eq!(seen.len(), 108);
        assert_eq!(levels(&seen, 100, 103), [62, 62, 62, 46]);
        // P and another key in one retrace: paused and resumed straight away.
        assert_eq!(play(&[hold()], &[(40, 0x19), (40, 0x1e)]).len(), 58);
    }

    #[test]
    fn pans_move_one_pixel_a_retrace_or_at_their_speed() {
        let pan = |speed| Entry {
            effect: 8,
            length: 3,
            pan_speed: speed,
            y: 10,
            ..hold()
        };
        let origins = |seen: &[(u8, (usize, usize))]| -> Vec<(usize, usize)> {
            seen.iter().map(|s| s.1).skip(32).take(7).collect()
        };
        // Set to (0, 10) at 33 and shown at 34, then a step shown each retrace; with a
        // speed, two waits come before the second step and one before each other.
        let fast = play(&[pan(0)], &[]);
        assert_eq!(
            origins(&fast),
            [(0, 0), (0, 10), (0, 11), (0, 12), (0, 13), (0, 13), (0, 13)]
        );
        let slow = play(&[pan(2)], &[]);
        assert_eq!(
            origins(&slow),
            [(0, 0), (0, 10), (0, 11), (0, 11), (0, 11), (0, 11), (0, 12)]
        );
        // A key during a pan puts the fade's target in the shadow, but the abort's fade
        // drops that upload and starts from what the DAC shows.
        let aborted = play(
            &[Entry {
                length: 20,
                ..pan(0)
            }],
            &[(36, 0x1c)],
        );
        assert_eq!(levels(&aborted, 36, 39), [15, 15, 15, 14]);
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_intro_loads_and_plays_to_its_end() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let mut intro = load(&files).unwrap();
        assert_eq!(intro.pictures.len(), 10);
        let mut fb = Intro::framebuffer();
        while !intro.retrace([], true, &mut fb) {
            assert!(intro.retraces < 20_000, "the intro never ends");
        }
        // Past the last end time by the last picture's two fades and a few waits.
        let last = intro.entries.last().unwrap();
        let tail = u64::from(intro.counter - last.end_time);
        assert!(!intro.aborted);
        assert!(tail > u64::from(last.fade_out) && tail < 2 * u64::from(last.fade_out) + 8);
    }
}
