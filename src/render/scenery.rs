//! Everything around the track. `spawn_common` is shared (ground, grass, trees, hills, clouds);
//! `spawn_meadow` and `spawn_circuit` add each track's own set pieces.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{light::NotShadowCaster, prelude::*};

use super::mesh::{v3, Lcg, MeshBuilder};
use super::corgi::{CorgiSet, SIT_HEAD, SIT_HEAD_SCALE};
use super::models::{matte, part, t, Kit, TransformExt};
use super::scene::{Scenery, SurfaceMats};
use super::signs::{SignAtlas, SIGN_ADS, SIGN_TITLE};
use super::trackmesh::{at, spawn_board, spawn_sign, BANK_SLOPE, WALL_THICK};
use crate::sim::track::{Theme, Track, TrackId, ROAD_HALF_WIDTH, WALL_DIST};

// ----- Animated scenery markers (animated in `ambient.rs`) ------------------------------------

#[derive(Component)]
pub struct Spectator {
    pub phase: f32,
    pub base: Vec3,
    pub rot: Quat,
}
#[derive(Component)]
pub struct StatueHead;
#[derive(Component)]
pub struct StatueTail;
#[derive(Component)]
pub struct FerrisWheel;
#[derive(Component)]
pub struct Gondola;
#[derive(Component)]
pub struct Drifter {
    pub center: Vec2,
    pub radius: f32,
    pub height: f32,
    pub speed: f32,
    pub phase: f32,
    /// Blimps point along their path; balloons just turn slowly.
    pub blimp: bool,
}
#[derive(Component)]
pub struct WavingFlag(pub f32);
#[derive(Component)]
pub struct StartLight(pub usize);
/// Floats gently up and down around `base`.
#[derive(Component)]
pub struct Bob {
    pub base: Vec3,
    pub phase: f32,
}
/// Follows the camera like the sky dome (stars).
#[derive(Component)]
pub struct SkyFollow;
/// Turns steadily about the vertical, in radians per second (the mirror ball).
#[derive(Component)]
pub struct Spin(pub f32);
/// Rides the ripples on the water around `base` (lanterns, moored yachts).
#[derive(Component)]
pub struct Floating {
    pub base: Vec3,
    pub phase: f32,
}
/// Glides back and forth between `a` and `b` every `period` seconds (cable cars).
#[derive(Component)]
pub struct Shuttle {
    pub a: Vec3,
    pub b: Vec3,
    pub period: f32,
    pub phase: f32,
}

// ----- Placement helpers --------------------------------------------------------------------

pub(super) fn bounds(track: &Track) -> (Vec2, Vec2) {
    track.samples.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), s| (lo.min(s.pos), hi.max(s.pos)))
}

/// Distance from `p` to the edge of the track's footprint (walls plus any embankment).
pub(super) fn clearance(track: &Track, p: Vec2) -> f32 {
    track
        .samples
        .iter()
        .step_by(3)
        .map(|s| s.pos.distance(p) - (WALL_DIST + WALL_THICK + BANK_SLOPE * s.height))
        .fold(f32::MAX, f32::min)
}

pub(super) struct Reserved(pub(super) Vec<(Vec2, f32)>);

impl Reserved {
    pub(super) fn free(&self, p: Vec2, r: f32) -> bool {
        self.0.iter().all(|(c, cr)| c.distance(p) > cr + r)
    }
    pub(super) fn add(&mut self, p: Vec2, r: f32) {
        self.0.push((p, r));
    }
}

