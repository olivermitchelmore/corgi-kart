//! The kart: a sculpted, lacquered body with a livery (twin racing stripes and a paw-print
//! roundel), chunky treaded tyres on chrome five-spoke rims, an exposed engine with twin
//! exhausts, and lights. Headlights, a brake light bar and neon underglow in the kart's colour
//! (which takes on the colour of the drift sparks) are real light sources in the raytraced
//! renderer, so they pool on the road and shine in its reflections.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::lod::{Lods, LOD_CELL_SCALE};
use super::models::*;
use super::sculpt::{cap, ell, rbox, spawn_inked, v, Sculpt, Sculpted, Shape};
use crate::game::{AppState, RaceEntity, SessionRes};
use crate::sim::kart::drift_stage;

/// Body inks.
const PAINT: u8 = 0;
const STRIPE: u8 = 1;
const CARBON: u8 = 2;
const WHITE: u8 = 3;
/// Tyre inks.
const RUBBER: u8 = 0;
const LETTERING: u8 = 1;

pub const CARBON_COLOR: Color = Color::srgb(0.035, 0.035, 0.04);
const WALL_COLOR: Color = Color::srgb(1.0, 0.82, 0.15);

/// Headlight lenses (right side; mirrored for the left).
const HEADLIGHT: Vec3 = Vec3::new(0.3, 0.44, -1.69);

/// Wheel radius and width, front and rear.
const FRONT_WHEEL: (f32, f32) = (0.36, 0.36);
const REAR_WHEEL: (f32, f32) = (0.44, 0.46);

fn body(s: &mut Sculpt) {
    // the tub, flowing into a long rounded nose
    s.add(rbox(v(0.0, 0.42, 0.15), v(0.6, 0.2, 0.95), 0.16), PAINT, 0.0);
    s.add(ell(v(0.0, 0.4, -1.1), v(0.52, 0.21, 0.72)), PAINT, 0.3);
    s.add(ell(v(0.0, 0.3, -1.55), v(0.44, 0.13, 0.32)), PAINT, 0.15);
    // a raised cowl ahead of the cockpit
    s.add(ell(v(0.0, 0.6, -0.55), v(0.4, 0.16, 0.42)), PAINT, 0.15);
    // side pods between the wheels, with intakes
    s.add(rbox(v(0.72, 0.36, -0.12), v(0.2, 0.15, 0.42), 0.12), PAINT, 0.12).mirror(1);
    s.sub(ell(v(0.74, 0.38, -0.56), v(0.13, 0.075, 0.12)), CARBON, 0.03).mirror(1);
    // engine deck with a ducktail lip
    s.add(rbox(v(0.0, 0.48, 1.2), v(0.52, 0.16, 0.3), 0.12), PAINT, 0.1);
    s.add(Shape::RoundBox(v(0.0, 0.66, 1.46), v(0.54, 0.03, 0.1), 0.03, Quat::from_rotation_x(-0.35)), CARBON, 0.06);
    // the cockpit, lined in carbon
    s.sub(rbox(v(0.0, 0.74, 0.32), v(0.45, 0.36, 0.62), 0.14), CARBON, 0.05);
    // headlight pods
    s.sub(ell(HEADLIGHT + v(0.0, 0.0, 0.01), v(0.13, 0.11, 0.1)), CARBON, 0.03).mirror(1);
    // floor pan, splitter and rear bumper
    s.add(rbox(v(0.0, 0.2, -0.1), v(0.62, 0.05, 1.5), 0.04), CARBON, 0.03);
    s.add(rbox(v(0.0, 0.18, -1.72), v(0.5, 0.03, 0.2), 0.03), CARBON, 0.02);
    s.add(rbox(v(0.0, 0.26, 1.55), v(0.6, 0.07, 0.08), 0.05), CARBON, 0.03);

    // livery: twin racing stripes down the nose, a roundel with a paw print on each pod
    s.paint(rbox(v(0.1, 0.9, -1.15), v(0.045, 0.5, 0.8), 0.0), STRIPE).mirror(1);
    let across = Quat::from_rotation_z(FRAC_PI_2);
    let pod = v(0.92, 0.37, -0.12);
    s.paint(Shape::Cylinder(pod, 0.15, 0.1, 0.0, across), WHITE).mirror(1);
    s.paint(Shape::Cylinder(pod + v(0.0, -0.035, 0.0), 0.055, 0.1, 0.0, across), CARBON).mirror(1);
    for (y, z) in [(0.03, -0.075), (0.065, -0.03), (0.065, 0.03), (0.03, 0.075)] {
        s.paint(Shape::Cylinder(pod + v(0.0, y, z), 0.026, 0.1, 0.0, across), CARBON).mirror(1);
    }
}

