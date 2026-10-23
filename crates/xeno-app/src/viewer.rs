//! `--view <file>`: one decoded asset shown through the real framebuffer and scaler, a
//! visual test of the parsers (T-035). Sprite banks are paged one sprite at a time;
//! pictures wider or taller than the view are panned over a larger surface, as the
//! hardware-scrolled screens are (`docs/formats/video-modes.md`).
//!
//! The palette goes through `xeno_render::Dac` as the original driver's does, so the
//! viewer can fade it out and in and rotate a range of colours with a synthetic
//! colour-animation record (T-037; `docs/formats/palette.md`).
//!
//! The state and its key handling are plain data, tested without a window; `window`
//! steps it once per retrace with that retrace's input.

use std::path::Path;

use xeno_formats::ban::{self, Sprite};
use xeno_formats::{FormatError, Palette, mal, pcx, vcl};
use xeno_render::anim::{self, flag};
use xeno_render::{Anchor, BlitMode, DAC_BYTES, Dac, Fade, Framebuffer, Rect, Size, SpriteBlit};

use crate::game_dir::GameFiles;
use crate::input::{InputEvent, Keys, ScanCode};

/// The kinds `--view` shows, for error messages.
pub const SUPPORTED: &str = "sprite banks and sky strips (.b@n), screens (.m@l), cutscene pictures (.vcl) and \
     backdrops (.pcx)";

/// What a file is, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `msprite0`..`msprite5.b@n`: one sprite at a time, centred.
    Sprites,
    /// `sky_a`..`sky_h.b@n`: one 342x54 strip.
    Sky,
    /// `.m@l`: a 320x200 screen.
    Screen,
    /// `.vcl`: a picture 640 wide, shown in the 320x240 mode.
    Cutscene,
    /// `.pcx`: a backdrop 768 wide.
    Backdrop,
}

impl Kind {
    /// The kind of the file `name` (any case), by its extension.
    pub fn of(name: &str) -> Result<Kind, String> {
        let lower = name.to_lowercase();
        let (stem, extension) = lower.rsplit_once('.').unwrap_or((&lower, ""));
        Ok(match extension {
            "b@n" if stem.starts_with("sky") => Kind::Sky,
            "b@n" => Kind::Sprites,
            "m@l" => Kind::Screen,
            "vcl" => Kind::Cutscene,
            "pcx" => Kind::Backdrop,
            _ => return Err(format!("cannot view {name}: the viewer shows {SUPPORTED}")),
        })
    }

    /// The view it is shown in: `.vcl` pictures in the 320x240 mode, the rest in 13h.
    pub fn view(self) -> Size {
        match self {
            Kind::Cutscene => Size::MODE_240,
            _ => Size::MODE_13H,
        }
    }
}

/// Where the palette comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteChoice<'a> {
    /// `--palette`.
    Given(&'a Path),
    /// The file's own.
    Own,
    /// A stock screen's, for files that carry none (`docs/formats/palette.md`).
    Stock(&'static str),
}

/// The palette for file `name` of `kind`. `--palette` wins. Banks get the screen they
/// are loaded with (`b@n.md`, "Sprite numbers"); `msprite0`, loaded at start-up, gets
/// the situation room's, which shares most colours with the cockpits (a guess). Sky
/// strips get the Humanoid cockpit's, which holds the 3D view they are drawn in.
pub fn palette_choice<'a>(kind: Kind, name: &str, given: Option<&'a Path>) -> PaletteChoice<'a> {
    if let Some(path) = given {
        return PaletteChoice::Given(path);
    }
    let lower = name.to_lowercase();
    let digit = lower.strip_prefix("msprite").and_then(|r| r.chars().next());
    match kind {
        Kind::Sprites => PaletteChoice::Stock(match digit {
            Some('2') => "humancon.m@l",
            Some('3') => "scorpcon.m@l",
            Some('4') => "scoutcon.m@l",
            Some('5') => "maproom.m@l",
            _ => "sitroom.m@l",
        }),
        Kind::Sky => PaletteChoice::Stock("humancon.m@l"),
        Kind::Screen | Kind::Cutscene | Kind::Backdrop => PaletteChoice::Own,
    }
}

