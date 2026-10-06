//! Signed-distance sculpting. A model is a list of primitives (ellipsoids, tapered capsules,
//! rounded boxes, tori, cylinders) that are smoothly added or carved away in order, meshed with
//! surface nets. Colours are "inks": each solid part has one, and paint-only volumes spray
//! more on top, so one mesh carries a whole coat or livery in its vertex colours, with ambient
//! occlusion baked in.
//!
//! The raytracer can't see vertex colours, so a model can also be split into one plain mesh
//! per ink, for the raytracing scene to use with a material of that colour.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};

#[derive(Clone, Copy)]
pub enum Shape {
    /// Centre, radii, rotation.
    Ellipsoid(Vec3, Vec3, Quat),
    /// Tapered capsule from a (radius ra) to b (radius rb).
    Capsule(Vec3, Vec3, f32, f32),
    /// Centre, half extents, corner radius, rotation.
    RoundBox(Vec3, Vec3, f32, Quat),
    /// Centre, ring radius, tube radius, rotation. The ring lies in the local XZ plane.
    Torus(Vec3, f32, f32, Quat),
    /// Centre, radius, half height, edge rounding, rotation. The axis is local Y.
    Cylinder(Vec3, f32, f32, f32, Quat),
}

impl Shape {
    pub fn dist(&self, p: Vec3) -> f32 {
        match *self {
            Shape::Ellipsoid(c, r, rot) => {
                let q = rot.inverse() * (p - c);
                // Inigo Quilez's ellipsoid distance approximation
                let k0 = (q / r).length();
                let k1 = (q / (r * r)).length();
                if k1 < 1e-6 { -r.min_element() } else { k0 * (k0 - 1.0) / k1 }
            }
            Shape::Capsule(a, b, ra, rb) => {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                p.distance(a + ab * t) - (ra + (rb - ra) * t)
            }
            Shape::RoundBox(c, h, r, rot) => {
                let q = (rot.inverse() * (p - c)).abs() - (h - Vec3::splat(r));
                q.max(Vec3::ZERO).length() + q.max_element().min(0.0) - r
            }
            Shape::Torus(c, big, small, rot) => {
                let q = rot.inverse() * (p - c);
                Vec2::new(Vec2::new(q.x, q.z).length() - big, q.y).length() - small
            }
            Shape::Cylinder(c, radius, hh, round, rot) => {
                let q = rot.inverse() * (p - c);
                let d = Vec2::new(Vec2::new(q.x, q.z).length() - (radius - round), q.y.abs() - (hh - round));
                d.max_element().min(0.0) + d.max(Vec2::ZERO).length() - round
            }
        }
    }

    fn bounds(&self) -> (Vec3, Vec3) {
        let m = match *self {
            Shape::Ellipsoid(c, r, _) => (c, r.max_element()),
            Shape::Capsule(a, b, ra, rb) => {
                let m = Vec3::splat(ra.max(rb));
                return (a.min(b) - m, a.max(b) + m);
            }
            Shape::RoundBox(c, h, _, _) => (c, h.length()),
            Shape::Torus(c, big, small, _) => (c, big + small),
            Shape::Cylinder(c, r, hh, _, _) => (c, Vec2::new(r, hh).length()),
        };
        (m.0 - Vec3::splat(m.1), m.0 + Vec3::splat(m.1))
    }

    /// Scale by `k`, rotate by `rot`, then move by `o` (all about the origin).
    fn transformed(self, rot: Quat, k: f32, o: Vec3) -> Shape {
        let p = |v: Vec3| rot * (v * k) + o;
        match self {
            Shape::Ellipsoid(c, r, q) => Shape::Ellipsoid(p(c), r * k, rot * q),
            Shape::Capsule(a, b, ra, rb) => Shape::Capsule(p(a), p(b), ra * k, rb * k),
            Shape::RoundBox(c, h, r, q) => Shape::RoundBox(p(c), h * k, r * k, rot * q),
            Shape::Torus(c, big, small, q) => Shape::Torus(p(c), big * k, small * k, rot * q),
            Shape::Cylinder(c, r, hh, round, q) => Shape::Cylinder(p(c), r * k, hh * k, round * k, rot * q),
        }
    }

