//! Jade Jungle: midday in a lost temple city, built for sharp raytraced sunlight.
//!
//! - Dappled light: the trees along the road are crowned with hundreds of separate leaves, so the
//!   sun reaches the paving in patches that the raytracer shadows leaf by leaf.
//! - The temple hall: a corbelled stone vault lit by braziers and by shafts of sun through
//!   skylights in its roof, over a polished floor, gold glyph bands and jade statues.
//! - A stepped pyramid with a jade corgi on top, a waterfall into the river at the ford, a
//!   viaduct on stone columns hung with vines, and stone heads sunk in the undergrowth.
//!
//! Nothing here depends on the layout, so tracks made in the editor can use it: the hall, the
//! pyramid and the waterfalls go wherever they fit (see `find_hall`).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{light::NotShadowCaster, prelude::*};

use super::mesh::{v3, Lcg, MeshBuilder};
use super::models::{glow, part, t, TransformExt};
use super::scene::{Build, Scenery};
use super::scenery::{bounds, clearance, find_spot, find_stand, sitting_corgi, spawn_gantry, spawn_grandstand, strip_fits, Reserved};
use super::shapes::{beam, blob, boxy, cone, leaf, palm, side_face, strip, tri_facing, tube};
use super::signs::SIGN_JUNGLE;
use super::trackmesh::{at, at3, bridge_deck, bridge_flags, embankment, spawn_fords, spawn_pads_and_ramps, spawn_pillars, spawn_puddles, spawn_sign, start_line, wall, Ford, WALL_THICK};
use crate::sim::track::{Track, TrackId, ROAD_HALF_WIDTH, WALL_DIST};

/// The temple hall's inner cross-section on the right-hand side, as (lateral, height): up the
/// wall, in over the corbelled vault, to the edge of the roof slab. The left side mirrors it.
const VAULT: [(f32, f32); 8] = [(17.0, 0.0), (17.0, 7.0), (14.0, 7.0), (14.0, 9.0), (10.0, 9.0), (10.0, 11.0), (5.0, 11.0), (5.0, 12.5)];
/// The hall's outside, point for point with `VAULT`.
const SHELL: [(f32, f32); 8] = [(19.5, 0.0), (19.5, 8.5), (16.0, 8.5), (16.0, 10.5), (12.0, 10.5), (12.0, 12.5), (6.5, 12.5), (6.5, 14.0)];
/// A skylight every this many metres along the hall, each `SKYLIGHT` long.
const BAY: usize = 14;
const SKYLIGHT: std::ops::Range<usize> = 4..11;
/// The sides are open between piers `PIER` apart: each opening is this far along from its pier,
/// and from the top of a low wall up to the first corbel.
const PIER: usize = 7;
const WINDOW: std::ops::Range<usize> = 2..7;
const WINDOW_Y: (f32, f32) = (1.3, VAULT[1].1);
/// How long a hall a custom track gets, if a stretch of it has the room.
const HALL_LENGTH: f32 = 112.0;

const STONE: Color = Color::srgb(0.6, 0.57, 0.5);
const MOSS: Color = Color::srgb(0.3, 0.45, 0.17);

struct Mats {
    /// Weathered stone, coloured by vertex colours.
    stone: Handle<StandardMaterial>,
    road: Handle<StandardMaterial>,
    /// The hall's floor: the same paving, polished by centuries of paws.
    floor: Handle<StandardMaterial>,
    earth: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    jade: Handle<StandardMaterial>,
    bronze: Handle<StandardMaterial>,
    /// Trunks, branches and vines (vertex coloured).
    bark: Handle<StandardMaterial>,
    /// Leaves are one-sided blades, drawn from both sides.
    leaves: Handle<StandardMaterial>,
    flame: Handle<StandardMaterial>,
    falls: Handle<StandardMaterial>,
}

impl Mats {
    fn new(b: &mut Build) -> Self {
        let tex = b.tex;
        // For the raytracer the fires are real lights, and have to hold their own in a hall
        // next to daylight: this is their brightness in nits. (Otherwise they just glow, and
        // `brazier` adds a point light.)
        let fire = if b.raytracing { LinearRgba::rgb(260_000.0, 96_000.0, 15_000.0) } else { LinearRgba::rgb(11.0, 3.6, 0.6) };
        let mut add = |m: StandardMaterial| b.mats.add(m);
        Mats {
            stone: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.concrete.clone()), perceptual_roughness: 0.88, ..default() }),
            road: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.flagstones.clone()), perceptual_roughness: 0.62, ..default() }),
            floor: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.flagstones.clone()), perceptual_roughness: 0.1, reflectance: 0.55, ..default() }),
            earth: add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.grass.clone()), perceptual_roughness: 0.95, ..default() }),
            gold: add(StandardMaterial { base_color: Color::srgb(1.0, 0.76, 0.3), metallic: 1.0, perceptual_roughness: 0.2, ..default() }),
            // Tinted by the sculpt's greens: polished nephrite.
            jade: add(StandardMaterial { base_color: Color::srgb(0.7, 1.0, 0.82), perceptual_roughness: 0.14, reflectance: 0.65, clearcoat: 1.0, clearcoat_perceptual_roughness: 0.05, ..default() }),
            bronze: add(StandardMaterial { base_color: Color::srgb(0.45, 0.3, 0.16), metallic: 1.0, perceptual_roughness: 0.4, ..default() }),
            bark: add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() }),
            leaves: add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.6, double_sided: true, cull_mode: None, ..default() }),
            flame: add(glow(fire)),
            falls: add(StandardMaterial { base_color: Color::srgb(0.86, 0.96, 0.97), base_color_texture: Some(tex.falls.clone()), perceptual_roughness: 0.3, reflectance: 0.4, ..default() }),
        }
    }
}

/// Everything for a jungle track: the road, the forest and the ruins.
pub fn spawn(b: &mut Build) {
    let track = b.track;
    let m = Mats::new(b);
    let mut rng = Lcg(0x1A_DE ^ track.length as u32);
    let mut reserved = Reserved(vec![]);
    let hall = find_hall(track);

    spawn_surface(b, &m, hall);
    spawn_ground(b, &m);
    if let Some(hall) = hall {
        spawn_hall(b, &m, hall, &mut reserved);
    }
    spawn_start(b, &m, &mut rng, &mut reserved);
    spawn_pyramid(b, &m, &mut reserved);
    spawn_waterfalls(b, &m, &mut reserved, &mut rng);
    spawn_ruins(b, &m, &mut reserved, &mut rng);
    spawn_forest(b, &m, &reserved, &mut rng);
    spawn_far(b, &mut rng);
}

/// The stretch of road the temple hall stands over, as sample indices: between the track's
/// marks, or on a custom track the first level stretch gentle and clear enough to take it.
fn find_hall(track: &Track) -> Option<(usize, usize)> {
    let idx = |s: f32| (s / track.spacing) as usize;
    if let (Some(s0), Some(s1)) = (track.find_mark("temple"), track.find_mark("temple_end")) {
        return Some((idx(s0), idx(s1)));
    }
    let wide = SHELL[0].0 + 3.0;
    let mut s0 = 90.0;
    while s0 + HALL_LENGTH < track.length - 60.0 {
        let fits = (0..=(HALL_LENGTH as usize)).step_by(4).all(|o| {
            let s = s0 + o as f32;
            let here = &track.samples[idx(s) % track.samples.len()];
            here.curvature.abs() < 1.0 / 70.0 && !track.in_water(s - 12.0) && !track.in_water(s + 12.0) && track.ramp_at(s, 0.0).is_none()
        }) && [-1.0, 1.0].into_iter().all(|side| strip_fits(track, s0 - 8.0, s0 + HALL_LENGTH + 8.0, side, WALL_DIST, wide, false));
        if fits {
            return Some((idx(s0), idx(s0 + HALL_LENGTH)));
        }
        s0 += 10.0;
    }
    None
}