/// A decoded file: sprites, or one picture (`width * height` indices, row by row).
#[derive(Debug, Clone)]
pub enum Content {
    Sprites(Vec<Sprite>),
    Picture {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}

/// Decodes `bytes` as `kind` with the existing parsers, and the palette it carries.
pub fn decode(kind: Kind, bytes: &[u8]) -> Result<(Content, Option<Palette>), FormatError> {
    let picture = |width, height, pixels| Content::Picture {
        width,
        height,
        pixels,
    };
    Ok(match kind {
        Kind::Sprites => (Content::Sprites(ban::parse_sprites(bytes)?.sprites), None),
        Kind::Sky => {
            let sky = ban::parse_sky(bytes)?;
            (picture(ban::Sky::WIDTH, ban::Sky::HEIGHT, sky.pixels), None)
        }
        Kind::Screen => {
            let s = mal::parse(bytes)?;
            let (w, h) = (mal::Screen::WIDTH, mal::Screen::HEIGHT);
            (picture(w, h, s.pixels), Some(s.palette))
        }
        Kind::Cutscene => {
            let p = vcl::parse(bytes)?;
            (
                picture(vcl::Picture::WIDTH, p.height, p.pixels),
                Some(p.palette),
            )
        }
        Kind::Backdrop => {
            let p = pcx::parse(bytes)?;
            (picture(p.width(), p.height(), p.pixels), Some(p.palette))
        }
    })
}

/// The palette of file `name` for `--palette`: only screens, cutscenes and backdrops.
pub fn palette_of(name: &str, bytes: &[u8]) -> Result<Palette, String> {
    let kind = Kind::of(name)?;
    match decode(kind, bytes) {
        Ok((_, Some(palette))) => Ok(palette),
        Ok((_, None)) => Err(format!(
            "{name} has no palette; --palette takes a .m@l, .vcl or .pcx file"
        )),
        Err(e) => Err(format!("{name}: {e}")),
    }
}

/// Reads `spec`: a bare name is looked up in the game directory (any case); anything
/// else, or a name not there, is read as a path. Returns the lowercased file name too.
fn read(files: &GameFiles, spec: &Path) -> Result<(String, Vec<u8>), String> {
    let name = spec
        .file_name()
        .ok_or_else(|| format!("{} names no file", spec.display()))?
        .to_string_lossy()
        .to_lowercase();
    let bare = spec.parent().is_none_or(|p| p.as_os_str().is_empty());
    let path = files.path(&name).filter(|_| bare).unwrap_or(spec);
    let bytes = std::fs::read(path).map_err(|e| {
        format!(
            "cannot read {} (looked in the game directory, then as a path): {e}",
            spec.display()
        )
    })?;
    Ok((name, bytes))
}

/// Loads `--view <view>` with `--palette <palette>`.
pub fn load(files: &GameFiles, view: &Path, palette: Option<&Path>) -> Result<Viewer, String> {
    let (name, bytes) = read(files, view)?;
    let kind = Kind::of(&name)?;
    let (content, own) = decode(kind, &bytes).map_err(|e| format!("{name}: {e}"))?;
    let (palette, source) = match palette_choice(kind, &name, palette) {
        PaletteChoice::Given(path) => {
            let (palette_name, bytes) = read(files, path)?;
            (palette_of(&palette_name, &bytes)?, palette_name)
        }
        PaletteChoice::Own => (own.expect("files with their own palette"), name.clone()),
        PaletteChoice::Stock(stock) => {
            let bytes = files.read(stock).map_err(|e| e.to_string())?;
            (palette_of(stock, &bytes)?, stock.to_string())
        }
    };
    let viewer = Viewer::new(name, kind, content, palette);
    log::info!(
        "viewing {} ({kind:?}): a {:?} view on a {:?} surface, palette from {source}",
        viewer.name,
        viewer.view,
        viewer.surface()
    );
    Ok(viewer)
}

const LEFT: ScanCode = ScanCode(0xe04b);
const RIGHT: ScanCode = ScanCode(0xe04d);
const UP: ScanCode = ScanCode(0xe048);
const DOWN: ScanCode = ScanCode(0xe050);
const PAGE_UP: ScanCode = ScanCode(0xe049);
const PAGE_DOWN: ScanCode = ScanCode(0xe051);
const HOME: ScanCode = ScanCode(0xe047);
const END: ScanCode = ScanCode(0xe04f);
const SPACE: ScanCode = ScanCode(0x39);
const F: ScanCode = ScanCode(0x21);
const I: ScanCode = ScanCode(0x17);
const R: ScanCode = ScanCode(0x13);

/// Retraces of the F and I fades (the intro's abort fade).
const FADE: u16 = 32;
/// The colours R rotates, a step every [`ROTATE_SPEED`] retraces. The range is the
/// viewer's choice; the colours are the file's own.
const ROTATE: std::ops::RangeInclusive<u8> = 16..=31;
const ROTATE_SPEED: u8 = 4;

/// Side of a background square behind sprites.
const CHECKER: usize = 8;

/// What is shown, and where.
#[derive(Debug, Clone)]
pub struct Viewer {
    name: String,
    content: Content,
    pub palette: Palette,
    view: Size,
    /// Sprites: the one shown, and whether colour 0 is see-through (else opaque).
    index: usize,
    keyed: bool,
    /// Pictures: the view's top-left on the surface.
    origin: (usize, usize),
    /// The palette as shown: `palette` in DAC levels, faded or rotated.
    dac: Dac,
    /// A fade in waiting for its black to be uploaded.
    fade_in: Option<Fade>,
}

impl Viewer {
    pub fn new(name: String, kind: Kind, content: Content, palette: Palette) -> Viewer {
        Viewer {
            name,
            content,
            view: kind.view(),
            index: 0,
            keyed: false,
            origin: (0, 0),
            dac: Dac::with_palette(&to_vga6(&palette)),
            fade_in: None,
            palette,
        }
    }

