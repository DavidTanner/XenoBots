//! `.ms3` music as `sounddvr.dvr`'s AdLib back end plays it (`docs/formats/sounddvr.md`,
//! `docs/formats/ms3.md`): a [`Driver`] that steps a song once a retrace and makes the
//! driver's OPL2 register writes, in its order, to any [`Opl`]; and [`Music`], the
//! driver on an emulated chip (Nuked-OPL3 in OPL2 mode) rendering at [`OPL_RATE`].

use std::collections::VecDeque;

use nuked_opl3::Opl3Chip;
use xeno_formats::ms3::{Event, Song};
use xeno_formats::sounddvr::{Bank, CHANNELS, VOICES};

/// The OPL2's own sample rate: its 3.579545 MHz clock over 72.
pub const OPL_RATE: u32 = 49_716;

/// Somewhere the driver's register writes go: the chip, or a recording in tests.
pub trait Opl {
    fn write(&mut self, reg: u8, value: u8);
}

impl Opl for Opl3Chip {
    fn write(&mut self, reg: u8, value: u8) {
        self.write_register(u16::from(reg), value);
    }
}

impl Opl for Vec<(u8, u8)> {
    fn write(&mut self, reg: u8, value: u8) {
        self.push((reg, value));
    }
}

/// One music channel's state, as the driver keeps it (its `DS` tables by channel).
#[derive(Clone, Copy, Default)]
struct Channel {
    program: u8,
    /// The program last loaded; 0xff (none) forces a load at the next note.
    loaded: u8,
    /// Copied from the loaded program.
    velocity_factor: u8,
    transpose: i8,
    envelope: Option<u8>,
    envelope_at: usize,
    /// `2 * note + transpose`; the `En` offset; the pitch last sent to the chip.
    base: u8,
    offset: i8,
    sent: u8,
    /// Scaled velocity of the last note on.
    velocity: u8,
    /// Kept for fidelity; the AdLib back end never reads it.
    active: bool,
}

/// The driver's music state and the AdLib back end's register shadows.
pub struct Driver {
    bank: Bank,
    song: Option<Song>,
    /// Next event; ticks left before it runs.
    at: usize,
    delay: u16,
    doubled_on: [u8; 16],
    channels: [Channel; CHANNELS],
    /// Each voice's `B0` (key, block, F-number high bits) and carrier level byte.
    b0: [u8; VOICES],
    carrier_level: [u8; VOICES],
    /// Whether the chip has been through `opl_init` (a later start shuts it down first).
    initialised: bool,
    /// Whether the tick hook is installed: from a start until a stop.
    hooked: bool,
}

impl Driver {
    pub fn new(bank: Bank) -> Self {
        Self {
            bank,
            song: None,
            at: 0,
            delay: 0,
            doubled_on: [0; 16],
            channels: [Channel::default(); CHANNELS],
            b0: [0; VOICES],
            carrier_level: [0; VOICES],
            initialised: false,
            hooked: false,
        }
    }

    /// Whether a song is playing (false once its `FF 2F` has run).
    pub fn is_playing(&self) -> bool {
        self.song.is_some()
    }

    /// `snd_start_song`: the music init (`opl_init`, after silencing the old one) then
    /// `snd_start_song_now`. The first event runs on the next [`Driver::tick`].
    pub fn start(&mut self, song: &Song, opl: &mut impl Opl) {
        if self.initialised {
            self.stop_all_notes(opl);
            self.silence_all(opl);
        }
        self.init(opl);
        self.stop_all_notes(opl);
        // snd_start_song_now
        self.stop_all_notes(opl);
        self.doubled_on = song.header.doubled_on;
        let programs = song.header.programs;
        for (ch, c) in self.channels.iter_mut().enumerate() {
            c.program = programs[ch];
            // The driver copies 16 programs into a 10-entry table, so the next table,
            // the programs last loaded, starts with the header's programs 10 to 15.
            c.loaded = programs.get(ch + CHANNELS).copied().unwrap_or(0xff);
        }
        self.song = Some(song.clone());
        self.at = 0;
        self.delay = 1;
        self.hooked = true;
    }