/// Everything chrome: engine, exhausts, bumper bar and headlight bezels.
fn chrome(s: &mut Sculpt) {
    // the engine block, finned, with twin velocity stacks
    s.add(rbox(v(0.0, 0.7, 1.16), v(0.24, 0.09, 0.16), 0.04), 0, 0.0);
    for i in 0..4 {
        s.add(rbox(v(0.0, 0.7, 1.04 + i as f32 * 0.08), v(0.29, 0.06, 0.012), 0.008), 0, 0.0);
    }
    for x in [-0.11f32, 0.11] {
        s.add(Shape::Cylinder(v(x, 0.83, 1.14), 0.065, 0.08, 0.015, Quat::IDENTITY), 0, 0.02);
        s.cut(Shape::Cylinder(v(x, 0.92, 1.14), 0.045, 0.08, 0.0, Quat::IDENTITY), 0.0);
    }
    // twin exhausts out through the tail, flared and hollow
    let along = Quat::from_rotation_x(FRAC_PI_2);
    s.add(cap(v(0.26, 0.44, 1.2), v(0.3, 0.44, 1.76), 0.06, 0.08), 0, 0.0).mirror(1);
    s.cut(Shape::Cylinder(v(0.3, 0.44, 1.84), 0.058, 0.1, 0.0, along), 0.0).mirror(1);
    // front bumper bar
    s.add(cap(v(-0.42, 0.25, -1.9), v(0.42, 0.25, -1.9), 0.04, 0.04), 0, 0.0);
    s.add(cap(v(0.42, 0.25, -1.9), v(0.6, 0.25, -1.62), 0.04, 0.04), 0, 0.03).mirror(1);
    // headlight bezels
    s.add(Shape::Torus(HEADLIGHT, 0.1, 0.02, along), 0, 0.0).mirror(1);
}

/// A three-spoke wheel on its column, facing the driver.
fn steering(s: &mut Sculpt) {
    s.add(Shape::Torus(Vec3::ZERO, 0.19, 0.028, Quat::IDENTITY), 0, 0.0);
    s.add(Shape::Cylinder(Vec3::ZERO, 0.055, 0.03, 0.012, Quat::IDENTITY), 0, 0.02);
    for a in [0.0f32, 2.1, 4.2] {
        let d = Vec3::new(a.sin(), 0.0, a.cos());
        s.add(cap(d * 0.04, d * 0.18, 0.022, 0.018), 0, 0.02);
    }
    s.add(cap(v(0.0, -0.02, 0.0), v(0.0, -0.5, 0.0), 0.03, 0.035), 0, 0.02);
}

/// Neon tubes under the pods, nose and tail.
fn underglow(s: &mut Sculpt) {
    s.add(cap(v(0.7, 0.13, -0.55), v(0.7, 0.13, 0.32), 0.04, 0.04), 0, 0.0).mirror(1);
    s.add(cap(v(-0.36, 0.12, -1.66), v(0.36, 0.12, -1.66), 0.035, 0.035), 0, 0.0);
    s.add(cap(v(-0.42, 0.16, 1.44), v(0.42, 0.16, 1.44), 0.035, 0.035), 0, 0.0);
}

/// Racing goggles pushed up on the forehead, in the head's frame: brass-rimmed cups with a
/// leather strap round the back of the head.
const GOGGLE: Vec3 = Vec3::new(0.105, 0.19, -0.2);
const STRAP: u8 = 0;
const BRASS: u8 = 1;

