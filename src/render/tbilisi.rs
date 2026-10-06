//! Tbilisi Twilight: blue hour in the old town, when the sun has gone and the city's own lights
//! take over. For the raytracer every one of them is a real light.
//!
//! - The Bridge of Peace: a bowed canopy of white steel and glass over the river, with an LED at
//!   every joint of its lattice, all of them mirrored in the polished deck underneath.
//! - The Mtkvari in its stone channel, reflecting the bridges, the lamps and the afterglow.
//! - Old Tbilisi's houses, each with carved wooden balconies painted its own colour, warm
//!   windows behind them, and festoon lights slung across the street.
//! - Wet cobbles under old street lamps; the brick domes of the sulphur baths and their tiled
//!   gateway; Narikala fortress floodlit on its hill with a church and the Mother of Corgis; a
//!   cable car gliding up to it; the TV tower glittering on the far ridge.
//!
//! The built-in lap crosses the river twice. On a track made in the editor the river runs past
//! instead, and everything else goes wherever it fits.

use std::f32::consts::{PI, TAU};

use bevy::{light::NotShadowCaster, prelude::*};

use super::mesh::{v3, Lcg, MeshBuilder};
use super::models::{glow, part, t, TransformExt};
use super::scene::{Build, Scenery};
use super::scenery::{bounds, clearance, find_spot, find_stand, sitting_corgi, spawn_gantry, spawn_grandstand, Reserved, Shuttle, SkyFollow};
use super::shapes::{beam, blob, boxy, cone, festoon, hip_roof, octahedron, side_face, strip, strip_tiled, tinted_mesh, tube};
use super::signs::SIGN_TBILISI;
use super::trackmesh::{at3, bridge_deck, bridge_flags, spawn_fords, spawn_pads_and_ramps, spawn_pillars, spawn_sign, start_line, wall, Ford, WaterMat, DECK, WALL_THICK};
use crate::sim::track::{Track, TrackId, ROAD_HALF_WIDTH, WALL_DIST};

/// How far the river's surface and its bed lie below the streets.
const WATER_Y: f32 = -2.6;
const BED_Y: f32 = -4.5;
const LIMESTONE: Color = Color::srgb(0.62, 0.55, 0.45);

/// The river: a straight channel `half` wide either side of the line through `at` along `dir`.
struct River {
    at: Vec2,
    dir: Vec2,
    half: f32,
}

impl River {
    /// Signed distance from the middle of the river.
    fn offset(&self, p: Vec2) -> f32 {
        (p - self.at).dot(self.dir.perp())
    }
}

/// Where things are on this layout.
struct Plan {
    river: River,
    /// Samples on a bridge: over the river, or over another part of the track.
    bridge: Vec<bool>,
    /// The stretch under the Bridge of Peace's canopy, if any bridge is long enough for it.
    canopy: Option<(usize, usize)>,
    /// Parkland on the far bank, where no houses are built.
    park: Option<(f32, f32)>,
}

impl Plan {
    fn new(track: &Track) -> Self {
        let n = track.samples.len();
        let idx = |s: f32| (s / track.spacing) as usize;
        let mid = |a: &str, b: &str| Some((track.point_at(track.find_mark(a)?).0 + track.point_at(track.find_mark(b)?).0) * 0.5);
        let (lo, hi) = bounds(track);
        // Under the middles of the built-in lap's two bridges; alongside any other layout.
        let river = match (mid("bridge", "bridge_end"), mid("metekhi", "metekhi_end")) {
            (Some(a), Some(b)) => River { at: a, dir: (b - a).normalize(), half: 27.0 },
            _ => River { at: Vec2::new((lo.x + hi.x) * 0.5, hi.y + WALL_DIST + 62.0), dir: -Vec2::X, half: 27.0 },
        };
        let mut bridge = bridge_flags(track);
        for (i, s) in track.samples.iter().enumerate() {
            bridge[i] |= river.offset(s.pos).abs() < river.half + 1.0;
        }
        let canopy = match (track.find_mark("bridge"), track.find_mark("bridge_end")) {
            (Some(s0), Some(s1)) => Some((idx(s0) - 6, idx(s1) + 6)),
            // The longest bridge on a custom track, if it's long enough to be worth it.
            _ => {
                let mut best: Option<(usize, usize)> = None;
                let mut i = 0;
                while i < n {
                    let run = (i..n).take_while(|&j| bridge[j] && track.samples[j].curvature.abs() < 1.0 / 60.0).count();
                    if run >= 44 && best.is_none_or(|(a, b)| run > b - a) {
                        best = Some((i, i + run.min(90)));
                    }
                    i += run.max(1);
                }
                best
            }
        };
        let park = track.find_mark("bridge_end").zip(track.find_mark("metekhi"));
        Plan { river, bridge, canopy, park }
    }

    fn under_canopy(&self, i: usize) -> bool {
        self.canopy.is_some_and(|(a, b)| (a..b).contains(&i))
    }
}

struct Mats {
    /// Stone, stucco, brick, wood and roofs are all coloured by vertex colours.
    stone: Handle<StandardMaterial>,
    stucco: Handle<StandardMaterial>,
    brick: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    iron: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    /// White-painted steel: the canopy's lattice.
    steel: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    /// Unlit windows: dark glass that picks up the street's lights.
    pane: Handle<StandardMaterial>,
    /// Lights. For the raytracer each is one merged mesh, so one light source.
    lit: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
    led: Handle<StandardMaterial>,
    /// The bridge's glowing handrails.
    rail: Handle<StandardMaterial>,
    flood: Handle<StandardMaterial>,
    mosaic: Handle<StandardMaterial>,
    tile: Handle<StandardMaterial>,
    silver: Handle<StandardMaterial>,
    foliage: Handle<StandardMaterial>,
}

impl Mats {
    fn new(b: &mut Build) -> Self {
        let tex = b.tex;
        // For the raytracer these are the town's real lights, small and far apart, so they're
        // given in nits: as bright as real lamps. Otherwise they just glow, and point lights do
        // the lighting.
        let rt = b.raytracing;
        let light = |nits: [f32; 3], glow: [f32; 3]| LinearRgba::from_f32_array_no_alpha(if rt { nits } else { glow });
        let lamp = light([440_000.0, 265_000.0, 110_000.0], [14.0, 8.5, 3.6]);
        let led = light([120_000.0, 135_000.0, 160_000.0], [9.0, 11.0, 14.0]);
        let flood = light([600_000.0, 390_000.0, 170_000.0], [10.0, 6.5, 3.0]);
        let mut add = |m: StandardMaterial| b.mats.add(m);
        Mats {
            stone: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.85, ..default() }),
            stucco: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.8, ..default() }),
            brick: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.brick.clone()), perceptual_roughness: 0.8, ..default() }),
            wood: add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, double_sided: true, cull_mode: None, ..default() }),
            iron: add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.06), metallic: 0.6, perceptual_roughness: 0.45, ..default() }),
            gold: add(StandardMaterial { base_color: Color::srgb(1.0, 0.78, 0.36), metallic: 1.0, perceptual_roughness: 0.22, ..default() }),
            steel: add(StandardMaterial { base_color: Color::srgb(0.92, 0.93, 0.95), perceptual_roughness: 0.3, reflectance: 0.6, ..default() }),
            glass: add(StandardMaterial {
                base_color: Color::srgba(0.7, 0.88, 1.0, 0.12),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.03,
                reflectance: 0.8,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            pane: add(StandardMaterial { base_color: Color::srgb(0.02, 0.03, 0.05), perceptual_roughness: 0.05, reflectance: 0.8, ..default() }),
            lit: add(glow(LinearRgba::rgb(7.0, 4.0, 1.5))),
            lamp: add(glow(lamp)),
            led: add(glow(led)),
            rail: add(glow(LinearRgba::rgb(10.0, 12.0, 15.0))),
            flood: add(glow(flood)),
            mosaic: add(StandardMaterial { base_color: Color::srgb(0.12, 0.42, 0.7), base_color_texture: Some(tex.mosaic.clone()), perceptual_roughness: 0.15, reflectance: 0.6, ..default() }),
            tile: add(StandardMaterial { base_color: Color::srgb(0.08, 0.5, 0.6), perceptual_roughness: 0.12, reflectance: 0.6, ..default() }),
            // Bronze where the sculpt is red, silver where it's white.
            silver: add(StandardMaterial { base_color: Color::srgb(0.9, 0.9, 0.92), metallic: 1.0, perceptual_roughness: 0.28, ..default() }),
            foliage: add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() }),
        }
    }
}