/// A sculpted corgi sitting on the ground (about 2.4 units tall). `statue` adds the animated
/// head and tail markers and a golden crown; `face` adds glossy eyes and nose.
pub(super) fn sitting_corgi(c: &mut ChildSpawnerCommands, kit: &Kit, set: &CorgiSet, mat: &Handle<StandardMaterial>, statue: bool, face: bool) {
    c.spawn((Mesh3d(set.sitting.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
    let tail = (t(0.0, 0.62, 0.9), Visibility::default());
    let tail_mesh = (Mesh3d(set.tail.clone()), MeshMaterial3d(mat.clone()), Transform::default());
    if statue {
        c.spawn((tail, StatueTail)).with_children(|tl| {
            tl.spawn(tail_mesh);
        });
    } else {
        c.spawn(tail).with_children(|tl| {
            tl.spawn(tail_mesh);
        });
    }
    let head = (Transform::from_translation(SIT_HEAD).with_scale(Vec3::splat(SIT_HEAD_SCALE)), Visibility::default());
    let build = |h: &mut ChildSpawnerCommands| {
        if face {
            super::models::corgi_face(h, kit, &set.head);
        } else {
            h.spawn((Mesh3d(set.head.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
        }
        if statue {
            // a golden crown for the Top Dog
            part(h, &kit.cylinder, &kit.gold, t(0.0, 0.33, 0.04).s(0.3, 0.1, 0.3));
            for k in 0..6 {
                let a = k as f32 / 6.0 * TAU;
                part(h, &kit.cone, &kit.gold, t(a.cos() * 0.13, 0.45, 0.04 + a.sin() * 0.13).s(0.07, 0.15, 0.07));
            }
        }
    };
    if statue {
        c.spawn((head, StatueHead)).with_children(build);
    } else {
        c.spawn(head).with_children(build);
    }
}

// ----- Shared ---------------------------------------------------------------------------------

pub fn spawn_common(commands: &mut Commands, kit: &Kit, surf: &SurfaceMats, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track) {
    let circuit = track.theme == Theme::Circuit;
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;
    let mut rng = Lcg(0xC0461 ^ track.length as u32);
    if track.theme == Theme::Prism {
        spawn_night_common(commands, kit, &surf.windows, meshes, mats, track, &mut rng);
        return;
    }

    // Ground: a big grid with gentle colour variation, and mowing stripes around the circuit.
    let mut ground = MeshBuilder::default();
    let (cell_x, cell_z, half) = (12.5, 25.0, 1600.0);
    let base = if circuit { Vec3::new(0.34, 0.62, 0.25) } else { Vec3::new(0.38, 0.64, 0.27) };
    let patch = |p: Vec2| {
        let n = (p.x * 0.011).sin() * (p.y * 0.013).cos() + (p.x * 0.004 + p.y * 0.005).sin() * 0.8;
        1.0 + n * 0.06
    };
    let mut z = center.y - half;
    while z < center.y + half {
        let mut x = center.x - half;
        while x < center.x + half {
            let near = (Vec2::new(x, z) - center).length() < (hi - lo).max_element() * 0.75;
            let stripe = if circuit && near && ((x / cell_x) as i32).rem_euclid(2) == 0 { 1.07 } else { 1.0 };
            let corners = [Vec2::new(x, z), Vec2::new(x, z + cell_z), Vec2::new(x + cell_x, z + cell_z), Vec2::new(x + cell_x, z)];
            let cols = corners.map(|c| {
                let k = patch(c) * stripe;
                Color::srgb(base.x * k, base.y * k, base.z * k)
            });
            ground.quad_vc(corners.map(|c| v3(c, -0.03)), Vec3::Y, cols, corners.map(|c| c / 5.0));
            x += cell_x;
        }
        z += cell_z;
    }
    commands.spawn((ground.tinted(meshes, mats, &surf.grass), NotShadowCaster, super::gfx::RaytraceSink(0.1), Scenery));

    // Grass tufts (and wild flowers on the meadow) near the track: thousands of blades, one mesh.
    let mut tufts = MeshBuilder::default();
    let flower_cols = [Color::srgb(1.0, 1.0, 0.95), Color::srgb(1.0, 0.85, 0.2), Color::srgb(0.95, 0.5, 0.7), Color::srgb(0.6, 0.6, 1.0)];
    let mut placed = 0;
    while placed < 7000 {
        let p = Vec2::new(rng.range(lo.x - 70.0, hi.x + 70.0), rng.range(lo.y - 70.0, hi.y + 70.0));
        let c = clearance(track, p);
        if !(0.5..60.0).contains(&c) {
            continue;
        }
        placed += 1;
        let shade = rng.range(0.75, 1.1);
        let col = Color::srgb(0.28 * shade, 0.55 * shade, 0.2 * shade);
        for _ in 0..3 {
            let a = rng.f() * TAU;
            let d = Vec2::from_angle(a) * 0.18;
            let h = rng.range(0.35, 0.7);
            let tip = v3(p + d * rng.range(1.0, 3.0), h);
            tufts.tri([v3(p - d, 0.0), v3(p + d, 0.0), tip], Vec3::Y, col);
            tufts.tri([v3(p + d, 0.0), v3(p - d, 0.0), tip], Vec3::Y, col);
        }
        if !circuit && rng.f() < 0.35 {
            let fc = *rng.pick(&flower_cols);
            let q = p + Vec2::new(rng.range(-0.6, 0.6), rng.range(-0.6, 0.6));
            let h = rng.range(0.25, 0.45);
            for a in [0.0, FRAC_PI_2] {
                let d = Vec2::from_angle(a) * 0.12;
                tufts.quad([v3(q - d, h - 0.1), v3(q + d, h - 0.1), v3(q + d, h + 0.1), v3(q - d, h + 0.1)], Vec3::Y, fc);
            }
        }
    }
    commands.spawn((tufts.tinted(meshes, mats, &surf.plain), NotShadowCaster, Scenery));

    // Rolling hills on the horizon
    let hill = mats.add(StandardMaterial { base_color: Color::srgb(0.36, 0.6, 0.3), perceptual_roughness: 1.0, ..default() });
    let far = (hi - lo).max_element() * 0.5 + 650.0;
    for k in 0..18 {
        let a = k as f32 / 18.0 * TAU + rng.range(-0.1, 0.1);
        let r = rng.range(far, far + 250.0);
        let p = center + Vec2::from_angle(a) * r;
        let s = rng.range(200.0, 360.0);
        commands.spawn((
            Mesh3d(kit.sphere.clone()),
            MeshMaterial3d(hill.clone()),
            Transform::from_translation(v3(p, -s * 0.16)).with_scale(Vec3::new(s * 1.6, s * 0.55, s)),
            NotShadowCaster,
            Scenery,
        ));
    }

    // Fluffy clouds
    // Forward-rendered (blend) so they're shaded the same with and without raytracing.
    let cloud = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.9, 0.9, 0.95),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let puff = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    for _ in 0..30 {
        let p = center + Vec2::new(rng.range(-1100.0, 1100.0), rng.range(-1100.0, 1100.0));
        let y = rng.range(260.0, 340.0);
        commands
            .spawn((Transform::from_translation(v3(p, y)), Visibility::default(), Scenery))
            .with_children(|cl| {
                for _ in 0..6 {
                    let o = Vec3::new(rng.range(-22.0, 22.0), rng.range(-3.0, 5.0), rng.range(-10.0, 10.0));
                    let s = rng.range(10.0, 17.0);
                    cl.spawn((Mesh3d(puff.clone()), MeshMaterial3d(cloud.clone()), Transform::from_translation(o).with_scale(Vec3::new(s * 1.4, s * 0.7, s)), NotShadowCaster, super::gfx::NoRaytrace));
                }
            });
    }
}

/// Trees around the track, avoiding the reserved areas.
fn spawn_trees(commands: &mut Commands, kit: &Kit, mats: &mut Assets<StandardMaterial>, track: &Track, count: usize, margin: f32, reserved: &Reserved, rng: &mut Lcg) {
    let (lo, hi) = bounds(track);
    let mut m = |c: Color| mats.add(matte(c));
    let trunk = m(Color::srgb(0.45, 0.3, 0.18));
    let leaves = [m(Color::srgb(0.13, 0.42, 0.18)), m(Color::srgb(0.2, 0.52, 0.2)), m(Color::srgb(0.3, 0.6, 0.22)), m(Color::srgb(0.45, 0.62, 0.2))];
    let mut placed = 0;
    let mut tries = 0;
    while placed < count && tries < count * 50 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - margin, hi.x + margin), rng.range(lo.y - margin, hi.y + margin));
        if clearance(track, p) < 5.0 || !reserved.free(p, 4.0) {
            continue;
        }
        placed += 1;
        let s = rng.range(0.8, 1.7);
        let leaf = rng.pick(&leaves).clone();
        let pine = rng.f() < 0.5;
        commands
            .spawn((Transform::from_translation(v3(p, 0.0)).with_scale(Vec3::splat(s)).with_rotation(Quat::from_rotation_y(rng.f() * PI)), Visibility::default(), Scenery))
            .with_children(|tr| {
                part(tr, &kit.cylinder, &trunk, t(0.0, 1.3, 0.0).s(0.7, 2.6, 0.7));
                if pine {
                    for (k, (r, hgt)) in [(4.4, 3.4), (3.6, 3.0), (2.6, 2.6)].into_iter().enumerate() {
                        part(tr, &kit.cone, &leaf, t(0.0, 3.4 + k as f32 * 1.8, 0.0).s(r, hgt, r));
                    }
                } else {
                    part(tr, &kit.sphere, &leaf, t(0.0, 4.6, 0.0).s(5.4, 4.8, 5.4));
                    part(tr, &kit.sphere, &leaf, t(1.2, 3.8, 0.8).s(3.4, 3.4, 3.4));
                    part(tr, &kit.sphere, &leaf, t(-1.0, 5.4, -0.6).s(3.0, 3.0, 3.0));
                }
            });
    }
}

fn bone_shape(b: &mut ChildSpawnerCommands, kit: &Kit) {
    part(b, &kit.capsule, &kit.cream, t(0.0, 0.0, 0.0).s(0.3, 0.8, 0.3).rz(FRAC_PI_2));
    for (x, y) in [(-0.7, -0.17), (-0.7, 0.17), (0.7, -0.17), (0.7, 0.17)] {
        part(b, &kit.sphere, &kit.cream, t(x, y, 0.0).s(0.42, 0.42, 0.42));
    }
}

// ----- Corgi Meadow -------------------------------------------------------------------------

pub fn spawn_meadow(commands: &mut Commands, kit: &Kit, _meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track) {
    let mut rng = Lcg(0xC0461);
    let mut reserved = Reserved(vec![]);
    let mut m = |c: Color| mats.add(matte(c));
    let red = m(Color::srgb(0.85, 0.15, 0.12));
    let wood = m(Color::srgb(0.7, 0.35, 0.2));
    let roof = m(Color::srgb(0.35, 0.2, 0.15));
    let stand = m(Color::srgb(0.6, 0.62, 0.66));
    let orange = m(Color::srgb(0.95, 0.55, 0.12));
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;

    // Start gantry with a giant bone on top
    let (c, d) = track.point_at(0.0);
    let yaw = -d.to_angle() - FRAC_PI_2;
    let span = ROAD_HALF_WIDTH + 2.5;
    commands
        .spawn((Transform::from_translation(v3(c, track.height_at(0.0))).with_rotation(Quat::from_rotation_y(yaw)), Visibility::default(), Scenery))
        .with_children(|g| {
            for x in [-span, span] {
                part(g, &kit.cylinder, &kit.metal, t(x, 3.75, 0.0).s(0.7, 7.5, 0.7));
            }
            part(g, &kit.cube, &orange, t(0.0, 7.2, 0.0).s(span * 2.0 + 1.0, 1.8, 0.6));
            for k in 0..24 {
                let x = -span + (k as f32 + 0.5) * (span * 2.0 / 24.0);
                let mt = if k % 2 == 0 { &kit.white } else { &kit.black };
                part(g, &kit.cube, mt, t(x, 6.45, 0.0).s(span * 2.0 / 24.0, 0.3, 0.65));
            }
            g.spawn((t(0.0, 9.0, 0.0).with_scale(Vec3::splat(3.0)), Visibility::default())).with_children(|b| bone_shape(b, kit));
        });

    let spot = |rng: &mut Lcg, reserved: &Reserved, clear: f32, margin: f32| loop {
        let p = Vec2::new(rng.range(lo.x - margin, hi.x + margin), rng.range(lo.y - margin, hi.y + margin));
        if clearance(track, p) > clear && reserved.free(p, clear) {
            return p;
        }
    };

    // Giant bones lying around
    for _ in 0..7 {
        let p = spot(&mut rng, &reserved, 8.0, 40.0);
        reserved.add(p, 6.0);
        commands
            .spawn((
                Transform::from_translation(v3(p, 1.2))
                    .with_rotation(Quat::from_rotation_y(rng.f() * PI) * Quat::from_rotation_x(FRAC_PI_2))
                    .with_scale(Vec3::splat(rng.range(3.0, 5.0))),
                Visibility::default(),
                Scenery,
            ))
            .with_children(|b| bone_shape(b, kit));
    }

    // Giant tennis balls
    for _ in 0..8 {
        let p = spot(&mut rng, &reserved, 6.0, 40.0);
        let s = rng.range(3.0, 6.0);
        reserved.add(p, s);
        commands
            .spawn((Transform::from_translation(v3(p, s * 0.5)).with_scale(Vec3::splat(s)).with_rotation(Quat::from_rotation_z(rng.f() * PI)), Visibility::default(), Scenery))
            .with_children(|b| {
                part(b, &kit.sphere, &kit.ball, Transform::default());
                part(b, &kit.torus, &kit.white, t(0.0, 0.0, 0.0).s(0.83, 0.3, 0.83).rx(0.5));
            });
    }

    // Doghouses
    for _ in 0..6 {
        let p = spot(&mut rng, &reserved, 6.0, 30.0);
        reserved.add(p, 5.0);
        let face = (center - p).to_angle();
        commands
            .spawn((Transform::from_translation(v3(p, 0.0)).with_rotation(Quat::from_rotation_y(-face - FRAC_PI_2)).with_scale(Vec3::splat(2.2)), Visibility::default(), Scenery))
            .with_children(|d| {
                part(d, &kit.cube, &wood, t(0.0, 0.8, 0.0).s(2.0, 1.6, 2.4));
                for sgn in [-1.0f32, 1.0] {
                    part(d, &kit.cube, &roof, t(sgn * 0.62, 2.05, 0.0).rz(-sgn * 0.75).s(1.6, 0.15, 2.7));
                }
                part(d, &kit.cube, &kit.black, t(0.0, 0.6, -1.19).s(0.9, 1.1, 0.05));
            });
    }

    // Fire hydrants along the hedges
    for k in 0..16 {
        let s = track.length * (k as f32 + 0.3) / 16.0;
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let (c, d) = track.point_at(s);
        let p = c + d.perp() * side * (WALL_DIST + 2.5);
        commands.spawn((Transform::from_translation(v3(p, 0.0)), Visibility::default(), Scenery)).with_children(|h| {
            part(h, &kit.cylinder, &red, t(0.0, 0.6, 0.0).s(0.6, 1.2, 0.6));
            part(h, &kit.sphere, &red, t(0.0, 1.2, 0.0).s(0.6, 0.6, 0.6));
            part(h, &kit.cylinder, &red, t(0.0, 0.85, 0.0).rz(FRAC_PI_2).s(0.25, 1.0, 0.25));
        });
    }

    // Grandstand full of corgi fans beside the start straight (on a custom track, if there's room)
    let custom = matches!(track.id, TrackId::Custom(_));
    if !custom || strip_fits(track, -26.0, 46.0, -1.0, WALL_DIST + 2.0, WALL_DIST + 22.0, true) {
        let (c, d) = track.point_at(10.0);
        let yaw = -d.to_angle() - FRAC_PI_2;
        let base = c - d.perp() * (WALL_DIST + 9.0);
        reserved.add(base, 40.0);
        commands
            .spawn((Transform::from_translation(v3(base, 0.0)).with_rotation(Quat::from_rotation_y(yaw)), Visibility::default(), Scenery))
            .with_children(|g| {
                for tier in 0..4 {
                    let z = -(tier as f32) * 2.0;
                    let y = tier as f32 * 1.2;
                    part(g, &kit.cube, &stand, t(0.0, y * 0.5 + 0.6, z).s(70.0, y + 1.2, 2.0));
                    for f in 0..40 {
                        if rng.f() < 0.25 {
                            continue;
                        }
                        let x = -34.0 + f as f32 * 1.7 + rng.range(-0.3, 0.3);
                        let pos = Vec3::new(x, y + 1.2, z);
                        spawn_spectator(g, kit, pos, rng.f() * TAU, (rng.f() * 3.0) as usize);
                    }
                }
                part(g, &kit.cube, &roof, t(0.0, 9.5, -3.0).s(72.0, 0.4, 10.0));
                for x in [-35.0, 35.0] {
                    part(g, &kit.cylinder, &kit.metal, t(x, 4.75, -7.0).s(0.5, 9.5, 0.5));
                }
            });
    }

    spawn_trees(commands, kit, mats, track, 320, 90.0, &reserved, &mut rng);
}

/// A cheering corgi fan, facing -Z of its parent: one sculpted mesh with the face baked in.
fn spawn_spectator(p: &mut ChildSpawnerCommands, kit: &Kit, pos: Vec3, phase: f32, coat: usize) {
    p.spawn((Transform::from_translation(pos), Visibility::default(), Spectator { phase, base: pos, rot: Quat::IDENTITY }, fan_mesh(kit, coat)));
}

fn fan_mesh(kit: &Kit, coat: usize) -> (Mesh3d, MeshMaterial3d<StandardMaterial>, super::lod::Lod) {
    let lods = &kit.corgi.coats[coat % 3].fan_lods;
    (Mesh3d(lods.full().clone()), MeshMaterial3d(kit.fur.clone()), lods.lod(super::lod::FAN_LOD))
}

// ----- Corgi Circuit ------------------------------------------------------------------------

/// A covered grandstand following the track between `s0` and `s1` on `side`, packed with fans.
pub(super) fn spawn_grandstand(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, surf: &SurfaceMats, track: &Track, s0: f32, s1: f32, side: f32, rng: &mut Lcg, reserved: &mut Reserved) {
    const TIERS: usize = 7;
    const DEPTH: f32 = 1.7;
    const RISE: f32 = 0.95;
    let start = WALL_DIST + WALL_THICK + 4.0;
    let roof_y = 3.0 + TIERS as f32 * RISE + 3.2;
    let mut mesh = MeshBuilder::default();
    let n = track.samples.len();
    let i0 = (s0 / track.spacing) as usize;
    let i1 = (s1 / track.spacing) as usize;
    let seat_cols = [Color::srgb(0.1, 0.3, 0.75), Color::srgb(0.95, 0.55, 0.12)];
    let p = |i: usize, lat: f32, y: f32| v3(at(track, i % n, side * lat), y);
    for i in i0..i1 {
        let j = i + 1;
        for k in 0..TIERS {
            let l0 = start + k as f32 * DEPTH;
            let l1 = l0 + DEPTH;
            let y = 0.6 + k as f32 * RISE;
            let c = seat_cols[(k + (i / 20)) % 2];
            // riser (facing the track) + tread (facing up)
            let to_track = v3(-track.samples[i % n].dir.perp() * side, 0.0);
            mesh.quad_facing([p(i, l0, y - RISE), p(j, l0, y - RISE), p(j, l0, y), p(i, l0, y)], to_track, Color::srgb(0.75, 0.75, 0.78), [Vec2::ZERO; 4]);
            mesh.quad_facing([p(i, l0, y), p(j, l0, y), p(j, l1, y), p(i, l1, y)], Vec3::Y, c, [Vec2::ZERO; 4]);
        }
        let back = start + TIERS as f32 * DEPTH;
        let (a, b) = if side > 0.0 { (j, i) } else { (i, j) };
        let wall = if (i / 10) % 2 == 0 { Color::srgb(0.12, 0.2, 0.45) } else { Color::srgb(0.14, 0.24, 0.52) };
        mesh.quad_auto([p(a, back, 0.0), p(b, back, 0.0), p(b, back, roof_y), p(a, back, roof_y)], wall, [Vec2::ZERO; 4]);
        mesh.quad_auto([p(b, back, 0.0), p(a, back, 0.0), p(a, back, roof_y), p(b, back, roof_y)], wall, [Vec2::ZERO; 4]);
        // roof (both faces)
        let roof = [p(i, start - 2.0, roof_y), p(j, start - 2.0, roof_y), p(j, back + 0.5, roof_y + 1.2), p(i, back + 0.5, roof_y + 1.2)];
        let rc = if (i / 6) % 2 == 0 { Color::srgb(0.95, 0.52, 0.1) } else { Color::srgb(0.97, 0.97, 0.95) };
        let flipped = [roof[3], roof[2], roof[1], roof[0]];
        let (top, under) = if side > 0.0 { (flipped, roof) } else { (roof, flipped) };
        mesh.quad_auto(top, rc, [Vec2::ZERO; 4]);
        mesh.quad_auto(under, Color::srgb(0.85, 0.85, 0.85), [Vec2::ZERO; 4]);
    }
    commands.spawn((Mesh3d(meshes.add(mesh.build())), MeshMaterial3d(surf.plain.clone()), Scenery));
    let back = start + TIERS as f32 * DEPTH;
    for i in (i0..=i1).step_by(18) {
        let q = at(track, i % n, side * (start - 1.5));
        commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(kit.metal.clone()), Transform::from_translation(v3(q, roof_y * 0.5)).with_scale(Vec3::new(0.35, roof_y, 0.35)), Scenery));
        // flags along the roof
        let fq = at(track, i % n, side * (back + 0.5));
        let dir = track.samples[i % n].dir;
        spawn_flag(commands, kit, v3(fq, roof_y + 1.2), dir, (i / 18) % 8, rng.f() * TAU);
    }
    for i in (i0..=i1).step_by(10) {
        reserved.add(at(track, i % n, side * (start + back) * 0.5), back - start);
    }

    // The crowd
    let mut s = s0 + 0.8;
    while s < s1 - 0.8 {
        let (c, d) = track.point_at(s);
        let facing = Quat::from_rotation_arc(-Vec3::Z, v3(-d.perp() * side, 0.0));
        for k in 0..TIERS {
            if rng.f() < 0.28 {
                continue;
            }
            let lat = start + (k as f32 + 0.5) * DEPTH;
            let pos = v3(c + d.perp() * side * lat, 0.6 + k as f32 * RISE);
            let coat = if rng.f() < 0.6 { 0 } else { (rng.f() * 3.0) as usize };
            commands
                .spawn((
                    Transform::from_translation(pos).with_rotation(facing).with_scale(Vec3::splat(1.3)),
                    Visibility::default(),
                    Spectator { phase: rng.f() * TAU, base: pos, rot: facing },
                    fan_mesh(kit, coat),
                    Scenery,
                ));
        }
        s += 1.5;
    }
}

