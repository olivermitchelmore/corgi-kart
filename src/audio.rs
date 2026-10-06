//! Procedural sound. One endless synthesiser voice is played through Bevy's audio as a custom
//! source; game systems set its continuous parameters (engine, drift, crowd) through atomics and
//! queue one-shot sound effects. No audio files.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{AddAudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

use crate::game::{AppState, SessionRes};
use crate::render::fx::{FxKind, RaceFx};
use crate::render::scenery::Spectator;
use crate::render::split::PlayerCamera;
use crate::sim::kart::{drift_stage, MAX_SPEED};
use crate::sim::{Item, Kart};

const RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug)]
pub enum Sfx {
    Beep { freq: f32, len: f32 },
    Chime,
    Blip(f32),
    Whoosh { up: bool },
    Bonk,
    Bark { pitch: f32 },
    Squeak,
    Thud,
    Splash,
    Fanfare,
    Lap,
    /// A dog whistle: a thin, wavering shriek.
    Whistle,
    /// A big dog's bark, from the chest.
    Woof,
    /// A rumble of thunder.
    Thunder,
    /// A quick run up the scale, for the zoomies.
    Jingle,
}

#[derive(Default)]
struct Params {
    engine: AtomicU32,
    throttle: AtomicU32,
    boost: AtomicU32,
    drift: AtomicU32,
    stage: AtomicU32,
    crowd: AtomicU32,
    water: AtomicU32,
    master: AtomicU32,
}