/// Everything for a Tbilisi track: the streets, the river and the city round them.
pub fn spawn(b: &mut Build) {
    let track = b.track;
    let m = Mats::new(b);
    let plan = Plan::new(track);
    let mut rng = Lcg(0x7B_1151 ^ track.length as u32);
    let mut reserved = Reserved(vec![]);

    spawn_surface(b, &m, &plan);
    spawn_river(b, &m, &plan);
    spawn_start(b, &m, &plan, &mut rng, &mut reserved);
    if let Some(span) = plan.canopy {
        spawn_canopy(b, &m, span);
    }
    spawn_baths(b, &m, &plan, &mut rng, &mut reserved);
    spawn_metekhi(b, &m, &mut reserved);
    spawn_streets(b, &m, &plan, &mut rng, &mut reserved);
    spawn_trees(b, &m, &plan, &reserved, &mut rng);
    spawn_fortress(b, &m, &plan, &mut rng);
    spawn_far(b, &mut rng);
}

// ----- Streets -----------------------------------------------------------------------------------

/// Wet cobbles between paved verges and limestone walls; a mirror-polished deck and glass
/// balustrades with lit handrails under the canopy; retaining walls where the road climbs; and
/// an old lamp post every few metres.
fn spawn_surface(b: &mut Build, m: &Mats, plan: &Plan) {
    let track = b.track;
    let n = track.samples.len();
    let (mut road, mut deck, mut verge, mut paint, mut stone, mut glass, mut led, mut iron, mut lamps) =
        <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();

    for i in 0..n {
        let canopy = plan.under_canopy(i);
        let band = if (i / 8) % 2 == 0 { 0.0 } else { 0.02 };
        if canopy {
            strip(&mut deck, track, i, -WALL_DIST, WALL_DIST, 0.02, Color::srgb(0.1 + band, 0.1 + band, 0.115 + band));
        } else {
            strip_tiled(&mut road, track, i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 0.02, Color::srgb(0.62 + band, 0.6 + band, 0.6 + band), 1.8);
            let slab = if (i / 4) % 2 == 0 { 0.0 } else { 0.02 };
            for (l0, l1) in [(-WALL_DIST, -ROAD_HALF_WIDTH), (ROAD_HALF_WIDTH, WALL_DIST)] {
                strip(&mut verge, track, i, l0, l1, 0.0, Color::srgb(0.4 + slab, 0.38 + slab, 0.36 + slab));
            }
        }
        // Granite kerbs; red and white, the flag's colours, through the corners.
        let near_corner = (0..12).any(|o| track.samples[(i + n + o * 3 - 18) % n].curvature.abs() > 1.0 / 90.0);
        let odd = (i / 3) % 2 == 0;
        let kerb = match (near_corner, odd) {
            (true, true) => Color::srgb(0.8, 0.08, 0.1),
            (true, false) => Color::srgb(0.93, 0.92, 0.88),
            (false, true) => Color::srgb(0.5, 0.5, 0.5),
            (false, false) => Color::srgb(0.4, 0.4, 0.41),
        };
        strip(&mut paint, track, i, -ROAD_HALF_WIDTH - 0.4, -ROAD_HALF_WIDTH + 1.1, 0.045, kerb);
        strip(&mut paint, track, i, ROAD_HALF_WIDTH - 1.1, ROAD_HALF_WIDTH + 0.4, 0.045, kerb);

        for side in [-1.0f32, 1.0] {
            if canopy {
                // A low steel sill, a glass panel and a handrail that glows.
                wall(&mut stone, track, &plan.bridge, i, side, 0.5, 0.25, Color::srgb(0.8, 0.8, 0.82), Color::srgb(0.85, 0.85, 0.87));
                for face in [-1.0f32, 1.0] {
                    side_face(&mut glass, track, i, side * (WALL_DIST + 0.25), 0.25, 1.2, face, Color::WHITE);
                }
                for face in [-1.0f32, 1.0] {
                    side_face(&mut led, track, i, side * (WALL_DIST + 0.25 + face * 0.06), 1.2, 1.3, face, Color::WHITE);
                }
            } else {
                let block = if (i / 3) % 2 == 0 { LIMESTONE } else { LIMESTONE.darker(0.04) };
                wall(&mut stone, track, &plan.bridge, i, side, WALL_THICK, 1.05, block, LIMESTONE.lighter(0.05));
                // An iron rail along the top.
                for face in [-1.0f32, 1.0] {
                    side_face(&mut iron, track, i, side * (WALL_DIST + WALL_THICK * 0.5), 1.38, 1.46, face, Color::WHITE);
                }
                if i % 3 == 0 {
                    let p = at3(track, i, side * (WALL_DIST + WALL_THICK * 0.5), 1.05);
                    beam(&mut iron, p, p + Vec3::Y * 0.4, 0.07, 0.07, Vec3::X, Color::WHITE);
                }
            }
        }
        bridge_deck(&mut stone, track, &plan.bridge, i, LIMESTONE.darker(0.1));

        // Street lamps, on alternate sides.
        if i % 11 == 5 && !canopy {
            let side = if (i / 11) % 2 == 0 { 1.0 } else { -1.0 };
            let base = at3(track, i, side * (WALL_DIST + WALL_THICK * 0.5), 1.05);
            let head = base + Vec3::Y * 3.9;
            tube(&mut iron, base, head, 0.09, 0.06, 6, Color::WHITE);
            blob(&mut iron, base + Vec3::Y * 0.12, Vec3::new(0.22, 0.2, 0.22), 6, false, Color::WHITE);
            cone(&mut iron, head + Vec3::Y * 0.62, 0.36, 0.3, 6, Color::WHITE);
            blob(&mut lamps, head + Vec3::Y * 0.3, Vec3::new(0.3, 0.36, 0.3), 8, false, Color::WHITE);
            if !b.raytracing && (i / 11) % 2 == 0 {
                b.commands.spawn((
                    PointLight { color: Color::srgb(1.0, 0.72, 0.42), intensity: 900_000.0, range: 30.0, radius: 0.4, shadow_maps_enabled: false, ..default() },
                    Transform::from_translation(head + Vec3::Y * 0.3),
                    Scenery,
                ));
            }
        }
    }
    start_line(track, &mut paint, true);

    // The cobbles' normal map needs tangents.
    let cobbles = b.mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(b.tex.cobbles.clone()),
        metallic_roughness_texture: Some(b.tex.cobbles_wet.clone()),
        normal_map_texture: Some(b.tex.cobbles_normal.clone()),
        perceptual_roughness: 1.0,
        reflectance: 0.5,
        ..default()
    });
    let mut mesh = road.build();
    let _ = mesh.generate_tangents();
    let (mesh, mat) = tinted_mesh(mesh, b.meshes, b.mats, &cobbles);
    b.commands.spawn((mesh, mat, NotShadowCaster, Scenery));
    // (Mirror polish: see the road in `lagoon.rs`.)
    let granite = b.mats.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(b.tex.tiles.clone()), perceptual_roughness: 0.025, reflectance: 0.6, ..default() });
    b.add_tinted(deck, &granite, false);
    let paving = b.mats.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(b.tex.tiles.clone()), perceptual_roughness: 0.45, ..default() });
    b.add_tinted(verge, &paving, false);
    b.add_tinted(paint, &m.stone, false);
    b.add_tinted(stone, &m.stone, true);
    b.add_flat(glass, &m.glass);
    b.add_flat(led, &m.rail);
    b.add(iron, &m.iron);
    b.add_flat(lamps, &m.lamp);
    spawn_pads_and_ramps(b.commands, b.meshes, b.mats, b.surf, track);
    // Only where the road crosses itself: the river bridges stand on piers (see `spawn_river`).
    spawn_pillars(b.commands, b.kit, &m.stone, track, &bridge_flags(track));
    if !track.water.is_empty() {
        spawn_fords(b.commands, b.meshes, b.mats, b.kit, b.tex, track, Ford::Night);
    }
}