fn spawn_flag(commands: &mut Commands, kit: &Kit, base: Vec3, dir: Vec2, idx: usize, phase: f32) {
    let yaw = Quat::from_rotation_y(-dir.to_angle());
    commands
        .spawn((Transform::from_translation(base).with_rotation(yaw), Visibility::default(), Scenery))
        .with_children(|f| {
            part(f, &kit.cylinder, &kit.metal, t(0.0, 2.5, 0.0).s(0.12, 5.0, 0.12));
            f.spawn((t(0.0, 4.4, 0.0), Visibility::default(), WavingFlag(phase))).with_children(|fl| {
                part(fl, &kit.cube, &kit.kart_colors[idx], t(-1.1, 0.0, 0.0).s(2.2, 1.2, 0.05));
            });
        });
}

/// Materials for Corgi Circuit's set pieces.
struct CircuitMats {
    orange: Handle<StandardMaterial>,
    navy: Handle<StandardMaterial>,
    white_paint: Handle<StandardMaterial>,
    tyre_red: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    statue_fur: Handle<StandardMaterial>,
    sign_mat: Handle<StandardMaterial>,
}

impl CircuitMats {
    fn new(mats: &mut Assets<StandardMaterial>, signs: &SignAtlas) -> Self {
        let mut m = |c: Color| mats.add(matte(c));
        let (orange, navy, white_paint) = (m(Color::srgb(0.95, 0.52, 0.1)), m(Color::srgb(0.1, 0.16, 0.35)), m(Color::srgb(0.95, 0.95, 0.93)));
        let (tyre_red, stone) = (m(Color::srgb(0.85, 0.12, 0.1)), m(Color::srgb(0.62, 0.6, 0.58)));
        // The statue is painted fibreglass: glossy, coloured by the sculpt's vertex colours.
        let statue_fur = mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.3, clearcoat: 0.6, clearcoat_perceptual_roughness: 0.1, ..default() });
        CircuitMats { orange, navy, white_paint, tyre_red, stone, statue_fur, sign_mat: signs.material.clone() }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_circuit(commands: &mut Commands, kit: &Kit, signs: &SignAtlas, surf: &SurfaceMats, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track) {
    let mut rng = Lcg(0xC1C017);
    let mut reserved = Reserved(vec![]);
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;
    let cm = CircuitMats::new(mats, signs);
    let (orange, navy, white_paint, sign_mat) = (cm.orange.clone(), cm.navy.clone(), cm.white_paint.clone(), cm.sign_mat.clone());
    let glass = kit.glass.clone();

    // --- Main grandstand along the start straight (outside), and one around the hairpin.
    spawn_grandstand(commands, kit, meshes, surf, track, 15.0, 245.0, -1.0, &mut rng, &mut reserved);
    let hp = track.mark("hairpin") / track.length;
    let hairpin = find_tightest(track, hp, hp + 0.05);
    let hs = track.samples[hairpin].s;
    spawn_grandstand(commands, kit, meshes, surf, track, hs - 45.0, hs + 45.0, 1.0, &mut rng, &mut reserved);

    spawn_gantry(commands, kit, meshes, &sign_mat, track, &navy, &orange, SIGN_TITLE);

    // --- Pit building and control tower on the infield side of the start straight.
    let pit_lat = WALL_DIST + WALL_THICK + 14.0;
    let (pc, pd) = track.point_at(95.0);
    let pyaw = Quat::from_rotation_y(-pd.to_angle() - FRAC_PI_2);
    let pit_pos = pc + pd.perp() * pit_lat;
    reserved.add(pit_pos, 80.0);
    // pit lane tarmac
    let mut lane = MeshBuilder::default();
    for i in 5..190 {
        let a = |i: usize, l: f32| v3(at(track, i, l), 0.01);
        let (l0, l1) = (WALL_DIST + WALL_THICK, pit_lat - 5.5);
        lane.quad([a(i, l0), a(i, l1), a(i + 1, l1), a(i + 1, l0)], Vec3::Y, Color::srgb(0.38, 0.38, 0.41));
        if i % 10 < 5 {
            lane.quad([a(i, l0 + 5.0), a(i, l0 + 5.3), a(i + 1, l0 + 5.3), a(i + 1, l0 + 5.0)], Vec3::Y, Color::srgb(0.95, 0.95, 0.95));
        }
    }
    commands.spawn((Mesh3d(meshes.add(lane.build())), MeshMaterial3d(surf.road.clone()), NotShadowCaster, Scenery));
    commands
        .spawn((Transform::from_translation(v3(pit_pos, 0.0)).with_rotation(pyaw), Visibility::default(), Scenery))
        .with_children(|b| {
            // local frame: -Z = direction of travel, +X = right = away from the track
            part(b, &kit.cube, &white_paint, t(6.0, 4.0, 0.0).s(12.0, 8.0, 150.0));
            part(b, &kit.cube, &navy, t(6.0, 8.3, 0.0).s(13.0, 0.6, 151.0));
            part(b, &kit.cube, &glass, t(-0.02, 6.2, 0.0).s(0.1, 1.6, 146.0));
            for k in 0..14 {
                let z = -65.0 + k as f32 * 10.0;
                part(b, &kit.cube, &kit.black, t(-0.01, 2.2, z).s(0.1, 4.2, 8.0));
                part(b, &kit.cube, &kit.kart_colors[k % 8], t(-0.03, 4.55, z).s(0.1, 0.5, 8.0));
            }
            // control tower
            part(b, &kit.cube, &white_paint, t(8.0, 9.0, 82.0).s(9.0, 18.0, 9.0));
            part(b, &kit.cube, &glass, t(8.0, 16.0, 82.0).s(9.4, 3.0, 9.4));
            part(b, &kit.cube, &orange, t(8.0, 18.3, 82.0).s(10.0, 0.8, 10.0));
        });
    // Boards on the pit roof and tower
    let tower = pit_pos + pd * -82.0 + pd.perp() * 3.4;
    spawn_board(commands, meshes, &sign_mat, kit, v3(tower, 18.8), -v3(pd.perp(), 0.0), SIGN_TITLE, Vec2::new(12.0, 2.6), false);
    for k in 0..4 {
        let q = pit_pos + pd * (-50.0 + k as f32 * 33.0);
        spawn_board(commands, meshes, &sign_mat, kit, v3(q, 8.6), -v3(pd.perp(), 0.0), 2 + k * 2, Vec2::new(14.0, 2.2), false);
    }

    let (fc, fd) = track.point_at(150.0);
    let wheel_at = fc - fd.perp() * 80.0;
    reserved.add(wheel_at, 40.0);
    ferris_wheel(commands, kit, meshes, mats, wheel_at, center);
    let statue_at = best_spot(track, Vec2::new(300.0, 170.0), 30.0, &reserved);
    reserved.add(statue_at, 22.0);
    crown_statue(commands, kit, meshes, &cm, track, statue_at);
    bridge_signs(commands, kit, meshes, &cm.sign_mat, track);
    barrier_boards(commands, kit, meshes, &cm.sign_mat, track, 260.0);
    marshal_posts(commands, kit, mats, &cm, track, &mut reserved);
    inflatable_arch(commands, kit, meshes, &cm, track, hs + 150.0);
    topiary(commands, kit, surf, mats, track, &mut rng, &mut reserved);
    balloons_and_blimp(commands, kit, meshes, &cm, center, &mut rng);

    spawn_trees(commands, kit, mats, track, 420, 110.0, &reserved, &mut rng);
}