fn goggle_axis() -> Vec3 {
    Vec3::new(0.3, 0.72, -0.62).normalize()
}

fn goggles(s: &mut Sculpt) {
    let up = Quat::from_rotation_arc(Vec3::Y, goggle_axis());
    // the strap: a thin slice of a skull slightly bigger than the corgi's
    #[allow(clippy::approx_constant)] // 0.318 is a width, not 1/pi
    s.add(ell(v(0.0, 0.02, 0.02), v(0.318, 0.29, 0.31)), STRAP, 0.0);
    s.intersect(Shape::RoundBox(v(0.0, 0.135, 0.02), v(0.5, 0.026, 0.5), 0.012, Quat::from_rotation_x(0.32)), 0.0);
    s.cut(ell(v(0.0, 0.02, 0.02), v(0.28, 0.25, 0.27)), 0.0);
    s.add(Shape::Cylinder(GOGGLE, 0.078, 0.03, 0.014, up), BRASS, 0.01).mirror(1);
    s.add(Shape::Torus(GOGGLE + goggle_axis() * 0.03, 0.07, 0.014, up), BRASS, 0.0).mirror(1);
    s.add(cap(GOGGLE * v(-1.0, 1.0, 1.0) + v(0.07, 0.01, -0.02), GOGGLE + v(-0.07, 0.01, -0.02), 0.016, 0.016), BRASS, 0.01);
}

/// A bandana round the neck, knotted at the back (driver's frame).
fn bandana(s: &mut Sculpt) {
    s.add(Shape::Torus(v(0.0, 0.8, -0.22), 0.225, 0.055, Quat::from_rotation_x(0.25)), 0, 0.0);
    s.add(ell(v(0.0, 0.8, -0.2), v(0.25, 0.1, 0.25)), 0, 0.04);
    s.cut(ell(v(0.0, 0.8, -0.22), v(0.19, 0.3, 0.19)), 0.02);
    // the knot, and a triangle of cloth down the chest
    s.add(ell(v(0.0, 0.86, 0.01), v(0.07, 0.06, 0.06)), 0, 0.02);
    s.add(ell(v(0.0, 0.66, -0.42), v(0.16, 0.12, 0.04)), 0, 0.06);
}

/// One tail of the bandana's knot, trailing along +Z from its root.
fn bandana_tail(s: &mut Sculpt) {
    s.add(ell(v(0.0, 0.0, 0.2), v(0.07, 0.015, 0.22)), 0, 0.0);
    s.add(ell(v(0.0, -0.01, 0.38), v(0.05, 0.013, 0.09)), 0, 0.04);
}

/// A tyre (axle along X, outer face +X) with chevron tread and a lettered sidewall.
fn tyre(s: &mut Sculpt, r: f32, w: f32) {
    let axle = Quat::from_rotation_z(FRAC_PI_2);
    s.add(Shape::Cylinder(Vec3::ZERO, r, w * 0.5, w * 0.24, axle), RUBBER, 0.0);
    s.cut(Shape::Cylinder(Vec3::ZERO, r * 0.64, w, 0.0, axle), 0.02);
    let n = (r * 48.0) as usize;
    for i in 0..n {
        let spin = Quat::from_rotation_x(i as f32 / n as f32 * TAU);
        for side in [-1.0f32, 1.0] {
            let rot = spin * Quat::from_rotation_y(side * 0.55);
            s.cut(Shape::RoundBox(spin * v(side * w * 0.2, r, 0.0), v(w * 0.19, 0.022, 0.015), 0.006, rot), 0.004);
        }
    }
    s.paint(Shape::Torus(v(w * 0.5, 0.0, 0.0), r * 0.8, 0.018, axle), LETTERING).mirror(1);
}