/// Is `p` in one of the ponds the fords are drawn as (see `spawn_fords`)?
fn in_pond(track: &Track, p: Vec2) -> bool {
    track.water.iter().any(|&(ws, len)| {
        let (c, d) = track.point_at(ws + len * 0.5);
        (p - c).dot(d).abs() < len * 0.5 + 8.0 && (p - c).dot(d.perp()).abs() < 84.0
    })
}

// ----- The road ----------------------------------------------------------------------------------

/// A causeway of mossy flagstones between stone walls, with earth verges; banked up with earth
/// where it climbs, and carried on stone columns where it crosses itself.
fn spawn_surface(b: &mut Build, m: &Mats, hall: Option<(usize, usize)>) {
    let track = b.track;
    let n = track.samples.len();
    let bridge = bridge_flags(track);
    let in_hall = |i: usize| hall.is_some_and(|(h0, h1)| (h0..h1).contains(&i));
    let (mut road, mut floor, mut verge, mut paint, mut stone, mut gold, mut vines, mut vine_leaves) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();

    for i in 0..n {
        let band = if (i / 8) % 2 == 0 { 0.0 } else { 0.02 };
        if in_hall(i) {
            strip(&mut floor, track, i, -WALL_DIST, WALL_DIST, 0.02, Color::srgb(0.36 + band, 0.33 + band, 0.29 + band));
        } else {
            strip(&mut road, track, i, -ROAD_HALF_WIDTH, ROAD_HALF_WIDTH, 0.02, Color::srgb(0.42 + band, 0.39 + band, 0.33 + band));
            let litter = if (i / 5) % 2 == 0 { 0.0 } else { 0.025 };
            for (l0, l1) in [(-WALL_DIST, -ROAD_HALF_WIDTH), (ROAD_HALF_WIDTH, WALL_DIST)] {
                strip(&mut verge, track, i, l0, l1, 0.0, Color::srgb(0.29 - litter, 0.21, 0.12 + litter));
            }
        }

        // Kerb stones: painted ochre and cream through the corners, bare and mossy elsewhere.
        let near_corner = (0..12).any(|o| track.samples[(i + n + o * 3 - 18) % n].curvature.abs() > 1.0 / 90.0);
        let odd = (i / 3) % 2 == 0;
        let kerb = match (near_corner, odd) {
            (true, true) => Color::srgb(0.8, 0.36, 0.12),
            (true, false) => Color::srgb(0.92, 0.86, 0.7),
            (false, true) => STONE,
            (false, false) => STONE.darker(0.14),
        };
        strip(&mut paint, track, i, -ROAD_HALF_WIDTH - 0.4, -ROAD_HALF_WIDTH + 1.1, 0.045, kerb);
        strip(&mut paint, track, i, ROAD_HALF_WIDTH - 1.1, ROAD_HALF_WIDTH + 0.4, 0.045, kerb);

        if in_hall(i) {
            continue;
        }
        // Walls of big stone blocks with moss along the top, and a gold-capped post now and then.
        for side in [-1.0f32, 1.0] {
            let block = if (i / 3) % 2 == 0 { STONE } else { STONE.darker(0.05) };
            wall(&mut stone, track, &bridge, i, side, WALL_THICK, 1.3, block, MOSS.mix(&STONE, 0.3));
            embankment(&mut verge, track, &bridge, i, side, Color::srgb(0.25, 0.36, 0.13));
            if i % 24 == 0 {
                let s = &track.samples[i];
                let base = at3(track, i, side * (WALL_DIST + WALL_THICK * 0.5), 0.0);
                let (fwd, out) = (v3(s.dir, 0.0), v3(s.dir.perp() * side, 0.0));
                boxy(&mut stone, base, fwd, out, 0.8, 2.1, 0.8, STONE.lighter(0.04));
                cone(&mut gold, base + Vec3::Y * 2.1, 1.0, 0.8, 4, Color::WHITE);
            }
            // Vines trailing off the viaduct.
            if bridge[i] && i % 3 == 0 {
                let top = at3(track, i, side * (WALL_DIST + WALL_THICK + 0.05), 1.2);
                let len = 2.5 + 4.0 * ((i * 37 % 11) as f32 / 11.0);
                tube(&mut vines, top, top - Vec3::Y * len, 0.05, 0.03, 4, Color::srgb(0.3, 0.24, 0.12));
                let out = v3(track.samples[i].dir.perp() * side, 0.0);
                for k in 1..(len as usize + 1) {
                    let dir = (Quat::from_rotation_y(k as f32 * 2.4) * out - Vec3::Y * 0.3).normalize();
                    leaf(&mut vine_leaves, top - Vec3::Y * k as f32, dir, 0.9, 0.5, 0.5, Color::srgb(0.2, 0.48, 0.14));
                }
            }
        }
        bridge_deck(&mut stone, track, &bridge, i, STONE.darker(0.08));
    }
    start_line(track, &mut paint, true);

    b.add_tinted(road, &m.road, false);
    b.add_tinted(floor, &m.floor, false);
    b.add_tinted(verge, &m.earth, false);
    b.add_tinted(paint, &m.stone, false);
    b.add_tinted(stone, &m.stone, true);
    b.add(gold, &m.gold);
    b.add_tinted(vines, &m.bark, true);
    b.add_tinted(vine_leaves, &m.leaves, true);
    spawn_puddles(b.commands, b.meshes, b.surf, track);
    spawn_pads_and_ramps(b.commands, b.meshes, b.mats, b.surf, track);
    spawn_pillars(b.commands, b.kit, &m.stone, track, &bridge);
    if !track.water.is_empty() {
        spawn_fords(b.commands, b.meshes, b.mats, b.kit, b.tex, track, Ford::River);
    }
}

/// The forest floor: dark earth and leaf mould, out to the horizon.
fn spawn_ground(b: &mut Build, m: &Mats) {
    let (lo, hi) = bounds(b.track);
    let center = (lo + hi) * 0.5;
    let mut ground = MeshBuilder::default();
    let (cell, half) = (25.0, 1600.0);
    let shade = |p: Vec2| {
        let k = 1.0 + ((p.x * 0.021).sin() * (p.y * 0.017).cos() + (p.x * 0.006 + p.y * 0.008).sin() * 0.8) * 0.12;
        Color::srgb(0.12 * k, 0.19 * k, 0.07 * k)
    };
    let mut z = center.y - half;
    while z < center.y + half {
        let mut x = center.x - half;
        while x < center.x + half {
            let q = [Vec2::new(x, z), Vec2::new(x, z + cell), Vec2::new(x + cell, z + cell), Vec2::new(x + cell, z)];
            ground.quad_vc(q.map(|c| v3(c, -0.03)), Vec3::Y, q.map(shade), q.map(|c| c / 5.0));
            x += cell;
        }
        z += cell;
    }
    b.commands.spawn((ground.tinted(b.meshes, b.mats, &m.earth), NotShadowCaster, super::gfx::RaytraceSink(0.1), Scenery));
}

