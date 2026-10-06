//! The track editor: draw a loop from above with the mouse, then shape it. Drag the points about,
//! raise the road into hills and bridges, and place the start line, item boxes, boost pads and
//! fords. Problems that would stop the map being raced show up in red.
//!
//! The map is drawn in 2D: the 2D world's (x, y) is the track's (x, -z), which is the view from
//! above with north up. The 3D world's camera is switched off while the editor is open.

mod demo;
pub mod files;
mod ui;
mod view;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::game::{AppState, SessionRes, Settings};
use crate::render::scene::MainCamera;
use crate::session::Session;
use crate::sim::custom::{self, Built, CustomMap, Ford, Pad, Problems, RampSpot};
use crate::sim::track::WALL_DIST;
use crate::sim::TrackId;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Editor), (open_editor, ui::spawn_ui, view::spawn_view).chain())
            .add_systems(OnEnter(AppState::Editor), demo::start.after(open_editor).run_if(|cli: Res<crate::game::Cli>| cli.demo))
            .add_systems(Update, demo::play.before(camera_controls).run_if(resource_exists::<demo::Demo>).run_if(in_state(AppState::Editor)))
            .add_systems(OnExit(AppState::Editor), close_editor)
            .add_systems(
                Update,
                (ui::buttons, ui::scroll_sidebar, typing, camera_controls, tools, rebuild, ui::style_buttons, ui::update_ui, view::draw_map, view::draw_overlay, view::update_labels)
                    .chain()
                    .run_if(in_state(AppState::Editor)),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Draw,
    Points,
    Height,
    Items,
    Boost,
    Ford,
    Start,
    Ramp,
    BoostRamp,
}

impl Tool {
    pub const ALL: [Tool; 9] = [Tool::Draw, Tool::Points, Tool::Height, Tool::Items, Tool::Boost, Tool::Ford, Tool::Start, Tool::Ramp, Tool::BoostRamp];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Draw => "Draw",
            Tool::Points => "Shape",
            Tool::Height => "Hills & bridges",
            Tool::Items => "Item boxes",
            Tool::Boost => "Boost pads",
            Tool::Ford => "Fords",
            Tool::Start => "Start line",
            Tool::Ramp => "Ramps",
            Tool::BoostRamp => "Boosted ramps",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Tool::Draw => "Hold the left button and draw a loop in the direction you'll race. This replaces the track.",
            Tool::Points => "Drag a point to reshape the road (hold Shift to move just that point). Click the road to add a point. Right-click a point to delete it.",
            Tool::Height => "Drag a point up or down (or scroll over it) to raise the road. Roads that cross need a 7.5m gap.",
            Tool::Items => "Click the road to add a row of item boxes. Right-click one to remove it.",
            Tool::Boost => "Click the road to add a boost pad where you click. Right-click one to remove it.",
            Tool::Ford => "Drag along the road to flood it. Right-click a ford to remove it.",
            Tool::Start => "Click the road to move the start line there. Reverse turns the race round.",
            Tool::Ramp => "Click the road to add a ramp where you click: karts jump off it and can trick for a boost. Leave a straight run after it to land on. Right-click one to remove it.",
            Tool::BoostRamp => "Click the road to add a boosted ramp: it has a boost pad on its face and throws karts about twice as far as a plain one. Right-click one to remove it.",
        }
    }

    fn key(self) -> KeyCode {
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9][Tool::ALL.iter().position(|&t| t == self).unwrap()]
    }
}

/// The map being edited.
#[derive(Resource)]
pub struct Doc {
    pub map: CustomMap,
    /// The file it's saved in.
    pub file: Option<String>,
    /// As last saved (or opened), to tell whether there are unsaved changes.
    saved: CustomMap,
    undo: Vec<CustomMap>,
    redo: Vec<CustomMap>,
    /// The map as a track, and what's wrong with it; `None` while there's no loop.
    pub built: Option<(Built, Problems)>,
    /// Bumped on every change, so the view knows to rebuild.
    pub version: u64,
    built_version: u64,
}

impl Doc {
    fn new(map: CustomMap, file: Option<String>) -> Self {
        Doc { saved: map.clone(), map, file, undo: vec![], redo: vec![], built: None, version: 1, built_version: 0 }
    }

    fn blank() -> Self {
        Self::new(CustomMap { name: "My track".into(), theme: crate::sim::Theme::Circuit, ..default() }, None)
    }

    pub fn dirty(&self) -> bool {
        self.map != self.saved
    }

    /// Call before a change, so it can be undone.
    fn checkpoint(&mut self) {
        self.undo.push(self.map.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    fn undo(&mut self) {
        if let Some(m) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.map, m));
            self.changed();
        }
    }

    fn redo(&mut self) {
        if let Some(m) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.map, m));
            self.changed();
        }
    }

    pub fn raceable(&self) -> bool {
        self.built.as_ref().is_some_and(|(_, p)| p.ok())
    }
}

