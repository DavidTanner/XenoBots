//! The cutscenes: the start-up intro (T-036), the ten-entry chain of `xeno.exe`,
//! `EALOGO` to `PRESENTS`, and the cutscenes after a mission (T-045): `WIN`, a lose
//! picture and `TRIUMPH`. Each is played in the 320x240 mode as the original's sequence
//! player (`1fe6:8fc4`) and `mgavideo.dvr`'s palette driver do it, with its captions
//! typed in font 6 (`docs/formats/vcl.md`, `palette.md`, `font.md`). The entries and
//! captions are read from the user's own `xeno.exe`, the font from `mgavideo.dvr`.
//!
//! The original is busy-waiting code run between retrace interrupts. Here each entry
//! becomes a list of [`Op`]s, and every retrace first does what the interrupt does
//! (count, apply the pending origin, upload the palette and step a fade, count a
//! 60 Hz tick) and then runs ops until one has to wait. So changes show at the same
//! retrace as in the original, if the code between retraces took no time (loading a
//! picture from disk did take time, which is not modelled).
//!
//! The music (T-062): a song's start resets the retrace counter `DS:0x370` that the end
//! times count from. `MAINTHEM` starts with the intro (the app starts it on the same
//! retrace, so the counter starts with both), `WIN1` and `WIN2` just before their
//! cutscene, and `BIGWIN3` inside `TRIUMPH`'s routine; each cutscene after a mission
//! stops its song before its last wait. The cutscene hands these over as [`Cue`]s.

use std::collections::VecDeque;

use xeno_formats::ending::{self, Numbers};
use xeno_formats::font::Font;
use xeno_formats::sequence::{self, Effect, Entry};
use xeno_formats::{lzexe, mgavideo, vcl};
use xeno_render::{DAC_BYTES, Dac, Fade, Framebuffer, Palette, Size, TextCursor, TextMode};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, ScanCode};
use crate::music::{self, Cue};

/// P pauses until the next key (the abort hook `1000:fccf`).
const PAUSE: ScanCode = ScanCode(0x19);
/// Video memory in the 320x240 mode: 640 wide, 400 rows (`video-modes.md`).
pub const SURFACE: Size = Size::new(640, 400);
/// Ticks the chain waits before its first entry (`1000:fd0a`).
const LEAD_IN: u32 = 30;
/// Retraces of the fade after an abort.
const ABORT_FADE: u16 = 32;
/// Captions: font 6 (the fourth), colour 252 (`1fe6:9032`).
const CAPTION_FONT: usize = 3;
const CAPTION_COLOUR: u8 = 252;
/// The player's own code: the next byte moves the cursor that many pixels right.
const MOVE_RIGHT: u8 = 9;
/// Ticks `TRIUMPH`'s routine waits before and after its entry, and every routine after
/// a mission before leaving (`ui_play_triumph_sequence`, `ui_play_win_cutscene`).
const TRIUMPH_BEFORE: u32 = 120;
const TRIUMPH_AFTER: u32 = 90;
const ENDING_TAIL: u32 = 15;

/// Which routine plays the entries (`vcl.md`, "Sequences").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `ui_play_intro_cutscene`: a 30-tick lead-in, then the chain; a key ends it.
    Intro,
    /// The win or a lose routine: one entry, then 15 ticks.
    Ending,
    /// `ui_play_triumph_sequence`: 120 ticks from the song's start, the entry, 105 more.
    Triumph,
}

/// A loaded picture, its palette in DAC levels and its entry's caption.
#[derive(Debug, Clone)]
pub struct Picture {
    pub name: &'static str,
    pub dac: [u8; DAC_BYTES],
    /// `640 * rows` pixels.
    pub pixels: Vec<u8>,
    /// The codes to type, without the NUL.
    pub caption: Vec<u8>,
}

