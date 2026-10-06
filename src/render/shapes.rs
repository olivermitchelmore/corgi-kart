//! Shapes the tracks' scenery is built from: strips and faces that follow the road, solids of
//! revolution, boxes and roofs, palms and little bulbs. Each adds to a `MeshBuilder` (or returns
//! a `Mesh`), so a whole street or forest ends up as a handful of meshes.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::mesh::{v3, Lcg, MeshBuilder};
use super::trackmesh::{at3, surface_normal};
use crate::sim::track::Track;

/// Quad on the road surface between samples `i` and `i + 1`, from `l0` to `l1` across. Normals
/// are per sample so the mirror-polished road reflects smoothly over crests, without facets.
pub(super) fn strip(b: &mut MeshBuilder, track: &Track, i: usize, l0: f32, l1: f32, dy: f32, c: Color) {
    strip_tiled(b, track, i, l0, l1, dy, c, 4.0);
}

/// `strip` with its texture repeating every `tile` metres.
#[allow(clippy::too_many_arguments)]
pub(super) fn strip_tiled(b: &mut MeshBuilder, track: &Track, i: usize, l0: f32, l1: f32, dy: f32, c: Color, tile: f32) {
    let v = [at3(track, i, l0, dy), at3(track, i, l1, dy), at3(track, i + 1, l1, dy), at3(track, i + 1, l0, dy)];
    let (n0, n1) = (surface_normal(track, i), surface_normal(track, i + 1));
    b.quad_smooth(v, [n0, n0, n1, n1], c, v.map(|p| Vec2::new(p.x, p.z) / tile));
}

/// Vertical face along the track at `lat`, from `y0` to `y1` above the road, facing `face`
/// (+1 = towards the right of the track, -1 = left).
#[allow(clippy::too_many_arguments)]
pub(super) fn side_face(b: &mut MeshBuilder, track: &Track, i: usize, lat: f32, y0: f32, y1: f32, face: f32, c: Color) {
    let n = track.samples.len();
    let q = [at3(track, i, lat, y0), at3(track, i + 1, lat, y0), at3(track, i + 1, lat, y1), at3(track, i, lat, y1)];
    let (s0, s1) = (track.samples[i % n].s, track.samples[i % n].s + track.spacing);
    let uv = [Vec2::new(s0, y0), Vec2::new(s1, y0), Vec2::new(s1, y1), Vec2::new(s0, y1)].map(|u| u / 3.0);
    b.quad_facing(q, v3(track.samples[i % n].dir.perp() * face, 0.0), c, uv);
}

/// Smooth upper hemisphere (plus a slight bulb: `stretch` > 1 makes it taller), base at y = 0.
pub(super) fn hemisphere(r: f32, stretch: f32) -> Mesh {
    let mut b = MeshBuilder::default();
    let (rings, segs) = (14, 40);
    let point = |i: usize, j: usize| {
        let a = i as f32 / rings as f32 * FRAC_PI_2;
        let t = j as f32 / segs as f32 * TAU;
        let n = Vec3::new(t.cos() * a.cos(), a.sin(), t.sin() * a.cos());
        (Vec3::new(n.x * r, n.y * r * stretch, n.z * r), Vec3::new(n.x, n.y / stretch, n.z).normalize())
    };
    for i in 0..rings {
        for j in 0..segs {
            let c = [point(i, j + 1), point(i, j), point(i + 1, j), point(i + 1, j + 1)];
            b.quad_smooth(c.map(|p| p.0), c.map(|p| p.1), Color::WHITE, [Vec2::ZERO; 4]);
        }
    }
    b.build()
}

/// Like `MeshBuilder::tinted`, for a mesh that's already built (the water needs tangents).
pub(super) fn tinted_mesh(mut mesh: Mesh, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, template: &Handle<StandardMaterial>) -> (Mesh3d, MeshMaterial3d<StandardMaterial>) {
    use bevy::mesh::VertexAttributeValues;
    let mut avg = [0.0f32; 3];
    if let Some(VertexAttributeValues::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        let n = cols.len().max(1) as f32;
        for c in cols.iter() {
            for k in 0..3 {
                avg[k] += c[k] / n;
            }
        }
        let avg = avg.map(|a| a.max(1e-3));
        for c in cols.iter_mut() {
            for k in 0..3 {
                c[k] /= avg[k];
            }
        }
    }
    let mut m = mats.get(template).cloned().unwrap_or_default();
    m.base_color = Color::linear_rgb(avg[0], avg[1], avg[2]);
    (Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.add(m)))
}

