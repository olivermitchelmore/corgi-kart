//! Procedural tiling textures (asphalt, grass, hedge, concrete), generated at startup with a
//! full mip chain so they stay calm in the distance. Colours come from vertex colours; these
//! textures only add grain, so they're mostly near-white.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

#[derive(Resource, Clone)]
pub struct Textures {
    pub asphalt: Handle<Image>,
    pub grass: Handle<Image>,
    pub hedge: Handle<Image>,
    pub concrete: Handle<Image>,
    /// Linear metallic/roughness map for wet asphalt (G = roughness, B = metallic).
    pub wet: Handle<Image>,
    /// Polished floor tiles with grout lines (one texture repeat = 2x2 tiles).
    pub tiles: Handle<Image>,
    /// Linear normal map of gentle ripples for water.
    pub ripples: Handle<Image>,
    /// Night-time office windows, some lit, for emissive skyscrapers.
    pub windows: Handle<Image>,
    /// A boost pad's glow: a bright arrow on orange, one `PAD_TILE` of it. It's scrolled
    /// along the pad, so it repeats seamlessly along the road (v).
    pub pad_arrows: Handle<Image>,
    /// A boosted ramp's glow: rainbow bands with a white arrow, one `RAMP_TILE`, scrolled up
    /// the ramp.
    pub rainbow: Handle<Image>,
    /// Old paving: big uneven flagstones with moss in the joints.
    pub flagstones: Handle<Image>,
    /// Rounded cobbles after rain, with its linear metallic/roughness map (the joints hold
    /// water) and its normal map.
    pub cobbles: Handle<Image>,
    pub cobbles_wet: Handle<Image>,
    pub cobbles_normal: Handle<Image>,
    /// Bricks in mortar.
    pub brick: Handle<Image>,
    /// Glazed tiles in a star pattern: white where the pattern's lines are.
    pub mosaic: Handle<Image>,
    /// Streaks of falling water, scrolled down a waterfall (v).
    pub falls: Handle<Image>,
}

/// Size in metres (across, along the road) of one repeat of `pad_arrows`.
pub const PAD_TILE: Vec2 = Vec2::new(4.8, 2.0);
/// Size in metres of one repeat of `rainbow`.
pub const RAMP_TILE: Vec2 = Vec2::new(7.0, 4.0);

/// For a ">" arrow pointing along +y, `x` metres off centre and `y` along a tile `len` long:
/// how far behind the arrow's front edge the point is (0 at the front, growing backwards),
/// with arms sweeping back `sweep` metres per metre out.
fn behind_arrow(x: f32, y: f32, len: f32, front: f32, sweep: f32) -> f32 {
    (front - (y + sweep * x.abs())).rem_euclid(len)
}

/// 0 outside `lo..hi`, 1 inside, blurred over `aa` at each edge.
fn band(t: f32, lo: f32, hi: f32, aa: f32) -> f32 {
    let s = |e: f32| ((t - e) / aa + 0.5).clamp(0.0, 1.0);
    s(lo) * (1.0 - s(hi))
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

const SIZE: usize = 256;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise that tiles with the given period (in lattice cells).
fn noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let h = |i: i32, j: i32| hash(i.rem_euclid(period), j.rem_euclid(period), seed);
    let a = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * sx;
    let b = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * sx;
    a + (b - a) * sy
}

/// Fractal noise over the unit square (u, v in 0..1), tiling seamlessly.
pub(super) fn fbm(u: f32, v: f32, base: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut period) = (0.0, 1.0, 0.0, base);
    for o in 0..octaves {
        sum += noise(u * period as f32, v * period as f32, period, seed + o * 17) * amp;
        norm += amp;
        amp *= 0.5;
        period *= 2;
    }
    sum / norm
}