/// What the mouse is doing in the editor.
#[derive(Resource)]
pub struct EditorState {
    pub tool: Tool,
    /// Typing into the name box.
    pub naming: bool,
    pub message: String,
    message_time: f32,
    /// World position of the cursor over the map, if it's over the map (not the sidebar).
    pub cursor: Option<Vec2>,
    /// Metres per pixel.
    pub zoom: f32,
    pub hover_point: Option<usize>,
    pub drag: Drag,
    /// The loop being drawn.
    pub stroke: Vec<Vec2>,
    /// Where the points were when a drag started.
    drag_from: Vec<Vec2>,
    /// Set when leaving or discarding would lose changes: a second press goes ahead.
    pub confirm: Option<Confirm>,
    /// Fit the view to the map next frame.
    fit: bool,
    /// Zoom by this factor about the middle of the map next frame (from the zoom buttons).
    zoom_by: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drag {
    None,
    Point(usize),
    Height { point: usize, start_h: f32, start_y: f32 },
    Ford { from: f32 },
}

#[derive(Clone, PartialEq, Debug)]
pub enum Confirm {
    Leave,
    New,
    Open(String),
}

impl EditorState {
    fn new(tool: Tool) -> Self {
        EditorState {
            tool,
            naming: false,
            message: String::new(),
            message_time: 0.0,
            cursor: None,
            zoom: 1.2,
            hover_point: None,
            drag: Drag::None,
            stroke: vec![],
            drag_from: vec![],
            confirm: None,
            fit: true,
            zoom_by: 1.0,
        }
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
        self.message_time = 5.0;
    }
}

/// Set while a custom track is being test-driven, to come back to the editor afterwards.
#[derive(Resource)]
pub struct TestDrive;

/// The editor's 2D camera.
#[derive(Component)]
pub struct EditorCamera;

/// Width of the sidebar, in logical pixels.
pub const SIDEBAR: f32 = 330.0;

fn open_editor(
    mut commands: Commands,
    doc: Option<Res<Doc>>,
    settings: Res<Settings>,
    headless: Option<Res<crate::render::scene::HeadlessTarget>>,
    mut main: Query<&mut Camera, With<MainCamera>>,
) {
    for mut cam in &mut main {
        cam.is_active = false;
    }
    commands.remove_resource::<TestDrive>();
    let mut has_track = doc.as_ref().is_some_and(|d| !d.map.is_empty());
    if doc.is_none() {
        // Open the track picked in the menu (or on the command line) if it's a custom one.
        let file = match settings.track {
            TrackId::Custom(h) => custom::file_of(h),
            _ => None,
        };
        let map = file.as_deref().and_then(custom::by_file).and_then(custom::map_of);
        let doc = match map {
            Some(map) => Doc::new(map, file),
            None => Doc::blank(),
        };
        has_track = !doc.map.is_empty();
        commands.insert_resource(doc);
    }
    commands.insert_resource(EditorState::new(if has_track { Tool::Points } else { Tool::Draw }));
    let mut cam = commands.spawn((Camera2d, Camera { order: 5, ..default() }, EditorCamera, DespawnOnExit(AppState::Editor)));
    if let Some(target) = headless {
        cam.insert(bevy::camera::RenderTarget::Image(target.0.clone().into()));
    }
}

fn close_editor(mut commands: Commands, mut main: Query<&mut Camera, With<MainCamera>>, test: Option<Res<TestDrive>>) {
    for mut cam in &mut main {
        cam.is_active = true;
    }
    commands.remove_resource::<EditorState>();
    // Keep the map while it's being test-driven; otherwise start afresh next time.
    if test.is_none() {
        commands.remove_resource::<Doc>();
    }
}

// ----- Actions (from buttons and keys) -------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Tool(Tool),
    /// The next (1) or previous (-1) scenery.
    Theme(i32),
    Name,
    Smooth,
    Reverse,
    Undo,
    Redo,
    New,
    Save,
    TestDrive,
    Leave,
    Fit,
    /// Zoom in (below 1) or out (above 1) about the middle of the map.
    Zoom(f32),
}

