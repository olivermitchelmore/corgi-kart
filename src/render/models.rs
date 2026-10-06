//! Procedural models: the shared kit of meshes and materials, corgi faces, item boxes and
//! items. Everything faces -Z. The karts are in `kart.rs`.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;

use crate::game::{AppState, RaceEntity};
use crate::sim::Item;

pub const KART_COLORS: [Color; 8] = [
    Color::srgb(0.90, 0.16, 0.16),
    Color::srgb(0.16, 0.38, 0.95),
    Color::srgb(0.20, 0.75, 0.28),
    Color::srgb(0.97, 0.80, 0.10),
    Color::srgb(0.62, 0.28, 0.88),
    Color::srgb(0.98, 0.45, 0.72),
    Color::srgb(0.10, 0.80, 0.85),
    Color::srgb(0.95, 0.95, 0.95),
];

/// Spark colours for charge stages 0..=3 (white, blue, orange, purple), as HDR emissive.
pub const SPARK_COLORS: [LinearRgba; 4] = [
    LinearRgba::rgb(3.0, 3.0, 2.2),
    LinearRgba::rgb(0.6, 1.8, 9.0),
    LinearRgba::rgb(9.0, 3.0, 0.3),
    LinearRgba::rgb(6.0, 0.8, 9.0),
];

#[derive(Resource)]
pub struct Kit {
    pub cube: Handle<Mesh>,
    /// A thin square of confetti.
    pub flake: Handle<Mesh>,
    pub sphere: Handle<Mesh>,
    pub cylinder: Handle<Mesh>,
    pub cone: Handle<Mesh>,
    pub torus: Handle<Mesh>,
    pub capsule: Handle<Mesh>,
    pub white: Handle<StandardMaterial>,
    pub black: Handle<StandardMaterial>,
    pub pink: Handle<StandardMaterial>,
    pub tire: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub gold: Handle<StandardMaterial>,
    pub concrete: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
    pub cream: Handle<StandardMaterial>,
    pub ball: Handle<StandardMaterial>,
    pub toy: Handle<StandardMaterial>,
    pub yellow: Handle<StandardMaterial>,
    pub glass: Handle<StandardMaterial>,
    pub box_core: Handle<StandardMaterial>,
    pub flame: Handle<StandardMaterial>,
    pub biscuit: Handle<StandardMaterial>,
    pub headlight: Handle<StandardMaterial>,
    pub brake_dim: Handle<StandardMaterial>,
    pub brake_on: Handle<StandardMaterial>,
    pub lamp_red: Handle<StandardMaterial>,
    pub lamp_green: Handle<StandardMaterial>,
    pub smoke: Handle<StandardMaterial>,
    pub dust: Handle<StandardMaterial>,
    pub star: Handle<StandardMaterial>,
    /// The new items' materials.
    pub balloon: Handle<StandardMaterial>,
    pub frisbee: Handle<StandardMaterial>,
    pub mud: Handle<StandardMaterial>,
    pub cloud: Handle<StandardMaterial>,
    pub squirrel: Handle<StandardMaterial>,
    pub sausage: Handle<StandardMaterial>,
    /// A decoy box's core: a dull red where a real one cycles through the rainbow.
    pub decoy_core: Handle<StandardMaterial>,
    pub confetti: Vec<Handle<StandardMaterial>>,
    pub sparks: [Handle<StandardMaterial>; 4],
    /// Corgi coats: red, sable, tri-colour.
    pub coats: [Handle<StandardMaterial>; 3],
    pub kart_colors: Vec<Handle<StandardMaterial>>,
    /// Sculpted corgi meshes, coloured by their vertex colours with `fur`.
    pub corgi: super::corgi::CorgiMeshes,
    pub fur: Handle<StandardMaterial>,
    /// Glossy black for eyes and noses.
    pub eye: Handle<StandardMaterial>,
    pub kart: super::kart::KartMeshes,
}

pub fn matte(c: Color) -> StandardMaterial {
    StandardMaterial { base_color: c, perceptual_roughness: 0.85, ..default() }
}

pub fn glow(c: LinearRgba) -> StandardMaterial {
    StandardMaterial { base_color: Color::BLACK, emissive: c, ..default() }
}

