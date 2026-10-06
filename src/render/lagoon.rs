//! Mirror Lagoon: golden hour on the Riviera, built around what raytracing does best.
//!
//! - Mirrors reflecting mirrors: the Pavilion tunnel is lined with tilted mirror panels and
//!   neon ribs, with a mirror ball turning under a starry vault.
//! - Water: a glassy lagoon that reflects the sunset, the lamps and the lanterns floating on it.
//! - Glossy and metallic surfaces: a polished terrazzo road, marble verges, gold everywhere and
//!   a chrome corgi.
//! - Real area lights: lamps, lanterns, festoon lights, lit windows and the lighthouse.
//! - Bounce light between the pastel houses of the harbour village.
//! - A low sun with a wide disc, raking long soft shadows through the gilded colonnade.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{light::NotShadowCaster, prelude::*};

use super::mesh::{v3, Lcg, MeshBuilder};
use super::models::{glow, matte, part, t, Kit, TransformExt};
use super::scene::{Scenery, SurfaceMats};
use super::shapes::{boxy, hemisphere, hip_roof, lathe, octahedron, palm, side_face, strip, tinted_mesh};
use super::scenery::{best_spot, bounds, clearance, sitting_corgi, spawn_balloons, spawn_gantry, spawn_grandstand, Floating, Reserved, Spin};
use super::signs::{SignAtlas, SIGN_LAGOON};
use super::textures::Textures;
use super::trackmesh::{at, at3, spawn_board, spawn_pads_and_ramps, spawn_sign, start_line, WaterMat};
use crate::sim::track::{Track, ROAD_HALF_WIDTH, WALL_DIST};

/// Height of the lagoon's surface (the road is at 0 or above).
pub const WATER_Y: f32 = -0.6;
/// Thickness of the stone plinth under the balustrade.
const PLINTH: f32 = 0.7;
const PLINTH_H: f32 = 0.6;
/// Pavilion cross-section: walls up to `HALL_WALL`, then an elliptical vault rising `HALL_RISE`.
const HALL_WALL: f32 = 7.5;
const HALL_RISE: f32 = 4.5;
/// The flat top of the land, just under the road.
const LAND_Y: f32 = -0.05;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    /// The start straight: a promenade and the marina on the left, the grandstand on the right.
    Promenade,
    /// T1 and the Pavilion.
    Headland,
    /// The arched causeway across the water.
    Causeway,
    /// The hairpin round the lighthouse point.
    Point,
    Colonnade,
    /// The chicane through the harbour village.
    Village,
}

/// Where each zone starts, by the track's marks.
const ZONES: [(&str, Zone); 6] = [
    ("headland", Zone::Headland),
    ("causeway", Zone::Causeway),
    ("point", Zone::Point),
    ("colonnade", Zone::Colonnade),
    ("village", Zone::Village),
    ("promenade", Zone::Promenade),
];

fn zone_at(track: &Track, s: f32) -> Zone {
    ZONES.iter().rev().find(|(mark, _)| s >= track.mark(mark)).map_or(Zone::Promenade, |z| z.1)
}

/// How the land meets the water: a stone quay or a sandy beach (rise per metre inland).
fn shore_slope(zone: Zone, side: usize) -> f32 {
    if zone == Zone::Promenade && side == 0 { 0.6 } else { 0.14 }
}

/// An island: sand round the edge, lawn or rock on top.
struct Island {
    c: Vec2,
    r: f32,
    top: f32,
    slope: f32,
    rock: bool,
}

/// Where everything is: the zone of every sample, the shoreline, and the islands.
struct Layout {
    zone: Vec<Zone>,
    /// Distance from the centreline to the shore, on the left and right.
    shore: Vec<[f32; 2]>,
    tunnel: (usize, usize),
    islands: Vec<Island>,
}

/// The ground under a point: height (below `WATER_Y` is lagoon bed), and what it's made of.
#[derive(Clone, Copy)]
struct Ground {
    h: f32,
    rock: bool,
    paved: bool,
}

impl Layout {
    fn new(track: &Track) -> Self {
        let n = track.samples.len();
        let zone: Vec<Zone> = track.samples.iter().map(|s| zone_at(track, s.s)).collect();
        let width = |z: Zone| match z {
            Zone::Promenade => [34.0, 70.0],
            Zone::Headland => [62.0, 55.0],
            Zone::Causeway => [0.0, 0.0],
            Zone::Point => [30.0, 26.0],
            Zone::Colonnade => [40.0, 24.0],
            Zone::Village => [70.0, 60.0],
        };
        // Smooth the shoreline so the land tapers in and out.
        let raw: Vec<[f32; 2]> = zone.iter().map(|&z| width(z)).collect();
        let k = 30;
        let shore = (0..n)
            .map(|i| {
                let mut acc = [0.0f32; 2];
                for o in 0..=2 * k {
                    let r = raw[(i + n + o - k) % n];
                    acc[0] += r[0];
                    acc[1] += r[1];
                }
                acc.map(|a| a / (2 * k + 1) as f32)
            })
            .collect();
        let at = |mark| (track.mark(mark) / track.spacing) as usize;
        let tunnel = (at("tunnel"), at("tunnel_end"));
        Layout { zone, shore, tunnel, islands: vec![] }
    }

    fn in_tunnel(&self, i: usize) -> bool {
        (self.tunnel.0..self.tunnel.1).contains(&i)
    }

    /// Nearest sample to `p`, its distance, and which side of the track `p` is on (0 = left).
    fn nearest(track: &Track, p: Vec2) -> (usize, f32, usize) {
        let n = track.samples.len();
        let mut best = (0, f32::MAX);
        for i in (0..n).step_by(3) {
            let d = track.samples[i].pos.distance_squared(p);
            if d < best.1 {
                best = (i, d);
            }
        }
        for o in 0..5 {
            let i = (best.0 + n + o - 2) % n;
            let d = track.samples[i].pos.distance_squared(p);
            if d < best.1 {
                best = (i, d);
            }
        }
        let s = &track.samples[best.0];
        let side = if (p - s.pos).dot(s.dir.perp()) < 0.0 { 0 } else { 1 };
        (best.0, best.1.sqrt(), side)
    }

    fn ground(&self, track: &Track, p: Vec2) -> Ground {
        let (i, d, side) = Self::nearest(track, p);
        let zone = self.zone[i];
        let edge = self.shore[i][side] - d;
        let mut h = (WATER_Y + edge * shore_slope(zone, side)).min(LAND_Y);
        let mut rock = false;
        for isl in &self.islands {
            let e = isl.r - isl.c.distance(p);
            // a little lumpiness on the rocks
            let bump = if isl.rock { ((p.x * 0.31).sin() * (p.y * 0.27).cos() + 1.0) * 0.6 } else { 0.0 };
            let hi = (WATER_Y + e * isl.slope).min(isl.top + bump);
            if hi > h {
                h = hi;
                rock = isl.rock && hi > WATER_Y + 0.35;
            }
        }
        // Never poke through the track itself.
        if d < WALL_DIST + PLINTH + 1.0 {
            h = h.min(LAND_Y);
        }
        let paved = matches!(zone, Zone::Village) || (zone == Zone::Promenade && side == 0);
        Ground { h, rock, paved }
    }
}

/// Every material on the track, made once.
struct Mats {
    gold: Handle<StandardMaterial>,
    rose_gold: Handle<StandardMaterial>,
    /// A perfect mirror: the raytracer reflects it up to three bounces deep.
    chrome: Handle<StandardMaterial>,
    ivory: Handle<StandardMaterial>,
    stucco: Handle<StandardMaterial>,
    lacquer: Handle<StandardMaterial>,
    black_marble: Handle<StandardMaterial>,
    glass: Handle<StandardMaterial>,
    /// Dark window glass: reflects the sunset.
    window: Handle<StandardMaterial>,
    lamp: Handle<StandardMaterial>,
    lantern: Handle<StandardMaterial>,
    lit_window: Handle<StandardMaterial>,
    neon: [Handle<StandardMaterial>; 4],
    terracotta: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    rock: Handle<StandardMaterial>,
    hull: Handle<StandardMaterial>,
    foliage: Handle<StandardMaterial>,
    flowers: Handle<StandardMaterial>,
    cypress: Handle<StandardMaterial>,
    shutters: Handle<StandardMaterial>,
}

/// Neon colours for the Pavilion ribs: magenta, amber, cyan, violet.
const NEON: [LinearRgba; 4] = [
    LinearRgba::rgb(9.0, 0.8, 5.0),
    LinearRgba::rgb(10.0, 4.0, 0.6),
    LinearRgba::rgb(0.5, 6.0, 10.0),
    LinearRgba::rgb(4.0, 1.0, 10.0),
];