#[allow(clippy::too_many_arguments)]
pub fn act(
    action: Action,
    doc: &mut Doc,
    st: &mut EditorState,
    commands: &mut Commands,
    settings: &mut Settings,
    next: &mut NextState<AppState>,
) {
    if !matches!(action, Action::Leave | Action::New) {
        st.confirm = None;
    }
    if action != Action::Name {
        st.naming = false;
    }
    match action {
        Action::Tool(t) => {
            st.tool = t;
            st.drag = Drag::None;
        }
        Action::Theme(step) => {
            doc.checkpoint();
            doc.map.theme = doc.map.theme.cycle(step);
            doc.changed();
        }
        Action::Name => st.naming = !st.naming,
        Action::Smooth | Action::Reverse if doc.map.is_empty() => st.say("Draw a track first."),
        Action::Smooth => {
            doc.checkpoint();
            doc.map.smooth();
            doc.changed();
        }
        Action::Reverse => {
            doc.checkpoint();
            doc.map.flip();
            doc.changed();
            st.say("The race now goes the other way round.");
        }
        Action::Undo => doc.undo(),
        Action::Redo => doc.redo(),
        Action::New => {
            if doc.dirty() && st.confirm != Some(Confirm::New) {
                st.confirm = Some(Confirm::New);
                st.say("There are unsaved changes: press New again to throw them away.");
                return;
            }
            st.confirm = None;
            *doc = Doc::blank();
            st.tool = Tool::Draw;
            st.say("A new track: draw a loop.");
        }
        Action::Save => save(doc, st, settings),
        Action::TestDrive => {
            if doc.map.is_empty() {
                st.say("Draw a track first.");
            } else if !doc.raceable() {
                st.say("Fix the problems in red first.");
            } else {
                let id = custom::register(doc.map.clone(), None);
                let players = if settings.players > 1 { settings.players } else { 1 };
                let mut s = Session::solo(&settings.name, players, settings.bots, id, settings.class);
                s.start_race();
                commands.insert_resource(SessionRes(s));
                commands.insert_resource(TestDrive);
                next.set(AppState::Race);
            }
        }
        Action::Leave => {
            if doc.dirty() && st.confirm != Some(Confirm::Leave) {
                st.confirm = Some(Confirm::Leave);
                st.say("There are unsaved changes: press Esc again to leave anyway.");
                return;
            }
            next.set(AppState::Menu);
        }
        Action::Fit => st.fit = true,
        Action::Zoom(f) => st.zoom_by *= f,
    }
}

fn save(doc: &mut Doc, st: &mut EditorState, settings: &mut Settings) {
    if doc.map.is_empty() {
        st.say("Draw a track first.");
        return;
    }
    if doc.map.name.trim().is_empty() {
        doc.map.name = "My track".into();
        doc.changed();
    }
    match files::save(&doc.map, doc.file.as_deref()) {
        Ok((file, id)) => {
            doc.file = Some(file.clone());
            doc.saved = doc.map.clone();
            if doc.raceable() {
                // Pick it in the menu, ready to race.
                settings.track = id;
                settings.save();
                st.say(format!("Saved as {file}. It's in the menu's track list."));
            } else {
                st.say(format!("Saved as {file}. Fix the problems in red to race it."));
            }
        }
        Err(e) => st.say(e),
    }
}

/// Open a saved map.
pub fn open(doc: &mut Doc, st: &mut EditorState, file: &str) {
    if doc.dirty() && st.confirm != Some(Confirm::Open(file.to_string())) {
        st.confirm = Some(Confirm::Open(file.to_string()));
        st.say("There are unsaved changes: click it again to open it anyway.");
        return;
    }
    st.confirm = None;
    match custom::by_file(file).and_then(custom::map_of) {
        Some(map) => {
            *doc = Doc::new(map, Some(file.to_string()));
            st.tool = Tool::Points;
            st.fit = true;
            st.say(format!("Opened {file}."));
        }
        None => st.say(format!("Couldn't open {file}.")),
    }
}

// ----- Input --------------------------------------------------------------------------------

/// Keyboard: the name box, shortcuts.
#[allow(clippy::too_many_arguments)]
fn typing(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    mut doc: ResMut<Doc>,
    mut st: ResMut<EditorState>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<AppState>>,
    time: Res<Time>,
) {
    st.message_time -= time.delta_secs();
    if st.message_time <= 0.0 {
        st.message.clear();
    }
    if st.naming {
        let mut name = doc.map.name.clone();
        for ev in typed.read() {
            if !ev.state.is_pressed() {
                continue;
            }
            match &ev.logical_key {
                Key::Backspace => {
                    name.pop();
                }
                Key::Enter | Key::Escape => st.naming = false,
                Key::Character(c) if name.chars().count() < 28 => name.extend(c.chars().filter(|ch| !ch.is_control() && *ch != '=')),
                _ => {}
            }
        }
        if name != doc.map.name {
            doc.map.name = name;
            doc.changed();
        }
        return;
    }
    typed.clear();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut action = None;
    for t in Tool::ALL {
        if keys.just_pressed(t.key()) {
            action = Some(Action::Tool(t));
        }
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        action = Some(if shift { Action::Redo } else { Action::Undo });
    } else if ctrl && keys.just_pressed(KeyCode::KeyY) {
        action = Some(Action::Redo);
    } else if ctrl && keys.just_pressed(KeyCode::KeyS) {
        action = Some(Action::Save);
    } else if keys.just_pressed(KeyCode::Escape) {
        action = Some(Action::Leave);
    } else if keys.just_pressed(KeyCode::F5) || (ctrl && keys.just_pressed(KeyCode::Enter)) {
        action = Some(Action::TestDrive);
    } else if keys.just_pressed(KeyCode::KeyF) && !ctrl {
        action = Some(Action::Fit);
    }
    if let Some(a) = action {
        act(a, &mut doc, &mut st, &mut commands, &mut settings, &mut next);
    }
}