impl Kit {
    /// `raytracing` also builds what the raytracer needs to see vertex-coloured models.
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, raytracing: bool) -> Self {
        let mut m = |c: Color| mats.add(matte(c));
        let white = m(Color::srgb(0.97, 0.95, 0.92));
        let black = m(Color::srgb(0.03, 0.03, 0.03));
        let pink = m(Color::srgb(0.95, 0.45, 0.55));
        let tire = m(Color::srgb(0.08, 0.08, 0.09));
        let dark = m(Color::srgb(0.18, 0.18, 0.2));
        let cream = m(Color::srgb(0.96, 0.9, 0.76));
        let ball = m(Color::srgb(0.78, 0.93, 0.15));
        let toy = m(Color::srgb(0.95, 0.3, 0.55));
        let yellow = m(Color::srgb(1.0, 0.85, 0.15));
        let concrete = m(Color::srgb(0.7, 0.7, 0.72));
        let biscuit = m(Color::srgb(0.78, 0.5, 0.25));
        let coats = [
            m(Color::srgb(0.86, 0.47, 0.17)),
            m(Color::srgb(0.58, 0.33, 0.14)),
            m(Color::srgb(0.1, 0.08, 0.08)),
        ];
        // Glossy lacquered paint that picks up reflections.
        let kart_colors = KART_COLORS
            .iter()
            .map(|&c| mats.add(StandardMaterial { base_color: c, perceptual_roughness: 0.35, clearcoat: 1.0, clearcoat_perceptual_roughness: 0.06, ..default() }))
            .collect();
        let metal = mats.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.76, 0.8),
            metallic: 0.8,
            perceptual_roughness: 0.3,
            ..default()
        });
        let gold = mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.78, 0.25),
            metallic: 1.0,
            perceptual_roughness: 0.25,
            ..default()
        });
        let smoke = mats.add(StandardMaterial {
            base_color: Color::srgba(0.92, 0.92, 0.95, 0.45),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        });
        let dust = mats.add(StandardMaterial {
            base_color: Color::srgba(0.55, 0.45, 0.3, 0.5),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 1.0,
            ..default()
        });
        let star = mats.add(glow(LinearRgba::rgb(9.0, 7.0, 1.0)));
        let mut glossy = |c: Color| mats.add(StandardMaterial { base_color: c, perceptual_roughness: 0.3, ..default() });
        let balloon = glossy(Color::srgb(0.15, 0.55, 0.98));
        let frisbee = glossy(Color::srgb(0.98, 0.35, 0.1));
        let mud = glossy(Color::srgb(0.38, 0.24, 0.12));
        let sausage = glossy(Color::srgb(0.72, 0.33, 0.24));
        let cloud = mats.add(matte(Color::srgb(0.42, 0.45, 0.52)));
        let squirrel = mats.add(matte(Color::srgb(0.62, 0.3, 0.12)));
        let decoy_core = mats.add(glow(LinearRgba::rgb(2.2, 0.25, 0.5)));
        let confetti = KART_COLORS.iter().map(|&c| mats.add(glow(c.to_linear() * 2.5))).collect();
        let glass = mats.add(StandardMaterial {
            base_color: Color::srgba(0.7, 0.9, 1.0, 0.35),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.05,
            emissive: LinearRgba::rgb(0.1, 0.15, 0.2),
            ..default()
        });
        let box_core = mats.add(glow(LinearRgba::rgb(3.0, 1.0, 1.0)));
        let flame = mats.add(glow(LinearRgba::rgb(8.0, 2.5, 0.4)));
        let sparks = SPARK_COLORS.map(|c| mats.add(glow(c)));
        let corgi = super::corgi::CorgiMeshes::new(meshes, raytracing);
        let kart = super::kart::KartMeshes::new(meshes, mats, raytracing);
        Kit {
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            flake: meshes.add(Cuboid::new(1.0, 0.08, 0.7)),
            sphere: meshes.add(Sphere::new(0.5).mesh().uv(24, 16)),
            cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
            cone: meshes.add(Cone { radius: 0.5, height: 1.0 }),
            torus: meshes.add(Torus::new(0.3, 0.5)),
            capsule: meshes.add(Capsule3d::new(0.5, 1.0)),
            white,
            black,
            pink,
            tire,
            metal,
            gold,
            concrete,
            dark,
            cream,
            ball,
            toy,
            yellow,
            glass,
            box_core,
            flame,
            biscuit,
            headlight: mats.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(9.0, 8.5, 7.0), ..default() }),
            brake_dim: mats.add(StandardMaterial { base_color: Color::srgb(0.5, 0.02, 0.02), emissive: LinearRgba::rgb(1.2, 0.05, 0.03), ..default() }),
            brake_on: mats.add(StandardMaterial { base_color: Color::srgb(0.9, 0.05, 0.05), emissive: LinearRgba::rgb(14.0, 0.5, 0.3), ..default() }),
            lamp_red: mats.add(glow(LinearRgba::rgb(14.0, 0.6, 0.3))),
            lamp_green: mats.add(glow(LinearRgba::rgb(0.5, 12.0, 1.0))),
            smoke,
            dust,
            star,
            balloon,
            frisbee,
            mud,
            cloud,
            squirrel,
            sausage,
            decoy_core,
            confetti,
            sparks,
            coats,
            kart_colors,
            corgi,
            fur: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.75, reflectance: 0.3, ..default() }),
            eye: mats.add(StandardMaterial {
                base_color: Color::srgb(0.02, 0.018, 0.018),
                perceptual_roughness: 0.08,
                clearcoat: 1.0,
                clearcoat_perceptual_roughness: 0.02,
                ..default()
            }),
            kart,
        }
    }
}

