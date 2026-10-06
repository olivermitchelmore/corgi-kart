//! Custom tracks, drawn in the editor: a closed loop of control points (with a road height at
//! each), a theme, the start line, item boxes, boost pads, ramps and fords.
//!
//! Places along the loop are stored as spline parameters (`k + t` is `t` of the way from point `k`
//! to point `k + 1`), so dragging a point carries everything on the road next to it along. When
//! the points themselves change (inserting, deleting, smoothing), features keep their share of the
//! lap instead.
//!
//! Maps are identified by a hash of their contents, so a host and its clients agree on which map
//! `TrackId::Custom(hash)` is, and any edit makes a new id (which is what tells the renderer to
//! rebuild). The registry here holds every map this process knows about: saved ones (see
//! `crate::editor::files`), ones a LAN host sent, and the one being test-driven.

use std::sync::{Arc, LazyLock, RwLock};

use bevy::math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::track::{BoostPad, Ramp, Spline, Theme, Track, TrackId, RAMP_HALF_WIDTH, RAMP_LENGTH, ROAD_HALF_WIDTH, WALL_DIST};

/// Tightest corner radius allowed: the karts can't get round anything much tighter.
pub const MIN_RADIUS: f32 = 15.0;
/// Road centres of different parts of the lap must be this far apart...
pub const MIN_SEPARATION: f32 = 2.0 * WALL_DIST + 6.0;
/// ...unless one passes over the other with at least this much height between them.
pub const BRIDGE_CLEARANCE: f32 = 7.5;
pub const MAX_HEIGHT: f32 = 16.0;
/// Steepest slope allowed (rise over run).
pub const MAX_GRADE: f32 = 0.2;
pub const MIN_LENGTH: f32 = 400.0;
pub const MAX_LENGTH: f32 = 6000.0;
pub const MIN_POINTS: usize = 4;
/// Far more than a map made in the editor has. These (and `MAX_AROUND`) are what `sane` holds a
/// map from a file or a LAN host to, so that building it can't take all the memory there is.
pub const MAX_POINTS: usize = 2000;
pub const MAX_FEATURES: usize = 500;
/// Furthest a point may be from the middle of the map.
pub const MAX_EXTENT: f32 = 50_000.0;
/// Longest way round the control points.
pub const MAX_AROUND: f32 = 4.0 * MAX_LENGTH;
pub const PAD_LENGTH: f32 = 6.0;
/// Boost pads keep this far from the road's edge.
pub const PAD_MAX_LATERAL: f32 = ROAD_HALF_WIDTH - 3.0;
/// Ramps stay on the road.
pub const RAMP_MAX_LATERAL: f32 = ROAD_HALF_WIDTH - RAMP_HALF_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapPoint {
    pub x: f32,
    pub z: f32,
    /// Road height above the ground.
    pub h: f32,
}

