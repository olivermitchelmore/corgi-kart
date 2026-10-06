//! Race events and particles.
//!
//! `detect_events` compares the world with what it saw last frame and announces what happened
//! (hits, item boxes, boosts, laps...). That works identically for host and client, since it
//! only looks at world state. Particles and sounds react to those events.

use bevy::{light::NotShadowCaster, prelude::*};

use super::kart::KartVis;
use super::models::{Kit, SPARK_COLORS};
use super::split::PlayerCamera;
use crate::game::SessionRes;
use crate::sim::kart::{drift_stage, TrickTry};
use crate::sim::{Item, COUNTDOWN};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FxKind {
    Countdown(u8),
    Go,
    Hit,
    BoxBreak,
    ItemReady,
    ItemUsed(Item),
    Boost,
    WallHit,
    Hop,
    DriftStage(usize),
    Lap(u8),
    FinalLap,
    Finish(u8),
    Splash,
    /// A trick off a ramp, timed right.
    Trick,
    /// Touching down after a jump.
    Land,
    /// Hitting someone with an item.
    Cheer,
    /// A water balloon or frisbee bursting, or a dog whistle being blown.
    Burst(Item),
    /// One of a big woof's warning barks, and the one that lands.
    WoofBark,
    WoofSlam,
    /// A kart made small, or flattened.
    Shrunk,
    Squashed,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct RaceFx {
    pub kind: FxKind,
    /// Kart involved (player 1's kart for race-wide events).
    pub kart: usize,
    pub pos: Vec3,
    /// Involves a kart driven on this machine.
    pub local: bool,
}

#[derive(Clone, Copy, Default)]
struct KartSnap {
    spin: f32,
    boost: f32,
    bump: f32,
    hop: f32,
    roulette: f32,
    item: Item,
    item_count: u8,
    lap: u8,
    stage: usize,
    finished: bool,
    wet: bool,
    trick: f32,
    airtime: f32,
    airborne: bool,
    cheer: f32,
    small: f32,
    flat: f32,
}

#[derive(Default)]
pub struct Tracker {
    race_id: Option<u32>,
    time: f32,
    karts: Vec<KartSnap>,
    boxes: Vec<f32>,
    /// The bursts already announced, and each big woof's owner and how far along it was.
    bursts: Vec<u16>,
    woofs: Vec<(u8, f32)>,
}

pub fn detect_events(session: Option<Res<SessionRes>>, mut last: Local<Tracker>, mut out: MessageWriter<RaceFx>) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else {
        last.race_id = None;
        return;
    };
    let w = &race.world;
    let snaps: Vec<KartSnap> = w
        .karts
        .iter()
        .map(|k| KartSnap {
            spin: k.spin,
            boost: k.boost,
            bump: k.bump,
            hop: k.hop,
            roulette: k.roulette,
            item: k.item,
            item_count: k.item_count,
            lap: k.lap,
            stage: if k.drift_dir != 0 { drift_stage(k.drift_charge) } else { 0 },
            finished: k.finished,
            wet: k.in_water,
            trick: if k.trick_try == TrickTry::Done { k.trick } else { 0.0 },
            airtime: k.airtime,
            airborne: k.airborne,
            cheer: k.cheer,
            small: k.small,
            flat: k.flat,
        })
        .collect();
    let boxes: Vec<f32> = w.boxes.iter().map(|b| b.respawn).collect();
    let bursts: Vec<u16> = w.bursts.iter().map(|b| b.id).collect();
    let woofs: Vec<(u8, f32)> = w.woofs.iter().map(|f| (f.owner, f.t)).collect();
    if last.race_id != Some(race.race_id) || last.karts.len() != snaps.len() {
        *last = Tracker { race_id: Some(race.race_id), time: w.time, karts: snaps, boxes, bursts, woofs };
        return;
    }
    let pos_of = |i: usize| {
        let k = &w.karts[i];
        Vec3::new(k.pos.x, race.track.height_idx(k.track_idx as usize), k.pos.y)
    };
    let mut emit = |kind: FxKind, kart: usize| {
        out.write(RaceFx { kind, kart, pos: pos_of(kart), local: race.locals.contains(&kart) });
    };

    // Countdown beeps
    for n in [3u8, 2, 1] {
        let at = COUNTDOWN - n as f32;
        if last.time < at && w.time >= at {
            emit(FxKind::Countdown(n), race.locals[0]);
        }
    }
    if last.time < COUNTDOWN && w.time >= COUNTDOWN {
        emit(FxKind::Go, race.locals[0]);
    }

    for (i, (now, was)) in snaps.iter().zip(&last.karts).enumerate() {
        if now.trick > was.trick + 0.1 {
            emit(FxKind::Trick, i);
        }
        if was.airborne && !now.airborne && was.airtime > 0.3 {
            emit(FxKind::Land, i);
        }
        if now.wet && !was.wet {
            emit(FxKind::Splash, i);
        }
        if now.spin > 0.0 && was.spin <= 0.0 {
            emit(FxKind::Hit, i);
        }
        if now.cheer > was.cheer + 0.1 {
            emit(FxKind::Cheer, i);
        }
        if now.boost > was.boost + 0.25 {
            emit(FxKind::Boost, i);
        }
        if now.bump > was.bump + 0.2 {
            emit(FxKind::WallHit, i);
        }
        if now.hop > was.hop + 0.1 {
            emit(FxKind::Hop, i);
        }
        if now.stage > was.stage {
            emit(FxKind::DriftStage(now.stage), i);
        }
        if was.roulette > 0.0 && now.roulette <= 0.0 && now.item != Item::None {
            emit(FxKind::ItemReady, i);
        }
        if now.small > was.small + 0.5 {
            emit(FxKind::Shrunk, i);
        }
        if now.flat > was.flat + 0.5 {
            emit(FxKind::Squashed, i);
        }
        let used = was.item != Item::None && was.roulette <= 0.0 && (now.item_count < was.item_count || now.item == Item::None);
        // Not if it was knocked out of its paws, or was a golden treat running out.
        let lost = now.small > was.small || (now.spin > 0.0 && was.spin <= 0.0) || was.item == Item::GoldenTreat;
        if used && !lost {
            emit(FxKind::ItemUsed(was.item), i);
        }
        if now.finished && !was.finished {
            emit(FxKind::Finish(w.karts[i].place), i);
        } else if now.lap > was.lap && now.lap >= 2 && !now.finished {
            emit(if now.lap == crate::sim::LAPS { FxKind::FinalLap } else { FxKind::Lap(now.lap) }, i);
        }
    }
    for (bi, (now, was)) in boxes.iter().zip(&last.boxes).enumerate() {
        if *now > 0.0 && *was <= 0.0 {
            let b = &w.boxes[bi];
            let p = Vec3::new(b.pos.x, race.track.height_idx(b.track_idx as usize) + 1.4, b.pos.y);
            let nearest = (0..w.karts.len()).min_by(|&a, &c| w.karts[a].pos.distance(b.pos).total_cmp(&w.karts[c].pos.distance(b.pos))).unwrap_or(0);
            out.write(RaceFx { kind: FxKind::BoxBreak, kart: nearest, pos: p, local: race.locals.contains(&nearest) });
        }
    }
    for b in w.bursts.iter().filter(|b| !last.bursts.contains(&b.id)) {
        let pos = Vec3::new(b.pos.x, race.track.height_idx(b.track_idx as usize), b.pos.y);
        let kart = b.owner as usize;
        out.write(RaceFx { kind: FxKind::Burst(b.kind), kart, pos, local: race.locals.contains(&kart) });
    }
    // A big woof is for everyone to hear, wherever its owner is.
    let mut woof = |kind: FxKind, owner: u8| {
        let kart = (owner as usize).min(w.karts.len() - 1);
        out.write(RaceFx { kind, kart, pos: pos_of(kart), local: true });
    };
    for &(owner, t) in &woofs {
        let before = last.woofs.iter().find(|f| f.0 == owner).map_or(-1.0, |f| f.1);
        if crate::sim::world::WOOF_BARKS.iter().any(|&at| before < at && t >= at) {
            woof(FxKind::WoofBark, owner);
        }
    }
    for &(owner, _) in last.woofs.iter().filter(|f| !woofs.iter().any(|g| g.0 == f.0)) {
        woof(FxKind::WoofSlam, owner);
    }
    *last = Tracker { race_id: Some(race.race_id), time: w.time, karts: snaps, boxes, bursts, woofs };
}

