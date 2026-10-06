//! Graphics quality: the default deferred renderer with screen-space reflections, SSAO,
//! contact shadows and TAA, or (with `--raytracing` on a capable GPU) Bevy's Solari hardware
//! raytraced lighting. Also builds the sky cubemap used for reflections and ambient light.

use std::collections::HashMap;

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    asset::RenderAssetUsages,
    camera::CameraMainTextureUsages,
    core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass},
    light::GeneratedEnvironmentMapLight,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceReflections},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension},
    solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins},
};

use super::look::Look;
use super::scenery;

/// Chosen at startup; switching needs a restart because the GPU device is created with it.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Graphics {
    pub raytracing: bool,
    /// DLSS Ray Reconstruction is denoising the raytraced image (needs the `dlss` build).
    pub dlss: bool,
    /// How far below the window's resolution DLSS renders before upscaling.
    pub dlss_quality: DlssQuality,
    /// The height the world is rendered at, before it's stretched over the window.
    pub render_res: RenderRes,
}

/// How many pixels the world is rendered with. Cost per frame follows the pixel count, so a
/// lower setting is a big speed-up on a big window, at the price of a softer picture. The HUD is
/// always drawn at the window's own resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderRes {
    #[default]
    Native,
    P1080,
    P720,
}

impl RenderRes {
    pub const ALL: [RenderRes; 3] = [Self::Native, Self::P1080, Self::P720];

    pub fn key(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == s)
    }

    /// The tallest the rendered picture gets, if it's capped.
    pub fn max_height(self) -> Option<u32> {
        match self {
            Self::Native => None,
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
        }
    }

    /// The size to render a `window` at: the window itself, or shrunk to the height cap.
    pub fn scaled(self, window: UVec2) -> UVec2 {
        match self.max_height() {
            Some(h) if window.y > h => UVec2::new((window.x as f32 * h as f32 / window.y as f32).round().max(1.0) as u32, h),
            _ => window,
        }
    }

    pub fn cycle(self, step: i32) -> Self {
        let i = Self::ALL.iter().position(|&r| r == self).unwrap_or(0) as i32;
        Self::ALL[(i + step).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

/// DLSS render scale: 67%, 58% or 50% of the window's width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DlssQuality {
    #[default]
    Quality,
    Balanced,
    Performance,
}

impl DlssQuality {
    pub const ALL: [DlssQuality; 3] = [Self::Quality, Self::Balanced, Self::Performance];

    pub fn key(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    pub fn cycle(self, step: i32) -> Self {
        let i = Self::ALL.iter().position(|&r| r == self).unwrap_or(0) as i32;
        Self::ALL[(i + step).rem_euclid(Self::ALL.len() as i32) as usize]
    }

    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "quality" => Some(Self::Quality),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            _ => None,
        }
    }
}

/// Opt an entity out of the raytracing scene (particles, sky, clouds...).
#[derive(Component)]
pub struct NoRaytrace;

#[derive(Component)]
pub struct RtChecked;

/// Put this entity's raytracing copy this far below it (in its own space), for big floors and
/// water seen from far away. Solari rebuilds each pixel's position from the depth buffer, and a
/// kilometre out that's off by more than its 1mm ray offset, so rays start just under the
/// surface and hit its underside: black blotches in reflections (crawling as the camera moves)
/// and dappled shadow on distant grass.
#[derive(Component)]
pub struct RaytraceSink(pub f32);

/// Can this machine run Solari? Checks every adapter for the required wgpu features.
pub fn probe_raytracing() -> Result<String, String> {
    let required = SolariPlugins::required_wgpu_features();
    let instance = wgpu::Instance::default();
    let adapters = bevy::tasks::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY));
    let mut names = vec![];
    for a in adapters {
        let info = a.get_info();
        if a.features().contains(required) {
            return Ok(info.name);
        }
        names.push(info.name);
    }
    Err(format!("no GPU with hardware ray tracing support found (checked: {})", names.join(", ")))
}