/// A chrome five-spoke rim to sit in `tyre(r, w)`.
fn rim(s: &mut Sculpt, r: f32, w: f32) {
    let axle = Quat::from_rotation_z(FRAC_PI_2);
    let (rr, face) = (r * 0.66, w * 0.4);
    s.add(Shape::Cylinder(Vec3::ZERO, rr, face, 0.02, axle), 0, 0.0);
    s.cut(Shape::Cylinder(v(face, 0.0, 0.0), rr * 0.84, 0.07, 0.0, axle), 0.012);
    for i in 0..5 {
        let spin = Quat::from_rotation_x(i as f32 / 5.0 * TAU);
        s.add(Shape::RoundBox(spin * v(face - 0.04, rr * 0.45, 0.0), v(0.03, rr * 0.45, 0.028), 0.012, spin), 0, 0.02);
    }
    s.add(Shape::Cylinder(v(face - 0.02, 0.0, 0.0), rr * 0.28, 0.035, 0.015, axle), 0, 0.02);
}

pub struct KartMeshes {
    /// Body per kart colour, with the matching raytracing materials per ink.
    bodies: Vec<Lods>,
    body_rt: Vec<(u8, Handle<Mesh>)>,
    body_rt_mats: Vec<Vec<Handle<StandardMaterial>>>,
    body_mat: Handle<StandardMaterial>,
    chrome: Lods,
    chrome_mat: Handle<StandardMaterial>,
    steering: Lods,
    underglow: Lods,
    /// Neon per kart; `animate_underglow` drives them.
    glow_mats: Vec<Handle<StandardMaterial>>,
    /// Front, rear.
    tyres: [Lods; 2],
    tyre_rt: [Vec<(u8, Handle<Mesh>)>; 2],
    tyre_mat: Handle<StandardMaterial>,
    tyre_rt_mats: Vec<Handle<StandardMaterial>>,
    rims: [Lods; 2],
    /// Raytracing materials for the corgi's inks, per coat.
    corgi_rt_mats: [Vec<Handle<StandardMaterial>>; 3],
    goggles: Lods,
    goggles_rt: Vec<(u8, Handle<Mesh>)>,
    goggles_mat: Handle<StandardMaterial>,
    goggles_rt_mats: Vec<Handle<StandardMaterial>>,
    lens_mat: Handle<StandardMaterial>,
    bandana: Lods,
    bandana_tail: Handle<Mesh>,
    /// Soft cloth per kart colour.
    cloth_mats: Vec<Handle<StandardMaterial>>,
}

/// Kart colour lifted into neon.
fn neon(c: Color) -> LinearRgba {
    let c = c.to_linear();
    c * (4.0 / c.red.max(c.green).max(c.blue))
}

