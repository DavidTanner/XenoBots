//! Turns winit window events into the game's input events (`input`) and the window's
//! own actions: quit, the fullscreen toggle and the game speed. Only this module and `window` see winit.
//!
//! The mouse is mapped from window pixels to view pixels through the presented
//! layout ([`Translator::set_layout`], `xeno_render::Layout::view_point`): moves are
//! clamped to the view, presses count only on the picture, and releases always pass so
//! no button sticks.
//!
//! Keys are mapped by physical position to IBM PC scan codes (set 1), the codes the
//! original read from the keyboard. The table follows Microsoft's "Keyboard Scan Code
//! Specification" (rev. 1.3a, 2000), appendix C, from the USB HID usages that winit's
//! `KeyCode` names. Pause, Print Screen and the Windows keys are left out: their codes
//! are multi-byte sequences or later than the game.

use winit::event::{ElementState, MouseButton as WinitButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use xeno_render::{Layout, Size};

use crate::input::{InputEvent, Keys, Mouse, MouseButton, ScanCode};

/// The arrows and the keypad's arrows, with and without `E0`.
const ARROWS: [u16; 8] = [0x48, 0x4b, 0x4d, 0x50, 0xe048, 0xe04b, 0xe04d, 0xe050];
/// A held arrow's first repeat on a console after 35 retraces (500 ms at 70 Hz), then
/// every 7 (10 a second): about an AT keyboard's typematic default (500 ms, 10.9 a
/// second), which the original's INT 9 handler sees as repeated make codes.
pub const ARROW_DELAY: u32 = 35;
pub const ARROW_EVERY: u32 = 7;

/// What the window should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Quit,
    ToggleFullscreen,
    LeaveFullscreen,
    /// F12: the next game speed (T-215). The original binds plain F12 nowhere.
    CycleSpeed,
}

/// Collects the game's input events between retraces, and decides the window keys.
#[derive(Debug, Default)]
pub struct Translator {
    /// Whether Alt (Option on macOS) is held.
    alt: bool,
    /// The keys the game has seen go down and not up.
    held: Keys,
    pending: Vec<InputEvent>,
    /// Set by a screen that uses ESC itself (T-040). Until then ESC belongs to the
    /// window: it leaves fullscreen, or quits from a window, and the game never sees it.
    pub screen_claims_escape: bool,
    /// Set while a robot console takes keys (T-207): held arrows then repeat by the
    /// port's own count ([`ARROW_DELAY`], [`ARROW_EVERY`]), not the system's.
    pub console_arrows: bool,
    /// The arrows held, and the retraces each has been held.
    arrows: Vec<(ScanCode, u32)>,
    /// The layout last presented and its view, to map the cursor.
    layout: Option<(Layout, Size)>,
    /// The cursor in window pixels, while it is over the window.
    cursor: Option<(f64, f64)>,
    /// Whether the cursor is on the picture.
    inside: bool,
    /// The pointer and buttons the game has been sent.
    mouse: Mouse,
}

