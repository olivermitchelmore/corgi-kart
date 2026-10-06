//! Split screen: each local player's camera renders into an image of its own, and a 2D camera
//! lays those out on the window, with the HUDs, the lines between the views and the results
//! board on top. The same machinery renders a single player's view at a lower resolution than
//! the window (`Graphics::render_res`): the world goes into a smaller image, which is stretched
//! over the window while the HUD stays sharp.
//!
//! Two players stack top and bottom; three or four get a quarter each. (Bevy's
//! camera viewports would avoid the extra images, but TAA, DLSS and Solari all misplace a view
//! that doesn't start at the texture's corner.)

use bevy::{
    camera::{visibility::RenderLayers, RenderTarget},
    prelude::*,
    render::render_resource::{Extent3d, TextureFormat, TextureUsages},
};

use super::gfx::{Graphics, RenderRes};
use super::scene::spawn_world_camera;
use crate::game::{AppState, RaceEntity};

/// The camera following local player `0`, `1`, ... The main camera is player 1's.
#[derive(Component)]
pub struct PlayerCamera(pub usize);

/// Puts the players' views on screen, and draws everything over them.
#[derive(Component)]
pub struct OverlayCamera;

/// While a split-screen race runs.
#[derive(Resource)]
pub struct SplitScreen {
    /// Where the main camera rendered before it became player 1's view.
    main_target: RenderTarget,
    /// What each player sees.
    views: Vec<Handle<Image>>,
}

/// HUD layouts are made for a 720-pixel-tall screen; a player's view can shrink them down to this
/// height before they start to shrink with it (a scale of 0.6 at most).
const HUD_MIN_HEIGHT: f32 = 600.0;

/// The main camera and what it renders to.
pub type MainView<'a> = (Entity, &'a Camera, Option<&'a RenderTarget>, &'a DistanceFog, &'a GeneratedEnvironmentMapLight);

/// Sets up the players' cameras: alone, the main camera; together, the main camera and one like it
/// for each other player, all rendering to images that an overlay camera puts on screen. Returns
/// the camera for the HUDs and everything the players share.
pub fn spawn_views(commands: &mut Commands, images: &mut Assets<Image>, players: usize, gfx: Graphics, main: MainView, split: Option<&SplitScreen>) -> Entity {
    let (main_cam, camera, target, fog, probe) = main;
    if players < 2 && gfx.render_res == RenderRes::Native {
        return main_cam;
    }
    // A new race in the same session keeps the views it had.
    let (main_target, views) = match split {
        Some(s) => (s.main_target.clone(), s.views.clone()),
        None => {
            let size = gfx.render_res.scaled(camera.physical_target_size().unwrap_or(UVec2::new(1920, 1080)));
            let views = (0..players).map(|p| images.add(view_image(view_rect(p, players, size).1))).collect();
            (target.cloned().unwrap_or_default(), views)
        }
    };
    commands.entity(main_cam).insert(RenderTarget::Image(views[0].clone().into()));
    for (p, view) in views.iter().enumerate().skip(1) {
        let mut cam = spawn_world_camera(commands, gfx, probe.environment_map.clone(), fog.clone(), Some(view.clone()));
        cam.insert((
            Camera { order: p as isize, ..default() },
            GeneratedEnvironmentMapLight { environment_map: probe.environment_map.clone(), intensity: probe.intensity, ..default() },
            PlayerCamera(p),
            RaceEntity,
            DespawnOnExit(AppState::Race),
        ));
    }
    // UI only: no render layers, or it would test every mesh in the world against its frustum.
    let ui = commands
        .spawn((Camera2d, Camera { order: 10, ..default() }, main_target.clone(), RenderLayers::none(), OverlayCamera, RaceEntity, DespawnOnExit(AppState::Race)))
        .id();
    commands
        .spawn((
            Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
            BackgroundColor(Color::srgb(0.04, 0.05, 0.11)),
            GlobalZIndex(-1),
            UiTargetCamera(ui),
            RaceEntity,
            DespawnOnExit(AppState::Race),
        ))
        .with_children(|r| {
            for (p, view) in views.iter().enumerate() {
                r.spawn((ImageNode::new(view.clone()), place(view_percent(p, players))));
            }
        });
    commands.insert_resource(SplitScreen { main_target, views });
    ui
}

fn view_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x.max(1), size.y.max(1), TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    image
}