impl MapPoint {
    pub fn pos(self) -> Vec2 {
        Vec2::new(self.x, self.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub u: f32,
    /// Metres right of the centre line.
    pub lateral: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RampSpot {
    /// The middle of the ramp.
    pub u: f32,
    pub lateral: f32,
    pub boosted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ford {
    /// From `from` to `to`, going the way the race goes.
    pub from: f32,
    pub to: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomMap {
    pub name: String,
    pub theme: Theme,
    /// The loop, in the direction of the race.
    pub points: Vec<MapPoint>,
    /// The start line.
    pub start: f32,
    /// Rows of item boxes.
    pub items: Vec<f32>,
    pub pads: Vec<Pad>,
    pub ramps: Vec<RampSpot>,
    pub fords: Vec<Ford>,
}

/// A map built into a track, plus what the editor needs to map clicks back onto the loop.
pub struct Built {
    pub track: Track,
    /// Spline parameter of each track sample.
    pub params: Vec<f32>,
    pub spline: Spline,
}

impl CustomMap {
    pub fn is_empty(&self) -> bool {
        self.points.len() < MIN_POINTS
    }

    /// Whether the map can be built at all: every number is a number, and nothing is so many or
    /// so far apart that the track would be enormous. The editor's own maps always are; one read
    /// from a file or sent by a LAN host might not be, so check before `build`.
    pub fn sane(&self) -> bool {
        let n = self.points.len();
        let lap = |u: f32| u.is_finite();
        let counts = n <= MAX_POINTS && self.items.len() <= MAX_FEATURES && self.pads.len() <= MAX_FEATURES && self.ramps.len() <= MAX_FEATURES && self.fords.len() <= MAX_FEATURES;
        let points = self.points.iter().all(|p| p.x.abs() <= MAX_EXTENT && p.z.abs() <= MAX_EXTENT && (0.0..=MAX_HEIGHT).contains(&p.h));
        let features = lap(self.start)
            && self.items.iter().all(|&u| lap(u))
            && self.pads.iter().all(|p| lap(p.u) && p.lateral.is_finite())
            && self.ramps.iter().all(|r| lap(r.u) && r.lateral.is_finite())
            && self.fords.iter().all(|f| lap(f.from) && lap(f.to));
        // NaN fails every comparison above, and this one.
        let around: f32 = (0..n).map(|i| self.points[i].pos().distance(self.points[(i + 1) % n].pos())).sum();
        counts && points && features && (1.0..=MAX_AROUND).contains(&around)
    }

    pub fn spline(&self) -> Spline {
        let pts: Vec<Vec3> = self.points.iter().map(|p| Vec3::new(p.x, p.z, p.h)).collect();
        Spline::new(&pts, true)
    }

    pub fn hash(&self) -> u64 {
        // FNV-1a over the serialised map: the same on every machine.
        let bytes = bincode::serialize(self).unwrap_or_default();
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
    }

    /// Replace the points, keeping everything on the road at the same share of the lap.
    pub fn set_points(&mut self, points: Vec<MapPoint>) {
        if self.is_empty() || points.len() < MIN_POINTS {
            self.points = points;
            return;
        }
        let old = self.spline();
        let old_len = old.length();
        self.points = points;
        let new = self.spline();
        let k = new.length() / old_len;
        let remap = |u: f32| new.u_at(old.s_at(u) * k);
        self.start = remap(self.start);
        self.items.iter_mut().for_each(|u| *u = remap(*u));
        self.pads.iter_mut().for_each(|p| p.u = remap(p.u));
        self.ramps.iter_mut().for_each(|r| r.u = remap(r.u));
        self.fords.iter_mut().for_each(|f| (f.from, f.to) = (remap(f.from), remap(f.to)));
    }

    /// Race the other way round. Point 0 stays first, so the start line stays put.
    pub fn flip(&mut self) {
        if self.is_empty() {
            return;
        }
        let n = self.points.len();
        let old = self.spline();
        let len = old.length();
        let mut pts = vec![self.points[0]];
        pts.extend(self.points[1..].iter().rev());
        self.points = pts;
        let new = self.spline();
        let k = new.length() / len;
        let remap = |u: f32| new.u_at((len - old.s_at(u)) * k);
        self.start = remap(self.start);
        self.items.iter_mut().for_each(|u| *u = remap(*u));
        for p in &mut self.pads {
            (p.u, p.lateral) = (remap(p.u), -p.lateral);
        }
        for r in &mut self.ramps {
            (r.u, r.lateral) = (remap(r.u), -r.lateral);
        }
        for f in &mut self.fords {
            (f.from, f.to) = (remap(f.to), remap(f.from));
        }
        debug_assert_eq!(self.points.len(), n);
    }

    /// Ease every corner a little: each point moves partway towards the middle of its neighbours.
    pub fn smooth(&mut self) {
        let n = self.points.len();
        if n < MIN_POINTS {
            return;
        }
        let pts = (0..n)
            .map(|i| {
                let (a, p, b) = (self.points[(i + n - 1) % n], self.points[i], self.points[(i + 1) % n]);
                let mid = (a.pos() + b.pos()) * 0.5;
                let q = p.pos().lerp(mid, 0.4);
                MapPoint { x: q.x, z: q.y, h: p.h }
            })
            .collect();
        self.set_points(pts);
    }

    /// Insert a point on the road at parameter `u`. Returns its index.
    pub fn insert_point(&mut self, u: f32) -> usize {
        let n = self.points.len();
        let spline = self.spline();
        let p = spline.point(spline.s_at(u));
        let k = (u.rem_euclid(n as f32).floor() as usize).min(n - 1);
        let mut pts = self.points.clone();
        pts.insert(k + 1, MapPoint { x: p.x, z: p.y, h: p.z.max(0.0) });
        self.set_points(pts);
        k + 1
    }

    pub fn remove_point(&mut self, i: usize) {
        if self.points.len() > MIN_POINTS {
            let mut pts = self.points.clone();
            pts.remove(i);
            self.set_points(pts);
        }
    }
}

/// Build `map` into a track. `map` mustn't be empty, and should be `sane`.
pub fn build(map: &CustomMap, id: TrackId) -> Built {
    let spline = map.spline();
    let len = spline.length();
    let start = spline.s_at(map.start);
    let s = |u: f32| (spline.s_at(u) - start).rem_euclid(len);
    let mut items: Vec<f32> = map.items.iter().map(|&u| s(u)).collect();
    items.sort_by(f32::total_cmp);
    let pads = map
        .pads
        .iter()
        .map(|p| BoostPad { s: (s(p.u) - PAD_LENGTH * 0.5).rem_euclid(len), lateral: p.lateral, length: PAD_LENGTH, half_width: 2.5 })
        .collect();
    let (mut track, params) = Track::sample(id, map.theme, &spline, start, items, pads);
    track.ramps = map.ramps.iter().map(|r| Ramp { s: (s(r.u) - RAMP_LENGTH * 0.5).rem_euclid(len), lateral: r.lateral, boosted: r.boosted }).collect();
    track.water = map.fords.iter().map(|f| (s(f.from), (s(f.to) - s(f.from)).rem_euclid(len))).collect();
    Built { track, params, spline }
}

// ----- Checks ---------------------------------------------------------------------------------

/// What stops a map being raced, with the samples to show in red.
#[derive(Default)]
pub struct Problems {
    pub bad: Vec<bool>,
    pub messages: Vec<String>,
}

impl Problems {
    pub fn ok(&self) -> bool {
        self.messages.is_empty()
    }
}

pub fn check(track: &Track) -> Problems {
    let n = track.samples.len();
    let mut bad = vec![false; n];
    let mut messages = vec![];
    // A lap with no length at all (every point in one place) comes out as NaN.
    if track.length.is_nan() || track.length < MIN_LENGTH {
        messages.push(format!("The lap is too short: it needs to be at least {MIN_LENGTH:.0}m."));
    }
    if track.length > MAX_LENGTH {
        messages.push(format!("The lap is too long: keep it under {:.0}km.", MAX_LENGTH / 1000.0));
    }

    let mut tight = false;
    for (b, s) in bad.iter_mut().zip(&track.samples) {
        if s.curvature.abs() > 1.0 / MIN_RADIUS {
            *b = true;
            tight = true;
        }
    }
    if tight {
        messages.push("Some corners are too tight: smooth them, or spread their points out.".into());
    }

    let mut steep = false;
    for (i, b) in bad.iter_mut().enumerate() {
        let j = (i + 4) % n;
        let grade = (track.samples[j].height - track.samples[i].height).abs() / (4.0 * track.spacing);
        if grade > MAX_GRADE {
            *b = true;
            steep = true;
        }
    }
    if steep {
        messages.push(format!("Some hills are too steep: at most 1m up for every {:.0}m along.", 1.0 / MAX_GRADE));
    }

    // Parts of the lap that come too close, unless one crosses over the other.
    let (mut close, mut low) = (false, false);
    for i in (0..n).step_by(2) {
        for j in (i + 2..n).step_by(2) {
            let arc = (j - i).min(n - (j - i)) as f32 * track.spacing;
            if arc < 70.0 {
                continue;
            }
            let (a, b) = (&track.samples[i], &track.samples[j]);
            if a.pos.distance_squared(b.pos) >= MIN_SEPARATION * MIN_SEPARATION {
                continue;
            }
            let dh = (a.height - b.height).abs();
            if dh >= BRIDGE_CLEARANCE {
                continue;
            }
            bad[i] = true;
            bad[j] = true;
            if a.pos.distance(b.pos) < 2.0 * WALL_DIST {
                low = true;
            } else {
                close = true;
            }
        }
    }
    if low {
        messages.push(format!("Where the road crosses itself, raise one side at least {BRIDGE_CLEARANCE}m above the other."));
    }
    if close {
        messages.push("Some parts of the road are too close together.".into());
    }
    // Spread the marks out a little so a problem spot is easy to see.
    let marked: Vec<usize> = (0..n).filter(|&i| bad[i]).collect();
    for i in marked {
        for o in 0..3 {
            bad[(i + o) % n] = true;
            bad[(i + n - o) % n] = true;
        }
    }
    Problems { bad, messages }
}

// ----- Drawing ------------------------------------------------------------------------------

/// Turn a freehand stroke into control points: close it, smooth out the wobbles, then keep just
/// enough points to follow its shape. `None` if it's too short to be a track.
pub fn points_from_stroke(stroke: &[Vec2]) -> Option<Vec<MapPoint>> {
    const STEP: f32 = 4.0;
    // Resample the closed loop evenly.
    let mut loop_pts = stroke.to_vec();
    loop_pts.dedup_by(|a, b| a.distance(*b) < 0.01);
    if loop_pts.len() < 3 {
        return None;
    }
    loop_pts.push(loop_pts[0]);
    let mut even = vec![loop_pts[0]];
    let mut carry = 0.0;
    for w in loop_pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = a.distance(b);
        let mut d = STEP - carry;
        while d <= seg {
            even.push(a.lerp(b, d / seg));
            d += STEP;
        }
        carry = seg - (d - STEP);
    }
    even.pop();
    let n = even.len();
    if (n as f32) * STEP < MIN_LENGTH * 0.5 {
        return None;
    }
    // Smooth out the hand's wobbles (a Gaussian of about 10m).
    for _ in 0..12 {
        even = (0..n).map(|i| (even[(i + n - 1) % n] + even[i] * 2.0 + even[(i + 1) % n]) * 0.25).collect();
    }
    // Keep the points where the loop bends, and enough on the straights.
    let mut keep = vec![false; n];
    keep[0] = true;
    let far = (0..n).max_by(|&a, &b| even[a].distance(even[0]).total_cmp(&even[b].distance(even[0]))).unwrap_or(n / 2);
    keep[far] = true;
    simplify(&even, 0, far, 2.5, &mut keep);
    simplify(&even, far, n, 2.5, &mut keep);
    let mut idx: Vec<usize> = (0..n).filter(|&i| keep[i]).collect();
    // No segment longer than 60m, and none shorter than 18m.
    let mut out: Vec<usize> = vec![];
    for (k, &i) in idx.iter().enumerate() {
        let j = if k + 1 < idx.len() { idx[k + 1] } else { idx[0] + n };
        out.push(i);
        let span = (j - i) as f32 * STEP;
        let extra = (span / 60.0).ceil() as usize;
        for e in 1..extra {
            out.push((i + (j - i) * e / extra) % n);
        }
    }
    idx = vec![];
    for i in out {
        if idx.last().is_none_or(|&l: &usize| even[l].distance(even[i]) >= 18.0) {
            idx.push(i);
        }
    }
    while idx.len() > MIN_POINTS && even[*idx.last().unwrap()].distance(even[idx[0]]) < 18.0 {
        idx.pop();
    }
    if idx.len() < MIN_POINTS {
        return None;
    }
    Some(idx.into_iter().map(|i| MapPoint { x: even[i].x, z: even[i].y, h: 0.0 }).collect())
}

/// Douglas-Peucker on `pts[a..=b]` (indices wrap).
fn simplify(pts: &[Vec2], a: usize, b: usize, tol: f32, keep: &mut [bool]) {
    let n = pts.len();
    if b <= a + 1 {
        return;
    }
    let (pa, pb) = (pts[a % n], pts[b % n]);
    let (mut worst, mut at) = (0.0, a);
    for i in a + 1..b {
        let p = pts[i % n];
        let ab = pb - pa;
        let t = ((p - pa).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let d = p.distance(pa + ab * t);
        if d > worst {
            (worst, at) = (d, i);
        }
    }
    if worst > tol {
        keep[at % n] = true;
        simplify(pts, a, at, tol, keep);
        simplify(pts, at, b, tol, keep);
    }
}

/// Spread `count` rows of item boxes evenly round the lap, clear of the start.
pub fn default_items(map: &CustomMap, count: usize) -> Vec<f32> {
    let spline = map.spline();
    let (len, start) = (spline.length(), spline.s_at(map.start));
    (0..count).map(|k| spline.u_at(start + len * (0.12 + 0.76 * k as f32 / (count - 1).max(1) as f32))).collect()
}

// ----- Registry -----------------------------------------------------------------------------

struct Entry {
    id: u64,
    map: Arc<CustomMap>,
    /// The file it's saved in (without the extension), if it is.
    file: Option<String>,
    /// Whether it passes `check`.
    ok: bool,
}

static REGISTRY: LazyLock<RwLock<Vec<Entry>>> = LazyLock::new(|| RwLock::new(vec![]));

/// Make `map` available as a track. `file` records where it's saved, which lists it in the menu.
pub fn register(map: CustomMap, file: Option<String>) -> TrackId {
    let id = map.hash();
    let known = REGISTRY.read().unwrap().iter().any(|e| e.id == id);
    if known && file.is_none() {
        return TrackId::Custom(id);
    }
    let ok = known || (!map.is_empty() && map.sane() && check(&build(&map, TrackId::Custom(id)).track).ok());
    let mut reg = REGISTRY.write().unwrap();
    if let Some(f) = &file {
        // A file holds one map: forget whatever it held before.
        for e in reg.iter_mut().filter(|e| e.file.as_ref() == Some(f)) {
            e.file = None;
        }
    }
    match reg.iter_mut().find(|e| e.id == id) {
        Some(e) => e.file = file.or(e.file.take()),
        None => reg.push(Entry { id, map: Arc::new(map), file, ok }),
    }
    TrackId::Custom(id)
}

/// The map behind a custom track, to send to LAN clients.
pub fn map_of(id: TrackId) -> Option<CustomMap> {
    match id {
        TrackId::Custom(h) => get(h).map(|m| (*m).clone()),
        _ => None,
    }
}

pub fn get(id: u64) -> Option<Arc<CustomMap>> {
    REGISTRY.read().unwrap().iter().find(|e| e.id == id).map(|e| e.map.clone())
}

pub fn file_of(id: u64) -> Option<String> {
    REGISTRY.read().unwrap().iter().find(|e| e.id == id).and_then(|e| e.file.clone())
}

pub fn by_file(file: &str) -> Option<TrackId> {
    REGISTRY.read().unwrap().iter().find(|e| e.file.as_deref() == Some(file)).map(|e| TrackId::Custom(e.id))
}

/// Whether the map can be raced.
pub fn raceable(id: u64) -> bool {
    REGISTRY.read().unwrap().iter().any(|e| e.id == id && e.ok)
}

/// Saved maps that can be raced, by file name.
pub fn saved() -> Vec<TrackId> {
    let reg = REGISTRY.read().unwrap();
    let mut v: Vec<(&str, u64)> = reg.iter().filter(|e| e.ok).filter_map(|e| Some((e.file.as_deref()?, e.id))).collect();
    v.sort();
    v.into_iter().map(|(_, id)| TrackId::Custom(id)).collect()
}

/// Every saved map (raceable or not), by file name.
pub fn saved_files() -> Vec<(String, TrackId)> {
    let reg = REGISTRY.read().unwrap();
    let mut v: Vec<(String, TrackId)> = reg.iter().filter_map(|e| Some((e.file.clone()?, TrackId::Custom(e.id)))).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A rounded rectangle, drawn anticlockwise on screen.
    fn stroke() -> Vec<Vec2> {
        let mut v = vec![];
        for k in 0..400 {
            let a = k as f32 / 400.0 * std::f32::consts::TAU;
            let (c, s) = (a.cos(), a.sin());
            // squarish: a superellipse 400 x 250
            let f = |x: f32| x.signum() * x.abs().powf(0.5);
            v.push(Vec2::new(200.0 * f(c), 125.0 * f(s)) + Vec2::new((k % 7) as f32 * 0.3, 0.0));
        }
        v
    }

    fn map() -> CustomMap {
        let mut m = CustomMap { name: "Test".into(), theme: Theme::Circuit, points: points_from_stroke(&stroke()).unwrap(), ..default() };
        m.items = default_items(&m, 3);
        m.pads = vec![Pad { u: 1.5, lateral: 2.0 }];
        m.ramps = vec![RampSpot { u: 0.6, lateral: -3.0, boosted: false }, RampSpot { u: 5.5, lateral: 3.0, boosted: true }];
        m.fords = vec![Ford { from: 2.2, to: 2.8 }];
        m
    }

    fn default() -> CustomMap {
        CustomMap::default()
    }

    /// Maps that pass `sane` build into something of a sensible size whatever they hold, and one
    /// that can't be raced is never built into a race.
    #[test]
    fn odd_maps_build_small_or_not_at_all() {
        let good = map();
        assert!(good.sane());
        let scaled = |k: f32| {
            let mut m = good.clone();
            m.points.iter_mut().for_each(|p| (p.x, p.z) = (p.x * k, p.z * k));
            m
        };
        assert!(!scaled(1e30).sane() && !scaled(3e8).sane() && !scaled(f32::NAN).sane() && !scaled(0.0).sane());
        let mut m = good.clone();
        m.points[0].h = f32::INFINITY;
        assert!(!m.sane());
        let mut m = good.clone();
        m.items.push(f32::NAN);
        assert!(!m.sane());

        // As big as `sane` lets through: built, flagged, and not raced.
        let big = scaled(MAX_AROUND * 0.9 / 1200.0);
        assert!(big.sane());
        let b = build(&big, TrackId::Custom(5));
        assert!(b.track.samples.len() < 60_000 && !check(&b.track).ok());
        // Every point in one place, as the editor could be made to do: one sample, not none.
        let b = build(&scaled(0.0), TrackId::Custom(6));
        assert_eq!(b.track.samples.len(), 1);
        assert!(!check(&b.track).ok());
        let _ = b.track.point_at(3.0);

        let id = register(big.clone(), None);
        assert!(!raceable(big.hash()));
        assert_eq!(Track::new(id).id, TrackId::Circuit);
    }

    #[test]
    fn stroke_makes_a_raceable_track() {
        let m = map();
        assert!(m.points.len() >= 8, "{} points", m.points.len());
        let b = build(&m, TrackId::Custom(1));
        let p = check(&b.track);
        println!("{} points, {:.0}m, problems {:?}", m.points.len(), b.track.length, p.messages);
        assert!(p.ok(), "{:?}", p.messages);
        assert_eq!(b.track.item_rows.len(), 3);
        assert_eq!(b.track.water.len(), 1);
        assert!(b.track.water[0].1 > 5.0);
    }

    #[test]
    fn features_survive_edits() {
        let mut m = map();
        let b = build(&m, TrackId::Custom(1));
        let pad_s = b.track.boost_pads[0].s;
        m.insert_point(3.5);
        let b2 = build(&m, TrackId::Custom(2));
        assert!((b2.track.boost_pads[0].s - pad_s).abs() < 3.0, "{} vs {pad_s}", b2.track.boost_pads[0].s);
        // Flipping twice gets back where we started.
        let before = build(&m, TrackId::Custom(3)).track;
        m.flip();
        m.flip();
        let after = build(&m, TrackId::Custom(4)).track;
        assert!((before.length - after.length).abs() < 0.5);
        assert!(before.samples[0].pos.distance(after.samples[0].pos) < 0.5);
        assert!((before.boost_pads[0].s - after.boost_pads[0].s).abs() < 1.0);
        assert!((before.ramps[1].s - after.ramps[1].s).abs() < 1.0);
        assert_eq!(before.ramps[1].lateral, after.ramps[1].lateral);
    }

    /// A figure of eight, starting where it crosses itself, and with a bridge if `raised`.
    fn eight(raised: bool) -> CustomMap {
        let pts: Vec<Vec2> = (0..600)
            .map(|k| {
                let a = k as f32 / 600.0 * std::f32::consts::TAU;
                Vec2::new(300.0 * a.sin(), 150.0 * (2.0 * a).sin())
            })
            .collect();
        let mut m = CustomMap { name: "Eight".into(), points: points_from_stroke(&pts).unwrap(), ..default() };
        if raised {
            // Raise the points near the second pass over the middle.
            let spline = m.spline();
            let len = spline.length();
            for (k, p) in m.points.iter_mut().enumerate() {
                let s = spline.s_at(k as f32);
                let d = (s - len * 0.5).abs().min(len - (s - len * 0.5).abs());
                p.h = (10.0 - (d - 30.0).max(0.0) * MAX_GRADE * 0.8).max(0.0);
            }
        }
        m.items = default_items(&m, 4);
        m
    }

    /// Custom tracks for the race tests.
    pub(crate) fn sample_maps() -> Vec<CustomMap> {
        vec![map(), eight(true)]
    }

    #[test]
    fn crossing_needs_a_bridge() {
        let p = check(&build(&eight(false), TrackId::Custom(5)).track);
        assert!(!p.ok());
        let p = check(&build(&eight(true), TrackId::Custom(6)).track);
        assert!(p.ok(), "{:?}", p.messages);
    }

    #[test]
    fn hash_is_stable() {
        let m = map();
        assert_eq!(m.hash(), m.clone().hash());
        let mut n = m.clone();
        n.points[0].x += 0.5;
        assert_ne!(m.hash(), n.hash());
    }
}
