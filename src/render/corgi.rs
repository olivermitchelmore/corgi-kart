//! Sculpted corgi meshes. Each corgi is a signed distance field built from smoothly blended
//! ellipsoids and tapered capsules, meshed with surface nets, so heads, bodies and legs flow
//! into each other like a real (plush) dog. Coat markings are painted into vertex colours by
//! "paint" volumes, so a single mesh carries the whole coat.

use bevy::prelude::*;

use super::lod::{Lods, LOD_CELL_SCALE};
use super::sculpt::{ell, ell_rot, v, Sculpted, Shape, Sculpt};

/// Coat colours: main fur, secondary (tan points / lighter underside), white, pink, dark.
#[derive(Clone, Copy)]
pub struct Palette {
    pub fur: Color,
    pub tan: Color,
    pub white: Color,
    pub pink: Color,
    pub dark: Color,
}

pub const COATS: [Palette; 3] = [
    // red and white
    Palette {
        fur: Color::srgb(0.87, 0.47, 0.17),
        tan: Color::srgb(0.93, 0.62, 0.3),
        white: Color::srgb(0.98, 0.95, 0.9),
        pink: Color::srgb(0.95, 0.55, 0.6),
        dark: Color::srgb(0.06, 0.05, 0.05),
    },
    // sable
    Palette {
        fur: Color::srgb(0.5, 0.28, 0.12),
        tan: Color::srgb(0.78, 0.5, 0.24),
        white: Color::srgb(0.97, 0.94, 0.88),
        pink: Color::srgb(0.9, 0.5, 0.55),
        dark: Color::srgb(0.06, 0.05, 0.05),
    },
    // tri-colour
    Palette {
        fur: Color::srgb(0.1, 0.085, 0.085),
        tan: Color::srgb(0.8, 0.48, 0.2),
        white: Color::srgb(0.97, 0.95, 0.92),
        pink: Color::srgb(0.9, 0.5, 0.55),
        dark: Color::srgb(0.03, 0.03, 0.03),
    },
];

/// A topiary corgi, clipped from a hedge.
pub const TOPIARY: Palette = Palette {
    fur: Color::srgb(0.22, 0.5, 0.18),
    tan: Color::srgb(0.3, 0.58, 0.22),
    white: Color::srgb(0.36, 0.64, 0.26),
    pink: Color::srgb(0.3, 0.55, 0.2),
    dark: Color::srgb(0.12, 0.3, 0.1),
};

#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Fur,
    Tan,
    White,
    Pink,
    Dark,
}

impl From<Ink> for u8 {
    fn from(i: Ink) -> u8 {
        i as u8
    }
}

impl Palette {
    /// Linear colours indexed by `Ink`.
    pub fn inks(&self) -> [LinearRgba; 5] {
        [self.fur, self.tan, self.white, self.pink, self.dark].map(|c| c.to_linear())
    }
}

