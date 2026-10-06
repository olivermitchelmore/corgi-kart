//! The track itself: road surface, verges, curbs and paint, walls, boost pads, and where the
//! road is raised, grassy embankments or a proper bridge deck on pillars.

use bevy::{light::NotShadowCaster, prelude::*};

use super::mesh::{v3, MeshBuilder};
use super::models::Kit;
use super::scene::{Scenery, SurfaceMats};
use super::signs::sign_uv;
use super::textures::{PAD_TILE, RAMP_TILE};
use crate::sim::track::{Ramp, Theme, Track, RAMP_HALF_WIDTH, RAMP_LENGTH, ROAD_HALF_WIDTH, WALL_DIST};

/// Wall thickness.
pub const WALL_THICK: f32 = 1.2;
/// Embankments fall away at 1 : `BANK_SLOPE`.
pub const BANK_SLOPE: f32 = 2.0;
pub(super) const DECK: f32 = 1.4;

/// Point on the track at sample `i` (wrapping), offset `lat` metres to the right.
pub fn at(track: &Track, i: usize, lat: f32) -> Vec2 {
    let s = &track.samples[i % track.samples.len()];
    s.pos + s.dir.perp() * lat
}

/// Road surface point at sample `i`, `lat` metres right and `dy` above the road.
pub(super) fn at3(track: &Track, i: usize, lat: f32, dy: f32) -> Vec3 {
    v3(at(track, i, lat), track.height_idx(i) + dy)
}

pub(super) fn surface_normal(track: &Track, i: usize) -> Vec3 {
    let n = track.samples.len();
    let slope = (track.height_idx(i + 1) - track.height_idx(i + n - 1)) / (2.0 * track.spacing);
    (Vec3::Y - v3(track.samples[i % n].dir, 0.0) * slope).normalize()
}

/// Samples of other parts of the track that pass well below sample `i`.
fn below<'a>(track: &'a Track, i: usize) -> impl Iterator<Item = &'a crate::sim::track::TrackSample> + 'a {
    let n = track.samples.len();
    let h = track.samples[i].height;
    track.samples.iter().enumerate().step_by(2).filter_map(move |(j, s)| {
        let arc = (j as isize - i as isize).unsigned_abs().min(n - (j as isize - i as isize).unsigned_abs());
        (arc as f32 * track.spacing > 70.0 && s.height < h - 5.0).then_some(s)
    })
}

/// Raised samples that must be a bridge (an embankment there would bury the road below).
pub fn bridge_flags(track: &Track) -> Vec<bool> {
    (0..track.samples.len())
        .map(|i| {
            let s = &track.samples[i];
            if s.height < 1.0 {
                return false;
            }
            let reach = 2.0 * WALL_DIST + WALL_THICK + BANK_SLOPE * s.height + 6.0;
            below(track, i).any(|o| o.pos.distance(s.pos) < reach)
        })
        .collect()
}

pub fn spawn_track(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, mats: &SurfaceMats, kit: &Kit, tex: &super::textures::Textures, track: &Track) {
    if track.theme == Theme::Prism {
        spawn_prism_track(commands, meshes, materials, mats, kit, tex, track);
        return;
    }
    let circuit = track.theme == Theme::Circuit;
    let n = track.samples.len();
    let bridge = bridge_flags(track);
    let mut road = MeshBuilder::default();
    let mut paint = MeshBuilder::default();
    let mut verge = MeshBuilder::default();
    let mut walls = MeshBuilder::default();
    let mut concrete = MeshBuilder::default();

    let strip = |b: &mut MeshBuilder, i: usize, l0: f32, l1: f32, dy: f32, c: Color| {
        let v = [at3(track, i, l0, dy), at3(track, i, l1, dy), at3(track, i + 1, l1, dy), at3(track, i + 1, l0, dy)];
        b.quad(v, surface_normal(track, i), c);
    };

    for i in 0..n {
        let curv = track.samples[i].curvature;
        let corner = curv.abs() > 1.0 / 70.0;

        // Verges between the road and the walls. The circuit has gravel traps on the outside of corners.
        for side in [-1.0f32, 1.0] {
            let outside = corner && side * curv < 0.0;
            let c = if circuit && outside {
                Color::srgb(0.86, 0.76, 0.55)
            } else if circuit {
                if (i / 12) % 2 == 0 { Color::srgb(0.36, 0.66, 0.26) } else { Color::srgb(0.32, 0.6, 0.23) }
            } else {
                Color::srgb(0.34, 0.58, 0.22)
            };
            let (l0, l1) = if side < 0.0 { (-WALL_DIST, -ROAD_HALF_WIDTH) } else { (ROAD_HALF_WIDTH, WALL_DIST) };
            strip(&mut verge, i, l0, l1, 0.0, c);
        }

        // Asphalt, with faint darker bands so speed reads well.
        let band = if (i / 8) % 2 == 0 { 0.0 } else { 0.012 };
        let asphalt = if circuit {
            Color::srgb(0.27 + band, 0.27 + band, 0.30 + band)
        } else {
            Color::srgb(0.29 + band, 0.29 + band, 0.32 + band)
        };
        strip(&mut road, i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 0.02, asphalt);

        // Curbs: everywhere on the meadow, only through corners on the circuit (white lines elsewhere).
        let near_corner = (0..12).any(|o| track.samples[(i + n + o * 3 - 18) % n].curvature.abs() > 1.0 / 90.0);
        if !circuit || near_corner {
            let red = if circuit { Color::srgb(0.88, 0.1, 0.1) } else { Color::srgb(0.85, 0.12, 0.12) };
            let curb = if (i / 3) % 2 == 0 { red } else { Color::srgb(0.95, 0.95, 0.95) };
            strip(&mut paint, i, -ROAD_HALF_WIDTH - 0.4, -ROAD_HALF_WIDTH + 1.1, 0.045, curb);
            strip(&mut paint, i, ROAD_HALF_WIDTH - 1.1, ROAD_HALF_WIDTH + 0.4, 0.045, curb);
        }
        if circuit {
            let white = Color::srgb(0.93, 0.93, 0.9);
            if !near_corner {
                strip(&mut paint, i, -ROAD_HALF_WIDTH + 0.35, -ROAD_HALF_WIDTH + 0.75, 0.04, white);
                strip(&mut paint, i, ROAD_HALF_WIDTH - 0.75, ROAD_HALF_WIDTH - 0.35, 0.04, white);
            }
        } else if i % 10 < 5 {
            strip(&mut paint, i, -0.2, 0.2, 0.04, Color::srgb(0.9, 0.9, 0.85));
        }

        // Walls: hedges on the meadow, painted concrete barriers on the circuit.
        let h = if circuit { 1.15 } else { 1.4 };
        for side in [-1.0f32, 1.0] {
            let (face, top) = if circuit {
                let stripe = (i / 4) % 2 == 0;
                let face = if near_corner {
                    if stripe { Color::srgb(0.9, 0.12, 0.1) } else { Color::srgb(0.95, 0.95, 0.95) }
                } else if stripe {
                    Color::srgb(0.12, 0.3, 0.75)
                } else {
                    Color::srgb(0.93, 0.93, 0.93)
                };
                (face, Color::srgb(0.8, 0.8, 0.78))
            } else {
                let shade = if (i / 4) % 2 == 0 { 0.0 } else { 0.02 };
                (Color::srgb(0.2 + shade, 0.46 + shade, 0.18), Color::srgb(0.26 + shade, 0.55 + shade, 0.21))
            };
            let target = if circuit { &mut concrete } else { &mut walls };
            wall(target, track, &bridge, i, side, WALL_THICK, h, face, top);

            embankment(&mut verge, track, &bridge, i, side, Color::srgb(0.33, 0.58, 0.24));
        }
        bridge_deck(&mut concrete, track, &bridge, i, Color::srgb(0.62, 0.62, 0.64));
    }

    start_line(track, &mut paint, circuit);

    let scenery = || (Scenery, NotShadowCaster);
    commands.spawn((road.tinted(meshes, materials, &mats.road), scenery()));
    commands.spawn((paint.tinted(meshes, materials, &mats.paint), scenery()));
    commands.spawn((verge.tinted(meshes, materials, &mats.grass), scenery()));
    if !walls.is_empty() {
        commands.spawn((walls.tinted(meshes, materials, &mats.hedge), Scenery));
    }
    if !concrete.is_empty() {
        commands.spawn((concrete.tinted(meshes, materials, &mats.concrete), Scenery));
    }

    spawn_puddles(commands, meshes, mats, track);
    spawn_pads_and_ramps(commands, meshes, materials, mats, track);
    spawn_pillars(commands, kit, &kit.concrete, track, &bridge);
    if !track.water.is_empty() {
        spawn_fords(commands, meshes, materials, kit, tex, track, Ford::Pond);
    }
}