/// Hands the main camera back its own target when the race ends.
pub fn end_split(mut commands: Commands, split: Option<Res<SplitScreen>>, main: Query<Entity, With<super::scene::MainCamera>>) {
    let Some(split) = split else { return };
    if let Ok(main) = main.single() {
        commands.entity(main).insert(split.main_target.clone());
    }
    commands.remove_resource::<SplitScreen>();
}

/// Player `p`'s slice of a `size` target shared by `players`.
pub fn view_rect(p: usize, players: usize, size: UVec2) -> (UVec2, UVec2) {
    let half = size / 2;
    match players {
        0 | 1 => (UVec2::ZERO, size),
        2 if p == 0 => (UVec2::ZERO, UVec2::new(size.x, half.y)),
        2 => (UVec2::new(0, half.y), UVec2::new(size.x, size.y - half.y)),
        _ => {
            let (col, row) = ((p % 2) as u32, (p / 2) as u32);
            let pos = UVec2::new(col * half.x, row * half.y);
            let end = UVec2::new(if col == 0 { half.x } else { size.x }, if row == 0 { half.y } else { size.y });
            (pos, end - pos)
        }
    }
}

/// Where player `p`'s view is on screen, in percent.
pub fn view_percent(p: usize, players: usize) -> Rect {
    let (pos, size) = view_rect(p, players, UVec2::splat(10_000));
    Rect::from_corners(pos.as_vec2() / 100.0, (pos + size).as_vec2() / 100.0)
}

/// A node covering `rect` (in percent) of its parent.
pub fn place(rect: Rect) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: percent(rect.min.x),
        top: percent(rect.min.y),
        width: percent(rect.width()),
        height: percent(rect.height()),
        ..default()
    }
}

/// Keeps the players' views the size of their part of the window, and shrinks the HUDs to suit.
pub fn layout_views(
    split: Option<Res<SplitScreen>>,
    gfx: Res<Graphics>,
    overlay: Query<&Camera, With<OverlayCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut ui_scale: ResMut<UiScale>,
) {
    let mut scale = 1.0;
    if let (Some(split), Ok(cam)) = (split, overlay.single()) {
        let (Some(size), Some(factor)) = (cam.physical_target_size(), cam.target_scaling_factor()) else { return };
        let players = split.views.len();
        for (p, view) in split.views.iter().enumerate() {
            let want = view_rect(p, players, gfx.render_res.scaled(size)).1.max(UVec2::ONE);
            if images.get(view).is_some_and(|i| i.size() != want)
                && let Some(mut image) = images.get_mut(view)
            {
                image.resize(Extent3d { width: want.x, height: want.y, depth_or_array_layers: 1 });
            }
        }
        if players > 1 {
            scale = (size.y as f32 / 2.0 / factor / HUD_MIN_HEIGHT).clamp(0.6, 1.0);
        }
    }
    if ui_scale.0 != scale {
        ui_scale.0 = scale;
    }
}

/// Thin dark lines between the players' views.
pub fn spawn_dividers(ui: &mut ChildSpawnerCommands, players: usize) {
    let line = Color::srgb(0.02, 0.02, 0.05);
    let bar = |ui: &mut ChildSpawnerCommands, node: Node| {
        ui.spawn((Node { position_type: PositionType::Absolute, ..node }, BackgroundColor(line)));
    };
    bar(ui, Node { left: px(0), width: percent(100), top: percent(50), height: px(4), margin: UiRect::top(px(-2)), ..default() });
    if players > 2 {
        bar(ui, Node { top: px(0), height: percent(100), left: percent(50), width: px(4), margin: UiRect::left(px(-2)), ..default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The views tile the window exactly, whatever its size.
    #[test]
    fn views_cover_the_window() {
        for size in [UVec2::new(1920, 1080), UVec2::new(2542, 1525), UVec2::new(3, 5)] {
            for players in 1..=4 {
                let area: u32 = (0..players).map(|p| view_rect(p, players, size).1).map(|s| s.x * s.y).sum();
                let covered = if players == 3 { size.x * size.y - (size.x - size.x / 2) * (size.y - size.y / 2) } else { size.x * size.y };
                assert_eq!(area, covered, "{players} players at {size}");
                for p in 0..players {
                    let (pos, s) = view_rect(p, players, size);
                    assert!((pos + s).cmple(size).all());
                }
            }
        }
    }
}