fn set(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}
fn get(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

#[derive(Default)]
pub struct Shared {
    params: Params,
    queue: Mutex<Vec<(Sfx, f32)>>,
}

#[derive(Asset, TypePath, Clone)]
pub struct Synth(Arc<Shared>);

#[derive(Resource, Clone)]
pub struct AudioBus(Arc<Shared>);

impl AudioBus {
    pub fn play(&self, sfx: Sfx, gain: f32) {
        if gain > 0.01 {
            if let Ok(mut q) = self.0.queue.lock() {
                if q.len() < 32 {
                    q.push((sfx, gain));
                }
            }
        }
    }
}

struct Voice {
    sfx: Sfx,
    gain: f32,
    t: f32,
    phase: f32,
}

pub struct SynthSource {
    shared: Arc<Shared>,
    voices: Vec<Voice>,
    rng: u32,
    // smoothed continuous parameters
    engine: f32,
    throttle: f32,
    boost: f32,
    drift: f32,
    crowd: f32,
    water: f32,
    master: f32,
    // oscillator / filter state
    ph: [f32; 3],
    lp: f32,
    skid_lp: f32,
    skid_bp: f32,
    crowd_lp: [f32; 2],
    water_lp: f32,
    counter: u32,
}

impl Synth {
    fn source(&self) -> SynthSource {
        SynthSource {
            shared: self.0.clone(),
            voices: vec![],
            rng: 0x2545_F491,
            engine: 0.0,
            throttle: 0.0,
            boost: 0.0,
            drift: 0.0,
            crowd: 0.0,
            water: 0.0,
            master: 0.0,
            ph: [0.0; 3],
            lp: 0.0,
            skid_lp: 0.0,
            skid_bp: 0.0,
            crowd_lp: [0.0; 2],
            water_lp: 0.0,
            counter: 0,
        }
    }
}

impl Decodable for Synth {
    type Decoder = SynthSource;
    fn decoder(&self) -> SynthSource {
        self.source()
    }
}

impl SynthSource {
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn voice(&mut self, i: usize) -> Option<f32> {
        let dt = 1.0 / RATE as f32;
        let n = self.noise();
        let v = &mut self.voices[i];
        v.t += dt;
        let t = v.t;
        let env = |attack: f32, len: f32| if t < attack { t / attack } else { (1.0 - (t - attack) / (len - attack)).max(0.0).powi(2) };
        let osc = |phase: &mut f32, freq: f32| {
            *phase = (*phase + freq * dt).fract();
            (*phase * TAU).sin()
        };
        let ph = &mut v.phase;
        let s = match v.sfx {
            Sfx::Beep { freq, len } => {
                if t > len {
                    return None;
                }
                osc(ph, freq) * env(0.005, len) * 0.5
            }
            Sfx::Blip(freq) => {
                if t > 0.09 {
                    return None;
                }
                let sq = if osc(ph, freq) > 0.0 { 1.0 } else { -1.0 };
                sq * env(0.002, 0.09) * 0.18
            }
            Sfx::Chime => {
                if t > 0.45 {
                    return None;
                }
                let note = [880.0, 1108.7, 1318.5][((t / 0.07) as usize).min(2)];
                osc(ph, note) * env(0.004, 0.45) * 0.4
            }
            Sfx::Lap => {
                if t > 0.5 {
                    return None;
                }
                let note = if t < 0.12 { 1046.5 } else { 1568.0 };
                osc(ph, note) * env(0.004, 0.5) * 0.4
            }
            Sfx::Fanfare => {
                if t > 1.6 {
                    return None;
                }
                let notes = [523.25, 659.25, 783.99, 1046.5];
                let k = ((t / 0.14) as usize).min(3);
                let f = notes[k];
                let tri = (osc(ph, f) + 0.3 * (*ph * TAU * 2.0).sin()) * 0.8;
                tri * env(0.01, 1.6) * 0.4
            }
            Sfx::Whoosh { up } => {
                if t > 0.5 {
                    return None;
                }
                let sweep = if up { t / 0.5 } else { 1.0 - t / 0.5 };
                let f = 200.0 + sweep * 900.0;
                (osc(ph, f) * 0.25 + n * 0.75) * env(0.05, 0.5) * 0.35
            }
            Sfx::Bonk => {
                if t > 0.55 {
                    return None;
                }
                let f = 420.0 * (1.0 - t * 1.4).max(0.2);
                (osc(ph, f) * 0.8 + n * 0.2 * (1.0 - t * 6.0).max(0.0)) * env(0.002, 0.55) * 0.6
            }
            Sfx::Thud => {
                if t > 0.25 {
                    return None;
                }
                (osc(ph, 70.0 + 60.0 * (1.0 - t * 4.0).max(0.0)) * 0.8 + n * 0.3) * env(0.002, 0.25) * 0.55
            }
            Sfx::Squeak => {
                if t > 0.3 {
                    return None;
                }
                let f = 1400.0 + (t * 30.0).sin() * 500.0;
                osc(ph, f) * env(0.01, 0.3) * 0.25
            }
            Sfx::Splash => {
                if t > 0.7 {
                    return None;
                }
                n * env(0.01, 0.7) * 0.4
            }
            Sfx::Whistle => {
                if t > 0.9 {
                    return None;
                }
                let f = 3100.0 + (t * 55.0).sin() * 140.0 + t * 250.0;
                (osc(ph, f) * 0.8 + n * 0.08) * env(0.02, 0.9) * 0.3
            }
            Sfx::Woof => {
                if t > 0.4 {
                    return None;
                }
                let f = 190.0 * (1.0 - t * 1.3);
                let growl = osc(ph, f) * 0.7 + (*ph * TAU * 2.0).sin() * 0.4 + (*ph * TAU * 3.0).sin() * 0.3 + (*ph * TAU * 5.0).sin() * 0.15;
                (growl + n * 0.25) * env(0.012, 0.4) * 0.7
            }
            Sfx::Thunder => {
                if t > 1.4 {
                    return None;
                }
                let roll = 0.6 + 0.4 * (t * 23.0).sin() * (t * 7.0).cos();
                (osc(ph, 55.0 + n * 20.0) * 0.6 + n * 0.5) * roll * env(0.03, 1.4) * 0.55
            }
            Sfx::Jingle => {
                if t > 0.6 {
                    return None;
                }
                let notes = [659.25, 783.99, 987.77, 1174.7, 1318.5, 1568.0];
                let f = notes[((t / 0.06) as usize).min(5)];
                (osc(ph, f) + 0.3 * (*ph * TAU * 2.0).sin()) * env(0.004, 0.6) * 0.35
            }
            Sfx::Bark { pitch } => {
                // "arf": a buzzy formant with a falling pitch
                if t > 0.2 {
                    return None;
                }
                let f = pitch * (1.0 - t * 1.8);
                let saw = osc(ph, f) * 0.6 + (*ph * TAU * 2.0).sin() * 0.3 + (*ph * TAU * 3.0).sin() * 0.25;
                (saw + n * 0.15) * env(0.008, 0.2) * 0.45
            }
        };
        Some(s * v.gain)
    }
}

impl Iterator for SynthSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let dt = 1.0 / RATE as f32;
        // Pick up new parameters and queued sounds every 64 samples.
        if self.counter.is_multiple_of(64) {
            let p = &self.shared.params;
            let k = 0.02;
            self.engine += (get(&p.engine) - self.engine) * k;
            self.throttle += (get(&p.throttle) - self.throttle) * k;
            self.boost += (get(&p.boost) - self.boost) * k;
            self.drift += (get(&p.drift) - self.drift) * 0.05;
            self.crowd += (get(&p.crowd) - self.crowd) * 0.005;
            self.water += (get(&p.water) - self.water) * 0.05;
            self.master += (get(&p.master) - self.master) * 0.01;
            if let Ok(mut q) = self.shared.queue.try_lock() {
                for (sfx, gain) in q.drain(..) {
                    if self.voices.len() < 24 {
                        self.voices.push(Voice { sfx, gain, t: 0.0, phase: 0.0 });
                    }
                }
            }
        }
        self.counter = self.counter.wrapping_add(1);

        // Engine: a buzzy two-stroke with a sub-octave, low-passed; pitch follows speed.
        let rpm = self.engine;
        let f = 48.0 + rpm * 125.0 + self.boost * 30.0;
        self.ph[0] = (self.ph[0] + f * dt).fract();
        self.ph[1] = (self.ph[1] + f * 0.5 * dt).fract();
        self.ph[2] = (self.ph[2] + f * 2.01 * dt).fract();
        let saw = self.ph[0] * 2.0 - 1.0;
        let sub = if self.ph[1] < 0.5 { 1.0 } else { -1.0 };
        let hi = (self.ph[2] * TAU).sin();
        let putt = 0.7 + 0.3 * (self.ph[1] * TAU).sin();
        let raw = (saw * 0.6 + sub * 0.35 + hi * 0.2) * putt;
        let cutoff = 0.05 + 0.1 * self.throttle + 0.08 * rpm + 0.1 * self.boost;
        self.lp += (raw - self.lp) * cutoff;
        let engine = self.lp * (0.1 + 0.12 * self.throttle + 0.06 * self.boost) * (rpm * 3.0).min(1.0).max(0.25);

        // Tyre squeal and spark crackle while drifting.
        let n = self.noise();
        self.skid_lp += (n - self.skid_lp) * 0.35;
        let bp = self.skid_lp - self.skid_bp;
        self.skid_bp += (self.skid_lp - self.skid_bp) * 0.08;
        let stage = get(&self.shared.params.stage);
        let crackle = if self.noise() > 0.995 - stage * 0.003 { self.noise() * 0.6 } else { 0.0 };
        let skid = (bp * 0.5 + crackle) * self.drift * 0.28;

        // Crowd: two bands of filtered noise with a slow swell.
        let c = self.noise();
        self.crowd_lp[0] += (c - self.crowd_lp[0]) * 0.06;
        self.crowd_lp[1] += (self.crowd_lp[0] - self.crowd_lp[1]) * 0.2;
        let swell = 0.6 + 0.4 * ((self.counter as f32 * dt * 0.7).sin() * 0.5 + 0.5);
        let crowd = (self.crowd_lp[0] - self.crowd_lp[1] * 0.5) * self.crowd * swell * 0.9;

        // Water churn.
        self.water_lp += (self.noise() - self.water_lp) * 0.15;
        let water = self.water_lp * self.water * 0.3;

        let mut s = engine + skid + crowd + water;
        let mut i = 0;
        while i < self.voices.len() {
            match self.voice(i) {
                Some(v) => {
                    s += v;
                    i += 1;
                }
                None => {
                    self.voices.swap_remove(i);
                }
            }
        }
        // soft clip
        Some((s * self.master).tanh() * 0.8)
    }
}