impl KartMeshes {
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, raytracing: bool) -> Self {
        let sculpt = |f: &dyn Fn(&mut Sculpt), cell: f32, ao: f32| {
            let mut s = Sculpt::default();
            f(&mut s);
            s.mesh(cell, ao)
        };
        let lacquer = |c: Color| StandardMaterial { base_color: c, perceptual_roughness: 0.3, clearcoat: 1.0, clearcoat_perceptual_roughness: 0.05, ..default() };
        // Every part is meshed at three levels of detail (see `lod`).
        let sculpt3 = |f: &dyn Fn(&mut Sculpt), cell: f32, ao: f32| LOD_CELL_SCALE.map(|k| sculpt(f, cell * k, ao));
        let lods = |m: &[Sculpted; 3], mesh: &mut dyn FnMut(&Sculpted) -> Handle<Mesh>| Lods::new([0, 1, 2].map(|i| mesh(&m[i])));
        let plain = |m: &Sculpted, meshes: &mut Assets<Mesh>| meshes.add(m.mesh(&[LinearRgba::WHITE], 0.0));
        let plain3 = |m: &[Sculpted; 3], meshes: &mut Assets<Mesh>| lods(m, &mut |s| plain(s, meshes));

        let body = sculpt3(&body, 0.019, 0.12);
        let carbon = CARBON_COLOR;
        let mut bodies = vec![];
        let mut body_rt_mats = vec![];
        for &paint in &KART_COLORS {
            let light = paint.luminance() > 0.6;
            let stripe = if light { Color::srgb(0.08, 0.1, 0.25) } else { Color::srgb(0.97, 0.95, 0.9) };
            let inks = [paint, stripe, carbon, Color::srgb(0.97, 0.95, 0.9)];
            let linear = inks.map(|c| c.to_linear());
            bodies.push(lods(&body, &mut |s| meshes.add(s.mesh(&linear, 0.0))));
            body_rt_mats.push(if raytracing { inks.map(|c| mats.add(lacquer(c))).to_vec() } else { vec![] });
        }
        let body_rt = if raytracing { body[0].rt_split(meshes) } else { vec![] };

        let wheel = |(r, w): (f32, f32)| (sculpt3(&|s| tyre(s, r, w), 0.0125, 0.05), sculpt3(&|s| rim(s, r, w), 0.011, 0.05));
        let (front, rear) = (wheel(FRONT_WHEEL), wheel(REAR_WHEEL));
        let tyre_inks = [Color::srgb(0.06, 0.06, 0.065), WALL_COLOR].map(|c| c.to_linear());
        let rt = |m: &Sculpted, meshes: &mut Assets<Mesh>| if raytracing { m.rt_split(meshes) } else { vec![] };

        let corgi_rt_mats = super::corgi::COATS.map(|pal| {
            if !raytracing {
                return vec![];
            }
            pal.inks().iter().map(|&c| mats.add(StandardMaterial { base_color: c.into(), perceptual_roughness: 0.75, reflectance: 0.3, ..default() })).collect()
        });

        let goggles = sculpt3(&goggles, 0.008, 0.02);
        let goggle_inks = [Color::srgb(0.3, 0.17, 0.08), Color::srgb(0.85, 0.62, 0.25)];
        let goggles_rt = if raytracing { goggles[0].rt_split(meshes) } else { vec![] };

        KartMeshes {
            goggles: lods(&goggles, &mut |s| meshes.add(s.mesh(&goggle_inks.map(|c| c.to_linear()), 0.0))),
            goggles_rt,
            goggles_mat: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.4, metallic: 0.0, ..default() }),
            goggles_rt_mats: if raytracing {
                vec![mats.add(matte(goggle_inks[0])), mats.add(StandardMaterial { base_color: goggle_inks[1], metallic: 1.0, perceptual_roughness: 0.3, ..default() })]
            } else {
                vec![]
            },
            // mirrored lenses
            lens_mat: mats.add(StandardMaterial { base_color: Color::srgb(0.35, 0.6, 0.9), metallic: 1.0, perceptual_roughness: 0.04, ..default() }),
            bandana: plain3(&sculpt3(&bandana, 0.012, 0.03), meshes),
            bandana_tail: plain(&sculpt(&bandana_tail, 0.008, 0.0), meshes),
            cloth_mats: KART_COLORS.iter().map(|&c| mats.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, reflectance: 0.2, ..default() })).collect(),
            bodies,
            body_rt,
            body_rt_mats,
            body_mat: mats.add(lacquer(Color::WHITE)),
            chrome: plain3(&sculpt3(&chrome, 0.014, 0.06), meshes),
            chrome_mat: mats.add(StandardMaterial { base_color: Color::srgb(0.92, 0.92, 0.95), metallic: 1.0, perceptual_roughness: 0.12, ..default() }),
            steering: plain3(&sculpt3(&steering, 0.01, 0.0), meshes),
            underglow: plain3(&sculpt3(&underglow, 0.012, 0.0), meshes),
            glow_mats: KART_COLORS.iter().map(|&c| mats.add(glow(neon(c)))).collect(),
            tyres: [lods(&front.0, &mut |s| meshes.add(s.mesh(&tyre_inks, 0.0))), lods(&rear.0, &mut |s| meshes.add(s.mesh(&tyre_inks, 0.0)))],
            tyre_rt: [rt(&front.0[0], meshes), rt(&rear.0[0], meshes)],
            tyre_mat: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.8, ..default() }),
            tyre_rt_mats: if raytracing { tyre_inks.map(|c| mats.add(matte(c.into()))).to_vec() } else { vec![] },
            rims: [plain3(&front.1, meshes), plain3(&rear.1, meshes)],
            corgi_rt_mats,
        }
    }
}

// ----- Kart component markers --------------------------------------------------------------