/// Camera components for the chosen mode.
pub fn camera_effects(commands: &mut EntityCommands, gfx: Graphics, env: Handle<Image>, dlss: bool) {
    if gfx.raytracing && dlss {
        // Ray Reconstruction denoises Solari's output (and anti-aliases), replacing TAA.
        #[cfg(feature = "dlss")]
        commands.insert(bevy::anti_alias::dlss::Dlss::<bevy::anti_alias::dlss::DlssRayReconstructionFeature> {
            perf_quality_mode: match gfx.dlss_quality {
                DlssQuality::Quality => bevy::anti_alias::dlss::DlssPerfQualityMode::Quality,
                DlssQuality::Balanced => bevy::anti_alias::dlss::DlssPerfQualityMode::Balanced,
                DlssQuality::Performance => bevy::anti_alias::dlss::DlssPerfQualityMode::Performance,
            },
            reset: false,
            _phantom_data: Default::default(),
        });
    } else {
        commands.insert(TemporalAntiAliasing::default());
    }
    commands.insert((
        Msaa::Off,
        DepthPrepass,
        MotionVectorPrepass,
        GeneratedEnvironmentMapLight { environment_map: env, intensity: 900.0, ..default() },
    ));
    if gfx.raytracing {
        commands.insert((SolariLighting::default(), CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING)));
    } else {
        commands.insert((
            ScreenSpaceReflections {
                min_perceptual_roughness: 0.0..0.02,
                max_perceptual_roughness: 0.4..0.55,
                thickness: 0.4,
                linear_steps: 24,
                linear_march_exponent: 1.2,
                bisection_steps: 6,
                use_secant: true,
                ..default()
            },
            ScreenSpaceAmbientOcclusion::default(),
            ContactShadows { linear_steps: 12, thickness: 0.25, length: 0.6 },
        ));
    }
}

/// Scenery that moves (animated in `ambient.rs`). Parts under these keep their own raytracing
/// instances; everything else in the scenery is static and gets merged (see `RtMerged`).
type Animated = Or<(
    With<scenery::Spectator>,
    With<scenery::StatueHead>,
    With<scenery::StatueTail>,
    With<scenery::FerrisWheel>,
    With<scenery::Gondola>,
    With<scenery::Drifter>,
    With<scenery::WavingFlag>,
    With<scenery::StartLight>,
    With<scenery::Bob>,
    With<scenery::SkyFollow>,
    With<scenery::Spin>,
    With<scenery::Floating>,
    With<scenery::Shuttle>,
    With<NoRtMerge>,
)>;

/// Keep this entity (and everything under it) out of the merged raytracing meshes.
#[derive(Component)]
pub struct NoRtMerge;

/// Meshes with more vertices than this keep their own instance: merging copies the geometry
/// once per instance, and a big mesh is rarely repeated much anyway.
const MERGE_MAX_VERTICES: usize = 20_000;

