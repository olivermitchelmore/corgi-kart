//! Per-track lighting and sky: sunny afternoons for the meadow and circuit, a neon night for
//! Prism Park, golden hour for Mirror Lagoon, hazy midday for Jade Jungle and blue hour for
//! Tbilisi Twilight. Applied whenever the built track changes.

use bevy::{
    light::{GeneratedEnvironmentMapLight, SunDisk},
    mesh::VertexAttributeValues,
    prelude::*,
};

use super::gfx::{sky_cubemap, spawn_sky_light, Graphics};
use super::scene::SkyDome;
use super::split::PlayerCamera;
use crate::game::TrackRes;
use crate::sim::{Theme, TrackId};

#[derive(Clone, Copy)]
pub struct Look {
    pub horizon: Color,
    pub zenith: Color,
    pub below: Color,
    pub sun_rot: Quat,
    pub sun_color: Color,
    pub sun_lux: f32,
    /// Emissive colour of the visible sun (or moon) disc.
    pub disc: LinearRgba,
    pub ambient: f32,
    pub env: f32,
    pub fog: (f32, f32),
    pub night: bool,
    /// Sunset: a wide glow around the sun that hugs the horizon...
    pub halo: LinearRgba,
    /// ...and the pink band on the horizon opposite it.
    pub belt: LinearRgba,
    /// Reflected below the horizon.
    pub ground: LinearRgba,
    /// Angular diameter of the sun in radians; bigger means softer raytraced shadows.
    pub sun_size: f32,
    /// Fog colour when looking towards the sun (the standard renderer's sun glare).
    pub glare: Color,
    /// Give the raytracer the full sky gradient to reflect, rather than two flat bands.
    pub sky_texture: bool,
    /// How brightly the sky lights the raytraced scene, next to how bright it looks: 1 by day,
    /// next to nothing at night, and more where most of it is hidden behind a forest canopy.
    pub sky_light: f32,
}

pub fn look_for(theme: Theme) -> Look {
    match theme {
        Theme::Prism => Look {
            horizon: Color::srgb(0.32, 0.12, 0.38),
            zenith: Color::srgb(0.02, 0.02, 0.09),
            below: Color::srgb(0.05, 0.03, 0.08),
            sun_rot: Quat::from_euler(EulerRot::YXZ, -0.6, -0.7, 0.0),
            sun_color: Color::srgb(0.6, 0.7, 1.0),
            sun_lux: 1800.0,
            disc: LinearRgba::rgb(6.0, 7.0, 9.0),
            ambient: 60.0,
            env: 500.0,
            fog: (250.0, 1600.0),
            night: true,
            sky_light: 0.05,
            ..day()
        },
        Theme::Lagoon => Look {
            horizon: Color::srgb(1.0, 0.62, 0.36),
            zenith: Color::srgb(0.2, 0.26, 0.47),
            // the fogged sea at the horizon, so there's no seam under it
            below: Color::srgb(0.9, 0.6, 0.42),
            // Low in the west, just left of the start straight, so it sets over the marina.
            sun_rot: Quat::from_rotation_arc(Vec3::Z, Vec3::new(0.96, 0.18, -0.2).normalize()),
            sun_color: Color::srgb(1.0, 0.7, 0.42),
            sun_lux: 9_000.0,
            disc: LinearRgba::rgb(1.0, 0.72, 0.4),
            ambient: 160.0,
            env: 700.0,
            fog: (300.0, 2600.0),
            night: false,
            halo: LinearRgba::rgb(1.4, 0.55, 0.18),
            belt: LinearRgba::rgb(0.45, 0.2, 0.32),
            ground: LinearRgba::rgb(0.02, 0.05, 0.08),
            sun_size: 0.022,
            glare: Color::srgba(1.0, 0.6, 0.3, 0.7),
            sky_texture: true,
            sky_light: 1.0,
        },
        Theme::Jungle => Look {
            // Humid haze: a pale horizon, and fog that greys out the far forest.
            horizon: Color::srgb(0.8, 0.92, 0.9),
            zenith: Color::srgb(0.26, 0.56, 0.92),
            below: Color::srgb(0.3, 0.42, 0.28),
            // Well up, behind the start straight's right shoulder: the trees along it dapple
            // the road, and the sun slants in through the side of the temple hall.
            sun_rot: Quat::from_rotation_arc(Vec3::Z, Vec3::new(-0.45, 0.72, 0.52).normalize()),
            sun_color: Color::srgb(1.0, 0.95, 0.82),
            sun_lux: 11_000.0,
            sky_light: 2.6,
            ambient: 230.0,
            env: 800.0,
            fog: (120.0, 1500.0),
            ground: LinearRgba::rgb(0.06, 0.13, 0.05),
            glare: Color::srgba(1.0, 0.95, 0.75, 0.35),
            ..day()
        },
        Theme::Tbilisi => Look {
            horizon: Color::srgb(0.36, 0.3, 0.46),
            zenith: Color::srgb(0.015, 0.035, 0.13),
            below: Color::srgb(0.05, 0.05, 0.09),
            // Just set: a sliver above the horizon ahead of the start straight, hidden by the
            // hills, so the raytracer gets no sunlight, only the afterglow.
            sun_rot: Quat::from_rotation_arc(Vec3::Z, Vec3::new(0.95, 0.03, 0.3).normalize()),
            sun_color: Color::srgb(1.0, 0.6, 0.4),
            sun_lux: 350.0,
            disc: LinearRgba::rgb(1.0, 0.45, 0.2),
            ambient: 90.0,
            env: 420.0,
            fog: (260.0, 2400.0),
            night: false,
            halo: LinearRgba::rgb(1.1, 0.42, 0.16),
            belt: LinearRgba::rgb(0.16, 0.1, 0.22),
            ground: LinearRgba::rgb(0.02, 0.02, 0.03),
            sun_size: 0.02,
            glare: Color::srgba(1.0, 0.5, 0.3, 0.0),
            sky_texture: true,
            // The afterglow lights more than a camera set for daylight would show.
            sky_light: 4.0,
        },
        Theme::Circuit | Theme::Meadow => day(),
    }
}