/// Scroll to zoom (about the cursor), drag with the middle button to pan, or use the arrow keys.
#[allow(clippy::too_many_arguments)]
fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    doc: Res<Doc>,
    demo: Option<Res<demo::Demo>>,
    mut st: ResMut<EditorState>,
    mut cam: Query<(&Camera, &GlobalTransform, &mut Transform, &mut Projection), With<EditorCamera>>,
    ui_buttons: Query<&Interaction, With<ui::EdButton>>,
) {
    let Ok((camera, gt, mut tr, mut proj)) = cam.single_mut() else { return };
    let size = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    // The map is in the area right of the sidebar: keep its centre where the camera looks.
    let canvas_centre = Vec2::new(SIDEBAR * 0.5, 0.0);

    // (Wait for the track to be built, to fit it.)
    if st.fit && (doc.built.is_some() || doc.map.is_empty()) {
        st.fit = false;
        let (lo, hi) = match &doc.built {
            Some((b, _)) => b.track.samples.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), s| (lo.min(s.pos), hi.max(s.pos))),
            None => (Vec2::splat(-300.0), Vec2::splat(300.0)),
        };
        let (lo, hi) = (lo - WALL_DIST * 3.0, hi + WALL_DIST * 3.0);
        let avail = (size - Vec2::new(SIDEBAR + 40.0, 80.0)).max(Vec2::splat(100.0));
        st.zoom = ((hi - lo) / avail).max_element().clamp(0.15, 6.0);
        let mid = (lo + hi) * 0.5;
        tr.translation = Vec2::new(mid.x, -mid.y).extend(0.0) - (canvas_centre * st.zoom).extend(0.0);
    }

    let cursor_px = demo.map(|d| d.cursor).or_else(|| windows.single().ok().and_then(|w| w.cursor_position()));
    // (Not over the buttons floating on the map, either.)
    let over_button = ui_buttons.iter().any(|i| *i != Interaction::None);
    let over_map = cursor_px.is_some_and(|p| p.x > SIDEBAR) && !over_button;
    let world = |p: Vec2| camera.viewport_to_world_2d(gt, p).ok();

    // Zoom, unless the height tool is using the wheel on a point.
    let steps = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    let wheel_for_height = st.tool == Tool::Height && st.hover_point.is_some();
    if steps != 0.0 && over_map && !wheel_for_height {
        let old = st.zoom;
        st.zoom = (st.zoom * 0.87f32.powf(steps)).clamp(0.12, 8.0);
        if let Some(c) = cursor_px.and_then(world) {
            // Keep the point under the cursor still.
            let cam = tr.translation.truncate();
            let new = c + (cam - c) * (st.zoom / old);
            tr.translation = new.extend(0.0);
        }
    }
    if st.zoom_by != 1.0 {
        // Keep the middle of the map still.
        let mid = tr.translation.truncate() + canvas_centre * st.zoom;
        st.zoom = (st.zoom * st.zoom_by).clamp(0.12, 8.0);
        st.zoom_by = 1.0;
        tr.translation = (mid - canvas_centre * st.zoom).extend(0.0);
    }
    if let Projection::Orthographic(o) = proj.as_mut() {
        o.scale = st.zoom;
    }

    // Pan
    let panning = mouse.pressed(MouseButton::Middle) || (st.tool == Tool::Draw && mouse.pressed(MouseButton::Right));
    if panning && motion.delta != Vec2::ZERO {
        tr.translation += Vec3::new(-motion.delta.x, motion.delta.y, 0.0) * st.zoom;
    }
    if !st.naming {
        let mut dir = Vec2::ZERO;
        for (k, d) in [(KeyCode::ArrowLeft, -Vec2::X), (KeyCode::ArrowRight, Vec2::X), (KeyCode::ArrowUp, Vec2::Y), (KeyCode::ArrowDown, -Vec2::Y), (KeyCode::KeyA, -Vec2::X), (KeyCode::KeyD, Vec2::X), (KeyCode::KeyW, Vec2::Y), (KeyCode::KeyS, -Vec2::Y)] {
            if keys.pressed(k) && !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
                dir += d;
            }
        }
        tr.translation += (dir * 600.0 * st.zoom * time.delta_secs()).extend(0.0);
    }

    // Map coordinates are (x, -y) of the 2D world.
    st.cursor = cursor_px.filter(|_| over_map).and_then(world).map(|w| Vec2::new(w.x, -w.y));
}