pub fn t(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

pub trait TransformExt {
    fn s(self, x: f32, y: f32, z: f32) -> Self;
    fn rx(self, a: f32) -> Self;
    fn rz(self, a: f32) -> Self;
}

impl TransformExt for Transform {
    fn s(self, x: f32, y: f32, z: f32) -> Self {
        self.with_scale(Vec3::new(x, y, z))
    }
    fn rx(mut self, a: f32) -> Self {
        self.rotation = Quat::from_rotation_x(a) * self.rotation;
        self
    }
    fn rz(mut self, a: f32) -> Self {
        self.rotation = Quat::from_rotation_z(a) * self.rotation;
        self
    }
}

/// A `part` that switches to coarser meshes with distance (a kart part, see `lod`).
pub fn part_lod(p: &mut ChildSpawnerCommands, mesh: &super::lod::Lods, mat: &Handle<StandardMaterial>, tr: Transform) {
    p.spawn((Mesh3d(mesh.full().clone()), MeshMaterial3d(mat.clone()), mesh.lod(super::lod::KART_LOD), tr));
}

pub fn part(p: &mut ChildSpawnerCommands, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tr: Transform) {
    p.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tr));
}

/// A sculpted head with glossy eyes (with catchlights), a shiny nose and a little tongue.
pub fn corgi_face(h: &mut ChildSpawnerCommands, kit: &Kit, head: &Handle<Mesh>) {
    h.spawn((Mesh3d(head.clone()), MeshMaterial3d(kit.fur.clone()), Transform::default()));
    corgi_features(h, kit);
}

/// Eyes, nose and tongue for a sculpted head.
pub fn corgi_features(h: &mut ChildSpawnerCommands, kit: &Kit) {
    for x in [-0.12f32, 0.12] {
        part(h, &kit.sphere, &kit.eye, t(x, 0.045, -0.235).s(0.1, 0.11, 0.08));
        part(h, &kit.sphere, &kit.white, t(x * 0.8, 0.075, -0.272).s(0.028, 0.028, 0.02));
    }
    part(h, &kit.sphere, &kit.eye, t(0.0, -0.035, -0.465).s(0.13, 0.095, 0.09));
    part(h, &kit.sphere, &kit.pink, t(0.025, -0.205, -0.33).s(0.08, 0.035, 0.12).rx(0.35));
}

// ----- Item boxes, projectiles, traps ------------------------------------------------------

#[derive(Component)]
pub struct BoxVis {
    pub index: usize,
}
#[derive(Component)]
pub struct BoxCore;

pub fn spawn_item_box(commands: &mut Commands, kit: &Kit, index: usize, pos: Vec3) {
    commands
        .spawn((Transform::from_translation(pos), Visibility::default(), BoxVis { index }, RaceEntity, DespawnOnExit(AppState::Race)))
        .with_children(|b| {
            part(b, &kit.cube, &kit.glass, Transform::from_scale(Vec3::splat(1.5)));
            b.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(kit.box_core.clone()), Transform::from_scale(Vec3::splat(0.6)), BoxCore));
        });
}

#[derive(Component)]
pub struct ProjVis {
    pub id: u16,
}
#[derive(Component)]
pub struct TrapVis {
    pub id: u16,
}

/// What marks a spawned item model: a thrown item, a trap, or something that goes round with a kart.
fn spawn_item<M: Component>(commands: &mut Commands, marker: M, build: impl FnOnce(&mut ChildSpawnerCommands)) -> Entity {
    commands.spawn((Transform::default(), Visibility::default(), marker, RaceEntity, DespawnOnExit(AppState::Race))).with_children(build).id()
}

