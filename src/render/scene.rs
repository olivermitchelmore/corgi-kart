//! The static world: camera, sky, sun and the track's scenery. The scenery is rebuilt whenever
//! the selected track changes, so the menu backdrop previews the track you've picked.

use bevy::{
    camera::RenderTarget,
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster},
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
};

use super::gfx::{camera_effects, sky_cubemap, Graphics, NoRaytrace};
use super::models::Kit;
use super::scenery;
use super::split::PlayerCamera;
use super::textures::Textures;
use super::trackmesh::spawn_track;
use crate::game::{SessionRes, Settings, TrackRes};
use crate::sim::{Theme, Track, TrackId};

pub const PAD_GLOW: LinearRgba = LinearRgba::rgb(3.2, 0.9, 0.1);

pub const SKY: Color = Color::srgb(0.70, 0.84, 0.97);

#[derive(Component)]
pub struct MainCamera;

/// With `--headless`, the image the main camera renders into instead of a window.
#[derive(Resource)]
pub struct HeadlessTarget(pub Handle<Image>);

/// Everything that belongs to the currently built track.
#[derive(Component)]
pub struct Scenery;

#[derive(Component)]
pub struct SkyDome;

/// Shared vertex-coloured, textured materials for the big merged meshes.
#[derive(Resource)]
pub struct SurfaceMats {
    pub road: Handle<StandardMaterial>,
    pub paint: Handle<StandardMaterial>,
    pub grass: Handle<StandardMaterial>,
    pub hedge: Handle<StandardMaterial>,
    pub concrete: Handle<StandardMaterial>,
    /// Boost pads: a glowing rim, a panel of arrows that scroll along the road (`ambient.rs`
    /// moves its texture), a soft glow on the road round it, and the metal frame.
    pub pad: Handle<StandardMaterial>,
    pub pad_arrows: Handle<StandardMaterial>,
    pub pad_halo: Handle<StandardMaterial>,
    pub pad_frame: Handle<StandardMaterial>,
    /// A boosted ramp's face: rainbow bands scrolling up it.
    pub rainbow: Handle<StandardMaterial>,
    /// Painted ramps (vertex coloured).
    pub ramp: Handle<StandardMaterial>,
    pub plain: Handle<StandardMaterial>,
    /// Standing water: dark and mirror-smooth.
    pub puddle: Handle<StandardMaterial>,
    pub windows: Handle<Image>,
}