/// Where a map position falls on the road: the nearest sample (preferring the upper road where
/// two cross), its spline parameter and how far right of the centre line it is.
pub struct RoadHit {
    pub idx: usize,
    pub u: f32,
    pub lateral: f32,
}

pub fn road_at(built: &Built, p: Vec2, reach: f32) -> Option<RoadHit> {
    let t = &built.track;
    let mut best: Option<(usize, f32)> = None;
    for (i, s) in t.samples.iter().enumerate() {
        let d = s.pos.distance(p);
        if d > reach {
            continue;
        }
        // Rank by distance, but anything on the upper deck wins.
        let score = d - s.height * 10.0;
        if best.is_none_or(|(_, b)| score < b) {
            best = Some((i, score));
        }
    }
    let (i, _) = best?;
    let s = &t.samples[i];
    Some(RoadHit { idx: i, u: built.params[i], lateral: (p - s.pos).dot(s.dir.perp()) })
}

/// Arc length (along the race) between two spline parameters on the built track.
fn arc_between(built: &Built, a: f32, b: f32) -> f32 {
    let len = built.spline.length();
    (built.spline.s_at(b) - built.spline.s_at(a)).rem_euclid(len)
}

/// Pixels within which the cursor picks a point.
const PICK_PX: f32 = 11.0;

