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
}