#[derive(Component)]
pub struct KartVis {
    pub index: usize,
}
#[derive(Component)]
pub struct KartBody {
    pub index: usize,
}
#[derive(Component)]
pub struct FrontPivot {
    pub index: usize,
}
#[derive(Component)]
pub struct Wheel {
    pub index: usize,
    pub radius: f32,
    pub angle: f32,
    /// Orientation at rest (the left wheels are turned round to face out).
    pub base: Quat,
}
#[derive(Component)]
pub struct CorgiHead {
    pub index: usize,
}
#[derive(Component)]
pub struct Flame {
    pub index: usize,
}

/// The corgi's throwing leg, pivoting at the shoulder.
#[derive(Component)]
pub struct CorgiArm {
    pub index: usize,
}

/// A tail of the bandana, fluttering in the wind; `side` is -1 or 1.
#[derive(Component)]
pub struct BandanaTail {
    pub index: usize,
    pub side: f32,
}

/// Brake light of kart `index`; its material swaps between `brake_dim` and `brake_on`.
#[derive(Component)]
pub struct BrakeLight {
    pub index: usize,
}

/// The standard renderer's stand-in for the underglow's light on the road.
#[derive(Component)]
pub struct UnderglowLight {
    pub index: usize,
}

/// `raytracing` makes the lights real light sources for the raytracer (and the corgi and
/// livery colours visible to it); otherwise the headlights and underglow get light entities.
pub fn spawn_kart(commands: &mut Commands, kit: &Kit, index: usize, coat: usize, raytracing: bool) -> Entity {
    let km = &kit.kart;
    let c = index % KART_COLORS.len();
    commands
        .spawn((Transform::default(), Visibility::default(), KartVis { index }, RaceEntity, DespawnOnExit(AppState::Race)))
        .with_children(|root| {
            root.spawn((Transform::default(), Visibility::default(), KartBody { index })).with_children(|b| {
                spawn_inked(b, &km.bodies[c], &km.body_mat, &km.body_rt, &km.body_rt_mats[c], Transform::default());
                part_lod(b, &km.chrome, &km.chrome_mat, Transform::default());
                part_lod(b, &km.underglow, &km.glow_mats[c], Transform::default());
                // steering wheel, angled up at the driver
                part_lod(b, &km.steering, &kit.dark, t(0.0, 1.05, -0.6).rx(1.2));
                // headlight lenses
                for x in [-HEADLIGHT.x, HEADLIGHT.x] {
                    part(b, &kit.sphere, &kit.headlight, t(x, HEADLIGHT.y, HEADLIGHT.z - 0.01).s(0.17, 0.15, 0.08));
                }
                // brake light bar across the tail
                part(b, &kit.cube, &kit.dark, t(0.0, 0.575, 1.505).s(0.86, 0.08, 0.03));
                b.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(kit.brake_dim.clone()), t(0.0, 0.575, 1.515).s(0.8, 0.05, 0.03), BrakeLight { index }));
                for x in [-0.3f32, 0.3] {
                    b.spawn((
                        Mesh3d(kit.cone.clone()),
                        MeshMaterial3d(kit.flame.clone()),
                        t(x, 0.44, 2.15).s(0.28, 0.7, 0.28).rx(FRAC_PI_2),
                        Visibility::Hidden,
                        Flame { index },
                    ));
                }
                if !raytracing {
                    b.spawn((
                        SpotLight {
                            color: Color::srgb(1.0, 0.95, 0.85),
                            intensity: 600_000.0,
                            range: 40.0,
                            radius: 0.1,
                            inner_angle: 0.22,
                            outer_angle: 0.6,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_xyz(0.0, 0.55, -1.95).looking_at(Vec3::new(0.0, -1.2, -16.0), Vec3::Y),
                    ));
                    // pointing down, so it can't light the driver through the floor
                    b.spawn((
                        SpotLight { color: KART_COLORS[c], intensity: 40_000.0, range: 5.0, radius: 0.6, inner_angle: 0.9, outer_angle: 1.4, shadow_maps_enabled: false, ..default() },
                        t(0.0, 0.6, -0.2).looking_at(v(0.0, -1.0, -0.2), Vec3::Z),
                        UnderglowLight { index },
                    ));
                }

                for (x, z, front) in [(-0.98, 0.95, false), (0.98, 0.95, false), (-0.9, -1.1, true), (0.9, -1.1, true)] {
                    let (r, _) = if front { FRONT_WHEEL } else { REAR_WHEEL };
                    let k = front as usize ^ 1;
                    let base = if x < 0.0 { Quat::from_rotation_y(PI) } else { Quat::IDENTITY };
                    let wheel = |p: &mut ChildSpawnerCommands, at: Vec3| {
                        let tyre = (Transform::from_translation(at).with_rotation(base), Wheel { index, radius: r, angle: 0.0, base });
                        spawn_inked(p, &km.tyres[1 - k], &km.tyre_mat, &km.tyre_rt[1 - k], &km.tyre_rt_mats, tyre).with_children(|w| {
                            part_lod(w, &km.rims[1 - k], &km.chrome_mat, Transform::default());
                            // a hub cap in the kart colour
                            let face = if front { FRONT_WHEEL.1 } else { REAR_WHEEL.1 } * 0.4 + 0.02;
                            part(w, &kit.sphere, &kit.kart_colors[c], t(face, 0.0, 0.0).s(0.05, r * 0.3, r * 0.3));
                        });
                    };
                    if front {
                        b.spawn((t(x, r, z), Visibility::default(), FrontPivot { index })).with_children(|p| wheel(p, Vec3::ZERO));
                    } else {
                        wheel(b, Vec3::new(x, r, z));
                    }
                }

                b.spawn((t(0.0, 0.45, 0.25).with_scale(Vec3::splat(1.15)), Visibility::default()))
                    .with_children(|p| spawn_corgi(p, kit, coat, index));
            });
        })
        .id()
}

