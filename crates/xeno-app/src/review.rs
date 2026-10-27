//! The review (T-044), as `ui_run_review_screen` and its widgets run it
//! (`docs/screens.md`, "The review (T-044)"): a reference of the robot kinds, one page
//! each, with three text panels and a view where the page's robot walks and turns.
//! Tab, Alt+Tab and the next button step the pages; ESC, Alt+Q and the exit button go
//! back to the map room. The widgets, keys, panels, font, colour and texts all come
//! from the user's `xeno.exe` ([`xeno_formats::review`]).
//!
//! The robot needs the 3D renderer (T-056), so its view is a labelled placeholder for
//! T-047.

use xeno_formats::font::Font;
use xeno_formats::maproom::Rect;
use xeno_formats::review::{self, Review};
use xeno_render::{Framebuffer, TextCursor, TextMode};

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
/// and a captured press.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewState {
    page: usize,
    xored: [bool; 2],
    capture: Option<(Button, MouseButton)>,
    dirty: bool,
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
            self.dirty = true;
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
    /// the robot's view a placeholder; the XORed button frames.
    pub fn draw(&self, fb: &mut Framebuffer, tables: &Review, font: &Font) {
        if let Some(page) = tables.pages.get(self.page) {
            for (panel, text) in tables.panels.iter().zip(&page.texts) {
                mission::fill(fb, *panel, PANEL_FILL);
                let (x, y) = (i32::from(panel.left), i32::from(panel.top));
                let mut cursor = TextCursor::at(x, y, tables.colour);
                fb.draw_text(font, &mut cursor, text, TextMode::Transparent);
            }
        }
        mission::hatch(fb, 255, Some(font), "T-047");
        for button in [Button::Next, Button::Exit] {
            if self.xored[button as usize] {
                mission::xor_frame(fb, area(tables, button), PRESSED_XOR);
            }
        }
    }
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
        r.draw(&mut fb, &tables, &font);
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
        r.draw(&mut fb, &tables, &font);
        assert_eq!(fb.get(55, 180), Some(7 ^ 9));
    }
}
