//! Main menu and LAN lobby screens (keyboard / gamepad driven). The 3D backdrop behind them
//! previews whichever track is selected.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::text::FontSize;

use crate::game::{start_host, start_join, start_solo, AppState, MenuStatus, SessionRes, Settings};
use crate::input::{read_nav, StickNav};
use crate::net::{local_ip, DEFAULT_PORT};
use crate::render::gfx::{DlssQuality, Graphics, RenderRes};
use crate::session::{Role, MAX_LOCAL_PLAYERS};
use crate::sim::TrackId;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuCursor>()
            .init_resource::<LobbyCursor>()
            .add_systems(OnEnter(AppState::Menu), spawn_menu)
            .add_systems(OnEnter(AppState::Lobby), spawn_lobby)
            .add_systems(Update, (menu_input, menu_render).chain().run_if(in_state(AppState::Menu)))
            .add_systems(Update, (lobby_input, lobby_render).chain().run_if(in_state(AppState::Lobby)));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum MenuItem {
    Solo,
    Players,
    Host,
    Join,
    Track,
    Editor,
    Class,
    Bots,
    Name,
    Address,
    Graphics,
    Dlss,
    Resolution,
    Fps,
    Quit,
}

const ITEMS: [MenuItem; 15] = [
    MenuItem::Solo,
    MenuItem::Players,
    MenuItem::Host,
    MenuItem::Join,
    MenuItem::Track,
    MenuItem::Editor,
    MenuItem::Class,
    MenuItem::Bots,
    MenuItem::Name,
    MenuItem::Address,
    MenuItem::Graphics,
    MenuItem::Dlss,
    MenuItem::Resolution,
    MenuItem::Fps,
    MenuItem::Quit,
];

#[derive(Resource, Default)]
struct MenuCursor(usize);

#[derive(Component)]
struct MenuLine(usize);

#[derive(Component)]
struct TrackBlurb;

#[derive(Component)]
enum LobbyText {
    Title,
    Info,
    Track,
    Class,
    Players,
    Bots,
    Help,
}

/// Host lobby rows that Left/Right change.
const LOBBY_TRACK: usize = 0;
const LOBBY_CLASS: usize = 1;
const LOBBY_BOTS: usize = 2;

#[derive(Resource, Default)]
struct LobbyCursor(usize);

#[derive(Component)]
struct StatusText;

pub const ORANGE: Color = Color::srgb(1.0, 0.62, 0.2);
const HILITE: Color = Color::srgb(1.0, 0.85, 0.3);

fn font(px: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(px), ..default() }
}

fn blurb(t: TrackId) -> String {
    let text = match t {
        TrackId::Circuit => "A 2km Grand Prix circuit: a flyover, the hairpin, a Ferris wheel\nand six hundred cheering corgis.",
        TrackId::Prism => "Rainbow glass tiles, neon arches and a ford through the canal\nat night. Looks best with ray tracing.",
        TrackId::Meadow => "The original countryside loop. Short, sweet and full of bones.",
        TrackId::Lagoon => "Golden hour on the Riviera: a mirror-lined tunnel, a causeway over\nthe lagoon and a chrome corgi. Made for ray tracing.",
        TrackId::Jungle => "Midday in a lost temple city: sunbeams through the canopy, a hall of\ntorches and gold, a river to jump and a jade corgi.",
        TrackId::Tbilisi => "Blue hour in old Tbilisi: the glittering Bridge of Peace, carved\nbalconies, the sulphur baths and Narikala floodlit above.",
        TrackId::Custom(_) => {
            let len = crate::sim::Track::new(t).length;
            return format!("A custom track, {:.1}km, with {} scenery.\nChange it in the track editor.", len / 1000.0, t.theme().name());
        }
    };
    text.to_string()
}

fn panel(commands: &mut Commands, state: AppState, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    commands
        .spawn((
            Node { width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            DespawnOnExit(state),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(48), px(24)),
                    row_gap: px(5),
                    min_width: px(560),
                    border: UiRect::top(px(6)),
                    border_radius: BorderRadius::all(px(22)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.05, 0.11, 0.78)),
                BorderColor::all(ORANGE),
            ))
            .with_children(children);
        });
}

