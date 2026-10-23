//! The intro drop (T-038): `vgaintro.dvr`'s scroll over `xenodrop.pcx`, then
//! `cavedrop.pcx`, with the `.fgd` silhouettes in front (`docs/formats/fgd.md`, "The
//! drop sequence"). Its numbers, colour records and credits script are read from the
//! user's own `vgaintro.dvr` by `xeno_formats::vgaintro`.
//!
//! As in `intro.rs`, the driver's busy-waiting code becomes a list of [`Op`]s, and
//! every retrace first does what the interrupt does (count, apply the pending origin,
//! upload the palette and run the colour records) and then runs ops until one waits.
//! The drawing works on a copy of video memory ([`SURFACE`]) the way the driver does:
//! the backdrop with its clean copy 128 rows lower, and only the silhouettes' changes
//! each frame, so its quirks (an edge row restored after a retreat, the clean copy's
//! first rows overwritten) show as in the original.
//!
//! Not yet: the credits text (it needs the game's font, T-041; the colour record that
//! fades it runs), and the music, whose clock the drop's waits and its end count.

use std::collections::VecDeque;

use xeno_formats::fgd::Layer;
use xeno_formats::pcx;
use xeno_formats::vgaintro::{self, CreditOp, IntroDrop, Numbers, PALETTE_BUFFER};
use xeno_render::{ColourAnims, DAC_BYTES, Dac, Framebuffer, Palette, Size};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, ScanCode};

/// P pauses until the next key (the abort hook `1000:fccf`).
const PAUSE: ScanCode = ScanCode(0x19);
/// Video memory as `vgaintro.dvr` uses it: 768-pixel rows, as many as fit a 64 KB
/// plane (rows past that are not drawn).
pub const SURFACE: Size = Size::new(768, 341);
/// The backdrop's first row (the driver draws from segment `0xa360`).
const BACKDROP_ROW: i32 = 72;
/// Rows from the backdrop to its clean copy (`0x6000` bytes).
const CLEAN_COPY: i32 = 128;
/// Rows of the credits area above the backdrop (`0x3600` bytes).
const CREDITS_ROWS: usize = 72;
/// Where the layer buffers A, B and C start, back to back (`0x0a8e`, `0x120e`, `0x198e`).
const BUFFERS: [usize; 3] = [0, Layer::LEN, 2 * Layer::LEN];

/// One part's files: its layers (A, B and, in the cave, C), its backdrop and the
/// backdrop's palette in DAC levels.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: &'static str,
    pub layers: Vec<Layer>,
    /// `768 * rows` pixels.
    pub backdrop: Vec<u8>,
    pub palette: [u8; DAC_BYTES],
}

/// Everything the drop reads from the game files.
#[derive(Debug, Clone)]
pub struct Assets {
    pub drop: IntroDrop,
    pub parts: [Part; 2],
}

/// Reads `vgaintro.dvr` and the two parts' layers and backdrops.
pub fn load(files: &GameFiles) -> Result<Assets, String> {
    let read = |name: &str| {
        files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))
    };
    let drop = vgaintro::parse(&read("vgaintro.dvr")?)
        .map_err(|e| format!("vgaintro.dvr: cannot read the drop sequence: {e}"))?;
    let part = |name: &'static str, layers: &[&str]| -> Result<Part, String> {
        let bytes = read(name)?;
        let picture = pcx::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
        if picture.width() != SURFACE.width {
            return Err(format!("{name}: {} pixels wide, not 768", picture.width()));
        }
        let mut palette = [0; DAC_BYTES];
        for (to, from) in palette.iter_mut().zip(&bytes[bytes.len() - DAC_BYTES..]) {
            *to = from >> 2;
        }
        let layers = layers
            .iter()
            .map(|&layer| {
                xeno_formats::fgd::parse(&read(layer)?).map_err(|e| format!("{layer}: {e}"))
            })
            .collect::<Result<_, String>>()?;
        Ok(Part {
            name,
            layers,
            backdrop: picture.pixels,
            palette,
        })
    };
    let parts = [
        part("xenodrop.pcx", &["layer3.fgd", "layer4.fgd"])?,
        part("cavedrop.pcx", &["layer1.fgd", "layer2.fgd", "layer5.fgd"])?,
    ];
    let n = &drop.numbers;
    log::info!(
        "drop: from counter {}, frames from {}, first part to frame {}, cave part until counter {}",
        n.start,
        n.frames,
        n.first_part_end,
        n.cave_end
    );
    Ok(Assets { drop, parts })
}

/// An immediate byte as the CPU uses it in a word instruction: sign-extended.
fn wide(b: u8) -> u16 {
    b as i8 as u16
}

