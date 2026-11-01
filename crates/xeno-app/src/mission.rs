//! A mission's views (T-043): the situation room, the view of the player's base, and
//! the console of each robot, as `ui_open_view_of_object` and their widgets run them
//! (`docs/screens.md`, "The mission's views", "Orders"). A console's main view is the
//! viewed robot's 3D view (T-101, [`crate::scene`]), and the situation room's main
//! view draws its map, the selected unit's view and the message log in modes M, R and
//! C (T-103, [`crate::mainview`]); what else the running mission
//! would draw (scanners, instruments, the roster's contents) is a labelled
//! placeholder; switching views and setting
//! orders work. So does the briefing (T-044): the scenario's `ORDR` text, typed into the
//! situation room's main view in its orders mode (O), as `ui_sitroom_type_orders_text`
//! types it (`docs/screens.md`, "The briefing (T-044)").
//!
//! The mission's state is a [`xeno_sim::Sim`] loaded from its `.usg` file
//! (`docs/sim.md`, "Loading a scenario"): its world holds the objects and the robots'
//! orders, which the views change only through the sim, so they persist across views.
//! The sim has the run's random word while the mission runs, and gives it back to the
//! campaign when the mission stops (`docs/sim.md`, "The run's random word"). The
//! widget sets, key tables, sprite buttons, program names and panel styles all come
//! from the user's `xeno.exe`, `msprite1.b@n` and `mgavideo.dvr`; what a key does is
//! chosen by the original handler it calls.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xeno_formats::ban::Sky;
use xeno_formats::ban::{self, Sprite};
use xeno_formats::filedialog::FileDialogs;
use xeno_formats::font::Font;
use xeno_formats::humancon::HumanCon;
use xeno_formats::maproom::{HANDLE_STEP, Rect};
use xeno_formats::orders;
use xeno_formats::scorpcon::ScorpCon;
use xeno_formats::scoutcon::ScoutCon;
use xeno_formats::sitroom::SitRoom;
use xeno_formats::usg::{self, Scenario, Side};
use xeno_formats::view3d::{self, View3d};
use xeno_formats::views::{
    self, CONSOLE_CORNER_TYPE, MESSAGE_PANEL_TYPE, MissionViews, PROGRAM_PANEL_TYPE,
    SITUATION_CORNER_TYPE, TrainingAction, TrainingMenu, UNIT_BUTTON_TYPE, View as ViewTables,
};
use xeno_formats::{
    editor, filedialog, humancon, lzexe, map, mapdvr, mgavideo, motion, scorpcon, scoutcon, sitroom,
};
use xeno_render::{BlitMode, Framebuffer, LoadedModel, Size, SpriteBlit, TextCursor, TextMode};
use xeno_sim::orders::{Change, OrderBook, Programs, RobotKind};
use xeno_sim::{EntityId, Parked, Pilot, Random, Sim, State, World};

use crate::dialog::{self, FileDialog, Outcome};
use crate::editor::{Editor, Effect};
use crate::game_dir::GameFiles;
use crate::input::{InputEvent, MouseButton};
use crate::mainview::{self, MapState};
use crate::maproom::{Campaign, scenario_file};
use crate::panels;
use crate::pointer::{ARROW, CORNER};
use crate::screens::View;
use crate::status;
use crate::terrain::{self, TerrainEditor};
use crate::{review, scene};

/// Original handlers (offsets in `xeno.exe`'s segment `1000`) whose keys the port acts
/// on; the key tables themselves are read from the user's copy.
mod handler {
    use xeno_sim::Pilot;

    /// Consoles: the side's base (ESC, Alt+Q).
    pub const BASE: u16 = 0x7ee7;
    pub const TO_PILOT: u16 = 0xd446;
    pub const TO_PROGRAM: u16 = 0xd42c;
    pub const PILOT_NEXT: u16 = 0xd3a2;
    pub const NEXT_UNIT: u16 = 0x7db2;
    pub const PREVIOUS_UNIT: u16 = 0x7dd9;
    /// Situation room: ends the mission on Alt+Q only (ESC shares it).
    pub const END: u16 = 0x7cf2;
    pub const VIEW_SELECTED: u16 = 0x6852;
    pub const NEXT_SELECTED: u16 = 0x67e8;
    pub const PREVIOUS_SELECTED: u16 = 0x680c;
    /// Situation room: Ctrl+Alt+P, save the mission as a `.usg` (no unlock needed).
    pub const SAVE: u16 = 0xefd5;
    /// Situation room and consoles: F1 pauses under a box until a key or button press
    /// (`1000:cff3`); Ctrl+Alt+S flips the shots on the map (`DS:0x62ec` bit 6,
    /// `1000:d177`); Ctrl+P (scorpion and scout), with unlock word 8, charges the viewed
    /// robot (`1000:d193`).
    pub const PAUSE: u16 = 0xcff3;
    pub const SHOTS: u16 = 0xd177;
    pub const CHARGE: u16 = 0xd193;
    /// Situation room, mode M only: = and keypad + zoom in, - and keypad - out; U, I,
    /// B, N and G flip the map's overlay bits 0 to 4.
    pub const ZOOM_IN: u16 = 0x5cc7;
    pub const ZOOM_OUT: u16 = 0x5cd7;
    pub const OVERLAYS: [(u16, u16); 5] = [
        (0x5daa, 0x01),
        (0x5d3e, 0x02),
        (0x5e82, 0x04),
        (0x5eee, 0x08),
        (0x5e16, 0x10),
    ];
    /// Situation room: the main view's modes, by key (O, S, M, R, C), and the mode each
    /// sets (`ui_sitroom_set_view_mode`).
    pub const MODES: [(u16, u8); 5] = [
        (0x67d0, 0),
        (0x67ca, 2),
        (0x67d6, 4),
        (0x67dc, 6),
        (0x67e2, 8),
    ];
    /// Consoles: V, T and M set the display mode (+0xef) of each kind
    /// (`ui_set_humanoid_display_mode`, the scorpion's `99c7`, the scout's `cbc1`);
    /// = and keypad +, - and keypad - step the display's zoom (`1000:da0e`, `da37`).
    pub const DISPLAY_MODES: [(u16, u8); 9] = [
        (0xb415, 0),
        (0xb41b, 1),
        (0xb421, 2),
        (0x9c5a, 0),
        (0x9c60, 1),
        (0x9c66, 2),
        (0xcbed, 0),
        (0xcbf3, 1),
        (0xcbf9, 2),
    ];
    pub const DISPLAY_ZOOM_IN: u16 = 0xda0e;
    pub const DISPLAY_ZOOM_OUT: u16 = 0xda37;
    /// Z, X and Y on each kind's console: the pan less or plus the zoom's step, or 0
    /// (`1000:db7e`, `db8f`; `docs/screens.md`, "The display's other modes (T-134)").
    pub const DISPLAY_PANS: [(u16, Option<bool>); 9] = [
        (0xb377, Some(false)),
        (0xb389, Some(true)),
        (0xb39b, None),
        (0x9bca, Some(false)),
        (0x9bdc, Some(true)),
        (0x9bee, None),
        (0xcb4f, Some(false)),
        (0xcb61, Some(true)),
        (0xcb73, None),
    ];
    /// Situation room: F3 recalls the selected unit (`1000:675e`), F2 launches it
    /// from the base (`1000:6704`).
    pub const RECALL: u16 = 0x675e;
    pub const LAUNCH: u16 = 0x6704;
    /// Situation room: keys 1 to 6 run the power rows' slider routines 1 to 6 once
    /// (`DS:0x5b0a`'s, as an arrow held does each tick).
    pub const SLIDERS: [(u16, u8); 6] = [
        (0x6629, 1),
        (0x663a, 2),
        (0x6656, 3),
        (0x6667, 4),
        (0x6683, 5),
        (0x6694, 6),
    ];
    /// Consoles: F5, F6 and the humanoid's F9 select a weapon (`docs/sim.md`, "The
    /// cockpit keys"); the arrows (left, right, up, down) of each kind (humanoid `1000:acc0`,
    /// scorpion `9420`, scout `c5e3`), `.` (opcode 0x53's routine) and the scout's Q,
    /// W, E with the stance each sets, the scorpion's U, Space, the trigger
    /// (`1000:d0da`), A and R, the next target and none (`1000:d812`, `d7f8`;
    /// `docs/sim.md`, "The cockpit keys"), F7 and F8, the ECM and the cloak
    /// (`d92d`, `d95e`; "The ECM and the cloak (T-136)"), and the scorpion's and
    /// scout's keys 1 to 4, the drops (`d5e7`, `d609`, `d62b`, `d64d`; "Mines and
    /// buoys (T-137)").
    pub const PILOT: [(u16, Pilot); 29] = [
        (0xd131, Pilot::Weapon(0)),
        (0xd142, Pilot::Weapon(1)),
        (0xb20c, Pilot::OtherWeapon),
        (0xacc0, Pilot::Left),
        (0xaccd, Pilot::Right),
        (0xacda, Pilot::Faster),
        (0xace0, Pilot::Slower),
        (0x9420, Pilot::Left),
        (0x942d, Pilot::Right),
        (0x943a, Pilot::Faster),
        (0x9440, Pilot::Slower),
        (0xc5e3, Pilot::Left),
        (0xc5f0, Pilot::Right),
        (0xc5fd, Pilot::Faster),
        (0xc603, Pilot::Slower),
        (0xd0a0, Pilot::Stop),
        (0xc33d, Pilot::Stance(0x60)),
        (0xc2f8, Pilot::Stance(0x30)),
        (0xc2b3, Pilot::Stance(0)),
        (0x9afa, Pilot::Deploy),
        (0xd0da, Pilot::Fire),
        (0xd812, Pilot::NextTarget),
        (0xd7f8, Pilot::DropTarget),
        (0xd92d, Pilot::Ecm),
        (0xd95e, Pilot::Cloak),
        (0xd5e7, Pilot::Drop(0)),
        (0xd609, Pilot::Drop(1)),
        (0xd62b, Pilot::Drop(2)),
        (0xd64d, Pilot::Drop(3)),
    ];
}

/// The consoles' buttons whose mouse handlers the port runs: the humanoid's fire,
/// weapon, pilot, program, lock, cloak and ECM buttons, then the scorpion's first five,
/// its stop and deploy buttons, its relay panel (which only shows its frame XORed
/// while held) and its drop buttons, then the scout's first five, its three stance
/// buttons, cloak, ECM and drop buttons.
const INSTRUMENTS: [u16; 27] = [
    crate::humancon::FIRE_TYPE,
    crate::humancon::WEAPON_TYPE,
    crate::humancon::PILOT_TYPE,
    crate::humancon::PROGRAM_TYPE,
    crate::humancon::LOCK_TYPE,
    crate::humancon::CLOAK_TYPE,
    crate::humancon::ECM_TYPE,
    crate::scorpcon::FIRE_TYPE,
    crate::scorpcon::WEAPON_TYPE,
    crate::scorpcon::PILOT_TYPE,
    crate::scorpcon::PROGRAM_TYPE,
    crate::scorpcon::LOCK_TYPE,
    crate::scorpcon::STOP_TYPE,
    crate::scorpcon::DEPLOY_TYPE,
    crate::scorpcon::RELAYS_TYPE,
    crate::scorpcon::DROP_TYPE,
    crate::scoutcon::FIRE_TYPE,
    crate::scoutcon::WEAPON_TYPE,
    crate::scoutcon::PILOT_TYPE,
    crate::scoutcon::PROGRAM_TYPE,
    crate::scoutcon::LOCK_TYPE,
    crate::scoutcon::STANCE_TYPES[0],
    crate::scoutcon::STANCE_TYPES[1],
    crate::scoutcon::STANCE_TYPES[2],
    crate::scoutcon::CLOAK_TYPE,
    crate::scoutcon::ECM_TYPE,
    crate::scoutcon::DROP_TYPE,
];
const FIRE_TYPES: [u16; 3] = [
    crate::humancon::FIRE_TYPE,
    crate::scorpcon::FIRE_TYPE,
    crate::scoutcon::FIRE_TYPE,
];
const LOCK_TYPES: [u16; 3] = [
    crate::humancon::LOCK_TYPE,
    crate::scorpcon::LOCK_TYPE,
    crate::scoutcon::LOCK_TYPE,
];
/// Buttons that flip the shared look on a press and back on a release.
const SHARED_LOOK_TYPES: [u16; 11] = [
    crate::scorpcon::DROP_TYPE,
    crate::scoutcon::DROP_TYPE,
    crate::humancon::CLOAK_TYPE,
    crate::humancon::ECM_TYPE,
    crate::scoutcon::CLOAK_TYPE,
    crate::scoutcon::ECM_TYPE,
    crate::scorpcon::STOP_TYPE,
    crate::scorpcon::DEPLOY_TYPE,
    crate::scoutcon::STANCE_TYPES[0],
    crate::scoutcon::STANCE_TYPES[1],
    crate::scoutcon::STANCE_TYPES[2],
];
/// The stances the scout's stance buttons ask for (`1000:c28c`, `c2d1`, `c316`), as E,
/// W and Q.
const STANCES: [u8; 3] = [0, 0x30, 0x60];

/// A robot console's second display (+0xef to +0xf5): the display mode (0 the
/// camera, 1 thermal, 2 the map, 3 the scorpion's missile camera), the camera's and
/// the map's zoom steps, and the camera's pan (`docs/screens.md`, "The second display
/// (T-104)"); the mode to go back to after the missile camera (+0xf0), the zoom
/// name's time (+0xf3, in 16s of ticks) and the missile camera's break-up (+0x117).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Display {
    pub mode: u8,
    pub zoom: u8,
    pub map_zoom: u8,
    pub pan: u16,
    pub before: u8,
    pub label: u8,
    pub static_time: u8,
    /// The port's: whether the scorpion's tail missile was in flight at the last
    /// tick, to see its launch (`1000:97b0` sets the mode itself).
    flying: bool,
}

/// Display mode 2, the map, and bit 2: the mode's device is damaged. Mode 7 (the
/// scorpion's lost missile camera) is left as it is by `1000:db5c`.
const MAP_DISPLAY: u8 = 2;
const DAMAGED: u8 = 4;
const LOST_MISSILE_CAMERA: u8 = 7;
const MISSILE_DISPLAY: u8 = 3;
/// The zoom's name shows for this many 16-tick periods after a mode or zoom key, and
/// the missile camera breaks up for this many once its missile is gone (`1000:998a`).
const LABEL_TIME: u8 = 6;
const STATIC_TIME: u8 = 5;
/// The zoom steps go from 0 to 4 (`1000:da0e`, `da37`).
const MAX_DISPLAY_ZOOM: u8 = 4;
/// The second display's region, and its view modes: the camera (6) and the map (0xc).
const SECOND_DISPLAY: u8 = 254;
/// The third display's region (T-130 to T-132 draw it).
const THIRD_DISPLAY: u8 = 253;
/// Per robot kind, the widgets whose mouse handlers test the 3D view's, the second
/// display's and the third display's regions (humanoid `1000:9d83`, `9eea`, `9fcc`;
/// scorpion `8081`, `828e`, `8370`; scout `b540`, `b68c`, `b76e`; T-106).
const REGION_WIDGETS: [[u16; 3]; 3] = [
    [0x84, 0x86, 0xaa],
    [0xbc, 0xbe, 0xd8],
    [0x106, 0x108, 0x126],
];
const CAMERA_VIEW: u8 = 6;
const THERMAL_VIEW: u8 = 0xa;
const MISSILE_VIEW: u8 = 8;
const MAP_VIEW: u8 = 0xc;
/// The second display's clear colour, the base of its noise (`DS:0x1456`), and the
/// zoom name's font and colour (the text state `DS:0x6a52`, as T-104 read it).
const DISPLAY_CLEAR: u8 = 0x20;
const LABEL_FONT: u16 = 2;
const LABEL_COLOUR: u8 = 0x0f;
/// The sound events of a display mode key (10) and of a damaged display (28).
const DISPLAY_SOUND: u8 = 10;
const NOISE_SOUND: u8 = 28;

/// A sound event to play, or to stop (`1000:eeb1`: the event's voice given silence
/// with the event's flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Play(u8),
    Stop(u8),
}

/// The situation room's main view (widget type 0x50) and its orders mode, and how
/// `ui_sitroom_type_orders_text` types the briefing in it: font 4, colour 0x1d (0x17 for
/// codes with bit 7), from (96, 30), after clearing the view to colour 0x10. Short code
/// constants, kept here.
const MAIN_VIEW_TYPE: u16 = 0x50;
pub const ORDERS_MODE: u8 = 0;
const ORDERS_FONT: u16 = 4;
const ORDERS_COLOUR: u8 = 0x1d;
const ORDERS_HIGH_COLOUR: u8 = 0x17;
const ORDERS_CLEAR: u8 = 0x10;
const ORDERS_CURSOR: (i32, i32) = (96, 30);
/// The main view's mode 2 (S, the selected unit's status, [`status`]), whose routine
/// (`1fe6:704a`) sets `DS:0x7d4d`: no time-of-day tint while it is shown.
const STATUS_MODE: u8 = 2;
/// The main view's modes 4 (M, the map), 6 (R, the selected unit's view) and 8 (C,
/// the message log).
const MAP_MODE: u8 = 4;
const ROBOT_VIEW_MODE: u8 = 6;
const LOG_MODE: u8 = 8;
/// `mgavideo.dvr`'s blend records over colours 49..=63 and 128..=252 that
/// `pal_tint_for_time_of_day` (`1000:ed70`) starts.
pub const TINT_RECORDS: [u8; 2] = [5, 6];

/// The records of `mgavideo.dvr`'s list a view's picture starts as it is drawn and
/// turns off where they stand as it is left (`docs/formats/palette.md`, "The views'
/// records"): blinks and frame cycles of colours 23..=25 and 253..=255, rotates over
/// 16..=31. None is a blend, so no weight.
pub fn view_records(view: crate::screens::View) -> &'static [u8] {
    use crate::screens::View;
    match view {
        View::SitRoom => &[0, 1, 0x10, 0x11],
        View::Humanoid => &[0xa, 0xb, 2, 0x10, 0x11],
        View::Scorpion => &[0xd, 3, 0x10, 0x11],
        View::Scout => &[0xc, 4, 0x10, 0x11],
    }
}

/// Alt+Q, the only key word the situation room's end handler acts on.
const ALT_Q: u16 = 0x2010;
/// The situation room's sprite buttons, by widget type (`docs/screens.md`).
mod button {
    pub const SCROLL_UP: u16 = 0x54;
    pub const SCROLL_DOWN: u16 = 0x56;
    pub const VIEW: u16 = 0x58;
    pub const LAUNCH: u16 = 0x5a;
    pub const RECALL: u16 = 0x5c;
    pub const END: u16 = 0x5e;
}
/// The unit-name button's font and colour, and the XOR of its pressed frame
/// (`1000:7d55`, `ui_unit_name_button_on_mouse`); the training buttons' XOR.
const UNIT_FONT: u16 = 0;
const UNIT_COLOUR: u8 = 0;
const UNIT_PRESSED_XOR: u8 = 8;
pub const TRAINING_PRESSED_XOR: u8 = 9;
/// The colours that mark where the mission draws (`docs/formats/m@l.md`): 255 in the
/// situation room, 253 to 255 in the consoles.
const VIEW_COLOURS: [u8; 3] = [253, 254, 255];
/// A view's region in the frame: left, top, width and height (`review::region_of`).
type Region = (usize, usize, usize, usize);
/// The port's own placeholder marks (not the original's).
const PLACEHOLDER: u8 = 255;
const PLACEHOLDER_INK: u8 = 0;

/// What a mission needs from the game files.
#[derive(Debug, Clone)]
pub struct Assets {
    pub views: MissionViews,
    pub training: TrainingMenu,
    pub programs: Programs,
    /// The movement tables the mission's sim moves robots by.
    pub motion: motion::Motion,
    /// By font id.
    pub fonts: BTreeMap<u16, Font>,
    /// `msprite1`, from `views.situation_bank`.
    pub sprites: Vec<Sprite>,
    /// Every training and campaign scenario, by lower-case file name; files that are
    /// missing or do not parse are left out (and logged).
    pub scenarios: BTreeMap<String, Scenario>,
    /// The scenario editor's tables (T-048), if the user's `xeno.exe` has them where
    /// the port reads them.
    pub editor: Option<editor::Editor>,
    /// Whether the user's `XENO.KEY` gives unlock word 9 (the editor's keys).
    pub editor_unlocked: bool,
    /// Whether it gives unlock word 8 (the developer keys: Ctrl+P on a console).
    pub developer_unlocked: bool,
    /// The terrain editor's keys (T-109), if the user's `mapdvr.dvr` has them where the
    /// port reads them; without them Ctrl+M does nothing, as when the original cannot
    /// load the driver.
    pub map_driver: Option<mapdvr::MapDriver>,
    /// Where Ctrl+Alt+P and the editor save (`--save-dir`, else the game directory).
    pub save_dir: PathBuf,
    /// The 3D view's tables (T-101), if the user's `xeno.exe` has them where the port
    /// reads them; without them the consoles' views keep the placeholder.
    pub view3d: Option<View3d>,
    /// Every model, by model number / 2.
    pub models: Vec<Option<LoadedModel>>,
    /// Each scenario's sky strip, by file name; none is the flat sky.
    pub skies: BTreeMap<String, Sky>,
    /// The situation room's main view tables (T-103), if the user's `xeno.exe` has
    /// them where the port reads them; without them modes M, R and C keep the
    /// placeholder.
    pub sitroom: Option<SitRoom>,
    /// `msprite0`, the map's sprites, from handle 0.
    pub map_sprites: Vec<Sprite>,
    /// The file dialogs (T-110), if the user's `xeno.exe` has them where the port
    /// reads them; without them Ctrl+Alt+P saves under a fixed name.
    pub file_dialogs: Option<FileDialogs>,
    /// The humanoid console's instruments (T-130), if the user's `xeno.exe` has them
    /// where the port reads them, and its sprites (`msprite2`).
    pub humancon: Option<HumanCon>,
    pub humancon_sprites: Vec<Sprite>,
    /// The scorpion console's (T-131) and its sprites (`msprite3`).
    pub scorpcon: Option<ScorpCon>,
    pub scorpcon_sprites: Vec<Sprite>,
    /// The scout console's (T-132) and its sprites (`msprite4`).
    pub scoutcon: Option<ScoutCon>,
    pub scoutcon_sprites: Vec<Sprite>,
    /// The thermal and missile cameras' remap, from `mgavideo.dvr` (T-134).
    pub thermal: Vec<u8>,
}

/// What a file dialog was opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// Ctrl+Alt+P (`ui_save_scenario_with_dialog`): the scenario written there.
    SaveScenario,
    /// Ctrl+Alt+O: the file read as the orders.
    Orders,
    /// The terrain editor's Alt+L and Alt+S.
    LoadMap,
    SaveMap,
    /// The map room's and training menu's Alt+L (`ui_load_scenario_with_dialog`): the
    /// path handed to the screens ([`Mission::take_load`]), which load it.
    LoadScenario,
}

/// The most Ctrl+Alt+O reads of a file (`mov cx, 0x320` at `1000:6d8e`).
const ORDERS_FILE_MAX: usize = 800;

/// The pointer's shape while a file dialog runs (`ui_run_file_dialog`, slot `0x1496`).
pub const DIALOG_SHAPE: u8 = 0;

/// The name Ctrl+Alt+P saves under without the file dialogs, and the terrain editor's
/// leave file.
pub const SAVED_SCENARIO: &str = "EDITED.USG";
pub const SAVED_TERRAIN: &str = "BACKUP.MAP";

/// Reads the views, the training menu and the programs from the user's `xeno.exe`,
/// `msprite1.b@n`, the fonts from `mgavideo.dvr`, and the scenarios named by the
/// training menu and the campaign's `scendef.bin` records.
pub fn load(files: &GameFiles, campaign_records: usize) -> Result<Assets, String> {
    let read = |name: &str| {
        files
            .read(name)
            .map_err(|e| format!("cannot read {name}: {e}"))
    };
    let exe = lzexe::unpack(&read("xeno.exe")?).map_err(|e| format!("xeno.exe: {e}"))?;
    let no = |what: &str, e| format!("xeno.exe: no {what} this port can read: {e}");
    let views = views::read(&exe).map_err(|e| no("mission views", e))?;
    let training = views::read_training(&exe).map_err(|e| no("training menu", e))?;
    let programs = orders::read(&exe).map_err(|e| no("program table", e))?;
    let motion = motion::read(&exe).map_err(|e| no("movement tables", e))?;
    let bank = |name: &str| {
        ban::parse_sprites(&read(name)?)
            .map(|b| b.sprites)
            .map_err(|e| format!("{name}: {e}"))
    };
    let sprites = bank("msprite1.b@n")?;
    let video =
        mgavideo::parse(&read("mgavideo.dvr")?).map_err(|e| format!("mgavideo.dvr: {e}"))?;
    let mut fonts = BTreeMap::new();
    for id in views.program_panel.iter().map(|s| s.font).chain([
        UNIT_FONT,
        ORDERS_FONT,
        mainview::LOG_FONT,
        panels::PANEL_FONT,
        panels::DETAIL_FONT,
        status::FONT,
        LABEL_FONT,
    ]) {
        let font = video
            .font(id)
            .ok_or(format!("mgavideo.dvr: no font {id}"))?;
        fonts.insert(id, font.clone());
    }
    let names = training
        .actions
        .iter()
        .filter_map(|a| match a {
            TrainingAction::Mission(name) => Some(file_name(name)),
            TrainingAction::Back => None,
        })
        .chain((0..campaign_records).map(scenario_file));
    let mut scenarios = BTreeMap::new();
    let mut skies = BTreeMap::new();
    for name in names {
        match read(&name).and_then(|b| usg::parse(&b).map_err(|e| format!("{name}: {e}"))) {
            Ok(scenario) => {
                if let Some(sky) = scene::load_sky(files, &scenario) {
                    skies.insert(name.clone(), sky);
                }
                scenarios.insert(name, scenario);
            }
            Err(e) => log::warn!("mission: {e}"),
        }
    }
    let editor = editor::read(&exe)
        .inspect_err(|e| log::warn!("xeno.exe: no scenario editor tables this port can read: {e}"))
        .ok();
    Ok(Assets {
        views,
        training,
        programs,
        motion,
        fonts,
        sprites,
        scenarios,
        editor,
        editor_unlocked: false,
        developer_unlocked: false,
        map_driver: read("mapdvr.dvr")
            .and_then(|b| mapdvr::parse(&b).map_err(|e| format!("mapdvr.dvr: {e}")))
            .inspect_err(|e| log::warn!("no terrain editor: {e}"))
            .ok(),
        save_dir: files.dir().path.clone(),
        file_dialogs: filedialog::read(&exe)
            .inspect_err(|e| log::warn!("xeno.exe: no file dialogs this port can read: {e}"))
            .ok(),
        view3d: view3d::read(&exe)
            .inspect_err(|e| log::warn!("xeno.exe: no 3D view tables this port can read: {e}"))
            .ok(),
        models: scene::load_models(files),
        skies,
        sitroom: sitroom::read(&exe)
            .inspect_err(|e| {
                log::warn!("xeno.exe: no situation room tables this port can read: {e}")
            })
            .ok(),
        map_sprites: bank("msprite0.b@n")
            .inspect_err(|e| log::warn!("situation room: no map sprites: {e}"))
            .unwrap_or_default(),
        humancon: humancon::read(&exe)
            .inspect_err(|e| log::warn!("xeno.exe: no humanoid console this port can read: {e}"))
            .ok(),
        humancon_sprites: bank("msprite2.b@n")
            .inspect_err(|e| log::warn!("humanoid console: no sprites: {e}"))
            .unwrap_or_default(),
        scorpcon: scorpcon::read(&exe)
            .inspect_err(|e| log::warn!("xeno.exe: no scorpion console this port can read: {e}"))
            .ok(),
        scorpcon_sprites: bank("msprite3.b@n")
            .inspect_err(|e| log::warn!("scorpion console: no sprites: {e}"))
            .unwrap_or_default(),
        scoutcon: scoutcon::read(&exe)
            .inspect_err(|e| log::warn!("xeno.exe: no scout console this port can read: {e}"))
            .ok(),
        scoutcon_sprites: bank("msprite4.b@n")
            .inspect_err(|e| log::warn!("scout console: no sprites: {e}"))
            .unwrap_or_default(),
        thermal: video.thermal.clone(),
    })
}

/// Logs what a mission loaded: each side's units (its robots) by kind, and the rest.
fn log_census(file: &str, world: &World) {
    let side = |side: Side| {
        let c = world.census(side);
        format!(
            "{} units ({} humanoid, {} scorpion, {} scout), {} base, {} relays, {} obstacles",
            c.units(),
            c.humanoids,
            c.scorpions,
            c.scouts,
            c.bases,
            c.relays,
            c.obstacles
        )
    };
    log::info!(
        "mission: {file}, {} objects; the player's: {}, {} robots with orders; the enemy's: {}",
        world.len(),
        side(Side::Human),
        world.orders.side(Side::Human).count(),
        side(Side::Alien)
    );
}

/// A padded 8.3 name as the game writes it, as a lower-case file name.
pub fn file_name(padded: &[u8]) -> String {
    String::from_utf8_lossy(padded)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

impl Assets {
    fn sprite(&self, handle: u16) -> Option<&Sprite> {
        let offset = handle.checked_sub(self.views.situation_bank)?;
        offset
            .is_multiple_of(HANDLE_STEP)
            .then(|| self.sprites.get(usize::from(offset / HANDLE_STEP)))
            .flatten()
    }

    #[cfg(test)]
    pub fn tables_for_test(&self, view: View) -> &ViewTables {
        self.tables(view)
    }

    fn tables(&self, view: View) -> &ViewTables {
        match view {
            View::SitRoom => &self.views.situation,
            View::Humanoid => &self.views.humanoid,
            View::Scorpion => &self.views.scorpion,
            View::Scout => &self.views.scout,
        }
    }

    /// Whether the view's key table binds ESC.
    pub fn claims_escape(&self, view: View) -> bool {
        self.tables(view).keys.iter().any(|&(word, _)| word == 0x01)
    }

    /// The first training mission, in the menu's order, with a human robot of `kind`
    /// (for `--screen` with a console), or the first one for the situation room.
    pub fn sample(&self, kind: Option<RobotKind>) -> Option<(String, &Scenario)> {
        self.training.actions.iter().find_map(|a| {
            let TrainingAction::Mission(name) = a else {
                return None;
            };
            let name = file_name(name);
            let scenario = self.scenarios.get(&name)?;
            let book = OrderBook::from_scenario(scenario);
            let fits = kind.is_none_or(|k| book.side(Side::Human).any(|(_, r)| r.kind == k));
            fits.then_some((name, scenario))
        })
    }
}

/// The object a view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewed {
    Base,
    Robot(u16),
}

/// What the screens must do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Show this view (the mission's state already points at its object).
    Show(View),
    /// The mission is over, with no result (the quit flag).
    End,
    /// A destroyed base's end script is over: won or lost.
    Over { won: bool },
}

/// Where a press captured the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Panel,
    UnitButton,
    Corner,
    Roster(usize),
    Button(usize),
    /// A map control or mode button: its index among the view's nested widgets.
    Control(usize),
    /// A console's 3D view (0) or second display (1).
    Region(usize),
    /// One of the humanoid console's buttons (T-130).
    Instrument(views::Widget),
}

/// The mission's sim: running, with the run's random word, or parked without it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Run {
    Running(Sim),
    Parked(Parked),
}

impl Default for Run {
    fn default() -> Self {
        Run::Parked(Parked::default())
    }
}