fn bone_parts(b: &mut ChildSpawnerCommands, kit: &Kit) {
    part(b, &kit.capsule, &kit.cream, t(0.0, 0.0, 0.0).s(0.3, 0.8, 0.3).rz(FRAC_PI_2));
    for (x, z) in [(-0.7, -0.17), (-0.7, 0.17), (0.7, -0.17), (0.7, 0.17)] {
        part(b, &kit.sphere, &kit.cream, t(x, 0.0, z).s(0.42, 0.42, 0.42));
    }
}

fn ball_parts(b: &mut ChildSpawnerCommands, kit: &Kit) {
    part(b, &kit.sphere, &kit.ball, Transform::from_scale(Vec3::splat(1.0)));
    part(b, &kit.torus, &kit.white, t(0.0, 0.0, 0.0).s(0.83, 0.3, 0.83).rx(0.5));
}

fn toy_parts(b: &mut ChildSpawnerCommands, kit: &Kit) {
    part(b, &kit.sphere, &kit.toy, t(0.0, 0.4, 0.0).s(1.1, 0.8, 1.1));
    part(b, &kit.sphere, &kit.yellow, t(0.0, 0.85, 0.0).s(0.35, 0.3, 0.35));
    for x in [-0.2, 0.2] {
        part(b, &kit.sphere, &kit.black, t(x, 0.55, -0.48).s(0.12, 0.14, 0.08));
    }
    for a in 0..5 {
        let ang = a as f32 / 5.0 * PI * 2.0;
        part(b, &kit.cone, &kit.toy, t(ang.cos() * 0.5, 0.45, ang.sin() * 0.5).s(0.18, 0.3, 0.18).rz(-ang.cos() * 1.2).rx(ang.sin() * 1.2));
    }
}

/// The icon model of `item` (see `item_icon`), scaled and placed for the world.
fn icon_parts(b: &mut ChildSpawnerCommands, kit: &Kit, item: Item, tr: Transform) {
    b.spawn((tr, Visibility::default())).with_children(|b| item_icon(b, kit, item));
}

/// A thrown bone, tennis ball or frisbee.
pub fn spawn_projectile(commands: &mut Commands, kit: &Kit, id: u16, kind: Item) -> Entity {
    spawn_item(commands, ProjVis { id }, |b| match kind {
        Item::Bone => bone_parts(b, kit),
        Item::Frisbee => icon_parts(b, kit, kind, Transform::from_scale(Vec3::splat(0.8))),
        _ => ball_parts(b, kit),
    })
}

/// A squeaky toy, decoy box or water balloon on the road.
pub fn spawn_trap(commands: &mut Commands, kit: &Kit, id: u16, kind: Item) -> Entity {
    spawn_item(commands, TrapVis { id }, |b| match kind {
        // The same size as a real box.
        Item::DecoyBox => icon_parts(b, kit, kind, Transform::from_scale(Vec3::splat(1.5 / 1.3))),
        Item::WaterBalloon => icon_parts(b, kit, kind, t(0.0, 0.5, 0.0).with_scale(Vec3::splat(0.65))),
        _ => toy_parts(b, kit),
    })
}

/// Something that goes round with kart `kart`: a bone or ball circling it (`slot` 0..3), the
/// rain cloud over it, or the squirrel it's chasing.
#[derive(Component)]
pub struct ExtraVis {
    pub kart: usize,
    pub slot: u8,
}
pub const CLOUD_SLOT: u8 = 3;
pub const SQUIRREL_SLOT: u8 = 4;

pub fn spawn_extra(commands: &mut Commands, kit: &Kit, kart: usize, slot: u8, kind: Item) -> Entity {
    spawn_item(commands, ExtraVis { kart, slot }, |b| match kind {
        // A little smaller than a thrown one, so three of them don't hide the kart.
        Item::Bone => {
            b.spawn((Transform::from_scale(Vec3::splat(0.7)), Visibility::default())).with_children(|b| bone_parts(b, kit));
        }
        Item::TennisBall => ball_parts(b, kit),
        // It runs ahead, so it's seen from behind.
        Item::Squirrel => icon_parts(b, kit, kind, t(0.0, 1.0, 0.0).with_rotation(Quat::from_rotation_y(PI))),
        _ => icon_parts(b, kit, kind, Transform::from_scale(Vec3::splat(1.3))),
    })
}

