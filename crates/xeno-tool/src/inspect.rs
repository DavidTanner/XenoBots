//! `inspect`: a generic summary of any file, plus what the parser makes of it for
//! formats that have one (so far: pcx, b@n, m@l, vcl, the `.mXX` unit models,
//! `scendef.bin`, the `.usg` scenarios, the `.fgd` layers, the `.ms3` music, the
//! `dsounds.bin` sound effects, the `.map` editor terrain, `state.rst`, and the LZEXE
//! packed `xeno.exe` and `config.dvr`, with `xeno.exe`'s intro sequence).

use std::fmt::Write;
use std::path::Path;

/// How many bytes the hex preview shows.
pub const PREVIEW_LEN: usize = 64;

const BYTES_PER_LINE: usize = 16;

/// The report printed for `file` with contents `bytes`.
pub fn report(file: &Path, bytes: &[u8]) -> String {
    let shown = bytes.len().min(PREVIEW_LEN);

    let mut out = String::new();
    writeln!(out, "file:      {}", file.display()).unwrap();
    writeln!(out, "size:      {0} bytes ({0:#x})", bytes.len()).unwrap();
    writeln!(out, "extension: {}", extension(file)).unwrap();
    if let Some(Ok(text) | Err(text)) = details(file, bytes) {
        out.push_str(&text);
    }
    if shown == 0 {
        return out;
    }
    writeln!(out, "first {shown} bytes:").unwrap();
    out.push_str(&hex_preview(&bytes[..shown]));
    out
}

/// What became of one file when its parser, if it has one, was run on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A parser handles the file and accepted it.
    Parsed,
    /// A parser handles the file and rejected it, with this error on one line.
    Failed(String),
    /// No parser handles the file (code, text, or a type nobody has decoded).
    NoParser,
}

/// Runs the parser for `file`, if there is one, on `bytes`.
pub fn outcome(file: &Path, bytes: &[u8]) -> Outcome {
    match details(file, bytes) {
        None => Outcome::NoParser,
        Some(Ok(_)) => Outcome::Parsed,
        Some(Err(text)) => Outcome::Failed(one_line(&text)),
    }
}

/// A failure report on one line: from its `cannot ...` line on, without colour codes,
/// column padding, blank lines or the frame of binrw's backtrace, parts joined by `; `.
fn one_line(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines().skip_while(|l| !l.contains("cannot ")) {
        let line = strip_ansi(line);
        let words: Vec<_> = line.split_whitespace().collect();
        if words.is_empty() || line.contains("Backtrace") || !line.contains(char::is_alphanumeric) {
            continue;
        }
        if !out.is_empty() {
            out.push_str(if out.ends_with(':') { " " } else { "; " });
        }
        out.push_str(&words.join(" "));
    }
    out
}

/// `line` without ANSI escape sequences (`ESC [ ... letter`).
fn strip_ansi(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(char::is_ascii_alphabetic);
        } else {
            out.push(c);
        }
    }
    out
}

/// The lower-case extension of `file`, or `(none)`.
pub fn extension(file: &Path) -> String {
    file.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "(none)".into())
}

/// A parser's report lines: `Ok` if it accepted the file, `Err` if not (from the line
/// with `cannot ...` on, they say why).
type Details = Result<String, String>;

/// Picks the parser for `file` by name, ignoring case, and runs it; `None` if there is
/// no parser for it.
fn details(file: &Path, bytes: &[u8]) -> Option<Details> {
    let name = file.file_name().unwrap_or_default();
    Some(match extension(file).as_str() {
        _ if name.eq_ignore_ascii_case("scendef.bin") => scendef_details(bytes),
        _ if name.eq_ignore_ascii_case("dsounds.bin") => dsounds_details(bytes),
        "pcx" => pcx_details(bytes),
        "b@n" => ban_details(file, bytes),
        "m@l" => mal_details(bytes),
        "vcl" => vcl_details(bytes),
        "m00" | "m08" | "m16" | "m32" => mxx_details(bytes),
        "usg" => usg_details(bytes),
        "fgd" | "kyl" => fgd_details(file, bytes),
        "ms3" => ms3_details(bytes),
        "map" => map_details(bytes),
        "rst" => rst_details(bytes),
        "exe" | "dvr" if xeno_formats::lzexe::is_packed(bytes) => exe_details(file, bytes),
        _ if name.eq_ignore_ascii_case("vgaintro.dvr") => vgaintro_details(bytes),
        _ if name.eq_ignore_ascii_case("mgavideo.dvr") => mgavideo_details(bytes),
        _ => return None,
    })
}

/// The intro drop's script, times and lists, read from `vgaintro.dvr`.
fn vgaintro_details(bytes: &[u8]) -> Details {
    use xeno_formats::vgaintro::{self, CreditOp};

    let drop = vgaintro::parse(bytes).map_err(|e| format!("vgaintro:  cannot parse: {e}\n"))?;
    let n = &drop.numbers;
    let draws = drop
        .credits
        .iter()
        .filter(|op| matches!(op, CreditOp::Draw(_)))
        .count();
    Ok(format!(
        "vgaintro:  drop at counter {}, blends to {} and {}, frames from {}, cave part until {}\n\
         vgaintro:  frames of {} retraces: accelerate to {}, first part ends at {}, view jumps at {}, \
         slow from {} to {}, end at {}\n\
         vgaintro:  {} numbers, a {}-byte colour-animation list, {} credits steps ({draws} text lists)\n",
        n.start,
        n.second_blend,
        n.third_blend,
        n.frames,
        n.cave_end,
        n.frame_retraces,
        n.accelerate_until,
        n.first_part_end,
        n.jump_at,
        n.slow_from,
        n.stop_at,
        n.hold_again_until,
        vgaintro::NUMBERS.len(),
        drop.anims.len(),
        drop.credits.len(),
    ))
}