/// Give every opaque lit mesh a raytracing proxy: same geometry, but with exactly the vertex
/// layout Solari wants (no vertex colours, u32 indices, generated tangents).
///
/// Static scenery doesn't get its own instance: its parts are baked into one raytracing mesh per
/// material instead (`RtMerged`, `rebuild_merged_raytracing`). Solari redoes its per-instance CPU
/// work every frame (extraction, scene bindings, the TLAS) at about 1µs an instance, and the
/// tracks have 1-4k instances, most of them trees, props and stands that never move.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn add_raytracing_meshes(
    mut commands: Commands,
    gfx: Res<Graphics>,
    new: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform, Option<&RaytraceSink>), (Without<RtChecked>, Without<NoRaytrace>)>,
    parents: Query<&ChildOf>,
    scenery: Query<(), With<super::scene::Scenery>>,
    animated: Query<(), Animated>,
    mut groups: ResMut<RtMergedGroups>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<Assets<StandardMaterial>>,
    mut cache: Local<HashMap<AssetId<Mesh>, Option<Handle<Mesh>>>>,
) {
    if !gfx.raytracing {
        return;
    }
    for (e, mesh, mat, at, sink) in &new {
        commands.entity(e).insert(RtChecked);
        let Some(m) = mats.get(&mat.0) else { continue };
        if m.unlit || !matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            continue;
        }
        // Emissive meshes stay separate: Solari picks lights by instance, so merging them would
        // change how it spreads its light samples (and so the noise).
        let root = parents.root_ancestor(e);
        let mergeable = sink.is_none()
            && m.emissive == LinearRgba::BLACK
            && scenery.contains(root)
            && !animated.contains(e)
            && !parents.iter_ancestors(e).any(|a| animated.contains(a))
            && meshes.get(&mesh.0).is_some_and(|src| src.count_vertices() <= MERGE_MAX_VERTICES);
        if mergeable {
            let group = mat.0.id();
            groups.members.insert(e, group);
            groups.groups.entry(group).or_insert_with(|| RtGroup { material: mat.0.clone(), proxy: None, dirty: false }).dirty = true;
            commands.entity(e).insert(RtMerged { group, mesh: mesh.0.id(), at: *at });
            continue;
        }
        let proxy = cache.entry(mesh.0.id()).or_insert_with(|| {
            let src = meshes.get(&mesh.0)?;
            let p = raytracing_proxy(src)?;
            Some(meshes.add(p))
        });
        let Some(p) = proxy else { continue };
        match sink {
            Some(s) => {
                commands.entity(e).with_child((RaytracingMesh3d(p.clone()), mat.clone(), Transform::from_xyz(0.0, -s.0, 0.0)));
            }
            None => {
                commands.entity(e).insert(RaytracingMesh3d(p.clone()));
            }
        }
    }
}

/// A static scenery part whose geometry is baked into its material's merged raytracing mesh
/// (as it was at `at`), rather than having its own raytracing instance.
#[derive(Component)]
pub struct RtMerged {
    group: AssetId<StandardMaterial>,
    mesh: AssetId<Mesh>,
    at: GlobalTransform,
}

/// The merged raytracing meshes, one per material, and which entities are in each.
#[derive(Resource, Default)]
pub struct RtMergedGroups {
    groups: HashMap<AssetId<StandardMaterial>, RtGroup>,
    members: HashMap<Entity, AssetId<StandardMaterial>>,
}

struct RtGroup {
    material: Handle<StandardMaterial>,
    /// The entity carrying the merged `RaytracingMesh3d`.
    proxy: Option<Entity>,
    dirty: bool,
}

/// Keep the merged raytracing meshes in step with the scenery: rebake a material's mesh when a
/// part joins or is despawned, and if a merged part ever moves or changes mesh or material,
/// give it back its own instance (so the raytraced scene can never go stale).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn rebuild_merged_raytracing(
    mut commands: Commands,
    gfx: Res<Graphics>,
    mut groups: ResMut<RtMergedGroups>,
    mut removed: RemovedComponents<RtMerged>,
    changed: Query<(Entity, &RtMerged, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform), Or<(Changed<GlobalTransform>, Changed<Mesh3d>, Changed<MeshMaterial3d<StandardMaterial>>)>>,
    members: Query<(&RtMerged, &Mesh3d, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sources: Local<HashMap<AssetId<Mesh>, Option<Mesh>>>,
) {
    if !gfx.raytracing {
        return;
    }
    let groups = &mut *groups;
    for e in removed.read() {
        if let Some(g) = groups.members.remove(&e).and_then(|g| groups.groups.get_mut(&g)) {
            g.dirty = true;
        }
    }
    for (e, merged, mesh, mat, at) in &changed {
        if *at == merged.at && mesh.0.id() == merged.mesh && mat.0.id() == merged.group {
            continue;
        }
        groups.members.remove(&e);
        if let Some(g) = groups.groups.get_mut(&merged.group) {
            g.dirty = true;
        }
        commands.entity(e).remove::<(RtMerged, RtChecked)>().insert(NoRtMerge);
    }
    if !groups.groups.values().any(|g| g.dirty) {
        return;
    }

    let mut parts: HashMap<AssetId<StandardMaterial>, Vec<(AssetId<Mesh>, GlobalTransform)>> = HashMap::new();
    for (&e, &g) in &groups.members {
        if !groups.groups[&g].dirty {
            continue;
        }
        if let Ok((merged, mesh, _)) = members.get(e) {
            parts.entry(g).or_default().push((mesh.0.id(), merged.at));
        }
    }
    groups.groups.retain(|g, group| {
        if !group.dirty {
            return true;
        }
        group.dirty = false;
        let list = parts.remove(g).unwrap_or_default();
        let merged = bake(&list, &meshes, &mut sources);
        match (merged, group.proxy) {
            (Some(m), Some(p)) => {
                commands.entity(p).insert(RaytracingMesh3d(meshes.add(m)));
            }
            (Some(m), None) => {
                group.proxy = Some(commands.spawn((RaytracingMesh3d(meshes.add(m)), MeshMaterial3d(group.material.clone()), Transform::default())).id());
            }
            (None, p) => {
                if let Some(p) = p {
                    commands.entity(p).despawn();
                }
                return false;
            }
        }
        true
    });
    // Only needed while baking.
    sources.clear();
}