// ----- The temple hall ---------------------------------------------------------------------------

/// A brazier standing on `base`: a stone pedestal, a bronze bowl and a flame (and, without
/// raytracing, a light for it).
fn brazier(b: &mut Build, stone: &mut MeshBuilder, bronze: &mut MeshBuilder, flames: &mut MeshBuilder, base: Vec3, range: f32) {
    boxy(stone, base, Vec3::X, Vec3::Z, 0.45, 1.3, 0.45, STONE.darker(0.1));
    blob(bronze, base + Vec3::Y * 1.75, Vec3::new(0.85, 0.45, 0.85), 10, false, Color::WHITE);
    blob(flames, base + Vec3::Y * 2.35, Vec3::new(0.62, 0.7, 0.62), 8, false, Color::WHITE);
    cone(flames, base + Vec3::Y * 2.6, 0.48, 1.4, 6, Color::WHITE);
    if !b.raytracing {
        b.commands.spawn((
            PointLight { color: Color::srgb(1.0, 0.55, 0.2), intensity: 600_000.0, range, radius: 0.5, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(base + Vec3::Y * 2.8),
            Scenery,
        ));
    }
}

/// The hall over samples `h0..h1`: a corbelled vault with skylights, pilasters, a gold glyph
/// band, braziers and jade statues inside; outside, stepped stone with a portal at each end and
/// a stone corgi's head over each portal.
fn spawn_hall(b: &mut Build, m: &Mats, (h0, h1): (usize, usize), reserved: &mut Reserved) {
    let track = b.track;
    let n = track.samples.len();
    let (mut inside, mut outside, mut gold, mut bronze, mut flames) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    let frame = |i: usize| {
        let s = &track.samples[i % n];
        (v3(s.pos, track.height_idx(i)), v3(s.dir.perp(), 0.0), v3(s.dir, 0.0))
    };
    // Point on the cross-section at sample `i`: `q` is (lateral, height).
    let cs = |i: usize, q: (f32, f32)| {
        let (o, right, _) = frame(i);
        o + right * q.0 + Vec3::Y * q.1
    };
    let skylit = |i: usize| i >= h0 + 6 && i + 8 < h1 && SKYLIGHT.contains(&((i - h0) % BAY));
    let windowed = |i: usize| i >= h0 + 4 && i + 5 < h1 && WINDOW.contains(&((i - h0) % PIER));
    let dark = Color::srgb(0.56, 0.5, 0.42);
    let (roof_in, roof_out) = (VAULT[7], SHELL[7]);

    for i in h0..h1 {
        let j = i + 1;
        reserved.add(track.samples[i].pos, SHELL[0].0 + 2.0);
        for side in [-1.0f32, 1.0] {
            let mirror = |q: (f32, f32)| (q.0 * side, q.1);
            for k in 0..VAULT.len() - 1 {
                // Moss on the outside's ledges, bare stone on its risers.
                let shade = if k % 2 == 0 { dark } else { dark.darker(0.05) };
                let col = if k % 2 == 1 { MOSS.mix(&STONE, 0.25) } else if (i / 4) % 2 == 0 { STONE } else { STONE.darker(0.04) };
                let inward = cs(i, (0.0, 5.0)) - cs(i, mirror(VAULT[k]));
                // The walls are in two pieces where there's a window: below it and above it.
                let spans: &[(f32, f32)] = if k == 0 && windowed(i) { &[(0.0, WINDOW_Y.0)] } else { &[(0.0, 1.0e3)] };
                for &(y0, y1) in spans {
                    for (profile, b, facing, c) in [(&VAULT, &mut inside, inward, shade), (&SHELL, &mut outside, -inward, col)] {
                        let (a, c2) = (mirror(profile[k]), mirror(profile[k + 1]));
                        let (a, c2) = ((a.0, a.1.max(y0)), (c2.0, c2.1.min(y1)));
                        let q = [cs(i, a), cs(i, c2), cs(j, c2), cs(j, a)];
                        b.quad_facing(q, facing, c, q.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
                    }
                }
            }
            // A window's sill and head, and its jambs where the wall starts again.
            if windowed(i) {
                let (l0, l1) = (VAULT[0].0 * side, SHELL[0].0 * side);
                for (y, up) in [(WINDOW_Y.0, Vec3::Y), (WINDOW_Y.1, -Vec3::Y)] {
                    inside.quad_facing([cs(i, (l0, y)), cs(i, (l1, y)), cs(j, (l1, y)), cs(j, (l0, y))], up, dark.lighter(0.04), [Vec2::ZERO; 4]);
                }
                // The outside wall above the opening, up to the first ledge.
                let q = [cs(i, (l1, WINDOW_Y.1)), cs(i, (l1, SHELL[1].1)), cs(j, (l1, SHELL[1].1)), cs(j, (l1, WINDOW_Y.1))];
                outside.quad_facing(q, frame(i).1 * side, STONE, q.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
                for (edge, closed, facing) in [(i, !windowed(i - 1), 1.0f32), (j, !windowed(j), -1.0)] {
                    if closed {
                        let q = [cs(edge, (l0, WINDOW_Y.0)), cs(edge, (l1, WINDOW_Y.0)), cs(edge, (l1, WINDOW_Y.1)), cs(edge, (l0, WINDOW_Y.1))];
                        inside.quad_facing(q, frame(edge).2 * facing, dark, [Vec2::ZERO; 4]);
                    }
                }
            }
            // Gold glyph bands on the piers, and a gold lip along the first corbel.
            if !windowed(i) {
                for (y0, y1) in [(4.4, 5.2), (2.2, 2.4)] {
                    side_face(&mut gold, track, i, side * (WALL_DIST - 0.06), y0, y1, -side, Color::WHITE);
                }
            }
            side_face(&mut gold, track, i, side * (VAULT[2].0 - 0.05), VAULT[2].1, VAULT[2].1 + 0.3, -side, Color::WHITE);
        }
        // The roof slab, inside and out, with the skylights left open.
        if skylit(i) {
            for side in [-1.0f32, 1.0] {
                let (a, c) = ((roof_in.0 * side, roof_in.1), (roof_in.0 * side, roof_out.1));
                inside.quad_facing([cs(i, a), cs(i, c), cs(j, c), cs(j, a)], -frame(i).1 * side, dark, [Vec2::ZERO; 4]);
                let (a, c) = ((roof_in.0 * side, roof_out.1), (roof_out.0 * side, roof_out.1));
                outside.quad_facing([cs(i, a), cs(i, c), cs(j, c), cs(j, a)], Vec3::Y, MOSS.mix(&STONE, 0.25), [Vec2::ZERO; 4]);
            }
        } else {
            let (a, c) = ((-roof_in.0, roof_in.1), (roof_in.0, roof_in.1));
            inside.quad_facing([cs(i, a), cs(i, c), cs(j, c), cs(j, a)], -Vec3::Y, dark.darker(0.06), [Vec2::ZERO; 4]);
            let (a, c) = ((-roof_out.0, roof_out.1), (roof_out.0, roof_out.1));
            outside.quad_facing([cs(i, a), cs(i, c), cs(j, c), cs(j, a)], Vec3::Y, MOSS.mix(&STONE, 0.25), [Vec2::ZERO; 4]);
        }
        // The ends of each skylight's shaft.
        if skylit(i) {
            for (edge, closed, facing) in [(i, !skylit(i - 1), 1.0f32), (j, !skylit(j), -1.0)] {
                if closed {
                    let q = [cs(edge, (-roof_in.0, roof_in.1)), cs(edge, (roof_in.0, roof_in.1)), cs(edge, (roof_in.0, roof_out.1)), cs(edge, (-roof_in.0, roof_out.1))];
                    inside.quad_facing(q, frame(edge).2 * facing, dark, [Vec2::ZERO; 4]);
                }
            }
        }

        // Against each pair of piers: a brazier on one side and a jade statue on the other.
        let k = i - h0;
        if k % PIER == 1 && k + PIER < h1 - h0 {
            let (o, right, _) = frame(i);
            let side = if (k / PIER) % 2 == 0 { 1.0 } else { -1.0 };
            brazier(b, &mut inside, &mut bronze, &mut flames, o + right * side * (WALL_DIST - 0.9), 24.0);
            let seat = o - right * side * (WALL_DIST - 1.3);
            boxy(&mut inside, seat, Vec3::X, Vec3::Z, 1.0, 1.1, 1.0, dark.lighter(0.08));
            boxy(&mut gold, seat + Vec3::Y * 1.1, Vec3::X, Vec3::Z, 1.08, 0.12, 1.08, Color::WHITE);
            // Facing across the hall.
            let yaw = Quat::from_rotation_arc(-Vec3::Z, right * side);
            b.commands
                .spawn((Transform::from_translation(seat + Vec3::Y * 1.22).with_rotation(yaw).with_scale(Vec3::splat(1.5)), Visibility::default(), Scenery))
                .with_children(|c| sitting_corgi(c, b.kit, &b.kit.corgi.topiary, &m.jade, false, false));
        }
    }

    // Portals: the stonework between the vault and the outside, a gold lintel, a stone corgi's
    // head on top and a brazier either side.
    for (i, out) in [(h0, -1.0f32), (h1, 1.0)] {
        let (o, right, fwd) = frame(i);
        let facing = fwd * out;
        let push = facing * 0.03;
        for side in [-1.0f32, 1.0] {
            for k in 0..VAULT.len() - 1 {
                let q = [VAULT[k], VAULT[k + 1], SHELL[k + 1], SHELL[k]].map(|(l, y)| cs(i, (l * side, y)) + push);
                for f in [facing, -facing] {
                    outside.quad_facing(q, f, STONE.lighter(0.03), q.map(|p| Vec2::new(p.x + p.z, p.y) / 3.0));
                }
            }
            brazier(b, &mut outside, &mut bronze, &mut flames, o + right * side * (SHELL[0].0 + 1.6) + facing * 2.5, 20.0);
        }
        let lintel = [(-roof_in.0, roof_in.1), (roof_in.0, roof_in.1), (roof_out.0, roof_out.1), (-roof_out.0, roof_out.1)].map(|q| cs(i, q) + push);
        outside.quad_facing(lintel, facing, STONE.lighter(0.03), [Vec2::ZERO; 4]);
        outside.quad_facing(lintel, -facing, STONE.lighter(0.03), [Vec2::ZERO; 4]);
        beam(&mut gold, cs(i, (-roof_in.0 - 1.0, roof_in.1 + 0.75)) + facing * 0.2, cs(i, (roof_in.0 + 1.0, roof_in.1 + 0.75)) + facing * 0.2, 0.3, 0.5, Vec3::Y, Color::WHITE);
        let yaw = Quat::from_rotation_arc(-Vec3::Z, facing);
        let head = stone_head_mat(b);
        b.commands.spawn((
            Mesh3d(b.kit.corgi.coats[0].head.clone()),
            MeshMaterial3d(head),
            Transform::from_translation(cs(i, (0.0, roof_out.1 + 2.3)) + facing * 0.6).with_rotation(yaw).with_scale(Vec3::splat(7.0)),
            Scenery,
        ));
    }

    b.add_tinted(inside, &m.stone, true);
    b.add_tinted(outside, &m.stone, true);
    b.add(gold, &m.gold);
    b.add(bronze, &m.bronze);
    b.add_flat(flames, &m.flame);
}

/// Stone for the carved heads: the sculpt's own colours make them terracotta with pale muzzles.
fn stone_head_mat(b: &mut Build) -> Handle<StandardMaterial> {
    b.mats.add(StandardMaterial { base_color: Color::srgb(0.74, 0.7, 0.64), base_color_texture: Some(b.tex.concrete.clone()), perceptual_roughness: 0.9, ..default() })
}

// ----- Set pieces --------------------------------------------------------------------------------

/// The start: a stone gantry, the grandstand, and sponsors' boards on the walls of the straights.
fn spawn_start(b: &mut Build, m: &Mats, rng: &mut Lcg, reserved: &mut Reserved) {
    let track = b.track;
    let pillar = b.mats.add(StandardMaterial { base_color: STONE, base_color_texture: Some(b.tex.concrete.clone()), perceptual_roughness: 0.88, ..default() });
    spawn_gantry(b.commands, b.kit, b.meshes, &b.signs.material, track, &pillar, &m.gold, SIGN_JUNGLE);
    let stand = if matches!(track.id, TrackId::Custom(_)) { find_stand(track, 120.0, 12.0) } else { Some((25.0, 185.0, -1.0)) };
    if let Some((s0, s1, side)) = stand {
        spawn_grandstand(b.commands, b.kit, b.meshes, b.surf, track, s0, s1, side, rng, reserved);
    }
    let n = track.samples.len();
    let (mut s, mut k) = (stand.map_or(60.0, |st| st.1 + 30.0), 0);
    while s < track.length - 30.0 {
        let i = (s / track.spacing) as usize % n;
        let straight = (0..9).all(|o| track.samples[(i + o * 3) % n].curvature.abs() < 1.0 / 150.0);
        // Only on level ground: a board up on the viaduct would stand its legs on the road below.
        if straight && track.samples[i].height < 0.3 && !track.in_water(s) && reserved.free(track.samples[i].pos, 0.0) {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let sign = [SIGN_JUNGLE + 1, 4, SIGN_JUNGLE, 8, 1][k % 5];
            spawn_sign(b.commands, b.meshes, &b.signs.material, b.kit, track, s, side * (WALL_DIST + WALL_THICK * 0.5), sign, Vec2::new(7.0, 1.5), 1.3);
            k += 1;
            s += 80.0;
        } else {
            s += 10.0;
        }
    }
}

/// A stepped pyramid with a stair up the side facing the start line, and on top a jade corgi
/// in a gold crown, whose head follows the karts.
fn spawn_pyramid(b: &mut Build, m: &Mats, reserved: &mut Reserved) {
    let track = b.track;
    let (lo, hi) = bounds(track);
    // Inside the first loop of the built-in lap; in the middle of a custom one.
    let want = if matches!(track.id, TrackId::Custom(_)) {
        (lo + hi) * 0.5
    } else {
        let (c, d) = track.point_at(115.0);
        c + d.perp() * 84.0
    };
    let Some(at) = [44.0, 36.0].into_iter().find_map(|r| find_spot(track, want, r, reserved, 30)) else { return };
    reserved.add(at, 44.0);
    let out = (track.point_at(0.0).0 - at).normalize();
    let (fwd, out) = (v3(out.perp(), 0.0), v3(out, 0.0));
    let base = v3(at, 0.0);
    const TIERS: usize = 6;
    const RISE: f32 = 3.0;
    let half = |k: usize| 31.0 - k as f32 * 4.3;
    let (mut stone, mut gold, mut bronze, mut flames) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    for k in 0..TIERS {
        let y = Vec3::Y * (k as f32 * RISE);
        let c = if k % 2 == 0 { STONE } else { STONE.darker(0.04) };
        boxy(&mut stone, base + y, fwd, out, half(k), RISE, half(k), c);
        // Moss on each terrace.
        let top = base + y + Vec3::Y * (RISE + 0.02);
        let q = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, z): (f32, f32)| top + (fwd * x + out * z) * half(k));
        stone.quad_facing(q, Vec3::Y, MOSS.mix(&STONE, 0.2), q.map(|p| Vec2::new(p.x, p.z) / 4.0));
    }
    let top_y = TIERS as f32 * RISE;
    // The stair: a ramp of steps between two stone cheeks.
    let steps = 30;
    let run = half(0) - half(TIERS - 1) + 4.0;
    for k in 0..steps {
        let f = k as f32 / steps as f32;
        let p = base + out * (half(0) + 4.0 - run * (f + 0.5 / steps as f32));
        boxy(&mut stone, p, fwd, out, 3.2, top_y * (k + 1) as f32 / steps as f32, run / steps as f32 * 0.5, STONE.lighter(0.05));
    }
    for sx in [-1.0f32, 1.0] {
        let (foot, head) = (base + out * (half(0) + 4.0) + fwd * sx * 3.7, base + out * half(TIERS - 1) + fwd * sx * 3.7 + Vec3::Y * top_y);
        beam(&mut stone, foot + Vec3::Y * 0.4, head + Vec3::Y * 0.4, 1.0, 1.6, Vec3::Y, STONE.darker(0.06));
        beam(&mut gold, foot + Vec3::Y * 1.3, head + Vec3::Y * 1.3, 0.5, 0.2, Vec3::Y, Color::WHITE);
    }
    // The shrine on top: a gold-edged dais, the idol, and a brazier at each corner.
    let dais = base + Vec3::Y * top_y;
    boxy(&mut stone, dais, fwd, out, 5.0, 1.2, 5.0, STONE.lighter(0.06));
    boxy(&mut gold, dais + Vec3::Y * 1.2, fwd, out, 5.15, 0.2, 5.15, Color::WHITE);
    for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        brazier(b, &mut stone, &mut bronze, &mut flames, dais + (fwd * x + out * z) * (half(TIERS - 1) - 1.4), 30.0);
    }
    let yaw = Quat::from_rotation_arc(-Vec3::Z, out);
    b.commands
        .spawn((Transform::from_translation(dais + Vec3::Y * 1.4).with_rotation(yaw).with_scale(Vec3::splat(6.5)), Visibility::default(), Scenery))
        .with_children(|c| sitting_corgi(c, b.kit, &b.kit.corgi.topiary, &m.jade, true, false));
    b.add_tinted(stone, &m.stone, true);
    b.add(gold, &m.gold);
    b.add(bronze, &m.bronze);
    b.add_flat(flames, &m.flame);
}

