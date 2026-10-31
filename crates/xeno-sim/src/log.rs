//! The bases' message logs (+0x59 to +0xb4) and the messages sent to a whole side
//! (`docs/screens.md`, "The message log (T-129)"): a line is the sender's name, the
//! mission clock and a text (a string-segment offset); a base keeps seven, dropping
//! the oldest.

use xeno_formats::usg::{Message, ObjectKind, Side};

use crate::motion::{Tables, movement};
use crate::world::{EntityId, State, World};

pub const LINES: usize = 7;

/// `1fe6:7f38`: a robot's kind's letters, `-` and its serial's last three digits; a
/// base, a relay and kind 0xa their fixed names.
pub fn name(world: &World, id: EntityId, tables: &Tables) -> [u8; 6] {
    let n = &tables.messages;
    let Some(e) = world.get(id) else {
        return [0; 6];
    };
    match e.kind {
        ObjectKind::Base => n.base,
        ObjectKind::Relay => n.relay,
        ObjectKind::Other(0x0a) => n.other,
        kind => {
            let serial = match e.state {
                State::Robot(m) => m.serial & 0x7fff,
                _ => 0,
            };
            let index = crate::orders::RobotKind::of(kind).map_or(0, |k| k.index());
            let [a, b] = n.letters[index];
            let d = format!("{serial:05}").into_bytes();
            [a, b, b'-', d[2], d[3], d[4]]
        }
    }
}

/// `1000:65a6`: `id` writes `text` into the log of `side`'s base: in the first empty
/// line or, with none, the last after the rest move up; the base's +0xb4 is set.
// shortcut: the original also asks the situation room to redraw its log, its C button
// and the status view (+0xe3, +0xfd, +0xc6); the port draws them every frame.
pub fn write(world: &mut World, id: EntityId, side: Side, text: u16, tables: &Tables) {
    let mut line = Message {
        hour: world.clock.hour,
        minute: world.clock.minute,
        second: world.clock.second,
        tick: world.clock.tick,
        text,
        ..Message::default()
    };
    line.name[..6].copy_from_slice(&name(world, id, tables));
    let Some(State::Base { depot, .. }) = world
        .base(side)
        .and_then(|b| world.get_mut(b))
        .map(|b| &mut b.state)
    else {
        return;
    };
    depot.new_message = 1;
    let free = depot.log.iter().position(|l| l.text == 0);
    let at = free.unwrap_or_else(|| {
        depot.log.rotate_left(1);
        LINES - 1
    });
    depot.log[at] = line;
}

/// `1000:d53f`: a base logs `text` for its side; anything else is sent it on its
/// panel.
pub fn say(world: &mut World, id: EntityId, text: u16, tables: &Tables) {
    match world.get(id).map(|e| (e.kind, e.side)) {
        Some((ObjectKind::Base, side)) => write(world, id, side, text, tables),
        _ => {
            if let Some(m) = movement(world, id) {
                m.panel.post(text);
            }
        }
    }
}

/// `1000:d54c`: `id` logs `text` for `side`, then every robot of `side` in the human
/// side's unit list (`DS:0x62f5`, the only list it walks) is sent the header and the
/// text.
pub fn tell_side(world: &mut World, id: EntityId, side: Side, text: u16, tables: &Tables) {
    write(world, id, side, text, tables);
    let header = tables.messages.header;
    for u in world.units[0].clone() {
        if world.get(u).is_some_and(|e| e.side == side)
            && let Some(m) = movement(world, u)
        {
            m.panel.post(header);
            m.panel.post(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Depot;

    fn log(world: &World, side: Side) -> [Message; LINES] {
        match world.get(world.base(side).unwrap()).unwrap().state {
            State::Base { depot, .. } => depot.log,
            _ => unreachable!(),
        }
    }

    #[test]
    fn lines_fill_then_the_oldest_drops() {
        let tables = crate::motion::synthetic_tables();
        let mut world = crate::damage::tests::world();
        let base = world.base(Side::Human).unwrap();
        let robot = world.units[0][0];
        if let Some(State::Robot(m)) = world.get_mut(robot).map(|e| &mut e.state) {
            m.serial = 0x8000 | 1234;
        }
        world.clock.hour = 9;
        for text in 1..=8 {
            write(&mut world, robot, Side::Human, text, &tables);
        }
        let l = log(&world, Side::Human);
        assert_eq!(l.map(|l| l.text), [2, 3, 4, 5, 6, 7, 8]);
        assert_eq!((&l[0].name, l[0].hour), (b"HU-234\0", 9));
        say(&mut world, base, 9, &tables);
        assert_eq!(log(&world, Side::Human)[6].name, *b"BB    \0");
        assert!(matches!(
            world.get(base).unwrap().state,
            State::Base {
                depot: Depot { new_message: 1, .. },
                ..
            }
        ));
    }

    #[test]
    fn a_side_s_robots_get_the_header_and_the_text() {
        let tables = crate::motion::synthetic_tables();
        let mut world = crate::damage::tests::world();
        let robot = world.units[0][0];
        tell_side(&mut world, robot, Side::Human, 5, &tables);
        let m = movement(&mut world, robot).unwrap();
        assert_eq!(m.panel.queue[..2], [tables.messages.header, 5]);
        assert_eq!(log(&world, Side::Human)[0].text, 5);
        // The alien side's robots are in no list the original walks.
        tell_side(&mut world, robot, Side::Alien, 6, &tables);
        assert_eq!(log(&world, Side::Alien)[0].text, 6);
    }
}