fn spawn_corgi(c: &mut ChildSpawnerCommands, kit: &Kit, coat: usize, index: usize) {
    let km = &kit.kart;
    let set = &kit.corgi.coats[coat % 3];
    let rt_mats = &km.corgi_rt_mats[coat % 3];
    let cloth = &km.cloth_mats[index % KART_COLORS.len()];
    spawn_inked(c, &set.driver_lods, &kit.fur, &kit.corgi.driver_rt, rt_mats, Transform::default());
    spawn_inked(c, &set.arm_lods, &kit.fur, &kit.corgi.arm_rt, rt_mats, (Transform::from_translation(super::corgi::SHOULDER), CorgiArm { index }));
    // a bandana in the kart colour, its tails streaming out behind
    part_lod(c, &km.bandana, cloth, Transform::default());
    for side in [-1.0f32, 1.0] {
        c.spawn((Mesh3d(km.bandana_tail.clone()), MeshMaterial3d(cloth.clone()), t(side * 0.03, 0.86, 0.04), BandanaTail { index, side }));
    }
    // corgis have big heads
    c.spawn((t(0.0, 1.0, -0.3).with_scale(Vec3::splat(1.18)), Visibility::default(), CorgiHead { index })).with_children(|h| {
        spawn_inked(h, &set.head_lods, &kit.fur, &kit.corgi.head_rt, rt_mats, Transform::default());
        corgi_features(h, kit);
        spawn_inked(h, &km.goggles, &km.goggles_mat, &km.goggles_rt, &km.goggles_rt_mats, Transform::default());
        for side in [-1.0f32, 1.0] {
            let axis = goggle_axis() * Vec3::new(side, 1.0, 1.0);
            let at = GOGGLE * Vec3::new(side, 1.0, 1.0) + axis * 0.028;
            let rot = Quat::from_rotation_arc(Vec3::Z, axis);
            part(h, &kit.sphere, &km.lens_mat, Transform::from_translation(at).with_rotation(rot).with_scale(Vec3::new(0.13, 0.13, 0.03)));
        }
    });
}