/// A rock mesa beside each ford with a waterfall down its face into the river.
fn spawn_waterfalls(b: &mut Build, m: &Mats, reserved: &mut Reserved, rng: &mut Lcg) {
    let track = b.track;
    let (mut rock, mut falls, mut foam) = <(MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    for &(ws, len) in &track.water {
        let (c, d) = track.point_at(ws + len * 0.5);
        // Whichever side of the road has the room.
        let spot = [1.0f32, -1.0].into_iter().map(|side| (side, c + d.perp() * side * 94.0)).find(|&(_, p)| clearance(track, p) > 42.0 && reserved.free(p, 36.0));
        let Some((side, at)) = spot else { continue };
        reserved.add(at, 40.0);
        let toward = -d.perp() * side;
        // The mesa: rings of rock, lumpier lower down.
        let rings = [(36.0, -0.5), (33.0, 7.0), (30.0, 15.0), (27.0, 21.0), (24.0, 24.0), (0.0, 25.5)];
        const SEGS: usize = 28;
        let lump: Vec<f32> = (0..SEGS).map(|_| rng.range(0.86, 1.1)).collect();
        let p = |k: usize, j: usize| {
            let (r, y) = rings[k];
            let a = j as f32 / SEGS as f32 * TAU;
            let wobble = 1.0 + (lump[j % SEGS] - 1.0) * (1.0 - k as f32 * 0.12);
            v3(at + Vec2::from_angle(a) * r * wobble, y)
        };
        for k in 0..rings.len() - 1 {
            for j in 0..SEGS {
                let q = [p(k, j), p(k, j + 1), p(k + 1, j + 1), p(k + 1, j)];
                let shade = 0.36 + 0.1 * lump[(j + k * 5) % SEGS];
                let col = if k >= 4 { Color::srgb(0.2, 0.36, 0.12) } else { Color::srgb(shade * 1.05, shade, shade * 0.9) };
                rock.quad_facing(q, (q[0] + q[2]) * 0.5 - v3(at, 8.0), col, q.map(|v| Vec2::new(v.x + v.z, v.y) / 5.0));
            }
        }
        // The fall: off the lip, out and down in an arc, into the pond.
        let across = v3(toward.perp(), 0.0);
        let fall = |u: f32, width: f32, x: f32| {
            let out = 24.5 + 9.0 * u.powf(0.6);
            v3(at + toward * out, 24.2 * (1.0 - u * u) + 0.25) + across * (x * width * (1.0 + 0.5 * u))
        };
        for (off, width) in [(0.0f32, 5.5f32), (9.5, 1.6), (-8.0, 2.2)] {
            const N: usize = 14;
            for k in 0..N {
                let (u0, u1) = (k as f32 / N as f32, (k + 1) as f32 / N as f32);
                let shift = across * off;
                let q = [fall(u0, width, -1.0) + shift, fall(u0, width, 1.0) + shift, fall(u1, width, 1.0) + shift, fall(u1, width, -1.0) + shift];
                let uv = [Vec2::new(0.0, u0 * 3.0), Vec2::new(width / 5.0, u0 * 3.0), Vec2::new(width / 5.0, u1 * 3.0), Vec2::new(0.0, u1 * 3.0)];
                falls.quad_facing(q, v3(toward, 0.5), Color::WHITE, uv);
            }
            for _ in 0..7 {
                let p = fall(1.0, width, rng.range(-1.2, 1.2)) + across * off + v3(toward, 0.0) * rng.range(-0.5, 2.0);
                let s = rng.range(0.7, 1.6);
                blob(&mut foam, p.with_y(0.25), Vec3::new(s * 1.4, s * 0.5, s * 1.4), 8, false, Color::WHITE);
            }
        }
        // Boulders round its foot.
        for _ in 0..16 {
            let a = rng.f() * TAU;
            let s = rng.range(2.0, 4.5);
            let shade = rng.range(0.34, 0.46);
            blob(&mut rock, v3(at + Vec2::from_angle(a) * rng.range(34.0, 40.0), s * 0.2), Vec3::new(s * 1.3, s * 0.8, s), 8, false, Color::srgb(shade * 1.05, shade, shade * 0.9));
        }
    }
    b.add_tinted(rock, &m.stone, true);
    b.add_flat(falls, &m.falls);
    let white = b.mats.add(StandardMaterial { base_color: Color::srgb(0.95, 0.98, 0.98), perceptual_roughness: 0.6, ..default() });
    b.add_flat(foam, &white);
    b.commands.insert_resource(super::ambient::ScrollMats(vec![(m.falls.clone(), Vec2::new(0.0, -0.45))]));
}

/// What the forest has swallowed: giant stone corgi heads, carved stelae banded with gold, and
/// fallen columns.
fn spawn_ruins(b: &mut Build, m: &Mats, reserved: &mut Reserved, rng: &mut Lcg) {
    let track = b.track;
    let (lo, hi) = bounds(track);
    let head_mat = stone_head_mat(b);
    let (mut stone, mut gold) = <(MeshBuilder, MeshBuilder)>::default();
    let spot = |rng: &mut Lcg, reserved: &Reserved, near: f32, far: f32, r: f32| {
        (0..400).find_map(|_| {
            let p = Vec2::new(rng.range(lo.x - far, hi.x + far), rng.range(lo.y - far, hi.y + far));
            let c = clearance(track, p);
            (c > near && c < far && reserved.free(p, r) && !in_pond(track, p)).then_some(p)
        })
    };
    for _ in 0..7 {
        let Some(p) = spot(rng, reserved, 9.0, 34.0, 7.0) else { break };
        reserved.add(p, 7.0);
        let s = rng.range(6.0, 10.0);
        let i = track.locate(p, 0).idx;
        let look = (track.samples[i].pos - p).normalize_or(Vec2::X);
        let tilt = Quat::from_rotation_z(rng.range(-0.3, 0.3)) * Quat::from_rotation_x(rng.range(-0.25, 0.1));
        b.commands.spawn((
            Mesh3d(b.kit.corgi.coats[0].head.clone()),
            MeshMaterial3d(head_mat.clone()),
            Transform::from_translation(v3(p, s * 0.28)).with_rotation(Quat::from_rotation_arc(-Vec3::Z, v3(look, 0.0)) * tilt).with_scale(Vec3::splat(s)),
            Scenery,
        ));
    }
    for _ in 0..14 {
        let Some(p) = spot(rng, reserved, 2.5, 9.0, 2.5) else { break };
        reserved.add(p, 2.5);
        let (fwd, h) = (Quat::from_rotation_y(rng.f() * TAU) * Vec3::X, rng.range(4.0, 6.5));
        let out = fwd.cross(Vec3::Y);
        boxy(&mut stone, v3(p, 0.0), fwd, out, 1.2, 0.6, 0.8, STONE.darker(0.05));
        boxy(&mut stone, v3(p, 0.6), fwd, out, 0.85, h, 0.45, STONE.lighter(rng.range(0.0, 0.06)));
        boxy(&mut gold, v3(p, 0.6 + h * 0.72), fwd, out, 0.9, 0.35, 0.5, Color::WHITE);
        boxy(&mut gold, v3(p, 0.6 + h * 0.3), fwd, out, 0.9, 0.12, 0.5, Color::WHITE);
    }
    for _ in 0..9 {
        let Some(p) = spot(rng, reserved, 4.0, 26.0, 5.0) else { break };
        reserved.add(p, 5.0);
        let dir = Vec2::from_angle(rng.f() * TAU);
        let (len, r) = (rng.range(5.0, 9.0), rng.range(0.7, 1.0));
        tube(&mut stone, v3(p - dir * len * 0.5, r * 0.8), v3(p + dir * len * 0.5, r * 0.8), r, r * 0.9, 12, STONE.lighter(0.03));
        boxy(&mut stone, v3(p + dir * (len * 0.5 + 0.2), 0.0), v3(dir, 0.0), v3(dir.perp(), 0.0), 0.35, r * 2.1, r * 1.25, STONE);
    }
    b.add_tinted(stone, &m.stone, true);
    b.add(gold, &m.gold);
}

// ----- The forest --------------------------------------------------------------------------------

const LEAF_GREENS: [Color; 5] = [Color::srgb(0.11, 0.34, 0.08), Color::srgb(0.15, 0.42, 0.1), Color::srgb(0.2, 0.5, 0.11), Color::srgb(0.08, 0.27, 0.08), Color::srgb(0.27, 0.53, 0.1)];

/// A forest giant: a buttressed trunk `height` tall and a crown `spread` across made of separate
/// leaves, so the sun comes through it in patches.
fn tree(trunks: &mut MeshBuilder, leaves: &mut MeshBuilder, base: Vec3, height: f32, spread: f32, rng: &mut Lcg) {
    let r0 = height * 0.032 + 0.22;
    let lean = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)) * height * 0.05;
    let spine = |f: f32| base + Vec3::Y * height * f + lean * f * f;
    let bark = Color::srgb(0.36, 0.28, 0.2).mix(&Color::srgb(0.3, 0.34, 0.2), rng.f() * 0.6);
    let profile = [(0.0, 1.7), (0.07, 1.15), (0.2, 1.0), (0.6, 0.8), (1.0, 0.55)];
    for w in profile.windows(2) {
        tube(trunks, spine(w[0].0), spine(w[1].0), r0 * w[0].1, r0 * w[1].1, 8, bark);
    }
    // Buttress roots.
    let turn = rng.f() * TAU;
    for k in 0..5 {
        let d = Quat::from_rotation_y(turn + k as f32 * TAU / 5.0) * Vec3::X;
        let fin = [base + d * r0 * 3.4, base + d * r0 * 0.9, base + d * r0 * 0.9 + Vec3::Y * r0 * 4.5];
        for f in [1.0f32, -1.0] {
            tri_facing(trunks, fin, d.cross(Vec3::Y) * f, bark.darker(0.03));
        }
    }
    // Boughs, and the crown: leaves pointing outwards all over a flattened dome.
    let top = spine(1.0);
    for k in 0..5 {
        let a = turn + k as f32 * TAU / 5.0 + rng.range(-0.3, 0.3);
        let end = top + Vec3::new(a.cos(), rng.range(0.1, 0.35), a.sin()) * spread * 0.55;
        tube(trunks, spine(0.86), end, r0 * 0.4, r0 * 0.12, 5, bark);
    }
    let count = (spread * 7.0) as usize;
    for _ in 0..count {
        let a = rng.f() * TAU;
        let up = rng.range(-0.15, 1.0f32);
        let flat = (1.0 - up * up * 0.8).sqrt() * rng.range(0.35, 1.0);
        let p = Vec3::new(a.cos() * flat, up * 0.42, a.sin() * flat) * spread;
        let dir = Vec3::new(p.x, p.y * 0.4 + rng.range(-0.1, 0.25) * spread, p.z).normalize_or(Vec3::X);
        let len = spread * rng.range(0.3, 0.5);
        // Drooping, so from the road you see their sunlit faces more than their undersides.
        leaf(leaves, top + p * 0.68, dir, len, len * rng.range(0.45, 0.65), rng.range(0.55, 1.1), *rng.pick(&LEAF_GREENS));
    }
}

