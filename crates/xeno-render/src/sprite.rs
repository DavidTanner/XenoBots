//! The `.b@n` sprite blitters of `mgavideo.dvr` (slots `0x5c0`–`0x5ec`, see
//! `docs/formats/b@n.md`, "Drawing").

use xeno_formats::ban::Sprite;

use crate::{Framebuffer, Rect};

/// Which pixels a blit writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlitMode {
    /// Every pixel the sprite covers, colour 0 included.
    #[default]
    Opaque,
    /// Every pixel the sprite covers except those of colour 0.
    Keyed,
}

/// How the sprite is turned. It always occupies the same rectangle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Normal,
    /// Upside down: the first row is drawn at the bottom.
    FlipVertical,
    /// Left to right: each row is drawn leftwards from the right edge.
    Mirror,
    /// Both of the above.
    Rotate180,
}

/// Which point of the sprite the blit position names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Anchor {
    /// The top-left corner.
    #[default]
    TopLeft,
    /// The centre: half the width and height, rounded down, are subtracted first
    /// (the anchor mode set by slot `0x5c0`).
    Centre,
    /// The sprite's centre is placed at the surface's centre plus the position (slots
    /// `0x5d4` and `0x5e8`). The original checks the rectangle at the position itself
    /// but draws at the centred one, which can write past the surface; we check the
    /// rectangle that is drawn.
    SurfaceCentre,
}

/// The options of [`Framebuffer::blit_sprite`]. The default is opaque, upright and
/// placed by its top-left corner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpriteBlit {
    pub mode: BlitMode,
    pub orientation: Orientation,
    pub anchor: Anchor,
}

impl Framebuffer {
    /// Draws `sprite` at surface position (`x`, `y`) as the game's sprite blitters do,
    /// and returns whether it was drawn.
    ///
    /// Only the pixels the sprite covers ([`Sprite::covers`]: all of a raw sprite, the
    /// spans of a row-span one) are written; the rest are left as they were. There is
    /// no clipping: a sprite whose rectangle does not lie wholly on the surface is not
    /// drawn at all.
    ///
    /// A mirrored row span starts its skip at the right edge, so the sprite is an
    /// exact mirror image. The original's opaque mirrored blitter (slot `0x5cc`, and
    /// `0x5d0` for 180 degrees) instead moves right by the skip before writing
    /// leftwards, so a span with a skip lands too far right (by twice the skip, by our
    /// reading of `b@n.md`); its keyed blitters get it right.
    pub fn blit_sprite(&mut self, sprite: &Sprite, x: i32, y: i32, blit: SpriteBlit) -> bool {
        let (width, height) = (sprite.width(), sprite.height());
        let (half_w, half_h) = ((width / 2) as i64, (height / 2) as i64);
        let (x, y) = (i64::from(x), i64::from(y));
        let (x, y) = match blit.anchor {
            Anchor::TopLeft => (x, y),
            Anchor::Centre => (x - half_w, y - half_h),
            Anchor::SurfaceCentre => (
                (self.surface().width / 2) as i64 + x - half_w,
                (self.surface().height / 2) as i64 + y - half_h,
            ),
        };
        let Some((left, top)) = self.place(x, y, width, height) else {
            return false;
        };
        let (mirror, flip) = match blit.orientation {
            Orientation::Normal => (false, false),
            Orientation::FlipVertical => (false, true),
            Orientation::Mirror => (true, false),
            Orientation::Rotate180 => (true, true),
        };
        let stride = self.surface().width;
        let pixels = self.pixels_mut();
        for sy in 0..height {
            let dy = top + if flip { height - 1 - sy } else { sy };
            for sx in (0..width).filter(|&sx| sprite.covers(sx, sy)) {
                let colour = sprite.pixels[sy * width + sx];
                if blit.mode == BlitMode::Keyed && colour == 0 {
                    continue;
                }
                let dx = left + if mirror { width - 1 - sx } else { sx };
                pixels[dy * stride + dx] = colour;
            }
        }
        true
    }

    /// Draws the `source` rectangle of `sprite` with its top-left at surface position
    /// (`x`, `y`), skipping colour 0 (slot `0x5ec`), and returns whether it was drawn.
    ///
    /// The original reads the window straight from a raw sprite's pixels and checks
    /// nothing. Here the window must lie inside the sprite and its destination on the
    /// surface, or nothing is drawn; a row-span sprite is read from its expanded
    /// pixels, where everything outside the spans is 0 and so is skipped.
    pub fn blit_sprite_window(&mut self, sprite: &Sprite, source: Rect, x: i32, y: i32) -> bool {
        let (width, height) = (source.width as usize, source.height as usize);
        let in_sprite = source.x >= 0
            && source.y >= 0
            && source.x as usize + width <= sprite.width()
            && source.y as usize + height <= sprite.height();
        let Some((left, top)) = self
            .place(x.into(), y.into(), width, height)
            .filter(|_| in_sprite)
        else {
            return false;
        };
        let (sx0, sy0) = (source.x as usize, source.y as usize);
        let stride = self.surface().width;
        let pixels = self.pixels_mut();
        for row in 0..height {
            let from = (sy0 + row) * sprite.width() + sx0;
            let to = (top + row) * stride + left;
            for (dst, &colour) in pixels[to..to + width]
                .iter_mut()
                .zip(&sprite.pixels[from..from + width])
            {
                if colour != 0 {
                    *dst = colour;
                }
            }
        }
        true
    }

