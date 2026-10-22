//! The game's own input events, free of winit: keys by PC scan code, typed text, the
//! set of held keys, and a log of the events each retrace received. The sim and the
//! screens read input only through the log, so a recorded log replays exactly
//! (`docs/game-loop.md`). `translate` turns winit events into these.
//!
//! When `xeno-sim` exists (T-050), these types move there; nothing here needs a window.

/// A key, by its IBM PC scan code (set 1, as the original's input queue sees it: P is
/// 0x19). Keys sent with an `E0` prefix (the cursor block, right Ctrl and Alt, keypad
/// Enter and /) are `0xE0xx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScanCode(pub u16);

impl ScanCode {
    pub const ESCAPE: ScanCode = ScanCode(0x01);
    pub const ENTER: ScanCode = ScanCode(0x1c);
    pub const F11: ScanCode = ScanCode(0x57);
    pub const KEYPAD_ENTER: ScanCode = ScanCode(0xe01c);

    /// 0..=127 for plain keys, 128..=255 for `E0` keys.
    pub fn index(self) -> usize {
        usize::from(self.0 & 0x7f) | if self.0 >> 8 == 0xe0 { 0x80 } else { 0 }
    }

    fn from_index(i: usize) -> ScanCode {
        let code = (i & 0x7f) as u16;
        ScanCode(if i & 0x80 != 0 { 0xe000 | code } else { code })
    }
}

/// One input event, as the game sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// A key went down; `repeat` is the keyboard's auto-repeat while it is held.
    KeyDown {
        code: ScanCode,
        repeat: bool,
    },
    KeyUp {
        code: ScanCode,
    },
    /// A printable character typed (layout and shift applied), for name entry.
    Text(char),
}

/// The keys held down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keys([u64; 4]);

impl Keys {
    pub fn is_down(&self, code: ScanCode) -> bool {
        let i = code.index();
        self.0[i / 64] & (1 << (i % 64)) != 0
    }

    /// Marks `code` held; returns whether it was up.
    pub fn press(&mut self, code: ScanCode) -> bool {
        let i = code.index();
        let was_up = !self.is_down(code);
        self.0[i / 64] |= 1 << (i % 64);
        was_up
    }

    /// Marks `code` up; returns whether it was held.
    pub fn release(&mut self, code: ScanCode) -> bool {
        let i = code.index();
        let was_down = self.is_down(code);
        self.0[i / 64] &= !(1 << (i % 64));
        was_down
    }

    pub fn apply(&mut self, event: &InputEvent) {
        match *event {
            InputEvent::KeyDown { code, .. } => {
                self.press(code);
            }
            InputEvent::KeyUp { code } => {
                self.release(code);
            }
            InputEvent::Text(_) => {}
        }
    }

    /// The held keys, in scan code order (plain keys first).
    pub fn held(&self) -> impl Iterator<Item = ScanCode> + '_ {
        (0..256)
            .map(ScanCode::from_index)
            .filter(|&c| self.is_down(c))
    }
}

/// The input each retrace received, in order. Events that arrive between two retraces
/// all go to the next one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputLog {
    entries: Vec<(u64, InputEvent)>,
}

impl InputLog {
    /// Records `events` for `retrace`, which must not be before the last one recorded.
    pub fn record(&mut self, retrace: u64, events: impl IntoIterator<Item = InputEvent>) {
        let last = self.entries.last().map_or(0, |&(r, _)| r);
        assert!(retrace >= last, "retrace {retrace} recorded after {last}");
        self.entries
            .extend(events.into_iter().map(|e| (retrace, e)));
    }

    /// The events recorded for `retrace`.
    pub fn events_at(&self, retrace: u64) -> impl Iterator<Item = &InputEvent> {
        let start = self.entries.partition_point(|&(r, _)| r < retrace);
        self.entries[start..]
            .iter()
            .take_while(move |&&(r, _)| r == retrace)
            .map(|(_, e)| e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(code: u16) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat: false,
        }
    }

    fn up(code: u16) -> InputEvent {
        InputEvent::KeyUp {
            code: ScanCode(code),
        }
    }

    #[test]
    fn keys_track_plain_and_extended_codes_apart() {
        let mut keys = Keys::default();
        assert!(keys.press(ScanCode(0x48))); // keypad 8
        assert!(!keys.press(ScanCode(0x48)));
        assert!(!keys.is_down(ScanCode(0xe048))); // the up arrow
        keys.apply(&down(0xe048));
        keys.apply(&down(0x7f));
        keys.apply(&InputEvent::Text('p'));
        let held: Vec<_> = keys.held().collect();
        assert_eq!(held, [ScanCode(0x48), ScanCode(0x7f), ScanCode(0xe048)]);
        keys.apply(&up(0x48));
        assert!(!keys.release(ScanCode(0x48)));
        assert!(keys.release(ScanCode(0xe048)));
        assert_eq!(keys.held().collect::<Vec<_>>(), [ScanCode(0x7f)]);
    }

    /// A stand-in for the sim: folds each retrace's events into a state.
    fn replay(log: &InputLog, retraces: u64) -> Vec<(u64, Keys, String)> {
        let (mut keys, mut text) = (Keys::default(), String::new());
        (1..=retraces)
            .map(|r| {
                for e in log.events_at(r) {
                    keys.apply(e);
                    if let InputEvent::Text(c) = e {
                        text.push(*c);
                    }
                }
                (r, keys.clone(), text.clone())
            })
            .collect()
    }

    #[test]
    fn a_log_replays_each_retraces_events() {
        let mut log = InputLog::default();
        log.record(1, []);
        log.record(2, [down(0x19), InputEvent::Text('p')]);
        log.record(2, [up(0x19)]);
        log.record(5, [down(0x1c)]);
        assert_eq!(log.events_at(2).count(), 3);
        assert_eq!(log.events_at(3).count(), 0);
        assert_eq!(log.events_at(5).collect::<Vec<_>>(), [&down(0x1c)]);
        let first = replay(&log, 6);
        assert_eq!(first, replay(&log.clone(), 6));
        assert!(!first[1].1.is_down(ScanCode(0x19)));
        assert_eq!(first[1].2, "p");
        assert!(first[4].1.is_down(ScanCode::ENTER));
        assert!(!first[3].1.is_down(ScanCode::ENTER));
    }

    #[test]
    #[should_panic(expected = "retrace 1 recorded after 2")]
    fn a_log_only_goes_forwards() {
        let mut log = InputLog::default();
        log.record(2, [down(1)]);
        log.record(1, [down(1)]);
    }
}