/// The forest: giants along the road whose crowns meet over it, lianas slung between them,
/// more giants behind, a wall of treetops beyond, and an understorey of ferns, big-leaved
/// plants, palms and flowers.
fn spawn_forest(b: &mut Build, m: &Mats, reserved: &Reserved, rng: &mut Lcg) {
    let track = b.track;
    let n = track.samples.len();
    let (lo, hi) = bounds(track);
    let (mut trunks, mut leaves, mut far, mut under, mut flowers, mut fronds) = <(MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder, MeshBuilder)>::default();
    let free = |p: Vec2, r: f32, reserved: &Reserved| clearance(track, p) > r && reserved.free(p, r) && !in_pond(track, p);

    // Along the road, alternating sides. Left open at the start so the grid is in the sun.
    let mut avenue: Vec<(usize, f32, Vec3)> = vec![];
    for i in (0..n).step_by(13) {
        let side = if (i / 13) % 2 == 0 { 1.0 } else { -1.0 };
        let h = track.height_idx(i);
        let p = at(track, i, side * (WALL_DIST + WALL_THICK + 2.0 * h + rng.range(4.5, 9.0)));
        if track.samples[i].s < 30.0 || !free(p, 3.0, reserved) {
            continue;
        }
        let (height, spread) = (rng.range(15.0, 23.0) + h * 0.5, rng.range(10.0, 14.5));
        tree(&mut trunks, &mut leaves, v3(p, 0.0), height, spread, rng);
        avenue.push((i, side, v3(p, height)));
    }
    // Lianas from one side's trees to the other's, with leaves hanging off them.
    for w in avenue.windows(2).step_by(2) {
        let ((i, sa, a), (_, sb, c)) = (w[0], w[1]);
        if sa == sb || a.distance(c) > 60.0 || track.height_idx(i) > 1.0 {
            continue;
        }
        let (a, c) = (a - Vec3::Y * 4.0, c - Vec3::Y * 5.0);
        let sag = rng.range(2.5, 5.0);
        let p = |u: f32| a.lerp(c, u) - Vec3::Y * sag * 4.0 * u * (1.0 - u);
        const N: usize = 16;
        for k in 0..N {
            let (p0, p1) = (p(k as f32 / N as f32), p((k + 1) as f32 / N as f32));
            tube(&mut trunks, p0, p1, 0.09, 0.09, 4, Color::srgb(0.3, 0.26, 0.14));
            if k % 2 == 0 {
                let down = (Vec3::new(rng.range(-0.4, 0.4), -1.0, rng.range(-0.4, 0.4))).normalize();
                leaf(&mut leaves, p0, down, rng.range(1.2, 2.4), 0.7, 0.0, *rng.pick(&LEAF_GREENS));
            }
        }
    }
    // Giants behind them.
    let (mut placed, mut tries) = (0, 0);
    while placed < 150 && tries < 20_000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 90.0, hi.x + 90.0), rng.range(lo.y - 90.0, hi.y + 90.0));
        let c = clearance(track, p);
        if !(12.0..90.0).contains(&c) || !free(p, 6.0, reserved) {
            continue;
        }
        placed += 1;
        tree(&mut trunks, &mut leaves, v3(p, 0.0), rng.range(16.0, 27.0), rng.range(8.0, 13.0), rng);
    }
    // Beyond those the forest is a wall of rounded treetops.
    let (mut placed, mut tries) = (0, 0);
    while placed < 1500 && tries < 60_000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 420.0, hi.x + 420.0), rng.range(lo.y - 420.0, hi.y + 420.0));
        let c = clearance(track, p);
        // Thinner where the leafy giants stand, solid beyond them.
        if !(38.0..420.0).contains(&c) || (c < 80.0 && rng.f() > (c - 30.0) / 50.0) || !reserved.free(p, 16.0) {
            continue;
        }
        placed += 1;
        let (r, h) = (rng.range(11.0, 20.0), rng.range(13.0, 24.0) + c * 0.06);
        let k = rng.range(0.7, 1.15);
        blob(&mut far, v3(p, h), Vec3::new(r, r * 0.62, r), 10, false, Color::srgb(0.17 * k, 0.38 * k, 0.12 * k));
        blob(&mut far, v3(p + Vec2::new(rng.range(-0.7, 0.7), rng.range(-0.7, 0.7)) * r, h - r * 0.3), Vec3::new(r * 0.7, r * 0.5, r * 0.7), 8, false, Color::srgb(0.22 * k, 0.46 * k, 0.13 * k));
        if c < 120.0 {
            tube(&mut trunks, v3(p, 0.0), v3(p, h), 0.9, 0.6, 6, Color::srgb(0.33, 0.27, 0.19));
        }
    }

    // The understorey, thickest near the road.
    let (mut placed, mut tries) = (0, 0);
    while placed < 3200 && tries < 60_000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 60.0, hi.x + 60.0), rng.range(lo.y - 60.0, hi.y + 60.0));
        let c = clearance(track, p);
        if !(0.6..55.0).contains(&c) || rng.f() * 55.0 < c * 0.7 || !reserved.free(p, 1.0) || in_pond(track, p) {
            continue;
        }
        placed += 1;
        let base = v3(p, 0.0);
        let turn = rng.f() * TAU;
        match placed % 9 {
            // Elephant ears: a few huge leaves on stalks.
            0 | 1 => {
                let col = *rng.pick(&LEAF_GREENS);
                for k in 0..5 {
                    let d = Quat::from_rotation_y(turn + k as f32 * 1.3) * Vec3::X;
                    let top = base + d * rng.range(0.3, 0.9) + Vec3::Y * rng.range(1.0, 2.0);
                    tube(&mut trunks, base, top, 0.05, 0.03, 4, Color::srgb(0.24, 0.42, 0.14));
                    leaf(&mut under, top, (d + Vec3::Y * 0.2).normalize(), rng.range(1.3, 2.1), rng.range(1.0, 1.5), 0.45, col);
                }
            }
            // Heliconias: a spray of leaves and a red and yellow flower spike.
            2 => {
                for k in 0..4 {
                    let d = Quat::from_rotation_y(turn + k as f32 * 1.6) * Vec3::X;
                    leaf(&mut under, base, (d * 0.4 + Vec3::Y).normalize(), rng.range(1.6, 2.4), 0.6, 0.25, LEAF_GREENS[1]);
                }
                let col = *rng.pick(&[Color::srgb(0.95, 0.12, 0.1), Color::srgb(1.0, 0.5, 0.05), Color::srgb(0.95, 0.15, 0.6), Color::srgb(1.0, 0.85, 0.15)]);
                for k in 0..5 {
                    let y = 1.0 + k as f32 * 0.28;
                    let d = Quat::from_rotation_y(k as f32 * PI) * Vec3::X;
                    leaf(&mut flowers, base + Vec3::Y * y, (d + Vec3::Y * 0.5).normalize(), 0.5, 0.26, 0.2, col);
                }
            }
            // Ferns.
            _ => {
                let col = *rng.pick(&LEAF_GREENS);
                let size = rng.range(0.9, 1.9);
                for k in 0..7 {
                    let d = Quat::from_rotation_y(turn + k as f32 * TAU / 7.0) * Vec3::X;
                    leaf(&mut under, base, (d + Vec3::Y * rng.range(0.7, 1.2)).normalize(), size, size * 0.34, 0.75, col);
                }
            }
        }
    }
    // Palms in the gaps.
    let (mut placed, mut tries) = (0, 0);
    while placed < 70 && tries < 8000 {
        tries += 1;
        let p = Vec2::new(rng.range(lo.x - 50.0, hi.x + 50.0), rng.range(lo.y - 50.0, hi.y + 50.0));
        if !(4.0..40.0).contains(&clearance(track, p)) || !free(p, 4.0, reserved) {
            continue;
        }
        placed += 1;
        let lean = Vec2::from_angle(rng.f() * TAU) * rng.range(0.5, 2.5);
        palm(&mut trunks, &mut fronds, v3(p, 0.0), rng.range(6.0, 11.0), lean, rng);
    }

    b.add_tinted(trunks, &m.bark, true);
    b.add_tinted(leaves, &m.leaves, true);
    b.add_tinted(far, &m.bark, true);
    b.add_tinted(under, &m.leaves, false);
    b.add_tinted(flowers, &m.leaves, false);
    b.add_tinted(fronds, &m.leaves, false);
}