    fn mirrored(self) -> Shape {
        let f = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        let fq = |q: Quat| Quat::from_xyzw(q.x, -q.y, -q.z, q.w);
        match self {
            Shape::Ellipsoid(c, r, q) => Shape::Ellipsoid(f(c), r, fq(q)),
            Shape::Capsule(a, b, ra, rb) => Shape::Capsule(f(a), f(b), ra, rb),
            Shape::RoundBox(c, h, r, q) => Shape::RoundBox(f(c), h, r, fq(q)),
            Shape::Torus(c, big, small, q) => Shape::Torus(f(c), big, small, fq(q)),
            Shape::Cylinder(c, r, hh, round, q) => Shape::Cylinder(f(c), r, hh, round, fq(q)),
        }
    }
}

pub fn ell(c: Vec3, r: Vec3) -> Shape {
    Shape::Ellipsoid(c, r, Quat::IDENTITY)
}

pub fn ell_rot(c: Vec3, r: Vec3, rot: Quat) -> Shape {
    Shape::Ellipsoid(c, r, rot)
}

pub fn rbox(c: Vec3, half: Vec3, r: f32) -> Shape {
    Shape::RoundBox(c, half, r, Quat::IDENTITY)
}

pub fn cap(a: Vec3, b: Vec3, ra: f32, rb: f32) -> Shape {
    Shape::Capsule(a, b, ra, rb)
}

pub fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    /// Carve away. Surfaces left by the cut take the part's ink, unless it has none.
    Sub,
    /// Keep only what's inside.
    Intersect,
    Paint,
}

/// No ink: a carve that leaves the surrounding colour.
const NO_INK: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct Part {
    shape: Shape,
    ink: u8,
    blend: f32,
    op: Op,
}

/// Soft width of painted edges.
const PAINT_EDGE: f32 = 0.02;

#[derive(Default, Clone)]
pub struct Sculpt {
    parts: Vec<Part>,
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

impl Sculpt {
    /// Add a solid part, blended into what's there over `blend` metres.
    pub fn add(&mut self, shape: Shape, ink: impl Into<u8>, blend: f32) -> &mut Self {
        self.parts.push(Part { shape, ink: ink.into(), blend, op: Op::Add });
        self
    }
    /// Carve a part away from everything added so far, lining the cut with `ink`.
    pub fn sub(&mut self, shape: Shape, ink: impl Into<u8>, blend: f32) -> &mut Self {
        self.parts.push(Part { shape, ink: ink.into(), blend, op: Op::Sub });
        self
    }
    /// Carve a part away, keeping the surrounding colour.
    pub fn cut(&mut self, shape: Shape, blend: f32) -> &mut Self {
        self.parts.push(Part { shape, ink: NO_INK, blend, op: Op::Sub });
        self
    }
    /// Keep only the part of everything so far that lies inside `shape`.
    pub fn intersect(&mut self, shape: Shape, blend: f32) -> &mut Self {
        self.parts.push(Part { shape, ink: NO_INK, blend, op: Op::Intersect });
        self
    }
    /// Paint `ink` onto the surface inside a volume.
    pub fn paint(&mut self, shape: Shape, ink: impl Into<u8>) -> &mut Self {
        self.parts.push(Part { shape, ink: ink.into(), blend: 0.0, op: Op::Paint });
        self
    }
    /// Mirror the most recent `n` parts across x = 0.
    pub fn mirror(&mut self, n: usize) -> &mut Self {
        let start = self.parts.len() - n;
        for i in start..self.parts.len() {
            let p = self.parts[i];
            self.parts.push(Part { shape: p.shape.mirrored(), ..p });
        }
        self
    }
    pub fn len(&self) -> usize {
        self.parts.len()
    }
    /// Scale every part added since `from` by `k`, rotate it by `rot` and move it by `o`.
    pub fn transform_since(&mut self, from: usize, rot: Quat, k: f32, o: Vec3) {
        for p in &mut self.parts[from..] {
            p.shape = p.shape.transformed(rot, k, o);
            p.blend *= k;
        }
    }