/// Each item as a model about two units across, centred on the origin and facing +Z: what the
/// HUD's item slot shows, and (scaled) what the newer items look like out on the track.
pub fn item_icon(b: &mut ChildSpawnerCommands, kit: &Kit, item: Item) {
    let biscuit = |b: &mut ChildSpawnerCommands, mat: &Handle<StandardMaterial>| {
        part(b, &kit.capsule, mat, t(0.0, 0.0, 0.0).s(0.45, 0.9, 0.35).rz(FRAC_PI_2));
        for (x, y) in [(-0.75, -0.22), (-0.75, 0.22), (0.75, -0.22), (0.75, 0.22)] {
            part(b, &kit.sphere, mat, t(x, y, 0.0).s(0.55, 0.55, 0.4));
        }
    };
    match item {
        Item::None => {}
        Item::Treat => biscuit(b, &kit.biscuit),
        Item::GoldenTreat => biscuit(b, &kit.gold),
        Item::Bone => {
            part(b, &kit.capsule, &kit.cream, t(0.0, 0.0, 0.0).s(0.35, 1.0, 0.35).rz(FRAC_PI_2));
            for (x, y) in [(-0.85, -0.2), (-0.85, 0.2), (0.85, -0.2), (0.85, 0.2)] {
                part(b, &kit.sphere, &kit.cream, t(x, y, 0.0).s(0.5, 0.5, 0.5));
            }
        }
        Item::TennisBall => {
            part(b, &kit.sphere, &kit.ball, t(0.0, 0.0, 0.0).s(1.5, 1.5, 1.5));
            part(b, &kit.torus, &kit.white, t(0.0, 0.0, 0.0).s(1.25, 0.45, 1.25).rx(0.5));
        }
        Item::SqueakyToy => {
            part(b, &kit.sphere, &kit.toy, t(0.0, -0.1, 0.0).s(1.3, 1.0, 1.3));
            part(b, &kit.sphere, &kit.yellow, t(0.0, 0.5, 0.0).s(0.45, 0.4, 0.45));
            for x in [-0.25, 0.25] {
                part(b, &kit.sphere, &kit.black, t(x, 0.1, 0.58).s(0.15, 0.18, 0.1));
            }
        }
        Item::DecoyBox => {
            part(b, &kit.cube, &kit.glass, Transform::from_scale(Vec3::splat(1.3)));
            part(b, &kit.cube, &kit.decoy_core, Transform::from_scale(Vec3::splat(0.52)).rx(0.6).rz(0.6));
        }
        Item::WaterBalloon => {
            part(b, &kit.sphere, &kit.balloon, t(0.0, -0.1, 0.0).s(1.35, 1.5, 1.35));
            part(b, &kit.cone, &kit.balloon, t(0.0, 0.68, 0.0).s(0.3, 0.3, 0.3));
            part(b, &kit.torus, &kit.balloon, t(0.0, 0.84, 0.0).s(0.3, 0.3, 0.3));
            part(b, &kit.sphere, &kit.white, t(-0.3, 0.25, 0.52).s(0.22, 0.32, 0.1));
        }
        Item::Frisbee => {
            part(b, &kit.cylinder, &kit.frisbee, t(0.0, 0.0, 0.0).s(1.8, 0.1, 1.8));
            part(b, &kit.torus, &kit.frisbee, t(0.0, -0.03, 0.0).s(2.1, 0.8, 2.1));
            part(b, &kit.cylinder, &kit.white, t(0.0, 0.015, 0.0).s(1.15, 0.1, 1.15));
            part(b, &kit.cylinder, &kit.frisbee, t(0.0, 0.03, 0.0).s(0.8, 0.1, 0.8));
        }
        Item::DogWhistle => {
            part(b, &kit.capsule, &kit.metal, t(-0.1, 0.0, 0.0).s(0.55, 0.75, 0.55).rz(FRAC_PI_2));
            part(b, &kit.cube, &kit.metal, t(0.85, 0.08, 0.0).s(0.6, 0.2, 0.36));
            part(b, &kit.cube, &kit.black, t(0.2, 0.26, 0.0).s(0.3, 0.08, 0.3));
            part(b, &kit.torus, &kit.gold, t(-1.0, 0.0, 0.0).s(0.55, 0.55, 0.55).rx(FRAC_PI_2));
        }
        // A paw print, glowing through the rainbow like the cores of the item boxes.
        Item::Zoomies => {
            part(b, &kit.sphere, &kit.box_core, t(0.0, -0.3, 0.0).s(1.1, 0.9, 0.35));
            for (x, y) in [(-0.62, 0.2), (-0.22, 0.56), (0.22, 0.56), (0.62, 0.2)] {
                part(b, &kit.sphere, &kit.box_core, t(x, y, 0.0).s(0.36, 0.46, 0.3));
            }
        }
        Item::Squirrel => {
            part(b, &kit.sphere, &kit.squirrel, t(0.0, -0.35, 0.0).s(0.8, 1.0, 0.75));
            part(b, &kit.sphere, &kit.cream, t(0.0, -0.35, 0.2).s(0.5, 0.75, 0.45));
            part(b, &kit.sphere, &kit.squirrel, t(0.0, 0.4, 0.15).s(0.62, 0.56, 0.6));
            for x in [-0.2f32, 0.2] {
                part(b, &kit.cone, &kit.squirrel, t(x, 0.76, 0.1).s(0.16, 0.3, 0.12));
                part(b, &kit.sphere, &kit.eye, t(x * 0.8, 0.46, 0.4).s(0.12, 0.14, 0.1));
                part(b, &kit.sphere, &kit.squirrel, t(x * 1.4, -0.2, 0.32).s(0.16, 0.34, 0.16));
            }
            part(b, &kit.sphere, &kit.eye, t(0.0, 0.34, 0.46).s(0.1, 0.08, 0.08));
            // a big bushy tail curling up behind
            part(b, &kit.capsule, &kit.squirrel, t(0.0, -0.1, -0.6).s(0.5, 0.55, 0.5).rx(-0.5));
            part(b, &kit.sphere, &kit.squirrel, t(0.0, 0.6, -0.72).s(0.62, 0.7, 0.6));
        }
        Item::Sausage => {
            for side in [-1.0f32, 1.0] {
                part(b, &kit.capsule, &kit.sausage, t(side * 0.55, 0.0, 0.0).s(0.6, 0.7, 0.6).rz(FRAC_PI_2 + side * 0.25));
                part(b, &kit.sphere, &kit.biscuit, t(side * 1.22, -0.3, 0.0).s(0.2, 0.2, 0.2));
            }
        }
        // A megaphone.
        Item::BigWoof => {
            part(b, &kit.cone, &kit.yellow, t(0.1, 0.0, 0.0).s(1.5, 1.7, 1.5).rz(FRAC_PI_2));
            part(b, &kit.torus, &kit.white, t(0.95, 0.0, 0.0).s(1.55, 0.6, 1.55).rz(FRAC_PI_2));
            part(b, &kit.cylinder, &kit.dark, t(-0.85, 0.0, 0.0).s(0.4, 0.3, 0.4).rz(FRAC_PI_2));
            part(b, &kit.cube, &kit.dark, t(-0.35, -0.55, 0.0).s(0.2, 0.6, 0.2));
        }
        Item::MudPie => {
            part(b, &kit.sphere, &kit.mud, t(0.0, -0.3, 0.0).s(1.9, 0.6, 1.9));
            part(b, &kit.sphere, &kit.mud, t(0.1, -0.05, 0.0).s(1.3, 0.6, 1.3));
            part(b, &kit.sphere, &kit.mud, t(-0.1, 0.2, 0.05).s(0.7, 0.45, 0.7));
            for (x, z, r) in [(0.95, 0.3, 0.3), (-0.9, -0.4, 0.34), (-0.5, 0.85, 0.26), (0.4, -0.9, 0.28)] {
                part(b, &kit.sphere, &kit.mud, t(x, -0.38, z).s(r * 1.6, r, r * 1.6));
            }
        }
        Item::RainCloud => {
            for (x, y, z, r) in [(-0.6, 0.2, 0.0, 0.8), (0.0, 0.42, 0.1, 1.0), (0.62, 0.22, 0.0, 0.85), (-0.25, 0.1, 0.4, 0.7), (0.3, 0.05, -0.35, 0.75)] {
                part(b, &kit.sphere, &kit.cloud, t(x, y, z).s(r, r * 0.8, r));
            }
            for (x, y) in [(-0.55, -0.55), (0.0, -0.75), (0.5, -0.5)] {
                part(b, &kit.sphere, &kit.balloon, t(x, y, 0.1).s(0.13, 0.3, 0.13).rz(0.2));
            }
        }
    }
}