/// The ground, the river in its stone channel, and the piers under the bridges.
fn spawn_river(b: &mut Build, m: &Mats, plan: &Plan) {
    let track = b.track;
    let river = &plan.river;
    let across = river.dir.perp();
    // In the river's own frame: `u` along it, `v` across.
    let p = |u: f32, v: f32, y: f32| v3(river.at + river.dir * u + across * v, y);
    let (mut ground, mut quay, mut water) = <(MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    let (cell, reach) = (60.0f32, 1560.0f32);
    let steps = (reach / cell) as i32;
    for iu in -steps..steps {
        let (u0, u1) = (iu as f32 * cell, (iu + 1) as f32 * cell);
        for side in [-1.0f32, 1.0] {
            for iv in 0..steps {
                let (v0, v1) = (side * (river.half + iv as f32 * cell), side * (river.half + (iv + 1) as f32 * cell));
                let q = [p(u0, v0, -0.03), p(u1, v0, -0.03), p(u1, v1, -0.03), p(u0, v1, -0.03)];
                let k = 1.0 + ((u0 * 0.011).sin() * (v0 * 0.013).cos()) * 0.1;
                ground.quad_facing(q, Vec3::Y, Color::srgb(0.2 * k, 0.2 * k, 0.21 * k), q.map(|c| Vec2::new(c.x, c.z) / 5.0));
            }
            // The quay wall, with a stone coping.
            let v = side * river.half;
            let q = [p(u0, v, BED_Y), p(u1, v, BED_Y), p(u1, v, 0.0), p(u0, v, 0.0)];
            quay.quad_facing(q, v3(-across * side, 0.0), LIMESTONE.darker(0.12), [Vec2::new(u0, BED_Y), Vec2::new(u1, BED_Y), Vec2::new(u1, 0.0), Vec2::new(u0, 0.0)].map(|c| c / 3.0));
            beam(&mut quay, p(u0, v + side * 0.4, 0.15), p(u1, v + side * 0.4, 0.15), 1.0, 0.4, Vec3::Y, LIMESTONE);
        }
        let q = [p(u0, -river.half, WATER_Y), p(u1, -river.half, WATER_Y), p(u1, river.half, WATER_Y), p(u0, river.half, WATER_Y)];
        water.quad_facing(q, Vec3::Y, Color::WHITE, [Vec2::new(u0, -river.half), Vec2::new(u1, -river.half), Vec2::new(u1, river.half), Vec2::new(u0, river.half)].map(|c| c / 9.0));
    }
    // A pier under each third of every bridge over the water.
    let n = track.samples.len();
    let over: Vec<bool> = track.samples.iter().map(|s| river.offset(s.pos).abs() < river.half).collect();
    let mut i = 0;
    while i < n {
        let run = (i..n).take_while(|&j| over[j]).count();
        for k in 1..=2 {
            if run > 20 {
                let j = i + run * k / 3;
                let s = &track.samples[j];
                let (fwd, out) = (v3(s.dir, 0.0), v3(s.dir.perp(), 0.0));
                boxy(&mut quay, v3(s.pos, BED_Y), fwd, out, 1.6, track.height_idx(j) - DECK - BED_Y, WALL_DIST - 2.0, LIMESTONE.darker(0.06));
            }
        }
        i += run.max(1);
    }

    let paving = b.mats.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(b.tex.concrete.clone()), perceptual_roughness: 0.9, ..default() });
    b.commands.spawn((ground.tinted(b.meshes, b.mats, &paving), NotShadowCaster, super::gfx::RaytraceSink(0.1), Scenery));
    b.add_tinted(quay, &m.stone, true);
    let mut mesh = water.build();
    let _ = mesh.generate_tangents();
    let surface = b.mats.add(StandardMaterial {
        base_color: Color::srgb(0.015, 0.035, 0.06),
        normal_map_texture: Some(b.tex.ripples.clone()),
        perceptual_roughness: 0.05,
        reflectance: 0.6,
        ..default()
    });
    b.commands.insert_resource(WaterMat(surface.clone()));
    b.commands.spawn((Mesh3d(b.meshes.add(mesh)), MeshMaterial3d(surface), NotShadowCaster, Scenery));
}

/// The start: a stone gantry, the grandstand, and sponsors' boards along the straights.
fn spawn_start(b: &mut Build, m: &Mats, plan: &Plan, rng: &mut Lcg, reserved: &mut Reserved) {
    let track = b.track;
    let pillar = b.mats.add(StandardMaterial { base_color: LIMESTONE, base_color_texture: Some(b.tex.concrete.clone()), perceptual_roughness: 0.85, ..default() });
    spawn_gantry(b.commands, b.kit, b.meshes, &b.signs.material, track, &pillar, &m.gold, SIGN_TBILISI);
    let stand = if matches!(track.id, TrackId::Custom(_)) { find_stand(track, 120.0, 12.0) } else { Some((25.0, 180.0, -1.0)) };
    if let Some((s0, s1, side)) = stand {
        spawn_grandstand(b.commands, b.kit, b.meshes, b.surf, track, s0, s1, side, rng, reserved);
    }
    let n = track.samples.len();
    let (mut s, mut k) = (stand.map_or(60.0, |st| st.1 + 40.0), 0);
    while s < track.length - 30.0 {
        let i = (s / track.spacing) as usize % n;
        let straight = (0..9).all(|o| track.samples[(i + o * 3) % n].curvature.abs() < 1.0 / 150.0);
        if straight && !track.in_water(s) && !plan.under_canopy(i) && track.height_idx(i) < 0.5 {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let sign = [SIGN_TBILISI + 1, 7, SIGN_TBILISI, 4, 1][k % 5];
            spawn_sign(b.commands, b.meshes, &b.signs.material, b.kit, track, s, side * (WALL_DIST + WALL_THICK * 0.5), sign, Vec2::new(7.0, 1.5), 1.5);
            k += 1;
            s += 110.0;
        } else {
            s += 10.0;
        }
    }
}