    pub fn sdf(&self, p: Vec3) -> f32 {
        let mut d = f32::MAX;
        for part in &self.parts {
            match part.op {
                Op::Add => {
                    let di = part.shape.dist(p);
                    d = if d == f32::MAX { di } else { smin(d, di, part.blend) };
                }
                Op::Sub if d != f32::MAX => d = -smin(-d, part.shape.dist(p), part.blend),
                Op::Intersect if d != f32::MAX => d = -smin(-d, -part.shape.dist(p), part.blend),
                _ => {}
            }
        }
        d
    }

    /// The ink of the solid part a surface point belongs to, and the paint layers over it.
    fn layers(&self, p: Vec3) -> (u8, Vec<(u8, f32)>) {
        // The part it's deepest inside, unless a later carve lined the surface here.
        let mut best = (f32::MAX, 0u8);
        for part in &self.parts {
            match part.op {
                Op::Add => {
                    let d = part.shape.dist(p);
                    if d < best.0 {
                        best = (d, part.ink);
                    }
                }
                Op::Sub if part.ink != NO_INK => {
                    if part.shape.dist(p) < part.blend * 0.5 + 0.01 {
                        best = (f32::MIN, part.ink);
                    }
                }
                _ => {}
            }
        }
        let mut paints = vec![];
        for part in self.parts.iter().filter(|p| p.op == Op::Paint) {
            let w = (0.5 - part.shape.dist(p) / PAINT_EDGE).clamp(0.0, 1.0);
            if w > 0.0 {
                paints.push((part.ink, w));
            }
        }
        (best.1, paints)
    }

    /// Mesh the zero surface with surface nets at the given cell size. `ao_reach` is how far
    /// (in metres) baked ambient occlusion looks for nearby surfaces; 0 bakes none.
    pub fn mesh(&self, cell: f32, ao_reach: f32) -> Sculpted {
        let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
        for p in self.parts.iter().filter(|p| p.op == Op::Add) {
            let (a, b) = p.shape.bounds();
            lo = lo.min(a);
            hi = hi.max(b);
        }
        lo -= Vec3::splat(cell * 2.0);
        hi += Vec3::splat(cell * 2.0);
        let n = ((hi - lo) / cell).ceil().as_uvec3() + UVec3::ONE;
        let (nx, ny, nz) = (n.x as usize, n.y as usize, n.z as usize);
        let at = |x: usize, y: usize, z: usize| lo + Vec3::new(x as f32, y as f32, z as f32) * cell;
        let idx = |x: usize, y: usize, z: usize| (z * ny + y) * nx + x;

        // Sample the field, a slab of z per thread.
        let mut field = vec![0.0f32; nx * ny * nz];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
        let slab = nz.div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for (i, chunk) in field.chunks_mut(slab * nx * ny).enumerate() {
                s.spawn(move || {
                    for (j, d) in chunk.iter_mut().enumerate() {
                        let (x, y, z) = (j % nx, (j / nx) % ny, i * slab + j / (nx * ny));
                        *d = self.sdf(at(x, y, z));
                    }
                });
            }
        });