/// The head, centred on the skull, facing -Z. About 0.7 wide including cheeks.
fn head(s: &mut Sculpt, with_face: bool) {
    s.add(ell(v(0.0, 0.02, 0.02), v(0.3, 0.27, 0.29)), Ink::Fur, 0.0);
    // fluffy cheeks, the bridge of the nose and the snout
    s.add(ell(v(0.15, -0.08, -0.08), v(0.17, 0.15, 0.16)), Ink::Fur, 0.08).mirror(1);
    s.add(ell(v(0.0, -0.02, -0.2), v(0.12, 0.12, 0.16)), Ink::Fur, 0.08);
    s.add(ell(v(0.0, -0.08, -0.33), v(0.12, 0.1, 0.14)), Ink::White, 0.06);
    s.add(ell(v(0.0, -0.17, -0.24), v(0.11, 0.06, 0.12)), Ink::White, 0.05);
    // big upright ears, slightly flared, with pink insides
    let tilt = Quat::from_rotation_z(-0.42) * Quat::from_rotation_x(-0.08);
    s.add(ell_rot(v(0.19, 0.3, 0.03), v(0.12, 0.2, 0.045), tilt), Ink::Fur, 0.05).mirror(1);
    s.add(ell_rot(v(0.25, 0.44, 0.04), v(0.075, 0.13, 0.035), tilt), Ink::Fur, 0.04).mirror(1);
    s.paint(ell_rot(v(0.215, 0.35, -0.02), v(0.075, 0.17, 0.03), tilt), Ink::Pink).mirror(1);
    // markings: white blaze up the forehead, white muzzle and chin, tan eyebrows for tri-colours
    s.paint(ell(v(0.0, 0.1, -0.24), v(0.05, 0.2, 0.1)), Ink::White);
    s.paint(ell(v(0.0, -0.12, -0.3), v(0.2, 0.12, 0.2)), Ink::White);
    s.paint(ell(v(0.1, 0.13, -0.23), v(0.04, 0.03, 0.05)), Ink::Tan).mirror(1);
    s.paint(ell(v(0.2, -0.06, -0.14), v(0.09, 0.07, 0.09)), Ink::Tan).mirror(1);
    if with_face {
        // baked eyes and nose, for distant crowd corgis
        s.paint(ell(v(0.12, 0.04, -0.25), v(0.045, 0.05, 0.05)), Ink::Dark).mirror(1);
        s.paint(ell(v(0.0, -0.04, -0.47), v(0.06, 0.045, 0.05)), Ink::Dark);
    }
}

/// Where the driver's throwing leg joins its body.
pub const SHOULDER: Vec3 = Vec3::new(0.2, 0.58, -0.28);

/// A corgi sitting in a kart seat, a paw reaching forward to the wheel. Same frame as the old
/// primitive model: seat at y ~ 0.1, head attaches at (0, 0.98, -0.3). The other front leg is
/// `driver_arm`, which attaches at `SHOULDER`.
fn driver_body(s: &mut Sculpt) {
    s.add(ell(v(0.0, 0.38, 0.08), v(0.38, 0.36, 0.46)), Ink::Fur, 0.0);
    s.add(ell(v(0.0, 0.58, -0.16), v(0.3, 0.3, 0.26)), Ink::Fur, 0.12);
    s.add(ell(v(0.0, 0.78, -0.24), v(0.2, 0.18, 0.18)), Ink::Fur, 0.1);
    // the famous fluffy corgi bottom: two round cheeks
    s.add(ell(v(0.13, 0.36, 0.42), v(0.2, 0.26, 0.2)), Ink::White, 0.08).mirror(1);
    s.add(ell(v(0.0, 0.55, 0.62), v(0.09, 0.08, 0.09)), Ink::Fur, 0.05);
    // a front leg out to the steering wheel, with a white paw
    s.add(Shape::Capsule(v(-0.2, 0.58, -0.28), v(-0.22, 0.62, -0.68), 0.1, 0.08), Ink::Fur, 0.06);
    s.add(ell(v(-0.22, 0.62, -0.74), v(0.1, 0.08, 0.11)), Ink::White, 0.04);
    // stubby hind legs tucked in the seat
    s.add(ell(v(0.27, 0.26, 0.1), v(0.16, 0.18, 0.24)), Ink::Fur, 0.08).mirror(1);
    s.add(ell(v(0.28, 0.12, -0.14), v(0.1, 0.07, 0.14)), Ink::White, 0.05).mirror(1);
    // white chest bib and belly, tan legs on tri-colours
    s.paint(ell(v(0.0, 0.55, -0.34), v(0.24, 0.34, 0.2)), Ink::White);
    s.paint(ell(v(0.0, 0.3, -0.2), v(0.25, 0.2, 0.3)), Ink::White);
    s.paint(Shape::Capsule(v(-0.22, 0.62, -0.5), v(-0.22, 0.62, -0.72), 0.1, 0.09), Ink::Tan);
}

