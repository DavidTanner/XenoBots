use super::*;
use crate::motion::synthetic_tables;
use crate::motion::tests::{at, base, robot};
use crate::orders::{Orders, Robot};

/// A human humanoid (slot 4, with orders) and scorpion (6), an alien scout (9), both
/// bases (0 human, 1 alien), an obstacle (2) and a relay (3).
fn world() -> World {
    let mut world = World::default();
    world.insert(EntityId(0), base(Side::Human, 0, 0, 0));
    world.insert(EntityId(1), base(Side::Alien, 50, 0, 0));
    world.bases = [Some(EntityId(0)), Some(EntityId(1))];
    for (slot, kind, side) in [
        (4, ObjectKind::Humanoid, Side::Human),
        (6, ObjectKind::Scorpion, Side::Human),
        (9, ObjectKind::Scout, Side::Alien),
    ] {
        world.insert(EntityId(slot), robot(kind, side, slot as i16, 7));
        world.units[usize::from(crate::world::side_code(side))].push(EntityId(slot));
    }
    let robot_kind = crate::orders::RobotKind::of(ObjectKind::Humanoid).unwrap();
    world.orders.insert(
        4,
        Robot {
            kind: robot_kind,
            side: Side::Human,
            orders: Orders::given(8),
        },
    );
    let mut relay = base(Side::Human, 3, 3, 0);
    (relay.kind, relay.state) = (ObjectKind::Relay, State::None);
    world.insert(EntityId(3), relay.clone());
    (relay.kind, relay.damage) = (ObjectKind::Obstacle, Damage::None);
    world.insert(EntityId(2), relay);
    world
}

fn parts(world: &World, id: u16) -> Components {
    match world.get(EntityId(id)).unwrap().damage {
        Damage::Robot(c) => c,
        other => panic!("{other:?}"),
    }
}

fn hit_for(world: &mut World, id: u16, points: u8, random: &mut Random) -> Vec<Event> {
    let mut events = Vec::new();
    hit(
        world,
        EntityId(id),
        points,
        &synthetic_tables(),
        random,
        &mut events,
    );
    events
}

#[test]
fn a_point_picks_the_entry_whose_chances_reach_the_random_byte_or_carry() {
    let t = synthetic_tables().damage;
    // The humanoid's: 0x80 wear the helm, then 0xff the weapon bits.
    let humanoid = &t.robots[0].parts;
    assert!(matches!(
        pick(humanoid, 0),
        Some(Part::Wear { offset: 0x24, .. })
    ));
    assert!(matches!(pick(humanoid, 0x80), Some(Part::Wear { .. })));
    assert!(matches!(pick(humanoid, 0x81), Some(Part::Bits(_))));
    // The scorpion's 0x40 then 0xc0: the second sum carries to 0 and still picks.
    let scorpion = &t.robots[1].parts;
    assert_eq!(pick(scorpion, 0x40), Some(&Part::Nothing));
    assert!(matches!(
        pick(scorpion, 0x41),
        Some(Part::Wear { offset: 0x4e, .. })
    ));
    assert!(matches!(pick(scorpion, 0xff), Some(Part::Wear { .. })));
}

#[test]
fn wear_stops_at_0_and_bits_set_the_first_clear_one() {
    let mut random = Random::seeded(3);
    let mut c = parts(&world(), 4);
    c.helm = 2;
    let helm = Part::Wear {
        offset: 0x24,
        mask: 0xff,
    };
    let mut expected = random.clone();
    let off = expected.word() as u8;
    wear(&mut c, &helm, &mut random);
    assert_eq!(c.helm, 2u8.saturating_sub(off));
    assert_eq!(random, expected);
    let weapon = Part::Bits(vec![(0xdc, 1), (0xf6, 0x40)]);
    wear(&mut c, &weapon, &mut random);
    assert_eq!((c.weapons, c.systems), (1, 0));
    wear(&mut c, &weapon, &mut random);
    assert_eq!((c.weapons, c.systems), (1, 0x40));
    let before = c;
    wear(&mut c, &weapon, &mut random);
    wear(&mut c, &Part::Nothing, &mut random);
    assert_eq!(c, before);
    // Bytes the sim does not keep are left alone, and so is the random word.
    wear(&mut c, &Part::Bits(vec![(0x99, 1)]), &mut random);
    assert_eq!(c, before);
    assert_eq!(random, expected);
}

#[test]
fn a_hit_wears_a_robot_point_by_point_and_posts_event_7() {
    let mut world = world();
    let mut random = Random::seeded(7);
    // 0 points: only the event, and no random word drawn.
    let before = random.clone();
    assert!(hit_for(&mut world, 9, 0, &mut random).is_empty());
    assert_eq!(random, before);
    let events = |w: &World| match w.get(EntityId(9)).unwrap().state {
        State::Robot(m) => m.script.events,
        _ => 0,
    };
    assert_eq!(events(&world), 1 << 7);
    // The scout's table: 0x10 nothing, then wear the power by a masked word.
    let mut expected = random.clone();
    let mut power = 100u8;
    for _ in 0..80 {
        if (expected.word() >> 8) as u8 > 0x10 {
            power = power.saturating_sub(expected.word() as u8 & 0xf);
        }
    }
    hit_for(&mut world, 9, 80, &mut random);
    assert!(power < 100);
    assert_eq!(parts(&world, 9).power, power);
    assert_eq!(random, expected);
    // 80 points is not over the threshold: the scout is still there.
    assert!(world.get(EntityId(9)).is_some());
}

