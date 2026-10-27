//! The review (T-044), as `ui_run_review_screen` and its widgets run it
//! (`docs/screens.md`, "The review (T-044)"): a reference of the robot kinds, one page
//! each, with three text panels and a view where the page's robot walks and turns.
//! Tab, Alt+Tab and the next button step the pages; ESC, Alt+Q and the exit button go
//! back to the map room. The widgets, keys, panels, font, colour and texts all come
//! from the user's `xeno.exe` ([`xeno_formats::review`]).
//!
//! Each page's robot walks and turns in the view by its script and parts routines, one
//! step a 60 Hz tick (`ui_review_tick`, T-047), drawn as the view's mode 0x1a draws it:
//! the buffer cleared to 0, the model at a fixed point and the script's scale, copied
//! through the picture's colour-255 pixels.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::mxx;
use xeno_formats::review::{self, FRAME_BYTES, Page, Review, Step};
use xeno_render::{
    Framebuffer, LoadedModel, ModelScale, ModelTarget, Size, TextCursor, TextMode, model_facing,
};
use xeno_sim::rate::Rate;

use crate::game_dir::GameFiles;
use crate::input::MouseButton;
use crate::mission;

/// The colour the panels are filled with before their text (`AL = 0` to slot `0x51a`).
const PANEL_FILL: u8 = 0;
/// The XOR of a pressed button's frame (`1000:dfd5`), as the training menu's.
const PRESSED_XOR: u8 = 9;

/// What the screens must do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Leave,
}

/// The models' files by model number / 2 (`docs/formats/mxx.md`, "Loading").
const MODEL_FILES: [&str; 11] = [
    "humanoid", "scorpion", "scout", "missile", "netrelay", "explode", "plasma", "laser",
    "basedome", "cloak", "obst",
];

/// Each page's model as the start-up loads a robot's (`.m00`, then `.m08` and `.m16`),
/// or none, with a warning, when it can't be read: the page then shows an empty view.
pub fn load_models(files: &GameFiles, tables: &Review) -> Vec<Option<LoadedModel>> {
    let read = |name: &str| -> Result<mxx::Model, String> {
        let bytes = files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))?;
        mxx::parse(&bytes).map_err(|e| format!("{name}: {e}"))
    };
    let load = |model: u8| -> Result<LoadedModel, String> {
        let name = MODEL_FILES
            .get(usize::from(model / 2))
            .ok_or(format!("no model {model}"))?;
        let passes = [read(&format!("{name}.m08"))?, read(&format!("{name}.m16"))?];
        Ok(LoadedModel::new(&read(&format!("{name}.m00"))?, &passes))
    };
    let pages = tables.pages.iter().map(|page| load(page.model));
    pages
        .map(|model| model.map_err(|e| log::warn!("review: {e}")).ok())
        .collect()
}

/// The two buttons, in the order of [`Review::next_button`] and
/// [`Review::exit_button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Next,
    Exit,
}

fn area(tables: &Review, button: Button) -> Rect {
    match button {
        Button::Next => tables.next_button,
        Button::Exit => tables.exit_button,
    }
}

/// The review's state: the page (`DS:0x61aa / 2`), which buttons' frames are XORed,
/// a captured press, and the page's robot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewState {
    page: usize,
    xored: [bool; 2],
    capture: Option<(Button, MouseButton)>,
    dirty: bool,
    robot: Robot,
    /// The review's tick count (`DS:0x61d2`), kept across pages.
    ticks: u16,
    /// Retraces and 60 Hz ticks since the review opened.
    retraces: u64,
    sixty_hz: u64,
}

/// What a script step changes each tick (`DS:0x61da`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Change {
    #[default]
    None,
    Turn(u16),
    Grow(u16),
    Shrink(u16),
}