/// The frame script's state (`gfx_step_drop_frame` and the state handlers): the
/// speeds of layers A, B, C and the view, their fractions, the layers' offsets and the
/// view's x, the frame count and the state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub speeds: [u16; 4],
    fractions: [u8; 4],
    pub offsets: [u16; 3],
    pub view_x: u16,
    pub frame: u16,
    pub state: u8,
    /// The end flag (`0x047b`).
    pub ended: bool,
}

/// What a frame of the script did besides moving things.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameEvents {
    /// The layers and view moved (and the floor must be redrawn).
    pub stepped: bool,
    /// The light changes (`pal_switch_to_cave_light`).
    pub light: bool,
}

impl Script {
    /// Before the first part's frames: everything still, offsets and view as given.
    pub fn new(n: &Numbers) -> Script {
        Script {
            speeds: [0; 4],
            fractions: [0; 4],
            offsets: [n.layer_start; 3],
            view_x: n.view_x,
            frame: 0,
            state: 0,
            ended: false,
        }
    }

    /// The cave part's start (`gfx_reset_layers_and_floor`): offsets and view back,
    /// the fractions, speeds, frame count and state kept.
    pub fn restart(&mut self, n: &Numbers) {
        self.offsets = [n.layer_start; 3];
        self.view_x = n.cave_view_x;
        self.ended = false;
    }

    /// `gfx_scroll_layers_and_view`: each speed into its fraction, the carry off the
    /// offset (the view's off x).
    fn step(&mut self) {
        for i in 0..4 {
            let sum = u16::from(self.fractions[i]).wrapping_add(self.speeds[i]);
            self.fractions[i] = sum as u8;
            let carry = sum >> 8;
            match self.offsets.get_mut(i) {
                Some(offset) => *offset = offset.wrapping_sub(carry),
                None => self.view_x = self.view_x.wrapping_sub(carry),
            }
        }
    }

    fn add(&mut self, steps: [u8; 4], sign: i8) {
        for (speed, step) in self.speeds.iter_mut().zip(steps) {
            *speed = match sign {
                1 => speed.wrapping_add(wide(step)),
                _ => speed.wrapping_sub(wide(step)),
            };
        }
    }

    /// One frame: count it and run the current state (`0x3b91`..`0x3cb2`).
    pub fn frame(&mut self, n: &Numbers) -> FrameEvents {
        self.frame = self.frame.wrapping_add(1);
        let f = self.frame;
        let mut events = FrameEvents::default();
        if self.state == 0 {
            if f != wide(n.accelerate_until) {
                self.add(
                    [
                        n.accelerate_a,
                        n.accelerate_b,
                        n.accelerate_c,
                        n.accelerate_view,
                    ],
                    1,
                );
                self.step();
                events.stepped = true;
                return events;
            }
            self.speeds = [n.cruise_a, n.cruise_b, n.cruise_c, n.cruise_view];
            self.state = 1;
        }
        if self.state == 1 {
            if f != n.slow_from {
                if f == n.first_part_end {
                    self.ended = true;
                    return events;
                }
                if f == n.jump_at {
                    self.view_x = self.view_x.wrapping_sub(n.jump_by);
                    events.light = true;
                }
                if f == n.boost_at {
                    self.speeds[1] = n.boost_b;
                }
                self.step();
                events.stepped = true;
                return events;
            }
            self.state = 2;
        }
        let hold = |until| (f == until).then_some(());
        match self.state {
            2 if f == n.stop_at => {
                self.speeds = [0; 4];
                self.state = 3;
            }
            2 => {
                self.add([n.slow_a, n.slow_b, n.slow_c, n.slow_view], -1);
                self.step();
                events.stepped = true;
            }
            3 | 4 => {
                let until = if self.state == 3 {
                    n.hold_until
                } else {
                    n.hold_again_until
                };
                if hold(until).is_some() {
                    self.state += 1;
                } else {
                    self.step();
                    events.stepped = true;
                }
            }
            _ => self.ended = true,
        }
        events
    }
}

/// Video memory row `row` of the backdrop (from [`BACKDROP_ROW`]), if on the surface.
fn at(row: i32, column: usize) -> Option<usize> {
    let row = BACKDROP_ROW + row;
    (0..SURFACE.height as i32)
        .contains(&row)
        .then(|| row as usize * SURFACE.width + column)
}

fn put(pixels: &mut [u8], row: i32, column: usize, colour: u8) {
    if let Some(i) = at(row, column) {
        pixels[i] = colour;
    }
}

/// Copies backdrop row `row` back from the clean copy.
fn restore(pixels: &mut [u8], row: i32, column: usize) {
    let from = at(row + CLEAN_COPY, column).map_or(0, |i| pixels[i]);
    put(pixels, row, column, from);
}