/// The fonts and the colour-animation list read from `mgavideo.dvr`.
fn mgavideo_details(bytes: &[u8]) -> Details {
    use xeno_formats::{font::Style, mgavideo};

    let video = mgavideo::parse(bytes).map_err(|e| format!("mgavideo:  cannot parse: {e}\n"))?;
    let fonts: Vec<_> = video
        .fonts
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let h = f.header;
            let shape = match f.style() {
                Style::Bitmap => format!("bitmap {}x{}", h.width, h.height),
                Style::Shaded => format!("shaded, {} high", h.height),
            };
            format!("{} {shape} ({} codes)", 2 * i, f.glyphs.len())
        })
        .collect();
    let p = video.set_up_pointer();
    let shapes: Vec<_> = video
        .pointers
        .iter()
        .map(|s| format!("{}x{}", s.width, s.height))
        .collect();
    Ok(format!(
        "mgavideo:  {} fonts: {}\n\
         mgavideo:  a {}-byte colour-animation list of {} records\n\
         mgavideo:  pointer {} of {} shapes ({}), {}x{}, hot spot ({}, {}), colour {}\n",
        fonts.len(),
        fonts.join(", "),
        video.anims.len(),
        video.anim_records(),
        video.pointer,
        shapes.len(),
        shapes.join(", "),
        p.width,
        p.height,
        p.hot_x,
        p.hot_y,
        p.colour,
    ))
}

/// What the LZEXE unpacker makes of `bytes`; for `xeno.exe`, also its intro chain
/// (`sequence`): each entry's picture, effect, origin, end time and fades; and the
/// shape of the map room's tables (`maproom`), the campaign's rules (`campaign`) and
/// the cutscenes after a mission (`ending`).
fn exe_details(file: &Path, bytes: &[u8]) -> Details {
    use xeno_formats::{lzexe, sequence};

    let exe = lzexe::unpack(bytes).map_err(|e| format!("lzexe:     cannot unpack: {e}\n"))?;
    let mut out = format!(
        "lzexe:     unpacks to {} bytes ({}-byte image), {} relocations, entry {:04x}:{:04x}, stack {:04x}:{:04x}\n",
        exe.exe_size(),
        exe.image.len(),
        exe.relocations.len(),
        exe.cs,
        exe.ip,
        exe.ss,
        exe.sp
    );
    let name = file.file_name().unwrap_or_default();
    if !name.eq_ignore_ascii_case("xeno.exe") {
        return Ok(out);
    }
    let fail = |e| format!("{out}intro:     cannot read: {e}\n");
    let ds = sequence::data_segment(&exe).map_err(fail)?;
    let chain = sequence::intro_chain(&exe).map_err(fail)?;
    writeln!(
        out,
        "intro:     {} entries at DS:{:#06x} (DS at image offset {ds:#x}):",
        chain.len(),
        sequence::INTRO_CHAIN
    )
    .unwrap();
    for e in &chain {
        writeln!(
            out,
            "intro:       {:<12} {:?} {} from ({}, {}), until {}, fade in {} out {}",
            e.file_name().unwrap_or("?"),
            e.effect().unwrap_or(sequence::Effect::Hold),
            e.length,
            e.x,
            e.y,
            e.end_time,
            e.fade_in,
            e.fade_out
        )
        .unwrap();
    }
    match xeno_formats::maproom::read(&exe) {
        Ok(room) => writeln!(
            out,
            "maproom:   {} buttons, {} keys, sprites from handle {:#x}",
            room.buttons.len(),
            room.keys.len(),
            room.bank_base
        ),
        Err(e) => writeln!(out, "maproom:   cannot read: {e}"),
    }
    .unwrap();
    match xeno_formats::campaign::read(&exe) {
        Ok(c) => {
            let b = c.won_box;
            writeln!(
                out,
                "campaign:  win bonus {}, progress +1 under {} open, up to {} by hand; {} alert chances; {} hover words; won box ({}, {}, {}, {}), message of {} codes",
                c.win_bonus,
                c.open_limit,
                c.max_progress,
                c.alert_chances.len(),
                c.hover_words.len(),
                b.top,
                b.left,
                b.bottom,
                b.right,
                c.won_message.len()
            )
        }
        Err(e) => writeln!(out, "campaign:  cannot read: {e}"),
    }
    .unwrap();
    match xeno_formats::review::read(&exe) {
        Ok(review) => writeln!(
            out,
            "review:    {} pages, {} keys, {} panels in font {}",
            review.pages.len(),
            review.keys.len(),
            review.panels.len(),
            review.font
        ),
        Err(e) => writeln!(out, "review:    cannot read: {e}"),
    }
    .unwrap();
    match xeno_formats::ending::read(&exe) {
        Ok(e) => writeln!(
            out,
            "endings:   win {}, lose {} by DS:{:#06x}, triumph {}; {} number fields, captions of {}, {}, {} codes",
            e.win.file_name().unwrap_or("?"),
            e.lose.map(|l| l.file_name().unwrap_or("?")).join("/"),
            e.lose_selector,
            e.triumph.file_name().unwrap_or("?"),
            e.fields.len(),
            e.win_caption(Default::default()).len(),
            e.lose_caption(0).len(),
            e.triumph_caption().len()
        ),
        Err(e) => writeln!(out, "endings:   cannot read: {e}"),
    }
    .unwrap();
    exe_mission_details(&exe, &mut out);
    Ok(out)
}