/// Scattered cells that tile every `period`: (distance to the nearest cell's point, how far the
/// point is inside its cell, a random number for that cell). The second is 0 on the borders
/// between cells, which makes joints between paving stones.
fn cells(u: f32, v: f32, period: i32, seed: u32) -> (f32, f32, f32) {
    let (x, y) = (u * period as f32, v * period as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (f32::MAX, f32::MAX, 0.0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let (hx, hy) = (cx.rem_euclid(period), cy.rem_euclid(period));
            let px = cx as f32 + 0.15 + 0.7 * hash(hx, hy, seed);
            let py = cy as f32 + 0.15 + 0.7 * hash(hx, hy, seed + 1);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < f1 {
                (f2, f1, id) = (f1, d, hash(hx, hy, seed + 2));
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2 - f1, id)
}

fn make(images: &mut Assets<Image>, f: impl Fn(f32, f32, usize, usize) -> [f32; 3]) -> Handle<Image> {
    make_fmt(images, true, f)
}

fn make_fmt(images: &mut Assets<Image>, srgb: bool, f: impl Fn(f32, f32, usize, usize) -> [f32; 3]) -> Handle<Image> {
    let mut level: Vec<[f32; 3]> = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            level.push(f(x as f32 / SIZE as f32, y as f32 / SIZE as f32, x, y));
        }
    }
    // Box-filter the mip chain on the CPU; runtime images don't get mips otherwise.
    let to_bytes = |lv: &[[f32; 3]], out: &mut Vec<u8>| {
        for c in lv {
            for ch in c {
                let v = if srgb { ch.clamp(0.0, 1.0).powf(1.0 / 2.2) } else { ch.clamp(0.0, 1.0) };
                out.push((v * 255.0).round() as u8);
            }
            out.push(255);
        }
    };
    let mut data = Vec::new();
    let mut size = SIZE;
    let mut mips = 1;
    to_bytes(&level, &mut data);
    while size > 1 {
        let half = size / 2;
        let mut next = Vec::with_capacity(half * half);
        for y in 0..half {
            for x in 0..half {
                let mut acc = [0.0; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = level[(y * 2 + dy) * size + x * 2 + dx];
                    for k in 0..3 {
                        acc[k] += p[k] * 0.25;
                    }
                }
                next.push(acc);
            }
        }
        level = next;
        size = half;
        mips += 1;
        to_bytes(&level, &mut data);
    }

    let extent = Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 };
    let base_len = SIZE * SIZE * 4;
    let mut img = Image::new(extent, TextureDimension::D2, data[..base_len].to_vec(), if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm }, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = mips;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    images.add(img)
}