#[cfg_attr(not(feature = "dlss"), allow(unused_mut))]
pub fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut gfx: ResMut<Graphics>,
    cli: Res<crate::game::Cli>,
    #[cfg(feature = "dlss")] rr: Option<Res<bevy::anti_alias::dlss::DlssRayReconstructionSupported>>,
) {
    #[cfg(feature = "dlss")]
    {
        gfx.dlss = gfx.raytracing && rr.is_some();
    }
    if gfx.raytracing {
        if gfx.dlss {
            info!("ray tracing: denoised with DLSS Ray Reconstruction");
        } else {
            warn!("ray tracing without DLSS Ray Reconstruction will be very noisy; build with `--features dlss` (see README)");
        }
    }
    let kit = Kit::new(&mut meshes, &mut mats, gfx.raytracing);
    if gfx.raytracing {
        // Headlights are real light sources for the raytracer: give the lenses physical
        // brightness (in nits) so they throw light onto the road ahead.
        if let Some(mut m) = mats.get_mut(&kit.headlight) {
            m.emissive = LinearRgba::rgb(60_000.0, 56_000.0, 46_000.0);
        }
    }
    let tex = Textures::new(&mut images);
    // Track-specific values are filled in by `look::apply_look`.
    let look = super::look::look_for(Theme::Circuit);
    let sun_rot = look.sun_rot;
    let sun_dir = sun_rot * Vec3::Z; // towards the sun
    let env = sky_cubemap(&mut images, sun_dir, &look);

    // No window: render into an image instead (the HUD and menu too), which screenshots read.
    let headless = cli.headless.then(|| {
        let (w, h) = cli.size;
        let mut target = Image::new_target_texture(w, h, TextureFormat::Rgba8UnormSrgb, None);
        target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        HeadlessTarget(images.add(target))
    });
    let fog = DistanceFog {
        color: SKY,
        directional_light_color: Color::srgba(1.0, 0.93, 0.75, 0.45),
        directional_light_exponent: 24.0,
        falloff: FogFalloff::Linear { start: 350.0, end: 2200.0 },
    };
    let mut cam = spawn_world_camera(&mut commands, *gfx, env, fog, headless.as_ref().map(|t| t.0.clone()));
    cam.insert((Transform::from_xyz(0.0, 150.0, -250.0).looking_at(Vec3::new(20.0, 0.0, 100.0), Vec3::Y), MainCamera, PlayerCamera(0)));
    if let Some(target) = headless {
        cam.insert(IsDefaultUiCamera);
        commands.insert_resource(target);
    }
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.9, 1.0), brightness: 250.0, ..default() });
    commands.insert_resource(DirectionalLightShadowMap { size: 4096 });
    commands.spawn((
        DirectionalLight {
            illuminance: if gfx.raytracing { 16_000.0 } else { 12_000.0 },
            color: Color::srgb(1.0, 0.96, 0.88),
            // Solari traces its own shadows.
            shadow_maps_enabled: !gfx.raytracing,
            contact_shadows_enabled: !gfx.raytracing,
            ..default()
        },
        Transform::from_rotation(sun_rot),
        super::look::Sun,
        // Three cascades cost ~0.5ms less than four with the same reach and look the same.
        CascadeShadowConfigBuilder { num_cascades: 3, first_cascade_far_bound: 30.0, maximum_distance: 260.0, ..default() }.build(),
    ));

    // Sky dome with a vertical gradient and a glowing sun; it follows the camera.
    let mut dome = Sphere::new(1.0).mesh().uv(96, 48);
    super::look::dome_colors(&mut dome, &look);
    let dome = meshes.add(dome);
    // Blend mode puts the sky in the forward transparent pass, which runs after deferred (or
    // raytraced) lighting, so it looks the same in both renderers.
    let sky_mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        alpha_mode: AlphaMode::Blend,
        // Drawn before anything else that's blended. Sorted by its centre, which is the camera,
        // it would otherwise be painted over whatever blended thing is in front of it.
        depth_bias: -1.0e6,
        ..default()
    });
    // Unlit: blended emissive gets the camera's exposure applied, which leaves it black.
    // (`apply_look` sets the colour.)
    let sun_mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut disc_entity = Entity::PLACEHOLDER;
    commands
        .spawn((Mesh3d(dome.clone()), MeshMaterial3d(sky_mat), Transform::from_scale(Vec3::splat(3000.0)), NotShadowCaster, NoRaytrace, SkyDome))
        .with_children(|d| {
            disc_entity = d
                .spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(sun_mat.clone()), Transform::from_translation(sun_dir * 0.9).with_scale(Vec3::splat(0.05)), NotShadowCaster, NoRaytrace))
                .id();
        });
    commands.insert_resource(super::look::SkyAssets { dome, disc: sun_mat, disc_entity });

    let textured = |mats: &mut Assets<StandardMaterial>, t: &Handle<Image>, rough: f32| {
        mats.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(t.clone()), perceptual_roughness: rough, ..default() })
    };
    // Just after a rain shower: a wet film over the asphalt (clearcoat), glossy paint, damp grass.
    let road = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(tex.asphalt.clone()),
        metallic_roughness_texture: Some(tex.wet.clone()),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        reflectance: 0.5,
        clearcoat: 0.4,
        clearcoat_perceptual_roughness: 0.14,
        ..default()
    });
    let surfaces = SurfaceMats {
        road,
        paint: mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(tex.concrete.clone()),
            perceptual_roughness: 0.3,
            clearcoat: 0.6,
            clearcoat_perceptual_roughness: 0.1,
            ..default()
        }),
        grass: textured(&mut mats, &tex.grass, 0.75),
        hedge: mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(tex.hedge.clone()),
            perceptual_roughness: 1.0,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        concrete: textured(&mut mats, &tex.concrete, 0.55),
        pad: mats.add(StandardMaterial { base_color: Color::srgb(0.9, 0.35, 0.05), emissive: PAD_GLOW, perceptual_roughness: 0.3, ..default() }),
        pad_arrows: mats.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.1, 0.02),
            emissive: LinearRgba::rgb(5.0, 5.0, 5.0),
            emissive_texture: Some(tex.pad_arrows.clone()),
            // Satin, so a low sun doesn't glare off it and hide the arrows.
            perceptual_roughness: 0.6,
            reflectance: 0.3,
            ..default()
        }),
        // Vertex coloured and added on top, so it only ever brightens the road. Unlit, so it looks
        // the same in both renderers, and without fog, which would add the fog colour too.
        pad_halo: mats.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Add, fog_enabled: false, cull_mode: None, ..default() }),
        rainbow: mats.add(StandardMaterial {
            base_color: Color::srgb(0.5, 0.5, 0.5),
            emissive: LinearRgba::rgb(2.5, 2.5, 2.5),
            emissive_texture: Some(tex.rainbow.clone()),
            perceptual_roughness: 0.6,
            reflectance: 0.3,
            ..default()
        }),
        pad_frame: mats.add(StandardMaterial { base_color: Color::WHITE, metallic: 0.8, perceptual_roughness: 0.32, ..default() }),
        ramp: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.4, clearcoat: 0.5, clearcoat_perceptual_roughness: 0.15, ..default() }),
        plain: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() }),
        puddle: mats.add(StandardMaterial {
            base_color: Color::srgb(0.035, 0.04, 0.045),
            perceptual_roughness: 0.02,
            reflectance: 0.9,
            ..default()
        }),
        windows: tex.windows.clone(),
    };
    commands.insert_resource(surfaces);
    commands.insert_resource(tex);
    commands.insert_resource(kit);
}

