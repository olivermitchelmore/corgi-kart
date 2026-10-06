//! Distance-based level of detail for the sculpted meshes.
//!
//! The sculpted karts, wheels and crowd corgis are meshed with cells of a centimetre or two, so
//! a single tyre has 47k triangles and a stand full of fans has millions, all drawn (and drawn
//! again into every shadow cascade) however far away they are. Each of these gets two coarser
//! meshes of the same sculpture (twice and four times the cell size, so about a quarter and a
//! sixteenth of the triangles), and `update_lods` swaps a mesh in once the coarser cells are
//! about a pixel across on screen, which is when nobody can tell them apart.
//!
//! Only the rasterised mesh changes: the raytracer keeps the full-detail proxies it made
//! from the level-0 mesh.

use bevy::prelude::*;

use super::split::PlayerCamera;

/// One sculpture meshed at three levels of detail; level 0 is the full-detail mesh.
#[derive(Clone)]
pub struct Lods([Handle<Mesh>; 3]);

/// Cell size multiplier of each level.
pub const LOD_CELL_SCALE: [f32; 3] = [1.0, 2.0, 4.0];

/// Distances (metres from the nearest player camera) beyond which a kart's parts use levels 1
/// and 2. Cells are 1-2cm, so at 20m they are about a pixel even at 2560x1600.
pub const KART_LOD: [f32; 2] = [20.0, 55.0];
/// The same for a crowd corgi, whose cells are 4.5cm (at 1.3x scale).
pub const FAN_LOD: [f32; 2] = [45.0, 110.0];

impl Lods {
    pub fn new(levels: [Handle<Mesh>; 3]) -> Self {
        Lods(levels)
    }

    /// The full-detail mesh.
    pub fn full(&self) -> &Handle<Mesh> {
        &self.0[0]
    }

    /// The component that swaps the entity's mesh between the levels. The entity starts on
    /// level 0, so spawn it with `full()`.
    pub fn lod(&self, switch: [f32; 2]) -> Lod {
        Lod { meshes: self.0.clone(), switch, level: 0 }
    }
}

/// Swaps the entity's `Mesh3d` between levels of detail by distance.
#[derive(Component)]
pub struct Lod {
    meshes: [Handle<Mesh>; 3],
    /// Distances beyond which levels 1 and 2 are used.
    switch: [f32; 2],
    level: usize,
}

/// A level only changes once the distance is this far past its threshold, so an entity sitting
/// on one doesn't flip back and forth.
const HYSTERESIS: f32 = 0.06;

pub fn update_lods(cams: Query<&GlobalTransform, With<PlayerCamera>>, mut lods: Query<(&GlobalTransform, &mut Lod, &mut Mesh3d)>, mut eyes: Local<Vec<Vec3>>) {
    eyes.clear();
    eyes.extend(cams.iter().map(|c| c.translation()));
    if eyes.is_empty() {
        return;
    }
    for (at, mut lod, mut mesh) in &mut lods {
        let p = at.translation();
        let d = eyes.iter().map(|e| e.distance_squared(p)).fold(f32::MAX, f32::min).sqrt();
        // Read through `Deref` and only write on a change, so unchanged meshes aren't re-extracted.
        let mut level = lod.level;
        while level < 2 && d > lod.switch[level] * (1.0 + HYSTERESIS) {
            level += 1;
        }
        while level > 0 && d < lod.switch[level - 1] * (1.0 - HYSTERESIS) {
            level -= 1;
        }
        if level != lod.level {
            lod.level = level;
            mesh.0 = lod.meshes[level].clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh steps down as the camera backs away, and doesn't flicker on a threshold.
    #[test]
    fn swaps_by_distance_with_hysteresis() {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default()).init_asset::<Mesh>().add_systems(Update, update_lods);
        let handles: [Handle<Mesh>; 3] = [0, 1, 2].map(|_| app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default())));
        let lods = Lods::new(handles.clone());
        let cam = app.world_mut().spawn((PlayerCamera(0), GlobalTransform::default())).id();
        let obj = app.world_mut().spawn((Mesh3d(lods.full().clone()), lods.lod([10.0, 30.0]), GlobalTransform::default())).id();
        let level_at = |app: &mut App, d: f32| {
            app.world_mut().entity_mut(cam).insert(GlobalTransform::from_xyz(0.0, 0.0, d));
            app.update();
            handles.iter().position(|h| *h == app.world().get::<Mesh3d>(obj).unwrap().0).unwrap()
        };
        assert_eq!(level_at(&mut app, 5.0), 0);
        assert_eq!(level_at(&mut app, 10.2), 0, "inside the hysteresis band");
        assert_eq!(level_at(&mut app, 12.0), 1);
        assert_eq!(level_at(&mut app, 9.8), 1, "inside the hysteresis band");
        assert_eq!(level_at(&mut app, 100.0), 2);
        assert_eq!(level_at(&mut app, 5.0), 0, "jumps straight back");
    }
}
