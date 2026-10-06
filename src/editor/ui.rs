//! The editor's UI. The sidebar has the map's name and scenery, the tools (with help for the one
//! picked), editing, the lap's numbers and anything stopping it being raced, your saved tracks,
//! saving and test driving. Over the map: messages, a hint while there's no track, zoom buttons
//! with a scale bar, the keyboard shortcuts, and what the button under the mouse does.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::window::PrimaryWindow;

use super::{act, open, view, Action, Doc, EditorCamera, EditorState, Tool, SIDEBAR};
use crate::game::{AppState, Settings};
use crate::menu::ORANGE;
use crate::render::trackmesh::rainbow;
use crate::sim::custom;

const MAX_SAVED: usize = 8;

const PANEL: Color = Color::srgba(0.04, 0.05, 0.11, 0.94);
const CARD: Color = Color::srgb(0.075, 0.085, 0.16);
const FIELD: Color = Color::srgb(0.02, 0.025, 0.06);
const FIELD_EDGE: Color = Color::srgb(0.24, 0.26, 0.4);
const BUTTON: Color = Color::srgb(0.13, 0.15, 0.25);
const BUTTON_HOVER: Color = Color::srgb(0.2, 0.23, 0.38);
const BUTTON_OFF: Color = Color::srgb(0.09, 0.1, 0.17);
/// The picked tool, the track that's open, the name box while typing.
const PICKED: Color = Color::srgb(0.24, 0.15, 0.08);
const PICKED_HOVER: Color = Color::srgb(0.3, 0.19, 0.1);
const GO: Color = Color::srgb(0.13, 0.52, 0.27);
const GO_HOVER: Color = Color::srgb(0.17, 0.62, 0.33);
const TEXT: Color = Color::srgb(0.92, 0.93, 0.97);
const DIM: Color = Color::srgb(0.62, 0.64, 0.72);
const FAINT: Color = Color::srgb(0.38, 0.4, 0.5);
const GOOD: Color = Color::srgb(0.42, 0.88, 0.52);
const BAD: Color = Color::srgb(1.0, 0.47, 0.42);
const NOTE: Color = Color::srgb(1.0, 0.85, 0.35);

#[derive(Component, Clone, Copy, PartialEq)]
pub enum EdButton {
    Do(Action),
    /// The `k`th saved track.
    Saved(usize),
}

/// Text coloured to match its button's state.
#[derive(Component)]
pub struct Label(EdButton);

/// Text that changes.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum EdText {
    Name,
    Theme,
    ToolHelp(Tool),
    Stat(usize),
    Status,
    Problems,
    File,
    Save,
    Saved(usize),
    SavedNote(usize),
    NoSaved,
    Toast,
    Empty,
    Hint,
    Scale,
}

/// Boxes that come and go, or change colour or size.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum EdBox {
    Status,
    Stats,
    Toast,
    Empty,
    Hint,
    ScaleBar,
}

/// The scenery's colours: 0 for the ground, 1 for the road.
#[derive(Component)]
pub struct Swatch(usize);

#[derive(Component)]
pub struct Sidebar;

const STATS: [&str; 7] = ["lap", "points", "top", "items", "boosts", "fords", "ramps"];

fn font(px: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(px), ..default() }
}

fn text(p: &mut ChildSpawnerCommands, s: &str, size: f32, c: Color) {
    p.spawn((Text::new(s), font(size), TextColor(c)));
}