impl Textures {
    pub fn new(images: &mut Assets<Image>) -> Self {
        let asphalt = make(images, |u, v, x, y| {
            let blotch = fbm(u, v, 4, 4, 1);
            let grain = hash(x as i32, y as i32, 2);
            let fleck = if grain > 0.985 { 0.12 } else if grain < 0.02 { -0.14 } else { (grain - 0.5) * 0.08 };
            let l = 0.86 + (blotch - 0.5) * 0.16 + fleck;
            [l, l, l * 1.01]
        });
        let grass = make(images, |u, v, x, y| {
            let patch = fbm(u, v, 3, 3, 11);
            let blades = fbm(u * 1.0, v * 1.0, 32, 2, 12);
            let spec = hash(x as i32, y as i32, 13);
            let l = 0.8 + (patch - 0.5) * 0.25 + (blades - 0.5) * 0.3 + (spec - 0.5) * 0.08;
            let warm = (patch - 0.5) * 0.12;
            [l * (1.0 + warm), l, l * (0.95 - warm)]
        });
        let hedge = make(images, |u, v, _, _| {
            let leaves = fbm(u, v, 16, 3, 21);
            let clumps = fbm(u, v, 4, 2, 22);
            let l = 0.62 + leaves * 0.45 + (clumps - 0.5) * 0.2;
            let hi = if leaves > 0.66 { 0.12 } else { 0.0 };
            [l + hi * 0.4, l + hi, l]
        });
        let concrete = make(images, |u, v, x, y| {
            let stain = fbm(u, v, 2, 4, 31);
            let grain = hash(x as i32, y as i32, 32);
            let l = 0.88 + (stain - 0.5) * 0.14 + (grain - 0.5) * 0.06;
            [l, l, l]
        });
        // Mostly a thin wet film, with drier rougher patches and a few near-mirror streaks.
        let wet = make_fmt(images, false, |u, v, _, _| {
            let dry = fbm(u, v, 3, 4, 41);
            let streak = fbm(u * 1.0, v, 8, 2, 42);
            let r = 0.2 + (dry - 0.45).max(0.0) * 1.1 - (streak - 0.6).max(0.0) * 0.6;
            [1.0, r.clamp(0.06, 0.7), 0.0]
        });
        let tiles = make(images, |u, v, x, y| {
            let (fu, fv) = ((u * 2.0).fract(), (v * 2.0).fract());
            let grout = fu < 0.025 || fu > 0.975 || fv < 0.025 || fv > 0.975;
            if grout {
                return [0.25, 0.25, 0.27];
            }
            let marble = fbm(u, v, 6, 4, 51);
            let vein = (1.0 - ((marble * 14.0).sin().abs())).powf(12.0) * 0.18;
            let fleck = (hash(x as i32, y as i32, 52) - 0.5) * 0.03;
            let l = 0.9 + (marble - 0.5) * 0.08 - vein + fleck;
            [l, l, l]
        });
        let ripples = {
            let h = |u: f32, v: f32| fbm(u, v, 6, 3, 61) + fbm(u, v, 14, 2, 62) * 0.35;
            make_fmt(images, false, move |u, v, _, _| {
                let e = 1.0 / SIZE as f32;
                let dx = (h(u + e, v) - h(u - e, v)) * 6.0;
                let dy = (h(u, v + e) - h(u, v - e)) * 6.0;
                let n = Vec3::new(-dx, -dy, 1.0).normalize();
                [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]
            })
        };
        let windows = make(images, |u, v, _, _| {
            let (cx, cy) = ((u * 16.0) as i32, (v * 32.0) as i32);
            let (fu, fv) = ((u * 16.0).fract(), (v * 32.0).fract());
            let frame = !(0.18..0.82).contains(&fu) || !(0.25..0.8).contains(&fv);
            if frame {
                return [0.0, 0.0, 0.0];
            }
            let r = hash(cx, cy, 71);
            if r < 0.45 {
                [0.0, 0.0, 0.0]
            } else if r < 0.8 {
                [1.0, 0.8, 0.5]
            } else if r < 0.92 {
                [0.6, 0.85, 1.0]
            } else {
                [1.0, 0.5, 0.9]
            }
        });
        let pad_arrows = make(images, |u, v, _, _| {
            let (x, y) = ((u - 0.5) * PAD_TILE.x, v * PAD_TILE.y);
            let aa = PAD_TILE.y / SIZE as f32 * 1.5;
            let (thick, arm) = (0.6, 1.95);
            let d = behind_arrow(x, y, PAD_TILE.y, 1.5, 0.5);
            let reach = band(x.abs(), -1.0, arm, aa);
            let lit = band(d, 0.0, thick, aa) * reach;
            // A dark edge round each arrow makes it pop against the orange.
            let edge = band(d, PAD_TILE.y - 0.1, PAD_TILE.y + 0.1, aa).max(band(d, thick, thick + 0.12, aa)) * band(x.abs(), -1.0, arm + 0.1, aa);
            let bg = [0.5, 0.12, 0.0];
            let bg = mix3(bg, [0.05, 0.012, 0.0], edge);
            // Brightest at the arrow's front edge.
            let arrow = mix3([1.0, 0.9, 0.55], [1.0, 0.55, 0.05], d / thick);
            mix3(bg, arrow, lit)
        });
        let rainbow = make(images, |u, v, _, _| {
            // Red at the back of each repeat to violet at the front, so they flow up the ramp.
            const BANDS: [[f32; 3]; 7] = [[1.0, 0.03, 0.02], [1.0, 0.3, 0.0], [1.0, 0.85, 0.0], [0.1, 0.9, 0.05], [0.0, 0.7, 1.0], [0.08, 0.15, 1.0], [0.6, 0.05, 1.0]];
            let (x, y) = ((u - 0.5) * RAMP_TILE.x, v * RAMP_TILE.y);
            let aa = RAMP_TILE.y / SIZE as f32 * 1.5;
            let f = y / RAMP_TILE.y * 7.0;
            let k = (f as usize).min(6);
            // A soft seam between bands.
            let seam = 1.0 - 0.35 * band(f.fract(), -0.06, 0.06, 0.04).max(band(f.fract(), 0.94, 1.06, 0.04));
            let col = BANDS[k].map(|c| c * seam);
            let (thick, arm) = (0.5, 2.9);
            let d = behind_arrow(x, y, RAMP_TILE.y, 2.5, 0.35);
            let reach = band(x.abs(), -1.0, arm, aa);
            let lit = band(d, 0.0, thick, aa) * reach;
            let edge = band(d, RAMP_TILE.y - 0.1, RAMP_TILE.y + 0.1, aa).max(band(d, thick, thick + 0.1, aa)) * band(x.abs(), -1.0, arm + 0.1, aa);
            let col = mix3(col, [0.02, 0.02, 0.03], edge * 0.85);
            let col = mix3(col, [1.0, 1.0, 1.0], lit);
            // A white trim down each side.
            let trim = band(x.abs(), RAMP_TILE.x * 0.5 - 0.2, 9.0, aa);
            let line = band(x.abs(), RAMP_TILE.x * 0.5 - 0.3, RAMP_TILE.x * 0.5 - 0.2, aa);
            mix3(mix3(col, [0.02, 0.02, 0.03], line), [1.0, 1.0, 1.0], trim)
        });
        let flagstones = make(images, |u, v, x, y| {
            let (_, edge, id) = cells(u, v, 7, 81);
            let worn = fbm(u, v, 5, 4, 82);
            let pits = fbm(u, v, 40, 2, 84);
            let grain = (hash(x as i32, y as i32, 83) - 0.5) * 0.07;
            let l = 0.6 + id * 0.38 + (worn - 0.5) * 0.2 + grain - (pits - 0.62).max(0.0) * 0.9;
            // Some stones are warmer, some greyer.
            let warm = (hash((id * 977.0) as i32, 3, 85) - 0.5) * 0.14;
            let stone = [l * (1.03 + warm), l, l * (0.93 - warm)];
            // Dark earth in the joints, with moss over it creeping onto the damper stones.
            let joint = (1.0 - edge / 0.05).clamp(0.0, 1.0);
            let moss = (1.0 - edge / 0.13).clamp(0.0, 1.0) * ((worn - 0.35) * 4.0).clamp(0.0, 1.0);
            mix3(mix3(stone, [0.22, 0.4, 0.1], moss * 0.85), [0.16, 0.14, 0.1], joint)
        });
        // A cobble's height: domed, and 0 in the joints.
        let cobble = |u: f32, v: f32| (cells(u, v, 9, 91).1 / 0.32).clamp(0.0, 1.0).powf(0.6);
        let cobbles = make(images, |u, v, x, y| {
            let (_, _, id) = cells(u, v, 9, 91);
            let h = cobble(u, v);
            let l = (0.5 + id * 0.3 + (hash(x as i32, y as i32, 92) - 0.5) * 0.05) * (0.3 + 0.7 * h);
            [l * 1.03, l, l * 0.96]
        });
        let cobbles_wet = make_fmt(images, false, |u, v, _, _| {
            let h = cobble(u, v);
            let film = fbm(u, v, 3, 3, 93);
            // Water lies in the joints and in patches over the stones.
            let r = 0.07 + 0.3 * h * (1.2 - film).clamp(0.2, 1.0);
            [1.0, r.clamp(0.05, 0.45), 0.0]
        });
        let cobbles_normal = make_fmt(images, false, move |u, v, _, _| {
            let e = 1.0 / SIZE as f32;
            let dx = (cobble(u + e, v) - cobble(u - e, v)) * 1.3;
            let dy = (cobble(u, v + e) - cobble(u, v - e)) * 1.3;
            let n = Vec3::new(-dx, -dy, 1.0).normalize();
            [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]
        });
        let brick = make(images, |u, v, x, y| {
            let row = (v * 8.0).floor();
            let (fu, fv) = ((u * 4.0 + row * 0.5).fract(), (v * 8.0).fract());
            if !(0.03..0.97).contains(&fu) || !(0.08..0.92).contains(&fv) {
                return [0.62, 0.6, 0.56];
            }
            let id = hash((u * 4.0 + row * 0.5).floor() as i32 % 4, row as i32, 101);
            let l = 0.72 + id * 0.26 + (hash(x as i32, y as i32, 102) - 0.5) * 0.07 + (fbm(u, v, 4, 3, 103) - 0.5) * 0.12;
            [l, l * 0.97, l * 0.93]
        });
        let mosaic = make(images, |u, v, _, _| {
            // Eight-pointed stars: two squares, one turned 45 degrees, repeated four times across.
            let (x, y) = ((u * 4.0).fract() - 0.5, (v * 4.0).fract() - 0.5);
            let square = x.abs().max(y.abs());
            let diamond = (x.abs() + y.abs()) * std::f32::consts::FRAC_1_SQRT_2;
            let star = square.min(diamond);
            let line = band(star, 0.27, 0.31, 0.01).max(band(star, 0.12, 0.15, 0.01)).max(band(square, 0.47, 0.6, 0.01));
            let l = 0.35 + 0.65 * line;
            [l, l, l]
        });
        let falls = make(images, |u, v, _, _| {
            let streak = fbm(u, v * 0.25, 24, 3, 111);
            let foam = fbm(u, v, 6, 3, 112);
            let l = 0.62 + (streak - 0.5) * 0.7 + (foam - 0.5) * 0.25;
            [l * 0.94, l, l]
        });
        Textures { asphalt, grass, hedge, concrete, wet, tiles, ripples, windows, pad_arrows, rainbow, flagstones, cobbles, cobbles_wet, cobbles_normal, brick, mosaic, falls }
    }
}
