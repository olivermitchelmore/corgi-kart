//! Per-frame presentation of the running race: karts, item boxes, projectiles and the camera.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;

use super::hud::{spawn_hud, spawn_shared_hud};
use super::kart::*;
use super::models::*;
use super::scene::MainCamera;
use super::split::{spawn_views, view_percent, PlayerCamera, SplitScreen};
use crate::game::{RaceEntity, SessionRes, TrackRes};
use crate::session::RaceState;
use crate::sim::kart::{CHEER_TIME, CLOUD_TIME, HOP_TIME, MAX_SPEED, SPIN_TIME, TRICK_TIME};
use crate::sim::world::{orbit_pos, FRISBEE_HEIGHT, FRISBEE_SWOOP};
use crate::sim::track::{Ramp, RAMP_HEIGHT, RAMP_LENGTH};
use crate::sim::{wrap_angle, Item, COUNTDOWN, DT};

/// How long a thrown item takes to leave the corgi's paw.
const THROW_TIME: f32 = 0.3;
/// How long a corgi looks over its shoulder after throwing or dropping an item behind it.
const GLANCE_TIME: f32 = 0.5;
/// How long a corgi's throwing leg takes to swing and get back to the wheel.
const SWING_TIME: f32 = 0.5;
/// Where a thrown item leaves the paw, in the kart's frame.
const PAW: Vec3 = Vec3::new(0.6, 1.6, 0.0);

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How long ago kart `i` last threw or dropped an item that's still around, and whether it
/// went backwards.
fn last_throw(race: &RaceState, i: usize) -> Option<(f32, bool)> {
    let w = &race.world;
    let k = w.karts.get(i)?;
    let thrown = w.projectiles.iter().filter(|p| p.owner as usize == i).map(|p| (p.age, p.vel.dot(k.dir()) < 0.0));
    // A toy still in the air was lobbed forwards; one lying still was dropped behind (or landed
    // long enough ago not to matter).
    let toys = w.traps.iter().filter(|t| t.owner as usize == i).map(|t| (t.age, t.flight <= 0.0));
    let (age, back) = thrown.chain(toys).min_by(|a, b| a.0.total_cmp(&b.0))?;
    Some((age + race.alpha() * DT, back))
}

/// How a corgi is holding itself: throwing, celebrating a hit, or just driving.
struct CorgiPose {
    /// The throwing leg, about its shoulder, and how far it stretches as it's raised.
    arm: Quat,
    reach: f32,
    /// Head turn (to look behind) and tilt (chin up), and how far it's lifted.
    head_turn: f32,
    head_tilt: f32,
    head_lift: f32,
}

fn corgi_pose(race: &RaceState, i: usize) -> CorgiPose {
    let mut pose = CorgiPose { arm: Quat::IDENTITY, reach: 1.0, head_turn: 0.0, head_tilt: 0.0, head_lift: 0.0 };
    // How far the leg is raised from the wheel: straight up at a quarter turn, back past that.
    let mut raise = 0.0;
    let throw = last_throw(race, i).filter(|t| t.0 < SWING_TIME.max(GLANCE_TIME));
    if let Some((age, back)) = throw {
        let p = age / SWING_TIME;
        raise = match back {
            // Over the shoulder, then back round to the wheel.
            true if p < 0.4 => 1.2 + 1.1 * smooth(p / 0.4),
            true => 2.3 * (1.0 - smooth((p - 0.4) / 0.6)),
            // The item leaves the paw at the top; the leg whips down after it.
            false => 2.1 * (1.0 - smooth(p / 0.45)),
        };
        if back && age < GLANCE_TIME {
            pose.head_turn = (PI * age / GLANCE_TIME).sin().powf(0.6) * 2.3;
        }
    } else if let Some(k) = race.world.karts.get(i).filter(|k| k.cheer > 0.0) {
        // "Yes!": chin up and a paw punching the air a few times.
        let p = 1.0 - (k.cheer - race.alpha() * DT).max(0.0) / CHEER_TIME;
        let on = smooth(p / 0.15) * smooth((1.0 - p) / 0.25);
        let pump = (p * 3.0 * std::f32::consts::TAU).sin();
        raise = on * (1.75 + 0.3 * pump);
        pose.head_tilt = on * (0.4 + 0.1 * pump);
        pose.head_lift = on * 0.05 * (1.0 + pump);
    }
    // Raised, it swings out past the corgi's ear rather than through its head.
    let up = raise.sin().max(0.0);
    pose.arm = Quat::from_rotation_z(-0.65 * up) * Quat::from_rotation_x(raise);
    pose.reach = 1.0 + 0.2 * up;
    pose
}

