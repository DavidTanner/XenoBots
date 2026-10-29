//! What the port reads from the user's `xeno.exe` for the robots' weapons and the shots
//! they fire (`docs/sim.md`, "Weapons and shots"): where each kind's tick table runs its
//! weapon step, each gun's energy, shot kind, sound, cool-down, damage, height and
//! muzzle offsets, the scorpion's burst and missile reload, the four shot kinds'
//! speeds, arming times, lives, turns and explosions, and the weapons' messages. Game
//! data, read at run time through the instructions that use it, after checking their
//! bytes.

use crate::FormatError;
use crate::damage::{Explosion, explosion};
use crate::farewell::operand;
use crate::maproom::{invalid, word};
use crate::sensing::TICKS_AT;

/// An immediate the port reads: the instruction's address, its bytes before the value,
/// and whether the value is a word (else a byte).
pub(crate) type Site = (usize, &'static [u8], bool);

/// The weapon steps, by their first bytes (`cmp byte [di + 0xdf], 0`, the ECM's
/// count; the scorpion's burst; its missile's reload): humanoid `1000:b08a`, scorpion
/// `9713` and its reload `9781`, scout `c8ab`.
pub(crate) const STEPS: [(usize, &[u8]); 4] = [
    (0xb08a, &[0x80, 0xbd, 0xdf, 0x00, 0x00]),
    (0x9713, &[0x80, 0xbd, 0x1e, 0x01, 0x00]),
    (0x9781, &[0x83, 0xbd, 0x25, 0x01, 0x00]),
    (0xc8ab, &[0x80, 0xbd, 0xdf, 0x00, 0x00]),
];

/// Where a gun's routine sets each of a [`Gun`]'s values; a field it lacks is `None`.
pub(crate) struct GunAt {
    pub energy: usize,
    pub kind: usize,
    pub sound: usize,
    pub cooldown: Site,
    pub damage: Option<Site>,
    pub height: Option<Site>,
    pub offsets: Option<usize>,
}

const ENERGY: &[u8] = &[0xb0];
const MOV_AX: &[u8] = &[0xb8];
/// The guns in [`Weapons::guns`]' order: the humanoid's cannon (`1000:b0f6`) and
/// missile (`b16e`), the scorpion's cannon (`982d`) and tail missile (`978d`), the
/// scout's plasma gun (`c8ff`) and cannon (`c990`).
pub(crate) const GUNS: [GunAt; 6] = [
    GunAt {
        energy: 0xb102,
        kind: 0xb10a,
        offsets: Some(0xb122),
        height: Some((0xb13c, &[0xb9], true)),
        damage: Some((0xb13f, &[0xba], true)),
        sound: 0xb145,
        cooldown: (0xb14b, &[0xc6, 0x85, 0x08, 0x01], false),
    },
    GunAt {
        energy: 0xb173,
        kind: 0xb17b,
        offsets: Some(0xb193),
        height: Some((0xb1ad, &[0xb9], true)),
        damage: None,
        sound: 0xb1b3,
        cooldown: (0xb1b9, &[0xc6, 0x85, 0x09, 0x01], false),
    },
    GunAt {
        energy: 0x9842,
        kind: 0x9871,
        offsets: None,
        height: None,
        damage: Some((0x987c, &[0xb0], false)),
        sound: 0x9881,
        cooldown: (0x988b, &[0xc6, 0x85, 0x1f, 0x01], false),
    },
    GunAt {
        energy: 0x97a9,
        kind: 0x97bf,
        offsets: None,
        height: None,
        damage: None,
        sound: 0x97d7,
        cooldown: (0x97f9, &[0xc7, 0x85, 0x25, 0x01], true),
    },
    GunAt {
        energy: 0xc91b,
        kind: 0xc923,
        offsets: Some(0xc93b),
        height: Some((0xc958, &[0x81, 0xe9], true)),
        damage: Some((0xc95c, &[0xba], true)),
        sound: 0xc962,
        cooldown: (0xc968, &[0xc6, 0x85, 0x05, 0x01], false),
    },
    GunAt {
        energy: 0xc9ac,
        kind: 0xc9b4,
        offsets: None,
        height: None,
        damage: Some((0xc9e6, &[0xb0], false)),
        sound: 0xc9eb,
        cooldown: (0xc9f5, &[0xc6, 0x85, 0x06, 0x01], false),
    },
];
/// The two sound routines a gun's `mov ax, 2 * event` is followed by a call to:
/// `snd_play_sound_effect_if_viewed` and `snd_play_sound_effect`.
pub(crate) const SOUND_VIEWED: usize = 0xeec6;
pub(crate) const SOUND_ALL: usize = 0xee90;
/// `mov byte [di + 0x11e], burst` in the scorpion's trigger (`1000:98f1`), and `mov
/// bx, [si + table]` in its weapon key (`98a9`): the field each weapon counts in.
pub(crate) const BURST: Site = (0x98f1, &[0xc6, 0x85, 0x1e, 0x01], false);
pub(crate) const COUNTS_AT: (usize, &[u8]) = (0x98cd, &[0x8b, 0x9c]);
/// `mov si, message` before each weapon message, in [`Messages`]' order.
pub(crate) const MESSAGES_AT: [usize; 7] = [0xb156, 0xb15b, 0x9810, 0x9806, 0x980b, 0xca0a, 0x97ea];

/// The shell's set-up (`1000:e235`) and tick (`e2c8`), in [`Shell`]'s order.
pub(crate) const SHELL: [Site; 9] = [
    (0xe238, &[0xc7, 0x44, 0x20], true),
    (0xe23d, &[0xc7, 0x44, 0x18], true),
    (0xe25a, MOV_AX, true),
    (0xe27d, MOV_AX, true),
    (0xe275, &[0x25], true),
    (0xe2a3, &[0x25], true),
    (0xe299, &[0xc7, 0x44, 0x1e], true),
    (0xe2c1, &[0x25], true),
    (0xe2eb, &[0x83, 0x6d, 0x1e], false),
];
/// `mov ax, 2 * event` before the homing shot's explosion's sound (`1000:e461`); every
/// shot's explosion plays the same.
pub(crate) const BLAST_SOUND_AT: usize = 0xe461;
/// The homing shot's set-up (`1000:e34a`) and tick (`e3ca`).
pub(crate) const HOMING: [Site; 5] = [
    (0xe35a, &[0xc7, 0x44, 0x25], true),
    (0xe35f, &[0xc7, 0x44, 0x1b], true),
    (0xe3cd, &[0x81, 0x7d, 0x27], true),
    (0xe3d7, &[0xf7, 0x45, 0x27], true),
    (0xe3fb, &[0xbb], true),
];
/// The missile's set-up (`1000:e48a`) and tick (`e537`).
pub(crate) const MISSILE: [Site; 7] = [
    (0xe48d, &[0xc7, 0x44, 0x23], true),
    (0xe492, &[0xc7, 0x44, 0x1b], true),
    (0xe497, &[0xc7, 0x44, 0x25], true),
    (0xe4db, &[0x25], true),
    (0xe4ea, MOV_AX, true),
    (0xe587, &[0xb0], false),
    (0xe531, &[0xc7, 0x44, 0x10], true),
];
/// The tail missile's set-up (`1000:e08e`) and tick (`e0e8`).
pub(crate) const TAIL: [Site; 13] = [
    (0xe0b9, MOV_AX, true),
    (0xe09c, &[0xc7, 0x44, 0x27], true),
    (0xe170, &[0x83, 0x45, 0x27], false),
    (0xe102, &[0x83, 0x7d, 0x25], false),
    (0xe112, &[0xc7, 0x45, 0x1b], true),
    (0xe129, &[0x81, 0x45, 0x1b], true),
    (0xe122, &[0x81, 0x7d, 0x1b], true),
    (0xe117, &[0xc7, 0x45, 0x1d], true),
    (0xe11c, &[0x83, 0x7d, 0x25], false),
    (0xe198, &[0x3d], true),
    (0xe21d, &[0x81, 0xfb], true),
    (0xe1fa, &[0xb0], false),
    (0xe1fe, &[0xbb], true),
];
/// `jmp [bx + table]` in `1000:f6d0`: the kinds' hit tests, which a shot's hit search
/// asks of an object it is over. `mov bx, [bx + table]` in `1000:e963`: the kinds'
/// heights, which aiming adds to the target's z.
pub(crate) const HITS_AT: (usize, &[u8]) = (0xf6d4, &[0xff, 0xa7]);
pub(crate) const HEIGHTS_AT: (usize, &[u8]) = (0xe963, &[0x8b, 0x9f]);
/// The kinds the tables cover (0 to 16, the obstacle).
pub const KINDS: usize = 17;

/// `mov ax, template` before each shot's explosion: the shell's hit and ground, the
/// homing shot's, the missile's, the tail missile's.
pub(crate) const EXPLOSIONS_AT: [usize; 7] =
    [0xe309, 0xe32e, 0xe45b, 0xe47c, 0xe57b, 0xe596, 0xe204];

/// Each kind's weapons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weapons {
    /// The entries of each kind's tick table (humanoid, scorpion, scout) that run its
    /// weapon step, and the scorpion's that counts its missile's reload.
    pub steps: [[u8; 2]; 3],
    pub reload_step: u8,
    /// The humanoid's cannon and missile, the scorpion's cannon and tail missile, the
    /// scout's plasma gun and cannon.
    pub guns: [Gun; 6],
    /// The fire requests the scorpion's trigger queues.
    pub burst: u8,
    /// The scorpion's weapons' counts, as offsets in the robot (rounds, missiles).
    pub counts: [u16; 2],
    pub messages: Messages,
    pub shell: Shell,
    pub homing: Homing,
    pub missile: Missile,
    pub tail: Tail,
    /// The sound event a shot's explosion plays for everyone.
    pub blast_sound: u8,
    /// By kind: whether a shot over an object of the kind hits it, and its height, which
    /// aiming adds to the target's z shifted left by 8 (8.8 units, as a radius).
    pub hits: [Hit; KINDS],
    pub heights: [u16; KINDS],
}