/// The page's robot as `ui_review_set_up_page` starts it: heading, scale and frames 0,
/// standing, the script's first step due at the next tick.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Robot {
    /// `DS:0x61ac`, 65,536 to a turn.
    heading: u16,
    /// `DS:0x61b6`, 256 = 1:1.
    scale: u16,
    frames: [u8; FRAME_BYTES],
    /// `DS:0x61e2` and the walking counter `DS:0x61e4`.
    walking: bool,
    stride: u16,
    /// The next step, the ticks left of this one (`DS:0x61d6`, `0x61d8`) and its change.
    next: usize,
    left: u16,
    change: Change,
}

impl Default for Robot {
    fn default() -> Robot {
        Robot {
            heading: 0,
            scale: 0,
            frames: [0; FRAME_BYTES],
            walking: false,
            stride: 0,
            next: 0,
            left: 1,
            change: Change::None,
        }
    }
}

impl Robot {
    /// Runs steps until one that lasts (`ui_review_step_script`): jumps and the parts
    /// switches go straight on to the next.
    fn step(&mut self, script: &[Step]) {
        for _ in 0..=script.len() {
            let Some(&step) = script.get(self.next) else {
                return;
            };
            self.next += 1;
            (self.left, self.change) = match step {
                Step::Turn { ticks, by } => (ticks, Change::Turn(by)),
                Step::Wait { ticks } => (ticks, Change::None),
                Step::Grow { ticks, by } => (ticks, Change::Grow(by)),
                Step::Shrink { ticks, by } => (ticks, Change::Shrink(by)),
                Step::Jump { to } => {
                    self.next = to;
                    continue;
                }
                Step::Walk | Step::Stand => {
                    self.walking = step == Step::Walk;
                    continue;
                }
            };
            return;
        }
    }

    /// One tick (`ui_review_tick`): the script, the step's change, then the parts
    /// routine. `ticks` is the review's count, already advanced.
    fn tick(&mut self, page: &Page, ticks: u16) {
        self.left = self.left.wrapping_sub(1);
        if self.left == 0 {
            self.step(&page.script);
        }
        match self.change {
            Change::None => {}
            Change::Turn(by) => self.heading = self.heading.wrapping_add(by),
            Change::Grow(by) => self.scale = self.scale.wrapping_add(by),
            Change::Shrink(by) => self.scale = self.scale.wrapping_sub(by),
        }
        let frames = &mut self.frames;
        if self.walking {
            self.stride = self.stride.wrapping_add(page.walking.speed.into());
            let at = usize::from(self.stride as u8 >> 4);
            for (table, parts) in &page.walking.tables {
                parts.iter().for_each(|&f| frames[f] = table[at]);
            }
        } else if ticks & page.standing.every == 0 {
            for rule in &page.standing.rules {
                let first = rule.frames[0];
                match (frames[first], rule.wrap) {
                    (0, _) => {}
                    (_, None) => frames[first] -= 1,
                    (_, Some(wrap)) => {
                        rule.frames
                            .iter()
                            .for_each(|&f| frames[f] = frames[f].wrapping_add(1));
                        if frames[first] >= wrap {
                            rule.frames.iter().for_each(|&f| frames[f] = 0);
                        }
                    }
                }
            }
        }
    }
}

impl ReviewState {
    /// A review opened from the map room: the first page (`ui_review_reset`).
    pub fn open() -> ReviewState {
        ReviewState {
            dirty: true,
            ..ReviewState::default()
        }
    }

    #[cfg(test)]
    pub fn page(&self) -> usize {
        self.page
    }

    /// Whether it needs drawing again; clears it.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The next (`step` 1) or previous page, wrapping (`ui_review_set_up_page`).
    fn turn(&mut self, step: isize, tables: &Review) {
        let n = tables.pages.len() as isize;
        if n > 0 {
            self.page = (self.page as isize + step).rem_euclid(n) as usize;
            log::debug!("review: page {}", self.page);
            self.robot = Robot::default();
            self.dirty = true;
        }
    }

