//! Synthetic test data. Tests build their inputs with these helpers instead of reading
//! game files, which may never be committed (see `AGENTS.md`).
//!
//! Available to this crate's tests, and to other crates through the `test-fixtures`
//! feature:
//!
//! ```toml
//! [dev-dependencies]
//! xeno-formats = { path = "../xeno-formats", features = ["test-fixtures"] }
//! ```

/// `len` pseudo-random bytes, the same for the same `seed` on every platform and run.
///
/// Use it to fill the parts of a synthetic file whose contents do not matter to the
/// test, such as pixel data, so tests stay reproducible without hand-written arrays.
pub fn synthetic_bytes(len: usize, seed: u64) -> Vec<u8> {
    // SplitMix64: tiny, fast and well distributed; quality beyond that is not needed.
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let mut bytes = Vec::with_capacity(len.next_multiple_of(8));
    while bytes.len() < len {
        bytes.extend_from_slice(&next().to_le_bytes());
    }
    bytes.truncate(len);
    bytes
}

/// A PCX 3.0 file, 8 bits per pixel, holding `pixels` (`width * height` indices, row
/// by row) and `palette` (768 bytes of 8-bit `r, g, b`). Scanlines are padded to an
/// even length, as the format asks, and each is run-length encoded on its own.
pub fn synthetic_pcx(width: usize, height: usize, pixels: &[u8], palette: &[u8]) -> Vec<u8> {
    use binrw::BinWrite;

    assert_eq!(pixels.len(), width * height, "pixel count");
    assert_eq!(palette.len(), crate::Palette::BYTES, "palette size");
    let stride = width.next_multiple_of(2);
    let header = crate::pcx::PcxHeader {
        version: 5,
        encoding: 1,
        bits_per_pixel: 8,
        x_min: 0,
        y_min: 0,
        x_max: (width - 1) as u16,
        y_max: (height - 1) as u16,
        h_dpi: 72,
        v_dpi: 72,
        ega_palette: [0; 48],
        reserved_40: 0,
        planes: 1,
        bytes_per_line: stride as u16,
        palette_info: 1,
        h_screen_size: 0,
        v_screen_size: 0,
        reserved_4a: [0; 54],
    };
    let mut out = std::io::Cursor::new(Vec::new());
    header.write(&mut out).unwrap();
    let mut out = out.into_inner();

    for row in pixels.chunks_exact(width) {
        let mut row = row.to_vec();
        row.resize(stride, 0);
        for run in row.chunk_by(|a, b| a == b) {
            for part in run.chunks(0x3f) {
                if part.len() > 1 || part[0] >= 0xc0 {
                    out.push(0xc0 | part.len() as u8);
                }
                out.push(part[0]);
            }
        }
    }
    out.push(crate::pcx::Pcx::PALETTE_MARKER);
    out.extend_from_slice(palette);
    out
}

/// `data` compressed the way `.b@n` and `.m@l` files are (see `crate::packed`), header
/// included. A greedy matcher: not the game's compressor, but the same stream format.
pub fn synthetic_packed(data: &[u8]) -> Vec<u8> {
    use crate::packed::{END, PackedHeader};

    const WINDOW: usize = 1024;
    const MAX_MATCH: usize = 32;
    // Length 1 can collide with the end marker; 3 is where a match starts to pay.
    const MIN_MATCH: usize = 3;

    let paragraphs = data.len().div_ceil(PackedHeader::PARAGRAPH);
    let mut out = u16::try_from(paragraphs)
        .expect("data fits in a u16 paragraph count")
        .to_le_bytes()
        .to_vec();
    let mut literals: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literals: &mut Vec<u8>| {
        for run in literals.chunks(0x80) {
            out.push(run.len() as u8 - 1);
            out.extend_from_slice(run);
        }
        literals.clear();
    };

    let mut pos = 0;
    while pos < data.len() {
        let (mut best_len, mut best_distance) = (0, 0);
        for distance in 1..=pos.min(WINDOW) {
            let len = (0..MAX_MATCH.min(data.len() - pos))
                .take_while(|&k| data[pos - distance + k] == data[pos + k])
                .count();
            if len > best_len {
                (best_len, best_distance) = (len, distance);
            }
        }
        if best_len < MIN_MATCH {
            literals.push(data[pos]);
            pos += 1;
            continue;
        }
        flush(&mut out, &mut literals);
        let d = best_distance - 1;
        out.push(0x80 | ((best_len - 1) as u8) << 2 | ((d >> 1) & 1) as u8 | ((d & 1) as u8) << 1);
        out.push((d >> 2) as u8);
        pos += best_len;
    }
    flush(&mut out, &mut literals);
    out.push(END);
    out
}

/// A packed `.b@n` sprite bank holding `sprites`, each `(width, height, pixels)` with
/// `width * height` pixels stored raw (no row spans).
pub fn synthetic_sprite_bank(sprites: &[(u8, u8, &[u8])]) -> Vec<u8> {
    let mut bank = vec![0; 4];
    for &(width, height, pixels) in sprites {
        assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
        let size = u16::try_from(crate::ban::SpriteHeader::SIZE + pixels.len()).unwrap();
        bank.extend(size.to_le_bytes());
        bank.extend([width, height, 0, 8]);
        bank.extend_from_slice(pixels);
    }
    bank.extend([0, 0]);
    synthetic_packed(&bank)
}

/// A packed `.m@l` screen: `palette` (768 bytes of VGA 6-bit triples), then `pixels`
/// (one 320x200 picture) in PackBits, each row encoded on its own as the game's files
/// are. Runs of three or more equal bytes become repeats; the rest are literals.
pub fn synthetic_screen(palette: &[u8], pixels: &[u8]) -> Vec<u8> {
    use crate::mal::Screen;

    assert_eq!(palette.len(), crate::Palette::BYTES, "palette size");
    assert_eq!(pixels.len(), Screen::WIDTH * Screen::HEIGHT, "pixel count");
    let mut data = palette.to_vec();
    pack_rows(&mut data, pixels, Screen::WIDTH);
    synthetic_packed(&data)
}

/// A `.vcl` picture: `palette` (768 bytes of VGA 6-bit triples), then `pixels` (rows
/// 640 wide) in PackBits, row by row as [`synthetic_screen`] does. Not packed.
pub fn synthetic_vcl(palette: &[u8], pixels: &[u8]) -> Vec<u8> {
    use crate::vcl::Picture;

    assert_eq!(palette.len(), crate::Palette::BYTES, "palette size");
    assert_eq!(pixels.len() % Picture::WIDTH, 0, "whole rows");
    let mut data = palette.to_vec();
    pack_rows(&mut data, pixels, Picture::WIDTH);
    data
}

/// Appends `pixels` to `data` in PackBits, each `width`-pixel row on its own. Runs of
/// three or more equal bytes become repeats; the rest are literals.
fn pack_rows(data: &mut Vec<u8>, pixels: &[u8], width: usize) {
    for row in pixels.chunks(width) {
        let mut literals: Vec<u8> = Vec::new();
        let flush = |data: &mut Vec<u8>, literals: &mut Vec<u8>| {
            for part in literals.chunks(128) {
                data.push(part.len() as u8 - 1);
                data.extend_from_slice(part);
            }
            literals.clear();
        };
        for run in row.chunk_by(|a, b| a == b) {
            if run.len() < 3 {
                literals.extend_from_slice(run);
                continue;
            }
            flush(data, &mut literals);
            for part in run.chunks(128) {
                data.extend([(257 - part.len()) as u8, part[0]]);
            }
        }
        flush(data, &mut literals);
    }
}

/// `(width, height, pixels)`: an image for [`synthetic_model`].
pub type SyntheticImage<'a> = (u8, u8, &'a [u8]);

/// A `.mXX` model with one view per `(facing, images)`, each image `(width, height,
/// pixels)` stored as one full-width span per row (width at least 1). A view with a
/// facing has one animation of one frame per index that draws every image at (0, 0),
/// flipped for the mirror facing. A facing of -2 makes a view with only images.
pub fn synthetic_model(views: &[(i16, &[SyntheticImage])]) -> Vec<u8> {
    let mut model = Vec::new();
    for &(facing, images) in views {
        let word = |n: usize| u16::try_from(n).unwrap().to_le_bytes();
        let mut body = Vec::new();
        // An index at view offset `at` and its frame list; returns (index, list).
        let index = |body: &mut Vec<u8>, flip: u16| {
            let at = 16 + body.len();
            body.extend(word(at + 2));
            body.extend(word(at + 4));
            body.extend([u8::try_from(images.len()).unwrap(), 0]);
            for i in 0..images.len() {
                body.extend((u16::try_from(i).unwrap() | flip).to_le_bytes());
                body.extend([0, 0]);
            }
            (at, at + 4)
        };
        let (mirror, offsets) = if facing == -2 {
            (-2, [0; 4])
        } else {
            let mirror = (64 - facing) % 64;
            let (i, l) = index(&mut body, 0);
            let (mi, ml) = if mirror == facing {
                (i, l)
            } else {
                index(&mut body, 0x8000)
            };
            (mirror, [i, mi, l, ml])
        };
        let data = 16 + body.len();
        let mut next = data + 2 * images.len();
        let mut pixels = Vec::new();
        for &(width, height, image) in images {
            assert_eq!(image.len(), usize::from(width) * usize::from(height));
            body.extend(word(next));
            let start = pixels.len();
            pixels.extend([width, height]);
            for row in image.chunks(width.into()) {
                pixels.extend([0, width]);
                pixels.extend_from_slice(row);
            }
            next += pixels.len() - start;
        }
        body.extend(pixels);
        model.extend(word(16 + body.len()));
        model.extend(facing.to_le_bytes());
        model.extend(mirror.to_le_bytes());
        for o in offsets.into_iter().chain([data]) {
            model.extend(word(o));
        }
        model.extend(body);
    }
    model.extend([0, 0]);
    model
}

/// `((top, left, bottom, right), unlock_level, name)`: a record for
/// [`synthetic_scendef`].
pub type SyntheticScenario<'a> = ((u16, u16, u16, u16), u16, &'a [u8]);

/// A `scendef.bin` table with one record per entry, numbered as the game expects (id
/// twice the position), state 0, each name NUL-terminated, then the end marker.
pub fn synthetic_scendef(scenarios: &[SyntheticScenario]) -> Vec<u8> {
    let mut table = Vec::new();
    for (i, &((top, left, bottom, right), unlock_level, name)) in scenarios.iter().enumerate() {
        let id = u16::try_from(2 * i).unwrap();
        for word in [id, top, left, bottom, right, unlock_level, 0] {
            table.extend(word.to_le_bytes());
        }
        table.extend_from_slice(name);
        table.push(0);
    }
    table.extend([0xff, 0xff]);
    table
}