/// The driver's other front leg, on its own so it can throw things and punch the air. Its
/// shoulder is at the origin, and at rest it reaches forward to the wheel like the first.
fn driver_arm(s: &mut Sculpt) {
    s.add(Shape::Capsule(v(0.0, 0.0, 0.0), v(0.02, 0.04, -0.4), 0.1, 0.08), Ink::Fur, 0.0);
    s.add(ell(v(0.02, 0.04, -0.46), v(0.1, 0.08, 0.11)), Ink::White, 0.04);
    s.paint(Shape::Capsule(v(0.02, 0.04, -0.22), v(0.02, 0.04, -0.44), 0.1, 0.09), Ink::Tan);
}

/// A corgi sitting on the ground, front legs straight. Ground at y = 0; the head (not
/// included) attaches at `SIT_HEAD`, scaled by `SIT_HEAD_SCALE`.
pub const SIT_HEAD: Vec3 = Vec3::new(0.0, 1.9, -0.3);
pub const SIT_HEAD_SCALE: f32 = 1.55;

fn sitting_body(s: &mut Sculpt) {
    s.add(ell(v(0.0, 0.55, 0.28), v(0.62, 0.52, 0.66)), Ink::Fur, 0.0);
    s.add(ell(v(0.0, 1.1, 0.0), v(0.5, 0.62, 0.48)), Ink::Fur, 0.25);
    s.add(ell(v(0.0, 1.55, -0.18), v(0.32, 0.32, 0.3)), Ink::Fur, 0.2);
    s.add(ell(v(0.0, 1.1, -0.3), v(0.4, 0.5, 0.32)), Ink::White, 0.15);
    s.add(Shape::Capsule(v(0.24, 0.9, -0.3), v(0.24, 0.12, -0.5), 0.15, 0.13), Ink::Fur, 0.1).mirror(1);
    s.add(ell(v(0.24, 0.09, -0.62), v(0.16, 0.1, 0.2)), Ink::White, 0.06).mirror(1);
    s.add(ell(v(0.5, 0.3, 0.2), v(0.26, 0.3, 0.42)), Ink::Fur, 0.14).mirror(1);
    s.add(ell(v(0.55, 0.08, -0.12), v(0.16, 0.1, 0.24)), Ink::White, 0.06).mirror(1);
    s.paint(ell(v(0.0, 1.15, -0.42), v(0.36, 0.55, 0.25)), Ink::White);
    s.paint(Shape::Capsule(v(0.24, 0.6, -0.4), v(0.24, 0.12, -0.52), 0.16, 0.15), Ink::Tan).mirror(1);
    s.paint(ell(v(0.0, 0.5, 0.85), v(0.45, 0.4, 0.2)), Ink::White);
}

fn sitting_tail(s: &mut Sculpt) {
    s.add(ell(v(0.0, 0.0, 0.1), v(0.2, 0.18, 0.22)), Ink::Fur, 0.0);
    s.paint(ell(v(0.0, -0.08, 0.2), v(0.2, 0.12, 0.2)), Ink::White);
}

/// A crowd corgi: head and shoulders in one mesh, face baked in.
fn fan(s: &mut Sculpt) {
    s.add(ell(v(0.0, 0.42, 0.0), v(0.34, 0.44, 0.3)), Ink::Fur, 0.0);
    s.add(ell(v(0.0, 0.46, -0.16), v(0.24, 0.3, 0.18)), Ink::White, 0.1);
    s.add(Shape::Capsule(v(0.2, 0.62, -0.12), v(0.3, 0.95, -0.1), 0.07, 0.06), Ink::Fur, 0.05).mirror(1);
    s.add(ell(v(0.31, 0.99, -0.1), v(0.07, 0.07, 0.07)), Ink::White, 0.03).mirror(1);
    let from = s.len();
    head(s, true);
    s.transform_since(from, Quat::IDENTITY, 1.0, v(0.0, 0.98, -0.04));
}

pub struct CorgiSet {
    pub head: Handle<Mesh>,
    pub sitting: Handle<Mesh>,
    pub tail: Handle<Mesh>,
    /// The kart driver, its throwing leg, its head and the crowd corgi at every level of detail.
    pub driver_lods: Lods,
    pub arm_lods: Lods,
    pub head_lods: Lods,
    pub fan_lods: Lods,
}