/// One raytracing mesh holding every part, in world space. Solari transforms an instance's
/// normals and tangents by the plain model matrix (then normalises after interpolating), so
/// baking them the same way gives exactly the shading the separate instances had.
fn bake(parts: &[(AssetId<Mesh>, GlobalTransform)], meshes: &Assets<Mesh>, sources: &mut HashMap<AssetId<Mesh>, Option<Mesh>>) -> Option<Mesh> {
    let (mut pos, mut nrm, mut uv, mut tan, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for (mesh, at) in parts {
        let Some(src) = sources.entry(*mesh).or_insert_with(|| raytracing_proxy(meshes.get(*mesh)?)) else { continue };
        let (
            Some(VertexAttributeValues::Float32x3(p)),
            Some(VertexAttributeValues::Float32x3(n)),
            Some(VertexAttributeValues::Float32x2(t)),
            Some(VertexAttributeValues::Float32x4(g)),
            Some(Indices::U32(i)),
        ) = (
            src.attribute(Mesh::ATTRIBUTE_POSITION),
            src.attribute(Mesh::ATTRIBUTE_NORMAL),
            src.attribute(Mesh::ATTRIBUTE_UV_0),
            src.attribute(Mesh::ATTRIBUTE_TANGENT),
            src.indices(),
        )
        else {
            continue;
        };
        let affine = at.affine();
        let linear = affine.matrix3;
        let base = pos.len() as u32;
        pos.extend(p.iter().map(|&v| affine.transform_point3a(v.into()).to_array()));
        nrm.extend(n.iter().map(|&v| (linear * Vec3A::from(v)).to_array()));
        uv.extend_from_slice(t);
        tan.extend(g.iter().map(|&[x, y, z, w]| {
            let d = linear * Vec3A::new(x, y, z);
            [d.x, d.y, d.z, w]
        }));
        idx.extend(i.iter().map(|&k| k + base));
    }
    if idx.is_empty() {
        return None;
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tan)
        .with_inserted_indices(Indices::U32(idx));
    m.enable_raytracing = true;
    Some(m)
}

/// Working out tangents takes about a second per 100k triangles, and only a normal map uses them.
/// Meshes bigger than this (a forest's leaves, merged into one) get a made-up tangent instead:
/// anything that big with a normal map should bring its own tangents, as the water does.
const MAX_TANGENT_VERTICES: usize = 150_000;

pub fn raytracing_proxy(src: &Mesh) -> Option<Mesh> {
    if src.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let pos = src.attribute(Mesh::ATTRIBUTE_POSITION)?.clone();
    let nrm = src.attribute(Mesh::ATTRIBUTE_NORMAL)?.clone();
    let count = pos.len();
    let uv = src.attribute(Mesh::ATTRIBUTE_UV_0).cloned().unwrap_or(VertexAttributeValues::Float32x2(vec![[0.0, 0.0]; count]));
    let idx: Vec<u32> = match src.indices() {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&x| x as u32).collect(),
        None => (0..count as u32).collect(),
    };
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    if let Some(t) = src.attribute(Mesh::ATTRIBUTE_TANGENT) {
        m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t.clone());
    } else if count > MAX_TANGENT_VERTICES || m.generate_tangents().is_err() {
        m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; count]);
    }
    m.enable_raytracing = true;
    Some(m)
}