/// The chunks of a small made-up `.usg` scenario, in the stock order, for
/// [`synthetic_usg_from`]: a start clock of 07:29:37 tick 20, day 0, month 3; a round
/// hill and a block (kinds 0x14 and 0x12); a human humanoid at (95.5, 136.25) and an
/// alien relay (both 0x18 bytes, not the stock 0x172 and 0x34, to keep snapshots
/// short); one alien RECON path of two points and no human paths; a three-line
/// briefing; and the sky `SKY_A   .BIN`.
pub fn synthetic_usg_chunks() -> Vec<([u8; 4], Vec<u8>)> {
    let words = |ws: &[u16]| ws.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    let fixed =
        |whole: i16, fraction: u16| (i32::from(whole) << 16 | i32::from(fraction)).to_le_bytes();

    let mut header = words(&[0x80]);
    header.extend([7, 29, 37, 20, 0, 3]);
    header.extend(words(&[0xa0]));
    header.resize(16, 0);

    let mut terrain = words(&[24, 0x1014]);
    terrain.extend(fixed(120, 0));
    terrain.extend(fixed(-2, 0));
    terrain.extend(words(&[0x2100, 3, 0x0965, 0x0643, 0x0321, 0x14]));
    terrain.extend(words(&[24, 0x0812]));
    terrain.extend(synthetic_bytes(20, 7));
    terrain.extend(words(&[0]));
    terrain.extend([0xa0; 6]);

    let object = |slot: u16, refs: u16, kind: u16, x: [u8; 4], y: [u8; 4], flags: u8| {
        let mut o = words(&[0x18, slot, refs, kind, slot]);
        o.extend(x);
        o.extend(y);
        let mut data = synthetic_bytes(12, u64::from(slot));
        data[0x16 - 12] = flags;
        o.extend(data);
        o
    };
    let mut objects = words(&[2]);
    objects.extend(object(0, 0, 0, fixed(95, 0x8000), fixed(136, 0x4000), 0x66));
    objects.extend(object(1, 1, 4, fixed(138, 0), fixed(138, 0), 0x5a));

    let path_table = |paths: &[(u8, &[(i16, i16)])]| {
        let mut table = words(&[u16::try_from(paths.len()).unwrap()]);
        for (mission, points) in paths {
            table.extend([u8::try_from(points.len()).unwrap(), *mission]);
            table.extend([0, 0, 1, 0, 3, 7, 7, 3, 1, 3]);
            for &(x, y) in *points {
                table.extend(fixed(x, 0x8888));
                table.extend(fixed(y, 0x7171));
            }
            table.resize(2 + (table.len() - 2).next_multiple_of(140), 0);
        }
        table.resize(1122, 0);
        table
    };

    vec![
        (*b"SHDR", header),
        (*b"MAPP", terrain),
        (*b"DCBS", objects),
        (*b"APTH", path_table(&[(3, &[(122, 138), (95, 136)])])),
        (*b"HPTH", path_table(&[])),
        (
            *b"ORDR",
            b"ATTACK THE BASE.\r\n\r\nGOOD LUCK.\r\n\x1a\x1a\x00".to_vec(),
        ),
        (*b"BGND", {
            let mut name = b"SKY_A   .BIN".to_vec();
            name.resize(64, 0);
            name
        }),
        (*b"TERM", Vec::new()),
    ]
}

/// A `.usg` file made of `chunks`, each written as tag, u16 size, data.
pub fn synthetic_usg_from(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    for (tag, data) in chunks {
        file.extend(tag);
        file.extend(u16::try_from(data.len()).unwrap().to_le_bytes());
        file.extend(data);
    }
    file
}

/// The scenario of [`synthetic_usg_chunks`] as a file.
pub fn synthetic_usg() -> Vec<u8> {
    synthetic_usg_from(&synthetic_usg_chunks())
}

/// A `.ms3` song: a `MS3-TEST` signature, no doubled channels, `programs` for the
/// first channels (the rest 0), then `events` as given (they should end with `FF 2F`).
pub fn synthetic_ms3(programs: &[u8], events: &[u8]) -> Vec<u8> {
    let mut song = b"MS3-TEST \r\n\x1a\x1a\x1a\xff\x00".to_vec();
    song.resize(0x20, 0);
    song.extend(programs);
    song.resize(0x30, 0);
    song.extend(events);
    song
}

/// A `dsounds.bin`: for each `(step, samples)`, a block of whole paragraphs with the
/// header and samples, padded with 0xaa; then the terminating zero word.
pub fn synthetic_dsounds(sounds: &[(u16, &[u8])]) -> Vec<u8> {
    let mut file = Vec::new();
    for &(step, samples) in sounds {
        let end = 4 + samples.len();
        let paragraphs = end.div_ceil(16);
        file.extend(u16::try_from(paragraphs).unwrap().to_le_bytes());
        file.extend(u16::try_from(end).unwrap().to_le_bytes());
        file.extend(step.to_le_bytes());
        file.extend(samples);
        file.resize(file.len() + paragraphs * 16 - end, 0xaa);
    }
    file.extend([0, 0]);
    file
}

/// An LZEXE 0.91 executable whose load image is `image`, stored as literals only, with
/// entry point `cs:ip` and relocations at the image offsets `relocations` (ascending,
/// each 1 to 0xffff bytes after the last). Only the parts [`crate::lzexe`] reads are
/// filled in; it would not run.
pub fn synthetic_lzexe(image: &[u8], (cs, ip): (u16, u16), relocations: &[usize]) -> Vec<u8> {
    // Every flag is 1 (a literal); a 0, 1 pair and a `00 00 00` copy end the stream.
    // A flag word goes before the bytes after its first bit, and the next one is read
    // as soon as its sixteenth bit is taken.
    let mut stream = vec![0, 0];
    let mut flags_at = 0;
    let mut used = 0;
    let mut flag = |stream: &mut Vec<u8>, bit: u16| {
        let word = u16::from_le_bytes([stream[flags_at], stream[flags_at + 1]]) | bit << used;
        stream[flags_at..flags_at + 2].copy_from_slice(&word.to_le_bytes());
        used += 1;
        if used == 16 {
            flags_at = stream.len();
            stream.extend([0, 0]);
            used = 0;
        }
    };
    for &b in image {
        flag(&mut stream, 1);
        stream.push(b);
    }
    flag(&mut stream, 0);
    flag(&mut stream, 1);
    stream.extend([0, 0, 0]);
    let paragraphs = stream.len().div_ceil(16);
    stream.resize(paragraphs * 16, 0);

    let mut file = vec![0u8; 32];
    file[0..2].copy_from_slice(b"MZ");
    file[0x08..0x0a].copy_from_slice(&2u16.to_le_bytes());
    file[0x16..0x18].copy_from_slice(&u16::try_from(paragraphs).unwrap().to_le_bytes());
    file[0x1c..0x20].copy_from_slice(b"LZ91");
    file.extend(stream);
    let loader = file.len();
    for word in [ip, cs, 0, 0, u16::try_from(paragraphs).unwrap(), 0, 0] {
        file.extend(word.to_le_bytes());
    }
    file.resize(loader + 0x158, 0);
    let mut last = 0;
    for &at in relocations {
        match at - last {
            span @ 1..=0xff => file.push(span as u8),
            span => {
                file.push(0);
                file.extend(u16::try_from(span).unwrap().to_le_bytes());
            }
        }
        last = at;
    }
    file.extend([0, 1, 0]);
    file
}

/// Made-up numbers for a synthetic `vgaintro.dvr` ([`synthetic_vgaintro`]): a drop
/// short enough to play through in a test. Two parts of a few frames, blends of three
/// steps, and speeds that slow to exactly 0.
pub fn synthetic_numbers() -> crate::vgaintro::Numbers {
    crate::vgaintro::Numbers {
        anim_paragraph: 0x500,
        data_paragraph: 0x41a,
        view_y: 72,
        split_row: 128,
        view_x: 448,
        cave_view_x: 448,
        layer_start: 1152,
        start: 100,
        second_blend: 110,
        third_blend: 120,
        frames: 130,
        cave_end: 200,
        frame_retraces: 2,
        cave_frame_retraces: 2,
        blend_steps: 3,
        blend_record: 0,
        second_blend_steps: 3,
        second_blend_record: 1,
        third_blend_steps: 3,
        third_blend_record: 0,
        last_blend_steps: 3,
        last_blend_record: 1,
        first_copy_from: 10,
        first_copy_to: 0,
        first_copy_len: 3,
        second_copy_from: 26,
        second_copy_to: 0,
        second_copy_len: 3,
        light_weight: 0x80,
        light_record: 1,
        light_from: 26,
        light_to: crate::vgaintro::PALETTE_BUFFER + 3,
        light_len: 3,
        accelerate_until: 4,
        accelerate_a: 0x40,
        accelerate_b: 0x20,
        accelerate_c: 0x40,
        accelerate_view: 0x10,
        cruise_a: 0x100,
        cruise_b: 0x80,
        cruise_c: 0x100,
        cruise_view: 0x40,
        slow_from: 20,
        first_part_end: 10,
        jump_at: 14,
        jump_by: 5,
        boost_at: 16,
        boost_b: 0x100,
        stop_at: 24,
        slow_a: 0x40,
        slow_b: 0x40,
        slow_c: 0x40,
        slow_view: 0x10,
        hold_until: 30,
        hold_again_until: 34,
        credits_script: 0x18,
        copyright: 0x40,
        credits_record: 1,
        credits_show_from: 0x7c,
        credits_step: 4,
        credits_full: 0x80,
        credits_hide_from: 4,
        credits_down: 4,
    }
}

/// A synthetic `vgaintro.dvr`: an MZ file whose image holds `numbers` behind the
/// instructions [`crate::vgaintro::NUMBERS`] expects, the state table, a list of two
/// one-colour blend records (kind 3, colour 7: white, then grey), and a credits
/// script (clear, wait for frame 3, draw "HI", fade in, fade out, wait forever).
/// Everything else is zero.
pub fn synthetic_vgaintro(numbers: &crate::vgaintro::Numbers) -> Vec<u8> {
    use crate::vgaintro::{CREDIT_HANDLERS, FOREVER, NUMBERS, STATE_HANDLERS, STATE_TABLE};

    let anims = usize::from(numbers.anim_paragraph) * 16;
    let data = usize::from(numbers.data_paragraph) * 16;
    let mut image = vec![0u8; anims.max(data + 0x60) + 34];
    let put = |image: &mut Vec<u8>, at: usize, bytes: &[u8]| {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    };
    for ((_, at, op, width), value) in NUMBERS.iter().zip(numbers.values()) {
        put(&mut image, at - op.len(), op);
        put(&mut image, *at, &value.to_le_bytes()[..*width]);
    }
    for (i, handler) in STATE_HANDLERS.iter().enumerate() {
        put(&mut image, STATE_TABLE + 2 * i, &handler.to_le_bytes());
    }
    for (i, colours) in [[63u8, 63, 63], [32, 32, 32]].iter().enumerate() {
        let at = anims + 16 * i;
        put(&mut image, at, &[16, 0, 3, 1, 1, 7, 1, 0]);
        put(&mut image, at + 8, &((16 * i + 10) as u16).to_le_bytes());
        put(&mut image, at + 10, colours);
    }
    let script = data + usize::from(numbers.credits_script);
    let steps = [(0, 0), (1, 3), (2, 0x40), (3, 0), (4, 0), (1, FOREVER)];
    for (i, (handler, arg)) in steps.into_iter().enumerate() {
        put(
            &mut image,
            script + 4 * i,
            &CREDIT_HANDLERS[handler].0.to_le_bytes(),
        );
        put(&mut image, script + 4 * i + 2, &arg.to_le_bytes());
    }
    put(
        &mut image,
        data + 0x40,
        &[1, 0, 9, 0, 8, 0, 6, 0, 252, 0x50, 0],
    );
    put(&mut image, data + 0x50, b"HI\0");
    // The MZ header: two paragraphs, no relocations.
    let size = 0x20 + image.len();
    let mut file = b"MZ".to_vec();
    for word in [(size % 512) as u16, size.div_ceil(512) as u16, 0, 2] {
        file.extend(word.to_le_bytes());
    }
    file.resize(0x20, 0);
    file.extend(image);
    file
}