// ----- Particle assets ----------------------------------------------------------------------

/// Meshes and materials made for particles.
#[derive(Resource)]
pub struct FxKit {
    /// A pointed shard along Z, widest near its head at -Z: sparks, flame tongues and droplets.
    shard: Handle<Mesh>,
    /// A unit square facing +Z, for camera-facing puffs and flat foam.
    quad: Handle<Mesh>,
    /// White-hot versions of the drift spark colours, for the flames at the wheels.
    hot: [Handle<StandardMaterial>; 4],
    /// The spark colours, dim enough that bloom leaves them coloured rather than white.
    streak: [Handle<StandardMaterial>; 4],
    /// Soft additive glows in the drift spark colours, for the flare round each rear wheel.
    flare: [Handle<StandardMaterial>; 4],
    /// Each ramp goes from faint to its full opacity, so a particle fades by stepping down it.
    ramps: [Vec<Handle<StandardMaterial>>; 4],
}

#[derive(Clone, Copy)]
enum Ramp {
    Mist,
    Smoke,
    Foam,
    Drop,
}

const RAMP_STEPS: usize = 8;

pub fn setup_fx(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    let mut b = super::mesh::MeshBuilder::default();
    let ring: Vec<Vec3> = (0..4).map(|i| {
        let a = i as f32 * std::f32::consts::FRAC_PI_2;
        Vec3::new(a.cos() * 0.5, a.sin() * 0.5, -0.25)
    }).collect();
    for tip in [Vec3::new(0.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 0.5)] {
        for i in 0..4 {
            let (mut p, mut q) = (ring[i], ring[(i + 1) % 4]);
            let mut n = (p - tip).cross(q - tip).normalize();
            if n.dot(tip + p + q) < 0.0 {
                std::mem::swap(&mut p, &mut q);
                n = -n;
            }
            b.tri([tip, p, q], n, Color::WHITE);
        }
    }
    let shard = meshes.add(b.build());
    let quad = meshes.add(Rectangle::new(1.0, 1.0));

    // A soft round puff with a billowy edge, and a broken ring of foam.
    let puff = soft_image(&mut images, |x, y| {
        let r = (x * x + y * y).sqrt();
        let n = super::textures::fbm(x * 0.5 + 0.5, y * 0.5 + 0.5, 4, 3, 41);
        let edge = 0.62 + (n - 0.5) * 0.5;
        let a = (1.0 - (r / edge).powi(2)).clamp(0.0, 1.0);
        a.sqrt() * (0.7 + n * 0.6)
    });
    // A bright core in a wide soft halo.
    let glow = soft_image(&mut images, |x, y| {
        let r2 = x * x + y * y;
        ((-r2 * 14.0).exp() + (-r2 * 4.0).exp() * 0.45) * (1.0 - r2).clamp(0.0, 1.0)
    });
    let foam = soft_image(&mut images, |x, y| {
        // Lacy blotches, densest in a loose ring, thinning out to nothing at the edge.
        let r = (x * x + y * y).sqrt();
        let n = super::textures::fbm(x * 0.5 + 0.5, y * 0.5 + 0.5, 6, 4, 43);
        let lace = super::textures::fbm(x * 0.5 + 0.5, y * 0.5 + 0.5, 24, 2, 44);
        let body = (1.0 - ((r - 0.55) / 0.45).abs()).clamp(0.0, 1.0);
        ((n * 0.8 + lace * 0.6 - 0.62) * 4.0 * body).clamp(0.0, 1.0)
    });

    // Water is unlit: lit by a night sky it came out a murky grey, and splashes should read
    // bright. (Blended emissive would get the raytraced camera's exposure and go black.)
    let ramp = |mats: &mut Assets<StandardMaterial>, base: Color, peak: f32, tex: Option<&Handle<Image>>, unlit: bool| -> Vec<Handle<StandardMaterial>> {
        (1..=RAMP_STEPS)
            .map(|i| {
                let a = peak * i as f32 / RAMP_STEPS as f32;
                mats.add(StandardMaterial {
                    base_color: base.with_alpha(a),
                    base_color_texture: tex.cloned(),
                    unlit,
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: if tex.is_some() { 0.9 } else { 0.08 },
                    reflectance: 0.6,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                })
            })
            .collect()
    };
    let ramps = [
        ramp(&mut mats, Color::srgb(0.62, 0.72, 0.82), 0.5, Some(&puff), true),
        ramp(&mut mats, Color::srgb(0.9, 0.9, 0.93), 0.4, Some(&puff), false),
        ramp(&mut mats, Color::srgb(0.7, 0.8, 0.88), 0.85, Some(&foam), true),
        ramp(&mut mats, Color::srgb(0.8, 0.9, 1.0), 0.9, None, true),
    ];
    let hot = SPARK_COLORS.map(|c| mats.add(super::models::glow(c * 0.8 + LinearRgba::rgb(1.2, 1.2, 1.2))));
    let streak = SPARK_COLORS.map(|c| mats.add(super::models::glow(c * 0.45)));
    // Unlit, so it looks the same in both renderers: the forward pass would apply the raytraced
    // camera's exposure to emissive and leave it black.
    let flare = SPARK_COLORS.map(|c| {
        mats.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::rgb(c.red, c.green, c.blue) * 0.9).with_alpha(1.0),
            base_color_texture: Some(glow.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            fog_enabled: false,
            ..default()
        })
    });
    commands.insert_resource(FxKit { shard, quad, hot, streak, flare, ramps });
}