impl Mats {
    fn new(mats: &mut Assets<StandardMaterial>, tex: &Textures, raytracing: bool) -> Self {
        let metal = |mats: &mut Assets<StandardMaterial>, c: Color, rough: f32| mats.add(StandardMaterial { base_color: c, metallic: 1.0, perceptual_roughness: rough, ..default() });
        let gold = metal(mats, Color::srgb(1.0, 0.78, 0.36), 0.2);
        let rose_gold = metal(mats, Color::srgb(0.96, 0.66, 0.54), 0.3);
        let chrome = metal(mats, Color::srgb(0.97, 0.97, 0.98), 0.0);
        Mats {
            gold,
            rose_gold,
            chrome,
            ivory: mats.add(StandardMaterial { base_color: Color::srgb(0.93, 0.89, 0.8), base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.45, ..default() }),
            stucco: mats.add(StandardMaterial { base_color: Color::srgb(0.95, 0.87, 0.76), base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.8, ..default() }),
            lacquer: mats.add(StandardMaterial { base_color: Color::srgb(0.02, 0.025, 0.06), perceptual_roughness: 0.1, reflectance: 0.6, ..default() }),
            black_marble: mats.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.035), base_color_texture: Some(tex.tiles.clone()), perceptual_roughness: 0.05, reflectance: 0.6, ..default() }),
            glass: mats.add(StandardMaterial {
                base_color: Color::srgba(0.75, 0.9, 0.95, 0.16),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.02,
                reflectance: 0.8,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            window: mats.add(StandardMaterial { base_color: Color::srgb(0.02, 0.03, 0.04), perceptual_roughness: 0.04, reflectance: 0.8, ..default() }),
            lamp: mats.add(glow(LinearRgba::rgb(10.0, 6.5, 3.2))),
            lantern: mats.add(glow(LinearRgba::rgb(8.0, 3.2, 0.9))),
            lit_window: mats.add(glow(LinearRgba::rgb(4.0, 2.4, 1.0))),
            neon: NEON.map(|c| mats.add(glow(c))),
            terracotta: mats.add(StandardMaterial { base_color: Color::srgb(0.72, 0.33, 0.2), perceptual_roughness: 0.75, ..default() }),
            wood: mats.add(StandardMaterial { base_color: Color::srgb(0.5, 0.33, 0.2), base_color_texture: Some(tex.hedge.clone()), perceptual_roughness: 0.7, ..default() }),
            rock: mats.add(StandardMaterial { base_color: Color::srgb(0.4, 0.36, 0.33), base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.9, ..default() }),
            // Lacquered white hulls; a touch rougher without raytracing so the screen-space
            // reflections don't flicker.
            hull: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: if raytracing { 0.12 } else { 0.2 }, reflectance: 0.6, ..default() }),
            foliage: mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.8, double_sided: true, cull_mode: None, ..default() }),
            flowers: mats.add(matte(Color::srgb(0.85, 0.12, 0.55))),
            cypress: mats.add(matte(Color::srgb(0.09, 0.22, 0.11))),
            shutters: mats.add(matte(Color::srgb(0.18, 0.42, 0.34))),
        }
    }
}

/// Everything for Mirror Lagoon: the track, the Pavilion, the land and water, and the scenery.
#[allow(clippy::too_many_arguments)]
pub fn spawn_lagoon(
    commands: &mut Commands,
    kit: &Kit,
    signs: &SignAtlas,
    surf: &SurfaceMats,
    tex: &Textures,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    track: &Track,
    raytracing: bool,
) {
    let m = Mats::new(materials, tex, raytracing);
    let mut layout = Layout::new(track);
    let mut rng = Lcg(0x1A_600A);
    let mut reserved = Reserved(vec![]);

    // The lighthouse sits on a rocky point outside the hairpin; the chrome corgi on an island
    // in the middle of the lagoon.
    let hairpin = super::scenery::find_tightest(track, track.mark("point") / track.length, track.mark("colonnade") / track.length);
    let (hc, hd) = track.point_at(track.samples[hairpin].s);
    let light_at = hc - hd.perp() * (WALL_DIST + 34.0);
    layout.islands.push(Island { c: light_at, r: 24.0, top: 2.2, slope: 0.45, rock: true });
    reserved.add(light_at, 24.0);
    let (lo, hi) = bounds(track);
    let statue_at = best_spot(track, (lo + hi) * 0.5 + Vec2::new(20.0, -10.0), 40.0, &reserved);
    layout.islands.push(Island { c: statue_at, r: 24.0, top: 0.5, slope: 0.12, rock: false });
    reserved.add(statue_at, 30.0);
    // A couple of palm islets
    let mut islets = vec![];
    for want in [Vec2::new(-20.0, 220.0), Vec2::new(200.0, 120.0)] {
        let c = best_spot(track, want, 25.0, &reserved);
        layout.islands.push(Island { c, r: 11.0, top: 0.3, slope: 0.12, rock: false });
        reserved.add(c, 14.0);
        islets.push(c);
    }

    spawn_surface(commands, meshes, materials, surf, tex, &m, track, &layout);
    spawn_pavilion(commands, meshes, kit, signs, &m, track, &layout, raytracing, &mut rng);
    spawn_land_and_water(commands, meshes, materials, tex, track, &layout);

    let scene = Scene { kit, m: &m, track, layout: &layout };
    scene.promenade(commands, meshes, surf, signs, &mut rng, &mut reserved);
    scene.causeway_lamps(commands);
    scene.lighthouse(commands, meshes, light_at, raytracing, &mut rng);
    scene.colonnade(commands, meshes);
    scene.village(commands, meshes, materials, &mut rng, &mut reserved);
    scene.statue(commands, materials, statue_at);
    scene.greenery(commands, meshes, materials, &islets, &reserved, &mut rng);
    scene.lanterns(commands, &mut rng);
    scene.sky(commands, meshes, materials, &mut rng);
}

// ----- Track surface --------------------------------------------------------------------------

/// Polished terrazzo road with gold inlay edges, glossy marble verges, curbs through the
/// corners, and a marble balustrade with glass panels and a gold rail (not in the tunnel).
#[allow(clippy::too_many_arguments)]
fn spawn_surface(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, surf: &SurfaceMats, tex: &Textures, m: &Mats, track: &Track, layout: &Layout) {
    let n = track.samples.len();
    let mut road = MeshBuilder::default();
    let mut verge = MeshBuilder::default();
    let mut paint = MeshBuilder::default();
    let mut gold = MeshBuilder::default();
    let mut stone = MeshBuilder::default();
    let mut glass = MeshBuilder::default();
    let mut seawall = MeshBuilder::default();

    for i in 0..n {
        let band = if (i / 8) % 2 == 0 { 0.0 } else { 0.012 };
        strip(&mut road, track, i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 0.02, Color::srgb(0.13 + band, 0.12 + band, 0.12 + band));
        let tint = if (i / 5) % 2 == 0 { 0.0 } else { 0.02 };
        for (l0, l1) in [(-WALL_DIST, -ROAD_HALF_WIDTH), (ROAD_HALF_WIDTH, WALL_DIST)] {
            strip(&mut verge, track, i, l0, l1, 0.0, Color::srgb(0.86 - tint, 0.8 - tint, 0.71 - tint));
        }

        // Coral and ivory curbs through the corners, gold inlay along the straights.
        let near_corner = (0..12).any(|o| track.samples[(i + n + o * 3 - 18) % n].curvature.abs() > 1.0 / 90.0);
        if near_corner {
            let c = if (i / 3) % 2 == 0 { Color::srgb(0.95, 0.36, 0.3) } else { Color::srgb(0.96, 0.94, 0.88) };
            strip(&mut paint, track, i, -ROAD_HALF_WIDTH - 0.4, -ROAD_HALF_WIDTH + 1.1, 0.045, c);
            strip(&mut paint, track, i, ROAD_HALF_WIDTH - 1.1, ROAD_HALF_WIDTH + 0.4, 0.045, c);
        } else {
            for side in [-1.0f32, 1.0] {
                let l = side * (ROAD_HALF_WIDTH + 0.15);
                strip(&mut gold, track, i, l - 0.2, l + 0.2, 0.04, Color::WHITE);
            }
        }

        if layout.in_tunnel(i) {
            continue;
        }
        // Balustrade: marble plinth, glass panel and a gold rail.
        for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            let (inner, outer) = (side * WALL_DIST, side * (WALL_DIST + PLINTH));
            let ivory = Color::srgb(0.93, 0.89, 0.8);
            side_face(&mut stone, track, i, inner, 0.0, PLINTH_H, -side, ivory);
            side_face(&mut stone, track, i, outer, 0.0, PLINTH_H, side, ivory);
            let top = [at3(track, i, inner, PLINTH_H), at3(track, i, outer, PLINTH_H), at3(track, i + 1, outer, PLINTH_H), at3(track, i + 1, inner, PLINTH_H)];
            stone.quad_facing(top, Vec3::Y, ivory, top.map(|p| Vec2::new(p.x, p.z) / 3.0));
            let mid = side * (WALL_DIST + PLINTH * 0.5);
            for face in [-1.0f32, 1.0] {
                side_face(&mut glass, track, i, mid, PLINTH_H, 1.3, face, Color::WHITE);
            }
            let (r0, r1) = (side * (WALL_DIST + 0.2), side * (WALL_DIST + 0.5));
            side_face(&mut gold, track, i, r0, 1.3, 1.44, -side, Color::WHITE);
            side_face(&mut gold, track, i, r1, 1.3, 1.44, side, Color::WHITE);
            let rail = [at3(track, i, r0, 1.44), at3(track, i, r1, 1.44), at3(track, i + 1, r1, 1.44), at3(track, i + 1, r0, 1.44)];
            gold.quad_facing(rail, Vec3::Y, Color::WHITE, [Vec2::ZERO; 4]);

            // A stone seawall down into the water wherever the land doesn't reach.
            if layout.shore[i][k] < WALL_DIST + 8.0 {
                let (p0, p1) = (at(track, i, outer), at(track, i + 1, outer));
                let (y0, y1) = (track.height_idx(i), track.height_idx(i + 1));
                let deep = WATER_Y - 1.5;
                let q = [v3(p0, deep), v3(p1, deep), v3(p1, y1), v3(p0, y0)];
                let s0 = track.samples[i].s;
                let uv = [Vec2::new(s0, deep), Vec2::new(s0 + 1.0, deep), Vec2::new(s0 + 1.0, y1), Vec2::new(s0, y0)].map(|u| u / 3.0);
                let c = Color::srgb(0.76, 0.66, 0.52);
                seawall.quad_facing(q, v3(track.samples[i].dir.perp() * side, 0.0), c, uv);
            }
        }
    }
    start_line(track, &mut paint, true);

    let road_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(tex.asphalt.clone()),
        // Mirror polish. Solari traces a single exact reflection ray (no noisy glossy samples
        // for the denoiser to blur) at roughness <= 0.001, i.e. perceptual <= ~0.03 once it's
        // been packed into 8 bits in the G-buffer.
        perceptual_roughness: 0.025,
        reflectance: 0.6,
        ..default()
    });
    let verge_mat = materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.tiles.clone()), perceptual_roughness: 0.14, reflectance: 0.55, ..default() });
    let wall_mat = materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.85, ..default() });
    let flat = || (Scenery, NotShadowCaster);
    commands.spawn((road.tinted(meshes, materials, &road_mat), flat()));
    commands.spawn((verge.tinted(meshes, materials, &verge_mat), flat()));
    commands.spawn((paint.tinted(meshes, materials, &surf.paint), flat()));
    commands.spawn((Mesh3d(meshes.add(gold.build())), MeshMaterial3d(m.gold.clone()), Scenery));
    commands.spawn((stone.tinted(meshes, materials, &m.ivory), Scenery));
    commands.spawn((Mesh3d(meshes.add(glass.build())), MeshMaterial3d(m.glass.clone()), NotShadowCaster, Scenery));
    commands.spawn((seawall.tinted(meshes, materials, &wall_mat), Scenery));
    spawn_pads_and_ramps(commands, meshes, materials, surf, track);
}