/// The Bridge of Peace's canopy over samples `i0..i1`: a lattice of white steel tubes bowed over
/// the deck, highest and widest in the middle, skinned in glass, with an LED at every joint.
fn spawn_canopy(b: &mut Build, m: &Mats, (i0, i1): (usize, usize)) {
    let track = b.track;
    let (mut steel, mut glass, mut leds) = <(MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    const ACROSS: usize = 12;
    let rows = (i1 - i0) / 2;
    // Joint `j` across on row `r`: rows are 2m apart, and alternate rows are staggered half a
    // step so the tubes between them make diamonds.
    let node = |r: usize, j: f32| {
        let i = i0 + r * 2;
        let u = r as f32 / rows as f32;
        let swell = (PI * u).sin().max(0.0);
        let (w, h) = (WALL_DIST + 1.2 + 3.2 * swell, 4.6 + 7.0 * swell.powf(0.7));
        let a = (j / ACROSS as f32).clamp(0.0, 1.0) * PI;
        let s = &track.samples[i % track.samples.len()];
        v3(s.pos + s.dir.perp() * w * a.cos(), track.height_idx(i) + 0.3 + h * a.sin())
    };
    let white = Color::WHITE;
    for r in 0..rows {
        let stagger = if r % 2 == 0 { 0.0 } else { 0.5 };
        for j in 0..=ACROSS {
            let j0 = j as f32 + stagger;
            if j0 > ACROSS as f32 {
                continue;
            }
            let p = node(r, j0);
            let inward = (v3(track.samples[(i0 + r * 2) % track.samples.len()].pos, p.y * 0.3) - p).normalize_or(-Vec3::Y);
            octahedron(&mut leds, p + inward * 0.22, 0.1);
            // Down to the two joints nearest it on the next row.
            for j1 in [j0 - 0.5, j0 + 0.5] {
                if (0.0..=ACROSS as f32).contains(&j1) && r + 1 <= rows {
                    let q = node(r + 1, j1);
                    tube(&mut steel, p, q, 0.075, 0.075, 4, white);
                }
            }
            // The glass: a diamond between this joint, its two below and the one two rows on.
            if r + 2 <= rows && j0 >= 0.5 && j0 + 0.5 <= ACROSS as f32 {
                let quad = [p, node(r + 1, j0 - 0.5), node(r + 2, j0), node(r + 1, j0 + 0.5)];
                glass.quad_facing(quad, -inward, Color::WHITE, [Vec2::ZERO; 4]);
            }
        }
    }
    // The two edge beams the lattice springs from, and the ribs at each end.
    for edge in [0.0, ACROSS as f32] {
        for r in 0..rows {
            tube(&mut steel, node(r, edge), node(r + 1, edge), 0.16, 0.16, 6, white);
        }
    }
    for r in [0, rows] {
        for j in 0..ACROSS {
            tube(&mut steel, node(r, j as f32), node(r, j as f32 + 1.0), 0.16, 0.16, 6, white);
        }
    }
    b.add(steel, &m.steel);
    b.add_flat(glass, &m.glass);
    b.add_flat(leds, &m.led);
    if !b.raytracing {
        for r in (2..rows).step_by(6) {
            b.commands.spawn((
                PointLight { color: Color::srgb(0.8, 0.9, 1.0), intensity: 1_500_000.0, range: 34.0, radius: 1.0, shadow_maps_enabled: false, ..default() },
                Transform::from_translation(node(r, ACROSS as f32 * 0.5) - Vec3::Y * 1.5),
                Scenery,
            ));
        }
    }
}

/// The meshes a street of houses is merged into.
#[derive(Default)]
struct Street {
    walls: MeshBuilder,
    roofs: MeshBuilder,
    wood: MeshBuilder,
    panes: MeshBuilder,
    lit: MeshBuilder,
    wire: MeshBuilder,
    bulbs: MeshBuilder,
}

const WALL_PAINT: [Color; 7] = [
    Color::srgb(0.82, 0.62, 0.36),
    Color::srgb(0.8, 0.52, 0.45),
    Color::srgb(0.62, 0.72, 0.55),
    Color::srgb(0.55, 0.68, 0.78),
    Color::srgb(0.88, 0.82, 0.7),
    Color::srgb(0.66, 0.34, 0.26),
    Color::srgb(0.86, 0.72, 0.5),
];
const WOOD_PAINT: [Color; 6] = [
    Color::srgb(0.16, 0.55, 0.56),
    Color::srgb(0.16, 0.3, 0.62),
    Color::srgb(0.9, 0.9, 0.86),
    Color::srgb(0.42, 0.26, 0.15),
    Color::srgb(0.18, 0.44, 0.26),
    Color::srgb(0.55, 0.12, 0.17),
];
const ROOF_PAINT: [Color; 3] = [Color::srgb(0.5, 0.2, 0.14), Color::srgb(0.62, 0.3, 0.2), Color::srgb(0.3, 0.36, 0.34)];

/// An old Tbilisi house, its front `w` wide and centred on `front`, facing `-out`: stucco walls
/// under a tin roof, a shop or a door at street level, and on every floor above a wooden balcony
/// right across the front, with turned posts, a lattice rail, fretwork arches and tall doors.
fn house(st: &mut Street, front: Vec3, fwd: Vec3, out: Vec3, w: f32, depth: f32, floors: usize, rng: &mut Lcg) {
    const GROUND: f32 = 3.4;
    const FLOOR: f32 = 3.1;
    let h = GROUND + (floors - 1) as f32 * FLOOR + 0.5;
    let (paint, trim) = (*rng.pick(&WALL_PAINT), *rng.pick(&WOOD_PAINT));
    boxy(&mut st.walls, front + out * depth * 0.5, fwd, out, w * 0.5, h, depth * 0.5, paint);
    hip_roof(&mut st.roofs, front + out * depth * 0.5 + Vec3::Y * h, fwd, out, w * 0.5 + 0.5, depth * 0.5 + 0.5, 1.9, *rng.pick(&ROOF_PAINT));
    // A point on the facade: `x` along it, `y` up, `push` out towards the street.
    let p = |x: f32, y: f32, push: f32| front + fwd * x + Vec3::Y * y - out * push;
    let window = |st: &mut Street, x: f32, y: f32, ww: f32, wh: f32, push: f32, chance: f32, rng: &mut Lcg| {
        let q = [p(x - ww * 0.5, y, push), p(x + ww * 0.5, y, push), p(x + ww * 0.5, y + wh, push), p(x - ww * 0.5, y + wh, push)];
        (if rng.f() < chance { &mut st.lit } else { &mut st.panes }).quad_facing(q, -out, Color::WHITE, [Vec2::ZERO; 4]);
    };
    side_windows(st, front + out * depth * 0.5, fwd, out, w * 0.5, depth * 0.5, floors, rng);
    // Street level: a wooden door and a shop window or two.
    let door = rng.range(-w * 0.3, w * 0.3);
    st.wood.quad_facing([p(door - 0.6, 0.0, 0.03), p(door + 0.6, 0.0, 0.03), p(door + 0.6, 2.4, 0.03), p(door - 0.6, 2.4, 0.03)], -out, trim.darker(0.12), [Vec2::ZERO; 4]);
    for x in [door - 2.4, door + 2.4] {
        if x.abs() < w * 0.5 - 1.0 {
            window(st, x, 0.9, 1.5, 1.6, 0.03, 0.55, rng);
        }
    }
    // Balconies.
    let bays = ((w - 0.4) / 1.55) as usize;
    let bay = (w - 0.4) / bays as f32;
    const REACH: f32 = 1.5;
    for f in 0..floors - 1 {
        let y = GROUND + f as f32 * FLOOR;
        let (x0, x1) = (-w * 0.5 + 0.2, w * 0.5 - 0.2);
        boxy(&mut st.wood, p(0.0, y - 0.18, REACH * 0.5), fwd, out, w * 0.5 - 0.1, 0.18, REACH * 0.5, trim.darker(0.1));
        beam(&mut st.wood, p(x0, y + 0.95, REACH), p(x1, y + 0.95, REACH), 0.1, 0.1, Vec3::Y, trim);
        beam(&mut st.wood, p(x0, y + 2.55, REACH), p(x1, y + 2.55, REACH), 0.14, 0.2, Vec3::Y, trim);
        // Its tin roof, sloping away from the wall.
        let roof = [p(x0 - 0.2, y + 2.95, 0.0), p(x1 + 0.2, y + 2.95, 0.0), p(x1 + 0.2, y + 2.66, REACH + 0.25), p(x0 - 0.2, y + 2.66, REACH + 0.25)];
        st.roofs.quad_facing(roof, Vec3::Y, ROOF_PAINT[2], [Vec2::ZERO; 4]);
        st.roofs.quad_facing(roof, -Vec3::Y, ROOF_PAINT[2].darker(0.1), [Vec2::ZERO; 4]);
        for k in 0..=bays {
            let x = x0 + k as f32 * bay;
            beam(&mut st.wood, p(x, y, REACH), p(x, y + 2.55, REACH), 0.12, 0.12, fwd, trim);
            if k == bays {
                break;
            }
            // The rail's lattice: flat boards crossed in each bay.
            for (a, c) in [(0.0, 1.0), (1.0, 0.0)] {
                let (pa, pc) = (p(x + bay * a, y + 0.05, REACH), p(x + bay * c, y + 0.9, REACH));
                st.wood.quad_facing([pa - Vec3::Y * 0.04, pc - Vec3::Y * 0.04, pc + Vec3::Y * 0.04, pa + Vec3::Y * 0.04], -out, trim, [Vec2::ZERO; 4]);
            }
            // Fretwork: an arch of thin board hung under the top beam.
            const N: usize = 5;
            let arch = |j: usize, drop: f32| {
                let a = j as f32 / N as f32 * PI;
                p(x + bay * 0.5 - bay * 0.5 * a.cos(), y + 2.45 - (0.42 + drop) * (1.0 - a.sin()), REACH)
            };
            for j in 0..N {
                st.wood.quad_facing([arch(j, 0.0), arch(j + 1, 0.0), arch(j + 1, 0.22), arch(j, 0.22)], -out, trim, [Vec2::ZERO; 4]);
            }
            // A tall glazed door behind.
            window(st, x + bay * 0.5, y + 0.1, bay * 0.6, 2.2, 0.03, 0.5, rng);
        }
    }
}

/// Windows on the back and the two ends of a house `hx` by `hz` (half sizes) about `centre`.
#[allow(clippy::too_many_arguments)]
fn side_windows(st: &mut Street, centre: Vec3, fwd: Vec3, out: Vec3, hx: f32, hz: f32, floors: usize, rng: &mut Lcg) {
    for (normal, along, half, off) in [(out, fwd, hx, hz), (fwd, out, hz, hx), (-fwd, out, hz, hx)] {
        let count = (half / 1.5) as usize;
        for f in 0..floors {
            for k in 0..count {
                let x = (k as f32 + 0.5) / count as f32 * 2.0 * half - half;
                let p = centre + normal * (off + 0.03) + along * x + Vec3::Y * (1.2 + f as f32 * 3.1);
                let q = [p - along * 0.5, p + along * 0.5, p + along * 0.5 + Vec3::Y * 1.5, p - along * 0.5 + Vec3::Y * 1.5];
                (if rng.f() < 0.3 { &mut st.lit } else { &mut st.panes }).quad_facing(q, normal, Color::WHITE, [Vec2::ZERO; 4]);
            }
        }
    }
}

/// Houses along both sides of the road wherever there's room for them (but not between the
/// road and the river, nor in the park), festoon lights across the road between them.
fn spawn_streets(b: &mut Build, m: &Mats, plan: &Plan, rng: &mut Lcg, reserved: &mut Reserved) {
    let track = b.track;
    let n = track.samples.len();
    let river = &plan.river;
    let mut st = Street::default();
    let dry = |p: Vec2| river.offset(p).abs() > river.half + 5.0;
    // Samples with a house on the left, and on the right.
    let mut housed = [vec![false; n], vec![false; n]];
    for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let mut i = 12;
        while i + 14 < n {
            let (w, depth, floors) = (rng.range(8.5, 12.5), rng.range(8.0, 11.0), 2 + (rng.f() * 2.3) as usize);
            let span = (w / track.spacing).ceil() as usize + 1;
            let j = i + span / 2;
            i += span;
            let s = &track.samples[j];
            let out = s.dir.perp() * side;
            let front = s.pos + out * (WALL_DIST + WALL_THICK + 3.2);
            let centre = front + out * depth * 0.5;
            let in_park = plan.park.is_some_and(|(s0, s1)| (s0 - 20.0..s1).contains(&s.s));
            // Facing the river across the road, close by: leave the embankment open.
            let riverside = out.dot(river.dir.perp()) * river.offset(s.pos) < 0.0 && river.offset(s.pos).abs() < river.half + 100.0;
            let corners = [-1.0f32, 1.0].map(|x| centre + s.dir * x * w * 0.5 + out * depth * 0.5);
            let clear = clearance(track, centre) > depth * 0.5 + 2.0 && corners.iter().all(|&c| clearance(track, c) > 2.0 && dry(c)) && dry(front);
            if in_park || riverside || !clear || s.height > 2.5 || s.curvature * side > 1.0 / 60.0 || !reserved.free(centre, depth * 0.5 + 1.0) || plan.bridge[j] {
                continue;
            }
            reserved.add(centre, w.max(depth) * 0.5);
            house(&mut st, v3(front, 0.0), v3(s.dir, 0.0), v3(out, 0.0), w, depth, floors, rng);
            for o in 0..span {
                housed[k][(j + o).saturating_sub(span / 2) % n] = true;
            }
        }
    }
    // The rest of the town, behind the streets: plainer houses, windows lit here and there.
    let (lo, hi) = bounds(track);
    let (mut placed, mut tries) = (0, 0);
    while placed < 260 && tries < 30_000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 230.0, hi.x + 230.0), rng.range(lo.y - 230.0, hi.y + 230.0));
        let (w, depth) = (rng.range(8.0, 14.0), rng.range(8.0, 12.0));
        let c = clearance(track, p);
        let in_park = plan.park.is_some_and(|(s0, s1)| (s0..s1).contains(&track.samples[track.locate(p, 0).idx].s)) && c < 80.0;
        if !(26.0..240.0).contains(&c) || in_park || !reserved.free(p, 10.0) || river.offset(p).abs() < river.half + 12.0 {
            continue;
        }
        placed += 1;
        reserved.add(p, 8.0);
        let fwd = Quat::from_rotation_y(rng.range(-0.3, 0.3) + if rng.f() < 0.5 { 0.0 } else { PI * 0.5 }) * Vec3::X;
        let out = fwd.cross(Vec3::Y);
        let floors = 2 + (rng.f() * 2.6) as usize;
        let h = floors as f32 * 3.1 + 0.8;
        boxy(&mut st.walls, v3(p, 0.0), fwd, out, w * 0.5, h, depth * 0.5, *rng.pick(&WALL_PAINT));
        hip_roof(&mut st.roofs, v3(p, h), fwd, out, w * 0.5 + 0.5, depth * 0.5 + 0.5, 1.9, *rng.pick(&ROOF_PAINT));
        side_windows(&mut st, v3(p, 0.0), fwd, out, w * 0.5, depth * 0.5, floors, rng);
        side_windows(&mut st, v3(p, 0.0), -fwd, -out, w * 0.5, depth * 0.5, floors, rng);
    }
    // Festoons from eaves to eaves wherever both sides are built up.
    for i in (10..n - 10).step_by(17) {
        if housed[0][i] && housed[1][i + 3] && track.height_idx(i) < 1.0 {
            let lat = WALL_DIST + WALL_THICK + 3.2;
            festoon(&mut st.wire, &mut st.bulbs, at3(track, i, -lat, 7.4), at3(track, i + 3, lat, 7.4), 2.4, 22);
        }
    }
    b.add_tinted(st.walls, &m.stucco, true);
    b.add_tinted(st.roofs, &m.wood, true);
    b.add_tinted(st.wood, &m.wood, true);
    b.add_flat(st.panes, &m.pane);
    b.add_flat(st.lit, &m.lit);
    b.add_flat(st.wire, &m.iron);
    b.add_flat(st.bulbs, &m.lamp);
}