    /// `snd_stop_song` (the music shutdown): every note off and every voice silenced
    /// at once, mid-note.
    pub fn stop(&mut self, opl: &mut impl Opl) {
        self.song = None;
        self.hooked = false;
        if self.initialised {
            self.stop_all_notes(opl);
            self.silence_all(opl);
        }
    }

    /// One retrace (`snd_music_tick`): the due events, then the pitch pass. Nothing
    /// once stopped; after a song's end the pitch pass still runs, as in the driver.
    pub fn tick(&mut self, opl: &mut impl Opl) {
        if !self.hooked {
            return;
        }
        if self.song.is_some() {
            self.run_due_events(opl);
        }
        for ch in (0..CHANNELS).rev() {
            let c = &mut self.channels[ch];
            let mut pitch = c.base.wrapping_add(c.offset as u8);
            if let Some(env) = c
                .envelope
                .and_then(|p| self.bank.pitch_envelopes.get(usize::from(p)))
            {
                if c.envelope_at >= env.steps.len() && env.looped {
                    c.envelope_at = 0;
                }
                if let Some(&step) = env.steps.get(c.envelope_at) {
                    pitch = pitch.wrapping_add(step as u8);
                    c.envelope_at += 1;
                }
            }
            if pitch != c.sent {
                c.sent = pitch;
                self.set_pitch(ch, pitch, opl);
            }
        }
    }

    /// `snd_run_due_events`: count the delay down, then run events until one waits.
    fn run_due_events(&mut self, opl: &mut impl Opl) {
        self.delay = self.delay.saturating_sub(1);
        while self.delay == 0 {
            let Some(step) = self
                .song
                .as_ref()
                .and_then(|s| s.steps.get(self.at))
                .copied()
            else {
                // FF 2F: every note stopped; silent until the next start.
                self.delay = 1;
                self.stop_all_notes(opl);
                self.song = None;
                return;
            };
            self.at += 1;
            self.delay = step.delay;
            match step.event {
                Event::NoteOn {
                    channel,
                    note,
                    velocity,
                } => {
                    self.note(channel, note.wrapping_mul(2), velocity, opl);
                }
                Event::NoteOff { channel } => self.note(channel, 0, 0, opl),
                Event::Program { channel, program } => {
                    if let Some(c) = self.channels.get_mut(usize::from(channel)) {
                        c.program = program;
                    }
                }
                Event::PitchOffset { channel, offset } => {
                    if let Some(c) = self.channels.get_mut(usize::from(channel)) {
                        c.offset = offset;
                    }
                }
                Event::Controller { .. } | Event::Skipped { .. } => {}
            }
        }
    }

    /// A note on (`pitch` = twice the note, not 0) or off on `channel` and every
    /// channel its `doubled_on` chain names.
    fn note(&mut self, channel: u8, pitch: u8, velocity: u8, opl: &mut impl Opl) {
        let mut ch = usize::from(channel);
        // shortcut: channels 10 to 15 (past the driver's tables) and chains longer
        // than the channel count are dropped; no stock song uses either.
        for _ in 0..CHANNELS {
            if ch >= CHANNELS {
                return;
            }
            let c = self.channels[ch];
            if c.program != c.loaded {
                self.load_program(ch, opl);
            }
            self.start_or_stop_note(ch, pitch, velocity, opl);
            match self.doubled_on[ch] {
                0 => return,
                next => ch = usize::from(next - 1),
            }
        }
    }

    /// `snd_load_channel_program`, then `opl_set_program`.
    fn load_program(&mut self, ch: usize, opl: &mut impl Opl) {
        let c = &mut self.channels[ch];
        let p = usize::from(c.program);
        c.loaded = c.program;
        c.envelope = Some(c.program);
        c.envelope_at = 0;
        c.velocity_factor = self.bank.velocity_factor.get(p).copied().unwrap_or(0);
        c.transpose = self.bank.transpose.get(p).copied().unwrap_or(0);
        let program = c.program;
        self.set_program(ch, program, opl);
    }