        // One vertex per cell that straddles the surface, at the mean of its edge crossings.
        const CORNERS: [(usize, usize, usize); 8] = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)];
        const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
        let mut cell_vert = vec![u32::MAX; nx * ny * nz];
        let mut pos: Vec<Vec3> = vec![];
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let v: [f32; 8] = CORNERS.map(|(dx, dy, dz)| field[idx(x + dx, y + dy, z + dz)]);
                    let inside = v.iter().filter(|&&d| d < 0.0).count();
                    if inside == 0 || inside == 8 {
                        continue;
                    }
                    let mut sum = Vec3::ZERO;
                    let mut k = 0.0;
                    for (a, b) in EDGES {
                        if (v[a] < 0.0) != (v[b] < 0.0) {
                            let t = v[a] / (v[a] - v[b]);
                            let (ca, cb) = (CORNERS[a], CORNERS[b]);
                            sum += at(x + ca.0, y + ca.1, z + ca.2).lerp(at(x + cb.0, y + cb.1, z + cb.2), t);
                            k += 1.0;
                        }
                    }
                    cell_vert[idx(x, y, z)] = pos.len() as u32;
                    pos.push(sum / k);
                }
            }
        }

        // A quad for every grid edge that crosses the surface, joining the four cells around it.
        let grad = |p: Vec3| {
            let e = cell * 0.5;
            Vec3::new(
                self.sdf(p + Vec3::X * e) - self.sdf(p - Vec3::X * e),
                self.sdf(p + Vec3::Y * e) - self.sdf(p - Vec3::Y * e),
                self.sdf(p + Vec3::Z * e) - self.sdf(p - Vec3::Z * e),
            )
            .normalize_or(Vec3::Y)
        };
        let mut indices: Vec<u32> = vec![];
        let mut quad = |q: [u32; 4], pos: &Vec<Vec3>| {
            if q.contains(&u32::MAX) {
                return;
            }
            let p = q.map(|i| pos[i as usize]);
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            let c = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            let q = if n.dot(grad(c)) < 0.0 { [q[0], q[3], q[2], q[1]] } else { q };
            indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
        };
        for z in 1..nz - 1 {
            for y in 1..ny - 1 {
                for x in 1..nx - 1 {
                    let d0 = field[idx(x, y, z)] < 0.0;
                    if (field[idx(x + 1, y, z)] < 0.0) != d0 {
                        quad([idx(x, y - 1, z - 1), idx(x, y, z - 1), idx(x, y, z), idx(x, y - 1, z)].map(|i| cell_vert[i]), &pos);
                    }
                    if (field[idx(x, y + 1, z)] < 0.0) != d0 {
                        quad([idx(x - 1, y, z - 1), idx(x, y, z - 1), idx(x, y, z), idx(x - 1, y, z)].map(|i| cell_vert[i]), &pos);
                    }
                    if (field[idx(x, y, z + 1)] < 0.0) != d0 {
                        quad([idx(x - 1, y - 1, z), idx(x, y - 1, z), idx(x, y, z), idx(x - 1, y, z)].map(|i| cell_vert[i]), &pos);
                    }
                }
            }
        }

        let nrm: Vec<Vec3> = pos.iter().map(|&p| grad(p)).collect();
        let layers: Vec<_> = pos.iter().map(|&p| self.layers(p)).collect();
        // Ambient occlusion: how much the surface around each vertex closes in on it.
        let ao = pos
            .iter()
            .zip(&nrm)
            .map(|(&p, &n)| {
                if ao_reach <= 0.0 {
                    return 1.0;
                }
                let mut occ = 0.0;
                let mut w = 1.0;
                for i in 1..=5 {
                    let h = ao_reach * i as f32 / 5.0;
                    occ += (h - self.sdf(p + n * h)).max(0.0) * w;
                    w *= 0.6;
                }
                (1.0 - occ / ao_reach * 0.9).clamp(0.3, 1.0)
            })
            .collect();
        // The ink that shows on each triangle, for the raytracing split.
        let top = |(base, paints): &(u8, Vec<(u8, f32)>)| paints.iter().rev().find(|(_, w)| *w >= 0.5).map_or(*base, |p| p.0);
        let vert_ink: Vec<u8> = layers.iter().map(top).collect();
        let tri_ink = indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| vert_ink[t[k] as usize]);
                if b == c { b } else { a }
            })
            .collect();
        Sculpted { pos, nrm, ao, layers, indices, tri_ink }
    }
}

/// A meshed sculpture, ready to be coloured.
pub struct Sculpted {
    pos: Vec<Vec3>,
    nrm: Vec<Vec3>,
    ao: Vec<f32>,
    layers: Vec<(u8, Vec<(u8, f32)>)>,
    indices: Vec<u32>,
    tri_ink: Vec<u8>,
}