fn button_node() -> Node {
    Node {
        padding: UiRect::axes(px(10), px(5)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(6)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A button with one line of text, `grow`ing to share a row's width.
fn button(p: &mut ChildSpawnerCommands, label: &str, kind: EdButton, size: f32, grow: bool) {
    let node = if grow { Node { flex_grow: 1.0, flex_basis: px(0), ..button_node() } } else { button_node() };
    p.spawn((Button, node, BackgroundColor(BUTTON), BorderColor::all(Color::NONE), kind)).with_children(|b| {
        b.spawn((Text::new(label), font(size), TextColor(TEXT), Label(kind)));
    });
}

fn heading(p: &mut ChildSpawnerCommands, label: &str) {
    p.spawn((Text::new(label), font(12.0), TextColor(DIM), Node { margin: UiRect::top(px(7)), ..default() }));
}

fn row(p: &mut ChildSpawnerCommands, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: px(5), row_gap: px(5), align_items: AlignItems::Stretch, ..default() }).with_children(children);
}

/// A small coloured box, `w` by `h` pixels.
fn square(p: &mut ChildSpawnerCommands, w: f32, h: f32, c: Color, radius: f32) {
    p.spawn((Node { width: px(w), height: px(h), border_radius: BorderRadius::all(px(radius)), ..default() }, BackgroundColor(c)));
}

/// A little picture of what each tool makes, in the colours it's drawn on the map.
fn tool_icon(p: &mut ChildSpawnerCommands, t: Tool) {
    let frame = Node { width: px(18), height: px(18), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    p.spawn(frame).with_children(|p| match t {
        Tool::Draw => {
            p.spawn((
                Node { width: px(15), height: px(11), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(6)), ..default() },
                BorderColor::all(ORANGE),
            ));
        }
        Tool::Points => {
            p.spawn((
                Node { width: px(13), height: px(13), border: UiRect::all(px(2)), border_radius: BorderRadius::MAX, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BorderColor::all(Color::WHITE),
            ))
            .with_children(|p| square(p, 3.0, 3.0, Color::WHITE, 2.0));
        }
        Tool::Height => {
            p.spawn(Node { width: px(16), height: px(14), align_items: AlignItems::FlexEnd, column_gap: px(2), ..default() }).with_children(|p| {
                for h in [5.0, 9.0, 14.0] {
                    square(p, 4.0, h, Color::srgb(0.55, 0.85, 1.0), 1.0);
                }
            });
        }
        Tool::Items => {
            p.spawn(Node { width: px(15), height: px(15), flex_wrap: FlexWrap::Wrap, column_gap: px(1), row_gap: px(1), ..default() }).with_children(|p| {
                for k in 0..4 {
                    square(p, 7.0, 7.0, rainbow(k * 2), 1.5);
                }
            });
        }
        Tool::Boost => {
            p.spawn((
                Node { width: px(10), height: px(16), border_radius: BorderRadius::all(px(2)), justify_content: JustifyContent::Center, padding: UiRect::top(px(3)), ..default() },
                BackgroundColor(Color::srgb(1.0, 0.5, 0.08)),
            ))
            .with_children(|p| square(p, 4.0, 7.0, Color::srgb(1.0, 0.92, 0.35), 1.0));
        }
        Tool::Ford => square(p, 17.0, 10.0, Color::srgb(0.2, 0.5, 0.85), 5.0),
        Tool::Ramp | Tool::BoostRamp => {
            // The deck with its dark lip, and the arrow (orange on a boosted one).
            let arrow = if t == Tool::BoostRamp { Color::srgb(1.0, 0.55, 0.1) } else { Color::srgb(0.95, 0.97, 1.0) };
            p.spawn((
                Node { width: px(13), height: px(17), border: UiRect::top(px(3)), border_radius: BorderRadius::all(px(2)), justify_content: JustifyContent::Center, padding: UiRect::top(px(3)), ..default() },
                BackgroundColor(Color::srgb(0.2, 0.45, 0.95)),
                BorderColor::all(Color::srgb(0.08, 0.12, 0.3)),
            ))
            .with_children(|p| square(p, 5.0, 7.0, arrow, 1.0));
        }
        Tool::Start => {
            p.spawn(Node { width: px(15), height: px(15), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|p| {
                for k in 0..9 {
                    square(p, 5.0, 5.0, if k % 2 == 0 { Color::WHITE } else { Color::srgb(0.08, 0.08, 0.1) }, 0.0);
                }
            });
        }
    });
}

fn tool_button(p: &mut ChildSpawnerCommands, t: Tool, k: usize) {
    let kind = EdButton::Do(Action::Tool(t));
    let node = Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Stretch, padding: UiRect::axes(px(8), px(3)), border: UiRect::all(px(2)), ..button_node() };
    p.spawn((Button, node, BackgroundColor(BUTTON), BorderColor::all(Color::NONE), kind)).with_children(|b| {
        b.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(|r| {
            r.spawn((
                Node { width: px(19), height: px(19), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border_radius: BorderRadius::all(px(4)), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
            ))
            .with_children(|b| text(b, &k.to_string(), 12.0, DIM));
            tool_icon(r, t);
            r.spawn((Text::new(t.name()), font(16.0), TextColor(TEXT), Label(kind)));
        });
        b.spawn((Text::new(t.help()), font(12.0), TextColor(Color::srgb(0.85, 0.8, 0.74)), Node { margin: UiRect::new(px(2), px(0), px(5), px(2)), ..default() }, EdText::ToolHelp(t)));
    });
}

pub(super) fn spawn_ui(mut commands: Commands, cam: Query<Entity, With<EditorCamera>>) {
    let Ok(cam) = cam.single() else { return };
    let card = |gap: f32| Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(px(10), px(8)), row_gap: px(gap), border_radius: BorderRadius::all(px(7)), ..default() };
    commands
        .spawn((
            Node {
                width: px(SIDEBAR),
                height: percent(100),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                border: UiRect::right(px(3)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(ORANGE),
            UiTargetCamera(cam),
            DespawnOnExit(AppState::Editor),
        ))
        .with_children(|p| {
            // Everything but saving and racing scrolls if it doesn't fit.
            let column = Node { flex_direction: FlexDirection::Column, row_gap: px(4), padding: UiRect::axes(px(14), px(12)), ..default() };
            p.spawn((Node { flex_grow: 1.0, min_height: px(0), overflow: Overflow::scroll_y(), ..column.clone() }, Sidebar)).with_children(|p| sidebar(p, card));
            p.spawn((Node { border: UiRect::top(px(1)), padding: UiRect::new(px(14), px(14), px(8), px(12)), ..column }, BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.08)))).with_children(footer);
        });
    overlays(&mut commands, cam);
}

/// The sidebar's scrolling part.
fn sidebar(p: &mut ChildSpawnerCommands, card: impl Fn(f32) -> Node) {
    p.spawn((Text::new("Track editor"), font(27.0), TextColor(ORANGE), TextShadow::default(), Node { margin: UiRect::bottom(px(4)), ..default() }));

    // The name, as a box to type in.
    let name = EdButton::Do(Action::Name);
    let node = Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexStart, padding: UiRect::axes(px(10), px(5)), row_gap: px(1), ..button_node() };
    p.spawn((Button, node, BackgroundColor(FIELD), BorderColor::all(FIELD_EDGE), name)).with_children(|b| {
        text(b, "NAME (click to rename)", 11.0, FAINT);
        b.spawn((Text::new(""), font(18.0), TextColor(TEXT), Label(name), EdText::Name));
    });

    // The scenery, with a sample of its colours.
    row(p, |r| {
        button(r, "<", EdButton::Do(Action::Theme(-1)), 16.0, false);
        r.spawn((
            Node { flex_grow: 1.0, align_items: AlignItems::Center, column_gap: px(9), padding: UiRect::axes(px(10), px(5)), border_radius: BorderRadius::all(px(6)), ..default() },
            BackgroundColor(CARD),
        ))
        .with_children(|m| {
            m.spawn((
                Node { width: px(22), height: px(16), border_radius: BorderRadius::all(px(3)), justify_content: JustifyContent::Center, ..default() },
                BackgroundColor(Color::BLACK),
                Swatch(0),
            ))
            .with_children(|s| {
                s.spawn((Node { width: px(6), height: percent(100), ..default() }, BackgroundColor(Color::BLACK), Swatch(1)));
            });
            m.spawn((Text::new(""), font(16.0), TextColor(TEXT), EdText::Theme));
            text(m, "scenery", 12.0, DIM);
        });
        button(r, ">", EdButton::Do(Action::Theme(1)), 16.0, false);
    });

    heading(p, "TOOLS");
    for (k, t) in Tool::ALL.into_iter().enumerate() {
        tool_button(p, t, k + 1);
    }

    heading(p, "EDIT");
    row(p, |r| {
        button(r, "Undo", EdButton::Do(Action::Undo), 14.0, true);
        button(r, "Redo", EdButton::Do(Action::Redo), 14.0, true);
        button(r, "Smooth", EdButton::Do(Action::Smooth), 14.0, true);
        button(r, "Reverse", EdButton::Do(Action::Reverse), 14.0, true);
    });

    heading(p, "THE LAP");
    p.spawn((Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, ..card(4.0) }, BackgroundColor(CARD), EdBox::Stats)).with_children(|c| {
        for (k, what) in STATS.into_iter().enumerate() {
            c.spawn(Node { width: percent(if k < 3 { 33.3 } else { 25.0 }), align_items: AlignItems::Baseline, column_gap: px(5), ..default() }).with_children(|s| {
                s.spawn((Text::new("-"), font(15.0), TextColor(TEXT), EdText::Stat(k)));
                text(s, what, 11.0, DIM);
            });
        }
    });
    p.spawn((Node { border: UiRect::left(px(3)), ..card(4.0) }, BackgroundColor(CARD), BorderColor::all(FAINT), EdBox::Status)).with_children(|c| {
        c.spawn((Text::new(""), font(15.0), TextColor(TEXT), EdText::Status));
        c.spawn((Text::new(""), font(12.5), TextColor(Color::srgb(1.0, 0.72, 0.68)), EdText::Problems));
    });

    heading(p, "YOUR TRACKS");
    p.spawn((Text::new(""), font(12.5), TextColor(FAINT), EdText::NoSaved));
    for k in 0..MAX_SAVED {
        let kind = EdButton::Saved(k);
        let node = Node { justify_content: JustifyContent::SpaceBetween, padding: UiRect::axes(px(10), px(4)), ..button_node() };
        p.spawn((Button, node, BackgroundColor(BUTTON), BorderColor::all(Color::NONE), kind)).with_children(|b| {
            b.spawn((Text::new(""), font(14.0), TextColor(TEXT), Label(kind), EdText::Saved(k)));
            b.spawn((Text::new(""), font(11.0), TextColor(BAD), EdText::SavedNote(k)));
        });
    }
}