/// A mission in progress: its file, its sim, and the views' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    /// The `.usg` file it was loaded from.
    pub file: String,
    run: Run,
    viewed: Viewed,
    /// Sound events heard in the view shown since the last [`Mission::take_sounds`].
    sounds: Vec<Sound>,
    /// Each robot's console display, as loaded and as its keys change it.
    displays: BTreeMap<u16, Display>,
    /// The world ticked since the displays last followed it ([`Mission::step_displays`]).
    ticked: bool,
    /// The damaged displays' random word (the original draws from the sim's; the port
    /// keeps the sim's for the sim, so a replay does not depend on what was drawn).
    noise: std::cell::RefCell<Random>,
    /// The colour record the open console's display runs (`DS:0x70d0`; none 0xff).
    display_record: Option<u8>,
    /// The open console's display mode when the noise was last checked
    /// (`DS:0x70ad`).
    noise_mode: u8,
    /// The situation room's roster: its selection (base +0x3e) and first row (+0x4c).
    selected: Option<u16>,
    roster_top: usize,
    /// The program panel is pressed, and which way its colours are swapped (+0x22).
    panel_pressed: bool,
    panel_swap: bool,
    /// The unit-name button's frame is XORed.
    unit_pressed: bool,
    /// The situation room's buttons' shared pressed look (base +0x40).
    button_look: bool,
    /// The humanoid console's buttons' look (+0x22 bits 3 and 7) and its fire button
    /// held (+0x1b = 7).
    // shortcut: one for every console, put back when a view opens; the original keeps
    // +0x22 and +0x1b in each robot.
    look: crate::console::Look,
    firing: bool,
    /// The briefing (`ORDR`), the main view's mode (base +0x37) and how many of the
    /// briefing's bytes are typed (+0x34).
    orders: Vec<u8>,
    mode: u8,
    typed: usize,
    capture: Option<(Target, MouseButton)>,
    /// The pointer's last position (`DS:0x14bc`, `0x14ba`), where the views' frames
    /// put the pointer probe.
    pointer: Option<(u16, u16)>,
    /// The console region the pointer last moved over (`DS:0x6a7e`, the humanoid's:
    /// 0 the 3D view, 1 the second display, 2 the third), set with its shape.
    region: Option<usize>,
    /// The object under the pointer in the last frame of the 3D view and of the second
    /// display (the humanoid's `DS:0x6a78`, `0x6a7a`, read by `1000:41b7`).
    probed: Cell<[Option<u16>; 2]>,
    /// The point under the pointer on the second display's last map frame
    /// (`ui_map_pointer_to_world`); none after a camera frame.
    map_point: Cell<Option<xeno_sim::Position>>,
    /// Alt in the last key word (`DS:0x1bf1` bit 0x20), as the mouse events carry it.
    alt: bool,
    /// The map display's Alt press waits for the next frame (+0x23 bit 2).
    goal_press: bool,
    dirty: bool,
    /// The tint's last weight (`DS:0x7d4c`); none (0xff) restarts it.
    tint: Option<u8>,
    /// The main view is to be redrawn (base +0x32 bit 14): mode S starts its colour
    /// record then.
    redraw: bool,
    /// What the situation room's map shows (base +0x32, +0x36, +0x4f).
    map: MapState,
    /// The scenario as loaded, which the editor's keys change; Ctrl+Alt+P and a save
    /// write the world over it ([`Mission::saved`]).
    scenario: Option<Scenario>,
    editor: Editor,
    /// The terrain editor while it is open (Ctrl+M): the sim stands still.
    terrain: Option<TerrainEditor>,
    /// A file dialog while one is open (T-110), and what for: the sim stands still, as
    /// the callers put the game clock back after it.
    dialog: Option<(FileDialog, Purpose)>,
    /// The dialogs' name field, which each leaves for the next (none: the first's).
    dialog_name: Option<Vec<u8>>,
    /// The file Alt+L's dialog chose, for the screens to load.
    load: Option<PathBuf>,
    /// F1's pause: the sim stands still under the box until a key or button press.
    paused: bool,
    /// Ctrl+Alt+S: the maps show the shots (`DS:0x62ec` bit 6).
    // shortcut: kept by the mission; the original's byte lasts the whole run.
    shots: bool,
}

impl Mission {
    /// A mission on `scenario`, starting in the situation room, its sim drawing from
    /// `random`, the run's random word (from [`Campaign::take_random`]), its robots
    /// moving by `motion`.
    pub fn new(
        file: String,
        scenario: &Scenario,
        random: Random,
        motion: &motion::Motion,
    ) -> Mission {
        let sim = Sim::load(scenario, random, motion.clone());
        log_census(&file, &sim.world);
        let mut mission = Mission::with_run(file, Run::Running(sim));
        mission.orders = scenario.briefing.clone().unwrap_or_default();
        mission.scenario = Some(scenario.clone());
        // The mission starts in the mode and map saved in the player's base.
        let base = mainview::base_of(Some(scenario), Side::Human);
        mission.mode = base.map_or(ORDERS_MODE, |b| b.view_mode);
        mission.map = base.map(MapState::of).unwrap_or_default();
        for o in scenario.objects.iter().flatten() {
            if let usg::Body::Robot(r) = &o.body {
                let (static_time, flying) = match &r.parts {
                    usg::Parts::Scorpion(p) => (p.static_time, p.missile != 0xffff),
                    _ => (0, false),
                };
                let display = Display {
                    mode: r.display,
                    zoom: r.zoom,
                    map_zoom: r.map_zoom,
                    pan: r.pan,
                    before: r.display_before,
                    label: r.zoom_label,
                    static_time,
                    flying,
                };
                mission.displays.insert(o.slot, display);
            }
        }
        log::debug!(
            "mission: the situation room starts in view mode {} ({} briefing bytes)",
            mission.mode,
            mission.orders_end()
        );
        mission
    }

    /// A mission with no file (no robots), parked: only the views' pictures.
    pub fn empty() -> Mission {
        Mission::with_run(String::new(), Run::default())
    }

    fn with_run(file: String, run: Run) -> Mission {
        let mut mission = Mission {
            file,
            run,
            viewed: Viewed::Base,
            sounds: Vec::new(),
            displays: BTreeMap::new(),
            ticked: false,
            noise: std::cell::RefCell::new(Random::seeded(0)),
            display_record: None,
            noise_mode: 0,
            selected: None,
            roster_top: 0,
            panel_pressed: false,
            panel_swap: false,
            unit_pressed: false,
            button_look: false,
            look: crate::console::Look::default(),
            firing: false,
            orders: Vec::new(),
            mode: ORDERS_MODE,
            typed: 0,
            capture: None,
            pointer: None,
            region: None,
            probed: Cell::default(),
            map_point: Cell::default(),
            alt: false,
            goal_press: false,
            dirty: true,
            tint: None,
            redraw: true,
            map: MapState::default(),
            scenario: None,
            editor: Editor::default(),
            terrain: None,
            dialog: None,
            dialog_name: None,
            load: None,
            paused: false,
            shots: false,
        };
        let first = mission.book().side(Side::Human).map(|(s, _)| s).next();
        mission.selected = first;
        mission
    }

    /// The campaign's day and month, which the loader puts back after the file's, in
    /// the clock (`DS:0x7d4a`, `0x7d4b`), which moves them on past midnight.
    pub fn set_date(&mut self, day: u8, month: u8) {
        let clock = &mut self.world_mut().clock;
        (clock.day, clock.month) = (day, month);
    }

    /// The mission stops (it ended, won, lost or with no result): its sim gives the
    /// run's random word back to `campaign`, and the date, which midnights in play
    /// moved on (the original's one calendar). The objects stay, as the original keeps
    /// them until another scenario loads, so the map room can run the mission again.
    pub fn park(&mut self, campaign: &mut Campaign) {
        self.run = match std::mem::take(&mut self.run) {
            Run::Running(sim) => {
                (campaign.day, campaign.month) = (sim.world.clock.day, sim.world.clock.month);
                let (parked, random) = sim.park();
                campaign.give_random(random);
                Run::Parked(parked)
            }
            // Parked already: it keeps its world.
            parked => parked,
        };
    }

    /// The mission goes on where it stopped, its sim taking the run's random word from
    /// `campaign`.
    pub fn resume(&mut self, campaign: &mut Campaign) {
        self.run = match std::mem::take(&mut self.run) {
            Run::Parked(parked) => Run::Running(parked.resume(campaign.take_random())),
            running => running,
        };
    }

    /// The scenario as loaded (and edited).
    #[cfg(test)]
    pub fn scenario(&self) -> Option<&Scenario> {
        self.scenario.as_ref()
    }

    /// The mission as `file_write_scenario` and `file_write_save_game` write it from
    /// memory (T-116): the world packed over the scenario as loaded (and edited)
    /// (`World::save`), with the views' state the mission starts from: the player's
    /// base's main view mode and map, and each robot's second display. No `SDEF`.
    pub fn saved(&self, assets: &Assets) -> Option<Scenario> {
        // shortcut: without the editor's tables the saved script pointers are not
        // offsets; the loader restarts every robot's program either way.
        let table = assets.editor.as_ref().map_or(0, |e| e.program_table);
        let mut s = self.world().save(self.scenario.as_ref()?, table);
        s.save = None;
        let human = self.world().base(Side::Human).map(|id| id.0);
        for o in s.objects.iter_mut().flatten() {
            match &mut o.body {
                usg::Body::Base(b) if Some(o.slot) == human => {
                    (b.view_mode, b.map_overlays, b.map_zoom) =
                        (self.mode, self.map.overlays, self.map.zoom);
                    b.map_centre = usg::Point {
                        x: self.map.centre.x.0,
                        y: self.map.centre.y.0,
                    };
                }
                usg::Body::Robot(r) => {
                    if let Some(d) = self.displays.get(&o.slot) {
                        (r.display, r.zoom, r.map_zoom, r.pan) =
                            (d.mode, d.zoom, d.map_zoom, d.pan);
                        (r.display_before, r.zoom_label) = (d.before, d.label);
                        if let usg::Parts::Scorpion(p) = &mut r.parts {
                            p.static_time = d.static_time;
                        }
                    }
                }
                _ => {}
            }
        }
        Some(s)
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        matches!(self.run, Run::Running(_))
    }

    /// The alert's damage to the player's base, at the start of a campaign scenario
    /// (`sim_apply_scenario_alert`); a parked mission changes nothing.
    pub fn apply_alert(&mut self, damage: u16) {
        if let Run::Running(sim) = &mut self.run
            && !sim.world.apply_alert(damage)
        {
            log::warn!("mission: no human base takes the alert's {damage} damage");
        }
    }

    pub fn world(&self) -> &World {
        match &self.run {
            Run::Running(sim) => &sim.world,
            Run::Parked(parked) => &parked.world,
        }
    }

    pub fn world_mut(&mut self) -> &mut World {
        match &mut self.run {
            Run::Running(sim) => &mut sim.world,
            Run::Parked(parked) => &mut parked.world,
        }
    }

    /// The robots' orders, in the sim's world.
    pub fn book(&self) -> &OrderBook {
        &self.world().orders
    }

    /// Changes the orders through the running sim, and logs the change; a parked
    /// mission changes nothing.
    fn change_orders(
        &mut self,
        programs: &Programs,
        f: impl FnOnce(&mut OrderBook) -> Option<Change>,
    ) {
        let change = match &mut self.run {
            Run::Running(sim) => sim.change_orders(f),
            Run::Parked(_) => None,
        };
        self.log_change(change, programs);
    }

    #[cfg(test)]
    pub fn viewed(&self) -> Viewed {
        self.viewed
    }

    /// Views `robot` (the player's), as with Tab or the roster.
    pub fn view_robot(&mut self, slot: u16) -> Option<View> {
        let robot = *self.book().get(slot)?;
        if robot.side != Side::Human {
            log::debug!("mission: cannot view the enemy's unit {slot}");
            return None;
        }
        self.leave_console();
        self.viewed = Viewed::Robot(slot);
        if let Run::Running(sim) = &mut self.run {
            sim.open_console();
        }
        self.reset_looks();
        Some(view_of(robot.kind))
    }

    /// The object viewed, for the log: the base, or the robot's slot and kind.
    pub fn viewed_label(&self) -> String {
        match self.viewed {
            Viewed::Base => "the base".into(),
            Viewed::Robot(slot) => match self.book().get(slot) {
                Some(robot) => format!("unit {slot} ({:?})", robot.kind),
                None => format!("unit {slot}"),
            },
        }
    }

    /// The view of the object viewed.
    pub fn view(&self) -> View {
        match self.viewed {
            Viewed::Base => View::SitRoom,
            Viewed::Robot(slot) => self
                .book()
                .get(slot)
                .map_or(View::SitRoom, |r| view_of(r.kind)),
        }
    }

    /// Whether the view needs drawing again; clears it.
    pub fn take_dirty(&mut self) -> bool {
        let dialog = self.dialog.as_mut().is_some_and(|(d, _)| d.take_dirty());
        std::mem::take(&mut self.dirty) | dialog
    }

    fn reset_looks(&mut self) {
        self.panel_pressed = false;
        self.unit_pressed = false;
        (self.look, self.firing) = (crate::console::Look::default(), false);
        self.capture = None;
        // A new view has drawn no frame yet: nothing is under the pointer.
        (self.goal_press, self.region) = (false, None);
        self.probed.set([None; 2]);
        self.map_point.set(None);
        self.dirty = true;
    }

    fn base(&mut self) -> Request {
        self.leave_console();
        self.viewed = Viewed::Base;
        // Drawing the situation room's picture starts the orders again (`1000:54fc`).
        self.typed = 0;
        self.reset_looks();
        Request::Show(View::SitRoom)
    }

    /// The player's robots, by slot (the port's order for the side's unit list).
    fn robots(&self) -> Vec<u16> {
        self.book()
            .side(Side::Human)
            .map(|(slot, _)| slot)
            .collect()
    }

    /// Whether robot `slot` is in its base (+0xfd bit 2).
    fn in_base(&self, slot: u16) -> bool {
        let e = self.world().get(EntityId(slot));
        e.is_some_and(|e| xeno_sim::base::in_base(&e.damage))
    }

    /// The next (`step` 1) or previous robot after `from`, wrapping; with `in_field`,
    /// skipping the others in the base, as the consoles do (`1000:7db2`, `7dd9`; the
    /// situation room's roster steps through them all, `1000:67e8`, `680c`).
    fn step_robot(&self, from: Option<u16>, step: isize, in_field: bool) -> Option<u16> {
        let mut robots = self.robots();
        robots.retain(|&r| !in_field || Some(r) == from || !self.in_base(r));
        let i = from.and_then(|s| robots.iter().position(|&r| r == s));
        let n = robots.len() as isize;
        if n == 0 {
            return None;
        }
        let next = match i {
            Some(i) => (i as isize + step).rem_euclid(n),
            None if step > 0 => 0,
            None => n - 1,
        };
        Some(robots[next as usize])
    }

    fn log_change(&mut self, change: Option<Change>, programs: &Programs) {
        if let Some(c) = change {
            log::info!(
                "orders: unit {} program {} ({}) -> {} ({}), {} -> {}",
                c.slot,
                c.before.program,
                String::from_utf8_lossy(programs.name(c.before.program)),
                c.after.program,
                String::from_utf8_lossy(programs.name(c.after.program)),
                mode(c.before.program_drives),
                mode(c.after.program_drives)
            );
            self.dirty = true;
        }
    }

    fn select_unit(&mut self, slot: Option<u16>) {
        if let Some(slot) = slot {
            log::debug!("situation room: unit {slot} selected");
            self.selected = Some(slot);
            // `1fe6:6f89`: in mode S a selection redraws the main view.
            self.redraw = true;
            let robots = self.robots();
            if let Some(i) = robots.iter().position(|&r| r == slot) {
                let rows = 5;
                if i < self.roster_top {
                    self.roster_top = i;
                } else if i >= self.roster_top + rows {
                    self.roster_top = i + 1 - rows;
                }
            }
            self.dirty = true;
        }
    }

    /// Space, J, the view button and a roster row's right release (`1000:6852`,
    /// `587a`): the selected unit's console, unless it is in the base.
    fn view_selected(&mut self) -> Option<Request> {
        let slot = self.selected.filter(|&s| !self.in_base(s))?;
        self.view_robot(slot).map(Request::Show)
    }

    /// The briefing's length as typed: up to its first NUL or 0x1a.
    fn orders_end(&self) -> usize {
        let end = self.orders.iter().position(|&b| b == 0 || b == 0x1a);
        end.unwrap_or(self.orders.len())
    }

    /// One retrace in the situation room: in the orders mode, one more byte of the
    /// briefing, until its end (one per pass of the original's mission loop).
    pub fn retrace(&mut self) {
        if self.paused {
            return;
        }
        if let Some((dialog, _)) = &mut self.dialog {
            dialog.retrace();
        }
        if self.viewed == Viewed::Base && self.mode == ORDERS_MODE && self.typed < self.orders_end()
        {
            self.typed += 1;
            self.dirty = true;
        }
    }

    /// A view's picture was just shown: the tint starts again at the next clock step
    /// (`1000:ecff`, called as each view is drawn).
    pub fn restart_tint(&mut self) {
        self.tint = None;
        // The main view's draw (`1000:54d3` on) also sets +0x32 bit 14.
        self.redraw = true;
    }

    /// The colour record mode S starts for the selected robot's kind on a redraw
    /// (`1000:686a`, `DS:0x56b8`): it sets colours 128 to 254 to the kind's picture's.
    pub fn status_record(&mut self, assets: &Assets) -> Option<u8> {
        let redraw = std::mem::take(&mut self.redraw);
        if !redraw || self.viewed != Viewed::Base || self.mode != STATUS_MODE {
            return None;
        }
        let kind = self.book().get(self.selected?)?.kind.index();
        assets.sitroom.as_ref()?.status.records.get(kind).copied()
    }

    /// The record the open console's display mode runs (`1000:d9ca`, `DS:0x70b6`),
    /// when it changes: the one to turn off where it stands, and the one to start.
    pub fn display_record(&mut self, assets: &Assets) -> Option<(Option<u8>, u8)> {
        let Viewed::Robot(slot) = self.viewed else {
            return None;
        };
        let records = &assets.views.second_display.records;
        let record = *records.get(usize::from(self.display_mode(slot, assets)))?;
        let old = self.display_record.replace(record);
        (old != Some(record)).then_some((old, record))
    }

    /// The display's record as the console's picture is closed (`1000:d9ef` with 0xff).
    pub fn take_display_record(&mut self) -> Option<u8> {
        self.display_record.take()
    }

    /// The weight to start [`TINT_RECORDS`] with now, if the mission clock's
    /// (`Clock::tint`) changed since the last (`pal_tint_for_time_of_day`); none in the
    /// situation room's mode 2 or while the mission is parked.
    pub fn tint(&mut self) -> Option<u8> {
        let Run::Running(sim) = &self.run else {
            return None;
        };
        if self.viewed == Viewed::Base && self.mode == STATUS_MODE {
            return None;
        }
        let weight = sim.world.clock.tint();
        (self.tint != Some(weight)).then(|| *self.tint.insert(weight))
    }

    /// The situation room recalls its selected unit, if any (`Sim::recall`).
    fn recall(&mut self) {
        if let (Run::Running(sim), Some(slot)) = (&mut self.run, self.selected) {
            log::debug!("situation room: unit {slot} recalled");
            sim.recall(EntityId(slot));
        }
    }

    /// The situation room launches its selected unit, if it is in the base
    /// (`Sim::launch`).
    fn launch(&mut self) {
        if let (Run::Running(sim), Some(slot)) = (&mut self.run, self.selected) {
            for event in sim.launch(EntityId(slot)) {
                log::debug!("situation room: {event:?}");
            }
            self.dirty = true;
        }
    }

    /// A release over one of a console's buttons (T-130 to T-132): a weapon button
    /// selects its weapon (`1000:a396`, `b214`; scorpion `8ac1`, `98a9`; scout `b81e`,
    /// `ca1d`), the switches hand the robot to the pilot or the program (`a5bb`, `a5fa`;
    /// `8b5f`, `8bc9`; `ba8c`, `bac1`), the lock button takes the next target (left) or
    /// drops it (right) and clears the look (`a3f1`, `8a26`, `be01`); the scorpion's stop
    /// asks for speed 0 and hands it to the pilot (`8e45`), its U deploys a relay
    /// (`8d0b`, `9b01`); the scout's stance buttons ask for a stance; the humanoid's and
    /// scout's cloak and ECM buttons run F8 and F7 (`aa5e`, `aa94`; `be77`, `bec5`); the
    /// scorpion's and scout's drop buttons run the key their number picks (`8b06`,
    /// `b88d`, through `1000:d5d5`).
    fn instrument(&mut self, slot: u16, w: views::Widget, right: bool, assets: &Assets) {
        let programs = &assets.programs;
        use crate::{humancon as h, scorpcon as s, scoutcon as c};
        match w.kind {
            h::WEAPON_TYPE | s::WEAPON_TYPE | c::WEAPON_TYPE => {
                if let Some(n) = self.widget_byte(slot, &w) {
                    self.pilot(slot, Pilot::Weapon(n));
                }
            }
            h::PILOT_TYPE
            | h::PROGRAM_TYPE
            | s::PILOT_TYPE
            | s::PROGRAM_TYPE
            | c::PILOT_TYPE
            | c::PROGRAM_TYPE => {
                let drives = matches!(w.kind, h::PROGRAM_TYPE | s::PROGRAM_TYPE | c::PROGRAM_TYPE);
                self.change_orders(programs, |book| book.set_program_drives(slot, drives));
            }
            kind if c::STANCE_TYPES.contains(&kind) => {
                let i = c::STANCE_TYPES.iter().position(|&k| k == kind).unwrap_or(1);
                self.pilot(slot, Pilot::Stance(STANCES[i]));
            }
            s::STOP_TYPE => {
                self.pilot(slot, Pilot::Stop);
                self.change_orders(programs, |book| book.set_program_drives(slot, false));
            }
            s::DEPLOY_TYPE => self.pilot(slot, Pilot::Deploy),
            s::DROP_TYPE | c::DROP_TYPE => {
                let buttons = assets.motion.weapons.drops.buttons;
                if let Some(&key) = self
                    .widget_byte(slot, &w)
                    .and_then(|n| buttons.get(usize::from(n)))
                {
                    self.pilot(slot, Pilot::Drop(key));
                }
            }
            h::CLOAK_TYPE | c::CLOAK_TYPE => self.pilot(slot, Pilot::Cloak),
            h::ECM_TYPE | c::ECM_TYPE => self.pilot(slot, Pilot::Ecm),
            h::LOCK_TYPE | s::LOCK_TYPE | c::LOCK_TYPE => {
                self.look = crate::console::Look::default();
                let key = if right {
                    Pilot::DropTarget
                } else {
                    Pilot::NextTarget
                };
                self.pilot(slot, key);
            }
            _ => {}
        }
    }

    /// A cockpit key acts on robot `slot` through the running sim; a parked mission
    /// changes nothing.
    fn pilot(&mut self, slot: u16, key: Pilot) {
        let Run::Running(sim) = &mut self.run else {
            return;
        };
        log::debug!("mission: {key:?} on robot {slot}");
        for event in sim.pilot(EntityId(slot), key) {
            log::debug!("mission: {event:?}");
            Self::hear(self.viewed, &mut self.sounds, event);
        }
        self.dirty = true;
    }