/// A Ferris wheel at `wheel_at`, facing `center`.
fn ferris_wheel(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, wheel_at: Vec2, center: Vec2) {
    let wheel_r = 30.0;
    let rim = meshes.add(Torus { minor_radius: 0.45, major_radius: wheel_r });
    let hub_y = wheel_r + 6.0;
    let face = (center - wheel_at).normalize();
    let wyaw = Quat::from_rotation_y(-face.to_angle() + FRAC_PI_2);
    let bulb_cols = [LinearRgba::rgb(8.0, 2.0, 1.0), LinearRgba::rgb(8.0, 6.0, 1.0), LinearRgba::rgb(1.5, 3.0, 9.0), LinearRgba::rgb(6.0, 1.5, 7.0)];
    let bulbs: Vec<_> = bulb_cols.iter().map(|&c| mats.add(super::models::glow(c))).collect();
    commands
        .spawn((Transform::from_translation(v3(wheel_at, 0.0)).with_rotation(wyaw), Visibility::default(), Scenery))
        .with_children(|w| {
            for z in [-4.0f32, 4.0] {
                for sx in [-1.0f32, 1.0] {
                    let lean = sx * 0.28;
                    part(w, &kit.cylinder, &kit.white, t(sx * hub_y * 0.28 * 0.5, hub_y * 0.5, z).rz(lean).s(0.9, hub_y * 1.04, 0.9));
                }
            }
            part(w, &kit.cylinder, &kit.metal, t(0.0, hub_y, 0.0).rx(FRAC_PI_2).s(2.0, 9.0, 2.0));
            w.spawn((t(0.0, hub_y, 0.0), Visibility::default(), FerrisWheel)).with_children(|r| {
                for z in [-2.5f32, 2.5] {
                    part(r, &rim, &kit.white, t(0.0, 0.0, z).rx(FRAC_PI_2));
                }
                for k in 0..16 {
                    let a = k as f32 / 16.0 * TAU;
                    for z in [-2.5f32, 2.5] {
                        part(r, &kit.cube, &kit.white, Transform::from_xyz(a.cos() * wheel_r * 0.5, a.sin() * wheel_r * 0.5, z).with_rotation(Quat::from_rotation_z(a)).s(wheel_r, 0.35, 0.35));
                    }
                    let gp = Vec3::new(a.cos() * wheel_r, a.sin() * wheel_r, 0.0);
                    r.spawn((Transform::from_translation(gp), Visibility::default(), Gondola)).with_children(|g| {
                        part(g, &kit.cylinder, &kit.metal, t(0.0, -1.0, 0.0).s(0.2, 2.0, 0.2));
                        part(g, &kit.capsule, &kit.kart_colors[k % 8], t(0.0, -3.0, 0.0).s(2.4, 1.2, 2.4));
                        part(g, &kit.cube, &kit.glass, t(0.0, -2.8, 0.0).s(2.5, 1.0, 1.4));
                    });
                    for kk in 0..2 {
                        let b = a + kk as f32 * TAU / 32.0;
                        r.spawn((
                            Mesh3d(kit.sphere.clone()),
                            MeshMaterial3d(bulbs[(k * 2 + kk) % bulbs.len()].clone()),
                            Transform::from_xyz(b.cos() * (wheel_r + 0.9), b.sin() * (wheel_r + 0.9), 3.3).with_scale(Vec3::splat(0.7)),
                            NotShadowCaster,
                        ));
                    }
                }
            });
        });
}