/// The mouse tools.
#[allow(clippy::too_many_arguments)]
fn tools(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    demo: Option<Res<demo::Demo>>,
    mut doc: ResMut<Doc>,
    mut st: ResMut<EditorState>,
) {
    let cursor = st.cursor;
    let zoom = st.zoom;
    let cursor_y = demo.map(|d| d.cursor).or_else(|| windows.single().ok().and_then(|w| w.cursor_position())).map_or(0.0, |p| p.y);

    // Hovered control point.
    st.hover_point = match (cursor, st.tool) {
        (Some(c), Tool::Points | Tool::Height) => {
            let near = doc.map.points.iter().enumerate().map(|(i, p)| (i, p.pos().distance(c))).min_by(|a, b| a.1.total_cmp(&b.1));
            near.filter(|&(_, d)| d < PICK_PX * zoom).map(|(i, _)| i)
        }
        _ => None,
    };
    if let Drag::Point(i) | Drag::Height { point: i, .. } = st.drag {
        st.hover_point = Some(i);
    }

    let pressed = mouse.just_pressed(MouseButton::Left) && cursor.is_some();
    let right = mouse.just_pressed(MouseButton::Right) && cursor.is_some();
    let released = mouse.just_released(MouseButton::Left);
    let hit = |doc: &Doc, reach: f32| cursor.and_then(|c| doc.built.as_ref().and_then(|(b, _)| road_at(b, c, reach)));

    match st.tool {
        Tool::Draw => {
            if pressed {
                st.stroke = vec![cursor.unwrap()];
                st.drag = Drag::None;
            }
            if mouse.pressed(MouseButton::Left)
                && let Some(c) = cursor
                && st.stroke.last().is_some_and(|l| l.distance(c) > 2.0 * zoom)
            {
                st.stroke.push(c);
            }
            if released && !st.stroke.is_empty() {
                let stroke = std::mem::take(&mut st.stroke);
                match custom::points_from_stroke(&stroke) {
                    Some(points) => {
                        doc.checkpoint();
                        let m = &mut doc.map;
                        m.points = points;
                        m.start = 0.0;
                        m.pads.clear();
                        m.ramps.clear();
                        m.fords.clear();
                        m.items = custom::default_items(m, 3);
                        doc.changed();
                        st.tool = Tool::Points;
                        st.say("Now shape it: drag the points, or use the other tools.");
                    }
                    None => st.say("That's too small for a track: draw a bigger loop."),
                }
            }
        }
        Tool::Points => {
            if pressed {
                if let Some(i) = st.hover_point {
                    doc.checkpoint();
                    st.drag = Drag::Point(i);
                } else if let Some(h) = hit(&doc, WALL_DIST) {
                    doc.checkpoint();
                    let i = doc.map.insert_point(h.u);
                    doc.changed();
                    st.drag = Drag::Point(i);
                }
                st.drag_from = doc.map.points.iter().map(|p| p.pos()).collect();
            }
            if let (Drag::Point(i), Some(c)) = (st.drag, cursor) {
                // The point follows the cursor, and pulls its neighbours along a little (unless
                // Shift is held), so the road bends smoothly instead of kinking.
                let alone = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                let delta = c - st.drag_from[i];
                let weights = drag_weights(&st.drag_from, i, if alone { 0.0 } else { DRAG_REACH });
                let mut moved = false;
                for (j, p) in doc.map.points.iter_mut().enumerate() {
                    let to = st.drag_from[j] + delta * weights[j];
                    if p.pos().distance(to) > 0.001 {
                        (p.x, p.z) = (to.x, to.y);
                        moved = true;
                    }
                }
                if moved {
                    doc.changed();
                }
            }
            let delete = right || keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace);
            if delete
                && st.drag == Drag::None
                && let Some(i) = st.hover_point
            {
                if doc.map.points.len() > custom::MIN_POINTS {
                    doc.checkpoint();
                    doc.map.remove_point(i);
                    doc.changed();
                    st.hover_point = None;
                } else {
                    st.say("A track needs at least four points.");
                }
            }
        }
        Tool::Height => {
            if pressed && let Some(i) = st.hover_point {
                doc.checkpoint();
                st.drag = Drag::Height { point: i, start_h: doc.map.points[i].h, start_y: cursor_y };
            }
            if let Drag::Height { point, start_h, start_y } = st.drag {
                // 10 pixels a metre, in 0.25m steps.
                let h = ((start_h + (start_y - cursor_y) / 10.0) * 4.0).round() / 4.0;
                let h = h.clamp(0.0, custom::MAX_HEIGHT);
                if let Some(p) = doc.map.points.get_mut(point)
                    && p.h != h
                {
                    p.h = h;
                    doc.changed();
                }
            }
            let steps = match scroll.unit {
                MouseScrollUnit::Line => scroll.delta.y,
                MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
            };
            let mut dh = steps.signum() * if steps != 0.0 { 0.5 } else { 0.0 };
            if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) || keys.just_pressed(KeyCode::PageUp) {
                dh += 0.5;
            }
            if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) || keys.just_pressed(KeyCode::PageDown) {
                dh -= 0.5;
            }
            if let (Some(i), true, Drag::None) = (st.hover_point, dh != 0.0, st.drag) {
                doc.checkpoint();
                let p = &mut doc.map.points[i];
                p.h = (p.h + dh).clamp(0.0, custom::MAX_HEIGHT);
                doc.changed();
            }
            if right && let Some(i) = st.hover_point {
                doc.checkpoint();
                doc.map.points[i].h = 0.0;
                doc.changed();
            }
        }
        Tool::Items => {
            if pressed && let Some(h) = hit(&doc, WALL_DIST) {
                let b = &doc.built.as_ref().unwrap().0;
                let near = doc.map.items.iter().any(|&u| arc_between(b, u, h.u).min(arc_between(b, h.u, u)) < 25.0);
                if near {
                    st.say("There's already a row of item boxes there.");
                } else {
                    doc.checkpoint();
                    doc.map.items.push(h.u);
                    doc.changed();
                }
            }
            if right {
                remove_nearest(&mut doc, cursor, |m| &mut m.items, |&u| u);
            }
        }
        Tool::Boost => {
            if pressed && let Some(h) = hit(&doc, WALL_DIST) {
                doc.checkpoint();
                doc.map.pads.push(Pad { u: h.u, lateral: h.lateral.clamp(-custom::PAD_MAX_LATERAL, custom::PAD_MAX_LATERAL) });
                doc.changed();
            }
            if right {
                remove_nearest(&mut doc, cursor, |m| &mut m.pads, |p| p.u);
            }
        }
        Tool::Ramp | Tool::BoostRamp => {
            if pressed && let Some(h) = hit(&doc, WALL_DIST) {
                doc.checkpoint();
                let lateral = h.lateral.clamp(-custom::RAMP_MAX_LATERAL, custom::RAMP_MAX_LATERAL);
                doc.map.ramps.push(RampSpot { u: h.u, lateral, boosted: st.tool == Tool::BoostRamp });
                doc.changed();
            }
            if right {
                remove_nearest(&mut doc, cursor, |m| &mut m.ramps, |r| r.u);
            }
        }
        Tool::Ford => {
            if pressed && let Some(h) = hit(&doc, WALL_DIST) {
                st.drag = Drag::Ford { from: h.u };
            }
            if released {
                if let (Drag::Ford { from }, Some(h)) = (st.drag, hit(&doc, WALL_DIST * 3.0)) {
                    let b = &doc.built.as_ref().unwrap().0;
                    // Whichever way it was dragged, the ford runs the short way between the ends.
                    let (a, z) = if arc_between(b, from, h.u) <= arc_between(b, h.u, from) { (from, h.u) } else { (h.u, from) };
                    let len = arc_between(b, a, z);
                    if len < 8.0 {
                        st.say("Drag along the road to make a ford.");
                    } else if len > 150.0 {
                        st.say("Keep fords under 150m long.");
                    } else {
                        doc.checkpoint();
                        doc.map.fords.push(Ford { from: a, to: z });
                        doc.changed();
                    }
                }
                st.drag = Drag::None;
            }
            if right && let Some(h) = hit(&doc, WALL_DIST) {
                let b = &doc.built.as_ref().unwrap().0;
                let inside = doc.map.fords.iter().position(|f| arc_between(b, f.from, h.u) <= arc_between(b, f.from, f.to) + 3.0);
                if let Some(i) = inside {
                    doc.checkpoint();
                    doc.map.fords.remove(i);
                    doc.changed();
                }
            }
        }
        Tool::Start => {
            if pressed && let Some(h) = hit(&doc, WALL_DIST) {
                doc.checkpoint();
                doc.map.start = h.u;
                doc.changed();
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        st.drag = Drag::None;
    }
}