/// What a track's scenery is built with. `jungle.rs` and `tbilisi.rs` take one of these; a new
/// track should too.
pub struct Build<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub meshes: &'a mut Assets<Mesh>,
    pub mats: &'a mut Assets<StandardMaterial>,
    pub kit: &'a Kit,
    pub surf: &'a SurfaceMats,
    pub signs: &'a super::signs::SignAtlas,
    pub tex: &'a Textures,
    pub track: &'a Track,
    pub raytracing: bool,
}

impl Build<'_, '_, '_> {
    /// A static mesh in `mat`. Nothing is spawned for an empty builder.
    pub fn add(&mut self, b: super::mesh::MeshBuilder, mat: &Handle<StandardMaterial>) {
        if !b.is_empty() {
            self.commands.spawn((Mesh3d(self.meshes.add(b.build())), MeshMaterial3d(mat.clone()), Scenery));
        }
    }

    /// Like `add`, for things that shouldn't cast shadows: flat ground, glass and anything that
    /// glows.
    pub fn add_flat(&mut self, b: super::mesh::MeshBuilder, mat: &Handle<StandardMaterial>) {
        if !b.is_empty() {
            self.commands.spawn((Mesh3d(self.meshes.add(b.build())), MeshMaterial3d(mat.clone()), NotShadowCaster, Scenery));
        }
    }

    /// A static mesh coloured by its vertex colours on top of `template` (see
    /// `MeshBuilder::tinted`). `shadows`: whether it casts them.
    pub fn add_tinted(&mut self, b: super::mesh::MeshBuilder, template: &Handle<StandardMaterial>, shadows: bool) {
        if b.is_empty() {
            return;
        }
        let mut e = self.commands.spawn((b.tinted(self.meshes, self.mats, template), Scenery));
        if !shadows {
            e.insert(NotShadowCaster);
        }
    }
}