/// A giant sitting corgi wearing a crown, on a plinth at `statue_at`, looking at the start line.
fn crown_statue(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, cm: &CircuitMats, track: &Track, statue_at: Vec2) {
    let (stone, statue_fur, sign_mat) = (cm.stone.clone(), cm.statue_fur.clone(), cm.sign_mat.clone());
    let look = (track.point_at(0.0).0 - statue_at).normalize();
    let syaw = Quat::from_rotation_y(-look.to_angle() - FRAC_PI_2);
    commands
        .spawn((Transform::from_translation(v3(statue_at, 0.0)).with_rotation(syaw), Visibility::default(), Scenery))
        .with_children(|s| {
            part(s, &kit.cylinder, &stone, t(0.0, 1.5, 0.0).s(24.0, 3.0, 24.0));
            part(s, &kit.cylinder, &stone, t(0.0, 3.4, 0.0).s(21.0, 0.8, 21.0));
            s.spawn((t(0.0, 3.8, 0.0).with_scale(Vec3::splat(10.0)), Visibility::default())).with_children(|c| sitting_corgi(c, kit, &kit.corgi.coats[0], &statue_fur, true, true));
        });
    for k in 0..4 {
        let a = look.to_angle() + k as f32 * FRAC_PI_2;
        let dir = Vec2::from_angle(a);
        spawn_board(commands, meshes, &sign_mat, kit, v3(statue_at + dir * 12.1, 0.4), v3(dir, 0.0), [1, 8, 1, 8][k], Vec2::new(9.0, 2.2), false);
    }
}

/// Signs on the bridge, readable from the road underneath.
fn bridge_signs(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, sign_mat: &Handle<StandardMaterial>, track: &Track) {
    if let Some(mid) = crossing_sample(track) {
        let h = track.height_idx(mid);
        for (side, sign) in [(-1.0f32, SIGN_TITLE), (1.0, 2)] {
            let q = at(track, mid, side * (WALL_DIST + WALL_THICK + 0.05));
            let out = v3(track.samples[mid].dir.perp() * side, 0.0);
            spawn_board(commands, meshes, sign_mat, kit, v3(q, h - 1.35), out, sign, Vec2::new(22.0, 2.4), false);
        }
    }
}

/// Advertising boards on top of the barriers along the straights, from arc length `from`.
fn barrier_boards(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, sign_mat: &Handle<StandardMaterial>, track: &Track, from: f32) {
    let n = track.samples.len();
    let mut s = from;
    let mut k = 1;
    while s < track.length - 30.0 {
        let i = (s / track.spacing) as usize % n;
        let straight = (0..9).all(|o| track.samples[(i + o * 3) % n].curvature.abs() < 1.0 / 150.0);
        if straight && track.height_idx(i) < 0.5 {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            spawn_sign(commands, meshes, sign_mat, kit, track, s, side * (WALL_DIST + WALL_THICK * 0.5), 1 + k % (SIGN_ADS - 1), Vec2::new(7.0, 1.5), 1.15);
            k += 1;
            s += 38.0;
        } else {
            s += 10.0;
        }
    }
}

/// Marshal posts with flag-waving corgis and tyre walls on the outside of the big corners.
fn marshal_posts(commands: &mut Commands, kit: &Kit, mats: &mut Assets<StandardMaterial>, cm: &CircuitMats, track: &Track, reserved: &mut Reserved) {
    let (orange, white_paint, tyre_red) = (cm.orange.clone(), cm.white_paint.clone(), cm.tyre_red.clone());
    let n = track.samples.len();
    let flag_cols = [Color::srgb(1.0, 0.85, 0.1), Color::srgb(0.1, 0.4, 0.95), Color::srgb(0.2, 0.8, 0.3)];
    for (ci, corner) in find_corners(track).into_iter().enumerate() {
        let curv = track.samples[corner].curvature;
        let side = -curv.signum(); // outside of the corner
        let lat = side * (WALL_DIST + WALL_THICK + 3.5);
        let q = at(track, corner, lat);
        let dir = track.samples[corner].dir;
        let face = v3(-dir.perp() * side, 0.0);
        let fyaw = Quat::from_rotation_arc(-Vec3::Z, face);
        let flag_mat = mats.add(matte(flag_cols[ci % 3]));
        commands.spawn((Transform::from_translation(v3(q, 0.0)).with_rotation(fyaw), Visibility::default(), Scenery)).with_children(|p| {
            part(p, &kit.cube, &white_paint, t(0.0, 1.0, 1.2).s(2.4, 2.0, 1.6));
            part(p, &kit.cube, &orange, t(0.0, 2.1, 1.2).s(2.6, 0.2, 1.8));
            p.spawn((t(0.0, 0.0, -0.3).with_scale(Vec3::splat(1.1)), Visibility::default())).with_children(|c| spawn_spectator(c, kit, Vec3::ZERO, 0.0, 0));
            p.spawn((t(0.45, 1.1, -0.5), Visibility::default(), WavingFlag(ci as f32))).with_children(|f| {
                part(f, &kit.cylinder, &kit.metal, t(0.0, 0.9, 0.0).s(0.06, 1.8, 0.06));
                part(f, &kit.cube, &flag_mat, t(0.55, 1.5, 0.0).s(1.0, 0.7, 0.04));
            });
        });
        // tyre stacks along the wall around the corner
        for o in (0..40).step_by(4) {
            let i = (corner + n + o - 20) % n;
            let tp = at(track, i, side * (WALL_DIST + WALL_THICK + 1.0));
            commands.spawn((Transform::from_translation(v3(tp, 0.0)), Visibility::default(), Scenery)).with_children(|st| {
                for lvl in 0..3 {
                    let mat = if lvl == 2 { if o % 8 == 0 { &tyre_red } else { &kit.white } } else { &kit.tire };
                    part(st, &kit.torus, mat, t(0.0, 0.22 + lvl as f32 * 0.42, 0.0).s(1.5, 1.8, 1.5));
                }
            });
        }
        reserved.add(q, 4.0);
    }
}

/// An inflatable arch over the track at `arch_s`.
fn inflatable_arch(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, cm: &CircuitMats, track: &Track, arch_s: f32) {
    let (orange, sign_mat) = (cm.orange.clone(), cm.sign_mat.clone());
    let (ac, ad) = track.point_at(arch_s);
    let ah = track.height_at(arch_s);
    // local X runs across the track
    let ayaw = Quat::from_rotation_y(-ad.to_angle() - FRAC_PI_2);
    let arch = meshes.add(Torus { minor_radius: 1.5, major_radius: WALL_DIST + 1.5 });
    commands
        .spawn((Transform::from_translation(v3(ac, ah)).with_rotation(ayaw), Visibility::default(), Scenery))
        .with_children(|a| {
            // the torus lies in XZ; stand it up in the XY plane, spanning the track
            let r = WALL_DIST + 1.5;
            part(a, &arch, &orange, t(0.0, 0.0, 0.0).rx(FRAC_PI_2));
            for k in 0..9 {
                let ang = PI * (k as f32 + 0.5) / 9.0;
                part(a, &kit.sphere, &kit.white, t(ang.cos() * r, ang.sin() * r, 0.0).s(2.2, 2.2, 2.2));
            }
        });
    spawn_board(commands, meshes, &sign_mat, kit, v3(ac, ah + WALL_DIST + 2.8), -v3(ad, 0.0), 5, Vec2::new(10.0, 2.4), false);
}

/// Topiary corgis and giant paw prints mown into the infield.
fn topiary(commands: &mut Commands, kit: &Kit, surf: &SurfaceMats, mats: &mut Assets<StandardMaterial>, track: &Track, rng: &mut Lcg, reserved: &mut Reserved) {
    let (lo, hi) = bounds(track);
    let paw = mats.add(StandardMaterial { base_color: Color::srgb(0.24, 0.48, 0.17), perceptual_roughness: 1.0, ..default() });
    let mut topiary = 0;
    let mut paws = 0;
    let mut tries = 0;
    while (topiary < 12 || paws < 6) && tries < 5000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x, hi.x), rng.range(lo.y, hi.y));
        if paws < 6 && clearance(track, p) > 22.0 && reserved.free(p, 16.0) {
            reserved.add(p, 14.0);
            paws += 1;
            let rot = Quat::from_rotation_y(rng.f() * TAU);
            commands.spawn((Transform::from_translation(v3(p, 0.0)).with_rotation(rot), Visibility::default(), Scenery)).with_children(|pp| {
                part(pp, &kit.cylinder, &paw, t(0.0, 0.0, 2.0).s(10.0, 0.04, 8.5));
                for (x, z) in [(-5.2, -3.6), (-1.8, -6.4), (1.8, -6.4), (5.2, -3.6)] {
                    part(pp, &kit.cylinder, &paw, t(x, 0.0, z).s(3.4, 0.04, 4.2));
                }
            });
        } else if topiary < 12 && clearance(track, p) > 8.0 && reserved.free(p, 5.0) {
            reserved.add(p, 4.0);
            topiary += 1;
            let rot = Quat::from_rotation_y(rng.f() * TAU);
            let a = &surf.hedge;
            commands
                .spawn((Transform::from_translation(v3(p, 0.0)).with_rotation(rot).with_scale(Vec3::splat(rng.range(1.6, 2.4))), Visibility::default(), Scenery))
                .with_children(|c| sitting_corgi(c, kit, &kit.corgi.topiary, a, false, false));
        }
    }
}

