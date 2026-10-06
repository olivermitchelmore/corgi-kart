//! In-race HUD: lap and timing panel, live standings tower, 3D item slot, big place badge,
//! speedometer with drift-charge gauge, animated countdown and banners, minimap, name tags and
//! the results board.
//!
//! Each local player has their own HUD over their view (`HudPlayer` says whose each part is);
//! the results board is shared.

use bevy::prelude::*;
use bevy::text::FontSize;

use super::models::{KART_COLORS, SPARK_COLORS};
use super::race::kart_pose;
use super::split::{place, spawn_dividers, view_percent, PlayerCamera};
use crate::game::{AppState, QuitArm, RaceEntity, SessionRes, TrackRes};
use crate::input::controls_hint;
use crate::session::{RaceState, Role};
use crate::sim::kart::{drift_stage, TrickTry, DRIFT_STAGES, MAX_SPEED, MUD_TIME};
use crate::sim::world::WOOF_BARKS;
use crate::sim::{Kart, Track, TrackId, World, COUNTDOWN, LAPS};

const PANEL: Color = Color::srgba(0.04, 0.05, 0.11, 0.74);
const ACCENT: Color = Color::srgb(1.0, 0.6, 0.15);
const MAP_SIZE: f32 = 200.0;
const MAP_PAD: f32 = 14.0;

#[derive(Component, Clone, Copy, PartialEq)]
pub enum HudText {
    Lap,
    Time,
    Best,
    Place,
    PlaceSuffix,
    ItemCount,
    Countdown,
    Message,
    Speed,
    Help,
    /// How a jump's trick went.
    Trick,
    /// The held item's name, and what's happening to the kart (huge, cowering, under a cloud...).
    ItemName,
    Status,
}

/// Which local player a HUD part belongs to.
#[derive(Component, Clone, Copy)]
pub struct HudPlayer(pub usize);

#[derive(Component)]
pub struct ResultsPanel;
#[derive(Component)]
pub struct ResultsRow(usize);
#[derive(Component)]
pub struct ResultsFooter;
#[derive(Component)]
pub struct MinimapDot {
    kart: usize,
    /// The map's size and the dot's.
    map: f32,
    dot: f32,
}
#[derive(Component)]
pub struct NameTag(usize);
#[derive(Component)]
pub struct TowerRow(usize);
#[derive(Component)]
pub struct SpeedBar;
#[derive(Component)]
pub struct DriftBar;
#[derive(Component)]
pub struct ItemSlot;
/// A splat of mud on a player's view; each has its own moment to land.
#[derive(Component)]
pub struct MudSplat(f32);

/// Where the splats land (percent of the view, from its top left) and how wide they are.
const MUD: [(f32, f32, f32); 11] = [
    (30.0, 30.0, 22.0),
    (55.0, 22.0, 17.0),
    (44.0, 48.0, 26.0),
    (68.0, 44.0, 19.0),
    (20.0, 55.0, 15.0),
    (60.0, 62.0, 14.0),
    (37.0, 20.0, 9.0),
    (76.0, 30.0, 8.0),
    (27.0, 44.0, 7.0),
    (52.0, 40.0, 10.0),
    (47.0, 70.0, 9.0),
];

fn font(px: f32) -> TextFont {
    TextFont { font_size: FontSize::Px(px), ..default() }
}

fn text(s: &str, size: f32, color: Color) -> (Text, TextFont, TextColor, TextShadow) {
    (Text::new(s), font(size), TextColor(color), TextShadow::default())
}

fn abs(left: Option<Val>, top: Option<Val>, right: Option<Val>, bottom: Option<Val>) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: left.unwrap_or(Val::Auto),
        top: top.unwrap_or(Val::Auto),
        right: right.unwrap_or(Val::Auto),
        bottom: bottom.unwrap_or(Val::Auto),
        ..default()
    }
}

pub struct MapXf {
    lo: Vec2,
    scale: f32,
    offset: Vec2,
}

impl MapXf {
    fn new(track: &Track, size: f32) -> Self {
        let (lo, hi) = track.samples.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), s| (lo.min(s.pos), hi.max(s.pos)));
        let ext = hi - lo;
        let scale = (size - 2.0 * MAP_PAD) / ext.max_element();
        let offset = (Vec2::splat(size) - ext * scale) * 0.5;
        MapXf { lo, scale, offset }
    }
    fn map(&self, p: Vec2) -> Vec2 {
        (p - self.lo) * self.scale + self.offset
    }
}