/// The unpacked `xeno.exe` and the caption font.
fn read_exe_and_font(files: &GameFiles) -> Result<(lzexe::Unpacked, Font), String> {
    let exe = files
        .read("xeno.exe")
        .map_err(|e| format!("cannot read xeno.exe: {e}"))?;
    let exe = lzexe::unpack(&exe).map_err(|e| format!("xeno.exe: cannot unpack it: {e}"))?;
    let driver = files
        .read("mgavideo.dvr")
        .map_err(|e| format!("cannot read mgavideo.dvr: {e}"))?;
    let font = mgavideo::parse(&driver)
        .map_err(|e| format!("mgavideo.dvr: no fonts this port can read: {e}"))?
        .fonts
        .get(CAPTION_FONT)
        .cloned()
        .ok_or("mgavideo.dvr: no caption font (font 6)")?;
    Ok((exe, font))
}

/// Loads `entry`'s picture, with `caption`.
fn picture(files: &GameFiles, entry: &Entry, caption: Vec<u8>) -> Result<Picture, String> {
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
        caption,
    })
}

/// Reads the intro chain and its captions from `xeno.exe` and loads its pictures.
pub fn load(files: &GameFiles) -> Result<Cutscene, String> {
    let (exe, font) = read_exe_and_font(files)?;
    let chain = sequence::intro_chain(&exe)
        .map_err(|e| format!("xeno.exe: cannot find the intro sequence: {e}"))?;
    let captions = sequence::captions(&exe, &chain)
        .map_err(|e| format!("xeno.exe: cannot read the intro's captions: {e}"))?;
    let pictures = chain
        .iter()
        .zip(captions)
        .map(|(entry, caption)| picture(files, entry, caption))
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
    Ok(Cutscene::new(Kind::Intro, &chain, pictures, Some(font)))
}

/// What the cutscenes after a mission need: the entries and captions, the pictures
/// (win, the three lose, triumph) and the font.
#[derive(Debug, Clone)]
pub struct Endings {
    pub table: ending::Endings,
    pictures: Vec<Picture>,
    font: Option<Font>,
}