/// Corgi hot-air balloons and a blimp drifting overhead.
fn balloons_and_blimp(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, cm: &CircuitMats, center: Vec2, rng: &mut Lcg) {
    let (orange, navy, sign_mat) = (cm.orange.clone(), cm.navy.clone(), cm.sign_mat.clone());
    spawn_balloons(commands, kit, center, 4, true, rng);
    let blimp = Drifter { center, radius: 330.0, height: 140.0, speed: 0.012, phase: 1.0, blimp: true };
    let blimp_board = {
        let mut b = MeshBuilder::default();
        for (side, sign) in [(1.0f32, SIGN_TITLE), (-1.0, 2)] {
            let x = side * 6.9;
            let q = [Vec3::new(x, -2.2, side * 16.0), Vec3::new(x, -2.2, -side * 16.0), Vec3::new(x, 2.2, -side * 16.0), Vec3::new(x, 2.2, side * 16.0)];
            b.quad_uv(q, Vec3::X * side, Color::WHITE, super::signs::sign_uv(sign));
        }
        meshes.add(b.build())
    };
    commands.spawn((Transform::default(), Visibility::default(), blimp, Scenery)).with_children(|b| {
        part(b, &kit.capsule, &orange, t(0.0, 0.0, 0.0).rx(FRAC_PI_2).s(14.0, 30.0, 14.0));
        part(b, &kit.capsule, &kit.white, t(0.0, -3.0, 0.0).rx(FRAC_PI_2).s(10.0, 34.0, 8.0));
        part(b, &kit.cube, &kit.dark, t(0.0, -7.5, -3.0).s(3.0, 2.0, 8.0));
        for (x, y, sx, sy) in [(0.0, 5.5, 0.4, 7.0), (0.0, -5.5, 0.4, 7.0), (5.5, 0.0, 7.0, 0.4), (-5.5, 0.0, 7.0, 0.4)] {
            part(b, &kit.cube, &navy, t(x, y, 21.0).s(sx, sy, 6.0));
        }
        for x in [-3.5f32, 3.5] {
            part(b, &kit.cone, &orange, t(x, 7.5, -12.0).s(4.0, 6.0, 2.0).rz(-x.signum() * 0.3));
        }
        b.spawn((Mesh3d(blimp_board.clone()), MeshMaterial3d(sign_mat.clone()), Transform::default()));
    });
}

/// Corgi-faced hot-air balloons drifting in slow circles around `center`.
/// Without `shadows` they neither cast shadows nor appear in the raytracing scene.
pub(super) fn spawn_balloons(commands: &mut Commands, kit: &Kit, center: Vec2, count: usize, shadows: bool, rng: &mut Lcg) {
    let part = |b: &mut ChildSpawnerCommands, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tr: Transform| {
        let mut e = b.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tr));
        if !shadows {
            e.insert((NotShadowCaster, super::gfx::NoRaytrace));
        }
    };
    for k in 0..count {
        let coat = &kit.coats[k % 3];
        let drift = Drifter { center, radius: rng.range(160.0, 380.0), height: rng.range(70.0, 120.0), speed: rng.range(0.008, 0.02) * if k % 2 == 0 { 1.0 } else { -1.0 }, phase: rng.f() * TAU, blimp: false };
        commands.spawn((Transform::default(), Visibility::default(), drift, Scenery)).with_children(|b| {
            part(b, &kit.sphere, coat, t(0.0, 0.0, 0.0).s(16.0, 15.0, 15.0));
            part(b, &kit.sphere, &kit.white, t(0.0, -2.5, -6.0).s(8.0, 6.0, 6.0));
            part(b, &kit.sphere, &kit.black, t(0.0, -1.2, -9.0).s(2.0, 1.5, 1.5));
            for x in [-3.0f32, 3.0] {
                part(b, &kit.sphere, &kit.black, t(x, 2.0, -6.8).s(1.4, 1.6, 1.0));
                part(b, &kit.cone, coat, t(x * 1.8, 8.5, 0.0).s(6.0, 9.0, 3.0).rz(-x.signum() * 0.35));
                part(b, &kit.cone, &kit.pink, t(x * 1.75, 8.0, -0.9).s(3.6, 6.0, 1.0).rz(-x.signum() * 0.35));
            }
            part(b, &kit.cube, &kit.biscuit, t(0.0, -14.0, 0.0).s(3.0, 2.0, 3.0));
            for (x, z) in [(-1.3, -1.3), (1.3, -1.3), (-1.3, 1.3), (1.3, 1.3)] {
                part(b, &kit.cylinder, &kit.dark, t(x * 1.6, -10.5, z * 1.6).s(0.08, 6.5, 0.08));
            }
        });
    }
}

/// Sample index of the tightest corner between two fractions of the lap.
pub(super) fn find_tightest(track: &Track, f0: f32, f1: f32) -> usize {
    let n = track.samples.len();
    let (a, b) = ((f0 * n as f32) as usize, (f1 * n as f32) as usize);
    (a..b).max_by(|&i, &j| track.samples[i].curvature.abs().total_cmp(&track.samples[j].curvature.abs())).unwrap_or(a)
}

/// Apexes of the notable corners (local curvature peaks, well spaced).
fn find_corners(track: &Track) -> Vec<usize> {
    let n = track.samples.len();
    let mut out: Vec<usize> = vec![];
    let mut idx: Vec<usize> = (0..n).filter(|&i| track.samples[i].curvature.abs() > 1.0 / 45.0).collect();
    idx.sort_by(|&a, &b| track.samples[b].curvature.abs().total_cmp(&track.samples[a].curvature.abs()));
    for i in idx {
        let spaced = out.iter().all(|&o| {
            let d = (o as isize - i as isize).unsigned_abs();
            d.min(n - d) as f32 * track.spacing > 140.0
        });
        if spaced && track.height_idx(i) < 0.5 && i > 20 {
            out.push(i);
        }
    }
    out
}