/// Player `player`'s HUD, drawn by camera `cam` over `area` (in percent) of the screen.
pub fn spawn_hud(commands: &mut Commands, race: &RaceState, icon: &Handle<Image>, player: usize, cam: Entity, area: Rect) {
    let me = race.locals[player];
    let n = race.world.karts.len();
    let owner = HudPlayer(player);
    commands
        .spawn((
            place(area),
            UiTargetCamera(cam),
            RaceEntity,
            DespawnOnExit(AppState::Race),
        ))
        .with_children(|r| {
            // --- Mud on the view, under everything else
            for (i, (x, y, size)) in MUD.into_iter().enumerate() {
                r.spawn((
                    Node { width: percent(size), aspect_ratio: Some(1.0 + (i % 3) as f32 * 0.15), border_radius: BorderRadius::MAX, ..abs(Some(percent(x - size * 0.5)), Some(percent(y - size * 0.5)), None, None) },
                    BackgroundColor(Color::NONE),
                    Visibility::Hidden,
                    MudSplat(i as f32 * 0.035),
                    owner,
                ));
            }

            // --- Lap / time panel and the standings tower underneath it
            r.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(10), ..abs(Some(px(18)), Some(px(16)), None, None) })
                .with_children(|col| {
                    col.spawn((
                        Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(px(16), px(10)), border_radius: BorderRadius::all(px(12)), border: UiRect::left(px(5)), ..default() },
                        BackgroundColor(PANEL),
                        BorderColor::all(ACCENT),
                    ))
                    .with_children(|p| {
                        p.spawn((text("", 34.0, Color::WHITE), HudText::Lap, owner));
                        p.spawn((text("", 22.0, Color::srgb(0.95, 0.95, 0.85)), HudText::Time, owner));
                        p.spawn((text("", 15.0, Color::srgb(0.6, 0.85, 1.0)), HudText::Best, owner));
                    });
                    col.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },)).with_children(|tw| {
                        for row in 0..n {
                            tw.spawn((
                                Node { width: px(190), height: px(24), align_items: AlignItems::Center, column_gap: px(8), padding: UiRect::horizontal(px(6)), border_radius: BorderRadius::all(px(5)), ..default() },
                                BackgroundColor(PANEL),
                                TowerRow(row),
                                owner,
                            ))
                            .with_children(|rw| {
                                rw.spawn((Node { width: px(22), ..default() },)).with_children(|c| {
                                    c.spawn((Text::new(format!("{}", row + 1)), font(15.0), TextColor(Color::WHITE)));
                                });
                                rw.spawn((Node { width: px(5), height: px(16), border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(Color::WHITE)));
                                rw.spawn((Text::new(""), font(15.0), TextColor(Color::WHITE)));
                            });
                        }
                    });
                });

            // --- Place badge (top right)
            r.spawn((
                Node { align_items: AlignItems::FlexStart, padding: UiRect::axes(px(18), px(4)), border_radius: BorderRadius::all(px(16)), ..abs(None, Some(px(14)), Some(px(20)), None) },
                BackgroundColor(PANEL),
            ))
            .with_children(|p| {
                p.spawn((text("", 86.0, Color::WHITE), HudText::Place, owner));
                p.spawn((Node { margin: UiRect::top(px(14)), ..default() },)).with_children(|s| {
                    s.spawn((text("", 34.0, Color::WHITE), HudText::PlaceSuffix, owner));
                });
            });

            // --- Item slot (top centre) with the rendered 3D item
            r.spawn(Node { position_type: PositionType::Absolute, width: percent(100), top: px(12), justify_content: JustifyContent::Center, ..default() }).with_children(|row| {
                row.spawn((
                    Node { width: px(112), height: px(112), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(4)), border_radius: BorderRadius::all(px(22)), ..default() },
                    BackgroundColor(PANEL),
                    BorderColor::all(ACCENT),
                    UiTransform::default(),
                    ItemSlot,
                    owner,
                ))
                .with_children(|b| {
                    b.spawn((ImageNode::new(icon.clone()), Node { width: px(104), height: px(104), ..default() }));
                    b.spawn((text("", 26.0, Color::srgb(1.0, 0.9, 0.4)), HudText::ItemCount, owner, abs(None, None, Some(px(6)), Some(px(0)))));
                });
            });

            for (kind, size, top, color) in [(HudText::ItemName, 17.0, 128.0, Color::WHITE), (HudText::Status, 24.0, 152.0, Color::srgb(1.0, 0.9, 0.4))] {
                r.spawn(Node { position_type: PositionType::Absolute, width: percent(100), top: px(top), justify_content: JustifyContent::Center, ..default() })
                    .with_children(|row| {
                        row.spawn((text("", size, color), kind, owner));
                    });
            }

            // --- Countdown and banner messages
            for (kind, size, top) in [(HudText::Countdown, 150.0, 22.0), (HudText::Message, 46.0, 38.0), (HudText::Trick, 40.0, 30.0)] {
                r.spawn(Node { position_type: PositionType::Absolute, width: percent(100), top: percent(top), justify_content: JustifyContent::Center, ..default() })
                    .with_children(|row| {
                        row.spawn((text("", size, Color::WHITE), kind, owner, UiTransform::default()));
                    });
            }
            r.spawn(Node { position_type: PositionType::Absolute, width: percent(100), bottom: px(18), justify_content: JustifyContent::Center, ..default() }).with_children(|row| {
                row.spawn((text("", 18.0, Color::srgb(0.95, 0.95, 0.95)), HudText::Help, owner));
            });

            // --- Speedometer and drift gauge (bottom right)
            r.spawn((
                Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: px(6), padding: UiRect::axes(px(16), px(10)), border_radius: BorderRadius::all(px(14)), ..abs(None, None, Some(px(20)), Some(px(20))) },
                BackgroundColor(PANEL),
            ))
            .with_children(|p| {
                p.spawn((text("", 44.0, Color::WHITE), HudText::Speed, owner));
                for (marker, h) in [(0, 8.0), (1, 6.0)] {
                    p.spawn((Node { width: px(200), height: px(h), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12))))
                        .with_children(|bar| {
                            let fill = (Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(ACCENT));
                            if marker == 0 {
                                bar.spawn((fill, SpeedBar, owner));
                            } else {
                                bar.spawn((fill, DriftBar, owner));
                            }
                        });
                }
            });

            // --- Minimap (bottom left)
            spawn_minimap(r, race, Some(me), MAP_SIZE, Node { position_type: PositionType::Absolute, left: px(18), bottom: px(18), ..default() });

            // --- Name tags over the other karts
            for (i, info) in race.roster.iter().enumerate() {
                if i == me {
                    continue;
                }
                r.spawn((
                    Text::new(info.name.clone()),
                    font(15.0),
                    TextColor(KART_COLORS[i % 8].lighter(0.2)),
                    TextShadow::default(),
                    Node { position_type: PositionType::Absolute, ..default() },
                    Visibility::Hidden,
                    NameTag(i),
                    owner,
                ));
            }
        });
}