/// How far along the loop dragging a point pulls its neighbours.
const DRAG_REACH: f32 = 45.0;

/// How much each point moves when point `i` is dragged: 1 for `i`, easing to nothing `reach`
/// metres away along the loop either way.
fn drag_weights(pts: &[Vec2], i: usize, reach: f32) -> Vec<f32> {
    let n = pts.len();
    let mut w = vec![0.0; n];
    w[i] = 1.0;
    for dir in [1, n - 1] {
        let (mut j, mut d) = (i, 0.0);
        loop {
            let k = (j + dir) % n;
            d += pts[j].distance(pts[k]);
            if d >= reach || k == i {
                break;
            }
            w[k] = f32::max(w[k], (1.0 - d / reach).powi(2));
            j = k;
        }
    }
    w
}

/// Right-click removes the nearest feature to the cursor (within 20m along the road).
fn remove_nearest<T>(doc: &mut Doc, cursor: Option<Vec2>, list: impl Fn(&mut CustomMap) -> &mut Vec<T>, u_of: impl Fn(&T) -> f32) {
    let Some((b, _)) = &doc.built else { return };
    let Some(h) = cursor.and_then(|c| road_at(b, c, WALL_DIST * 1.5)) else { return };
    let dists: Vec<f32> = list(&mut doc.map).iter().map(|x| arc_between(b, u_of(x), h.u).min(arc_between(b, h.u, u_of(x)))).collect();
    if let Some((i, _)) = dists.iter().enumerate().filter(|(_, d)| **d < 20.0).min_by(|a, b| a.1.total_cmp(b.1)) {
        doc.checkpoint();
        list(&mut doc.map).remove(i);
        doc.changed();
    }
}

