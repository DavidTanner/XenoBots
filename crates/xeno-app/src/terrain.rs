//! The terrain editor (`mapdvr.dvr`, the scenario editor's Ctrl+M; `docs/screens.md`,
//! "The scenario editor (T-048)", T-109): a full-screen map of its own (view mode 0xe)
//! over the mission's objects, with circles that become hills. The keys come from the
//! user's driver (`xeno_formats::mapdvr`) and are known here by their handlers'
//! offsets; the colours, sprite and sizes below are short code constants.

use xeno_formats::map::{self, Circle};
use xeno_formats::usg::{Terrain, TerrainFeature};
use xeno_formats::view3d::View3d;
use xeno_render::Framebuffer;
use xeno_render::view3d::{Camera, focal_shifted};
use xeno_sim::motion::Tables;
use xeno_sim::terrain::approx_distance;
use xeno_sim::{Entity, Fixed, Position, World};

use crate::mainview::{self, Flags, Inputs, MapState};
use crate::scene::trig;

/// The handlers in the driver's key table (`DS:0x186`), by their offset in its code.
mod handler {
    /// Up, Down, Left, Right: the camera's (x, y) moved by this many units.
    pub const CAMERA: [(u16, i32, i32); 4] =
        [(0x3a5, 0, -1), (0x3c1, 0, 1), (0x3dd, 1, 0), (0x3f9, -1, 0)];
    /// Ctrl+Up, Down, Left, Right: the selected circle's.
    pub const CIRCLE: [(u16, i32, i32); 4] =
        [(0x415, 0, 1), (0x440, 0, -1), (0x46b, -1, 0), (0x496, 1, 0)];
    pub const ZOOM_IN: u16 = 0x356;
    pub const ZOOM_OUT: u16 = 0x37e;
    pub const GROW: u16 = 0x4c1;
    pub const SHRINK: u16 = 0x4ec;
    pub const PREVIOUS: u16 = 0x517;
    pub const NEXT: u16 = 0x53d;
    pub const POINTER: u16 = 0x348;
    pub const LOAD: u16 = 0x66f;
    pub const SAVE: u16 = 0x705;
    pub const CLEAR: u16 = 0x694;
    pub const IMPORT: u16 = 0x6a3;
    pub const APPLY: u16 = 0x6b5;
    pub const LEAVE: u16 = 0x6cf;
    pub const SCREENSHOT: u16 = 0x6f1;
}

/// One unit, 16.16.
const UNIT: i32 = 0x1_0000;
/// The circles the buffer holds: 0x1000 bytes of 12-byte circles (`0x0839`).
const CAPACITY: usize = 0x1000 / 12;
/// The zoom factor on entering and the largest Z gives (`0x006e`, `0x0356`).
const START_ZOOM: u8 = 2;
const MAX_ZOOM: u8 = 0x80;
/// The largest radius a drag gives (`0x05e4`): under 256 units.
const MAX_RADIUS: i32 = 0xff_ffff;
/// The view's clear colour (`DS:0x1457`), the circles' (`0x022a`), the origin's
/// sprite (`0x01f5`, in `msprite0`) and the pointer's shape (`0x006e`).
pub const CLEAR_COLOUR: u8 = 0x1f;
const SELECTED_COLOUR: u8 = 0x0e;
const CIRCLE_COLOUR: u8 = 0x0c;
const ORIGIN_SPRITE: u16 = 0x1a0;
const POINTER_SHAPE: u8 = 7;
/// A mouse event's key word's high byte for a plain and a Ctrl left press (`0x0563`).
const PLAIN: u8 = 0x01;
const CTRL: u8 = 0x11;

/// What a key asks of the mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Alt+E: the circles become the mission's terrain (`0x06b5`).
    Apply,
    /// Alt+Q: save `BACKUP.MAP` and leave (`0x06cf`).
    Leave,
    /// Alt+L: the open dialog, then the file as the terrain and its circles imported
    /// (`0x066f`).
    Load,
    /// Alt+S: the SAVE GAME dialog, then the circles written there (`0x0705`).
    Save,
    /// A key the port does not run yet, with what it does.
    Later(&'static str),
}