// ----- The Pavilion ------------------------------------------------------------------------------

/// The hall's inner cross-section as (lateral, height) points: up the right wall, over the vault,
/// down the left wall. `inset` pulls it in towards the middle.
fn hall_profile(inset: f32) -> Vec<Vec2> {
    let (w, wall) = (WALL_DIST - inset, HALL_WALL);
    let mut p = vec![Vec2::new(w, PLINTH_H), Vec2::new(w, wall * 0.5)];
    let k = 18;
    for j in 0..=k {
        let a = j as f32 / k as f32 * PI;
        p.push(Vec2::new(w * a.cos(), wall + (HALL_RISE - inset) * a.sin()));
    }
    p.push(Vec2::new(-w, wall * 0.5));
    p.push(Vec2::new(-w, PLINTH_H));
    p
}

/// A walk-through mirror hall: tilted mirror panels on gold, neon ribs, a starry lacquer
/// vault and a turning mirror ball. Outside: stucco walls with lit windows, a rose-gold roof
/// and dome, and grand portals at each end.
#[allow(clippy::too_many_arguments)]
fn spawn_pavilion(commands: &mut Commands, meshes: &mut Assets<Mesh>, kit: &Kit, signs: &SignAtlas, m: &Mats, track: &Track, layout: &Layout, raytracing: bool, rng: &mut Lcg) {
    let n = track.samples.len();
    let (t0, t1) = layout.tunnel;
    let mut mirrors = MeshBuilder::default();
    let mut gold = MeshBuilder::default();
    let mut lacquer = MeshBuilder::default();
    let mut plinth = MeshBuilder::default();
    let mut stars = MeshBuilder::default();
    let mut neon: [MeshBuilder; 4] = Default::default();
    let mut stucco = MeshBuilder::default();
    let mut roof = MeshBuilder::default();
    let mut windows = MeshBuilder::default();
    let frame = |i: usize| {
        let s = &track.samples[i % n];
        (v3(s.pos, track.height_idx(i)), v3(s.dir.perp(), 0.0), v3(s.dir, 0.0))
    };
    // Point on the cross-section at sample i.
    let cs = |i: usize, q: Vec2| {
        let (o, right, _) = frame(i);
        o + right * q.x + Vec3::Y * q.y
    };
    let profile = hall_profile(0.0);
    let outer_w = WALL_DIST + 1.6;

    for i in t0..t1 {
        let j = i + 1;
        // Black marble skirting under the mirrors.
        for side in [-1.0f32, 1.0] {
            side_face(&mut plinth, track, i, side * WALL_DIST, 0.0, PLINTH_H, -side, Color::WHITE);
            // gold backing, seen between the mirror panels
            side_face(&mut gold, track, i, side * (WALL_DIST + 0.05), PLINTH_H, HALL_WALL, -side, Color::WHITE);
        }
        // The vault (inside) and the roof (outside).
        for k in 2..profile.len() - 3 {
            let (a, b) = (profile[k], profile[k + 1]);
            let q = [cs(i, a), cs(i, b), cs(j, b), cs(j, a)];
            let mid = (a + b) * 0.5;
            let inward = frame(i).1 * -mid.x + Vec3::Y * (HALL_WALL - mid.y);
            lacquer.quad_facing(q, inward, Color::WHITE, [Vec2::ZERO; 4]);
            let scale = |p: Vec2| Vec2::new(p.x * outer_w / WALL_DIST, HALL_WALL + 2.1 + (p.y - HALL_WALL) * 1.05);
            let (oa, ob) = (scale(a), scale(b));
            let q = [cs(i, oa), cs(i, ob), cs(j, ob), cs(j, oa)];
            let seam = (i / 9) % 2 == 0 && i % 9 == 0;
            roof.quad_facing(q, -inward, if seam { Color::srgb(1.0, 0.85, 0.5) } else { Color::WHITE }, [Vec2::ZERO; 4]);
        }
        // Outside walls with a gold cornice.
        for side in [-1.0f32, 1.0] {
            side_face(&mut stucco, track, i, side * outer_w, -0.6, HALL_WALL + 1.5, side, Color::WHITE);
            side_face(&mut gold, track, i, side * (outer_w + 0.25), HALL_WALL + 1.5, HALL_WALL + 2.1, side, Color::WHITE);
            let lip = [cs(i, Vec2::new(side * outer_w, HALL_WALL + 2.1)), cs(i, Vec2::new(side * (outer_w + 0.25), HALL_WALL + 2.1)), cs(j, Vec2::new(side * (outer_w + 0.25), HALL_WALL + 2.1)), cs(j, Vec2::new(side * outer_w, HALL_WALL + 2.1))];
            gold.quad_facing(lip, Vec3::Y, Color::WHITE, [Vec2::ZERO; 4]);
            // tall lit windows between the ribs
            if (i - t0) % 9 == 3 {
                let (o, right, fwd) = frame(i);
                let c = o + right * side * (outer_w + 0.04);
                let face = right * side;
                let q = [c - fwd * 1.1 + Vec3::Y * 2.2, c + fwd * 1.1 + Vec3::Y * 2.2, c + fwd * 1.1 + Vec3::Y * 6.4, c - fwd * 1.1 + Vec3::Y * 6.4];
                windows.quad_facing(q, face, Color::WHITE, [Vec2::ZERO; 4]);
                let arch = [c - fwd * 1.1 + Vec3::Y * 6.4, c + fwd * 1.1 + Vec3::Y * 6.4, c + Vec3::Y * 7.3, c + Vec3::Y * 7.3];
                windows.quad_facing(arch, face, Color::WHITE, [Vec2::ZERO; 4]);
            }
        }

        // Mirror panels, two rows, each tilted a little so the reflections break into facets.
        if (i - t0) % 3 == 0 && i + 3 <= t1 {
            for side in [-1.0f32, 1.0] {
                for (y0, y1) in [(PLINTH_H + 0.06, 4.0), (4.06, HALL_WALL - 0.06)] {
                    let (ta, tb) = (rng.range(-0.22, 0.22), rng.range(-0.22, 0.22));
                    let corner = |di: usize, y: f32, u: f32| {
                        let v = (y - y0) / (y1 - y0);
                        let push = 0.25 + ta * (u - 0.5) + tb * (v - 0.5);
                        let (o, right, fwd) = frame(i + di);
                        o + right * side * (WALL_DIST - push) + Vec3::Y * y + fwd * (0.5 - u) * 0.12
                    };
                    let q = [corner(0, y0, 0.0), corner(3, y0, 1.0), corner(3, y1, 1.0), corner(0, y1, 0.0)];
                    mirrors.quad_facing(q, frame(i).1 * -side, Color::WHITE, [Vec2::ZERO; 4]);
                }
            }
        }

        // Neon ribs over the whole cross-section.
        if (i - t0) % 9 == 5 {
            // standing proud of the tilted mirrors
            let rib = hall_profile(0.55);
            let k = ((i - t0) / 9) % 4;
            let (_, _, fwd) = frame(i);
            for w in 0..rib.len() - 1 {
                let (a, b) = (cs(i, rib[w]), cs(i, rib[w + 1]));
                let q = [a - fwd * 0.2, b - fwd * 0.2, b + fwd * 0.2, a + fwd * 0.2];
                let mid = (rib[w] + rib[w + 1]) * 0.5;
                neon[k].quad_facing(q, frame(i).1 * -mid.x + Vec3::Y * (HALL_WALL - mid.y), Color::WHITE, [Vec2::ZERO; 4]);
            }
            if !raytracing {
                commands.spawn((
                    PointLight { color: Color::from(NEON[k]), intensity: 500_000.0, range: 25.0, radius: 1.0, shadow_maps_enabled: false, ..default() },
                    Transform::from_translation(cs(i, Vec2::new(0.0, HALL_WALL))),
                    Scenery,
                ));
            }
        }
    }

    // Stars on the vault.
    for _ in 0..420 {
        let i = t0 + (rng.f() * (t1 - t0 - 1) as f32) as usize;
        let a = rng.range(0.12, PI - 0.12);
        let q = Vec2::new((WALL_DIST - 0.05) * a.cos(), HALL_WALL + (HALL_RISE - 0.05) * a.sin());
        let p = cs(i, q);
        let (_, right, fwd) = frame(i);
        let nrm = (right * -q.x / WALL_DIST + Vec3::Y * -(q.y - HALL_WALL) / HALL_RISE).normalize();
        let side = nrm.cross(fwd).normalize();
        let r = rng.range(0.025, 0.055);
        stars.quad_facing([p - fwd * r - side * r, p + fwd * r - side * r, p + fwd * r + side * r, p - fwd * r + side * r], nrm, Color::WHITE, [Vec2::ZERO; 4]);
    }

    // Portals: a stucco face with a gold-trimmed arch, pilasters and the name above.
    let top = HALL_WALL + HALL_RISE + 5.0;
    let half = outer_w + 5.0;
    for (i, out) in [(t0, -1.0f32), (t1, 1.0)] {
        let (o, right, fwd) = frame(i);
        let facing = fwd * out;
        let p = |x: f32, y: f32| o + right * x + Vec3::Y * y + facing * 0.02;
        for side in [-1.0f32, 1.0] {
            let (a, b) = (side * WALL_DIST, side * half);
            stucco.quad_facing([p(a, -0.6), p(b, -0.6), p(b, top), p(a, top)], facing, Color::WHITE, [Vec2::ZERO; 4]);
            stucco.quad_facing([p(a, -0.6), p(b, -0.6), p(b, top), p(a, top)], -facing, Color::WHITE, [Vec2::ZERO; 4]);
            // pilasters
            let g = |x: f32, y: f32| p(x, y) + facing * 0.15;
            for x in [side * (WALL_DIST + 1.2), side * (half - 1.4)] {
                gold.quad_facing([g(x - 0.6, 0.0), g(x + 0.6, 0.0), g(x + 0.6, top - 1.0), g(x - 0.6, top - 1.0)], facing, Color::WHITE, [Vec2::ZERO; 4]);
            }
        }
        for k in 2..profile.len() - 3 {
            let (a, b) = (profile[k], profile[k + 1]);
            let q = [p(a.x, a.y), p(b.x, b.y), p(b.x, top), p(a.x, top)];
            stucco.quad_facing(q, facing, Color::WHITE, [Vec2::ZERO; 4]);
            stucco.quad_facing(q, -facing, Color::WHITE, [Vec2::ZERO; 4]);
        }
        // gold trim round the arch
        let trim = hall_profile(-0.9);
        let g = |q: Vec2| p(q.x, q.y) + facing * 0.12;
        for w in 0..profile.len() - 1 {
            gold.quad_facing([g(profile[w]), g(profile[w + 1]), g(trim[w + 1]), g(trim[w])], facing, Color::WHITE, [Vec2::ZERO; 4]);
        }
        gold.quad_facing([g(Vec2::new(-half, top - 1.0)), g(Vec2::new(half, top - 1.0)), g(Vec2::new(half, top)), g(Vec2::new(-half, top))], facing, Color::WHITE, [Vec2::ZERO; 4]);
        spawn_board(commands, meshes, &signs.material, kit, p(0.0, HALL_WALL + HALL_RISE + 0.9) + facing * 0.2, facing, SIGN_LAGOON, Vec2::new(15.0, 2.6), false);
    }

    let add = |commands: &mut Commands, meshes: &mut Assets<Mesh>, b: MeshBuilder, mat: &Handle<StandardMaterial>, shadow: bool| {
        let mut e = commands.spawn((Mesh3d(meshes.add(b.build())), MeshMaterial3d(mat.clone()), Scenery));
        if !shadow {
            e.insert(NotShadowCaster);
        }
    };
    add(commands, meshes, mirrors, &m.chrome, true);
    add(commands, meshes, gold, &m.gold, true);
    add(commands, meshes, lacquer, &m.lacquer, true);
    add(commands, meshes, plinth, &m.black_marble, true);
    add(commands, meshes, stars, &m.lamp, false);
    for (k, b) in neon.into_iter().enumerate() {
        add(commands, meshes, b, &m.neon[k], false);
    }
    add(commands, meshes, stucco, &m.stucco, true);
    add(commands, meshes, roof, &m.rose_gold, true);
    add(commands, meshes, windows, &m.lit_window, false);

    // The mirror ball, halfway through.
    let mid = (t0 + t1) / 2;
    let ball_at = cs(mid, Vec2::new(0.0, 8.4));
    let mut ball = MeshBuilder::default();
    let (rings, segs) = (14, 28);
    let r = 2.0;
    let dir = |a: f32, b: f32| Vec3::new(b.cos() * a.sin(), a.cos(), b.sin() * a.sin());
    for i in 0..rings {
        for j in 0..segs {
            let (a0, a1) = (i as f32 / rings as f32 * PI, (i + 1) as f32 / rings as f32 * PI);
            let (b0, b1) = (j as f32 / segs as f32 * TAU, (j + 1) as f32 / segs as f32 * TAU);
            let c = dir((a0 + a1) * 0.5, (b0 + b1) * 0.5) * r;
            // inset each tile towards its centre, leaving dark grout
            let q = [dir(a0, b0), dir(a0, b1), dir(a1, b1), dir(a1, b0)].map(|d| c + (d * r - c) * 0.9);
            ball.quad_facing(q, c, Color::WHITE, [Vec2::ZERO; 4]);
        }
    }
    commands
        .spawn((Transform::from_translation(ball_at), Visibility::default(), Spin(0.35), Scenery))
        .with_children(|b| {
            b.spawn((Mesh3d(meshes.add(ball.build())), MeshMaterial3d(m.chrome.clone()), Transform::default()));
            part(b, &kit.sphere, &kit.black, Transform::from_scale(Vec3::splat(r * 1.9)));
        });
    // Pin spots on the vault for the facets to catch.
    for (di, x) in [(0usize, -7.0f32), (0, 7.0), (8, -7.0), (8, 7.0)] {
        let a = (x / WALL_DIST).acos();
        let p = cs(mid + di - 4, Vec2::new(x, HALL_WALL + (HALL_RISE - 0.35) * a.sin()));
        commands.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(m.lamp.clone()), Transform::from_translation(p).with_scale(Vec3::splat(0.5)), NotShadowCaster, Scenery));
    }
    let crown = cs(mid, Vec2::new(0.0, HALL_WALL + HALL_RISE));
    commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.gold.clone()), Transform::from_translation((ball_at + crown) * 0.5 + Vec3::Y * r * 0.5).with_scale(Vec3::new(0.12, crown.y - ball_at.y - r, 0.12)), Scenery));

    // The dome over the middle of the hall: an ivory drum, a rose-gold dome, a gold lantern.
    let base = cs(mid, Vec2::ZERO);
    let drum_y = HALL_WALL + HALL_RISE + 0.5;
    let drum_h = 5.0;
    let dome_r = 9.6;
    commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.stucco.clone()), Transform::from_translation(base + Vec3::Y * (drum_y + drum_h * 0.5)).with_scale(Vec3::new(18.0, drum_h, 18.0)), Scenery));
    commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.gold.clone()), Transform::from_translation(base + Vec3::Y * (drum_y + drum_h)).with_scale(Vec3::new(19.6, 0.5, 19.6)), Scenery));
    let mut drum_windows = MeshBuilder::default();
    for k in 0..16 {
        let a = k as f32 / 16.0 * TAU;
        let d = Vec3::new(a.cos(), 0.0, a.sin());
        let side = Vec3::Y.cross(d);
        let c = base + d * 9.03 + Vec3::Y * (drum_y + 1.2);
        drum_windows.quad_facing([c - side * 0.7, c + side * 0.7, c + side * 0.7 + Vec3::Y * 2.6, c - side * 0.7 + Vec3::Y * 2.6], d, Color::WHITE, [Vec2::ZERO; 4]);
    }
    add(commands, meshes, drum_windows, &m.lit_window, false);
    let dome_base = base + Vec3::Y * (drum_y + drum_h + 0.25);
    commands.spawn((Mesh3d(meshes.add(hemisphere(dome_r, 1.05))), MeshMaterial3d(m.rose_gold.clone()), Transform::from_translation(dome_base), Scenery));
    let top_y = dome_r * 1.05;
    commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.gold.clone()), Transform::from_translation(dome_base + Vec3::Y * (top_y + 1.0)).with_scale(Vec3::new(3.0, 2.6, 3.0)), Scenery));
    commands.spawn((Mesh3d(kit.cone.clone()), MeshMaterial3d(m.gold.clone()), Transform::from_translation(dome_base + Vec3::Y * (top_y + 4.3)).with_scale(Vec3::new(1.8, 4.0, 1.8)), Scenery));
    commands.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(m.gold.clone()), Transform::from_translation(dome_base + Vec3::Y * (top_y + 6.6)).with_scale(Vec3::splat(0.9)), Scenery));
}