/// The shape of the programs (`orders`), the mission's views and the training menu
/// (`views`), with no game text.
fn exe_mission_details(exe: &xeno_formats::lzexe::Unpacked, out: &mut String) {
    use xeno_formats::orders::{self, RobotKind};
    use xeno_formats::views;

    match orders::read(exe) {
        Ok(p) => {
            let offered = RobotKind::ALL.map(|k| p.offered_to(k).count());
            writeln!(
                out,
                "orders:    {} programs; offered to humanoid {}, scorpion {}, scout {}",
                p.programs.len(),
                offered[0],
                offered[1],
                offered[2]
            )
        }
        Err(e) => writeln!(out, "orders:    cannot read: {e}"),
    }
    .unwrap();
    match views::read(exe) {
        Ok(v) => {
            for (name, view) in [
                ("humanoid", &v.humanoid),
                ("scorpion", &v.scorpion),
                ("scout", &v.scout),
                ("situation", &v.situation),
            ] {
                writeln!(
                    out,
                    "views:     {name:<9} {} widgets, {} keys, state at +{:#x}",
                    view.widgets.len(),
                    view.keys.len(),
                    view.state_offset
                )
                .unwrap();
            }
            writeln!(
                out,
                "views:     situation room: {} sprite buttons from handle {:#x}, {} roster rows",
                v.situation_buttons.len(),
                v.situation_bank,
                v.roster_rows.len()
            )
        }
        Err(e) => writeln!(out, "views:     cannot read: {e}"),
    }
    .unwrap();
    match views::read_training(exe) {
        Ok(t) => writeln!(
            out,
            "training:  {} buttons, {} keys, {} actions",
            t.buttons.len(),
            t.keys.len(),
            t.actions.len()
        ),
        Err(e) => writeln!(out, "training:  cannot read: {e}"),
    }
    .unwrap();
}

/// What the PCX parser makes of `bytes`, or why it could not parse them.
fn pcx_details(bytes: &[u8]) -> Details {
    match xeno_formats::pcx::parse(bytes) {
        Ok(pcx) => {
            let h = &pcx.header;
            Ok(format!(
                "pcx:       version {}, {}x{}, {} bpp, {} plane(s), {} bytes/line, 256-colour palette\n",
                h.version,
                pcx.width(),
                pcx.height(),
                h.bits_per_pixel,
                h.planes,
                h.bytes_per_line,
            ))
        }
        Err(e) => Err(format!("pcx:       cannot parse: {e}\n")),
    }
}

/// What the `.b@n` parsers make of `bytes`: a sky strip if the name starts with
/// `sky`, otherwise a sprite bank.
fn ban_details(file: &Path, bytes: &[u8]) -> Details {
    use xeno_formats::{ban, packed};

    let decoded = match packed::decompress(bytes) {
        Ok(data) => data.len(),
        Err(e) => return Err(format!("packed:    cannot decompress: {e}\n")),
    };
    let packed = format!("packed:    {decoded} bytes decoded\n");
    let is_sky = file
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("sky"));
    let details = if is_sky {
        match ban::parse_sky(bytes) {
            Ok(_) => Ok(format!(
                "b@n:       sky strip, {}x{}",
                ban::Sky::WIDTH,
                ban::Sky::HEIGHT
            )),
            Err(e) => Err(format!("b@n:       cannot parse as a sky strip: {e}")),
        }
    } else {
        match ban::parse_sprites(bytes) {
            Ok(bank) => {
                let spans = bank
                    .sprites
                    .iter()
                    .filter(|s| s.header.has_row_spans())
                    .count();
                Ok(format!(
                    "b@n:       sprite bank, {} sprites ({spans} in row spans)",
                    bank.sprites.len()
                ))
            }
            Err(e) => Err(format!("b@n:       cannot parse as a sprite bank: {e}")),
        }
    };
    with_packed(&packed, details)
}

/// `details`, as a line, after the `packed` line.
fn with_packed(packed: &str, details: Details) -> Details {
    details
        .map(|d| format!("{packed}{d}\n"))
        .map_err(|e| format!("{packed}{e}\n"))
}

/// What the screen parser makes of `bytes`: the picture's size and its PackBits runs.
fn mal_details(bytes: &[u8]) -> Details {
    use xeno_formats::mal::{self, Run, Screen};
    use xeno_formats::packed;

    let data = match packed::decompress(bytes) {
        Ok(data) => data,
        Err(e) => return Err(format!("packed:    cannot decompress: {e}\n")),
    };
    let packed = format!("packed:    {} bytes decoded\n", data.len());
    let details = match mal::parse_unpacked(&data) {
        Ok(_) => {
            let src = &data[Screen::PICTURE_OFFSET..];
            let runs = mal::runs(src, Screen::WIDTH * Screen::HEIGHT).unwrap_or_default();
            let repeats = runs
                .iter()
                .filter(|r| matches!(r, Run::Repeat { .. }))
                .count();
            Ok(format!(
                "m@l:       {}x{} screen with a VGA 6-bit palette, {} PackBits runs ({} literal, {repeats} repeat)",
                Screen::WIDTH,
                Screen::HEIGHT,
                runs.len(),
                runs.len() - repeats,
            ))
        }
        Err(e) => Err(format!("m@l:       cannot parse: {e}")),
    };
    with_packed(&packed, details)
}

