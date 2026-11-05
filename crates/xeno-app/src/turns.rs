//! How a console's three views take turns at drawing a frame (`docs/render.md`, "The
//! views take turns (T-209)"): they share `DS:0x1450`, so on each pass of the mission
//! loop at most one view's frame is drawn, the 3D view first, then the second display,
//! then the third display's chance, which starts a frame only every `chances`-th time
//! (`DS:0x62ed`) and costs one more pass to copy.

/// A console view whose frames take turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The 3D view (`DS:0x1450` = 1).
    Main,
    /// The second display (2).
    Second,
    /// The third display (3).
    Third,
}

/// The view whose frame is drawn on `pass` (0 is the console's first), the third
/// display's every `chances` (0: never).
fn turn(pass: u32, chances: u8) -> Option<Turn> {
    const ROUND: [Option<Turn>; 3] = [Some(Turn::Main), Some(Turn::Second), None];
    if chances == 0 {
        return ROUND[pass as usize % 3];
    }
    // The opener sets the count to 1, so the first chance draws; its copy takes the
    // next pass, and the 3D view starts again after it.
    match pass % (3 * u32::from(chances) + 1) {
        0 => Some(Turn::Main),
        1 => Some(Turn::Second),
        2 => Some(Turn::Third),
        3 => None,
        o => ROUND[(o as usize + 2) % 3],
    }
}

/// The pass on which `view`'s frame shown on `pass` was drawn: its latest at or before
/// `pass`, or its first if it has drawn none yet.
pub fn frame_pass(pass: u32, chances: u8, view: Turn) -> u32 {
    let first = view as u32;
    // A whole cycle back always finds one, once the first has been drawn.
    let cycle = 3 * u32::from(chances) + 2;
    (pass.saturating_sub(cycle)..=pass)
        .rev()
        .find(|&p| turn(p, chances) == Some(view))
        .unwrap_or(first)
}

/// The situation room's main view in modes M and R (`1000:5590`, `55d6`;
/// `docs/render.md`, "The situation room's frames (T-218)"): on each pass its handler
/// starts a frame when none is running (`DS:0x1450` 0) or copies the finished one (bit
/// 7), the map's only once `gap` counts of the game clock (`DS:0x374`) have gone by
/// since its last copy (`DS:0x568c`). Every frame is done in the pass it starts (the
/// fast machine), and the next starts the pass after a copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sitroom {
    /// The pass the running frame started on.
    pub running: Option<u32>,
    /// The pass the frame on screen started on.
    pub shown: Option<u32>,
    /// The game clock at the map's last copy (`DS:0x568c`).
    copied: u16,
    /// The map's gap (`xeno_formats::sitroom::SitRoom::map_frame_gap`), set with the
    /// tables when the view is drawn; 0, copying at once, before.
    pub gap: u8,
}

impl Sitroom {
    /// One pass of the handler, with the game clock as the pass begins, for the map
    /// (`map`) or mode R, which copies at once and keeps no clock.
    pub fn pass(&mut self, pass: u32, clock: u16, map: bool) {
        let Some(start) = self.running else {
            self.running = Some(pass);
            return;
        };
        if map {
            if clock.wrapping_sub(self.copied) < u16::from(self.gap) {
                return;
            }
            self.copied = clock;
        }
        (self.shown, self.running) = (Some(start), None);
    }

    /// The pass whose frame is on screen at `pass`: before the first copy, the running
    /// frame's (drawn early), or else this pass's.
    pub fn frame(&self, pass: u32) -> u32 {
        self.shown.or(self.running).unwrap_or(pass)
    }

    /// No frame running or shown, as after a view switch; the last copy's clock kept.
    pub fn restart(&mut self) {
        (self.running, self.shown) = (None, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_views_take_turns() {
        use Turn::*;
        let passes: Vec<_> = (0..14).map(|p| turn(p, 3)).collect();
        // The third display's first chance draws; its next two do not.
        let want = [
            Some(Main),
            Some(Second),
            Some(Third),
            None,
            Some(Main),
            Some(Second),
            None,
            Some(Main),
            Some(Second),
            None,
            Some(Main),
            Some(Second),
            Some(Third),
            None,
        ];
        assert_eq!(passes, want);
        assert_eq!(turn(4, 0), Some(Second));
        assert_eq!(turn(5, 0), None);
    }

    #[test]
    fn a_frame_is_shown_until_the_view_s_next() {
        assert_eq!(frame_pass(0, 3, Turn::Third), 2, "the first, early");
        assert_eq!(frame_pass(11, 3, Turn::Third), 2);
        assert_eq!(frame_pass(12, 3, Turn::Third), 12);
        assert_eq!(frame_pass(6, 3, Turn::Main), 4);
        assert_eq!(frame_pass(9, 3, Turn::Second), 8);
        assert_eq!(frame_pass(1000, 0, Turn::Main), 999);
    }

    #[test]
    fn the_map_s_frames_wait_for_the_clock() {
        // The clock at each pass's start: 60 counts to 70 passes, from 3.
        let clock = |p: u32| (3 + p * 6 / 7) as u16;
        let shown = |map| {
            let mut s = Sitroom {
                gap: 3,
                ..Sitroom::default()
            };
            (0..12)
                .map(|p| {
                    s.pass(p, clock(p), map);
                    s.frame(p)
                })
                .collect::<Vec<_>>()
        };
        // A gap of 3: copies at passes 1, 4 (3 counts after 1), 7 and 11, each of the
        // frame started the pass after the copy before.
        assert_eq!(shown(true), [0, 0, 0, 0, 2, 2, 2, 5, 5, 5, 5, 8]);
        // Mode R: a copy every other pass.
        assert_eq!(shown(false), [0, 0, 0, 2, 2, 4, 4, 6, 6, 8, 8, 10]);
        // Across the clock's wrap: 2 counts wait, 3 copy.
        let mut s = Sitroom {
            gap: 3,
            ..Sitroom::default()
        };
        for (p, c) in [(0, 0xfffe), (1, 0xffff), (2, 0), (3, 1)] {
            s.pass(p, c, true);
        }
        assert_eq!((s.shown, s.running), (Some(0), Some(2)));
        s.pass(4, 2, true);
        assert_eq!((s.shown, s.running), (Some(2), None));
        s.restart();
        assert_eq!(s.frame(7), 7);
    }
}