// ----- Land and water ----------------------------------------------------------------------------

/// Lawns, paving, beaches and rocks as one heightfield, and the lagoon over everything else:
/// turquoise over the shallows fading to deep blue, rippled by a scrolling normal map.
fn spawn_land_and_water(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, tex: &Textures, track: &Track, layout: &Layout) {
    let (lo, hi) = bounds(track);
    let (lo, hi) = (lo - Vec2::splat(140.0), hi + Vec2::splat(140.0));
    let cell = 2.5;
    let (nx, nz) = (((hi.x - lo.x) / cell) as usize + 1, ((hi.y - lo.y) / cell) as usize + 1);
    let grid: Vec<Ground> = (0..=nz).flat_map(|z| (0..=nx).map(move |x| (x, z))).map(|(x, z)| layout.ground(track, lo + Vec2::new(x as f32, z as f32) * cell)).collect();
    let g = |x: usize, z: usize| grid[z * (nx + 1) + x];
    let pos = |x: usize, z: usize| lo + Vec2::new(x as f32, z as f32) * cell;

    let colour = |gr: Ground, p: Vec2| {
        let n = (p.x * 0.05).sin() * (p.y * 0.043).cos() * 0.5 + (p.x * 0.013 + p.y * 0.017).sin() * 0.5;
        let k = 1.0 + n * 0.07;
        let (r, gg, b) = if gr.h < WATER_Y + 0.15 {
            (0.62, 0.52, 0.38) // wet sand
        } else if gr.h < LAND_Y - 0.12 {
            (0.9, 0.8, 0.62) // beach
        } else if gr.rock {
            (0.5, 0.45, 0.4)
        } else if gr.paved {
            (0.82, 0.76, 0.66)
        } else {
            (0.34, 0.54, 0.22)
        };
        Color::srgb(r * k, gg * k, b * k)
    };
    let mut land = MeshBuilder::default();
    for z in 0..nz {
        for x in 0..nx {
            let c = [(x, z), (x, z + 1), (x + 1, z + 1), (x + 1, z)];
            let gs = c.map(|(x, z)| g(x, z));
            if gs.iter().all(|gr| gr.h < WATER_Y - 0.05) {
                continue;
            }
            let ps = c.map(|(x, z)| v3(pos(x, z), g(x, z).h));
            let n = (ps[1] - ps[0]).cross(ps[2] - ps[0]).normalize_or(Vec3::Y);
            let cols = [0, 1, 2, 3].map(|k| colour(gs[k], pos(c[k].0, c[k].1)));
            land.quad_vc(ps, if n.y < 0.0 { -n } else { n }, cols, c.map(|(x, z)| pos(x, z) / 5.0));
        }
    }
    let land_mat = materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.grass.clone()), perceptual_roughness: 0.85, ..default() });
    commands.spawn((land.tinted(meshes, materials, &land_mat), NotShadowCaster, Scenery));

    // The water: a fine grid over the lagoon for the shallows' colour, then open sea.
    let wcell = 10.0;
    let (wx, wz) = (((hi.x - lo.x) / wcell).ceil() as usize, ((hi.y - lo.y) / wcell).ceil() as usize);
    let depth_col = |p: Vec2| {
        let d = (WATER_Y - layout.ground(track, p).h).max(0.0);
        let shallow = Vec3::new(0.32, 0.74, 0.68);
        let mid = Vec3::new(0.06, 0.36, 0.44);
        let deep = Vec3::new(0.03, 0.14, 0.24);
        let c = if d < 1.2 { shallow.lerp(mid, d / 1.2) } else { mid.lerp(deep, ((d - 1.2) / 4.0).min(1.0)) };
        Color::srgb(c.x, c.y, c.z)
    };
    let deep = Color::srgb(0.03, 0.14, 0.24);
    let mut water = MeshBuilder::default();
    let wy = WATER_Y;
    let wpos = |x: usize, z: usize| lo + Vec2::new(x as f32 * wcell, z as f32 * wcell);
    let cols: Vec<Color> = (0..=wz).flat_map(|z| (0..=wx).map(move |x| (x, z))).map(|(x, z)| depth_col(wpos(x, z))).collect();
    for z in 0..wz {
        for x in 0..wx {
            let c = [(x, z), (x, z + 1), (x + 1, z + 1), (x + 1, z)];
            let p = c.map(|(x, z)| wpos(x, z));
            water.quad_vc(p.map(|q| v3(q, wy)), Vec3::Y, c.map(|(x, z)| cols[z * (wx + 1) + x]), p.map(|q| q / 9.0));
        }
    }
    // Open sea out to the horizon, in strips that meet the grid's edges.
    let (a, b) = (lo, wpos(wx, wz));
    let far = 4000.0;
    let ring = [
        [Vec2::new(a.x - far, a.y - far), Vec2::new(a.x - far, b.y + far), Vec2::new(a.x, b.y + far), Vec2::new(a.x, a.y - far)],
        [Vec2::new(b.x, a.y - far), Vec2::new(b.x, b.y + far), Vec2::new(b.x + far, b.y + far), Vec2::new(b.x + far, a.y - far)],
        [Vec2::new(a.x, a.y - far), Vec2::new(a.x, a.y), Vec2::new(b.x, a.y), Vec2::new(b.x, a.y - far)],
        [Vec2::new(a.x, b.y), Vec2::new(a.x, b.y + far), Vec2::new(b.x, b.y + far), Vec2::new(b.x, b.y)],
    ];
    for q in ring {
        water.quad_vc(q.map(|p| v3(p, wy)), Vec3::Y, [deep; 4], q.map(|p| p / 9.0));
    }
    let mut mesh = water.build();
    let _ = mesh.generate_tangents();
    let template = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        normal_map_texture: Some(tex.ripples.clone()),
        // Just rough enough that the sun glints off it as a glittering path.
        perceptual_roughness: 0.06,
        reflectance: 0.35,
        ..default()
    });
    let (mesh3d, mat) = tinted_mesh(mesh, meshes, materials, &template);
    commands.insert_resource(WaterMat(mat.0.clone()));
    commands.spawn((mesh3d, mat, NotShadowCaster, super::gfx::RaytraceSink(0.1), Scenery));
}