    /// What the window shows: the palette after fades and rotation.
    pub fn shown_palette(&self) -> Palette {
        self.dac.palette()
    }

    /// The view, or the picture if it is wider or taller.
    pub fn surface(&self) -> Size {
        match &self.content {
            Content::Picture { width, height, .. } => Size::new(
                (*width).max(self.view.width),
                (*height).max(self.view.height),
            ),
            Content::Sprites(_) => self.view,
        }
    }

    /// The furthest the view's top-left can go.
    fn max_origin(&self) -> (usize, usize) {
        let surface = self.surface();
        (
            surface.width - self.view.width,
            surface.height - self.view.height,
        )
    }

    /// A framebuffer of the right size, drawn.
    pub fn framebuffer(&self) -> Framebuffer {
        let mut fb = Framebuffer::with_surface(self.view, self.surface())
            .expect("the surface holds the view");
        self.draw(&mut fb);
        fb
    }

    /// One retrace: `events` are its input, `keys` the keys held after it. Paging keys
    /// act on every press (auto-repeat included); a held or tapped arrow pans one
    /// pixel. Returns whether anything changed.
    pub fn step<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a InputEvent>,
        keys: &Keys,
    ) -> bool {
        self.dac.retrace();
        if !self.dac.dirty()
            && let Some(fade) = self.fade_in.take()
        {
            self.dac.start(fade);
        }
        let before = (self.index, self.keyed, self.origin);
        let mut pressed = Keys::default();
        for event in events {
            if let InputEvent::KeyDown { code, .. } = *event {
                pressed.press(code);
                self.key(code);
            }
        }
        if matches!(self.content, Content::Picture { .. }) {
            let on = |code| isize::from(keys.is_down(code) || pressed.is_down(code));
            let (max_x, max_y) = self.max_origin();
            let pan = |at: usize, by: isize, max: usize| at.saturating_add_signed(by).min(max);
            self.origin = (
                pan(self.origin.0, on(RIGHT) - on(LEFT), max_x),
                pan(self.origin.1, on(DOWN) - on(UP), max_y),
            );
        }
        before != (self.index, self.keyed, self.origin)
    }

    fn key(&mut self, code: ScanCode) {
        match code {
            F => {
                // Slot 0x4c0: drop a pending upload, then fade what the DAC shows.
                self.fade_in = None;
                self.dac.stop();
                self.dac.fade_out(0, 255, FADE, u16::MAX);
            }
            I => {
                let target = to_vga6(&self.palette);
                self.fade_in = Some(self.dac.prepare_fade_in(&target, 0, 255, FADE));
            }
            R => self.toggle_rotation(),
            _ => {}
        }
        let last = match &self.content {
            Content::Sprites(sprites) => sprites.len().saturating_sub(1),
            Content::Picture { .. } => {
                let max_x = self.max_origin().0;
                match code {
                    HOME => self.origin.0 = 0,
                    END => self.origin.0 = max_x,
                    _ => {}
                }
                return;
            }
        };
        match code {
            RIGHT | PAGE_DOWN => self.index = (self.index + 1).min(last),
            LEFT | PAGE_UP => self.index = self.index.saturating_sub(1),
            HOME => self.index = 0,
            END => self.index = last,
            SPACE => self.keyed = !self.keyed,
            _ => {}
        }
    }