/// Bandana tails flap in the wind, harder the faster the kart goes.
pub fn animate_bandanas(time: Res<Time>, session: Option<Res<SessionRes>>, mut tails: Query<(&BandanaTail, &mut Transform)>) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let t = time.elapsed_secs();
    for (b, mut tr) in &mut tails {
        let Some(k) = race.world.karts.get(b.index) else { continue };
        let wind = (k.speed.abs() / crate::sim::kart::MAX_SPEED).min(1.3);
        let phase = t * (6.0 + 14.0 * wind) + b.index as f32 * 1.7 + b.side;
        // hanging down at rest, streaming back at speed
        let lift = 1.2 - wind * 1.05 + phase.sin() * (0.08 + 0.2 * wind);
        let splay = b.side * (0.3 + 0.1 * (phase * 0.7).cos()) - k.steer * 0.3 * wind;
        tr.rotation = Quat::from_rotation_y(splay) * Quat::from_rotation_x(lift) * Quat::from_rotation_z((phase * 1.3).sin() * 0.3 * wind);
    }
}

/// Underglow: the kart's colour, taking on the drift spark colour as a drift charges, and
/// flaring while boosting.
pub fn animate_underglow(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    gfx: Res<super::gfx::Graphics>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut lights: Query<(&UnderglowLight, &mut SpotLight)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let t = time.elapsed_secs();
    // The tubes are thin, so for the raytracer they burn much brighter than the rest of the
    // neon to throw a real pool of light on the road.
    let scale = if gfx.raytracing { super::gfx::EMISSIVE_TO_NITS * 12.0 } else { 1.0 };
    let mut glow = vec![LinearRgba::BLACK; race.world.karts.len()];
    for (i, k) in race.world.karts.iter().enumerate() {
        let base = neon(KART_COLORS[i % KART_COLORS.len()]);
        let stage = if k.drift_dir != 0 { drift_stage(k.drift_charge) } else { 0 };
        let mut c = if stage > 0 {
            let s = SPARK_COLORS[stage];
            s * (5.0 / s.red.max(s.green).max(s.blue)) * (1.0 + 0.25 * (t * 30.0).sin())
        } else {
            base * (1.0 + 0.08 * (t * 3.0 + i as f32).sin())
        };
        if k.zoom > 0.0 {
            // The zoomies: racing through the rainbow.
            c = Color::hsl((t * 500.0 + i as f32 * 40.0) % 360.0, 1.0, 0.5).to_linear() * 9.0;
        }
        if k.is_boosting() {
            c = c * 1.8;
        }
        glow[i] = c;
        if let Some(mut m) = kit.kart.glow_mats.get(i).and_then(|h| mats.get_mut(h)) {
            m.emissive = c * scale;
        }
    }
    for (l, mut light) in &mut lights {
        if let Some(c) = glow.get(l.index) {
            let peak = c.red.max(c.green).max(c.blue).max(1e-3);
            light.color = Color::from(*c * (1.0 / peak));
            light.intensity = 60_000.0 * peak;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kart part meshes, and the body fits inside the wheels and under the driver.
    #[test]
    fn kart_meshes_build() {
        let start = std::time::Instant::now();
        let parts: [(&str, &dyn Fn(&mut Sculpt), f32); 6] = [
            ("body", &body, 0.019),
            ("chrome", &chrome, 0.014),
            ("goggles", &goggles, 0.008),
            ("bandana", &bandana, 0.012),
            ("tyre", &|s| tyre(s, REAR_WHEEL.0, REAR_WHEEL.1), 0.0125),
            ("rim", &|s| rim(s, REAR_WHEEL.0, REAR_WHEEL.1), 0.011),
        ];
        for (name, f, cell) in parts {
            let mut s = Sculpt::default();
            f(&mut s);
            let m = s.mesh(cell, 0.0).mesh(&[LinearRgba::WHITE; 4], 0.0);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let (lo, hi) = p.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &v| (lo.min(v.into()), hi.max(v.into())));
            println!("{name}: {} verts, {lo:.2} .. {hi:.2}", p.len());
            assert!(p.len() > 300, "{name} too sparse");
            if name == "body" {
                let inner = FRONT_WHEEL.1.min(REAR_WHEEL.1);
                assert!(hi.x < 0.98 - inner * 0.5 + 0.2, "body pokes through the wheels");
                assert!(lo.y > 0.05 && hi.y < 0.9, "body height {lo} .. {hi}");
            }
        }
        println!("built in {:?}", start.elapsed());
    }
}
