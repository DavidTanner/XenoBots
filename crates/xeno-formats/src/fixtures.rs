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
/// terrain feature and a kind-0x12 record; a human humanoid at (95.5, 136.25) and an
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