impl Translator {
    /// Handles one window event; the caller acts on the returned action.
    pub fn window_event(&mut self, event: &WindowEvent, fullscreen: bool) -> Option<WindowAction> {
        match event {
            WindowEvent::CloseRequested => Some(WindowAction::Quit),
            // Keys released while another window has focus never arrive; release them
            // now so the game doesn't see a stuck key.
            WindowEvent::Focused(false) => {
                self.release_all();
                None
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.alt = modifiers.state().alt_key();
                None
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
                self.map_cursor();
                None
            }
            WindowEvent::CursorLeft { .. } => {
                (self.cursor, self.inside) = (None, false);
                None
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    WinitButton::Left => MouseButton::Left,
                    WinitButton::Right => MouseButton::Right,
                    WinitButton::Middle => MouseButton::Middle,
                    _ => return None,
                };
                self.button(button, *state == ElementState::Pressed);
                None
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let code = match event.physical_key {
                    PhysicalKey::Code(code) => scan_code(code),
                    PhysicalKey::Unidentified(_) => None,
                };
                let text = event.text.as_deref();
                self.key(
                    code,
                    event.state.is_pressed(),
                    event.repeat,
                    text,
                    fullscreen,
                )
            }
            _ => None,
        }
    }

    /// A key event: `code` is `None` for a key with no scan code, `text` what it typed.
    fn key(
        &mut self,
        code: Option<ScanCode>,
        pressed: bool,
        repeat: bool,
        text: Option<&str>,
        fullscreen: bool,
    ) -> Option<WindowAction> {
        let typed = text.unwrap_or_default().chars().filter(|c| !c.is_control());
        let typed = typed.map(InputEvent::Text);
        let Some(code) = code else {
            if pressed {
                self.pending.extend(typed);
            }
            return None;
        };
        let window_key = match code {
            ScanCode::F11 => Some(WindowAction::ToggleFullscreen),
            ScanCode::F12 => Some(WindowAction::CycleSpeed),
            ScanCode::ENTER | ScanCode::KEYPAD_ENTER if self.alt => {
                Some(WindowAction::ToggleFullscreen)
            }
            ScanCode::ESCAPE if !self.screen_claims_escape => Some(if fullscreen {
                WindowAction::LeaveFullscreen
            } else {
                WindowAction::Quit
            }),
            _ => None,
        };
        // A window key is not passed on (its release finds it not held), and a held one
        // doesn't repeat its action. A key the game already holds stays the game's.
        if let Some(action) = window_key
            && pressed
            && !self.held.is_down(code)
        {
            return (!repeat).then_some(action);
        }
        // Not faithful (T-207): the keyboard repeats only the last key pressed, so in the
        // original Space or any key stopped a held arrow's turn. The port repeats each
        // held arrow itself (in `take`) and drops the system's repeat for it.
        let arrow = ARROWS.contains(&code.0);
        if arrow && pressed && repeat && self.console_arrows && self.held.is_down(code) {
            return None;
        }
        if arrow && pressed && !self.held.is_down(code) {
            self.arrows.push((code, 0));
        } else if arrow && !pressed {
            self.arrows.retain(|&(c, _)| c != code);
        }
        if pressed {
            let was_up = self.held.press(code);
            self.pending.push(InputEvent::KeyDown {
                code,
                repeat: repeat && !was_up,
            });
            self.pending.extend(typed);
        } else if self.held.release(code) {
            self.pending.push(InputEvent::KeyUp { code });
        }
        None
    }

    /// The layout the picture was last presented with (window pixels) and its view. A
    /// change moves the pointer if the cursor is still.
    pub fn set_layout(&mut self, layout: Layout, view: Size) {
        if self.layout != Some((layout, view)) {
            self.layout = Some((layout, view));
            self.map_cursor();
        }
    }

    /// Whether the cursor is over the picture (for hiding the system cursor there).
    pub fn cursor_on_picture(&self) -> bool {
        self.inside
    }

    /// Sends the pointer's view pixel if it moved.
    fn map_cursor(&mut self) {
        let (Some((layout, view)), Some((x, y))) = (self.layout, self.cursor) else {
            return;
        };
        let p = layout.view_point(view, x, y);
        self.inside = p.inside;
        let at = Some((p.x as u16, p.y as u16));
        if at != self.mouse.position {
            let (x, y) = (p.x as u16, p.y as u16);
            let event = InputEvent::MouseMove { x, y };
            self.mouse.apply(&event);
            self.pending.push(event);
        }
    }

    /// A press counts only on the picture, once the pointer is known; a release only
    /// for a button the game saw go down.
    fn button(&mut self, button: MouseButton, pressed: bool) {
        let send = if pressed {
            self.inside && self.mouse.position.is_some()
        } else {
            self.mouse.is_down(button)
        };
        if send && self.mouse.set(button, pressed) {
            self.pending.push(if pressed {
                InputEvent::MouseDown { button }
            } else {
                InputEvent::MouseUp { button }
            });
        }
    }

    fn release_all(&mut self) {
        let held: Vec<_> = self.held.held().collect();
        self.pending
            .extend(held.into_iter().map(|code| InputEvent::KeyUp { code }));
        let buttons: Vec<_> = self.mouse.held().collect();
        for button in buttons {
            self.button(button, false);
        }
        self.held = Keys::default();
        self.arrows.clear();
        self.alt = false;
    }

    /// The events since the last call, for the next retrace; called once a retrace.
    /// On a console a held arrow's own repeats come last.
    pub fn take(&mut self) -> std::vec::Drain<'_, InputEvent> {
        for (code, held) in &mut self.arrows {
            let due = *held >= ARROW_DELAY && (*held - ARROW_DELAY).is_multiple_of(ARROW_EVERY);
            if due && self.console_arrows {
                let event = InputEvent::KeyDown {
                    code: *code,
                    repeat: true,
                };
                self.pending.push(event);
            }
            *held += 1;
        }
        self.pending.drain(..)
    }
}