/// A kind's hit test: `stc`, `clc`, a band of the shot's height over the object
/// (`low < dz <= high`; the humanoid's) or a height the shot must be under (the
/// obstacle's; z in `0..below`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hit {
    Always,
    #[default]
    Never,
    Band(u16, u16),
    Below(u16),
}

/// One gun: what firing takes, makes and plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gun {
    /// The energy (+0x50) firing takes on the battery.
    pub energy: u8,
    /// The kind of shot it makes.
    pub kind: u16,
    /// The sound event, and whether everyone hears it (else only the robot's console).
    pub sound: u8,
    pub heard_by_all: bool,
    /// The weapon steps (the tail missile: ticks) before it fires again.
    pub cooldown: u16,
    /// The shot's damage; 0 where the shot kind has its own.
    pub damage: u8,
    /// The shot's height over the robot (the scout's plasma: under its eye), 16.16's
    /// low word; 0 where the shot kind has its own.
    pub height: u16,
    /// The muzzle's x and y offsets (1/65,536ths) for the 8 eighths of a quarter turn
    /// of the heading; turned by the quarter (`DS:0x708f`).
    pub offsets: Option<[(i16, i16); 8]>,
}

/// The weapons' messages to the robot's panel (string-segment offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Messages {
    pub no_target: u16,
    pub no_power: u16,
    pub damaged: u16,
    pub no_missiles: u16,
    pub reloading: u16,
    pub no_shells: u16,
    pub launched: u16,
}

