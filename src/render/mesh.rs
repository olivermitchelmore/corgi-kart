//! Helpers for building static meshes in code: a vertex-coloured triangle soup builder and a
//! deterministic RNG for scattering scenery.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

#[derive(Default)]
pub struct MeshBuilder {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl MeshBuilder {
    /// Quad with planar world-space UVs (x, z) / `tile`.
    pub fn quad(&mut self, v: [Vec3; 4], n: Vec3, c: Color) {
        self.quad_uv(v, n, c, v.map(|p| Vec2::new(p.x, p.z) / 4.0));
    }

    pub fn quad_uv(&mut self, v: [Vec3; 4], n: Vec3, c: Color, uv: [Vec2; 4]) {
        let base = self.pos.len() as u32;
        let c = c.to_linear().to_f32_array();
        for (p, t) in v.into_iter().zip(uv) {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.col.push(c);
            self.uv.push(t.to_array());
        }
        self.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Quad with a colour per corner.
    pub fn quad_vc(&mut self, v: [Vec3; 4], n: Vec3, c: [Color; 4], uv: [Vec2; 4]) {
        let base = self.pos.len() as u32;
        for k in 0..4 {
            self.pos.push(v[k].to_array());
            self.nrm.push(n.to_array());
            self.col.push(c[k].to_linear().to_f32_array());
            self.uv.push(uv[k].to_array());
        }
        self.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Quad with a normal per corner, for smooth curved surfaces.
    pub fn quad_smooth(&mut self, v: [Vec3; 4], n: [Vec3; 4], c: Color, uv: [Vec2; 4]) {
        let base = self.pos.len() as u32;
        let c = c.to_linear().to_f32_array();
        for k in 0..4 {
            self.pos.push(v[k].to_array());
            self.nrm.push(n[k].to_array());
            self.col.push(c);
            self.uv.push(uv[k].to_array());
        }
        self.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Quad whose normal is worked out from its winding (counter-clockwise = front).
    pub fn quad_auto(&mut self, v: [Vec3; 4], c: Color, uv: [Vec2; 4]) {
        let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or(Vec3::Y);
        self.quad_uv(v, n, c, uv);
    }

    /// Quad that faces `toward` whichever way its corners were listed.
    pub fn quad_facing(&mut self, v: [Vec3; 4], toward: Vec3, c: Color, uv: [Vec2; 4]) {
        let n = (v[1] - v[0]).cross(v[2] - v[0]);
        if n.dot(toward) >= 0.0 {
            self.quad_auto(v, c, uv);
        } else {
            self.quad_auto([v[3], v[2], v[1], v[0]], c, [uv[3], uv[2], uv[1], uv[0]]);
        }
    }

    pub fn tri(&mut self, v: [Vec3; 3], n: Vec3, c: Color) {
        let base = self.pos.len() as u32;
        let c = c.to_linear().to_f32_array();
        for p in v {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.col.push(c);
            self.uv.push([p.x / 4.0, p.z / 4.0]);
        }
        self.idx.extend([base, base + 1, base + 2]);
    }

    /// Build with vertex colours made relative to their average, returning that average as a
    /// material colour derived from `template`. Raytraced bounce light only sees the material
    /// colour, so this keeps grass green and roads grey in the raytraced mode too.
    pub fn tinted(mut self, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, template: &Handle<StandardMaterial>) -> (Mesh3d, MeshMaterial3d<StandardMaterial>) {
        let n = self.col.len().max(1) as f32;
        let mut avg = [0.0f32; 3];
        for c in &self.col {
            for k in 0..3 {
                avg[k] += c[k] / n;
            }
        }
        let avg = avg.map(|a| a.max(1e-3));
        for c in &mut self.col {
            for k in 0..3 {
                c[k] /= avg[k];
            }
        }
        let mut m = mats.get(template).cloned().unwrap_or_default();
        let base = m.base_color.to_linear();
        m.base_color = Color::linear_rgba(base.red * avg[0], base.green * avg[1], base.blue * avg[2], base.alpha);
        (Mesh3d(meshes.add(self.build())), MeshMaterial3d(mats.add(m)))
    }

    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    pub fn build(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

pub fn v3(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Deterministic scatter RNG so the scenery is identical for everyone.
pub struct Lcg(pub u32);

impl Lcg {
    pub fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.f() * items.len() as f32) as usize % items.len()]
    }
}