pub struct CorgiMeshes {
    /// Per coat: red, sable, tri-colour.
    pub coats: [CorgiSet; 3],
    pub topiary: CorgiSet,
    /// The kart driver's body, throwing leg and head split by ink, for the raytracer (empty
    /// otherwise).
    pub driver_rt: Vec<(u8, Handle<Mesh>)>,
    pub arm_rt: Vec<(u8, Handle<Mesh>)>,
    pub head_rt: Vec<(u8, Handle<Mesh>)>,
}

/// Mesh every corgi shape at `detail` (a multiplier on cell size), once per palette.
fn build_sets<const N: usize>(meshes: &mut Assets<Mesh>, pals: [&Palette; N], detail: f32, rt: Option<&mut [Vec<(u8, Handle<Mesh>)>; 3]>) -> [CorgiSet; N] {
    let make = |f: &dyn Fn(&mut Sculpt), cell: f32, ao: f32| {
        let mut s = Sculpt::default();
        f(&mut s);
        s.mesh(cell * detail, ao)
    };
    let make_lods = |f: &dyn Fn(&mut Sculpt), cell: f32, ao: f32| LOD_CELL_SCALE.map(|k| make(f, cell * k, ao));
    let driver = make_lods(&driver_body, 0.022, 0.1);
    let arm = make_lods(&driver_arm, 0.022, 0.1);
    let head = make_lods(&|s| head(s, false), 0.016, 0.06);
    let sitting = make(&sitting_body, 0.035, 0.15);
    let tail = make(&sitting_tail, 0.03, 0.05);
    let fan = make_lods(&fan, 0.045, 0.08);
    if let Some(rt) = rt {
        *rt = [driver[0].rt_split(meshes), arm[0].rt_split(meshes), head[0].rt_split(meshes)];
    }
    pals.map(|pal| {
        let inks = pal.inks();
        let mut add = |m: &Sculpted| meshes.add(m.mesh(&inks, 0.06));
        let mut add_lods = |m: &[Sculpted; 3]| Lods::new([0, 1, 2].map(|i| add(&m[i])));
        let (driver_lods, arm_lods, head_lods, fan_lods) = (add_lods(&driver), add_lods(&arm), add_lods(&head), add_lods(&fan));
        CorgiSet {
            head: head_lods.full().clone(),
            sitting: add(&sitting),
            tail: add(&tail),
            driver_lods,
            arm_lods,
            head_lods,
            fan_lods,
        }
    })
}

impl CorgiMeshes {
    pub fn new(meshes: &mut Assets<Mesh>, raytracing: bool) -> Self {
        let [topiary] = build_sets(meshes, [&TOPIARY], 1.6, None);
        let mut rt = [vec![], vec![], vec![]];
        let coats = build_sets(meshes, [&COATS[0], &COATS[1], &COATS[2]], 1.0, raytracing.then_some(&mut rt));
        let [driver_rt, arm_rt, head_rt] = rt;
        CorgiMeshes { coats, topiary, driver_rt, arm_rt, head_rt }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sculpted meshes are closed, reasonably dense, and sized as expected.
    #[test]
    fn corgi_meshes_build() {
        for (name, f, cell) in [("driver", driver_body as fn(&mut Sculpt), 0.022), ("sitting", sitting_body, 0.035), ("fan", fan, 0.045)] {
            let mut s = Sculpt::default();
            f(&mut s);
            let m = s.mesh(cell, 0.0).mesh(&COATS[0].inks(), 0.0);
            let verts = m.count_vertices();
            let tris = m.indices().unwrap().len() / 3;
            println!("{name}: {verts} verts, {tris} tris");
            assert!(verts > 500 && tris > verts, "{name} too sparse");
        }
        let mut s = Sculpt::default();
        head(&mut s, false);
        let m = s.mesh(0.016, 0.0).mesh(&COATS[0].inks(), 0.0);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let top = p.iter().map(|v| v[1]).fold(f32::MIN, f32::max);
        println!("head: {} verts, ear tips at y={top:.2}", p.len());
        assert!(top > 0.45, "ears should stick up");
    }
}