/// Saving and racing, always in view at the bottom.
fn footer(p: &mut ChildSpawnerCommands) {
    p.spawn((Text::new(""), font(12.0), TextColor(DIM), EdText::File));
    row(p, |r| {
        let save = EdButton::Do(Action::Save);
        r.spawn((Button, Node { flex_grow: 2.0, flex_basis: px(0), ..button_node() }, BackgroundColor(BUTTON), BorderColor::all(Color::NONE), save)).with_children(|b| {
            b.spawn((Text::new("Save"), font(15.0), TextColor(TEXT), Label(save), EdText::Save));
        });
        button(r, "New", EdButton::Do(Action::New), 15.0, true);
        button(r, "Menu", EdButton::Do(Action::Leave), 15.0, true);
    });
    let go = EdButton::Do(Action::TestDrive);
    let node = Node { padding: UiRect::axes(px(10), px(7)), margin: UiRect::top(px(2)), column_gap: px(10), ..button_node() };
    p.spawn((Button, node, BackgroundColor(GO), BorderColor::all(Color::NONE), go)).with_children(|b| {
        b.spawn((Text::new("Test drive"), font(19.0), TextColor(TEXT), Label(go)));
        b.spawn((Text::new("F5"), font(12.0), TextColor(TEXT), Label(go)));
    });
}