/// Abanotubani: the brick domes of the sulphur baths, each with a lantern on top, and the
/// blue-tiled gateway of the Orbeliani bath behind them.
fn spawn_baths(b: &mut Build, m: &Mats, plan: &Plan, rng: &mut Lcg, reserved: &mut Reserved) {
    let track = b.track;
    let s = track.find_mark("baths").map_or(track.length * 0.62, |s| s + 10.0);
    let (c, d) = track.point_at(s);
    let dry = |p: Vec2| plan.river.offset(p).abs() > plan.river.half + 30.0;
    let spot = [1.0f32, -1.0].into_iter().find_map(|side| find_spot(track, c + d.perp() * side * 48.0, 27.0, reserved, 12).filter(|&p| dry(p)));
    let Some(at) = spot else { return };
    reserved.add(at, 27.0);
    let toward = (c - at).normalize();
    let (fwd, out) = (v3(toward.perp(), 0.0), v3(-toward, 0.0));
    let base = v3(at, 0.0);
    let (mut brick, mut stone, mut lamps, mut tiles, mut mosaic, mut floods) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    let brick_red = Color::srgb(0.6, 0.33, 0.24);
    // A raised brick terrace with the domes growing out of it.
    boxy(&mut brick, base, fwd, out, 21.0, 1.0, 13.0, brick_red.darker(0.05));
    for k in 0..9 {
        let (x, z) = ((k % 3) as f32 * 12.5 - 12.5 + rng.range(-1.5, 1.5), (k / 3) as f32 * 7.0 - 8.5 + rng.range(-1.0, 1.0));
        let r = rng.range(3.0, 4.8);
        let foot = base + fwd * x + out * z + Vec3::Y * 1.0;
        tube(&mut brick, foot, foot + Vec3::Y * 0.9, r, r, 16, brick_red.darker(0.03));
        blob(&mut brick, foot + Vec3::Y * 0.9, Vec3::new(r, r * 0.72, r), 16, true, brick_red.lighter(rng.range(0.0, 0.06)));
        // The lantern that lets the steam out, glowing from the bath below.
        let top = foot + Vec3::Y * (0.9 + r * 0.72);
        tube(&mut stone, top - Vec3::Y * 0.2, top + Vec3::Y * 0.7, 0.55, 0.5, 8, LIMESTONE);
        blob(&mut lamps, top + Vec3::Y * 0.85, Vec3::new(0.42, 0.3, 0.42), 8, false, Color::WHITE);
        cone(&mut stone, top + Vec3::Y * 1.05, 0.7, 0.5, 8, LIMESTONE.darker(0.1));
    }
    // The gateway: a tiled front with a deep pointed arch, between two little minarets.
    let gate = base + out * 13.5;
    let (gw, gh) = (9.0, 11.0);
    boxy(&mut brick, gate + out * 2.0, fwd, out, gw, gh, 2.0, brick_red);
    let face = |x: f32, y: f32, push: f32| gate + fwd * x + Vec3::Y * y - out * push;
    let q = [face(-gw, 1.0, 0.04), face(gw, 1.0, 0.04), face(gw, gh, 0.04), face(-gw, gh, 0.04)];
    mosaic.quad_facing(q, -out, Color::WHITE, [Vec2::new(0.0, 0.0), Vec2::new(gw / 2.0, 0.0), Vec2::new(gw / 2.0, gh / 4.0), Vec2::new(0.0, gh / 4.0)]);
    // The arch: a warm-lit recess.
    const N: usize = 8;
    let arch = |j: usize| {
        let f = j as f32 / N as f32;
        let x = (f * 2.0 - 1.0) * 3.2;
        // pointed: two arcs meeting at the top
        (x, 1.0 + 7.6 * (1.0 - (x.abs() / 3.2).powf(1.7)).max(0.0).powf(0.6))
    };
    for j in 0..N {
        let ((xa, ya), (xb, yb)) = (arch(j), arch(j + 1));
        lamps.quad_facing([face(xa, 1.0, 0.08), face(xb, 1.0, 0.08), face(xb, yb.max(1.0), 0.08), face(xa, ya.max(1.0), 0.08)], -out, Color::WHITE, [Vec2::ZERO; 4]);
    }
    for sx in [-1.0f32, 1.0] {
        let foot = gate + fwd * sx * (gw + 0.6) + out * 2.0;
        tube(&mut tiles, foot, foot + Vec3::Y * (gh + 3.5), 1.3, 1.1, 12, Color::WHITE);
        blob(&mut tiles, foot + Vec3::Y * (gh + 3.5), Vec3::new(1.5, 1.9, 1.5), 12, true, Color::WHITE);
        boxy(&mut floods, gate + fwd * sx * 5.5 - out * 6.0, fwd, out, 0.5, 0.35, 0.3, Color::WHITE);
    }
    b.add_tinted(brick, &m.brick, true);
    b.add_tinted(stone, &m.stone, true);
    b.add_flat(lamps, &m.lit);
    b.add(tiles, &m.tile);
    b.add(mosaic, &m.mosaic);
    b.add_flat(floods, &m.flood);
    if !b.raytracing {
        b.commands.spawn((PointLight { color: Color::srgb(1.0, 0.7, 0.4), intensity: 4_000_000.0, range: 45.0, radius: 1.0, shadow_maps_enabled: false, ..default() }, Transform::from_translation(gate - out * 8.0 + Vec3::Y * 5.0), Scenery));
    }
}