/// The minimap, highlighting kart `me` (or every local player's, with none).
fn spawn_minimap(parent: &mut ChildSpawnerCommands, race: &RaceState, me: Option<usize>, size: f32, node: Node) {
    let track = &race.track;
    let xf = MapXf::new(track, size);
    let s = size / MAP_SIZE;
    parent.spawn((Node { width: px(size), height: px(size), border_radius: BorderRadius::all(px(18)), ..node }, BackgroundColor(PANEL))).with_children(|m| {
        // Low parts of the track first so the bridge draws on top.
        let mut order: Vec<usize> = (0..track.samples.len()).step_by(3).collect();
        order.sort_by(|&a, &b| track.samples[a].height.total_cmp(&track.samples[b].height));
        for (size, color) in [(8.0 * s, Color::srgba(0.0, 0.0, 0.0, 0.7)), (5.0 * s, Color::srgba(1.0, 1.0, 1.0, 0.9))] {
            for &i in &order {
                let p = xf.map(track.samples[i].pos);
                let c = if track.samples[i].height > 3.0 && size < 6.0 * s { Color::srgb(1.0, 0.8, 0.5) } else { color };
                m.spawn((Node { position_type: PositionType::Absolute, left: px(p.x - size * 0.5), top: px(p.y - size * 0.5), width: px(size), height: px(size), border_radius: BorderRadius::all(px(size)), ..default() }, BackgroundColor(c)));
            }
        }
        let (c, d) = track.point_at(0.0);
        for k in -2..=2 {
            let p = xf.map(c + d.perp() * k as f32 * 3.0);
            m.spawn((Node { position_type: PositionType::Absolute, left: px(p.x - 2.0), top: px(p.y - 2.0), width: px(4), height: px(4), ..default() }, BackgroundColor(if k % 2 == 0 { Color::BLACK } else { Color::WHITE })));
        }
        // Highlighted karts last, so they draw on top.
        let big = |i: usize| me.map_or(race.locals.contains(&i), |me| me == i);
        let mut karts: Vec<usize> = (0..race.world.karts.len()).collect();
        karts.sort_by_key(|&i| big(i));
        for i in karts {
            let dot = if big(i) { 15.0 } else { 11.0 };
            m.spawn((
                Node { position_type: PositionType::Absolute, width: px(dot), height: px(dot), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(dot)), ..default() },
                BackgroundColor(KART_COLORS[i % 8]),
                // Everyone playing on this screen gets a white ring.
                BorderColor::all(if race.locals.contains(&i) { Color::WHITE } else { Color::BLACK }),
                MinimapDot { kart: i, map: size, dot },
            ));
        }
    });
}

