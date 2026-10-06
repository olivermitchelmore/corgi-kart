//! `--record`: render offscreen at a steady 60 frames a second and save it as a video, with the
//! game's own sound. Game time advances exactly a sixtieth of a second per frame however long the
//! frame takes to render, so the video is smooth and (with `--autodrive`) repeatable.
//!
//! Frames are piped to `ffmpeg` as they're captured; the synthesiser is run alongside, a frame's
//! worth of samples at a time, and joined on at the end.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;

use crate::audio::Offline;
use crate::game::{Cli, RaceEntity};
use crate::render::scene::HeadlessTarget;

pub const FPS: u32 = 60;

/// Real seconds to hold the clock at the start, while shaders compile.
const WARM_UP: f32 = 4.0;

/// A camera that films your kart from a set place around it, in place of the chase camera.
#[derive(Clone, Copy, Debug)]
pub struct FilmCam {
    pub dist: f32,
    pub height: f32,
    /// Radians round from behind the kart (pi is head on).
    pub angle: f32,
    /// Radians a second.
    pub spin: f32,
    /// Degrees.
    pub fov: f32,
}

impl FilmCam {
    pub fn parse(s: &str) -> Option<Self> {
        let v: Vec<f32> = s.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
        let at = |i: usize, d: f32| v.get(i).copied().unwrap_or(d);
        (2..=5).contains(&v.len()).then(|| FilmCam {
            dist: v[0],
            height: v[1],
            angle: at(2, 0.0).to_radians(),
            spin: at(3, 0.0).to_radians(),
            fov: at(4, 60.0),
        })
    }
}

pub struct RecordPlugin;

impl Plugin for RecordPlugin {
    fn build(&self, app: &mut App) {
        crate::audio::add_offline(app);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / FPS as f64)))
            .init_resource::<Recorder>()
            .add_systems(First, warm_up.before(bevy::time::TimeSystems))
            .add_systems(Update, hide_hud.run_if(|cli: Res<Cli>| cli.no_hud))
            .add_systems(Last, record);
    }
}

/// Hold game time still for the first few seconds, so nothing is missed while shaders compile.
fn warm_up(mut time: ResMut<Time<Virtual>>, mut started: Local<Option<Instant>>) {
    let waited = started.get_or_insert_with(Instant::now).elapsed().as_secs_f32();
    if waited < WARM_UP {
        time.pause();
    } else if time.is_paused() {
        time.unpause();
    }
}

fn hide_hud(mut roots: Query<&mut Node, (With<RaceEntity>, With<UiTargetCamera>)>) {
    for mut node in &mut roots {
        if node.display != Display::None {
            node.display = Display::None;
        }
    }
}

#[derive(Resource, Default)]
struct Recorder {
    ffmpeg: Option<Child>,
    /// Frames asked for, and written.
    asked: u32,
    written: u32,
    /// Frames that arrived before an earlier one.
    early: BTreeMap<u32, Vec<u8>>,
    sound: Vec<i16>,
}

impl Recorder {
    fn frame(&mut self, n: u32, pixels: Vec<u8>) {
        self.early.insert(n, pixels);
        while let Some(pixels) = self.early.remove(&self.written) {
            let pipe = self.ffmpeg.as_mut().and_then(|f| f.stdin.as_mut()).expect("ffmpeg is running");
            pipe.write_all(&pixels).expect("ffmpeg takes the frame");
            self.written += 1;
        }
    }
}

fn silent(file: &str) -> String {
    format!("{file}.video.mp4")
}

fn wav(file: &str) -> String {
    format!("{file}.wav")
}

fn record(
    mut commands: Commands,
    time: Res<Time>,
    cli: Res<Cli>,
    target: Res<HeadlessTarget>,
    mut synth: ResMut<Offline>,
    mut rec: ResMut<Recorder>,
    mut exit: MessageWriter<AppExit>,
) {
    let file = cli.record.as_deref().unwrap();
    let frames = (cli.record_secs * FPS as f32).round() as u32;
    // The synthesiser runs all along, so the engine is already humming when recording starts.
    let per_frame = (Offline::RATE / FPS) as usize;
    let sound: Vec<i16> = if time.delta_secs() > 0.0 { synth.samples(per_frame).map(|s| (s * 32767.0) as i16).collect() } else { vec![] };

    if time.elapsed_secs() >= cli.record_from && rec.asked < frames && time.delta_secs() > 0.0 {
        if rec.ffmpeg.is_none() {
            let (w, h) = cli.size;
            #[rustfmt::skip]
            let child = Command::new("ffmpeg")
                .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba"])
                .args(["-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"])
                .args(["-c:v", "libx264", "-preset", "fast", "-crf", "12", "-pix_fmt", "yuv420p", "-f", "mp4", &silent(file)])
                .stdin(Stdio::piped())
                .spawn();
            match child {
                Ok(c) => rec.ffmpeg = Some(c),
                Err(e) => {
                    error!("--record needs ffmpeg: {e}");
                    exit.write(AppExit::error());
                    return;
                }
            }
        }
        rec.sound.extend(sound);
        let n = rec.asked;
        rec.asked += 1;
        commands.spawn(Screenshot::image(target.0.clone())).observe(move |shot: On<ScreenshotCaptured>, mut rec: ResMut<Recorder>| {
            rec.frame(n, shot.image.data.clone().unwrap_or_default());
        });
    }

    if rec.written == frames && rec.ffmpeg.is_some() {
        let mut ffmpeg = rec.ffmpeg.take().unwrap();
        drop(ffmpeg.stdin.take());
        let ok = ffmpeg.wait().is_ok_and(|s| s.success()) && write_wav(&wav(file), &rec.sound).is_ok() && {
            #[rustfmt::skip]
            let join = Command::new("ffmpeg")
                .args(["-loglevel", "error", "-y", "-i", &silent(file), "-i", &wav(file)])
                .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "256k", file])
                .status();
            join.is_ok_and(|s| s.success())
        };
        let _ = std::fs::remove_file(silent(file));
        let _ = std::fs::remove_file(wav(file));
        if ok {
            println!("recorded {frames} frames to {file}");
        } else {
            error!("couldn't record {file}");
        }
        exit.write(if ok { AppExit::Success } else { AppExit::error() });
    }
}

fn write_wav(path: &str, samples: &[i16]) -> std::io::Result<()> {
    let bytes = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + bytes as usize);
    out.extend(b"RIFF");
    out.extend((36 + bytes).to_le_bytes());
    out.extend(b"WAVEfmt ");
    // 16 bytes of format: PCM, mono, 16 bits.
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(Offline::RATE.to_le_bytes());
    out.extend((Offline::RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(bytes.to_le_bytes());
    out.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    std::fs::write(path, out)
}