/// Plane trees in autumn colour along the streets and all over the park.
fn spawn_trees(b: &mut Build, m: &Mats, plan: &Plan, reserved: &Reserved, rng: &mut Lcg) {
    let track = b.track;
    let (lo, hi) = bounds(track);
    let (mut trunks, mut crowns) = <(MeshBuilder, MeshBuilder)>::default();
    let autumn = [Color::srgb(0.75, 0.5, 0.12), Color::srgb(0.6, 0.3, 0.1), Color::srgb(0.3, 0.4, 0.14), Color::srgb(0.8, 0.62, 0.18), Color::srgb(0.22, 0.34, 0.15)];
    let (mut placed, mut tries) = (0, 0);
    while placed < 230 && tries < 30_000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 70.0, hi.x + 70.0), rng.range(lo.y - 70.0, hi.y + 70.0));
        let c = clearance(track, p);
        let i = track.locate(p, 0).idx;
        let in_park = plan.park.is_some_and(|(s0, s1)| (s0..s1).contains(&track.samples[i].s));
        // Thick in the park, a scattering elsewhere.
        if !(3.5..if in_park { 70.0 } else { 26.0 }).contains(&c) || (!in_park && rng.f() < 0.6) || !reserved.free(p, 4.0) || plan.river.offset(p).abs() < plan.river.half + 3.0 {
            continue;
        }
        placed += 1;
        let (h, r) = (rng.range(5.0, 8.5), rng.range(3.2, 5.2));
        tube(&mut trunks, v3(p, 0.0), v3(p, h), 0.4, 0.25, 6, Color::srgb(0.42, 0.38, 0.32));
        let col = *rng.pick(&autumn);
        for k in 0..3 {
            let o = Vec3::new(rng.range(-0.5, 0.5), k as f32 * 0.35, rng.range(-0.5, 0.5)) * r;
            blob(&mut crowns, v3(p, h + r * 0.3) + o, Vec3::new(r, r * 0.75, r) * rng.range(0.7, 1.0), 8, false, col.mix(rng.pick(&autumn), 0.3));
        }
    }
    b.add_tinted(trunks, &m.foliage, true);
    b.add_tinted(crowns, &m.foliage, true);
}

// ----- The hill ----------------------------------------------------------------------------------

/// A little Georgian church standing on `base`, its nave along `fwd`: a cross of gabled halls
/// with a drum over the crossing and a pointed roof on the drum.
fn church(stone: &mut MeshBuilder, roofs: &mut MeshBuilder, gold: &mut MeshBuilder, lit: &mut MeshBuilder, base: Vec3, fwd: Vec3, k: f32) {
    let out = fwd.cross(Vec3::Y);
    let warm = Color::srgb(0.72, 0.6, 0.44);
    let tile = Color::srgb(0.5, 0.24, 0.16);
    for (a, c, hx, hz) in [(fwd, out, 7.0, 3.2), (out, fwd, 5.2, 3.0)] {
        boxy(stone, base, a, c, hx * k, 7.0 * k, hz * k, warm);
        hip_roof(roofs, base + Vec3::Y * 7.0 * k, a, c, hx * k + 0.3, hz * k + 0.3, 2.4 * k, tile);
    }
    let drum = base + Vec3::Y * 8.6 * k;
    tube(stone, drum, drum + Vec3::Y * 4.6 * k, 2.6 * k, 2.6 * k, 12, warm.lighter(0.03));
    cone(roofs, drum + Vec3::Y * 4.6 * k, 3.2 * k, 5.2 * k, 12, tile);
    // Slit windows round the drum, lit from inside.
    for j in 0..6 {
        let d = Quat::from_rotation_y(j as f32 * TAU / 6.0) * fwd;
        let side = d.cross(Vec3::Y) * 0.22 * k;
        let p = drum + d * 2.64 * k + Vec3::Y * 1.2 * k;
        lit.quad_facing([p - side, p + side, p + side + Vec3::Y * 2.4 * k, p - side + Vec3::Y * 2.4 * k], d, Color::WHITE, [Vec2::ZERO; 4]);
    }
    let top = drum + Vec3::Y * 9.9 * k;
    beam(gold, top, top + Vec3::Y * 1.8 * k, 0.16 * k, 0.16 * k, fwd, Color::WHITE);
    beam(gold, top + Vec3::Y * 1.2 * k - out * 0.55 * k, top + Vec3::Y * 1.2 * k + out * 0.55 * k, 0.16 * k, 0.16 * k, Vec3::Y, Color::WHITE);
}

/// The Metekhi church on its rock above the river, by the bridge named after it.
fn spawn_metekhi(b: &mut Build, m: &Mats, reserved: &mut Reserved) {
    let track = b.track;
    let Some(s) = track.find_mark("metekhi") else { return };
    let (c, d) = track.point_at(s - 6.0);
    let Some(at) = [-1.0f32, 1.0].into_iter().find_map(|side| find_spot(track, c + d.perp() * side * 42.0, 16.0, reserved, 6)) else { return };
    reserved.add(at, 18.0);
    let (mut stone, mut roofs, mut gold, mut lit, mut floods) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    // A crag for it to stand on.
    for k in 0..7 {
        let a = k as f32 * 0.9;
        let shade = 0.26 + 0.03 * (k % 3) as f32;
        blob(&mut stone, v3(at + Vec2::from_angle(a) * 7.0, 0.0), Vec3::new(9.0, 4.0 + (k % 3) as f32, 8.0), 10, true, Color::srgb(shade * 1.1, shade, shade * 0.85));
    }
    let base = v3(at, 4.2);
    church(&mut stone, &mut roofs, &mut gold, &mut lit, base, v3(d.perp(), 0.0), 1.0);
    for k in 0..4 {
        let o = Quat::from_rotation_y(k as f32 * TAU / 4.0 + 0.6) * Vec3::X * 11.0;
        boxy(&mut floods, base + o - Vec3::Y * 0.6, Vec3::X, Vec3::Z, 0.7, 0.4, 0.4, Color::WHITE);
    }
    b.add_tinted(stone, &m.stone, true);
    b.add_tinted(roofs, &m.wood, true);
    b.add(gold, &m.gold);
    b.add_flat(lit, &m.lit);
    b.add_flat(floods, &m.flood);
}