/// A solid of revolution through `(radius, height)` points, smooth-shaded.
pub(super) fn lathe(profile: &[(f32, f32)], segs: usize) -> Mesh {
    let mut b = MeshBuilder::default();
    for w in profile.windows(2) {
        let ((r0, y0), (r1, y1)) = (w[0], w[1]);
        // outward normal of the slanted side
        let slope = Vec2::new(y1 - y0, r0 - r1).normalize();
        for j in 0..segs {
            let (a0, a1) = (j as f32 / segs as f32 * TAU, (j + 1) as f32 / segs as f32 * TAU);
            let d = |a: f32| Vec3::new(a.cos(), 0.0, a.sin());
            let p = |r: f32, y: f32, a: f32| d(a) * r + Vec3::Y * y;
            let nrm = |a: f32| (d(a) * slope.x + Vec3::Y * slope.y).normalize();
            b.quad_smooth([p(r0, y0, a1), p(r0, y0, a0), p(r1, y1, a0), p(r1, y1, a1)], [nrm(a1), nrm(a0), nrm(a0), nrm(a1)], Color::WHITE, [Vec2::ZERO; 4]);
        }
    }
    b.build()
}

/// A palm: a gently curving ringed trunk and a crown of drooping, creased fronds.
pub(super) fn palm(trunks: &mut MeshBuilder, fronds: &mut MeshBuilder, base: Vec3, height: f32, lean: Vec2, rng: &mut Lcg) {
    let spine = |t: f32| base + Vec3::Y * height * t + v3(lean, 0.0) * t * t;
    let (rings, sides) = (9, 7);
    for k in 0..rings {
        let (t0, t1) = (k as f32 / rings as f32, (k + 1) as f32 / rings as f32);
        let r = |t: f32| 0.42 - 0.16 * t;
        let c = if k % 2 == 0 { Color::srgb(0.58, 0.47, 0.35) } else { Color::srgb(0.5, 0.4, 0.3) };
        for j in 0..sides {
            let (a0, a1) = (j as f32 / sides as f32 * TAU, (j + 1) as f32 / sides as f32 * TAU);
            let d = |a: f32| Vec3::new(a.cos(), 0.0, a.sin());
            let p = |t: f32, a: f32| spine(t) + d(a) * r(t);
            trunks.quad_smooth([p(t0, a1), p(t0, a0), p(t1, a0), p(t1, a1)], [d(a1), d(a0), d(a0), d(a1)], c, [Vec2::ZERO; 4]);
        }
    }
    let crown = spine(1.0);
    let count = 11;
    for f in 0..count {
        let phi = f as f32 / count as f32 * TAU + rng.range(-0.2, 0.2);
        let dir = Vec2::from_angle(phi);
        let len = rng.range(4.2, 5.8);
        let lift = rng.range(0.7, 1.3);
        let shade = rng.range(0.8, 1.1);
        let col = Color::srgb(0.2 * shade, 0.46 * shade, 0.16 * shade);
        let q = |s: f32| crown + v3(dir, 0.0) * len * s + Vec3::Y * (lift * s - 2.3 * s * s) * len / 5.0;
        // (sin(PI) is a hair below zero, and a negative powf is NaN)
        let width = |s: f32| 0.8 * (PI * s).sin().max(0.0).powf(0.7) * len / 5.0;
        let side = v3(dir.perp(), 0.0);
        let segs = 6;
        for k in 0..segs {
            let (s0, s1) = (k as f32 / segs as f32, (k + 1) as f32 / segs as f32);
            for sd in [-1.0f32, 1.0] {
                let edge = |s: f32| q(s) + side * sd * width(s) - Vec3::Y * 0.3 * width(s);
                // both windings: seen from below as much as from above
                for up in [Vec3::Y, -Vec3::Y] {
                    fronds.quad_facing([q(s0), q(s1), edge(s1), edge(s0)], up, col, [Vec2::ZERO; 4]);
                }
            }
        }
    }
}