    /// One retrace: the robot steps once for each 60 Hz tick it brings (the game clock
    /// `DS:0x374`).
    pub fn retrace(&mut self, tables: &Review) {
        self.retraces += 1;
        let due = Rate::MODE_13H.convert(self.retraces, Rate::SIXTY_HZ);
        while self.sixty_hz < due {
            self.sixty_hz += 1;
            self.ticks = self.ticks.wrapping_add(1);
            if let Some(page) = tables.pages.get(self.page) {
                self.robot.tick(page, self.ticks);
                self.dirty = true;
            }
        }
    }

    /// The key word `word` went down (`ui_review_on_key`).
    pub fn key(&mut self, word: u16, tables: &Review) -> Option<Request> {
        let handler = tables
            .keys
            .iter()
            .find_map(|&(w, h)| (w == word & !0x80).then_some(h))?;
        match handler {
            review::LEAVE => return Some(Request::Leave),
            review::NEXT_PAGE => self.turn(1, tables),
            review::PREVIOUS_PAGE => self.turn(-1, tables),
            other => log::debug!("review: key {word:#06x} names handler {other:#06x}"),
        }
        None
    }

    fn over(tables: &Review, at: Option<(u16, u16)>) -> Option<Button> {
        let (x, y) = at?;
        [Button::Next, Button::Exit]
            .into_iter()
            .find(|&b| area(tables, b).contains(x, y))
    }

    fn xor(&mut self, button: Button) {
        self.xored[button as usize] ^= true;
        self.dirty = true;
    }

    /// A button went down: one over a review button captures the pointer and XORs its
    /// frame.
    pub fn mouse_down(&mut self, button: MouseButton, at: Option<(u16, u16)>, tables: &Review) {
        if self.capture.is_some() || button == MouseButton::Middle {
            return;
        }
        if let Some(target) = Self::over(tables, at) {
            self.capture = Some((target, button));
            self.xor(target);
        }
    }

    /// A button went up: over the captured review button it XORs the frame back, and a
    /// left release acts. Elsewhere nothing happens and the frame stays XORed.
    pub fn mouse_up(
        &mut self,
        button: MouseButton,
        at: Option<(u16, u16)>,
        tables: &Review,
    ) -> Option<Request> {
        let (target, by) = self.capture?;
        if by != button {
            return None;
        }
        self.capture = None;
        if Self::over(tables, at) != Some(target) {
            return None;
        }
        self.xor(target);
        if button != MouseButton::Left {
            return None;
        }
        match target {
            Button::Next => {
                self.turn(1, tables);
                None
            }
            Button::Exit => Some(Request::Leave),
        }
    }

    /// Draws the page over the review's picture, which `fb` already shows: each panel
    /// filled, then its text from its top-left corner (`ui_draw_review_text_panels`);
    /// the robot's view; the XORed button frames. `models` are the pages' models, in
    /// page order.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        tables: &Review,
        font: &Font,
        models: &[Option<LoadedModel>],
    ) {
        if let Some(page) = tables.pages.get(self.page) {
            for (panel, text) in tables.panels.iter().zip(&page.texts) {
                mission::fill(fb, *panel, PANEL_FILL);
                let (x, y) = (i32::from(panel.left), i32::from(panel.top));
                let mut cursor = TextCursor::at(x, y, tables.colour);
                fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
            }
            self.draw_robot(fb, page, models.get(self.page));
        }
        for button in [Button::Next, Button::Exit] {
            if self.xored[button as usize] {
                mission::xor_frame(fb, area(tables, button), PRESSED_XOR);
            }
        }
    }

    /// The view (mode 0x1a: `1000:374e`, `1000:2ea4` and the node routine `1000:3424`):
    /// a buffer the size of the picture's colour-255 region cleared to 0, the model's
    /// anchor at its centre less the page's height, facing by its heading alone, at the
    /// script's scale; then the buffer copied through the region (slot `0x5a4`).
    fn draw_robot(&self, fb: &mut Framebuffer, page: &Page, model: Option<&Option<LoadedModel>>) {
        let Some((left, top, width, height)) = region(fb) else {
            return;
        };
        let mut buffer = Framebuffer::new(Size::new(width, height));
        if let Some(Some(model)) = model {
            let target = ModelTarget {
                clip: xeno_render::Rect::new(0, 0, width as u32, height as u32),
                anchor: (
                    (width / 2) as i32,
                    (height / 2) as i32 - i32::from(page.height),
                ),
                scale: ModelScale::new(self.robot.scale),
            };
            let (facing, frames) = (model_facing(self.robot.heading), &self.robot.frames);
            buffer.draw_model(model, facing, frames, &target);
        }
        let stride = fb.surface().width;
        let pixels = fb.pixels_mut();
        for (y, row) in buffer.pixels().chunks(width).enumerate() {
            let start = (top + y) * stride + left;
            for (p, &colour) in pixels[start..start + width].iter_mut().zip(row) {
                if *p == 255 {
                    *p = colour;
                }
            }
        }
    }
}