/// Narikala: a rocky hill beyond the track with the fortress's walls and towers round its
/// crown, floodlit; inside them a church, and the Mother of Corgis keeping watch with her bowl
/// and her sword. A cable car runs up to it from the middle of the town.
fn spawn_fortress(b: &mut Build, m: &Mats, plan: &Plan, rng: &mut Lcg) {
    let track = b.track;
    let (lo, hi) = bounds(track);
    let center = (lo + hi) * 0.5;
    // Beyond the hairpin under the fortress; on a custom track, beyond the side away from the river.
    let from = match track.find_mark("fortress") {
        Some(s) => track.point_at(s + 40.0).0,
        None => center + plan.river.dir.perp() * plan.river.offset(center).signum() * (hi - lo).max_element() * 0.4,
    };
    let away = (from - center).normalize_or(-Vec2::Y);
    let Some(at) = (0..40).map(|k| from + away * (70.0 + k as f32 * 10.0)).find(|&p| clearance(track, p) > 96.0 && plan.river.offset(p).abs() > plan.river.half + 100.0) else { return };
    let (radius, height) = (92.0f32, 34.0f32);
    let ground = |p: Vec2| {
        let f = (p.distance(at) / radius).min(1.0);
        let lump = 1.0 + 0.12 * ((p.x * 0.11).sin() * (p.y * 0.09).cos());
        // Flat on top, steep at the shoulder, easing out at the foot.
        height * (1.0 - f * f * (3.0 - 2.0 * f)).powf(0.8) * if f < 0.45 { 1.0 } else { lump }
    };
    let on = |p: Vec2| v3(p, ground(p));
    let (mut rock, mut stone, mut roofs, mut gold, mut lit, mut floods) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    const RINGS: usize = 12;
    const SEGS: usize = 40;
    let hp = |k: usize, j: usize| on(at + Vec2::from_angle(j as f32 / SEGS as f32 * TAU) * radius * k as f32 / RINGS as f32);
    for k in 0..RINGS {
        for j in 0..SEGS {
            let q = [hp(k, j), hp(k, j + 1), hp(k + 1, j + 1), hp(k + 1, j)];
            let shade = 0.2 + 0.05 * ((j * 7 + k * 3) % 5) as f32 / 5.0;
            let col = if k < 5 { Color::srgb(0.16, 0.2, 0.13) } else { Color::srgb(shade * 1.1, shade, shade * 0.85) };
            rock.quad_facing(q, Vec3::Y, col, q.map(|p| Vec2::new(p.x, p.z) / 6.0));
        }
    }
    // The curtain wall round the crown, crenellated, with a tower every few lengths.
    let warm = Color::srgb(0.7, 0.58, 0.42);
    const WALLS: usize = 30;
    let ring = |j: usize| at + Vec2::from_angle(j as f32 / WALLS as f32 * TAU) * radius * (0.44 + 0.04 * ((j * 5 % 7) as f32 / 7.0));
    for j in 0..WALLS {
        let (p0, p1) = (ring(j), ring(j + 1));
        let mid = (p0 + p1) * 0.5;
        let along = (p1 - p0).normalize();
        let (fwd, out) = (v3(along, 0.0), v3(along.perp(), 0.0));
        let foot = ground(mid).min(ground(p0)).min(ground(p1)) - 1.5;
        let top = ground(mid) + 6.5;
        let half = p0.distance(p1) * 0.5 + 0.4;
        boxy(&mut stone, v3(mid, foot), fwd, out, half, top - foot, 0.9, warm);
        for k in 0..4 {
            boxy(&mut stone, v3(mid + along * (k as f32 - 1.5) * half * 0.5, top), fwd, out, half * 0.13, 1.0, 0.9, warm.lighter(0.03));
        }
        let outward = (mid - at).normalize();
        if j % 6 == 2 {
            let base = on(mid);
            tube(&mut stone, base - Vec3::Y * 3.0, base + Vec3::Y * 12.0, 4.2, 3.8, 14, warm.lighter(0.02));
            for k in 0..8 {
                let d = Quat::from_rotation_y(k as f32 * TAU / 8.0) * Vec3::X;
                boxy(&mut stone, base + Vec3::Y * 12.0 + d * 3.4, d.cross(Vec3::Y), d, 0.7, 1.1, 0.45, warm.lighter(0.04));
            }
        }
        // Floodlights in the scrub below the wall, shining up it.
        if j % 2 == 0 {
            let p = mid + outward * 7.0;
            boxy(&mut floods, on(p), fwd, out, 0.9, 0.5, 0.5, Color::WHITE);
            if !b.raytracing && j % 6 == 0 {
                b.commands.spawn((PointLight { color: Color::srgb(1.0, 0.72, 0.42), intensity: 30_000_000.0, range: 70.0, radius: 2.0, shadow_maps_enabled: false, ..default() }, Transform::from_translation(on(p) + Vec3::Y * 3.0), Scenery));
            }
        }
    }
    let toward = (center - at).normalize();
    church(&mut stone, &mut roofs, &mut gold, &mut lit, on(at - toward * 14.0 + toward.perp() * 10.0), v3(toward.perp(), 0.0), 1.25);
    boxy(&mut floods, on(at + toward.perp() * 10.0), Vec3::X, Vec3::Z, 0.9, 0.5, 0.5, Color::WHITE);
    // The Mother of Corgis on her plinth at the edge of the crown, looking out over the town.
    let plinth = on(at + toward * 22.0 - toward.perp() * 12.0);
    tube(&mut stone, plinth - Vec3::Y * 2.0, plinth + Vec3::Y * 5.0, 5.0, 4.4, 16, warm.darker(0.05));
    boxy(&mut floods, plinth + v3(toward, 0.0) * 8.5 - Vec3::Y * 1.5, Vec3::X, Vec3::Z, 1.0, 0.5, 0.6, Color::WHITE);
    let yaw = Quat::from_rotation_arc(-Vec3::Z, v3(toward, 0.0));
    let kit = b.kit;
    b.commands.spawn((Transform::from_translation(plinth + Vec3::Y * 5.0).with_rotation(yaw), Visibility::default(), Scenery)).with_children(|s| {
        s.spawn((Transform::from_scale(Vec3::splat(8.0)), Visibility::default())).with_children(|c| sitting_corgi(c, kit, &kit.corgi.coats[0], &m.silver, false, false));
        // A bowl of wine for friends, a sword for enemies.
        part(s, &kit.sphere, &m.silver, t(-5.5, 9.0, -3.0).s(4.2, 2.0, 4.2));
        part(s, &kit.cylinder, &m.silver, t(-5.5, 7.4, -3.0).s(0.6, 2.2, 0.6));
        part(s, &kit.cube, &m.silver, t(5.6, 9.0, -2.0).s(0.5, 15.0, 0.16));
        part(s, &kit.cube, &m.gold, t(5.6, 3.6, -2.0).s(2.6, 0.4, 0.4));
    });
    b.add_tinted(rock, &m.stone, true);
    b.add_tinted(stone, &m.stone, true);
    b.add_tinted(roofs, &m.wood, true);
    b.add(gold, &m.gold);
    b.add_flat(lit, &m.lit);
    b.add_flat(floods, &m.flood);

    // The cable car: from a pylon in town up to a station by the wall.
    let Some(town) = find_spot(track, center, 5.0, &Reserved(vec![]), 30).filter(|&p| plan.river.offset(p).abs() > plan.river.half + 6.0) else { return };
    let (low, high) = (v3(town, 17.0), on(at + toward * 30.0) + Vec3::Y * 9.0);
    let (mut pylons, mut cable) = <(MeshBuilder, MeshBuilder)>::default();
    for (p, h) in [(v3(town, 0.0), 17.0), (on(at + toward * 30.0) - Vec3::Y, 10.0)] {
        tube(&mut pylons, p, p + Vec3::Y * h, 0.7, 0.45, 8, Color::WHITE);
        beam(&mut pylons, p + Vec3::Y * h - v3(toward.perp(), 0.0) * 2.2, p + Vec3::Y * h + v3(toward.perp(), 0.0) * 2.2, 0.5, 0.4, Vec3::Y, Color::WHITE);
    }
    for sx in [-1.0f32, 1.0] {
        let o = v3(toward.perp(), 0.0) * sx * 1.8;
        tube(&mut cable, low + o, high + o, 0.05, 0.05, 4, Color::WHITE);
    }
    b.add(pylons, &m.steel);
    b.add_flat(cable, &m.iron);
    let red = kit.kart_colors[0].clone();
    let along = Quat::from_rotation_arc(-Vec3::Z, v3(toward, 0.0));
    for k in 0..4 {
        let o = v3(toward.perp(), 0.0) * if k % 2 == 0 { 1.8 } else { -1.8 };
        let car = Shuttle { a: low + o - Vec3::Y * 3.2, b: high + o - Vec3::Y * 3.2, period: 70.0, phase: k as f32 * 0.25 + rng.f() * 0.05 };
        b.commands.spawn((Transform::from_translation(car.a).with_rotation(along), Visibility::default(), car, Scenery)).with_children(|c| {
            part(c, &kit.cube, &red, t(0.0, 0.0, 0.0).s(2.0, 0.9, 2.8));
            part(c, &kit.cube, &red, t(0.0, 1.75, 0.0).s(2.0, 0.25, 2.8));
            c.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(m.lit.clone()), t(0.0, 1.05, 0.0).s(1.9, 1.15, 2.7), NotShadowCaster));
            part(c, &kit.cylinder, &m.iron, t(0.0, 2.55, 0.0).s(0.12, 1.4, 0.12));
        });
    }
}