/// `file_load_pcx_to_video`: each pixel to its row and to the clean copy, in order.
pub fn draw_backdrop(pixels: &mut [u8], backdrop: &[u8]) {
    for (i, &colour) in backdrop.iter().enumerate() {
        let (row, column) = ((i / SURFACE.width) as i32, i % SURFACE.width);
        put(pixels, row, column, colour);
        put(pixels, row + CLEAN_COPY, column, colour);
    }
}

/// `gfx_draw_floor_changes` for one column, from edge row `old` to `new`: colour 0 on
/// rows `new..=old` where the floor rose, rows `old..=new` (the new edge included)
/// copied back where it fell. The count is a signed byte, as in the driver.
pub fn floor_column(pixels: &mut [u8], column: usize, old: u8, new: u8) {
    let mut count = old.wrapping_sub(new) as i8;
    let mut row = i32::from(new);
    if count == 0 {
        return;
    }
    if count < 0 {
        while count <= 0 {
            restore(pixels, row, column);
            row -= 1;
            count = count.wrapping_add(1);
        }
    } else {
        while count >= 0 {
            put(pixels, row, column, 0);
            row += 1;
            count = count.wrapping_sub(1);
        }
    }
}

/// `gfx_draw_ceiling_changes` for one column: colour 0 on rows `old..=new` where the
/// ceiling came down, rows `new..=old` copied back where it went up.
pub fn ceiling_column(pixels: &mut [u8], column: usize, old: u8, new: u8) {
    let mut count = new.wrapping_sub(old) as i8;
    let mut row = i32::from(old);
    if count == 0 {
        return;
    }
    if count < 0 {
        while count <= 0 {
            restore(pixels, row, column);
            row -= 1;
            count = count.wrapping_add(1);
        }
    } else {
        while count >= 0 {
            put(pixels, row, column, 0);
            row += 1;
            count = count.wrapping_sub(1);
        }
    }
}

/// `gfx_draw_ceiling_full` for one column: colour 0 from the edge row up to row 0.
pub fn ceiling_full_column(pixels: &mut [u8], column: usize, edge: u8) {
    let mut count = edge as i8;
    let mut row = i32::from(edge);
    loop {
        put(pixels, row, column, 0);
        row -= 1;
        count = count.wrapping_sub(1);
        if count < 0 {
            break;
        }
    }
}

/// The three layer buffers, back to back as in the driver; a read past them gives 0.
#[derive(Debug, Clone)]
struct Buffers(Vec<u8>);

impl Buffers {
    fn new(layers: &[Layer]) -> Buffers {
        let mut bytes = vec![0; 3 * Layer::LEN];
        for (i, layer) in layers.iter().enumerate() {
            bytes[BUFFERS[i]..][..Layer::LEN].copy_from_slice(&layer.rows);
        }
        Buffers(bytes)
    }

    fn row(&self, buffer: usize, offset: u16, column: usize) -> u8 {
        let i = BUFFERS[buffer] + usize::from(offset) + column;
        self.0.get(i).copied().unwrap_or(0)
    }

    /// `gfx_combine_floor_layers`: per column the higher of A's and B's edges.
    fn floor(&self, offsets: [u16; 3]) -> Vec<u8> {
        (0..SURFACE.width)
            .map(|c| self.row(0, offsets[0], c).min(self.row(1, offsets[1], c)))
            .collect()
    }
}

/// What a poll of the input hook says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Poll {
    Go,
    Paused,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Slot `0x4b4` before the driver is entered: black, uploaded.
    Blank,
    WaitUpload,
    /// Set up part `n`: view, files, floor (and ceiling), its palette to the shadow,
    /// marked.
    Part(usize),
    /// `sim_wait_counter_reaches`.
    WaitCounter(u16),
    /// Slot `0x4a8` from the palette buffer.
    ToShadow,
    /// Slot `0x316`: start a record at a weight.
    StartAnim(u8, u8),
    /// `sim_wait_retrace`: until the counter moves.
    WaitRetrace,
    Poll,
    /// List bytes into the palette buffer: from, to, length.
    Copy(u16, u16, u16),
    /// A text list in the credits area (not drawn yet).
    Text(u16),
    /// The credits colour's weight set to this and its record started.
    Credits(u8),
    /// Before the first part's frames: speeds 0, frame count and states reset.
    StartFrames,
    /// Part `n`'s frames, until the script ends it (first part) or the counter
    /// reaches the end (cave part).
    Frames(usize),
    End,
}