/// Table mountains in the haze, each with a ribbon of waterfall, and clouds.
fn spawn_far(b: &mut Build, rng: &mut Lcg) {
    let (lo, hi) = bounds(b.track);
    let center = (lo + hi) * 0.5;
    let reach = (hi - lo).max_element() * 0.5;
    // Each mesa is one mesh, painted on unlit and kept out of the raytraced scene. Lit by rays
    // they shimmered: rays that start over a kilometre off are too imprecise to tell a cap from
    // the cliff under it, or a waterfall from the rock it runs down. Blended, so the falls can
    // lie in the plane of the cliff and still draw over it. Fog would wipe them out at this
    // range, so the haze is in their colours.
    let haze = b.mats.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Blend, ..default() });
    let mist = Vec3::new(0.8, 0.92, 0.9);
    let sun = Vec3::new(-0.45, 0.72, 0.52).normalize();
    const SEGS: usize = 18;
    for k in 0..7 {
        let a = k as f32 / 7.0 * TAU + rng.range(-0.25, 0.25);
        let p = center + Vec2::from_angle(a) * (reach + rng.range(1100.0, 1500.0));
        let (r, h) = (rng.range(150.0, 280.0), rng.range(260.0, 420.0));
        // Scree, cliff, then the forest on top doming over the rim.
        let rings = [(2.2, 0.0), (1.25, 0.3), (1.05, 0.42), (0.9, 1.0), (0.94, 1.01), (0.8, 1.06), (0.45, 1.1), (0.0, 1.115)];
        let mut bumps = Lcg(k as u32 * 977 + 5);
        let lump: Vec<f32> = (0..SEGS).map(|_| bumps.range(0.9, 1.1)).collect();
        // The fall runs down the middle of the face nearest the track.
        let front = (((center - p).to_angle() / TAU).rem_euclid(1.0) * SEGS as f32) as usize % SEGS;
        let at = |ring: usize, j: f32| {
            let (j0, f) = (j.floor() as usize, j.fract());
            let corner = |j: usize| Vec2::from_angle(j as f32 / SEGS as f32 * TAU) * rings[ring].0 * r * lump[j % SEGS];
            v3(corner(j0).lerp(corner(j0 + 1), f), rings[ring].1 * h)
        };
        let mut mesa = MeshBuilder::default();
        let face = |mesa: &mut MeshBuilder, ring: usize, j0: f32, j1: f32, col: Vec3| {
            let q = [at(ring, j1), at(ring, j0), at(ring + 1, j0), at(ring + 1, j1)];
            let n = (q[1] - q[0]).cross(q[3] - q[0]).normalize_or_zero();
            let c = col * (0.72 + 0.28 * n.dot(sun).max(0.0));
            let c = c.lerp(mist, 0.3);
            mesa.quad_auto(q, Color::srgb(c.x, c.y, c.z), [Vec2::ZERO; 4]);
        };
        for ring in 0..rings.len() - 1 {
            for j in 0..SEGS {
                let col = match ring {
                    0 => Vec3::new(0.4, 0.56, 0.5),
                    1 | 2 => Vec3::new(0.62, 0.72, 0.78) * (0.94 + 0.12 * lump[(j * 7 + ring) % SEGS]),
                    _ => Vec3::new(0.3, 0.5, 0.36),
                };
                face(&mut mesa, ring, j as f32, j as f32 + 1.0, col);
            }
        }
        for ring in 1..3 {
            face(&mut mesa, ring, front as f32 + 0.4, front as f32 + 0.6, Vec3::new(0.95, 1.0, 1.0) / 0.72);
        }
        b.commands.spawn((Mesh3d(b.meshes.add(mesa.build())), MeshMaterial3d(haze.clone()), Transform::from_translation(v3(p, 0.0)), NotShadowCaster, super::gfx::NoRaytrace, Scenery));
    }
    // Forward-rendered (blend) so they're shaded the same with and without raytracing.
    let cloud = b.mats.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(0.9, 0.92, 0.95), perceptual_roughness: 1.0, alpha_mode: AlphaMode::Blend, ..default() });
    let puff = b.meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    for _ in 0..24 {
        let p = center + Vec2::new(rng.range(-1100.0, 1100.0), rng.range(-1100.0, 1100.0));
        b.commands.spawn((Transform::from_translation(v3(p, rng.range(300.0, 380.0))), Visibility::default(), Scenery)).with_children(|cl| {
            for _ in 0..6 {
                let o = Vec3::new(rng.range(-24.0, 24.0), rng.range(-3.0, 6.0), rng.range(-12.0, 12.0));
                let s = rng.range(11.0, 19.0);
                cl.spawn((Mesh3d(puff.clone()), MeshMaterial3d(cloud.clone()), Transform::from_translation(o).with_scale(Vec3::new(s * 1.4, s * 0.75, s)), NotShadowCaster, super::gfx::NoRaytrace));
            }
        });
    }
    // Scarlet macaws wheeling over the canopy.
    let (red, blue, yellow) = (b.kit.kart_colors[0].clone(), b.kit.kart_colors[1].clone(), b.kit.yellow.clone());
    for k in 0..7 {
        let flight = super::scenery::Drifter { center: center + Vec2::new(rng.range(-150.0, 150.0), rng.range(-150.0, 150.0)), radius: rng.range(40.0, 110.0), height: rng.range(30.0, 46.0), speed: rng.range(0.12, 0.2) * if k % 2 == 0 { 1.0 } else { -1.0 }, phase: rng.f() * TAU, blimp: true };
        b.commands.spawn((Transform::default(), Visibility::default(), flight, Scenery)).with_children(|bird| {
            part(bird, &b.kit.capsule, &red, t(0.0, 0.0, 0.0).rx(FRAC_PI_2).s(0.35, 0.7, 0.35));
            part(bird, &b.kit.cube, &blue, t(0.0, 0.05, 0.1).s(2.6, 0.05, 0.6));
            part(bird, &b.kit.cube, &yellow, t(0.0, 0.06, -0.05).s(1.5, 0.05, 0.3));
            part(bird, &b.kit.cube, &red, t(0.0, 0.0, 0.9).s(0.16, 0.05, 1.1));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The built-in lap's hall is where its marks say, and the sample custom maps each either get
    /// one on a stretch that's level, dry and gentle, or go without.
    #[test]
    fn hall_fits_the_road() {
        let jungle = Track::jungle();
        let (h0, h1) = find_hall(&jungle).expect("the jungle has its hall");
        assert!((h1 - h0) as f32 * jungle.spacing > 100.0);
        for map in crate::sim::custom::tests::sample_maps() {
            let track = crate::sim::custom::build(&map, TrackId::Custom(map.hash())).track;
            if let Some((h0, h1)) = find_hall(&track) {
                println!("{}: hall over {h0}..{h1}", map.name);
                for i in h0..h1 {
                    let s = &track.samples[i];
                    assert!(s.height < 0.4 && s.curvature.abs() < 1.0 / 60.0 && !track.in_water(s.s), "{}: the hall is over sample {i}", map.name);
                }
            }
        }
    }
}
