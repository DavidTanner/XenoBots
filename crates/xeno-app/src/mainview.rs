//! The situation room's main view in modes M, R and C (T-103; `docs/screens.md`, "The
//! situation room's main view"): the map as view modes 0xc and 0xe draw it through
//! the table at `DS:0x39ae`, the selected unit's own view, and the units' message log.

use xeno_formats::ban::Sprite;
use xeno_formats::font::Font;
use xeno_formats::maproom::{HANDLE_STEP, Rect};
use xeno_formats::sitroom::SitRoom;
use xeno_formats::usg::{Base, Message, ObjectKind, Scenario, Side, TerrainFeature, flag};
use xeno_formats::view3d::View3d;
use xeno_formats::views::Widget;
use xeno_render::shape::fill_polygon;
use xeno_render::view3d::{Camera, GameFloat, PROBE, focal_shifted, focal_zoomed};
use xeno_render::{Anchor, BlitMode, Framebuffer, SpriteBlit, TextCursor, TextMode};
use xeno_sim::motion::Tables;
use xeno_sim::{Damage, Entity, EntityId, Fixed, Position, State, World};

use crate::scene::trig;

/// The main view's map mode.
const MAP_MODE: u8 = 4;
/// The map's colours (`1fe6:6c05`): the background the clear step fills (`DS:0x1457`)
/// and the hills (`DS:0x1458`). Short code constants, kept here.
pub const MAP_BACKGROUND: u8 = 0x43;
pub const MAP_HILL: u8 = 0x42;
/// The terrain records the map draws: those whose program starts with op 0x14.
const HILL_OP: u8 = 0x14;
/// The objects' map routines' sprite handles (in `msprite0`) and dot colours.
const MARKER_SPRITE: u16 = 0x198;
const OBJECT_MARKER_SPRITE: u16 = 0x1a0;
const BASE_SPRITE: u16 = 0x1b8;
const BASE_SPRITES: [u16; 2] = [0x1a8, 0x1b0];
const RELAY_SPRITE: u16 = 0x180;
const RELAY_SPRITES: [u16; 2] = [0x188, 0x190];
const RELAY_DOTS: [u8; 2] = [0xfe, 0xfd];
const RELAY_UNLINKED_DOT: u8 = 7;
const ALIEN_DOT: u8 = 0xfd;
pub const OBSTACLE_DOT: u8 = 1;
const MARK_DOT: u8 = 0x0f;
/// `+0x17` bits: bit 0 hides a robot from the map; bit 2, an alien one is detected.
const HIDDEN: u8 = 0x01;
const DETECTED: u8 = 0x04;
/// A robot's `+0xfd` bit 2: in its base.
const IN_BASE: u8 = 0x04;
/// Sprites of zoom factors from this up are the large ones.
const LARGE_ZOOM: u8 = 8;
/// Where the map's zoom label and the robot view's heading go in the view's buffer,
/// and the at-base text (`1fe6:6cb9`, `1000:5630`); the heading's degree sign.
const ZOOM_LABEL: (i32, i32) = (8, 0x5a);
const HEADING_LABEL: (i32, i32) = (0x0c, 0x60);
const AT_BASE_LABEL: (i32, i32) = (0x30, 0x28);
const DEGREE: u8 = 0x17;
/// shortcut: the labels' colour is whatever the last text drawn left in the text
/// state (`DS:0x644` +0x16); 0x0f is a guess, to check against DOSBox.
const LABEL_COLOUR: u8 = 0x0f;
/// Mode C (`1000:56a7`, `1fe6:6d02`): the heading's place and colours, the rule's
/// colour, the rows (font 2) from y 0x28 every 12, names and times at x 0x60 in 0x14,
/// texts at x 0x9c in 0x13. The view is cleared to 0x10 first.
pub const LOG_CLEAR: u8 = 0x10;
const LOG_HEADING: (i32, i32) = (0x60, 0x1e);
const LOG_COLOUR: u8 = 0x14;
const LOG_HIGH_COLOUR: u8 = 0xff;
pub const LOG_FONT: u16 = 2;
const LOG_TOP: i32 = 0x28;
const LOG_STEP: i32 = 0x0c;
const LOG_NAME_X: i32 = 0x60;
const LOG_TEXT_X: i32 = 0x9c;
const LOG_TEXT_COLOUR: u8 = 0x13;

/// The map controls (`1000:5c59` to `5f4b`), shown live in mode M only: the zoom
/// button (0x300 plus the shared pressed look, 0x310 in other modes) and the overlay
/// toggles (type, bit, sprite when off, the next when on, and other modes' sprite),
/// all drawn opaque a pixel in from their corner. Short code constants, kept here.
pub const ZOOM_TYPE: u16 = 0x64;
const ZOOM_SPRITES: [u16; 2] = [0x300, 0x310];
const TOGGLES: [(u16, u16, u16, u16); 5] = [
    (0x66, Base::MAP_ROBOTS, 0x330, 0x340),
    (0x68, Base::MAP_SPRITES, 0x318, 0x328),
    (0x6a, Base::MAP_MINES, 0x348, 0x358),
    (0x6c, Base::MAP_RELAYS, 0x360, 0x370),
    (0x6e, Base::MAP_GRID, 0x378, 0x388),
];
/// The highest zoom.
pub const MAX_ZOOM: u8 = 6;
/// The mode buttons (`1000:5efd`): the mode's label at (+0xe, +2) and the button's
/// frame, 0x390 plus the pressed look, at the corner.
pub const MODE_BUTTON_TYPE: u16 = 0x70;
const MODE_LABEL_AT: (i32, i32) = (0x0e, 2);
const MODE_FRAME: u16 = 0x390;
/// The roster's rows (`1000:57e3`, `1fe6:6d87`): font 0 with an x step of 3, the call
/// sign at (0x18, 0x12) in 0x0f (0x0c selected) on 0x42, icons from y 0x10, a row
/// every 11 pixels; an empty row's text at x 6 and the blank icon.
const ROSTER_STEP_X: i32 = 3;
const ROSTER_TEXT: (i32, i32) = (0x18, 0x12);
const ROSTER_EMPTY_X: i32 = 6;
const ROSTER_ICON_TOP: i32 = 0x10;
const ROSTER_ROW: i32 = 0x0b;
const ROSTER_COLOUR: u8 = 0x0f;
const ROSTER_SELECTED: u8 = 0x0c;
const ROSTER_BACKGROUND: u8 = 0x42;
const BLANK_ICON: u16 = 0x478;
pub const ROSTER_ROWS: usize = 5;

/// The overlay bit a toggle's widget type flips.
pub fn toggle_bit(kind: u16) -> Option<u16> {
    TOGGLES.iter().find(|t| t.0 == kind).map(|t| t.1)
}

fn opaque(fb: &mut Framebuffer, sprite: Option<&Sprite>, x: i32, y: i32) {
    if let Some(sprite) = sprite {
        fb.blit_sprite(sprite, x, y, SpriteBlit::default());
    }
}