/// The ops of the run routine (`gfx_run_intro_drop`).
fn compile(n: &Numbers) -> Vec<Op> {
    let blend = |ops: &mut Vec<Op>, record: u8, steps: u16| {
        let mut weight = 0u8;
        for _ in 0..u32::from(steps) {
            ops.extend([Op::StartAnim(record, weight), Op::WaitRetrace, Op::Poll]);
            weight = weight.wrapping_add(2);
        }
    };
    let mut ops = vec![
        Op::Blank,
        Op::WaitUpload,
        Op::Part(0),
        Op::WaitCounter(n.start),
    ];
    ops.push(Op::ToShadow);
    blend(&mut ops, n.blend_record, n.blend_steps);
    ops.extend([
        Op::Copy(n.first_copy_from, n.first_copy_to, n.first_copy_len),
        Op::ToShadow,
        Op::Text(n.copyright),
        Op::Credits(n.credits_show_from.wrapping_add(n.credits_step)),
        Op::WaitCounter(n.second_blend),
    ]);
    blend(&mut ops, n.second_blend_record, n.second_blend_steps);
    ops.extend([
        Op::Copy(n.second_copy_from, n.second_copy_to, n.second_copy_len),
        Op::ToShadow,
        Op::WaitCounter(n.third_blend),
    ]);
    blend(&mut ops, n.third_blend_record, n.third_blend_steps);
    ops.extend([
        Op::Credits(n.credits_hide_from.wrapping_sub(n.credits_down)),
        Op::WaitCounter(n.frames),
        Op::StartFrames,
        Op::Frames(0),
        Op::Part(1),
        Op::Frames(1),
        Op::ToShadow,
    ]);
    blend(&mut ops, n.last_blend_record, n.last_blend_steps);
    ops.push(Op::End);
    ops
}

/// The drop's state while it plays.
#[derive(Debug, Clone)]
pub struct Player {
    assets: Assets,
    ops: Vec<Op>,
    pc: usize,
    dac: Dac,
    /// The driver's palette buffer (`0x16a`).
    buffer: [u8; DAC_BYTES],
    script: Script,
    layers: Buffers,
    /// The floor drawn last, and the ceiling's offset then.
    floor: Vec<u8>,
    ceiling_offset: u16,
    /// The credits script's next step and the credits colour's weight.
    credit: usize,
    weight: u8,
    /// The retrace counter `DS:0x370`.
    counter: u16,
    /// A frame's start, and the counter when a wait for it to move began.
    frame_start: Option<u16>,
    since: Option<u16>,
    pending: Option<(usize, usize)>,
    keys: VecDeque<ScanCode>,
    paused: bool,
    part: usize,
    aborted: bool,
    finished: bool,
}

impl Player {
    /// The drop, its counter starting at `counter` (where the intro left it).
    pub fn new(assets: Assets, counter: u16) -> Player {
        let ops = compile(&assets.drop.numbers);
        let script = Script::new(&assets.drop.numbers);
        let mut dac = Dac::default();
        dac.set_anims(ColourAnims::new(assets.drop.anims.clone()));
        log::debug!("drop: starts at counter {counter}");
        Player {
            layers: Buffers::new(&assets.parts[0].layers),
            assets,
            ops,
            pc: 0,
            dac,
            buffer: [0; DAC_BYTES],
            script,
            floor: vec![0; SURFACE.width],
            ceiling_offset: 0,
            credit: 0,
            weight: 0,
            counter,
            frame_start: None,
            since: None,
            pending: None,
            keys: VecDeque::new(),
            paused: false,
            part: 0,
            aborted: false,
            finished: false,
        }
    }

    /// The 320x200 view on the drop's video memory, split after the backdrop.
    pub fn framebuffer(&self) -> Framebuffer {
        let mut fb =
            Framebuffer::with_surface(Size::MODE_13H, SURFACE).expect("the surface holds the view");
        fb.set_split(Some(usize::from(self.assets.drop.numbers.split_row)));
        fb
    }

    pub fn palette(&self) -> Palette {
        self.dac.palette()
    }