fn spawn_menu(mut commands: Commands, cli: Res<crate::game::Cli>) {
    if cli.view.is_some() {
        return;
    }
    panel(&mut commands, AppState::Menu, |p| {
        p.spawn((Text::new("corgiKart"), font(64.0), TextColor(ORANGE), TextShadow::default()));
        p.spawn((Text::new("a very good kart racer"), font(20.0), TextColor(Color::srgb(0.8, 0.8, 0.85))));
        p.spawn(Node { height: px(12), ..default() });
        for i in 0..ITEMS.len() {
            p.spawn((Text::new(""), font(24.0), TextColor(Color::WHITE), MenuLine(i)));
        }
        p.spawn(Node { height: px(6), ..default() });
        p.spawn((Text::new(""), font(16.0), TextColor(Color::srgb(0.75, 0.85, 1.0)), TextLayout::justify(Justify::Center), TrackBlurb));
        p.spawn((Text::new(""), font(18.0), TextColor(Color::srgb(1.0, 0.5, 0.45)), StatusText));
        p.spawn((
            Text::new("Up/Down: select   Enter: confirm   Left/Right: change   type to edit"),
            font(15.0),
            TextColor(Color::srgb(0.6, 0.6, 0.68)),
        ));
    });
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    stick: Res<StickNav>,
    mut typed: MessageReader<KeyboardInput>,
    mut cursor: ResMut<MenuCursor>,
    mut settings: ResMut<Settings>,
    mut status: ResMut<MenuStatus>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    let nav = read_nav(&keys, &pads, &stick);
    let n = ITEMS.len();
    if nav.up {
        cursor.0 = (cursor.0 + n - 1) % n;
    }
    if nav.down {
        cursor.0 = (cursor.0 + 1) % n;
    }
    let item = ITEMS[cursor.0];

    // Text entry for name / address
    let editing = match item {
        MenuItem::Name => Some(&mut settings.name),
        MenuItem::Address => Some(&mut settings.join_addr),
        _ => None,
    };
    if let Some(field) = editing {
        for ev in typed.read() {
            if !ev.state.is_pressed() {
                continue;
            }
            match &ev.logical_key {
                Key::Backspace => {
                    field.pop();
                }
                Key::Character(c) if field.chars().count() < 24 => {
                    field.extend(c.chars().filter(|ch| !ch.is_control() && *ch != '='));
                }
                _ => {}
            }
        }
    } else {
        typed.clear();
    }

    let step = if nav.left { -1 } else if nav.right || nav.confirm { 1 } else { 0 };
    if step != 0 {
        match item {
            MenuItem::Bots => settings.bots = (settings.bots as i32 + step).rem_euclid(8) as u8,
            MenuItem::Players => {
                settings.players = (settings.players as i32 - 1 + step).rem_euclid(MAX_LOCAL_PLAYERS as i32) as u8 + 1;
                settings.save();
            }
            MenuItem::Track => {
                settings.track = settings.track.cycle(step);
                settings.save();
            }
            MenuItem::Class => {
                settings.class = settings.class.cycle(step);
                settings.save();
            }
            MenuItem::Fps => {
                settings.fps = !settings.fps;
                settings.save();
            }
            MenuItem::Dlss => {
                settings.dlss_quality = settings.dlss_quality.cycle(step);
                settings.save();
            }
            MenuItem::Resolution => {
                settings.render_res = settings.render_res.cycle(step);
                settings.save();
            }
            MenuItem::Graphics => {
                let want = !settings.raytracing;
                if want {
                    match crate::render::gfx::probe_raytracing() {
                        Ok(_) => status.0.clear(),
                        Err(e) => {
                            status.0 = format!("Ray tracing isn't available: {e}");
                            return;
                        }
                    }
                }
                settings.raytracing = want;
                settings.save();
            }
            _ => {}
        }
    }

    if !nav.confirm {
        return;
    }
    if settings.name.trim().is_empty() {
        settings.name = "Corgi".into();
    }
    settings.save();
    let unraceable = matches!(settings.track, TrackId::Custom(h) if !crate::sim::custom::raceable(h));
    if unraceable && matches!(item, MenuItem::Solo | MenuItem::Host) {
        status.0 = "That track has problems: fix them in the track editor first.".into();
        return;
    }
    let result = match item {
        MenuItem::Editor => {
            status.0.clear();
            next.set(AppState::Editor);
            Ok(())
        }
        MenuItem::Solo => {
            status.0.clear();
            start_solo(&mut commands, &settings, &mut next, false);
            Ok(())
        }
        MenuItem::Host => start_host(&mut commands, &settings, DEFAULT_PORT, &mut next, false),
        MenuItem::Join => start_join(&mut commands, &settings, &mut next, false),
        MenuItem::Quit => {
            exit.write(AppExit::Success);
            Ok(())
        }
        MenuItem::Name | MenuItem::Address => {
            cursor.0 += 1;
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = result {
        status.0 = e;
    }
}

#[allow(clippy::type_complexity)]
fn menu_render(
    time: Res<Time>,
    cursor: Res<MenuCursor>,
    settings: Res<Settings>,
    gfx: Res<Graphics>,
    status: Res<MenuStatus>,
    mut lines: Query<(&MenuLine, &mut Text, &mut TextColor), (Without<StatusText>, Without<TrackBlurb>)>,
    mut status_text: Query<&mut Text, (With<StatusText>, Without<TrackBlurb>)>,
    mut blurb_text: Query<&mut Text, With<TrackBlurb>>,
    mut blurb_for: Local<Option<(TrackId, String)>>,
) {
    let caret = if time.elapsed_secs().fract() < 0.5 { "_" } else { " " };
    for (line, mut text, mut color) in &mut lines {
        let sel = line.0 == cursor.0;
        let caret = if sel { caret } else { "" };
        let label = match ITEMS[line.0] {
            MenuItem::Solo if settings.players > 1 => "Split-screen race".to_string(),
            MenuItem::Solo => "Solo race".to_string(),
            MenuItem::Players if settings.players > 1 => format!("Players: < {} >  (split screen)", settings.players),
            MenuItem::Players => format!("Players: < {} >", settings.players),
            MenuItem::Host => "Host LAN game".to_string(),
            MenuItem::Join => "Join LAN game".to_string(),
            MenuItem::Track => format!("Track: < {} >", settings.track.name()),
            MenuItem::Editor => "Track editor".to_string(),
            MenuItem::Class => format!("Class: < {} >", settings.class.name()),
            MenuItem::Bots => format!("CPU racers: < {} >", settings.bots),
            MenuItem::Name => format!("Name: {}{caret}", settings.name),
            MenuItem::Address => format!("Host address: {}{caret}", settings.join_addr),
            MenuItem::Graphics => {
                let mode = match (settings.raytracing, gfx.raytracing && gfx.dlss) {
                    (false, _) => "Standard",
                    (true, true) => "Ray traced (DLSS)",
                    (true, false) if cfg!(feature = "dlss") => "Ray traced",
                    (true, false) => "Ray traced (noisy: no DLSS build)",
                };
                let note = if settings.raytracing != gfx.raytracing { "  - restart to apply" } else { "" };
                format!("Graphics: < {mode} >{note}")
            }
            MenuItem::Dlss => {
                let (name, scale) = match settings.dlss_quality {
                    DlssQuality::Quality => ("Quality", "67%"),
                    DlssQuality::Balanced => ("Balanced", "58%"),
                    DlssQuality::Performance => ("Performance", "50%"),
                };
                let note = if settings.dlss_quality != gfx.dlss_quality { "  - restart to apply" } else { "" };
                format!("DLSS (ray tracing): < {name}, {scale} >{note}")
            }
            MenuItem::Resolution => {
                let name = match settings.render_res {
                    RenderRes::Native => "Native",
                    RenderRes::P1080 => "1080p",
                    RenderRes::P720 => "720p",
                };
                let note = if settings.render_res != gfx.render_res { "  - restart to apply" } else { "" };
                format!("Render resolution: < {name} >{note}")
            }
            MenuItem::Fps => format!("FPS counter: < {} >   (F3)", if settings.fps { "On" } else { "Off" }),
            MenuItem::Quit => "Quit".to_string(),
        };
        text.0 = if sel { format!("> {label} <") } else { label };
        color.0 = if sel { HILITE } else { Color::WHITE };
    }
    for mut t in &mut status_text {
        t.0 = status.0.clone();
    }
    // (A custom track's blurb means building it, so only redo it when the track changes.)
    if blurb_for.as_ref().is_none_or(|(id, _)| *id != settings.track) {
        *blurb_for = Some((settings.track, blurb(settings.track)));
    }
    for mut t in &mut blurb_text {
        let b = &blurb_for.as_ref().unwrap().1;
        if t.0 != *b {
            t.0 = b.clone();
        }
    }
}

fn spawn_lobby(mut commands: Commands) {
    panel(&mut commands, AppState::Lobby, |p| {
        p.spawn((Text::new(""), font(44.0), TextColor(ORANGE), TextShadow::default(), LobbyText::Title));
        p.spawn((Text::new(""), font(20.0), TextColor(Color::srgb(0.85, 0.9, 1.0)), LobbyText::Info));
        p.spawn(Node { height: px(10), ..default() });
        p.spawn((Text::new(""), font(26.0), TextColor(Color::WHITE), LobbyText::Track));
        p.spawn((Text::new(""), font(26.0), TextColor(Color::WHITE), LobbyText::Class));
        p.spawn((Text::new(""), font(26.0), TextColor(Color::WHITE), LobbyText::Players));
        p.spawn((Text::new(""), font(22.0), TextColor(Color::srgb(0.8, 0.8, 0.8)), LobbyText::Bots));
        p.spawn(Node { height: px(10), ..default() });
        p.spawn((Text::new(""), font(18.0), TextColor(HILITE), LobbyText::Help));
    });
}

#[allow(clippy::too_many_arguments)]
fn lobby_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    stick: Res<StickNav>,
    session: Option<ResMut<SessionRes>>,
    mut settings: ResMut<Settings>,
    mut cursor: ResMut<LobbyCursor>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut session) = session else {
        next.set(AppState::Menu);
        return;
    };
    let nav = read_nav(&keys, &pads, &stick);
    if nav.back {
        session.0.leave();
        commands.remove_resource::<SessionRes>();
        next.set(AppState::Menu);
        return;
    }
    if let Role::Host(_) = session.0.role {
        let max = session.0.max_bots();
        if nav.up {
            cursor.0 = (cursor.0 + 2) % 3;
        }
        if nav.down {
            cursor.0 = (cursor.0 + 1) % 3;
        }
        let step = if nav.left { -1 } else if nav.right { 1 } else { 0 };
        if step != 0 {
            let s = &mut session.0;
            match cursor.0 {
                LOBBY_TRACK => s.track = s.track.cycle(step),
                LOBBY_CLASS => s.class = s.class.cycle(step),
                _ => s.bots = (s.bots as i32 + step).clamp(0, max as i32) as u8,
            }
        }
        session.0.bots = session.0.bots.min(max);
        if settings.bots != session.0.bots || settings.track != session.0.track || settings.class != session.0.class {
            settings.bots = session.0.bots;
            settings.track = session.0.track;
            settings.class = session.0.class;
            settings.save();
        }
        if nav.confirm {
            session.0.start_race();
            next.set(AppState::Race);
        }
    }
}