/// The map controls and the mode buttons, from the view's nested widgets; `modes`
/// gives each mode button's mode (its state's second byte).
#[allow(clippy::too_many_arguments)]
pub fn draw_controls<'a>(
    fb: &mut Framebuffer,
    widgets: &[Widget],
    modes: impl Fn(&Widget) -> Option<u8>,
    map: &MapState,
    mode: u8,
    pressed: bool,
    room: &SitRoom,
    sprite: impl Fn(u16) -> Option<&'a Sprite>,
) {
    let live = mode == MAP_MODE;
    let look = if pressed { HANDLE_STEP } else { 0 };
    for w in widgets {
        let Some(area) = w.area else { continue };
        let (x, y) = (i32::from(area.left), i32::from(area.top));
        let toggle = TOGGLES.iter().find(|t| t.0 == w.kind);
        let handle = match (w.kind, toggle) {
            (ZOOM_TYPE, _) if live => ZOOM_SPRITES[0] + look,
            (ZOOM_TYPE, _) => ZOOM_SPRITES[1],
            (_, Some(&(_, bit, off, _))) if live => {
                off + if map.overlays & bit != 0 {
                    HANDLE_STEP
                } else {
                    0
                }
            }
            (_, Some(&(_, _, _, idle))) => idle,
            (MODE_BUTTON_TYPE, _) => {
                let Some(m) = modes(w) else { continue };
                if let Some(&label) = room.mode_labels.get(usize::from(m / 2)) {
                    let current = if m == mode { HANDLE_STEP } else { 0 };
                    opaque(
                        fb,
                        sprite(label + current),
                        x + MODE_LABEL_AT.0,
                        y + MODE_LABEL_AT.1,
                    );
                }
                opaque(fb, sprite(MODE_FRAME + look), x, y);
                continue;
            }
            _ => continue,
        };
        opaque(fb, sprite(handle), x + 1, y + 1);
    }
}

/// A roster row's unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RosterUnit {
    /// +0xdb, +0xf9, the kind (0 to 2), at its base, the selected unit.
    pub call_sign: u8,
    pub repair: u8,
    pub kind: usize,
    pub in_base: bool,
    pub selected: bool,
}

/// The roster's five rows: each unit's call sign, status icon and kind icon, or an
/// empty row.
pub fn draw_roster<'a>(
    fb: &mut Framebuffer,
    rows: &[Option<RosterUnit>],
    room: &SitRoom,
    font: &Font,
    sprite: impl Fn(u16) -> Option<&'a Sprite>,
) {
    let keyed = SpriteBlit {
        mode: BlitMode::Keyed,
        ..SpriteBlit::default()
    };
    let [status_x, kind_x] = room.icon_lefts.map(i32::from);
    for (i, row) in rows.iter().take(ROSTER_ROWS).enumerate() {
        let (text_y, icon_y) = (
            ROSTER_TEXT.1 + ROSTER_ROW * i as i32,
            ROSTER_ICON_TOP + ROSTER_ROW * i as i32,
        );
        let (x, colour, text, icons) = match row {
            Some(u) => (
                ROSTER_TEXT.0,
                if u.selected {
                    ROSTER_SELECTED
                } else {
                    ROSTER_COLOUR
                },
                room.call_sign(u.call_sign),
                [
                    room.status_icons[usize::from(u.repair >> 4) & 7],
                    room.kind_icons[usize::from(u.in_base)]
                        .get(u.kind)
                        .copied()
                        .unwrap_or(BLANK_ICON),
                ],
            ),
            None => (
                ROSTER_EMPTY_X,
                ROSTER_COLOUR,
                room.empty_row.as_slice(),
                [BLANK_ICON; 2],
            ),
        };
        let mut cursor = TextCursor::at(x, text_y, colour);
        cursor.background = ROSTER_BACKGROUND;
        cursor.spacing_x = ROSTER_STEP_X - i32::from(font.header.step_x);
        fb.draw_text(font, &mut cursor, text, TextMode::Opaque);
        for (handle, x) in icons.into_iter().zip([status_x, kind_x]) {
            if let Some(s) = sprite(handle) {
                fb.blit_sprite(s, x, icon_y, keyed);
            }
        }
    }
}

/// The roster's bytes the sim does not keep: +0xdb and +0xf9 by slot.
/// shortcut: the scenario's as loaded; robots made since have call sign 0.
pub fn roster_bytes(scenario: Option<&Scenario>) -> [(u8, u8); 240] {
    let mut bytes = [(0, 0); 240];
    for o in scenario.iter().flat_map(|s| s.objects.iter().flatten()) {
        if let (Some(b), xeno_formats::usg::Body::Robot(r)) =
            (bytes.get_mut(usize::from(o.slot)), &o.body)
        {
            *b = (r.call_sign, r.damage);
        }
    }
    bytes
}

/// What the map shows: the base's +0x32 bits, +0x36 zoom and +0x4f centre, and the
/// editor's Ctrl+E (`DS:0x6ffa`): every alien robot, detected or not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapState {
    pub overlays: u16,
    pub zoom: u8,
    pub centre: Position,
    pub reveal: bool,
}

impl MapState {
    pub fn of(base: &Base) -> MapState {
        MapState {
            overlays: base.map_overlays,
            zoom: base.map_zoom,
            centre: Position {
                x: xeno_sim::Fixed(base.map_centre.x),
                y: xeno_sim::Fixed(base.map_centre.y),
            },
            reveal: false,
        }
    }

    fn factor(&self) -> u8 {
        1u8.checked_shl(self.zoom.into()).unwrap_or(0)
    }
}

/// The objects' +0x16 and +0x17 by slot.
pub type Flags = [(u8, u8); 240];

/// A relay's +0x16 bits 4 and 6 and an alarm buoy's bit 4 are the sim's.
/// shortcut: the sim keeps no detection bits (+0x17) and no other object's +0x16, so
/// those are the scenario's as loaded until it does.
pub fn flags(scenario: Option<&Scenario>, world: &World) -> Flags {
    let mut flags = [(0, 0); 240];
    for o in scenario.iter().flat_map(|s| s.objects.iter().flatten()) {
        if let Some(f) = flags.get_mut(usize::from(o.slot)) {
            *f = (o.flags, o.status);
        }
    }
    for (id, e) in world.iter() {
        let Some(f) = flags.get_mut(usize::from(id.0)) else {
            continue;
        };
        match e.state {
            State::Relay(r) => f.0 = f.0 & !xeno_sim::relay::FLAGS | r.flags,
            State::Drop(d) => {
                f.0 = f.0 & !flag::RELAY_LINKED | (u8::from(d.alarm) * flag::RELAY_LINKED);
            }
            _ => {}
        }
    }
    flags
}

/// Whether robot `e` is in its base (+0xfd bit 2).
pub fn in_base(e: &Entity) -> bool {
    matches!(e.damage, Damage::Robot(c) if c.equipment & IN_BASE != 0)
}