/// A wall along one edge of the road from sample `i` to the next, starting `WALL_DIST` out:
/// its inner face, its top and its outer face, which runs down to the ground (or, on a bridge,
/// to the underside of the deck).
#[allow(clippy::too_many_arguments)]
pub(super) fn wall(b: &mut MeshBuilder, track: &Track, bridge: &[bool], i: usize, side: f32, thick: f32, h: f32, face: Color, top: Color) {
    let (s0, s1) = (track.samples[i].s, track.samples[i].s + track.spacing);
    let inner = side * WALL_DIST;
    let outer = side * (WALL_DIST + thick);
    let (a0, a1, b0, b1) = (at3(track, i, inner, 0.0), at3(track, i + 1, inner, 0.0), at3(track, i, outer, 0.0), at3(track, i + 1, outer, 0.0));
    let up = Vec3::Y * h;
    let drop = |p: Vec3| if bridge[i] { p.y - DECK } else if p.y > 0.05 { 0.0 } else { p.y };
    let (ob0, ob1) = (Vec3::new(b0.x, drop(b0), b0.z), Vec3::new(b1.x, drop(b1), b1.z));
    let uvs = |y0: f32, y1: f32| [Vec2::new(s0 / 3.0, y0 / 3.0), Vec2::new(s1 / 3.0, y0 / 3.0), Vec2::new(s1 / 3.0, y1 / 3.0), Vec2::new(s0 / 3.0, y1 / 3.0)];
    // Windings differ per side so every face points outwards.
    if side > 0.0 {
        b.quad_auto([a0, a0 + up, a1 + up, a1], face, uvs(0.0, h));
        b.quad_auto([a0 + up, b0 + up, b1 + up, a1 + up], top, uvs(0.0, 1.0));
        b.quad_auto([ob0, ob1, b1 + up, b0 + up], face.darker(0.08), uvs(ob0.y - b0.y, h));
    } else {
        b.quad_auto([a0, a1, a1 + up, a0 + up], face, uvs(0.0, h));
        b.quad_auto([a0 + up, a1 + up, b1 + up, b0 + up], top, uvs(0.0, 1.0));
        b.quad_auto([ob0, b0 + up, b1 + up, ob1], face.darker(0.08), uvs(ob0.y - b0.y, h));
    }
}

/// The bank down to the ground on one side of sample `i`, wherever the road is raised and isn't
/// a bridge.
pub(super) fn embankment(b: &mut MeshBuilder, track: &Track, bridge: &[bool], i: usize, side: f32, c: Color) {
    let (h0, h1) = (track.height_idx(i), track.height_idx(i + 1));
    if bridge[i] || (h0 <= 0.05 && h1 <= 0.05) {
        return;
    }
    let outer = side * (WALL_DIST + WALL_THICK);
    let (b0, b1) = (at3(track, i, outer, 0.0), at3(track, i + 1, outer, 0.0));
    let foot0 = v3(at(track, i, side * (WALL_DIST + WALL_THICK + BANK_SLOPE * h0)), -0.01);
    let foot1 = v3(at(track, i + 1, side * (WALL_DIST + WALL_THICK + BANK_SLOPE * h1)), -0.01);
    let uv = [b0, b1, foot1, foot0].map(|p| Vec2::new(p.x, p.z) / 4.0);
    if side > 0.0 {
        b.quad_auto([b0, foot0, foot1, b1], c, [uv[0], uv[3], uv[2], uv[1]]);
    } else {
        b.quad_auto([b0, b1, foot1, foot0], c, uv);
    }
}