/// A synthetic `sounddvr.dvr`: an MZ file whose `DS` (paragraph 0x120) holds every
/// table behind the instructions [`crate::sounddvr::TABLES`] expects. Patch `p` is the
/// bytes `p..p + 16`; frequency `n` is `0x1000 | n`; program `p` has transpose `-p`,
/// velocity factor `p`, MIDI program `p + 1`, Tandy words `0x100 + p` and `0x200 + p`;
/// program 0's pitch envelope is `2, -2` looped and every other one is `0` then end.
/// The envelope scripts are the image's last 0x40 bytes.
pub fn synthetic_sounddvr() -> Vec<u8> {
    use crate::sounddvr::{DS_AT, PATCHES, PITCHES, PROGRAMS, TABLES};

    let ds = 0x1200;
    let mut image = vec![0u8; ds + 0xf00];
    let put = |image: &mut Vec<u8>, at: usize, bytes: &[u8]| {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    };
    put(&mut image, DS_AT.0, &[DS_AT.1, &[0x6b, 0x08]].concat());
    put(&mut image, 0x86b, &0x120u16.to_le_bytes());
    let offsets = [
        0x10, 0x810, 0xa10, 0xb14, 0xc18, 0xd1c, 0xd9e, 0xe20, 0xea0, 0xeaa, 0xeb3,
    ];
    for ((_, at, op), offset) in TABLES.iter().zip(offsets) {
        put(
            &mut image,
            *at,
            &[*op, &(offset as u16).to_le_bytes()].concat(),
        );
    }
    let program_words = |f: &dyn Fn(usize) -> u16| -> Vec<u8> {
        (0..PROGRAMS).flat_map(|p| f(p).to_le_bytes()).collect()
    };
    let patches: Vec<u8> = (0..PATCHES)
        .flat_map(|p| (0..16).map(move |i| (p + i) as u8))
        .collect();
    let frequencies: Vec<u8> = (0..PITCHES)
        .flat_map(|n| (0x1000 | n as u16).to_le_bytes())
        .collect();
    let envelopes = program_words(&|p| if p == 0 { 0xec0 } else { 0xec4 });
    let tables: [Vec<u8>; 11] = [
        patches,
        frequencies,
        envelopes,
        program_words(&|p| 0x100 + p as u16),
        program_words(&|p| 0x200 + p as u16),
        (0..PROGRAMS).map(|p| p as u8).collect(),
        (0..PROGRAMS).map(|p| (p as u8).wrapping_neg()).collect(),
        (0..PATCHES).map(|p| p as u8 + 1).collect(),
        vec![0, 1, 2, 3, 4, 5, 6, 7, 99, 8],
        vec![0, 1, 2, 8, 9, 10, 16, 17, 18],
        vec![3, 4, 5, 11, 12, 13, 19, 20, 21],
    ];
    for (offset, table) in offsets.iter().zip(&tables) {
        put(&mut image, ds + offset, table);
    }
    put(&mut image, ds + 0xec0, &[2, 0xfe, 0x81, 0, 0, 0x80]);
    // The MZ header: two paragraphs, no relocations.
    let size = 0x20 + image.len();
    let mut file = b"MZ".to_vec();
    for word in [(size % 512) as u16, size.div_ceil(512) as u16, 0, 2] {
        file.extend(word.to_le_bytes());
    }
    file.resize(0x20, 0);
    file.extend(image);
    file
}

/// An LZEXE-packed `config.dvr` holding `keys` as player 1's and 2's default tables
/// where [`crate::setup::default_keys`] looks: the menu walker's instruction pointing
/// at a made-up first menu entry that ends with the last remap label and the tables.
pub fn synthetic_config_dvr(keys: &[[u8; crate::setup::KEY_SLOTS]; 2]) -> Vec<u8> {
    use crate::setup::{MENU_HEAD_AT, MENU_HEAD_OP};

    let head = 0x100;
    let mut entry = vec![0, 0];
    entry.extend_from_slice(b"LABELS\0FIRE\0");
    for table in keys {
        entry.extend_from_slice(table);
        entry.extend([0; 4]);
    }
    let size = entry.len() as u16;
    entry[..2].copy_from_slice(&size.to_le_bytes());
    let mut image = vec![0u8; MENU_HEAD_AT + 2];
    image[MENU_HEAD_AT - MENU_HEAD_OP.len()..MENU_HEAD_AT].copy_from_slice(&MENU_HEAD_OP);
    image[MENU_HEAD_AT..].copy_from_slice(&(head as u16).to_le_bytes());
    image[head..head + entry.len()].copy_from_slice(&entry);
    synthetic_lzexe(&image, (0, 0), &[])
}

/// A glyph of a synthetic font: its code, first byte (width) and rows.
type SyntheticGlyph<'a> = (u8, u8, &'a [u8]);

/// One font laid out from `base` in a font block: the header (`kind`, `width`,
/// `height`, no steps), a table of `count` glyph offsets, then the glyphs. Codes below
/// 0x20 are control codes; the codes in `glyphs` get their (width, rows); every other
/// code shares one blank glyph of `blank_width`.
fn synthetic_font(
    base: usize,
    (kind, width, height): (u8, u8, u8),
    count: usize,
    blank_width: u8,
    glyphs: &[SyntheticGlyph],
) -> Vec<u8> {
    let table = base + 6;
    let mut data = Vec::new();
    let control = table + 2 * count;
    data.push(0);
    let blank = control + data.len();
    let rows = usize::from(height) - 1;
    let blank_size = if kind == 7 {
        rows
    } else {
        rows * usize::from(blank_width)
    };
    data.push(blank_width);
    data.extend(std::iter::repeat_n(0, blank_size));
    let mut offsets: Vec<usize> = (0..count)
        .map(|c| if c < 0x20 { control } else { blank })
        .collect();
    for &(code, glyph_width, rows) in glyphs {
        offsets[usize::from(code)] = control + data.len();
        data.push(glyph_width);
        data.extend_from_slice(rows);
    }
    let mut font = vec![kind, 0, width, height, 0, 0];
    for offset in offsets {
        font.extend((offset as u16).to_le_bytes());
    }
    font.extend(data);
    font
}

/// A synthetic font block (`docs/formats/font.md`): the four-entry table, then
///
/// 0. a bitmap font (kind 7), cells 4x3 (two rows), 128 codes, `A` a bar over two
///    outer pixels (`XXX.` / `X.X.`);
/// 1. a bitmap font, cells 6x3, 128 codes, all blank;
/// 2. a shaded font (kind 0), height 3, 128 codes, all one pixel wide and blank;
/// 3. a shaded font, height 3, 0x60 codes (so lower case has none), `B` two pixels wide
///    in all three shades (`3 1` / `2 0`), the rest one pixel wide and blank.
///
/// The block ends with the last glyph.
pub fn synthetic_font_block() -> Vec<u8> {
    let mut block = vec![0u8; 8];
    let fonts: [(_, usize, u8, &[SyntheticGlyph]); 4] = [
        ((7, 4, 3), 128, 4, &[(b'A', 4, &[0xe0, 0xa0])]),
        ((7, 6, 3), 128, 6, &[]),
        ((0, 6, 3), 128, 1, &[]),
        ((0, 5, 3), 0x60, 1, &[(b'B', 2, &[3, 1, 2, 0])]),
    ];
    for (i, (shape, count, blank, glyphs)) in fonts.into_iter().enumerate() {
        let base = block.len();
        block[2 * i..2 * i + 2].copy_from_slice(&(base as u16).to_le_bytes());
        block.extend(synthetic_font(base, shape, count, blank, glyphs));
    }
    block
}

/// A synthetic `mgavideo.dvr`: an MZ file whose init copies a colour-animation list
/// (one rotate record over colours 16..=17, then the end marker), two pointer shapes
/// and [`synthetic_font_block`] into data blocks, and sets up pointer shape 1 in
/// colour 0x3c, with the instructions [`crate::mgavideo`] expects.
pub fn synthetic_mgavideo() -> Vec<u8> {
    use crate::mgavideo::{ANIMS, FONTS, POINTERS};

    let anims_at = 0x100;
    let pointers_at = 0x180;
    let fonts_at = 0x200;
    let mut anims = vec![16, 0, 0x01, 1, 1, 16, 2, 0, 10, 0];
    anims.extend([63, 0, 0, 0, 63, 0]);
    anims.extend([0, 0]);
    // Two pointer shapes; the set-up picks shape 1: 3 by 2, hot spot (1, 0), image
    // then mask, the mask clear on the left two columns.
    let mut pointers = vec![4, 0, 4, 0, 3, 2, 1, 0, 12, 0, 18, 0];
    pointers.extend([0xff, 0x0f, 0xff, 0x00, 0xff, 0xff]);
    pointers.extend([0, 0, 1, 0, 0, 1]);
    let fonts = synthetic_font_block();
    let mut image = vec![0u8; 0x2970.max(fonts_at + fonts.len())];
    image[anims_at..anims_at + anims.len()].copy_from_slice(&anims);
    image[pointers_at..pointers_at + pointers.len()].copy_from_slice(&pointers);
    image[fonts_at..fonts_at + fonts.len()].copy_from_slice(&fonts);
    for (block, size, data) in [
        (ANIMS, anims.len(), anims_at),
        (POINTERS, pointers.len(), pointers_at),
        (FONTS, fonts.len(), fonts_at),
    ] {
        let at = block.at;
        let mut op = vec![0xb8];
        op.extend((size as u16).to_le_bytes());
        op.push(0xbb);
        op.extend(block.descriptor.to_le_bytes());
        op.extend([0x33, 0xf6, 0xba]);
        op.extend(((data / 16) as u16).to_le_bytes());
        op.extend([0xff, 0x1e, 0x36, 0x00]);
        image[at..at + op.len()].copy_from_slice(&op);
    }
    // The pointer set-up: shape 1 of 2, colour 0x3c.
    image[0x2949..0x2953].copy_from_slice(&[0xb0, 1, 0xe8, 0x18, 0, 0xb0, 0x3c, 0xe8, 0x65, 0]);
    image[0x2966..0x2968].copy_from_slice(&[0x3c, 2]);
    // The MZ header: two paragraphs, no relocations.
    let size = 0x20 + image.len();
    let mut file = b"MZ".to_vec();
    for word in [(size % 512) as u16, size.div_ceil(512) as u16, 0, 2] {
        file.extend(word.to_le_bytes());
    }
    file.resize(0x20, 0);
    file.extend(image);
    file
}

/// Where [`synthetic_map_room`] puts the widget set, its state template and the
/// strings.
pub const SYNTHETIC_MAP_ROOM_WIDGETS: usize = 0x18000;
pub const SYNTHETIC_MAP_ROOM_TEMPLATE: usize = 0x18100;
pub const SYNTHETIC_MAP_ROOM_STRINGS: usize = 0x19000;

