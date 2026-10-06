//! Life in the scenery: cheering crowds, the statue watching you, the Ferris wheel, balloons,
//! the blimp, waving flags, the start lights, and things afloat in Mirror Lagoon.

use bevy::prelude::*;

use super::models::Kit;
use super::race::kart_pose;
use super::scene::MainCamera;
use super::scenery::*;
use crate::game::SessionRes;
use crate::sim::COUNTDOWN;

/// Where the karts driven on this machine are.
fn local_karts(session: &Option<Res<SessionRes>>) -> Vec<Vec2> {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return vec![] };
    race.locals.iter().map(|&i| kart_pose(race, i).0).collect()
}

/// The local kart nearest `p`, or else `fallback` (the menu camera).
fn nearest_local(karts: &[Vec2], fallback: Option<Vec2>, p: Vec2) -> Option<Vec2> {
    karts.iter().copied().min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p))).or(fallback)
}

pub fn animate_crowd(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut fans: Query<(&Spectator, &mut Transform, &GlobalTransform)>,
) {
    let t = time.elapsed_secs();
    let karts = local_karts(&session);
    let menu = cam.single().ok().map(|c| c.translation().xz());
    for (fan, mut tr, gt) in &mut fans {
        let p = gt.translation().xz();
        let near = nearest_local(&karts, menu, p);
        let excite = near.map_or(0.3, |k| (1.0 - (k.distance(p) - 25.0) / 70.0).clamp(0.3, 1.0));
        let hop = (t * 7.0 + fan.phase).sin().max(0.0) * 0.45 * excite;
        tr.translation = fan.base + Vec3::Y * hop;
        tr.rotation = fan.rot * Quat::from_rotation_z((t * 3.0 + fan.phase).sin() * 0.12 * excite);
    }
}

pub fn animate_statue(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut heads: Query<(&mut Transform, &GlobalTransform), With<StatueHead>>,
    mut tails: Query<&mut Transform, (With<StatueTail>, Without<StatueHead>)>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let karts = local_karts(&session);
    let menu = cam.single().ok().map(|c| c.translation().xz());
    for (mut tr, gt) in &mut heads {
        let Some(target) = nearest_local(&karts, menu, gt.translation().xz()) else { continue };
        let parent_rot = gt.rotation() * tr.rotation.inverse();
        let to = Vec3::new(target.x, gt.translation().y, target.y) - gt.translation();
        let local = parent_rot.inverse() * to;
        let want = (-local.x).atan2(-local.z).clamp(-1.2, 1.2);
        let (cur, _, _) = tr.rotation.to_euler(EulerRot::YXZ);
        let yaw = cur + (want - cur) * (1.0 - (-2.0 * dt).exp());
        tr.rotation = Quat::from_euler(EulerRot::YXZ, yaw, 0.0, (t * 0.7).sin() * 0.08);
    }
    for mut tr in &mut tails {
        tr.rotation = Quat::from_rotation_y((t * 9.0).sin() * 0.6);
    }
}

pub fn animate_rides(
    time: Res<Time>,
    mut wheels: Query<&mut Transform, With<FerrisWheel>>,
    mut gondolas: Query<&mut Transform, (With<Gondola>, Without<FerrisWheel>)>,
    mut drifters: Query<(&Drifter, &mut Transform), (Without<FerrisWheel>, Without<Gondola>)>,
    mut flags: Query<(&WavingFlag, &mut Transform), (Without<FerrisWheel>, Without<Gondola>, Without<Drifter>)>,
) {
    let t = time.elapsed_secs();
    let angle = t * 0.06;
    for mut tr in &mut wheels {
        tr.rotation = Quat::from_rotation_z(angle);
    }
    for mut tr in &mut gondolas {
        tr.rotation = Quat::from_rotation_z(-angle);
    }
    for (d, mut tr) in &mut drifters {
        let a = d.phase + t * d.speed;
        let p = d.center + Vec2::from_angle(a) * d.radius;
        tr.translation = Vec3::new(p.x, d.height + (t * 0.3 + d.phase).sin() * 3.0, p.y);
        if d.blimp {
            let tangent = Vec2::from_angle(a).perp() * d.speed.signum();
            tr.rotation = Quat::from_rotation_arc(-Vec3::Z, Vec3::new(tangent.x, 0.0, tangent.y));
        } else {
            tr.rotation = Quat::from_rotation_y(t * 0.05 + d.phase) * Quat::from_rotation_z((t * 0.4 + d.phase).sin() * 0.04);
        }
    }
    for (f, mut tr) in &mut flags {
        let w = (t * 3.2 + f.0).sin();
        tr.rotation = Quat::from_rotation_y(w * 0.35) * Quat::from_rotation_z((t * 5.0 + f.0 * 2.0).sin() * 0.06);
    }
}