/// The underside of the bridge deck at sample `i`, and an abutment closing off the embankment's
/// cross-section where it meets the bridge.
pub(super) fn bridge_deck(b: &mut MeshBuilder, track: &Track, bridge: &[bool], i: usize, c: Color) {
    let n = track.samples.len();
    let w = WALL_DIST + WALL_THICK;
    if bridge[i] {
        let (l, r) = (at3(track, i, -w, -DECK), at3(track, i, w, -DECK));
        let (l1, r1) = (at3(track, i + 1, -w, -DECK), at3(track, i + 1, w, -DECK));
        b.quad_auto([l, l1, r1, r], c, [l, l1, r1, r].map(|p| Vec2::new(p.x, p.z) / 4.0));
    }
    if bridge[i] != bridge[(i + 1) % n] && track.height_idx(i) > 0.5 {
        let j = if bridge[i] { i + 1 } else { i };
        let hj = track.height_idx(j);
        let foot = w + BANK_SLOPE * hj;
        let p = |lat: f32, y: f32| v3(at(track, j, lat), y);
        let c = c.lighter(0.04);
        let face = [p(-foot, 0.0), p(-w, hj), p(w, hj), p(foot, 0.0)];
        let uv = face.map(|q| Vec2::new(q.x + q.z, q.y) / 4.0);
        b.quad_auto(face, c, uv);
        b.quad_auto([face[3], face[2], face[1], face[0]], c, uv);
    }
}

/// Rain puddles: irregular mirror-smooth blobs on the road and verges.
pub(super) fn spawn_puddles(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &SurfaceMats, track: &Track) {
    let mut rng = super::mesh::Lcg(0x9ADD_1E5 ^ track.length as u32);
    let mut b = MeshBuilder::default();
    let count = (track.length / 13.0) as usize;
    let pads: Vec<f32> = track.boost_pads.iter().map(|p| p.s).chain(track.ramps.iter().map(|r| r.s)).collect();
    let mut placed = 0;
    while placed < count {
        let s = rng.range(0.0, track.length);
        // skip the start grid and the boost pads
        if !(40.0..track.length - 60.0).contains(&s) || pads.iter().any(|&ps| (s - ps).abs() < 12.0) || track.in_water(s) {
            continue;
        }
        placed += 1;
        // Water collects near the edges more than on the crown of the road.
        let edge = if rng.f() < 0.5 { -1.0 } else { 1.0 };
        let lat = edge * rng.range(1.0, WALL_DIST - 2.0).powf(1.0) * if rng.f() < 0.35 { 0.4 } else { 1.0 };
        let (c, d) = track.point_at(s);
        let y = track.height_at(s) + 0.035;
        let center = c + d.perp() * lat;
        let (rx, rz) = (rng.range(1.0, 3.8), rng.range(0.8, 2.6));
        let rot = Vec2::from_angle(rng.f() * std::f32::consts::TAU);
        let k = 14;
        let wobble: Vec<f32> = (0..k).map(|_| rng.range(0.75, 1.15)).collect();
        let pt = |j: usize| {
            let a = j as f32 / k as f32 * std::f32::consts::TAU;
            let w = wobble[j % k];
            v3(center + rot.rotate(Vec2::new(a.cos() * rx * w, a.sin() * rz * w)), y)
        };
        let mid = v3(center, y);
        for j in 0..k {
            b.tri([mid, pt(j + 1), pt(j)], Vec3::Y, Color::WHITE);
        }
    }
    commands.spawn((Mesh3d(meshes.add(b.build())), MeshMaterial3d(mats.puddle.clone()), NotShadowCaster, Scenery));
}