/// Radius of the raytraced sky, centred on the origin. Everything must fit well inside it
/// (Mirror Lagoon's open sea reaches ~7km out): a reflection from anything outside finds no sky
/// and comes out black. It's as big as Solari's 100km rays allow, so the sun's hole in it lines
/// up from anywhere on the track.
const SKY_LIGHT_RADIUS: f32 = 80_000.0;

/// Every track (and the land around it) lies within this distance of the origin.
const PLAY_RADIUS: f32 = 800.0;

/// Solari only lights with the sun and emissive surfaces, so give it a glowing sky: a dome
/// that exists only in the raytracing scene (the camera never rasterises it).
///
/// With `Look::sky_texture` its emissive texture is the same sky as the cubemap, so mirrors and
/// water reflect the real gradient and sunset glow. That dome is built around the sun's axis,
/// leaving an exact small cap open around the sun so it doesn't block the sun's shadow rays.
/// Otherwise it's two flat bands.
#[allow(clippy::too_many_arguments)]
pub fn spawn_sky_light(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    sun: Vec3,
    look: &Look,
    strength: f32,
) -> Vec<Entity> {
    if !look.sky_texture {
        return band_sky_light(commands, meshes, mats, sun, look.horizon.to_linear() * strength, look.zenith.to_linear() * strength);
    }
    const R: f32 = SKY_LIGHT_RADIUS;
    let (rings, segs) = (48usize, 64usize);
    // The cone Solari aims the sun's shadow rays into, widened for parallax (the dome is centred
    // on the origin, the track isn't a point) and a hair for the polygon's edges cutting inside
    // the circle. Mirror-smooth water and the Lagoon road reflect the hole as black (Solari
    // leaves their sun glint to a pass that can't see directional lights), so no wider.
    let hole = look.sun_size * 0.5 * 1.02 + (PLAY_RADIUS / R).asin() + 0.02f32.to_radians();
    let side = sun.any_orthonormal_vector();
    let up = sun.cross(side);
    // (u, v) in 0..1: around the sun, and from the edge of the hole to the point opposite it.
    let dir = |u: f32, v: f32| {
        let theta = hole + v * (std::f32::consts::PI - hole);
        let phi = u * std::f32::consts::TAU;
        sun * theta.cos() + (side * phi.cos() + up * phi.sin()) * theta.sin()
    };

    let mut b = super::mesh::MeshBuilder::default();
    for i in 0..rings {
        for j in 0..segs {
            let (u0, u1) = (j as f32 / segs as f32, (j + 1) as f32 / segs as f32);
            let (v0, v1) = (i as f32 / rings as f32, (i + 1) as f32 / rings as f32);
            let uv = [Vec2::new(u0, v0), Vec2::new(u1, v0), Vec2::new(u1, v1), Vec2::new(u0, v1)];
            let q = uv.map(|t| dir(t.x, t.y));
            // Only the sky: the world hides everything below the horizon.
            if q.iter().all(|p| p.y < -0.06) {
                continue;
            }
            // wound to face inwards
            b.quad_facing(q.map(|p| p * R), -(q[0] + q[2]), Color::WHITE, uv);
        }
    }

    const W: usize = 128;
    const H: usize = 96;
    let mut data = Vec::with_capacity(W * H * 8);
    for y in 0..H {
        for x in 0..W {
            let d = dir((x as f32 + 0.5) / W as f32, (y as f32 + 0.5) / H as f32).normalize();
            let c = sky_base(d, sun, look) * strength;
            for ch in [c.x, c.y, c.z, 1.0] {
                data.extend_from_slice(&f16_bits(ch).to_le_bytes());
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    let mat = sky_light_material(mats, LinearRgba::rgb(1200.0, 1200.0, 1200.0), Some(images.add(img)));
    let Some(proxy) = raytracing_proxy(&b.build()) else { return vec![] };
    vec![commands.spawn((RaytracingMesh3d(meshes.add(proxy)), MeshMaterial3d(mat), Transform::default())).id()]
}

/// The sky light's material: pure glow, and a black perfect mirror so it reflects nothing.
///
/// The mirror matters for speed, not looks. Solari skips direct-light sampling at perfect mirrors
/// and a black one ends the path, so no ray ever starts out on the sky. Otherwise every
/// reflection that reaches the sky fires a shadow ray from 80km away back across the world, and
/// at that distance rays are so imprecise that they wade through most of the scene's bounding
/// boxes: it made Solari's specular pass 10-40x slower (half the frame on most tracks).
fn sky_light_material(mats: &mut Assets<StandardMaterial>, emissive: LinearRgba, texture: Option<Handle<Image>>) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive,
        emissive_texture: texture,
        metallic: 1.0,
        perceptual_roughness: 0.0,
        cull_mode: None,
        ..default()
    })
}