/// A white texture with alpha from `f` over -1..1, with a mip chain.
fn soft_image(images: &mut Assets<Image>, f: impl Fn(f32, f32) -> f32) -> Handle<Image> {
    use bevy::{
        asset::RenderAssetUsages,
        image::{ImageSampler, ImageSamplerDescriptor},
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    const N: usize = 128;
    let mut level: Vec<f32> = (0..N * N).map(|i| {
        let (x, y) = ((i % N) as f32 + 0.5, (i / N) as f32 + 0.5);
        f(x / N as f32 * 2.0 - 1.0, y / N as f32 * 2.0 - 1.0)
    }).collect();
    let mut data = Vec::new();
    let (mut size, mut mips) = (N, 0);
    loop {
        data.extend(level.iter().flat_map(|a| [255, 255, 255, (a.clamp(0.0, 1.0) * 255.0).round() as u8]));
        mips += 1;
        if size == 1 {
            break;
        }
        let half = size / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                (level[y * size + x] + level[y * size + x + 1] + level[(y + 1) * size + x] + level[(y + 1) * size + x + 1]) * 0.25
            })
            .collect();
        size = half;
    }
    let extent = Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 };
    let mut img = Image::new(extent, TextureDimension::D2, data[..N * N * 4].to_vec(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = mips;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    images.add(img)
}

// ----- Particles ----------------------------------------------------------------------------

/// How a particle is drawn as it moves.
#[derive(Clone, Copy, Default)]
enum Shape {
    /// Tumbles as it flies.
    #[default]
    Tumble,
    /// Stretched along its motion as the camera sees it, as if blurred over this many seconds.
    Streak(f32),
    /// Keeps the direction it was spawned in; this many times longer than it is wide.
    Spike(f32),
    /// Faces the camera, turning slowly.
    Billboard,
    /// Lies flat (foam on the water), turning slowly.
    Flat,
}

#[derive(Component)]
pub struct Particle {
    vel: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    gravity: f32,
    drag: f32,
    spin: Vec3,
    shape: Shape,
    /// The ground's height and how much speed a bounce keeps; nothing bounces back up from
    /// below zero, it just ends there.
    floor: Option<(f32, f32)>,
    ramp: Option<Ramp>,
    /// Lives in its kart's space rather than the world's, like sparks stuck to the wheels.
    attached: bool,
}

const MAX_PARTICLES: usize = 1800;

#[derive(Resource, Default)]
pub struct ParticleCount(usize);

struct Spawn<'a> {
    mesh: &'a Handle<Mesh>,
    mat: &'a Handle<StandardMaterial>,
    pos: Vec3,
    vel: Vec3,
    life: f32,
    size: (f32, f32),
    gravity: f32,
    drag: f32,
}

/// The rest of a particle's behaviour; the default tumbles freely through the world.
#[derive(Clone, Copy, Default)]
struct Style {
    shape: Shape,
    floor: Option<(f32, f32)>,
    ramp: Option<Ramp>,
    /// Spawn in this kart's space instead of the world's.
    parent: Option<Entity>,
    /// Initial orientation, for spikes.
    dir: Option<Vec3>,
}

fn spawn(commands: &mut Commands, count: &mut ParticleCount, p: Spawn, rng: &mut Rng) {
    spawn_styled(commands, count, p, Style::default(), rng);
}

fn spawn_styled(commands: &mut Commands, count: &mut ParticleCount, p: Spawn, style: Style, rng: &mut Rng) {
    if count.0 >= MAX_PARTICLES {
        return;
    }
    count.0 += 1;
    let s = p.size.0;
    let rotation = match style.dir {
        Some(d) => Quat::from_rotation_arc(Vec3::NEG_Z, d.normalize_or(Vec3::NEG_Z)),
        None if matches!(style.shape, Shape::Streak(_)) => Quat::from_rotation_arc(Vec3::NEG_Z, p.vel.normalize_or(Vec3::NEG_Z)),
        None => Quat::from_euler(EulerRot::XYZ, rng.f() * 6.0, rng.f() * 6.0, 0.0),
    };
    let scale = match style.shape {
        Shape::Spike(len) => Vec3::new(s, s, s * len),
        Shape::Streak(_) => Vec3::new(s, s, s * 2.0),
        _ => Vec3::splat(s),
    };
    let particle = Particle {
        vel: p.vel,
        age: 0.0,
        life: p.life,
        size: p.size,
        gravity: p.gravity,
        drag: p.drag,
        spin: rng.v3() * 8.0,
        shape: style.shape,
        floor: style.floor,
        ramp: style.ramp,
        attached: style.parent.is_some(),
    };
    let common = (
        Mesh3d(p.mesh.clone()),
        MeshMaterial3d(p.mat.clone()),
        Transform { translation: p.pos, rotation, scale },
        particle,
        NotShadowCaster,
        super::gfx::NoRaytrace,
    );
    match style.parent {
        // Goes with its kart, so it mustn't be despawned separately.
        Some(parent) => {
            commands.entity(parent).with_child(common);
        }
        None => {
            commands.spawn((common, crate::game::RaceEntity, DespawnOnExit(crate::game::AppState::Race)));
        }
    }
}

