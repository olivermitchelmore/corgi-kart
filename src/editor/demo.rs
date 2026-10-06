//! `editor --demo`: the editor builds a track by itself, for the trailer. A pretend mouse draws a
//! figure of eight, bends a corner, lifts one road over the other into a bridge, drops in a ramp
//! and boost pads, and saves it. It works the real tools: only the cursor and the buttons held
//! are made up.

use bevy::prelude::*;

use super::{road_at, Doc, EditorCamera};
use crate::game::AppState;
use crate::sim::track::WALL_DIST;

/// The pretend mouse.
#[derive(Resource)]
pub struct Demo {
    /// Where the cursor is, in logical pixels.
    pub cursor: Vec2,
    step: usize,
    /// Seconds into the current step.
    t: f32,
    from: Vec2,
    /// Where this step's move ends, once it has been worked out.
    to: Option<Vec2>,
    held: Vec<KeyCode>,
}

/// The name the demo saves its track under.
pub const NAME: &str = "Biscuit Bends";

#[derive(Clone, Copy)]
enum Step {
    Wait(f32),
    /// Glide the cursor somewhere, over this many seconds.
    Move(Aim, f32),
    /// Trace the figure of eight.
    Draw(f32),
    Press,
    Release,
    Keys(&'static [KeyCode]),
}

#[derive(Clone, Copy)]
enum Aim {
    /// The control point nearest this place on the map.
    Point(Vec2),
    /// The control point where the second pass crosses the first, or one this many along from it.
    Crossing(i32),
    /// The road nearest this place, and this far right of its centre line.
    Road(Vec2, f32),
    /// This far from where the cursor is, in metres on the map (x east, y south).
    By(Vec2),
    /// And in pixels.
    Pixels(Vec2),
}

/// The loop to draw: a figure of eight, a little lopsided so it isn't too regular. It starts
/// (and so the race does) on a straight, clear of the crossing.
fn eight(k: f32) -> Vec2 {
    let a = (k + START) * std::f32::consts::TAU;
    Vec2::new(250.0 * a.sin() + 25.0 * (2.0 * a).sin(), -165.0 * (2.0 * a).sin() - 30.0 * a.cos() + 10.0)
}

const START: f32 = 0.06;
/// How far round the loop the second pass over the crossing is.
const CROSSING: f32 = 0.5174 - START;

#[rustfmt::skip]
const SCRIPT: &[Step] = &[
    Step::Wait(0.4),
    // Draw the loop.
    Step::Press, Step::Draw(2.3), Step::Release, Step::Wait(0.5),
    // Shape: pull a couple of corners out.
    Step::Move(Aim::Point(Vec2::new(262.0, 10.0)), 0.35), Step::Press, Step::Move(Aim::By(Vec2::new(17.0, -13.0)), 0.45), Step::Release,
    Step::Move(Aim::Point(Vec2::new(-262.0, 10.0)), 0.45), Step::Press, Step::Move(Aim::By(Vec2::new(-16.0, 14.0)), 0.45), Step::Release,
    Step::Wait(0.15),
    // Hills & bridges: lift one road over the other.
    Step::Keys(&[KeyCode::Digit3]),
    Step::Move(Aim::Crossing(-1), 0.35), Step::Press, Step::Move(Aim::Pixels(Vec2::new(0.0, -70.0)), 0.25), Step::Release,
    Step::Move(Aim::Crossing(2), 0.25), Step::Press, Step::Move(Aim::Pixels(Vec2::new(0.0, -45.0)), 0.25), Step::Release,
    Step::Move(Aim::Crossing(1), 0.2), Step::Press, Step::Move(Aim::Pixels(Vec2::new(0.0, -95.0)), 0.3), Step::Release,
    Step::Move(Aim::Crossing(0), 0.2), Step::Press, Step::Move(Aim::Pixels(Vec2::new(0.0, -120.0)), 0.4), Step::Release,
    Step::Wait(0.3),
    // A boosted ramp and some boost pads.
    Step::Keys(&[KeyCode::Digit9]),
    Step::Move(Aim::Road(Vec2::new(-128.0, 112.0), 0.0), 0.35), Step::Press, Step::Release, Step::Wait(0.15),
    Step::Keys(&[KeyCode::Digit5]),
    Step::Move(Aim::Road(Vec2::new(-89.0, -92.0), -4.0), 0.35), Step::Press, Step::Release,
    Step::Move(Aim::Road(Vec2::new(-98.0, -108.0), 4.0), 0.15), Step::Press, Step::Release,
    Step::Move(Aim::Road(Vec2::new(165.0, -168.0), 0.0), 0.35), Step::Press, Step::Release,
    Step::Wait(0.3),
    Step::Keys(&[KeyCode::ControlLeft, KeyCode::KeyS]),
];

#[derive(Component)]
pub(super) struct Pointer;

pub(super) fn start(mut commands: Commands, mut images: ResMut<Assets<Image>>, cam: Query<Entity, With<EditorCamera>>, mut doc: ResMut<Doc>) {
    let Ok(cam) = cam.single() else { return };
    doc.map.name = NAME.into();
    doc.saved = doc.map.clone();
    commands.insert_resource(Demo { cursor: Vec2::new(760.0, 300.0), step: 0, t: 0.0, from: Vec2::ZERO, to: None, held: vec![] });
    commands.spawn((
        ImageNode::new(images.add(arrow())),
        Node { position_type: PositionType::Absolute, width: px(20), height: px(28), ..default() },
        GlobalZIndex(100),
        UiTargetCamera(cam),
        Pointer,
        DespawnOnExit(AppState::Editor),
    ));
}

/// A mouse pointer: a white arrow with a dark edge.
fn arrow() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const OUTLINE: [Vec2; 7] = [Vec2::new(1.0, 1.0), Vec2::new(1.0, 20.0), Vec2::new(5.5, 15.5), Vec2::new(9.0, 23.0), Vec2::new(12.0, 21.5), Vec2::new(8.5, 14.5), Vec2::new(15.0, 14.5)];
    let (w, h, scale) = (60u32, 84u32, 3.0);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / scale;
            let (mut inside, mut edge) = (false, f32::MAX);
            for k in 0..OUTLINE.len() {
                let (a, b) = (OUTLINE[k], OUTLINE[(k + 1) % OUTLINE.len()]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    inside = !inside;
                }
                let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                edge = edge.min(p.distance(a + (b - a) * along));
            }
            let px: [u8; 4] = match (inside, edge) {
                (true, e) if e > 0.9 => [255, 255, 255, 255],
                (_, e) if e <= 0.9 => [20, 20, 30, 255],
                _ => [0, 0, 0, 0],
            };
            data.extend(px);
        }
    }
    Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, default())
}