#[derive(Resource, Default)]
pub struct RaceScene {
    pub race_id: Option<u32>,
    projectiles: HashMap<u16, Entity>,
    traps: HashMap<u16, Entity>,
    /// What goes round with the karts, by kart and slot (see `ExtraVis`), and which item it is.
    extras: HashMap<(usize, u8), (Entity, Item)>,
}

pub fn reset_scene(mut scene: ResMut<RaceScene>) {
    *scene = RaceScene::default();
}

fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Bevy yaw for a sim heading (models face -Z).
pub fn yaw(heading: f32) -> Quat {
    Quat::from_rotation_y(-heading - FRAC_PI_2)
}

/// Interpolated position and heading of kart `i` between the last two ticks.
pub fn kart_pose(race: &RaceState, i: usize) -> (Vec2, f32) {
    let a = race.alpha();
    let (p, c) = (&race.prev.karts[i], &race.world.karts[i]);
    let pos = if p.pos.distance(c.pos) > 10.0 { c.pos } else { p.pos.lerp(c.pos, a) };
    (pos, p.heading + wrap_angle(c.heading - p.heading) * a)
}

fn hash(x: f32) -> f32 {
    ((x * 12.9898).sin() * 43758.547).fract().abs()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_race_scene(
    mut commands: Commands,
    session: Option<Res<SessionRes>>,
    mut scene: ResMut<RaceScene>,
    kit: Res<Kit>,
    gfx: Res<super::gfx::Graphics>,
    icon: Res<super::icons::ItemIcon>,
    split: Option<Res<SplitScreen>>,
    mut images: ResMut<Assets<Image>>,
    main: Query<(Entity, &Camera, Option<&bevy::camera::RenderTarget>, &DistanceFog, &GeneratedEnvironmentMapLight), With<MainCamera>>,
    existing: Query<Entity, With<RaceEntity>>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    if scene.race_id == Some(race.race_id) {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    *scene = RaceScene { race_id: Some(race.race_id), ..default() };
    for i in 0..race.world.karts.len() {
        // Humans get the classic red corgi; CPUs mix in sable and tri-colour coats.
        let coat = if race.roster[i].bot { i % 3 } else { 0 };
        spawn_kart(&mut commands, &kit, i, coat, gfx.raytracing);
    }
    for (i, b) in race.world.boxes.iter().enumerate() {
        spawn_item_box(&mut commands, &kit, i, v3(b.pos, 1.3 + race.track.height_idx(b.track_idx as usize)));
    }
    let Ok(main) = main.single() else { return };
    let players = race.locals.len();
    let ui = spawn_views(&mut commands, &mut images, players, *gfx, main, split.as_deref());
    for p in 0..players {
        spawn_hud(&mut commands, race, &icon.images[p], p, ui, view_percent(p, players));
    }
    spawn_shared_hud(&mut commands, race, ui);
}

/// Road height under a kart, and the slope along its heading (rise per metre).
pub fn kart_ground(race: &RaceState, i: usize, pos: Vec2, heading: f32) -> (f32, f32) {
    let track = &race.track;
    let k = &race.world.karts[i];
    let s = track.locate(pos, k.track_idx as usize).s;
    let h = track.height_at(s);
    let (_, dir) = track.point_at(s);
    let along = Vec2::from_angle(heading).dot(dir);
    let slope = (track.height_at(s + 1.5) - track.height_at(s - 1.5)) / 3.0 * along;
    (h, slope)
}

/// How high kart `i` is above the road (on a ramp or in the air), and how far its nose is
/// pitched up by the ramp or its climb (rise per metre).
pub fn kart_lift(race: &RaceState, i: usize, pos: Vec2, heading: f32) -> (f32, f32) {
    let (p, k) = (&race.prev.karts[i], &race.world.karts[i]);
    if k.airborne || p.airborne {
        let air = p.air + (k.air - p.air) * race.alpha();
        let climb = if k.airborne { (k.vz / k.speed.abs().max(8.0)).clamp(-0.5, 0.6) * 0.6 } else { 0.0 };
        return (air.max(0.0), climb);
    }
    let tp = race.track.locate(pos, k.track_idx as usize);
    match race.track.ramp_at(tp.s, tp.lateral) {
        Some((_, x)) => (Ramp::height(x), 2.0 * RAMP_HEIGHT * x / (RAMP_LENGTH * RAMP_LENGTH) * Vec2::from_angle(heading).dot(tp.dir)),
        None => (0.0, 0.0),
    }
}

pub fn animate_karts(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    mut roots: Query<(&KartVis, &mut Transform)>,
    mut bodies: Query<(&KartBody, &mut Transform), Without<KartVis>>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let t = time.elapsed_secs();
    let ease = 1.0 - (-10.0 * time.delta_secs()).exp();
    for (kv, mut tr) in &mut roots {
        let Some(k) = race.world.karts.get(kv.index) else { continue };
        let (pos, heading) = kart_pose(race, kv.index);
        let (h, slope) = kart_ground(race, kv.index, pos, heading);
        let (air, pitch) = kart_lift(race, kv.index, pos, heading);
        tr.translation = v3(pos, h + air);
        tr.rotation = yaw(heading) * Quat::from_rotation_x(slope.atan() + pitch.atan());
        // Huge, cowering or flattened: grow and shrink into it rather than popping.
        let size = k.size();
        let want = if k.flat > 0.0 { Vec3::new(size * 1.2, size * 0.25, size * 1.2) } else { Vec3::splat(size) };
        if tr.scale != want {
            tr.scale = if tr.scale.distance_squared(want) < 1e-4 { want } else { tr.scale.lerp(want, ease) };
        }
    }
    for (kb, mut tr) in &mut bodies {
        let Some(k) = race.world.karts.get(kb.index) else { continue };
        let hop = if k.hop > 0.0 { (PI * (1.0 - k.hop / HOP_TIME)).sin() * 0.45 } else { 0.0 };
        let rumble = if k.offroad && k.speed.abs() > 3.0 { (t * 40.0 + kb.index as f32).sin() * 0.03 } else { 0.0 };
        let spin = if k.spin > 0.0 { (SPIN_TIME - k.spin) / SPIN_TIME * PI * 4.0 } else { 0.0 };
        let lean = k.steer * (k.speed / MAX_SPEED).clamp(0.0, 1.2) * 0.12;
        let pitch = if k.is_boosting() { 0.06 } else { 0.0 };
        // A trick: a front flip, or a barrel roll the way you were steering, about the kart's
        // middle, with a little pop upwards.
        let (trick, pop) = if k.trick > 0.0 {
            let p = 1.0 - k.trick / TRICK_TIME;
            let turn = p * p * (3.0 - 2.0 * p) * std::f32::consts::TAU;
            let rot = match k.trick_style {
                0 => Quat::from_rotation_x(-turn),
                s => Quat::from_rotation_z(-s as f32 * turn),
            };
            (rot, (PI * p).sin() * 0.5)
        } else {
            (Quat::IDENTITY, 0.0)
        };
        let middle = Vec3::Y * 0.7;
        tr.translation = Vec3::new(0.0, hop + rumble + pop, 0.0) + middle - trick * middle;
        tr.rotation = trick * Quat::from_rotation_y(-k.slip - spin) * Quat::from_rotation_z(lean) * Quat::from_rotation_x(pitch);
    }
}

pub fn animate_kart_parts(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    mut wheels: Query<(&mut Wheel, &mut Transform)>,
    mut pivots: Query<(&FrontPivot, &mut Transform), Without<Wheel>>,
    mut heads: Query<(&CorgiHead, &mut Transform), (Without<Wheel>, Without<FrontPivot>)>,
    mut arms: Query<(&CorgiArm, &mut Transform), (Without<Wheel>, Without<FrontPivot>, Without<CorgiHead>)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut w, mut tr) in &mut wheels {
        let Some(k) = race.world.karts.get(w.index) else { continue };
        w.angle = (w.angle - k.speed * dt / w.radius) % (PI * 2.0);
        tr.rotation = Quat::from_rotation_x(w.angle) * w.base;
    }
    for (p, mut tr) in &mut pivots {
        let Some(k) = race.world.karts.get(p.index) else { continue };
        tr.rotation = Quat::from_rotation_y(-k.steer * 0.45);
    }
    for (h, mut tr) in &mut heads {
        let Some(k) = race.world.karts.get(h.index) else { continue };
        let bob = (t * 11.0 + h.index as f32).sin() * 0.025 * (k.speed.abs() / MAX_SPEED).min(1.0);
        let dizzy = if k.spin > 0.0 { (t * 14.0).sin() * 0.35 } else { 0.0 };
        let pose = corgi_pose(race, h.index);
        tr.translation.y = 1.0 + bob + pose.head_lift;
        tr.rotation = Quat::from_rotation_y(-k.steer * 0.3 + dizzy + pose.head_turn)
            * Quat::from_rotation_z(k.steer * 0.15)
            * Quat::from_rotation_x(pose.head_tilt);
    }
    for (a, mut tr) in &mut arms {
        let pose = corgi_pose(race, a.index);
        tr.rotation = pose.arm;
        tr.scale = Vec3::splat(pose.reach);
    }
}

/// Brake lights flare when a kart slows down (or is stopped), and stay lit briefly.
pub fn animate_brake_lights(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    mut hold: Local<Vec<f32>>,
    mut lights: Query<(&BrakeLight, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let n = race.world.karts.len();
    hold.resize(n, 0.0);
    for (i, h) in hold.iter_mut().enumerate() {
        let (now, was) = (&race.world.karts[i], &race.prev.karts[i]);
        let decel = (was.speed - now.speed) / DT;
        let braking = (decel > 8.0 && now.speed > 1.0) || now.speed.abs() < 0.5 || now.spin > 0.0;
        *h = if braking { 0.35 } else { (*h - time.delta_secs()).max(0.0) };
    }
    for (b, mut mat) in &mut lights {
        let on = hold.get(b.index).is_some_and(|&h| h > 0.0);
        let want = if on { &kit.brake_on } else { &kit.brake_dim };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

pub fn animate_effects(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    mut flames: Query<(&Flame, &mut Transform, &mut Visibility)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let t = time.elapsed_secs();
    let frame = (t * 30.0).floor();
    for (f, mut tr, mut vis) in &mut flames {
        let Some(k) = race.world.karts.get(f.index) else { continue };
        let on = k.is_boosting() || k.chase > 0.0;
        *vis = if on { Visibility::Visible } else { Visibility::Hidden };
        let flicker = 0.7 + 0.5 * hash(frame + f.index as f32 * 3.3 + tr.translation.x);
        tr.scale = Vec3::new(0.3, 0.8 * flicker, 0.3);
    }
}

pub fn animate_boxes(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut boxes: Query<(&BoxVis, &mut Transform, &mut Visibility)>,
    mut cores: Query<&mut Transform, (With<BoxCore>, Without<BoxVis>)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let t = time.elapsed_secs();
    for (bv, mut tr, mut vis) in &mut boxes {
        let Some(b) = race.world.boxes.get(bv.index) else { continue };
        let grow = (1.0 - b.respawn / 0.4).clamp(0.0, 1.0);
        *vis = if grow > 0.0 { Visibility::Visible } else { Visibility::Hidden };
        tr.scale = Vec3::splat(grow);
        tr.translation.y = race.track.height_idx(b.track_idx as usize) + 1.4 + (t * 2.0 + bv.index as f32).sin() * 0.2;
        tr.rotation = Quat::from_rotation_y(t * 1.2 + bv.index as f32) * Quat::from_rotation_x(0.4);
    }
    for mut tr in &mut cores {
        tr.rotation = Quat::from_rotation_y(-t * 2.5) * Quat::from_rotation_z(t * 1.7);
    }
    if let Some(mut m) = mats.get_mut(&kit.box_core) {
        m.emissive = Color::hsl((t * 90.0) % 360.0, 1.0, 0.5).to_linear() * 4.0;
    }
}

pub fn sync_items(
    mut commands: Commands,
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    mut scene: ResMut<RaceScene>,
    mut projs: Query<(&ProjVis, &mut Transform)>,
    mut traps: Query<(&TrapVis, &mut Transform), Without<ProjVis>>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    if scene.race_id != Some(race.race_id) {
        return;
    }
    let t = time.elapsed_secs();
    let w = &race.world;
    let lead = race.alpha() * DT;

    scene.projectiles.retain(|id, e| {
        let keep = w.projectiles.iter().any(|p| p.id == *id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for p in &w.projectiles {
        if !scene.projectiles.contains_key(&p.id) {
            let e = spawn_projectile(&mut commands, &kit, p.id, p.kind);
            scene.projectiles.insert(p.id, e);
        }
    }
    for (pv, mut tr) in &mut projs {
        let Some(p) = w.projectiles.iter().find(|p| p.id == pv.id) else { continue };
        let pos = p.pos + p.vel * lead;
        let ground = race.track.height_idx(p.track_idx as usize);
        if p.kind == Item::Bone {
            tr.translation = v3(pos, ground + 0.8);
            tr.rotation = Quat::from_rotation_y(t * 14.0);
        } else if p.kind == Item::Frisbee {
            // High over the road, coming down as it closes on the leader.
            let near = w.karts.get(p.target.max(0) as usize).map_or(1.0, |k| (k.pos.distance(pos) / FRISBEE_SWOOP).min(1.0));
            tr.translation = v3(pos, ground + 1.0 + (FRISBEE_HEIGHT - 1.0) * near);
            let bank = v3(p.vel.perp(), 0.0).normalize_or(Vec3::X);
            tr.rotation = Quat::from_axis_angle(bank, 0.25) * Quat::from_rotation_y(t * 25.0);
        } else {
            tr.translation = v3(pos, ground + 0.55 + (t * 9.0).sin().abs() * 0.5);
            let axis = v3(p.vel.perp(), 0.0).normalize_or(Vec3::X);
            tr.rotation = Quat::from_axis_angle(axis, -t * 20.0);
        }
        // It leaves the corgi's paw in a little arc before it's away. Thrown backwards it would
        // otherwise be past the camera within a few frames, and never seen.
        let f = (p.age + lead) / THROW_TIME;
        let owner = p.owner as usize;
        if f < 1.0 && owner < w.karts.len() {
            let (kpos, heading) = kart_pose(race, owner);
            let road = kart_ground(race, owner, kpos, heading).0 + kart_lift(race, owner, kpos, heading).0;
            let from = v3(kpos, road + (PI * f).sin() * 0.7) + yaw(heading) * PAW;
            tr.translation = from.lerp(tr.translation, f * f);
        }
    }

    scene.traps.retain(|id, e| {
        let keep = w.traps.iter().any(|tr| tr.id == *id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for tp in &w.traps {
        if !scene.traps.contains_key(&tp.id) {
            let e = spawn_trap(&mut commands, &kit, tp.id, tp.kind);
            scene.traps.insert(tp.id, e);
        }
    }
    for (tv, mut tr) in &mut traps {
        let Some(tp) = w.traps.iter().find(|x| x.id == tv.id) else { continue };
        let ground = race.track.height_idx(tp.track_idx as usize);
        if tp.flight > 0.0 {
            // Tumbling through the air after a throw forwards.
            tr.translation = v3(tp.pos + tp.vel * lead, ground + tp.height());
            tr.scale = Vec3::ONE;
            let axis = v3(tp.vel.perp(), 0.0).normalize_or(Vec3::X);
            tr.rotation = Quat::from_axis_angle(axis, -tp.age * 12.0) * Quat::from_rotation_y(tv.id as f32);
            continue;
        }
        match tp.kind {
            // Floating and turning like a real box.
            Item::DecoyBox => {
                tr.translation = v3(tp.pos, ground + 1.4 + (t * 2.0 + tv.id as f32).sin() * 0.2);
                tr.scale = Vec3::ONE;
                tr.rotation = Quat::from_rotation_y(t * 1.2 + tv.id as f32) * Quat::from_rotation_x(0.4);
            }
            // A balloon swells and shivers faster as its time runs out.
            Item::WaterBalloon => {
                let urgency = 1.0 / (0.3 + tp.fuse.max(0.0));
                let squish = 1.0 + (t * (6.0 + 10.0 * urgency)).sin() * (0.05 + 0.05 * urgency).min(0.2);
                tr.translation = v3(tp.pos, ground);
                tr.scale = Vec3::new(1.0 / squish, squish, 1.0 / squish) * (1.0 + 0.1 * urgency.min(3.0));
                tr.rotation = Quat::from_rotation_y(tv.id as f32);
            }
            _ => {
                tr.translation = v3(tp.pos, ground);
                let squish = 1.0 + (t * 6.0 + tv.id as f32).sin() * 0.06;
                tr.scale = Vec3::new(1.0 / squish, squish, 1.0 / squish);
                tr.rotation = Quat::from_rotation_y(tv.id as f32);
            }
        }
    }
}

/// What goes round with the karts: three bones or balls circling one, the rain cloud over one,
/// the squirrel one is chasing. They come and go with the items, so they're only spawned while
/// they're wanted.
pub fn sync_kart_extras(
    mut commands: Commands,
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    mut scene: ResMut<RaceScene>,
    mut extras: Query<(&ExtraVis, &mut Transform)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    if scene.race_id != Some(race.race_id) {
        return;
    }
    let w = &race.world;
    let wanted = |kart: usize, slot: u8| -> Option<Item> {
        let k = w.karts.get(kart)?;
        match slot {
            CLOUD_SLOT => (k.cloud > 0.0).then_some(Item::RainCloud),
            SQUIRREL_SLOT => (k.chase > 0.0).then_some(Item::Squirrel),
            _ => (slot < k.orbiting()).then_some(k.item),
        }
    };
    scene.extras.retain(|&(kart, slot), (e, kind)| {
        let keep = wanted(kart, slot) == Some(*kind);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    // Nearly always, nobody has any of these.
    for (i, k) in w.karts.iter().enumerate() {
        if k.cloud <= 0.0 && k.chase <= 0.0 && k.orbiting() == 0 {
            continue;
        }
        for slot in 0..=SQUIRREL_SLOT {
            if let Some(kind) = wanted(i, slot).filter(|_| !scene.extras.contains_key(&(i, slot))) {
                scene.extras.insert((i, slot), (spawn_extra(&mut commands, &kit, i, slot, kind), kind));
            }
        }
    }
    let t = time.elapsed_secs();
    for (ev, mut tr) in &mut extras {
        let Some(k) = w.karts.get(ev.kart) else { continue };
        let (pos, heading) = kart_pose(race, ev.kart);
        let road = kart_ground(race, ev.kart, pos, heading).0;
        let lift = kart_lift(race, ev.kart, pos, heading).0;
        match ev.slot {
            CLOUD_SLOT => {
                // It billows up as its time runs out.
                let left = (k.cloud / CLOUD_TIME).clamp(0.0, 1.0);
                let swell = 1.0 + (1.0 - left) * 0.5 + if left < 0.3 { (t * 18.0).sin() * 0.06 } else { 0.0 };
                tr.translation = v3(pos, road + lift + 3.3 * k.size() + (t * 2.5).sin() * 0.12);
                tr.rotation = yaw(heading);
                tr.scale = Vec3::splat(swell);
            }
            SQUIRREL_SLOT => {
                // Bounding along just out of reach.
                let ahead = pos + Vec2::from_angle(heading) * 8.0;
                let s = race.track.locate(ahead, k.track_idx as usize).s;
                tr.translation = v3(ahead, race.track.height_at(s) + (t * 14.0).sin().abs() * 1.3);
                tr.rotation = yaw(heading) * Quat::from_rotation_x((t * 16.0).cos() * 0.25);
            }
            slot => {
                // Lead the tick like the kart does, so the ring stays centred on it.
                let at = orbit_pos(k, slot, w.time + race.alpha() * DT) - k.pos + pos;
                tr.translation = v3(at, road + lift + 0.8 * k.size());
                tr.rotation = Quat::from_rotation_y(t * 9.0 + slot as f32);
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct CamRig {
    yaw: f32,
    fov: f32,
    height: f32,
    /// How far back to sit, as the kart's size changes.
    size: f32,
    shake: f32,
    init: bool,
}

/// Vertical field of view (radians) that shows as much to the sides as `fov` degrees does at
/// 16:9, for views wider than that (two players' views are twice as wide).
fn fit_fov(fov: f32, aspect: f32) -> f32 {
    const WIDE: f32 = 16.0 / 9.0;
    if aspect <= WIDE {
        return fov.to_radians();
    }
    2.0 * ((fov * 0.5).to_radians().tan() * WIDE / aspect).atan()
}

/// Each player's chase camera.
pub fn race_camera(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    cli: Res<crate::game::Cli>,
    mut rigs: Local<Vec<CamRig>>,
    mut cams: Query<(&PlayerCamera, &Camera, &mut Transform, &mut Projection)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    rigs.resize(race.locals.len(), CamRig::default());
    for (player, camera, mut tr, mut proj) in &mut cams {
        let (Some(&me), Some(rig)) = (race.locals.get(player.0), rigs.get_mut(player.0)) else { continue };
        let aspect = camera.physical_viewport_size().map_or(16.0 / 9.0, |s| s.x as f32 / s.y.max(1) as f32);
        chase(&time, race, me, rig, aspect, &mut tr, &mut proj);
        if let Some(film) = cli.cam {
            // `--cam`: film the kart from a set place around it instead.
            let around = Vec2::from_angle(rig.yaw + film.angle + film.spin * race.world.time);
            let kart = v3(kart_pose(race, me).0, rig.height);
            tr.translation = kart - v3(around, 0.0) * film.dist + Vec3::Y * film.height;
            tr.look_at(kart + Vec3::Y * 0.9, Vec3::Y);
            if let Projection::Perspective(p) = proj.as_mut() {
                p.fov = fit_fov(film.fov, aspect);
            }
        }
    }
}

fn chase(time: &Time, race: &RaceState, me: usize, rig: &mut CamRig, aspect: f32, tr: &mut Transform, proj: &mut Projection) {
    let dt = time.delta_secs();
    let k = &race.world.karts[me];
    let (pos, heading) = kart_pose(race, me);

    let (ground, _) = kart_ground(race, me, pos, heading);
    // Rise with jumps, but not all the way: the kart should look like it's flying.
    let ground = ground + kart_lift(race, me, pos, heading).0 * 0.7;
    if !rig.init || race.world.time < 0.1 {
        rig.yaw = heading;
        rig.fov = 70.0;
        rig.height = ground;
        rig.size = 1.0;
        rig.init = true;
    }
    rig.height += (ground - rig.height) * (1.0 - (-8.0 * dt).exp());
    // Shake on bumps and hits.
    if k.bump > 0.3 || (k.spin > 1.2) {
        rig.shake = rig.shake.max(if k.spin > 0.0 { 0.35 } else { 0.18 });
    }
    rig.shake = (rig.shake - dt * 0.8).max(0.0);
    let tt = time.elapsed_secs();
    let shake = Vec3::new((tt * 53.0).sin(), (tt * 61.0).cos(), 0.0) * rig.shake * rig.shake * 1.5;
    // Lag behind the kart's rotation a little, and swing slightly into drifts.
    let target = heading + k.slip * 0.35;
    rig.yaw += wrap_angle(target - rig.yaw) * (1.0 - (-6.0 * dt).exp());
    // Only part of the way, so a huge kart fills more of the view and a small one less.
    rig.size += (1.0 + (k.size() - 1.0) * 0.45 - rig.size) * (1.0 - (-4.0 * dt).exp());
    let boosting = if k.is_boosting() || k.chase > 0.0 || k.zoom > 0.0 { 1.0 } else { 0.0 };
    let want_fov = 70.0 + boosting * 12.0 + (k.speed / MAX_SPEED).clamp(0.0, 1.3) * 4.0;
    rig.fov += (want_fov - rig.fov) * (1.0 - (-5.0 * dt).exp());

    // Pre-race: swing around from the front of the kart to behind it.
    let intro = (race.world.time / (COUNTDOWN - 0.8)).clamp(0.0, 1.0);
    let ease = intro * intro * (3.0 - 2.0 * intro);
    let orbit = rig.yaw + PI * (1.0 - ease);
    let dist = (7.5 + 3.0 * (1.0 - ease)) * rig.size;
    let back = Vec2::from_angle(orbit);
    let kart3 = v3(pos, rig.height);
    tr.translation = kart3 - v3(back, 0.0) * dist + Vec3::Y * (3.0 + (1.0 - ease) * 1.5) * rig.size + shake;
    tr.look_at(kart3 + v3(back, 0.0) * 4.0 * ease + Vec3::Y * 1.3 * rig.size + shake * 0.5, Vec3::Y);

    if let Projection::Perspective(p) = proj {
        p.fov = fit_fov(rig.fov, aspect);
    }
}

pub fn menu_camera(
    time: Res<Time>,
    track: Res<TrackRes>,
    cli: Res<crate::game::Cli>,
    mut cam: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let Ok((mut tr, mut proj)) = cam.single_mut() else { return };
    if let Some((eye, target)) = cli.view {
        *tr = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        return;
    }
    let (lo, hi) = track.0.samples.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), s| (lo.min(s.pos), hi.max(s.pos)));
    let c = (lo + hi) * 0.5;
    let ext = (hi - lo).max_element();
    let a = time.elapsed_secs() * 0.035 + 2.2;
    tr.translation = v3(c + Vec2::from_angle(a) * (ext * 0.62 + 40.0), ext * 0.2 + 45.0);
    tr.look_at(v3(c, 0.0), Vec3::Y);
    if let Projection::Perspective(p) = proj.as_mut() {
        p.fov = 60f32.to_radians();
    }
}