/// A camera that renders the world (into `image`, if not the window): the main camera, or
/// another split-screen player's.
pub fn spawn_world_camera<'a>(commands: &'a mut Commands, gfx: Graphics, env: Handle<Image>, fog: DistanceFog, image: Option<Handle<Image>>) -> EntityCommands<'a> {
    let mut cam = commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), far: 4000.0, ..default() }),
        fog,
        bevy::post_process::bloom::Bloom::NATURAL,
        bevy::core_pipeline::tonemapping::Tonemapping::AcesFitted,
    ));
    camera_effects(&mut cam, gfx, env, gfx.dlss);
    if let Some(image) = image {
        cam.insert(RenderTarget::Image(image.into()));
    }
    cam
}

/// The sky is centred between the players' cameras (it's big enough for them all to be inside).
pub fn follow_sky(cams: Query<&GlobalTransform, With<PlayerCamera>>, mut sky: Query<&mut Transform, Or<(With<SkyDome>, With<scenery::SkyFollow>)>>) {
    if cams.is_empty() {
        return;
    }
    let centre = cams.iter().map(|c| c.translation()).sum::<Vec3>() / cams.iter().len() as f32;
    for mut tr in &mut sky {
        tr.translation = centre;
    }
}

/// Which track should be on screen: the race's, else the lobby's, else the menu selection.
fn wanted_track(session: Option<&SessionRes>, settings: &Settings) -> TrackId {
    match session {
        Some(s) => s.0.race.as_ref().map_or(s.0.track, |r| r.track.id),
        None => settings.track,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn sync_track_scene(
    mut commands: Commands,
    session: Option<Res<SessionRes>>,
    settings: Res<Settings>,
    mut track: ResMut<TrackRes>,
    mut built: Local<Option<TrackId>>,
    existing: Query<Entity, With<Scenery>>,
    kit: Res<Kit>,
    surfaces: Res<SurfaceMats>,
    signs: Res<super::signs::SignAtlas>,
    tex: Res<Textures>,
    gfx: Res<Graphics>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let want = wanted_track(session.as_deref(), &settings);
    if *built == Some(want) {
        return;
    }
    if track.0.id != want {
        track.0 = Track::new(want);
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    let track = &track.0;
    // Jungle and Tbilisi scenery fits any layout, so tracks made in the editor can have it too.
    if matches!(track.theme, Theme::Jungle | Theme::Tbilisi) {
        let mut build = Build { commands: &mut commands, meshes: &mut meshes, mats: &mut mats, kit: &kit, surf: &surfaces, signs: &signs, tex: &tex, track, raytracing: gfx.raytracing };
        match track.theme {
            Theme::Jungle => super::jungle::spawn(&mut build),
            _ => super::tbilisi::spawn(&mut build),
        }
        *built = Some(want);
        return;
    }
    if track.theme != Theme::Lagoon {
        spawn_track(&mut commands, &mut meshes, &mut mats, &surfaces, &kit, &tex, track);
        scenery::spawn_common(&mut commands, &kit, &surfaces, &mut meshes, &mut mats, track);
    }
    match want {
        TrackId::Jungle | TrackId::Tbilisi => unreachable!("built above"),
        TrackId::Meadow => scenery::spawn_meadow(&mut commands, &kit, &mut meshes, &mut mats, track),
        TrackId::Circuit => scenery::spawn_circuit(&mut commands, &kit, &signs, &surfaces, &mut meshes, &mut mats, track),
        TrackId::Prism => scenery::spawn_prism(&mut commands, &kit, &signs, &surfaces, &mut meshes, &mut mats, track, gfx.raytracing),
        TrackId::Lagoon => super::lagoon::spawn_lagoon(&mut commands, &kit, &signs, &surfaces, &tex, &mut meshes, &mut mats, track, gfx.raytracing),
        TrackId::Custom(_) => match track.theme {
            Theme::Circuit => scenery::spawn_circuit_custom(&mut commands, &kit, &signs, &surfaces, &mut meshes, &mut mats, track),
            Theme::Prism => scenery::spawn_prism(&mut commands, &kit, &signs, &surfaces, &mut meshes, &mut mats, track, gfx.raytracing),
            Theme::Jungle | Theme::Tbilisi => unreachable!("built above"),
            Theme::Meadow | Theme::Lagoon => scenery::spawn_meadow(&mut commands, &kit, &mut meshes, &mut mats, track),
        },
    }
    *built = Some(want);
}