/// The sample on the bridge closest to the road it crosses.
fn crossing_sample(track: &Track) -> Option<usize> {
    let n = track.samples.len();
    let mut best: Option<(usize, f32)> = None;
    for i in 0..n {
        let s = &track.samples[i];
        if s.height < 6.0 {
            continue;
        }
        for (j, o) in track.samples.iter().enumerate().step_by(2) {
            let arc = (j as isize - i as isize).unsigned_abs();
            if arc.min(n - arc) as f32 * track.spacing > 70.0 && o.height < 1.0 {
                let d = o.pos.distance(s.pos);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((i, d));
                }
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Nearest point to `want` that's at least `r` from the track and reserved areas.
pub(super) fn best_spot(track: &Track, want: Vec2, r: f32, reserved: &Reserved) -> Vec2 {
    find_spot(track, want, r, reserved, 20).unwrap_or(want)
}

/// Nearest point to `want`, within `reach` steps of 5m, that's at least `r` from the track and
/// reserved areas.
pub(super) fn find_spot(track: &Track, want: Vec2, r: f32, reserved: &Reserved, reach: i32) -> Option<Vec2> {
    let mut best = None;
    let mut best_d = f32::MAX;
    for gx in -reach..=reach {
        for gz in -reach..=reach {
            let p = want + Vec2::new(gx as f32, gz as f32) * 5.0;
            if clearance(track, p) > r && reserved.free(p, r) {
                let d = p.distance(want);
                if d < best_d {
                    best_d = d;
                    best = Some(p);
                }
            }
        }
    }
    best
}

/// Whether the strip from `lat0` to `lat1` metres out on `side`, between arc lengths `s0` and
/// `s1`, is clear of the rest of the track: for grandstands on custom tracks. It has to be beside
/// road at ground level, away from fords and not round the inside of a tight bend; with
/// `straight`, not round any bend.
pub(super) fn strip_fits(track: &Track, s0: f32, s1: f32, side: f32, lat0: f32, lat1: f32, straight: bool) -> bool {
    let n = track.samples.len();
    let mut s = s0;
    while s <= s1 {
        let i = (track.wrap_s(s) / track.spacing) as usize % n;
        let here = &track.samples[i];
        let bend = here.curvature * side;
        let too_bent = if straight { here.curvature.abs() > 1.0 / 400.0 } else { bend > 1.0 / (lat1 + 40.0) || here.curvature.abs() > 1.0 / 35.0 };
        if here.height > 0.3 || track.in_water(s) || too_bent {
            return false;
        }
        for lat in [lat0, (lat0 + lat1) * 0.5, lat1] {
            let p = here.pos + here.dir.perp() * side * lat;
            let hit = track.samples.iter().enumerate().step_by(2).any(|(j, o)| {
                let arc = (j as isize - i as isize).unsigned_abs();
                arc.min(n - arc) as f32 * track.spacing > lat1 + 20.0
                    && o.pos.distance(p) < WALL_DIST + WALL_THICK + BANK_SLOPE * o.height + 3.0
            });
            if hit {
                return false;
            }
        }
        s += 5.0;
    }
    true
}

/// How far out from the centre line a grandstand reaches (see `spawn_grandstand`).
const STAND_LAT: (f32, f32) = (WALL_DIST + WALL_THICK + 2.0, WALL_DIST + WALL_THICK + 4.0 + 7.0 * 1.7 + 1.0);

/// The first stretch `len` long, from arc length `from` on, with room for a grandstand.
pub(super) fn find_stand(track: &Track, len: f32, from: f32) -> Option<(f32, f32, f32)> {
    let mut s = from;
    while s + len < track.length - 30.0 {
        for side in [-1.0, 1.0] {
            if strip_fits(track, s, s + len, side, STAND_LAT.0, STAND_LAT.1, false) {
                return Some((s, s + len, side));
            }
        }
        s += 10.0;
    }
    None
}

/// Corgi Circuit's set pieces fitted round any layout, for custom tracks.
#[allow(clippy::too_many_arguments)]
pub fn spawn_circuit_custom(commands: &mut Commands, kit: &Kit, signs: &SignAtlas, surf: &SurfaceMats, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track) {
    let mut rng = Lcg(0xC1C017 ^ track.length as u32);
    let mut reserved = Reserved(vec![]);
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;
    let cm = CircuitMats::new(mats, signs);

    // The biggest grandstand that fits near the start, and one round the outside of the
    // tightest corner.
    let main = [200.0, 140.0, 90.0].into_iter().find_map(|len| find_stand(track, len, 12.0));
    if let Some((s0, s1, side)) = main {
        spawn_grandstand(commands, kit, meshes, surf, track, s0, s1, side, &mut rng, &mut reserved);
    }
    let tight = find_tightest(track, 0.05, 0.95);
    let hs = track.samples[tight].s;
    let outside = -track.samples[tight].curvature.signum();
    let clash = main.is_some_and(|(s0, s1, _)| hs + 45.0 > s0 && hs - 45.0 < s1);
    if !clash && strip_fits(track, hs - 45.0, hs + 45.0, outside, STAND_LAT.0, STAND_LAT.1, false) {
        spawn_grandstand(commands, kit, meshes, surf, track, hs - 45.0, hs + 45.0, outside, &mut rng, &mut reserved);
    }

    spawn_gantry(commands, kit, meshes, &cm.sign_mat, track, &cm.navy, &cm.orange, SIGN_TITLE);

    // A Ferris wheel behind the main grandstand, or wherever there's room.
    let want = main.map_or(center, |(s0, s1, side)| {
        let (c, d) = track.point_at((s0 + s1) * 0.5);
        c + d.perp() * side * 80.0
    });
    if let Some(at) = find_spot(track, want, 36.0, &reserved, 30) {
        reserved.add(at, 40.0);
        ferris_wheel(commands, kit, meshes, mats, at, center);
    }
    if let Some(at) = find_spot(track, center, 30.0, &reserved, 30) {
        reserved.add(at, 22.0);
        crown_statue(commands, kit, meshes, &cm, track, at);
    }
    bridge_signs(commands, kit, meshes, &cm.sign_mat, track);
    barrier_boards(commands, kit, meshes, &cm.sign_mat, track, main.map_or(40.0, |m| m.1 + 20.0));
    marshal_posts(commands, kit, mats, &cm, track, &mut reserved);
    let arch_s = hs + 150.0;
    if (-5..=5).all(|k| !track.in_water(arch_s + k as f32)) {
        inflatable_arch(commands, kit, meshes, &cm, track, arch_s);
    }
    topiary(commands, kit, surf, mats, track, &mut rng, &mut reserved);
    balloons_and_blimp(commands, kit, meshes, &cm, center, &mut rng);

    spawn_trees(commands, kit, mats, track, 420, 110.0, &reserved, &mut rng);
}

/// Start gantry with F1-style start lights, a giant bone and the circuit name.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_gantry(commands: &mut Commands, kit: &Kit, meshes: &mut Assets<Mesh>, sign_mat: &Handle<StandardMaterial>, track: &Track, navy: &Handle<StandardMaterial>, orange: &Handle<StandardMaterial>, title: usize) {
    let (c, d) = track.point_at(0.0);
    let yaw = Quat::from_rotation_y(-d.to_angle() - FRAC_PI_2);
    let span = WALL_DIST + 1.5;
    let h0 = track.height_at(0.0);
    commands
        .spawn((Transform::from_translation(v3(c, h0)).with_rotation(yaw), Visibility::default(), Scenery))
        .with_children(|g| {
            for x in [-span, span] {
                part(g, &kit.cube, navy, t(x, 5.5, 0.0).s(1.4, 11.0, 1.4));
                part(g, &kit.cube, orange, t(x, 11.2, 0.0).s(1.8, 0.4, 1.8));
            }
            part(g, &kit.cube, navy, t(0.0, 9.6, 0.0).s(span * 2.0 + 1.4, 2.6, 1.2));
            // five light pods facing oncoming karts (-Z is the direction of travel)
            for k in 0..5 {
                let x = (k as f32 - 2.0) * 1.6;
                part(g, &kit.cube, &kit.black, t(x, 7.7, -0.3).s(1.1, 1.6, 0.8));
                for (row, y) in [(0, 8.1), (1, 7.3)] {
                    g.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(kit.dark.clone()), t(x, y, 0.15).s(0.55, 0.55, 0.3), StartLight(k * 2 + row)));
                }
            }
            g.spawn((t(0.0, 12.8, 0.0).with_scale(Vec3::splat(3.4)), Visibility::default())).with_children(|b| bone_shape(b, kit));
        });
    // Title boards on both faces of the gantry
    let gantry_up = v3(c, h0 + 10.95 - 2.6);
    spawn_board(commands, meshes, sign_mat, kit, gantry_up + v3(d, 0.0) * 0.7, v3(d, 0.0), title, Vec2::new(16.0, 2.2), false);
    spawn_board(commands, meshes, sign_mat, kit, gantry_up - v3(d, 0.0) * 0.7, -v3(d, 0.0), title, Vec2::new(7.0, 1.0), false);

}

// ----- Prism Park ---------------------------------------------------------------------------

/// The night plaza: a polished tiled ground, a starry sky and a lit-up skyline.
fn spawn_night_common(commands: &mut Commands, kit: &Kit, windows: &Handle<Image>, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track, rng: &mut Lcg) {
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;

    // Polished dark stone plaza, one big tiled mesh.
    let mut ground = MeshBuilder::default();
    let half = 1400.0;
    let cell = 50.0;
    let mut z = center.y - half;
    while z < center.y + half {
        let mut x = center.x - half;
        while x < center.x + half {
            let q = [Vec2::new(x, z), Vec2::new(x, z + cell), Vec2::new(x + cell, z + cell), Vec2::new(x + cell, z)];
            ground.quad_uv(q.map(|c| v3(c, -0.03)), Vec3::Y, Color::WHITE, q.map(|c| c / 6.0));
            x += cell;
        }
        z += cell;
    }
    let tiles = mats.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.08), perceptual_roughness: 0.08, reflectance: 0.6, ..default() });
    commands.spawn((Mesh3d(meshes.add(ground.build())), MeshMaterial3d(tiles), NotShadowCaster, super::gfx::RaytraceSink(0.1), Scenery));

    // Stars: tiny glowing quads on a far sphere that follows the camera.
    let mut stars = MeshBuilder::default();
    for _ in 0..1800 {
        let d = Vec3::new(rng.range(-1.0, 1.0), rng.range(0.05, 1.0), rng.range(-1.0, 1.0)).normalize();
        let p = d * 2600.0;
        let right = d.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(d);
        let s = rng.range(1.5, 5.0);
        let (r, u) = (right * s, up * s);
        stars.quad_uv([p - r - u, p + r - u, p + r + u, p - r + u], -d, Color::WHITE, [Vec2::ZERO; 4]);
    }
    let star_mat = mats.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, cull_mode: None, alpha_mode: AlphaMode::Blend, ..default() });
    commands.spawn((Mesh3d(meshes.add(stars.build())), MeshMaterial3d(star_mat), NotShadowCaster, super::gfx::NoRaytrace, Transform::default(), SkyFollow, Scenery));

    // A skyline of towers with lit windows all around the park.
    let far = (hi - lo).max_element() * 0.5 + 260.0;
    let window_mat = |mats: &mut Assets<StandardMaterial>, k: usize| {
        mats.add(StandardMaterial {
            base_color: [Color::srgb(0.08, 0.08, 0.12), Color::srgb(0.1, 0.07, 0.14), Color::srgb(0.06, 0.09, 0.12)][k % 3],
            perceptual_roughness: 0.15,
            reflectance: 0.7,
            emissive: LinearRgba::rgb(3.0, 3.0, 3.0),
            emissive_texture: Some(windows.clone()),
            ..default()
        })
    };
    let building_mats: Vec<_> = (0..3).map(|k| window_mat(mats, k)).collect();
    for k in 0..46 {
        let a = k as f32 / 46.0 * std::f32::consts::TAU + rng.range(-0.05, 0.05);
        let r = far + rng.range(0.0, 260.0);
        let p = center + Vec2::from_angle(a) * r;
        let (w, h, dpt) = (rng.range(30.0, 60.0), rng.range(60.0, 220.0), rng.range(30.0, 60.0));
        let rot = Quat::from_rotation_y(-a);
        commands
            .spawn((Transform::from_translation(v3(p, 0.0)).with_rotation(rot), Visibility::default(), Scenery))
            .with_children(|b| {
                b.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(building_mats[k % 3].clone()), Transform::from_xyz(0.0, h * 0.5, 0.0).with_scale(Vec3::new(dpt, h, w))));
                // antenna light on top
                b.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(kit.lamp_red.clone()), Transform::from_xyz(0.0, h + 3.0, 0.0).with_scale(Vec3::splat(2.5)), NotShadowCaster));
            });
    }
}

