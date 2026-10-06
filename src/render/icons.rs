//! Little offscreen studios that render each player's held item as a spinning 3D model for their
//! HUD.

use bevy::{camera::visibility::RenderLayers, camera::RenderTarget, prelude::*, render::render_resource::TextureFormat};

use super::gfx::NoRaytrace;
use super::models::{item_icon, Kit};
use crate::game::SessionRes;
use crate::session::MAX_LOCAL_PLAYERS;
use crate::sim::Item;

const LAYER: usize = 3;
/// Far below the world so the models never show up anywhere else.
const STUDIO: Vec3 = Vec3::new(0.0, -600.0, 0.0);
/// Space between the players' studios, so each camera only sees its own.
const STUDIO_GAP: f32 = 60.0;

/// The rendered item icon for each player.
#[derive(Resource)]
pub struct ItemIcon {
    pub images: Vec<Handle<Image>>,
}

#[derive(Component)]
pub struct IconModel(Item, usize);

/// Where a player's item models go.
#[derive(Component)]
pub struct IconRoot(usize);

#[derive(Component)]
pub struct IconCamera(usize);

pub fn setup_icons(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let images: Vec<_> = (0..MAX_LOCAL_PLAYERS).map(|p| spawn_studio(&mut commands, &mut images, p)).collect();
    commands.insert_resource(ItemIcon { images });
}

fn spawn_studio(commands: &mut Commands, images: &mut Assets<Image>, player: usize) -> Handle<Image> {
    let studio = STUDIO + Vec3::X * STUDIO_GAP * player as f32;
    let image = images.add(Image::new_target_texture(256, 256, TextureFormat::Rgba8UnormSrgb, None));
    commands.spawn((
        Camera3d::default(),
        Camera { order: -5 - player as isize, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        RenderTarget::Image(image.clone().into()),
        // Everything opaque renders deferred, so this little camera needs the deferred prepass too.
        Msaa::Off,
        bevy::core_pipeline::prepass::DepthPrepass,
        bevy::core_pipeline::prepass::DeferredPrepass,
        bevy::anti_alias::fxaa::Fxaa::default(),
        Transform::from_translation(studio + Vec3::new(0.0, 0.5, 2.9)).looking_at(studio, Vec3::Y),
        RenderLayers::layer(LAYER),
        IconCamera(player),
    ));
    commands.spawn((PointLight { intensity: 400_000.0, range: 20.0, ..default() }, Transform::from_translation(studio + Vec3::new(2.0, 3.0, 3.0)), RenderLayers::layer(LAYER)));

    // The models themselves are made as they're first shown (see `animate_icons`).
    commands.spawn((Transform::from_translation(studio), Visibility::default(), IconRoot(player)));
    image
}

/// Render layers aren't inherited, so tag every part of a studio model once it exists.
pub fn tag_icon_layers(mut commands: Commands, new: Query<Entity, Added<IconModel>>, children: Query<&Children>) {
    for model in &new {
        for e in children.iter_descendants(model) {
            commands.entity(e).insert((RenderLayers::layer(LAYER), NoRaytrace));
        }
    }
}

/// Frames the icon camera keeps rendering after the slot empties, so the last item is cleared
/// from the image before the camera switches off.
const CLEAR_FRAMES: u32 = 3;

pub fn animate_icons(
    mut commands: Commands,
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    kit: Res<Kit>,
    roots: Query<(Entity, &IconRoot)>,
    mut built: Local<[u32; MAX_LOCAL_PLAYERS]>,
    mut models: Query<(&IconModel, &mut Transform, &mut Visibility)>,
    mut cams: Query<(&mut Camera, &IconCamera)>,
    mut empty_for: Local<[u32; MAX_LOCAL_PLAYERS]>,
) {
    let t = time.elapsed_secs();
    let race = session.as_ref().and_then(|s| s.0.race.as_ref());
    const CYCLE: [Item; 15] = Item::ALL;
    // What each player's slot shows, and whether it's spinning through the roulette.
    let slots: [(Item, bool); MAX_LOCAL_PLAYERS] = std::array::from_fn(|p| match race.and_then(|r| r.locals.get(p).map(|&i| &r.world.karts[i])) {
        Some(k) if k.roulette > 0.0 => (CYCLE[(t * 14.0 + p as f32 * 1.3) as usize % CYCLE.len()], true),
        Some(k) => (k.item, false),
        None => (Item::None, false),
    });
    // Each player's model of an item is built the first time their slot shows it: most of the
    // four studios' sixty models are never wanted.
    for (root, IconRoot(p)) in &roots {
        let item = slots[*p].0;
        let bit = 1u32 << item as u32;
        if item != Item::None && built[*p] & bit == 0 {
            built[*p] |= bit;
            commands.entity(root).with_children(|r| {
                r.spawn((Transform::default(), Visibility::Hidden, IconModel(item, *p))).with_children(|b| item_icon(b, &kit, item));
            });
        }
    }
    // Only render while there's something to show: each studio is a whole extra 3D view.
    for (mut cam, IconCamera(p)) in &mut cams {
        let n = &mut empty_for[*p];
        *n = if slots[*p].0 == Item::None { n.saturating_add(1) } else { 0 };
        let active = *n <= CLEAR_FRAMES;
        if cam.is_active != active {
            cam.is_active = active;
        }
    }
    for (m, mut tr, mut vis) in &mut models {
        let (show, rolling) = slots[m.1];
        *vis = if m.0 == show { Visibility::Inherited } else { Visibility::Hidden };
        let pop = if rolling { 0.85 } else { 1.0 + (t * 3.0).sin() * 0.04 };
        tr.rotation = Quat::from_rotation_y(t * 1.8) * Quat::from_rotation_x(0.35);
        tr.scale = Vec3::splat(pop);
    }
}