impl Source for SynthSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).unwrap()
    }
    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Synth>()
            .add_systems(Startup, setup_audio)
            .add_systems(Update, (race_audio, fx_audio, menu_audio));
    }
}

/// The synthesiser without a sound card, for `--record`: the same systems feed it, and the
/// recorder asks for each frame's samples.
#[derive(Resource)]
pub struct Offline(SynthSource);

impl Offline {
    pub const RATE: u32 = RATE;

    pub fn samples(&mut self, n: usize) -> impl Iterator<Item = f32> + '_ {
        self.0.by_ref().take(n)
    }
}

pub fn add_offline(app: &mut App) {
    let shared = Arc::new(Shared::default());
    set(&shared.params.master, 1.0);
    app.insert_resource(Offline(Synth(shared.clone()).source()))
        .insert_resource(AudioBus(shared))
        .add_systems(Update, (race_audio, fx_audio));
}

fn setup_audio(mut commands: Commands, mut synths: ResMut<Assets<Synth>>) {
    let shared = Arc::new(Shared::default());
    set(&shared.params.master, 1.0);
    let handle = synths.add(Synth(shared.clone()));
    commands.spawn(AudioPlayer::<Synth>(handle));
    commands.insert_resource(AudioBus(shared));
}

fn race_audio(
    bus: Option<Res<AudioBus>>,
    session: Option<Res<SessionRes>>,
    state: Res<State<AppState>>,
    cams: Query<&GlobalTransform, With<PlayerCamera>>,
    fans: Query<&GlobalTransform, With<Spectator>>,
    mut crowd_cache: Local<(f32, f32)>,
    time: Res<Time>,
) {
    let Some(bus) = bus else { return };
    let p = &bus.0.params;
    let race = session.as_ref().and_then(|s| s.0.race.as_ref()).filter(|_| *state.get() == AppState::Race);
    match race {
        Some(race) => {
            // One engine voice: in split screen it follows whichever local kart is loudest.
            let karts: Vec<&Kart> = race.locals.iter().map(|&i| &race.world.karts[i]).collect();
            let most = |f: &dyn Fn(&Kart) -> f32| karts.iter().map(|k| f(k)).fold(0.0, f32::max);
            set(&p.engine, most(&|k| (k.speed.abs() / MAX_SPEED).min(1.4)));
            let throttle = if race.world.time < crate::sim::COUNTDOWN { 0.3 } else if karts.iter().all(|k| k.finished) { 0.2 } else { 0.8 };
            set(&p.throttle, throttle);
            set(&p.boost, most(&|k| if k.is_boosting() { 1.0 } else { 0.0 }));
            set(&p.drift, most(&|k| if k.drift_dir != 0 && k.speed > 8.0 { 1.0 } else { 0.0 }));
            set(&p.stage, most(&|k| if k.drift_dir != 0 { drift_stage(k.drift_charge) as f32 } else { 0.0 }));
            set(&p.water, most(&|k| if k.in_water && k.speed.abs() > 3.0 { (k.speed.abs() / MAX_SPEED).min(1.0) } else { 0.0 }));
        }
        None => {
            for a in [&p.engine, &p.throttle, &p.boost, &p.drift, &p.stage, &p.water] {
                set(a, 0.0);
            }
        }
    }
    // Crowd loudness from how close the camera is to the fans (checked a few times a second).
    crowd_cache.1 -= time.delta_secs();
    if crowd_cache.1 <= 0.0 {
        crowd_cache.1 = 0.25;
        if !cams.is_empty() {
            let nearest = (cams.iter().flat_map(|c| fans.iter().map(|f| f.translation().distance(c.translation())))).fold(f32::MAX, f32::min);
            crowd_cache.0 = (1.0 - (nearest - 10.0) / 80.0).clamp(0.0, 1.0);
        }
    }
    set(&p.crowd, crowd_cache.0 * if race.is_some() { 1.0 } else { 0.4 });
}