    /// R: rotate [`ROTATE`] through the file's own colours there, or stop and put the
    /// shadow palette back.
    fn toggle_rotation(&mut self) {
        let running = self
            .dac
            .anims()
            .flags(0)
            .is_some_and(|f| f & flag::ACTIVE != 0);
        if running {
            self.dac.anims_mut().stop_all();
            let shadow = *self.dac.shadow();
            self.dac.set(&shadow);
            return;
        }
        let (first, last) = (*ROTATE.start(), *ROTATE.end());
        let ring = &to_vga6(&self.palette)[usize::from(first) * 3..(usize::from(last) + 1) * 3];
        let count = last - first + 1;
        self.dac
            .set_anims(anim::list(&[(1, ROTATE_SPEED, first, count, ring)]));
        self.dac.start_anim(0, 0);
    }

    /// Draws the current state into `fb`, made by [`Viewer::framebuffer`].
    pub fn draw(&self, fb: &mut Framebuffer) {
        match &self.content {
            Content::Sprites(sprites) => {
                let (dark, light) = contrast_pair(&self.palette);
                let view = fb.view();
                for y in (0..view.height).step_by(CHECKER) {
                    for x in (0..view.width).step_by(CHECKER) {
                        let colour = if ((x + y) / CHECKER).is_multiple_of(2) {
                            dark
                        } else {
                            light
                        };
                        let size = CHECKER as u32;
                        fb.fill_rect(Rect::new(x as i32, y as i32, size, size), colour);
                    }
                }
                let Some(sprite) = sprites.get(self.index) else {
                    return;
                };
                let blit = SpriteBlit {
                    mode: if self.keyed {
                        BlitMode::Keyed
                    } else {
                        BlitMode::Opaque
                    },
                    anchor: Anchor::SurfaceCentre,
                    ..SpriteBlit::default()
                };
                if !fb.blit_sprite(sprite, 0, 0, blit) {
                    log::warn!("{}: sprite {} does not fit the view", self.name, self.index);
                }
            }
            Content::Picture {
                width,
                height,
                pixels,
            } => {
                fb.clear(0);
                let surface = fb.surface();
                let left = (surface.width - width) / 2;
                let top = (surface.height - height) / 2;
                let dst = fb.pixels_mut();
                for (y, row) in pixels.chunks_exact(*width).take(*height).enumerate() {
                    let start = (top + y) * surface.width + left;
                    dst[start..start + width].copy_from_slice(row);
                }
                fb.set_origin(self.origin.0, self.origin.1)
                    .expect("the origin stays on the surface");
            }
        }
    }

    /// The window title: the file, and the sprite or the view's position.
    pub fn title(&self) -> String {
        let what = match &self.content {
            Content::Sprites(sprites) => match sprites.get(self.index) {
                Some(s) => format!(
                    "sprite {} (0 to {}), {}x{}, {}",
                    self.index,
                    sprites.len() - 1,
                    s.width(),
                    s.height(),
                    if self.keyed {
                        "colour 0 clear"
                    } else {
                        "opaque"
                    }
                ),
                None => "no sprites".to_string(),
            },
            Content::Picture { width, height, .. } => {
                let (x, y) = self.origin;
                format!("{width}x{height}, view at ({x}, {y})")
            }
        };
        format!("Xenobots - {}: {what}", self.name)
    }
}

/// `palette` back in 6-bit DAC levels: every palette here was widened from them (or,
/// for a `.pcx`, stored widened the same way), so this is exact.
pub(crate) fn to_vga6(palette: &Palette) -> [u8; DAC_BYTES] {
    std::array::from_fn(|b| {
        let c = palette[(b / 3) as u8];
        [c.r, c.g, c.b][b % 3] >> 2
    })
}