/// Smooth 3D value noise in 0..1, for mottling fur.
fn noise(p: Vec3) -> f32 {
    let h = |c: IVec3| {
        let n = (c.x.wrapping_mul(73_856_093) ^ c.y.wrapping_mul(19_349_663) ^ c.z.wrapping_mul(83_492_791)) as u32;
        let n = n.wrapping_mul(0x2710_0001).rotate_left(13).wrapping_mul(0x9E37_79B9);
        (n >> 8) as f32 / (1u32 << 24) as f32
    };
    let i = p.floor().as_ivec3();
    let f = p - p.floor();
    let f = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let mut out = 0.0;
    for k in 0..8 {
        let o = IVec3::new(k & 1, (k >> 1) & 1, (k >> 2) & 1);
        let w = (if o.x == 1 { f.x } else { 1.0 - f.x }) * (if o.y == 1 { f.y } else { 1.0 - f.y }) * (if o.z == 1 { f.z } else { 1.0 - f.z });
        out += h(i + o) * w;
    }
    out
}

impl Sculpted {
    /// The whole model in vertex colours from `palette` (indexed by ink), with baked occlusion
    /// and, if `mottle` > 0, a soft fur-like variation in brightness of that strength.
    pub fn mesh(&self, palette: &[LinearRgba], mottle: f32) -> Mesh {
        let colors: Vec<[f32; 4]> = self
            .layers
            .iter()
            .zip(&self.pos)
            .zip(&self.ao)
            .map(|(((base, paints), &p), &ao)| {
                let mut c = palette[*base as usize];
                for &(ink, w) in paints {
                    c = c.mix(&palette[ink as usize], w);
                }
                let m = if mottle > 0.0 { 1.0 + (noise(p * 18.0) * 0.65 + noise(p * 47.0) * 0.35 - 0.5) * 2.0 * mottle } else { 1.0 };
                (c * (ao * m)).with_alpha(1.0).to_f32_array()
            })
            .collect();
        self.build(&self.indices, Some(colors))
    }

    /// Inks used on the surface.
    pub fn inks(&self) -> Vec<u8> {
        let mut inks: Vec<u8> = self.tri_ink.clone();
        inks.sort_unstable();
        inks.dedup();
        inks
    }

    /// The triangles showing `ink`, as a plain mesh (for the raytracing scene).
    pub fn ink_mesh(&self, ink: u8) -> Mesh {
        let idx: Vec<u32> = self.indices.chunks(3).zip(&self.tri_ink).filter(|(_, k)| **k == ink).flat_map(|(t, _)| t.iter().copied()).collect();
        self.build(&idx, None)
    }

    fn build(&self, idx: &[u32], colors: Option<Vec<[f32; 4]>>) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos.iter().map(|p| p.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm.iter().map(|p| p.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; self.pos.len()])
            .with_inserted_indices(Indices::U32(idx.to_vec()));
        if let Some(c) = colors {
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
        }
        m
    }
}

impl Sculpted {
    /// Raytracing meshes for each ink on the surface (see `spawn_inked`).
    pub fn rt_split(&self, meshes: &mut Assets<Mesh>) -> Vec<(u8, Handle<Mesh>)> {
        self.inks().into_iter().filter_map(|k| Some((k, meshes.add(super::gfx::raytracing_proxy(&self.ink_mesh(k))?)))).collect()
    }
}

/// Spawn a kart part's vertex-coloured sculpture, which drops to coarser meshes with distance. If `rt` is given (the raytraced renderer), the raytracer
/// sees it as one mesh per ink instead, each with that ink's material from `rt_mats`.
pub fn spawn_inked<'a>(
    p: &'a mut ChildSpawnerCommands,
    mesh: &super::lod::Lods,
    mat: &Handle<StandardMaterial>,
    rt: &[(u8, Handle<Mesh>)],
    rt_mats: &[Handle<StandardMaterial>],
    extra: impl Bundle,
) -> EntityCommands<'a> {
    let mut e = p.spawn((Mesh3d(mesh.full().clone()), MeshMaterial3d(mat.clone()), mesh.lod(super::lod::KART_LOD), extra));
    if !rt.is_empty() {
        e.insert(super::gfx::NoRaytrace).with_children(|c| {
            for (ink, m) in rt {
                c.spawn((RaytracingMesh3d(m.clone()), MeshMaterial3d(rt_mats[*ink as usize].clone()), Transform::default()));
            }
        });
    }
    e
}