/// Boost pads and ramps. A pad is a glowing orange panel of arrows that run along the road, with
/// a bright rim, set in a bevelled metal frame with hazard stripes across its ends, and lighting
/// up the road round it. Ramps curve up like a skate ramp in the track's colours; a boosted ramp's
/// face is glowing rainbow bands flowing up to the lip.
pub(super) fn spawn_pads_and_ramps(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, mats: &SurfaceMats, track: &Track) {
    let mut panels = Panels::default();
    let mut frame = MeshBuilder::default();
    let mut paint = MeshBuilder::default();
    let mut lips = MeshBuilder::default();

    for pad in &track.boost_pads {
        let mid = pad.s + pad.length * 0.5;
        let at = |l: f32, a: f32, dy: f32| {
            let (c, d) = track.point_at(mid + a);
            v3(c + d.perp() * (pad.lateral + l), track.height_at(mid + a) + dy)
        };
        let (hw, hl) = (pad.half_width, pad.length * 0.5);
        let metal = |_: usize, _: usize| Color::srgb(0.2, 0.21, 0.24);
        let brushed = |side: usize, j: usize| match side % 2 {
            0 => Color::srgb(0.5, 0.51, 0.55),
            _ if j.is_multiple_of(2) => Color::srgb(1.0, 0.7, 0.04),
            _ => Color::srgb(0.03, 0.03, 0.03),
        };
        ring(&mut frame, &at, (hw + 0.55, hl + 0.45, 0.01), (hw + 0.3, hl + 0.22, 0.13), 6, metal);
        ring(&mut frame, &at, (hw + 0.3, hl + 0.22, 0.13), (hw + 0.06, hl + 0.06, 0.13), 9, brushed);
        ring(&mut frame, &at, (hw + 0.06, hl + 0.06, 0.13), (hw + 0.06, hl + 0.06, 0.07), 1, |_, _| Color::srgb(0.06, 0.06, 0.07));
        panels.add(&at, hw + 0.06, hl + 0.06, 0.07);
        // Keep the glow on the road.
        let room = |side: f32| (ROAD_HALF_WIDTH - 0.3 - (side * pad.lateral + hw + 0.55)).max(0.0);
        halo(&mut panels.halo, &at, hw + 0.55, hl + 0.45, [room(1.0), room(-1.0)]);
    }

    let look = RampLook::of(track.theme);
    for r in &track.ramps {
        let hw = RAMP_HALF_WIDTH;
        // On the ramp's face, `x` metres past its foot, or on the road under it.
        let face = |l: f32, x: f32, dy: f32| {
            let (c, d) = track.point_at(r.s + x);
            v3(c + d.perp() * (r.lateral + l), track.height_at(r.s + x) + Ramp::height(x) + dy)
        };
        let ground = |l: f32, x: f32| face(l, x, -Ramp::height(x));
        let up = |q: [Vec3; 4], c: Color, b: &mut MeshBuilder| b.quad_facing(q, Vec3::Y, c, [Vec2::ZERO; 4]);
        const N: usize = 16;
        let edge = hw - 0.5;
        if r.boosted {
            scrolling(&mut panels.rainbow, &face, hw, 0.0, RAMP_LENGTH, 0.0, RAMP_TILE.y, N);
        }
        for j in 0..N {
            let (x0, x1) = (j as f32 * RAMP_LENGTH / N as f32, (j + 1) as f32 * RAMP_LENGTH / N as f32);
            if !r.boosted {
                let plank = if look.wood { 0.85 + 0.1 * ((j * 7) % 3) as f32 } else { 1.0 };
                for (l0, l1, c) in [(-hw, -edge, look.stripes[j % 2]), (-edge, edge, look.deck.darker(1.0 - plank)), (edge, hw, look.stripes[j % 2])] {
                    up([face(l0, x0, 0.0), face(l1, x0, 0.0), face(l1, x1, 0.0), face(l0, x1, 0.0)], c, &mut paint);
                }
            }
            for side in [-1.0, 1.0] {
                let l = side * hw;
                let out = face(l + side, x0, 0.0) - face(l, x0, 0.0);
                paint.quad_facing([ground(l, x0), ground(l, x1), face(l, x1, 0.0), face(l, x0, 0.0)], out, look.side, [Vec2::ZERO; 4]);
            }
        }
        // The back of the lip, and a metal edge along it with a glowing line.
        let fwd = face(0.0, RAMP_LENGTH + 1.0, 0.0) - face(0.0, RAMP_LENGTH, 0.0);
        let l = RAMP_LENGTH;
        paint.quad_facing([ground(-hw, l), ground(hw, l), face(hw, l, 0.0), face(-hw, l, 0.0)], fwd.with_y(0.0), look.side, [Vec2::ZERO; 4]);
        up([face(-hw, l - 0.4, 0.015), face(hw, l - 0.4, 0.015), face(hw, l, 0.015), face(-hw, l, 0.015)], Color::srgb(0.75, 0.72, 0.65), &mut frame);
        let line = [face(-hw + 0.1, l - 0.22, 0.025), face(hw - 0.1, l - 0.22, 0.025), face(hw - 0.1, l - 0.1, 0.025), face(-hw + 0.1, l - 0.1, 0.025)];
        if r.boosted {
            up(line, Color::WHITE, &mut panels.lips);
        } else {
            up(line, Color::WHITE, &mut lips);
            // Painted arrows up the face.
            for x in [1.6, 4.2] {
                chevron(&mut paint, &|l: f32, a: f32, dy: f32| face(l, a, dy), x, edge_arrow(hw), 0.9, 0.45, 0.01, look.arrow);
            }
        }
    }

    let scenery = || (Scenery, NotShadowCaster);
    if !frame.is_empty() {
        commands.spawn((frame.tinted(meshes, materials, &mats.pad_frame), scenery()));
    }
    if !paint.is_empty() {
        commands.spawn((paint.tinted(meshes, materials, &mats.ramp), Scenery));
    }
    if !lips.is_empty() {
        let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: look.glow, ..default() });
        commands.spawn((Mesh3d(meshes.add(lips.build())), MeshMaterial3d(glow), scenery()));
    }
    if !panels.lips.is_empty() {
        let white = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(4.0, 4.0, 4.0), ..default() });
        commands.spawn((Mesh3d(meshes.add(panels.lips.build())), MeshMaterial3d(white), scenery()));
    }
    for (b, mat) in [(panels.rim, &mats.pad), (panels.arrows, &mats.pad_arrows), (panels.rainbow, &mats.rainbow), (panels.halo, &mats.pad_halo)] {
        if !b.is_empty() {
            commands.spawn((Mesh3d(meshes.add(b.build())), MeshMaterial3d(mat.clone()), scenery()));
        }
    }
}

/// Half-width of the painted arrows on a ramp `hw` wide.
fn edge_arrow(hw: f32) -> f32 {
    hw - 1.3
}

/// How a track's plain ramps look.
struct RampLook {
    deck: Color,
    stripes: [Color; 2],
    side: Color,
    arrow: Color,
    glow: LinearRgba,
    /// Planks: the deck's shade changes along it.
    wood: bool,
}

impl RampLook {
    fn of(theme: Theme) -> Self {
        let white = Color::srgb(0.92, 0.92, 0.9);
        match theme {
            Theme::Circuit => RampLook {
                deck: Color::srgb(0.1, 0.28, 0.75),
                stripes: [Color::srgb(0.85, 0.08, 0.06), white],
                side: Color::srgb(0.16, 0.18, 0.24),
                arrow: white,
                glow: LinearRgba::rgb(3.0, 3.0, 3.4),
                wood: false,
            },
            Theme::Meadow | Theme::Jungle | Theme::Tbilisi => RampLook {
                deck: Color::srgb(0.6, 0.4, 0.22),
                stripes: [Color::srgb(0.98, 0.78, 0.1), Color::srgb(0.08, 0.07, 0.06)],
                side: Color::srgb(0.42, 0.27, 0.14),
                arrow: Color::srgb(0.98, 0.9, 0.6),
                glow: LinearRgba::rgb(3.0, 2.4, 1.2),
                wood: true,
            },
            Theme::Prism => RampLook {
                deck: Color::srgb(0.28, 0.1, 0.5),
                stripes: [Color::srgb(0.15, 0.8, 0.95), white],
                side: Color::srgb(0.08, 0.06, 0.14),
                arrow: Color::srgb(0.6, 0.95, 1.0),
                glow: LinearRgba::rgb(0.6, 3.5, 4.5),
                wood: false,
            },
            Theme::Lagoon => RampLook {
                deck: Color::srgb(0.08, 0.42, 0.48),
                stripes: [white, Color::srgb(0.95, 0.5, 0.3)],
                side: Color::srgb(0.85, 0.8, 0.7),
                arrow: white,
                glow: LinearRgba::rgb(3.5, 2.6, 1.4),
                wood: false,
            },
        }
    }
}

/// The glowing parts of boost pads and boosted ramps, merged across the track.
#[derive(Default)]
struct Panels {
    rim: MeshBuilder,
    arrows: MeshBuilder,
    rainbow: MeshBuilder,
    halo: MeshBuilder,
    /// The glowing line along a boosted ramp's lip.
    lips: MeshBuilder,
}