/// What the players share, over the whole window: the results board and, in split screen, the
/// lines between the views (and with three players, a map in the spare corner).
pub fn spawn_shared_hud(commands: &mut Commands, race: &RaceState, cam: Entity) {
    let n = race.world.karts.len();
    let players = race.locals.len();
    commands
        .spawn((
            Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
            GlobalZIndex(1),
            UiTargetCamera(cam),
            RaceEntity,
            DespawnOnExit(AppState::Race),
        ))
        .with_children(|r| {
            if players == 3 {
                let spare = view_percent(3, 4);
                r.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..place(spare) }, BackgroundColor(Color::srgb(0.04, 0.05, 0.11))))
                .with_children(|q| spawn_minimap(q, race, None, 300.0, Node::default()));
            }
            if players > 1 {
                spawn_dividers(r, players);
            }

            // --- Results board
            r.spawn((
                Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                Visibility::Hidden,
                ResultsPanel,
            ))
            .with_children(|row| {
                row.spawn((
                    Node { flex_direction: FlexDirection::Column, row_gap: px(4), padding: UiRect::axes(px(36), px(24)), border_radius: BorderRadius::all(px(20)), border: UiRect::top(px(6)), min_width: px(480), ..default() },
                    BackgroundColor(Color::srgba(0.04, 0.05, 0.11, 0.9)),
                    BorderColor::all(ACCENT),
                ))
                .with_children(|b| {
                    b.spawn((text("RESULTS", 40.0, ACCENT), Node { align_self: AlignSelf::Center, margin: UiRect::bottom(px(10)), ..default() }));
                    for place in 0..n {
                        b.spawn((
                            Node { height: px(34), align_items: AlignItems::Center, column_gap: px(12), padding: UiRect::horizontal(px(10)), border_radius: BorderRadius::all(px(6)), ..default() },
                            BackgroundColor(Color::NONE),
                            ResultsRow(place),
                        ))
                        .with_children(|rw| {
                            rw.spawn((Node { width: px(56), ..default() },)).with_children(|c| {
                                c.spawn((Text::new(ordinal(place + 1)), font(22.0), TextColor(Color::WHITE)));
                            });
                            rw.spawn((Node { width: px(8), height: px(22), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(Color::WHITE)));
                            rw.spawn((Node { width: px(240), ..default() },)).with_children(|c| {
                                c.spawn((Text::new(""), font(22.0), TextColor(Color::WHITE)));
                            });
                            rw.spawn((Text::new(""), font(22.0), TextColor(Color::srgb(0.9, 0.9, 0.8))));
                        });
                    }
                    b.spawn((text("", 17.0, Color::srgb(0.75, 0.75, 0.8)), ResultsFooter, Node { align_self: AlignSelf::Center, margin: UiRect::top(px(12)), ..default() }));
                });
            });
        });
}

pub fn ordinal(n: usize) -> String {
    format!("{n}{}", suffix(n))
}