/// Rebuild the track and its checks when the map changes.
fn rebuild(mut doc: ResMut<Doc>) {
    if doc.built_version == doc.version {
        return;
    }
    doc.built_version = doc.version;
    doc.built = (!doc.map.is_empty()).then(|| {
        let b = custom::build(&doc.map, TrackId::Custom(0));
        let p = custom::check(&b.track);
        (b, p)
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The mouse tools, driven directly (there's no window to click in a test).
    struct Rig(App);

    impl Rig {
        fn new() -> Self {
            let mut app = App::new();
            app.init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<AccumulatedMouseScroll>()
                .insert_resource(Doc::blank())
                .insert_resource(EditorState::new(Tool::Draw))
                .add_systems(Update, (tools, rebuild).chain());
            Rig(app)
        }

        fn frame(&mut self, cursor: Vec2) {
            self.0.world_mut().resource_mut::<EditorState>().cursor = Some(cursor);
            self.0.update();
            self.0.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            self.0.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::ZERO;
        }

        fn click(&mut self, at: Vec2, button: MouseButton) {
            self.mouse(button, true);
            self.frame(at);
            self.mouse(button, false);
            self.frame(at);
        }

        fn drag(&mut self, path: &[Vec2]) {
            self.mouse(MouseButton::Left, true);
            for &p in path {
                self.frame(p);
            }
            self.mouse(MouseButton::Left, false);
            self.frame(*path.last().unwrap());
        }

        fn mouse(&mut self, b: MouseButton, down: bool) {
            let mut m = self.0.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            if down { m.press(b) } else { m.release(b) }
        }

        fn tool(&mut self, t: Tool) {
            self.0.world_mut().resource_mut::<EditorState>().tool = t;
        }

        fn doc(&self) -> &Doc {
            self.0.world().resource::<Doc>()
        }

        /// A point on the road `s` metres round from point 0, and `lat` to the right.
        fn road(&self, s: f32, lat: f32) -> Vec2 {
            let (b, _) = self.doc().built.as_ref().unwrap();
            let p = b.spline.point(s).truncate();
            let d = (b.spline.point(s + 1.0).truncate() - p).normalize();
            p + d.perp() * lat
        }
    }

    #[test]
    fn draw_and_shape_a_track() {
        let mut rig = Rig::new();
        // Draw a big rounded rectangle.
        let stroke: Vec<Vec2> = (0..=300)
            .map(|k| {
                let a = k as f32 / 300.0 * std::f32::consts::TAU;
                let f = |x: f32| x.signum() * x.abs().powf(0.5);
                Vec2::new(250.0 * f(a.cos()), 150.0 * f(a.sin()))
            })
            .collect();
        rig.drag(&stroke);
        assert_eq!(rig.0.world().resource::<EditorState>().tool, Tool::Points);
        let n = rig.doc().map.points.len();
        assert!(n >= 8, "{n} points");
        assert_eq!(rig.doc().map.items.len(), 3);
        assert!(rig.doc().raceable(), "{:?}", rig.doc().built.as_ref().unwrap().1.messages);

        // Drag point 2 outwards.
        let p2 = rig.doc().map.points[2].pos();
        let out = p2.normalize() * 8.0;
        rig.drag(&[p2, p2 + out * 0.5, p2 + out]);
        assert!(rig.doc().map.points[2].pos().distance(p2 + out) < 0.01);
        assert!(rig.doc().raceable(), "{:?}", rig.doc().built.as_ref().unwrap().1.messages);
        // Click the road in the middle of the longest gap between points to add one there.
        let mid = {
            let (b, _) = rig.doc().built.as_ref().unwrap();
            let k = (0..n).max_by(|&i, &j| (b.spline.s_at(i as f32 + 1.0) - b.spline.s_at(i as f32)).total_cmp(&(b.spline.s_at(j as f32 + 1.0) - b.spline.s_at(j as f32)))).unwrap();
            let s = b.spline.s_at(k as f32 + 0.5);
            rig.road(s, 3.0)
        };
        rig.click(mid, MouseButton::Left);
        assert_eq!(rig.doc().map.points.len(), n + 1);
        // Right-click it away again.
        let added = rig.doc().map.points.iter().map(|p| p.pos()).min_by(|a, b| a.distance(mid).total_cmp(&b.distance(mid))).unwrap();
        rig.click(added, MouseButton::Right);
        assert_eq!(rig.doc().map.points.len(), n);

        // Item boxes, boost pads, a ford, the start line.
        rig.tool(Tool::Items);
        rig.click(rig.road(500.0, 0.0), MouseButton::Left);
        assert_eq!(rig.doc().map.items.len(), 4);
        rig.click(rig.road(505.0, 0.0), MouseButton::Left);
        assert_eq!(rig.doc().map.items.len(), 4, "too close to the last row");
        rig.click(rig.road(502.0, 0.0), MouseButton::Right);
        assert_eq!(rig.doc().map.items.len(), 3);
        rig.tool(Tool::Boost);
        rig.click(rig.road(300.0, 4.0), MouseButton::Left);
        assert_eq!(rig.doc().map.pads.len(), 1);
        assert!((rig.doc().map.pads[0].lateral - 4.0).abs() < 0.5);
        rig.tool(Tool::Ramp);
        rig.click(rig.road(400.0, -2.0), MouseButton::Left);
        rig.tool(Tool::BoostRamp);
        rig.click(rig.road(600.0, 9.0), MouseButton::Left);
        let t = &rig.doc().built.as_ref().unwrap().0.track;
        assert_eq!(t.ramps.len(), 2);
        assert!(!t.ramps[0].boosted && t.ramps[1].boosted);
        assert!((t.ramps[0].lateral + 2.0).abs() < 0.5);
        assert_eq!(t.ramps[1].lateral, custom::RAMP_MAX_LATERAL, "kept on the road");
        rig.click(rig.road(603.0, 0.0), MouseButton::Right);
        assert_eq!(rig.doc().map.ramps.len(), 1);
        rig.tool(Tool::Ford);
        let path: Vec<Vec2> = (0..=10).map(|k| rig.road(700.0 + k as f32 * 5.0, 0.0)).collect();
        rig.drag(&path);
        let t = &rig.doc().built.as_ref().unwrap().0.track;
        assert_eq!(t.water.len(), 1);
        assert!((t.water[0].1 - 50.0).abs() < 3.0, "ford {:?}", t.water);
        rig.tool(Tool::Start);
        rig.click(rig.road(200.0, 0.0), MouseButton::Left);
        let t = &rig.doc().built.as_ref().unwrap().0.track;
        assert!(t.samples[0].pos.distance(rig.road(200.0, 0.0)) < 2.0);

        // Raise a point with the scroll wheel, then undo it.
        rig.tool(Tool::Height);
        let p5 = rig.doc().map.points[5].pos();
        rig.frame(p5);
        rig.0.world_mut().resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, 1.0);
        rig.frame(p5);
        assert_eq!(rig.doc().map.points[5].h, 0.5);
        rig.0.world_mut().resource_mut::<Doc>().undo();
        rig.frame(p5);
        assert_eq!(rig.doc().map.points[5].h, 0.0);
        assert!(rig.doc().raceable(), "{:?}", rig.doc().built.as_ref().unwrap().1.messages);
    }
}