/// Solari treats emissive as physical luminance and applies camera exposure (the standard
/// renderer shows it as-is), so scale our artistic emissive values up to match. This makes neon,
/// windows and glowing tiles real light sources in the raytraced scene.
/// (1 / exposure at the default EV100 of 9.7 is ~1000; a quarter of that reads like the
/// standard renderer once the extra bounce light is added.)
pub const EMISSIVE_TO_NITS: f32 = 250.0;

pub fn scale_emissive_for_raytracing(
    gfx: Res<Graphics>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut done: Local<std::collections::HashSet<AssetId<StandardMaterial>>>,
) {
    if !gfx.raytracing {
        events.clear();
        return;
    }
    let added: Vec<_> = events.read().filter_map(|e| if let AssetEvent::Added { id } = e { Some(*id) } else { None }).collect();
    for id in added {
        if !done.insert(id) {
            continue;
        }
        let Some(mut m) = mats.get_mut(id) else { continue };
        let e = m.emissive;
        let peak = e.red.max(e.green).max(e.blue);
        // Already physical (the sky light), unlit, or forward-rendered: leave alone.
        if peak <= 0.0 || peak > 100.0 || m.unlit || !matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            continue;
        }
        m.emissive = e * EMISSIVE_TO_NITS;
    }
}

// ----- Sky cubemap --------------------------------------------------------------------------