#[derive(Default)]
pub struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    fn s(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
    fn v3(&mut self) -> Vec3 {
        Vec3::new(self.s(), self.s(), self.s())
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn side(&mut self) -> f32 {
        if self.f() < 0.5 { -1.0 } else { 1.0 }
    }
}

/// Opacity through a particle's life (0..1): a quick fade in, then out over the second half.
fn fade(f: f32) -> f32 {
    (f / 0.08).min(1.0) * ((1.0 - f) / 0.55).min(1.0)
}

#[allow(clippy::type_complexity)]
pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxKit>,
    mut count: ResMut<ParticleCount>,
    cameras: Query<(&PlayerCamera, &Transform), Without<Particle>>,
    mut last_cams: Local<Vec<Vec3>>,
    mut parts: Query<(Entity, &mut Particle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();
    // Each player's camera: position, rotation and velocity. Streaks blur by their motion relative
    // to the camera, which follows the kart.
    let mut cams: Vec<(usize, Vec3, Quat)> = cameras.iter().map(|(p, c)| (p.0, c.translation, c.rotation)).collect();
    cams.sort_by_key(|c| c.0);
    if cams.is_empty() {
        cams.push((0, Vec3::ZERO, Quat::IDENTITY));
    }
    let cams: Vec<(Vec3, Quat, Vec3)> = (cams.iter().enumerate())
        .map(|(i, &(_, pos, rot))| {
            let vel = match last_cams.get(i) {
                Some(&p) if dt > 0.0 => ((pos - p) / dt).clamp_length_max(150.0),
                _ => Vec3::ZERO,
            };
            (pos, rot, vel)
        })
        .collect();
    *last_cams = cams.iter().map(|c| c.0).collect();
    let mut alive = 0;
    for (e, mut p, mut tr, mut mat) in &mut parts {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        let drag = (-p.drag * dt).exp();
        p.vel *= drag;
        p.vel.y -= p.gravity * dt;
        tr.translation += p.vel * dt;
        if let Some((y, keep)) = p.floor
            && tr.translation.y < y
            && p.vel.y < 0.0
        {
            if keep <= 0.0 {
                commands.entity(e).despawn();
                continue;
            }
            tr.translation.y = y;
            p.vel.y *= -keep;
            p.vel.x *= 0.7;
            p.vel.z *= 0.7;
        }
        alive += 1;
        // Shaped for whichever player's camera is closest.
        let &(cam_pos, cam_rot, cam_vel) = if cams.len() == 1 { &cams[0] } else { cams.iter().min_by(|a, b| a.0.distance_squared(tr.translation).total_cmp(&b.0.distance_squared(tr.translation))).unwrap() };
        let f = p.age / p.life;
        let mut s = p.size.0 + (p.size.1 - p.size.0) * f;
        // Shrink away rather than filling the screen as it flies past the camera.
        if !p.attached {
            s *= ((tr.translation.distance(cam_pos) - 1.5) / 3.0).clamp(0.0, 1.0);
        }
        match p.shape {
            Shape::Tumble => {
                tr.scale = Vec3::splat(s);
                let spin = p.spin * dt;
                tr.rotate(Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z));
            }
            Shape::Streak(blur) => {
                let rel = if p.attached { p.vel } else { p.vel - cam_vel };
                let len = rel.length();
                if len > 0.1 {
                    tr.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, rel / len);
                }
                tr.scale = Vec3::new(s, s, (len * blur).min(2.2).max(s * 1.5));
            }
            Shape::Spike(ratio) => tr.scale = Vec3::new(s, s, s * ratio),
            Shape::Billboard => {
                tr.scale = Vec3::splat(s);
                tr.rotation = cam_rot * Quat::from_rotation_z(p.spin.z * 0.15 * p.age);
            }
            Shape::Flat => {
                tr.scale = Vec3::splat(s);
                tr.rotation = Quat::from_rotation_y(p.spin.y * 0.05 * p.age) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
            }
        }
        if let Some(r) = p.ramp {
            let steps = &fx.ramps[r as usize];
            let i = ((fade(f) * RAMP_STEPS as f32).ceil() as usize).clamp(1, RAMP_STEPS) - 1;
            if mat.0 != steps[i] {
                mat.0 = steps[i].clone();
            }
        }
    }
    count.0 = alive;
}