    /// `snd_start_or_stop_note`: a pitch of 0 is a note off.
    fn start_or_stop_note(&mut self, ch: usize, pitch: u8, velocity: u8, opl: &mut impl Opl) {
        let c = &mut self.channels[ch];
        if pitch == 0 {
            c.active = false;
            return self.note_on_off(ch, 0, opl);
        }
        c.velocity =
            ((u16::from(velocity.wrapping_mul(2)) * u16::from(c.velocity_factor)) >> 8) as u8;
        c.envelope_at = 0;
        c.active = true;
        c.base = pitch.wrapping_add(c.transpose as u8);
        // 0xff: no pitch sent, so the pitch pass sends this one.
        c.sent = 0xff;
        let base = c.base;
        self.note_on_off(ch, base, opl);
    }

    /// `snd_stop_all_notes`: every channel's `En` offset cleared and its note stopped.
    fn stop_all_notes(&mut self, opl: &mut impl Opl) {
        for ch in 0..CHANNELS {
            self.channels[ch].offset = 0;
            self.start_or_stop_note(ch, 0, 0, opl);
        }
    }

    /// The channel's OPL2 voice; channel 8 (and any entry of 9 or more) has none.
    fn voice(&self, ch: usize) -> Option<usize> {
        let v = usize::from(*self.bank.channel_voices.get(ch)?);
        (v < VOICES).then_some(v)
    }

    fn slots(&self, v: usize) -> (u8, u8) {
        (self.bank.modulator_slots[v], self.bank.carrier_slots[v])
    }

    /// `opl_init`.
    fn init(&mut self, opl: &mut impl Opl) {
        opl.write(0x01, 0x20);
        opl.write(0x08, 0x00);
        opl.write(0xbd, 0xc0);
        self.silence_all(opl);
        self.b0 = [0x1f; VOICES];
        for ch in 0..VOICES {
            self.set_program(ch, 0, opl);
        }
        self.initialised = true;
    }

    /// `opl_silence_all`: carriers to the quietest level, `B0` cleared (not its shadow).
    fn silence_all(&mut self, opl: &mut impl Opl) {
        for v in 0..VOICES {
            opl.write(0x40u8.wrapping_add(self.slots(v).1), 0x3f);
            opl.write(0xb0 + v as u8, 0);
        }
    }

    /// `opl_set_program`.
    fn set_program(&mut self, ch: usize, program: u8, opl: &mut impl Opl) {
        let Some(v) = self.voice(ch) else { return };
        // shortcut: programs 128 to 255 have no patch here (the driver reads the next
        // table's bytes as one); no stock song uses them.
        let Some(&p) = self.bank.patches.get(usize::from(program)) else {
            return;
        };
        let (m, c) = self.slots(v);
        opl.write(0xb0 + v as u8, self.b0[v] & 0xdf);
        for (reg, value) in [
            (0x20u8.wrapping_add(c), p.carrier_20),
            (0x60u8.wrapping_add(c), p.carrier_60),
            (0x80u8.wrapping_add(c), p.carrier_80),
            (0xe0u8.wrapping_add(c), p.carrier_e0),
            (0x20u8.wrapping_add(m), p.modulator_20),
            (0x40u8.wrapping_add(m), p.modulator_level ^ 0x3f),
            (0x60u8.wrapping_add(m), p.modulator_60),
            (0x80u8.wrapping_add(m), p.modulator_80),
            (0xe0u8.wrapping_add(m), p.modulator_e0),
            (0xc0 + v as u8, p.c0),
        ] {
            opl.write(reg, value);
        }
        self.carrier_level[v] = p.carrier_40;
    }

