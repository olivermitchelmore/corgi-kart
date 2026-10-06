//! Trackside advertising boards. Bevy UI renders the banner text into an atlas texture once at
//! startup (no font files needed), and the scenery maps quads onto regions of it. The atlas
//! camera switches off once the atlas is drawn (`freeze_signs`), so it costs nothing after.

use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
    text::FontSize,
};

const COLS: usize = 2;
const ROWS: usize = 9;
const CELL_W: u32 = 512;
const CELL_H: u32 = 128;

pub const SIGN_TITLE: usize = 0;
pub const SIGN_PRISM: usize = 10;
pub const SIGN_LAGOON: usize = 12;
/// Signs before this one are sponsors and slogans that suit any track.
pub const SIGN_ADS: usize = 14;
pub const SIGN_JUNGLE: usize = 14;
pub const SIGN_TBILISI: usize = 16;
pub const SIGN_COUNT: usize = COLS * ROWS;

/// (text, background, text colour)
const SIGNS: [(&str, Color, Color); SIGN_COUNT] = [
    ("CORGI CIRCUIT", Color::srgb(0.95, 0.52, 0.1), Color::WHITE),
    ("WOOF!", Color::srgb(0.08, 0.14, 0.4), Color::srgb(1.0, 0.85, 0.2)),
    ("GOOD BOY OIL", Color::srgb(0.82, 0.1, 0.12), Color::WHITE),
    ("BARK & CO.", Color::srgb(0.1, 0.5, 0.25), Color::WHITE),
    ("TREATS R US", Color::srgb(1.0, 0.82, 0.15), Color::srgb(0.3, 0.15, 0.05)),
    ("ZOOMIES", Color::srgb(0.45, 0.15, 0.7), Color::WHITE),
    ("SPLOOT TYRES", Color::srgb(0.06, 0.06, 0.07), Color::srgb(1.0, 0.55, 0.1)),
    ("FLOOF FM 99.1", Color::srgb(0.2, 0.6, 0.95), Color::WHITE),
    ("GO CORGIS!", Color::WHITE, Color::srgb(0.9, 0.35, 0.05)),
    ("LOAF MOTORS", Color::srgb(0.95, 0.9, 0.78), Color::srgb(0.55, 0.28, 0.1)),
    ("PRISM PARK", Color::srgb(0.12, 0.04, 0.22), Color::srgb(0.4, 1.0, 1.0)),
    ("GLOW DOGS", Color::srgb(0.9, 0.2, 0.6), Color::WHITE),
    ("MIRROR LAGOON", Color::srgb(0.06, 0.1, 0.24), Color::srgb(1.0, 0.8, 0.35)),
    ("SUNDOWNER", Color::srgb(0.95, 0.42, 0.35), Color::WHITE),
    ("JADE JUNGLE", Color::srgb(0.05, 0.22, 0.14), Color::srgb(1.0, 0.82, 0.3)),
    ("BANANA BOOST", Color::srgb(1.0, 0.86, 0.2), Color::srgb(0.2, 0.35, 0.1)),
    ("TBILISI TWILIGHT", Color::srgb(0.1, 0.09, 0.26), Color::srgb(1.0, 0.72, 0.4)),
    ("KHINKALI HOUSE", Color::srgb(0.55, 0.08, 0.12), Color::srgb(1.0, 0.93, 0.8)),
];

#[derive(Resource)]
pub struct SignAtlas {
    pub material: Handle<StandardMaterial>,
    image: Handle<Image>,
}

#[derive(Component)]
pub struct SignCamera;

/// UVs for sign `i`, in quad order bottom-left, bottom-right, top-right, top-left.
pub fn sign_uv(i: usize) -> [Vec2; 4] {
    let (col, row) = ((i % COLS) as f32, (i / COLS) as f32);
    let (du, dv) = (1.0 / COLS as f32, 1.0 / ROWS as f32);
    // inset half a texel to avoid bleeding from neighbours
    let e = 1.0 / (CELL_W * COLS as u32) as f32;
    let (u0, u1, v0, v1) = (col * du + e, (col + 1.0) * du - e, row * dv + e, (row + 1.0) * dv - e);
    [Vec2::new(u0, v1), Vec2::new(u1, v1), Vec2::new(u1, v0), Vec2::new(u0, v0)]
}

pub fn setup_signs(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let (w, h) = (CELL_W * COLS as u32, CELL_H * ROWS as u32);
    let mut atlas = Image::new_target_texture(w, h, TextureFormat::Rgba8UnormSrgb, None);
    // `freeze_signs` reads it back to see when it's finished.
    atlas.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = images.add(atlas);
    let cam = commands
        .spawn((
            Camera2d,
            Camera { order: -10, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
            RenderTarget::Image(image.clone().into()),
            SignCamera,
        ))
        .id();
    commands
        .spawn((
            Node { width: px(w), height: px(h), position_type: PositionType::Absolute, ..default() },
            UiTargetCamera(cam),
        ))
        .with_children(|root| {
            for (i, (text, bg, fg)) in SIGNS.iter().enumerate() {
                let (col, row) = (i % COLS, i / COLS);
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(col as u32 * CELL_W),
                        top: px(row as u32 * CELL_H),
                        width: px(CELL_W),
                        height: px(CELL_H),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(*bg),
                    BorderColor::all(fg.with_alpha(0.9)),
                ))
                .with_children(|c| {
                    let size = if text.len() > 14 { 48.0 } else if text.len() > 11 { 58.0 } else { 70.0 };
                    c.spawn((Text::new(*text), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(*fg)));
                });
            }
        });
    let material = mats.add(StandardMaterial {
        base_color_texture: Some(image.clone()),
        perceptual_roughness: 0.6,
        emissive: LinearRgba::rgb(0.08, 0.08, 0.08),
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(SignAtlas { material, image });
}

/// Where `freeze_signs` has got to: the last capture's pixels, and whether one is in flight.
#[derive(Default)]
pub struct SignCapture {
    last: Option<Vec<u8>>,
    pending: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>>,
    done: bool,
}

/// The atlas never changes, so stop rendering it once it's drawn: its camera would otherwise
/// redo the UI layout, extraction and render pass every frame. Shaders and glyphs can take a
/// while to be ready, so rather than guess, read the atlas back every half second and switch
/// the camera off when two captures in a row match and aren't blank. The texture keeps its
/// contents while the camera is off.
pub fn freeze_signs(
    mut commands: Commands,
    time: Res<Time<Real>>,
    atlas: Res<SignAtlas>,
    mut cams: Query<&mut Camera, With<SignCamera>>,
    mut state: Local<SignCapture>,
    mut next: Local<f32>,
) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    if state.done {
        return;
    }
    if let Some(slot) = state.pending.clone() {
        let Some(pixels) = slot.lock().unwrap().take() else { return };
        state.pending = None;
        let blank = pixels.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]);
        if !blank && state.last.as_ref() == Some(&pixels) {
            for mut cam in &mut cams {
                cam.is_active = false;
            }
            state.done = true;
            return;
        }
        state.last = Some(pixels);
    }
    let t = time.elapsed_secs();
    if state.pending.is_some() || t < *next {
        return;
    }
    *next = t + 0.5;
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    state.pending = Some(slot.clone());
    commands.spawn(Screenshot::image(atlas.image.clone())).observe(move |shot: On<ScreenshotCaptured>| {
        *slot.lock().unwrap() = Some(shot.image.data.clone().unwrap_or_default());
    });
}