/// A closed box standing on `base`: `fwd` and `out` span the floor, half-sizes `hx` along `fwd`
/// and `hz` along `out`, `h` tall. No bottom.
#[allow(clippy::too_many_arguments)]
pub(super) fn boxy(b: &mut MeshBuilder, base: Vec3, fwd: Vec3, out: Vec3, hx: f32, h: f32, hz: f32, c: Color) {
    let p = |x: f32, y: f32, z: f32| base + fwd * x * hx + Vec3::Y * y + out * z * hz;
    for (dir, q) in [
        (fwd, [p(1.0, 0.0, -1.0), p(1.0, 0.0, 1.0), p(1.0, h, 1.0), p(1.0, h, -1.0)]),
        (-fwd, [p(-1.0, 0.0, -1.0), p(-1.0, 0.0, 1.0), p(-1.0, h, 1.0), p(-1.0, h, -1.0)]),
        (out, [p(-1.0, 0.0, 1.0), p(1.0, 0.0, 1.0), p(1.0, h, 1.0), p(-1.0, h, 1.0)]),
        (-out, [p(-1.0, 0.0, -1.0), p(1.0, 0.0, -1.0), p(1.0, h, -1.0), p(-1.0, h, -1.0)]),
        (Vec3::Y, [p(-1.0, h, -1.0), p(1.0, h, -1.0), p(1.0, h, 1.0), p(-1.0, h, 1.0)]),
    ] {
        let uv = q.map(|v| Vec2::new(v.x + v.z, v.y) / 3.0);
        b.quad_facing(q, dir, c, uv);
    }
}

/// A hipped roof over a `hx` by `hz` rectangle at `base`, rising `rise` to a ridge along `fwd`.
#[allow(clippy::too_many_arguments)]
pub(super) fn hip_roof(b: &mut MeshBuilder, base: Vec3, fwd: Vec3, out: Vec3, hx: f32, hz: f32, rise: f32, c: Color) {
    let p = |x: f32, z: f32| base + fwd * x + out * z;
    let ridge = (hx - hz).max(0.3);
    let (r0, r1) = (base - fwd * ridge + Vec3::Y * rise, base + fwd * ridge + Vec3::Y * rise);
    b.quad_facing([p(-hx, hz), p(hx, hz), r1, r0], out + Vec3::Y, c, [Vec2::ZERO; 4]);
    b.quad_facing([p(-hx, -hz), p(hx, -hz), r1, r0], -out + Vec3::Y, c, [Vec2::ZERO; 4]);
    for (sx, r) in [(1.0f32, r1), (-1.0, r0)] {
        tri_facing(b, [p(sx * hx, -hz), p(sx * hx, hz), r], fwd * sx + Vec3::Y, c);
    }
}

/// Triangle wound to face `toward`.
pub(super) fn tri_facing(b: &mut MeshBuilder, mut v: [Vec3; 3], toward: Vec3, c: Color) {
    let mut n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or(Vec3::Y);
    if n.dot(toward) < 0.0 {
        v.swap(0, 1);
        n = -n;
    }
    b.tri(v, n, c);
}

/// A little glowing bulb.
pub(super) fn octahedron(b: &mut MeshBuilder, c: Vec3, r: f32) {
    let (x, y, z) = (Vec3::X * r, Vec3::Y * r * 1.3, Vec3::Z * r);
    for (a, bb) in [(x, z), (z, -x), (-x, -z), (-z, x)] {
        for v in [y, -y] {
            tri_facing(b, [c + a, c + bb, c + v], a + bb + v, Color::WHITE);
        }
    }
}