    /// `opl_note_on_off`: key on with the voice's previous frequency (the pitch pass
    /// sends the new one in the same tick), or key off.
    fn note_on_off(&mut self, ch: usize, pitch: u8, opl: &mut impl Opl) {
        let Some(v) = self.voice(ch) else { return };
        if pitch != 0 {
            let level = (self.channels[ch].velocity / 4 + 0x20).min(0x3f);
            opl.write(
                0x40 + self.slots(v).1,
                (self.carrier_level[v] | level) ^ 0x3f,
            );
            self.b0[v] |= 0x20;
        } else {
            self.b0[v] &= 0xdf;
        }
        opl.write(0xb0 + v as u8, self.b0[v]);
    }

    /// `opl_set_pitch`: F-number low bits, then `B0` with the key bit kept.
    fn set_pitch(&mut self, ch: usize, pitch: u8, opl: &mut impl Opl) {
        let Some(v) = self.voice(ch) else { return };
        let f = self
            .bank
            .frequencies
            .get(usize::from(pitch))
            .copied()
            .unwrap_or(0);
        self.b0[v] = (self.b0[v] & 0xe0) | (f >> 8) as u8;
        opl.write(0xa0 + v as u8, f as u8);
        opl.write(0xb0 + v as u8, self.b0[v]);
    }
}

/// The driver on an emulated OPL2, rendering mono samples at [`OPL_RATE`].
pub struct Music {
    pub driver: Driver,
    chip: Chip,
}

/// The emulated chip, which runs one sample after each register write, as the
/// driver's delay after each write gives the real one about that long (`opl_write_register`).
/// Without it a key off and the next key on land on the same sample and the chip never
/// sees the key off, so a repeated note (the drums) is not struck again.
struct Chip {
    opl: Opl3Chip,
    /// Samples run during writes, played before any new ones.
    pending: VecDeque<i16>,
}

impl Chip {
    fn sample(&mut self) -> i16 {
        let mut frame = [0i16; 2];
        let _ = self.opl.generate(&mut frame);
        // In OPL2 mode both outputs carry the same mix.
        frame[0]
    }
}

impl Opl for Chip {
    fn write(&mut self, reg: u8, value: u8) {
        self.opl.write_register(u16::from(reg), value);
        let s = self.sample();
        self.pending.push_back(s);
    }
}

impl Music {
    pub fn new(bank: Bank) -> Self {
        Self {
            driver: Driver::new(bank),
            chip: Chip {
                opl: Opl3Chip::new(OPL_RATE),
                pending: VecDeque::new(),
            },
        }
    }

    /// Starts `song` from its first event (replacing any song playing).
    pub fn start(&mut self, song: &Song) {
        self.driver.start(song, &mut self.chip);
    }

    /// Silences the music at once.
    pub fn stop(&mut self) {
        self.driver.stop(&mut self.chip);
    }

    /// One retrace of the current video mode.
    pub fn tick(&mut self) {
        self.driver.tick(&mut self.chip);
    }