/// Kind 6, the cannons' shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shell {
    /// Steps before it can hit (two a tick), its speed, its height over the robot.
    pub arming: u16,
    pub speed: u16,
    pub height: u16,
    /// The speed the aim's lead is worked out with.
    pub lead: u16,
    /// The random spread of its heading (mask; half is taken off) with a target and
    /// without.
    pub spread: u16,
    pub wild_spread: u16,
    /// Its climb with a target, and the random climb's mask without.
    pub climb: u16,
    pub wild_climb: u16,
    /// Taken off the climb each step.
    pub fall: u8,
    pub hit: Explosion,
    pub ground: Explosion,
}

/// Kind 7, the homing shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Homing {
    pub arming: u16,
    pub speed: u16,
    /// Ticks it lives.
    pub life: u16,
    /// It turns to its target on the ticks whose age has none of these bits, by at
    /// most `turn`.
    pub steer_mask: u16,
    pub turn: u16,
    pub hit: Explosion,
    pub ground: Explosion,
}

/// Kind 8, the humanoid's missile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Missile {
    pub arming: u16,
    pub speed: u16,
    pub life: u16,
    /// The spread's mask (half taken off).
    pub spread: u16,
    pub lead: u16,
    pub damage: u8,
    /// The heading it keeps (its model's).
    pub heading: u16,
    pub hit: Explosion,
    pub ground: Explosion,
}