// ----- Scenery -----------------------------------------------------------------------------------

struct Scene<'a> {
    kit: &'a Kit,
    m: &'a Mats,
    track: &'a Track,
    layout: &'a Layout,
}

impl Scene<'_> {
    fn n(&self) -> usize {
        self.track.samples.len()
    }

    /// Samples in `zone`, every `step`, starting `offset` in.
    fn every(&self, zone: Zone, step: usize, offset: usize) -> Vec<usize> {
        (0..self.n()).filter(|&i| self.layout.zone[i] == zone && (i + offset).is_multiple_of(step)).collect()
    }

    fn on_land(&self, p: Vec2) -> bool {
        self.layout.ground(self.track, p).h > LAND_Y - 0.02
    }

    /// A gold lamp post with a glowing globe, standing at `base`.
    fn lamp_post(&self, commands: &mut Commands, base: Vec3, height: f32) {
        let (kit, m) = (self.kit, self.m);
        commands.spawn((Transform::from_translation(base), Visibility::default(), Scenery)).with_children(|l| {
            part(l, &kit.cylinder, &m.gold, t(0.0, height * 0.5, 0.0).s(0.16, height, 0.16));
            part(l, &kit.cylinder, &m.gold, t(0.0, 0.15, 0.0).s(0.45, 0.3, 0.45));
            l.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(m.lamp.clone()), t(0.0, height + 0.35, 0.0).s(0.75, 0.75, 0.75), NotShadowCaster));
            part(l, &kit.cone, &m.gold, t(0.0, height + 0.85, 0.0).s(0.45, 0.35, 0.45));
        });
    }

    /// The start straight: gantry, grandstand, the lamp-lit promenade, and the marina.
    #[allow(clippy::too_many_arguments)]
    fn promenade(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>, surf: &SurfaceMats, signs: &SignAtlas, rng: &mut Lcg, reserved: &mut Reserved) {
        let (kit, m, track) = (self.kit, self.m, self.track);
        spawn_gantry(commands, kit, meshes, &signs.material, track, &m.ivory, &m.gold, SIGN_LAGOON);
        spawn_grandstand(commands, kit, meshes, surf, track, 25.0, 190.0, 1.0, rng, reserved);

        for i in self.every(Zone::Promenade, 22, 0) {
            let p = at(track, i, -(WALL_DIST + 3.2));
            if self.on_land(p) {
                self.lamp_post(commands, v3(p, 0.0), 4.4);
            }
        }
        // Sponsor boards on the outside of T1.
        let t1 = track.mark("headland");
        for (s, sign) in [(t1 + 10.0, 13), (t1 + 40.0, 7)] {
            spawn_sign(commands, meshes, &signs.material, kit, track, s, -(WALL_DIST + 0.35), sign, Vec2::new(7.0, 1.5), 1.44);
        }

        // Pontoons out from the quay, with yachts moored either side.
        let hull = meshes.add(hull_mesh());
        let deck = meshes.add(deck_mesh());
        let hull_mat = m.hull.clone();
        let mut yachts = 0;
        for s in [45.0, 95.0, 145.0, 195.0] {
            let i = (s / track.spacing) as usize;
            let (c, d) = track.point_at(s);
            let out = -d.perp();
            let quay = self.layout.shore[i][0] - 1.0;
            let len = 36.0;
            let mid = c + out * (quay + len * 0.5);
            let yaw = Quat::from_rotation_arc(-Vec3::Z, v3(out, 0.0));
            commands.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(m.wood.clone()), Transform::from_translation(v3(mid, WATER_Y + 0.35)).with_rotation(yaw).with_scale(Vec3::new(2.4, 0.25, len)), Scenery));
            for k in 0..5 {
                let q = c + out * (quay + 4.0 + k as f32 * 8.0);
                for side in [-1.0f32, 1.0] {
                    let post = q + d * side * 1.3;
                    commands.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.wood.clone()), Transform::from_translation(v3(post, WATER_Y + 0.3)).with_scale(Vec3::new(0.3, 1.8, 0.3)), Scenery));
                }
                // Yachts in alternate berths, bows pointing out to sea.
                if k % 2 == 1 && k < 5 {
                    for side in [-1.0f32, 1.0] {
                        if rng.f() < 0.2 {
                            continue;
                        }
                        let berth = q + d * side * 5.6 + out * 3.0;
                        self.yacht(commands, &hull, &deck, &hull_mat, v3(berth, WATER_Y), yaw, yachts, rng);
                        yachts += 1;
                    }
                }
            }
        }
    }

    /// A moored yacht: lacquered hull, teak deck, cabin with dark windows, mast and chrome rails.
    #[allow(clippy::too_many_arguments)]
    fn yacht(&self, commands: &mut Commands, hull: &Handle<Mesh>, deck: &Handle<Mesh>, hull_mat: &Handle<StandardMaterial>, at: Vec3, yaw: Quat, k: usize, rng: &mut Lcg) {
        let (kit, m) = (self.kit, self.m);
        let scale = rng.range(0.85, 1.15);
        let tilt = Quat::from_rotation_y(rng.range(-0.05, 0.05));
        commands
            .spawn((Transform::from_translation(at).with_rotation(yaw * tilt).with_scale(Vec3::splat(scale)), Visibility::default(), Floating { base: at, phase: k as f32 * 1.7 }, Scenery))
            .with_children(|y| {
                y.spawn((Mesh3d(hull.clone()), MeshMaterial3d(hull_mat.clone()), Transform::default()));
                y.spawn((Mesh3d(deck.clone()), MeshMaterial3d(m.wood.clone()), Transform::default()));
                part(y, &kit.cube, hull_mat, t(0.0, 1.95, 1.2).s(3.0, 1.1, 5.2));
                part(y, &kit.cube, &m.window, t(0.0, 2.05, 1.2).s(3.06, 0.45, 4.4));
                y.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(m.lit_window.clone()), t(0.0, 2.05, 3.81).s(1.8, 0.4, 0.04)));
                part(y, &kit.cylinder, hull_mat, t(0.0, 9.5, -0.6).s(0.24, 16.0, 0.24));
                part(y, &kit.cylinder, hull_mat, t(0.0, 3.4, 1.8).rx(FRAC_PI_2).s(0.18, 5.4, 0.18));
                for x in [-1.0f32, 1.0] {
                    part(y, &kit.cylinder, &m.chrome, t(x * 1.95, 2.1, 0.6).rx(FRAC_PI_2).s(0.06, 10.0, 0.06));
                }
                part(y, &kit.cylinder, &m.chrome, t(0.0, 1.9, -6.2).rx(FRAC_PI_2 - 0.3).s(0.08, 2.0, 0.08));
            });
    }

    /// Lamps on the causeway's balustrade, their globes doubled in the water below.
    fn causeway_lamps(&self, commands: &mut Commands) {
        for i in self.every(Zone::Causeway, 18, 0) {
            let side = if (i / 18) % 2 == 0 { 1.0 } else { -1.0 };
            for s in [side, -side] {
                let j = if s == side { i } else { i + 9 };
                self.lamp_post(commands, at3(self.track, j, s * (WALL_DIST + PLINTH * 0.5), PLINTH_H), 3.8);
            }
        }
    }

    /// An ivory lighthouse on the rocks outside the hairpin, its lamp lit for the evening.
    fn lighthouse(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>, at: Vec2, raytracing: bool, rng: &mut Lcg) {
        let (kit, m) = (self.kit, self.m);
        let base = v3(at, 1.9);
        let (h, r0, r1) = (21.0, 3.4, 2.3);
        let tower = lathe(&[(r0 + 0.6, 0.0), (r0, 0.8), (r1, h), (r1 + 0.1, h)], 24);
        commands.spawn((Mesh3d(meshes.add(tower)), MeshMaterial3d(m.stucco.clone()), Transform::from_translation(base), Scenery));
        commands.spawn((Transform::from_translation(base), Visibility::default(), Scenery)).with_children(|l| {
            for y in [7.0, 14.0] {
                let r = r0 + (r1 - r0) * y / h;
                part(l, &kit.cylinder, &m.gold, t(0.0, y, 0.0).s(r * 2.0 + 0.1, 0.5, r * 2.0 + 0.1));
            }
            part(l, &kit.cylinder, &m.gold, t(0.0, h + 0.2, 0.0).s(7.2, 0.4, 7.2));
            part(l, &kit.torus, &m.gold, t(0.0, h + 1.3, 0.0).s(6.6, 1.0, 6.6));
            for k in 0..12 {
                let a = k as f32 / 12.0 * TAU;
                part(l, &kit.cylinder, &m.gold, t(a.cos() * 3.4, h + 0.8, a.sin() * 3.4).s(0.08, 1.1, 0.08));
            }
            l.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(m.lamp.clone()), t(0.0, h + 2.0, 0.0).s(2.2, 2.2, 2.2), NotShadowCaster));
            l.spawn((Mesh3d(kit.cylinder.clone()), MeshMaterial3d(m.glass.clone()), t(0.0, h + 2.0, 0.0).s(4.2, 3.2, 4.2), NotShadowCaster));
            part(l, &kit.cone, &m.rose_gold, t(0.0, h + 4.7, 0.0).s(5.2, 2.2, 5.2));
            part(l, &kit.sphere, &m.gold, t(0.0, h + 6.0, 0.0).s(0.8, 0.8, 0.8));
            // keeper's cottage
            part(l, &kit.cube, &m.stucco, t(7.5, 1.8, 2.0).s(6.0, 3.6, 5.0));
            part(l, &kit.cube, &m.terracotta, t(7.5, 3.9, 2.0).s(6.6, 0.6, 5.6));
            l.spawn((Mesh3d(kit.cube.clone()), MeshMaterial3d(m.lit_window.clone()), t(4.47, 1.8, 2.0).s(0.05, 1.3, 1.2)));
            if !raytracing {
                l.spawn((PointLight { color: Color::srgb(1.0, 0.75, 0.45), intensity: 8_000_000.0, range: 70.0, radius: 1.0, shadow_maps_enabled: false, ..default() }, t(0.0, h + 2.0, 0.0)));
            }
        });
        // Rocks tumbling into the water.
        for k in 0..18 {
            let a = k as f32 / 18.0 * TAU + rng.range(-0.1, 0.1);
            let p = at + Vec2::from_angle(a) * rng.range(15.0, 22.0);
            let s = rng.range(2.5, 5.5);
            commands.spawn((
                Mesh3d(kit.sphere.clone()),
                MeshMaterial3d(m.rock.clone()),
                Transform::from_translation(v3(p, WATER_Y + s * 0.1)).with_scale(Vec3::new(s * 1.4, s * 0.8, s * 1.1)).with_rotation(Quat::from_rotation_y(rng.f() * TAU)),
                Scenery,
            ));
        }
    }

    /// Gold columns either side, a stone pergola overhead, bougainvillea and hanging lanterns.
    /// With the sun low on the right, the columns stripe the road with long shadows.
    fn colonnade(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        let (kit, m, track) = (self.kit, self.m, self.track);
        let idx: Vec<usize> = (0..self.n()).filter(|&i| self.layout.zone[i] == Zone::Colonnade).collect();
        let (c0, c1) = (idx[0] + 4, *idx.last().unwrap() - 4);
        let (lat0, lat1) = (WALL_DIST + 1.2, WALL_DIST + 3.6);
        let (y0, y1) = (9.5, 10.5);
        let mut stone = MeshBuilder::default();
        let ivory = Color::srgb(0.93, 0.89, 0.8);
        for i in c0..c1 {
            for side in [-1.0f32, 1.0] {
                side_face(&mut stone, track, i, side * lat0, y0, y1, -side, ivory);
                side_face(&mut stone, track, i, side * lat1, y0, y1, side, ivory);
                for (y, up) in [(y1, Vec3::Y), (y0, -Vec3::Y)] {
                    let q = [at3(track, i, side * lat0, y), at3(track, i, side * lat1, y), at3(track, i + 1, side * lat1, y), at3(track, i + 1, side * lat0, y)];
                    stone.quad_facing(q, up, ivory, [Vec2::ZERO; 4]);
                }
            }
            // pergola beams across the top
            if (i - c0) % 4 == 0 {
                let (o, d) = (track.samples[i].pos, track.samples[i].dir);
                let h = track.height_idx(i);
                let p = |l: f32, a: f32, y: f32| v3(o + d.perp() * l + d * a, h + y);
                let (a0, a1, ya, yb) = (-0.25, 0.25, y1, y1 + 0.55);
                stone.quad_facing([p(-lat1, a0, yb), p(lat1, a0, yb), p(lat1, a1, yb), p(-lat1, a1, yb)], Vec3::Y, ivory, [Vec2::ZERO; 4]);
                stone.quad_facing([p(-lat1, a0, ya), p(lat1, a0, ya), p(lat1, a1, ya), p(-lat1, a1, ya)], -Vec3::Y, ivory, [Vec2::ZERO; 4]);
                for a in [a0, a1] {
                    stone.quad_facing([p(-lat1, a, ya), p(lat1, a, ya), p(lat1, a, yb), p(-lat1, a, yb)], v3(d * a.signum(), 0.0), ivory, [Vec2::ZERO; 4]);
                }
            }
        }
        commands.spawn((Mesh3d(meshes.add(stone.build())), MeshMaterial3d(m.ivory.clone()), Scenery));

        for i in (c0..=c1).step_by(8) {
            let h = track.height_idx(i);
            for side in [-1.0f32, 1.0] {
                let p = at(track, i, side * (WALL_DIST + 2.4));
                commands.spawn((Transform::from_translation(v3(p, h)), Visibility::default(), Scenery)).with_children(|c| {
                    part(c, &kit.cube, &m.ivory, t(0.0, 0.45, 0.0).s(2.0, 0.9, 2.0));
                    part(c, &kit.cylinder, &m.gold, t(0.0, 4.9, 0.0).s(1.5, 8.0, 1.5));
                    part(c, &kit.cube, &m.ivory, t(0.0, 9.2, 0.0).s(2.1, 0.6, 2.1));
                });
                // bougainvillea tumbling off the beam
                for k in 0..3 {
                    let q = at(track, i + k * 2, side * (WALL_DIST + 2.0 + k as f32 * 0.6));
                    let s = 1.3 + k as f32 * 0.3;
                    commands.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(m.flowers.clone()), Transform::from_translation(v3(q, h + 10.4 - k as f32 * 0.9)).with_scale(Vec3::new(s * 1.3, s, s * 1.2)), Scenery));
                }
            }
            // a lantern hanging over the middle of the road
            if (i - c0) % 16 == 8 {
                let p = v3(track.samples[i].pos, h);
                commands.spawn((Transform::from_translation(p), Visibility::default(), Scenery)).with_children(|l| {
                    part(l, &kit.cylinder, &m.gold, t(0.0, 9.3, 0.0).s(0.05, 2.4, 0.05));
                    part(l, &kit.cone, &m.gold, t(0.0, 8.05, 0.0).s(1.0, 0.5, 1.0));
                    l.spawn((Mesh3d(kit.capsule.clone()), MeshMaterial3d(m.lantern.clone()), t(0.0, 7.4, 0.0).s(0.7, 0.45, 0.7), NotShadowCaster));
                });
            }
        }
    }

    /// Pastel houses with shutters, awnings and lit windows either side of the chicane, and
    /// festoon lights strung across the road between them.
    fn village(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, rng: &mut Lcg, reserved: &mut Reserved) {
        let (m, track) = (self.m, self.track);
        let pastel = [
            Color::srgb(0.96, 0.62, 0.5),
            Color::srgb(0.98, 0.8, 0.46),
            Color::srgb(0.98, 0.9, 0.7),
            Color::srgb(0.95, 0.55, 0.42),
            Color::srgb(0.7, 0.84, 0.72),
            Color::srgb(0.62, 0.78, 0.9),
            Color::srgb(0.98, 0.72, 0.62),
        ];
        let mut walls = MeshBuilder::default();
        let mut roofs = MeshBuilder::default();
        let mut glass = MeshBuilder::default();
        let mut lit = MeshBuilder::default();
        let mut shutters = MeshBuilder::default();
        let mut awnings = MeshBuilder::default();
        let idx: Vec<usize> = (0..self.n()).filter(|&i| self.layout.zone[i] == Zone::Village).collect();
        let (v0, v1) = (idx[0], *idx.last().unwrap());
        for side in [-1.0f32, 1.0] {
            let mut i = v0;
            while i < v1 {
                let (o, d) = track.point_at(track.samples[i].s);
                let (w, depth, floors) = (rng.range(8.5, 10.5), rng.range(9.0, 12.0), 2 + (rng.f() * 2.6) as usize);
                let front = WALL_DIST + 5.5;
                let c = o + d.perp() * side * (front + depth * 0.5);
                i += (w / track.spacing).ceil() as usize + 1;
                if !self.on_land(c) || !reserved.free(c, 4.0) {
                    continue;
                }
                let h = 1.0 + floors as f32 * 3.2 + 0.6;
                let fwd = v3(d, 0.0);
                let out = v3(d.perp() * side, 0.0); // away from the track
                let base = v3(c, 0.0);
                let col = *rng.pick(&pastel);
                boxy(&mut walls, base, fwd, out, w * 0.5, h, depth * 0.5, col);
                hip_roof(&mut roofs, base + Vec3::Y * h, fwd, out, w * 0.5 + 0.4, depth * 0.5 + 0.4, 2.2, Color::WHITE);
                // The facade facing the road.
                let face = base - out * (depth * 0.5 + 0.03);
                let cols = ((w - 2.0) / 2.4) as usize;
                for f in 0..floors {
                    for k in 0..cols {
                        let x = (k as f32 - (cols - 1) as f32 * 0.5) * 2.4;
                        let y = 1.0 + 3.2 * (f as f32 + 1.0) + 0.3;
                        let q = |dx: f32, dy: f32, push: f32| face + fwd * (x + dx) + Vec3::Y * (y + dy) - out * push;
                        let win = [q(-0.5, 0.0, 0.0), q(0.5, 0.0, 0.0), q(0.5, 1.6, 0.0), q(-0.5, 1.6, 0.0)];
                        (if rng.f() < 0.35 { &mut lit } else { &mut glass }).quad_facing(win, -out, Color::WHITE, [Vec2::ZERO; 4]);
                        for sx in [-1.0f32, 1.0] {
                            let (a, b) = (sx * 0.55, sx * 1.05);
                            shutters.quad_facing([q(a, 0.0, 0.02), q(b, 0.0, 0.02), q(b, 1.6, 0.02), q(a, 1.6, 0.02)], -out, Color::WHITE, [Vec2::ZERO; 4]);
                        }
                    }
                }
                // A shop on the ground floor: a lit window under a striped awning.
                let q = |dx: f32, dy: f32, push: f32| face + fwd * dx + Vec3::Y * dy - out * push;
                let shop = [q(-w * 0.35, 0.4, 0.0), q(w * 0.35, 0.4, 0.0), q(w * 0.35, 2.9, 0.0), q(-w * 0.35, 2.9, 0.0)];
                (if rng.f() < 0.7 { &mut lit } else { &mut glass }).quad_facing(shop, -out, Color::WHITE, [Vec2::ZERO; 4]);
                let stripe = [Color::srgb(0.95, 0.94, 0.9), *rng.pick(&[Color::srgb(0.85, 0.2, 0.2), Color::srgb(0.15, 0.45, 0.6), Color::srgb(0.2, 0.55, 0.35)])];
                let strips = 8;
                for k in 0..strips {
                    let (x0, x1) = (-w * 0.4 + k as f32 * w * 0.8 / strips as f32, -w * 0.4 + (k + 1) as f32 * w * 0.8 / strips as f32);
                    let a = [q(x0, 3.4, 0.0), q(x1, 3.4, 0.0), q(x1, 2.6, 1.8), q(x0, 2.6, 1.8)];
                    awnings.quad_facing(a, Vec3::Y - out, stripe[k % 2], [Vec2::ZERO; 4]);
                    awnings.quad_facing(a, -Vec3::Y + out, stripe[k % 2], [Vec2::ZERO; 4]);
                }
            }
        }
        let wall_mat = m.stucco.clone();
        commands.spawn((walls.tinted(meshes, materials, &wall_mat), Scenery));
        let roof_tile = materials.add(StandardMaterial { base_color: Color::srgb(0.72, 0.33, 0.2), perceptual_roughness: 0.7, ..default() });
        commands.spawn((Mesh3d(meshes.add(roofs.build())), MeshMaterial3d(roof_tile), Scenery));
        commands.spawn((Mesh3d(meshes.add(glass.build())), MeshMaterial3d(m.window.clone()), NotShadowCaster, Scenery));
        commands.spawn((Mesh3d(meshes.add(lit.build())), MeshMaterial3d(m.lit_window.clone()), NotShadowCaster, Scenery));
        commands.spawn((Mesh3d(meshes.add(shutters.build())), MeshMaterial3d(m.shutters.clone()), NotShadowCaster, Scenery));
        commands.spawn((awnings.tinted(meshes, materials, &m.foliage), Scenery));

        // Festoon lights across the road.
        let mut bulbs = MeshBuilder::default();
        let mut coloured = MeshBuilder::default();
        let mut wire = MeshBuilder::default();
        let mut k = 0;
        for i in (v0 + 6..v1 - 4).step_by(13) {
            let a = at3(track, i, -(WALL_DIST + 5.5), 7.6);
            let b = at3(track, i + 3, WALL_DIST + 5.5, 7.6);
            let sag = 2.2;
            let p = |u: f32| a.lerp(b, u) - Vec3::Y * sag * 4.0 * u * (1.0 - u);
            let segs = 24;
            for s in 0..segs {
                let (u0, u1) = (s as f32 / segs as f32, (s + 1) as f32 / segs as f32);
                let (p0, p1) = (p(u0), p(u1));
                let w = Vec3::Y * 0.03;
                let across = (p1 - p0).cross(Vec3::Y);
                for f in [across, -across] {
                    wire.quad_facing([p0 - w, p1 - w, p1 + w, p0 + w], f, Color::WHITE, [Vec2::ZERO; 4]);
                }
                let bulb = p(u0 + 0.5 / segs as f32) - Vec3::Y * 0.2;
                k += 1;
                octahedron(if k % 5 == 0 { &mut coloured } else { &mut bulbs }, bulb, 0.09);
            }
        }
        commands.spawn((Mesh3d(meshes.add(bulbs.build())), MeshMaterial3d(m.lamp.clone()), NotShadowCaster, Scenery));
        let rose = materials.add(glow(LinearRgba::rgb(9.0, 1.6, 2.4)));
        commands.spawn((Mesh3d(meshes.add(coloured.build())), MeshMaterial3d(rose), NotShadowCaster, Scenery));
        let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.05), cull_mode: None, double_sided: true, ..default() });
        commands.spawn((Mesh3d(meshes.add(wire.build())), MeshMaterial3d(dark), NotShadowCaster, Scenery));
    }

    /// A chrome corgi on a black marble plinth on its own island, watching you go by.
    fn statue(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, at: Vec2) {
        let (kit, m) = (self.kit, self.m);
        let look = (self.track.point_at(0.0).0 - at).normalize();
        let yaw = Quat::from_rotation_y(-look.to_angle() - FRAC_PI_2);
        let up = materials.add(glow(LinearRgba::rgb(10.0, 7.0, 4.0)));
        commands.spawn((Transform::from_translation(v3(at, 0.5)).with_rotation(yaw), Visibility::default(), Scenery)).with_children(|s| {
            part(s, &kit.cylinder, &m.black_marble, t(0.0, 1.2, 0.0).s(15.0, 2.4, 15.0));
            part(s, &kit.cylinder, &m.gold, t(0.0, 2.45, 0.0).s(15.4, 0.12, 15.4));
            part(s, &kit.cylinder, &m.black_marble, t(0.0, 2.8, 0.0).s(12.0, 0.7, 12.0));
            for k in 0..10 {
                let a = k as f32 / 10.0 * TAU;
                s.spawn((Mesh3d(kit.sphere.clone()), MeshMaterial3d(up.clone()), t(a.cos() * 7.1, 2.5, a.sin() * 7.1).s(0.35, 0.15, 0.35), NotShadowCaster));
            }
            s.spawn((t(0.0, 3.15, 0.0).with_scale(Vec3::splat(7.0)), Visibility::default())).with_children(|c| sitting_corgi(c, kit, &kit.corgi.coats[0], &m.chrome, true, true));
        });
    }

    /// Palms along the promenade, round the islands and at the point; Italian cypresses on the
    /// headland and beside the colonnade.
    #[allow(clippy::too_many_arguments)]
    fn greenery(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, islets: &[Vec2], reserved: &Reserved, rng: &mut Lcg) {
        let (m, track) = (self.m, self.track);
        let mut trunks = MeshBuilder::default();
        let mut fronds = MeshBuilder::default();
        let mut palm_at = |p: Vec2, rng: &mut Lcg| {
            let lean = Vec2::from_angle(rng.f() * TAU) * rng.range(0.8, 2.6);
            palm(&mut trunks, &mut fronds, v3(p, LAND_Y), rng.range(8.0, 12.5), lean, rng);
        };
        for i in self.every(Zone::Promenade, 22, 11) {
            let p = at(track, i, -(WALL_DIST + 7.5));
            if self.on_land(p) {
                palm_at(p, rng);
            }
        }
        for i in self.every(Zone::Point, 14, 0) {
            let p = at(track, i, WALL_DIST + 5.5);
            if self.on_land(p) {
                palm_at(p, rng);
            }
        }
        for &c in islets {
            for k in 0..3 {
                palm_at(c + Vec2::from_angle(k as f32 * 2.1 + rng.f()) * rng.range(2.0, 5.0), rng);
            }
        }
        for isl in &self.layout.islands[1..2] {
            for k in 0..5 {
                palm_at(isl.c + Vec2::from_angle(k as f32 * TAU / 5.0 + 0.3) * 15.0, rng);
            }
        }
        // Scattered over the headland.
        let (lo, hi) = bounds(track);
        let mut placed = (0, 0);
        let mut tries = 0;
        while (placed.0 < 22 || placed.1 < 30) && tries < 20000 {
            tries += 1;
            let p = Vec2::new(rng.range(lo.x, hi.x + 80.0), rng.range(lo.y - 60.0, hi.y + 40.0));
            let (i, _, _) = Layout::nearest(track, p);
            if self.layout.zone[i] != Zone::Headland || clearance(track, p) < 5.0 || !reserved.free(p, 6.0) || !self.on_land(p) || !self.on_land(p + Vec2::splat(4.0)) {
                continue;
            }
            // keep clear of the Pavilion's walls and portals
            if (self.layout.tunnel.0.saturating_sub(12)..self.layout.tunnel.1 + 12).contains(&i) && clearance(track, p) < 14.0 {
                continue;
            }
            if placed.0 < 22 && rng.f() < 0.45 {
                palm_at(p, rng);
                placed.0 += 1;
            } else if placed.1 < 30 {
                self.cypress(commands, p, rng);
                placed.1 += 1;
            }
        }
        for i in self.every(Zone::Colonnade, 10, 0) {
            let p = at(track, i, -(WALL_DIST + 9.0));
            if self.on_land(p) {
                self.cypress(commands, p, rng);
            }
        }
        commands.spawn((trunks.tinted(meshes, materials, &m.foliage), Scenery));
        commands.spawn((fronds.tinted(meshes, materials, &m.foliage), NotShadowCaster, Scenery));
    }

    fn cypress(&self, commands: &mut Commands, p: Vec2, rng: &mut Lcg) {
        let h = rng.range(8.0, 13.0);
        let w = rng.range(1.6, 2.2);
        commands.spawn((Mesh3d(self.kit.capsule.clone()), MeshMaterial3d(self.m.cypress.clone()), Transform::from_translation(v3(p, h * 0.5)).with_scale(Vec3::new(w, h * 0.5, w)), Scenery));
    }

    /// Paper lanterns floating on the lagoon.
    fn lanterns(&self, commands: &mut Commands, rng: &mut Lcg) {
        let (lo, hi) = bounds(self.track);
        let mut placed = 0;
        let mut tries = 0;
        while placed < 55 && tries < 5000 {
            tries += 1;
            let p = Vec2::new(rng.range(lo.x, hi.x), rng.range(lo.y, hi.y));
            if clearance(self.track, p) < 4.0 || self.layout.ground(self.track, p).h > WATER_Y - 0.3 {
                continue;
            }
            placed += 1;
            let base = v3(p, WATER_Y + 0.2);
            commands.spawn((
                Mesh3d(self.kit.capsule.clone()),
                MeshMaterial3d(self.m.lantern.clone()),
                Transform::from_translation(base).with_scale(Vec3::new(0.9, 0.5, 0.9)),
                Floating { base, phase: rng.f() * TAU },
                NotShadowCaster,
                Scenery,
            ));
        }
    }

    /// Sunset clouds, far islands on the horizon (none where the sun sets), and balloons.
    fn sky(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, rng: &mut Lcg) {
        let (lo, hi) = bounds(self.track);
        let center = (lo + hi) * 0.5;
        let sun = super::look::look_for(crate::sim::Theme::Lagoon).sun_rot * Vec3::Z;
        let sun_az = Vec2::new(sun.x, sun.z).to_angle();
        // Unlit and blended, so they glow the same with and without raytracing.
        let cloud = |c: Color| StandardMaterial { base_color: c, unlit: true, alpha_mode: AlphaMode::Blend, ..default() };
        let warm = materials.add(cloud(Color::srgba(1.0, 0.68, 0.45, 0.85)));
        let pink = materials.add(cloud(Color::srgba(0.86, 0.58, 0.68, 0.75)));
        let puff = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
        for _ in 0..26 {
            let a = rng.f() * TAU;
            let p = center + Vec2::from_angle(a) * rng.range(300.0, 1500.0);
            let toward = (a - sun_az).cos();
            let mat = if toward > 0.3 { warm.clone() } else { pink.clone() };
            let y = rng.range(230.0, 320.0);
            commands.spawn((Transform::from_translation(v3(p, y)), Visibility::default(), Scenery)).with_children(|cl| {
                for _ in 0..6 {
                    let o = Vec3::new(rng.range(-28.0, 28.0), rng.range(-2.0, 3.0), rng.range(-10.0, 10.0));
                    let s = rng.range(9.0, 16.0);
                    cl.spawn((Mesh3d(puff.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(o).with_scale(Vec3::new(s * 1.6, s * 0.45, s)), NotShadowCaster, super::gfx::NoRaytrace));
                }
            });
        }
        let haze = materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.2, 0.3), perceptual_roughness: 1.0, ..default() });
        for k in 0..14 {
            let a = k as f32 / 14.0 * TAU + rng.range(-0.15, 0.15);
            if crate::sim::wrap_angle(a - sun_az).abs() < 0.6 {
                continue;
            }
            let p = center + Vec2::from_angle(a) * rng.range(1300.0, 1900.0);
            let s = rng.range(160.0, 320.0);
            commands.spawn((Mesh3d(self.kit.sphere.clone()), MeshMaterial3d(haze.clone()), Transform::from_translation(v3(p, -s * 0.2)).with_scale(Vec3::new(s * 1.8, s * 0.45, s)), NotShadowCaster, Scenery));
        }
        // With the sun this low their shadows would stretch across the whole lagoon.
        spawn_balloons(commands, self.kit, center, 3, false, rng);
    }
}