    /// One retrace of the mission's world with that retrace's input, if it runs (a
    /// parked mission waits). What it did goes to the log. [`Request::Over`] once a
    /// destroyed base's end script is over (`docs/sim.md`, "The mission's end"); the
    /// base's view when the robot on view is taken in (`1000:6460`, `7ee7`).
    pub fn tick<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a InputEvent>,
    ) -> Option<Request> {
        // The map display's press (`1000:466b`), after a frame of the map: the object
        // under the pointer becomes the goal, or else the point there the destination.
        // shortcut: it acts in the press's retrace (unless an Alt release came in the
        // same retrace), on the last frame drawn; the original acts when the next map
        // frame is done, a few retraces on, on that frame's probe.
        if let (true, Viewed::Robot(slot), Some(point)) = (
            std::mem::take(&mut self.goal_press),
            self.viewed,
            self.map_point.get(),
        ) {
            let key = match self.probed.get()[1] {
                Some(object) => Pilot::GoalObject(EntityId(object)),
                None => Pilot::Destination(point),
            };
            self.pilot(slot, key);
        }
        // The fire button held: the robot's tick sets the fire request (`1000:b3cf`).
        // shortcut: sent before the tick, so the request is a tick earlier than the
        // original's, set at the end of the robot's tick.
        if let (true, Viewed::Robot(slot)) = (self.firing, self.viewed) {
            self.pilot(slot, Pilot::Fire);
        }
        // `ui_run_map_editor`'s and `ui_run_file_dialog`'s loops never tick the mission.
        // Nor does F1's (the game clock is put back after it).
        let (Run::Running(sim), None, None, false) =
            (&mut self.run, &self.terrain, &self.dialog, self.paused)
        else {
            return None;
        };
        let (mut over, mut taken) = (None, false);
        self.ticked = true;
        for event in sim.tick(events) {
            log::debug!("mission: retrace {}: {event:?}", sim.retrace());
            match event {
                xeno_sim::motion::Event::MissionOver { won } => over = Some(won),
                xeno_sim::motion::Event::TakenIn(r) => taken |= self.viewed == Viewed::Robot(r.0),
                _ => {}
            }
            Self::hear(self.viewed, &mut self.sounds, event);
        }
        // The views are drawn from the world, which just moved.
        self.dirty = true;
        match over {
            Some(won) => Some(Request::Over { won }),
            None => taken.then(|| self.base()),
        }
    }

    /// Keeps a sound `event` that is heard everywhere, or only while its object's
    /// console is open and it is (`DS:0x7e16`; none in the situation room).
    fn hear(viewed: Viewed, sounds: &mut Vec<Sound>, event: xeno_sim::motion::Event) {
        if let xeno_sim::motion::Event::Sound { event, only_for } = event
            && only_for.is_none_or(|id| viewed == Viewed::Robot(id.0))
        {
            sounds.push(Sound::Play(event));
        }
    }

    /// The sound events heard since the last call.
    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    /// Robot `slot`'s console display.
    fn display(&self, slot: u16) -> Display {
        self.displays.get(&slot).copied().unwrap_or_default()
    }

    /// Robot `slot`'s display mode as `1000:db5c` leaves it each tick: bit 2 set while
    /// the mode's device (+0xf6) is damaged.
    fn display_mode(&self, slot: u16, assets: &Assets) -> u8 {
        let mode = self.display(slot).mode;
        if mode == LOST_MISSILE_CAMERA {
            return mode;
        }
        let mode = mode & !DAMAGED;
        let systems = match self.world().get(EntityId(slot)).map(|e| e.damage) {
            Some(xeno_sim::Damage::Robot(c)) => c.systems,
            _ => 0,
        };
        let devices = &assets.views.second_display.devices;
        let device = devices.get(usize::from(mode)).copied().unwrap_or(0);
        if systems & device != 0 {
            mode | DAMAGED
        } else {
            mode
        }
    }

    /// `1000:daa2`, after each frame of the open console's second display: when its
    /// display mode changed, the noise plays if the new one is damaged, else its voice
    /// is given silence.
    pub fn check_noise(&mut self, assets: &Assets) {
        let Viewed::Robot(slot) = self.viewed else {
            return;
        };
        let mode = self.display_mode(slot, assets);
        if mode != self.noise_mode {
            self.noise_mode = mode;
            self.sounds.push(if mode & DAMAGED != 0 {
                Sound::Play(NOISE_SOUND)
            } else {
                Sound::Stop(NOISE_SOUND)
            });
        }
    }

    /// A console's leave routine (`1000:dac0`): the noise's voice silenced.
    fn leave_console(&mut self) {
        if let Viewed::Robot(_) = self.viewed {
            self.sounds.push(Sound::Stop(NOISE_SOUND));
            self.noise_mode = 0;
        }
    }

    /// A display mode key (`ui_set_humanoid_display_mode` and the other kinds'): the
    /// mode set, the zoom's name shown and the mode's sound played (heard whichever
    /// console is open, as `1000:ee90` plays it), whatever the mode was.
    fn set_display_mode(&mut self, slot: u16, mode: u8) {
        let d = self.displays.entry(slot).or_default();
        (d.mode, d.label, d.static_time) = (mode, LABEL_TIME, 0);
        self.sounds.push(Sound::Play(DISPLAY_SOUND));
        self.dirty = true;
    }

    /// Z, X and Y (`1000:db7e`, `db8f`, `b39b`): the pan less or plus the zoom's step,
    /// or 0; the target no longer tracked.
    fn pan_display(&mut self, slot: u16, more: Option<bool>, assets: &Assets) {
        let steps = &assets.views.second_display.pan_steps;
        let d = self.displays.entry(slot).or_default();
        let step = steps.get(usize::from(d.zoom)).copied().unwrap_or(0);
        d.pan = match more {
            Some(true) => d.pan.wrapping_add(step),
            Some(false) => d.pan.wrapping_sub(step),
            None => 0,
        };
        self.pilot(slot, Pilot::Untrack);
    }

    /// After each tick of the world, every robot's display as its kind's tick leaves
    /// it (`docs/screens.md`, "The display's other modes (T-134)"): following a tracked
    /// target's distance and bearing every tick; the scorpion's missile camera on a
    /// launch; every 16 ticks (tick table entry 0) the zoom name's time down, and the
    /// missile camera's break-up once its missile is gone, then the mode before it.
    pub fn step_displays(&mut self, assets: &Assets) {
        if !std::mem::take(&mut self.ticked) {
            return;
        }
        let ranges = &assets.views.second_display.ranges;
        let mut modes = Vec::new();
        for (&slot, d) in &mut self.displays {
            let Run::Running(sim) = &self.run else {
                return;
            };
            let Some(e) = sim.world.get(EntityId(slot)) else {
                continue;
            };
            let State::Robot(m) = &e.state else {
                continue;
            };
            if m.target.is_some() && m.status & usg::status::TRACKING != 0 {
                let zoom = ranges.iter().position(|&r| m.target_distance <= r);
                d.zoom = zoom.unwrap_or(ranges.len() - 1) as u8;
                d.pan = m.target_bearing.wrapping_sub(e.heading);
            }
            let flying = m.guns.missile.is_some();
            if flying && !std::mem::replace(&mut d.flying, flying) {
                d.before = d.mode;
                modes.push((slot, MISSILE_DISPLAY));
            }
            d.flying = flying;
            if m.tick & 0x1e != 0 {
                continue;
            }
            d.label = d.label.saturating_sub(1);
            if e.kind != usg::ObjectKind::Scorpion {
                continue;
            }
            if d.mode == MISSILE_DISPLAY && !flying {
                d.static_time = STATIC_TIME;
            }
            if d.static_time != 0 {
                d.mode |= DAMAGED;
                d.static_time -= 1;
                if d.static_time == 0 {
                    modes.push((slot, d.before & !DAMAGED));
                }
            }
        }
        for (slot, mode) in modes {
            self.set_display_mode(slot, mode);
        }
    }

    /// `1000:da0e`, `da37`: the map's zoom step in the map mode, the camera's in the
    /// others, within 0 to 4.
    fn zoom_display(&mut self, slot: u16, closer: bool, assets: &Assets) {
        let map = self.display_mode(slot, assets) == MAP_DISPLAY;
        let d = self.displays.entry(slot).or_default();
        let step = if map { &mut d.map_zoom } else { &mut d.zoom };
        // Only 4 and 0 stop it: a step out of range wraps on.
        *step = match closer {
            true if *step != MAX_DISPLAY_ZOOM => step.wrapping_add(1),
            false if *step != 0 => step.wrapping_sub(1),
            _ => *step,
        };
        d.label = LABEL_TIME;
        self.pilot(slot, Pilot::Untrack);
    }

    /// The robots the player's side destroyed and lost (its base's +0x2c and +0x2b).
    pub fn kills_and_losses(&self) -> (u8, u8) {
        let world = self.world();
        match world.base(Side::Human).and_then(|b| world.get(b)) {
            Some(e) => match e.state {
                xeno_sim::State::Base { kills, losses, .. } => (kills, losses),
                _ => (0, 0),
            },
            None => (0, 0),
        }
    }

    /// The key word `word` went down in the view shown.
    pub fn key(&mut self, word: u16, assets: &Assets) -> Option<Request> {
        if self.unpause() {
            return None;
        }
        if let Some((dialog, _)) = &mut self.dialog {
            let outcome = dialog.key(crate::screens::KeyWord(word));
            self.close_dialog(outcome, assets);
            return None;
        }
        if self.terrain.is_some() {
            let keys = assets.map_driver.iter().flat_map(|d| &d.keys);
            if let Some(&(_, handler)) = keys.into_iter().find(|&&(w, _)| w == word) {
                self.terrain_key(handler, assets);
            }
            return None;
        }
        // The situation room's editor widget comes before its key table.
        if let (Viewed::Base, true, Some(tables)) =
            (self.viewed, assets.editor_unlocked, &assets.editor)
            && let Some(&(_, handler)) = tables.keys.iter().find(|&&(w, _)| w == word)
        {
            self.editor_key(handler, tables, assets);
            return None;
        }
        let view = self.view();
        let handler = assets
            .tables(view)
            .keys
            .iter()
            .find_map(|&(w, h)| (w == word).then_some(h))?;
        let programs = &assets.programs;
        match (self.viewed, handler) {
            (_, handler::PAUSE) => {
                self.paused = true;
                self.dirty = true;
                None
            }
            (_, handler::SHOTS) => {
                self.shots = !self.shots;
                self.dirty = true;
                None
            }
            (Viewed::Robot(slot), handler::CHARGE) if assets.developer_unlocked => {
                self.pilot(slot, Pilot::Charge);
                None
            }
            (Viewed::Robot(_), handler::BASE) => Some(self.base()),
            (Viewed::Robot(slot), handler::TO_PILOT) => {
                self.change_orders(programs, |book| book.set_program_drives(slot, false));
                None
            }
            (Viewed::Robot(slot), handler::TO_PROGRAM) => {
                self.change_orders(programs, |book| book.set_program_drives(slot, true));
                None
            }
            (Viewed::Robot(slot), handler::PILOT_NEXT) => {
                self.change_orders(programs, |book| book.select_next(slot, programs));
                None
            }
            (Viewed::Robot(slot), h) if handler::DISPLAY_MODES.iter().any(|&(m, _)| m == h) => {
                let mode = handler::DISPLAY_MODES.iter().find(|&&(m, _)| m == h)?.1;
                self.set_display_mode(slot, mode);
                None
            }
            (Viewed::Robot(slot), handler::DISPLAY_ZOOM_IN | handler::DISPLAY_ZOOM_OUT) => {
                self.zoom_display(slot, handler == handler::DISPLAY_ZOOM_IN, assets);
                None
            }
            (Viewed::Robot(slot), h) if handler::DISPLAY_PANS.iter().any(|&(p, _)| p == h) => {
                let pan = handler::DISPLAY_PANS.iter().find(|&&(p, _)| p == h)?.1;
                self.pan_display(slot, pan, assets);
                None
            }
            (Viewed::Robot(slot), h) if handler::PILOT.iter().any(|&(p, _)| p == h) => {
                let key = handler::PILOT.iter().find(|&&(p, _)| p == h)?.1;
                self.pilot(slot, key);
                None
            }
            (Viewed::Robot(slot), handler::NEXT_UNIT | handler::PREVIOUS_UNIT) => {
                let step = if handler == handler::NEXT_UNIT { 1 } else { -1 };
                let next = self.step_robot(Some(slot), step, true)?;
                self.view_robot(next).map(Request::Show)
            }
            (Viewed::Base, handler::END) => (word == ALT_Q).then_some(Request::End),
            (Viewed::Base, handler::SAVE) => {
                match &assets.file_dialogs {
                    Some(tables) => {
                        self.open_dialog(Purpose::SaveScenario, tables, &assets.save_dir)
                    }
                    None => self.save_scenario(&assets.save_dir.join(SAVED_SCENARIO), assets),
                }
                None
            }
            (Viewed::Base, handler::VIEW_SELECTED) => self.view_selected(),
            (Viewed::Base, handler::RECALL) => {
                self.recall();
                None
            }
            (Viewed::Base, handler::LAUNCH) => {
                self.launch();
                None
            }
            (Viewed::Base, h) if handler::SLIDERS.iter().any(|&(k, _)| k == h) => {
                let n = handler::SLIDERS.iter().find(|&&(k, _)| k == h)?.1;
                if let Run::Running(sim) = &mut self.run {
                    sim.step_slider(Side::Human, n);
                }
                self.dirty = true;
                None
            }
            (Viewed::Base, h) if handler::MODES.iter().any(|&(m, _)| m == h) => {
                let mode = handler::MODES.iter().find(|&&(m, _)| m == h)?.1;
                self.set_mode(mode);
                None
            }
            (Viewed::Base, handler::ZOOM_IN | handler::ZOOM_OUT) => {
                self.zoom(handler == handler::ZOOM_IN);
                None
            }
            (Viewed::Base, h) if handler::OVERLAYS.iter().any(|&(o, _)| o == h) => {
                let bit = handler::OVERLAYS.iter().find(|&&(o, _)| o == h)?.1;
                self.flip_overlay(bit);
                None
            }
            (Viewed::Base, handler::NEXT_SELECTED | handler::PREVIOUS_SELECTED) => {
                let step = if handler == handler::NEXT_SELECTED {
                    1
                } else {
                    -1
                };
                let next = self.step_robot(self.selected, step, false);
                self.select_unit(next);
                None
            }
            _ => {
                log::debug!(
                    "mission: key {word:#06x} (handler {handler:#06x}) acts on the running mission, not modelled yet (T-056)"
                );
                None
            }
        }
    }

    /// The situation room's main view mode (`ui_sitroom_set_view_mode`).
    fn set_mode(&mut self, mode: u8) {
        log::debug!("situation room: main view mode {mode}");
        self.mode = mode;
        self.typed = 0;
        self.tint = None;
        self.redraw = true;
        self.dirty = true;
    }

    /// The map's zoom up or down a step, within 0 to 6, in mode M only.
    fn zoom(&mut self, closer: bool) {
        if self.mode == MAP_MODE {
            // `1000:5cc7`, `5cd7`: up only below 6, down only above 0.
            let z = self.map.zoom;
            if closer && z < mainview::MAX_ZOOM {
                self.map.zoom = z + 1;
            } else if !closer && z > 0 {
                self.map.zoom = z - 1;
            }
            self.dirty = true;
        }
    }

    /// An overlay bit of the map flipped, in mode M only.
    fn flip_overlay(&mut self, bit: u16) {
        if self.mode == MAP_MODE {
            self.map.overlays ^= bit;
            self.dirty = true;
        }
    }

    /// Runs an editor key on the mission's scenario, logs the editor's status, and
    /// saves the terrain if the key asks.
    fn editor_key(&mut self, handler: u16, tables: &editor::Editor, assets: &Assets) {
        let Some(scenario) = &mut self.scenario else {
            log::debug!("editor: no scenario loaded");
            return;
        };
        let effect = self.editor.key(handler, scenario, tables);
        self.dirty = true;
        if let Effect::Scroll(x, y) = effect {
            let c = &mut self.map.centre;
            (c.x.0, c.y.0) = (c.x.0.wrapping_add(x), c.y.0.wrapping_add(y));
        }
        if effect == Effect::TerrainEditor {
            match assets.map_driver {
                Some(_) => self.terrain = Some(TerrainEditor::enter()),
                None => log::info!("editor: no terrain editor without mapdvr.dvr's keys"),
            }
        }
        for row in self.editor.status(scenario, tables) {
            log::info!("editor: {row}");
        }
        if effect == Effect::DestroySelected {
            self.destroy_selected();
        }
        if let (Effect::Orders, Some(dialogs)) = (effect, &assets.file_dialogs) {
            self.open_dialog(Purpose::Orders, dialogs, &assets.save_dir);
        }
    }

    /// Runs a terrain editor key: Apply changes the mission's terrain through the sim,
    /// Leave writes `BACKUP.MAP` in `--save-dir` and closes the editor, Load and Save
    /// open the file dialogs.
    fn terrain_key(&mut self, handler: u16, assets: &Assets) {
        let dir = &assets.save_dir;
        let Some(editor) = &mut self.terrain else {
            return;
        };
        let world = self.scenario.as_ref().and_then(|s| s.terrain.as_ref());
        let effect = editor.key(handler, world.map_or(&[][..], |t| &t.features));
        let (saved, status) = (map::write(&editor.saved()), editor.status().to_string());
        self.dirty = true;
        match effect {
            terrain::Effect::None => {}
            terrain::Effect::Later(what) => log::info!("terrain editor: this key {what}"),
            terrain::Effect::Apply => {
                // shortcut: the original writes `BACKUP.MAP`, loads it and deletes it;
                // the port applies the same records without the file. Upgrade if a
                // save's MAPP chunk must match DOSBox's byte for byte.
                if self.load_terrain(&saved).is_ok() {
                    log::info!("terrain editor: applied");
                }
            }
            terrain::Effect::Load | terrain::Effect::Save => {
                let purpose = match effect {
                    terrain::Effect::Load => Purpose::LoadMap,
                    _ => Purpose::SaveMap,
                };
                match &assets.file_dialogs {
                    Some(tables) => self.open_dialog(purpose, tables, dir),
                    None => log::info!("terrain editor: no file dialogs from xeno.exe"),
                }
            }
            terrain::Effect::Leave => {
                let path = dir.join(SAVED_TERRAIN);
                match std::fs::write(&path, saved) {
                    Ok(()) => log::info!("terrain editor: left, {} written", path.display()),
                    Err(e) => log::warn!("terrain editor: cannot write {}: {e}", path.display()),
                }
                self.terrain = None;
                return;
            }
        }
        log::debug!("terrain editor: {status}");
    }

    /// A `.map`'s bytes made the mission's terrain, through the sim and in the scenario
    /// copy, as host slot `0x1506` loads one (the origin added; the buffer's leftover
    /// memory to a paragraph taken as zeros).
    fn load_terrain(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut world = map::to_world(&map::parse(bytes).map_err(|e| e.to_string())?);
        world.padding = vec![0; bytes.len().next_multiple_of(16) - bytes.len()];
        if let Run::Running(sim) = &mut self.run {
            sim.set_terrain(&world.features);
        }
        if let Some(scenario) = &mut self.scenario {
            scenario.terrain = Some(world);
        }
        Ok(())
    }

    /// Alt+L's file (`0x066f`): the terrain, then the circles imported from it. A file
    /// that does not load changes nothing.
    fn load_map(&mut self, path: &Path) {
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| self.load_terrain(&bytes));
        if let Err(e) = loaded {
            return log::warn!("terrain editor: cannot load {}: {e}", path.display());
        }
        let world = self.scenario.as_ref().and_then(|s| s.terrain.as_ref());
        let features = world.map_or(&[][..], |t| &t.features);
        if let Some(editor) = &mut self.terrain {
            editor.import(features);
        }
        log::info!("terrain editor: {} loaded", path.display());
    }

    /// Alt+S's file (`0x0705`): the circles written there.
    fn save_map(&self, path: &Path) {
        let Some(editor) = &self.terrain else { return };
        match std::fs::write(path, map::write(&editor.saved())) {
            Ok(()) => log::info!("terrain editor: {} written", path.display()),
            Err(e) => log::warn!("terrain editor: cannot write {}: {e}", path.display()),
        }
    }

    /// The terrain editor's view's camera and rectangle: the whole screen.
    fn terrain_camera(&self, assets: &Assets) -> Option<xeno_render::view3d::Camera> {
        let view = assets.view3d.as_ref()?;
        Some(self.terrain.as_ref()?.camera(view, &assets.motion))
    }

    /// The ground point under the pointer at `at` in the terrain editor
    /// (`ui_map_pointer_to_world`). shortcut: from the editor's camera now, where the
    /// original uses the last frame's; they differ only between a camera move and the
    /// next frame.
    fn terrain_point(&self, at: (u16, u16), assets: &Assets) -> Option<xeno_sim::Position> {
        let camera = self.terrain_camera(assets)?;
        let screen = Rect {
            top: 0,
            left: 0,
            bottom: 200,
            right: 320,
        };
        Some(mainview::pointer_to_world(&camera, screen, at))
    }

    /// A mouse button went down (or up) at `at` in the terrain editor with the key
    /// word `held` (Shift, Ctrl and Alt bits): only the left button counts. Returns
    /// whether the editor is open (and took the event).
    pub fn terrain_mouse(
        &mut self,
        button: MouseButton,
        down: bool,
        at: Option<(u16, u16)>,
        held: u16,
        assets: &Assets,
    ) -> bool {
        // shortcut: the event's key word's high byte is taken as the modifiers with
        // bit 0 set (1 plain, 0x11 with Ctrl), the 1 guessed to be the left button;
        // upgrade when the mouse events' writer is traced (T-133).
        let modifiers = (held >> 8) as u8 | 1;
        if self.terrain.is_none() {
            return false;
        }
        let point = at.and_then(|at| self.terrain_point(at, assets));
        let Some(editor) = &mut self.terrain else {
            return false;
        };
        match (button, down, point) {
            (MouseButton::Left, true, Some(point)) => editor.press(point, modifiers),
            (MouseButton::Left, false, _) => {
                if !editor.release() {
                    log::info!("terrain editor: no room for another circle");
                }
            }
            _ => return true,
        }
        self.dirty = true;
        true
    }

    /// The last key word's Alt (`DS:0x1bf1` bit 0x20), which the mouse events carry.
    pub fn set_alt(&mut self, alt: bool) {
        self.alt = alt;
    }

    /// The pointer moved to `at`: in the terrain editor, the ground point under it. A
    /// console and the situation room's map draw again, so their views probe the new
    /// position (the original draws them continuously).
    pub fn hover(&mut self, at: (u16, u16), assets: &Assets) {
        self.pointer = Some(at);
        // shortcut: redraws on every move, the original every 4 ticks (its frames
        // run over several passes; the map's too, T-134).
        self.dirty |= matches!(self.viewed, Viewed::Robot(_))
            || (self.viewed == Viewed::Base && self.mode == MAP_MODE);
        if let (Some((dialog, _)), Some(tables)) = (&mut self.dialog, &assets.file_dialogs) {
            dialog.mouse_move(at, tables);
            return;
        }
        if let Some(point) = self.terrain_point(at, assets)
            && let Some(editor) = &mut self.terrain
        {
            editor.hover(point);
            self.dirty = true;
        }
    }

    /// The situation room's map as drawn: the base's, with the editor's Ctrl+E.
    fn shown_map(&self) -> MapState {
        MapState {
            reveal: self.editor.reveal(),
            shots: self.shots,
            ..self.map
        }
    }

    /// Ctrl+D: the selected unit is destroyed through the sim and leaves the scenario
    /// a save writes.
    fn destroy_selected(&mut self) {
        let (Some(slot), Run::Running(sim)) = (self.selected, &mut self.run) else {
            return;
        };
        for event in sim.destroy_robot(EntityId(slot)) {
            Self::hear(self.viewed, &mut self.sounds, event);
        }
        if self.book().get(slot).is_none() {
            if let Some(objects) = self.scenario.as_mut().and_then(|s| s.objects.as_mut()) {
                objects.retain(|o| o.slot != slot);
            }
            // shortcut: which unit the roster picks next (`sim_remove_unit_from_list`,
            // `1fe6:6e71`) is not traced: the first; upgrade with T-128.
            self.selected = None;
            let first = self.step_robot(None, 1, false);
            self.select_unit(first);
        }
    }

    /// A left press with Ctrl at `at` (`1000:5781`): over the situation room's main view
    /// in mode M (the map), with no other widget there, it is the editor's place
    /// (`ui_editor_place_at_pointer`), which needs unlock word 9 and a running sim.
    /// Returns whether the press was taken.
    pub fn place_at_pointer(&mut self, at: Option<(u16, u16)>, assets: &Assets) -> bool {
        let Some(at) = at else {
            return false;
        };
        let view = self.main_view(assets);
        if self.viewed != Viewed::Base
            || self.mode != MAP_MODE
            || !view.contains(at.0, at.1)
            || self.target(assets, at).is_some()
        {
            return false;
        }
        let (Some(camera), Some(tables)) = (self.map_camera(assets), &assets.editor) else {
            return true;
        };
        let (Run::Running(sim), Some(scenario), true) =
            (&mut self.run, &mut self.scenario, assets.editor_unlocked)
        else {
            return true;
        };
        let point = mainview::pointer_to_world(&camera, view, at);
        if self.editor.place(point, scenario, sim, tables) {
            for row in self.editor.status(scenario, tables) {
                log::info!("editor: {row}");
            }
        }
        self.dirty = true;
        true
    }

    /// A left press on the situation room's map in mode M with no widget there
    /// (`1000:5781`): on one of the player's units probed in the last frame it clears
    /// the map's bit 15 and selects it (`1fe6:6f31`); otherwise it sets bit 15 and
    /// moves the centre (+0x4f, +0x53) to the point under the pointer
    /// (`ui_map_pointer_to_world`).
    fn map_press(&mut self, at: (u16, u16), assets: &Assets) {
        let view = self.main_view(assets);
        if self.viewed != Viewed::Base || self.mode != MAP_MODE || !view.contains(at.0, at.1) {
            return;
        }
        // The roster's robots are the objects with +0x16 bit 5 on the player's side.
        let unit = self.probed.get()[0].filter(|s| self.robots().contains(s));
        if unit.is_some() {
            self.map.overlays &= !usg::Base::MAP_FREE;
            self.select_unit(unit);
        } else if let Some(camera) = self.map_camera(assets) {
            self.map.overlays |= usg::Base::MAP_FREE;
            self.map.centre = mainview::pointer_to_world(&camera, view, at);
            self.dirty = true;
        }
    }

    /// The situation room map's camera, as the last frame drew it.
    fn map_camera(&self, assets: &Assets) -> Option<xeno_render::view3d::Camera> {
        let view = assets.view3d.as_ref()?;
        let world = self.world();
        let selected = self.selected.and_then(|s| world.get(EntityId(s)));
        let base = world
            .iter()
            .find(|(_, e)| e.kind == usg::ObjectKind::Base && e.side == Side::Human)
            .map(|(_, e)| e.position)
            .unwrap_or_default();
        Some(mainview::camera(
            base,
            selected,
            &self.map,
            view,
            &assets.motion,
        ))
    }

    /// Ctrl+Alt+P: writes the mission's scenario as the original's writer does.
    /// Opens a file dialog in `dir` (`ui_run_file_dialog`), its field as the last one
    /// left it.
    fn open_dialog(&mut self, purpose: Purpose, tables: &FileDialogs, dir: &Path) {
        let name = self
            .dialog_name
            .clone()
            .unwrap_or_else(|| tables.first_name.clone());
        let kind = match purpose {
            Purpose::SaveScenario | Purpose::SaveMap => dialog::Kind::Save,
            Purpose::Orders | Purpose::LoadMap | Purpose::LoadScenario => dialog::Kind::Open,
        };
        let dialog = FileDialog::open(kind, dir.to_path_buf(), name, tables);
        self.dialog = Some((dialog, purpose));
        self.dirty = true;
    }

    /// A dialog's key or click ended it (or not, with `None`): its caller goes on with
    /// the path chosen.
    fn close_dialog(&mut self, outcome: Option<Outcome>, assets: &Assets) {
        let Some(outcome) = outcome else { return };
        let Some((dialog, purpose)) = self.dialog.take() else {
            return;
        };
        self.dialog_name = Some(dialog.name().to_vec());
        self.dirty = true;
        let path = match outcome {
            Outcome::Chosen(path) => path,
            Outcome::Cancelled => return log::info!("mission: file dialog cancelled"),
        };
        match purpose {
            Purpose::SaveScenario => self.save_scenario(&path, assets),
            Purpose::Orders => self.load_orders(&path),
            Purpose::LoadMap => self.load_map(&path),
            Purpose::SaveMap => self.save_map(&path),
            Purpose::LoadScenario => self.load = Some(path),
        }
    }

    /// Ctrl+Alt+O's file (`1000:6d75`): its first 800 bytes over the briefing buffer's,
    /// whatever was after them kept, then the orders mode, typed from the start. A file
    /// that does not open changes nothing.
    fn load_orders(&mut self, path: &Path) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return log::warn!("editor: cannot read {}: {e}", path.display()),
        };
        let bytes = &bytes[..bytes.len().min(ORDERS_FILE_MAX)];
        let over = |buffer: &mut Vec<u8>| {
            if buffer.len() < bytes.len() {
                buffer.resize(bytes.len(), 0);
            }
            buffer[..bytes.len()].copy_from_slice(bytes);
        };
        over(&mut self.orders);
        if let Some(scenario) = &mut self.scenario {
            over(scenario.briefing.get_or_insert_with(Vec::new));
        }
        self.set_mode(ORDERS_MODE);
        log::info!("editor: the orders read from {}", path.display());
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The map room's and training menu's Alt+L (`0x14c`) and Alt+S (`0x14e`): the
    /// open dialog, whose file [`Mission::take_load`] hands over, or the save dialog, which
    /// writes this mission's scenario as Ctrl+Alt+P does. False without the dialogs.
    pub fn open_file_dialog(&mut self, save: bool, assets: &Assets) -> bool {
        let Some(tables) = &assets.file_dialogs else {
            return false;
        };
        let purpose = if save {
            Purpose::SaveScenario
        } else {
            Purpose::LoadScenario
        };
        self.open_dialog(purpose, tables, &assets.save_dir);
        true
    }

    /// The file Alt+L's dialog chose, once.
    pub fn take_load(&mut self) -> Option<PathBuf> {
        self.load.take()
    }

    /// `next` takes this mission's place (Alt+L's load), keeping the dialogs' name
    /// field, which the original keeps for the whole run.
    pub fn replace_with(&mut self, mut next: Mission) {
        next.dialog_name = self.dialog_name.take();
        *self = next;
    }

    /// Draws the open file dialog alone, over another screen (the map room's or the
    /// training menu's picture).
    pub fn draw_dialog(&self, fb: &mut Framebuffer, assets: &Assets) {
        if let (Some((dialog, _)), Some(tables)) = (&self.dialog, &assets.file_dialogs) {
            dialog.draw(fb, tables, &assets.fonts, &assets.map_sprites);
        }
    }

    /// One retrace of an open file dialog away from the mission (a held arrow).
    pub fn dialog_retrace(&mut self) {
        if let Some((dialog, _)) = &mut self.dialog {
            dialog.retrace();
        }
    }

    /// F1's pause is on.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// A key or button press ends F1's pause (slot `0x340`, mask 0x82a); true if it
    /// did, and the press does nothing else.
    fn unpause(&mut self) -> bool {
        self.dirty |= self.paused;
        std::mem::take(&mut self.paused)
    }

    /// A character typed, for an open file dialog's name field.
    pub fn typed(&mut self, c: char) {
        if let Some((dialog, _)) = &mut self.dialog {
            dialog.typed(c);
        }
    }

    /// A mouse button went down or up at `at` with a file dialog open; whether one is.
    pub fn dialog_mouse(
        &mut self,
        button: MouseButton,
        down: bool,
        at: Option<(u16, u16)>,
        assets: &Assets,
    ) -> bool {
        let (Some((dialog, _)), Some(tables)) = (&mut self.dialog, &assets.file_dialogs) else {
            return false;
        };
        let Some(at) = at else { return true };
        if down {
            dialog.mouse_down(button, at, tables);
        } else {
            let outcome = dialog.mouse_up(button, at, tables);
            self.close_dialog(outcome, assets);
        }
        true
    }

    fn save_scenario(&self, path: &Path, assets: &Assets) {
        let Some(scenario) = self.saved(assets) else {
            log::info!("mission: nothing loaded to save");
            return;
        };
        let written = usg::write(&scenario)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
        match written {
            Ok(()) => log::info!("mission: saved the scenario to {}", path.display()),
            Err(e) => log::warn!("mission: cannot save {}: {e}", path.display()),
        }
    }

    /// The pointer's shape at `at`, where the view's picture has `colour` (T-106,
    /// `docs/screens.md`, "Shape and visibility"). The dispatcher (`1000:2509`) hands a
    /// move to the last widget whose rectangle holds it: the corners pick shape 2; the
    /// situation room's main view the shape for its mode (`1000:575b`) over its
    /// colour-255 pixels (slot `0x5bc`); a console's 3D view, second and third displays
    /// theirs over colours 255, 254 and 253, the second display's map mode with `alt`
    /// held its own. Anywhere else the arrow, as those handlers set it when the pointer
    /// leaves them or their region. A console's handlers also note the region.
    pub fn pointer_shape(
        &mut self,
        assets: &Assets,
        (x, y): (u16, u16),
        colour: u8,
        alt: bool,
    ) -> u8 {
        self.region = None;
        if self.dialog.is_some() {
            return DIALOG_SHAPE;
        }
        if let Some(editor) = &self.terrain {
            return editor.pointer_shape();
        }
        let Some(top) = assets
            .tables(self.view())
            .widgets
            .iter()
            .rev()
            .find(|w| w.area.is_some_and(|a| a.contains(x, y)))
        else {
            return ARROW;
        };
        let shapes = &assets.views.pointer_shapes;
        match (self.viewed, top.kind) {
            (_, SITUATION_CORNER_TYPE | CONSOLE_CORNER_TYPE) => CORNER,
            (Viewed::Base, MAIN_VIEW_TYPE) if colour == 255 => shapes
                .main_view
                .get(usize::from(self.mode / 2))
                .copied()
                .unwrap_or(ARROW),
            (Viewed::Robot(slot), kind) => {
                let Some(robot) = self.book().get(slot) else {
                    return ARROW;
                };
                let regions = REGION_WIDGETS[robot.kind.index()];
                let region = regions.iter().position(|&k| k == kind);
                let colours = [255, SECOND_DISPLAY, THIRD_DISPLAY];
                self.region = region.filter(|&r| colours[r] == colour);
                match region {
                    Some(0) if colour == 255 => shapes.console[0],
                    Some(1) if colour == SECOND_DISPLAY => {
                        let map = self.display_mode(slot, assets) == MAP_DISPLAY;
                        shapes.console[if map && alt { 2 } else { 1 }]
                    }
                    Some(2) if colour == THIRD_DISPLAY => shapes.console[3],
                    _ => ARROW,
                }
            }
            _ => ARROW,
        }
    }

    /// The widget under (`x`, `y`) in the view shown.
    fn target(&self, assets: &Assets, (x, y): (u16, u16)) -> Option<Target> {
        let view = self.view();
        let tables = assets.tables(view);
        let hit = |kind: u16| {
            tables
                .find(kind)
                .and_then(|w| w.area)
                .is_some_and(|a| a.contains(x, y))
        };
        if view == View::SitRoom {
            if hit(SITUATION_CORNER_TYPE) {
                return Some(Target::Corner);
            }
            if let Some(i) = assets
                .views
                .situation_buttons
                .iter()
                .position(|b| b.area.contains(x, y))
            {
                return Some(Target::Button(i));
            }
            // The map controls answer only in mode M; the mode buttons always.
            if let Some(i) = tables.nested.iter().position(|w| {
                w.area.is_some_and(|a| a.contains(x, y))
                    && (w.kind == mainview::MODE_BUTTON_TYPE
                        || (self.mode == MAP_MODE
                            && (w.kind == mainview::ZOOM_TYPE
                                || mainview::toggle_bit(w.kind).is_some())))
            }) {
                return Some(Target::Control(i));
            }
            return assets
                .views
                .roster_rows
                .iter()
                .position(|r| r.contains(x, y))
                .map(Target::Roster);
        }
        let regions = match self.viewed {
            Viewed::Robot(slot) => self
                .book()
                .get(slot)
                .map(|r| REGION_WIDGETS[r.kind.index()]),
            Viewed::Base => None,
        };
        [
            (CONSOLE_CORNER_TYPE, Target::Corner),
            (PROGRAM_PANEL_TYPE, Target::Panel),
            (UNIT_BUTTON_TYPE, Target::UnitButton),
        ]
        .into_iter()
        .chain(
            regions
                .into_iter()
                .flat_map(|r| [(r[0], Target::Region(0)), (r[1], Target::Region(1))]),
        )
        .find_map(|(kind, target)| hit(kind).then_some(target))
        .or_else(|| {
            // The consoles' buttons whose handlers are ported (T-130 to T-132).
            let kinds: &[u16] = match view {
                View::Humanoid => &INSTRUMENTS[..7],
                View::Scorpion => &INSTRUMENTS[7..16],
                View::Scout => &INSTRUMENTS[16..],
                _ => &[],
            };
            tables
                .widgets
                .iter()
                .chain(&tables.nested)
                .find(|w| kinds.contains(&w.kind) && w.area.is_some_and(|a| a.contains(x, y)))
                .map(|&w| Target::Instrument(w))
        })
    }

    /// A mouse button went down at `at` (`None` off the picture).
    pub fn mouse_down(&mut self, button: MouseButton, at: Option<(u16, u16)>, assets: &Assets) {
        if self.unpause() || self.capture.is_some() || button == MouseButton::Middle {
            return;
        }
        let Some(target) = at.and_then(|at| self.target(assets, at)) else {
            if let (MouseButton::Left, Some(at)) = (button, at) {
                self.map_press(at, assets);
            }
            return;
        };
        self.capture = Some((target, button));
        match target {
            Target::Panel => {
                self.panel_pressed = true;
                self.panel_swap = !self.panel_swap;
            }
            Target::UnitButton => self.unit_pressed = !self.unit_pressed,
            Target::Button(i) => {
                self.button_look = !self.button_look;
                // A power row's arrow holds its slider (+0x2d) until let go.
                if let Some(n) = self.arrow(assets, i) {
                    self.hold_slider(n);
                }
            }
            // The zoom and the mode buttons share the buttons' pressed look.
            Target::Control(i)
                if mainview::toggle_bit(assets.views.situation.nested[i].kind).is_none() =>
            {
                self.button_look = !self.button_look;
            }
            Target::Control(_) => {}
            // The roster selects on the press (`ui_sitroom_roster_on_mouse`).
            Target::Roster(row) if button == MouseButton::Left => {
                let slot = self.robots().get(self.roster_top + row).copied();
                self.select_unit(slot);
            }
            // The map display's Alt press (`1000:9eea`), acted on after the next frame.
            Target::Region(1) if button == MouseButton::Left && self.alt => {
                if let Viewed::Robot(slot) = self.viewed {
                    self.goal_press |= self.display_mode(slot, assets) == MAP_DISPLAY;
                }
            }
            // `1000:a329`, `89b8`: the fire button fires every tick while held (+0x1b =
            // 7); `a3f1`, `8a26`: the lock button's press sets +0x22 bit 7. Both, the
            // scorpion's stop and deploy buttons and the scout's stance buttons flip the
            // look.
            Target::Instrument(w) if FIRE_TYPES.contains(&w.kind) => {
                self.look.pressed = !self.look.pressed;
                self.firing = true;
            }
            Target::Instrument(w) if LOCK_TYPES.contains(&w.kind) => {
                self.look.pressed = !self.look.pressed;
                self.look.lock_held = true;
            }
            Target::Instrument(w) if SHARED_LOOK_TYPES.contains(&w.kind) => {
                self.look.pressed = !self.look.pressed;
            }
            Target::Roster(_) | Target::Corner | Target::Region(_) | Target::Instrument(_) => {}
        }
        self.dirty = true;
    }

    /// A mouse button went up with the pointer at `at`.
    pub fn mouse_up(
        &mut self,
        button: MouseButton,
        at: Option<(u16, u16)>,
        assets: &Assets,
    ) -> Option<Request> {
        let (target, by) = self.capture?;
        if by != button {
            return None;
        }
        self.capture = None;
        // An arrow lets its slider go (+0x2d = 0) on a release over it or on the pointer
        // leaving it (0x400), and either flips the shared look back.
        // shortcut: leaving the arrow while it is held stops the slider only at the
        // release; upgrade if the views get the pointer's leave events.
        if let Target::Button(i) = target
            && self.arrow(assets, i).is_some()
        {
            self.hold_slider(0);
            self.button_look = !self.button_look;
            self.dirty = true;
            return None;
        }
        // So does the fire button (+0x1b = 0); the same shortcut. The scorpion's stop and
        // deploy buttons and the scout's stance buttons flip the look back too, and act
        // only over themselves.
        // shortcut: the scorpion's fire release also clears +0xfd bit 5 (the "no energy"
        // message told), which the port does not; upgrade with T-114's energy messages.
        if let Target::Instrument(w) = target
            && (FIRE_TYPES.contains(&w.kind) || SHARED_LOOK_TYPES.contains(&w.kind))
        {
            self.look.pressed = !self.look.pressed;
            self.dirty = true;
            if FIRE_TYPES.contains(&w.kind) {
                self.firing = false;
                return None;
            }
        }
        // The handlers act only on events over the widget that captured the pointer:
        // released elsewhere, nothing happens and a pressed look stays as it is.
        if at.and_then(|at| self.target(assets, at)) != Some(target) {
            return None;
        }
        let right = button == MouseButton::Right;
        let programs = &assets.programs;
        self.dirty = true;
        match target {
            Target::Panel => {
                // The release clears the panel's state (+0x22 = 0) and redraws it.
                self.panel_pressed = false;
                self.panel_swap = false;
                self.look = crate::console::Look::default();
                let Viewed::Robot(slot) = self.viewed else {
                    return None;
                };
                self.change_orders(programs, |book| {
                    if right {
                        book.select_previous(slot, programs)
                    } else {
                        book.select_next(slot, programs)
                    }
                });
                None
            }
            Target::UnitButton => {
                self.unit_pressed = !self.unit_pressed;
                let Viewed::Robot(slot) = self.viewed else {
                    return None;
                };
                let next = self.step_robot(Some(slot), if right { -1 } else { 1 }, true)?;
                self.view_robot(next).map(Request::Show)
            }
            Target::Corner => match self.viewed {
                Viewed::Robot(_) => Some(self.base()),
                Viewed::Base => Some(Request::End),
            },
            // A release over the view's region (`1000:9d83`, `9eea`) with an object
            // under the pointer in the last frame: the left locks it (`1000:d8a7`), the
            // right opens its console (`1000:7eac`), if it is one of the player's.
            Target::Region(i) => {
                let (Viewed::Robot(slot), true) = (self.viewed, self.region == Some(i)) else {
                    return None;
                };
                // On the second display a left release with Alt takes the press back.
                if i == 1 && !right && self.alt {
                    self.goal_press = false;
                    return None;
                }
                let object = self.probed.get()[i]?;
                if right {
                    // shortcut: only the player's robots can be viewed, so `7eac`'s
                    // message for another side's and its Ctrl developer case are not
                    // ported; upgrade if the port lets the enemy's robots be viewed.
                    return self.view_robot(object).map(Request::Show);
                }
                self.pilot(slot, Pilot::Lock(EntityId(object)));
                None
            }
            Target::Roster(_) if right => self.view_selected(),
            Target::Roster(_) => None,
            Target::Instrument(w) => {
                let Viewed::Robot(slot) = self.viewed else {
                    return None;
                };
                self.instrument(slot, w, right, assets);
                None
            }
            Target::Button(i) => {
                self.button_look = !self.button_look;
                self.situation_button(assets.views.situation_buttons[i].kind)
            }
            Target::Control(i) => {
                let widget = assets.views.situation.nested[i];
                if let Some(bit) = mainview::toggle_bit(widget.kind) {
                    self.flip_overlay(bit);
                    return None;
                }
                self.button_look = !self.button_look;
                match widget.kind {
                    mainview::ZOOM_TYPE => self.zoom(!right),
                    _ if !right => {
                        if let Some(mode) = self.mode_of(&widget) {
                            self.set_mode(mode);
                        }
                    }
                    _ => {}
                }
                None
            }
        }
    }

    /// A situation room button was released over itself.
    fn situation_button(&mut self, kind: u16) -> Option<Request> {
        match kind {
            button::SCROLL_UP => {
                self.roster_top = self.roster_top.saturating_sub(1);
                None
            }
            button::SCROLL_DOWN => {
                if self.roster_top + 1 < self.robots().len() {
                    self.roster_top += 1;
                }
                None
            }
            button::VIEW => self.view_selected(),
            button::END => Some(Request::End),
            button::RECALL => {
                self.recall();
                None
            }
            button::LAUNCH => {
                self.launch();
                None
            }
            _ => {
                log::debug!("situation room: button {kind:#x} acts on the running mission (T-056)");
                None
            }
        }
    }

    /// Draws the view's own parts over its picture, which `fb` already shows.
    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        self.draw_under_dialog(fb, assets);
        self.draw_dialog(fb, assets);
        if let (true, Some(room)) = (self.paused, &assets.sitroom) {
            draw_pause(fb, room, assets.fonts.get(&PAUSE_FONT));
        }
    }

    fn draw_under_dialog(&self, fb: &mut Framebuffer, assets: &Assets) {
        if self.draw_terrain_editor(fb, assets) {
            return;
        }
        let view = self.view();
        let colours: &[u8] = if view == View::SitRoom {
            &VIEW_COLOURS[2..]
        } else {
            &VIEW_COLOURS
        };
        if self.viewed == Viewed::Base && self.mode == ORDERS_MODE {
            self.draw_orders(fb, assets);
        }
        let font = assets.fonts.get(&UNIT_FONT);
        // Measured from the picture before anything is drawn, which can leave the
        // regions' colours behind (the placeholders' labels, a probe's pixel).
        let [main, second] = [255, SECOND_DISPLAY].map(|c| review::region_of(fb, c));
        for &colour in colours {
            if colour == 255 && main.is_some_and(|r| self.draw_view(fb, assets, r)) {
                continue;
            }
            if colour == SECOND_DISPLAY && second.is_some_and(|r| self.draw_second(fb, assets, r)) {
                continue;
            }
            // Without the situation room's tables, its main view (T-103); without the
            // 3D view's, a console's views (T-101); the third displays T-166, T-170
            // and T-173.
            let task = match (view, colour) {
                (View::SitRoom, _) => "T-103",
                (_, 255 | SECOND_DISPLAY) => "T-101",
                (View::Humanoid, _) => "T-166",
                (View::Scorpion, _) => "T-170",
                _ => "T-173",
            };
            hatch(fb, colour, font, task);
        }
        match self.viewed {
            Viewed::Base => self.draw_situation(fb, assets),
            Viewed::Robot(slot) => self.draw_console(fb, assets, slot),
        }
    }

    /// The terrain editor's view over the whole screen; `false` when it is closed or
    /// the tables are missing.
    fn draw_terrain_editor(&self, fb: &mut Framebuffer, assets: &Assets) -> bool {
        let (Some(editor), Some(camera), Some(view), Some(room)) = (
            &self.terrain,
            self.terrain_camera(assets),
            &assets.view3d,
            &assets.sitroom,
        ) else {
            return false;
        };
        let inputs = mainview::Inputs {
            room,
            view,
            sprites: &assets.map_sprites,
            font: None,
        };
        let world = self.world();
        let selected = self.selected.and_then(|s| world.get(EntityId(s)));
        let flags = mainview::flags(self.scenario.as_ref(), self.world());
        let map = self.shown_map();
        editor.draw(fb, &camera, world, &map, &flags, selected, &inputs);
        true
    }

    /// The colour-255 region's view, drawn in a buffer the size of the region and
    /// copied through it: a console's 3D view (T-101), or the situation room's main
    /// view in modes M, R and C (T-103, [`mainview`]). `false` (nothing drawn) without
    /// the tables, when the robot is gone, and in the other modes.
    fn draw_view(&self, fb: &mut Framebuffer, assets: &Assets, region: Region) -> bool {
        if self.viewed == Viewed::Base && self.mode == STATUS_MODE {
            return self.draw_status(fb, assets);
        }
        let Some(view) = &assets.view3d else {
            return false;
        };
        let (left, top, width, height) = region;
        let terrain = self
            .scenario
            .as_ref()
            .and_then(|s| s.terrain.as_ref())
            .map_or(&[][..], |t| &t.features);
        let probe = self.probe_at(left, top);
        let robot_view = |buffer: &mut Framebuffer, slot: u16| {
            let drawn = scene::draw(
                buffer,
                self.world(),
                EntityId(slot),
                view,
                &assets.motion,
                terrain,
                &assets.models,
                assets.skies.get(&self.file),
                probe,
            );
            let [_, second] = self.probed.get();
            self.probed.set([drawn.flatten(), second]);
            drawn.is_some()
        };
        let mut buffer = Framebuffer::new(Size::new(width, height));
        let drawn = match (self.viewed, self.mode, &assets.sitroom) {
            (Viewed::Robot(slot), _, _) => robot_view(&mut buffer, slot),
            (Viewed::Base, MAP_MODE | ROBOT_VIEW_MODE | LOG_MODE, Some(room)) => {
                let inputs = mainview::Inputs {
                    room,
                    view,
                    sprites: &assets.map_sprites,
                    font: assets.fonts.get(&UNIT_FONT),
                };
                let world = self.world();
                let selected = self.selected.and_then(|s| world.get(EntityId(s)));
                match self.mode {
                    MAP_MODE => {
                        let Some(camera) = self.map_camera(assets) else {
                            return false;
                        };
                        let flags = mainview::flags(self.scenario.as_ref(), self.world());
                        let map = self.shown_map();
                        let under = mainview::draw_map(
                            &mut buffer,
                            world,
                            &camera,
                            &map,
                            terrain,
                            &flags,
                            selected,
                            &inputs,
                            [mainview::MAP_BACKGROUND, mainview::MAP_HILL],
                            probe,
                        );
                        // `1000:5619`: the press reads it (`map_press`).
                        let [_, second] = self.probed.get();
                        self.probed.set([under, second]);
                        true
                    }
                    ROBOT_VIEW_MODE => match (self.selected, selected) {
                        (Some(slot), Some(e)) if robot_view(&mut buffer, slot) => {
                            mainview::label_robot_view(&mut buffer, e, &inputs);
                            true
                        }
                        // `1000:55f6`: no unit, the view is cleared to 0.
                        _ => {
                            recolour(fb, self.main_view(assets), 255, 0);
                            return true;
                        }
                    },
                    _ => {
                        recolour(fb, self.main_view(assets), 255, mainview::LOG_CLEAR);
                        let world = self.world();
                        let log = match world.base(Side::Human).and_then(|b| world.get(b)) {
                            Some(e) => match e.state {
                                State::Base { depot, .. } => Some(depot.log),
                                _ => None,
                            },
                            None => None,
                        };
                        let fonts = (
                            assets.fonts.get(&UNIT_FONT),
                            assets.fonts.get(&mainview::LOG_FONT),
                        );
                        if let (Some(log), (Some(heading), Some(rows))) = (log, fonts) {
                            mainview::draw_log(fb, &log, room, heading, rows);
                        }
                        return true;
                    }
                }
            }
            _ => false,
        };
        if drawn {
            review::copy_through(fb, &buffer, left, top);
        }
        drawn
    }

    /// Mode S ([`status`]): the main view cleared to 0 (slot `0x5b4`), then the selected
    /// robot's picture, captions, name and figures, or the no-unit text; `false`
    /// without the tables.
    fn draw_status(&self, fb: &mut Framebuffer, assets: &Assets) -> bool {
        let (Some(room), Some(font)) = (&assets.sitroom, assets.fonts.get(&status::FONT)) else {
            return false;
        };
        recolour(fb, self.main_view(assets), 255, 0);
        let parts = |slot| match self.world().get(EntityId(slot)).map(|e| &e.damage) {
            Some(xeno_sim::Damage::Robot(parts)) => Some(*parts),
            _ => None,
        };
        let unit = self.detail_unit(assets).and_then(|u| {
            Some(status::Unit {
                kind: u.kind,
                serial: u.serial,
                parts: parts(self.selected?)?,
            })
        });
        status::draw(fb, room, font, unit, |h| assets.sprite(h));
        true
    }

    /// The pointer probe's pixel in a view whose region starts at (`left`, `top`): the
    /// pointer less that (`1000:4042`).
    fn probe_at(&self, left: usize, top: usize) -> Option<(usize, usize)> {
        let (x, y) = self.pointer?;
        let x = usize::from(x).checked_sub(left)?;
        Some((x, usize::from(y).checked_sub(top)?))
    }

    /// The open console's second display (the colour-254 region, `1000:dbe8` and the
    /// kinds' draw handlers): the camera turned by the pan at its zoom, or the map
    /// around the robot. `false` (nothing drawn) in the other modes and without the
    /// tables.
    fn draw_second(&self, fb: &mut Framebuffer, assets: &Assets, region: Region) -> bool {
        let Viewed::Robot(slot) = self.viewed else {
            return false;
        };
        let (left, top, width, height) = region;
        let (Some(view), Some(robot)) = (&assets.view3d, self.book().get(slot)) else {
            return false;
        };
        let tables = &assets.views.second_display;
        let d = self.display(slot);
        let mode = self.display_mode(slot, assets);
        let (world, motion) = (self.world(), &assets.motion);
        let terrain = self
            .scenario
            .as_ref()
            .and_then(|s| s.terrain.as_ref())
            .map_or(&[][..], |t| &t.features);
        let mut buffer = Framebuffer::new(Size::new(width, height));
        let probe = self.probe_at(left, top);
        let view_mode = tables.view_modes.get(usize::from(mode)).copied();
        let (under, point) = match view_mode {
            Some(CAMERA_VIEW | THERMAL_VIEW | MISSILE_VIEW) => {
                let id = EntityId(slot);
                let camera = if view_mode == Some(MISSILE_VIEW) {
                    let missile = match world.get(id).map(|e| &e.state) {
                        Some(State::Robot(m)) => m.guns.missile,
                        _ => None,
                    };
                    missile.and_then(|m| scene::missile_camera(world, m, view, motion))
                } else {
                    scene::second_camera(world, id, view, motion, (d.zoom, d.pan))
                };
                match camera {
                    Some(camera) => {
                        let sky = assets.skies.get(&self.file);
                        let models = &assets.models;
                        let under = scene::draw_from(
                            &mut buffer,
                            world,
                            &camera,
                            view,
                            motion,
                            terrain,
                            models,
                            sky,
                            probe,
                        );
                        // Lists 8 and 0xa end with slot `0x5f0`'s remap.
                        if view_mode != Some(CAMERA_VIEW) {
                            remap(buffer.pixels_mut(), &assets.thermal);
                        }
                        (under, None)
                    }
                    // shortcut: until the next 16-tick step breaks the picture up, a
                    // missile camera whose missile is gone shows the noise; the
                    // original's camera reads slot 0xffff then (T-190).
                    None if view_mode == Some(MISSILE_VIEW) => {
                        self.draw_noise(&mut buffer);
                        (None, None)
                    }
                    None => return false,
                }
            }
            Some(MAP_VIEW) => {
                let (Some(room), Some(e)) = (&assets.sitroom, world.get(EntityId(slot))) else {
                    return false;
                };
                let (camera, mut map) = mainview::console_map(e.position, d.map_zoom, view, motion);
                (map.reveal, map.shots) = (self.shown_map().reveal, self.shots);
                // No font: the zoom label is the situation room's.
                let inputs = mainview::Inputs {
                    room,
                    view,
                    sprites: &assets.map_sprites,
                    font: None,
                };
                let flags = mainview::flags(self.scenario.as_ref(), self.world());
                let colours = tables.map_colours[robot.kind.index()];
                let (map_ref, selected) = (&map, Some(e));
                let under = mainview::draw_map(
                    &mut buffer,
                    world,
                    &camera,
                    map_ref,
                    terrain,
                    &flags,
                    selected,
                    &inputs,
                    colours,
                    probe,
                );
                // The display's rectangle, the bottom and right edges outside.
                let (x, y) = (left as u16, top as u16);
                let rect = Rect {
                    top: y,
                    left: x,
                    bottom: y + height as u16,
                    right: x + width as u16,
                };
                let point = self
                    .pointer
                    .map(|at| mainview::pointer_to_world(&camera, rect, at));
                (under, point)
            }
            // View mode 0x18, a damaged device: `1000:3c20`'s noise.
            _ => {
                self.draw_noise(&mut buffer);
                (None, None)
            }
        };
        if let (Some((at, name)), Some(font)) = (
            zoom_name(robot.kind, mode, d, tables),
            assets.fonts.get(&LABEL_FONT),
        ) {
            let mut cursor = TextCursor::at(at.0.into(), at.1.into(), LABEL_COLOUR);
            // shortcut: the names' bit-7 codes in the same colour (T-189 checks the
            // text state).
            cursor.high_colour = LABEL_COLOUR;
            buffer.draw_text(font, &mut cursor, name, TextMode::Transparent);
        }
        let [main, _] = self.probed.get();
        self.probed.set([main, under]);
        self.map_point.set(point);
        review::copy_through_colour(fb, &buffer, (left, top), SECOND_DISPLAY);
        true
    }

    /// A frame of list 0x18's noise from the display's own random word.
    fn draw_noise(&self, buffer: &mut Framebuffer) {
        let word = self.noise.borrow_mut().word();
        noise(buffer.pixels_mut(), word, DISPLAY_CLEAR);
    }

    /// The situation room's main view (widget type 0x50).
    fn main_view(&self, assets: &Assets) -> Rect {
        assets
            .views
            .situation
            .find(MAIN_VIEW_TYPE)
            .and_then(|w| w.area)
            .unwrap_or(Rect {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            })
    }

    /// The orders mode: the main view's colour-255 pixels cleared to 0x10 (slot `0x5b4`,
    /// as we read it), then the briefing's bytes typed so far, transparent, CR LF as the
    /// font's control codes, with no wrapping.
    fn draw_orders(&self, fb: &mut Framebuffer, assets: &Assets) {
        let tables = &assets.views.situation;
        if let Some(area) = tables.find(MAIN_VIEW_TYPE).and_then(|w| w.area) {
            recolour(fb, area, 255, ORDERS_CLEAR);
        }
        if let Some(font) = assets.fonts.get(&ORDERS_FONT) {
            let (x, y) = ORDERS_CURSOR;
            let mut cursor = TextCursor::at(x, y, ORDERS_COLOUR);
            cursor.high_colour = ORDERS_HIGH_COLOUR;
            let typed = &self.orders[..self.typed.min(self.orders.len())];
            fb.draw_text(font, &mut cursor, typed, TextMode::Transparent);
        }
    }

    fn draw_situation(&self, fb: &mut Framebuffer, assets: &Assets) {
        for button in &assets.views.situation_buttons {
            let handle = button.handle + if self.button_look { HANDLE_STEP } else { 0 };
            match assets.sprite(handle) {
                Some(sprite) => {
                    let blit = SpriteBlit {
                        mode: BlitMode::Opaque,
                        ..SpriteBlit::default()
                    };
                    fb.blit_sprite(
                        sprite,
                        button.area.left.into(),
                        button.area.top.into(),
                        blit,
                    );
                }
                None => log::warn!("situation room: no sprite {handle:#x} in msprite1"),
            }
        }
        let (Some(room), Some(font)) = (&assets.sitroom, assets.fonts.get(&UNIT_FONT)) else {
            return;
        };
        let sprite = |h| assets.sprite(h);
        mainview::draw_controls(
            fb,
            &assets.views.situation.nested,
            |w| self.mode_of(w),
            &self.map,
            self.mode,
            self.button_look,
            room,
            sprite,
        );
        let bytes = mainview::roster_bytes(self.scenario.as_ref());
        let world = self.world();
        let robots = self.robots();
        let rows: Vec<_> = (0..mainview::ROSTER_ROWS)
            .map(|row| {
                let &slot = robots.get(self.roster_top + row)?;
                let e = world.get(EntityId(slot));
                let (call_sign, repair) = bytes[usize::from(slot) % bytes.len()];
                Some(mainview::RosterUnit {
                    call_sign,
                    repair,
                    kind: self.book().get(slot).map_or(0, |r| r.kind.index()),
                    in_base: e.is_some_and(mainview::in_base),
                    selected: self.selected == Some(slot),
                })
            })
            .collect();
        mainview::draw_roster(fb, &rows, room, font, sprite);
        self.draw_panels(fb, assets, room);
    }

    /// The status counts, the reserve bar, the clock and the power rows ([`panels`]),
    /// from the player's base and the mission clock.
    fn draw_panels(&self, fb: &mut Framebuffer, assets: &Assets, room: &SitRoom) {
        let tables = &assets.views.situation;
        let area = |kind| tables.find(kind).and_then(|w| w.area);
        let Some(font) = assets.fonts.get(&panels::PANEL_FONT) else {
            return;
        };
        let world = self.world();
        let base = world.base(Side::Human).and_then(|b| world.get(b));
        if let Some(State::Base { counts, depot, .. }) = base.map(|e| e.state) {
            let mut all = [depot.relays; 8];
            all[1..].copy_from_slice(&counts);
            panels::draw_counts(fb, font, all);
            if let Some(a) = area(panels::RESERVE_TYPE) {
                panels::draw_bar(fb, a, depot.power[0], room, panels::RESERVE_REST);
            }
            for (kind, &value) in panels::BAR_TYPES.into_iter().zip(&depot.power[1..]) {
                if let Some(a) = area(kind) {
                    panels::draw_bar(fb, a, value, room, panels::BAR_REST);
                }
            }
        }
        if let Some(a) = area(panels::CLOCK_TYPE) {
            let c = world.clock;
            let text = room.clock_of(c.month, c.day, c.hour, c.minute);
            panels::draw_clock(fb, a, font, &text, c.second);
        }
        if let Some(font) = assets.fonts.get(&panels::DETAIL_FONT) {
            let unit = self.detail_unit(assets);
            panels::draw_detail(fb, font, &room.detail, unit.as_ref());
        }
    }

    /// The selected unit (+0x3c) as the detail panel reads it; none without one.
    fn detail_unit<'a>(&self, assets: &'a Assets) -> Option<panels::DetailUnit<'a>> {
        let slot = self.selected?;
        let world = self.world();
        let e = world.get(EntityId(slot))?;
        let (xeno_sim::Damage::Robot(c), State::Robot(m)) = (&e.damage, &e.state) else {
            return None;
        };
        let robot = self.book().get(slot)?;
        let base = world.base(Side::Human).and_then(|b| world.get(b));
        let repairing = matches!(base.map(|b| &b.state),
            Some(State::Base { depot, .. }) if depot.repairing == Some(EntityId(slot)));
        // shortcut: the serial (+0x18) is the scenario's as loaded (robots made since
        // show 0): the sim does not keep it.
        let scenario = self.scenario.as_ref();
        let objects = scenario
            .into_iter()
            .flat_map(|s| s.objects.iter().flatten());
        let loaded = objects.filter(|o| o.slot == slot).find_map(|o| o.robot());
        let orders = robot.orders;
        Some(panels::DetailUnit {
            kind: robot.kind.index(),
            serial: loaded.map_or(0, |r| r.serial),
            in_base: mainview::in_base(e),
            repairing,
            total: c.total,
            program: orders
                .program_drives
                .then(|| assets.programs.name(orders.program)),
            heading: e.heading,
            speed: m.speed,
            signal: m.signal,
        })
    }

    /// The slider a power row's arrow (sprite button `i`) runs while held, by its row
    /// (its state's second byte).
    fn arrow(&self, assets: &Assets, i: usize) -> Option<u8> {
        let b = assets.views.situation_buttons.get(i)?;
        let tables = &assets.views.situation;
        let w = tables.all(b.kind).find(|w| w.area == Some(b.area))?;
        panels::slider(b.kind, self.mode_of(w)?)
    }

    /// The player's base holds slider `n` (`Sim::hold_slider`; 0 lets go).
    fn hold_slider(&mut self, n: u8) {
        if let Run::Running(sim) = &mut self.run {
            sim.hold_slider(Side::Human, n);
        }
    }

    /// A mode button's mode, or a power row's arrow's row: the second byte of its state
    /// in the base's view state.
    fn mode_of(&self, widget: &views::Widget) -> Option<u8> {
        let base = mainview::base_of(self.scenario.as_ref(), Side::Human)?;
        base.view_state
            .0
            .get(usize::from(widget.state) + 1)
            .copied()
    }

    fn draw_console(&self, fb: &mut Framebuffer, assets: &Assets, slot: u16) {
        let Some(robot) = self.book().get(slot) else {
            return;
        };
        let tables = assets.console_tables(robot.kind);
        if let Some(area) = tables.find(PROGRAM_PANEL_TYPE).and_then(|w| w.area) {
            let style = assets.views.program_panel[robot.kind.index()];
            if self.panel_pressed {
                // `gfx_recolour_rect`: one pressed colour becomes the other.
                let [a, b] = style.pressed;
                let (from, to) = if self.panel_swap { (b, a) } else { (a, b) };
                recolour(fb, area, from, to);
            } else {
                fill(fb, area, style.fill);
                let colour = if robot.orders.program_drives {
                    style.program_text
                } else {
                    assets.views.pilot_text
                };
                if let Some(font) = assets.fonts.get(&style.font) {
                    let mut cursor = TextCursor::at(
                        i32::from(area.left + style.x_offset),
                        i32::from(area.top) + 1,
                        colour,
                    );
                    let name = assets.programs.name(robot.orders.program);
                    fb.draw_text(font, &mut cursor, name, TextMode::Transparent);
                }
            }
        }
        // `1000:d241`: the panel filled, then the robot's text typed from its corner.
        if let (Some(area), Some(State::Robot(m))) = (
            tables.find(MESSAGE_PANEL_TYPE).and_then(|w| w.area),
            self.world().get(EntityId(slot)).map(|e| &e.state),
        ) {
            let style = assets.views.message_panel;
            let [background, colour] = style.colours[robot.kind.index()];
            fill(fb, area, background);
            if let Some(font) = assets.fonts.get(&style.font) {
                let mut cursor = TextCursor::at(area.left.into(), area.top.into(), colour);
                cursor.high_colour = style.high;
                fb.draw_text(font, &mut cursor, m.panel.shown(), TextMode::Transparent);
            }
        }
        // `1000:7d55`: the call sign (`1fe6:7fd5`) at the button's place.
        if let (Some(font), Some(room)) = (assets.fonts.get(&UNIT_FONT), &assets.sitroom) {
            let (x, y) = assets.views.unit_label;
            let mut cursor = TextCursor::at(x.into(), y.into(), UNIT_COLOUR);
            let bytes = mainview::roster_bytes(self.scenario.as_ref());
            let call_sign = room.call_sign(bytes[usize::from(slot) % bytes.len()].0);
            fb.draw_text(font, &mut cursor, call_sign, TextMode::Transparent);
        }
        if self.unit_pressed
            && let Some(area) = tables.find(UNIT_BUTTON_TYPE).and_then(|w| w.area)
        {
            xor_frame(fb, area, UNIT_PRESSED_XOR);
        }
        match robot.kind {
            RobotKind::Humanoid => self.draw_humanoid(fb, assets, slot),
            RobotKind::Scorpion => self.draw_scorpion(fb, assets, slot),
            RobotKind::Scout => self.draw_scout(fb, assets, slot),
        }
    }

    /// The humanoid console's instruments ([`humancon`]) from the sim.
    fn draw_humanoid(&self, fb: &mut Framebuffer, assets: &Assets, slot: u16) {
        let (Some(con), Some(room), Some(font)) = (
            &assets.humancon,
            &assets.sitroom,
            assets.fonts.get(&crate::console::FONT),
        ) else {
            return;
        };
        let Some(robot) = self.instruments(slot, room, &con.base_name) else {
            return;
        };
        let tables = &assets.views.humanoid;
        let area = |kind| tables.all(kind).next().and_then(|w| w.area);
        let lookup = |h| crate::console::lookup(&assets.humancon_sprites, con.bank, h);
        let sprites: crate::console::Sprites = &lookup;
        use crate::humancon as h;
        h::draw_power(fb, con, sprites, &robot);
        if let Some(a) = area(h::WARNING_TYPE) {
            h::draw_warning(fb, a, sprites, &robot);
        }
        h::draw_speed(fb, font, con, &room.detail.speeds, robot.speed);
        h::draw_compass(fb, font, con, sprites, robot.heading);
        if let Some(a) = area(h::GEAR_TYPE) {
            h::draw_gear(fb, a, sprites, robot.gear);
        }
        h::draw_target(fb, font, con, &robot);
        if let Some(a) = area(h::STATUS_TYPE) {
            let colours = assets.views.message_panel.colours[RobotKind::Humanoid.index()];
            h::draw_status(fb, a, font, con, colours, &robot);
        }
        let widgets = tables.widgets.iter().chain(&tables.nested);
        for w in widgets {
            let Some(a) = w.area else { continue };
            let weapon = self.widget_byte(slot, w).unwrap_or(0);
            h::draw_button(fb, sprites, con, (w.kind, a, weapon), &robot, self.look);
            // `1000:dfd5`: a switch held shows its frame XORed.
            if self
                .capture
                .is_some_and(|(t, _)| t == Target::Instrument(*w))
                && matches!(w.kind, h::WEAPON_TYPE | h::PILOT_TYPE | h::PROGRAM_TYPE)
            {
                xor_frame(fb, a, crate::console::PRESSED_XOR);
            }
        }
    }

    /// The scorpion console's instruments ([`scorpcon`]) from the sim.
    fn draw_scorpion(&self, fb: &mut Framebuffer, assets: &Assets, slot: u16) {
        let (Some(con), Some(room), Some(font)) = (
            &assets.scorpcon,
            &assets.sitroom,
            assets.fonts.get(&crate::console::FONT),
        ) else {
            return;
        };
        let name = assets.humancon.as_ref().map_or(&[0u8; 6], |h| &h.base_name);
        let Some(robot) = self.instruments(slot, room, name) else {
            return;
        };
        let tables = &assets.views.scorpion;
        let area = |kind| tables.all(kind).next().and_then(|w| w.area);
        let lookup = |h| crate::console::lookup(&assets.scorpcon_sprites, con.bank, h);
        let sprites: crate::console::Sprites = &lookup;
        use crate::scorpcon as s;
        s::draw_power(fb, con, sprites, &robot);
        if let Some(a) = area(s::WARNING_TYPE) {
            s::draw_warning(fb, a, sprites, &robot);
        }
        s::draw_speed(fb, font, con, &room.detail.speeds, robot.speed);
        if let Some(a) = area(s::LEVER_TYPE) {
            s::draw_lever(fb, a, con, sprites, robot.speed);
        }
        s::draw_compass(fb, font, con, sprites, robot.heading);
        if let Some(a) = area(s::DAMAGE_TYPE) {
            s::draw_damage(fb, a, con, sprites, &robot);
        }
        for (kind, gauge) in s::GAUGE_TYPES.into_iter().zip(robot.gauges) {
            if let Some(a) = area(kind) {
                s::draw_gauge(fb, a, sprites, gauge);
            }
        }
        s::draw_target(fb, font, con, &robot);
        if let Some(a) = area(s::RELAYS_TYPE) {
            s::draw_relays(fb, a, font, con, robot.relays);
        }
        for w in tables.widgets.iter().chain(&tables.nested) {
            let Some(a) = w.area else { continue };
            let n = self.widget_byte(slot, w).unwrap_or(0);
            let held = self
                .capture
                .is_some_and(|(t, _)| t == Target::Instrument(*w));
            s::draw_button(fb, sprites, con, (w.kind, a, n), &robot, (self.look, held));
            // `1000:dfd5`: a weapon button or the relay panel held shows its frame XORed.
            if held && matches!(w.kind, s::WEAPON_TYPE | s::RELAYS_TYPE) {
                xor_frame(fb, a, crate::console::PRESSED_XOR);
            }
        }
    }

    /// The scout console's instruments ([`scoutcon`]) from the sim.
    fn draw_scout(&self, fb: &mut Framebuffer, assets: &Assets, slot: u16) {
        let (Some(con), Some(room), Some(font)) = (
            &assets.scoutcon,
            &assets.sitroom,
            assets.fonts.get(&crate::console::FONT),
        ) else {
            return;
        };
        let name = assets.humancon.as_ref().map_or(&[0u8; 6], |h| &h.base_name);
        let Some(robot) = self.instruments(slot, room, name) else {
            return;
        };
        let tables = &assets.views.scout;
        let area = |kind| tables.all(kind).next().and_then(|w| w.area);
        let lookup = |h| crate::console::lookup(&assets.scoutcon_sprites, con.bank, h);
        let sprites: crate::console::Sprites = &lookup;
        use crate::scoutcon as c;
        c::draw_power(fb, con, sprites, &robot);
        if let Some(a) = area(c::WARNING_TYPE) {
            c::draw_warning(fb, a, sprites, &robot);
        }
        c::draw_speed(fb, font, con, &room.detail.speeds, robot.speed);
        c::draw_compass(fb, font, con, sprites, robot.heading);
        if let Some(a) = area(c::GEAR_TYPE) {
            c::draw_gear(fb, a, sprites, robot.gear);
        }
        if let Some(a) = area(c::STANCE_TYPE) {
            c::draw_stance(fb, a, sprites, robot.stance);
        }
        c::draw_target(fb, font, con, &robot);
        if let (Some(a), Some(human)) = (area(c::STATUS_TYPE), &assets.humancon) {
            let colours = assets.views.message_panel.colours[RobotKind::Scout.index()];
            let texts = (&human.ok[..], &human.out[..]);
            c::draw_status(fb, (a, font), con, texts, colours, &robot);
        }
        for w in tables.widgets.iter().chain(&tables.nested) {
            let Some(a) = w.area else { continue };
            let n = self.widget_byte(slot, w).unwrap_or(0);
            let held = self
                .capture
                .is_some_and(|(t, _)| t == Target::Instrument(*w));
            c::draw_button(fb, sprites, con, (w.kind, a, n), &robot, (self.look, held));
            // `1000:dfd5`: a switch held shows its frame XORed.
            if held && matches!(w.kind, c::PILOT_TYPE | c::PROGRAM_TYPE) {
                xor_frame(fb, a, crate::console::PRESSED_XOR);
            }
        }
    }

    /// The second byte of `widget`'s state in robot `slot`'s widget state (humanoid
    /// +0x10c, scorpion +0x127, scout +0x10d), as loaded: a weapon or drop button's number. The sim
    /// never changes it.
    fn widget_byte(&self, slot: u16, widget: &views::Widget) -> Option<u8> {
        let widgets = match &self.loaded_robot(slot)?.parts {
            usg::Parts::Humanoid(p) => &p.widgets.0[..],
            usg::Parts::Scorpion(p) => &p.widgets.0[..],
            usg::Parts::Scout(p) => &p.widgets.0[..],
        };
        widgets.get(usize::from(widget.state) + 1).copied()
    }

    /// Robot `slot` as the scenario loaded it.
    fn loaded_robot(&self, slot: u16) -> Option<&usg::Robot> {
        self.scenario
            .iter()
            .flat_map(|s| s.objects.iter().flatten())
            .filter(|o| o.slot == slot)
            .find_map(|o| o.robot())
    }

    /// Robot `slot` as its console's instruments read it.
    fn instruments(
        &self,
        slot: u16,
        room: &SitRoom,
        base_name: &[u8; 6],
    ) -> Option<crate::console::Robot> {
        let world = self.world();
        let e = world.get(EntityId(slot))?;
        let (xeno_sim::Damage::Robot(c), State::Robot(m)) = (&e.damage, &e.state) else {
            return None;
        };
        // shortcut: +0xee, the status page and a scorpion's +0xfa bit 7 are worked out from the passes since the load
        // (`console::blink`, `console::page`, T-167).
        let loaded = self.loaded_robot(slot);
        let passes = world.ticks;
        let (blink, page) = loaded.map_or((0, 0), |r| {
            // The humanoid's three pages of +0x10a, the scout's four of +0x108.
            let (light, pages) = match &r.parts {
                usg::Parts::Humanoid(p) => (p.light, 3),
                usg::Parts::Scout(p) => (p.light, 4),
                usg::Parts::Scorpion(_) => (0, 1),
            };
            (
                crate::console::blink(r.tick, r.blink, passes),
                crate::console::page(r.tick, r.clock, light, passes, pages),
            )
        });
        // The tick table's entry 4 flips +0xfa bit 7 (`1000:9a14`); +0x119 is the
        // scenario's (nothing changes it).
        let (damage_shown, extra) = loaded.map_or((false, 0), |r| {
            let flips = crate::console::calls(r.tick, passes) % 2 == 1;
            let extra = match &r.parts {
                usg::Parts::Scorpion(p) => p.unknown_119,
                _ => 0,
            };
            ((r.clock & 0x80 != 0) != flips, extra)
        });
        // `1fe6:7f38`: a robot's letters, `-` and its serial's last three digits; a
        // base's name. shortcut: a robot's serial (+0x18) is the scenario's as loaded,
        // and other kinds (never locked in the stock scenarios) get no name.
        let target = m.target.and_then(|t| {
            let name = match world.get(t)?.kind {
                usg::ObjectKind::Base => base_name.to_vec(),
                kind => {
                    let index = RobotKind::of(kind)?.index();
                    let mut name = room.detail.kind_letters[index].to_vec();
                    name.push(b'-');
                    let serial = self.loaded_robot(t.0).map_or(0, |r| r.serial);
                    name.extend(panels::digits(serial & 0x7fff, 3));
                    name
                }
            };
            Some([name, vec![0x0c]].concat())
        });
        Some(crate::console::Robot {
            heading: e.heading,
            speed: m.speed,
            gear: m.gear,
            on_battery: c.equipment & usg::Robot::ON_BATTERY != 0,
            signal: m.signal,
            energy: m.guns.energy,
            locked_on: m.locked_on,
            blink,
            target,
            target_distance: m.target_distance,
            target_bearing: m.target_bearing.wrapping_sub(e.heading),
            targeting: c.targeting,
            weapons: c.weapons,
            cooldowns: m.guns.cooldown,
            weapon: m.weapon,
            power: c.power,
            helm: c.helm,
            systems: c.systems,
            cloaks: m.guns.cloaks,
            ecm: m.guns.ecm,
            program_drives: self
                .book()
                .get(slot)
                .is_some_and(|r| r.orders.program_drives),
            page,
            relays: m.relays,
            rounds: m.guns.rounds,
            gauges: crate::scorpcon::gauges(m.relays, extra),
            deployer_damaged: c.equipment & usg::Robot::DEPLOYER_DAMAGED != 0,
            damage_shown,
            stance: m.stance,
            drops: m.guns.drops,
        })
    }
}