/// Kind 5, the scorpion's tail missile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tail {
    /// Its height over the scorpion at launch.
    pub height: u16,
    /// Its first turn limit, and what each homing tick adds.
    pub turn: u16,
    pub turn_step: u8,
    /// Ticks it waits on the tail.
    pub launch: u8,
    /// Its speed when it leaves the tail, what each boosting tick adds, and the top.
    pub speed: u16,
    pub boost: u16,
    pub top: u16,
    /// Its climb while boosting, and the boost's ticks.
    pub climb: u16,
    pub boost_ticks: u8,
    /// Within this (8.8) of its target it climbs or dives at it, by at most `dive`.
    pub near: u16,
    pub dive: u16,
    /// Its blast: points and radius (8.8).
    pub points: u8,
    pub radius: u16,
    pub explosion: Explosion,
}

fn value(image: &[u8], &(at, op, wide): &Site) -> Result<u16, FormatError> {
    if wide {
        return operand(image, at, op);
    }
    match image.get(at..=at + op.len()) {
        Some(b) if b[..op.len()] == *op => Ok(b[op.len()].into()),
        _ => Err(invalid(
            at,
            format!("not the expected instruction {op:02x?}"),
        )),
    }
}

fn values<const N: usize>(image: &[u8], sites: &[Site; N]) -> Result<[u16; N], FormatError> {
    let mut out = [0; N];
    for (v, site) in out.iter_mut().zip(sites) {
        *v = value(image, site)?;
    }
    Ok(out)
}

/// The sound a `mov ax, 2 * event` at `at` plays, and whether for everyone, by the
/// sound routine the `call` after it reaches.
fn sound(image: &[u8], at: usize) -> Result<(u8, bool), FormatError> {
    let event = operand(image, at, MOV_AX)?;
    let call = operand(image, at + 3, &[0xe8])?;
    let to = usize::from((at as u16 + 6).wrapping_add(call));
    if event % 2 != 0 || event > 0x1ff || ![SOUND_VIEWED, SOUND_ALL].contains(&to) {
        return Err(invalid(at, "not a sound event"));
    }
    Ok(((event / 2) as u8, to == SOUND_ALL))
}

/// A muzzle table through `mov ax, [bx + table]`, `mov cx, [bx + table + 2]`.
fn offsets(image: &[u8], ds: usize, at: usize) -> Result<[(i16, i16); 8], FormatError> {
    let table = operand(image, at, &[0x8b, 0x87])?;
    if operand(image, at + 4, &[0x8b, 0x8f])? != table.wrapping_add(2) {
        return Err(invalid(at, "not a muzzle table"));
    }
    let mut out = [(0, 0); 8];
    for (i, o) in out.iter_mut().enumerate() {
        let at = ds + usize::from(table) + 4 * i;
        *o = (word(image, at)? as i16, word(image, at + 2)? as i16);
    }
    Ok(out)
}

fn gun(image: &[u8], ds: usize, g: &GunAt) -> Result<Gun, FormatError> {
    let (sound, heard_by_all) = sound(image, g.sound)?;
    let optional = |s: &Option<Site>| s.as_ref().map_or(Ok(0), |s| value(image, s));
    Ok(Gun {
        energy: value(image, &(g.energy, ENERGY, false))? as u8,
        kind: operand(image, g.kind, MOV_AX)?,
        sound,
        heard_by_all,
        cooldown: value(image, &g.cooldown)?,
        damage: optional(&g.damage)? as u8,
        height: optional(&g.height)?,
        offsets: g.offsets.map(|at| offsets(image, ds, at)).transpose()?,
    })
}

/// The entries of the tick table through the `call` at `at` whose routine is `routine`.
fn entries(image: &[u8], ds: usize, at: usize, routine: usize) -> Result<Vec<u8>, FormatError> {
    let table = ds + usize::from(operand(image, at, &[0xff, 0x97])?);
    let mut found = Vec::new();
    for n in 0..crate::sensing::TICKS {
        if usize::from(word(image, table + 2 * n)?) == routine {
            found.push(n as u8);
        }
    }
    Ok(found)
}

fn steps(image: &[u8], ds: usize) -> Result<([[u8; 2]; 3], u8), FormatError> {
    for (at, bytes) in STEPS {
        if image.get(at..at + bytes.len()) != Some(bytes) {
            return Err(invalid(at, "not a weapon step"));
        }
    }
    let two = |kind: usize, routine: usize| match *entries(image, ds, TICKS_AT[kind], routine)? {
        [a, b] => Ok([a, b]),
        _ => Err(invalid(
            TICKS_AT[kind],
            "a weapon step not twice in its tick table",
        )),
    };
    let reload = match *entries(image, ds, TICKS_AT[1], STEPS[2].0)? {
        [n] => n,
        _ => {
            return Err(invalid(
                TICKS_AT[1],
                "the reload not once in its tick table",
            ));
        }
    };
    Ok((
        [
            two(0, STEPS[0].0)?,
            two(1, STEPS[1].0)?,
            two(2, STEPS[3].0)?,
        ],
        reload,
    ))
}