fn f16_bits(v: f32) -> u16 {
    // Positive, finite values only (it's a sky).
    let v = v.clamp(0.0, 65000.0);
    if v < 6.1e-5 {
        return 0;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = (bits >> 13) & 0x3ff;
    ((exp as u32) << 10 | mant) as u16
}

/// Sky colour in direction `d` (linear HDR): the same gradient as the sky dome, a warm sun
/// glow, and the ground below the horizon.
pub fn sky_radiance(d: Vec3, sun: Vec3, look: &Look) -> Vec3 {
    let s = d.dot(sun).max(0.0);
    let disc = if look.night { s.powf(1500.0) * 8.0 * Vec3::new(0.7, 0.8, 1.0) } else { s.powf(900.0) * 60.0 * Vec3::new(1.0, 0.85, 0.6) };
    sky_base(d, sun, look) + disc
}

/// The sky without the sun's disc.
fn sky_base(d: Vec3, sun: Vec3, look: &Look) -> Vec3 {
    let v = |c: LinearRgba| Vec3::new(c.red, c.green, c.blue);
    let (h, z) = (v(look.horizon.to_linear()), v(look.zenith.to_linear()));
    let c = if d.y >= 0.0 { h.lerp(z, d.y.powf(0.55)) } else { h.lerp(v(look.ground), (-d.y * 5.0).min(1.0)) };
    let s = d.dot(sun).max(0.0);
    if look.night {
        return c + Vec3::new(0.7, 0.8, 1.0) * s.powf(40.0) * 0.08;
    }
    c + Vec3::new(1.0, 0.85, 0.6) * s.powf(24.0) * 0.5 + sunset_glow(d, sun, look)
}

/// Sunset colour on top of the gradient: a wide glow round the sun that spreads along the
/// horizon, and a pink band low in the sky opposite it. Nothing for looks without them.
pub fn sunset_glow(d: Vec3, sun: Vec3, look: &Look) -> Vec3 {
    let v = |c: LinearRgba| Vec3::new(c.red, c.green, c.blue);
    let flat = |p: Vec3| Vec3::new(p.x, 0.0, p.z).normalize_or_zero();
    let toward = flat(d).dot(flat(sun));
    let s = d.dot(sun).max(0.0);
    let up = d.y.max(0.0);
    let halo = s.powf(5.0) * 0.6 + s.powf(60.0) * 1.5 + toward.max(0.0).powf(3.0) * (-up * 9.0).exp() * 0.7;
    let belt = (-toward).max(0.0).powf(0.7) * (-(d.y - 0.07).abs() * 10.0).exp();
    v(look.halo) * halo + v(look.belt) * belt
}

pub fn sky_cubemap(images: &mut Assets<Image>, sun: Vec3, look: &Look) -> Handle<Image> {
    const N: usize = 128;
    let mut data = Vec::with_capacity(N * N * 6 * 8);
    for face in 0..6 {
        for y in 0..N {
            for x in 0..N {
                let u = (x as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                let d = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let c = sky_radiance(d, sun, look);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&f16_bits(ch).to_le_bytes());
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    images.add(img)
}

/// The simpler sky light: two flat bands (paler at the horizon) with a wide hole round the sun.
fn band_sky_light(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, sun: Vec3, horizon: LinearRgba, zenith: LinearRgba) -> Vec<Entity> {
    let mut out = vec![];
    const R: f32 = SKY_LIGHT_RADIUS;
    let (lat_n, lon_n) = (12, 32);
    let hole = 22f32.to_radians().cos();
    // Two bands so the horizon glows paler than the zenith.
    for (band, (y0, y1)) in [(0.0f32, 0.35f32), (0.35, 1.0)].into_iter().enumerate() {
        let mut b = super::mesh::MeshBuilder::default();
        for i in 0..lat_n {
            let a0 = (i as f32 / lat_n as f32 * std::f32::consts::FRAC_PI_2).sin();
            let a1 = ((i + 1) as f32 / lat_n as f32 * std::f32::consts::FRAC_PI_2).sin();
            // By the row's middle, so every row lands in exactly one band (the split isn't
            // on a row boundary, and a missing row reflects as a black ring on shiny floors).
            let mid_y = (a0 + a1) * 0.5;
            if mid_y < y0 || mid_y >= y1 {
                continue;
            }
            for j in 0..lon_n {
                let p = |y: f32, k: usize| {
                    let r = (1.0 - y * y).sqrt();
                    let t = k as f32 / lon_n as f32 * std::f32::consts::TAU;
                    Vec3::new(r * t.cos(), y, r * t.sin())
                };
                let q = [p(a0, j), p(a1, j), p(a1, j + 1), p(a0, j + 1)];
                let mid = (q[0] + q[1] + q[2] + q[3]).normalize();
                if mid.dot(sun) > hole {
                    continue;
                }
                // wound to face inwards
                b.quad_auto([q[3] * R, q[2] * R, q[1] * R, q[0] * R], Color::WHITE, [Vec2::ZERO; 4]);
            }
        }
        let c = if band == 0 { horizon } else { zenith.mix(&horizon, 0.3) };
        let mat = sky_light_material(mats, LinearRgba::rgb(c.red, c.green, c.blue) * 1200.0, None);
        let Some(proxy) = raytracing_proxy(&b.build()) else { continue };
        out.push(commands.spawn((RaytracingMesh3d(meshes.add(proxy)), MeshMaterial3d(mat), Transform::default())).id());
    }
    out
}