impl Assets {
    fn console_tables(&self, kind: RobotKind) -> &ViewTables {
        self.views.console(kind)
    }
}

fn mode(program_drives: bool) -> &'static str {
    if program_drives { "program" } else { "pilot" }
}

/// The console of a robot kind.
pub fn view_of(kind: RobotKind) -> View {
    match kind {
        RobotKind::Humanoid => View::Humanoid,
        RobotKind::Scorpion => View::Scorpion,
        RobotKind::Scout => View::Scout,
    }
}

fn pixels(area: Rect) -> impl Iterator<Item = (usize, usize)> {
    (area.top..area.bottom)
        .flat_map(move |y| (area.left..area.right).map(move |x| (usize::from(x), usize::from(y))))
}

fn set(fb: &mut Framebuffer, (x, y): (usize, usize), colour: u8) {
    let width = fb.surface().width;
    if let Some(p) = fb.pixels_mut().get_mut(y * width + x) {
        *p = colour;
    }
}

/// Slot `0x51a`: the rectangle in one colour.
pub fn fill(fb: &mut Framebuffer, area: Rect, colour: u8) {
    for at in pixels(area) {
        set(fb, at, colour);
    }
}

/// Slot `0x522` (`gfx_recolour_rect`): the rectangle's pixels of colour `from` become
/// `to`.
pub fn recolour(fb: &mut Framebuffer, area: Rect, from: u8, to: u8) {
    for (x, y) in pixels(area) {
        if fb.get(x, y) == Some(from) {
            set(fb, (x, y), to);
        }
    }
}