/// What the `.vcl` parser makes of `bytes`: the picture's size and its PackBits runs.
fn vcl_details(bytes: &[u8]) -> Details {
    use xeno_formats::mal::Run;
    use xeno_formats::vcl::{self, Picture};

    match vcl::parse(bytes) {
        Ok(picture) => {
            let runs = vcl::runs(bytes).unwrap_or_default();
            let repeats = runs
                .iter()
                .filter(|r| matches!(r, Run::Repeat { .. }))
                .count();
            Ok(format!(
                "vcl:       {}x{} picture with a VGA 6-bit palette, {} PackBits runs ({} literal, {repeats} repeat)\n",
                Picture::WIDTH,
                picture.height,
                runs.len(),
                runs.len() - repeats,
            ))
        }
        Err(e) => Err(format!("vcl:       cannot parse: {e}\n")),
    }
}

/// What the model parser makes of `bytes`: the views, each as `facing/mirror`
/// (one number when the facing is its own mirror, `-` when there is none), then how
/// many images, animations and frames they hold in all.
fn mxx_details(bytes: &[u8]) -> Details {
    use xeno_formats::mxx::{self, ViewHeader};

    match mxx::parse(bytes) {
        Ok(model) => {
            let facings: Vec<_> = model
                .views
                .iter()
                .map(|v| match (v.header.facing, v.header.mirror_facing) {
                    (ViewHeader::NO_FACING, _) => "-".to_string(),
                    (f, m) if f == m => f.to_string(),
                    (f, m) => format!("{f}/{m}"),
                })
                .collect();
            let images: usize = model.views.iter().map(|v| v.images.len()).sum();
            let animations: Vec<_> = model
                .views
                .iter()
                .flat_map(|v| {
                    v.animations
                        .iter()
                        .chain(v.mirror_animations.iter().flatten())
                })
                .collect();
            let frames: usize = animations.iter().map(|a| a.len()).sum();
            Ok(format!(
                "mxx:       {} view(s), facings in 64ths of a turn: {}\n\
                 mxx:       {images} image(s), {} animation(s), {frames} frame(s)\n",
                model.views.len(),
                facings.join(", "),
                animations.len(),
            ))
        }
        Err(e) => Err(format!("mxx:       cannot parse: {e}\n")),
    }
}

/// What the layer parser makes of `bytes`: the range of rows, and what `vgaintro.dvr`
/// uses the file for (known from its name).
fn fgd_details(file: &Path, bytes: &[u8]) -> Details {
    use xeno_formats::fgd::{self, Layer, Role};

    let layer = match fgd::parse(bytes) {
        Ok(layer) => layer,
        Err(e) => return Err(format!("fgd:       cannot parse: {e}\n")),
    };
    let min = layer.rows.iter().min().unwrap_or(&0);
    let max = layer.rows.iter().max().unwrap_or(&0);
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let role = match Role::of(&name) {
        Some((Role::Unused, _)) => "not used by the game".to_string(),
        Some((role, backdrop)) => format!("{role:?} over {backdrop}"),
        None => "unknown role".to_string(),
    };
    Ok(format!(
        "fgd:       {}-column silhouette edge, rows {min}..={max}, {}-column window; {role}\n",
        Layer::LEN,
        Layer::WINDOW,
    ))
}

/// What the sound parser makes of `bytes`: one line per sound with its length, the
/// rate the driver plays it at and its duration.
fn dsounds_details(bytes: &[u8]) -> Details {
    let bank = match xeno_formats::dsounds::parse(bytes) {
        Ok(bank) => bank,
        Err(e) => return Err(format!("dsounds:   cannot parse: {e}\n")),
    };
    let mut out = format!(
        "dsounds:   {} sound(s), unsigned 8-bit mono: index, samples, rate, seconds\n",
        bank.sounds.len()
    );
    for (i, sound) in bank.sounds.iter().enumerate() {
        let (len, rate) = (sound.samples.len(), sound.sample_rate());
        let seconds = len as f64 / f64::from(rate.max(1));
        writeln!(
            out,
            "dsounds:   {i:2}  {len:5}  {rate:4} Hz  {seconds:.2} s"
        )
        .unwrap();
    }
    Ok(out)
}