impl Panels {
    /// A pad's panel `hw` by `hl` (half sizes) at height `dy`, laid out by `at(lateral, along, dy)`.
    fn add(&mut self, at: &dyn Fn(f32, f32, f32) -> Vec3, hw: f32, hl: f32, dy: f32) {
        let rim = 0.16;
        ring(&mut self.rim, at, (hw, hl, dy + 0.004), (hw - rim, hl - rim, dy + 0.004), 6, |_, _| Color::WHITE);
        scrolling(&mut self.arrows, at, hw - rim, -(hl - rim), hl - rim, dy, PAD_TILE.y, 6);
    }
}

/// A strip `hw` either side of the middle from `a0` to `a1` along, in `n` pieces, for a texture
/// that repeats every `tile` metres along and is stretched across the whole width.
#[allow(clippy::too_many_arguments)]
fn scrolling(b: &mut MeshBuilder, at: &dyn Fn(f32, f32, f32) -> Vec3, hw: f32, a0: f32, a1: f32, dy: f32, tile: f32, n: usize) {
    for j in 0..n {
        let (s0, s1) = (a0 + (a1 - a0) * j as f32 / n as f32, a0 + (a1 - a0) * (j + 1) as f32 / n as f32);
        let q = [at(-hw, s0, dy), at(hw, s0, dy), at(hw, s1, dy), at(-hw, s1, dy)];
        let uv = [Vec2::new(0.0, s0 / tile), Vec2::new(1.0, s0 / tile), Vec2::new(1.0, s1 / tile), Vec2::new(0.0, s1 / tile)];
        b.quad_facing(q, Vec3::Y, Color::WHITE, uv);
    }
}

/// Orange light spilling onto the road round a pad whose frame is `hw` by `hl` (half sizes),
/// fading out smoothly over a few metres, but reaching no more than `room` to the right and left.
fn halo(b: &mut MeshBuilder, at: &dyn Fn(f32, f32, f32) -> Vec3, hw: f32, hl: f32, room: [f32; 2]) {
    use std::f32::consts::FRAC_PI_2;
    let glow = LinearRgba::rgb(1.5, 0.5, 0.05) * 0.45;
    const REACH: f32 = 4.0;
    const RINGS: usize = 10;
    const ARC: usize = 4;
    // Flat at the frame and at the far edge, so neither end shows a line.
    let fade = |t: f32| (1.0 - t).powi(3) * (1.0 + 3.0 * t);
    // The frame grown by `d`, with rounded corners, going round anticlockwise from the back
    // right. Every ring has the same points, so neighbouring rings join up quad for quad.
    let outline = |d: f32| {
        let (r, l) = (d.min(room[0]), d.min(room[1]));
        let corners = [(hw, hl, r), (-hw, hl, l), (-hw, -hl, l), (hw, -hl, r)];
        let arc = |k: usize, t: f32| {
            let (cx, cy, rx) = corners[k];
            let a = (k as f32 + t) * FRAC_PI_2;
            Vec2::new(cx + rx * a.cos(), cy + d * a.sin())
        };
        let mut pts = Vec::new();
        for k in 0..4 {
            // The side leading into corner `k`; the long ones follow the road's curve.
            let (from, to) = (arc((k + 3) % 4, 1.0), arc(k, 0.0));
            let n = if k % 2 == 0 { 6 } else { 2 };
            pts.extend((0..n).map(|j| from.lerp(to, j as f32 / n as f32)));
            pts.extend((0..ARC).map(|j| arc(k, j as f32 / ARC as f32)));
        }
        pts
    };
    let rings: Vec<(Vec<Vec2>, Color)> = (0..=RINGS)
        .map(|i| {
            let t = i as f32 / RINGS as f32;
            (outline(REACH * t), Color::LinearRgba(glow * fade(t)))
        })
        .collect();
    for w in rings.windows(2) {
        let ((inner, ci), (outer, co)) = (&w[0], &w[1]);
        let p = |q: Vec2| at(q.x, q.y, 0.05);
        for j in 0..inner.len() {
            let k = (j + 1) % inner.len();
            b.quad_vc([p(inner[j]), p(inner[k]), p(outer[k]), p(outer[j])], Vec3::Y, [*ci, *ci, *co, *co], [Vec2::ZERO; 4]);
        }
    }
}

/// A ">" arrow pointing along the road, its arms `w` out to each side, starting at `a`.
#[allow(clippy::too_many_arguments)]
fn chevron(b: &mut MeshBuilder, at: &dyn Fn(f32, f32, f32) -> Vec3, a: f32, w: f32, depth: f32, thick: f32, dy: f32, c: Color) {
    for side in [-1.0, 1.0] {
        let q = [at(side * w, a, dy), at(0.0, a + depth, dy), at(0.0, a + depth + thick, dy), at(side * w, a + thick, dy)];
        b.quad_facing(q, Vec3::Y, c, [Vec2::ZERO; 4]);
    }
}

/// A band round a rectangle, from an outer outline to an inner one, each given as (half width,
/// half length, height). Bevels, flat rims and walls are all bands. Each side is split into `n`
/// pieces coloured by `colour(side, piece)`; sides go right, front, left, back.
fn ring(b: &mut MeshBuilder, at: &dyn Fn(f32, f32, f32) -> Vec3, outer: (f32, f32, f32), inner: (f32, f32, f32), n: usize, colour: impl Fn(usize, usize) -> Color) {
    let corners = |(w, h, _): (f32, f32, f32)| [Vec2::new(w, -h), Vec2::new(w, h), Vec2::new(-w, h), Vec2::new(-w, -h)];
    let (o, i) = (corners(outer), corners(inner));
    // Faces outwards if it rises going in (a bevel), inwards if it drops (the wall of a recess).
    let out_sign = if inner.2 < outer.2 { -1.0 } else { 1.0 };
    for side in 0..4 {
        let (o0, o1, i0, i1) = (o[side], o[(side + 1) % 4], i[side], i[(side + 1) % 4]);
        let mid = (o0 + o1) * 0.5;
        let out = (at(mid.x * 1.01, mid.y * 1.01, 0.0) - at(mid.x, mid.y, 0.0)).normalize_or(Vec3::X) * out_sign + Vec3::Y * 0.3;
        for j in 0..n {
            let (t0, t1) = (j as f32 / n as f32, (j + 1) as f32 / n as f32);
            let p = |e0: Vec2, e1: Vec2, t: f32, dy: f32| {
                let q = e0.lerp(e1, t);
                at(q.x, q.y, dy)
            };
            let quad = [p(o0, o1, t0, outer.2), p(o0, o1, t1, outer.2), p(i0, i1, t1, inner.2), p(i0, i1, t0, inner.2)];
            b.quad_facing(quad, out, colour(side, j), [Vec2::ZERO; 4]);
        }
    }
}