/// The rectangle's one-pixel frame, inside its bottom and right edges.
fn frame_pixels(area: Rect) -> impl Iterator<Item = (usize, usize)> {
    pixels(area).filter(move |&(x, y)| {
        x == usize::from(area.left)
            || y == usize::from(area.top)
            || x + 1 == usize::from(area.right)
            || y + 1 == usize::from(area.bottom)
    })
}

/// F1's box (`1000:cff3`): filled in 0, framed in 0x0f, and its text typed in font 0,
/// colour 0x0f, transparent, 4 pixels in from its top-left corner.
const PAUSE_FONT: u16 = 0;
const PAUSE_FILL: u8 = 0;
const PAUSE_COLOUR: u8 = 0x0f;
const PAUSE_INSET: u16 = 4;

fn draw_pause(fb: &mut Framebuffer, room: &SitRoom, font: Option<&Font>) {
    let area = room.pause_box;
    fill(fb, area, PAUSE_FILL);
    frame(fb, area, PAUSE_COLOUR);
    if let Some(font) = font {
        let (x, y) = (area.left + PAUSE_INSET, area.top + PAUSE_INSET);
        let mut cursor = TextCursor::at(x.into(), y.into(), PAUSE_COLOUR);
        fb.draw_text(font, &mut cursor, &room.pause_text, TextMode::Transparent);
    }
}

/// Slot `0x52a` (`gfx_frame_rect`): the rectangle's frame in `colour`.
pub fn frame(fb: &mut Framebuffer, area: Rect, colour: u8) {
    for at in frame_pixels(area) {
        set(fb, at, colour);
    }
}

/// Slot `0x5f0` (`mgavideo.dvr` 0x28f6): every pixel through the thermal table.
fn remap(pixels: &mut [u8], table: &[u8]) {
    for p in pixels {
        *p = table.get(usize::from(*p)).copied().unwrap_or(*p);
    }
}

/// List 0x18's step (`1000:3c20`, `docs/screens.md`, "The display's other modes
/// (T-134)"): a shift register started from the random `word` visits every pixel but
/// the first, each getting `clear` plus a count down by one, modulo 16; then the first
/// gets `clear`.
fn noise(pixels: &mut [u8], word: u16, clear: u8) {
    let n = pixels.len();
    // The register never reaches 0, so one pixel has nothing to visit.
    if n < 2 {
        pixels.fill(clear);
        return;
    }
    const MASKS: [(u16, u16); 6] = [
        (0x8000, 0xb400),
        (0x4000, 0x6000),
        (0x2000, 0x3500),
        (0x1000, 0x1b00),
        (0x0800, 0x0ca0),
        (0x0400, 0x0500),
    ];
    let size = n as u16;
    let mask = MASKS
        .iter()
        .find(|&&(bit, _)| size & bit != 0)
        .map_or(0x0240, |&(_, m)| m);
    let mut register = (word % mask) | 1;
    let mut colour = register as u8;
    for _ in 0..n {
        loop {
            let out = register & 1 != 0;
            register >>= 1;
            if out {
                register ^= mask;
            }
            if usize::from(register) < n {
                break;
            }
        }
        colour = (colour.wrapping_sub(1) & 0xf).wrapping_add(clear);
        pixels[usize::from(register)] = colour;
    }
    pixels[0] = clear;
}

/// Where the open console's overlay puts the zoom's name, and the name, while it is
/// shown (+0xf3): in the camera modes the camera's zoom, in the map mode the map's;
/// the humanoid's and the scorpion's none in the missile camera and the damaged modes,
/// the scout's the camera's in every mode but the map (`1000:9e79`, `9ec5`, `81d6`,
/// `8222`, `b632`).
fn zoom_name(
    kind: RobotKind,
    mode: u8,
    d: Display,
    t: &views::SecondDisplay,
) -> Option<((u16, u16), &[u8])> {
    if d.label == 0 || (kind != RobotKind::Scout && mode > MAP_DISPLAY) {
        return None;
    }
    let map = mode == MAP_DISPLAY;
    let (names, zoom) = match map {
        true => (&t.map_zoom_names, d.map_zoom),
        false => (&t.zoom_names, d.zoom),
    };
    let at = t.zoom_name_at[kind.index()][usize::from(map)];
    Some((at, names.get(usize::from(zoom))?))
}

/// Slot `0x526` (`mgavideo.dvr` 0x0f34): the rectangle's pixels XORed with `value`.
pub fn xor_fill(fb: &mut Framebuffer, area: Rect, value: u8) {
    for (x, y) in pixels(area) {
        if let Some(p) = fb.get(x, y) {
            set(fb, (x, y), p ^ value);
        }
    }
}

/// Slot `0x536` (`gfx_xor_rect_frame`): the rectangle's frame XORed with `value`.
pub fn xor_frame(fb: &mut Framebuffer, area: Rect, value: u8) {
    for (x, y) in frame_pixels(area) {
        if let Some(p) = fb.get(x, y) {
            set(fb, (x, y), p ^ value);
        }
    }
}