/// The things floating over the map.
fn overlays(commands: &mut Commands, cam: Entity) {
    // A message at the top of the map.
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: px(SIDEBAR), right: px(0), top: px(14), justify_content: JustifyContent::Center, ..default() },
            UiTargetCamera(cam),
            DespawnOnExit(AppState::Editor),
        ))
        .with_children(|p| {
            p.spawn((
                Node { padding: UiRect::axes(px(16), px(8)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(8)), ..default() },
                BackgroundColor(PANEL),
                BorderColor::all(NOTE),
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), px(0), px(3), px(0), px(10)),
                EdBox::Toast,
            ))
            .with_children(|t| {
                t.spawn((Text::new(""), font(16.0), TextColor(NOTE), EdText::Toast));
            });
        });

    // What to do while there's no track.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(SIDEBAR),
                right: px(0),
                top: px(0),
                bottom: px(0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            UiTargetCamera(cam),
            DespawnOnExit(AppState::Editor),
            EdBox::Empty,
        ))
        .with_children(|p| {
            p.spawn((
                Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(10), padding: UiRect::axes(px(34), px(24)), border_radius: BorderRadius::all(px(14)), ..default() },
                BackgroundColor(Color::srgba(0.04, 0.05, 0.11, 0.72)),
            ))
            .with_children(|c| {
                c.spawn((Text::new("Draw a track"), font(36.0), TextColor(ORANGE), TextShadow::default()));
                c.spawn((Text::new(""), font(17.0), TextColor(TEXT), TextLayout::justify(Justify::Center), EdText::Empty));
                c.spawn((
                    Text::new("It becomes a road you can reshape, raise into bridges\nand fill with item boxes, boost pads and fords."),
                    font(14.0),
                    TextColor(DIM),
                    TextLayout::justify(Justify::Center),
                ));
            });
        });

    // Bottom left: what the button under the mouse does, and the keyboard shortcuts.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(SIDEBAR + 12.0),
                right: px(300),
                bottom: px(12),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(6),
                ..default()
            },
            UiTargetCamera(cam),
            DespawnOnExit(AppState::Editor),
        ))
        .with_children(|p| {
            p.spawn((
                Node { padding: UiRect::axes(px(10), px(6)), border_radius: BorderRadius::all(px(6)), border: UiRect::left(px(3)), ..default() },
                BackgroundColor(PANEL),
                BorderColor::all(ORANGE),
                EdBox::Hint,
            ))
            .with_children(|h| {
                h.spawn((Text::new(""), font(14.0), TextColor(TEXT), EdText::Hint));
            });
            p.spawn((
                Node { flex_wrap: FlexWrap::Wrap, column_gap: px(14), row_gap: px(5), padding: UiRect::axes(px(10), px(6)), border_radius: BorderRadius::all(px(6)), ..default() },
                BackgroundColor(Color::srgba(0.04, 0.05, 0.11, 0.8)),
            ))
            .with_children(|bar| {
                for (key, what) in [("Scroll", "zoom"), ("Middle-drag", "move"), ("F", "fit"), ("Ctrl+Z", "undo"), ("Ctrl+S", "save"), ("F5", "test drive"), ("Esc", "menu")] {
                    bar.spawn(Node { align_items: AlignItems::Center, column_gap: px(5), ..default() }).with_children(|c| {
                        c.spawn((
                            Node { padding: UiRect::axes(px(5), px(1)), border_radius: BorderRadius::all(px(4)), ..default() },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.13)),
                        ))
                        .with_children(|k| text(k, key, 12.5, TEXT));
                        text(c, what, 12.5, DIM);
                    });
                }
            });
        });

    // Bottom right: the scale and zoom buttons.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                bottom: px(12),
                align_items: AlignItems::Center,
                column_gap: px(5),
                padding: UiRect::all(px(5)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.11, 0.8)),
            UiTargetCamera(cam),
            DespawnOnExit(AppState::Editor),
        ))
        .with_children(|p| {
            p.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, margin: UiRect::axes(px(8), px(0)), ..default() }).with_children(|s| {
                s.spawn((Text::new(""), font(12.0), TextColor(TEXT), EdText::Scale));
                s.spawn((Node { width: px(50), height: px(6), border: UiRect::new(px(2), px(2), px(0), px(2)), ..default() }, BorderColor::all(TEXT), EdBox::ScaleBar));
            });
            button(p, "-", EdButton::Do(Action::Zoom(1.25)), 16.0, false);
            button(p, "+", EdButton::Do(Action::Zoom(0.8)), 16.0, false);
            button(p, "Fit", EdButton::Do(Action::Fit), 14.0, false);
        });
}