pub(super) fn spawn_pillars(commands: &mut Commands, kit: &Kit, mat: &Handle<StandardMaterial>, track: &Track, bridge: &[bool]) {
    let n = track.samples.len();
    for i in (0..n).step_by(14) {
        if !bridge[i] {
            continue;
        }
        let h = track.height_idx(i) - DECK;
        for lat in [-11.0, 11.0] {
            let p = at(track, i, lat);
            // Don't plant a pillar on the road underneath.
            if below(track, i).any(|o| o.pos.distance(p) < WALL_DIST + WALL_THICK + 1.5) {
                continue;
            }
            commands.spawn((
                Mesh3d(kit.cylinder.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(v3(p, h * 0.5)).with_scale(Vec3::new(2.2, h, 2.2)),
                Scenery,
            ));
        }
    }
}

/// An advertising board facing the track at arc length `s`, `lat` metres to the side.
pub fn spawn_sign(commands: &mut Commands, meshes: &mut Assets<Mesh>, mat: &Handle<StandardMaterial>, kit: &Kit, track: &Track, s: f32, lat: f32, sign: usize, size: Vec2, raise: f32) {
    let (c, d) = track.point_at(s);
    let base = track.height_at(s);
    let pos = v3(c + d.perp() * lat, base + raise);
    // Face back across the track.
    let facing = -v3(d.perp() * lat.signum(), 0.0);
    spawn_board(commands, meshes, mat, kit, pos, facing, sign, size, true);
}

/// A board centred at `pos` (bottom edge), readable from direction `facing`.
pub fn spawn_board(commands: &mut Commands, meshes: &mut Assets<Mesh>, mat: &Handle<StandardMaterial>, kit: &Kit, pos: Vec3, facing: Vec3, sign: usize, size: Vec2, legs: bool) {
    // The viewer looks along -facing, so their right is Y x facing.
    let right = Vec3::Y.cross(facing).normalize();
    let mut b = MeshBuilder::default();
    let (hw, h) = (size.x * 0.5, size.y);
    let o = facing * 0.08;
    let q = [-right * hw + o, right * hw + o, right * hw + Vec3::Y * h + o, -right * hw + Vec3::Y * h + o];
    b.quad_uv(q, facing, Color::WHITE, sign_uv(sign));
    let back = [right * hw - o, -right * hw - o, -right * hw + Vec3::Y * h - o, right * hw + Vec3::Y * h - o];
    b.quad_uv(back, -facing, Color::WHITE, sign_uv(sign));
    commands.spawn((Mesh3d(meshes.add(b.build())), MeshMaterial3d(mat.clone()), Transform::from_translation(pos), Scenery));
    // frame + legs
    let yaw = Quat::from_rotation_arc(Vec3::Z, facing);
    commands.spawn((
        Mesh3d(kit.cube.clone()),
        MeshMaterial3d(kit.dark.clone()),
        Transform::from_translation(pos + Vec3::Y * h * 0.5).with_rotation(yaw).with_scale(Vec3::new(size.x + 0.3, h + 0.3, 0.12)),
        Scenery,
    ));
    if legs && pos.y > 0.3 {
        for x in [-hw * 0.7, hw * 0.7] {
            let p = pos + right * x;
            commands.spawn((
                Mesh3d(kit.cylinder.clone()),
                MeshMaterial3d(kit.metal.clone()),
                Transform::from_translation(Vec3::new(p.x, p.y * 0.5, p.z)).with_scale(Vec3::new(0.2, p.y, 0.2)),
                Scenery,
            ));
        }
    }
}

// ----- Prism Park ---------------------------------------------------------------------------

/// Hue `k` of the seven-colour rainbow.
pub fn rainbow(k: usize) -> Color {
    Color::hsl((k % 7) as f32 * 360.0 / 7.0, 0.9, 0.58)
}

/// Rippling water whose normal map scrolls (animated in `ambient.rs`).
#[derive(Resource)]
pub struct WaterMat(pub Handle<StandardMaterial>);

/// Rainbow glass-tile road that glows, polished dark stone verges, neon edge lines, glass
/// barriers with neon rails, a glass-sided skyway on crystal pillars, and the ford.
#[allow(clippy::too_many_arguments)]
pub fn spawn_prism_track(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    surf: &SurfaceMats,
    kit: &Kit,
    tex: &super::textures::Textures,
    track: &Track,
) {
    let n = track.samples.len();
    let mut road: Vec<MeshBuilder> = (0..7).map(|_| MeshBuilder::default()).collect();
    let mut neon: Vec<MeshBuilder> = (0..7).map(|_| MeshBuilder::default()).collect();
    let mut glass: Vec<MeshBuilder> = (0..7).map(|_| MeshBuilder::default()).collect();
    let mut floor = MeshBuilder::default();
    let mut deck = MeshBuilder::default();
    let raised: Vec<bool> = track.samples.iter().map(|s| s.height > 0.8).collect();

    let uv = |i: usize, lat: f32| Vec2::new(lat / 2.5, track.samples[i % n].s / 2.5);
    let strip_uv = |b: &mut MeshBuilder, i: usize, l0: f32, l1: f32, dy: f32| {
        let j = i + 1;
        let (u0, u1) = (uv(i, l0), uv(i, l1));
        // keep v continuous across the lap seam
        let v1 = if j == n { (track.length) / 2.5 } else { uv(j, 0.0).y };
        let v = [at3(track, i, l0, dy), at3(track, i, l1, dy), at3(track, j, l1, dy), at3(track, j, l0, dy)];
        b.quad_uv(v, surface_normal(track, i), Color::WHITE, [u0, u1, Vec2::new(u1.x, v1), Vec2::new(u0.x, v1)]);
    };

    for i in 0..n {
        let band = (track.samples[i].s / 8.0) as usize % 7;
        strip_uv(&mut road[band], i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 0.02);
        strip_uv(&mut floor, i, -WALL_DIST, -ROAD_HALF_WIDTH, 0.0);
        strip_uv(&mut floor, i, ROAD_HALF_WIDTH, WALL_DIST, 0.0);
        // neon edge lines
        for side in [-1.0f32, 1.0] {
            let l = side * (ROAD_HALF_WIDTH + 0.25);
            strip_uv(&mut neon[band], i, l - 0.22, l + 0.22, 0.05);
        }
        // glass barriers with a glowing rail along the top
        let h = 1.3;
        for side in [-1.0f32, 1.0] {
            let (inner, outer) = (side * WALL_DIST, side * (WALL_DIST + 0.35));
            let (a0, a1, b0, b1) = (at3(track, i, inner, 0.0), at3(track, i + 1, inner, 0.0), at3(track, i, outer, 0.0), at3(track, i + 1, outer, 0.0));
            let up = Vec3::Y * h;
            let z = [Vec2::ZERO; 4];
            glass[band].quad_auto([a0, a0 + up, a1 + up, a1], Color::WHITE, z);
            glass[band].quad_auto([b0, b1, b1 + up, b0 + up], Color::WHITE, z);
            let r = Vec3::Y * 0.12;
            neon[band].quad_auto(if side > 0.0 { [a0 + up, b0 + up, b1 + up, a1 + up] } else { [a0 + up, a1 + up, b1 + up, b0 + up] }, Color::WHITE, z);
            neon[band].quad_auto(if side > 0.0 { [a0 + up - r, a0 + up, a1 + up, a1 + up - r] } else { [a0 + up - r, a1 + up - r, a1 + up, a0 + up] }, Color::WHITE, z);
        }
        // skyway deck underside and edge fascia
        if raised[i] {
            let w = WALL_DIST + 0.35;
            let (l, r) = (at3(track, i, -w, -DECK), at3(track, i, w, -DECK));
            let (l1, r1) = (at3(track, i + 1, -w, -DECK), at3(track, i + 1, w, -DECK));
            deck.quad_auto([l, l1, r1, r], Color::WHITE, [Vec2::ZERO; 4]);
            for side in [-1.0f32, 1.0] {
                let (p0, p1) = (at3(track, i, side * w, 0.0), at3(track, i + 1, side * w, 0.0));
                let d = Vec3::Y * DECK;
                let q = if side > 0.0 { [p0 - d, p1 - d, p1, p0] } else { [p0 - d, p0, p1, p1 - d] };
                deck.quad_auto(q, Color::WHITE, [Vec2::ZERO; 4]);
                let s = Vec3::Y * 0.25;
                let q = if side > 0.0 { [p0 - d * 0.5 - s, p1 - d * 0.5 - s, p1 - d * 0.5, p0 - d * 0.5] } else { [p0 - d * 0.5 - s, p0 - d * 0.5, p1 - d * 0.5, p1 - d * 0.5 - s] };
                let off = v3(track.samples[i].dir.perp() * side * 0.02, 0.0);
                neon[band].quad_auto(q.map(|p| p + off), Color::WHITE, [Vec2::ZERO; 4]);
            }
        }
    }

    for k in 0..7 {
        let hue = rainbow(k).to_linear();
        let road_mat = materials.add(StandardMaterial {
            base_color: rainbow(k).mix(&Color::WHITE, 0.25),
            base_color_texture: Some(tex.tiles.clone()),
            emissive: hue * 0.9,
            emissive_texture: Some(tex.tiles.clone()),
            perceptual_roughness: 0.12,
            clearcoat: 1.0,
            clearcoat_perceptual_roughness: 0.03,
            ..default()
        });
        let neon_mat = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: hue * 12.0 + LinearRgba::rgb(2.0, 2.0, 2.0), ..default() });
        let glass_mat = materials.add(StandardMaterial {
            base_color: rainbow(k).with_alpha(0.22),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.02,
            reflectance: 0.9,
            emissive: hue * 0.25,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let r = std::mem::take(&mut road[k]);
        let ne = std::mem::take(&mut neon[k]);
        let g = std::mem::take(&mut glass[k]);
        commands.spawn((Mesh3d(meshes.add(r.build())), MeshMaterial3d(road_mat), NotShadowCaster, Scenery));
        commands.spawn((Mesh3d(meshes.add(ne.build())), MeshMaterial3d(neon_mat), NotShadowCaster, Scenery));
        commands.spawn((Mesh3d(meshes.add(g.build())), MeshMaterial3d(glass_mat), NotShadowCaster, Scenery));
    }
    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.07, 0.1),
        base_color_texture: Some(tex.tiles.clone()),
        perceptual_roughness: 0.06,
        reflectance: 0.6,
        ..default()
    });
    commands.spawn((Mesh3d(meshes.add(floor.build())), MeshMaterial3d(floor_mat.clone()), NotShadowCaster, Scenery));
    commands.spawn((Mesh3d(meshes.add(deck.build())), MeshMaterial3d(floor_mat), Scenery));

    // Start line, grid and boost pads as on the other tracks.
    let mut paint = MeshBuilder::default();
    start_line(track, &mut paint, true);
    commands.spawn((Mesh3d(meshes.add(paint.build())), MeshMaterial3d(surf.paint.clone()), NotShadowCaster, Scenery));
    spawn_pads_and_ramps(commands, meshes, materials, surf, track);

    // Crystal pillars under the skyway: glass hexagonal prisms with glowing cores.
    let prism_mesh = meshes.add(Cylinder::new(0.5, 1.0).mesh().resolution(6));
    let crystal = materials.add(StandardMaterial {
        base_color: Color::srgba(0.7, 0.85, 1.0, 0.3),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.02,
        reflectance: 0.9,
        ..default()
    });
    let core = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(2.0, 6.0, 12.0), ..default() });
    for i in (0..n).step_by(16) {
        if !raised[i] || track.height_idx(i) < 2.0 {
            continue;
        }
        let h = track.height_idx(i) - DECK;
        for lat in [-9.0, 9.0] {
            let p = at(track, i, lat);
            commands.spawn((Mesh3d(prism_mesh.clone()), MeshMaterial3d(crystal.clone()), Transform::from_translation(v3(p, h * 0.5)).with_scale(Vec3::new(2.4, h, 2.4)), NotShadowCaster, Scenery));
            commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(core.clone()), Transform::from_translation(v3(p, h * 0.5)).with_scale(Vec3::new(0.35, h, 0.35)), NotShadowCaster, Scenery));
        }
    }

    spawn_fords(commands, meshes, materials, kit, tex, track, Ford::Night);
}