/// The port's placeholder over the pixels of `colour`, where the mission would draw:
/// a sparse grid of the colour on black, and `label` (the task that fills it) at the
/// region's top-left corner.
pub fn hatch(fb: &mut Framebuffer, colour: u8, font: Option<&Font>, label: &str) {
    let view = fb.view();
    let mut corner: Option<(usize, usize)> = None;
    for y in 0..view.height {
        for x in 0..view.width {
            if fb.get(x, y) == Some(colour) {
                corner.get_or_insert((x, y));
                let ink = if (x + y) % 4 == 0 {
                    colour
                } else {
                    PLACEHOLDER_INK
                };
                set(fb, (x, y), ink);
            }
        }
    }
    if let (Some((x, y)), Some(font)) = (corner, font) {
        let mut cursor = TextCursor::at(x as i32 + 2, y as i32 + 2, PLACEHOLDER);
        fb.draw_text(font, &mut cursor, label.as_bytes(), TextMode::Opaque);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::editor::handler as editor_handler;
    use xeno_formats::fixtures::{synthetic_mission_usg, synthetic_programs, synthetic_views};
    use xeno_render::Size;
    use xeno_sim::orders::Orders;

    const DS: usize = 0x20000;
    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    /// The synthetic views and programs of `xeno_formats::fixtures`, the synthetic
    /// fonts, 80 raw 2x2 sprites (every pixel its number + 1), and the synthetic
    /// mission with an alien base added as `atrain.usg`, the training menu's mission, and as every campaign
    /// scenario.
    pub fn test_assets() -> Assets {
        use xeno_formats::ban::SpriteHeader;
        use xeno_formats::fixtures::synthetic_font_block;
        let mut image = vec![0u8; DS + 0x800];
        let relocated = synthetic_views(&mut image, DS);
        let mut programs = vec![0u8; DS + 0x9c00];
        synthetic_programs(&mut programs, DS);
        let fonts = xeno_formats::font::parse(&synthetic_font_block()).unwrap();
        let sprites = (0..80u8)
            .map(|n| Sprite {
                header: SpriteHeader {
                    size: 10,
                    width: 2,
                    height: 2,
                    flags: 0,
                    unknown_05: 8,
                },
                pixels: vec![n + 1; 4],
                spans: Vec::new(),
                trailing_empty_span: false,
            })
            .collect();
        let mut scenario = usg::parse(&synthetic_mission_usg()).unwrap();
        // An alien base in slot 1, so either side's base can be destroyed, and both
        // with their end scripts at the start, as in stock files.
        let objects = scenario.objects.as_mut().unwrap();
        if let usg::Body::Base(b) = &mut objects[0].body {
            b.end_step = 0;
        }
        let mut alien = objects[0].clone();
        (alien.slot, alien.flags) = (1, alien.flags | usg::flag::ALIEN);
        objects.insert(1, alien);
        // The consoles' region widgets (T-106): the 3D view over rows 40 to 99, the
        // second display 100 to 129, the third 130 to 159.
        let mut views = views::read_image(&image, DS, &relocated).unwrap();
        let region = |kind, top, bottom| views::Widget {
            kind,
            state: 0,
            area: Some(Rect {
                top,
                left: 40,
                bottom,
                right: 280,
            }),
        };
        for (view, kinds) in [&mut views.humanoid, &mut views.scorpion, &mut views.scout]
            .into_iter()
            .zip(REGION_WIDGETS)
        {
            for (kind, (top, bottom)) in kinds.into_iter().zip([(40, 100), (100, 130), (130, 160)])
            {
                view.widgets.push(region(kind, top, bottom));
            }
        }
        Assets {
            views,
            training: views::read_training_image(&image, DS, &relocated).unwrap(),
            programs: orders::read_image(&programs, DS).unwrap(),
            fonts: [0, 2, ORDERS_FONT]
                .into_iter()
                .map(|id| (id, fonts[0].clone()))
                .collect(),
            sprites,
            scenarios: std::iter::once("atrain.usg".to_string())
                .chain((0..13).map(scenario_file))
                .map(|name| (name, scenario.clone()))
                .collect(),
            editor: None,
            editor_unlocked: false,
            developer_unlocked: false,
            map_driver: None,
            save_dir: PathBuf::new(),
            view3d: None,
            models: Vec::new(),
            skies: BTreeMap::new(),
            sitroom: None,
            map_sprites: Vec::new(),
            file_dialogs: None,
            humancon: None,
            humancon_sprites: Vec::new(),
            scorpcon: None,
            scorpcon_sprites: Vec::new(),
            scoutcon: None,
            scoutcon_sprites: Vec::new(),
            // Each colour turned to 255 less it: not the stock table.
            thermal: (0..=255u8).rev().collect(),
            motion: motion::Motion {
                sine: vec![0; motion::SINE_ENTRIES],
                turn_step: [0x5b; 3],
                weak_drive: 0x28,
                scorpion_speeds: vec![[0; motion::SPEED_ENTRIES]],
                mission_programs: [[0; 3]; motion::MISSIONS],
                atan: vec![0; motion::ATAN_ENTRIES],
                script_speeds: [[0; 3]; 5],
                gears: motion::Gears {
                    humanoid: vec![-20, 0, 20],
                    scout: vec![-30, 0, 30],
                    scorpion_step: 5,
                    scorpion_top: 25,
                    scorpion_bottom: -15,
                    turn: 0x80,
                },
                weapon_keys: motion::WeaponKeys {
                    sound: 27,
                    selected: [[1, 2], [3, 4], [5, 6]],
                    scorpion_empty: [7, 8],
                    scorpion_damaged: [9, 10],
                    scorpion_bits: [1, 2],
                },
                recall_program: 4,
                scripts: {
                    let mut image = vec![0; 0x29c00];
                    xeno_formats::fixtures::synthetic_scripts(&mut image, 0x20000);
                    xeno_formats::script::read_image(&image, 0x20000).unwrap()
                },
                damage: xeno_formats::fixtures::synthetic_damage_tables(),
                sensing: xeno_formats::fixtures::synthetic_sensing_tables(),
                net: xeno_formats::fixtures::synthetic_net_tables(),
                relays: xeno_formats::fixtures::synthetic_relay_tables(),
                weapons: xeno_formats::fixtures::synthetic_weapon_tables(),
                bases: xeno_formats::fixtures::synthetic_base_tables(),
                calendar: xeno_formats::fixtures::synthetic_calendar_tables(),
                messages: xeno_formats::fixtures::synthetic_messages_tables(),
                // Legs that never leave frame 0: no footsteps; heights with no bob.
                walk: motion::Walk {
                    humanoid_legs: [[0; motion::STEPS]; 2],
                    humanoid_arms: [[0; motion::STEPS]; 2],
                    scout_legs: [[0; motion::STEPS]; 2],
                    scout_crouch: [0; motion::CROUCHES],
                    scorpion_legs: [[0; motion::STEPS]; motion::SCORPION_LEGS],
                    humanoid_height: 0x0a00,
                    humanoid_bob: [0; motion::STEPS],
                    humanoid_eye: 0x0f80,
                    scout_height: 0x0c80,
                    scout_bob: [0; motion::STEPS],
                    scout_crouched: 0x0c80,
                    scout_eye: 0x0d00,
                },
                strings: motion::Strings::default(),
            },
        }
    }

    fn mission(assets: &Assets) -> Mission {
        let (file, scenario) = assets.sample(None).unwrap();
        Mission::new(file, scenario, Random::seeded(1), &assets.motion)
    }

    /// Each tick of the world asks for the views again, so a console's 3D view and the
    /// map follow the robots with no input.
    #[test]
    fn a_tick_redraws_the_views() {
        let assets = test_assets();
        let mut m = mission(&assets);
        m.take_dirty();
        m.tick(&[]);
        assert!(m.take_dirty());
        assert!(!m.take_dirty());
    }

    /// A save carries the views' state the mission starts from: the situation room's
    /// mode and map in the player's base, each robot's second display.
    #[test]
    fn a_saved_mission_starts_in_the_views_it_was_left_in() {
        let assets = test_assets();
        let mut m = mission(&assets);
        m.mode = 6;
        (m.map.zoom, m.map.overlays) = (3, usg::Base::MAP_GRID);
        m.displays.get_mut(&2).unwrap().mode = 1;
        let saved = m.saved(&assets).unwrap();
        assert_eq!(saved.save, None);
        let again = Mission::new(m.file.clone(), &saved, Random::seeded(1), &assets.motion);
        assert_eq!(
            (again.mode, again.map.zoom, again.map.overlays),
            (6, 3, 0x10)
        );
        assert_eq!(again.displays[&2], m.displays[&2]);
        assert_eq!(again.world().clock, m.world().clock);
    }

    /// The tint starts once per weight, again after a view is shown, and not in the
    /// situation room's mode 2.
    #[test]
    fn the_tint_starts_when_the_clock_s_weight_changes() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let set_hour = |m: &mut Mission, hour| match &mut m.run {
            Run::Running(sim) => sim.world.clock.hour = hour,
            Run::Parked(_) => unreachable!(),
        };
        set_hour(&mut m, 3);
        assert_eq!(m.tint(), Some(0x80));
        assert_eq!(m.tint(), None, "the same weight is not restarted");
        set_hour(&mut m, 12);
        assert_eq!(m.tint(), Some(0));
        m.restart_tint();
        assert_eq!(m.tint(), Some(0));
        m.mode = STATUS_MODE;
        m.restart_tint();
        assert_eq!(m.tint(), None);
        m.mode = ORDERS_MODE;
        assert_eq!(m.tint(), Some(0));
    }

    /// Mode S starts the selected robot's kind's record once a redraw, and only there.
    #[test]
    fn mode_s_starts_its_record_on_a_redraw() {
        let mut assets = test_assets();
        assets.sitroom = Some(crate::mainview::tests::room());
        let mut m = mission(&assets);
        let robot = m.robots()[0];
        let record = assets.sitroom.as_ref().unwrap().status.records
            [m.book().get(robot).unwrap().kind.index()];
        m.set_mode(MAP_MODE);
        m.select_unit(Some(robot));
        assert_eq!(m.status_record(&assets), None, "not in mode M");
        m.set_mode(STATUS_MODE);
        assert_eq!(m.status_record(&assets), Some(record));
        assert_eq!(m.status_record(&assets), None, "once a redraw");
        m.select_unit(Some(robot));
        assert_eq!(m.status_record(&assets), Some(record));
    }

    /// With unlock word 9 the editor's keys act in the situation room; its terrain
    /// key and Ctrl+Alt+P's save write files the parsers read back.
    #[test]
    fn the_editor_s_keys_change_and_save_the_scenario() {
        let dir = std::env::temp_dir().join(format!("xeno-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut assets = test_assets();
        let tables = |keys| editor::Editor {
            keys,
            kind_names: vec![None; editor::KINDS],
            side_names: [b"H".to_vec(), b"A".to_vec()],
            show_names: [b"N".to_vec(), b"Y".to_vec()],
            mission_names: vec![b"M".to_vec(); 8],
            mission_defaults: vec![[0; 4]; 7],
            mix_percent: vec![0; 11],
            status: b"T:\r".to_vec(),
            made_parts: [(0x10c, vec![1]), (0x127, vec![2]), (0x10d, vec![3])],
            program_table: 0,
        };
        // Ctrl+Alt+R removes the relays; Ctrl+M opens the terrain editor.
        let (remove, terrain) = ((0x3013, 0x6ac8), (0x1032, 0x448c));
        assets.editor = Some(tables(vec![remove, terrain]));
        assets.save_dir = dir.clone();
        let mut m = mission(&assets);
        let relays = |m: &Mission| {
            let objects = m.scenario.as_ref().unwrap().objects.iter().flatten();
            objects.filter(|o| o.kind == usg::ObjectKind::Relay).count()
        };
        assert!(relays(&m) > 0);
        m.key(remove.0, &assets);
        assert!(relays(&m) > 0, "locked: nothing happens");
        assets.editor_unlocked = true;
        m.key(remove.0, &assets);
        assert_eq!(relays(&m), 0);
        // Ctrl+M opens the terrain editor (with the driver's keys), which stops the
        // sim; a Ctrl+drag makes a circle, Alt+E applies it through the sim and Alt+Q
        // leaves, writing the circles.
        m.key(terrain.0, &assets);
        assert!(m.terrain.is_none(), "no driver, no editor");
        let (apply, leave) = ((0x2012, 0x6b5), (0x2010, 0x6cf));
        assets.map_driver = Some(mapdvr::MapDriver {
            keys: vec![apply, leave],
        });
        m.key(terrain.0, &assets);
        let retrace = |m: &Mission| match &m.run {
            Run::Running(sim) => sim.retrace(),
            _ => unreachable!(),
        };
        let before = retrace(&m);
        m.tick(&[]);
        assert_eq!(retrace(&m), before, "the sim stands still");
        let at = |x: i32| xeno_sim::Position {
            x: xeno_sim::Fixed(x << 16),
            y: xeno_sim::Fixed(x << 16),
        };
        let editor = m.terrain.as_mut().unwrap();
        editor.press(at(200), 0x11);
        editor.hover(at(203));
        assert!(editor.release());
        m.key(apply.0, &assets);
        let features = &m
            .scenario
            .as_ref()
            .unwrap()
            .terrain
            .as_ref()
            .unwrap()
            .features;
        assert_eq!(
            (features.len(), features[0].x, features[0].radius),
            (2, 200 << 16, 0x480)
        );
        let Run::Running(sim) = &m.run else {
            unreachable!()
        };
        assert!(sim.world.terrain.hill_at(at(201)).is_some());
        m.key(leave.0, &assets);
        assert!(m.terrain.is_none());
        let saved = map::parse(&std::fs::read(dir.join(SAVED_TERRAIN)).unwrap()).unwrap();
        assert_eq!(saved.features.len(), 2);
        assert_eq!(saved.features[1], map::closing_block());
        m.save_scenario(&dir.join(SAVED_SCENARIO), &assets);
        // It reads back as saved (the terrain filled to a paragraph, as the game's).
        let bytes = std::fs::read(dir.join(SAVED_SCENARIO)).unwrap();
        let back = usg::parse(&bytes).unwrap();
        assert_eq!(back.objects, m.saved(&assets).unwrap().objects);
        assert_eq!(usg::write(&back).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Ctrl+Alt+P saves through the SAVE GAME dialog and Ctrl+Alt+O (with unlock word
    /// 9) reads the orders through the open dialog; the sim stands still meanwhile.
    #[test]
    fn the_file_dialogs_save_the_scenario_and_read_the_orders() {
        let dir = std::env::temp_dir().join(format!("xeno-dialogs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ord.txt"), b"HELLO").unwrap();
        let mut assets = test_assets();
        let (save, orders) = ((0x3019, handler::SAVE), (0x3018, editor_handler::ORDERS));
        let terrain = (0x1032, editor_handler::TERRAIN);
        let (load_map, save_map, clear) = ((0x2026, 0x66f), (0x201f, 0x705), (0x2031, 0x694));
        assets.views.situation.keys.push(save);
        assets.map_driver = Some(mapdvr::MapDriver {
            keys: vec![load_map, save_map, clear],
        });
        assets.editor = Some(editor::Editor {
            keys: vec![orders, terrain],
            kind_names: vec![None; editor::KINDS],
            side_names: [b"H".to_vec(), b"A".to_vec()],
            show_names: [b"N".to_vec(), b"Y".to_vec()],
            mission_names: vec![b"M".to_vec(); 8],
            mission_defaults: vec![[0; 4]; 7],
            mix_percent: vec![0; 11],
            status: b"T:\r".to_vec(),
            made_parts: [(0x10c, vec![1]), (0x127, vec![2]), (0x10d, vec![3])],
            program_table: 0,
        });
        assets.editor_unlocked = true;
        assets.save_dir = dir.clone();
        assets.file_dialogs = Some(dialog::test_tables());
        let mut m = mission(&assets);
        let type_in = |m: &mut Mission, text: &str| text.chars().for_each(|c| m.typed(c));
        let enter = 0x1c;

        m.key(save.0, &assets);
        assert!(m.dialog_open());
        assert_eq!(m.pointer_shape(&assets, (0, 0), 0, false), DIALOG_SHAPE);
        let Run::Running(sim) = &m.run else {
            unreachable!()
        };
        let before = sim.retrace();
        m.tick(&[]);
        let Run::Running(sim) = &m.run else {
            unreachable!()
        };
        assert_eq!(sim.retrace(), before, "the sim stands still");
        // The field starts as `*.*`; Enter on it only lists again.
        m.key(enter, &assets);
        assert!(m.dialog_open());
        (0..3).for_each(|_| assert!(m.key(0x0e, &assets).is_none()));
        type_in(&mut m, "t.usg");
        m.key(enter, &assets);
        assert!(!m.dialog_open());
        let back = usg::parse(&std::fs::read(dir.join("T.USG")).unwrap()).unwrap();
        assert_eq!(back.objects, m.saved(&assets).unwrap().objects);
        assert_eq!(back.save, None);

        // The open dialog starts on `*.USG`; ESC cancels it.
        m.key(orders.0, &assets);
        assert!(m.dialog_open());
        m.key(0x01, &assets);
        assert!(!m.dialog_open());
        m.key(orders.0, &assets);
        (0..5).for_each(|_| assert!(m.key(0x0e, &assets).is_none()));
        type_in(&mut m, "ord.txt");
        m.key(enter, &assets);
        assert!(m.orders.starts_with(b"HELLO"));
        assert_eq!((m.mode, m.typed), (ORDERS_MODE, 0));
        assert!(
            m.scenario
                .as_ref()
                .unwrap()
                .briefing
                .as_ref()
                .unwrap()
                .starts_with(b"HELLO")
        );

        // The terrain editor's Alt+S writes its circles where the SAVE GAME dialog
        // says; Alt+L loads a .map as the terrain and imports its circles.
        m.key(terrain.0, &assets);
        let at = |x: i32| xeno_sim::Position {
            x: xeno_sim::Fixed(x << 16),
            y: xeno_sim::Fixed(x << 16),
        };
        let editor = m.terrain.as_mut().unwrap();
        editor.press(at(200), 0x11);
        editor.hover(at(203));
        assert!(editor.release());
        m.key(save_map.0, &assets);
        assert!(m.dialog_open());
        (0..7).for_each(|_| assert!(m.key(0x0e, &assets).is_none()));
        type_in(&mut m, "hill.map");
        m.key(enter, &assets);
        let saved = map::parse(&std::fs::read(dir.join("HILL.MAP")).unwrap()).unwrap();
        assert_eq!(saved.features.len(), 2);
        m.key(clear.0, &assets);
        m.key(load_map.0, &assets);
        (0..5).for_each(|_| assert!(m.key(0x0e, &assets).is_none()));
        type_in(&mut m, "hill.map");
        m.key(enter, &assets);
        assert!(!m.dialog_open());
        assert_eq!(
            m.terrain.as_ref().unwrap().saved(),
            saved,
            "the circles imported"
        );
        let world = m.scenario.as_ref().unwrap().terrain.as_ref().unwrap();
        assert_eq!(world.features.len(), 2);
        assert_eq!(
            map::save_circles(&map::import_circles(&world.features)),
            saved
        );
        let Run::Running(sim) = &m.run else {
            unreachable!()
        };
        assert!(sim.world.terrain.hill_at(at(201)).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A left press on the map in mode M (`1000:5781`): on one of the player's robots
    /// probed in the last frame it frees the camera's hold on the centre and selects
    /// it; elsewhere (nothing probed, or not the player's) it moves the centre there.
    #[test]
    fn a_press_on_the_map_selects_or_recentres() {
        use xeno_sim::{Fixed, Position};
        let mut assets = test_assets();
        assets.view3d = Some(mainview::tests::view());
        assets.sitroom = Some(mainview::tests::room());
        let mut main_view = assets.views.humanoid.widgets.last().copied().unwrap();
        main_view.kind = MAIN_VIEW_TYPE;
        main_view.area = Some(Rect {
            top: 40,
            left: 100,
            bottom: 100,
            right: 220,
        });
        assets.views.situation.widgets.push(main_view);
        let mut m = mission(&assets);
        m.mode = MAP_MODE;
        m.map.overlays |= usg::Base::MAP_FREE;
        (m.map.centre, m.map.zoom) = (Position::default(), 0);
        let press = |m: &mut Mission, at| m.mouse_down(MouseButton::Left, Some(at), &assets);
        // Nothing under the pointer: the centre moves to the point (5, 3).
        press(&mut m, (165, 67));
        let unit = Fixed::from_int;
        assert_eq!((m.map.centre.x, m.map.centre.y), (unit(5), unit(3)));
        // One of the player's robots: selected, the centre let go.
        let robot = m.robots()[0];
        m.probed.set([Some(robot), None]);
        press(&mut m, (100, 50));
        assert_eq!(m.selected, Some(robot));
        assert_eq!(m.map.overlays & usg::Base::MAP_FREE, 0);
        assert_eq!(m.map.centre.x, unit(5), "the centre kept");
        // Not a unit (the base): the centre moves and is held again.
        let base = m
            .world()
            .iter()
            .find(|(_, e)| e.kind == usg::ObjectKind::Base)
            .map(|(id, _)| id.0);
        m.probed.set([base, None]);
        press(&mut m, (165, 67));
        assert_ne!(m.map.overlays & usg::Base::MAP_FREE, 0);
        assert_eq!(m.selected, Some(robot));
        // Off the main view, or in another mode, nothing.
        let before = m.map;
        press(&mut m, (10, 10));
        m.mode = LOG_MODE;
        press(&mut m, (130, 50));
        assert_eq!(m.map, before);
        // Drawn: the map probes the pointer over another robot's dot, and the press
        // there selects it.
        let other = *m.robots().last().unwrap();
        assert_ne!(other, robot);
        let at = m.world().get(xeno_sim::EntityId(other)).unwrap().position;
        let objects = m.scenario.as_mut().unwrap().objects.iter_mut().flatten();
        for o in objects.filter(|o| o.slot == other) {
            o.status &= !1; // Not hidden (+0x17 bit 0), so its dot is drawn.
        }
        (m.mode, m.probed) = (MAP_MODE, Cell::default());
        let camera = m.map_camera(&assets).unwrap();
        let map = Framebuffer::new(Size::new(120, 60));
        let (x, y) = mainview::pixel(&camera, &map, at.x.0, at.y.0);
        let at = ((100 + x) as u16, (40 + y) as u16);
        m.hover(at, &assets);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.fill_rect(xeno_render::Rect::new(100, 40, 120, 60), 255);
        m.draw(&mut fb, &assets);
        press(&mut m, at);
        assert_eq!(m.selected, Some(other));
    }

    /// With unlock word 9, a Ctrl+left press on the map in mode M places the picked
    /// kind at the point under the pointer through the sim, and Ctrl+Alt+arrows
    /// scroll the map; the save holds what was placed.
    #[test]
    fn the_editor_places_on_the_map_and_scrolls_it() {
        use xeno_sim::{Fixed, Position};
        let mut assets = test_assets();
        let keys = vec![
            (0x1018, 0x6ae6), // Ctrl+O: obstacles
            (0x1030, 0x6b08), // Ctrl+B: the base
            (0x1019, 0x6afb), // Ctrl+P: path points
            (0x1052, 0x6b0f), // Ctrl+Ins: a new path
            (0x304b, 0x6dde), // Ctrl+Alt+Left
            (0x1023, 0x6ad8), // Ctrl+H: humanoids
            (0x1020, 0x6ab7), // Ctrl+D
            (0x1012, 0x6be2), // Ctrl+E
        ];
        assets.editor = Some(editor::Editor {
            keys: keys.clone(),
            kind_names: vec![None; editor::KINDS],
            side_names: [b"H".to_vec(), b"A".to_vec()],
            show_names: [b"N".to_vec(), b"Y".to_vec()],
            mission_names: vec![b"M".to_vec(); 8],
            mission_defaults: vec![[0; 4]; 7],
            mix_percent: vec![0; 11],
            status: b"T:\r".to_vec(),
            made_parts: [(0x10c, vec![1]), (0x127, vec![2]), (0x10d, vec![3])],
            program_table: 0,
        });
        assets.view3d = Some(mainview::tests::view());
        let mut main_view = assets.views.humanoid.widgets.last().copied().unwrap();
        main_view.kind = MAIN_VIEW_TYPE;
        main_view.area = Some(Rect {
            top: 40,
            left: 100,
            bottom: 100,
            right: 220,
        });
        assets.views.situation.widgets.push(main_view);
        let mut m = mission(&assets);
        m.mode = MAP_MODE;
        // The map free on (0, 0) at zoom 0: a unit a pixel from the middle (160, 70).
        m.map.overlays |= usg::Base::MAP_FREE;
        (m.map.centre, m.map.zoom) = (Position::default(), 0);
        let count = |m: &Mission| m.scenario.as_ref().unwrap().objects.as_ref().unwrap().len();
        let objects = count(&m);
        assert!(
            m.place_at_pointer(Some((165, 67)), &assets),
            "the editor's press"
        );
        assert_eq!(count(&m), objects, "locked: nothing placed");
        assert!(
            !m.place_at_pointer(Some((10, 10)), &assets),
            "off the main view"
        );
        assets.editor_unlocked = true;
        let key = |m: &mut Mission, word: u16| m.key(word, &assets);
        let press = |m: &mut Mission, at| m.place_at_pointer(Some(at), &assets);
        key(&mut m, 0x1018);
        assert!(press(&mut m, (165, 67)));
        let unit = Fixed::from_int;
        let o = m
            .scenario
            .as_ref()
            .unwrap()
            .objects
            .as_ref()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(
            (o.kind, o.x, o.y),
            (usg::ObjectKind::Obstacle, 5 << 16, 3 << 16)
        );
        let made = m.world().get(EntityId(o.slot)).unwrap();
        assert_eq!(
            (made.kind, made.position.x, made.position.y),
            (o.kind, unit(5), unit(3))
        );
        // A humanoid, the player's, with its orders and in the roster.
        key(&mut m, 0x1023);
        press(&mut m, (160, 70));
        let h = m
            .scenario
            .as_ref()
            .unwrap()
            .objects
            .as_ref()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!((h.kind, h.x, h.y), (usg::ObjectKind::Humanoid, 0, 0));
        assert!(m.book().get(h.slot).is_some_and(|r| r.side == Side::Human));
        assert!(m.robots().contains(&h.slot));
        // The base moves to the point; a path point needs a path first.
        key(&mut m, 0x1030);
        press(&mut m, (150, 80));
        let slot = m.world().base(Side::Human).unwrap();
        let base = m.world().get(slot).unwrap().position;
        assert_eq!((base.x, base.y), (unit(-10), unit(-10)));
        key(&mut m, 0x1019);
        press(&mut m, (161, 69));
        let human = |m: &Mission| m.scenario.as_ref().unwrap().human_paths.clone();
        assert!(
            human(&m).is_none_or(|t| t.paths[0].point_count == 0),
            "no path yet"
        );
        key(&mut m, 0x1052);
        press(&mut m, (161, 69));
        let t = human(&m).unwrap();
        assert_eq!((t.count, t.paths[0].point_count), (1, 1));
        assert_eq!(
            t.paths[0].points[0],
            usg::Point {
                x: 1 << 16,
                y: 1 << 16
            }
        );
        assert_eq!(
            m.world().paths.human.slots[0].points.len(),
            1,
            "the sim's too"
        );
        // Ctrl+Alt+Left moves the centre's x up by 0x7fff.
        key(&mut m, 0x304b);
        assert_eq!(m.map.centre.x, Fixed(0x7fff));
        // Ctrl+D destroys the selected unit, through the sim and out of the save;
        // Ctrl+E shows undetected aliens.
        let gone = m.selected.unwrap();
        key(&mut m, 0x1020);
        assert!(m.world().get(EntityId(gone)).is_none() && m.book().get(gone).is_none());
        assert!(
            !m.scenario
                .as_ref()
                .unwrap()
                .objects
                .iter()
                .flatten()
                .any(|o| o.slot == gone)
        );
        assert_ne!(m.selected, Some(gone));
        assert!(!m.shown_map().reveal);
        key(&mut m, 0x1012);
        assert!(m.shown_map().reveal);
        // The save holds the obstacle and the moved base.
        let back = usg::parse(&usg::write(m.scenario.as_ref().unwrap()).unwrap()).unwrap();
        let objects = back.objects.unwrap();
        assert!(objects.contains(&o));
        let base = objects.iter().find(|b| b.slot == slot.0).unwrap();
        assert_eq!((base.x, base.y), (-10 << 16, -10 << 16));
    }

    /// The tests' stand-in for play (T-150): `side`'s base takes hits through the sim
    /// until it is destroyed, so the mission ends by its real end script.
    pub(crate) fn destroy_base(m: &mut Mission, side: Side) {
        let Run::Running(sim) = &mut m.run else {
            return;
        };
        let base = sim.world.base(side).unwrap();
        while let Some(State::Base {
            destroyed: false, ..
        }) = sim.world.get(base).map(|e| e.state)
        {
            sim.hit(base, u8::MAX);
        }
    }

    fn click(
        m: &mut Mission,
        assets: &Assets,
        at: (u16, u16),
        button: MouseButton,
    ) -> Option<Request> {
        m.mouse_down(button, Some(at), assets);
        m.mouse_up(button, Some(at), assets)
    }

    /// The player's base counts the robots each side lost, and a destroyed base ends
    /// the mission after its end script: won for the alien base.
    #[test]
    fn a_destroyed_base_ends_the_mission_with_the_player_s_counts() {
        let assets = test_assets();
        for (side, won) in [(Side::Alien, true), (Side::Human, false)] {
            let mut m = mission(&assets);
            let (kills, losses) = m.kills_and_losses();
            if let Run::Running(sim) = &mut m.run {
                sim.hit(xeno_sim::EntityId(2), 81);
                sim.hit(xeno_sim::EntityId(7), 81);
            }
            assert_eq!(m.kills_and_losses(), (kills + 1, losses + 1));
            assert_eq!((0..600).find_map(|_| m.tick([])), None);
            destroy_base(&mut m, side);
            let ended = (1..=33 * 16).find_map(|t| m.tick([]).map(|w| (t, w)));
            assert!(
                ended.is_some_and(|(t, w)| t > 32 * 16 && w == Request::Over { won }),
                "{ended:?}"
            );
        }
    }

    #[test]
    fn robot_sounds_are_heard_only_on_their_console() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let robot = m.book().side(Side::Human).map(|(s, _)| s).next().unwrap();
        let hit = |m: &mut Mission, points| {
            let Run::Running(sim) = &mut m.run else {
                unreachable!()
            };
            for event in sim.hit(xeno_sim::EntityId(robot), points) {
                Mission::hear(m.viewed, &mut m.sounds, event);
            }
            m.take_sounds()
        };
        // The hit sound (event 21) is the robot's console's; destroyed (2), everyone's.
        assert_eq!(hit(&mut m, 0), []);
        m.view_robot(robot).unwrap();
        assert_eq!(hit(&mut m, 0), [Sound::Play(21)]);
        // Leaving the console gives the display noise's voice silence (`1000:dac0`).
        m.base();
        assert_eq!(hit(&mut m, 81), [Sound::Stop(28), Sound::Play(2)]);
    }

    /// The pointer's shape by the picture's colour under it, the view and its mode
    /// (T-106; the synthetic tables give M and R 3, and the consoles 3, 4, 6, 4).
    #[test]
    fn the_pointer_takes_the_regions_shapes() {
        let mut assets = test_assets();
        let mut main_view = assets.views.humanoid.widgets.last().copied().unwrap();
        main_view.kind = MAIN_VIEW_TYPE;
        let area = main_view.area.as_mut().unwrap();
        (area.top, area.bottom) = (40, 100);
        assets.views.situation.widgets.push(main_view);
        // A later widget over the third display's last rows takes the moves there.
        let mut over = main_view;
        (over.kind, over.area) = (
            0x0e,
            over.area.map(|a| Rect {
                top: 150,
                bottom: 160,
                ..a
            }),
        );
        assets.views.humanoid.widgets.push(over);
        let mut m = mission(&assets);
        // The widgets of `test_assets`, and the main view: the main view or 3D view on
        // row 50, the second display on row 110, the third on row 140.
        let shape =
            |m: &mut Mission, y, colour, alt| m.pointer_shape(&assets, (100, y), colour, alt);
        m.mode = ORDERS_MODE;
        assert_eq!(shape(&mut m, 50, 255, false), 0);
        m.mode = MAP_MODE;
        assert_eq!(shape(&mut m, 50, 255, false), 3);
        assert_eq!(
            shape(&mut m, 50, 254, false),
            0,
            "only the main view's region"
        );
        assert_eq!(
            shape(&mut m, 170, 255, false),
            0,
            "only in the main view's widget"
        );
        assert_eq!(m.pointer_shape(&assets, (1, 1), 255, false), CORNER);
        let robot = m.book().side(Side::Human).map(|(s, _)| s).next().unwrap();
        m.view_robot(robot).unwrap();
        let shapes = [
            (50, 255),
            (110, 254),
            (140, 253),
            (50, 254),
            (140, 255),
            (170, 255),
        ]
        .map(|(y, c)| shape(&mut m, y, c, true));
        assert_eq!(
            shapes,
            [3, 4, 4, 0, 0, 0],
            "each widget tests its own colour"
        );
        assert_eq!(
            shape(&mut m, 155, 253, false),
            0,
            "the last widget takes the move"
        );
        m.set_display_mode(robot, MAP_DISPLAY);
        assert_eq!(shape(&mut m, 110, 254, true), 6, "the map with Alt");
        assert_eq!(shape(&mut m, 110, 254, false), 4);
    }

    /// A release over the 3D view's region with an object under the pointer in the
    /// last frame: the left locks it, the right opens its console if it is the
    /// player's. Off the region's colour, or with nothing probed, nothing.
    #[test]
    fn a_release_on_the_view_locks_or_opens_the_object_under_the_pointer() {
        let assets = test_assets();
        let mut m = mission(&assets);
        // The fixture's human humanoid (2) and scorpion (5), and an alien robot (7)
        // moved a unit ahead of the humanoid, whose targeting computer is repaired.
        let (robot, friend, enemy) = (2, 5, 7);
        m.view_robot(robot).unwrap();
        let world = m.world_mut();
        if let Some(xeno_sim::Damage::Robot(c)) =
            world.get_mut(EntityId(robot)).map(|e| &mut e.damage)
        {
            c.targeting = 0;
        }
        let mut at = world.get(EntityId(robot)).unwrap().position;
        at.x.0 += 1 << 16;
        world.get_mut(EntityId(enemy)).unwrap().position = at;
        let target = |m: &Mission| match m.world().get(EntityId(robot)).map(|e| e.state) {
            Some(State::Robot(r)) => r.target,
            _ => None,
        };
        let left = MouseButton::Left;
        m.probed.set([Some(enemy), None]);
        m.pointer_shape(&assets, (100, 50), 254, false);
        assert_eq!(click(&mut m, &assets, (100, 50), left), None);
        assert_eq!(target(&m), None, "off the region's colour");
        m.pointer_shape(&assets, (100, 50), 255, false);
        m.probed.set([None, None]);
        click(&mut m, &assets, (100, 50), left);
        assert_eq!(target(&m), None, "nothing under the pointer");
        m.probed.set([Some(enemy), None]);
        click(&mut m, &assets, (100, 50), left);
        assert_eq!(target(&m), Some(EntityId(enemy)));
        // The right button on the enemy does nothing; on a friend, its console.
        let right = MouseButton::Right;
        assert_eq!(click(&mut m, &assets, (100, 50), right), None);
        m.probed.set([Some(friend), None]);
        let opened = click(&mut m, &assets, (100, 50), right);
        assert_eq!(opened, Some(Request::Show(View::Scorpion)));
        assert_eq!(m.viewed(), Viewed::Robot(friend));
    }

    /// On the second display a left release locks as on the 3D view; in the map mode an
    /// Alt press gives the robot, after the next frame, the object under the pointer
    /// as its goal or the point there as its destination, and an Alt release before
    /// then takes it back.
    #[test]
    fn the_map_display_s_alt_press_sets_a_goal_after_the_next_frame() {
        let mut assets = test_assets();
        // The map needs no device (the synthetic map's bit is the lock warning's).
        assets.views.second_display.devices[usize::from(MAP_DISPLAY)] = 0;
        let mut m = mission(&assets);
        let (robot, enemy) = (2, 7);
        m.view_robot(robot).unwrap();
        // Its displays' devices whole (the fixture's are damaged).
        if let Some(xeno_sim::Damage::Robot(c)) = m
            .world_mut()
            .get_mut(EntityId(robot))
            .map(|e| &mut e.damage)
        {
            c.systems = 0;
        }
        let state = |m: &Mission| match m.world().get(EntityId(robot)).map(|e| e.state) {
            Some(State::Robot(r)) => (r.goal, r.goal_object, r.destination),
            other => panic!("{other:?}"),
        };
        let point = xeno_sim::Position {
            x: xeno_sim::Fixed(5 << 16),
            y: xeno_sim::Fixed(-(3 << 16)),
        };
        let (left, at) = (MouseButton::Left, (100, 110));
        m.pointer_shape(&assets, at, SECOND_DISPLAY, false);
        // Not in the map mode, the Alt press does nothing.
        m.set_alt(true);
        m.map_point.set(Some(point));
        m.mouse_down(left, Some(at), &assets);
        m.tick(&[]);
        assert_ne!(state(&m).0, 0x02);
        m.mouse_up(left, Some(at), &assets);
        m.set_display_mode(robot, MAP_DISPLAY);
        // Released with Alt before the next frame: nothing.
        click(&mut m, &assets, at, left);
        m.tick(&[]);
        assert_ne!(state(&m).0, 0x02);
        // Held over a frame: the destination, then the object under the pointer.
        m.mouse_down(left, Some(at), &assets);
        m.tick(&[]);
        assert_eq!((state(&m).0, state(&m).2), (0x02, point));
        m.mouse_up(left, Some(at), &assets);
        m.probed.set([None, Some(enemy)]);
        m.mouse_down(left, Some(at), &assets);
        m.tick(&[]);
        assert_eq!((state(&m).0, state(&m).1), (0x01, enemy));
        // Without Alt a release locks the object (here refused: the fixture's
        // targeting computer is damaged, which posts its message).
        m.mouse_up(left, Some(at), &assets);
        m.set_alt(false);
        let panel = |m: &Mission| match m.world().get(EntityId(robot)).map(|e| e.state) {
            Some(State::Robot(r)) => r.panel.queue,
            _ => unreachable!(),
        };
        let before = panel(&m);
        click(&mut m, &assets, at, left);
        assert_ne!(panel(&m), before, "the lock's message");
    }

    #[test]
    fn display_keys_set_the_mode_and_zoom_and_a_damaged_device_makes_noise() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let robot = m.book().side(Side::Human).map(|(s, _)| s).next().unwrap();
        m.view_robot(robot).unwrap();
        m.set_display_mode(robot, MAP_DISPLAY);
        assert_eq!(m.take_sounds(), [Sound::Play(DISPLAY_SOUND)]);
        // In the map mode the keys step the map's zoom, up to 4.
        m.displays.entry(robot).or_default().map_zoom = 0;
        let camera_zoom = m.display(robot).zoom;
        (0..6).for_each(|_| m.zoom_display(robot, true, &assets));
        assert_eq!(
            (m.display(robot).map_zoom, m.display(robot).zoom),
            (4, camera_zoom)
        );
        m.zoom_display(robot, false, &assets);
        assert_eq!(m.display(robot).map_zoom, 3);
        // A changed mode with its device working silences the noise's voice.
        m.check_noise(&assets);
        assert_eq!(m.take_sounds(), [Sound::Stop(NOISE_SOUND)]);
        m.check_noise(&assets);
        assert_eq!(m.take_sounds(), []);
        // The camera's device (synthetic: +0xf6 bit 3) damaged: the noise plays.
        m.set_display_mode(robot, 0);
        if let Run::Running(sim) = &mut m.run
            && let Some(xeno_sim::Damage::Robot(c)) =
                sim.world.get_mut(EntityId(robot)).map(|e| &mut e.damage)
        {
            c.systems |= 0x08;
        }
        m.check_noise(&assets);
        assert_eq!(
            m.take_sounds(),
            [Sound::Play(DISPLAY_SOUND), Sound::Play(NOISE_SOUND)]
        );
        assert_eq!(m.display_mode(robot, &assets), DAMAGED);
    }

    /// T-134: a tracked target's distance and bearing pick the zoom and pan every
    /// tick until a pan key; the zoom's name counts down every 16 ticks; the scorpion's
    /// missile camera comes on at a launch and breaks up for five steps once the
    /// missile is gone, then gives the mode back.
    #[test]
    fn the_display_follows_its_target_and_the_scorpion_s_missile() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let robot = m.book().side(Side::Human).map(|(s, _)| s).next().unwrap();
        let other = m
            .world()
            .iter()
            .map(|(id, _)| id)
            .find(|id| id.0 != robot)
            .unwrap();
        m.view_robot(robot).unwrap();
        let edit = |m: &mut Mission, f: &dyn Fn(&mut xeno_sim::Entity)| {
            f(m.world_mut().get_mut(EntityId(robot)).unwrap());
        };
        fn movement(e: &mut xeno_sim::Entity) -> &mut xeno_sim::Movement {
            match &mut e.state {
                State::Robot(r) => r,
                _ => unreachable!(),
            }
        }
        let step = |m: &mut Mission, tick: u8| {
            edit(m, &|e| movement(e).tick = tick);
            m.ticked = true;
            m.step_displays(&assets);
        };
        edit(&mut m, &|e| {
            let heading = e.heading;
            let r = movement(e);
            (r.target, r.target_distance) = (Some(other), 15);
            r.target_bearing = heading.wrapping_add(0x1234);
            r.status |= usg::status::TRACKING;
        });
        step(&mut m, 2);
        assert_eq!((m.display(robot).zoom, m.display(robot).pan), (1, 0x1234));
        // X: the pan plus the zoom's step, and no more tracking.
        m.pan_display(robot, Some(true), &assets);
        step(&mut m, 2);
        assert_eq!(m.display(robot).pan, 0x1234 + 80);
        m.pan_display(robot, None, &assets);
        assert_eq!(m.display(robot).pan, 0);
        // The zoom's name: six steps of 16 ticks.
        m.set_display_mode(robot, 0);
        step(&mut m, 0);
        step(&mut m, 2);
        assert_eq!(m.display(robot).label, LABEL_TIME - 1);
        // The scorpion's missile.
        m.take_sounds();
        edit(&mut m, &|e| {
            e.kind = usg::ObjectKind::Scorpion;
            movement(e).guns.missile = Some(other);
        });
        step(&mut m, 2);
        let d = m.display(robot);
        assert_eq!((d.mode, d.before), (MISSILE_DISPLAY, 0));
        assert_eq!(m.take_sounds(), [Sound::Play(DISPLAY_SOUND)]);
        edit(&mut m, &|e| movement(e).guns.missile = None);
        for _ in 0..STATIC_TIME - 1 {
            step(&mut m, 0);
            assert_eq!(m.display(robot).mode, LOST_MISSILE_CAMERA);
        }
        step(&mut m, 0);
        assert_eq!(m.display(robot).mode, 0);
        assert_eq!(m.take_sounds(), [Sound::Play(DISPLAY_SOUND)]);
    }

    /// List 0x18's noise visits every pixel once, in colours 0x20 to 0x2f; the zoom's
    /// name goes by kind and mode.
    #[test]
    fn a_damaged_display_is_noise_and_the_zoom_s_name_goes_by_kind() {
        for n in [37, 0x500, 100 * 60] {
            let mut pixels = vec![0; n];
            noise(&mut pixels, 0x1357, DISPLAY_CLEAR);
            assert!(pixels.iter().all(|&p| (0x20..=0x2f).contains(&p)), "{n}");
            assert_eq!(pixels[0], DISPLAY_CLEAR);
        }
        let mut pixels = [7, 0, 255];
        remap(&mut pixels, &(0..=255u8).rev().collect::<Vec<_>>());
        assert_eq!(pixels, [248, 255, 0]);
        let t = &test_assets().views.second_display;
        let d = Display {
            zoom: 1,
            map_zoom: 2,
            label: 1,
            ..Display::default()
        };
        let name = |kind, mode, d| zoom_name(kind, mode, d, t);
        assert_eq!(
            name(RobotKind::Humanoid, 0, d),
            Some(((0x40, 0x20), &b"bx"[..]))
        );
        assert_eq!(
            name(RobotKind::Scorpion, 2, d),
            Some(((0x45, 0x25), &b"hx"[..]))
        );
        assert_eq!(name(RobotKind::Humanoid, 4, d), None);
        assert_eq!(
            name(RobotKind::Scout, 6, d),
            Some(((0x49, 0x29), &b"bx"[..]))
        );
        assert_eq!(name(RobotKind::Scout, 1, Display { label: 0, ..d }), None);
    }

    #[test]
    fn the_cockpit_keys_drive_the_viewed_robot_through_the_sim() {
        let mut assets = test_assets();
        let mut m = mission(&assets);
        let robot = m.book().side(Side::Human).map(|(s, _)| s).next().unwrap();
        let view = m.view_robot(robot).unwrap();
        let keys = match view {
            View::Humanoid => &mut assets.views.humanoid.keys,
            View::Scorpion => &mut assets.views.scorpion.keys,
            _ => &mut assets.views.scout.keys,
        };
        // The humanoid's up arrow and `.`, wherever the table has them.
        keys.extend([(0x48, 0xacda), (0x34, 0xd0a0)]);
        let speed = |m: &Mission| match m.world().get(EntityId(robot)).map(|e| e.state) {
            Some(State::Robot(r)) => (r.target_speed, r.gear),
            other => panic!("{other:?}"),
        };
        m.key(0x34, &assets);
        assert_eq!(speed(&m), (0, 2));
        assert!(m.book().get(robot).unwrap().orders.program_drives);
        m.key(0x48, &assets);
        assert_eq!(speed(&m), (20, 4));
        assert!(
            !m.book().get(robot).unwrap().orders.program_drives,
            "the pilot drives"
        );
        // F9 flips the weapon: its sound is heard on this console.
        let _ = m.take_sounds();
        let keys = match view {
            View::Humanoid => &mut assets.views.humanoid.keys,
            View::Scorpion => &mut assets.views.scorpion.keys,
            _ => &mut assets.views.scout.keys,
        };
        keys.push((0x43, 0xb20c));
        m.key(0x43, &assets);
        let sound = Sound::Play(assets.motion.weapon_keys.sound);
        assert_eq!(m.take_sounds(), [sound]);
        // The situation room's F3 recalls the selected unit (the synthetic robot is
        // saved recalled: not any more); its tick hands it to the recall's program.
        if let Run::Running(sim) = &mut m.run
            && let Some(State::Robot(r)) = sim.world.get_mut(EntityId(robot)).map(|e| &mut e.state)
        {
            r.mode &= !xeno_sim::Movement::RECALL;
        }
        m.base();
        m.select_unit(Some(robot));
        assets.views.situation.keys.push((0x3d, 0x675e));
        m.key(0x3d, &assets);
        m.tick(&[]);
        let orders = m.book().get(robot).unwrap().orders;
        assert_eq!(
            (orders.program, orders.program_drives),
            (assets.motion.recall_program, true)
        );
    }

    /// Keys 1 to 6 step a power row once; an arrow holds its row's slider until let go,
    /// even released off it.
    #[test]
    fn keys_and_arrows_move_the_power_rows() {
        let mut assets = test_assets();
        let mut m = mission(&assets);
        let depot = |m: &Mission| {
            let world = m.world();
            match world.base(Side::Human).and_then(|b| world.get(b)) {
                Some(e) => match e.state {
                    State::Base { depot, .. } => depot,
                    _ => panic!("{:?}", e.state),
                },
                None => panic!("no base"),
            }
        };
        let before = depot(&m).power;
        // Key 2 (`1000:663a`) raises the net's power a step.
        assets.views.situation.keys.insert(0, (0x03, 0x663a));
        m.key(0x03, &assets);
        assert!(depot(&m).power[1] > before[1], "{before:?}");
        // The left arrow of the row whose byte is 2 (repairs) holds slider 3.
        let area = Rect {
            top: 159,
            left: 103,
            bottom: 166,
            right: 111,
        };
        let kind = panels::LOWER_TYPE;
        let state = 0x12;
        assets.views.situation.widgets.push(views::Widget {
            kind,
            state,
            area: Some(area),
        });
        let handle = 0x3f8;
        let arrow = views::SpriteButton { kind, area, handle };
        assets.views.situation_buttons.insert(0, arrow);
        if let Some(usg::Body::Base(b)) = m
            .scenario
            .as_mut()
            .and_then(|s| s.objects.iter_mut().flatten().find(|o| o.slot == 0))
            .map(|o| &mut o.body)
        {
            b.view_state.0[usize::from(state) + 1] = 2;
        }
        m.mouse_down(LEFT, Some((105, 160)), &assets);
        assert_eq!((depot(&m).slider, m.button_look), (3, true));
        m.mouse_up(LEFT, Some((0, 199)), &assets);
        assert_eq!((depot(&m).slider, m.button_look), (0, false));
    }

    /// The robot on view taken in turns the view to the base; it is then skipped by
    /// the consoles' Tab and cannot be viewed, but the roster steps to it, and F2
    /// launches it at the base's position.
    #[test]
    fn a_robot_taken_in_leaves_its_console_and_f2_launches_it() {
        use xeno_formats::usg::Robot as Bits;
        let mut assets = test_assets();
        let mut m = mission(&assets);
        // The fixture's human humanoid (slot 2) and scorpion (slot 5); the base, slot 0.
        let (robot, other) = (2, 5);
        assert_eq!(m.view_robot(robot), Some(View::Humanoid));
        let Run::Running(sim) = &mut m.run else {
            unreachable!()
        };
        let base = sim.world.get(EntityId(0)).unwrap().position;
        if let Some(State::Base { tick, .. }) = sim.world.get_mut(EntityId(0)).map(|e| &mut e.state)
        {
            // The take-in's phase (9) comes next.
            *tick = 8;
        }
        let e = sim.world.get_mut(EntityId(robot)).unwrap();
        e.position = base;
        if let xeno_sim::Damage::Robot(c) = &mut e.damage {
            c.equipment &= !(Bits::ENTERING_BASE | Bits::IN_BASE);
        }
        assert_eq!(m.tick(&[]), Some(Request::Show(View::SitRoom)));
        assert_eq!(m.viewed(), Viewed::Base);
        assert!(m.in_base(robot));
        // Not viewed from the roster; skipped by a console's Tab, not by the roster's.
        m.select_unit(Some(robot));
        assert_eq!(m.view_selected(), None);
        assert_eq!(m.step_robot(Some(other), 1, true), Some(other));
        assert_eq!(m.step_robot(Some(other), 1, false), Some(robot));
        // F2 launches it (the synthetic tables give F2 the consoles' handler).
        assets.views.situation.keys.insert(0, (0x3c, 0x6704));
        m.key(0x3c, &assets);
        let e = m.world().get(EntityId(robot)).unwrap();
        assert_eq!(e.position, base);
        assert!(!m.in_base(robot));
        assert_eq!(m.view_selected(), Some(Request::Show(View::Humanoid)));
    }

    #[test]
    fn the_alert_damages_the_player_s_base_while_running() {
        let assets = test_assets();
        let mut campaign = crate::maproom::tests::test_campaign();
        let (file, scenario) = assets.sample(None).unwrap();
        let mut m = Mission::new(file, scenario, campaign.take_random(), &assets.motion);
        // The fixture's human base (slot 0) has taken 20.
        m.apply_alert(30);
        let damage = |m: &Mission| m.world().get(xeno_sim::EntityId(0)).unwrap().damage;
        assert_eq!(damage(&m), xeno_sim::Damage::Taken(50));
        m.park(&mut campaign);
        m.apply_alert(30);
        assert_eq!(damage(&m), xeno_sim::Damage::Taken(50));
    }

    /// A midnight in play moves the campaign's calendar on (the synthetic month 1 has
    /// two days).
    #[test]
    fn a_midnight_in_play_moves_the_campaign_s_date() {
        let assets = test_assets();
        let mut campaign = crate::maproom::tests::test_campaign();
        let (file, scenario) = assets.sample(None).unwrap();
        let mut m = Mission::new(file, scenario, campaign.take_random(), &assets.motion);
        m.set_date(1, 1);
        let Run::Running(sim) = &mut m.run else {
            unreachable!()
        };
        let c = &mut sim.world.clock;
        (c.hour, c.minute, c.second, c.tick) = (23, 59, 59, 56);
        sim.tick(&[]);
        m.park(&mut campaign);
        assert_eq!((campaign.day, campaign.month), (0, 2));
        // Parked again, it keeps the date it gave.
        campaign.day = 7;
        m.park(&mut campaign);
        assert_eq!(campaign.day, 7);
    }

    #[test]
    fn padded_names_become_file_names() {
        assert_eq!(file_name(b"ATRAIN.USG"), "atrain.usg");
        assert_eq!(file_name(b"ABC     .USG"), "abc.usg");
    }

    /// F1 pauses the mission under its box until the next key or press, Ctrl+Alt+S
    /// flips the shots on the maps, and Ctrl+P charges the viewed robot only with
    /// unlock word 8.
    #[test]
    fn the_developer_and_pause_keys() {
        let mut assets = test_assets();
        assets.sitroom = Some(crate::mainview::tests::room());
        let (f1, shots, charge) = (0x3b, 0x301f, 0x1019);
        for keys in [
            &mut assets.views.situation.keys,
            &mut assets.views.scorpion.keys,
        ] {
            keys.extend([
                (f1, handler::PAUSE),
                (shots, handler::SHOTS),
                (charge, handler::CHARGE),
            ]);
        }
        let mut m = mission(&assets);
        let ticks = |m: &Mission| m.world().ticks;
        assert_eq!(m.key(f1, &assets), None);
        assert!(m.paused());
        let before = ticks(&m);
        assert_eq!(m.tick([]), None);
        assert_eq!(ticks(&m), before, "the world stands still");
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.draw(&mut fb, &assets);
        // The test box (2, 4, 8, 30): framed in 0x0f, filled in 0.
        assert_eq!((fb.get(4, 2), fb.get(10, 7)), (Some(0x0f), Some(0x0f)));
        assert_eq!(fb.get(29, 3), Some(0x0f));
        // The next key only ends the pause; so does a press.
        assert_eq!(m.key(shots, &assets), None);
        assert!(!m.paused() && !m.shown_map().shots);
        m.tick([]);
        assert_ne!(ticks(&m), before);
        m.key(f1, &assets);
        m.mouse_down(LEFT, Some((5, 16)), &assets);
        assert!(!m.paused());
        m.key(shots, &assets);
        assert!(m.shown_map().shots);
        // Ctrl+P on the scorpion's console: nothing without the word, then a charge.
        assert_eq!(m.view_robot(5), Some(View::Scorpion));
        let energy = |m: &Mission| match m.world().get(EntityId(5)).map(|e| e.state) {
            Some(State::Robot(r)) => r.guns.energy,
            other => panic!("{other:?}"),
        };
        if let Some(State::Robot(r)) = m.world_mut().get_mut(EntityId(5)).map(|e| &mut e.state) {
            r.guns.energy = 1;
        }
        m.key(charge, &assets);
        assert_eq!(energy(&m), 1, "locked");
        assets.developer_unlocked = true;
        m.key(charge, &assets);
        assert_eq!(energy(&m), 0xff);
    }

    #[test]
    fn orders_are_set_on_the_console_and_kept_across_views() {
        let mut assets = test_assets();
        // The original's F3 (`DS:` key tables): the program takes over.
        assets.views.humanoid.keys.push((0x3d, handler::TO_PROGRAM));
        let mut m = mission(&assets);
        assert_eq!(m.view(), View::SitRoom);
        assert_eq!(m.viewed_label(), "the base");
        // Space? The synthetic situation room binds only F2 and ESC: neither does
        // anything there. The roster's first row selects unit 2; a right release opens
        // its console.
        assert_eq!(m.key(0x3c, &assets), None);
        assert_eq!(click(&mut m, &assets, (5, 16), LEFT), None);
        assert_eq!(
            click(&mut m, &assets, (5, 16), RIGHT),
            Some(Request::Show(View::Humanoid))
        );
        assert_eq!(m.viewed(), Viewed::Robot(2));
        assert_eq!(m.viewed_label(), "unit 2 (Humanoid)");
        // A left release on the program panel: the next program, the pilot driving.
        assert_eq!(click(&mut m, &assets, (130, 92), LEFT), None);
        assert_eq!(m.book().get(2).unwrap().orders, Orders::given(2));
        // A right release: back to 1.
        click(&mut m, &assets, (130, 92), RIGHT);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        // F2 binds the pilot handler; the pilot already drives. F3 hands the robot to
        // its program (+0x23 bit 7 set), F2 back to the pilot.
        assert_eq!(m.key(0x3c, &assets), None);
        assert!(!m.book().get(2).unwrap().orders.program_drives);
        m.key(0x3d, &assets);
        assert!(m.book().get(2).unwrap().orders.program_drives);
        m.key(0x3c, &assets);
        assert!(!m.book().get(2).unwrap().orders.program_drives);
        // The unit-name button: the next unit, the scorpion in slot 5.
        assert_eq!(
            click(&mut m, &assets, (300, 190), LEFT),
            Some(Request::Show(View::Scorpion))
        );
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(5).unwrap().orders.program, 3);
        // ESC: back to the situation room; the orders are kept.
        assert_eq!(m.key(0x01, &assets), Some(Request::Show(View::SitRoom)));
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        assert_eq!(m.book().get(5).unwrap().orders.program, 3);
        assert_eq!(
            m.book().get(7).unwrap().orders.program,
            2,
            "the enemy's unchanged"
        );
        // A release off the panel changes nothing, and leaves it looking pressed.
        m.view_robot(2);
        m.mouse_down(LEFT, Some((130, 92)), &assets);
        m.mouse_up(LEFT, Some((10, 150)), &assets);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        assert!(m.panel_pressed);
        m.view_robot(2);
        // The corner goes back to the situation room, and there ends the mission.
        assert_eq!(
            click(&mut m, &assets, (1, 1), LEFT),
            Some(Request::Show(View::SitRoom))
        );
        assert_eq!(click(&mut m, &assets, (1, 1), LEFT), Some(Request::End));
        // The enemy's robots are never viewed.
        assert_eq!(m.view_robot(7), None);
    }

    #[test]
    fn a_mission_holds_the_run_s_random_word_only_while_it_runs() {
        let assets = test_assets();
        let mut campaign = crate::maproom::tests::test_campaign();
        let (file, scenario) = assets.sample(None).unwrap();
        let mut m = Mission::new(file, scenario, campaign.take_random(), &assets.motion);
        assert!(m.is_running() && !campaign.has_random());
        // The sim's world is the scenario's.
        assert_eq!(m.world().len(), 6);
        assert_eq!(m.world().census(Side::Human).units(), 2);
        // Parked, the word is back as it was; the orders stay but cannot change.
        m.park(&mut campaign);
        assert!(!m.is_running());
        assert_eq!(campaign.random().clone(), Random::seeded(1));
        m.view_robot(2);
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(2).unwrap().orders.program, 1);
        // Resumed, it takes the word again, and the orders change through its sim.
        m.resume(&mut campaign);
        assert!(m.is_running() && !campaign.has_random());
        click(&mut m, &assets, (130, 92), LEFT);
        assert_eq!(m.book().get(2).unwrap().orders, Orders::given(2));
        // Parking twice gives back one word and keeps the world.
        m.park(&mut campaign);
        m.park(&mut campaign);
        assert!(campaign.has_random());
        assert_eq!(m.world().len(), 6);
    }

    #[test]
    fn the_situation_room_buttons_flip_together_and_act_on_release() {
        let assets = test_assets();
        let mut m = mission(&assets);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.mouse_down(LEFT, Some((15, 75)), &assets);
        m.draw(&mut fb, &assets);
        // Handle 0x2a8 is sprite 1 (pixels 2).
        assert_eq!(fb.get(13, 73), Some(2));
        m.mouse_up(LEFT, Some((15, 75)), &assets);
        fb.clear(0);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(13, 73), Some(1));
    }

    /// The zoom, the overlay toggles and their keys act in mode M only; a mode
    /// button's left release picks its mode, from the base's view state.
    #[test]
    fn the_map_controls_zoom_flip_overlays_and_pick_modes() {
        let mut assets = test_assets();
        let (u, i) = (handler::OVERLAYS[0], handler::OVERLAYS[1]);
        assets.views.situation.keys.extend([
            (0x0d, handler::ZOOM_IN),
            (0x0c, handler::ZOOM_OUT),
            (0x16, u.0),
            (0x17, i.0),
        ]);
        let mut m = mission(&assets);
        let state = &mut m.scenario.as_mut().unwrap().objects.as_mut().unwrap()[0].body;
        let usg::Body::Base(base) = state else {
            unreachable!()
        };
        (base.view_state.0[10], base.view_state.0[13]) = (8, 6);
        m.set_mode(ORDERS_MODE);
        m.map.zoom = 2;
        let (zoom, overlays) = (m.map.zoom, m.map.overlays);
        click(&mut m, &assets, (292, 95), LEFT);
        click(&mut m, &assets, (292, 103), LEFT);
        m.key(0x0d, &assets);
        m.key(0x16, &assets);
        assert_eq!((m.map.zoom, m.map.overlays), (zoom, overlays));
        m.set_mode(MAP_MODE);
        click(&mut m, &assets, (292, 95), LEFT);
        assert_eq!(m.map.zoom, zoom + 1);
        click(&mut m, &assets, (292, 95), RIGHT);
        assert_eq!(m.map.zoom, zoom);
        for _ in 0..10 {
            m.key(0x0d, &assets);
        }
        assert_eq!(m.map.zoom, mainview::MAX_ZOOM);
        m.key(0x0c, &assets);
        assert_eq!(m.map.zoom, mainview::MAX_ZOOM - 1);
        m.map.zoom = 0;
        m.key(0x0c, &assets);
        assert_eq!(m.map.zoom, 0);
        m.map.zoom = zoom;
        click(&mut m, &assets, (292, 103), RIGHT);
        assert_eq!(m.map.overlays, overlays ^ u.1);
        m.key(0x16, &assets);
        m.key(0x17, &assets);
        assert_eq!(m.map.overlays, overlays ^ i.1);
        // The mode buttons: the press shows the shared pressed look, the release
        // takes it back; a right release picks nothing.
        m.mouse_down(LEFT, Some((260, 145)), &assets);
        assert!(m.button_look);
        m.mouse_up(LEFT, Some((260, 145)), &assets);
        assert!(!m.button_look);
        assert_eq!(m.mode, 8);
        click(&mut m, &assets, (260, 153), RIGHT);
        assert_eq!(m.mode, 8);
        click(&mut m, &assets, (260, 153), LEFT);
        assert_eq!(m.mode, 6);
    }

    #[test]
    fn the_console_draws_the_program_and_its_pressed_look() {
        let mut assets = test_assets();
        // The synthetic font draws only `A`.
        assets.programs.programs[1].name = b"AA".to_vec();
        let mut m = mission(&assets);
        m.view_robot(2);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.draw(&mut fb, &assets);
        // Filled with the humanoid's fill colour (0x10), the name in its text colour (the program drives).
        assert_eq!(fb.get(193, 97), Some(0x10));
        let inked: Vec<_> = (126..194).filter_map(|x| fb.get(x, 92)).collect();
        assert!(inked.contains(&0x20), "{inked:?}");
        // Pressed: the fill (0x10) is recoloured to 0x30.
        m.mouse_down(LEFT, Some((130, 92)), &assets);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(193, 97), Some(0x30));
        // The unit-name button's frame is XORed while pressed.
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.mouse_up(LEFT, Some((0, 199)), &assets);
        m.mouse_down(RIGHT, Some((300, 190)), &assets);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(273, 185), Some(8));
    }

    #[test]
    fn the_console_shows_the_robot_s_messages_and_call_sign() {
        let mut assets = test_assets();
        // Every call sign is the string `A` at offset 1.
        assets.sitroom = Some(crate::mainview::tests::room());
        let mut m = mission(&assets);
        m.view_robot(2);
        if let Run::Running(sim) = &mut m.run
            && let Some(State::Robot(r)) = sim.world.get_mut(EntityId(2)).map(|e| &mut e.state)
        {
            r.panel.text[..3].copy_from_slice(b"A\x0cA");
        }
        let mut fb = Framebuffer::new(Size::MODE_13H);
        m.draw(&mut fb, &assets);
        // The panel filled with the humanoid's fill (0x20), `A` in its text colour
        // (0x30) at the corner and again a line lower.
        assert_eq!(fb.get(101, 35), Some(0x20));
        let inked = |rows: std::ops::Range<usize>| {
            rows.flat_map(|y| (7..12).map(move |x| (x, y)))
                .any(|(x, y)| fb.get(x, y) == Some(0x30))
        };
        assert!(inked(7..9) && inked(10..12));
        // The call sign in colour 0 from the button's place.
        let (x, y) = assets.views.unit_label;
        let signed = (y..y + 6).any(|y| (x..x + 6).any(|x| fb.get(x.into(), y.into()) == Some(0)));
        assert!(signed);
    }

    #[test]
    fn the_orders_type_one_byte_a_retrace_with_no_wrapping() {
        let mut assets = test_assets();
        let area = Rect {
            top: 26,
            left: 91,
            bottom: 130,
            right: 292,
        };
        let main_view = views::Widget {
            kind: MAIN_VIEW_TYPE,
            state: 0,
            area: Some(area),
        };
        assets.views.situation.widgets.push(main_view);
        // A later widget over the third display's last rows takes the moves there.
        let mut over = main_view;
        (over.kind, over.area) = (
            0x0e,
            over.area.map(|a| Rect {
                top: 150,
                bottom: 160,
                ..a
            }),
        );
        assets.views.humanoid.widgets.push(over);
        // O and M, bound as the original binds them.
        assets
            .views
            .situation
            .keys
            .extend([(0x18, 0x67d0), (0x32, 0x67d6)]);
        let mut m = mission(&assets);
        // A synthetic briefing: `A`, CR LF, `A`, a line too long for the view, then the
        // end-of-file byte and more text that is never typed.
        let mut orders = b"A\r\nA\r\n".to_vec();
        orders.extend([b'A'; 60]);
        orders.extend(b"\x1a\x1aA");
        m.orders = orders;
        m.mode = ORDERS_MODE;
        for _ in 0..100 {
            m.retrace();
        }
        assert_eq!(m.typed, 66, "stops at 0x1a");
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.fill_rect(xeno_render::Rect::new(91, 26, 201, 104), 255);
        m.draw(&mut fb, &assets);
        // The synthetic font: `A` is a bar `XXX.` over `X.X.`, cells 4 wide, lines 3
        // high. CR goes back to x 96, LF down a line; the long line runs on past the
        // view's right edge (no wrapping).
        assert_eq!(fb.get(96, 30), Some(0x1d));
        assert_eq!(fb.get(96, 33), Some(0x1d));
        assert_eq!(fb.get(96 + 4 * 49, 36), Some(0x1d), "past the right edge");
        assert_eq!(fb.get(96, 39), Some(0x10), "cleared, no fourth line");
        // M: another mode, its placeholder, nothing typed; O starts the orders again.
        assert_eq!(m.key(0x32, &assets), None);
        m.retrace();
        assert_eq!((m.mode, m.typed), (4, 0));
        m.key(0x18, &assets);
        m.retrace();
        assert_eq!((m.mode, m.typed), (ORDERS_MODE, 1));
        // Viewing a console stops it; coming back starts it again.
        m.view_robot(2);
        m.retrace();
        assert_eq!(m.typed, 1);
        m.key(0x01, &assets);
        assert_eq!(m.typed, 0);
    }

    /// Draws the SAVE GAME dialog from the user's files over the situation room: the
    /// box in 12, the title bar in 6 with the title in colour 0 on it.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_save_dialog_draws() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let mut assets = load(&files, 13).unwrap();
        assets.save_dir = std::env::temp_dir();
        let tables = assets.file_dialogs.clone().unwrap();
        let mut m = Mission::empty();
        m.open_dialog(Purpose::SaveScenario, &tables, &assets.save_dir);
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(0xff);
        m.draw(&mut fb, &assets);
        assert_eq!(fb.get(77, 27), Some(12));
        let bar: Vec<u8> = (147..200).filter_map(|x| fb.get(x, 32)).collect();
        assert!(bar.contains(&6) && bar.contains(&0), "the title on its bar");
        assert_eq!(fb.get(10, 10), Some(0xff));
    }

    /// The user's key tables bind F1 and Ctrl+Alt+S in every view and Ctrl+P on the
    /// scorpion's and scout's consoles (not the humanoid's) to the handlers the port runs; F1's box is drawn in its two colours
    /// with its text inside.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_pause_and_developer_keys_are_bound() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let v = &assets.views;
        let bound = |view: &xeno_formats::views::View, word, h| view.keys.contains(&(word, h));
        for view in [&v.situation, &v.humanoid, &v.scorpion, &v.scout] {
            assert!(bound(view, 0x3b, handler::PAUSE));
            assert!(bound(view, 0x301f, handler::SHOTS));
        }
        for view in [&v.scorpion, &v.scout] {
            assert!(bound(view, 0x1019, handler::CHARGE));
        }
        assert!(!v.humanoid.keys.iter().any(|&(_, h)| h == handler::CHARGE));
        let mut m = Mission::empty();
        m.paused = true;
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.clear(0x80);
        m.draw(&mut fb, &assets);
        let area = assets.sitroom.as_ref().unwrap().pause_box;
        let inside: Vec<u8> = pixels(area).filter_map(|(x, y)| fb.get(x, y)).collect();
        assert!(inside.iter().all(|&c| c == PAUSE_FILL || c == PAUSE_COLOUR));
        let lit = inside.iter().filter(|&&c| c == PAUSE_COLOUR).count();
        assert!(lit > frame_pixels(area).count(), "the text is drawn inside");
        assert_eq!(fb.get(0, 0), Some(0x80));
    }

    /// Mode S draws each robot kind's picture, captions, name and figures from the
    /// user's files inside the main view.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_mode_s_draws_every_kind_inside_the_main_view() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let tables = &assets.views.situation;
        let area = tables.find(MAIN_VIEW_TYPE).and_then(|w| w.area).unwrap();
        let (room, font) = (
            assets.sitroom.as_ref().unwrap(),
            &assets.fonts[&status::FONT],
        );
        for kind in 0..xeno_formats::sitroom::KINDS {
            let mut fb = Framebuffer::new(Size::MODE_13H);
            let unit = status::Unit {
                kind,
                serial: 123,
                // Some parts damaged, the helm whole, the power not.
                parts: xeno_sim::Components {
                    helm: 100,
                    power: 37,
                    weapons: 1,
                    targeting: 0,
                    systems: 5,
                    equipment: 0x80,
                    total: 0,
                },
            };
            status::draw(&mut fb, room, font, Some(unit), |h| assets.sprite(h));
            let screen = Rect {
                top: 0,
                left: 0,
                bottom: 200,
                right: 320,
            };
            let drawn: Vec<_> = pixels(screen)
                .filter(|&(x, y)| fb.get(x, y) != Some(0))
                .collect();
            assert!(drawn.len() > 500, "kind {kind}: {} pixels", drawn.len());
            for (x, y) in drawn {
                assert!(area.contains(x as u16, y as u16), "kind {kind}: ({x}, {y})");
            }
        }
    }

    /// Types every stock scenario's briefing from the user's files to its end in the
    /// orders mode, and checks it stays inside the main view.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_briefing_types_inside_the_main_view() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let tables = &assets.views.situation;
        let area = tables.find(MAIN_VIEW_TYPE).and_then(|w| w.area).unwrap();
        assert_eq!(assets.scenarios.len(), 17);
        assert_eq!(assets.map_driver.as_ref().map(|d| d.keys.len()), Some(22));
        for (name, scenario) in &assets.scenarios {
            let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
            m.mode = ORDERS_MODE;
            while m.typed < m.orders_end() {
                m.retrace();
            }
            assert!(m.typed > 0, "{name}");
            // What the typing adds to the screen, against the same view untyped (the
            // roster and controls have those colours too).
            let draw = |m: &Mission| {
                let mut fb = Framebuffer::new(Size::MODE_13H);
                let (w, h) = (area.right - area.left, area.bottom - area.top);
                let rect =
                    xeno_render::Rect::new(area.left.into(), area.top.into(), w.into(), h.into());
                fb.fill_rect(rect, 255);
                m.draw(&mut fb, &assets);
                fb
            };
            let fb = draw(&m);
            let untyped = draw(&Mission {
                typed: 0,
                ..m.clone()
            });
            let mut inked = 0;
            for (x, y) in pixels(Rect {
                top: 0,
                left: 0,
                bottom: 200,
                right: 320,
            }) {
                if fb.get(x, y) != untyped.get(x, y)
                    && fb.get(x, y).is_some_and(|p| (0x1d..=0x1f).contains(&p))
                {
                    assert!(area.contains(x as u16, y as u16), "{name}: ({x}, {y})");
                    inked += 1;
                }
            }
            assert!(inked > 100, "{name}: {inked} pixels inked");
        }
    }

    /// With the user's focal factor, the grid's dots at zooms 0 to 5 are each within a
    /// pixel of a lattice point's (x, y even, x = y mod 4), and there are some.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_grid_dots_the_lattice() {
        use crate::game_dir::{ENV_VAR, GameDir};
        use xeno_formats::usg::Base;
        use xeno_sim::{Fixed, Position};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let view = assets.view3d.as_ref().unwrap();
        for zoom in 0..=5 {
            let map = MapState {
                overlays: Base::MAP_FREE,
                zoom,
                centre: Position {
                    x: Fixed(0x82_4000),
                    y: Fixed(0x7f_c000),
                },
                ..MapState::default()
            };
            let camera = mainview::camera(Position::default(), None, &map, view, &assets.motion);
            let mut fb = Framebuffer::new(Size::new(256, 128));
            fb.clear(mainview::MAP_BACKGROUND);
            mainview::draw_grid(&mut fb, &camera);
            let mut points = Vec::new();
            for x in (0..0x100).step_by(2) {
                for y in (0..0x100).step_by(2).filter(|y| (x - y) % 4 == 0) {
                    points.push(mainview::pixel(&camera, &fb, x << 16, y << 16));
                }
            }
            let dots: Vec<_> = (0..256)
                .flat_map(|c| (0..128).map(move |r| (c, r)))
                .filter(|&(c, r)| fb.get(c, r) == Some(0))
                .collect();
            assert!(!dots.is_empty(), "zoom {zoom}: {} dots", dots.len());
            for (c, r) in dots {
                let near = |&(pc, pr): &(i32, i32)| {
                    (pc - c as i32).abs() <= 1 && (pr - r as i32).abs() <= 1
                };
                assert!(points.iter().any(near), "zoom {zoom}: ({c}, {r})");
            }
        }
    }

    /// Modes M, R and C fill the whole main view in every stock scenario: the map's
    /// background, the robot's view, the log's clear colour.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_every_main_view_mode_fills_the_view() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        assert!(assets.sitroom.is_some() && assets.map_sprites.len() == 64);
        let area = assets
            .views
            .situation
            .find(MAIN_VIEW_TYPE)
            .and_then(|w| w.area)
            .unwrap();
        for (name, scenario) in &assets.scenarios {
            for mode in [MAP_MODE, ROBOT_VIEW_MODE, LOG_MODE] {
                let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
                m.mode = mode;
                let mut fb = Framebuffer::new(Size::MODE_13H);
                let (w, h) = (area.right - area.left, area.bottom - area.top);
                let rect =
                    xeno_render::Rect::new(area.left.into(), area.top.into(), w.into(), h.into());
                fb.fill_rect(rect, 255);
                m.draw(&mut fb, &assets);
                let left = pixels(area)
                    .filter(|&(x, y)| fb.get(x, y) == Some(255))
                    .count();
                assert_eq!(left, 0, "{name} mode {mode}");
                if mode == MAP_MODE {
                    let background = pixels(area)
                        .filter(|&(x, y)| fb.get(x, y) == Some(mainview::MAP_BACKGROUND))
                        .count();
                    assert!(background > 1000, "{name}: {background}");
                }
            }
        }
    }

    /// scen00a's panels: each bar as wide as the user's table says for its value (key 1
    /// narrowing the net's), the counts typed once the base has counted, the clock's
    /// mark blinking with the seconds, the detail panel showing the selected unit.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_situation_room_panels_show_the_base() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let room = assets.sitroom.as_ref().unwrap();
        let name = "scen00a.usg";
        let scenario = &assets.scenarios[name];
        let mut m = Mission::new(name.into(), scenario, Random::seeded(1), &assets.motion);
        // Past the count (every 256 ticks).
        for _ in 0..300 {
            m.tick(&[]);
        }
        let area = |kind| {
            let tables = &assets.views.situation;
            tables.find(kind).and_then(|w| w.area).unwrap()
        };
        let draw = |m: &Mission| {
            let mut fb = Framebuffer::new(Size::MODE_13H);
            m.draw(&mut fb, &assets);
            fb
        };
        let count = |fb: &Framebuffer, a: Rect, colour| {
            pixels(a)
                .filter(|&(x, y)| fb.get(x, y) == Some(colour))
                .count()
        };
        let depot = |m: &Mission| match m.world().get(EntityId(0)).map(|e| e.state) {
            Some(State::Base { depot, .. }) => depot,
            other => panic!("{other:?}"),
        };
        let fb = draw(&m);
        let power = depot(&m).power;
        let kinds = [panels::RESERVE_TYPE].into_iter().chain(panels::BAR_TYPES);
        for (kind, value) in kinds.zip(power) {
            let (a, w) = (area(kind), room.bar_widths[usize::from(value >> 3)]);
            let h = usize::from(a.bottom - a.top);
            assert_eq!(count(&fb, a, 0x0d), w as usize * h, "{kind:#x} at {value}");
        }
        let net = area(panels::BAR_TYPES[0]);
        let wide = count(&fb, net, 0x0d);
        assert!(count(&fb, area(0x60), 0x1d) > 50, "the counts");
        assert!(count(&fb, area(panels::CLOCK_TYPE), 0x14) > 50, "the clock");
        // Key 1 lowers the net's power a step at a time.
        for _ in 0..8 {
            m.key(0x02, &assets);
        }
        assert_eq!(depot(&m).power[1], power[1] - 16);
        assert!(count(&draw(&m), net, 0x0d) < wide);
        // The next second's mark, just right of the widget: drawn or not.
        let second = m.world().clock.second;
        let clock = area(panels::CLOCK_TYPE);
        let clock = Rect {
            right: clock.right + 8,
            ..clock
        };
        let shown = count(&draw(&m), clock, 0x14);
        while m.world().clock.second == second {
            m.tick(&[]);
        }
        assert_ne!(count(&draw(&m), clock, 0x14), shown);
        // The detail panel: the selected unit's lines in font 4's shades of 0x1a, fewer
        // with no unit.
        let detail = area(0x62);
        let inked = |fb: &Framebuffer| {
            pixels(detail)
                .filter(|&(x, y)| fb.get(x, y).is_some_and(|p| (0x1a..=0x1d).contains(&p)))
                .count()
        };
        assert!(m.selected.is_some());
        let unit = inked(&draw(&m));
        m.selected = None;
        let none = inked(&draw(&m));
        assert!(unit > none + 100 && none > 50, "{unit} {none}");
    }

    /// An obstacle placed with the pointer in every stock scenario's map (the editor
    /// unlocked here, not by the user's `XENO.KEY`) is drawn under the pointer.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_a_placed_obstacle_is_drawn_under_the_pointer() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let mut assets = load(&files, 13).unwrap();
        assets.editor_unlocked = true;
        let tables = assets.editor.as_ref().unwrap();
        let obstacles = tables.keys.iter().find(|k| k.1 == 0x6ae6).unwrap().0;
        let area = assets
            .views
            .situation
            .find(MAIN_VIEW_TYPE)
            .and_then(|w| w.area)
            .unwrap();
        let at = (
            (area.left + area.right) / 2 + 13,
            (area.top + area.bottom) / 2 - 7,
        );
        for (name, scenario) in &assets.scenarios {
            for zoom in [0, 3, 6] {
                let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
                (m.mode, m.map.zoom) = (MAP_MODE, zoom);
                m.key(obstacles, &assets);
                assert!(m.place_at_pointer(Some(at), &assets), "{name}");
                let mut fb = Framebuffer::new(Size::MODE_13H);
                let (w, h) = (area.right - area.left, area.bottom - area.top);
                let rect =
                    xeno_render::Rect::new(area.left.into(), area.top.into(), w.into(), h.into());
                fb.fill_rect(rect, 255);
                m.draw(&mut fb, &assets);
                // Both ways truncate, so the dot can land a pixel short, as the
                // original's does.
                let near = (at.1 - 1..=at.1 + 1)
                    .flat_map(|y| (at.0 - 1..=at.0 + 1).map(move |x| (x, y)))
                    .any(|(x, y)| fb.get(x.into(), y.into()) == Some(mainview::OBSTACLE_DOT));
                assert!(near, "{name} zoom {zoom}");
            }
        }
    }

    /// Robots of each kind and side made by the editor in every stock scenario are
    /// saved as the original saves a robot (script and target in the save's form),
    /// read back, load and run.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_made_robots_save_load_and_run() {
        use crate::game_dir::{ENV_VAR, GameDir};
        use xeno_sim::editor::{Place, Placed};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let tables = assets.editor.as_ref().unwrap();
        let program = assets.motion.scripts.program(9);
        for (name, scenario) in &assets.scenarios {
            let mut sim = Sim::load(scenario, Random::seeded(1), assets.motion.clone());
            let mut edited = scenario.clone();
            for kind in [
                usg::ObjectKind::Humanoid,
                usg::ObjectKind::Scorpion,
                usg::ObjectKind::Scout,
            ] {
                for side in [Side::Human, Side::Alien] {
                    let what = Place::Robot {
                        kind,
                        side,
                        heading: 0x4000,
                    };
                    let at = xeno_sim::Position::default();
                    let Some(Placed::Made(o)) = sim.place(what, at, tables) else {
                        panic!("{name}: nothing made");
                    };
                    let r = o.robot().unwrap();
                    assert_eq!(r.script & 0xf800, 0xa000, "{name}");
                    assert_eq!(
                        (r.script & 0x7ff).wrapping_add(tables.program_table),
                        program
                    );
                    assert_eq!((r.program_given, r.target), (9, 0xffff), "{name}");
                    edited.objects.as_mut().unwrap().push(*o);
                }
            }
            let back = usg::parse(&usg::write(&edited).unwrap()).unwrap();
            assert_eq!(back.objects, {
                let mut o = edited.objects.clone().unwrap();
                o.sort_by_key(|o| o.slot);
                Some(o)
            });
            let mut again = Sim::load(&back, Random::seeded(1), assets.motion.clone());
            assert_eq!(again.world.len(), sim.world.len(), "{name}");
            (0..200).for_each(|_| _ = again.tick(&[]));
        }
    }

    /// A stock mission run five minutes, left and saved to `<NAME>.LOG`, loads back
    /// where it stood (T-116): every object where it was, the clock as it was; saved
    /// and loaded again it is the same, and both copies go on identically.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_a_mission_left_saved_and_loaded_resumes() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let file = "scen00a.usg".to_string();
        let mut m = Mission::new(
            file.clone(),
            &assets.scenarios[&file],
            Random::seeded(1),
            &assets.motion,
        );
        (0..4500).for_each(|_| _ = m.tick(&[]));
        // Saved and loaded as the map room's N and the log-in do.
        let save_and_load = |m: &Mission| {
            let campaign = crate::maproom::tests::test_campaign();
            let bytes = crate::save::write(&campaign, Some(0), &m.saved(&assets).unwrap()).unwrap();
            let mut back = crate::maproom::tests::test_campaign();
            let (_, scenario) = crate::save::load(&bytes, &mut back).unwrap();
            Mission::new(file.clone(), &scenario, Random::seeded(2), &assets.motion)
        };
        let resumed = save_and_load(&m);
        let (before, after) = (m.world(), resumed.world());
        assert_eq!((after.len(), after.clock), (before.len(), before.clock));
        for (id, e) in before.iter() {
            let a = after.get(id).unwrap();
            assert_eq!(
                (a.kind, a.position, a.heading, a.damage),
                (e.kind, e.position, e.heading, e.damage)
            );
        }
        let (mut once, mut twice) = (resumed.clone(), save_and_load(&resumed));
        assert_eq!(twice.world(), once.world());
        for _ in 0..2000 {
            _ = once.tick(&[]);
            _ = twice.tick(&[]);
        }
        assert_eq!(twice.world(), once.world());
    }

    /// The pointer's regions (T-106): over each one, the widget the dispatcher hands
    /// a move to (the last whose rectangle holds it) is the one whose handler tests
    /// that region, for nearly all of its pixels.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_pointer_regions_are_their_widgets() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let pictures = crate::screens::load(&files).unwrap();
        let v = &assets.views;
        let [h, s, c] = REGION_WIDGETS;
        for (view, file, colours, kinds) in [
            (
                &v.situation,
                "sitroom.m@l",
                &[255][..],
                &[MAIN_VIEW_TYPE][..],
            ),
            (&v.humanoid, "humancon.m@l", &[255, 254, 253], &h),
            (&v.scorpion, "scorpcon.m@l", &[255, 254, 253], &s),
            (&v.scout, "scoutcon.m@l", &[255, 254, 253], &c),
        ] {
            let picture = &pictures.iter().find(|(n, _)| *n == file).unwrap().1;
            for (&colour, &kind) in colours.iter().zip(kinds) {
                let (mut ours, mut others) = (0, 0);
                for (i, _) in picture
                    .pixels
                    .iter()
                    .enumerate()
                    .filter(|&(_, &p)| p == colour)
                {
                    let (x, y) = ((i % 320) as u16, (i / 320) as u16);
                    let top = view
                        .widgets
                        .iter()
                        .rev()
                        .find(|w| w.area.is_some_and(|a| a.contains(x, y)));
                    *if top.is_some_and(|w| w.kind == kind) {
                        &mut ours
                    } else {
                        &mut others
                    } += 1;
                }
                assert!(
                    ours > 400 && others * 20 < ours,
                    "{file} {colour}: {others} of {ours}"
                );
            }
        }
    }

    /// Every stock scenario's first robot of each kind: its console's second display
    /// fills the colour-254 region in the camera, thermal and map modes and with a
    /// damaged device; thermal is in the remap's colours and the noise in 0x20..0x2f.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_second_display_fills_its_region() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let pictures = crate::screens::load(&files).unwrap();
        for (name, scenario) in &assets.scenarios {
            let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
            for kind in RobotKind::ALL {
                let Some(robot) = m
                    .robots()
                    .into_iter()
                    .find(|&s| m.book().get(s).unwrap().kind == kind)
                else {
                    continue;
                };
                let view = m.view_robot(robot).unwrap();
                let file = crate::screens::Screen::Mission(view).file().unwrap();
                let picture = pictures.iter().find(|(n, _)| *n == file).unwrap();
                for mode in [0, 1, MAP_DISPLAY, LOST_MISSILE_CAMERA] {
                    m.displays.entry(robot).or_default().mode = mode;
                    let mut fb = Framebuffer::new(Size::MODE_13H);
                    fb.pixels_mut().copy_from_slice(&picture.1.pixels);
                    let region: Vec<bool> =
                        fb.pixels().iter().map(|&p| p == SECOND_DISPLAY).collect();
                    let before = region.iter().filter(|&&r| r).count();
                    m.draw(&mut fb, &assets);
                    let count = |colour| {
                        fb.pixels()
                            .iter()
                            .zip(&region)
                            .filter(|&(&p, &r)| r && p == colour)
                            .count()
                    };
                    let left = count(SECOND_DISPLAY);
                    assert!(
                        before > 1000 && left < before / 10,
                        "{name} {kind:?} mode {mode}: {left} of {before}"
                    );
                    // The map is mostly its kind's clear colour.
                    let clear = assets.views.second_display.map_colours[kind.index()][0];
                    if mode == MAP_DISPLAY {
                        assert!(count(clear) > before / 2, "{name} {kind:?}");
                    }
                    let within = |colours: &[u8]| {
                        fb.pixels()
                            .iter()
                            .zip(&region)
                            .filter(|&(p, &r)| r && !colours.contains(p))
                            .count()
                    };
                    // Allow for the zoom's name, which mode 7 does not draw.
                    let ramp: Vec<u8> = (DISPLAY_CLEAR..DISPLAY_CLEAR + 16).collect();
                    match mode {
                        1 => assert!(within(&assets.thermal) < before / 20, "{name}"),
                        LOST_MISSILE_CAMERA => assert_eq!(within(&ramp), 0, "{name}"),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Each console's 3D view is drawn in a buffer the size of its picture's colour-255
    /// region, whatever the displays drawn after it leave there: the placeholders'
    /// labels, or the pointer probe's pixel on the second display (T-163).
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_3d_view_keeps_its_picture_s_region() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let pictures = crate::screens::load(&files).unwrap();
        let (name, scenario) = assets.scenarios.iter().next().unwrap();
        let mut m = Mission::new(name.clone(), scenario, Random::seeded(1), &assets.motion);
        for kind in RobotKind::ALL {
            let robot = m
                .robots()
                .into_iter()
                .find(|&s| m.book().get(s).unwrap().kind == kind)
                .unwrap();
            let view = m.view_robot(robot).unwrap();
            let file = crate::screens::Screen::Mission(view).file().unwrap();
            let picture = &pictures.iter().find(|(n, _)| *n == file).unwrap().1;
            let mut fb = Framebuffer::new(Size::MODE_13H);
            fb.pixels_mut().copy_from_slice(&picture.pixels);
            let (left, top, width, height) = review::region(&fb).unwrap();
            let (second_left, second_top, ..) = review::region_of(&fb, SECOND_DISPLAY).unwrap();
            let terrain = m.scenario.as_ref().and_then(|s| s.terrain.as_ref());
            let mut want = Framebuffer::new(Size::new(width, height));
            scene::draw(
                &mut want,
                m.world(),
                EntityId(robot),
                assets.view3d.as_ref().unwrap(),
                &assets.motion,
                terrain.map_or(&[][..], |t| &t.features),
                &assets.models,
                assets.skies.get(&m.file),
                None,
            );
            let second = ((second_left + 1) as u16, (second_top + 1) as u16);
            for pointer in [None, Some(second)] {
                m.pointer = pointer;
                fb.pixels_mut().copy_from_slice(&picture.pixels);
                m.draw(&mut fb, &assets);
                for (y, x) in (0..height).flat_map(|y| (0..width).map(move |x| (y, x))) {
                    if picture.pixels[(top + y) * 320 + left + x] == 255 {
                        let got = fb.get(left + x, top + y);
                        assert_eq!(got, want.get(x, y), "{kind:?} {pointer:?} at ({x}, {y})");
                    }
                }
            }
        }
    }

    /// The humanoid console's gauges follow the robot: the compass with its heading,
    /// the speed's digits, the energy bar, the gear lever and the warning light.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_humanoid_instruments_follow_the_robot() {
        use crate::game_dir::{ENV_VAR, GameDir};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let con = assets.humancon.as_ref().unwrap();
        assert_eq!(assets.humancon_sprites.len(), 57);
        let pictures = crate::screens::load(&files).unwrap();
        let picture = &pictures
            .iter()
            .find(|(n, _)| *n == "humancon.m@l")
            .unwrap()
            .1;
        for name in ["htrain.usg", "scen00a.usg"] {
            let mut m = Mission::new(
                name.into(),
                &assets.scenarios[name],
                Random::seeded(1),
                &assets.motion,
            );
            let robot = m
                .robots()
                .into_iter()
                .find(|&s| m.book().get(s).unwrap().kind == RobotKind::Humanoid)
                .unwrap();
            m.view_robot(robot).unwrap();
            let draw = |m: &Mission| {
                let mut fb = Framebuffer::new(Size::MODE_13H);
                fb.pixels_mut().copy_from_slice(&picture.pixels);
                m.draw(&mut fb, &assets);
                fb
            };
            let tables = &assets.views.humanoid;
            let area = |kind| {
                tables
                    .find(kind)
                    .or_else(|| tables.nested.iter().find(|w| w.kind == kind))
                    .and_then(|w| w.area)
                    .unwrap()
            };
            let pixels = |fb: &Framebuffer, a: Rect| -> Vec<u8> {
                pixels(a).map(|(x, y)| fb.get(x, y).unwrap()).collect()
            };
            let set = |m: &mut Mission, f: &dyn Fn(&mut xeno_sim::Entity)| {
                f(m.world_mut().get_mut(EntityId(robot)).unwrap());
            };
            set(&mut m, &|e| {
                e.heading = 0;
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.gear, r.guns.energy, r.locked_on, r.signal) = (0, 0, 0, 0, 0);
                }
            });
            let before = draw(&m);
            set(&mut m, &|e| {
                e.heading = 0x8000;
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.gear, r.guns.energy, r.signal) = (40, 4, 0xff, 0xff);
                }
            });
            let after = draw(&m);
            // The needle turns, and the heading reads 180 in colour 0x0f.
            assert_ne!(
                pixels(&before, con.dial),
                pixels(&after, con.dial),
                "{name}"
            );
            let compass = area(0xa6);
            let lit = |fb: &Framebuffer, a| pixels(fb, a).iter().filter(|&&p| p == 0x0f).count();
            assert!(lit(&after, compass) > 10, "{name}");
            // The speed: two digits and a sign over the unlit ones.
            let speed = area(0x9c);
            assert!(
                lit(&before, speed) > 10 && lit(&after, speed) > 10,
                "{name}"
            );
            assert_ne!(pixels(&before, speed), pixels(&after, speed), "{name}");
            // The energy bar and the signal's (the sim's) fill.
            assert_ne!(pixels(&before, con.bars[1]), pixels(&after, con.bars[1]));
            assert_ne!(pixels(&before, con.bars[0]), pixels(&after, con.bars[0]));
            assert_ne!(pixels(&before, area(0xa4)), pixels(&after, area(0xa4)));
            // The warning light blinks only while a lock holds.
            let warning = area(0xa0);
            let looks = |m: &mut Mission, locked: u8| {
                set(m, &|e| {
                    if let State::Robot(r) = &mut e.state {
                        r.locked_on = locked;
                    }
                });
                let mut looks: Vec<_> = (0..64)
                    .map(|t| {
                        m.world_mut().ticks = t;
                        pixels(&draw(m), warning)
                    })
                    .collect();
                looks.dedup();
                looks.len()
            };
            assert_eq!(looks(&mut m, 0), 1, "{name}");
            assert!(
                looks(&mut m, 0xff) > 2,
                "{name}: the warning light never blinks"
            );
            // The target panel: the no-target lines and both weapons ready, in 0x57 on
            // 0x68; the status panel filled in the message panel's colours.
            let shown = draw(&m);
            let target = area(0xa2);
            let count = |a, c| pixels(&shown, a).iter().filter(|&&p| p == c).count();
            assert!(
                count(target, 0x57) > 100 && count(target, 0x68) > 100,
                "{name}"
            );
            let [fill, text] = assets.views.message_panel.colours[0];
            let status = area(crate::humancon::STATUS_TYPE);
            assert!(
                count(status, fill) > 500 && count(status, text) > 50,
                "{name}"
            );
        }
    }

    /// The real assets and a mission of `name` with its first robot of `kind` viewed.
    fn console_of(kind: RobotKind, name: &str) -> Option<(Assets, Mission, u16)> {
        use crate::game_dir::{ENV_VAR, GameDir};
        let path = std::env::var_os(ENV_VAR)?;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let mut m = Mission::new(
            name.into(),
            &assets.scenarios[name],
            Random::seeded(1),
            &assets.motion,
        );
        let robot = m
            .robots()
            .into_iter()
            .find(|&s| m.book().get(s).unwrap().kind == kind)
            .unwrap();
        m.view_robot(robot).unwrap();
        Some((assets, m, robot))
    }

    /// The scorpion console's gauges follow the robot: the compass, the speed's digits
    /// and lever, the energy bar, the cargo gauges, the damage panel and the warning
    /// light.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_scorpion_instruments_follow_the_robot() {
        use crate::scorpcon as s;
        for name in ["ctrain.usg", "scen00a.usg"] {
            let Some((assets, mut m, robot)) = console_of(RobotKind::Scorpion, name) else {
                return;
            };
            let con = assets.scorpcon.as_ref().unwrap();
            assert_eq!(assets.scorpcon_sprites.len(), 103);
            let draw = |m: &Mission| {
                let mut fb = Framebuffer::new(Size::MODE_13H);
                m.draw(&mut fb, &assets);
                fb
            };
            let tables = &assets.views.scorpion;
            let area = |kind| tables.all(kind).next().and_then(|w| w.area).unwrap();
            let pixels = |fb: &Framebuffer, a: Rect| -> Vec<u8> {
                pixels(a).map(|(x, y)| fb.get(x, y).unwrap()).collect()
            };
            let set = |m: &mut Mission, f: &dyn Fn(&mut xeno_sim::Entity)| {
                f(m.world_mut().get_mut(EntityId(robot)).unwrap());
            };
            set(&mut m, &|e| {
                e.heading = 0;
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.guns.energy, r.locked_on, r.relays, r.signal) = (0, 0, 0, 0, 0);
                }
            });
            let before = draw(&m);
            set(&mut m, &|e| {
                e.heading = 0x8000;
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.guns.energy, r.relays, r.signal) = (40, 0xff, 12, 0xff);
                }
            });
            let after = draw(&m);
            // The needle turns, and the heading reads 180 in colour 0x0e.
            assert_ne!(pixels(&before, con.dial), pixels(&after, con.dial));
            let lit = |fb: &Framebuffer, a| pixels(fb, a).iter().filter(|&&p| p == 0x0e).count();
            assert!(lit(&after, area(0xd6)) > 10, "{name}");
            // The speed: two digits over the unlit ones; the lever moves.
            let speed = area(0xce);
            assert!(lit(&before, speed) > 5 && lit(&after, speed) > 5, "{name}");
            assert_ne!(pixels(&before, speed), pixels(&after, speed), "{name}");
            let lever = area(s::LEVER_TYPE);
            assert_ne!(pixels(&before, lever), pixels(&after, lever), "{name}");
            // The energy bar and the signal's (the sim's) fill.
            assert_ne!(pixels(&before, con.bars[1]), pixels(&after, con.bars[1]));
            assert_ne!(pixels(&before, con.bars[0]), pixels(&after, con.bars[0]));
            // The cargo gauges follow the relays.
            for kind in s::GAUGE_TYPES {
                assert_ne!(pixels(&before, area(kind)), pixels(&after, area(kind)));
            }
            // The damage panel shows a light every other 16 ticks once the helm is low;
            // undamaged, it only blinks the cover off and on.
            let damage = area(s::DAMAGE_TYPE);
            let looks = |m: &mut Mission| {
                let mut looks: Vec<_> = (0..64)
                    .map(|t| {
                        m.world_mut().ticks = t;
                        pixels(&draw(m), damage)
                    })
                    .collect();
                looks.sort();
                looks.dedup();
                looks.len()
            };
            assert_eq!(looks(&mut m), 1, "{name}");
            set(&mut m, &|e| {
                if let xeno_sim::Damage::Robot(c) = &mut e.damage {
                    c.helm = 50;
                }
            });
            assert_eq!(looks(&mut m), 2, "{name}: the helm's light never shows");
            // The warning light blinks only while a lock holds.
            let warning = area(s::WARNING_TYPE);
            let blinks = |m: &mut Mission, locked: u8| {
                set(m, &|e| {
                    if let State::Robot(r) = &mut e.state {
                        r.locked_on = locked;
                    }
                });
                let mut looks: Vec<_> = (0..64)
                    .map(|t| {
                        m.world_mut().ticks = t;
                        pixels(&draw(m), warning)
                    })
                    .collect();
                looks.dedup();
                looks.len()
            };
            assert_eq!(blinks(&mut m, 0), 1, "{name}");
            assert!(blinks(&mut m, 0xff) > 2, "{name}: the light never blinks");
            // The target panel in 0x7a on 0; the relay panel's text in 0x77, which
            // changes with the count.
            let shown = draw(&m);
            let count = |fb, a, c| pixels(fb, a).iter().filter(|&&p| p == c).count();
            assert!(count(&shown, area(0xd0), 0x7a) > 100, "{name}");
            let relays = area(s::RELAYS_TYPE);
            assert!(count(&before, relays, 0x77) > 20, "{name}");
            assert_ne!(pixels(&before, relays), pixels(&after, relays), "{name}");
            // The drop buttons are numbered 0 to 3, the weapon buttons 0 and 1.
            let numbers = |kind| {
                let w = tables.all(kind);
                w.map(|w| m.widget_byte(robot, w).unwrap())
                    .collect::<Vec<_>>()
            };
            assert_eq!(numbers(s::DROP_TYPE), [0, 1, 2, 3], "{name}");
            assert_eq!(numbers(s::WEAPON_TYPE), [0, 1], "{name}");
        }
    }

    /// The scout console's gauges follow the robot: the compass, the speed's digits, the
    /// power panel's lights and energy bar, the gear lever, the stance gauge and the
    /// warning light.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_scout_instruments_follow_the_robot() {
        use crate::scoutcon as c;
        for name in ["strain.usg", "scen00a.usg"] {
            let Some((assets, mut m, robot)) = console_of(RobotKind::Scout, name) else {
                return;
            };
            let con = assets.scoutcon.as_ref().unwrap();
            assert_eq!(assets.scoutcon_sprites.len(), 80);
            let draw = |m: &Mission| {
                let mut fb = Framebuffer::new(Size::MODE_13H);
                m.draw(&mut fb, &assets);
                fb
            };
            let tables = &assets.views.scout;
            let area = |kind| tables.all(kind).next().and_then(|w| w.area).unwrap();
            let pixels = |fb: &Framebuffer, a: Rect| -> Vec<u8> {
                pixels(a).map(|(x, y)| fb.get(x, y).unwrap()).collect()
            };
            let set = |m: &mut Mission, f: &dyn Fn(&mut xeno_sim::Entity)| {
                f(m.world_mut().get_mut(EntityId(robot)).unwrap());
            };
            let battery = |on: bool| {
                move |e: &mut xeno_sim::Entity| {
                    if let xeno_sim::Damage::Robot(c) = &mut e.damage {
                        c.equipment = (c.equipment & !usg::Robot::ON_BATTERY) | u8::from(on);
                    }
                }
            };
            set(&mut m, &|e| {
                e.heading = 0;
                battery(false)(e);
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.guns.energy, r.locked_on, r.signal) = (0, 0, 0, 0);
                    (r.gear, r.stance) = (0, 0);
                }
            });
            let before = draw(&m);
            set(&mut m, &|e| {
                e.heading = 0x8000;
                battery(true)(e);
                if let State::Robot(r) = &mut e.state {
                    (r.speed, r.guns.energy, r.signal) = (20, 0xff, 0xff);
                    (r.gear, r.stance) = (10, 0x60);
                }
            });
            let after = draw(&m);
            // The needle turns, and the heading reads 180 in colour 0x0e.
            assert_ne!(pixels(&before, con.dial), pixels(&after, con.dial));
            let count = |fb, a, c| pixels(fb, a).iter().filter(|&&p| p == c).count();
            assert!(count(&after, area(0x10a), 0x0e) > 10, "{name}");
            // The speed: two digits in 0x60 over the unlit ones.
            let speed = area(0x12c);
            assert!(count(&before, speed, 0x60) > 5 && count(&after, speed, 0x60) > 5);
            assert_ne!(pixels(&before, speed), pixels(&after, speed), "{name}");
            // The lights swap on the battery; the energy bar and the signal's (the
            // sim's) fill.
            for light in con.lights {
                assert_ne!(pixels(&before, light), pixels(&after, light), "{name}");
            }
            assert_ne!(pixels(&before, con.bars[0]), pixels(&after, con.bars[0]));
            // The bar fills from the bottom: its lowest rows light first.
            let low = Rect {
                top: con.bars[1].bottom - 4,
                ..con.bars[1]
            };
            assert_ne!(pixels(&before, low), pixels(&after, low), "{name}");
            // The gear lever and the stance gauge move.
            for kind in [c::GEAR_TYPE, c::STANCE_TYPE] {
                assert_ne!(pixels(&before, area(kind)), pixels(&after, area(kind)));
            }
            // The warning light blinks only while a lock holds.
            let warning = area(c::WARNING_TYPE);
            let blinks = |m: &mut Mission, locked: u8| {
                set(m, &|e| {
                    if let State::Robot(r) = &mut e.state {
                        r.locked_on = locked;
                    }
                });
                let mut looks: Vec<_> = (0..64)
                    .map(|t| {
                        m.world_mut().ticks = t;
                        pixels(&draw(m), warning)
                    })
                    .collect();
                looks.dedup();
                looks.len()
            };
            assert_eq!(blinks(&mut m, 0), 1, "{name}");
            assert!(blinks(&mut m, 0xff) > 2, "{name}: the light never blinks");
            // The target panel in 0x60 on 0x01; the status panel turns its four pages
            // every 512 passes.
            let shown = draw(&m);
            assert!(count(&shown, area(0x11e), 0x60) > 100, "{name}");
            let status = area(c::STATUS_TYPE);
            let mut pages: Vec<_> = (0..4)
                .map(|p| {
                    m.world_mut().ticks = 512 * p;
                    pixels(&draw(&m), status)
                })
                .collect();
            pages.sort();
            pages.dedup();
            assert_eq!(pages.len(), 4, "{name}");
            // The weapon buttons are numbered 0 and 1.
            let numbers: Vec<_> = tables
                .all(c::WEAPON_TYPE)
                .map(|w| m.widget_byte(robot, w).unwrap())
                .collect();
            assert_eq!(numbers, [0, 1], "{name}");
        }
    }

    /// The scout's buttons act on the robot through the sim: the weapon buttons select
    /// (one held shows its own look), the switches hand over, the stance buttons ask for
    /// a stance with the shared look while held, the lock button's look, the fire button
    /// held, the drop buttons drop one of each.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_scout_buttons_act() {
        use crate::scoutcon as c;
        let Some((assets, mut m, robot)) = console_of(RobotKind::Scout, "strain.usg") else {
            return;
        };
        let tables = &assets.views.scout;
        let find = |kind| tables.all(kind).next().unwrap();
        let centre = |w: &views::Widget| {
            let a = w.area.unwrap();
            Some(((a.left + a.right) / 2, (a.top + a.bottom) / 2))
        };
        let click = |m: &mut Mission, w: &views::Widget, button| {
            m.mouse_down(button, centre(w), &assets);
            m.mouse_up(button, centre(w), &assets);
        };
        let state = |m: &Mission| match m.world().get(EntityId(robot)).unwrap().state {
            State::Robot(r) => r,
            _ => unreachable!(),
        };
        let draw = |m: &Mission| {
            let mut fb = Framebuffer::new(Size::MODE_13H);
            m.draw(&mut fb, &assets);
            pixels(find(c::WEAPON_TYPE).area.unwrap())
                .chain(pixels(
                    tables.all(c::WEAPON_TYPE).nth(1).unwrap().area.unwrap(),
                ))
                .map(|(x, y)| fb.get(x, y).unwrap())
                .collect::<Vec<_>>()
        };
        let weapons: Vec<_> = tables.all(c::WEAPON_TYPE).collect();
        click(&mut m, weapons[0], LEFT);
        assert_eq!(state(&m).weapon, 0);
        // Weapon 1 held shows its next look; let go, it is the one selected.
        let before = draw(&m);
        m.mouse_down(LEFT, centre(weapons[1]), &assets);
        let held = draw(&m);
        assert_ne!(before, held);
        m.mouse_up(LEFT, centre(weapons[1]), &assets);
        assert_eq!(state(&m).weapon, 1);
        assert_ne!(draw(&m), held);
        let drives = |m: &Mission| m.book().get(robot).unwrap().orders.program_drives;
        click(&mut m, find(c::PILOT_TYPE), LEFT);
        assert!(!drives(&m));
        click(&mut m, find(c::PROGRAM_TYPE), LEFT);
        assert!(drives(&m));
        // The stance buttons: E, W and Q's stances, the look pressed while held.
        for (kind, stance) in c::STANCE_TYPES.into_iter().zip([0, 0x30, 0x60]) {
            m.mouse_down(LEFT, centre(find(kind)), &assets);
            assert!(m.look.pressed);
            m.mouse_up(LEFT, centre(find(kind)), &assets);
            assert!(!m.look.pressed);
            assert_eq!(state(&m).stance_target, stance);
        }
        assert!(drives(&m), "a stance leaves the program driving");
        // The cloak and ECM buttons: pressed while held; let go, F8 and F7 run.
        for kind in [c::CLOAK_TYPE, c::ECM_TYPE] {
            m.mouse_down(LEFT, centre(find(kind)), &assets);
            assert!(m.look.pressed);
            m.mouse_up(LEFT, centre(find(kind)), &assets);
            assert!(!m.look.pressed);
        }
        let s = usg::status::CLOAKED | usg::status::ECM;
        assert_eq!(state(&m).status & s, s);
        // The drop buttons: pressed while held; let go, each drops one of its count
        // (buttons 0 to 3 run keys 1, 4, 3 and 2).
        if let Some(State::Robot(r)) = m.world_mut().get_mut(EntityId(robot)).map(|e| &mut e.state)
        {
            r.guns.drops = [2; 4];
        }
        for w in tables.all(c::DROP_TYPE).collect::<Vec<_>>() {
            m.mouse_down(LEFT, centre(w), &assets);
            assert!(m.look.pressed);
            m.mouse_up(LEFT, centre(w), &assets);
            assert!(!m.look.pressed);
        }
        assert_eq!(state(&m).guns.drops, [1; 4]);
        // Keys 1 to 4 are in the console's key table, as the handlers `PILOT` maps.
        for h in [0xd5e7, 0xd609, 0xd62b, 0xd64d] {
            assert!(tables.keys.iter().any(|&(_, k)| k == h), "{h:#x}");
        }
        // The lock button: held, its look is pressed and held; let go, both clear.
        let lock = find(c::LOCK_TYPE);
        m.mouse_down(RIGHT, centre(lock), &assets);
        assert!(m.look.pressed && m.look.lock_held);
        m.mouse_up(RIGHT, centre(lock), &assets);
        assert_eq!(m.look, crate::console::Look::default());
        // The fire button: held, every tick sets the fire request; let go, it stops.
        let fire = find(c::FIRE_TYPE);
        m.mouse_down(LEFT, centre(fire), &assets);
        assert!(m.firing && m.look.pressed);
        m.mouse_up(LEFT, None, &assets);
        assert!(!m.firing && !m.look.pressed);
    }

    /// The scorpion's buttons act on the robot through the sim: the weapon buttons
    /// select, the switches hand over, the lock button's look, the fire button held,
    /// stop asks for speed 0 and hands the robot to the pilot, U deploys a relay, the drop
    /// buttons drop one of each.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_scorpion_buttons_act() {
        use crate::scorpcon as s;
        let Some((assets, mut m, robot)) = console_of(RobotKind::Scorpion, "ctrain.usg") else {
            return;
        };
        let tables = &assets.views.scorpion;
        let find = |kind| tables.all(kind).next().unwrap();
        let centre = |w: &views::Widget| {
            let a = w.area.unwrap();
            Some(((a.left + a.right) / 2, (a.top + a.bottom) / 2))
        };
        let click = |m: &mut Mission, w: &views::Widget, button| {
            m.mouse_down(button, centre(w), &assets);
            m.mouse_up(button, centre(w), &assets);
        };
        let state = |m: &Mission| match m.world().get(EntityId(robot)).unwrap().state {
            State::Robot(r) => r,
            _ => unreachable!(),
        };
        for w in tables
            .all(s::WEAPON_TYPE)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            click(&mut m, w, LEFT);
            assert_eq!(state(&m).weapon, m.widget_byte(robot, w).unwrap());
        }
        assert_eq!(state(&m).weapon, 0);
        let drives = |m: &Mission| m.book().get(robot).unwrap().orders.program_drives;
        click(&mut m, find(s::PILOT_TYPE), LEFT);
        assert!(!drives(&m));
        click(&mut m, find(s::PROGRAM_TYPE), LEFT);
        assert!(drives(&m));
        // Stop: speed 0 asked for, the pilot drives; the look flips and back.
        if let Some(State::Robot(r)) = m.world_mut().get_mut(EntityId(robot)).map(|e| &mut e.state)
        {
            r.target_speed = 42;
        }
        m.mouse_down(LEFT, centre(find(s::STOP_TYPE)), &assets);
        assert!(m.look.pressed);
        m.mouse_up(LEFT, centre(find(s::STOP_TYPE)), &assets);
        assert!(!m.look.pressed && !drives(&m));
        assert_eq!(state(&m).target_speed, 0);
        // U lays a relay where the scorpion stands.
        let relays = state(&m).relays;
        click(&mut m, find(s::DEPLOY_TYPE), LEFT);
        assert_eq!(state(&m).relays, relays - 1);
        // The drop buttons: pressed while held; let go, each drops one of its count
        // (buttons 0 to 3 run keys 1, 4, 3 and 2).
        if let Some(State::Robot(r)) = m.world_mut().get_mut(EntityId(robot)).map(|e| &mut e.state)
        {
            r.guns.drops = [2; 4];
        }
        for w in tables.all(s::DROP_TYPE).collect::<Vec<_>>() {
            m.mouse_down(LEFT, centre(w), &assets);
            assert!(m.look.pressed);
            m.mouse_up(LEFT, centre(w), &assets);
            assert!(!m.look.pressed);
        }
        assert_eq!(state(&m).guns.drops, [1; 4]);
        // Keys 1 to 4 are in the console's key table, as the handlers `PILOT` maps.
        for h in [0xd5e7, 0xd609, 0xd62b, 0xd64d] {
            assert!(tables.keys.iter().any(|&(_, k)| k == h), "{h:#x}");
        }
        // The lock button: held, its look is pressed and held; let go, both clear.
        let lock = find(s::LOCK_TYPE);
        m.mouse_down(LEFT, centre(lock), &assets);
        assert!(m.look.pressed && m.look.lock_held);
        m.mouse_up(LEFT, centre(lock), &assets);
        assert_eq!(m.look, crate::console::Look::default());
        // The fire button: held, every tick sets the fire request; let go, it stops.
        let fire = find(s::FIRE_TYPE);
        m.mouse_down(LEFT, centre(fire), &assets);
        assert!(m.firing && m.look.pressed);
        m.mouse_up(LEFT, None, &assets);
        assert!(!m.firing && !m.look.pressed);
    }

    /// The humanoid's buttons act on the robot through the sim, as their mouse handlers
    /// do: the weapon buttons select, the switches hand over, the lock button's press
    /// lights its other look and its release clears it, the fire button fires while
    /// held.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_the_humanoid_buttons_act() {
        use crate::game_dir::{ENV_VAR, GameDir};
        use crate::humancon as h;
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let name = "htrain.usg";
        let mut m = Mission::new(
            name.into(),
            &assets.scenarios[name],
            Random::seeded(1),
            &assets.motion,
        );
        let robot = m
            .robots()
            .into_iter()
            .find(|&s| m.book().get(s).unwrap().kind == RobotKind::Humanoid)
            .unwrap();
        m.view_robot(robot).unwrap();
        let tables = &assets.views.humanoid;
        let all: Vec<_> = tables.widgets.iter().chain(&tables.nested).collect();
        let centre = |w: &views::Widget| {
            let a = w.area.unwrap();
            Some(((a.left + a.right) / 2, (a.top + a.bottom) / 2))
        };
        let click = |m: &mut Mission, w: &views::Widget, button| {
            m.mouse_down(button, centre(w), &assets);
            m.mouse_up(button, centre(w), &assets);
        };
        let weapon = |m: &Mission| match m.world().get(EntityId(robot)).unwrap().state {
            State::Robot(r) => r.weapon,
            _ => unreachable!(),
        };
        // The weapon buttons' weapons are 0 and 1 in the stock files.
        let weapons: Vec<_> = all.iter().filter(|w| w.kind == h::WEAPON_TYPE).collect();
        assert_eq!(weapons.len(), 2);
        for (n, w) in weapons.into_iter().enumerate().rev() {
            assert_eq!(m.widget_byte(robot, w), Some(n as u8));
            click(&mut m, w, LEFT);
            assert_eq!(weapon(&m), n as u8);
        }
        let find = |kind| *all.iter().find(|w| w.kind == kind).unwrap();
        let drives = |m: &Mission| m.book().get(robot).unwrap().orders.program_drives;
        click(&mut m, find(h::PILOT_TYPE), LEFT);
        assert!(!drives(&m));
        click(&mut m, find(h::PROGRAM_TYPE), RIGHT);
        assert!(drives(&m));
        // The cloak and ECM buttons run F8 and F7 on the release: one cloak fewer.
        let robot_state = |m: &Mission| match m.world().get(EntityId(robot)).unwrap().state {
            State::Robot(r) => r,
            _ => unreachable!(),
        };
        let cloaks = robot_state(&m).guns.cloaks;
        click(&mut m, find(h::CLOAK_TYPE), LEFT);
        click(&mut m, find(h::ECM_TYPE), LEFT);
        let s = usg::status::CLOAKED | usg::status::ECM;
        assert_eq!(robot_state(&m).status & s, s);
        assert_eq!(robot_state(&m).guns.cloaks, cloaks - 1);
        // The lock button: held, its look is pressed and held; let go, both clear.
        let lock = find(h::LOCK_TYPE);
        m.mouse_down(LEFT, centre(lock), &assets);
        assert_eq!(
            m.look,
            crate::console::Look {
                pressed: true,
                lock_held: true
            }
        );
        m.mouse_up(LEFT, centre(lock), &assets);
        assert_eq!(m.look, crate::console::Look::default());
        // The fire button: held, every tick sets the fire request; let go, it stops.
        let fire = find(h::FIRE_TYPE);
        m.mouse_down(LEFT, centre(fire), &assets);
        assert!(m.firing && m.look.pressed);
        m.mouse_up(LEFT, None, &assets);
        assert!(!m.firing && !m.look.pressed);
    }

    #[test]
    fn view_colours_get_a_placeholder() {
        let assets = test_assets();
        let m = Mission::empty();
        let mut fb = Framebuffer::new(Size::MODE_13H);
        fb.fill_rect(xeno_render::Rect::new(100, 50, 40, 20), 255);
        m.draw(&mut fb, &assets);
        let left: Vec<_> = (100..140).filter_map(|x| fb.get(x, 69)).collect();
        assert!(left.contains(&0) && left.contains(&255));
        assert!(assets.claims_escape(View::Humanoid));
    }

    /// A training mission is won and lost by play alone, through the keys (T-150, T-177,
    /// T-141). On `ntrain`, the first scorpion is left to its program for 60,000
    /// retraces (about 14 minutes: the alien escort's first attacks are spent on the
    /// human net), then its console: Up for speed, F6 (the tail gun), steered at the
    /// alien base by Left and Right, stopped by `.` near it; A when nothing is locked or,
    /// near the base, until the base is; Space fires at what is locked. Won about 22
    /// minutes in. On `htrain`, each of the player's units in turn is steered by its
    /// console's keys to stand just behind the player's base, away from the aliens, and
    /// stopped with `.`: the aliens' shots at them hit the base, which falls about 15
    /// minutes in. (Left alone, the units fight the aliens on the human net, where the
    /// aliens run on their batteries, and win: `docs/sim.md`, "The port (T-148)".)
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_a_training_mission_is_won_and_lost_by_play() {
        use crate::game_dir::{ENV_VAR, GameDir};
        use xeno_sim::{Fixed, Position};
        let Some(path) = std::env::var_os(ENV_VAR) else {
            return;
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = GameDir::resolve(None, Some(root.join(path).into()))
            .check()
            .unwrap();
        let assets = load(&files, 13).unwrap();
        let start = |name: &str| {
            let s = &assets.scenarios[name];
            Mission::new(name.into(), s, Random::seeded(1), &assets.motion)
        };
        let end = |m: &mut Mission, play: &mut dyn FnMut(&mut Mission)| {
            (0..200_000).find_map(|_| {
                play(m);
                m.tick([]).filter(|r| matches!(r, Request::Over { .. }))
            })
        };
        // What the player sees of `at` from robot `slot`: its bearing off the robot's
        // heading, how far in units, and the robot's speed.
        let sight = |m: &Mission, slot: u16, at: Position| {
            let me = m.world().get(EntityId(slot))?;
            let State::Robot(r) = me.state else {
                return None;
            };
            let (dx, dy) = (
                f64::from(at.x.0) - f64::from(me.position.x.0),
                f64::from(at.y.0) - f64::from(me.position.y.0),
            );
            let off = ((dx.atan2(dy) / std::f64::consts::TAU * 65536.0) as i64 as u16)
                .wrapping_sub(me.heading) as i16;
            Some((off, dx.hypot(dy) / 65536.0, r))
        };
        let mut m = start("ntrain.usg");
        let robot = m
            .robots()
            .into_iter()
            .find(|&s| m.book().get(s).unwrap().kind == RobotKind::Scorpion)
            .unwrap();
        m.view_robot(robot).unwrap();
        let base = m.world().base(Side::Alien).unwrap();
        let mut retrace = 0u32;
        let won = end(&mut m, &mut |m| {
            retrace += 1;
            if retrace < 60_000 {
                return;
            }
            let at = m.world().get(base).unwrap().position;
            let Some((off, units, r)) = sight(m, robot, at) else {
                panic!("the scorpion is gone at retrace {retrace}");
            };
            let mut press = |word| _ = m.key(word, &assets);
            if retrace == 60_000 {
                (0..5).for_each(|_| press(0x48));
                press(0x40);
            }
            if off > 0x100 {
                press(0x4d);
            } else if off < -0x100 {
                press(0x4b);
            }
            if units < 5.0 && r.speed > 0 {
                press(0x34);
            }
            if retrace.is_multiple_of(16)
                && (r.target.is_none() || units < 8.0 && r.target != Some(base))
            {
                press(0x1e);
            }
            if retrace.is_multiple_of(4) && r.target.is_some() {
                press(0x39);
            }
        });
        assert_eq!(won, Some(Request::Over { won: true }));

        let mut m = start("htrain.usg");
        let at = m
            .world()
            .get(m.world().base(Side::Human).unwrap())
            .unwrap()
            .position;
        let units = m.robots();
        // Half a unit behind the base (the aliens come from +x), 0.3 units apart.
        let spot = |k: usize| Position {
            x: Fixed(at.x.0 - 0x8000),
            y: Fixed(at.y.0 + (k as i32 - 1) * 0x4ccd),
        };
        let mut stood = vec![false; units.len()];
        let mut retrace = 0u32;
        let lost = end(&mut m, &mut |m| {
            retrace += 1;
            let k = (retrace / 4) as usize % units.len();
            if !retrace.is_multiple_of(4) || stood[k] {
                return;
            }
            let Some((off, distance, r)) = sight(m, units[k], spot(k)) else {
                return;
            };
            if m.viewed() != Viewed::Robot(units[k]) {
                m.view_robot(units[k]);
            }
            let mut press = |word| _ = m.key(word, &assets);
            if distance < 0.08 {
                press(0x34);
                stood[k] = r.speed == 0;
                return;
            }
            if off > 0x200 {
                press(0x4d);
            } else if off < -0x200 {
                press(0x4b);
            }
            if r.speed == 0 && off.unsigned_abs() < 0x800 {
                press(0x48);
            }
        });
        assert_eq!(lost, Some(Request::Over { won: false }));
    }
}