    /// Renders the next `out.len()` samples: those run during writes first, so the
    /// chip's clock stays the device's.
    pub fn render(&mut self, out: &mut [i16]) {
        for s in out {
            *s = match self.chip.pending.pop_front() {
                Some(s) => s,
                None => self.chip.sample(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xeno_formats::ms3::{Header, Step};
    use xeno_formats::sounddvr::{Envelope, Patch};

    /// Patch `p`'s bytes are `p * 16 + i`; frequency `n` is `0x1000 + n`; channel `c`
    /// is voice `c` but channel 8 has none and 9 is voice 8; slots are v (modulator)
    /// and 0x0a + v (carrier); program 1 transposes by 2, halves velocity
    /// and has a pitch envelope of +1, -1 that loops.
    fn bank() -> Bank {
        let patch = |p: usize| {
            let b: Vec<u8> = (0..16).map(|i| (p * 16 + i) as u8).collect();
            Patch {
                carrier_20: b[0],
                carrier_40: b[1],
                carrier_60: b[2],
                carrier_80: b[3],
                carrier_e0: b[4],
                modulator_20: b[5],
                modulator_level: b[6],
                modulator_60: b[7],
                modulator_80: b[8],
                modulator_e0: b[9],
                c0: b[10],
                unknown_0b: [0; 5],
            }
        };
        let flat = Envelope {
            steps: vec![0],
            looped: false,
        };
        let mut pitch_envelopes = vec![flat; 130];
        pitch_envelopes[1] = Envelope {
            steps: vec![1, -1],
            looped: true,
        };
        let mut velocity_factor = vec![128; 130];
        velocity_factor[1] = 64;
        let mut transpose = vec![0; 130];
        transpose[1] = 2;
        Bank {
            patches: (0..128).map(patch).collect(),
            frequencies: (0..256).map(|n| 0x1000 + n).collect(),
            pitch_envelopes,
            tandy_attack: vec![0; 130],
            tandy_release: vec![0; 130],
            velocity_factor,
            transpose,
            midi_programs: vec![0; 128],
            channel_voices: vec![0, 1, 2, 3, 4, 5, 6, 7, 9, 8],
            modulator_slots: (0..9).collect(),
            carrier_slots: (0x0a..0x13).collect(),
        }
    }

    fn song(programs: [u8; 16], steps: &[(Event, u16)]) -> Song {
        Song {
            header: Header {
                signature: *b"MS3-KGF'91\x00\x00\x00\x1a",
                unknown_0e: 0xff,
                unknown_0f: 0,
                doubled_on: [0; 16],
                programs,
            },
            steps: steps
                .iter()
                .map(|&(event, delay)| Step { event, delay })
                .collect(),
            unknown_trailer: vec![0],
        }
    }

    fn on(channel: u8, note: u8, velocity: u8) -> Event {
        Event::NoteOn {
            channel,
            note,
            velocity,
        }
    }

    /// The writes of each of `ticks` ticks after the start.
    fn play(driver: &mut Driver, song: &Song, ticks: usize) -> Vec<Vec<(u8, u8)>> {
        let mut w = Vec::new();
        driver.start(song, &mut w);
        (0..ticks)
            .map(|_| {
                let mut w = Vec::new();
                driver.tick(&mut w);
                w
            })
            .collect()
    }

    /// `opl_set_program`'s writes for program `p` on voice `v` (slots as in [`bank`]).
    fn program(v: u8, p: u8, b0: u8) -> Vec<(u8, u8)> {
        let b = |i: u8| p * 16 + i;
        let (m, c) = (v, 0x0a + v);
        vec![
            (0xb0 + v, b0 & 0xdf),
            (0x20 + c, b(0)),
            (0x60 + c, b(2)),
            (0x80 + c, b(3)),
            (0xe0 + c, b(4)),
            (0x20 + m, b(5)),
            (0x40 + m, b(6) ^ 0x3f),
            (0x60 + m, b(7)),
            (0x80 + m, b(8)),
            (0xe0 + m, b(9)),
            (0xc0 + v, b(10)),
        ]
    }

    #[test]
    fn start_initialises_the_chip_then_stops_every_note() {
        let mut d = Driver::new(bank());
        let mut w = Vec::new();
        d.start(&song([0; 16], &[]), &mut w);
        assert_eq!(w[..3], [(0x01, 0x20), (0x08, 0), (0xbd, 0xc0)]);
        assert_eq!(w[3..5], [(0x40 + 0x0a, 0x3f), (0xb0, 0)]);
        // Program 0 on channels 0..=7 (channel 8 has no voice), then two rounds of
        // note off on the nine voices.
        assert_eq!(w[21..32], program(0, 0, 0x1f));
        assert_eq!(w.len(), 3 + 18 + 8 * 11 + 2 * 9);
        assert!(
            w[w.len() - 18..]
                .iter()
                .all(|&(r, v)| (0xb0..0xb9).contains(&r) && v == 0x1f)
        );
    }

    #[test]
    fn a_note_loads_its_program_keys_on_then_sets_its_pitch() {
        let mut d = Driver::new(bank());
        let s = song(
            [1; 16],
            &[(on(7, 30, 100), 2), (Event::NoteOff { channel: 7 }, 1)],
        );
        let ticks = play(&mut d, &s, 5);
        let mut first = program(7, 1, 0x1f);
        first.extend([
            // Velocity 100 scaled by factor 64: (200 * 64) >> 8 = 50; level
            // 50 / 4 + 0x20 = 0x2c, ORed into the patch's carrier byte 0x11, XOR 0x3f.
            (0x40 + 0x11, (0x11 | 0x2c) ^ 0x3f),
            // Key on with the previous frequency bits.
            (0xb7, 0x3f),
            // Pitch pass: 2 * 30 + transpose 2 + envelope +1 = 63.
            (0xa7, 63),
            (0xb7, 0x20 | 0x10),
        ]);
        assert_eq!(ticks[0], first);
        // The envelope's -1 this tick, then it loops to +1 as the note stops.
        assert_eq!(ticks[1], [(0xa7, 61), (0xb7, 0x30)]);
        assert_eq!(ticks[2], [(0xb7, 0x10), (0xa7, 63), (0xb7, 0x10)]);
        // FF 2F: every note stopped (channel 8 has no voice), and the song is over.
        assert_eq!(ticks[3][..2], [(0xb0, 0x1f), (0xb1, 0x1f)]);
        assert_eq!(ticks[3].len(), 9 + 2);
        assert!(!d.is_playing());
        assert_eq!(ticks[4], [(0xa7, 63), (0xb7, 0x10)]);
    }

    #[test]
    fn header_programs_10_to_15_count_as_loaded_on_channels_0_to_5() {
        let mut d = Driver::new(bank());
        let mut programs = [0; 16];
        programs[2] = 3;
        programs[12] = 3;
        programs[6] = 3;
        let s = song(programs, &[(on(2, 30, 100), 0), (on(6, 30, 100), 1)]);
        let ticks = play(&mut d, &s, 1);
        // Channel 2 keys on with no program load (and its never-loaded factor of 0),
        // channel 6 loads program 3.
        assert_eq!(
            ticks[0][..2],
            [(0x40 + 0x0c, (0x01 | 0x20) ^ 0x3f), (0xb2, 0x3f)]
        );
        assert_eq!(ticks[0][2..13], program(6, 3, 0x1f));
    }

    #[test]
    fn channel_8_is_silent_and_doubled_channels_follow() {
        let mut d = Driver::new(bank());
        let mut s = song([1; 16], &[(on(8, 30, 100), 1)]);
        assert_eq!(play(&mut d, &s, 1)[0], []);
        // Channel 8 doubled on channel 9 (voice 8): only voice 8 sounds.
        s.header.doubled_on[8] = 10;
        let w = &play(&mut d, &s, 1)[0];
        assert!(w.contains(&(0xb8, 0x3f)));
        assert!(
            !w.iter()
                .any(|&(r, _)| (0xa0..0xa8).contains(&r) || (0xb0..0xb8).contains(&r))
        );
    }

    #[test]
    fn pitch_offset_and_stop() {
        let mut d = Driver::new(bank());
        let mut programs = [0; 16];
        programs[10] = 5;
        let s = song(
            programs,
            &[
                (on(0, 30, 124), 1),
                (
                    Event::PitchOffset {
                        channel: 0,
                        offset: -4,
                    },
                    5,
                ),
            ],
        );
        let ticks = play(&mut d, &s, 2);
        // Velocity 124 at factor 128: level 0x3f, so full volume (attenuation 0).
        assert!(ticks[0].contains(&(0x40 + 0x0a, 0x3f ^ 0x3f)));
        assert_eq!(ticks[1], [(0xa0, 56), (0xb0, 0x30)]);
        let mut w = Vec::new();
        d.stop(&mut w);
        assert!(!d.is_playing());
        assert!(w.ends_with(&[(0x40 + 0x12, 0x3f), (0xb8, 0)]));
        w.clear();
        d.tick(&mut w);
        assert_eq!(w, []);
    }

    #[test]
    fn a_repeated_note_sends_its_pitch_again() {
        let mut d = Driver::new(bank());
        let mut programs = [0; 16];
        programs[10] = 5;
        let s = song(programs, &[(on(0, 30, 124), 1), (on(0, 30, 124), 1)]);
        let ticks = play(&mut d, &s, 2);
        assert_eq!(ticks[1][2..], [(0xa0, 60), (0xb0, 0x30)]);
    }

    /// A drum hit: the note off and the next note on in one tick strike it again.
    #[test]
    fn a_note_off_and_on_in_one_tick_strikes_again() {
        let mut bank = bank();
        // A 440 Hz sine (modulator silent) that decays and does not sustain.
        bank.patches[0] = Patch {
            carrier_20: 0x01,
            carrier_40: 0,
            carrier_60: 0xf8,
            carrier_80: 0xff,
            carrier_e0: 0,
            modulator_20: 0x01,
            modulator_level: 0,
            modulator_60: 0xf0,
            modulator_80: 0xff,
            modulator_e0: 0,
            c0: 0,
            unknown_0b: [0; 5],
        };
        bank.frequencies = vec![0x1244; 256];
        bank.modulator_slots = vec![0, 1, 2, 8, 9, 0xa, 0x10, 0x11, 0x12];
        bank.carrier_slots = vec![3, 4, 5, 0xb, 0xc, 0xd, 0x13, 0x14, 0x15];
        let s = song(
            [0; 16],
            &[
                (on(0, 30, 127), 20),
                (Event::NoteOff { channel: 0 }, 0),
                (on(0, 30, 127), 20),
            ],
        );
        let mut m = Music::new(bank);
        m.start(&s);
        let mut peaks = Vec::new();
        for _ in 0..24 {
            m.tick();
            let mut buf = [0i16; 700];
            m.render(&mut buf);
            peaks.push(buf.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
        }
        // Struck in the first tick, decayed by the 20th, struck again in the 21st.
        assert!(peaks[0] > 4 * peaks[19], "{peaks:?}");
        assert!(peaks[20] > 4 * peaks[19], "{peaks:?}");
    }

    /// Plays the four songs the game starts from the user's files, through the chip,
    /// to their ends: each ends one tick after its total delay, then stays silent.
    #[test]
    #[ignore = "needs XENOBOTS_GAME_DIR"]
    fn real_assets_songs_play_to_their_end() {
        let Some(dir) = std::env::var_os("XENOBOTS_GAME_DIR") else {
            return;
        };
        // `cargo test` runs in the crate directory; resolve against the workspace root.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        let bank = xeno_formats::sounddvr::parse(&std::fs::read(dir.join("sounddvr.dvr")).unwrap())
            .unwrap();
        let mut music = Music::new(bank);
        for name in ["mainthem", "win1", "win2", "bigwin3"] {
            let bytes = std::fs::read(dir.join(format!("{name}.ms3"))).unwrap();
            let song = xeno_formats::ms3::parse(&bytes).unwrap();
            music.start(&song);
            let mut ticks = 0;
            let mut loud = false;
            let mut buf = [0i16; 709]; // about one 70 Hz retrace
            while music.driver.is_playing() {
                music.tick();
                ticks += 1;
                if ticks % 64 == 0 {
                    music.render(&mut buf);
                    loud |= buf.iter().any(|&s| s.unsigned_abs() > 1000);
                }
            }
            assert_eq!(ticks, song.ticks() + 1, "{name}");
            assert!(loud, "{name} made no sound");
        }
    }
}