/// A sunny afternoon.
fn day() -> Look {
    Look {
        horizon: Color::srgb(0.70, 0.84, 0.97),
        zenith: Color::srgb(0.22, 0.47, 0.88),
        below: Color::srgb(0.62, 0.76, 0.86),
        sun_rot: Quat::from_euler(EulerRot::YXZ, 2.4, -0.85, 0.0),
        sun_color: Color::srgb(1.0, 0.96, 0.88),
        sun_lux: 12_000.0,
        disc: LinearRgba::rgb(40.0, 32.0, 20.0),
        ambient: 250.0,
        env: 900.0,
        fog: (350.0, 2200.0),
        night: false,
        halo: LinearRgba::NONE,
        belt: LinearRgba::NONE,
        ground: LinearRgba::rgb(0.16, 0.24, 0.12),
        sun_size: SunDisk::EARTH.angular_size,
        glare: Color::srgba(1.0, 0.93, 0.75, 0.45),
        sky_texture: false,
        sky_light: 1.0,
    }
}

/// Handles to the sky assets that change per track.
#[derive(Resource)]
pub struct SkyAssets {
    pub dome: Handle<Mesh>,
    pub disc: Handle<StandardMaterial>,
    pub disc_entity: Entity,
}

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct SkyLight;

pub fn dome_colors(mesh: &mut Mesh, look: &Look) {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let (zen, hor, below) = (look.zenith.to_linear(), look.horizon.to_linear(), look.below.to_linear());
    let sun = look.sun_rot * Vec3::Z;
    let cols: Vec<[f32; 4]> = pos
        .iter()
        .map(|p| {
            let y = p[1];
            let c = if y >= 0.0 { hor.mix(&zen, y.powf(0.55)) } else { hor.mix(&below, (-y * 4.0).min(1.0)) };
            let g = super::gfx::sunset_glow(Vec3::from_array(*p).normalize_or(Vec3::Y), sun, look);
            LinearRgba::rgb(c.red + g.x, c.green + g.y, c.blue + g.z).to_f32_array()
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
}

#[allow(clippy::too_many_arguments)]
pub fn apply_look(
    mut commands: Commands,
    track: Res<TrackRes>,
    gfx: Res<Graphics>,
    sky: Res<SkyAssets>,
    mut applied: Local<Option<TrackId>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(Entity, &mut DirectionalLight, &mut Transform), With<Sun>>,
    mut cams: Query<(&mut DistanceFog, &mut GeneratedEnvironmentMapLight), With<PlayerCamera>>,
    mut disc: Query<&mut Transform, (Without<Sun>, Without<SkyDome>)>,
    old_lights: Query<Entity, With<SkyLight>>,
) {
    if *applied == Some(track.0.id) {
        return;
    }
    *applied = Some(track.0.id);
    let look = look_for(track.0.theme);
    let sun_dir = look.sun_rot * Vec3::Z;

    if let Ok((e, mut light, mut tr)) = sun.single_mut() {
        light.illuminance = if gfx.raytracing { look.sun_lux * 1.3 } else { look.sun_lux };
        light.color = look.sun_color;
        tr.rotation = look.sun_rot;
        commands.entity(e).insert(SunDisk { angular_size: look.sun_size, ..SunDisk::EARTH });
    }
    ambient.brightness = look.ambient;
    clear.0 = look.horizon;
    if let Some(mut dome) = meshes.get_mut(&sky.dome) {
        dome_colors(&mut dome, &look);
    }
    if let Some(mut m) = mats.get_mut(&sky.disc) {
        // Values over 1 come out black in the blended pass, so keep it at full brightness.
        let d = look.disc;
        m.base_color = Color::LinearRgba(d * (1.0 / d.red.max(d.green).max(d.blue).max(1.0)));
    }
    if let Ok(mut tr) = disc.get_mut(sky.disc_entity) {
        tr.translation = sun_dir * 0.9;
        tr.scale = Vec3::splat(if look.night { 0.035 } else { 0.05 });
    }
    let env = sky_cubemap(&mut images, sun_dir, &look);
    for (mut fog, mut probe) in &mut cams {
        fog.color = look.horizon;
        fog.falloff = FogFalloff::Linear { start: look.fog.0, end: look.fog.1 };
        fog.directional_light_color = if look.night { Color::NONE } else { look.glare };
        probe.environment_map = env.clone();
        probe.intensity = look.env;
    }
    for e in &old_lights {
        commands.entity(e).despawn();
    }
    if gfx.raytracing {
        for e in spawn_sky_light(&mut commands, &mut meshes, &mut mats, &mut images, sun_dir, &look, look.sky_light) {
            commands.entity(e).insert(SkyLight);
        }
    }
}