/// The set 1 scan code of a physical key.
pub fn scan_code(key: KeyCode) -> Option<ScanCode> {
    use KeyCode::*;
    // Runs of consecutive codes: the first code, then the keys.
    const RUNS: [(u16, &[KeyCode]); 9] = [
        (
            0x01,
            &[
                Escape,
                Digit1,
                Digit2,
                Digit3,
                Digit4,
                Digit5,
                Digit6,
                Digit7,
                Digit8,
                Digit9,
                Digit0,
                Minus,
                Equal,
                Backspace,
                Tab,
                KeyQ,
                KeyW,
                KeyE,
                KeyR,
                KeyT,
                KeyY,
                KeyU,
                KeyI,
                KeyO,
                KeyP,
                BracketLeft,
                BracketRight,
                Enter,
                ControlLeft,
                KeyA,
                KeyS,
                KeyD,
                KeyF,
                KeyG,
                KeyH,
                KeyJ,
                KeyK,
                KeyL,
                Semicolon,
                Quote,
                Backquote,
                ShiftLeft,
                Backslash,
                KeyZ,
                KeyX,
                KeyC,
                KeyV,
                KeyB,
                KeyN,
                KeyM,
                Comma,
                Period,
                Slash,
                ShiftRight,
                NumpadMultiply,
                AltLeft,
                Space,
                CapsLock,
                F1,
                F2,
                F3,
                F4,
                F5,
                F6,
                F7,
                F8,
                F9,
                F10,
                NumLock,
                ScrollLock,
                Numpad7,
                Numpad8,
                Numpad9,
                NumpadSubtract,
                Numpad4,
                Numpad5,
                Numpad6,
                NumpadAdd,
                Numpad1,
                Numpad2,
                Numpad3,
                Numpad0,
                NumpadDecimal,
            ],
        ),
        // 0x54 and 0x55 are not keys on a PC keyboard.
        (0x56, &[IntlBackslash, F11, F12]),
        (0xe01c, &[NumpadEnter, ControlRight]),
        (0xe035, &[NumpadDivide]),
        (0xe038, &[AltRight]),
        (0xe047, &[Home, ArrowUp, PageUp]),
        (0xe04b, &[ArrowLeft]),
        (0xe04d, &[ArrowRight]),
        (0xe04f, &[End, ArrowDown, PageDown, Insert, Delete]),
    ];
    RUNS.iter().find_map(|&(first, keys)| {
        let i = keys.iter().position(|&k| k == key)?;
        Some(ScanCode(first + i as u16))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::ModifiersState;

    fn sc(code: u16) -> Option<ScanCode> {
        Some(ScanCode(code))
    }

    #[test]
    fn keys_map_to_set_1_scan_codes() {
        use KeyCode::*;
        for (key, code) in [
            (Escape, 0x01),
            (Digit1, 0x02),
            (Digit0, 0x0b),
            (Backspace, 0x0e),
            (KeyQ, 0x10),
            (KeyP, 0x19),
            (Enter, 0x1c),
            (ControlLeft, 0x1d),
            (KeyA, 0x1e),
            (KeyL, 0x26),
            (Backquote, 0x29),
            (ShiftLeft, 0x2a),
            (KeyZ, 0x2c),
            (KeyM, 0x32),
            (Slash, 0x35),
            (ShiftRight, 0x36),
            (AltLeft, 0x38),
            (Space, 0x39),
            (F1, 0x3b),
            (F10, 0x44),
            (Numpad7, 0x47),
            (NumpadSubtract, 0x4a),
            (NumpadDecimal, 0x53),
            (IntlBackslash, 0x56),
            (F11, 0x57),
            (F12, 0x58),
            (NumpadEnter, 0xe01c),
            (ControlRight, 0xe01d),
            (NumpadDivide, 0xe035),
            (AltRight, 0xe038),
            (Home, 0xe047),
            (ArrowUp, 0xe048),
            (PageUp, 0xe049),
            (ArrowLeft, 0xe04b),
            (ArrowRight, 0xe04d),
            (End, 0xe04f),
            (ArrowDown, 0xe050),
            (PageDown, 0xe051),
            (Insert, 0xe052),
            (Delete, 0xe053),
        ] {
            assert_eq!(scan_code(key), sc(code), "{key:?}");
        }
        assert_eq!(scan_code(Pause), None);
        assert_eq!(scan_code(SuperLeft), None);
    }

    fn events(t: &mut Translator) -> Vec<InputEvent> {
        t.take().collect()
    }

    fn down(code: u16, repeat: bool) -> InputEvent {
        InputEvent::KeyDown {
            code: ScanCode(code),
            repeat,
        }
    }

    #[test]
    fn presses_releases_repeats_and_text_reach_the_game() {
        let mut t = Translator::default();
        assert_eq!(t.key(sc(0x19), true, false, Some("p"), false), None);
        assert_eq!(t.key(sc(0x19), true, true, Some("p"), false), None);
        t.key(sc(0x19), false, false, None, false);
        t.key(sc(0x19), false, false, None, false); // not held: no second release
        t.key(None, true, false, Some("é"), false); // no scan code, but typed text
        t.key(sc(0x0e), true, false, Some("\u{8}"), false); // Backspace types nothing
        assert_eq!(
            events(&mut t),
            [
                down(0x19, false),
                InputEvent::Text('p'),
                down(0x19, true),
                InputEvent::Text('p'),
                InputEvent::KeyUp {
                    code: ScanCode(0x19)
                },
                InputEvent::Text('é'),
                down(0x0e, false),
            ]
        );
        assert_eq!(events(&mut t), []);
    }

    #[test]
    fn f11_and_alt_enter_toggle_fullscreen_without_reaching_the_game() {
        let mut t = Translator::default();
        let toggle = Some(WindowAction::ToggleFullscreen);
        assert_eq!(t.key(sc(0x57), true, false, None, false), toggle);
        assert_eq!(t.key(sc(0x57), true, true, None, true), None);
        assert_eq!(t.key(sc(0x57), false, false, None, true), None);
        // F12, the game speed (T-215), is the window's too.
        let speed = Some(WindowAction::CycleSpeed);
        assert_eq!(t.key(sc(0x58), true, false, None, false), speed);
        assert_eq!(t.key(sc(0x58), true, true, None, false), None);
        assert_eq!(t.key(sc(0x58), false, false, None, false), None);
        let alt = |t: &mut Translator, held| {
            let state = if held {
                ModifiersState::ALT
            } else {
                ModifiersState::empty()
            };
            t.window_event(&WindowEvent::ModifiersChanged(state.into()), false)
        };
        alt(&mut t, true);
        assert_eq!(t.key(sc(0x1c), true, false, Some("\r"), false), toggle);
        assert_eq!(t.key(sc(0xe01c), true, false, None, true), toggle);
        t.key(sc(0x1c), false, false, None, true);
        t.key(sc(0xe01c), false, false, None, true);
        assert_eq!(events(&mut t), []);
        alt(&mut t, false);
        assert_eq!(t.key(sc(0x1c), true, false, Some("\r"), false), None);
        assert_eq!(events(&mut t), [down(0x1c, false)]);
        // Enter already held when Alt goes down stays a game key.
        alt(&mut t, true);
        assert_eq!(t.key(sc(0x1c), true, true, None, false), None);
        assert_eq!(events(&mut t), [down(0x1c, true)]);
    }

    #[test]
    fn escape_leaves_fullscreen_then_quits_unless_a_screen_claims_it() {
        let mut t = Translator::default();
        let esc = |t: &mut Translator, fullscreen| t.key(sc(0x01), true, false, None, fullscreen);
        assert_eq!(esc(&mut t, true), Some(WindowAction::LeaveFullscreen));
        assert_eq!(esc(&mut t, false), Some(WindowAction::Quit));
        assert_eq!(t.key(sc(0x01), true, true, None, false), None);
        t.key(sc(0x01), false, false, None, false);
        assert_eq!(events(&mut t), []);
        t.screen_claims_escape = true;
        assert_eq!(esc(&mut t, true), None);
        assert_eq!(events(&mut t), [down(0x01, false)]);
    }

    #[test]
    fn the_cursor_maps_to_view_pixels_and_buttons_count_on_the_picture() {
        use winit::dpi::PhysicalPosition;
        use winit::event::DeviceId;
        use xeno_render::{Aspect, Scale};
        let id = DeviceId::dummy();
        let moved = |t: &mut Translator, x, y| {
            let position = PhysicalPosition::new(x, y);
            t.window_event(
                &WindowEvent::CursorMoved {
                    device_id: id,
                    position,
                },
                true,
            )
        };
        let click = |t: &mut Translator, pressed| {
            let state = if pressed {
                ElementState::Pressed
            } else {
                ElementState::Released
            };
            let button = WinitButton::Left;
            let event = WindowEvent::MouseInput {
                device_id: id,
                state,
                button,
            };
            t.window_event(&event, true)
        };
        let left = MouseButton::Left;
        let mut t = Translator::default();
        // No layout yet, so no pointer, and no press.
        moved(&mut t, 330.0, 50.0);
        click(&mut t, true);
        assert_eq!(events(&mut t), []);
        // 4x5 at (320, 40) in 1920x1080, as in fullscreen with the cursor hidden.
        let view = Size::MODE_13H;
        let layout = Layout::new(view, Size::new(1920, 1080), Scale::Auto, Aspect::Correct);
        t.set_layout(layout, view);
        assert_eq!(events(&mut t), [InputEvent::MouseMove { x: 2, y: 2 }]);
        moved(&mut t, 331.0, 51.0); // the same view pixel: nothing
        click(&mut t, true);
        click(&mut t, true); // already down
        moved(&mut t, 10.0, 2000.0); // dragged off: clamped, and the release passes
        click(&mut t, false);
        click(&mut t, true); // on the bar: dropped
        click(&mut t, false);
        assert_eq!(
            events(&mut t),
            [
                InputEvent::MouseDown { button: left },
                InputEvent::MouseMove { x: 0, y: 199 },
                InputEvent::MouseUp { button: left },
            ]
        );
        // A resize under a still cursor moves the pointer; leaving keeps it.
        moved(&mut t, 400.0, 300.0);
        t.set_layout(
            Layout::new(view, Size::new(800, 600), Scale::Fixed(5), Aspect::Correct),
            view,
        );
        assert!(t.cursor_on_picture());
        t.window_event(&WindowEvent::CursorLeft { device_id: id }, true);
        assert!(!t.cursor_on_picture());
        assert_eq!(
            events(&mut t),
            [
                InputEvent::MouseMove { x: 20, y: 52 },
                InputEvent::MouseMove { x: 160, y: 100 },
            ]
        );
        // Losing focus releases a held button.
        moved(&mut t, 400.0, 300.0);
        click(&mut t, true);
        t.window_event(&WindowEvent::Focused(false), true);
        assert_eq!(
            events(&mut t),
            [
                InputEvent::MouseDown { button: left },
                InputEvent::MouseUp { button: left },
            ]
        );
    }

    #[test]
    fn close_quits_and_losing_focus_releases_held_keys() {
        let mut t = Translator::default();
        assert_eq!(
            t.window_event(&WindowEvent::CloseRequested, false),
            Some(WindowAction::Quit)
        );
        t.key(sc(0x1e), true, false, Some("a"), false);
        t.key(sc(0xe048), true, false, None, false);
        events(&mut t);
        assert_eq!(t.window_event(&WindowEvent::Focused(false), false), None);
        let up = |code| InputEvent::KeyUp {
            code: ScanCode(code),
        };
        assert_eq!(events(&mut t), [up(0x1e), up(0xe048)]);
        t.key(sc(0x1e), false, false, None, false);
        assert_eq!(events(&mut t), []);
    }

    /// The retraces (of `n`) at which `code` went down, with `at(r)` run before each.
    fn presses_of(
        t: &mut Translator,
        code: u16,
        n: u32,
        mut at: impl FnMut(&mut Translator, u32),
    ) -> Vec<u32> {
        (0..n)
            .filter(|&r| {
                at(t, r);
                let events = events(t);
                events
                    .iter()
                    .any(|e| matches!(e, InputEvent::KeyDown { code: c, .. } if c.0 == code))
            })
            .collect()
    }

    #[test]
    fn a_held_arrow_on_a_console_repeats_by_its_own_count_whatever_else_is_pressed() {
        let left = 0xe04b;
        // The system's repeats come every retrace; the console drops them.
        let os = |t: &mut Translator, r: u32| {
            t.key(sc(left), true, r > 0, None, false);
        };
        let mut t = Translator {
            console_arrows: true,
            ..Translator::default()
        };
        let alone = presses_of(&mut t, left, 60, os);
        assert_eq!(alone, [0, 35, 42, 49, 56]);
        // Space pressed, held and released, and A tapped, change nothing.
        let mut t = Translator {
            console_arrows: true,
            ..Translator::default()
        };
        let busy = presses_of(&mut t, left, 60, |t, r| {
            if r == 0 {
                os(t, 0);
            }
            match r {
                10 | 40 => t.key(sc(0x39), true, false, Some(" "), false),
                11..=39 => t.key(sc(0x39), true, true, Some(" "), false),
                45 => t.key(sc(0x39), false, false, None, false),
                47 => t.key(sc(0x1e), true, false, Some("a"), false),
                48 => t.key(sc(0x1e), false, false, None, false),
                _ => None,
            };
        });
        assert_eq!(busy, alone);
        // Releasing stops it; so does losing focus, for the keypad's arrow too.
        t.key(sc(left), false, false, None, false);
        assert_eq!(presses_of(&mut t, left, 60, |_, _| {}), []);
        t.key(sc(0x4d), true, false, None, false);
        t.window_event(&WindowEvent::Focused(false), false);
        events(&mut t);
        assert_eq!(presses_of(&mut t, 0x4d, 60, |_, _| {}), []);
    }

    #[test]
    fn arrows_elsewhere_keep_the_systems_repeat() {
        let mut t = Translator::default();
        let os = |t: &mut Translator, r: u32| {
            t.key(sc(0xe048), true, r > 0, None, false);
        };
        assert_eq!(
            presses_of(&mut t, 0xe048, 40, os),
            (0..40).collect::<Vec<_>>()
        );
        let mut t = Translator::default();
        t.key(sc(0xe048), true, false, None, false);
        assert_eq!(presses_of(&mut t, 0xe048, 60, |_, _| {}), [0]);
    }
}
