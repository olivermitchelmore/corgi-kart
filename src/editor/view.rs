//! Drawing the map in the editor: the road, verges and walls as one vertex-coloured 2D mesh
//! (rebuilt when the map changes), the start line, item boxes and boost pads as another (rebuilt
//! when zooming too, as they grow so they can still be seen from far out), and the control points,
//! grid, problems and tool previews drawn as gizmos on top.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite::Text2dShadow;

use super::{arc_between, road_at, Doc, Drag, EditorCamera, EditorState, Tool};
use crate::game::AppState;
use crate::render::trackmesh::rainbow;
use crate::sim::custom::{self, Built, Problems, PAD_LENGTH};
use crate::sim::track::{RAMP_HALF_WIDTH, RAMP_LENGTH};
use crate::sim::track::{Theme, ROAD_HALF_WIDTH, WALL_DIST};

/// The map's mesh.
#[derive(Component)]
pub struct MapMesh;

/// The start line, item boxes and boost pads.
#[derive(Component)]
pub struct FeatureMesh;

/// "START", next to the start line.
#[derive(Component)]
pub struct StartLabel;

/// A height label next to a control point.
#[derive(Component)]
pub struct HeightLabel(usize);

/// From the track's (x, z) to the 2D world, which is the view from above.
pub fn to2d(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

pub fn spawn_view(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<ColorMaterial>>) {
    let mat = mats.add(ColorMaterial::from_color(Color::WHITE));
    let empty = || Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    commands.spawn((Mesh2d(meshes.add(empty())), MeshMaterial2d(mat.clone()), Transform::default(), MapMesh, DespawnOnExit(AppState::Editor)));
    commands.spawn((Mesh2d(meshes.add(empty())), MeshMaterial2d(mat), Transform::default(), FeatureMesh, DespawnOnExit(AppState::Editor)));
    commands.spawn((
        Text2d::new("START"),
        TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
        TextColor(Color::WHITE),
        Text2dShadow { offset: Vec2::new(1.5, -1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
        Transform::default(),
        Visibility::Hidden,
        StartLabel,
        DespawnOnExit(AppState::Editor),
    ));
}

/// Colours for each theme: (ground, verge, wall, road).
pub(super) fn palette(theme: Theme) -> (Color, Color, Color, Color) {
    match theme {
        Theme::Prism => (Color::srgb(0.05, 0.05, 0.1), Color::srgb(0.13, 0.13, 0.2), Color::srgb(0.45, 0.9, 1.0), Color::srgb(0.4, 0.4, 0.46)),
        Theme::Circuit => (Color::srgb(0.27, 0.45, 0.22), Color::srgb(0.38, 0.6, 0.3), Color::srgb(0.93, 0.93, 0.93), Color::srgb(0.33, 0.33, 0.37)),
        Theme::Jungle => (Color::srgb(0.13, 0.26, 0.11), Color::srgb(0.33, 0.25, 0.15), Color::srgb(0.62, 0.58, 0.5), Color::srgb(0.47, 0.44, 0.38)),
        Theme::Tbilisi => (Color::srgb(0.1, 0.1, 0.17), Color::srgb(0.3, 0.29, 0.28), Color::srgb(0.66, 0.58, 0.46), Color::srgb(0.4, 0.38, 0.4)),
        Theme::Meadow | Theme::Lagoon => (Color::srgb(0.3, 0.5, 0.24), Color::srgb(0.4, 0.62, 0.3), Color::srgb(0.16, 0.36, 0.14), Color::srgb(0.36, 0.36, 0.4)),
    }
}

/// Coloured quads at different heights, drawn lowest first so a bridge covers the road under it.
#[derive(Default)]
struct Quads(Vec<(f32, [Vec2; 4], Color)>);

impl Quads {
    fn quad(&mut self, z: f32, p: [Vec2; 4], c: Color) {
        self.0.push((z, p.map(to2d), c));
    }

    fn mesh(mut self) -> Mesh {
        self.0.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut pos = Vec::with_capacity(self.0.len() * 4);
        let mut col = Vec::with_capacity(self.0.len() * 4);
        let mut idx = Vec::with_capacity(self.0.len() * 6);
        for (z, p, c) in self.0 {
            let base = pos.len() as u32;
            let c = c.to_linear().to_f32_array();
            for v in p {
                pos.push([v.x, v.y, z]);
                col.push(c);
            }
            // Either winding: 2D meshes aren't culled.
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let n = pos.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
            .with_inserted_indices(Indices::U32(idx))
    }
}

/// Rebuild the map's meshes when the map changes, and the features' when zooming too.
#[allow(clippy::too_many_arguments)]
pub fn draw_map(
    doc: Res<Doc>,
    st: Res<EditorState>,
    mut built_for: Local<u64>,
    mut features_for: Local<(u64, f32)>,
    mut meshes: ResMut<Assets<Mesh>>,
    map_mesh: Query<&Mesh2d, With<MapMesh>>,
    feature_mesh: Query<&Mesh2d, With<FeatureMesh>>,
    mut cam: Query<&mut Camera, With<EditorCamera>>,
) {
    if *features_for != (doc.version, st.zoom)
        && let Ok(handle) = feature_mesh.single()
        && let Some(mut m) = meshes.get_mut(&handle.0)
    {
        *features_for = (doc.version, st.zoom);
        *m = match &doc.built {
            Some((b, _)) => feature_quads(b, st.zoom).mesh(),
            None => Quads::default().mesh(),
        };
    }
    if *built_for == doc.version {
        return;
    }
    *built_for = doc.version;
    let (ground, ..) = palette(doc.map.theme);
    for mut c in &mut cam {
        c.clear_color = ClearColorConfig::Custom(ground);
    }
    let Ok(handle) = map_mesh.single() else { return };
    let mesh = match &doc.built {
        Some((b, p)) => map_quads(b, p, doc.map.theme).mesh(),
        None => Quads::default().mesh(),
    };
    if let Some(mut m) = meshes.get_mut(&handle.0) {
        *m = mesh;
    }
}

fn map_quads(built: &Built, problems: &Problems, theme: Theme) -> Quads {
    let t = &built.track;
    let n = t.samples.len();
    let (_, verge, wall, asphalt) = palette(theme);
    let mut q = Quads::default();
    let at = |i: usize, lat: f32| {
        let s = &t.samples[i % n];
        s.pos + s.dir.perp() * lat
    };
    // Each sample's slab of road sits at its height; layers within it are a hair apart.
    let z = |i: usize, layer: f32| t.samples[i % n].height * 0.5 + layer * 0.01;
    let strip = |q: &mut Quads, i: usize, l0: f32, l1: f32, layer: f32, c: Color| {
        q.quad(z(i, layer), [at(i, l0), at(i, l1), at(i + 1, l1), at(i + 1, l0)], c);
    };
    let water = Color::srgb(0.2, 0.5, 0.85);

    // Ponds under the fords.
    for &(ws, len) in &t.water {
        let mid = ws + len * 0.5;
        let (c, d) = t.point_at(mid);
        let i = (t.wrap_s(mid) / t.spacing) as usize;
        let (along, across) = (len * 0.5 + 3.0, 60.0);
        let k = 40;
        let edge = |j: usize| {
            let a = j as f32 / k as f32 * std::f32::consts::TAU;
            let sq = |x: f32| x.signum() * x.abs().powf(0.6);
            c + d * sq(a.cos()) * along + d.perp() * sq(a.sin()) * across
        };
        for j in 0..k {
            q.quad(z(i, 0.5), [c, edge(j), edge(j + 1), c], water.darker(0.15));
        }
    }

    for i in 0..n {
        let s = &t.samples[i];
        let bad = problems.bad.get(i).copied().unwrap_or(false);
        strip(&mut q, i, -WALL_DIST, WALL_DIST, 0.0, verge);
        for side in [-1.0f32, 1.0] {
            strip(&mut q, i, side * WALL_DIST, side * (WALL_DIST + 1.2), 0.1, wall);
        }
        let raise = (s.height / custom::MAX_HEIGHT).min(1.0);
        let road = if bad {
            Color::srgb(0.9, 0.18, 0.15)
        } else if t.in_water(s.s) {
            water
        } else if theme == Theme::Prism {
            rainbow((s.s / 8.0) as usize % 7).mix(&asphalt, 0.35)
        } else {
            asphalt
        };
        // Higher road is lighter, so hills and bridges stand out.
        let road = road.mix(&Color::srgb(0.95, 0.9, 0.75), raise * 0.55);
        strip(&mut q, i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 1.0, road);
        if theme != Theme::Prism {
            let edge = Color::srgb(0.92, 0.92, 0.88);
            strip(&mut q, i, -ROAD_HALF_WIDTH, -ROAD_HALF_WIDTH + 0.6, 1.5, edge);
            strip(&mut q, i, ROAD_HALF_WIDTH - 0.6, ROAD_HALF_WIDTH, 1.5, edge);
        }
    }

    // Arrows along the road showing which way the race goes.
    let mut s = 30.0;
    while s < t.length - 20.0 {
        let (c, d) = t.point_at(s);
        let i = (s / t.spacing) as usize;
        let tip = c + d * 3.5;
        let (l, r) = (c - d * 2.5 + d.perp() * 4.0, c - d * 2.5 - d.perp() * 4.0);
        let col = Color::srgba(1.0, 1.0, 1.0, 1.0).mix(&asphalt, 0.35);
        q.quad(z(i, 2.0), [l, tip, tip, c - d * 0.5], col);
        q.quad(z(i, 2.0), [r, tip, tip, c - d * 0.5], col);
        s += 45.0;
    }

    // The starting grid (the line itself is with the other features).
    for slot in 0..8 {
        let (row, col) = ((slot / 2) as f32, slot % 2);
        let s = -8.0 - row * 7.0 - col as f32 * 3.5;
        let lat = if col == 0 { -4.5 } else { 4.5 };
        let (c, d) = t.point_at(s);
        let p = |l: f32, a: f32| c + d.perp() * (lat + l) + d * a;
        let i = (t.wrap_s(s) / t.spacing) as usize;
        q.quad(z(i, 2.5), [p(-1.6, -0.2), p(1.6, -0.2), p(1.6, 0.2), p(-1.6, 0.2)], Color::srgb(0.95, 0.95, 0.92));
    }

    q
}

/// The start line, item boxes and boost pads. When zoomed out (`zoom` is metres per pixel) they
/// grow about their middles so they're still a few pixels across.
fn feature_quads(built: &Built, zoom: f32) -> Quads {
    let t = &built.track;
    let n = t.samples.len();
    let z = |s: f32, layer: f32| t.samples[(t.wrap_s(s) / t.spacing) as usize % n].height * 0.5 + layer * 0.01;
    let grow = |px: f32, metres: f32| (zoom * px / metres).clamp(1.0, 5.0);
    let mut q = Quads::default();

    // The start line: a chequered strip.
    let (c, d) = t.point_at(0.0);
    let sq = 2.0 * ROAD_HALF_WIDTH / 10.0;
    let (wide, deep) = (grow(24.0, 2.0 * ROAD_HALF_WIDTH), grow(10.0, 2.0 * sq));
    let p = |l: f32, a: f32| c + d.perp() * l * wide + d * a * deep;
    for row in 0..2 {
        let a0 = (row as f32 - 1.0) * sq;
        for col in 0..10 {
            let l0 = -ROAD_HALF_WIDTH + col as f32 * sq;
            let c = if (row + col) % 2 == 0 { Color::WHITE } else { Color::srgb(0.05, 0.05, 0.05) };
            q.quad(z(0.0, 3.0), [p(l0, a0), p(l0 + sq, a0), p(l0 + sq, a0 + sq), p(l0, a0 + sq)], c);
        }
    }

    // Each box at least 7 pixels across, with a gap between.
    let (spread, half) = (grow(9.0, 4.0), (3.5 * zoom).max(1.3));
    for &s in &t.item_rows {
        let (c, d) = t.point_at(s);
        for (j, lat) in [-6.0f32, -2.0, 2.0, 6.0].into_iter().enumerate() {
            let m = c + d.perp() * lat * spread;
            let (a, b) = (d * half, d.perp() * half);
            q.quad(z(s, 3.0), [m - a - b, m - a + b, m + a + b, m + a - b], rainbow(j * 2));
        }
    }
    let k = grow(16.0, PAD_LENGTH);
    for pad in &t.boost_pads {
        let mid = pad.s + pad.length * 0.5;
        let (c, d) = t.point_at(mid);
        let centre = c + d.perp() * pad.lateral;
        let p = |l: f32, a: f32| centre + (d.perp() * l + d * a) * k;
        let (hw, hl) = (pad.half_width, PAD_LENGTH * 0.5);
        q.quad(z(mid, 3.0), [p(-hw, -hl), p(hw, -hl), p(hw, hl), p(-hw, hl)], Color::srgb(1.0, 0.5, 0.08));
        q.quad(z(mid, 3.5), [p(-hw * 0.8, -1.0), p(0.0, 1.6), p(0.0, 1.6), p(hw * 0.8, -1.0)], Color::srgb(1.0, 0.92, 0.35));
    }
    // Ramps: the deck, a dark band at the lip, and an arrow (orange on a boosted one).
    let k = grow(16.0, RAMP_LENGTH);
    for r in &t.ramps {
        let mid = r.s + RAMP_LENGTH * 0.5;
        let (c, d) = t.point_at(mid);
        let centre = c + d.perp() * r.lateral;
        let p = |l: f32, a: f32| centre + (d.perp() * l + d * a) * k;
        let (hw, hl) = (RAMP_HALF_WIDTH, RAMP_LENGTH * 0.5);
        let arrow = if r.boosted { Color::srgb(1.0, 0.55, 0.1) } else { Color::srgb(0.95, 0.97, 1.0) };
        q.quad(z(mid, 3.0), [p(-hw, -hl), p(hw, -hl), p(hw, hl), p(-hw, hl)], Color::srgb(0.2, 0.45, 0.95));
        q.quad(z(mid, 3.2), [p(-hw, hl - 1.0), p(hw, hl - 1.0), p(hw, hl), p(-hw, hl)], Color::srgb(0.08, 0.12, 0.3));
        q.quad(z(mid, 3.5), [p(-hw * 0.7, -2.4), p(0.0, 1.8), p(0.0, 1.8), p(hw * 0.7, -2.4)], arrow);
    }
    q
}

/// Grid spacing in metres for a zoom level: lines at least ~45 pixels apart.
pub fn grid_step(zoom: f32) -> f32 {
    [10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0].into_iter().find(|&g| g / zoom >= 45.0).unwrap_or(2000.0)
}

/// The grid, the control points and what the current tool would do.
pub fn draw_overlay(mut gizmos: Gizmos, time: Res<Time>, doc: Res<Doc>, st: Res<EditorState>, cam: Query<(&Camera, &Transform), With<EditorCamera>>) {
    let Ok((camera, tr)) = cam.single() else { return };
    let zoom = st.zoom;
    let half = camera.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0)) * 0.5 * zoom;
    let centre = tr.translation.truncate();
    let (lo, hi) = (centre - half, centre + half);

    // Grid, with every fifth line stronger.
    let g = grid_step(zoom);
    let line = |k: f32| if (k / g).round() as i64 % 5 == 0 { Color::srgba(1.0, 1.0, 1.0, 0.16) } else { Color::srgba(1.0, 1.0, 1.0, 0.06) };
    let mut x = (lo.x / g).floor() * g;
    while x <= hi.x {
        gizmos.line_2d(Vec2::new(x, lo.y), Vec2::new(x, hi.y), line(x));
        x += g;
    }
    let mut y = (lo.y / g).floor() * g;
    while y <= hi.y {
        gizmos.line_2d(Vec2::new(lo.x, y), Vec2::new(hi.x, y), line(y));
        y += g;
    }

    // Which way the race goes from the start line.
    if let Some((built, _)) = &doc.built {
        let (c, d) = built.track.point_at(0.0);
        let from = c + d * (4.0 + 6.0 * zoom);
        gizmos.arrow_2d(to2d(from), to2d(from + d * (30.0 * zoom).max(14.0)), Color::WHITE).with_tip_length(9.0 * zoom);
    }

    // Ring each stretch of road that stops the map being raced, pulsing so it's hard to miss.
    if let Some((built, problems)) = &doc.built
        && !problems.ok()
    {
        let t = &built.track;
        let n = t.samples.len();
        let bad = |i: usize| problems.bad.get(i % n).copied().unwrap_or(false);
        let red = Color::srgba(1.0, 0.25, 0.2, 0.6 + 0.35 * (time.elapsed_secs() * 5.0).sin());
        let first = (0..n).find(|&i| !bad(i)).unwrap_or(0);
        let mut run = vec![];
        for k in 0..=n {
            let i = (first + k) % n;
            if k < n && bad(i) {
                run.push(i);
                continue;
            }
            if run.is_empty() {
                continue;
            }
            for side in [-1.0, 1.0] {
                gizmos.linestrip_2d(run.iter().map(|&i| to2d(t.samples[i].pos + t.samples[i].dir.perp() * side * (WALL_DIST + 3.0))), red);
            }
            let centre = (t.samples[run[0]].pos + t.samples[run[run.len() - 1]].pos + t.samples[run[run.len() / 2]].pos) / 3.0;
            let r = run.iter().map(|&i| t.samples[i].pos.distance(centre)).fold(0.0, f32::max) + WALL_DIST + 6.0;
            gizmos.circle_2d(Isometry2d::from_translation(to2d(centre)), r.max(16.0 * zoom), red);
            run.clear();
        }
    }

    // The loop being drawn.
    if st.stroke.len() > 1 {
        gizmos.linestrip_2d(st.stroke.iter().map(|&p| to2d(p)), Color::srgb(1.0, 0.62, 0.2));
        gizmos.line_2d(to2d(*st.stroke.last().unwrap()), to2d(st.stroke[0]), Color::srgba(1.0, 0.62, 0.2, 0.35));
    }

    // Control points, and the spline through them.
    let shaping = matches!(st.tool, Tool::Points | Tool::Height);
    let pts = &doc.map.points;
    if shaping && pts.len() >= 2 {
        for i in 0..pts.len() {
            let (a, b) = (pts[i].pos(), pts[(i + 1) % pts.len()].pos());
            gizmos.line_2d(to2d(a), to2d(b), Color::srgba(1.0, 1.0, 1.0, 0.15));
        }
    }
    for (i, p) in pts.iter().enumerate() {
        let hover = st.hover_point == Some(i);
        let c = if hover {
            Color::srgb(1.0, 0.85, 0.3)
        } else if p.h > 0.0 {
            Color::srgb(0.55, 0.85, 1.0)
        } else {
            Color::WHITE
        };
        let r = if hover { 8.0 } else if shaping { 5.5 } else { 3.0 };
        // A dark rim, to stand out on pale road.
        gizmos.circle_2d(Isometry2d::from_translation(to2d(p.pos())), (r + 1.5) * zoom, Color::srgba(0.0, 0.0, 0.0, 0.55));
        gizmos.circle_2d(Isometry2d::from_translation(to2d(p.pos())), r * zoom, c);
        if shaping || hover {
            gizmos.circle_2d(Isometry2d::from_translation(to2d(p.pos())), r * zoom * 0.45, c);
        }
    }

    // What the tool would do here.
    let Some(cursor) = st.cursor else { return };
    let Some((built, _)) = &doc.built else { return };
    let t = &built.track;
    let hit = road_at(built, cursor, WALL_DIST);
    let ghost = Color::srgba(1.0, 0.85, 0.3, 0.9);
    let across = |gizmos: &mut Gizmos, i: usize, c: Color| {
        let s = &t.samples[i];
        gizmos.line_2d(to2d(s.pos - s.dir.perp() * WALL_DIST), to2d(s.pos + s.dir.perp() * WALL_DIST), c);
    };
    match (st.tool, &hit) {
        (Tool::Points, Some(h)) if st.hover_point.is_none() && st.drag == Drag::None => {
            let p = to2d(t.samples[h.idx].pos);
            gizmos.line_2d(p - Vec2::X * 6.0 * zoom, p + Vec2::X * 6.0 * zoom, ghost);
            gizmos.line_2d(p - Vec2::Y * 6.0 * zoom, p + Vec2::Y * 6.0 * zoom, ghost);
        }
        (Tool::Items, Some(h)) => {
            let s = &t.samples[h.idx];
            for lat in [-6.0f32, -2.0, 2.0, 6.0] {
                gizmos.rect_2d(Isometry2d::new(to2d(s.pos + s.dir.perp() * lat), Rot2::radians(-s.dir.to_angle())), Vec2::splat(2.6), ghost);
            }
        }
        (Tool::Boost, Some(h)) => {
            let s = &t.samples[h.idx];
            let lat = h.lateral.clamp(-custom::PAD_MAX_LATERAL, custom::PAD_MAX_LATERAL);
            gizmos.rect_2d(Isometry2d::new(to2d(s.pos + s.dir.perp() * lat), Rot2::radians(-s.dir.to_angle())), Vec2::new(PAD_LENGTH, 5.0), ghost);
        }
        (Tool::Ramp | Tool::BoostRamp, Some(h)) => {
            let s = &t.samples[h.idx];
            let lat = h.lateral.clamp(-custom::RAMP_MAX_LATERAL, custom::RAMP_MAX_LATERAL);
            gizmos.rect_2d(Isometry2d::new(to2d(s.pos + s.dir.perp() * lat), Rot2::radians(-s.dir.to_angle())), Vec2::new(RAMP_LENGTH, 2.0 * RAMP_HALF_WIDTH), ghost);
        }
        (Tool::Start, Some(h)) => {
            across(&mut gizmos, h.idx, ghost);
            let s = &t.samples[h.idx];
            gizmos.arrow_2d(to2d(s.pos), to2d(s.pos + s.dir * 18.0), ghost);
        }
        (Tool::Ford, _) => {
            if let (Drag::Ford { from }, Some(h)) = (st.drag, &hit) {
                // The stretch it would flood, the short way round.
                let (a, b) = if arc_between(built, from, h.u) <= arc_between(built, h.u, from) { (from, h.u) } else { (h.u, from) };
                let len = arc_between(built, a, b);
                let s0 = built.spline.s_at(a);
                let pts: Vec<Vec2> = (0..=((len / 2.0) as usize)).map(|k| to2d(built.spline.point(s0 + k as f32 * 2.0).truncate())).collect();
                gizmos.linestrip_2d(pts, Color::srgb(0.4, 0.75, 1.0));
            } else if let Some(h) = &hit {
                across(&mut gizmos, h.idx, Color::srgba(0.4, 0.75, 1.0, 0.9));
            }
        }
        _ => {}
    }
}

/// Heights next to the points while the height tool is out (and next to any raised point).
#[allow(clippy::type_complexity)]
pub fn update_labels(
    mut commands: Commands,
    doc: Res<Doc>,
    st: Res<EditorState>,
    mut labels: Query<(Entity, &HeightLabel, &mut Text2d, &mut TextColor, &mut Transform, &mut Visibility), Without<StartLabel>>,
    mut start: Query<(&mut Transform, &mut Visibility), With<StartLabel>>,
) {
    // "START" beside the start line, on the left.
    for (mut tr, mut vis) in &mut start {
        *vis = match &doc.built {
            Some((b, _)) => {
                let (c, d) = b.track.point_at(0.0);
                let at = c - d.perp() * (WALL_DIST + 30.0 * st.zoom);
                tr.translation = to2d(at).extend(60.0);
                tr.scale = Vec3::splat(st.zoom);
                Visibility::Visible
            }
            None => Visibility::Hidden,
        };
    }

    let pts = &doc.map.points;
    let mut have = 0;
    for (e, label, mut text, mut colour, mut tr, mut vis) in &mut labels {
        have = have.max(label.0 + 1);
        let Some(p) = pts.get(label.0) else {
            commands.entity(e).despawn();
            continue;
        };
        let show = st.tool == Tool::Height || (p.h > 0.0 && st.tool == Tool::Points) || st.hover_point == Some(label.0) && st.tool == Tool::Height;
        *vis = if show { Visibility::Visible } else { Visibility::Hidden };
        if show {
            let s = format!("{:.2}m", p.h).replace(".00m", "m");
            if text.0 != s {
                text.0 = s;
            }
            colour.set_if_neq(TextColor(if p.h > 0.0 { Color::srgb(0.7, 0.9, 1.0) } else { Color::WHITE }));
            tr.translation = (to2d(p.pos()) + Vec2::new(0.0, 16.0) * st.zoom).extend(50.0);
            tr.scale = Vec3::splat(st.zoom);
        }
    }
    for i in have..pts.len() {
        commands.spawn((
            Text2d::new(""),
            TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
            TextColor(Color::WHITE),
            Text2dShadow { offset: Vec2::new(1.5, -1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
            Transform::default(),
            Visibility::Hidden,
            HeightLabel(i),
            DespawnOnExit(AppState::Editor),
        ));
    }
}