pub fn animate_night(
    time: Res<Time>,
    water: Option<Res<super::trackmesh::WaterMat>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut lanterns: Query<(&Bob, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    for (b, mut tr) in &mut lanterns {
        tr.translation = b.base + Vec3::new((t * 0.3 + b.phase).sin() * 0.8, (t * 0.7 + b.phase).sin() * 1.2, (t * 0.25 + b.phase * 2.0).cos() * 0.8);
        tr.rotation = Quat::from_rotation_y(t * 0.2 + b.phase);
    }
    if let Some(mut m) = water.and_then(|w| mats.get_mut(&w.0)) {
        m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(t * 0.018, t * 0.011));
    }
}

/// Materials whose texture slides steadily, in repeats a second (falling water).
#[derive(Resource, Default)]
pub struct ScrollMats(pub Vec<(Handle<StandardMaterial>, Vec2)>);

/// Cable cars glide between their stations, and waterfalls fall.
pub fn animate_shuttles(time: Res<Time>, scroll: Option<Res<ScrollMats>>, mut mats: ResMut<Assets<StandardMaterial>>, mut cars: Query<(&Shuttle, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, mut tr) in &mut cars {
        let u = 0.5 - 0.5 * ((t / s.period + s.phase) * std::f32::consts::TAU).cos();
        tr.translation = s.a.lerp(s.b, u);
    }
    for (h, speed) in scroll.iter().flat_map(|s| &s.0) {
        if let Some(mut m) = mats.get_mut(h) {
            m.uv_transform = bevy::math::Affine2::from_translation((*speed * t).fract());
        }
    }
}

/// Mirror Lagoon: the mirror ball turns; lanterns and yachts bob and sway on the water.
pub fn animate_lagoon(time: Res<Time>, mut spinners: Query<(&Spin, &mut Transform)>, mut floaters: Query<(&Floating, &mut Transform), Without<Spin>>) {
    let t = time.elapsed_secs();
    for (s, mut tr) in &mut spinners {
        tr.rotation = Quat::from_rotation_y(t * s.0);
    }
    for (f, mut tr) in &mut floaters {
        tr.translation = f.base + Vec3::Y * ((t * 0.9 + f.phase).sin() * 0.07 + (t * 1.7 + f.phase * 1.3).sin() * 0.03);
    }
}

/// Boost pad arrows run along the road and a boosted ramp's rainbow flows up it: both just
/// scroll their texture, so this is one small material change each a frame.
pub fn animate_pads(time: Res<Time>, surf: Res<super::scene::SurfaceMats>, mut mats: ResMut<Assets<StandardMaterial>>) {
    use super::textures::{PAD_TILE, RAMP_TILE};
    let t = time.elapsed_secs();
    // In metres per second; moving the texture back moves the pattern forward.
    for (h, speed, tile) in [(&surf.pad_arrows, 4.0, PAD_TILE.y), (&surf.rainbow, 3.0, RAMP_TILE.y)] {
        if let Some(mut m) = mats.get_mut(h) {
            m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(0.0, -(t * speed / tile).fract()));
        }
    }
}

pub fn start_lights(
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    mut lights: Query<(&StartLight, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let time = session.as_ref().and_then(|s| s.0.race.as_ref()).map(|r| r.world.time);
    for (light, mut mat) in &mut lights {
        let pod = light.0 / 2;
        let want = match time {
            Some(t) if t < COUNTDOWN => {
                let lit = ((t - 0.8) / 0.6).floor().clamp(0.0, 5.0) as usize;
                if pod < lit { &kit.lamp_red } else { &kit.dark }
            }
            Some(t) if t < COUNTDOWN + 2.5 => &kit.lamp_green,
            _ => &kit.dark,
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}