fn fx_audio(bus: Option<Res<AudioBus>>, mut events: MessageReader<RaceFx>, cams: Query<&GlobalTransform, With<PlayerCamera>>) {
    let Some(bus) = bus else {
        events.clear();
        return;
    };
    let cams: Vec<Vec3> = cams.iter().map(|c| c.translation()).collect();
    for ev in events.read() {
        // Loudness from the nearest player's camera.
        let dist = cams.iter().map(|c| ev.pos.distance(*c)).fold(f32::MAX, f32::min);
        let near = if ev.local { 1.0 } else { (1.0 - dist / 60.0).clamp(0.0, 1.0) * 0.7 };
        let local = if ev.local { 1.0 } else { 0.0 };
        match ev.kind {
            FxKind::Countdown(_) => bus.play(Sfx::Beep { freq: 587.3, len: 0.3 }, 1.0),
            FxKind::Go => bus.play(Sfx::Beep { freq: 1174.7, len: 0.7 }, 1.0),
            FxKind::Hit => {
                bus.play(Sfx::Bonk, near);
                bus.play(Sfx::Bark { pitch: 520.0 + (ev.kart % 4) as f32 * 60.0 }, near);
            }
            FxKind::BoxBreak => bus.play(Sfx::Chime, near * 0.8),
            FxKind::ItemReady => bus.play(Sfx::Blip(1320.0), local),
            FxKind::ItemUsed(Item::Treat | Item::GoldenTreat) => bus.play(Sfx::Whoosh { up: true }, near),
            FxKind::ItemUsed(Item::SqueakyToy) => bus.play(Sfx::Squeak, near),
            FxKind::ItemUsed(Item::DecoyBox) => bus.play(Sfx::Blip(330.0), near),
            FxKind::ItemUsed(Item::Zoomies) => bus.play(Sfx::Jingle, near),
            FxKind::ItemUsed(Item::Sausage) => {
                bus.play(Sfx::Whoosh { up: true }, near);
                bus.play(Sfx::Woof, near * 0.7);
            }
            FxKind::ItemUsed(Item::Squirrel) => {
                bus.play(Sfx::Squeak, near);
                bus.play(Sfx::Bark { pitch: 760.0 }, near);
            }
            FxKind::ItemUsed(Item::MudPie) => bus.play(Sfx::Splash, near.max(0.5)),
            FxKind::ItemUsed(Item::RainCloud) => bus.play(Sfx::Thunder, near),
            // The whistle and the woof are heard through their own events.
            FxKind::ItemUsed(Item::DogWhistle | Item::BigWoof) => {}
            FxKind::ItemUsed(_) => bus.play(Sfx::Whoosh { up: false }, near),
            FxKind::Burst(Item::DogWhistle) => bus.play(Sfx::Whistle, 1.0),
            FxKind::Burst(_) => {
                bus.play(Sfx::Splash, near.max(0.15));
                bus.play(Sfx::Thud, near);
            }
            FxKind::WoofBark => bus.play(Sfx::Woof, 0.75),
            FxKind::WoofSlam => {
                bus.play(Sfx::Woof, 1.0);
                bus.play(Sfx::Thunder, 0.6);
            }
            FxKind::Shrunk => bus.play(Sfx::Whoosh { up: false }, near),
            FxKind::Squashed => bus.play(Sfx::Thud, near),
            FxKind::Boost => bus.play(Sfx::Whoosh { up: true }, near * 0.8),
            FxKind::WallHit => bus.play(Sfx::Thud, near),
            FxKind::Hop => bus.play(Sfx::Blip(420.0), local * 0.6),
            FxKind::DriftStage(s) => bus.play(Sfx::Blip(700.0 + s as f32 * 260.0), local),
            FxKind::Lap(_) => bus.play(Sfx::Lap, local),
            FxKind::FinalLap => bus.play(Sfx::Fanfare, local * 0.8),
            FxKind::Finish(_) => {
                bus.play(Sfx::Fanfare, local);
                bus.play(Sfx::Bark { pitch: 600.0 }, local);
            }
            FxKind::Splash => bus.play(Sfx::Splash, near),
            FxKind::Trick => {
                bus.play(Sfx::Whoosh { up: true }, near * 0.7);
                bus.play(Sfx::Bark { pitch: 640.0 + (ev.kart % 4) as f32 * 50.0 }, near * 0.8);
            }
            FxKind::Land => bus.play(Sfx::Thud, near * 0.5),
            FxKind::Cheer => bus.play(Sfx::Bark { pitch: 720.0 + (ev.kart % 4) as f32 * 50.0 }, near * 0.9),
        }
    }
}