/// Continuous emitters (drift sparks and smoke, dust, water spray, boost flames) and event bursts.
#[allow(clippy::too_many_arguments)]
pub fn emit_particles(
    mut commands: Commands,
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    fx: Res<FxKit>,
    mut count: ResMut<ParticleCount>,
    mut events: MessageReader<RaceFx>,
    karts: Query<(Entity, &KartVis, &Transform)>,
    cameras: Query<&Transform, (With<PlayerCamera>, Without<KartVis>)>,
    mut acc: Local<Vec<[f32; 12]>>,
    mut rng: Local<Rng>,
    mut fireworks: Local<f32>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let dt = time.delta_secs().min(0.05);
    if rng.0 == 0 {
        rng.0 = 0x1234_5678;
    }
    acc.resize(race.world.karts.len(), [0.0; 12]);
    let cube = &kit.cube;
    let sphere = &kit.sphere;
    let cams: Vec<Vec3> = cameras.iter().map(|c| c.translation).collect();
    let events: Vec<RaceFx> = events.read().copied().collect();
    let mut roots = vec![None; race.world.karts.len()];
    // The karts' roots are placed on the road this frame (their global transforms lag a frame).
    for (entity, kv, tr) in &karts {
        let Some(k) = race.world.karts.get(kv.index) else { continue };
        roots[kv.index] = Some((entity, *tr));
        let a = &mut acc[kv.index];
        let (rot, pos) = (tr.rotation, tr.translation);
        let local = |p: Vec3| pos + rot * p;
        let (back, right, up) = (rot * Vec3::Z, rot * Vec3::X, rot * Vec3::Y);
        let speed = k.speed.abs();
        let kvel = -back * k.speed;
        // Karts far from every camera shed fewer particles.
        let near = cams.iter().map(|c| (1.0 - (c.distance(pos) - 30.0) / 90.0).clamp(0.15, 1.0)).reduce(f32::max).unwrap_or(1.0);
        // The body (and its rear wheels) swings round into a drift.
        let slip = Quat::from_rotation_y(-k.slip);

        // Drift: sparks in the charge colour, stuck to the rear wheels. Each tyre
        // flares with a flickering glow and big white-hot flame tongues, and streaks spray back,
        // bouncing off the road.
        let rear = |side: f32, y: f32| slip * Vec3::new(side * 1.0, y, 1.1);
        if k.drift_dir != 0 && k.drift_charge > 0.25 && k.hop <= 0.0 {
            let stage = drift_stage(k.drift_charge);
            let heat = [0.5, 1.0, 1.15, 1.3][stage];
            a[0] += dt * 150.0 * heat * near;
            while a[0] >= 1.0 {
                a[0] -= 1.0;
                let side = rng.side();
                let vel = slip * Vec3::new(side * rng.range(-0.5, 4.5), rng.range(1.5, 6.0), rng.range(4.0, 13.0)) * heat.max(0.8);
                let s = rng.range(0.045, 0.075) * heat.max(0.6);
                let style = Style { shape: Shape::Streak(0.1), floor: Some((0.03, 0.45)), parent: Some(entity), ..default() };
                let p = Spawn { mesh: &fx.shard, mat: &fx.streak[stage], pos: rear(side, 0.1), vel, life: rng.range(0.16, 0.36), size: (s, s * 0.4), gravity: 24.0, drag: 1.5 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            a[5] += dt * if stage > 0 { 90.0 } else { 40.0 } * near;
            while a[5] >= 1.0 {
                a[5] -= 1.0;
                let side = rng.side();
                let dir = (slip * Vec3::new(side * rng.range(-0.4, 1.0), rng.range(0.2, 1.4), rng.range(0.5, 1.4))).normalize();
                let w = rng.range(0.18, 0.28) * heat;
                let len = rng.range(3.5, 5.5);
                let at = rear(side, 0.15) + dir * w * len * 0.4;
                let mat = if rng.f() < 0.35 { &fx.hot[stage] } else { &fx.streak[stage] };
                let style = Style { shape: Shape::Spike(len), parent: Some(entity), dir: Some(-dir), ..default() };
                let p = Spawn { mesh: &fx.shard, mat, pos: at, vel: Vec3::ZERO, life: rng.range(0.05, 0.1), size: (w, w * 0.5), gravity: 0.0, drag: 0.0 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            // The glow moves with the kart for its brief life, so it stays on the wheel.
            a[9] += dt * 50.0 * near;
            while a[9] >= 1.0 {
                a[9] -= 1.0;
                let side = rng.side();
                let s = rng.range(1.6, 2.4) * heat;
                let style = Style { shape: Shape::Billboard, ..default() };
                let p = Spawn { mesh: &fx.quad, mat: &fx.flare[stage], pos: local(rear(side, 0.35)), vel: kvel, life: rng.range(0.06, 0.1), size: (s, s * 0.8), gravity: 0.0, drag: 0.0 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            a[1] += dt * 9.0 * near;
        }
        // Stage up: a flash and a burst of sparks from both wheels in the new colour.
        for ev in events.iter().filter(|e| e.kart == kv.index) {
            let FxKind::DriftStage(stage) = ev.kind else { continue };
            let stage = stage.min(3);
            for i in 0..70 {
                let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                let vel = slip * Vec3::new(side * rng.range(-1.0, 8.0), rng.range(2.0, 9.0), rng.range(2.0, 14.0));
                let s = rng.range(0.09, 0.15);
                let style = Style { shape: Shape::Streak(0.06), floor: Some((0.03, 0.45)), parent: Some(entity), ..default() };
                let p = Spawn { mesh: &fx.shard, mat: &fx.hot[stage], pos: rear(side, 0.2), vel, life: rng.range(0.3, 0.55), size: (s, s * 0.3), gravity: 22.0, drag: 1.2 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            for side in [-1.0, 1.0] {
                let style = Style { shape: Shape::Billboard, ..default() };
                let p = Spawn { mesh: &fx.quad, mat: &fx.flare[stage], pos: local(rear(side, 0.4)), vel: kvel, life: 0.18, size: (4.0, 1.5), gravity: 0.0, drag: 0.0 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
        }
        // Tyre smoke from a drift: soft puffs left behind on the road.
        while a[1] >= 1.0 {
            a[1] -= 1.0;
            let side = rng.side();
            let vel = kvel * 0.25 + back * rng.range(0.5, 2.0) + up * rng.range(0.6, 1.4) + rng.v3() * 0.4;
            let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Smoke), ..default() };
            let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Smoke as usize][0], pos: local(slip * Vec3::new(side * 1.0, 0.35, 1.3)), vel, life: rng.range(0.7, 1.0), size: (0.6, 2.2), gravity: -0.6, drag: 2.5 };
            spawn_styled(&mut commands, &mut count, p, style, &mut rng);
        }

        // Dust when driving on the grass.
        if k.offroad && speed > 5.0 {
            a[2] += dt * (8.0 + speed * 0.8);
            while a[2] >= 1.0 {
                a[2] -= 1.0;
                let side = if rng.f() < 0.5 { -0.9 } else { 0.9 };
                let vel = back * rng.range(1.0, 4.0) + Vec3::Y * rng.range(1.0, 2.5) + rng.v3();
                spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.dust, pos: local(Vec3::new(side, 0.2, 1.1)), vel, life: 0.7, size: (0.4, 1.5), gravity: 1.5, drag: 2.5 }, &mut rng);
            }
        }

        // Splashing through a ford: rooster tails thrown up by the rear wheels, curtains of
        // spray peeling off the front ones, mist, and a foam wake spreading out behind.
        if k.in_water && speed > 4.0 {
            let pace = (speed / 30.0).clamp(0.3, 1.4);
            let water = pos.y + 0.22;
            let at = |x: f32, z: f32| {
                let p = local(slip * Vec3::new(x, 0.0, z));
                Vec3::new(p.x, water, p.z)
            };
            let drop = Style { shape: Shape::Streak(0.022), floor: Some((water, 0.0)), ramp: Some(Ramp::Drop), ..default() };
            let drop_mat = &fx.ramps[Ramp::Drop as usize][RAMP_STEPS - 1];
            a[4] += dt * 150.0 * pace * near;
            while a[4] >= 1.0 {
                a[4] -= 1.0;
                let side = rng.side();
                let vel = kvel * rng.range(0.72, 0.92) + up * rng.range(4.0, 10.0) * pace + back * rng.range(0.0, 2.5) + right * side * rng.range(0.2, 3.5);
                let s = rng.range(0.09, 0.16);
                let p = Spawn { mesh: &fx.shard, mat: drop_mat, pos: at(side * 0.98, 1.1) + Vec3::Y * 0.1, vel, life: rng.range(0.5, 0.9), size: (s, s * 0.6), gravity: 18.0, drag: 0.6 };
                spawn_styled(&mut commands, &mut count, p, drop, &mut rng);
            }
            a[6] += dt * 100.0 * pace * near;
            while a[6] >= 1.0 {
                a[6] -= 1.0;
                let side = rng.side();
                let vel = kvel * rng.range(0.8, 0.95) + right * side * rng.range(2.5, 6.5) * pace + up * rng.range(1.5, 4.5) * pace;
                let s = rng.range(0.08, 0.14);
                let p = Spawn { mesh: &fx.shard, mat: drop_mat, pos: at(side * 1.0, -1.1) + Vec3::Y * 0.05, vel, life: rng.range(0.35, 0.6), size: (s, s * 0.6), gravity: 18.0, drag: 0.6 };
                spawn_styled(&mut commands, &mut count, p, drop, &mut rng);
            }
            a[7] += dt * 40.0 * pace * near;
            while a[7] >= 1.0 {
                a[7] -= 1.0;
                let side = rng.side();
                let vel = kvel * rng.range(0.6, 0.8) + up * rng.range(2.5, 5.5) * pace + right * side * rng.range(0.5, 2.5) + back;
                let s = rng.range(0.7, 1.1);
                let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Mist), ..default() };
                let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Mist as usize][0], pos: at(side * 1.0, 1.3) + Vec3::Y * 0.4, vel, life: rng.range(0.55, 0.85), size: (s, s * 3.5), gravity: 2.0, drag: 1.6 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            a[8] += dt * 20.0 * near;
            while a[8] >= 1.0 {
                a[8] -= 1.0;
                let side = rng.side();
                let vel = right * side * rng.range(1.0, 2.5) + kvel * 0.05;
                let style = Style { shape: Shape::Flat, ramp: Some(Ramp::Foam), ..default() };
                let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Foam as usize][0], pos: at(side * rng.range(0.6, 1.1), 1.0) + Vec3::Y * 0.02, vel, life: rng.range(1.2, 1.6), size: (1.2, 4.5), gravity: 0.0, drag: 1.0 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
        }

        // The zoomies: a stream of bright sparkles off the whole kart.
        if k.zoom > 0.0 {
            a[10] += dt * 50.0 * near;
            while a[10] >= 1.0 {
                a[10] -= 1.0;
                let at = local(Vec3::new(rng.s() * 1.1, rng.range(0.3, 1.7), rng.s() * 1.4));
                let vel = kvel * 0.6 + up * rng.range(0.5, 3.0) + rng.v3() * 1.5;
                let mat = &kit.confetti[(rng.f() * kit.confetti.len() as f32) as usize % kit.confetti.len()];
                spawn(&mut commands, &mut count, Spawn { mesh: cube, mat, pos: at, vel, life: rng.range(0.35, 0.6), size: (0.26, 0.03), gravity: 3.0, drag: 1.5 }, &mut rng);
                // And a glow about it that flickers from colour to colour.
                if rng.f() < 0.4 {
                    let s = rng.range(3.5, 5.0);
                    let style = Style { shape: Shape::Billboard, ..default() };
                    let p = Spawn { mesh: &fx.quad, mat: &fx.flare[1 + (rng.f() * 3.0) as usize % 3], pos: local(Vec3::new(0.0, 0.9, 0.0)), vel: kvel, life: rng.range(0.08, 0.14), size: (s, s * 0.8), gravity: 0.0, drag: 0.0 };
                    spawn_styled(&mut commands, &mut count, p, style, &mut rng);
                }
            }
        }
        // Rain from the cloud overhead.
        if k.cloud > 0.0 {
            a[11] += dt * 45.0 * near;
            while a[11] >= 1.0 {
                a[11] -= 1.0;
                let at = pos + Vec3::new(rng.s() * 1.1, 3.0 * k.size(), rng.s() * 1.1);
                let s = rng.range(0.06, 0.1);
                let style = Style { shape: Shape::Streak(0.022), floor: Some((pos.y + 0.05, 0.0)), ramp: Some(Ramp::Drop), ..default() };
                let p = Spawn { mesh: &fx.shard, mat: &fx.ramps[Ramp::Drop as usize][RAMP_STEPS - 1], pos: at, vel: kvel * 0.95 - up * 3.0, life: 0.6, size: (s, s * 0.6), gravity: 18.0, drag: 0.3 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
        }

        // Boost flames trailing from the exhausts.
        if k.is_boosting() || k.chase > 0.0 {
            a[3] += dt * 45.0;
            while a[3] >= 1.0 {
                a[3] -= 1.0;
                let x = if rng.f() < 0.5 { -0.3 } else { 0.3 };
                let vel = back * rng.range(6.0, 10.0) + rng.v3() * 1.2;
                spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.flame, pos: local(Vec3::new(x, 0.8, 2.1)), vel, life: 0.22, size: (0.32, 0.04), gravity: -2.0, drag: 3.0 }, &mut rng);
            }
        }
    }

    // Fireworks over Prism Park.
    if race.track.theme == crate::sim::Theme::Prism {
        *fireworks -= dt;
        if *fireworks <= 0.0 {
            *fireworks = rng.range(0.6, 1.8);
            // Somewhere over one of the local players.
            let n = race.locals.len();
            let who = if n > 1 { race.locals[(rng.f() * n as f32) as usize % n] } else { race.locals[0] };
            let me = race.world.karts[who].pos;
            let at = Vec3::new(me.x + rng.s() * 160.0, rng.range(55.0, 95.0), me.y + rng.s() * 160.0);
            let mat = &kit.confetti[(rng.f() * kit.confetti.len() as f32) as usize % kit.confetti.len()];
            for _ in 0..40 {
                let vel = rng.v3().normalize_or(Vec3::Y) * rng.range(14.0, 20.0);
                spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat, pos: at, vel, life: rng.range(1.2, 1.8), size: (0.9, 0.1), gravity: 4.0, drag: 1.4 }, &mut rng);
            }
        }
    }

    for ev in &events {
        let p = ev.pos;
        match ev.kind {
            FxKind::Hit => {
                for k in 0..14 {
                    let a = k as f32 / 14.0 * std::f32::consts::TAU;
                    let vel = Vec3::new(a.cos() * 7.0, rng.range(5.0, 9.0), a.sin() * 7.0);
                    spawn(&mut commands, &mut count, Spawn { mesh: cube, mat: &kit.star, pos: p + Vec3::Y * 1.2, vel, life: 0.9, size: (0.35, 0.05), gravity: 12.0, drag: 1.5 }, &mut rng);
                }
                for _ in 0..8 {
                    let vel = rng.v3() * 3.0 + Vec3::Y * 2.0;
                    spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.smoke, pos: p + Vec3::Y * 0.6, vel, life: 1.0, size: (0.8, 2.4), gravity: -0.4, drag: 2.0 }, &mut rng);
                }
            }
            FxKind::BoxBreak => {
                for _ in 0..22 {
                    let vel = rng.v3() * 6.0 + Vec3::Y * 5.0;
                    let mat = &kit.confetti[(rng.f() * kit.confetti.len() as f32) as usize % kit.confetti.len()];
                    spawn(&mut commands, &mut count, Spawn { mesh: &kit.flake, mat, pos: p, vel, life: 1.1, size: (0.16, 0.06), gravity: 9.0, drag: 1.8 }, &mut rng);
                }
            }
            // Hitting the water: a crown of droplets, a burst of mist and a ring of foam.
            FxKind::Splash => {
                let Some((_, tr)) = roots.get(ev.kart).copied().flatten() else { continue };
                let k = &race.world.karts[ev.kart];
                let kvel = tr.rotation * Vec3::NEG_Z * k.speed;
                let water = tr.translation.y + 0.22;
                let centre = Vec3::new(tr.translation.x, water, tr.translation.z);
                let drop = Style { shape: Shape::Streak(0.022), floor: Some((water, 0.0)), ramp: Some(Ramp::Drop), ..default() };
                for i in 0..70 {
                    let a = i as f32 / 70.0 * std::f32::consts::TAU + rng.s() * 0.1;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let vel = kvel * 0.6 + out * rng.range(2.5, 7.0) + Vec3::Y * rng.range(4.0, 10.0);
                    let s = rng.range(0.07, 0.14);
                    let p = Spawn { mesh: &fx.shard, mat: &fx.ramps[Ramp::Drop as usize][RAMP_STEPS - 1], pos: centre + out * 1.2 + Vec3::Y * 0.1, vel, life: rng.range(0.7, 1.1), size: (s, s * 0.6), gravity: 18.0, drag: 0.5 };
                    spawn_styled(&mut commands, &mut count, p, drop, &mut rng);
                }
                for i in 0..14 {
                    let a = i as f32 / 14.0 * std::f32::consts::TAU;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let vel = kvel * 0.5 + out * rng.range(1.5, 4.0) + Vec3::Y * rng.range(2.0, 5.0);
                    let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Mist), ..default() };
                    let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Mist as usize][0], pos: centre + out + Vec3::Y * 0.5, vel, life: rng.range(0.7, 1.0), size: (1.0, 3.8), gravity: 2.0, drag: 1.5 };
                    spawn_styled(&mut commands, &mut count, p, style, &mut rng);
                }
                let style = Style { shape: Shape::Flat, ramp: Some(Ramp::Foam), ..default() };
                let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Foam as usize][0], pos: centre + Vec3::Y * 0.03, vel: kvel * 0.3, life: 1.6, size: (2.5, 9.0), gravity: 0.0, drag: 1.5 };
                spawn_styled(&mut commands, &mut count, p, style, &mut rng);
            }
            // A trick: a ring of stars bursting out round the kart and a flash of sparks.
            FxKind::Trick => {
                let Some((_, tr)) = roots.get(ev.kart).copied().flatten() else { continue };
                let k = &race.world.karts[ev.kart];
                let kvel = tr.rotation * Vec3::NEG_Z * k.speed;
                let centre = tr.translation + Vec3::Y * 0.8;
                for i in 0..12 {
                    let a = i as f32 / 12.0 * std::f32::consts::TAU;
                    let out = tr.rotation * Vec3::new(a.cos(), a.sin() * 0.6 + 0.4, a.sin() * 0.3);
                    let vel = kvel * 0.9 + out * rng.range(5.0, 7.0);
                    spawn(&mut commands, &mut count, Spawn { mesh: cube, mat: &kit.star, pos: centre + out * 0.8, vel, life: 0.6, size: (0.3, 0.04), gravity: 2.0, drag: 2.5 }, &mut rng);
                }
                for _ in 0..40 {
                    let vel = kvel * 0.85 + rng.v3().normalize_or(Vec3::Y) * rng.range(4.0, 9.0);
                    let s = rng.range(0.07, 0.12);
                    let style = Style { shape: Shape::Streak(0.08), ..default() };
                    let p = Spawn { mesh: &fx.shard, mat: &fx.hot[0], pos: centre, vel, life: rng.range(0.25, 0.45), size: (s, s * 0.3), gravity: 6.0, drag: 2.0 };
                    spawn_styled(&mut commands, &mut count, p, style, &mut rng);
                }
            }
            // Touching down: dust puffs out from under the wheels.
            FxKind::Land => {
                let Some((_, tr)) = roots.get(ev.kart).copied().flatten() else { continue };
                let k = &race.world.karts[ev.kart];
                let kvel = tr.rotation * Vec3::NEG_Z * k.speed;
                for i in 0..10 {
                    let a = i as f32 / 10.0 * std::f32::consts::TAU;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let vel = kvel * 0.4 + out * rng.range(3.0, 5.0) + Vec3::Y * rng.range(0.5, 1.5);
                    let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Smoke), ..default() };
                    let p = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Smoke as usize][0], pos: tr.translation + out * 1.2 + Vec3::Y * 0.3, vel, life: rng.range(0.5, 0.8), size: (0.8, 2.4), gravity: -0.3, drag: 3.0 };
                    spawn_styled(&mut commands, &mut count, p, style, &mut rng);
                }
            }
            // A balloon or frisbee going off: water thrown out as far as it reaches, and mist.
            FxKind::Burst(Item::WaterBalloon | Item::Frisbee) => {
                let reach = crate::sim::world::BLAST_RADIUS;
                let drop = Style { shape: Shape::Streak(0.022), floor: Some((p.y, 0.0)), ramp: Some(Ramp::Drop), ..default() };
                for i in 0..80 {
                    let a = i as f32 / 80.0 * std::f32::consts::TAU + rng.s() * 0.1;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let vel = out * rng.range(2.0, reach * 1.3) + Vec3::Y * rng.range(5.0, 13.0);
                    let s = rng.range(0.1, 0.2);
                    let sp = Spawn { mesh: &fx.shard, mat: &fx.ramps[Ramp::Drop as usize][RAMP_STEPS - 1], pos: p + out * 0.5 + Vec3::Y * 0.4, vel, life: rng.range(0.7, 1.2), size: (s, s * 0.6), gravity: 20.0, drag: 0.4 };
                    spawn_styled(&mut commands, &mut count, sp, drop, &mut rng);
                }
                for i in 0..14 {
                    let a = i as f32 / 14.0 * std::f32::consts::TAU;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let vel = out * rng.range(2.0, reach) + Vec3::Y * rng.range(2.0, 6.0);
                    let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Mist), ..default() };
                    let sp = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Mist as usize][0], pos: p + out + Vec3::Y * 0.8, vel, life: rng.range(0.8, 1.2), size: (1.5, 5.0), gravity: 1.5, drag: 1.6 };
                    spawn_styled(&mut commands, &mut count, sp, style, &mut rng);
                }
                let style = Style { shape: Shape::Flat, ramp: Some(Ramp::Foam), ..default() };
                let sp = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Foam as usize][0], pos: p + Vec3::Y * 0.05, vel: Vec3::ZERO, life: 1.6, size: (3.0, reach * 2.2), gravity: 0.0, drag: 1.5 };
                spawn_styled(&mut commands, &mut count, sp, style, &mut rng);
            }
            // The whistle: a ring of white streaks racing out across the road.
            FxKind::Burst(Item::DogWhistle) => {
                for i in 0..64 {
                    let a = i as f32 / 64.0 * std::f32::consts::TAU;
                    let out = Vec3::new(a.cos(), 0.0, a.sin());
                    let s = rng.range(0.12, 0.2);
                    let style = Style { shape: Shape::Streak(0.05), ..default() };
                    let sp = Spawn { mesh: &fx.shard, mat: &fx.hot[0], pos: p + out * 1.5 + Vec3::Y * rng.range(0.6, 1.8), vel: out * rng.range(45.0, 60.0), life: rng.range(0.5, 0.8), size: (s, s * 0.3), gravity: 0.0, drag: 0.6 };
                    spawn_styled(&mut commands, &mut count, sp, style, &mut rng);
                }
            }
            // A big woof landing: the ground jumps under everyone ahead who's still on it.
            FxKind::WoofSlam => {
                let place = race.world.karts[ev.kart].place;
                for (_, tr) in roots.iter().flatten().zip(&race.world.karts).filter(|(_, k)| k.place < place && k.spin > 0.0).map(|(r, _)| r) {
                    for i in 0..12 {
                        let a = i as f32 / 12.0 * std::f32::consts::TAU;
                        let out = Vec3::new(a.cos(), 0.0, a.sin());
                        let style = Style { shape: Shape::Billboard, ramp: Some(Ramp::Smoke), ..default() };
                        let sp = Spawn { mesh: &fx.quad, mat: &fx.ramps[Ramp::Smoke as usize][0], pos: tr.translation + out * 1.4 + Vec3::Y * 0.3, vel: out * rng.range(5.0, 8.0) + Vec3::Y, life: rng.range(0.6, 0.9), size: (1.0, 3.2), gravity: -0.3, drag: 3.0 };
                        spawn_styled(&mut commands, &mut count, sp, style, &mut rng);
                    }
                }
            }
            FxKind::Shrunk | FxKind::Squashed => {
                for _ in 0..10 {
                    let vel = rng.v3() * 3.5 + Vec3::Y * 2.0;
                    spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.smoke, pos: p + Vec3::Y * 0.8, vel, life: 0.8, size: (0.7, 2.0), gravity: -0.4, drag: 2.0 }, &mut rng);
                }
            }
            // Mud flung up from the kart that threw it.
            FxKind::ItemUsed(Item::MudPie) => {
                for _ in 0..24 {
                    let vel = rng.v3() * 5.0 + Vec3::Y * rng.range(6.0, 11.0);
                    spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.mud, pos: p + Vec3::Y * 1.2, vel, life: 1.0, size: (0.3, 0.08), gravity: 16.0, drag: 0.8 }, &mut rng);
                }
            }
            FxKind::WallHit => {
                for _ in 0..10 {
                    let vel = rng.v3() * 5.0 + Vec3::Y * 4.0;
                    spawn(&mut commands, &mut count, Spawn { mesh: cube, mat: &kit.sparks[0], pos: p + Vec3::Y * 0.5, vel, life: 0.4, size: (0.12, 0.02), gravity: 14.0, drag: 1.0 }, &mut rng);
                }
            }
            FxKind::Boost if ev.local => {
                for _ in 0..10 {
                    let vel = rng.v3() * 4.0 + Vec3::Y * 1.5;
                    spawn(&mut commands, &mut count, Spawn { mesh: sphere, mat: &kit.flame, pos: p + Vec3::Y * 0.8, vel, life: 0.3, size: (0.5, 0.05), gravity: 0.0, drag: 3.0 }, &mut rng);
                }
            }
            FxKind::Finish(_) if ev.local => {
                for _ in 0..120 {
                    let vel = rng.v3() * 9.0 + Vec3::Y * 12.0;
                    let mat = &kit.confetti[(rng.f() * kit.confetti.len() as f32) as usize % kit.confetti.len()];
                    spawn(&mut commands, &mut count, Spawn { mesh: &kit.flake, mat, pos: p + Vec3::Y * 2.0, vel, life: 2.5, size: (0.22, 0.1), gravity: 7.0, drag: 1.2 }, &mut rng);
                }
            }
            _ => {}
        }
    }
}