/// What the music parser makes of `bytes`: the signature, the events by kind and the
/// length in ticks, then each channel the events use with its starting program.
fn ms3_details(bytes: &[u8]) -> Details {
    use xeno_formats::ms3::{self, Event};

    let song = match ms3::parse(bytes) {
        Ok(song) => song,
        Err(e) => return Err(format!("ms3:       cannot parse: {e}\n")),
    };
    let (mut notes, mut programs, mut channels) = (0, 0, [false; 16]);
    for step in &song.steps {
        let channel = match step.event {
            Event::NoteOn { channel, .. } => {
                notes += 1;
                channel
            }
            Event::Program { channel, .. } => {
                programs += 1;
                channel
            }
            Event::NoteOff { channel }
            | Event::PitchOffset { channel, .. }
            | Event::Controller { channel, .. } => channel,
            Event::Skipped { .. } => continue,
        };
        channels[usize::from(channel)] = true;
    }
    let h = &song.header;
    let used: Vec<String> = (0..16)
        .filter(|&c| channels[c])
        .map(|c| match h.doubled_on[c] {
            0 => format!("{c}:{}", h.programs[c]),
            d => format!("{c}:{} (also on {})", h.programs[c], d - 1),
        })
        .collect();
    Ok(format!(
        "ms3:       {:?}, {} event(s): {notes} note(s), {programs} program change(s); {} ticks\n\
         ms3:       channel:starting program {}\n",
        h.signature_text(),
        song.steps.len(),
        song.ticks(),
        used.join(" "),
    ))
}

/// What the scenario table parser makes of `bytes`: one line per scenario with its
/// index, map box, unlock level, state and name (lines joined with ` / `).
fn scendef_details(bytes: &[u8]) -> Details {
    let table = match xeno_formats::scendef::parse(bytes) {
        Ok(table) => table,
        Err(e) => return Err(format!("scendef:   cannot parse: {e}\n")),
    };
    let mut out = format!(
        "scendef:   {} scenario(s): index, map box top,left..bottom,right, unlock level, state, name\n",
        table.scenarios.len()
    );
    for s in &table.scenarios {
        let b = s.map_box;
        writeln!(
            out,
            "scendef:   {:2}  {},{}..{},{}  unlock {}  state {}  {}",
            s.index(),
            b.top,
            b.left,
            b.bottom,
            b.right,
            s.unlock_level,
            s.state,
            s.name_lines().join(" / "),
        )
        .unwrap();
    }
    Ok(out)
}

/// What the `.map` parser makes of `bytes`: the kind of each terrain record (their
/// positions are relative to the map editor's origin).
fn map_details(bytes: &[u8]) -> Details {
    match xeno_formats::map::parse(bytes) {
        Ok(t) => {
            let kinds: Vec<_> = t
                .features
                .iter()
                .map(|f| format!("{:#04x}", f.kind))
                .collect();
            Ok(format!(
                "map:       editor terrain, record kinds [{}]\n",
                kinds.join(", ")
            ))
        }
        Err(e) => Err(format!("map:       cannot parse: {e}\n")),
    }
}

/// What the `.rst` parser makes of `bytes`: its line count and section names.
fn rst_details(bytes: &[u8]) -> Details {
    match xeno_formats::rst::parse(bytes) {
        Ok(r) => Ok(format!(
            "rst:       text, not read by the game; {} line(s), sections {:?}\n",
            r.lines.len(),
            r.sections()
        )),
        Err(e) => Err(format!("rst:       cannot parse: {e}\n")),
    }
}

/// What the scenario parser makes of `bytes`: the start clock, the sky, the extent of
/// the object positions (the file has no map grid), the terrain features, one line per
/// object, each side's paths and the briefing's first line.
fn usg_details(bytes: &[u8]) -> Details {
    use xeno_formats::usg::{self, Side, fixed_to_f64 as f};
    let s = match usg::parse(bytes) {
        Ok(s) => s,
        Err(e) => return Err(format!("usg:       cannot parse: {e}\n")),
    };
    let c = s.header.start;
    let mut out = format!(
        "usg:       starts {:02}:{:02}:{:02} tick {}, day {} month {} (date not applied)\n",
        c.hour, c.minute, c.second, c.tick, c.day, c.month
    );
    if let Some(name) = s.background_name() {
        writeln!(out, "usg:       sky {name}").unwrap();
    }
    let objects = s.objects.as_deref().unwrap_or_default();
    let span = |coord: fn(&usg::Object) -> i32| {
        let values = objects.iter().map(|o| f(coord(o)));
        let min = values.clone().fold(f64::INFINITY, f64::min);
        let max = values.fold(f64::NEG_INFINITY, f64::max);
        format!("{min:.2}..{max:.2}")
    };
    if !objects.is_empty() {
        writeln!(
            out,
            "usg:       no map grid; objects span x {}, y {}",
            span(|o| o.x),
            span(|o| o.y)
        )
        .unwrap();
    }
    if let Some(terrain) = &s.terrain {
        writeln!(
            out,
            "usg:       {} terrain feature(s)",
            terrain.features.len()
        )
        .unwrap();
        for t in &terrain.features {
            match (t.centre(), t.radius()) {
                (Some(p), Some(r)) => writeln!(
                    out,
                    "usg:         circle at {:.2}, {:.2}  radius {:.2}",
                    f(p.x),
                    f(p.y),
                    f64::from(r) / 256.0
                ),
                _ => writeln!(out, "usg:         kind {:#04x}, {} bytes", t.kind, t.size),
            }
            .unwrap();
        }
    }
    writeln!(
        out,
        "usg:       {} object(s): slot, kind, side, x, y, references",
        objects.len()
    )
    .unwrap();
    for o in objects {
        let side = match o.side() {
            Some(Side::Human) => "human",
            Some(Side::Alien) => "alien",
            None => "?",
        };
        writeln!(
            out,
            "usg:       {:3}  {:<10} {side}  {:.2}, {:.2}  refs {}",
            o.slot,
            format!("{:?}", o.kind).to_lowercase(),
            f(o.x),
            f(o.y),
            o.ref_count
        )
        .unwrap();
    }
    for (label, table) in [("alien", &s.alien_paths), ("human", &s.human_paths)] {
        let Some(table) = table else { continue };
        writeln!(out, "usg:       {} {label} path(s)", table.count).unwrap();
        for p in table.used() {
            let mission = p.mission_name().unwrap_or("?");
            let points: Vec<String> = p
                .waypoints()
                .iter()
                .map(|w| format!("{:.0},{:.0}", f(w.x), f(w.y)))
                .collect();
            writeln!(out, "usg:         {mission:<10} {}", points.join(" ")).unwrap();
        }
    }
    let briefing = s.briefing_lines();
    if let Some(first) = briefing.first() {
        writeln!(
            out,
            "usg:       briefing, {} line(s): {first}",
            briefing.len()
        )
        .unwrap();
    }
    for chunk in &s.other_chunks {
        let tag = String::from_utf8_lossy(&chunk.tag);
        writeln!(
            out,
            "usg:       other chunk {tag:?}, {} bytes",
            chunk.data.len()
        )
        .unwrap();
    }
    Ok(out)
}