/// Prism Park set pieces: gantry, neon grandstand, rainbow arches, crystal clusters, a glass
/// corgi, and lanterns floating over the plaza.
#[allow(clippy::too_many_arguments)]
pub fn spawn_prism(commands: &mut Commands, kit: &Kit, signs: &SignAtlas, surf: &SurfaceMats, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, track: &Track, raytracing: bool) {
    use super::trackmesh::rainbow;
    let mut rng = Lcg(0x9815_3);
    let mut reserved = Reserved(vec![]);
    let (lo, hi) = bounds(track);
    let n = track.samples.len();
    let sign_mat = signs.material.clone();
    let black = mats.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.05), perceptual_roughness: 0.1, reflectance: 0.7, ..default() });
    let neon_cyan = mats.add(super::models::glow(LinearRgba::rgb(0.6, 8.0, 10.0)));
    spawn_gantry(commands, kit, meshes, &sign_mat, track, &black, &neon_cyan, super::signs::SIGN_PRISM);

    // A neon grandstand of corgi fans along the start straight.
    if !matches!(track.id, TrackId::Custom(_)) {
        spawn_grandstand(commands, kit, meshes, surf, track, 20.0, 140.0, -1.0, &mut rng, &mut reserved);
    } else if let Some((s0, s1, side)) = find_stand(track, 120.0, 12.0) {
        spawn_grandstand(commands, kit, meshes, surf, track, s0, s1, side, &mut rng, &mut reserved);
    }
    // Keep the canal at the ford clear (see `spawn_prism_track`).
    for &(ws, len) in &track.water {
        let (c, d) = track.point_at(ws + len * 0.5);
        for k in -3..=3 {
            reserved.add(c + d.perp() * k as f32 * 25.0, 30.0);
        }
    }

    // Rainbow arches over the track: emissive rings that light the road with raytracing, plus
    // point lights for the standard renderer.
    let ring = meshes.add(Torus { minor_radius: 0.55, major_radius: WALL_DIST + 1.0 });
    let mut s = 60.0;
    let mut k = 0;
    while s < track.length - 40.0 {
        let in_water = track.in_water(s) || track.in_water(s + 20.0);
        if !in_water {
            let (c, d) = track.point_at(s);
            let h = track.height_at(s);
            let hue = rainbow(k);
            let mat = mats.add(super::models::glow(hue.to_linear() * 10.0));
            commands.spawn((
                Mesh3d(ring.clone()),
                MeshMaterial3d(mat),
                // ring in local XY; yaw so local X runs across the track
                Transform::from_translation(v3(c, h)).with_rotation(Quat::from_rotation_y(-d.to_angle() - FRAC_PI_2) * Quat::from_rotation_x(FRAC_PI_2)),
                NotShadowCaster,
                Scenery,
            ));
            if !raytracing {
                commands.spawn((
                    PointLight { color: hue, intensity: 3_000_000.0, range: 45.0, radius: 1.0, shadow_maps_enabled: false, ..default() },
                    Transform::from_translation(v3(c, h + WALL_DIST * 0.8)),
                    Scenery,
                ));
            }
            k += 1;
        }
        s += 95.0;
    }

    // Crystal clusters: glass shards with glowing cores.
    let prism_mesh = meshes.add(Cylinder::new(0.5, 1.0).mesh().resolution(6));
    let tip = meshes.add(Cone { radius: 0.5, height: 1.0 }.mesh().resolution(6));
    let mut clusters = 0;
    let mut tries = 0;
    while clusters < 26 && tries < 4000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 80.0, hi.x + 80.0), rng.range(lo.y - 80.0, hi.y + 80.0));
        if clearance(track, p) < 8.0 || !reserved.free(p, 10.0) {
            continue;
        }
        clusters += 1;
        reserved.add(p, 8.0);
        let hue = rainbow(clusters);
        let glass = mats.add(StandardMaterial {
            base_color: hue.mix(&Color::WHITE, 0.5).with_alpha(0.28),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.02,
            reflectance: 0.9,
            emissive: hue.to_linear() * 0.3,
            ..default()
        });
        let core = mats.add(super::models::glow(hue.to_linear() * 8.0));
        let rot = Quat::from_rotation_y(rng.f() * TAU);
        commands.spawn((Transform::from_translation(v3(p, 0.0)).with_rotation(rot), Visibility::default(), Scenery)).with_children(|c| {
            for j in 0..rng.range(3.0, 6.0) as usize {
                let h = rng.range(4.0, 13.0) * if j == 0 { 1.4 } else { 1.0 };
                let r = rng.range(1.0, 2.2);
                let tilt = Quat::from_rotation_z(rng.range(-0.35, 0.35)) * Quat::from_rotation_x(rng.range(-0.35, 0.35));
                let off = Vec3::new(rng.range(-2.5, 2.5), 0.0, rng.range(-2.5, 2.5));
                c.spawn((Transform::from_translation(off).with_rotation(tilt), Visibility::default())).with_children(|sh| {
                    sh.spawn((Mesh3d(prism_mesh.clone()), MeshMaterial3d(glass.clone()), Transform::from_xyz(0.0, h * 0.5, 0.0).with_scale(Vec3::new(r, h, r)), NotShadowCaster));
                    sh.spawn((Mesh3d(tip.clone()), MeshMaterial3d(glass.clone()), Transform::from_xyz(0.0, h + r * 0.6, 0.0).with_scale(Vec3::new(r, r * 1.2, r)), NotShadowCaster));
                    sh.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(core.clone()), Transform::from_xyz(0.0, h * 0.5, 0.0).with_scale(Vec3::new(r * 0.25, h * 0.9, r * 0.25)), NotShadowCaster));
                });
            }
        });
    }

    // A giant glass corgi with a glowing heart, on a mirror plinth.
    let corgi_at = best_spot(track, (lo + hi) * 0.5 + Vec2::new(-20.0, 30.0), 22.0, &reserved);
    reserved.add(corgi_at, 18.0);
    let glass_fur = mats.add(StandardMaterial {
        // tinted orange and white by the sculpt's vertex colours
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.32),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.02,
        reflectance: 0.9,
        emissive: LinearRgba::rgb(0.25, 0.15, 0.08),
        ..default()
    });
    let heart = mats.add(super::models::glow(LinearRgba::rgb(14.0, 1.5, 4.0)));
    let look = (track.point_at(0.0).0 - corgi_at).normalize();
    commands
        .spawn((Transform::from_translation(v3(corgi_at, 0.0)).with_rotation(Quat::from_rotation_y(-look.to_angle() - FRAC_PI_2)), Visibility::default(), Scenery))
        .with_children(|s| {
            part(s, &kit.cylinder, &black, t(0.0, 1.0, 0.0).s(22.0, 2.0, 22.0));
            part(s, &kit.torus, &neon_cyan, t(0.0, 2.0, 0.0).s(13.4, 0.5, 13.4));
            s.spawn((t(0.0, 2.0, 0.0).with_scale(Vec3::splat(8.0)), Visibility::default())).with_children(|c| {
                sitting_corgi(c, kit, &kit.corgi.coats[0], &glass_fur, true, false);
                part(c, &kit.sphere, &heart, t(0.0, 1.15, -0.1).s(0.35, 0.35, 0.3));
            });
        });

    // Paper lanterns floating over the plaza.
    let warm = [LinearRgba::rgb(9.0, 4.0, 1.2), LinearRgba::rgb(8.0, 2.0, 4.0), LinearRgba::rgb(3.0, 6.0, 9.0)];
    let lantern_mats: Vec<_> = warm.iter().map(|&c| mats.add(super::models::glow(c))).collect();
    for j in 0..60 {
        let p = Vec2::new(rng.range(lo.x - 40.0, hi.x + 40.0), rng.range(lo.y - 40.0, hi.y + 40.0));
        if clearance(track, p) < 3.0 {
            continue;
        }
        let base = v3(p, rng.range(9.0, 30.0));
        commands.spawn((
            Mesh3d(kit.capsule.clone()),
            MeshMaterial3d(lantern_mats[j % 3].clone()),
            Transform::from_translation(base).with_scale(Vec3::new(1.1, 0.8, 1.1)),
            Bob { base, phase: rng.f() * TAU },
            NotShadowCaster,
            Scenery,
        ));
    }

    // Boards on top of the glass barriers along the straights.
    let mut s = 180.0;
    let mut k = 1;
    while s < track.length - 30.0 {
        let i = (s / track.spacing) as usize % n;
        let straight = (0..9).all(|o| track.samples[(i + o * 3) % n].curvature.abs() < 1.0 / 150.0);
        if straight && !track.in_water(s) {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            spawn_sign(commands, meshes, &sign_mat, kit, track, s, side * (WALL_DIST + 0.2), [11, 1, 5, 7, 8][k % 5], Vec2::new(7.0, 1.5), 1.35);
            k += 1;
            s += 70.0;
        } else {
            s += 10.0;
        }
    }
}