/// Writes made-up map room tables (`crate::maproom`) into `image`, whose data
/// segment starts at `ds`, and returns the image offsets to relocate, ascending: a map
/// area and two buttons (ids 2 and 0, in that order), two keys, handles from 0x100,
/// made-up rectangles and words. `image` must reach `SYNTHETIC_MAP_ROOM_STRINGS + 16`
/// and `ds + 0xa0`, and the tables use `ds + 0x10` to `ds + 0xa0`.
pub fn synthetic_map_room(image: &mut [u8], ds: usize) -> Vec<usize> {
    use crate::maproom::*;
    let (widgets, template, strings) = (
        SYNTHETIC_MAP_ROOM_WIDGETS,
        SYNTHETIC_MAP_ROOM_TEMPLATE,
        SYNTHETIC_MAP_ROOM_STRINGS,
    );
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let op = |(at, op): (usize, &[u8]), value: u16| {
        let mut bytes = op.to_vec();
        bytes.extend(value.to_le_bytes());
        (at, bytes)
    };
    let mut ops = vec![
        op(WIDGETS_SEGMENT, (widgets / 16) as u16),
        op(WIDGETS_FIRST, 0x10),
        op(WIDGETS_END, 0x10 + 4 * 12),
        op(BANK_BASE, 0x100),
        op(BUTTON_SPRITES, 0x10),
        op(BUTTON_HANDLERS, 0x20),
        op(KEYS, 0x30),
        op(KEY_COUNT, 2),
        op(LOOKS, 0x40),
        op(UNAVAILABLE, 0x138),
        op(HIGHLIGHT, 0x150),
        op(ALERT_LABEL, 0x80),
        op(ALERT_LEVELS, 0x90),
        op(STRINGS_SEGMENT, (strings / 16) as u16),
    ];
    for (at, sprite, rect) in [
        (NAME_PANEL, 0x158, 0x60),
        (SCORE_PANEL, 0x160, 0x68),
        (INFO_PANEL, 0x168, 0x70),
    ] {
        ops.push(op((at, &[0xb8]), sprite));
        ops.push(op((at + 3, &[0xbb]), rect));
    }
    for (at, bytes) in ops {
        put(at, &bytes);
    }
    put(widgets, &words(&[0x40, 0, (template / 16) as u16, 0, 9]));
    #[rustfmt::skip]
    put(widgets + 0x10, &words(&[
        0x36, 0, 0, 0, 200, 320,
        MAP_AREA_TYPE, 1, 10, 5, 190, 280,
        BUTTON_TYPE, 3, 150, 270, 160, 310,
        BUTTON_TYPE, 6, 160, 270, 170, 310,
    ]));
    put(template, &[0, 0, 0, 0, 2, 0, 0, 0, 0]);
    put(ds + 0x10, &words(&[0x100, 0x110]));
    put(ds + 0x20, &words(&[0x7000, 0x7100]));
    put(ds + 0x30, &words(&[0x19, 0x7000, 0x2010, 0x7100]));
    put(
        ds + 0x40,
        &words(&[0x120, 0x128, 0x130, 0x130, 0x138, 0x140, 0x148]),
    );
    put(ds + 0x60, &words(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
    put(ds + 0x80, b"LEVEL\0");
    put(ds + 0x90, &words(&[0, 2, 4, 6, 8, 8, 8]));
    put(strings, b"A\0B\0C\0D\0E\0");
    vec![WIDGETS_SEGMENT.0 + 1, STRINGS_SEGMENT.0 + 1, widgets + 4]
}

/// Writes made-up campaign rules (`crate::campaign`) into `image`, beside
/// [`synthetic_map_room`]'s tables (whose string segment and alert label it uses):
/// unlock values 0x1234 and 0x5678, a bonus of 100, an open limit of 2, a progress cap
/// of 9, four chances ending at the label, alert damage 0, 5, 10, 15 then zeros, hover
/// words `B`..`H`, a box and a two-line message, and two prompts. It uses `ds + 0x78`
/// to `ds + 0x80`, `ds + 0xc0` to `ds + 0xce` and `ds + 0xe0` to `ds + 0xf8`, and the
/// string segment from offset 0xa0 to 0xc0.
pub fn synthetic_campaign(image: &mut [u8], ds: usize) {
    use crate::campaign::*;
    let strings = SYNTHETIC_MAP_ROOM_STRINGS;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    for ((at, op), value) in [
        (DEVELOPER_UNLOCK, 0x1234),
        (EDITOR_UNLOCK, 0x5678),
        (WIN_BONUS, 100),
        (OPEN_LIMIT, 2),
        (MAX_PROGRESS, 9),
        (CHANCES, 0x78),
        (ALERT_DAMAGE, 0xc0),
        (HOVER_WORDS, 0xe0),
        (WON_BOX, 0xf0),
        (WON_MESSAGE, 0xb0),
        (PROMPT, 0xb8),
        (PROMPT_WON, 0xbc),
    ] {
        put(at, op);
        put(at + op.len(), &u16::to_le_bytes(value));
    }
    put(ds + 0x78, &words(&[0x8000, 0x3000, 0x1000, 0]));
    put(ds + 0xc0, &words(&[0, 5, 10, 15, 0, 0, 0]));
    put(
        ds + 0xe0,
        &words(&[0xa0, 0xa2, 0xa4, 0xa6, 0xa8, 0xaa, 0xac]),
    );
    put(ds + 0xf0, &words(&[60, 90, 110, 230]));
    put(strings + 0xa0, b"B\0C\0D\0E\0F\0G\0H\0\0");
    put(strings + 0xb0, b"W\x0cW\0\0\0\0\0P\0\0\0Q\0");
}

/// Where [`synthetic_review`] puts the widget set and the texts (in the map room's
/// string segment, at offset 0x40).
pub const SYNTHETIC_REVIEW_WIDGETS: usize = 0x18400;
pub const SYNTHETIC_REVIEW_STRINGS: usize = SYNTHETIC_MAP_ROOM_STRINGS + 0x40;
/// The script of [`synthetic_review`]'s pages: grow by 0x80 for 2 ticks, walk, turn by
/// 0x400 for 3 ticks, wait 2, stand, and back to the walk.
pub const SYNTHETIC_REVIEW_SCRIPT: [u16; 12] = [6, 2, 0x80, 10, 0, 3, 0x400, 2, 2, 12, 4, 6];

/// Writes made-up review tables (`crate::review`) into `image`, whose data segment
/// starts at `ds`, and returns the image offsets to relocate, ascending: a picture, a
/// text widget, the view and both buttons; three keys (ESC leave, Tab next, Alt+Tab
/// previous); font 2 in colour 0x33; three panels; two pages (models 0 and 2, heights 5
/// and -3) whose texts are `A`..`E` and `F`, new line, `G`. Both run
/// [`SYNTHETIC_REVIEW_SCRIPT`] and the same parts routines: standing every fourth tick,
/// frames 1 and 2 step up together to 3, frame 3 down; walking 16 a tick, frames 1 and 4
/// from the table 0, 1, 2, 0, 1, 2, ... `image` must reach
/// `SYNTHETIC_REVIEW_STRINGS + 16` and `ds + 0x3a0`, and the tables use `ds + 0xa0` to
/// `ds + 0xe0` and `ds + 0x380` to `ds + 0x398`; it leaves the other synthetic tables
/// alone, sharing the string segment [`synthetic_map_room`] points at.
pub fn synthetic_review(image: &mut [u8], ds: usize) -> Vec<usize> {
    use crate::maproom::STRINGS_SEGMENT;
    use crate::review::*;
    let (widgets, strings) = (SYNTHETIC_REVIEW_WIDGETS, SYNTHETIC_MAP_ROOM_STRINGS);
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    const KERNEL: usize = 0xfe60;
    for ((at, code), value) in [
        (WIDGETS_SEGMENT, (widgets / 16) as u16),
        (WIDGETS_FIRST, 0x10),
        (WIDGETS_END, 0x10 + 5 * 12),
        (KEYS, 0xa0),
        (KEY_COUNT, 3),
        (FONT, 2),
        (COLOUR, 0x33),
        (PANELS[0], 0xc0),
        (PANELS[1], 0xc8),
        (PANELS[2], 0xd0),
        (PAGE_TABLE, 0x7b30),
        (STRINGS_SEGMENT, (strings / 16) as u16),
    ] {
        put(at, &op(code, value));
    }
    put(LAST_PAGE.0, &[LAST_PAGE.1, &[2]].concat());
    #[rustfmt::skip]
    put(widgets + 0x10, &words(&[
        PICTURE_TYPE, 0, 0, 0, 200, 320,
        TEXT_TYPE, 1, 0, 0, 200, 320,
        VIEW_TYPE, 2, 14, 160, 110, 310,
        NEXT_BUTTON_TYPE, 3, 180, 10, 190, 50,
        EXIT_BUTTON_TYPE, 4, 180, 55, 195, 100,
    ]));
    put(
        ds + 0xa0,
        &words(&[0x01, LEAVE, 0x0f, NEXT_PAGE, 0x200f, PREVIOUS_PAGE]),
    );
    put(
        ds + 0xc0,
        &words(&[20, 10, 80, 150, 110, 10, 180, 150, 130, 170, 180, 300]),
    );
    put(KERNEL + 0x7b30, &words(&[0x7b40, 0x7b80]));
    let segment = (KERNEL / 16) as u16;
    let mut relocated = vec![WIDGETS_SEGMENT.0 + 1, STRINGS_SEGMENT.0 + 1];
    let texts = [(0, 5, [0x40, 0x42, 0x44]), (2, -3i16, [0x46, 0x48, 0x4a])];
    for (page, (model, height, texts)) in texts.into_iter().enumerate() {
        let code = KERNEL + 0x7b40 + 0x40 * page;
        put(code + PAGE_MODEL.0, &[PAGE_MODEL.1, &[model]].concat());
        for ((at, code_op), text) in PAGE_TEXTS.into_iter().zip(texts) {
            put(code + at, &op(code_op, text));
        }
        for ((at, code_op), value) in [
            (PAGE_HEIGHT, height as u16),
            (PAGE_STANDING, 0x7c00),
            (PAGE_STANDING_SEGMENT, segment),
            (PAGE_SCRIPT, 0x380),
        ] {
            put(code + at, &op(code_op, value));
        }
        relocated.push(code + PAGE_STANDING_SEGMENT.0 + 4);
        put(KERNEL + 0x7ca0 + 4 * page, &words(&[0x7c40, segment]));
        relocated.push(KERNEL + 0x7ca0 + 4 * page + 2);
    }
    put(WALKING.0, &op(WALKING.1, 0x7ca0));
    #[rustfmt::skip]
    put(KERNEL + 0x7c00, &[
        0xf7, 0x06, 0xd2, 0x61, 0x03, 0x00, 0x75, 0x2b,
        0x80, 0x3e, 0xbd, 0x61, 0x00, 0x74, 0x19, 0xfe, 0x06, 0xbd, 0x61, 0xfe, 0x06, 0xbe,
        0x61, 0x80, 0x3e, 0xbd, 0x61, 0x03, 0x72, 0x0a, 0xc6, 0x06, 0xbd, 0x61, 0x00, 0xc6,
        0x06, 0xbe, 0x61, 0x00,
        0x80, 0x3e, 0xbf, 0x61, 0x00, 0x74, 0x04, 0xfe, 0x0e, 0xbf, 0x61,
        0xcb,
    ]);
    #[rustfmt::skip]
    put(KERNEL + 0x7c40, &[
        0x83, 0x06, 0xe4, 0x61, 0x10, 0x8b, 0x1e, 0xe4, 0x61, 0xd0, 0xeb, 0xd0, 0xeb, 0xd0,
        0xeb, 0xd0, 0xeb, 0x83, 0xe3, 0x0f, 0x2e, 0x8a, 0x87, 0x80, 0x7c, 0xa2, 0xbd, 0x61,
        0xa2, 0xc0, 0x61, 0xcb,
    ]);
    put(KERNEL + 0x7c80, &[0, 1, 2].repeat(6)[..16]);
    put(ds + 0x380, &words(&SYNTHETIC_REVIEW_SCRIPT));
    put(strings + 0x40, b"A\0B\0C\0D\0E\0F\x0cG\0");
    relocated.sort_unstable();
    relocated
}

/// Writes a made-up program table (`crate::orders`) into `image`, whose data segment
/// starts at `ds`: five programs at `ds + 0x9a40` on (0 never offered, 1 for every kind,
/// 2 for every kind with a header of no item list, 3 for the scorpion only, 4 never
/// offered), the table at `ds + 0x9a00` and the kind masks 1, 2, 4 at `ds + 0x9a20`.
/// `image` must reach `0xd384` and `ds + 0x9b40`; it leaves the other synthetic tables
/// alone.
pub fn synthetic_programs(image: &mut [u8], ds: usize) {
    use crate::orders::{COUNT, MASKS, TABLE};
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(TABLE.0, &op(TABLE.1, 0x9a00));
    put(COUNT.0, &[COUNT.1[0], 5]);
    put(MASKS.0, &op(MASKS.1, 0x9a20));
    put(ds + 0x9a20, &[1, 2, 4]);
    let programs: [&[u8]; 5] = [
        b"\x0f\x11\x02\x87\x05ZERO\0\x0a",
        b"\x0f\x05\x76\x9a\x09\x76\x9a\x11\x02\x07\x04ONE\0\x14\x5d\x32\x00\x43\xfb\x0a\x06",
        b"\x02\x07\x04TWO\0\x0a",
        b"\x0f\x09\x76\x9a\x11\x02\x02\x06THREE\0\x0a",
        b"\x0f\x11\x02\x82\x05FOUR\0\x0a",
    ];
    for (n, program) in programs.iter().enumerate() {
        let at = 0x9a40 + 0x20 * n;
        put(ds + 0x9a00 + 2 * n, &(at as u16).to_le_bytes());
        put(ds + at, program);
    }
}

/// Writes made-up scripts into `image` (`xeno_formats::script`): the programs of
/// [`synthetic_programs`] at `DS:0x9a40..0x9b00` (program 1 waits a second, tests its
/// distance, jumps back while far and halts; its header sends events 5 and 9, and
/// program 3's event 9, to a return at `0x9a76`; the others halt after their names),
/// 136 opcodes at `DS:0x97e0` (each one's time its number, 60 for 0x14), and both
/// event tables (`DS:0x9b80`, `0x9bc0`) sending events 0 to 12 to `0x9a76`.
pub fn synthetic_scripts(image: &mut [u8], ds: usize) {
    use crate::script::{END, HANDLERS, PILOT_EVENTS, PROGRAM_EVENTS, START, TIMES};
    synthetic_programs(image, ds);
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(HANDLERS.0, &op(HANDLERS.1, 0x97e0));
    put(TIMES.0, &op(TIMES.1, 0x97e2));
    for n in 0..0x88u16 {
        let time = if n == 0x14 { 60 } else { n };
        put(ds + 0x97e2 + 4 * usize::from(n), &time.to_le_bytes());
    }
    put(START.0, &op(START.1, 0x9a40));
    put(END.0, &op(END.1, 0x9b00));
    put(PILOT_EVENTS.0, &op(PILOT_EVENTS.1, 0x9b80));
    put(PROGRAM_EVENTS.0, &op(PROGRAM_EVENTS.1, 0x9bc0));
    let table = [[0x76, 0x9a].repeat(13), vec![0xff, 0xff]].concat();
    put(ds + 0x9b80, &table);
    put(ds + 0x9bc0, &table);
}

/// A `.usg` scenario whose objects have the stock sizes (0x172 bytes, 0x34 for the
/// relay): a human base (slot 0), a human humanoid on program 1, the program driving (slot 2), a
/// human relay (slot 3), a human scorpion on program 2 (slot 5) and an alien scout on
/// program 2 (slot 7), at x 100 + slot, y 130, heading slot x 0x1000. The robots are
/// units with helm 90 and power 80 and no script events; the base has taken 20 damage and starts in view
/// mode 0; the relay has taken 3. The other bytes are synthetic, and the other chunks
/// are [`synthetic_usg_chunks`]'s.
pub fn synthetic_mission_usg() -> Vec<u8> {
    let words = |ws: &[u16]| ws.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    let object = |slot: u16, kind: u16, side: u8, mode: u8, program: u8| {
        let size: u16 = if kind == 4 { 0x34 } else { 0x172 };
        let mut o = words(&[size, slot, 0, kind, slot]);
        o.extend((i32::from(100 + slot) << 16).to_le_bytes());
        o.extend((130i32 << 16).to_le_bytes());
        // From +0x0c; `at(n)` is in-memory offset n.
        let mut data = synthetic_bytes(usize::from(size) - 12, u64::from(slot));
        let at = |offset: usize| offset - 12;
        data[at(0x10)..at(0x12)].copy_from_slice(&(slot << 12).to_le_bytes());
        data[at(0x16)] = side | if kind <= 2 { 0x20 } else { 0 };
        match kind {
            0..=2 => {
                data[at(0x23)] = mode;
                data[at(0x24)] = 90;
                data[at(0x2e)] = program;
                data[at(0x30)] = program;
                // No events waiting for the script.
                data[at(0x36)..at(0x38)].fill(0);
                data[at(0x4e)] = 80;
            }
            3 => {
                data[at(0x2e)..at(0x30)].copy_from_slice(&20u16.to_le_bytes());
                data[at(0x37)] = 0;
            }
            _ => data[at(0x33)] = 3,
        }
        o.extend(data);
        o
    };
    let mut objects = words(&[5]);
    objects.extend(object(0, 3, 0, 0, 0));
    objects.extend(object(2, 0, 0, 0x80, 1));
    objects.extend(object(3, 4, 0, 0, 0));
    objects.extend(object(5, 1, 0, 0x01, 2));
    objects.extend(object(7, 2, 8, 0x80, 2));
    let mut chunks = synthetic_usg_chunks();
    for (tag, data) in &mut chunks {
        if tag == b"DCBS" {
            *data = objects.clone();
        }
    }
    synthetic_usg_from(&chunks)
}

/// Image offsets [`synthetic_views`] uses.
pub struct SyntheticViews;

impl SyntheticViews {
    /// Where each view's state block starts, plus the object kind.
    pub const STATE: u16 = 0x100;
    /// The one key-table handler every view's key widget uses.
    pub const KEY_HANDLER: usize = 0x1200;
    /// The training menu's "back" action.
    pub const BACK_ACTION: usize = 0x1430;
}

/// Writes made-up mission views and a training menu (`crate::views`) into `image`,
/// whose data segment starts at `ds`, and returns the image offsets to relocate. Each
/// console has a program panel, a unit-name button, a corner and a key widget; the
/// situation room a sprite button (handle 0x2a0), a roster of two rows, a corner and a
/// key widget; every view's keys are F2, ESC, Space, Tab and Alt+Q, bound to the
/// original's pilot, base, view-selected, next-unit and end handlers. The training menu has two buttons
/// (action 0, a mission `ATRAIN.USG`, and action 2, back) and two keys (ESC back, A the
/// mission). `image` must reach `ds + 0x700` and `0xdc00`; it uses the code offsets
/// 0x1000 to 0x1440 and the image from 0x1a000 to 0x1a910, and leaves the other
/// synthetic tables alone.
pub fn synthetic_views(image: &mut [u8], ds: usize) -> Vec<usize> {
    use crate::views::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    const NONE: [u16; 4] = [0x7fff; 4];
    const KEY_TYPE: u16 = 0x144;
    let widget =
        |kind: u16, state: u16, area: [u16; 4]| words(&[&[kind, state][..], &area].concat());

    // The handler tables: everything draws and keys nothing, but for a sprite button
    // (type 0x54) and the key widgets.
    let (draw, keys) = (0x200, 0x400);
    for t in (0..0x170).step_by(2) {
        put(ds + draw + t, &words(&[0x1300]));
        put(ds + keys + t, &words(&[0x1300]));
    }
    put(ds + draw + 0x54, &words(&[0x1100]));
    put(ds + keys + usize::from(KEY_TYPE), &words(&[0x1200]));
    put(0x1300, &[0xc3]);
    put(
        0x1100,
        &[
            0x8b, 0x3e, 0xf1, 0x62, 0x8a, 0x45, 0x40, 0x98, 0x05, 0xa0, 0x02,
        ],
    );
    put(
        SyntheticViews::KEY_HANDLER,
        &[
            &[0xa9, 0x00, 0x18, 0x74, 0x14, 0x8b, 0x44, 0x02, 0x24, 0x7f][..],
            &op(&[0xbb], 0x600),
            &op(&[0xb9], 5),
        ]
        .concat(),
    );
    put(
        ds + 0x600,
        &words(&[
            0x3c, 0xd446, 0x01, 0x7ee7, 0x39, 0x6852, 0x0f, 0x7db2, 0x2010, 0x7cf2,
        ]),
    );
    put(DRAW_HANDLERS.0, &op(DRAW_HANDLERS.1, draw as u16));
    put(KEY_HANDLERS.0, &op(KEY_HANDLERS.1, keys as u16));

    // The four views, each opened by its own code and with its own widget set.
    let mut relocated = Vec::new();
    put(VIEW_OPENERS.0, &op(VIEW_OPENERS.1, 0x100));
    for kind in 0..4u16 {
        let opener = 0x1000 + 0x20 * usize::from(kind);
        let set = 0x1a000 + 0x200 * usize::from(kind);
        let list: Vec<Vec<u8>> = if kind < 3 {
            vec![
                widget(0x0e, 0, NONE),
                widget(PROGRAM_PANEL_TYPE, 3, [91, 126, 98, 194]),
                widget(UNIT_BUTTON_TYPE, 4, [185, 273, 200, 318]),
                widget(CONSOLE_CORNER_TYPE, 5, [0, 0, 2, 2]),
                widget(KEY_TYPE, 6, NONE),
                widget(MESSAGE_PANEL_TYPE, 7, [7, 7, 36, 102]),
            ]
        } else {
            vec![
                widget(0x0e, 0, NONE),
                widget(0x54, 3, [73, 13, 81, 20]),
                widget(ROSTER_TYPE, 4, [14, 2, 70, 78]),
                widget(SITUATION_CORNER_TYPE, 5, [0, 0, 2, 2]),
                widget(KEY_TYPE, 6, NONE),
            ]
        };
        let end = 0x10 + 12 * list.len() as u16;
        put(set, &words(&[0x100, (set / 16) as u16]));
        put(set + 0x10, &list.concat());
        // The situation room's group members: the zoom, a toggle and two mode buttons.
        if kind == 3 {
            let nested = [
                widget(0x64, 7, [91, 288, 100, 303]),
                widget(0x66, 8, [99, 288, 108, 319]),
                widget(0x70, 9, [142, 254, 150, 306]),
                widget(0x70, 12, [150, 254, 158, 306]),
            ];
            put(set + usize::from(end), &nested.concat());
        }
        put(ds + 0x100 + 2 * usize::from(kind), &words(&[opener as u16]));
        put(
            opener,
            &[
                op(&[0xb8], (set / 16) as u16),
                op(&[0xbb], 0x10),
                op(&[0x81, 0xc1], SyntheticViews::STATE + kind),
                op(&[0xba], end),
            ]
            .concat(),
        );
        relocated.push(opener + 1);
    }
    put(SITUATION_BANK.0, &op(SITUATION_BANK.1, 0x2a0));
    put(ROSTER_ROWS.0, &op(ROSTER_ROWS.1, 0x620));
    put(ROSTER_COUNT.0, &[ROSTER_COUNT.1[0], ROSTER_COUNT.1[1], 4]);
    put(ds + 0x620, &words(&[0x630, 0x638]));
    put(ds + 0x630, &words(&[14, 2, 25, 78, 25, 2, 36, 78]));
    for (entry, at, values) in [
        (PANEL_FONTS, 0x640, [0, 2, 2]),
        (PANEL_X_OFFSETS, 0x648, [2, 1, 1]),
        (PANEL_COLOURS, 0x650, [0x2010, 0x2111, 0x2212]),
        (PANEL_PRESSED, 0x658, [0x1030, 0x1131, 0x1232]),
    ] {
        put(entry.0, &op(entry.1, at as u16));
        put(ds + at, &words(&values));
    }
    put(PANEL_PILOT.0, &[PANEL_PILOT.1, 6]);
    put(UNIT_LABEL.0, &[op(&[0xb8], 191), op(&[0xbb], 279)].concat());
    put(MESSAGE_FONT.0, &op(MESSAGE_FONT.1, 4));
    put(MESSAGE_COLOURS.0, &op(MESSAGE_COLOURS.1, 0x660));
    put(ds + 0x660, &words(&[0x3020, 0x3121, 0x3222]));
    put(MESSAGE_HIGH.0, &[MESSAGE_HIGH.1, 0x3f]);
    put(DISPLAY_VIEW_MODES.0, &op(DISPLAY_VIEW_MODES.1, 0x670));
    put(
        ds + 0x670,
        &[0x0e, 0x04, 0x10, 0x04, 0x1c, 0x1c, 0x1c, 0x1c],
    );
    put(DISPLAY_DEVICES.0, &op(DISPLAY_DEVICES.1, 0x678));
    put(ds + 0x678, &[0x08, 0x10, 0x20, 0]);
    put(MAIN_VIEW_SHAPES.0, &op(MAIN_VIEW_SHAPES.1, 0x6e0));
    put(ds + 0x6e0, &words(&[0, 0, 3, 3, 0]));
    for (at, shape) in CONSOLE_SHAPES.into_iter().zip([3, 4, 6, 4]) {
        put(at, &op(&[0xb8], shape));
    }
    for (kind, at) in DISPLAY_MAP_COLOURS.into_iter().enumerate() {
        let k = kind as u8;
        put(
            at,
            &[
                0xc6,
                0x06,
                0x57,
                0x14,
                0x40 + k,
                0xc6,
                0x06,
                0x58,
                0x14,
                0x48 + k,
            ],
        );
    }

    // The training menu.
    let (set, template) = (0x1a800, 0x1a900);
    put(
        set,
        &words(&[0x40, (set / 16) as u16, (template / 16) as u16, 0, 10]),
    );
    put(
        set + 0x10,
        &[
            widget(0x30, 3, [0, 0, 200, 320]),
            widget(TRAINING_BUTTON_TYPE, 4, [14, 200, 25, 275]),
            widget(TRAINING_BUTTON_TYPE, 7, [24, 200, 35, 275]),
            widget(0x34, 9, NONE),
        ]
        .concat(),
    );
    put(template, &[0, 0, 0, 0, 0, 0, 0, 0, 2, 0]);
    put(
        TRAINING_WIDGETS.0,
        &op(TRAINING_WIDGETS.1, (set / 16) as u16),
    );
    put(TRAINING_FIRST.0, &op(TRAINING_FIRST.1, 0x10));
    put(TRAINING_END.0, &op(TRAINING_END.1, 0x10 + 4 * 12));
    put(TRAINING_KEYS.0, &op(TRAINING_KEYS.1, 0x680));
    put(TRAINING_KEY_COUNT.0, &op(TRAINING_KEY_COUNT.1, 2));
    put(TRAINING_ACTION.0, &op(TRAINING_ACTION.1, 0x5e50));
    put(TRAINING_ACTIONS.0, &op(TRAINING_ACTIONS.1, 0x6a0));
    put(ds + 0x680, &words(&[0x01, 0x1400, 0x1e, 0x1410]));
    put(
        0x1400,
        &[op(&[0xc7, 0x06], 0x5e50), words(&[2]), vec![0xc3]].concat(),
    );
    put(
        0x1410,
        &[op(&[0xc7, 0x06], 0x5e50), words(&[0]), vec![0xc3]].concat(),
    );
    put(
        ds + 0x6a0,
        &words(&[0x1420, SyntheticViews::BACK_ACTION as u16]),
    );
    put(0x1420, &op(&[0xbe], 0x6c0));
    put(SyntheticViews::BACK_ACTION, &[0xc3]);
    put(ds + 0x6c0, b"ATRAIN.USG\0");
    relocated.extend([TRAINING_WIDGETS.0 + 1, set + 4]);
    relocated
}

/// Where [`synthetic_endings`] puts the start-up fix-up table, the entries (from
/// `ds + SYNTHETIC_ENDINGS_DATA`) and the captions (in the map room's string segment).
pub const SYNTHETIC_FIXUPS: usize = 0x18800;
pub const SYNTHETIC_ENDINGS_DATA: usize = 0xe800;
pub const SYNTHETIC_CAPTIONS: u16 = 0x100;

/// Writes made-up endings (`crate::ending`) and a start-up fix-up table pointing
/// `DS:0x60` at [`SYNTHETIC_MAP_ROOM_STRINGS`] (`crate::sequence::string_segment`)
/// into `image`, whose data segment starts at `ds`, and returns the image offsets to
/// relocate, ascending. Win at `DS:+0x00`, lose entries at `+0x20` and `+0x3a` (the
/// first used for selectors 0 and 4), triumph at `+0x80`, the lose table at `+0xc0`,
/// weights 7 and 3 at `+0x100`, the points at `+0x104`, the number buffer at `+0x110`
/// (offsets from `ds + SYNTHETIC_ENDINGS_DATA`). Each entry holds 1 tick, with its
/// caption at (10, 20), end time 100, fades of 4 and a caption speed of 2. The
/// captions: win `K=00`, new line, `L=00`, new line, `P=` and six spaces; `LOST`;
/// `GONE`; 9, 5, `ALL`. `image` must reach `SYNTHETIC_FIXUPS + 16`,
/// `SYNTHETIC_MAP_ROOM_STRINGS + 0x200` and `ds + 0xea00`.
pub fn synthetic_endings(image: &mut [u8], ds: usize) -> Vec<usize> {
    use crate::ending::*;
    use crate::sequence::{FIXUPS_SEGMENT, FIXUPS_TABLE};
    let data = ds + SYNTHETIC_ENDINGS_DATA;
    let at = |offset: u16| (SYNTHETIC_ENDINGS_DATA + usize::from(offset)) as u16;
    let strings = SYNTHETIC_MAP_ROOM_STRINGS;
    let caption = |offset: u16| SYNTHETIC_CAPTIONS + offset;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    let entry = |file_id: u16, text: u16| {
        words(&[file_id, 0, 0, 0, 1, caption(text), 10, 20, 100, 4, 4, 2, 0])
    };
    put(data, &entry(0x16, 0));
    put(data + 0x20, &entry(0x1c, 0x30));
    put(data + 0x3a, &entry(0x1a, 0x38));
    put(data + 0x80, &entry(0x1e, 0x40));
    put(data + 0xc0, &words(&[at(0x20), at(0x3a), at(0x20)]));
    put(data + 0x100, &words(&[7, 3]));
    put(
        strings + usize::from(caption(0)),
        b"K=00\x0cL=00\x0cP=      \0",
    );
    put(strings + usize::from(caption(0x30)), b"LOST\0");
    put(strings + usize::from(caption(0x38)), b"GONE\0");
    put(strings + usize::from(caption(0x40)), b"\x09\x05ALL\0");
    let buffer = at(0x110);
    for ((code, bytes), value) in [
        (WIN_ENTRY, at(0)),
        (TRIUMPH_ENTRY, at(0x80)),
        (LOSE_SELECTOR, 0x6012),
        (LOSE_TABLE, at(0xc0)),
        (PER_KILL, at(0x100)),
        (PER_LOSS, at(0x102)),
        (POINTS, at(0x104)),
        (WIN_POINTS, at(0x104)),
        (BUFFERS[0], buffer),
        (BUFFERS[1], buffer),
        (BUFFERS[2], buffer),
    ] {
        put(code, &op(bytes, value));
    }
    for es in STRINGS_LOADS {
        put(es, LOAD_STRINGS);
    }
    for ((_, load, store), (from, to)) in
        COPIES
            .into_iter()
            .zip([(3, 2), (3, 7), (0, 0xc), (2, 0xe), (4, 0x10)])
    {
        put(load, &op(&[0xa1], buffer + from));
        put(store, &op(&[0x26, 0xa3], caption(to)));
    }
    // The fix-ups: `DS:0x5c` to some segment, then `DS:0x60` to the strings'.
    let table = SYNTHETIC_FIXUPS;
    put(FIXUPS_SEGMENT.0, &op(FIXUPS_SEGMENT.1, (table / 16) as u16));
    put(FIXUPS_TABLE.0, &op(FIXUPS_TABLE.1, 0));
    put(
        table,
        &words(&[0, 0x5c, 0x1000, 0x60, (strings / 16) as u16, 0]),
    );
    vec![
        FIXUPS_SEGMENT.0 + FIXUPS_SEGMENT.1.len(),
        table,
        table + 4,
        table + 8,
    ]
}

/// Writes made-up damage tables (`crate::damage`) into a load image whose data segment
/// starts at `ds`: every kind destroyed over 0x50; chance tables at `DS:0x9b00` (the
/// humanoid's: 0x80 wear the helm by 7, then 0xff the weapon bit 0 or else systems
/// bit 0x40), `0x9b10` (the scorpion's: 0x40 nothing, 0xc0 wear the power by 0xf) and
/// `0x9b20` (the scout's: 0x10 nothing, 0xff wear the power); component routines at 0x8000 to 0x8050;
/// a base destroyed at 300, a relay at 20, explosions of kind 13 from templates at
/// `DS:0x9b40` (frames 0 to 9, 10 ticks each) and `0x9b45` (8 ticks each).
pub fn synthetic_damage(image: &mut [u8], ds: usize) {
    use crate::damage::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(0x8000, &[0xc3]);
    put(0x8010, &[0xe8, 0, 0, 0x24, 7, 0x28, 0x45, 0x24]);
    put(0x8020, &[0xe8, 0, 0, 0x24, 0xf, 0x28, 0x45, 0x4e]);
    let bit = |offset: u16, bit: u8| {
        let [lo, hi] = offset.to_le_bytes();
        [0xf6, 0x85, lo, hi, bit, 0x75, 9, 0x80, 0x8d, lo, hi, bit]
    };
    put(0x8030, &bit(0xdc, 1));
    put(0x8040, &bit(0xf6, 0x40));
    put(0x8050, &[0xc3]);
    let tables: [&[(u8, u16)]; 3] = [
        &[(0x80, 0x8010), (0xff, 0x8030)],
        &[(0x40, 0x8000), (0xc0, 0x8020)],
        &[(0x10, 0x8000), (0xff, 0x8020)],
    ];
    for (kind, table) in tables.into_iter().enumerate() {
        let at = 0x9b00 + 0x10 * kind as u16;
        put(THRESHOLD_AT[kind], &[0x3c, 0x50, 0x77]);
        put(TABLE_AT[kind], &op(&[0xbb], at));
        for (i, &(chance, routine)) in table.iter().enumerate() {
            put(ds + usize::from(at) + 3 * i, &op(&[chance], routine));
        }
    }
    put(BASE_AT.0, &op(BASE_AT.1, 300));
    put(RELAY_AT.0, &[RELAY_AT.1, &[20]].concat());
    put(EXPLOSION_KIND_AT, &op(&[0xb8], 13));
    put(ROBOT_EXPLOSION_AT, &op(&[0xb8], 0x9b40));
    put(RELAY_EXPLOSION_AT, &op(&[0xb8], 0x9b45));
    put(ds + 0x9b40, &[0, 9, 10, 0x40, 1, 0, 9, 8, 0, 1]);
    // A base's end: five templates (frames 0 to `i`, `i + 1` ticks), radii 0x2800 and
    // 0x4000, the end at step 0x24.
    for (i, at) in END_EXPLOSIONS_AT.into_iter().enumerate() {
        let t = 0x9b50 + 5 * i as u16;
        put(at, &op(&[0xb8], t));
        put(ds + usize::from(t), &[0, i as u8 + 1, i as u8 + 1, 0, 0]);
    }
    put(END_RADII_AT[0], &op(&[0xc7, 0x44, 0x14], 0x2800));
    put(END_RADII_AT[1], &op(&[0xc7, 0x44, 0x14], 0x4000));
    put(END_STEP_AT.0, &[END_STEP_AT.1, &[0x24]].concat());
}

/// The tables [`synthetic_damage`] writes, read back.
pub fn synthetic_damage_tables() -> crate::damage::Damage {
    let mut image = vec![0; 0x1_9c00];
    synthetic_damage(&mut image, 0x1_0000);
    crate::damage::read_image(&image, 0x1_0000).expect("synthetic damage tables")
}

/// [`synthetic_sensing`]'s tables, read.
pub fn synthetic_sensing_tables() -> crate::sensing::Sensing {
    let mut image = vec![0; 0x1_9c00];
    synthetic_sensing(&mut image, 0x1_0000);
    crate::sensing::read_image(&image, 0x1_0000).expect("synthetic sensing tables")
}

/// Writes made-up walking tables (`crate::motion::Walk`) at 0xa100 up, with the
/// instructions that read them: legs `step & 3` and `(step + 2) & 3` (humanoid, then
/// scout), crouched legs `stance / 16`.
pub fn synthetic_walk(image: &mut [u8]) {
    use crate::motion::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let tables = [
        (HUMANOID_LEGS_AT[0], 0),
        (HUMANOID_LEGS_AT[1], 2),
        (SCOUT_LEGS_AT[0], 0),
        (SCOUT_LEGS_AT[1], 2),
    ];
    for (i, (at, shift)) in tables.into_iter().enumerate() {
        let table = 0xa100 + 0x10 * i as u16;
        put(
            at,
            &[&[0x2e, 0x8a, 0x87][..], &table.to_le_bytes()].concat(),
        );
        let legs: Vec<u8> = (0..STEPS as u8).map(|k| (k + shift) & 3).collect();
        put(usize::from(table), &legs);
    }
    put(SCOUT_CROUCH_AT, &[0x2e, 0x8a, 0x87, 0x40, 0xa1]);
    put(0xa140, &(0..STEPS as u8).collect::<Vec<_>>());
}

/// Writes made-up movement tables (`crate::motion`) into a load image whose data
/// segment starts at `ds` and string segment at `strings`: a sine table of `i * 0x100`
/// at `DS:0x9000`, turn steps 0x5b, a weak drive at 0x28, one speed table (the speed
/// itself) at `strings + 0x800`, every mission's programs 8, 13, 8 at `DS:0x9300`,
/// and the walking and damage tables of [`synthetic_walk`] and [`synthetic_damage`].
pub fn synthetic_motion(image: &mut [u8], ds: usize, strings: usize) {
    use crate::motion::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(SINE_NEXT.0, &op(SINE_NEXT.1, 0x9002));
    put(SINE_AT.0, &op(SINE_AT.1, 0x9000));
    for i in 0..SINE_ENTRIES {
        put(ds + 0x9000 + 2 * i, &((i as u16) << 8).to_le_bytes());
    }
    for at in TURN_AT {
        put(at, &op(&[0xbb], 0x5b));
    }
    put(WEAK_DRIVE.0, &[WEAK_DRIVE.1, &[0x28]].concat());
    put(SPEEDS_SEGMENT.0, SPEEDS_SEGMENT.1);
    put(SPEEDS_AT.0, &op(SPEEDS_AT.1, 0x800));
    put(strings + 0x800, &0x802u16.to_le_bytes());
    let identity: Vec<u8> = (0..SPEED_ENTRIES as u8).collect();
    put(strings + 0x802, &identity);
    put(PROGRAMS_AT.0, &op(PROGRAMS_AT.1, 0x9300));
    put(ds + 0x9300, &[8, 13, 8].repeat(MISSIONS));
    // An arctangent of `i * 0x100`, and every kind's script speeds by
    // `mov word [di + 0x28], 10 * (op - 0x53)`.
    put(ATAN_AT.0, &op(ATAN_AT.1, 0x9400));
    for i in 0..ATAN_ENTRIES {
        put(ds + 0x9400 + 2 * i, &((i as u16) << 8).to_le_bytes());
    }
    for (i, at) in SPEED_OPS.into_iter().enumerate() {
        let (table, routine) = (0x9600 + 6 * i as u16, 0xb000 + 5 * i);
        put(at, &op(&[0xff, 0xa7], table));
        put(
            ds + usize::from(table),
            &(routine as u16).to_le_bytes().repeat(3),
        );
        put(routine, &op(&[0xc7, 0x45, 0x28], 10 * (i as u16 + 1)));
    }
    synthetic_walk(image);
    synthetic_damage(image, ds);
    synthetic_gears(image, ds);
    synthetic_weapon_keys(image, ds);
    synthetic_sensing(image, ds);
    synthetic_net(image);
}

/// Writes made-up sensing tables (`crate::sensing`): tick tables at `DS:0x9700`,
/// `0x9720`, `0x9740` with the scan at entry 15, the lock's upkeep at 5, 6, 5 (code
/// at 0xa200 up; arcs 0x1000, 0x1100, 0x1200, ranges 0x300, 0x310, 0x320) and the
/// lock warning's decay after it; scanning ranges 0x500, 0x600, 0x700 and locking
/// ranges 0x800, 0x900, 0xa00 (`DS:0x9760`, `0x9766`); the lock's messages 0x40 to
/// 0x49 in `MESSAGES_AT`'s order and its sound event 26; weapons by kind needing
/// drives 10 and 22 (the scorpion's first with all of bits 3 damaged); the
/// worked-out point (1.0000 + 2/65536, -1/65536) at `DS:0x9770`.
pub fn synthetic_sensing(image: &mut [u8], ds: usize) {
    use crate::sensing::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(SCAN.0, SCAN.1);
    put(LOCKED_ON.0, LOCKED_ON.1);
    put(KEEP_LOCK, &[0xa3, 0x01, 0x72, 0x89, 0x1e]);
    put(SCAN_RANGE_AT, &op(&[0x8b, 0x87], 0x9760));
    put(LOCK_RANGE_AT, &op(&[0x8b, 0x87], 0x9766));
    put(ds + 0x9760, &[0, 5, 0, 6, 0, 7, 0, 8, 0, 9, 0, 10]);
    for (kind, lock) in [5usize, 6, 5].into_iter().enumerate() {
        let (table, routine) = (0x9700 + 0x20 * kind as u16, 0xa200 + 0x10 * kind as u16);
        put(TICKS_AT[kind], &op(&[0xff, 0x97], table));
        let t = ds + usize::from(table);
        put(t + 2 * lock, &routine.to_le_bytes());
        put(t + 2 * (lock + 1), &(LOCKED_ON.0 as u16).to_le_bytes());
        put(t + 30, &(SCAN.0 as u16).to_le_bytes());
        let (arc, range) = (0x1000 + 0x100 * kind as u16, 0x300 + 0x10 * kind as u16);
        let call = (KEEP_LOCK as u16).wrapping_sub(routine + 9);
        let r = usize::from(routine);
        put(
            r,
            &[op(&[0xb8], arc), op(&[0xbb], range), op(&[0xe8], call)].concat(),
        );
    }
    for (i, at) in MESSAGES_AT.into_iter().enumerate() {
        put(at, &op(&[0xbe], 0x40 + i as u16));
    }
    put(LOCK_SOUND_AT, &op(&[0xb8], 52));
    // Weapons: humanoid 0, 1 needing 10 and 22 (bits 1, 2); scorpion 0 taking all of
    // bits 3, then bit 4; scout like the humanoid. Tables at 0xa2e0, routines 0xa300 up.
    put(DRIVE_CHECK.0, DRIVE_CHECK.1);
    for (w, at) in WEAPON_STATE_AT.into_iter().enumerate() {
        let table = 0xa2e0 + 8 * w as u16;
        put(at, &op(&[0x2e, 0xff, 0x97], table));
        for kind in 0..3 {
            let r = 0xa300 + 0x20 * (2 * kind + w) as u16;
            put(usize::from(table) + 2 * kind, &r.to_le_bytes());
            let call = (DRIVE_CHECK.0 as u16).wrapping_sub(r + 10);
            let drive = if w == 0 { 10 } else { 22 };
            let head = [0xb0, w as u8, 0x32, 0xe4, 0x50, 0xb0, drive, 0xe8];
            put(usize::from(r), &head);
            put(usize::from(r) + 8, &call.to_le_bytes());
            put(usize::from(r) + 10, &[0x58, 0x73, 0x03, 0x80, 0xcc, 0x20]);
            let test = match (kind, w) {
                (1, 0) => vec![0x8a, 0x85, 0xdc, 0x00, 0x24, 3, 0x3c, 3, 0x74, 3],
                (1, _) => vec![0xf6, 0x85, 0xdc, 0x00, 4, 0x75, 3, 0x80, 0xcc, 1],
                _ => vec![0xf6, 0x85, 0xdc, 0x00, 1 << w, 0x75, 3, 0x80, 0xcc, 1],
            };
            put(usize::from(r) + 16, &test);
        }
    }
    put(POINT_AT.0, &op(POINT_AT.1, 0x9770));
    put(ds + 0x9770, &[2, 0, 1, 0, 0xff, 0xff, 0xff, 0xff]);
}

/// Writes made-up relay-net instructions (`crate::net`): DENSE NET (13) on a mask of
/// 0xfe, SPARSE NET (12) and the others on 0xfc, the others probing; deploying plays
/// sound 25 and posts 0x50 (none) or 0x51 (deployed); relays of radius 0x500.
pub fn synthetic_net(image: &mut [u8]) {
    use crate::net::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    put(
        LINES_AT,
        &[
            0xe8, 0, 0, 0x8a, 0x45, 0x30, 0xba, 0xfe, 0x00, 0x3c, 13, 0x74, 8, 0xb2, 0xfc, 0x3c,
            12, 0x74, 2, 0xb6, 0xff,
        ],
    );
    put(DEPLOY_AT, &[0xf6, 0x85, 0xfd, 0x00, 0x80, 0x75, 0x1d]);
    put(DEPLOY_AT + 7, &[0x80, 0xbd, 0x18, 0x01, 0x00, 0x74, 0x16]);
    let call = (MAKE_AT as u16).wrapping_sub(DEPLOY_AT as u16 + 17);
    put(DEPLOY_AT + 14, &[&[0xe8][..], &call.to_le_bytes()].concat());
    put(MAKE_AT, &[0x57, 0xb8, 0x04, 0x00, 0xe8]);
    put(DEPLOY_SOUND_AT, &[0xb8, 50, 0]);
    put(NO_RELAY_AT, &[0xbe, 0x50, 0]);
    put(DEPLOYED_AT, &[0xbe, 0x51, 0]);
    put(RADIUS_AT, &[0xc7, 0x44, 0x14, 0x00, 0x05]);
}

/// The made-up relay-net tables of [`synthetic_net`].
pub fn synthetic_net_tables() -> crate::net::Net {
    let mut image = vec![0; 0x1_0000];
    synthetic_net(&mut image);
    crate::net::read_image(&image).expect("synthetic net tables")
}

/// Writes made-up weapon key routines (`crate::motion::WeaponKeys`), code at 0xb180
/// up and tables at `DS:0x96a0` up: sound 5; messages 0x10 and 0x11 (humanoid), 0x30
/// and 0x31 (scout), the scorpion's 0x20, 0x21 loaded, 0x22, 0x23 empty, 0x24, 0x25
/// damaged, its damaged bits 1 and 2; and the recall's program, 6.
pub fn synthetic_weapon_keys(image: &mut [u8], ds: usize) {
    use crate::motion::{RECALL_AT, WEAPON_KEYS};
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    put(WEAPON_KEYS.0, &op(WEAPON_KEYS.1, 0xb180));
    put(0xb180, &[0x90, 0xb1, 0xf0, 0xb1, 0xc0, 0xb1]);
    for (at, load, table) in [(0xb190, 0x10, 0x96a0u16), (0xb1c0, 0x08, 0x96a4)] {
        put(at, &[0x8a, 0xd8, 0x3a, 0x9d, 0xdd, 0x00, 0x74, 0x24]);
        put(at + load, &op(&[0x8b, 0xb7], table));
        put(at + load + 4, &[0xe8, 0, 0, 0xb8, 0x0a, 0]);
    }
    put(
        0xb1f0,
        &[0x32, 0xe4, 0x8b, 0xf0, 0x3a, 0x85, 0xdd, 0x00, 0x74, 0x3c],
    );
    put(0xb1f0 + 0x18, &op(&[0x8a, 0x84], 0x96b4));
    for (offset, table) in [(0x2d, 0x96a8), (0x33, 0x96ac), (0x39, 0x96b0)] {
        put(0xb1f0 + offset, &op(&[0x8b, 0xb4], table));
    }
    put(0xb1f0 + 0x40, &[0xb8, 0x0a, 0]);
    let messages: Vec<u8> = [
        0x10u16, 0x11, 0x30, 0x31, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25,
    ]
    .iter()
    .flat_map(|m| m.to_le_bytes())
    .collect();
    put(ds + 0x96a0, &messages);
    put(ds + 0x96b4, &[1, 2]);
    put(RECALL_AT, &[0xf6, 0x45, 0x23, 0x08, 0x74, 0x0f, 0xb0, 6]);
}

/// Writes made-up gear steps (`crate::motion::Gears`) with the routines that read
/// them, code at 0xb100 up and tables at `DS:0x9640` up: humanoid gears -20, -10, 0,
/// 10, 20; scout gears -30, 0, 30; a scorpion step of 5 within -15..=25 (its up
/// routine after an `int 3`, as the stock build's); turns of 0x80.
pub fn synthetic_gears(image: &mut [u8], ds: usize) {
    use crate::motion::{GEAR_OPS, KEY_TURN};
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    let words = |w: &[i16]| w.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>();
    put(GEAR_OPS[0], &op(&[0xff, 0xa7], 0x9640));
    put(GEAR_OPS[1], &op(&[0xff, 0xa7], 0x9646));
    let (up, down) = ([0xb100u16, 0xb150, 0xb120], [0xb118u16, 0xb160, 0xb138]);
    let routines: Vec<i16> = up.iter().chain(&down).map(|&r| r as i16).collect();
    put(ds + 0x9640, &words(&routines));
    for (kind, field, top, set, table, gears) in [
        (
            0,
            0x0b,
            8,
            0xb140u16,
            0x9660u16,
            &[-20i16, -10, 0, 10, 20][..],
        ),
        (2, 0x09, 4, 0xb148, 0x9680, &[-30, 0, 30]),
    ] {
        let at = usize::from(up[kind]);
        let call = set.wrapping_sub(up[kind] + 17);
        put(at, &[0x8a, 0x9d, field, 0x01, 0x80, 0xfb, top, 0x74, 0x0a]);
        put(at + 9, &op(&[0x32, 0xff, 0x83, 0xc3, 0x02, 0xe8], call));
        put(
            usize::from(down[kind]),
            &[0x8a, 0x9d, field, 1, 0x80, 0xfb, 0],
        );
        put(usize::from(set), &[0x88, 0x9d, field, 0x01, 0x8b, 0x87]);
        put(usize::from(set) + 6, &table.to_le_bytes());
        put(ds + usize::from(table), &words(gears));
    }
    put(
        0xb150,
        &[0xcc, 0x8b, 0x45, 0x28, 0x3d, 25, 0, 0x7d, 0x10, 0x05, 5, 0],
    );
    put(
        0xb160,
        &[0x8b, 0x45, 0x28, 0x3d, 0xf1, 0xff, 0x7e, 0x10, 0x2d, 5, 0],
    );
    put(KEY_TURN.0, &op(KEY_TURN.1, 0x80));
}

/// A synthetic `mapdvr.dvr`: an MZ file whose init hands the host its data segment
/// (paragraph 0x10) from offset 0xde, 0x104 bytes, and whose key dispatcher scans
/// `keys` there, ended by a zero word (`crate::mapdvr`).
pub fn synthetic_mapdvr(keys: &[(u16, u16)]) -> Vec<u8> {
    let mut image = vec![0u8; 0x400];
    let put = |image: &mut Vec<u8>, at: usize, bytes: &[u8]| {
        image[at..at + bytes.len()].copy_from_slice(bytes);
    };
    put(&mut image, 0x24, &[0xb8, 0x04, 0x01]);
    put(&mut image, 0x2c, &[0xbe, 0x10, 0x00]);
    put(&mut image, 0x31, &[0xbe, 0xde, 0x00]);
    put(&mut image, 0x333, &[0xbf, 0x84, 0x01]);
    for (i, &(key, handler)) in keys.iter().enumerate() {
        let at = 0x100 + 0x186 + 4 * i;
        if at + 4 <= image.len() {
            put(&mut image, at, &key.to_le_bytes());
            put(&mut image, at + 2, &handler.to_le_bytes());
        }
    }
    // The MZ header: two paragraphs, no relocations.
    let size = 0x20 + image.len();
    let mut file = b"MZ".to_vec();
    for word in [(size % 512) as u16, size.div_ceil(512) as u16, 0, 2] {
        file.extend(word.to_le_bytes());
    }
    file.resize(0x20, 0);
    file.extend(image);
    file
}

/// Where `synthetic_file_dialogs` puts the dialogs' widget set (segment 0x1400).
pub const SYNTHETIC_FILE_DIALOG_SET: usize = 0x14000;

/// The code and data `crate::filedialog` reads, in a load image whose data segment
/// starts at `ds`: a save dialog with its directories list and scroll bar, an open
/// dialog with OK, the files list and the directories' up arrow, and the error box's
/// OK. Returns the relocated words.
pub fn synthetic_file_dialogs(image: &mut [u8], ds: usize) -> Vec<usize> {
    use crate::filedialog::*;
    let mut put = |at: usize, bytes: &[u8]| image[at..at + bytes.len()].copy_from_slice(bytes);
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let op = |op: &[u8], value: u16| [op, &value.to_le_bytes()[..]].concat();
    let set = SYNTHETIC_FILE_DIALOG_SET;
    let seg = (set / 16) as u16;
    const NONE: [u16; 4] = [0x7fff; 4];
    let widget =
        |kind: u16, state: u16, area: [u16; 4]| words(&[&[kind, state][..], &area].concat());

    put(RUN, &op(&[0xb8], seg));
    put(OPEN, &op(&[0xb8], 0x1100));
    put(OPEN + 6, &op(&[0xbf], 0x1300));
    put(OPEN + 9, &op(&[0xbe], 0x1310));
    put(OPEN + 0x1b, &op(&[0xbb], 0x7c));
    put(OPEN + 0x1e, &op(&[0xba], 0xa0));
    put(SAVE, &op(&[0xb8], 0x1000));
    put(SAVE + 6, &op(&[0xbb], 0x10));
    put(SAVE + 9, &op(&[0xba], 0x70));
    put(ERROR, &op(&[0xb8], 0x1200));
    put(ERROR + 0xb, &op(&[0xb8], seg));
    put(ERROR + 0xe, &op(&[0xbb], 0x70));
    put(ERROR + 0x13, &op(&[0xba], 0x7c));
    put(NAME_FIELD.0, &op(NAME_FIELD.1, 0x1330));
    put(DIRECTORY_PATTERN.0, &op(DIRECTORY_PATTERN.1, 0x1320));

    let fill = |colour: u8, r: [u16; 4]| [&[2, colour][..], &words(&r)].concat();
    let frame = |r: [u16; 4]| [&[4][..], &words(&r)].concat();
    let text = |y: u16, x: u16, s: &[u8]| [&[6][..], &words(&[y, x]), &[0], s, &[0]].concat();
    let font = |id: u16| [&[8][..], &words(&[id])].concat();
    let save_script = [
        fill(12, [26, 76, 131, 265]),
        frame([26, 76, 131, 265]),
        font(0),
        text(30, 146, b"SAVE"),
        font(2),
        text(42, 81, b"NAME:"),
        vec![0],
    ]
    .concat();
    put(ds + 0x1000, &save_script);
    put(
        ds + 0x1100,
        &[fill(12, [26, 76, 131, 265]), text(43, 234, b"OK"), vec![0]].concat(),
    );
    put(
        ds + 0x1200,
        &[frame([26, 76, 131, 265]), text(66, 101, b"WRONG"), vec![0]].concat(),
    );
    put(ds + 0x1300, b"\x03*.*\0");
    put(ds + 0x1310, b"\x05*.USG\0");
    put(ds + 0x1320, b"*.*\0");
    put(ds + 0x1330, &words(&[42, 127, 48, 195]));

    // The widgets' states: an arrow's byte +1 picks its list, the error OK's word +3
    // is its sprite.
    put(ds + 0x1401, &[1]);
    put(ds + 0x1403, &[2]);
    put(ds + 0x1405, &[2]);
    put(ds + 0x1408, &words(&[0x1d0]));
    let list = [
        // Save: 0x10..0x70.
        widget(0x0e, 0x1400, NONE),
        widget(0x1a, 0x1400, NONE),
        widget(0x15c, 0x1400, [0, 0, 200, 320]),
        widget(0x15e, 0x1400, [41, 222, 50, 261]),
        widget(0x160, 0x1400, [54, 222, 63, 261]),
        widget(0x164, 0x1400, [78, 135, 88, 145]),
        widget(0x166, 0x1402, [87, 135, 119, 144]),
        widget(0x16e, 0x1400, [78, 81, 129, 135]),
        // The error box: 0x70..0x7c.
        widget(0x16a, 0x1405, [41, 222, 50, 261]),
    ];
    put(set + 0x10, &list.concat());
    // Open: 0x7c..0xa0.
    let open = [
        widget(0x15e, 0x1400, [41, 222, 50, 261]),
        widget(0x16c, 0x1400, [78, 81, 129, 135]),
        widget(0x164, 0x1404, [78, 218, 88, 228]),
    ];
    put(set + 0x7c, &open.concat());
    vec![RUN + 1, ERROR + 0xc]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_synthetic_lzexe_file_unpacks_to_its_image() {
        let image = synthetic_bytes(1000, 3);
        let file = synthetic_lzexe(&image, (0x10, 2), &[3, 0x103, 0x400]);
        let unpacked = crate::lzexe::unpack(&file).unwrap();
        assert_eq!(unpacked.image, image);
        assert_eq!((unpacked.cs, unpacked.ip), (0x10, 2));
        let at: Vec<_> = unpacked.relocations.iter().map(|r| r.linear()).collect();
        assert_eq!(at, [3, 0x103, 0x400]);
    }

    #[test]
    fn has_the_requested_length() {
        for len in [0, 1, 7, 8, 9, 1000] {
            assert_eq!(synthetic_bytes(len, 1).len(), len);
        }
    }

    #[test]
    fn same_seed_same_bytes_and_prefix_stable() {
        assert_eq!(synthetic_bytes(100, 42), synthetic_bytes(100, 42));
        assert_eq!(synthetic_bytes(100, 42)[..10], synthetic_bytes(10, 42)[..]);
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(synthetic_bytes(32, 1), synthetic_bytes(32, 2));
    }

    #[test]
    fn output_is_pinned() {
        // Guards against accidental changes to the generator, which would silently
        // change every test that uses it.
        assert_eq!(
            synthetic_bytes(8, 0),
            0xe220_a839_7b1d_cdafu64.to_le_bytes()
        );
    }
}