/// The map's camera (`1fe6:6c05`): heading 0, at the map's centre while it is free,
/// else on the selected unit out of its base, else on the base.
pub fn camera(
    base: Position,
    selected: Option<&Entity>,
    map: &MapState,
    view: &View3d,
    tables: &Tables,
) -> Camera {
    let at = match selected {
        _ if map.overlays & Base::MAP_FREE != 0 => map.centre,
        Some(e) if !in_base(e) => e.position,
        _ => base,
    };
    Camera {
        x: at.x.0,
        y: at.y.0,
        z: 0,
        heading: 0,
        sin: trig(tables, 0),
        cos: trig(tables, 0x4000),
        focal: focal_zoomed(view.focal[2], map.factor()),
        viewer: u16::MAX,
    }
}

/// A console's map (`1000:dbe8` in display mode 2): on the robot, heading 0, at a zoom
/// of `1 << map_zoom` (+0xf2) and a zoom shift of 1, showing the robots as sprites,
/// the mines and the relays (`DS:0x62ea` = 0x0f).
pub fn console_map(
    at: Position,
    map_zoom: u8,
    view: &View3d,
    tables: &Tables,
) -> (Camera, MapState) {
    let camera = Camera {
        x: at.x.0,
        y: at.y.0,
        z: 0,
        heading: 0,
        sin: trig(tables, 0),
        cos: trig(tables, 0x4000),
        focal: focal_shifted(
            view.focal[2],
            1u8.checked_shl(map_zoom.into()).unwrap_or(0),
            1,
        ),
        viewer: u16::MAX,
    };
    let overlays = Base::MAP_ROBOTS | Base::MAP_SPRITES | Base::MAP_MINES | Base::MAP_RELAYS;
    let map = MapState {
        overlays,
        zoom: map_zoom,
        centre: at,
        reveal: false,
    };
    (camera, map)
}

/// A position's pixel in the buffer (`1000:3ad8`): columns right with the lateral
/// offset (`CX`, x at the map's heading 0), rows up with the depth (`AX`, y), from the
/// middle. [`pointer_to_world`] is its inverse.
pub fn pixel(camera: &Camera, buffer: &Framebuffer, x: i32, y: i32) -> (i32, i32) {
    let (across, along) = camera.map_offset(x.wrapping_sub(camera.x), y.wrapping_sub(camera.y));
    let size = buffer.surface();
    (
        (size.width / 2) as i32 + i32::from(across),
        (size.height / 2) as i32 - i32::from(along),
    )
}

/// `ui_map_pointer_to_world` (`1000:3fd3`): the point under the pointer at `(px, py)`
/// on a map drawn by `camera` in `view` (the target rectangle, `DS:0x145a`): each
/// offset from the rectangle's middle, a 16-bit whole number of pixels, over the focal
/// factor (`DS:0x4d24`), back to a 16.16 (`1000:0d6f`) and added to the camera's
/// position (`DS:0x1462`, `0x1466`). The map's heading is 0, so nothing is turned back.
pub fn pointer_to_world(camera: &Camera, view: Rect, (px, py): (u16, u16)) -> Position {
    let offset = |pixels: i32| {
        // The middle is the halved 17-bit sum, the offset kept to 16 bits.
        let d = pixels as i16;
        let d = GameFloat {
            mantissa: d.unsigned_abs(),
            exponent: 0,
            negative: d < 0,
        };
        d.over(camera.focal).to_raw()
    };
    let middle = |a: u16, b: u16| ((u32::from(a) + u32::from(b)) >> 1) as i32;
    let rows = middle(view.top, view.bottom) - i32::from(py);
    let columns = i32::from(px) - middle(view.left, view.right);
    Position {
        x: Fixed(camera.x.wrapping_add(offset(columns))),
        y: Fixed(camera.y.wrapping_add(offset(rows))),
    }
}

/// Node type 0xc (`1000:38bc`): one pixel, if it is in the buffer.
fn dot(buffer: &mut Framebuffer, (x, y): (i32, i32), colour: u8) {
    let size = buffer.surface();
    if (0..size.width as i32).contains(&x) && (0..size.height as i32).contains(&y) {
        buffer.pixels_mut()[y as usize * size.width + x as usize] = colour;
    }
}

/// Node type 0xa (`1000:3912`): the sprite centred on the point, keyed, if it fits;
/// then a dot of `colour` unless it is 0.
pub fn mark(buffer: &mut Framebuffer, at: (i32, i32), sprite: Option<&Sprite>, colour: u8) {
    if let Some(sprite) = sprite {
        let blit = SpriteBlit {
            mode: BlitMode::Keyed,
            anchor: Anchor::Centre,
            ..SpriteBlit::default()
        };
        buffer.blit_sprite(sprite, at.0, at.1, blit);
    }
    if colour != 0 {
        dot(buffer, at, colour);
    }
}

/// Node type 0x12 (`1000:3a27`): an octagon at `(x, y)` in `colour`, at the radius `r`
/// (16.16) on the axes and two thirds of it on the diagonals, filled by the 3D view's
/// polygon fill. A hill (`gfx_draw_hill_outline_on_map`, `1000:39fb`) is one in the
/// hill colour at its 8.8 radius shifted to a 16.16.
pub fn octagon(buffer: &mut Framebuffer, camera: &Camera, (x, y): (i32, i32), r: i32, colour: u8) {
    let d = ((r as u32).wrapping_mul(2) / 3) as i32;
    let r = i32::from(camera.map_offset(r, r).1);
    let d = i32::from(camera.map_offset(d, d).1);
    let (x, y) = pixel(camera, buffer, x, y);
    let corners = [
        (x, y - r),
        (x - d, y - d),
        (x - r, y),
        (x - d, y + d),
        (x, y + r),
        (x + d, y + d),
        (x + r, y),
        (x + d, y - d),
    ]
    .map(|(x, y)| (x as i16, y as i16));
    fill_polygon(buffer, &corners, colour);
}

/// The maps' pointer probe (`docs/render.md`, "The pointer probe (T-133)"): armed on
/// a pixel after the clear, it names the object of the last sprite node drawn over it,
/// or of a dot drawn exactly on it.
#[derive(Debug, Default)]
pub struct Probe {
    at: Option<(usize, usize)>,
    pub under: Option<u16>,
}

impl Probe {
    /// `1000:4042`: the pixel `at` (in the buffer), if it is in it, marked [`PROBE`].
    pub fn arm(buffer: &mut Framebuffer, at: Option<(usize, usize)>) -> Probe {
        let size = buffer.surface();
        let at = at.filter(|&(x, y)| x < size.width && y < size.height);
        if let Some((x, y)) = at {
            buffer.pixels_mut()[y * size.width + x] = PROBE;
        }
        Probe { at, under: None }
    }

    /// After a sprite node (`1000:3912` calls `40bc`): `object` named if the node
    /// changed the pixel, which is marked again.
    fn sprite(&mut self, buffer: &mut Framebuffer, object: Option<u16>) {
        let Some((x, y)) = self.at else {
            return;
        };
        let i = y * buffer.surface().width + x;
        if buffer.pixels()[i] != PROBE {
            buffer.pixels_mut()[i] = PROBE;
            self.under = object;
        }
    }

    /// After a dot (`1000:38bc`): `object` named if the dot is on the pixel and is not
    /// the viewer's; the pixel keeps the dot's colour.
    fn dot(&mut self, at: (i32, i32), object: u16, viewer: u16) {
        if self.at.map(|(x, y)| (x as i32, y as i32)) == Some(at) && object != viewer {
            self.under = Some(object);
        }
    }
}