/// A classic hex dump: offset, 16 bytes in two groups of eight, then the printable
/// ASCII characters. The last line is padded so its ASCII column lines up.
pub fn hex_preview(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(BYTES_PER_LINE).enumerate() {
        write!(out, "{:08x} ", line * BYTES_PER_LINE).unwrap();
        for i in 0..BYTES_PER_LINE {
            if i % 8 == 0 {
                out.push(' ');
            }
            match chunk.get(i) {
                Some(b) => write!(out, "{b:02x} ").unwrap(),
                None => out.push_str("   "),
            }
        }
        out.push_str(" |");
        out.extend(chunk.iter().map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '.'
            }
        }));
        out.push_str("|\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::fixtures::{
        synthetic_bytes, synthetic_dsounds, synthetic_lzexe, synthetic_mgavideo, synthetic_model,
        synthetic_ms3, synthetic_numbers, synthetic_pcx, synthetic_scendef, synthetic_screen,
        synthetic_sprite_bank, synthetic_usg, synthetic_vcl, synthetic_vgaintro,
    };

    #[test]
    fn report_of_a_synthetic_file() {
        let mut bytes = synthetic_bytes(100, 7);
        bytes[..12].copy_from_slice(b"XENO test \x00\xff");
        insta::assert_snapshot!(report(Path::new("dir/ORDERS.TXT"), &bytes));
    }

    #[test]
    fn short_file_pads_the_last_line() {
        insta::assert_snapshot!(report(Path::new("orders"), b"Hello, world!\r\n\x01\x02ab"));
    }

    #[test]
    fn empty_file_has_no_preview() {
        let out = report(Path::new("empty.txt"), &[]);
        assert_eq!(
            out,
            "file:      empty.txt\nsize:      0 bytes (0x0)\nextension: txt\n"
        );
    }

    #[test]
    fn preview_stops_at_64_bytes() {
        let out = report(Path::new("big.bin"), &synthetic_bytes(1000, 1));
        assert!(out.contains("first 64 bytes:\n"));
        assert_eq!(out.lines().filter(|l| l.starts_with("000000")).count(), 4);
    }

    #[test]
    fn pcx_files_show_their_header() {
        let bytes = synthetic_pcx(5, 3, &synthetic_bytes(15, 1), &synthetic_bytes(768, 2));
        let out = report(Path::new("drop.pcx"), &bytes);
        assert!(
            out.contains(
                "pcx:       version 5, 5x3, 8 bpp, 1 plane(s), 6 bytes/line, 256-colour palette\n"
            ),
            "{out}"
        );
        let out = report(Path::new("bad.PCX"), &bytes[..10]);
        assert!(out.contains("pcx:       cannot parse: "), "{out}");
    }

    #[test]
    fn sprite_banks_and_sky_strips_are_summarised() {
        let bank = synthetic_sprite_bank(&[(1, 1, &[1]), (2, 2, &[1, 2, 3, 4])]);
        let out = report(Path::new("MSPRITE0.B@N"), &bank);
        assert!(out.contains("packed:    23 bytes decoded\n"), "{out}");
        assert!(
            out.contains("b@n:       sprite bank, 2 sprites (0 in row spans)\n"),
            "{out}"
        );
        let out = report(Path::new("sky_a.b@n"), &bank);
        assert!(
            out.contains("b@n:       cannot parse as a sky strip: "),
            "{out}"
        );
        let out = report(Path::new("sky_a.b@n"), b"\x01");
        assert!(out.contains("packed:    cannot decompress: "), "{out}");
    }

    #[test]
    fn screens_count_their_runs() {
        // Each row: 256 equal bytes (two repeats of 128), then 64 literals.
        let row: Vec<u8> = [vec![1; 256], (0..64).collect()].concat();
        let bytes = synthetic_screen(&[0; 768], &row.repeat(200));
        let out = report(Path::new("LOGIN.M@L"), &bytes);
        assert!(
            out.contains(
                "m@l:       320x200 screen with a VGA 6-bit palette, 600 PackBits runs (200 literal, 400 repeat)\n"
            ),
            "{out}"
        );
        let out = report(
            Path::new("login.m@l"),
            &synthetic_screen(&[0; 768], &[0; 64_000])[..20],
        );
        assert!(out.contains("packed:    cannot decompress: "), "{out}");
        let out = report(
            Path::new("login.m@l"),
            &xeno_formats::fixtures::synthetic_packed(&[0; 800]),
        );
        assert!(out.contains("m@l:       cannot parse: "), "{out}");
    }

    #[test]
    fn vcl_pictures_count_their_rows_and_runs() {
        // Each row: 512 equal bytes (four repeats of 128), then 128 literals.
        let row: Vec<u8> = [vec![1; 512], (0..128).collect()].concat();
        let bytes = synthetic_vcl(&[0; 768], &row.repeat(240));
        let out = report(Path::new("EALOGO.VCL"), &bytes);
        assert!(
            out.contains(
                "vcl:       640x240 picture with a VGA 6-bit palette, 1200 PackBits runs (240 literal, 960 repeat)\n"
            ),
            "{out}"
        );
        let out = report(Path::new("ealogo.vcl"), &bytes[..800]);
        assert!(out.contains("vcl:       cannot parse: "), "{out}");
    }

    #[test]
    fn models_list_their_facings() {
        let model = synthetic_model(&[(0, &[(1, 1, &[3])]), (8, &[])]);
        let out = report(Path::new("LASER.M08"), &model);
        assert!(
            out.contains(
                "mxx:       2 view(s), facings in 64ths of a turn: 0, 8/56\n\
                 mxx:       1 image(s), 3 animation(s), 3 frame(s)\n"
            ),
            "{out}"
        );
        let out = report(Path::new("laser.m00"), &[]);
        assert!(out.contains("mxx:       cannot parse: "), "{out}");
    }

    #[test]
    fn scenario_tables_list_their_scenarios() {
        let table = synthetic_scendef(&[
            ((10, 20, 19, 29), 1, b"FIRST\x0c"),
            ((100, 200, 109, 209), 3, b"SECOND\x0cPLACE\x0c"),
        ]);
        let out = report(Path::new("SCENDEF.BIN"), &table);
        assert!(
            out.contains(
                "scendef:   2 scenario(s): index, map box top,left..bottom,right, unlock level, state, name\n\
                 scendef:    0  10,20..19,29  unlock 1  state 0  FIRST\n\
                 scendef:    1  100,200..109,209  unlock 3  state 0  SECOND / PLACE\n"
            ),
            "{out}"
        );
        let out = report(Path::new("scendef.bin"), &table[..5]);
        assert!(out.contains("scendef:   cannot parse: "), "{out}");
        assert!(!report(Path::new("dsounds.bin"), &table).contains("scendef:"));
    }

    #[test]
    fn scenarios_list_their_objects_and_paths() {
        let file = synthetic_usg();
        let out = report(Path::new("SCEN00A.USG"), &file);
        let details: String = out
            .lines()
            .filter(|l| l.starts_with("usg:"))
            .map(|l| format!("{l}\n"))
            .collect();
        insta::assert_snapshot!(details);
        let out = report(Path::new("scen00a.usg"), &file[..file.len() - 6]);
        assert!(out.contains("usg:       cannot parse: "), "{out}");
    }

    #[test]
    fn maps_and_rst_files_are_summarised() {
        let map = [4, 0, 0x14, 0x10, 4, 0, 0x12, 0x08, 0, 0];
        let out = report(Path::new("DEFAULT.MAP"), &map);
        assert!(
            out.contains("map:       editor terrain, record kinds [0x14, 0x12]\n"),
            "{out}"
        );
        let out = report(Path::new("backup.map"), &map[..8]);
        assert!(out.contains("map:       cannot parse: "), "{out}");

        let out = report(Path::new("STATE.RST"), b"1 2\r\n[A]\r\nx\r\n\x1a");
        let summary = "rst:       text, not read by the game; 3 line(s), sections [\"A\"]\n";
        assert!(out.contains(summary), "{out}");
    }

    #[test]
    fn songs_count_their_events() {
        let mut song = synthetic_ms3(&[7, 0, 9], &[0x90, 60, 99, 0x10, 0x80, 0, 0xc2, 4]);
        song.extend([0x81, 0x00, 0xff, 0x2f, 0x00]);
        song[0x12] = 1;
        let out = report(Path::new("WIN1.MS3"), &song);
        assert!(
            out.contains(
                "ms3:       \"MS3-TEST\", 3 event(s): 1 note(s), 1 program change(s); 144 ticks\n\
                 ms3:       channel:starting program 0:7 2:9 (also on 0)\n"
            ),
            "{out}"
        );
        let out = report(Path::new("win1.ms3"), &song[..song.len() - 3]);
        assert!(out.contains("ms3:       cannot parse: "), "{out}");
    }

    #[test]
    fn sounds_show_their_length_and_rate() {
        let bytes = synthetic_dsounds(&[(0xffff, &[0x80; 7231]), (0x4000, &[0x80; 904])]);
        let out = report(Path::new("DSOUNDS.BIN"), &bytes);
        assert!(
            out.contains(
                "dsounds:   2 sound(s), unsigned 8-bit mono: index, samples, rate, seconds\n\
                 dsounds:    0   7231  7231 Hz  1.00 s\n\
                 dsounds:    1    904  1808 Hz  0.50 s\n"
            ),
            "{out}"
        );
        let out = report(Path::new("dsounds.bin"), &bytes[..10]);
        assert!(out.contains("dsounds:   cannot parse: "), "{out}");
    }

    #[test]
    fn layers_show_their_rows_and_role() {
        let mut rows = vec![40; 1920];
        rows[7] = 3;
        let out = report(Path::new("LAYER5.FGD"), &rows);
        assert!(
            out.contains(
                "fgd:       1920-column silhouette edge, rows 3..=40, 768-column window; Ceiling over cavedrop.pcx\n"
            ),
            "{out}"
        );
        let out = report(Path::new("fgarray.kyl"), &rows);
        assert!(out.contains("; not used by the game\n"), "{out}");
        let out = report(Path::new("layer1.fgd"), &rows[..10]);
        assert!(out.contains("fgd:       cannot parse: "), "{out}");
    }

    #[test]
    fn packed_executables_show_their_header_and_xeno_exe_its_intro() {
        // Entry 0001:0000 loads DS = 0x20; ten made-up entries at DS:0xecac, each
        // `ealogo.vcl` held 60 ticks until 100, fades 32 and 64.
        let chain = 0x200 + 0xecac;
        let mut image = vec![0u8; chain + 260];
        image[0x10..0x15].copy_from_slice(&[0xb8, 0x20, 0, 0x8e, 0xd8]);
        let entry: Vec<u8> = [0x20u16, 0, 0, 0, 60, 0, 0, 0, 100, 32, 64, 2, 0]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        for i in 0..10 {
            image[chain + i * 26..][..26].copy_from_slice(&entry);
        }
        let file = synthetic_lzexe(&image, (1, 0), &[0x11]);
        let out = report(Path::new("XENO.EXE"), &file);
        let size = image.len() + 512;
        assert!(
            out.contains(&format!(
                "lzexe:     unpacks to {size} bytes ({}-byte image), 1 relocations, entry 0001:0000, stack 0000:0000\n\
                 intro:     10 entries at DS:0xecac (DS at image offset 0x200):\n\
                 intro:       ealogo.vcl   Hold 60 from (0, 0), until 100, fade in 32 out 64\n",
                image.len()
            )),
            "{out}"
        );
        let out = report(Path::new("config.dvr"), &file);
        assert!(
            out.contains("1 relocations") && !out.contains("intro:"),
            "{out}"
        );
        let out = report(Path::new("xeno.exe"), &synthetic_lzexe(&image, (1, 0), &[]));
        assert!(out.contains("intro:     cannot read: "), "{out}");
        assert_eq!(outcome(Path::new("xeno.exe"), b"MZ"), Outcome::NoParser);
    }

    #[test]
    fn vgaintro_shows_its_drop() {
        let file = synthetic_vgaintro(&synthetic_numbers());
        let out = report(Path::new("VGAINTRO.DVR"), &file);
        assert!(
            out.contains(
                "vgaintro:  drop at counter 100, blends to 110 and 120, frames from 130, cave part until 200\n\
                 vgaintro:  frames of 2 retraces: accelerate to 4, first part ends at 10, view jumps at 14, \
                 slow from 20 to 24, end at 34\n\
                 vgaintro:  63 numbers, a 34-byte colour-animation list, 6 credits steps (1 text lists)\n"
            ),
            "{out}"
        );
        assert!(matches!(
            outcome(Path::new("vgaintro.dvr"), b"MZ"),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn mgavideo_shows_its_fonts() {
        let out = report(Path::new("MGAVIDEO.DVR"), &synthetic_mgavideo());
        assert!(
            out.contains(
                "mgavideo:  4 fonts: 0 bitmap 4x3 (128 codes), 2 bitmap 6x3 (128 codes), \
                 4 shaded, 3 high (128 codes), 6 shaded, 3 high (96 codes)\n\
                 mgavideo:  a 18-byte colour-animation list of 1 records\n\
                 mgavideo:  pointer 1 of 2 shapes (3x2, 3x2), 3x2, hot spot (1, 0), colour 60\n"
            ),
            "{out}"
        );
        assert!(matches!(
            outcome(Path::new("mgavideo.dvr"), b"MZ"),
            Outcome::Failed(_)
        ));
    }

    #[test]
    fn outcomes_say_whether_a_parser_accepted_the_file() {
        let rows = vec![40; 1920];
        assert_eq!(outcome(Path::new("LAYER1.FGD"), &rows), Outcome::Parsed);
        assert_eq!(outcome(Path::new("XENO.EXE"), &rows), Outcome::NoParser);
        assert_eq!(outcome(Path::new("other.bin"), &rows), Outcome::NoParser);
        let Outcome::Failed(error) = outcome(Path::new("Scendef.Bin"), &[1, 2, 3]) else {
            panic!("a 3-byte scenario table parsed");
        };
        assert!(error.starts_with("scendef: cannot parse: "), "{error}");
        assert!(!error.contains(['\n', '\x1b']), "{error}");
    }

    #[test]
    fn failure_reports_are_put_on_one_line() {
        let text = "packed:    9 bytes decoded\nm@l:       cannot parse: \n \u{257a}\u{2501} Backtrace \u{2501}\u{2578}\n\n 0: \x1b[1mError: short\x1b[22m\n     at here\n \u{257a}\u{2501}\u{2578}\n";
        assert_eq!(
            one_line(text),
            "m@l: cannot parse: 0: Error: short; at here"
        );
    }

    #[test]
    fn unusual_extensions_are_kept() {
        assert!(report(Path::new("login.M@L"), b"x").contains("extension: m@l\n"));
    }
}