/// Clicks on the buttons.
#[allow(clippy::too_many_arguments)]
pub(super) fn buttons(
    mut commands: Commands,
    pressed: Query<(&Interaction, &EdButton), Changed<Interaction>>,
    mut doc: ResMut<Doc>,
    mut st: ResMut<EditorState>,
    mut settings: ResMut<Settings>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, b) in &pressed {
        if *i != Interaction::Pressed {
            continue;
        }
        match *b {
            EdButton::Do(a) => act(a, &mut doc, &mut st, &mut commands, &mut settings, &mut next),
            EdButton::Saved(k) => {
                if let Some((file, _)) = custom::saved_files().get(k) {
                    open(&mut doc, &mut st, file);
                }
            }
        }
    }
}

/// The mouse wheel scrolls the sidebar when it's over it (it zooms the map everywhere else).
pub(super) fn scroll_sidebar(
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sidebar: Query<&mut ScrollPosition, With<Sidebar>>,
) {
    let over = windows.single().ok().and_then(|w| w.cursor_position()).is_some_and(|p| p.x <= SIDEBAR);
    let px = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y * 40.0,
        MouseScrollUnit::Pixel => scroll.delta.y,
    };
    if over && px != 0.0 {
        for mut pos in &mut sidebar {
            pos.y = (pos.y - px).max(0.0);
        }
    }
}