/// Reads the cutscenes after a mission from `xeno.exe` and loads their pictures.
pub fn load_endings(files: &GameFiles) -> Result<Endings, String> {
    let (exe, font) = read_exe_and_font(files)?;
    let table = ending::read(&exe)
        .map_err(|e| format!("xeno.exe: cannot find the cutscenes after a mission: {e}"))?;
    let entries = [
        &table.win,
        &table.lose[0],
        &table.lose[1],
        &table.lose[2],
        &table.triumph,
    ];
    let pictures = entries
        .into_iter()
        .map(|entry| picture(files, entry, Vec::new()))
        .collect::<Result<Vec<_>, String>>()?;
    log::info!(
        "endings: {} from xeno.exe",
        pictures
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(Endings::new(table, pictures, Some(font)))
}

impl Endings {
    /// `pictures`: win, the three lose by index, triumph.
    pub fn new(table: ending::Endings, pictures: Vec<Picture>, font: Option<Font>) -> Endings {
        assert_eq!(pictures.len(), 5, "a picture for each entry");
        Endings {
            table,
            pictures,
            font,
        }
    }

    fn play(&self, kind: Kind, index: usize, entry: Entry, caption: Vec<u8>) -> Cutscene {
        let picture = Picture {
            caption,
            ..self.pictures[index].clone()
        };
        Cutscene::new(kind, &[entry], vec![picture], self.font.clone())
    }

    /// `WIN`, its caption showing `numbers`.
    pub fn won(&self, numbers: Numbers) -> Cutscene {
        let caption = self.table.win_caption(numbers);
        self.play(Kind::Ending, 0, self.table.win, caption)
            .with_song(music::WIN1)
    }

    /// The lose entry for the selector word / 2 (the robot console opened last).
    pub fn lost(&self, index: usize) -> Cutscene {
        let index = index.min(ending::LOSE_ENTRIES - 1);
        let caption = self.table.lose_caption(index);
        self.play(Kind::Ending, 1 + index, self.table.lose[index], caption)
            .with_song(music::WIN2)
    }

    pub fn triumph(&self) -> Cutscene {
        let caption = self.table.triumph_caption();
        self.play(Kind::Triumph, 4, self.table.triumph, caption)
            .with_song(music::BIG_WIN3)
    }
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
    /// `sim_wait_ticks_polled`: this many ticks, polling before each; a key ends it.
    WaitTicks(u32),
    /// `snd_start_song`: the cutscene's song starts and the counter goes back to 0.
    StartSong,
    /// `snd_stop_song`.
    StopSong,
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
    /// Type entry `n`'s caption: a poll, then a code and `caption_speed` retraces.
    Caption(usize),
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

/// The ops for `kind` playing `entries`, picture `i` being entry `i`'s, and where each
/// entry's abort goes on: the end of the intro, or what follows the entry.
fn compile(kind: Kind, entries: &[Entry]) -> (Vec<Op>, Vec<usize>) {
    let mut ops = vec![Op::Blank, Op::WaitUpload];
    let mut resume = Vec::new();
    match kind {
        Kind::Intro => {
            for _ in 0..LEAD_IN {
                ops.extend([Op::Poll(OnKey::Ignore), Op::WaitTick]);
            }
        }
        Kind::Triumph => ops.extend([Op::StartSong, Op::WaitTicks(TRIUMPH_BEFORE)]),
        Kind::Ending => {}
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
        ops.extend([
            Op::Caption(i),
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
        resume.push(ops.len());
    }
    match kind {
        // An abort ends the whole chain.
        Kind::Intro => resume.fill(ops.len()),
        Kind::Triumph => ops.extend([
            Op::WaitTicks(TRIUMPH_AFTER),
            Op::StopSong,
            Op::WaitTicks(ENDING_TAIL),
        ]),
        // The 15 ticks after leaving the mode are waited here, in it, before the mode
        // goes back to 13h: the screen is black either way.
        Kind::Ending => ops.extend([Op::StopSong, Op::WaitTicks(ENDING_TAIL)]),
    }
    ops.push(Op::End);
    (ops, resume)
}

/// A caption being typed: the next code and the cursor.
#[derive(Debug, Clone, Copy)]
struct Typing {
    at: usize,
    cursor: TextCursor,
}

/// The player's state.
#[derive(Debug, Clone)]
pub struct Cutscene {
    kind: Kind,
    entries: Vec<Entry>,
    pictures: Vec<Picture>,
    font: Option<Font>,
    ops: Vec<Op>,
    resume: Vec<usize>,
    pc: usize,
    typing: Option<Typing>,
    /// Ticks left of a [`Op::WaitTicks`].
    ticks_left: Option<u32>,
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
    /// The song this cutscene starts and stops, and a start or stop not yet taken.
    song: Option<&'static str>,
    cue: Option<Cue>,
    aborted: bool,
    finished: bool,
}

impl Cutscene {
    pub fn new(
        kind: Kind,
        entries: &[Entry],
        pictures: Vec<Picture>,
        font: Option<Font>,
    ) -> Cutscene {
        assert_eq!(entries.len(), pictures.len(), "one picture an entry");
        let (ops, resume) = compile(kind, entries);
        Cutscene {
            kind,
            entries: entries.to_vec(),
            pictures,
            font,
            ops,
            resume,
            pc: 0,
            typing: None,
            ticks_left: None,
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
            song: None,
            cue: None,
            aborted: false,
            finished: false,
        }
    }

    /// Plays `song`. An ending's starts before its first retrace, a triumph's where
    /// its routine starts it.
    fn with_song(mut self, song: &'static str) -> Cutscene {
        self.song = Some(song);
        if self.kind == Kind::Ending {
            self.cue = Some(Cue::Start(song));
        }
        self
    }

    /// A music start or stop since the last call.
    pub fn take_cue(&mut self) -> Option<Cue> {
        self.cue.take()
    }

    /// The 320x240 view on video memory.
    pub fn framebuffer() -> Framebuffer {
        Framebuffer::with_surface(Size::MODE_240, SURFACE).expect("the surface holds the view")
    }

    pub fn palette(&self) -> Palette {
        self.dac.palette()
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Whether a key ended the chain (which skips the drop that follows).
    pub fn aborted(&self) -> bool {
        self.aborted
    }

    /// The retrace counter `DS:0x370`, which the drop goes on counting.
    pub fn counter(&self) -> u16 {
        self.counter
    }

    /// One retrace, with its input; `ticked` is whether the 60 Hz clock moved. Returns
    /// whether the cutscene is over (the mode can go back to 13h).
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
            log::warn!("cutscene: origin ({x}, {y}): {e}");
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
                log::debug!("cutscene: resumed at counter {}", self.counter);
                return Some(false);
            }
            match key {
                None => return Some(false),
                Some(PAUSE) => {
                    self.paused = true;
                    log::debug!("cutscene: paused at counter {}", self.counter);
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
                    "cutscene: entry {i} ({}) begins at counter {}, {:?} {} from ({}, {}), \
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
            Op::WaitTicks(n) => return self.wait_ticks(n),
            Op::StartSong => {
                self.counter = 0;
                self.cue = self.song.map(Cue::Start);
            }
            Op::StopSong => self.cue = self.song.map(|_| Cue::Stop),
            Op::Caption(i) => return self.type_caption(i, fb),
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
                    "cutscene: entry {} held until counter {} (end time {end})",
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
                    "cutscene: {:?} {} at counter {} ({} retraces)",
                    self.kind,
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

    /// `sim_wait_ticks_polled`: before each of `n` ticks, poll; a key ends the wait.
    fn wait_ticks(&mut self, n: u32) -> bool {
        let left = *self.ticks_left.get_or_insert(n);
        let Some(start) = self.since else {
            let key = if left == 0 { Some(true) } else { self.poll() };
            match key {
                None => return false,
                Some(true) => {
                    if left > 0 {
                        log::debug!("cutscene: a key ended a wait at counter {}", self.counter);
                    }
                    self.ticks_left = None;
                    self.pc += 1;
                }
                Some(false) => self.since = Some(self.ticks),
            }
            return true;
        };
        if self.ticks == start {
            return false;
        }
        self.since = None;
        self.ticks_left = Some(left - 1);
        true
    }

    /// Types entry `i`'s caption (`1fe6:9032` on): each code after a poll; 9 and the
    /// next byte move the cursor right; any other code is drawn, or run as a control
    /// code, then waited on for the entry's caption speed. The end of the text is the
    /// NUL.
    fn type_caption(&mut self, i: usize, fb: &mut Framebuffer) -> bool {
        let e = self.entries[i];
        if let Some(start) = self.since {
            if self.retraces - start < u64::from(e.caption_speed) {
                return false;
            }
            self.since = None;
        }
        let mut typing = self.typing.take().unwrap_or_else(|| Typing {
            at: 0,
            cursor: TextCursor::at(e.caption_x.into(), e.caption_y.into(), CAPTION_COLOUR),
        });
        loop {
            match self.poll() {
                None => {
                    self.typing = Some(typing);
                    return false;
                }
                Some(true) => return self.abort(),
                Some(false) => {}
            }
            let text = &self.pictures[i].caption;
            let code = text.get(typing.at).copied().unwrap_or(0);
            typing.at += 1;
            match code {
                0 => {
                    log::debug!(
                        "cutscene: entry {i}'s caption ({} codes) typed by counter {}",
                        text.len(),
                        self.counter
                    );
                    self.pc += 1;
                    return true;
                }
                MOVE_RIGHT => {
                    let n = text.get(typing.at).copied().unwrap_or(0);
                    typing.at += 1;
                    typing.cursor.x += i32::from(n);
                }
                code => {
                    if let Some(font) = &self.font {
                        fb.draw_char(font, &mut typing.cursor, code, TextMode::Transparent);
                    }
                    if e.caption_speed > 0 {
                        self.since = Some(self.retraces);
                        self.typing = Some(typing);
                        return false;
                    }
                }
            }
        }
    }

    /// The player's abort path: everything fades out over 32 retraces; the intro's
    /// chain ends, a routine after a mission goes on with its waits.
    fn abort(&mut self) -> bool {
        log::debug!(
            "cutscene: key at counter {}, entry {}: fading out",
            self.counter,
            self.entry
        );
        self.aborted = true;
        let rest = self.ops[self.resume[self.entry]..].to_vec();
        self.ops = [
            Op::Stop,
            Op::WaitRetraces(1),
            Op::FadeOut {
                last: 255,
                length: ABORT_FADE,
                max: u16::MAX,
            },
            Op::WaitFade,
        ]
        .into_iter()
        .chain(rest)
        .collect();
        self.pc = 0;
        self.since = None;
        self.typing = None;
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
            caption: Vec::new(),
        }
    }

    fn down(code: u16) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat: false,
        }
    }

    /// A retrace's DAC colour 0 and view origin.
    type Seen = (u8, (usize, usize));

    /// Plays the intro chain of `entries` to the end with `keys`; returns each
    /// retrace's DAC colour 0 and view origin.
    fn play(entries: &[Entry], keys: &[(u64, u16)]) -> Vec<Seen> {
        let pictures = entries.iter().map(|_| picture(400)).collect();
        run(Cutscene::new(Kind::Intro, entries, pictures, None), keys).0
    }

    /// Plays `intro` to the end (at most 10,000 retraces) with `keys` at their
    /// retraces; returns each retrace's DAC colour 0 and view origin, the player and
    /// the framebuffer.
    fn run(mut intro: Cutscene, keys: &[(u64, u16)]) -> (Vec<Seen>, Cutscene, Framebuffer) {
        let mut fb = Cutscene::framebuffer();
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
                return (seen, intro, fb);
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

    /// The synthetic font 6 with code 0x0c a control code: `B` is two pixels wide,
    /// colours 252 and 254 over 253 and nothing; other glyphs are one blank pixel.
    fn font() -> Font {
        let block = xeno_formats::fixtures::synthetic_font_block();
        let mut font = xeno_formats::font::parse(&block)
            .unwrap()
            .swap_remove(CAPTION_FONT);
        font.glyphs[0x0c] = None;
        font
    }

    #[test]
    fn captions_type_a_code_every_caption_speed_retraces() {
        let entry = Entry {
            caption_x: 10,
            caption_y: 20,
            ..hold()
        };
        let mut shown = picture(240);
        shown.caption = b"B\x09\x05B\x0cB".to_vec();
        let mut intro = Cutscene::new(Kind::Intro, &[entry], vec![shown], Some(font()));
        let mut fb = Cutscene::framebuffer();
        let mut first = [None; 3];
        for r in 1..100u64 {
            intro.retrace([], true, &mut fb);
            for (seen, (x, y)) in first.iter_mut().zip([(10, 20), (18, 20), (10, 23)]) {
                if seen.is_none() && fb.get(x, y) == Some(252) {
                    *seen = Some(r);
                }
            }
        }
        // The fade-in starts at 34 and the one-tick hold ends at 35, so the caption
        // starts there. 9 and its byte move 5 pixels right after the glyph's 3 with no
        // wait; the new line costs a wait as a glyph does, and goes back to x = 10 one
        // line (3 rows) down.
        assert_eq!(first, [Some(35), Some(37), Some(41)]);
        assert_eq!(
            (fb.get(11, 20), fb.get(10, 21), fb.get(11, 21)),
            (Some(254), Some(253), Some(7))
        );
        // With a speed of 0 the whole caption is typed at once.
        let mut shown = picture(240);
        shown.caption = b"B\x0cB".to_vec();
        let fast = Entry {
            caption_speed: 0,
            caption_x: 10,
            caption_y: 20,
            ..hold()
        };
        let (_, _, fb) = run(
            Cutscene::new(Kind::Intro, &[fast], vec![shown], Some(font())),
            &[],
        );
        assert_eq!((fb.get(10, 20), fb.get(10, 23)), (Some(252), Some(252)));
    }

    #[test]
    fn a_key_while_typing_aborts_and_endings_wait_after_their_entry() {
        let mut shown = picture(240);
        shown.caption = b"BBBB".to_vec();
        let typed = || Cutscene::new(Kind::Ending, &[hold()], vec![shown.clone()], Some(font()));
        // Alone: the entry as in the intro (no lead-in, so 30 ticks sooner, and black
        // until the fade in), then 15 ticks.
        let (seen, ending, _) = run(typed(), &[]);
        assert_eq!(levels(&seen, 6, 9), [15, 31, 47, 62]);
        assert!(!ending.aborted());
        assert_eq!(seen.len(), 58 + 15);
        // A key while typing fades out over 32 retraces, then the 15 ticks follow; a
        // key in them ends them at once.
        let (seen, ending, fb) = run(typed(), &[(8, 0x1c)]);
        assert!(ending.aborted());
        assert_eq!(fb.get(3, 0), Some(252));
        assert_eq!(fb.get(6, 0), Some(7), "no more codes typed after the key");
        assert_eq!(seen.len(), 8 + 34 + 15);
        assert_eq!(run(typed(), &[(8, 0x1c), (45, 0x1c)]).0.len(), 45);
    }

    #[test]
    fn triumph_waits_around_its_entry_from_where_its_song_starts() {
        let (seen, triumph, _) = run(
            Cutscene::new(
                Kind::Triumph,
                &[Entry {
                    end_time: 200,
                    ..hold()
                }],
                vec![picture(240)],
                None,
            ),
            &[],
        );
        // Blank (the counter restarts at retrace 2, where the song would start), 120
        // ticks of black, the entry (its fade in shown from 126, held until counter
        // 200, then its two fades out), then 90 and 15 ticks.
        assert_eq!(levels(&seen, 1, 125), [0; 125]);
        assert_eq!(levels(&seen, 126, 129), [15, 31, 47, 62]);
        assert_eq!(triumph.kind(), Kind::Triumph);
        assert_eq!(seen.len(), 2 + 200 + 8 + 90 + 15);
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
        assert!(intro.font.is_some());
        let mut fb = Cutscene::framebuffer();
        while !intro.retrace([], true, &mut fb) {
            assert!(intro.retraces < 20_000, "the intro never ends");
        }
        // Past the last end time by the last picture's two fades and a few waits.
        let last = intro.entries.last().unwrap();
        let tail = u64::from(intro.counter - last.end_time);
        assert!(!intro.aborted);
        assert!(tail > u64::from(last.fade_out) && tail < 2 * u64::from(last.fade_out) + 8);
    }

    /// Plays the won (with numbers), every lost and the triumph cutscene from the
    /// user's files to their end, captions and all.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_endings_play_to_their_end() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let endings = load_endings(&files).unwrap();
        let numbers = Numbers {
            kills: 7,
            losses: 3,
            points: endings.table.points(7, 3),
        };
        let cutscenes = [
            endings.won(numbers),
            endings.lost(0),
            endings.lost(1),
            endings.lost(2),
            endings.triumph(),
        ];
        for mut cutscene in cutscenes {
            assert!(!cutscene.pictures[0].caption.is_empty());
            let mut fb = Cutscene::framebuffer();
            while !cutscene.retrace([], true, &mut fb) {
                assert!(
                    cutscene.retraces < 20_000,
                    "{} never ends",
                    cutscene.pictures[0].name
                );
            }
            assert!(!cutscene.aborted());
            // The caption was typed: some of its colours are on the surface.
            assert!(fb.pixels().iter().any(|&p| (252..=254).contains(&p)));
            let entry = cutscene.entries[0];
            assert!(u64::from(cutscene.counter) > u64::from(entry.end_time));
        }
    }
}