/// The darkest and the brightest colour of `palette` (lowest index on a tie), for a
/// background that shows which sprite pixels are left alone.
pub fn contrast_pair(palette: &Palette) -> (u8, u8) {
    let luma = |i: &u8| {
        let c = palette[*i];
        299 * u32::from(c.r) + 587 * u32::from(c.g) + 114 * u32::from(c.b)
    };
    let dark = (0..=255).min_by_key(luma).expect("256 colours");
    let light = (0..=255).rev().max_by_key(luma).expect("256 colours");
    (dark, light)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures;

    /// A 6-bit palette whose colour `i` is grey level `i / 4`: 0 is the darkest, and
    /// 252 the first of the four brightest.
    fn grey_vga6() -> Vec<u8> {
        (0..=255u8).flat_map(|i| [i / 4; 3]).collect()
    }

    fn down(code: ScanCode) -> InputEvent {
        InputEvent::KeyDown {
            code,
            repeat: false,
        }
    }

    fn viewer(kind: Kind, bytes: &[u8]) -> Viewer {
        let (content, own) = decode(kind, bytes).unwrap();
        let palette = own.unwrap_or_else(|| Palette::from_vga6(&grey_vga6()).unwrap());
        Viewer::new("test".into(), kind, content, palette)
    }

    fn picture(kind: Kind, width: usize, height: usize) -> Viewer {
        let pixels = fixtures::synthetic_bytes(width * height, 7);
        let content = Content::Picture {
            width,
            height,
            pixels,
        };
        Viewer::new(
            "test".into(),
            kind,
            content,
            Palette::from_vga6(&grey_vga6()).unwrap(),
        )
    }

    /// Presses `code` (one KeyDown) and returns whether anything changed.
    fn press(v: &mut Viewer, code: ScanCode) -> bool {
        v.step(&[down(code)], &Keys::default())
    }

    /// Holds `codes` for `retraces` retraces.
    fn hold(v: &mut Viewer, codes: &[ScanCode], retraces: usize) {
        let mut keys = Keys::default();
        codes.iter().for_each(|&c| _ = keys.press(c));
        (0..retraces).for_each(|_| _ = v.step(&[], &keys));
    }

    #[test]
    fn kinds_are_told_by_name_in_any_case() {
        for (name, kind) in [
            ("msprite1.b@n", Kind::Sprites),
            ("MSPRITE1.B@N", Kind::Sprites),
            ("sky_a.b@n", Kind::Sky),
            ("login.m@l", Kind::Screen),
            ("EALOGO.VCL", Kind::Cutscene),
            ("xenodrop.pcx", Kind::Backdrop),
        ] {
            assert_eq!(Kind::of(name), Ok(kind), "{name}");
        }
        for name in ["scen00a.usg", "humanoid.m08", "readme"] {
            let e = Kind::of(name).unwrap_err();
            assert!(e.contains(name) && e.contains(SUPPORTED), "{e}");
        }
        assert_eq!(Kind::Cutscene.view(), Size::MODE_240);
        assert_eq!(Kind::Screen.view(), Size::MODE_13H);
    }

    #[test]
    fn palettes_come_from_the_flag_the_file_or_a_stock_screen() {
        use PaletteChoice::*;
        for (name, screen) in [
            ("msprite0.b@n", "sitroom.m@l"),
            ("msprite1.b@n", "sitroom.m@l"),
            ("MSPRITE2.B@N", "humancon.m@l"),
            ("msprite3.b@n", "scorpcon.m@l"),
            ("msprite4.b@n", "scoutcon.m@l"),
            ("msprite5.b@n", "maproom.m@l"),
            ("other.b@n", "sitroom.m@l"),
        ] {
            assert_eq!(palette_choice(Kind::Sprites, name, None), Stock(screen));
        }
        assert_eq!(
            palette_choice(Kind::Sky, "sky_c.b@n", None),
            Stock("humancon.m@l")
        );
        for kind in [Kind::Screen, Kind::Cutscene, Kind::Backdrop] {
            assert_eq!(palette_choice(kind, "x", None), Own);
        }
        let given = Path::new("review.m@l");
        assert_eq!(palette_choice(Kind::Screen, "x", Some(given)), Given(given));
    }

    #[test]
    fn palette_flag_takes_only_files_with_a_palette() {
        let screen = fixtures::synthetic_screen(&grey_vga6(), &[3; 64_000]);
        let palette = palette_of("LOGIN.M@L", &screen).unwrap();
        assert_eq!(palette[255].r, 255);
        let bank = fixtures::synthetic_sprite_bank(&[(1, 1, &[1])]);
        assert!(
            palette_of("msprite0.b@n", &bank)
                .unwrap_err()
                .contains("no palette")
        );
        assert!(palette_of("x.vcl", &[1, 2, 3]).is_err());
    }

    #[test]
    fn sprites_page_and_stop_at_either_end() {
        let bank =
            fixtures::synthetic_sprite_bank(&[(2, 1, &[1, 2]), (1, 1, &[3]), (3, 2, &[4; 6])]);
        let mut v = viewer(Kind::Sprites, &bank);
        assert_eq!(v.surface(), Size::MODE_13H);
        assert!(
            v.title().ends_with("sprite 0 (0 to 2), 2x1, opaque"),
            "{}",
            v.title()
        );
        assert!(press(&mut v, RIGHT));
        assert!(press(&mut v, PAGE_DOWN));
        assert!(!press(&mut v, RIGHT));
        assert_eq!(v.index, 2);
        assert!(press(&mut v, HOME));
        assert!(!press(&mut v, LEFT));
        assert!(press(&mut v, END));
        assert!(press(&mut v, PAGE_UP));
        assert_eq!(v.index, 1);
        // Auto-repeat pages too; held keys alone do not.
        let repeat = InputEvent::KeyDown {
            code: RIGHT,
            repeat: true,
        };
        assert!(v.step(&[repeat], &Keys::default()));
        hold(&mut v, &[LEFT], 5);
        assert_eq!(v.index, 2);
        assert!(press(&mut v, SPACE));
        assert!(v.title().ends_with("3x2, colour 0 clear"), "{}", v.title());
    }

    #[test]
    fn an_empty_bank_shows_the_background() {
        let mut v = viewer(Kind::Sprites, &fixtures::synthetic_sprite_bank(&[]));
        assert!(!press(&mut v, END));
        assert!(v.title().ends_with("no sprites"));
        v.framebuffer();
    }

    #[test]
    fn keyed_sprites_let_the_background_through() {
        // A 2x2 sprite of colour 0 and 9, centred on the 320x200 view at (159, 99).
        let bank = fixtures::synthetic_sprite_bank(&[(2, 2, &[0, 9, 9, 9])]);
        let mut v = viewer(Kind::Sprites, &bank);
        assert_eq!(contrast_pair(&v.palette), (0, 252));
        let fb = v.framebuffer();
        assert_eq!(fb.get(159, 99), Some(0));
        assert_eq!(fb.get(160, 100), Some(9));
        press(&mut v, SPACE);
        let fb = v.framebuffer();
        // (159, 99) is in the 8x8 square at (152, 96): the 39th across, 12th down.
        assert_eq!(fb.get(159, 99), Some(252));
        assert_eq!(fb.get(160, 99), Some(9));
    }

    #[test]
    fn f_fades_out_i_fades_back_and_r_rotates_the_files_colours() {
        let mut v = picture(Kind::Screen, 320, 200);
        let grey = |v: &Viewer, c: u8| v.shown_palette()[c].r >> 2;
        assert_eq!(grey(&v, 200), 50);
        press(&mut v, F);
        hold(&mut v, &[], usize::from(FADE) + 2);
        assert!((0..=255).all(|c| grey(&v, c) == 0));
        // The fade in starts once its black is uploaded and ends one level short.
        press(&mut v, I);
        hold(&mut v, &[], usize::from(FADE) + 3);
        assert_eq!((grey(&v, 200), grey(&v, 4)), (49, 0));
        // R: colours 16..=31 (grey 4..=7) move one place every 4 retraces, in the
        // file's own colours.
        let mut v = picture(Kind::Screen, 320, 200);
        press(&mut v, R);
        hold(&mut v, &[], 1);
        let before: Vec<u8> = (16..=31).map(|c| grey(&v, c)).collect();
        assert_eq!(before[..5], [4, 4, 4, 4, 5]);
        hold(&mut v, &[], 4);
        let after: Vec<u8> = (16..=31).map(|c| grey(&v, c)).collect();
        assert_eq!((&after[..15], after[15]), (&before[1..], before[0]));
        assert_eq!((grey(&v, 15), grey(&v, 32)), (3, 8));
        // R again stops it and puts the shadow palette back.
        press(&mut v, R);
        hold(&mut v, &[], 1);
        assert_eq!(grey(&v, 16), 4);
    }

    #[test]
    fn contrast_pair_takes_the_lowest_index_on_a_tie() {
        let mut palette = Palette::from_vga6(&[20; 768]).unwrap();
        assert_eq!(contrast_pair(&palette), (0, 0));
        palette.colors[7].g = 255;
        palette.colors[9].r = 1;
        palette.colors[9].g = 0;
        assert_eq!(contrast_pair(&palette), (9, 7));
    }

    #[test]
    fn cutscenes_pan_a_320x240_view_over_the_picture() {
        let pixels = fixtures::synthetic_bytes(640 * 400, 1);
        let vcl = fixtures::synthetic_vcl(&grey_vga6(), &pixels);
        let mut v = viewer(Kind::Cutscene, &vcl);
        assert_eq!(v.surface(), Size::new(640, 400));
        let fb = v.framebuffer();
        assert_eq!((fb.view(), fb.origin()), (Size::MODE_240, (0, 0)));
        assert_eq!(fb.pixels(), &pixels[..]);
        // A tap moves one pixel; holding moves one per retrace, up to the edge.
        assert!(press(&mut v, RIGHT));
        assert_eq!(v.origin, (1, 0));
        hold(&mut v, &[RIGHT, DOWN], 100);
        assert_eq!(v.origin, (101, 100));
        hold(&mut v, &[RIGHT, DOWN], 1000);
        assert_eq!(v.origin, (320, 160));
        assert!(!v.step(&[], &Keys::default()));
        // Opposite keys cancel; Home and End jump across.
        hold(&mut v, &[LEFT, RIGHT], 10);
        assert_eq!(v.origin, (320, 160));
        assert!(press(&mut v, HOME));
        assert_eq!(v.origin, (0, 160));
        hold(&mut v, &[UP, LEFT], 1000);
        assert_eq!(v.origin, (0, 0));
        assert!(press(&mut v, END));
        let mut fb = v.framebuffer();
        v.draw(&mut fb);
        assert_eq!(fb.origin(), (320, 0));
        assert!(
            v.title().ends_with("640x400, view at (320, 0)"),
            "{}",
            v.title()
        );
    }

    #[test]
    fn each_picture_kind_gets_its_surface_and_pan_limits() {
        let pcx = fixtures::synthetic_pcx(768, 130, &[5; 768 * 130], &[0; 768]);
        let screen = fixtures::synthetic_screen(&grey_vga6(), &[3; 64_000]);
        let sky = fixtures::synthetic_packed(&[4; 342 * 54]);
        let vcl = fixtures::synthetic_vcl(&grey_vga6(), &[2; 640 * 240]);
        for (kind, bytes, surface, max) in [
            (Kind::Backdrop, pcx, Size::new(768, 200), (448, 0)),
            (Kind::Screen, screen, Size::MODE_13H, (0, 0)),
            (Kind::Sky, sky, Size::new(342, 200), (22, 0)),
            (Kind::Cutscene, vcl, Size::new(640, 240), (320, 0)),
        ] {
            let mut v = viewer(kind, &bytes);
            assert_eq!((v.surface(), v.max_origin()), (surface, max), "{kind:?}");
            hold(&mut v, &[RIGHT, DOWN], 1000);
            assert_eq!(v.origin, max, "{kind:?}");
        }
        // Pictures shorter than the view are centred on it, with colour 0 around.
        let fb = picture(Kind::Backdrop, 768, 130).framebuffer();
        assert_eq!(fb.get(0, 34), Some(0));
        assert_eq!(fb.get(0, 35), Some(fixtures::synthetic_bytes(1, 7)[0]));
        assert_eq!(fb.get(0, 165), Some(0));
    }

    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR pointing at a real install"]
    fn real_assets_every_supported_file_loads_into_the_viewer() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(workspace_root.join(path).into()))
            .check()
            .unwrap();
        let mut entries: Vec<_> = std::fs::read_dir(&files.dir().path)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| Kind::of(name).is_ok())
            .collect();
        entries.sort();
        assert_eq!(entries.len(), 38, "{entries:?}");
        for name in entries {
            let mut v = load(&files, Path::new(&name), None).unwrap_or_else(|e| panic!("{e}"));
            let mut fb = v.framebuffer();
            // Page through every sprite in both modes, and pan to the far corner.
            press(&mut v, SPACE);
            while press(&mut v, RIGHT) {
                v.draw(&mut fb);
            }
            hold(&mut v, &[RIGHT, DOWN], 1000);
            v.draw(&mut fb);
        }
        let v = load(
            &files,
            Path::new("sky_a.b@n"),
            Some(Path::new("XENODROP.PCX")),
        );
        assert!(v.is_ok());
    }
}