// ----- Models --------------------------------------------------------------------------------------

/// A yacht's hull, bow towards -Z, waterline at y = 0: white topsides and a navy boot stripe.
fn hull_mesh() -> Mesh {
    let mut b = MeshBuilder::default();
    let (st, sections) = (12, hull_sections());
    let white = Color::srgb(0.95, 0.95, 0.94);
    let navy = Color::srgb(0.06, 0.1, 0.26);
    let red = Color::srgb(0.4, 0.08, 0.08);
    for k in 0..st {
        let (a, b2) = (&sections[k], &sections[k + 1]);
        for side in [-1.0f32, 1.0] {
            let pt = |s: &[Vec3; 4], j: usize| Vec3::new(s[j].x * side, s[j].y, s[j].z);
            for (j, c) in [(0, white), (1, navy), (2, red)] {
                let q = [pt(a, j), pt(b2, j), pt(b2, j + 1), pt(a, j + 1)];
                b.quad_facing(q, Vec3::X * side + Vec3::Y * 0.1, c, [Vec2::ZERO; 4]);
            }
        }
    }
    // transom
    let s = &sections[st];
    let m = |j: usize, side: f32| Vec3::new(s[j].x * side, s[j].y, s[j].z);
    for (j, c) in [(0, white), (1, navy), (2, red)] {
        b.quad_facing([m(j, -1.0), m(j, 1.0), m(j + 1, 1.0), m(j + 1, -1.0)], Vec3::Z, c, [Vec2::ZERO; 4]);
    }
    b.build()
}