/// A map node of [`draw_objects`]: a dot (type 0xc) or a sprite with its dot (0xa).
enum Node<'a> {
    Dot(u8),
    Mark(Option<&'a Sprite>, u8),
}

/// What the main view needs to draw.
pub struct Inputs<'a> {
    pub room: &'a SitRoom,
    pub view: &'a View3d,
    /// `msprite0`, from handle 0.
    pub sprites: &'a [Sprite],
    pub font: Option<&'a Font>,
}

impl Inputs<'_> {
    pub fn sprite(&self, handle: u16) -> Option<&Sprite> {
        self.sprites.get(usize::from(handle / HANDLE_STEP))
    }
}

/// The map (view modes 0xc and 0xe, `1000:3877`): cleared, the pointer probe armed at
/// `probe`, then the hills, then [`draw_objects`]; the object under the probe.
/// `colours`: the clear and hill colours (`DS:0x1457`, `0x1458`).
#[allow(clippy::too_many_arguments)]
pub fn draw_map(
    buffer: &mut Framebuffer,
    world: &World,
    camera: &Camera,
    map: &MapState,
    terrain: &[TerrainFeature],
    flags: &Flags,
    selected: Option<&Entity>,
    inputs: &Inputs,
    [clear, hill_colour]: [u8; 2],
    probe: Option<(usize, usize)>,
) -> Option<u16> {
    buffer.clear(clear);
    let mut probe = Probe::arm(buffer, probe);
    for f in terrain.iter().filter(|f| f.shape.first() == Some(&HILL_OP)) {
        octagon(
            buffer,
            camera,
            (f.x, f.y),
            i32::from(f.radius) << 8,
            hill_colour,
        );
    }
    draw_objects(
        buffer, world, camera, map, flags, selected, inputs, &mut probe,
    );
    probe.under
}

/// The map's nodes after the terrain's: the selected unit's goal and the objects in
/// slot order, each probed, then the zoom label (the situation room's, drawn only with
/// a font).
#[allow(clippy::too_many_arguments)]
pub fn draw_objects(
    buffer: &mut Framebuffer,
    world: &World,
    camera: &Camera,
    map: &MapState,
    flags: &Flags,
    selected: Option<&Entity>,
    inputs: &Inputs,
    probe: &mut Probe,
) {
    // `1000:f914` (through `1000:4714`): the selected robot's goal object (+0x32 bit
    // 0, at the object) or else its destination (bit 1), a node with no object.
    let goal = match selected.map(|e| e.state) {
        Some(State::Robot(m)) if m.goal & 1 != 0 => world
            .get(EntityId(m.goal_object))
            .map(|o| (o.position, OBJECT_MARKER_SPRITE)),
        Some(State::Robot(m)) if m.goal & 2 != 0 => Some((m.destination, MARKER_SPRITE)),
        _ => None,
    };
    if let Some((point, sprite)) = goal {
        let at = pixel(camera, buffer, point.x.0, point.y.0);
        mark(buffer, at, inputs.sprite(sprite), 0);
        probe.sprite(buffer, None);
    }
    let sprites = map.overlays & Base::MAP_SPRITES != 0;
    for (id, e) in world.iter() {
        let (f16, f17) = flags[usize::from(id.0) % flags.len()];
        let side = usize::from(e.side == Side::Alien);
        let at = pixel(camera, buffer, e.position.x.0, e.position.y.0);
        let kind = u16::from(e.kind);
        let node = match kind {
            // `1000:f976`, and a decoy (10) as a robot of its dropper's model
            // (`1000:fa70`, `e5fe`: +0x19).
            0..=2 | 10 => {
                let unseen = e.side == Side::Alien && f17 & DETECTED == 0 && !map.reveal;
                if unseen || f17 & HIDDEN != 0 || map.overlays & Base::MAP_ROBOTS == 0 {
                    continue;
                }
                let model = match e.state {
                    State::Drop(d) => d.ticks,
                    _ => inputs.view.kind_models[usize::from(kind)],
                };
                let model = usize::from(model / 2);
                if !sprites {
                    Node::Dot(inputs.room.robot_dots.get(model).copied().unwrap_or(0))
                } else {
                    let size = usize::from(map.factor() >= LARGE_ZOOM);
                    let first = inputs.room.robot_sprites[size].get(model).copied();
                    let octant = (e.heading.wrapping_add(0x1000) >> 13) * HANDLE_STEP;
                    let sprite = first.and_then(|h| inputs.sprite(h + octant));
                    Node::Mark(sprite, if side == 1 { ALIEN_DOT } else { 0 })
                }
            }
            // `1000:f9e5`.
            3 => {
                let handle = if sprites {
                    BASE_SPRITES[side]
                } else {
                    BASE_SPRITE
                };
                Node::Mark(inputs.sprite(handle), 0)
            }
            // `1000:fa04`.
            4 if map.overlays & Base::MAP_RELAYS != 0 => {
                let linked = f16 & flag::RELAY_LINKED != 0;
                match (sprites, linked) {
                    (false, false) => Node::Dot(RELAY_UNLINKED_DOT),
                    (false, true) => Node::Dot(RELAY_DOTS[side]),
                    (true, false) => Node::Mark(inputs.sprite(RELAY_SPRITE), 0),
                    (true, true) => Node::Mark(inputs.sprite(RELAY_SPRITES[side]), 0),
                }
            }
            // `1000:fa6a`, `fa5c`.
            9 | 12 => Node::Dot(MARK_DOT),
            11 => Node::Dot(if f16 & flag::RELAY_LINKED != 0 {
                0xff
            } else {
                MARK_DOT
            }),
            // `1000:fa47`.
            16 => Node::Dot(OBSTACLE_DOT),
            // Shots (5 to 8) only with the developer's Ctrl+Alt+S (`DS:0x62ec` bit
            // 6, not modelled); 13 to 15 never.
            _ => continue,
        };
        match node {
            Node::Dot(colour) => {
                dot(buffer, at, colour);
                probe.dot(at, id.0, camera.viewer);
            }
            Node::Mark(sprite, colour) => {
                mark(buffer, at, sprite, colour);
                probe.sprite(buffer, Some(id.0));
            }
        }
    }
    if let (Some(font), Some(name)) = (
        inputs.font,
        inputs.room.zoom_names.get(usize::from(map.zoom)),
    ) {
        let (x, y) = ZOOM_LABEL;
        let mut cursor = TextCursor::at(x, y, LABEL_COLOUR);
        let text = [inputs.room.zoom_prefix.as_slice(), name].concat();
        buffer.draw_text(font, &mut cursor, &text, TextMode::Transparent);
    }
}

/// Mode R's text over the robot's view (`1000:5630`): at its base the buffer is
/// cleared to 0 under the at-base text, otherwise its heading in degrees.
pub fn label_robot_view(buffer: &mut Framebuffer, e: &Entity, inputs: &Inputs) {
    if in_base(e) {
        buffer.clear(0);
    }
    let Some(font) = inputs.font else {
        return;
    };
    let ((x, y), text) = robot_label(e, inputs.room);
    let mut cursor = TextCursor::at(x, y, LABEL_COLOUR);
    buffer.draw_text(font, &mut cursor, &text, TextMode::Transparent);
}