fn suffix(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

fn fmt_time(t: f32) -> String {
    let m = (t / 60.0) as u32;
    format!("{m}:{:05.2}", t - m as f32 * 60.0)
}

fn place_color(place: u8) -> Color {
    match place {
        0 => Color::srgb(1.0, 0.84, 0.2),
        1 => Color::srgb(0.85, 0.88, 0.95),
        2 => Color::srgb(0.95, 0.58, 0.3),
        _ => Color::WHITE,
    }
}

/// Lap timing and short-lived banners for a local player.
#[derive(Default)]
pub struct HudState {
    race_id: Option<u32>,
    lap: u8,
    lap_start: f32,
    best: Option<f32>,
    banner: Option<(String, Color, f32)>,
    /// The last trick tried, and when.
    trick: Option<(TrickTry, f32)>,
    last_try: TrickTry,
}

impl HudState {
    /// Keep up with kart `k`: time its laps and announce them.
    fn follow(&mut self, race_id: u32, k: &Kart, w: &World, t: f32) {
        if self.race_id != Some(race_id) {
            *self = HudState { race_id: Some(race_id), lap: k.lap, ..default() };
        }
        if k.trick_try != self.last_try {
            if self.last_try == TrickTry::Ready && matches!(k.trick_try, TrickTry::Done | TrickTry::Early | TrickTry::Late) {
                self.trick = Some((k.trick_try, t));
            }
            self.last_try = k.trick_try;
        }
        if k.lap <= self.lap {
            return;
        }
        let now = if k.finished { k.finish_time } else { w.race_time() };
        if self.lap >= 1 {
            let lap_time = now - self.lap_start;
            let new_best = self.best.is_none_or(|b| lap_time < b);
            if new_best {
                self.best = Some(lap_time);
            }
            let (label, col) = if k.finished {
                (format!("FINISH!  {}", ordinal(k.place as usize + 1)), Color::srgb(1.0, 0.85, 0.3))
            } else if k.lap == LAPS {
                (format!("FINAL LAP!   {}", fmt_time(lap_time)), Color::srgb(1.0, 0.5, 0.9))
            } else {
                (format!("LAP {}   {}{}", k.lap, fmt_time(lap_time), if new_best { "  BEST!" } else { "" }), Color::srgb(0.6, 0.9, 1.0))
            };
            self.banner = Some((label, col, t));
        }
        self.lap = k.lap;
        self.lap_start = now;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_hud(
    time: Res<Time>,
    session: Option<Res<SessionRes>>,
    quit: Res<QuitArm>,
    pads: Query<&Gamepad>,
    mut states: Local<Vec<HudState>>,
    mut texts: Query<(&HudText, &HudPlayer, &mut Text, &mut TextColor, Option<&mut UiTransform>)>,
    mut bars: Query<(&HudPlayer, &mut Node, &mut BackgroundColor, Has<SpeedBar>), Or<(With<SpeedBar>, With<DriftBar>)>>,
    mut slot: Query<(&HudPlayer, &mut UiTransform), (With<ItemSlot>, Without<HudText>)>,
) {
    let Some(session) = session else { return };
    let Some(race) = session.0.race.as_ref() else { return };
    let w = &race.world;
    let t = time.elapsed_secs();
    let remaining = COUNTDOWN - w.time;
    let players = race.locals.len();
    states.resize_with(players, HudState::default);
    for (st, &i) in states.iter_mut().zip(&race.locals) {
        st.follow(race.race_id, &w.karts[i], w, t);
    }
    let kart = |p: &HudPlayer| race.locals.get(p.0).map(|&i| &w.karts[i]);

    for (kind, owner, mut text, mut color, ui) in &mut texts {
        let (Some(k), Some(st)) = (kart(owner), states.get(owner.0)) else { continue };
        let mut scale = 1.0;
        let (s, c): (String, Option<Color>) = match kind {
            HudText::Lap => (format!("LAP {}/{LAPS}", k.lap.clamp(1, LAPS)), None),
            HudText::Time => (fmt_time(if k.finished { k.finish_time } else { w.race_time() }), None),
            HudText::Best => (st.best.map_or(String::new(), |b| format!("BEST {}", fmt_time(b))), None),
            HudText::Place => (format!("{}", k.place + 1), Some(place_color(k.place))),
            HudText::PlaceSuffix => (suffix(k.place as usize + 1).into(), Some(place_color(k.place))),
            HudText::ItemCount => {
                let s = if k.golden > 0.0 {
                    format!("{:.0}s", k.golden.ceil())
                } else if k.item_count > 1 && k.roulette <= 0.0 {
                    format!("x{}", k.item_count)
                } else {
                    String::new()
                };
                (s, None)
            }
            HudText::ItemName => (if k.roulette <= 0.0 { k.item.name().into() } else { String::new() }, None),
            HudText::Status => {
                if k.cloud > 0.0 {
                    scale = 1.0 + (t * 6.0).sin().abs() * 0.06;
                    (format!("RAIN CLOUD {:.0}  -  bump it onto someone!", k.cloud.ceil()), Some(Color::srgb(0.6, 0.8, 1.0)))
                } else if k.chase > 0.0 {
                    ("SQUIRREL!".into(), Some(Color::srgb(1.0, 0.7, 0.3)))
                } else if k.zoom > 0.0 {
                    (format!("ZOOMIES {:.0}", k.zoom.ceil()), Some(Color::hsl((t * 300.0) % 360.0, 1.0, 0.7)))
                } else if k.big > 0.0 {
                    (format!("HUGE {:.0}", k.big.ceil()), Some(Color::srgb(1.0, 0.6, 0.5)))
                } else if k.flat > 0.0 {
                    ("FLATTENED".into(), Some(Color::srgb(0.8, 0.8, 0.85)))
                } else if k.small > 0.0 {
                    (format!("COWERING {:.0}", k.small.ceil()), Some(Color::srgb(0.8, 0.8, 0.85)))
                } else {
                    (String::new(), None)
                }
            }
            HudText::Countdown => {
                if remaining > 3.0 {
                    (String::new(), None)
                } else if remaining > 0.0 {
                    let n = remaining.ceil() as u32;
                    let frac = remaining.fract();
                    scale = 1.0 + frac.powi(3) * 0.8;
                    let c = [Color::srgb(1.0, 0.9, 0.2), Color::srgb(1.0, 0.6, 0.1), Color::srgb(1.0, 0.25, 0.2)][(n as usize - 1).min(2)];
                    (n.to_string(), Some(c))
                } else if remaining > -1.0 {
                    scale = 1.0 + (1.0 + remaining) * 0.4;
                    ("GO!".into(), Some(Color::srgb(0.3, 1.0, 0.4)))
                } else {
                    (String::new(), None)
                }
            }
            HudText::Message => {
                let wrong_way = {
                    let (_, dir) = race.track.point_at(k.s);
                    k.dir().dot(dir) < -0.3 && k.speed > 3.0 && k.spin <= 0.0
                };
                let banner = st.banner.as_ref().filter(|(_, _, at)| t - at < 2.6);
                // A big woof on its way from someone behind.
                let woof = w.woofs.iter().find(|f| w.karts.get(f.owner as usize).is_some_and(|o| o.place > k.place));
                if quit.0 > 0.0 {
                    ("Press Esc again to leave the race".into(), Some(Color::WHITE))
                } else if let Some(f) = woof {
                    let barks = WOOF_BARKS.iter().filter(|&&at| f.t >= at).count();
                    scale = 1.0 + (1.0 - ((f.t - WOOF_BARKS[barks.max(1) - 1]) / 0.25).min(1.0)) * 0.4;
                    (["", "WOOF!", "WOOF!  WOOF!", "WOOF!  WOOF!  HOP!"][barks.min(3)].into(), Some(Color::srgb(1.0, 0.45, 0.2)))
                } else if wrong_way && !w.over {
                    scale = 1.0 + (t * 8.0).sin().abs() * 0.08;
                    ("WRONG WAY!".into(), Some(Color::srgb(1.0, 0.3, 0.3)))
                } else if let Some((label, col, at)) = banner {
                    let age = t - at;
                    scale = 1.0 + (1.0 - (age / 0.25).min(1.0)) * 0.5;
                    (label.clone(), Some(*col))
                } else if k.finished && !w.over {
                    (format!("FINISH!  {}", ordinal(k.place as usize + 1)), Some(Color::srgb(1.0, 0.85, 0.3)))
                } else {
                    (String::new(), None)
                }
            }
            HudText::Speed => (format!("{:.0} km/h", k.speed.abs() * 3.6), if k.is_boosting() { Some(ACCENT) } else { Some(Color::WHITE) }),
            HudText::Trick => match st.trick.filter(|&(_, at)| t - at < 0.9) {
                Some((how, at)) => {
                    scale = 1.0 + (1.0 - ((t - at) / 0.2).min(1.0)) * 0.6;
                    match how {
                        TrickTry::Done => ("TRICK!".into(), Some(Color::srgb(1.0, 0.85, 0.2))),
                        TrickTry::Early => ("Too early".into(), Some(Color::srgb(0.7, 0.8, 0.95))),
                        _ => ("Too late".into(), Some(Color::srgb(0.7, 0.8, 0.95))),
                    }
                }
                None => (String::new(), None),
            },
            HudText::Help => {
                if remaining > -2.0 {
                    (controls_hint(owner.0, players, pads.iter().len()), None)
                } else {
                    (String::new(), None)
                }
            }
        };
        if text.0 != s {
            text.0 = s;
        }
        if let Some(c) = c {
            color.0 = c;
        }
        if let Some(mut ui) = ui {
            ui.scale = Vec2::splat(scale);
        }
    }

    for (owner, mut node, mut bg, is_speed) in &mut bars {
        let Some(k) = kart(owner) else { continue };
        if is_speed {
            node.width = percent((k.speed.abs() / (MAX_SPEED * k.cc * 1.35) * 100.0).clamp(0.0, 100.0));
            bg.0 = if k.is_boosting() { Color::srgb(1.0, 0.45, 0.1) } else { ACCENT };
        } else {
            let stage = if k.drift_dir != 0 { drift_stage(k.drift_charge) } else { 0 };
            let frac = if k.drift_dir == 0 { 0.0 } else if stage >= 3 { 1.0 } else { k.drift_charge / DRIFT_STAGES[stage] };
            node.width = percent((frac * 100.0).clamp(0.0, 100.0));
            let c = SPARK_COLORS[stage];
            let m = c.red.max(c.green).max(c.blue);
            bg.0 = Color::linear_rgb(c.red / m, c.green / m, c.blue / m);
        }
    }
    for (owner, mut ui) in &mut slot {
        let Some(k) = kart(owner) else { continue };
        let pulse = if k.roulette > 0.0 { 1.0 + (t * 20.0).sin() * 0.04 } else { 1.0 };
        ui.scale = Vec2::splat(pulse);
    }
}

/// Mud splats land on the view one after another, and fade away as the mud wears off.
pub fn update_mud(session: Option<Res<SessionRes>>, mut splats: Query<(&MudSplat, &HudPlayer, &mut BackgroundColor, &mut Visibility)>) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    for (splat, owner, mut bg, mut vis) in &mut splats {
        let mud = race.locals.get(owner.0).map_or(0.0, |&i| race.world.karts[i].mud);
        let on = mud > 0.0 && MUD_TIME - mud >= splat.0;
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if on {
            // No two splats quite the same brown.
            let shade = 0.85 + 0.3 * (splat.0 * 97.0).sin();
            bg.0 = Color::srgba(0.2 * shade, 0.125 * shade, 0.06 * shade, (mud / 1.5).min(1.0) * 0.96);
        }
    }
}

pub fn update_standings(
    session: Option<Res<SessionRes>>,
    mut rows: Query<(&TowerRow, &HudPlayer, &mut BackgroundColor, &Children)>,
    mut chips: Query<&mut BackgroundColor, Without<TowerRow>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    let order = race.world.standings(&race.track);
    for (row, owner, mut bg, kids) in &mut rows {
        let Some(&i) = order.get(row.0) else { continue };
        let me = race.locals.get(owner.0) == Some(&i);
        // Your row stands out; the other players sharing the screen get a fainter one.
        bg.0 = if me {
            Color::srgba(1.0, 0.55, 0.12, 0.85)
        } else if race.locals.contains(&i) {
            Color::srgba(0.35, 0.3, 0.55, 0.85)
        } else {
            PANEL
        };
        // children: [position box, colour chip, name]
        if let Some(mut chip) = kids.get(1).and_then(|&e| chips.get_mut(e).ok()) {
            chip.0 = KART_COLORS[i % 8];
        }
        if let Some(&name) = kids.get(2) {
            if let Ok((mut t, mut c)) = texts.get_mut(name) {
                let k = &race.world.karts[i];
                let label = if k.finished { format!("{}  FIN", race.roster[i].name) } else { race.roster[i].name.clone() };
                if t.0 != label {
                    t.0 = label;
                }
                c.0 = Color::WHITE;
            }
        }
    }
}

pub fn update_results(
    session: Option<Res<SessionRes>>,
    mut panel: Query<&mut Visibility, With<ResultsPanel>>,
    mut rows: Query<(&ResultsRow, &mut BackgroundColor, &Children)>,
    mut chips: Query<&mut BackgroundColor, Without<ResultsRow>>,
    wrappers: Query<&Children, Without<ResultsRow>>,
    mut texts: Query<&mut Text, Without<ResultsFooter>>,
    mut footer: Query<&mut Text, With<ResultsFooter>>,
    test_drive: Option<Res<crate::editor::TestDrive>>,
) {
    let Some(session) = session else { return };
    let Some(race) = session.0.race.as_ref() else { return };
    let w = &race.world;
    for mut v in &mut panel {
        *v = if w.over { Visibility::Visible } else { Visibility::Hidden };
    }
    if !w.over {
        return;
    }
    let order = w.standings(&race.track);
    for (row, mut bg, kids) in &mut rows {
        let Some(&i) = order.get(row.0) else { continue };
        bg.0 = if race.locals.contains(&i) { Color::srgba(1.0, 0.55, 0.12, 0.5) } else { Color::NONE };
        if let Some(mut chip) = kids.get(1).and_then(|&e| chips.get_mut(e).ok()) {
            chip.0 = KART_COLORS[i % 8];
        }
        let name_text = kids.get(2).and_then(|&e| wrappers.get(e).ok()).and_then(|c| c.first().copied());
        if let Some(mut t) = name_text.and_then(|e| texts.get_mut(e).ok()) {
            t.0 = race.roster[i].name.clone();
        }
        if let Some(mut t) = kids.get(3).and_then(|&e| texts.get_mut(e).ok()) {
            let k = &w.karts[i];
            t.0 = if k.finished { fmt_time(k.finish_time) } else { "--:--.--".into() };
        }
    }
    for mut f in &mut footer {
        f.0 = match session.0.role {
            Role::Client(_) => "Waiting for the host...".into(),
            Role::Host(_) => "Enter: back to lobby     Esc x2: quit".into(),
            Role::Solo if test_drive.is_some() => "Enter: race again     Esc x2: back to the editor".into(),
            Role::Solo => "Enter: race again     Esc x2: menu".into(),
        };
    }
}

pub fn update_minimap(session: Option<Res<SessionRes>>, track: Res<TrackRes>, mut dots: Query<(&MinimapDot, &mut Node)>, mut maps: Local<Vec<(TrackId, f32, MapXf)>>) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    for (d, mut node) in &mut dots {
        let Some(k) = race.world.karts.get(d.kart) else { continue };
        let i = match maps.iter().position(|(id, size, _)| *id == track.0.id && *size == d.map) {
            Some(i) => i,
            None => {
                maps.push((track.0.id, d.map, MapXf::new(&track.0, d.map)));
                maps.len() - 1
            }
        };
        let p = maps[i].2.map(k.pos);
        node.left = px(p.x - d.dot * 0.5);
        node.top = px(p.y - d.dot * 0.5);
    }
}

pub fn update_name_tags(
    session: Option<Res<SessionRes>>,
    cams: Query<(&PlayerCamera, &Camera, &GlobalTransform)>,
    overlay: Query<&Camera, With<super::split::OverlayCamera>>,
    gfx: Res<super::gfx::Graphics>,
    mut tags: Query<(&NameTag, &HudPlayer, &mut Node, &mut Visibility, &ComputedNode)>,
) {
    let Some(race) = session.as_ref().and_then(|s| s.0.race.as_ref()) else { return };
    // Views rendered below the window's resolution are stretched over it.
    let stretch = overlay
        .single()
        .ok()
        .and_then(|o| o.physical_target_size())
        .map_or(Vec2::ONE, |size| size.as_vec2() / gfx.render_res.scaled(size).as_vec2());
    for (tag, owner, mut node, mut vis, computed) in &mut tags {
        let (Some(k), Some(&me)) = (race.world.karts.get(tag.0), race.locals.get(owner.0)) else { continue };
        let Some((_, camera, cam_tr)) = cams.iter().find(|(p, _, _)| p.0 == owner.0) else { continue };
        let (pos, heading) = kart_pose(race, tag.0);
        let (h, _) = super::race::kart_ground(race, tag.0, pos, heading);
        let h = h + super::race::kart_lift(race, tag.0, pos, heading).0;
        let world = Vec3::new(pos.x, h + 2.9, pos.y);
        let visible = race.world.karts[me].pos.distance(k.pos) < 120.0;
        match camera.world_to_viewport(cam_tr, world) {
            Ok(p) if visible => {
                // From the view's pixels (a window's, or a split-screen image's) to the HUD's.
                let p = p * stretch * camera.target_scaling_factor().unwrap_or(1.0) * computed.inverse_scale_factor();
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = px(p.x - size.x * 0.5);
                node.top = px(p.y - size.y);
                *vis = Visibility::Visible;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}