fn lobby_render(session: Option<Res<SessionRes>>, cursor: Res<LobbyCursor>, mut texts: Query<(&LobbyText, &mut Text, &mut TextColor)>, mut ip: Local<Option<String>>) {
    let Some(session) = session else { return };
    let s = &session.0;
    let ip = ip.get_or_insert_with(|| local_ip().map_or("<your LAN IP>".into(), |i| i.to_string()));
    let players = s.human_names();
    let host = matches!(s.role, Role::Host(_));
    for (kind, mut text, mut color) in &mut texts {
        // Highlight the row the host is editing.
        let row = match kind {
            LobbyText::Track => Some(LOBBY_TRACK),
            LobbyText::Class => Some(LOBBY_CLASS),
            LobbyText::Bots => Some(LOBBY_BOTS),
            _ => None,
        };
        if let Some(row) = row {
            color.0 = if host && row == cursor.0 { HILITE } else { Color::WHITE };
        }
        let sel = |r: usize, label: String| if host && r == cursor.0 { format!("> {label} <") } else { label };
        text.0 = match (kind, &s.role) {
            (LobbyText::Title, Role::Host(_)) => "Hosting a LAN game".into(),
            (LobbyText::Title, _) => "LAN lobby".into(),
            (LobbyText::Info, Role::Host(h)) => {
                format!("Friends join with address:  {ip}:{}\n(make sure UDP port {} isn't firewalled)", h.port, h.port)
            }
            (LobbyText::Info, Role::Client(c)) if !c.connected => format!("Connecting to {} ...", c.host),
            (LobbyText::Info, Role::Client(c)) => format!("Connected to {}", c.host),
            (LobbyText::Info, Role::Solo) => String::new(),
            (LobbyText::Track, Role::Host(_)) => sel(LOBBY_TRACK, format!("Track: < {} >", s.track.name())),
            (LobbyText::Track, _) => format!("Track: {}", s.track.name()),
            (LobbyText::Class, Role::Host(_)) => sel(LOBBY_CLASS, format!("Class: < {} >", s.class.name())),
            (LobbyText::Class, _) => format!("Class: {}", s.class.name()),
            (LobbyText::Players, _) => {
                let mut out = String::from("Players:\n");
                for (i, p) in players.iter().enumerate() {
                    let tag = if i == 0 { "  (host)" } else { "" };
                    out += &format!("  {}. {p}{tag}\n", i + 1);
                }
                out
            }
            (LobbyText::Bots, Role::Client(c)) => format!("CPU racers: {}", c.lobby_bots),
            (LobbyText::Bots, _) => sel(LOBBY_BOTS, format!("CPU racers: < {} >  (max {})", s.bots, s.max_bots())),
            (LobbyText::Help, Role::Host(_)) => "Enter: start race   Up/Down: select   Left/Right: change   Esc: leave".into(),
            (LobbyText::Help, _) => "Waiting for the host to start...   Esc: leave".into(),
        };
    }
}