/// Play the script: move the cursor, and hold and let go of the buttons and keys.
#[allow(clippy::too_many_arguments)]
pub(super) fn play(
    time: Res<Time>,
    mut demo: ResMut<Demo>,
    doc: Res<Doc>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    cam: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut pointer: Query<&mut Node, With<Pointer>>,
) {
    let Ok((camera, gt)) = cam.single() else { return };
    // The map's (x, z) is the 2D world's (x, -y).
    let to_px = |m: Vec2| camera.world_to_viewport(gt, Vec3::new(m.x, -m.y, 0.0)).unwrap_or_default();
    let to_map = |p: Vec2| camera.viewport_to_world_2d(gt, p).map(|w| Vec2::new(w.x, -w.y)).unwrap_or_default();
    for k in std::mem::take(&mut demo.held) {
        keys.release(k);
    }
    let dt = time.delta_secs();
    if dt > 0.0 && demo.step < SCRIPT.len() {
        demo.t += dt;
        let ease = |x: f32| x * x * (3.0 - 2.0 * x);
        let done = match SCRIPT[demo.step] {
            Step::Wait(secs) => demo.t >= secs,
            Step::Draw(secs) => {
                // (Before the button goes down, so the stroke starts on the loop.)
                demo.cursor = to_px(eight(ease((demo.t / secs).min(1.0)) * 0.985));
                demo.t >= secs
            }
            Step::Move(aim, secs) => {
                if demo.to.is_none() {
                    demo.from = demo.cursor;
                    demo.to = Some(match aim {
                        Aim::Point(near) => to_px(doc.map.points.iter().map(|p| p.pos()).min_by(|a, b| a.distance(near).total_cmp(&b.distance(near))).unwrap_or(near)),
                        Aim::Crossing(along) => {
                            // Of the points drawn on the second pass, the one nearest the crossing.
                            let n = doc.map.points.len();
                            let near = eight(CROSSING);
                            let second = (n / 4..n * 3 / 4).min_by(|&a, &b| doc.map.points[a].pos().distance(near).total_cmp(&doc.map.points[b].pos().distance(near)));
                            to_px(second.map_or(near, |i| doc.map.points[(i as i32 + along) as usize].pos()))
                        }
                        Aim::Road(near, lateral) => {
                            let on_road = doc.built.as_ref().and_then(|(b, _)| road_at(b, near, WALL_DIST * 20.0).map(|h| &b.track.samples[h.idx]));
                            to_px(on_road.map_or(near, |s| s.pos + s.dir.perp() * lateral))
                        }
                        Aim::By(metres) => to_px(to_map(demo.cursor) + metres),
                        Aim::Pixels(by) => demo.cursor + by,
                    });
                }
                demo.cursor = demo.from.lerp(demo.to.unwrap(), ease((demo.t / secs).min(1.0)));
                demo.t >= secs
            }
            Step::Press => {
                if let Step::Draw(_) = SCRIPT[demo.step + 1] {
                    demo.cursor = to_px(eight(0.0));
                }
                mouse.press(MouseButton::Left);
                true
            }
            Step::Release => {
                mouse.release(MouseButton::Left);
                true
            }
            Step::Keys(ks) => {
                for &k in ks {
                    keys.press(k);
                    demo.held.push(k);
                }
                true
            }
        };
        if done {
            demo.step += 1;
            demo.t = 0.0;
            demo.to = None;
            if demo.step == SCRIPT.len() {
                let problems = doc.built.as_ref().map(|(_, p)| p.messages.clone()).unwrap_or_default();
                info!("editor demo finished: {} points, problems: {problems:?}", doc.map.points.len());
                if !problems.is_empty() {
                    error!("the demo's track can't be raced");
                }
            }
        }
    }
    for mut node in &mut pointer {
        node.left = px(demo.cursor.x - 1.0);
        node.top = px(demo.cursor.y - 1.0);
    }
}