/// The editor's state (`DS:0xfa`..`0x131` and its circle buffer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainEditor {
    camera: (i32, i32),
    zoom: u8,
    shift: u8,
    circles: Vec<Circle>,
    /// The selected slot: only its lower bound is checked, so it can be past the end.
    selected: usize,
    /// The ground point under the pointer (`DS:0x102`) and a drag's centre (`0x10a`).
    pointer: (i32, i32),
    drag: Option<(i32, i32)>,
    shape: u8,
}

fn moved((x, y): (i32, i32), dx: i32, dy: i32) -> (i32, i32) {
    (
        x.wrapping_add(dx.wrapping_mul(UNIT)),
        y.wrapping_add(dy.wrapping_mul(UNIT)),
    )
}

fn point(p: Position) -> (i32, i32) {
    (p.x.0, p.y.0)
}

impl TerrainEditor {
    /// Entering (`ui_enter_map_editor`, `0x006e`): the camera on the map origin, zoom
    /// 2, no circles, the first slot selected, pointer shape 7.
    pub fn enter() -> TerrainEditor {
        TerrainEditor {
            camera: (map::ORIGIN.x, map::ORIGIN.y),
            zoom: START_ZOOM,
            shift: 0,
            circles: Vec::new(),
            selected: 0,
            pointer: (0, 0),
            drag: None,
            shape: POINTER_SHAPE,
        }
    }

    pub fn pointer_shape(&self) -> u8 {
        self.shape
    }

    /// Runs the key's `handler`; `terrain` is the mission's, for Alt+I.
    pub fn key(&mut self, handler: u16, terrain: &[TerrainFeature]) -> Effect {
        if let Some(&(_, dx, dy)) = handler::CAMERA.iter().find(|k| k.0 == handler) {
            self.camera = moved(self.camera, dx, dy);
            return Effect::None;
        }
        if let Some(&(_, dx, dy)) = handler::CIRCLE.iter().find(|k| k.0 == handler) {
            if let Some(c) = self.circles.get_mut(self.selected) {
                (c.x, c.y) = moved((c.x, c.y), dx, dy);
            }
            return Effect::None;
        }
        match handler {
            handler::ZOOM_IN => match self.shift.checked_sub(1) {
                Some(shift) => self.shift = shift,
                None => self.zoom = self.zoom.checked_mul(2).unwrap_or(MAX_ZOOM),
            },
            handler::ZOOM_OUT => match self.zoom >> 1 {
                0 => self.shift = self.shift.saturating_add(1),
                zoom => self.zoom = zoom,
            },
            handler::GROW | handler::SHRINK => {
                let step = if handler == handler::GROW {
                    UNIT
                } else {
                    -UNIT
                };
                if let Some(c) = self.circles.get_mut(self.selected) {
                    c.radius = c.radius.wrapping_add(step);
                }
            }
            handler::PREVIOUS => self.selected = self.selected.saturating_sub(1),
            handler::NEXT => self.selected = self.selected.saturating_add(1),
            handler::POINTER => self.shape = self.shape.wrapping_add(1),
            handler::CLEAR => self.import(&[]),
            handler::IMPORT => self.import(terrain),
            handler::APPLY => return Effect::Apply,
            handler::LEAVE => return Effect::Leave,
            handler::LOAD => return Effect::Load,
            handler::SAVE => return Effect::Save,
            handler::SCREENSHOT => return Effect::Later("takes a screenshot"),
            _ => log::debug!("terrain editor: handler {handler:#06x} not known"),
        }
        Effect::None
    }

    /// The circles cleared and imported from `terrain` (Alt+N with none, Alt+I, and
    /// Alt+L after its load).
    pub fn import(&mut self, terrain: &[TerrainFeature]) {
        self.circles = map::import_circles(terrain);
        self.circles.truncate(CAPACITY);
        self.selected = self.circles.len().saturating_sub(1);
    }

    /// A left press at the ground point `at` with the event's key word's high byte
    /// `modifiers` (`ui_map_editor_on_mouse`, `0x0563`): plain, the camera moves
    /// there; with Ctrl, a circle's drag starts there.
    pub fn press(&mut self, at: Position, modifiers: u8) {
        match modifiers {
            PLAIN => self.camera = point(at),
            CTRL => self.drag = Some(point(at)),
            _ => {}
        }
        self.pointer = point(at);
    }