/// How a ford's water looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ford {
    /// A dark pond with lily pads.
    Pond,
    /// A canal at night, with glowing lily pads.
    Night,
    /// A clear jungle river.
    River,
}

/// Fords: a pond or canal across the road wherever it runs through water, with rippling
/// mirror-like water, rocks along the banks and lily pads (glowing ones at night).
#[allow(clippy::too_many_arguments)]
pub fn spawn_fords(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, kit: &Kit, tex: &super::textures::Textures, track: &Track, look: Ford) {
    let night = look == Ford::Night;
    let water = materials.add(StandardMaterial {
        base_color: match look {
            Ford::Night => Color::srgb(0.02, 0.07, 0.09),
            Ford::Pond => Color::srgb(0.04, 0.13, 0.16),
            Ford::River => Color::srgb(0.03, 0.2, 0.19),
        },
        normal_map_texture: Some(tex.ripples.clone()),
        perceptual_roughness: 0.03,
        reflectance: 0.7,
        ..default()
    });
    commands.insert_resource(WaterMat(water.clone()));
    let mut rng = super::mesh::Lcg(0xF0_4D);
    for &(ws, len) in &track.water {
        let mid = ws + len * 0.5;
        let (c, d) = track.point_at(mid);
        let y = track.height_at(mid) + 0.22;
        let along = len * 0.5 + 3.0;
        let across = 75.0;
        let k = 64;
        let wob: Vec<f32> = (0..k).map(|_| rng.range(0.92, 1.08)).collect();
        let edge = |j: usize| {
            let a = j as f32 / k as f32 * std::f32::consts::TAU;
            let w = wob[j % k];
            // squarer than an ellipse so it spans the road cleanly
            let (ca, sa) = (a.cos(), a.sin());
            let sq = |x: f32| x.signum() * x.abs().powf(0.6);
            c + d * sq(ca) * along * w + d.perp() * sq(sa) * across * w
        };
        let mut b = MeshBuilder::default();
        let m = v3(c, y);
        for j in 0..k {
            let (p0, p1) = (v3(edge(j), y), v3(edge(j + 1), y));
            b.quad_uv([m, p1, p0, m], Vec3::Y, Color::WHITE, [m, p1, p0, m].map(|p| Vec2::new(p.x, p.z) / 9.0));
        }
        let mut mesh = b.build();
        let _ = mesh.generate_tangents();
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(water.clone()), NotShadowCaster, Scenery));
        // rocks along the banks, away from the road
        for j in (0..k).step_by(2) {
            let p = edge(j);
            let lat = (p - c).dot(d.perp()).abs();
            if lat < WALL_DIST + 2.0 {
                continue;
            }
            let s = rng.range(1.2, 3.0);
            commands.spawn((
                Mesh3d(kit.sphere.clone()),
                MeshMaterial3d(kit.dark.clone()),
                Transform::from_translation(v3(p, 0.1)).with_scale(Vec3::new(s * 1.6, s * 0.7, s * 1.3)).with_rotation(Quat::from_rotation_y(rng.f() * 6.0)),
                Scenery,
            ));
        }
        // glowing lily pads out on the water
        let lily = if night {
            [materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(0.5, 4.0, 1.5), ..default() }), materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(4.0, 0.8, 3.0), ..default() })]
        } else {
            [materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.5, 0.15), perceptual_roughness: 0.6, ..default() }), materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.75, 0.85), perceptual_roughness: 0.6, ..default() })]
        };
        for j in 0..14 {
            let side = if j % 2 == 0 { 1.0 } else { -1.0 };
            let p = c + d * rng.range(-along * 0.7, along * 0.7) + d.perp() * side * rng.range(WALL_DIST + 5.0, across * 0.75);
            let s = rng.range(0.8, 1.8);
            commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(lily[j % 2].clone()), Transform::from_translation(v3(p, y + 0.03)).with_scale(Vec3::new(s * 2.0, 0.05, s * 2.0)), NotShadowCaster, Scenery));
        }
    }
}