/// Hull cross-sections from bow to stern: deck edge, top of the stripe, chine, keel.
fn hull_sections() -> Vec<[Vec3; 4]> {
    let (st, len) = (12, 14.0);
    (0..=st)
        .map(|k| {
            let t = k as f32 / st as f32;
            let z = -len * 0.5 + t * len;
            let w = 2.2 * (1.0 - (1.0 - t).powf(2.2)).sqrt().max(0.02);
            let deck = 1.3 + 0.6 * (1.0 - t).powi(2);
            [Vec3::new(w, deck, z), Vec3::new(w * 0.97, 0.5, z), Vec3::new(w * 0.82, 0.05, z), Vec3::new(0.0, -0.8, z)]
        })
        .collect()
}

fn deck_mesh() -> Mesh {
    let mut b = MeshBuilder::default();
    let s = hull_sections();
    for k in 0..s.len() - 1 {
        let (a, c) = (s[k][0], s[k + 1][0]);
        let q = [Vec3::new(-a.x, a.y, a.z), Vec3::new(a.x, a.y, a.z), Vec3::new(c.x, c.y, c.z), Vec3::new(-c.x, c.y, c.z)];
        b.quad_facing(q, Vec3::Y, Color::WHITE, q.map(|p| Vec2::new(p.x, p.z) * 0.5));
    }
    b.build()
}


#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// A NaN vertex poisons the mesh's bounding box, and it gets culled from some angles.
    #[test]
    fn models_are_finite() {
        let (mut trunks, mut fronds, mut bulbs, mut roofs) = Default::default();
        let mut rng = Lcg(7);
        for k in 0..20 {
            palm(&mut trunks, &mut fronds, Vec3::new(k as f32, 0.0, 0.0), rng.range(8.0, 12.5), Vec2::from_angle(k as f32) * 2.0, &mut rng);
        }
        octahedron(&mut bulbs, Vec3::ZERO, 0.1);
        hip_roof(&mut roofs, Vec3::ZERO, Vec3::X, Vec3::Z, 5.0, 6.0, 2.2, Color::WHITE);
        let meshes = [trunks.build(), fronds.build(), bulbs.build(), roofs.build(), hull_mesh(), deck_mesh(), hemisphere(9.6, 1.05), lathe(&[(4.0, 0.0), (3.4, 0.8), (2.3, 21.0)], 24)];
        for (k, m) in meshes.iter().enumerate() {
            let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("mesh {k} has no positions") };
            assert!(p.iter().flatten().all(|v| v.is_finite()), "mesh {k} has a non-finite vertex");
        }
    }
}