/// Where mode R's text goes and what it says.
fn robot_label(e: &Entity, room: &SitRoom) -> ((i32, i32), Vec<u8>) {
    if in_base(e) {
        return (AT_BASE_LABEL, room.at_base.clone());
    }
    // `1000:02b7` prints five digits with zeros; the label starts at the third.
    let degrees = crate::editor::degrees(e.heading);
    let mut text = format!("{degrees:03}").into_bytes();
    text.push(DEGREE);
    (HEADING_LABEL, text)
}

/// Slot `0x558` (`mgavideo.dvr` 0x13f9) from row and column `[y0, x0, y1, x1]`: a level
/// or upright line covers its lower end to its higher one, not the higher one itself
/// (none if the two are the same); any other runs from its left end to its right one,
/// both drawn, one pixel a step along its longer side, the other side stepped when an
/// error from minus half the longer side has the shorter added and is no longer
/// negative. Pixels off the surface are dropped (the driver clips first).
pub fn draw_line(fb: &mut Framebuffer, [y0, x0, y1, x1]: [u16; 4], colour: u8) {
    let [y0, x0, y1, x1] = [y0, x0, y1, x1].map(i32::from);
    let width = fb.surface().width as i32;
    let mut set = |x: i32, y: i32| {
        if (0..width).contains(&x)
            && let Some(p) = fb.pixels_mut().get_mut((y * width + x) as usize)
        {
            *p = colour;
        }
    };
    if y0 == y1 {
        (x0.min(x1)..x0.max(x1)).for_each(|x| set(x, y0));
        return;
    }
    if x0 == x1 {
        (y0.min(y1)..y0.max(y1)).for_each(|y| set(x0, y));
        return;
    }
    let ((mut x, mut y), (dx, dy), step) = if x1 > x0 {
        ((x0, y0), (x1 - x0, (y1 - y0).abs()), (y1 - y0).signum())
    } else {
        ((x1, y1), (x0 - x1, (y0 - y1).abs()), (y0 - y1).signum())
    };
    let (long, short) = (dx.max(dy), dx.min(dy));
    let mut error = -(long >> 1);
    for _ in 0..=long {
        set(x, y);
        error += short;
        let side = error >= 0;
        if side {
            error -= long;
        }
        if dx > dy {
            x += 1;
            y += step * i32::from(side);
        } else {
            y += step;
            x += i32::from(side);
        }
    }
}

/// Mode C (`1000:56a7`) on the screen, whose main view is already cleared: the
/// heading and its rule, then a row for each line of the base's `log` (the sim's,
/// `xeno_sim::log`) up to the first empty one.
pub fn draw_log(
    fb: &mut Framebuffer,
    log: &[Message],
    room: &SitRoom,
    heading: &Font,
    rows: &Font,
) {
    let (x, y) = LOG_HEADING;
    let mut cursor = TextCursor::at(x, y, LOG_COLOUR);
    cursor.high_colour = LOG_HIGH_COLOUR;
    fb.draw_text(
        heading,
        &mut cursor,
        &room.log_heading,
        TextMode::Transparent,
    );
    draw_line(fb, room.log_rule, LOG_COLOUR);
    let rows_y = (LOG_TOP..).step_by(LOG_STEP as usize);
    for (m, y) in log.iter().take_while(|m| m.text != 0).zip(rows_y) {
        let name = m.name.split(|&b| b == 0).next().unwrap_or_default();
        let mut cursor = TextCursor::at(LOG_NAME_X, y, LOG_COLOUR);
        fb.draw_text(rows, &mut cursor, name, TextMode::Transparent);
        fb.draw_text(
            rows,
            &mut cursor,
            &room.time_of(m.hour, m.minute),
            TextMode::Transparent,
        );
        let mut cursor = TextCursor::at(LOG_TEXT_X, y, LOG_TEXT_COLOUR);
        fb.draw_text(
            rows,
            &mut cursor,
            room.string(m.text),
            TextMode::Transparent,
        );
    }
}