    /// The pointer moved over the ground point `at`.
    pub fn hover(&mut self, at: Position) {
        self.pointer = point(at);
    }

    /// A left release (`ui_map_editor_on_drag`, `0x05af`): the drag's circle is added
    /// with the radius from its centre to the last point the pointer moved over, and
    /// selected. Returns false when the buffer is full (the original beeps twice).
    pub fn release(&mut self) -> bool {
        let Some((x, y)) = self.drag.take() else {
            return true;
        };
        if self.circles.len() >= CAPACITY {
            return false;
        }
        let radius = self.drag_radius((x, y)).min(MAX_RADIUS);
        self.selected = self.circles.len();
        self.circles.push(Circle { x, y, radius });
        true
    }

    fn drag_radius(&self, (x, y): (i32, i32)) -> i32 {
        let at = |(x, y)| Position {
            x: Fixed(x),
            y: Fixed(y),
        };
        approx_distance(at((x, y)), at(self.pointer)).0
    }

    /// What Alt+E loads and Alt+Q writes: the circles as hills, then the closing block
    /// (`file_write_map`).
    pub fn saved(&self) -> Terrain {
        map::save_circles(&self.circles)
    }

    /// The view's camera (`gfx_start_map_editor_frame`, `0x0164`): heading 0, the zoom
    /// and shift with the maps' focal factor, no viewer.
    pub fn camera(&self, view: &View3d, tables: &Tables) -> Camera {
        Camera {
            x: self.camera.0,
            y: self.camera.1,
            z: 0,
            heading: 0,
            sin: trig(tables, 0),
            cos: trig(tables, 0x4000),
            focal: focal_shifted(view.focal[2], self.zoom, self.shift),
            viewer: u16::MAX,
        }
    }

