//! The on-screen frame rate counter (F3, or the menu, turns it on and off).

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::text::FontSize;

use super::scene::MainCamera;
use super::split::OverlayCamera;
use crate::game::{Cli, Settings};

#[derive(Component)]
pub struct FpsText;

pub fn setup_fps(mut commands: Commands) {
    // Bottom left, under the minimap, where nothing else goes.
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.85, 0.9, 0.85)),
        // The default shadow's offset doubles up text this small.
        TextShadow { offset: Vec2::splat(1.0), ..default() },
        Node { position_type: PositionType::Absolute, left: px(18), bottom: px(1), ..default() },
        GlobalZIndex(100),
        Visibility::Hidden,
        FpsText,
    ));
}

pub fn toggle_fps(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<Settings>) {
    if keys.just_pressed(KeyCode::F3) {
        settings.fps = !settings.fps;
        settings.save();
    }
}

#[allow(clippy::type_complexity)]
pub fn update_fps(
    mut commands: Commands,
    time: Res<Time<Real>>,
    settings: Res<Settings>,
    cli: Res<Cli>,
    diagnostics: Res<DiagnosticsStore>,
    overlay: Query<Entity, With<OverlayCamera>>,
    main: Query<Entity, With<MainCamera>>,
    mut text: Query<(Entity, &mut Text, &mut Visibility, Option<&UiTargetCamera>), With<FpsText>>,
    mut since: Local<f32>,
) {
    let Ok((e, mut text, mut vis, target)) = text.single_mut() else { return };
    // Headless screenshots and benchmarks leave it off unless asked for.
    let show = settings.fps && (!cli.headless || cli.fps);
    vis.set_if_neq(if show { Visibility::Visible } else { Visibility::Hidden });
    if !show {
        return;
    }
    // Draw over the whole window: in split screen that's the overlay, not player 1's view.
    if let Some(cam) = overlay.iter().next().or_else(|| main.iter().next())
        && target.map(|t| t.0) != Some(cam)
    {
        commands.entity(e).insert(UiTargetCamera(cam));
    }
    *since += time.delta_secs();
    if *since < 0.25 && !text.0.is_empty() {
        return;
    }
    *since = 0.0;
    let smoothed = |path| diagnostics.get(path).and_then(|d| d.smoothed());
    if let (Some(fps), Some(ms)) = (smoothed(&FrameTimeDiagnosticsPlugin::FPS), smoothed(&FrameTimeDiagnosticsPlugin::FRAME_TIME)) {
        text.0 = format!("{fps:.0} fps  {ms:.1} ms");
    }
}