/// The hills round the city, sprinkled with lights; the TV tower on the highest; stars coming out.
fn spawn_far(b: &mut Build, rng: &mut Lcg) {
    let (lo, hi) = bounds(b.track);
    let center = (lo + hi) * 0.5;
    let reach = (hi - lo).max_element() * 0.5;
    let dusk = b.mats.add(StandardMaterial { base_color: Color::srgb(0.035, 0.04, 0.07), perceptual_roughness: 1.0, ..default() });
    // Unlit, and not in the raytraced scene: they only twinkle.
    let speck = |c: Color| StandardMaterial { base_color: c, unlit: true, fog_enabled: false, cull_mode: None, alpha_mode: AlphaMode::Blend, ..default() };
    let (mut lights, mut stars) = <(MeshBuilder, MeshBuilder)>::default();
    let mut tower_at = Vec3::ZERO;
    const HILLS: usize = 18;
    for k in 0..HILLS {
        let a = k as f32 / HILLS as f32 * TAU + rng.range(-0.1, 0.1);
        let p = center + Vec2::from_angle(a) * (reach + rng.range(640.0, 880.0));
        let (w, h) = (rng.range(260.0, 420.0), rng.range(90.0, 190.0));
        b.commands.spawn((Mesh3d(b.kit.sphere.clone()), MeshMaterial3d(dusk.clone()), Transform::from_translation(v3(p, -h * 0.25)).with_scale(Vec3::new(w * 1.8, h * 2.0, w * 1.2)).with_rotation(Quat::from_rotation_y(-a)), NotShadowCaster, Scenery));
        if k == HILLS / 2 {
            tower_at = v3(p, h * 0.7);
        }
        // Houses up the near slope.
        let toward = (center - p).normalize();
        for _ in 0..90 {
            let q = p + toward * rng.range(w * 0.25, w * 0.62) + toward.perp() * rng.range(-w * 0.8, w * 0.8);
            let y = rng.range(4.0, h * 0.45);
            let s = rng.range(0.8, 1.7);
            let pos = v3(q, y);
            let right = v3(toward.perp(), 0.0) * s;
            lights.quad_uv([pos - right, pos + right, pos + right + Vec3::Y * s * 1.4, pos - right + Vec3::Y * s * 1.4], v3(toward, 0.0), Color::WHITE, [Vec2::ZERO; 4]);
        }
    }
    let warm = b.mats.add(speck(Color::srgb(1.0, 0.74, 0.42)));
    b.commands.spawn((Mesh3d(b.meshes.add(lights.build())), MeshMaterial3d(warm), NotShadowCaster, super::gfx::NoRaytrace, Scenery));
    for _ in 0..700 {
        let d = Vec3::new(rng.range(-1.0, 1.0), rng.range(0.18, 1.0), rng.range(-1.0, 1.0)).normalize();
        let p = d * 2600.0;
        let right = d.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(d);
        let s = rng.range(1.2, 3.6);
        stars.quad_uv([p - right * s - up * s, p + right * s - up * s, p + right * s + up * s, p - right * s + up * s], -d, Color::WHITE, [Vec2::ZERO; 4]);
    }
    let pale = b.mats.add(speck(Color::srgba(0.9, 0.93, 1.0, 0.75)));
    b.commands.spawn((Mesh3d(b.meshes.add(stars.build())), MeshMaterial3d(pale), NotShadowCaster, super::gfx::NoRaytrace, Transform::default(), SkyFollow, Scenery));

    // The TV tower: a mast in a tripod, strung with lights.
    let (mut mast, mut glitter) = <(MeshBuilder, MeshBuilder)>::default();
    let top = tower_at + Vec3::Y * 230.0;
    tube(&mut mast, tower_at, top, 5.0, 1.2, 8, Color::WHITE);
    for k in 0..3 {
        let d = Quat::from_rotation_y(k as f32 * TAU / 3.0) * Vec3::X;
        tube(&mut mast, tower_at + d * 46.0 - Vec3::Y * 20.0, tower_at + Vec3::Y * 120.0, 2.6, 1.6, 6, Color::WHITE);
    }
    for k in 0..46 {
        let y = k as f32 * 5.0;
        let r = 5.6 - 4.0 * y / 230.0;
        tube(&mut glitter, tower_at + Vec3::Y * y, tower_at + Vec3::Y * (y + 1.6), r, r, 8, Color::WHITE);
    }
    let steel = b.mats.add(StandardMaterial { base_color: Color::srgb(0.3, 0.3, 0.34), metallic: 0.5, perceptual_roughness: 0.5, ..default() });
    b.add(mast, &steel);
    let sparkle = b.mats.add(speck(Color::srgb(1.0, 0.92, 0.8)));
    b.commands.spawn((Mesh3d(b.meshes.add(glitter.build())), MeshMaterial3d(sparkle), NotShadowCaster, super::gfx::NoRaytrace, Scenery));
    b.commands.spawn((Mesh3d(b.kit.sphere.clone()), MeshMaterial3d(b.kit.lamp_red.clone()), Transform::from_translation(top + Vec3::Y * 4.0).with_scale(Vec3::splat(7.0)), NotShadowCaster, Scenery));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The built-in lap is over the river on its two bridges and nowhere else, and the Bridge
    /// of Peace's canopy covers the first of them.
    #[test]
    fn river_runs_under_the_bridges() {
        let track = Track::tbilisi();
        let plan = Plan::new(&track);
        let over = |s: f32| plan.river.offset(track.point_at(s).0).abs() < plan.river.half + WALL_DIST + 4.0;
        let spans = [("bridge", "bridge_end"), ("metekhi", "metekhi_end")].map(|(a, b)| (track.mark(a), track.mark(b)));
        let mut s = 0.0;
        while s < track.length {
            let on_bridge = spans.iter().any(|&(a, b)| (a - 14.0..b + 14.0).contains(&s));
            assert!(on_bridge || !over(s), "the road is in the river at {s:.0}m");
            s += 2.0;
        }
        for (a, b) in spans {
            assert!(over((a + b) * 0.5));
            assert!(plan.bridge[((a + b) * 0.5 / track.spacing) as usize]);
        }
        let (c0, c1) = plan.canopy.expect("the bridge has its canopy");
        assert!((c0 as f32 * track.spacing) < spans[0].0 && (c1 as f32 * track.spacing) > spans[0].1);
    }

    /// On custom tracks the river keeps clear of the road.
    #[test]
    fn river_passes_custom_tracks_by() {
        for map in crate::sim::custom::tests::sample_maps() {
            let track = crate::sim::custom::build(&map, TrackId::Custom(map.hash())).track;
            let plan = Plan::new(&track);
            for s in &track.samples {
                assert!(plan.river.offset(s.pos).abs() > plan.river.half + WALL_DIST + 20.0, "{}: the river reaches the road", map.name);
            }
        }
    }
}