fn hit(image: &[u8], at: usize) -> Result<Hit, FormatError> {
    let b = image
        .get(at..at + 11)
        .ok_or_else(|| invalid(at, "past the end of the image"))?;
    let w = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    Ok(match *b {
        [0xf9, 0xc3, ..] => Hit::Always,
        [0xf8, 0xc3, ..] => Hit::Never,
        [0xb9, _, _, 0xba, _, _, 0xe8, ..] => Hit::Band(w(1), w(4)),
        [0x83, 0x7d, 0x0e, 0x00, 0x75, 0x09, 0x81, 0x7d, 0x0c, ..] => Hit::Below(w(9)),
        _ => return Err(invalid(at, "not a hit test")),
    })
}

fn by_kind<T: Copy + Default>(
    image: &[u8],
    ds: usize,
    (at, op): (usize, &[u8]),
    f: impl Fn(u16) -> Result<T, FormatError>,
) -> Result<[T; KINDS], FormatError> {
    let table = ds + usize::from(operand(image, at, op)?);
    let mut out = [T::default(); KINDS];
    for (i, o) in out.iter_mut().enumerate() {
        *o = f(word(image, table + 2 * i)?)?;
    }
    Ok(out)
}

/// Reads the tables from a load image whose data segment starts at `ds`.
pub fn read_image(image: &[u8], ds: usize) -> Result<Weapons, FormatError> {
    let (steps, reload_step) = steps(image, ds)?;
    let e = |i: usize| explosion(image, ds, EXPLOSIONS_AT[i]);
    let m = |i: usize| operand(image, MESSAGES_AT[i], &[0xbe]);
    let counts = ds + usize::from(operand(image, COUNTS_AT.0, COUNTS_AT.1)?);
    let [
        arming,
        speed,
        height,
        lead,
        spread,
        wild_spread,
        climb,
        wild_climb,
        fall,
    ] = values(image, &SHELL)?;
    let (blast_sound, _) = sound(image, BLAST_SOUND_AT)?;
    let shell = Shell {
        arming,
        speed,
        height,
        lead,
        spread,
        wild_spread,
        climb,
        wild_climb,
        fall: fall as u8,
        hit: e(0)?,
        ground: e(1)?,
    };
    let [arming, speed, life, steer_mask, turn] = values(image, &HOMING)?;
    let homing = Homing {
        arming,
        speed,
        life,
        steer_mask,
        turn,
        hit: e(2)?,
        ground: e(3)?,
    };
    let [arming, speed, life, spread, lead, damage, heading] = values(image, &MISSILE)?;
    let missile = Missile {
        arming,
        speed,
        life,
        spread,
        lead,
        damage: damage as u8,
        heading,
        hit: e(4)?,
        ground: e(5)?,
    };
    let [
        h,
        turn,
        turn_step,
        launch,
        speed,
        boost,
        top,
        climb,
        boost_ticks,
        near,
        dive,
        points,
        radius,
    ] = values(image, &TAIL)?;
    let tail = Tail {
        height: h,
        turn,
        turn_step: turn_step as u8,
        launch: launch as u8,
        speed,
        boost,
        top,
        climb,
        boost_ticks: boost_ticks as u8,
        near,
        dive,
        points: points as u8,
        radius,
        explosion: e(6)?,
    };
    let g = |i: usize| gun(image, ds, &GUNS[i]);
    Ok(Weapons {
        steps,
        reload_step,
        guns: [g(0)?, g(1)?, g(2)?, g(3)?, g(4)?, g(5)?],
        burst: value(image, &BURST)? as u8,
        counts: [word(image, counts)?, word(image, counts + 2)?],
        messages: Messages {
            no_target: m(0)?,
            no_power: m(1)?,
            damaged: m(2)?,
            no_missiles: m(3)?,
            reloading: m(4)?,
            no_shells: m(5)?,
            launched: m(6)?,
        },
        shell,
        homing,
        missile,
        tail,
        blast_sound,
        hits: by_kind(image, ds, HITS_AT, |r| hit(image, usize::from(r)))?,
        heights: by_kind(image, ds, HEIGHTS_AT, Ok)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: usize = 0x10000;

    fn synthetic() -> Vec<u8> {
        let mut image = vec![0u8; DS + 0xa000];
        crate::fixtures::synthetic_sensing(&mut image, DS);
        crate::fixtures::synthetic_weapons(&mut image, DS);
        image
    }

    #[test]
    fn synthetic_tables_read() {
        let w = read_image(&synthetic(), DS).unwrap();
        assert_eq!((w.steps, w.reload_step), ([[1, 9], [1, 12], [1, 11]], 9));
        let cannon = w.guns[0];
        assert_eq!((cannon.energy, cannon.kind, cannon.cooldown), (10, 7, 12));
        assert_eq!((cannon.sound, cannon.heard_by_all), (1, false));
        assert_eq!(cannon.offsets.unwrap()[1], (0x100, -0x200));
        assert_eq!((w.guns[2].sound, w.guns[2].heard_by_all), (5, true));
        assert_eq!((w.guns[3].cooldown, w.guns[3].offsets), (400, None));
        assert_eq!((w.burst, w.counts), (5, [0x120, 0x122]));
        assert_eq!((w.shell.fall, w.blast_sound, w.tail.points), (3, 4, 100));
        assert_eq!(w.homing.hit.last, 0x1e);
        assert_eq!(
            (w.hits[0], w.hits[7], w.hits[16]),
            (Hit::Band(0, 0x1300), Hit::Never, Hit::Below(0x900))
        );
        assert_eq!(
            (w.hits[3], w.heights[0], w.heights[3]),
            (Hit::Always, 10, 20)
        );
        insta::assert_debug_snapshot!(w);
    }

    #[test]
    fn another_build_is_refused() {
        for at in [
            STEPS[2].0 + 1,
            TICKS_AT[0] + 1,
            GUNS[1].energy,
            GUNS[4].offsets.unwrap() + 4,
            GUNS[2].sound + 4,
            GUNS[3].cooldown.0 + 2,
            BURST.0 + 3,
            COUNTS_AT.0 + 1,
            MESSAGES_AT[6],
            SHELL[8].0 + 1,
            TAIL[12].0,
            EXPLOSIONS_AT[6],
            HITS_AT.0,
            HEIGHTS_AT.0 + 1,
            0x8104,
            0x8110 + 2,
        ] {
            let mut image = synthetic();
            image[at] ^= 0x01;
            assert!(read_image(&image, DS).is_err(), "{at:#x}");
        }
        // A step in a tick table once only.
        let mut image = synthetic();
        image[DS + 0x9700 + 2 * 9] = 0;
        assert!(read_image(&image, DS).is_err());
    }

    /// The user's tables: every kind steps its weapons twice in 16 ticks, the guns make
    /// the shot kinds `docs/sim.md` names and play known sounds, and the shots' numbers
    /// have the traced shapes.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_weapons_read() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let exe = crate::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
        let motion = crate::motion::read(&exe).unwrap();
        let w = &motion.weapons;
        assert!(w.steps.iter().all(|s| s[0] < s[1]));
        let kinds: Vec<u16> = w.guns.iter().map(|g| g.kind).collect();
        assert_eq!(kinds, [7, 8, 6, 5, 7, 6]);
        assert!(w.guns.iter().all(|g| g.sound < 29 && g.cooldown > 0));
        assert_eq!(w.counts, [0x120, 0x122]);
        // Robots, bases and relays can be hit; shots and explosions cannot.
        assert!(matches!(w.hits[0], Hit::Band(..)) && matches!(w.hits[16], Hit::Below(_)));
        assert!(w.hits[1..5].iter().all(|&h| h == Hit::Always));
        assert!(
            w.hits[5..9]
                .iter()
                .chain(&w.hits[13..16])
                .all(|&h| h == Hit::Never)
        );
        assert!(w.shell.speed > w.homing.speed && w.homing.speed > w.missile.speed);
        assert!(w.tail.speed < w.tail.top && w.tail.near > 0);
        for e in [
            w.shell.hit,
            w.homing.hit,
            w.missile.ground,
            w.tail.explosion,
        ] {
            assert!(e.first < e.last && e.ticks > 0);
        }
        let m = w.messages;
        for at in [
            m.no_target,
            m.no_power,
            m.damaged,
            m.no_missiles,
            m.launched,
        ] {
            assert!(motion.strings.byte(at) != 0, "{at:#x}");
        }
    }
}