/// Whether a button is picked (the tool in use, say), whether it would do anything now, and
/// whether it's the big green one.
fn state(b: EdButton, doc: &Doc, st: &EditorState, saved: &[(String, crate::sim::TrackId)]) -> (bool, bool, bool) {
    let track = !doc.map.is_empty();
    match b {
        EdButton::Do(Action::Tool(t)) => (st.tool == t, true, false),
        EdButton::Do(Action::Name) => (st.naming, true, false),
        EdButton::Do(Action::Undo) => (false, !doc.undo.is_empty(), false),
        EdButton::Do(Action::Redo) => (false, !doc.redo.is_empty(), false),
        EdButton::Do(Action::Smooth | Action::Reverse | Action::Fit) => (false, track, false),
        EdButton::Do(Action::Save) => (false, track && (doc.dirty() || doc.file.is_none()), false),
        EdButton::Do(Action::New) => (false, track || doc.file.is_some(), false),
        EdButton::Do(Action::TestDrive) => (false, doc.raceable(), true),
        EdButton::Saved(k) => (saved.get(k).is_some_and(|(f, _)| doc.file.as_ref() == Some(f)), true, false),
        EdButton::Do(_) => (false, true, false),
    }
}

/// Buttons' colours, and the saved tracks' buttons coming and going.
pub(super) fn style_buttons(
    doc: Res<Doc>,
    st: Res<EditorState>,
    mut buttons: Query<(&EdButton, &Interaction, &mut BackgroundColor, &mut BorderColor, &mut Node)>,
    mut labels: Query<(&Label, &mut TextColor)>,
) {
    let saved = custom::saved_files();
    for (&b, interaction, mut bg, mut edge, mut node) in &mut buttons {
        let (picked, on, go) = state(b, &doc, &st, &saved);
        let hover = on && *interaction != Interaction::None;
        let field = b == EdButton::Do(Action::Name);
        let colour = if go && on {
            [GO, GO_HOVER][hover as usize]
        } else if picked {
            [PICKED, PICKED_HOVER][hover as usize]
        } else if field {
            FIELD
        } else if !on {
            BUTTON_OFF
        } else {
            [BUTTON, BUTTON_HOVER][hover as usize]
        };
        bg.set_if_neq(BackgroundColor(colour));
        let e = if picked { ORANGE } else if field { FIELD_EDGE } else { Color::NONE };
        edge.set_if_neq(BorderColor::all(e));
        if let EdButton::Saved(k) = b {
            let display = if k < saved.len() { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
    }
    for (label, mut colour) in &mut labels {
        let (picked, on, go) = state(label.0, &doc, &st, &saved);
        let c = if !on { FAINT } else if picked && !go { ORANGE } else { TEXT };
        colour.set_if_neq(TextColor(c));
    }
}

/// What the buttons do, for the hint over the map.
fn describe(b: EdButton) -> String {
    let s = match b {
        EdButton::Do(a) => match a {
            Action::Name => "Click, then type a new name. Enter when you're done.",
            Action::Theme(_) => "Change the scenery round the track.",
            Action::Tool(t) => return format!("{}: press {}.", t.name(), Tool::ALL.iter().position(|&x| x == t).unwrap() + 1),
            Action::Undo => "Undo the last change (Ctrl+Z).",
            Action::Redo => "Redo what was undone (Ctrl+Y).",
            Action::Smooth => "Ease every bend a little, to fix corners that are too tight.",
            Action::Reverse => "Race the other way round.",
            Action::New => "Start again with a blank map.",
            Action::Save => "Save the track (Ctrl+S). Saved tracks are in the menu's track list.",
            Action::TestDrive => "Race it now (F5). Leaving the race brings you back here.",
            Action::Leave => "Back to the menu (Esc).",
            Action::Fit => "Fit the whole track in view (F).",
            Action::Zoom(f) if f < 1.0 => "Zoom in (or scroll over the map).",
            Action::Zoom(_) => "Zoom out (or scroll over the map).",
        },
        EdButton::Saved(_) => "Open this track.",
    };
    s.into()
}

#[allow(clippy::type_complexity)]
pub(super) fn update_ui(
    time: Res<Time>,
    doc: Res<Doc>,
    st: Res<EditorState>,
    hovered: Query<(&EdButton, &Interaction)>,
    mut texts: Query<(&EdText, &mut Text, &mut TextColor, &mut Node), Without<EdBox>>,
    mut boxes: Query<(&EdBox, &mut Node, &mut BorderColor), Without<EdText>>,
    mut swatches: Query<(&Swatch, &mut BackgroundColor)>,
) {
    let saved = custom::saved_files();
    let caret = if st.naming && time.elapsed_secs().fract() < 0.5 { "_" } else { "" };
    let hint = hovered.iter().find(|(_, i)| **i != Interaction::None).map(|(b, _)| describe(*b));
    let problems = doc.built.as_ref().map(|(_, p)| p);
    let empty = doc.map.is_empty() && st.stroke.is_empty();
    let grid = view::grid_step(st.zoom);

    for (kind, mut text, mut colour, mut node) in &mut texts {
        let mut c = None;
        let s = match *kind {
            EdText::Name => format!("{}{caret}", doc.map.name),
            EdText::Theme => doc.map.theme.name().into(),
            EdText::ToolHelp(t) if t == st.tool => t.help().into(),
            EdText::ToolHelp(_) => String::new(),
            EdText::Stat(k) => match &doc.built {
                None => "-".into(),
                Some((b, _)) => {
                    let t = &b.track;
                    match k {
                        0 => format!("{:.2}km", t.length / 1000.0),
                        1 => doc.map.points.len().to_string(),
                        2 => format!("{:.1}m", t.samples.iter().map(|s| s.height).fold(0.0, f32::max)),
                        3 => t.item_rows.len().to_string(),
                        4 => t.boost_pads.len().to_string(),
                        5 => t.water.len().to_string(),
                        _ => t.ramps.len().to_string(),
                    }
                }
            },
            EdText::Status => match problems {
                None => {
                    c = Some(DIM);
                    "No track yet.".into()
                }
                Some(p) if p.ok() => {
                    c = Some(GOOD);
                    "Ready to race!".into()
                }
                Some(p) => {
                    c = Some(BAD);
                    match p.messages.len() {
                        1 => "1 thing to fix before racing:".into(),
                        n => format!("{n} things to fix before racing:"),
                    }
                }
            },
            EdText::Problems => match problems {
                Some(p) if !p.ok() => p.messages.iter().map(|m| format!("- {m}")).collect::<Vec<_>>().join("\n"),
                _ => String::new(),
            },
            EdText::File => match &doc.file {
                Some(f) if doc.dirty() => {
                    c = Some(NOTE);
                    format!("{f}.track: unsaved changes")
                }
                Some(f) => {
                    c = Some(DIM);
                    format!("Saved as {f}.track")
                }
                None if doc.map.is_empty() => String::new(),
                None => {
                    c = Some(NOTE);
                    "Not saved yet".into()
                }
            },
            EdText::Save => if doc.dirty() && doc.file.is_some() { "Save changes" } else { "Save" }.into(),
            EdText::Saved(k) => saved.get(k).map_or(String::new(), |(_, id)| id.name()),
            EdText::SavedNote(k) => match saved.get(k) {
                Some((_, crate::sim::TrackId::Custom(h))) if !custom::raceable(*h) => "needs fixing".into(),
                _ => String::new(),
            },
            EdText::NoSaved => if saved.is_empty() { "None saved yet." } else { "" }.into(),
            EdText::Toast => st.message.clone(),
            EdText::Empty if st.tool == Tool::Draw => "Hold the left mouse button and draw a loop,\nin the direction you'll race.".into(),
            EdText::Empty => "Pick Draw (press 1), then hold the left mouse\nbutton and draw a loop.".into(),
            EdText::Hint => hint.clone().unwrap_or_default(),
            EdText::Scale if grid >= 1000.0 => format!("{}km", grid / 1000.0),
            EdText::Scale => format!("{grid}m"),
        };
        if let Some(c) = c {
            colour.set_if_neq(TextColor(c));
        }
        let display = if s.is_empty() { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
        if text.0 != s {
            text.0 = s;
        }
    }

    for (kind, mut node, mut edge) in &mut boxes {
        let show = match kind {
            EdBox::Status => {
                let c = match problems {
                    None => FAINT,
                    Some(p) if p.ok() => GOOD,
                    Some(_) => BAD,
                };
                edge.set_if_neq(BorderColor::all(c));
                true
            }
            EdBox::Stats => doc.built.is_some(),
            EdBox::Toast => !st.message.is_empty(),
            EdBox::Empty => empty,
            EdBox::Hint => hint.is_some(),
            EdBox::ScaleBar => {
                let w = px(grid / st.zoom);
                if node.width != w {
                    node.width = w;
                }
                true
            }
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }

    let (ground, _, _, road) = view::palette(doc.map.theme);
    for (s, mut bg) in &mut swatches {
        bg.set_if_neq(BackgroundColor(if s.0 == 0 { ground } else { road }));
    }
}