/// A smooth-shaded quad whose corners may be listed either way round: it's wound to match its
/// normals.
fn quad_rounded(b: &mut MeshBuilder, v: [Vec3; 4], n: [Vec3; 4], c: Color) {
    let face = (v[1] - v[0]).cross(v[2] - v[0]) + (v[2] - v[0]).cross(v[3] - v[0]);
    if face.dot(n[0] + n[1] + n[2] + n[3]) >= 0.0 {
        b.quad_smooth(v, n, c, v.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
    } else {
        let (v, n) = ([v[3], v[2], v[1], v[0]], [n[3], n[2], n[1], n[0]]);
        b.quad_smooth(v, n, c, v.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
    }
}

/// A round bar from `a` to `c`, tapering from radius `r0` to `r1`, without end caps: trunks,
/// branches, vines, posts and cables.
pub(super) fn tube(b: &mut MeshBuilder, a: Vec3, c: Vec3, r0: f32, r1: f32, sides: usize, col: Color) {
    let axis = (c - a).normalize_or(Vec3::Y);
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let d = |j: usize| {
        let t = j as f32 / sides as f32 * TAU;
        u * t.cos() + v * t.sin()
    };
    for j in 0..sides {
        let (d0, d1) = (d(j), d(j + 1));
        quad_rounded(b, [a + d0 * r0, a + d1 * r0, c + d1 * r1, c + d0 * r1], [d0, d1, d1, d0], col);
    }
}

/// A square-edged bar from `a` to `c`, `w` wide and `h` deep (`h` is measured along `up`).
pub(super) fn beam(b: &mut MeshBuilder, a: Vec3, c: Vec3, w: f32, h: f32, up: Vec3, col: Color) {
    let axis = (c - a).normalize_or(Vec3::X);
    let side = axis.cross(up).normalize_or(Vec3::X) * w * 0.5;
    let up = side.cross(axis).normalize_or(Vec3::Y) * h * 0.5;
    for (n, o) in [(side, up), (-side, up), (up, side), (-up, side)] {
        let q = [a + n - o, c + n - o, c + n + o, a + n + o];
        b.quad_facing(q, n, col, q.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
    }
    for (p, n) in [(a, -axis), (c, axis)] {
        b.quad_facing([p - side - up, p + side - up, p + side + up, p - side + up], n, col, [Vec2::ZERO; 4]);
    }
}

/// An ellipsoid with radii `r` about `c`, smooth-shaded: boulders, bushes, tree crowns, globes.
/// With `top` it's only the upper half, standing on `c`: a dome.
pub(super) fn blob(b: &mut MeshBuilder, c: Vec3, r: Vec3, segs: usize, top: bool, col: Color) {
    let rings = if top { segs / 4 } else { segs / 2 }.max(2);
    let point = |i: usize, j: usize| {
        let a = if top { i as f32 / rings as f32 * FRAC_PI_2 } else { i as f32 / rings as f32 * PI - FRAC_PI_2 };
        let t = j as f32 / segs as f32 * TAU;
        let n = Vec3::new(t.cos() * a.cos(), a.sin(), t.sin() * a.cos());
        (c + n * r, (n / r).normalize_or(Vec3::Y))
    };
    for i in 0..rings {
        for j in 0..segs {
            let q = [point(i, j + 1), point(i, j), point(i + 1, j), point(i + 1, j + 1)];
            quad_rounded(b, q.map(|p| p.0), q.map(|p| p.1), col);
        }
    }
}

/// A cone standing on `base`: spires, and the pointed roofs of Georgian churches. Faceted when
/// `segs` is small.
pub(super) fn cone(b: &mut MeshBuilder, base: Vec3, r: f32, h: f32, segs: usize, col: Color) {
    let tip = base + Vec3::Y * h;
    for j in 0..segs {
        let p = |j: usize| {
            let t = j as f32 / segs as f32 * TAU;
            base + Vec3::new(t.cos(), 0.0, t.sin()) * r
        };
        let (p0, p1) = (p(j), p(j + 1));
        tri_facing(b, [p0, p1, tip], (p0 + p1) * 0.5 - base + Vec3::Y * r, col);
    }
}

/// A leaf or frond: a pointed blade from `root`, `len` long towards `dir` and `width` across,
/// creased along its midrib and drooping by `droop` of its length at the tip. One-sided: draw
/// it with a double-sided material.
pub(super) fn leaf(b: &mut MeshBuilder, root: Vec3, dir: Vec3, len: f32, width: f32, droop: f32, col: Color) {
    let side = dir.cross(Vec3::Y).normalize_or(Vec3::X);
    let rib = |s: f32| root + dir * len * s - Vec3::Y * droop * len * s * s;
    // (sin(PI) is a hair below zero, and a negative powf is NaN)
    let half = |s: f32| width * 0.5 * (PI * s).sin().max(0.0).powf(0.6);
    const SEGS: usize = 4;
    for k in 0..SEGS {
        let (s0, s1) = (k as f32 / SEGS as f32, (k + 1) as f32 / SEGS as f32);
        for sd in [-1.0f32, 1.0] {
            let edge = |s: f32| rib(s) + side * sd * half(s) + Vec3::Y * 0.25 * half(s);
            b.quad_facing([rib(s0), rib(s1), edge(s1), edge(s0)], Vec3::Y, col, [Vec2::ZERO; 4]);
        }
    }
}

/// A string of lights sagging from `a` to `c`: a thin wire, and a bulb every `1 / count` of the
/// way along.
pub(super) fn festoon(wire: &mut MeshBuilder, bulbs: &mut MeshBuilder, a: Vec3, c: Vec3, sag: f32, count: usize) {
    let p = |u: f32| a.lerp(c, u) - Vec3::Y * sag * 4.0 * u * (1.0 - u);
    for s in 0..count {
        let (u0, u1) = (s as f32 / count as f32, (s + 1) as f32 / count as f32);
        let (p0, p1) = (p(u0), p(u1));
        let w = Vec3::Y * 0.03;
        let across = (p1 - p0).cross(Vec3::Y);
        for f in [across, -across] {
            wire.quad_facing([p0 - w, p1 - w, p1 + w, p0 + w], f, Color::WHITE, [Vec2::ZERO; 4]);
        }
        octahedron(bulbs, p((u0 + u1) * 0.5) - Vec3::Y * 0.2, 0.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// A NaN vertex poisons the mesh's bounding box, and it gets culled from some angles.
    #[test]
    fn shapes_are_finite() {
        let mut b = MeshBuilder::default();
        let mut rng = Lcg(3);
        tube(&mut b, Vec3::ZERO, Vec3::new(1.0, 5.0, 2.0), 0.5, 0.2, 7, Color::WHITE);
        tube(&mut b, Vec3::ZERO, Vec3::ZERO, 0.5, 0.2, 5, Color::WHITE);
        beam(&mut b, Vec3::ZERO, Vec3::Y * 3.0, 0.2, 0.3, Vec3::Y, Color::WHITE);
        blob(&mut b, Vec3::ONE, Vec3::new(2.0, 1.0, 3.0), 10, false, Color::WHITE);
        blob(&mut b, Vec3::ONE, Vec3::splat(4.0), 12, true, Color::WHITE);
        cone(&mut b, Vec3::ZERO, 2.0, 5.0, 8, Color::WHITE);
        for k in 0..20 {
            let dir = Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)).normalize_or(Vec3::Y);
            leaf(&mut b, Vec3::splat(k as f32), dir, 3.0, 1.5, 0.4, Color::WHITE);
        }
        leaf(&mut b, Vec3::ZERO, Vec3::Y, 3.0, 1.5, 0.4, Color::WHITE);
        let mut bulbs = MeshBuilder::default();
        festoon(&mut b, &mut bulbs, Vec3::ZERO, Vec3::new(30.0, 0.0, 4.0), 2.0, 12);
        for (k, m) in [b.build(), bulbs.build()].iter().enumerate() {
            let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("mesh {k} has no positions") };
            assert!(p.iter().flatten().all(|v| v.is_finite()), "mesh {k} has a non-finite vertex");
            let Some(VertexAttributeValues::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!("mesh {k} has no normals") };
            assert!(n.iter().flatten().all(|v| v.is_finite()), "mesh {k} has a non-finite normal");
        }
    }
}