/// Little blips for menu navigation.
fn menu_audio(bus: Option<Res<AudioBus>>, state: Res<State<AppState>>, keys: Res<ButtonInput<KeyCode>>, pads: Query<&Gamepad>, stick: Res<crate::input::StickNav>) {
    let Some(bus) = bus else { return };
    if *state.get() == AppState::Race {
        return;
    }
    let nav = crate::input::read_nav(&keys, &pads, &stick);
    if nav.up || nav.down {
        bus.play(Sfx::Blip(660.0), 0.5);
    }
    if nav.left || nav.right {
        bus.play(Sfx::Blip(880.0), 0.5);
    }
    if nav.confirm {
        bus.play(Sfx::Chime, 0.6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sound renders finite, in-range, non-silent samples, and one-shots end.
    #[test]
    fn synth_renders_sensible_audio() {
        let shared = Arc::new(Shared::default());
        set(&shared.params.master, 1.0);
        set(&shared.params.engine, 0.8);
        set(&shared.params.throttle, 1.0);
        set(&shared.params.drift, 1.0);
        set(&shared.params.crowd, 1.0);
        let bus = AudioBus(shared.clone());
        let all = [
            Sfx::Beep { freq: 587.3, len: 0.3 },
            Sfx::Chime,
            Sfx::Blip(700.0),
            Sfx::Whoosh { up: true },
            Sfx::Bonk,
            Sfx::Bark { pitch: 560.0 },
            Sfx::Squeak,
            Sfx::Thud,
            Sfx::Splash,
            Sfx::Fanfare,
            Sfx::Lap,
            Sfx::Whistle,
            Sfx::Woof,
            Sfx::Thunder,
            Sfx::Jingle,
        ];
        for s in all {
            bus.play(s, 1.0);
        }
        let mut src = Synth(shared).source();
        let samples: Vec<f32> = (0..RATE * 3).map(|_| src.next().unwrap()).collect();
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        println!("rms {rms:.3}");
        assert!(rms > 0.02, "too quiet: {rms}");
        assert!(src.voices.is_empty(), "one-shot sounds should have finished");
    }
}