    /// The view (mode 0xe): cleared, the grid, the circles (the selected one in its own colour), the drag's,
    /// the origin's sprite, then the mission's objects as the situation room's map
    /// shows them, with no zoom label.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        buffer: &mut Framebuffer,
        camera: &Camera,
        world: &World,
        map: &MapState,
        flags: &Flags,
        selected: Option<&Entity>,
        inputs: &Inputs,
    ) {
        buffer.clear(CLEAR_COLOUR);
        mainview::draw_grid(buffer, camera);
        for (i, c) in self.circles.iter().enumerate() {
            let colour = if i == self.selected {
                SELECTED_COLOUR
            } else {
                CIRCLE_COLOUR
            };
            mainview::octagon(buffer, camera, (c.x, c.y), c.radius, colour);
        }
        if let Some(centre) = self.drag {
            let r = self.drag_radius(centre);
            mainview::octagon(buffer, camera, centre, r, SELECTED_COLOUR);
        }
        let origin = mainview::pixel(camera, buffer, map::ORIGIN.x, map::ORIGIN.y);
        mainview::mark(buffer, origin, inputs.sprite(ORIGIN_SPRITE), 0);
        let inputs = Inputs {
            font: None,
            ..*inputs
        };
        let probe = &mut mainview::Probe::default();
        mainview::draw_objects(buffer, world, camera, map, flags, selected, &inputs, probe);
    }

    /// The status the original prints on its debug screen (`0x086b`).
    pub fn status(&self) -> String {
        format!(
            "pointer {:#010x} {:#010x}, camera {:#010x} {:#010x}, {} circles",
            self.pointer.0,
            self.pointer.1,
            self.camera.0,
            self.camera.1,
            self.circles.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> Position {
        Position {
            x: Fixed(x << 16),
            y: Fixed(y << 16),
        }
    }

    #[test]
    fn a_ctrl_drag_makes_a_circle_and_the_keys_move_it() {
        let mut e = TerrainEditor::enter();
        assert_eq!((e.camera, e.zoom, e.shift), ((128 << 16, 128 << 16), 2, 0));
        // A plain press moves the camera; other modifiers do nothing.
        e.press(at(100, 90), PLAIN);
        assert_eq!(e.camera, (100 << 16, 90 << 16));
        e.press(at(1, 1), 0x09);
        assert_eq!((e.camera, e.drag), ((100 << 16, 90 << 16), None));
        // Ctrl: the drag's radius is to the last point moved over (4 + 3 / 2).
        e.press(at(100, 100), CTRL);
        e.hover(at(103, 104));
        assert!(e.release());
        let c = Circle {
            x: 100 << 16,
            y: 100 << 16,
            radius: 0x5_8000,
        };
        assert_eq!((e.circles.as_slice(), e.selected), (&[c][..], 0));
        // A release with no drag adds nothing.
        assert!(e.release());
        assert_eq!(e.circles.len(), 1);
        // Ctrl+Up moves it up a unit, Ctrl+Z grows it; Up moves the camera down.
        e.key(0x415, &[]);
        e.key(handler::GROW, &[]);
        e.key(0x3a5, &[]);
        assert_eq!((e.circles[0].y, e.circles[0].radius), (101 << 16, 0x6_8000));
        assert_eq!(e.camera.1, 89 << 16);
        // Ctrl+. walks past the last circle; the keys then change nothing.
        e.key(handler::NEXT, &[]);
        e.key(handler::GROW, &[]);
        assert_eq!((e.selected, e.circles[0].radius), (1, 0x6_8000));
        assert_eq!(e.key(handler::LEAVE, &[]), Effect::Leave);
        let saved = e.saved();
        assert_eq!(saved.features.len(), 2);
        assert_eq!(
            (saved.features[0].x, saved.features[0].radius),
            (-28 << 16, 0x680)
        );
    }

    #[test]
    fn zoom_doubles_then_shifts_and_import_takes_the_hills() {
        let mut e = TerrainEditor::enter();
        for _ in 0..8 {
            e.key(handler::ZOOM_IN, &[]);
        }
        assert_eq!((e.zoom, e.shift), (MAX_ZOOM, 0));
        for _ in 0..9 {
            e.key(handler::ZOOM_OUT, &[]);
        }
        assert_eq!((e.zoom, e.shift), (1, 2));
        e.key(handler::ZOOM_IN, &[]);
        assert_eq!((e.zoom, e.shift), (1, 1));
        let hills = map::to_world(&map::editor_save(&[])).features;
        let mut hill = hills[0].clone();
        (hill.kind, hill.radius) = (TerrainFeature::HILL, 0x380);
        e.key(handler::IMPORT, &[hill.clone(), hills[0].clone()]);
        assert_eq!((e.circles.len(), e.circles[0].radius), (1, 0x3_8000));
        e.key(handler::CLEAR, &[]);
        assert!(e.circles.is_empty());
    }

    /// The circles in their colours, the selected one in its own, the origin's sprite
    /// (handle 0x1a0, sprite 52, colour 53) in the middle at the start, nothing else.
    #[test]
    fn the_view_shows_circles_and_the_origin() {
        use crate::mainview::tests::{room, sprites, view};
        let mut e = TerrainEditor::enter();
        for x in [133, 123] {
            e.press(at(x, 128), CTRL);
            e.hover(at(x + 1, 128));
            e.release();
        }
        let (room, view, sprites) = (room(), view(), sprites());
        let inputs = Inputs {
            room: &room,
            view: &view,
            sprites: &sprites,
            font: None,
        };
        let tables = crate::mission::tests::test_assets().motion;
        let camera = e.camera(&view, &tables);
        let mut b = Framebuffer::new(xeno_render::Size::new(40, 20));
        let world = World::default();
        let map = MapState::default();
        e.draw(&mut b, &camera, &world, &map, &[(0, 0); 240], None, &inputs);
        // Zoom 2: two pixels a unit, x along the columns from the middle.
        let at = |x, y| b.get(x, y).unwrap();
        assert_eq!([at(30, 10), at(10, 10), at(20, 10)], [0x0c, 0x0e, 53]);
        assert_eq!([at(0, 0), at(30, 13)], [CLEAR_COLOUR; 2]);
    }

    #[test]
    fn a_full_buffer_takes_no_more() {
        let mut e = TerrainEditor::enter();
        e.circles = vec![Circle::default(); CAPACITY];
        e.press(at(0, 0), CTRL);
        assert!(!e.release());
        assert_eq!(e.circles.len(), 341);
    }
}
