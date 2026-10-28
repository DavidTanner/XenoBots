//! A robot's message panel (+0x52 to +0xda; `docs/screens.md`, "The message panel
//! (T-104)"): the messages waiting, typed one character a tick into a text of up to
//! six lines that scrolls up a line when a seventh begins.

use xeno_formats::motion::Strings;

/// The text's bytes (+0x5e up to the call sign at +0xdb).
pub const TEXT_LEN: usize = 0x7d;
/// +0x52: the lines after the first (bits 0 to 3), a line just ended (bit 6), and
/// the text moved up, so the console redraws the panel from the top (bit 7).
const LINES: u8 = 0x0f;
const LINE_ENDED: u8 = 0x40;
pub const REDRAW: u8 = 0x80;
const MAX_LINES: u8 = 5;
/// The font's new line; a message's CR becomes one too.
const NEW_LINE: u8 = 0x0c;
const CR: u8 = 0x0d;
/// The last index a message's closing new line may take (`cmp bx, 0x7c`).
const LAST: usize = 0x7c;
/// `1000:d2a2` looks for a free entry among the first four.
const FREE_ENTRIES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// +0x52.
    pub flags: u8,
    /// +0x53: where the next character goes.
    pub length: u8,
    /// +0x54: the messages waiting, string-segment offsets (0: none); the first is
    /// being typed and moves on as it is.
    pub queue: [u16; 5],
    /// +0x5e: the text, NUL-terminated.
    pub text: [u8; TEXT_LEN],
}

impl Default for Panel {
    fn default() -> Panel {
        Panel {
            flags: 0,
            length: 0,
            queue: [0; 5],
            text: [0; TEXT_LEN],
        }
    }
}

impl Panel {
    /// The text as the console shows it: up to its NUL.
    pub fn shown(&self) -> &[u8] {
        let end = self.text.iter().position(|&b| b == 0).unwrap_or(TEXT_LEN);
        &self.text[..end]
    }

    /// `1000:d2a2`: message `text` waits in the first free entry; with none it is
    /// dropped.
    pub fn post(&mut self, text: u16) {
        if let Some(entry) = self.queue[..FREE_ENTRIES].iter_mut().find(|e| **e == 0) {
            *entry = text;
        }
    }

    fn set(&mut self, at: usize, byte: u8) {
        // shortcut: the original writes on past the text into the call sign and the
        // fields after it; no stock message line is that long.
        if let Some(b) = self.text.get_mut(at) {
            *b = byte;
        }
    }

    /// `1fe6:8142`: a new line begins: counted, or with six lines already the first
    /// is dropped and the rest move up.
    fn new_line(&mut self) {
        self.flags &= !LINE_ENDED;
        let lines = self.flags & LINES;
        if lines != MAX_LINES {
            // The whole byte is written: the redraw bit goes too.
            self.flags = lines + 1;
            return;
        }
        self.flags |= REDRAW;
        let first = self
            .text
            .iter()
            .position(|&b| b == NEW_LINE)
            .unwrap_or(LAST)
            + 1;
        self.length = self.length.wrapping_sub(first as u8);
        // shortcut: 100 bytes are copied, reading past the text into the next fields;
        // the port reads zeros there, all after the text's NUL.
        let moved: Vec<u8> = (first..first + 100)
            .map(|i| self.text.get(i).copied().unwrap_or(0))
            .collect();
        self.text[..100].copy_from_slice(&moved);
    }

    /// `1fe6:826d`, every tick after the robot moves: the next character of the
    /// first message waiting. `true` when something was typed (the console then
    /// redraws the panel).
    pub fn type_one(&mut self, strings: &Strings) -> bool {
        let Some(&at) = self.queue.first().filter(|&&a| a != 0) else {
            return false;
        };
        let mut i = usize::from(self.length);
        if self.flags & LINE_ENDED != 0 {
            self.new_line();
            i = usize::from(self.length);
        }
        match strings.byte(at) {
            0 => {
                // The message ends with a new line.
                if i >= LAST {
                    i -= 1;
                }
                self.set(i, NEW_LINE);
                self.set(i + 1, 0);
                self.length = (i + 1) as u8;
                self.flags |= LINE_ENDED;
                self.queue.copy_within(1.., 0);
                self.queue[4] = 0;
            }
            c => {
                if c == CR || c == NEW_LINE {
                    self.set(i, NEW_LINE);
                    self.flags |= LINE_ENDED;
                } else {
                    self.set(i, c);
                }
                self.set(i + 1, 0);
                self.length = (i + 1) as u8;
                self.queue[0] = at.wrapping_add(1);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings() -> Strings {
        // Offsets 1 "AB", 4 "C\rD", 8 a line of one character.
        Strings(b"\0AB\0C\rD\0E\0".to_vec())
    }

    fn typed(p: &mut Panel) -> usize {
        let s = strings();
        (0..).take_while(|_| p.type_one(&s)).count()
    }

    #[test]
    fn messages_wait_and_type_a_character_a_tick() {
        let mut p = Panel::default();
        p.post(1);
        p.post(4);
        assert!(p.type_one(&strings()));
        assert_eq!(p.shown(), b"A");
        // Two characters and the closing new line, then the second message.
        assert_eq!(typed(&mut p), 2 + 4);
        assert_eq!(p.shown(), b"AB\x0cC\x0cD\x0c");
        assert_eq!(p.flags, 2 | LINE_ENDED);
        assert_eq!((p.queue, p.length), ([0; 5], 7));
        // Only four can wait.
        (0..6).for_each(|_| p.post(8));
        assert_eq!(p.queue, [8, 8, 8, 8, 0]);
    }

    #[test]
    fn a_seventh_line_moves_the_text_up() {
        let mut p = Panel::default();
        for _ in 0..6 {
            p.post(8);
            typed(&mut p);
        }
        assert_eq!(p.flags, MAX_LINES | LINE_ENDED);
        // The seventh line's first character pushes the first out.
        p.post(1);
        p.type_one(&strings());
        assert_eq!(p.shown(), b"E\x0cE\x0cE\x0cE\x0cE\x0cA");
        assert_eq!(p.flags, MAX_LINES | REDRAW);
        assert_eq!(p.length, 11);
    }
}