/// The bounding box of `fb`'s colour-255 pixels as slot `0x59c` measures it (left,
/// top, width, height): when the width is odd, the left edge moves down to an even
/// column and the right edge up to one.
fn region(fb: &Framebuffer) -> Option<(usize, usize, usize, usize)> {
    let width = fb.surface().width;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, _) in fb.pixels().iter().enumerate().filter(|&(_, &p)| p == 255) {
        let (x, y) = (i % width, i / width);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
    }
    if x1 == 0 {
        return None;
    }
    if (x0 ^ x1) & 1 == 1 {
        (x0, x1) = (x0 & !1, (x1 + 1) & !1);
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use xeno_formats::fixtures::synthetic_review;
    use xeno_render::Size;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    /// The synthetic review of `xeno_formats::fixtures`: two pages, ESC, Tab and
    /// Alt+Tab, the next button at (180, 10, 190, 50) and the exit at (180, 55, 195,
    /// 100), font 2 in colour 0x33.
    pub fn tables() -> Review {
        let ds = 0x20000;
        let mut image = vec![0u8; ds + 0x400];
        let relocated = synthetic_review(&mut image, ds);
        review::read_image(&image, ds, &relocated).unwrap()
    }

    #[test]
    fn keys_and_the_next_button_step_the_pages_and_wrap() {
        let tables = tables();
        let mut r = ReviewState::open();
        assert_eq!(r.key(0x0f, &tables), None);
        assert_eq!(r.page(), 1);
        r.key(0x0f, &tables);
        assert_eq!(r.page(), 0, "past the last page, the first");
        r.key(0x200f, &tables);
        assert_eq!(r.page(), 1, "before the first, the last");
        // A key with its release bit is the same key; an unbound key does nothing.
        r.key(0x8f, &tables);
        assert_eq!(r.page(), 0);
        assert_eq!(r.key(0x1c, &tables), None);
        assert_eq!(r.key(0x01, &tables), Some(Request::Leave));
        // A left release over the next button turns the page; a right one only XORs
        // the frame twice.
        r.mouse_down(LEFT, Some((20, 185)), &tables);
        assert!(r.xored[0]);
        assert_eq!(r.mouse_up(LEFT, Some((20, 185)), &tables), None);
        assert_eq!((r.page(), r.xored[0]), (1, false));
        r.mouse_down(RIGHT, Some((20, 185)), &tables);
        r.mouse_up(RIGHT, Some((20, 185)), &tables);
        assert_eq!((r.page(), r.xored[0]), (1, false));
        // Released off the exit button: nothing, and its frame stays XORed.
        r.mouse_down(LEFT, Some((60, 185)), &tables);
        assert_eq!(r.mouse_up(LEFT, Some((5, 5)), &tables), None);
        assert!(r.xored[1]);
        r.mouse_down(LEFT, Some((60, 185)), &tables);
        assert_eq!(
            r.mouse_up(LEFT, Some((60, 185)), &tables),
            Some(Request::Leave)
        );
    }

    /// Runs `n` ticks of page 0's robot, counting from `*ticks`.
    fn run(robot: &mut Robot, page: &Page, ticks: &mut u16, n: usize) {
        for _ in 0..n {
            *ticks += 1;
            robot.tick(page, *ticks);
        }
    }

    #[test]
    fn the_script_turns_grows_and_walks_the_robot() {
        // `SYNTHETIC_REVIEW_SCRIPT`: grow 0x80 for 2 ticks, walk, turn 0x400 for 3, wait
        // 2, stand, back to the walk. Walking steps the counter 16 a tick, frames 1 and
        // 4 from the table 0, 1, 2, 0, ... by bits 4 to 7.
        let tables = tables();
        let page = &tables.pages[0];
        let (mut robot, mut ticks) = (Robot::default(), 0);
        run(&mut robot, page, &mut ticks, 2);
        assert_eq!(
            (robot.scale, robot.heading, robot.walking),
            (0x100, 0, false)
        );
        run(&mut robot, page, &mut ticks, 1);
        assert_eq!((robot.heading, robot.walking), (0x400, true));
        assert_eq!((robot.frames[1], robot.frames[4]), (1, 1));
        run(&mut robot, page, &mut ticks, 2);
        assert_eq!((robot.heading, robot.frames[1]), (0xc00, 0));
        // The wait changes nothing but the legs.
        run(&mut robot, page, &mut ticks, 2);
        assert_eq!((robot.heading, robot.frames[4]), (0xc00, 2));
        // Stand, then the jump back to the walk, and the turn again, all at once.
        run(&mut robot, page, &mut ticks, 1);
        assert_eq!(
            (robot.heading, robot.walking, robot.left),
            (0x1000, true, 3)
        );
        assert_eq!(robot.scale, 0x100);
        // A shrink takes from the scale (no stock script reaches one).
        let mut page = page.clone();
        page.script = vec![Step::Shrink { ticks: 2, by: 0x40 }, Step::Wait { ticks: 9 }];
        let mut robot = Robot {
            scale: 0x100,
            ..Robot::default()
        };
        run(&mut robot, &page, &mut ticks, 3);
        assert_eq!(robot.scale, 0x80);
    }

    #[test]
    fn standing_settles_the_parts_every_fourth_tick() {
        // Frames 1 and 2 step up together and go back to 0 at 3; frame 3 steps down.
        let tables = tables();
        let page = &tables.pages[0];
        let mut robot = Robot {
            left: 100,
            ..Robot::default()
        };
        robot.frames[..4].copy_from_slice(&[0, 1, 1, 2]);
        robot.tick(page, 5);
        assert_eq!(robot.frames[..4], [0, 1, 1, 2], "not on a fourth tick");
        robot.tick(page, 8);
        assert_eq!(robot.frames[..4], [0, 2, 2, 1]);
        robot.tick(page, 12);
        assert_eq!(robot.frames[..4], [0, 0, 0, 0]);
        // A rule whose first frame is 0 is left alone, whatever the others.
        robot.frames[2] = 2;
        robot.tick(page, 16);
        assert_eq!(robot.frames[..4], [0, 0, 2, 0]);
    }

    #[test]
    fn the_robot_ticks_at_60_hz_and_restarts_with_each_page() {
        let tables = tables();
        let mut r = ReviewState::open();
        for _ in 0..70 {
            r.retrace(&tables);
        }
        assert_eq!(
            u64::from(r.ticks),
            Rate::MODE_13H.convert(70, Rate::SIXTY_HZ)
        );
        assert_eq!(r.ticks, 59);
        assert_ne!(r.robot, Robot::default());
        r.key(0x0f, &tables);
        assert_eq!((r.robot.clone(), r.ticks), (Robot::default(), 59));
    }

    #[test]
    fn the_robot_is_drawn_through_the_views_colour_255_pixels() {
        use xeno_formats::mxx::{Frame, Image, Part, Span, View, ViewHeader};
        // One view at facing 32 (heading 0): a 1x1 image of colour 9 at the anchor.
        let view = View {
            header: ViewHeader {
                size: 0,
                facing: 32,
                mirror_facing: 32,
                index_offset: 0,
                mirror_index_offset: 0,
                lists_offset: 0,
                mirror_lists_offset: 0,
                data_offset: 0,
            },
            animations: vec![vec![Frame {
                depth: 0,
                parts: vec![Part {
                    image: 0,
                    x: 0,
                    y: 0,
                }],
            }]],
            mirror_animations: None,
            images: vec![Image {
                width: 1,
                height: 1,
                rows: vec![Span {
                    skip: 0,
                    pixels: vec![9],
                }],
            }],
        };
        let empty = mxx::Model { views: Vec::new() };
        let model = LoadedModel::new(&empty, &[mxx::Model { views: vec![view] }]);
        let (tables, font) = (tables(), synthetic_font());
        let mut r = ReviewState::open();
        r.robot.scale = 0x100;
        // The colour-255 region is columns 161..310 and rows 15..111: 149 wide, so
        // measured from column 160, 150 by 96. The anchor is (75, 48) less page 0's
        // height 5: (235, 58) on screen. Only colour-255 pixels take the buffer.
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(3);
        fb.fill_rect(xeno_render::Rect::new(161, 15, 149, 96), 255);
        fb.fill_rect(xeno_render::Rect::new(170, 20, 10, 10), 7);
        assert_eq!(region(&fb), Some((160, 15, 150, 96)));
        r.draw(&mut fb, &tables, &font, &[Some(model.clone())]);
        assert_eq!(fb.get(235, 58), Some(9));
        assert_eq!(fb.get(236, 58), Some(0));
        assert_eq!((fb.get(161, 15), fb.get(309, 110)), (Some(0), Some(0)));
        assert_eq!((fb.get(160, 15), fb.get(310, 110)), (Some(3), Some(3)));
        assert_eq!(fb.get(175, 25), Some(7));
        // At scale 0 nothing is drawn; facing 0 (turned half round) has no view here.
        for (scale, heading) in [(0, 0), (0x100, 0x8000)] {
            (r.robot.scale, r.robot.heading) = (scale, heading);
            fb.fill_rect(xeno_render::Rect::new(161, 15, 149, 96), 255);
            r.draw(&mut fb, &tables, &font, &[Some(model.clone())]);
            assert_eq!(fb.get(235, 58), Some(0));
        }
    }

    fn synthetic_font() -> Font {
        xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block())
            .unwrap()
            .remove(0)
    }

    #[test]
    fn the_page_fills_its_panels_and_draws_its_texts() {
        let mut tables = tables();
        let font = xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block())
            .unwrap()
            .remove(0);
        // The synthetic font draws only `A`: page 0's first panel `A`, its second `AA`
        // over two lines.
        tables.pages[0].texts = [b"A".to_vec(), b"A\x0cA".to_vec(), Vec::new()];
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(7);
        let mut r = ReviewState::open();
        r.draw(&mut fb, &tables, &font, &[]);
        // Panels (20, 10, 80, 150) and (110, 10, 180, 150): filled with 0, `A`'s bar in
        // colour 0x33 at each corner, the new line 3 rows down at the left edge.
        assert_eq!(fb.get(149, 79), Some(0));
        assert_eq!(fb.get(150, 80), Some(7));
        assert_eq!(fb.get(10, 20), Some(0x33));
        assert_eq!(fb.get(12, 110), Some(0x33));
        assert_eq!(fb.get(10, 113), Some(0x33));
        assert_eq!(fb.get(14, 113), Some(0));
        assert!(r.take_dirty() && !r.take_dirty());
        // The exit button's frame XORed with 9 while pressed.
        r.mouse_down(LEFT, Some((60, 185)), &tables);
        r.draw(&mut fb, &tables, &font, &[]);
        assert_eq!(fb.get(55, 180), Some(7 ^ 9));
    }
}