/// The scenario's base of `side`.
pub fn base_of(scenario: Option<&Scenario>, side: Side) -> Option<&Base> {
    scenario?
        .objects
        .iter()
        .flatten()
        .find(|o| o.kind == ObjectKind::Base && o.side() == side)?
        .base()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use xeno_formats::ban::SpriteHeader;
    use xeno_formats::usg::Point;
    use xeno_formats::view3d::Float;
    use xeno_sim::{Components, EntityId, Fixed, Movement};

    /// Made-up tables: dots 0x30, 0x31, 0x32; small sprites from 0x10, large from 0x40.
    pub(crate) fn room() -> SitRoom {
        SitRoom {
            robot_dots: [0x30, 0x31, 0x32],
            robot_sprites: [[0x10, 0x20, 0x30], [0x40, 0x50, 0x60]],
            zoom_prefix: b"Z".to_vec(),
            zoom_names: vec![b"0".to_vec(); 7],
            log_heading: b"LOG".to_vec(),
            log_rule: [5, 6, 5, 60],
            time: b"[00-00]".to_vec(),
            time_digits: [1, 4],
            at_base: b"A".to_vec(),
            status_icons: [0x100, 0x108, 0x110, 0x118, 0x120, 0x128, 0x130, 0x138],
            kind_icons: [[0x140, 0x148, 0x150], [0x158, 0x160, 0x168]],
            icon_lefts: [5, 14],
            empty_row: b"A".to_vec(),
            call_signs: vec![1; 64],
            mode_labels: [0x170, 0x178, 0x180, 0x188, 0x190],
            bar_widths: [0; 32],
            clock: b"mmmm dd hh:nn".to_vec(),
            months: vec![b'M'; 48],
            clock_fields: [0, 5, 8, 11],
            detail: crate::panels::tests::detail(),
            status: xeno_formats::sitroom::Status {
                pictures: [0x2a0, 0x2a8, 0x2b0],
                places: [Rect {
                    top: 0,
                    left: 0,
                    bottom: 1,
                    right: 1,
                }; 3],
                records: [0x12, 0x13, 0x14],
                captions: [Vec::new(), Vec::new(), Vec::new()],
                no_units: b"A".to_vec(),
                figures: [Vec::new(), Vec::new(), Vec::new()],
                flags: [b"A".to_vec(), b"A".to_vec()],
            },
            strings: b"\0A\0".to_vec(),
        }
    }

    /// Kinds' models as the game's (humanoid 0, scorpion 2, scout 4); a map focal
    /// factor of 1/256, so a unit is a pixel at zoom 0.
    pub(crate) fn view() -> View3d {
        View3d {
            kind_models: [0, 2, 4, 0x10, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x14],
            frame_offsets: [0; 17],
            node_types: [None; 17],
            focal: [Float {
                mantissa: 256,
                exponent: -16,
            }; 3],
            model_sizes: [0x100; 11],
            default_size: Float {
                mantissa: 0x8000,
                exponent: -8,
            },
            max_distance: 0x7d,
            box_program: Vec::new(),
            hill_program: Vec::new(),
        }
    }

    /// 3x3 raw sprites, sprite `n` all colour `n + 1`.
    pub(crate) fn sprites() -> Vec<Sprite> {
        (0..60u8)
            .map(|n| Sprite {
                header: SpriteHeader {
                    size: 15,
                    width: 3,
                    height: 3,
                    flags: 0,
                    unknown_05: 8,
                },
                pixels: vec![n + 1; 9],
                spans: Vec::new(),
                trailing_empty_span: false,
            })
            .collect()
    }

    fn entity(kind: ObjectKind, side: Side, x: i16, y: i16, state: State) -> Entity {
        Entity {
            kind,
            side,
            position: Position {
                x: Fixed::from_int(x),
                y: Fixed::from_int(y),
            },
            z: Fixed(0),
            model_height: 0,
            heading: 0x2000,
            radius: 0x0a00,
            damage: Damage::None,
            state,
        }
    }

    /// A base at (10, 10); robots at (15, 13), (5, 8) (alien, detected) and (0, 0)
    /// (alien, not detected); relays at (0, 10) (not linked) and (2, 10) (alien,
    /// linked); an obstacle at (10, 5); a hill of radius 3 at (10, 16). The humanoid
    /// heads for (20, 12), the scout for the obstacle as its goal object.
    fn world() -> (World, Flags, Vec<TerrainFeature>) {
        let robot = State::Robot(Movement {
            goal: 2,
            destination: Position {
                x: Fixed::from_int(20),
                y: Fixed::from_int(12),
            },
            ..Movement::default()
        });
        let mut world = World::default();
        let mut flags = [(0, 0); 240];
        let mut put = |slot: u16, e: Entity, f| {
            world.insert(EntityId(slot), e);
            flags[usize::from(slot)] = f;
        };
        let (h, a) = (Side::Human, Side::Alien);
        put(
            0,
            entity(ObjectKind::Base, h, 10, 10, State::default()),
            (0, 0),
        );
        put(2, entity(ObjectKind::Humanoid, h, 15, 13, robot), (0, 0));
        let target = State::Robot(Movement {
            goal: 3,
            goal_object: 7,
            ..match robot {
                State::Robot(m) => m,
                _ => unreachable!(),
            }
        });
        put(3, entity(ObjectKind::Scout, a, 5, 8, target), (0, DETECTED));
        put(4, entity(ObjectKind::Scorpion, a, 0, 0, robot), (0, 0));
        put(
            5,
            entity(ObjectKind::Relay, h, 0, 10, State::default()),
            (0, 0),
        );
        let linked = (flag::RELAY_LINKED, 0);
        put(
            6,
            entity(ObjectKind::Relay, a, 2, 10, State::default()),
            linked,
        );
        put(
            7,
            entity(ObjectKind::Obstacle, h, 10, 5, State::default()),
            (0, 0),
        );
        let hill = TerrainFeature {
            size: 0x1a,
            kind: HILL_OP,
            point_slots: 0,
            x: 10 << 16,
            y: 16 << 16,
            z: 0,
            radius: 3 << 8,
            diagonal: 0,
            height: 0,
            shape: vec![HILL_OP, 0],
        };
        (world, flags, vec![hill])
    }

    fn draw(overlays: u16, zoom: u8, selected: Option<u16>) -> Framebuffer {
        let map = MapState {
            overlays,
            zoom,
            centre: Position::default(),
            ..MapState::default()
        };
        draw_with(map, selected)
    }

    fn draw_with(map: MapState, selected: Option<u16>) -> Framebuffer {
        probed(map, selected, None).0
    }

    /// The map, and the object the probe at `probe` names.
    fn probed(
        map: MapState,
        selected: Option<u16>,
        probe: Option<(usize, usize)>,
    ) -> (Framebuffer, Option<u16>) {
        let (world, flags, terrain) = world();
        let tables = crate::mission::tests::test_assets().motion;
        let (room, view, sprites) = (room(), view(), sprites());
        let selected = selected.and_then(|s| world.get(EntityId(s)));
        let base = world.get(EntityId(0)).unwrap().position;
        let camera = camera(base, selected, &map, &view, &tables);
        let mut buffer = Framebuffer::new(xeno_render::Size::new(40, 20));
        let inputs = Inputs {
            room: &room,
            view: &view,
            sprites: &sprites,
            font: None,
        };
        let under = draw_map(
            &mut buffer,
            &world,
            &camera,
            &map,
            &terrain,
            &flags,
            selected,
            &inputs,
            [MAP_BACKGROUND, MAP_HILL],
            probe,
        );
        (buffer, under)
    }

    /// The probe names a dot drawn exactly on it, the last sprite drawn over it, and
    /// not the goal's marker nor the background.
    #[test]
    fn the_map_s_probe_names_the_object_under_the_pointer() {
        let map = |overlays| MapState {
            overlays,
            ..MapState::default()
        };
        let dots = map(Base::MAP_ROBOTS | Base::MAP_RELAYS);
        let under = |m, selected, at| probed(m, selected, Some(at)).1;
        assert_eq!(under(dots, None, (25, 7)), Some(2), "the humanoid's dot");
        assert_eq!(under(dots, None, (24, 7)), None, "next to it");
        assert_eq!(under(dots, None, (20, 15)), Some(7), "the obstacle's dot");
        let sprites = map(Base::MAP_ROBOTS | Base::MAP_RELAYS | Base::MAP_SPRITES);
        assert_eq!(under(sprites, None, (21, 11)), Some(0), "the base's sprite");
        assert_eq!(
            under(sprites, None, (24, 6)),
            Some(2),
            "the humanoid's sprite"
        );
        assert_eq!(under(sprites, None, (0, 0)), None);
        // The humanoid's destination marker (25, 11) names nothing.
        assert_eq!(under(dots, Some(2), (25, 11)), None);
    }

    #[test]
    fn the_map_shows_hills_and_objects_as_dots() {
        let b = draw(Base::MAP_ROBOTS | Base::MAP_RELAYS, 0, None);
        let at = |x, y| b.get(x, y).unwrap();
        // x runs along the columns from the middle (20, 10), y up the rows.
        assert_eq!(at(0, 0), MAP_BACKGROUND);
        assert_eq!(at(25, 7), 0x30, "the humanoid");
        assert_eq!(at(15, 12), 0x32, "the detected scout");
        assert_eq!(at(10, 10), RELAY_UNLINKED_DOT);
        assert_eq!(at(12, 10), RELAY_DOTS[1]);
        assert_eq!(at(20, 15), OBSTACLE_DOT);
        // The base's dotless sprite (0x1b8, sprite 55), centred.
        assert_eq!(
            [at(19, 9), at(21, 11), at(22, 10)],
            [56, 56, MAP_BACKGROUND]
        );
        // The hill: 3 pixels on the axes, 2 on the diagonals, around (20, 4).
        assert_eq!([at(20, 4), at(20, 2), at(18, 4)], [MAP_HILL; 3]);
        assert_eq!(at(17, 1), MAP_BACKGROUND);
        // Undetected aliens, and every robot without the overlay, are not shown.
        let colours = |b: &Framebuffer| b.pixels().iter().filter(|&&p| p == 0x31).count();
        assert_eq!(colours(&b), 0);
        let b = draw(Base::MAP_RELAYS, 0, None);
        assert_eq!(b.get(25, 7), Some(MAP_BACKGROUND));
        assert_eq!(draw(0, 0, None).get(10, 10), Some(MAP_BACKGROUND));
    }

    /// An alarm buoy's +0x16 bit 4 is the sim's, whatever the file held.
    #[test]
    fn an_alarm_buoy_s_alarm_is_the_sim_s() {
        let mut world = World::default();
        let buoy = |alarm| Entity {
            kind: ObjectKind::Other(11),
            side: Side::Human,
            position: Position::default(),
            z: Fixed(0),
            model_height: 0,
            heading: 0,
            radius: 0,
            damage: Damage::None,
            state: State::Drop(xeno_sim::drops::Drop {
                alarm,
                ..Default::default()
            }),
        };
        world.insert(EntityId(3), buoy(true));
        world.insert(EntityId(5), buoy(false));
        let f = flags(None, &world);
        assert_eq!((f[3].0, f[5].0), (flag::RELAY_LINKED, 0));
    }

    /// The editor's Ctrl+E shows the undetected alien robot, at (0, 0) here.
    #[test]
    fn revealed_aliens_are_shown_undetected() {
        let map = MapState {
            overlays: Base::MAP_ROBOTS | Base::MAP_FREE,
            ..MapState::default()
        };
        assert_eq!(draw_with(map, None).get(20, 10), Some(MAP_BACKGROUND));
        let map = MapState {
            reveal: true,
            ..map
        };
        assert_eq!(draw_with(map, None).get(20, 10), Some(0x31));
    }

    /// The colour of the synthetic sprite a handle names.
    fn colour(handle: u16) -> u8 {
        (handle / HANDLE_STEP + 1) as u8
    }

    #[test]
    fn the_map_shows_sprites_by_heading_and_zoom() {
        let all = Base::MAP_ROBOTS | Base::MAP_RELAYS | Base::MAP_SPRITES;
        let b = draw(all, 0, None);
        let at = |b: &Framebuffer, x, y| b.get(x, y).unwrap();
        // Heading 0x2000 is the second octant: the humanoid's 0x10 + 8.
        assert_eq!(at(&b, 24, 6), colour(0x18));
        // An alien robot's sprite has a dot in the middle (the scout's 0x30 + 8).
        assert_eq!([at(&b, 14, 11), at(&b, 15, 12)], [colour(0x38), ALIEN_DOT]);
        assert_eq!(at(&b, 12, 10), colour(RELAY_SPRITES[1]));
        assert_eq!(at(&b, 20, 10), colour(BASE_SPRITES[0]));
        // At zoom 3, a factor of 8, the large sprites; centred on the selected unit.
        let b = draw(all, 3, Some(2));
        assert_eq!(at(&b, 20, 10), colour(0x48));
    }

    #[test]
    fn the_selected_unit_s_destination_is_marked() {
        let b = draw(Base::MAP_ROBOTS, 0, Some(2));
        // The camera follows the humanoid at (15, 13); its destination (20, 12) is a
        // row down and five columns right.
        assert_eq!(b.get(25, 11), Some(colour(MARKER_SPRITE)));
        assert_eq!(b.get(20, 10), Some(0x30));
        // A robot after a goal object (bit 0) has its marker on the object: the scout
        // at (5, 8) after the obstacle at (10, 5), not on its destination 4 rows up
        // and 15 columns right (the obstacle's own dot is over the marker's middle).
        let b = draw(Base::MAP_ROBOTS, 0, Some(3));
        assert_eq!(b.get(35, 6), Some(MAP_BACKGROUND));
        assert_eq!(b.get(24, 13), Some(colour(OBJECT_MARKER_SPRITE)));
        // With the map free the camera stays on the centre, (0, 0) here.
        let b = draw(Base::MAP_ROBOTS | Base::MAP_FREE, 0, Some(2));
        assert_eq!(b.get(20, 10), Some(MAP_BACKGROUND));
    }

    /// `pointer_to_world` names the point drawn under the pointer: x to the right, y
    /// up, at the zoom's pixels a unit.
    #[test]
    fn a_pointer_on_the_map_names_the_point_drawn_under_it() {
        let tables = crate::mission::tests::test_assets().motion;
        let centre = Position {
            x: Fixed::from_int(3),
            y: Fixed::from_int(-4),
        };
        let map = MapState {
            overlays: Base::MAP_FREE,
            zoom: 2,
            centre,
            ..MapState::default()
        };
        let camera = camera(Position::default(), None, &map, &view(), &tables);
        let rect = Rect {
            top: 20,
            left: 30,
            bottom: 60,
            right: 110,
        };
        let buffer = Framebuffer::new(xeno_render::Size::new(80, 40));
        // The middle (70, 40) is the centre; 4 pixels right is a unit up x, 4 up y.
        let at = |px, py| pointer_to_world(&camera, rect, (px, py));
        assert_eq!(at(70, 40), centre);
        assert_eq!(at(74, 40).x, Fixed::from_int(4));
        assert_eq!(at(70, 36).y, Fixed::from_int(-3));
        for (x, y) in [(3, -4), (10, 0), (-2, -9)] {
            let (c, r) = pixel(&camera, &buffer, x << 16, y << 16);
            let p = at((c + 30) as u16, (r + 20) as u16);
            assert_eq!((p.x.0, p.y.0), (x << 16, y << 16));
        }
    }

    #[test]
    fn lines_cover_their_ends_as_slot_0x558_does() {
        let mut fb = Framebuffer::new(xeno_render::Size::new(8, 8));
        let lit = |fb: &Framebuffer| {
            let mut at: Vec<_> = (0..8)
                .flat_map(|y| (0..8).map(move |x| (x, y)))
                .filter(|&(x, y)| fb.get(x, y) == Some(1))
                .collect();
            at.sort_by_key(|&(x, y)| (y, x));
            at
        };
        // Upright, bottom to top: the lower row up to, not with, the higher one.
        draw_line(&mut fb, [4, 2, 1, 2], 1);
        assert_eq!(lit(&fb), [(2, 1), (2, 2), (2, 3)]);
        // Steep, from the right end: from the left one, both ends drawn.
        let mut fb = Framebuffer::new(xeno_render::Size::new(8, 8));
        draw_line(&mut fb, [5, 3, 0, 2], 1);
        assert_eq!(lit(&fb), [(2, 0), (2, 1), (3, 2), (3, 3), (3, 4), (3, 5)]);
        // The same ends: nothing.
        let mut fb = Framebuffer::new(xeno_render::Size::new(8, 8));
        draw_line(&mut fb, [3, 3, 3, 3], 1);
        assert!(lit(&fb).is_empty());
    }

    #[test]
    fn the_log_lists_messages_to_the_first_empty_slot() {
        let font = xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block())
            .unwrap()
            .remove(0);
        let mut base =
            match xeno_formats::usg::parse(&xeno_formats::fixtures::synthetic_mission_usg())
                .unwrap()
                .objects
                .unwrap()[0]
                .body
                .clone()
            {
                xeno_formats::usg::Body::Base(b) => b,
                _ => unreachable!(),
            };
        let message = |text| Message {
            name: *b"A\0\0\0\0\0\0",
            hour: 9,
            minute: 5,
            second: 0,
            tick: 0,
            text,
        };
        base.messages = [
            message(1),
            message(1),
            message(0),
            message(1),
            message(0),
            message(0),
            message(0),
        ];
        base.map_centre = Point::default();
        let mut fb = Framebuffer::new(xeno_render::Size::MODE_13H);
        draw_log(&mut fb, &base.messages, &room(), &font, &font);
        let rows: Vec<usize> = (0..200)
            .filter(|&y| (0x9c..320).any(|x| fb.get(x, y) == Some(LOG_TEXT_COLOUR)))
            .collect();
        // Two rows of text, from y 0x28 every 12; the third slot ends the log.
        assert!(
            rows.iter().all(|&y| (0x28..0x28 + 24).contains(&y)),
            "{rows:?}"
        );
        assert!(rows.iter().any(|&y| y >= 0x34));
        assert_eq!(fb.get(6, 5), Some(LOG_COLOUR), "the rule");
        assert_eq!(fb.get(59, 5), Some(LOG_COLOUR));
        assert_eq!(fb.get(60, 5), Some(0), "not the rule's right end");
        assert_eq!(fb.get(61, 5), Some(0));
    }

    #[test]
    fn the_robot_view_is_labelled_or_cleared_at_its_base() {
        let font = xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block())
            .unwrap()
            .remove(0);
        let (room, view, sprites) = (room(), view(), sprites());
        let inputs = Inputs {
            room: &room,
            view: &view,
            sprites: &sprites,
            font: Some(&font),
        };
        let mut e = entity(ObjectKind::Humanoid, Side::Human, 0, 0, State::default());
        // 0x2000 is 45 degrees.
        assert_eq!(robot_label(&e, &room), (HEADING_LABEL, b"045\x17".to_vec()));
        let mut buffer = Framebuffer::new(xeno_render::Size::new(200, 104));
        buffer.clear(9);
        label_robot_view(&mut buffer, &e, &inputs);
        assert_eq!(buffer.get(0, 0), Some(9));
        e.damage = Damage::Robot(Components {
            helm: 100,
            power: 100,
            weapons: 0,
            targeting: 0,
            systems: 0,
            equipment: IN_BASE,
            total: 0,
        });
        assert_eq!(robot_label(&e, &room), (AT_BASE_LABEL, b"A".to_vec()));
        label_robot_view(&mut buffer, &e, &inputs);
        assert_eq!(buffer.get(0, 0), Some(0));
        assert!(buffer.pixels().contains(&LABEL_COLOUR));
    }

    pub(crate) fn roster_font() -> Font {
        xeno_formats::font::parse(&xeno_formats::fixtures::synthetic_font_block())
            .unwrap()
            .remove(0)
    }

    #[test]
    fn roster_rows_show_call_signs_and_icons() {
        let (room, sprites) = (room(), sprites());
        let unit = |selected, in_base| RosterUnit {
            call_sign: 0,
            repair: 0x25,
            kind: 1,
            in_base,
            selected,
        };
        let rows = [Some(unit(false, false)), Some(unit(true, true)), None];
        let mut fb = Framebuffer::new(xeno_render::Size::MODE_13H);
        let sprite = |h: u16| sprites.get(usize::from(h / HANDLE_STEP));
        draw_roster(&mut fb, &rows, &room, &roster_font(), sprite);
        let at = |x, y| fb.get(x, y).unwrap();
        // The status icon by bits 4 to 6 of +0xf9 (2: 0x110), the kind icon in the
        // field (0x148) and at the base (0x160), each row 11 pixels down.
        assert_eq!([at(5, 0x10), at(14, 0x10)], [colour(0x110), colour(0x148)]);
        assert_eq!(at(14, 0x1b), colour(0x160));
        // The call sign's first glyph (`A`, a bar) at (0x18, 0x12) in 0x0f on 0x42,
        // the selected row's in 0x0c.
        assert_eq!(
            [at(0x18, 0x12), at(0x19, 0x13)],
            [ROSTER_COLOUR, ROSTER_BACKGROUND]
        );
        assert_eq!(at(0x18, 0x12 + 11), ROSTER_SELECTED);
        // An empty row: its text at x 6, no icons (the blank one is not in the bank).
        assert_eq!(at(6, 0x12 + 22), ROSTER_COLOUR);
        assert_eq!(at(5, 0x10 + 22), 0);
    }

    #[test]
    fn map_controls_are_live_in_mode_m_only() {
        use xeno_formats::maproom::Rect;
        let room = room();
        // A 3x3 sprite for every handle up to 0x400, coloured by its number.
        let bank: Vec<Sprite> = (0..0x80u16)
            .map(|n| Sprite {
                pixels: vec![n as u8; 9],
                ..sprites()[0].clone()
            })
            .collect();
        let widget = |kind, state, top: u16, left: u16| Widget {
            kind,
            state,
            area: Some(Rect {
                top,
                left,
                bottom: top + 8,
                right: left + 8,
            }),
        };
        let widgets = [
            widget(ZOOM_TYPE, 0, 0, 0),
            widget(0x66, 1, 0, 10),
            widget(0x68, 2, 0, 20),
            widget(MODE_BUTTON_TYPE, 3, 0, 30),
            widget(MODE_BUTTON_TYPE, 6, 0, 40),
        ];
        let map = MapState {
            overlays: Base::MAP_ROBOTS,
            ..MapState::default()
        };
        let sprite = |h: u16| bank.get(usize::from(h / HANDLE_STEP));
        let draw = |mode, pressed| {
            let mut fb = Framebuffer::new(xeno_render::Size::new(60, 20));
            let modes = |w: &Widget| Some(if w.state == 3 { 4 } else { 8 });
            draw_controls(&mut fb, &widgets, modes, &map, mode, pressed, &room, sprite);
            fb
        };
        let c = |h: u16| (h / HANDLE_STEP) as u8;
        let b = draw(MAP_MODE, true);
        assert_eq!(b.get(1, 1), Some(c(0x308)), "the zoom, pressed");
        assert_eq!(b.get(0, 0), Some(0), "a pixel in from the corner");
        assert_eq!(b.get(11, 1), Some(c(0x338)), "UNITS on");
        assert_eq!(b.get(21, 1), Some(c(0x318)), "icons off");
        // A mode button: its label at (+0xe, +2), the next sprite for the current
        // mode (M); then its frame at the corner, pressed.
        assert_eq!(b.get(30 + 14, 2), Some(c(0x180 + 8)));
        assert_eq!(b.get(40 + 14, 2), Some(c(0x190)));
        assert_eq!(b.get(30, 0), Some(c(0x398)));
        let b = draw(8, false);
        assert_eq!(
            [b.get(1, 1), b.get(11, 1)],
            [Some(c(0x310)), Some(c(0x340))]
        );
        assert_eq!(b.get(40 + 14, 2), Some(c(0x190 + 8)));
    }
}