    /// One retrace with its input. Returns whether the drop is over.
    pub fn retrace<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a InputEvent>,
        fb: &mut Framebuffer,
    ) -> bool {
        if self.finished {
            return true;
        }
        self.counter = self.counter.wrapping_add(1);
        if let Some((x, y)) = self.pending.take()
            && let Err(e) = fb.set_origin(x, y)
        {
            log::warn!("drop: origin ({x}, {y}): {e}");
        }
        self.dac.retrace();
        for event in events {
            if let InputEvent::KeyDown { code, .. } = *event {
                self.keys.push_back(code);
            }
        }
        while !self.finished && self.exec(fb) {}
        self.finished
    }

    /// The abort hook: P pauses until the next key, which is used up; any other key
    /// aborts.
    fn poll(&mut self) -> Poll {
        loop {
            let key = self.keys.pop_front();
            if self.paused {
                if key.is_none() {
                    return Poll::Paused;
                }
                self.paused = false;
                log::debug!("drop: resumed at counter {}", self.counter);
                return Poll::Go;
            }
            match key {
                None => return Poll::Go,
                Some(PAUSE) => {
                    self.paused = true;
                    log::debug!("drop: paused at counter {}", self.counter);
                }
                Some(_) => return Poll::Abort,
            }
        }
    }

    fn abort(&mut self) -> bool {
        log::debug!(
            "drop: key at counter {}, frame {}: ended at once",
            self.counter,
            self.script.frame
        );
        self.aborted = true;
        self.finished = true;
        false
    }

    /// Moves on if `done`, else waits for the next retrace.
    fn wait(&mut self, done: bool) -> bool {
        if done {
            self.since = None;
            self.pc += 1;
        }
        done
    }

    fn set_origin(&mut self) {
        let y = usize::from(self.assets.drop.numbers.view_y);
        self.pending = Some((usize::from(self.script.view_x), y));
    }

    fn start_anim(&mut self, record: u8, weight: u8) {
        if !self.dac.start_anim(record, weight) {
            log::warn!("drop: no colour record {record}");
        }
    }

    /// Runs the op at `pc`; returns whether to go on to the next.
    fn exec(&mut self, fb: &mut Framebuffer) -> bool {
        match self.ops[self.pc] {
            Op::Blank => self.dac.blank(),
            Op::WaitUpload => return self.wait(!self.dac.dirty()),
            Op::Part(p) => self.start_part(p, fb),
            Op::WaitCounter(n) => return self.wait(self.counter >= n),
            Op::ToShadow => self.dac.copy(&self.buffer),
            Op::StartAnim(record, weight) => self.start_anim(record, weight),
            Op::WaitRetrace => {
                let start = *self.since.get_or_insert(self.counter);
                return self.wait(self.counter != start);
            }
            Op::Poll => match self.poll() {
                Poll::Go => {}
                Poll::Paused => return false,
                Poll::Abort => return self.abort(),
            },
            Op::Copy(from, to, len) => self.copy(from, to, len),
            Op::Text(list) => self.text(list),
            Op::Credits(weight) => {
                self.weight = weight;
                self.start_anim(self.assets.drop.numbers.credits_record, weight);
            }
            Op::StartFrames => {
                let n = &self.assets.drop.numbers;
                self.script.speeds = [0; 4];
                self.script.frame = 0;
                self.script.state = 0;
                self.script.ended = false;
                self.credit = 0;
                log::debug!(
                    "drop: first part's frames from counter {}, view x {}",
                    self.counter,
                    n.view_x
                );
            }
            Op::Frames(p) => return self.frames(p, fb),
            Op::End => {
                self.finished = true;
                log::debug!(
                    "drop: ended at counter {}, frame {}, offsets {:?}, view x {}",
                    self.counter,
                    self.script.frame,
                    self.script.offsets,
                    self.script.view_x
                );
                return false;
            }
        }
        self.pc += 1;
        true
    }

    /// Op::Part: loads part `p` into video memory and draws its silhouettes.
    fn start_part(&mut self, p: usize, fb: &mut Framebuffer) {
        let n = self.assets.drop.numbers.clone();
        self.part = p;
        if p == 0 {
            // `gfx_enter_drop_mode`: the credits area cleared.
            fb.pixels_mut()[..CREDITS_ROWS * SURFACE.width].fill(0);
        } else {
            self.script.restart(&n);
        }
        self.set_origin();
        let part = &self.assets.parts[p];
        let name = part.name;
        self.layers = Buffers::new(&part.layers);
        draw_backdrop(fb.pixels_mut(), &part.backdrop);
        self.buffer = part.palette;
        // `gfx_reset_layers_and_floor`: the floor from an old edge of 127 everywhere.
        self.script.offsets = [n.layer_start; 3];
        self.ceiling_offset = n.layer_start;
        self.floor = vec![127; SURFACE.width];
        self.draw_floor(fb);
        if p == 1 {
            for c in 0..SURFACE.width {
                let edge = self.layers.row(2, self.script.offsets[2], c);
                ceiling_full_column(fb.pixels_mut(), c, edge);
            }
        }
        self.dac.copy(&self.buffer);
        self.dac.mark();
        log::debug!(
            "drop: part {p} ({name}) at counter {}, frame {}, view x {}",
            self.counter,
            self.script.frame,
            self.script.view_x
        );
    }

    fn draw_floor(&mut self, fb: &mut Framebuffer) {
        let new = self.layers.floor(self.script.offsets);
        for (c, (&old, &new)) in self.floor.iter().zip(&new).enumerate() {
            floor_column(fb.pixels_mut(), c, old, new);
        }
        self.floor = new;
    }

    fn draw_ceiling(&mut self, fb: &mut Framebuffer) {
        let (old, new) = (self.ceiling_offset, self.script.offsets[2]);
        for c in 0..SURFACE.width {
            let (a, b) = (self.layers.row(2, old, c), self.layers.row(2, new, c));
            ceiling_column(fb.pixels_mut(), c, a, b);
        }
        self.ceiling_offset = new;
    }

    fn copy(&mut self, from: u16, to: u16, len: u16) {
        let list = self.dac.anims().bytes();
        let (from, to, len) = (usize::from(from), usize::from(to), usize::from(len));
        match (
            list.get(from..from + len),
            self.buffer.get_mut(to..to + len),
        ) {
            (Some(from), Some(to)) => to.copy_from_slice(from),
            _ => log::warn!("drop: cannot copy {len} colour bytes from list offset {from}"),
        }
    }

    fn text(&self, list: u16) {
        match self.assets.drop.text_list(list) {
            Ok(items) => log::debug!(
                "drop: text list {list:#x} ({} strings) at counter {}, not drawn (needs the game's font)",
                items.len(),
                self.counter
            ),
            Err(e) => log::warn!("drop: text list {list:#x}: {e}"),
        }
    }

    /// One step of the credits script (`ui_step_credits_script`).
    fn credits(&mut self, fb: &mut Framebuffer) {
        let n = self.assets.drop.numbers.clone();
        let Some(&op) = self.assets.drop.credits.get(self.credit) else {
            return;
        };
        let next = match op {
            CreditOp::Clear => {
                fb.pixels_mut()[..CREDITS_ROWS * SURFACE.width].fill(0);
                true
            }
            CreditOp::WaitFrame(frame) => self.script.frame >= frame,
            CreditOp::Draw(list) => {
                self.text(list);
                true
            }
            CreditOp::FadeIn => {
                self.weight = self.weight.wrapping_add(n.credits_step);
                self.start_anim(n.credits_record, self.weight);
                self.weight >= n.credits_full
            }
            CreditOp::FadeOut => {
                self.weight = self.weight.wrapping_sub(n.credits_down);
                self.start_anim(n.credits_record, self.weight);
                self.weight == 0
            }
        };
        if next {
            self.credit += 1;
        }
    }

    /// Op::Frames: one frame each time its wait is over.
    fn frames(&mut self, p: usize, fb: &mut Framebuffer) -> bool {
        let n = self.assets.drop.numbers.clone();
        let retraces = if p == 0 {
            n.frame_retraces
        } else {
            n.cave_frame_retraces
        };
        if let (Some(start), Some(moved)) = (self.frame_start, self.since) {
            // Waiting for the counter to move and be `retraces` past the frame's start.
            if self.counter == moved || self.counter.wrapping_sub(start) < retraces {
                return false;
            }
            self.frame_start = None;
            self.since = None;
            let over = match p {
                0 => self.script.ended,
                _ => self.counter >= n.cave_end,
            };
            if over {
                log::debug!(
                    "drop: part {p} ends at counter {}, frame {}, offsets {:?}, view x {}",
                    self.counter,
                    self.script.frame,
                    self.script.offsets,
                    self.script.view_x
                );
                self.pc += 1;
                return true;
            }
        }
        self.frame_start.get_or_insert(self.counter);
        match self.poll() {
            Poll::Go => {}
            Poll::Paused => return false,
            Poll::Abort => return self.abort(),
        }
        let events = self.script.frame(&n);
        if events.light {
            self.start_anim(n.light_record, n.light_weight);
            let to = n.light_to.wrapping_sub(PALETTE_BUFFER);
            self.copy(n.light_from, to, n.light_len);
            self.dac.copy(&self.buffer);
            log::debug!(
                "drop: frame {}: the view jumps to x {}, the light changes",
                self.script.frame,
                self.script.view_x
            );
        }
        if events.stepped {
            self.draw_floor(fb);
        }
        self.set_origin();
        if p == 1 {
            self.draw_ceiling(fb);
        }
        self.credits(fb);
        self.since = Some(self.counter);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{synthetic_numbers, synthetic_vgaintro};

    fn numbers() -> Numbers {
        synthetic_numbers()
    }

    /// Synthetic files: flat layers at rows 100 (A), 110 (B) and 20 (C), backdrops of
    /// colour 5 (and 6 in the cave), 128 rows each.
    fn assets() -> Assets {
        let drop = vgaintro::parse(&synthetic_vgaintro(&numbers())).unwrap();
        let layer = |row| Layer {
            rows: [row; Layer::LEN],
        };
        let part = |name, colour, layers| Part {
            name,
            layers,
            backdrop: vec![colour; 768 * 128],
            palette: [colour; DAC_BYTES],
        };
        Assets {
            drop,
            parts: [
                part("x.pcx", 5, vec![layer(100), layer(110)]),
                part("c.pcx", 6, vec![layer(100), layer(110), layer(20)]),
            ],
        }
    }

    fn down(code: u16) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat: false,
        }
    }

    #[test]
    fn the_script_accelerates_cruises_slows_and_stops() {
        let n = numbers();
        let mut s = Script::new(&n);
        let mut speeds = Vec::new();
        let mut events = Vec::new();
        while !s.ended {
            events.push(s.frame(&n));
            speeds.push(s.speeds);
        }
        // Frames 1..=3 add the acceleration; 4 sets the cruise speeds and steps.
        assert_eq!(speeds[0], [0x40, 0x20, 0x40, 0x10]);
        assert_eq!(speeds[2], [0xc0, 0x60, 0xc0, 0x30]);
        assert_eq!(speeds[3], [0x100, 0x80, 0x100, 0x40]);
        // The first part ends at frame 10 without a step.
        assert_eq!(s.frame, 10);
        assert!(!events[9].stepped && events[8].stepped);
        // Moved: A 0x40 + 0x80 + 0xc0 + 6 * 0x100 = 7.75 columns, B 3.75, the view 1.875.
        assert_eq!(s.offsets, [1152 - 7, 1152 - 3, 1152 - 7]);
        assert_eq!(s.view_x, 448 - 1);

        // The cave part: same speeds, frame count running on.
        s.restart(&n);
        let mut events = Vec::new();
        while !s.ended {
            events.push((s.frame, s.frame(&n)));
        }
        // The jump (frame 14) moves the view 5 left and changes the light.
        let jump = events.iter().find(|e| e.1.light).unwrap();
        assert_eq!(jump.0 + 1, 14);
        // B's boost at 16, slowing on frames 20..=23 to exactly 0, stop at 24, holds
        // until 30 and 34, then the end state.
        assert_eq!(s.speeds, [0; 4]);
        assert_eq!((s.frame, s.state), (35, 5));
        let last_step = events.iter().rev().find(|e| e.1.stepped).unwrap().0 + 1;
        assert_eq!(last_step, 33);
        // Frames 11..=19 cruise (B boosted from 16), then 20..=23 slow down; the
        // fractions carry over from the first part.
        assert_eq!(s.offsets, [1152 - 11, 1152 - 8, 1152 - 11]);
        assert_eq!(s.view_x, 448 - 5 - 3);
    }

    /// A column of the surface, rows 0..8 of the backdrop.
    fn column(pixels: &[u8], c: usize) -> Vec<u8> {
        (0..8).map(|r| pixels[at(r, c).unwrap()]).collect()
    }

    #[test]
    fn silhouettes_fill_and_restore_per_column_like_the_driver() {
        let mut pixels = vec![0; SURFACE.width * SURFACE.height];
        let backdrop: Vec<u8> = (0..768 * 8).map(|i| 10 + (i / 768) as u8).collect();
        draw_backdrop(&mut pixels, &backdrop);
        assert_eq!(column(&pixels, 3), [10, 11, 12, 13, 14, 15, 16, 17]);
        // The floor rises from 7 to 4: rows 4..=7 black.
        floor_column(&mut pixels, 3, 7, 4);
        assert_eq!(column(&pixels, 3), [10, 11, 12, 13, 0, 0, 0, 0]);
        // Falls back to 6: rows 4..=6 restored, the new edge row 6 included.
        floor_column(&mut pixels, 3, 4, 6);
        assert_eq!(column(&pixels, 3), [10, 11, 12, 13, 14, 15, 16, 0]);
        // A ceiling to row 2, then down to 3, then up to 1 (rows 1..=3 restored).
        ceiling_full_column(&mut pixels, 5, 2);
        assert_eq!(column(&pixels, 5)[..4], [0, 0, 0, 13]);
        ceiling_column(&mut pixels, 5, 2, 3);
        assert_eq!(column(&pixels, 5)[..5], [0, 0, 0, 0, 14]);
        ceiling_column(&mut pixels, 5, 3, 1);
        assert_eq!(column(&pixels, 5)[..5], [0, 11, 12, 13, 14]);
        // The count is a signed byte, as in the driver: from 0 to 200 it is +56, so
        // rows 200..=256 (below the split) turn black and rows 0..=200 are not restored.
        let mut more = pixels.clone();
        floor_column(&mut more, 7, 0, 200);
        assert_eq!(column(&more, 7), column(&pixels, 7));
    }

    #[test]
    fn the_backdrop_bottom_rows_overwrite_the_clean_copy() {
        let mut pixels = vec![0; SURFACE.width * SURFACE.height];
        let backdrop: Vec<u8> = (0..768 * 130).map(|i| (i / 768) as u8).collect();
        draw_backdrop(&mut pixels, &backdrop);
        // Row 0's clean copy holds row 128, so a floor leaving row 0 shows row 128.
        floor_column(&mut pixels, 0, 0, 1);
        assert_eq!(column(&pixels, 0)[..2], [128, 129]);
    }

    /// A retrace's counter, frame and view origin.
    type Seen = (u16, u16, (usize, usize));

    /// Plays to the end (at most 2,000 retraces) with `keys` at their retraces;
    /// returns the player and each retrace's (counter, frame, origin).
    fn play(keys: &[(u16, u16)]) -> (Player, Vec<Seen>) {
        let mut player = Player::new(assets(), 90);
        let mut fb = player.framebuffer();
        let mut seen = Vec::new();
        for r in 1..2000 {
            let events: Vec<_> = keys
                .iter()
                .filter(|(at, _)| *at == r)
                .map(|&(_, k)| down(k))
                .collect();
            let done = player.retrace(&events, &mut fb);
            seen.push((player.counter, player.script.frame, fb.origin()));
            if done {
                return (player, seen);
            }
        }
        panic!("the drop never ends");
    }

    #[test]
    fn the_parts_hand_over_and_the_cave_ends_on_the_counter() {
        let (player, seen) = play(&[]);
        assert!(!player.aborted);
        let at = |frame| seen.iter().find(|s| s.1 == frame).unwrap().0;
        // Blends of 3 retraces at 100, 110 and 120; frames from 130, two retraces each.
        assert_eq!((at(1), at(2), at(10)), (130, 132, 148));
        // The cave part follows at once, its frame count running on.
        assert_eq!(at(11), 150);
        // Ends after the frame whose wait passes 200, then a 3-step blend.
        let last = seen.iter().rev().find(|s| s.0 < 200).unwrap();
        assert_eq!(last.1, 35);
        assert_eq!(player.counter, 203);
        // The view starts at 448 in each part, at row 72.
        assert!(seen.iter().any(|s| s.2 == (448, 72)));
        assert_eq!(seen.last().unwrap().2, (448 - 5 - 3, 72));
    }

    #[test]
    fn a_key_ends_the_drop_at_once_and_p_pauses_it() {
        let (player, seen) = play(&[(50, 0x1c)]);
        assert!(player.aborted);
        assert_eq!(seen.len(), 50);
        // P at retrace 45 (counter 135) pauses at the next frame's poll (136); the key
        // at 60 (150) resumes. The frame's start was noted before the poll, so the
        // next frame follows one retrace later. The counter ran on, so the cave part,
        // which ends at counter 200, is 6 frames shorter.
        let (paused, seen) = play(&[(45, 0x19), (60, 0x1e)]);
        assert!(!paused.aborted);
        assert_eq!((seen.len(), paused.script.frame), (114, 29));
        assert_eq!(play(&[]).1.len(), 113);
    }

    #[test]
    fn the_view_shows_the_backdrop_above_the_split_and_the_credits_below() {
        let mut player = Player::new(assets(), 90);
        let mut fb = player.framebuffer();
        for _ in 0..5 {
            player.retrace([], &mut fb);
        }
        assert_eq!(fb.origin(), (448, 72));
        // The backdrop (colour 5) above the floor at row 100, black floor, the clean
        // credits area below the split.
        let row = |r: usize| fb.pixels()[fb.view_row_start(r)];
        assert_eq!(
            (row(0), row(99), row(100), row(127), row(128)),
            (5, 5, 0, 0, 0)
        );
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_drop_loads_and_plays_to_its_end() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files).unwrap();
        let n = assets.drop.numbers.clone();
        let mut player = Player::new(assets, n.start);
        let mut fb = player.framebuffer();
        let mut first_part_end = None;
        let mut retraces = 0;
        while !player.retrace([], &mut fb) {
            retraces += 1;
            assert!(retraces < 10_000, "the drop never ends");
            if player.part == 1 && first_part_end.is_none() {
                first_part_end = Some(player.script.frame);
            }
        }
        assert!(!player.aborted);
        // The cave's first frame runs in the retrace the first part ends.
        assert_eq!(first_part_end, Some(n.first_part_end + 1));
        // The cave part reached the end of the frame script and ran until its end time.
        assert_eq!(player.script.state, 5);
        assert!(player.counter >= n.cave_end + n.last_blend_steps);
        assert!(usize::from(player.script.view_x) + 320 <= SURFACE.width);
    }
}