#[test]
fn a_robot_hit_over_the_threshold_is_destroyed_and_counted() {
    let mut world = world();
    let mut random = Random::seeded(1);
    let events = hit_for(&mut world, 4, 0x51, &mut random);
    assert_eq!(events, [Event::Destroyed(EntityId(4))]);
    assert!(world.orders.get(4).is_none());
    assert_eq!(world.units(Side::Human), [EntityId(6)]);
    // A loss for its side, a kill for the other.
    let counts = |id: u16| match world.get(EntityId(id)).unwrap().state {
        State::Base { kills, losses, .. } => (kills, losses),
        _ => panic!(),
    };
    assert_eq!((counts(0), counts(1)), ((0, 1), (1, 0)));
    // An explosion where it stood, in the lowest free slot (5): its own is freed after.
    assert!(world.get(EntityId(4)).is_none());
    let e = world.get(EntityId(5)).unwrap();
    assert_eq!(
        (e.kind, e.position, e.heading),
        (ObjectKind::Other(13), at(4, 7), 0x140)
    );
    assert_eq!(
        e.state,
        State::Explosion {
            frame: 0,
            last: 9,
            ticks: 10,
            countdown: 10
        }
    );
}

#[test]
fn a_base_is_destroyed_at_300_and_a_relay_removed_at_20() {
    let mut world = world();
    let mut random = Random::seeded(1);
    world.get_mut(EntityId(0)).unwrap().damage = Damage::Taken(250);
    assert!(hit_for(&mut world, 0, 49, &mut random).is_empty());
    assert_eq!(
        hit_for(&mut world, 0, 1, &mut random),
        [Event::Destroyed(EntityId(0))]
    );
    // A destroyed base stays, and takes no more.
    assert!(hit_for(&mut world, 0, 9, &mut random).is_empty());
    let b = world.get(EntityId(0)).unwrap();
    assert_eq!(b.damage, Damage::Taken(300));
    assert!(matches!(
        b.state,
        State::Base {
            destroyed: true,
            ..
        }
    ));
    // A relay at 19 stays; one more removes it and leaves an explosion.
    assert!(hit_for(&mut world, 3, 19, &mut random).is_empty());
    assert_eq!(
        hit_for(&mut world, 3, 1, &mut random),
        [Event::Destroyed(EntityId(3))]
    );
    assert!(world.get(EntityId(3)).is_none());
    assert_eq!(world.get(EntityId(5)).unwrap().kind, ObjectKind::Other(13));
    // The relay's damage is a byte: a sum that carries removes it too.
    let mut world = super::tests::world();
    world.get_mut(EntityId(3)).unwrap().damage = Damage::Taken(250);
    assert_eq!(
        hit_for(&mut world, 3, 10, &mut random),
        [Event::Destroyed(EntityId(3))]
    );
}

#[test]
fn an_explosion_steps_its_frames_and_goes_after_the_last() {
    let tables = synthetic_tables();
    let mut world = World::default();
    spawn_explosion(&mut world, at(1, 1), 13, tables.damage.robot_explosion);
    let mut random = Random::seeded(1);
    let frame = |w: &World| match w.get(EntityId(0)).map(|e| e.state) {
        Some(State::Explosion { frame, .. }) => Some(frame),
        _ => None,
    };
    for _ in 0..9 {
        crate::motion::tick(&mut world, &tables, &mut random);
    }
    assert_eq!(frame(&world), Some(0));
    crate::motion::tick(&mut world, &tables, &mut random);
    assert_eq!(frame(&world), Some(1));
    // Frames 0 to 9 at 10 ticks each: gone on the 100th tick.
    for _ in 10..99 {
        crate::motion::tick(&mut world, &tables, &mut random);
    }
    assert_eq!(frame(&world), Some(9));
    crate::motion::tick(&mut world, &tables, &mut random);
    assert!(world.is_empty());
}

/// On stock data a hit of 80 wears a robot down and one of 81 destroys it, counted
/// on both bases.
#[test]
#[ignore = "needs XENOBOTS_GAME_DIR"]
fn real_assets_a_stock_robot_is_worn_and_destroyed() {
    let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
        return;
    };
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    let exe = xeno_formats::lzexe::unpack(&std::fs::read(dir.join("xeno.exe")).unwrap()).unwrap();
    let tables = xeno_formats::motion::read(&exe).unwrap();
    let file = std::fs::read(dir.join("scen00a.usg")).unwrap();
    let scenario = xeno_formats::usg::parse(&file).unwrap();
    let mut sim = crate::Sim::load(&scenario, Random::seeded(1), tables);
    let [first, second] = sim.world.units(Side::Human)[..2] else {
        panic!("scen00a has human units");
    };
    let before = sim.world.get(first).unwrap().damage;
    assert!(sim.hit(first, 80).is_empty());
    assert_ne!(sim.world.get(first).unwrap().damage, before);
    assert_eq!(sim.hit(second, 81), [Event::Destroyed(second)]);
    assert!(!sim.world.units(Side::Human).contains(&second));
    let kills = |side| match sim.world.get(sim.world.base(side).unwrap()).unwrap().state {
        State::Base { kills, losses, .. } => (kills, losses),
        _ => panic!(),
    };
    let base = |side| match scenario
        .objects
        .iter()
        .flatten()
        .find(|o| o.kind == ObjectKind::Base && o.side() == side)
        .map(|o| &o.body)
    {
        Some(xeno_formats::usg::Body::Base(b)) => (b.kills, b.losses),
        _ => panic!(),
    };
    assert_eq!(kills(Side::Alien).0, base(Side::Alien).0 + 1);
    assert_eq!(kills(Side::Human).1, base(Side::Human).1 + 1);
    assert!(
        sim.world
            .iter()
            .any(|(_, e)| matches!(e.state, State::Explosion { .. }))
    );
}