    /// The top-left of a `width` x `height` rectangle at (`x`, `y`) if it lies wholly
    /// on the surface.
    fn place(&self, x: i64, y: i64, width: usize, height: usize) -> Option<(usize, usize)> {
        let surface = self.surface();
        let fits = x >= 0
            && y >= 0
            && x as u64 + width as u64 <= surface.width as u64
            && y as u64 + height as u64 <= surface.height as u64;
        fits.then_some((x as usize, y as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;
    use proptest::prelude::*;
    use xeno_formats::ban::SpriteHeader;

    fn header(width: usize, height: usize, flags: u8) -> SpriteHeader {
        SpriteHeader {
            size: 0,
            width: width as u8,
            height: height as u8,
            flags,
            unknown_05: 8,
        }
    }

    fn raw(width: usize, height: usize, pixels: &[u8]) -> Sprite {
        assert_eq!(pixels.len(), width * height);
        Sprite {
            header: header(width, height, 0),
            pixels: pixels.to_vec(),
            spans: Vec::new(),
            trailing_empty_span: false,
        }
    }

    /// A row-span sprite: each row is a skip and the span's pixels.
    fn spans(width: usize, rows: &[(u8, &[u8])]) -> Sprite {
        let mut pixels = vec![0; width * rows.len()];
        for (y, (skip, span)) in rows.iter().enumerate() {
            let at = y * width + usize::from(*skip);
            pixels[at..at + span.len()].copy_from_slice(span);
        }
        Sprite {
            header: header(width, rows.len(), SpriteHeader::ROW_SPANS),
            pixels,
            spans: rows.iter().map(|(s, p)| (*s, p.len() as u8)).collect(),
            trailing_empty_span: false,
        }
    }

    /// The surface as text: `.` for colour 0, a hex digit for 1 to 15, `#` above.
    fn ascii(fb: &Framebuffer) -> String {
        fb.pixels()
            .chunks(fb.surface().width)
            .map(|row| {
                row.iter()
                    .map(|&p| match p {
                        0 => '.',
                        1..=15 => char::from_digit(p.into(), 16).unwrap(),
                        _ => '#',
                    })
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    fn drawn(sprite: &Sprite, size: Size, x: i32, y: i32, blit: SpriteBlit) -> String {
        let mut fb = Framebuffer::new(size);
        fb.clear(0xe);
        assert!(fb.blit_sprite(sprite, x, y, blit));
        ascii(&fb)
    }

    const KEYED: SpriteBlit = SpriteBlit {
        mode: BlitMode::Keyed,
        orientation: Orientation::Normal,
        anchor: Anchor::TopLeft,
    };

    #[test]
    fn opaque_writes_colour_0_and_keyed_skips_it() {
        let sprite = raw(3, 2, &[1, 0, 2, 3, 4, 0]);
        let opaque = drawn(&sprite, Size::new(4, 3), 1, 1, SpriteBlit::default());
        assert_eq!(opaque, "eeee\ne1.2\ne34.\n");
        assert_eq!(
            drawn(&sprite, Size::new(4, 3), 1, 1, KEYED),
            "eeee\ne1e2\ne34e\n"
        );
    }

    #[test]
    fn row_spans_leave_pixels_outside_the_span_untouched() {
        let sprite = spans(4, &[(1, &[0, 5]), (0, &[]), (3, &[6])]);
        let opaque = drawn(&sprite, Size::new(4, 3), 0, 0, SpriteBlit::default());
        assert_eq!(opaque, "e.5e\neeee\neee6\n");
        assert_eq!(
            drawn(&sprite, Size::new(4, 3), 0, 0, KEYED),
            "ee5e\neeee\neee6\n"
        );
    }

    #[test]
    fn orientations_turn_the_sprite_in_place() {
        let sprite = raw(3, 2, &[1, 2, 3, 4, 5, 6]);
        let cases = [
            (Orientation::Normal, "123\n456\n"),
            (Orientation::FlipVertical, "456\n123\n"),
            (Orientation::Mirror, "321\n654\n"),
            (Orientation::Rotate180, "654\n321\n"),
        ];
        for (orientation, expected) in cases {
            for mode in [BlitMode::Opaque, BlitMode::Keyed] {
                let blit = SpriteBlit {
                    mode,
                    orientation,
                    ..SpriteBlit::default()
                };
                assert_eq!(drawn(&sprite, Size::new(3, 2), 0, 0, blit), expected);
            }
        }
    }

    #[test]
    fn mirrored_spans_count_their_skip_from_the_right() {
        // The original opaque mirrored blitter would write columns 5 and 4 instead,
        // past the sprite's right edge (see `blit_sprite`).
        let sprite = spans(5, &[(1, &[7, 8])]);
        for mode in [BlitMode::Opaque, BlitMode::Keyed] {
            let blit = SpriteBlit {
                mode,
                orientation: Orientation::Mirror,
                ..SpriteBlit::default()
            };
            assert_eq!(drawn(&sprite, Size::new(7, 1), 0, 0, blit), "ee87eee\n");
        }
    }

    #[test]
    fn anchors_round_half_the_size_down() {
        let sprite = raw(3, 3, &[1; 9]);
        let centre = SpriteBlit {
            anchor: Anchor::Centre,
            ..SpriteBlit::default()
        };
        let mut fb = Framebuffer::new(Size::new(6, 5));
        assert!(fb.blit_sprite(&sprite, 1, 1, centre));
        assert!(!fb.blit_sprite(&sprite, 0, 1, centre));
        assert_eq!(ascii(&fb), "111...\n111...\n111...\n......\n......\n");

        let surface_centre = SpriteBlit {
            anchor: Anchor::SurfaceCentre,
            ..SpriteBlit::default()
        };
        let mut fb = Framebuffer::new(Size::new(6, 5));
        assert!(fb.blit_sprite(&raw(2, 2, &[2; 4]), 1, -1, surface_centre));
        assert_eq!(ascii(&fb), "...22.\n...22.\n......\n......\n......\n");
        assert!(!fb.blit_sprite(&raw(2, 2, &[2; 4]), 3, 0, surface_centre));
    }

    #[test]
    fn a_sprite_that_does_not_fit_is_not_drawn() {
        let sprite = raw(2, 2, &[3; 4]);
        let mut fb = Framebuffer::new(Size::new(4, 4));
        for (x, y) in [
            (-1, 0),
            (0, -1),
            (3, 0),
            (0, 3),
            (i32::MAX, 0),
            (i32::MIN, 0),
        ] {
            assert!(!fb.blit_sprite(&sprite, x, y, SpriteBlit::default()));
        }
        assert_eq!(ascii(&fb), "....\n".repeat(4));
        assert!(fb.blit_sprite(&sprite, 2, 2, SpriteBlit::default()));
        assert_eq!(fb.get(3, 3), Some(3));
    }

    #[test]
    fn windows_are_keyed_and_must_fit_both_ends() {
        let sprite = raw(4, 3, &[1, 2, 3, 4, 5, 0, 7, 8, 9, 10, 11, 12]);
        let mut fb = Framebuffer::new(Size::new(4, 3));
        fb.clear(0xe);
        assert!(fb.blit_sprite_window(&sprite, Rect::new(1, 0, 2, 2), 2, 1));
        assert_eq!(ascii(&fb), "eeee\nee23\neee7\n");
        for (source, x, y) in [
            (Rect::new(3, 0, 2, 1), 0, 0),
            (Rect::new(0, 2, 1, 2), 0, 0),
            (Rect::new(-1, 0, 1, 1), 0, 0),
            (Rect::new(0, 0, 2, 2), 3, 0),
            (Rect::new(0, 0, 2, 2), 0, -1),
        ] {
            assert!(!fb.blit_sprite_window(&sprite, source, x, y), "{source:?}");
        }
        assert_eq!(ascii(&fb), "eeee\nee23\neee7\n");
    }

    #[test]
    fn scene_snapshot() {
        let mut fb = Framebuffer::new(Size::new(24, 10));
        fb.fill_rect(Rect::new(0, 6, 24, 4), 1);
        fb.fill_rect(Rect::new(18, -2, 9, 5), 2);
        // A raw "robot" with a colour-0 visor, opaque and keyed.
        let robot = raw(3, 4, &[0, 4, 0, 4, 0, 4, 5, 5, 5, 6, 0, 6]);
        fb.blit_sprite(&robot, 1, 3, SpriteBlit::default());
        fb.blit_sprite(&robot, 5, 3, KEYED);
        // A row-span "flag", mirrored and upside down, keyed.
        let flag = spans(
            5,
            &[(0, &[7]), (0, &[7, 8, 8, 8]), (0, &[7, 8, 0, 8]), (0, &[7])],
        );
        let blit = SpriteBlit {
            mode: BlitMode::Keyed,
            orientation: Orientation::Rotate180,
            anchor: Anchor::Centre,
        };
        fb.blit_sprite(&flag, 12, 4, blit);
        // The robot's legs, keyed over the corner box through a window.
        fb.blit_sprite_window(&robot, Rect::new(0, 2, 3, 2), 20, 0);
        // Off the edge: not drawn.
        fb.blit_sprite(&robot, 22, 7, SpriteBlit::default());
        insta::assert_snapshot!(ascii(&fb));
    }

    fn any_sprite() -> impl Strategy<Value = Sprite> {
        (0usize..6, 0usize..6, any::<bool>(), any::<u64>()).prop_map(|(w, h, rows, seed)| {
            let pixels: Vec<u8> = (0..w * h).map(|i| (seed >> (i % 60)) as u8 & 3).collect();
            if !rows || w == 0 {
                return raw(w, h, &pixels);
            }
            let spans_of: Vec<(u8, Vec<u8>)> = (0..h)
                .map(|y| {
                    let skip = (seed as usize >> y) % w;
                    let count = (seed as usize >> (y + 8)) % (w - skip + 1);
                    (
                        skip as u8,
                        pixels[y * w + skip..y * w + skip + count].to_vec(),
                    )
                })
                .collect();
            let rows: Vec<(u8, &[u8])> = spans_of.iter().map(|(s, p)| (*s, &p[..])).collect();
            spans(w, &rows)
        })
    }

    /// Every combination of mode, orientation and anchor.
    fn all_blits() -> Vec<SpriteBlit> {
        let orientations = [
            Orientation::Normal,
            Orientation::FlipVertical,
            Orientation::Mirror,
            Orientation::Rotate180,
        ];
        let anchors = [Anchor::TopLeft, Anchor::Centre, Anchor::SurfaceCentre];
        let mut blits = Vec::new();
        for mode in [BlitMode::Opaque, BlitMode::Keyed] {
            for orientation in orientations {
                for anchor in anchors {
                    blits.push(SpriteBlit {
                        mode,
                        orientation,
                        anchor,
                    });
                }
            }
        }
        blits
    }

    proptest! {
        /// A blit writes only inside its rectangle, writes every covered pixel there
        /// (unless keyed), and leaves the surface alone when the rectangle is off it.
        #[test]
        fn blits_never_write_outside_their_rectangle(
            sprite in any_sprite(),
            blit in proptest::sample::select(all_blits()),
            (w, h) in (1usize..9, 1usize..9),
            (x, y) in (-8i32..12, -8i32..12),
        ) {
            let mut fb = Framebuffer::new(Size::new(w, h));
            fb.clear(0xff);
            let (sw, sh) = (sprite.width() as i32, sprite.height() as i32);
            let (left, top) = match blit.anchor {
                Anchor::TopLeft => (x, y),
                Anchor::Centre => (x - sw / 2, y - sh / 2),
                Anchor::SurfaceCentre => (w as i32 / 2 + x - sw / 2, h as i32 / 2 + y - sh / 2),
            };
            let fits = left >= 0 && top >= 0 && left + sw <= w as i32 && top + sh <= h as i32;
            prop_assert_eq!(fb.blit_sprite(&sprite, x, y, blit), fits);
            let mut covered = 0;
            for (i, &p) in fb.pixels().iter().enumerate() {
                let (px, py) = ((i % w) as i32 - left, (i / w) as i32 - top);
                if p != 0xff {
                    prop_assert!(fits && (0..sw).contains(&px) && (0..sh).contains(&py));
                    covered += 1;
                }
            }
            let expected = if !fits {
                0
            } else {
                (0..sprite.height())
                    .flat_map(|sy| (0..sprite.width()).map(move |sx| (sx, sy)))
                    .filter(|&(sx, sy)| sprite.covers(sx, sy))
                    .filter(|&(sx, sy)| {
                        blit.mode == BlitMode::Opaque || sprite.pixels[sy * sprite.width() + sx] != 0
                    })
                    .count()
            };
            prop_assert_eq!(covered, expected);
        }
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_bank_sprite_draws() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let mut sprites = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if !(name.starts_with("msprite") && name.ends_with(".b@n")) {
                continue;
            }
            let bank = xeno_formats::ban::parse_sprites(&std::fs::read(&path).unwrap()).unwrap();
            let mut fb = Framebuffer::new(Size::MODE_13H);
            for sprite in &bank.sprites {
                for blit in all_blits() {
                    let (x, y) = match blit.anchor {
                        Anchor::TopLeft => (0, 0),
                        Anchor::Centre => (160, 100),
                        Anchor::SurfaceCentre => (0, 0),
                    };
                    assert!(fb.blit_sprite(sprite, x, y, blit), "{name}");
                }
                sprites += 1;
            }
        }
        assert_eq!(sprites, 390, "a stock install has 390 bank sprites");
    }
}