/// Checkered start line, plus painted grid boxes when `grid` is set.
pub fn start_line(track: &Track, paint: &mut MeshBuilder, grid: bool) {
    let squares = 10;
    let sq = 2.0 * ROAD_HALF_WIDTH / squares as f32;
    for row in 0..2 {
        let (c0, d) = track.point_at(row as f32 * sq - sq);
        for col in 0..squares {
            let l0 = -ROAD_HALF_WIDTH + col as f32 * sq;
            let p = |l: f32, a: f32| v3(c0 + d.perp() * l + d * a, track.height_at(0.0) + 0.05);
            let c = if (row + col) % 2 == 0 { Color::WHITE } else { Color::srgb(0.05, 0.05, 0.05) };
            paint.quad([p(l0, 0.0), p(l0 + sq, 0.0), p(l0 + sq, sq), p(l0, sq)], Vec3::Y, c);
        }
    }
    if grid {
        for slot in 0..8 {
            let (row, col) = ((slot / 2) as f32, slot % 2);
            let s = -8.0 - row * 7.0 - col as f32 * 3.5 + 2.0;
            let lat = if col == 0 { -4.5 } else { 4.5 };
            let (c, d) = track.point_at(s);
            let y = track.height_at(s) + 0.05;
            let p = |l: f32, a: f32| v3(c + d.perp() * (lat + l) + d * a, y);
            let white = Color::srgb(0.95, 0.95, 0.92);
            paint.quad([p(-1.6, -0.25), p(1.6, -0.25), p(1.6, 0.0), p(-1.6, 0.0)], Vec3::Y, white);
            for x in [-1.6, 1.35] {
                paint.quad([p(x, -1.5), p(x + 0.25, -1.5), p(x + 0.25, 0.0), p(x, 0.0)], Vec3::Y, white);
            }
        }
    }

}
