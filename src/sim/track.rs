//! The tracks. Each is a closed Catmull-Rom spline resampled to ~1m steps. Corgi Meadow's goes
//! through a few control points; the others are laid out as straights and corners of a given
//! radius (see `Piece`), which makes it easy to say exactly how tight each corner is.
//!
//! Coordinates are `Vec2(x, z)` on the ground plane. For a direction `dir`, `dir.perp()` is the
//! driver's right-hand side, and positive lateral offsets are to the right.
//!
//! Tracks also have a height profile. The physics stays 2D, so height only matters for
//! rendering and for deciding whether two things at the same (x, z) are actually touching:
//! Corgi Circuit is a figure of eight whose back straight flies over the lower section.

use bevy::math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::custom;

pub const ROAD_HALF_WIDTH: f32 = 10.0;
pub const WALL_DIST: f32 = 17.0;
pub const CHECKPOINTS: usize = 12;
/// Things further apart vertically than this don't collide (one is on the bridge).
pub const LEVEL_GAP: f32 = 4.0;
/// The most samples a track is cut into: ten times the longest lap that can be raced.
const MAX_SAMPLES: usize = 60_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackId {
    Meadow,
    #[default]
    Circuit,
    Prism,
    Lagoon,
    Jungle,
    Tbilisi,
    /// A track made in the editor, by the hash of its `CustomMap` (see `custom.rs`).
    Custom(u64),
}

impl TrackId {
    /// The built-in tracks.
    pub const ALL: [TrackId; 6] = [TrackId::Circuit, TrackId::Prism, TrackId::Lagoon, TrackId::Jungle, TrackId::Tbilisi, TrackId::Meadow];

    pub fn name(self) -> String {
        match self {
            TrackId::Meadow => "Corgi Meadow".into(),
            TrackId::Circuit => "Corgi Circuit".into(),
            TrackId::Prism => "Prism Park".into(),
            TrackId::Lagoon => "Mirror Lagoon".into(),
            TrackId::Jungle => "Jade Jungle".into(),
            TrackId::Tbilisi => "Tbilisi Twilight".into(),
            TrackId::Custom(h) => custom::get(h).map_or("Custom track".into(), |m| m.name.clone()),
        }
    }

    /// How the track is named in settings and on the command line.
    pub fn key(self) -> String {
        match self {
            TrackId::Meadow => "meadow".into(),
            TrackId::Circuit => "circuit".into(),
            TrackId::Prism => "prism".into(),
            TrackId::Lagoon => "lagoon".into(),
            TrackId::Jungle => "jungle".into(),
            TrackId::Tbilisi => "tbilisi".into(),
            TrackId::Custom(h) => format!("custom:{}", custom::file_of(h).unwrap_or_default()),
        }
    }

    /// A built-in track, or a saved custom one as `custom:<file name>`.
    pub fn from_key(k: &str) -> Option<Self> {
        match k.strip_prefix("custom:") {
            Some(file) => custom::by_file(file),
            None => Self::ALL.into_iter().find(|t| t.key() == k),
        }
    }

    /// The tracks you can pick: the built-in ones, then the saved custom tracks that are raceable.
    pub fn choices() -> Vec<TrackId> {
        Self::ALL.into_iter().chain(custom::saved()).collect()
    }

    pub fn cycle(self, step: i32) -> Self {
        let all = Self::choices();
        let n = all.len() as i32;
        let i = all.iter().position(|&t| t == self).unwrap_or(0) as i32;
        all[(i + step).rem_euclid(n) as usize]
    }

    pub fn theme(self) -> Theme {
        match self {
            TrackId::Meadow => Theme::Meadow,
            TrackId::Circuit => Theme::Circuit,
            TrackId::Prism => Theme::Prism,
            TrackId::Lagoon => Theme::Lagoon,
            TrackId::Jungle => Theme::Jungle,
            TrackId::Tbilisi => Theme::Tbilisi,
            TrackId::Custom(h) => custom::get(h).map_or(Theme::Meadow, |m| m.theme),
        }
    }
}

/// How a track looks: its road, walls, scenery, sky and lighting. Custom tracks pick one of
/// `Theme::CUSTOM`; Mirror Lagoon's is too tied to its layout to reuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Meadow,
    Circuit,
    Prism,
    Lagoon,
    Jungle,
    Tbilisi,
}

impl Theme {
    pub const CUSTOM: [Theme; 5] = [Theme::Circuit, Theme::Meadow, Theme::Prism, Theme::Jungle, Theme::Tbilisi];

    pub fn name(self) -> &'static str {
        match self {
            Theme::Meadow => "Meadow",
            Theme::Circuit => "Circuit",
            Theme::Prism => "Prism",
            Theme::Lagoon => "Lagoon",
            Theme::Jungle => "Jungle",
            Theme::Tbilisi => "Tbilisi",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Theme::Meadow => "meadow",
            Theme::Circuit => "circuit",
            Theme::Prism => "prism",
            Theme::Lagoon => "lagoon",
            Theme::Jungle => "jungle",
            Theme::Tbilisi => "tbilisi",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::CUSTOM.into_iter().find(|t| t.key() == k)
    }

    pub fn cycle(self, step: i32) -> Self {
        let i = Self::CUSTOM.iter().position(|&t| t == self).unwrap_or(0) as i32;
        Self::CUSTOM[(i + step).rem_euclid(Self::CUSTOM.len() as i32) as usize]
    }
}

/// Corgi Meadow: a relaxed countryside loop. (x, z)
const MEADOW: &[(f32, f32)] = &[
    (0.0, 0.0),
    (70.0, 0.0),
    (140.0, 0.0),
    (200.0, 20.0),
    (215.0, 80.0),
    (185.0, 130.0),
    (130.0, 135.0),
    (95.0, 105.0),
    (50.0, 95.0),
    (20.0, 130.0),
    (-10.0, 185.0),
    (-70.0, 200.0),
    (-130.0, 180.0),
    (-165.0, 125.0),
    (-170.0, 60.0),
    (-130.0, 12.0),
    (-65.0, 0.0),
];

/// A piece of track, for laying tracks out like a turtle: starting at the origin heading along +x,
/// each piece carries on from the end of the last one. Every piece ends at road height `h`.
#[derive(Clone, Copy, Debug)]
enum Piece {
    Straight { len: f32, h: f32 },
    /// A straight whose length is worked out so the lap closes. Each track has exactly two, and
    /// they mustn't be parallel.
    Fit { h: f32 },
    /// A corner of radius `r` turning `deg` degrees, positive to the right.
    Corner { r: f32, deg: f32, h: f32 },
    /// Names this point on the lap, for placing scenery.
    Mark(&'static str),
}

const fn straight(len: f32, h: f32) -> Piece {
    Piece::Straight { len, h }
}
const fn fit(h: f32) -> Piece {
    Piece::Fit { h }
}
const fn corner(r: f32, deg: f32, h: f32) -> Piece {
    Piece::Corner { r, deg, h }
}
const fn mark(name: &'static str) -> Piece {
    Piece::Mark(name)
}

/// Corgi Circuit: a Grand Prix track in the style of Suzuka, a figure of eight. The main straight,
/// T1, the uphill esses, the two Degners, a flick and a kink either side of the bridge, the
/// hairpin, a long right into the double-apex Spoon, the back straight over the bridge, 130R, the
/// infield esses and the final double-right.
const CIRCUIT: &[Piece] = &[
    straight(280.0, 0.0),       // main straight
    corner(50.0, 90.0, 0.0),    // T1
    straight(20.0, 0.5),
    corner(38.0, -60.0, 1.5),   // uphill esses
    corner(35.0, 120.0, 3.0),
    corner(38.0, -60.0, 3.5),
    straight(20.0, 3.0),
    corner(45.0, 45.0, 2.0),    // Degner 1
    straight(20.0, 1.0),
    corner(30.0, 45.0, 0.0),    // Degner 2
    corner(55.0, -45.0, 0.0),   // a flick before the bridge
    corner(55.0, 45.0, 0.0),
    fit(0.0),                   // under the bridge
    corner(50.0, -40.0, 0.0),   // kink
    corner(50.0, 40.0, 0.0),
    straight(20.0, 0.0),
    mark("hairpin"),
    corner(26.0, -180.0, 0.0),  // hairpin
    straight(30.0, 0.0),
    corner(80.0, 45.0, 1.5),    // long right
    corner(40.0, -90.0, 5.5),   // Spoon, two apexes
    straight(10.0, 7.0),
    corner(50.0, -45.0, 9.5),
    fit(9.5),                   // back straight, over the bridge
    straight(20.0, 9.0),
    corner(60.0, -90.0, 4.0),   // 130R
    straight(20.0, 1.0),
    corner(35.0, -60.0, 0.0),   // infield esses
    corner(32.0, 120.0, 0.0),
    corner(35.0, -60.0, 0.0),
    straight(20.0, 0.0),
    corner(40.0, 90.0, 0.0),    // final double-right
    straight(30.0, 0.0),
    corner(50.0, 90.0, 0.0),
    straight(40.0, 0.0),
];

/// Prism Park: a rainbow-glass road through a crystal plaza at night. T1 climbs onto a glass
/// skyway with esses on top, a ramp drops into a hairpin underneath it, then the canal straight
/// with a ford right through the water, the far loop, esses along the bottom and a winding run up
/// the west side.
const PRISM: &[Piece] = &[
    straight(170.0, 0.0),       // start straight
    corner(40.0, 90.0, 1.0),    // T1, starting the climb
    straight(20.0, 3.0),
    corner(60.0, 90.0, 6.5),    // onto the skyway
    straight(25.0, 8.0),
    corner(30.0, -90.0, 8.0),   // the skyway esses
    corner(35.0, 90.0, 7.5),
    straight(45.0, 3.0),        // down the ramp
    corner(30.0, -180.0, 0.0),  // hairpin under the skyway
    straight(43.0, 0.0),        // the canal straight...
    mark("ford"),
    straight(44.0, 0.0),        // ...through the ford...
    straight(43.0, 0.0),        // ...and out
    corner(55.0, 90.0, 0.0),    // far loop
    fit(0.0),
    corner(40.0, 90.0, 0.0),
    corner(35.0, -60.0, 0.0),   // esses along the bottom
    corner(28.0, 120.0, 0.0),
    corner(35.0, -60.0, 0.0),
    straight(30.0, 0.0),
    corner(55.0, 45.0, 0.0),
    corner(55.0, -45.0, 0.0),
    corner(40.0, 90.0, 0.0),    // up the west side
    straight(50.0, 0.0),
    corner(80.0, -40.0, 0.0),
    corner(80.0, 40.0, 0.0),
    straight(60.0, 0.0),
    corner(35.0, 70.0, 0.0),
    corner(35.0, -70.0, 0.0),
    corner(50.0, 90.0, 0.0),    // last corner
    fit(0.0),
];

/// Mirror Lagoon: a golden-hour Riviera lap round a lagoon. The harbour promenade, a tight T1
/// into the mirror-lined Pavilion tunnel, an arched causeway that swings out into the lagoon for a
/// hairpin, the lighthouse hairpin on the point, then the gilded colonnade and a chicane through
/// the old harbour village.
const LAGOON: &[Piece] = &[
    straight(240.0, 0.0),       // promenade: start straight
    mark("headland"),
    corner(32.0, 105.0, 0.0),   // T1
    straight(20.0, 0.0),
    mark("tunnel"),
    corner(75.0, -60.0, 0.0),   // Pavilion: long left, long right
    straight(10.0, 0.0),
    corner(65.0, 85.0, 0.0),
    mark("tunnel_end"),
    straight(15.0, 0.0),
    corner(42.0, -40.0, 0.0),
    fit(0.0),
    mark("causeway"),
    corner(38.0, 90.0, 1.0),    // onto the causeway
    straight(30.0, 2.0),
    corner(55.0, 90.0, 3.0),    // a long right out into the lagoon
    straight(30.0, 3.5),
    corner(28.0, -180.0, 3.5),  // the lagoon hairpin
    straight(30.0, 3.0),
    corner(40.0, 90.0, 1.5),
    straight(30.0, 0.5),
    corner(45.0, -60.0, 0.0),
    corner(45.0, 60.0, 0.0),
    mark("point"),
    corner(50.0, -50.0, 0.0),   // out onto the point
    corner(26.0, 190.0, 0.0),   // lighthouse hairpin
    corner(50.0, -50.0, 0.0),
    mark("colonnade"),
    straight(70.0, 0.0),
    corner(80.0, -50.0, 0.0),   // long left through the colonnade
    straight(20.0, 0.0),
    mark("village"),
    corner(30.0, 80.0, 0.0),    // village chicane
    corner(28.0, -50.0, 0.0),
    corner(35.0, 20.0, 0.0),
    straight(20.0, 0.0),
    mark("promenade"),
    corner(50.0, 90.0, 0.0),    // last corner onto the promenade
    fit(0.0),
];

/// Jade Jungle: a figure of eight through a lost temple city. T1 and a run of esses drop into the
/// temple hall, the road ducks under the viaduct, winds through the deep jungle to the river ford
/// below the waterfall, then climbs onto the viaduct and comes back down past the jade idol.
const JUNGLE: &[Piece] = &[
    straight(215.0, 0.0),       // the clearing: start straight
    corner(36.0, 90.0, 0.0),    // T1
    corner(40.0, -40.0, 0.0),   // esses down to the temple
    corner(40.0, 80.0, 0.0),
    corner(40.0, -40.0, 0.0),
    corner(32.0, 90.0, 0.0),
    straight(15.0, 0.0),
    mark("temple"),
    corner(110.0, -22.0, 0.0),  // the temple hall: a long left, a long right
    straight(40.0, 0.0),
    corner(110.0, 22.0, 0.0),
    mark("temple_end"),
    straight(20.0, 0.0),
    corner(45.0, -45.0, 0.0),
    corner(45.0, 45.0, 0.0),
    fit(0.0),                   // under the viaduct
    corner(50.0, -35.0, 0.0),
    corner(50.0, 35.0, 0.0),
    corner(55.0, 45.0, 0.0),    // into the deep jungle
    corner(32.0, -135.0, 0.0),
    corner(40.0, -45.0, 0.0),
    corner(40.0, 90.0, 0.0),
    corner(40.0, -45.0, 0.0),
    corner(30.0, -90.0, 0.0),
    straight(20.0, 0.0),
    mark("ford"),
    straight(44.0, 0.0),        // the river
    straight(30.0, 0.0),
    corner(40.0, -50.0, 2.0),   // the climb
    corner(40.0, 50.0, 4.5),
    corner(40.0, -90.0, 9.5),
    mark("viaduct"),
    straight(15.0, 9.5),
    fit(9.5),                   // over the road below
    straight(15.0, 9.5),
    mark("viaduct_end"),
    corner(50.0, -40.0, 7.5),   // back down
    corner(40.0, 80.0, 3.5),
    corner(50.0, -40.0, 0.8),
    straight(15.0, 0.0),
    corner(45.0, 90.0, 0.0),    // last corner
    straight(60.0, 0.0),
];

/// Tbilisi Twilight: the river embankment, T1 onto the Bridge of Peace, a loop through the park
/// on the far bank, back over the Metekhi bridge, then the old town: balconied streets, the
/// sulphur baths, a climb to the hairpin under Narikala fortress and a last hairpin onto the
/// embankment. The river runs under the middles of the two bridges.
const TBILISI: &[Piece] = &[
    straight(205.0, 0.0),       // the embankment: start straight
    corner(34.0, 90.0, 0.3),    // T1
    straight(12.0, 1.2),
    mark("bridge"),
    straight(38.0, 3.0),        // the Bridge of Peace, arched
    straight(38.0, 1.2),
    mark("bridge_end"),
    straight(12.0, 0.2),
    corner(32.0, -60.0, 0.0),   // the park loop
    corner(30.0, 150.0, 0.0),
    corner(40.0, 50.0, 0.0),
    corner(30.0, -100.0, 0.0),
    corner(40.0, 50.0, 0.0),
    fit(0.0),
    corner(50.0, -30.0, 0.0),
    corner(45.0, 60.0, 0.0),
    corner(50.0, -30.0, 0.0),
    straight(20.0, 0.0),
    corner(36.0, -90.0, 0.0),
    corner(27.0, 180.0, 0.0),   // hairpin back towards the river
    corner(40.0, -40.0, 0.0),
    corner(40.0, 80.0, 0.0),
    corner(40.0, -40.0, 0.3),
    straight(10.0, 1.0),
    mark("metekhi"),
    straight(36.0, 2.5),        // the Metekhi bridge
    straight(36.0, 1.0),
    mark("metekhi_end"),
    fit(0.0),
    corner(40.0, 45.0, 0.0),
    corner(40.0, -45.0, 0.0),
    mark("old_town"),
    corner(32.0, 90.0, 0.0),
    corner(35.0, -50.0, 0.0),
    corner(30.0, 100.0, 0.0),
    corner(35.0, -50.0, 0.5),
    mark("baths"),
    straight(20.0, 1.0),
    corner(45.0, -90.0, 5.0),   // the climb to Narikala
    straight(15.0, 6.5),
    mark("fortress"),
    corner(26.0, 180.0, 8.5),   // the fortress hairpin
    corner(45.0, -35.0, 6.5),
    corner(45.0, 35.0, 4.0),
    straight(10.0, 3.0),
    corner(40.0, 90.0, 1.0),
    mark("descent"),
    straight(15.0, 0.0),
    corner(60.0, -25.0, 0.0),
    corner(60.0, 25.0, 0.0),
    straight(15.0, 0.0),
    mark("embankment"),
    corner(28.0, -180.0, 0.0),  // last hairpin onto the embankment
    straight(60.0, 0.0),
];

#[derive(Clone, Copy, Debug)]
pub struct TrackSample {
    pub pos: Vec2,
    pub dir: Vec2,
    pub s: f32,
    pub height: f32,
    /// Signed turn in radians per metre; positive turns right.
    #[allow(dead_code)]
    pub curvature: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BoostPad {
    pub s: f32,
    pub lateral: f32,
    pub length: f32,
    pub half_width: f32,
}

/// A kicker ramp across part of the road. It curves up like a skate ramp, steepest at its lip,
/// and launches karts into the air. A boosted ramp has a boost pad on its face.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    /// Where its foot is (arc length); the lip is `RAMP_LENGTH` further on.
    pub s: f32,
    pub lateral: f32,
    pub boosted: bool,
}

pub const RAMP_LENGTH: f32 = 8.0;
pub const RAMP_HEIGHT: f32 = 1.4;
pub const RAMP_HALF_WIDTH: f32 = 3.5;

impl Ramp {
    /// Height of the ramp's face `x` metres past its foot.
    pub fn height(x: f32) -> f32 {
        let t = (x / RAMP_LENGTH).clamp(0.0, 1.0);
        RAMP_HEIGHT * t * t
    }

    /// Rise per metre at the lip.
    pub const LIP_SLOPE: f32 = 2.0 * RAMP_HEIGHT / RAMP_LENGTH;
}

#[derive(Clone, Copy, Debug)]
pub struct TrackPos {
    pub idx: usize,
    pub s: f32,
    pub lateral: f32,
    pub dir: Vec2,
}

pub struct Track {
    pub id: TrackId,
    pub theme: Theme,
    pub samples: Vec<TrackSample>,
    pub spacing: f32,
    pub length: f32,
    pub item_rows: Vec<f32>,
    pub boost_pads: Vec<BoostPad>,
    pub ramps: Vec<Ramp>,
    /// Fords: stretches of road under shallow water, as (start s, length).
    pub water: Vec<(f32, f32)>,
    /// Named points on the lap (arc length), from the track's `mark` pieces.
    marks: Vec<(&'static str, f32)>,
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Centripetal Catmull-Rom: unlike the uniform kind it never overshoots into loops or cusps when
/// the points are unevenly spaced, which hand-placed points always are.
fn centripetal(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let knot = |a: Vec3, b: Vec3| a.distance(b).sqrt().max(1e-3);
    let (t0, t1) = (0.0, knot(p0, p1));
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * t;
    let mix = |a: Vec3, b: Vec3, ta: f32, tb: f32| a * ((tb - t) / (tb - ta)) + b * ((t - ta) / (tb - ta));
    let (a1, a2, a3) = (mix(p0, p1, t0, t1), mix(p1, p2, t1, t2), mix(p2, p3, t2, t3));
    let (b1, b2) = (mix(a1, a2, t0, t2), mix(a2, a3, t1, t3));
    mix(b1, b2, t1, t2)
}

/// A closed spline through points (x, z, height), finely sampled. Positions along it are given
/// either as arc length from the first point or as a parameter `u`: `k + t` is `t` of the way
/// from point `k` to point `k + 1`.
pub struct Spline {
    /// Points about every 0.5m, `sub` per control point, as (x, z, height).
    dense: Vec<Vec3>,
    /// Arc length to each dense point (and back round to the first).
    cum: Vec<f32>,
    sub: usize,
}

impl Spline {
    pub fn new(pts: &[Vec3], centripetal_knots: bool) -> Self {
        let n = pts.len();
        // Enough steps between points for about 0.5m resolution.
        let sub = (6000 / n).clamp(8, 200);
        let mut dense = Vec::with_capacity(n * sub);
        for i in 0..n {
            let (p0, p1, p2, p3) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
            for k in 0..sub {
                let t = k as f32 / sub as f32;
                dense.push(if centripetal_knots { centripetal(p0, p1, p2, p3, t) } else { catmull_rom(p0, p1, p2, p3, t) });
            }
        }
        let dn = dense.len();
        let mut cum = vec![0.0f32; dn + 1];
        for i in 0..dn {
            cum[i + 1] = cum[i] + dense[i].truncate().distance(dense[(i + 1) % dn].truncate());
        }
        Spline { dense, cum, sub }
    }

    pub fn length(&self) -> f32 {
        self.cum[self.dense.len()]
    }

    /// Arc length from the first point to parameter `u`.
    pub fn s_at(&self, u: f32) -> f32 {
        let f = (u * self.sub as f32).rem_euclid(self.dense.len() as f32);
        let j = (f.floor() as usize).min(self.dense.len() - 1);
        self.cum[j] + (self.cum[j + 1] - self.cum[j]) * (f - j as f32)
    }

    /// The parameter at arc length `s` from the first point.
    pub fn u_at(&self, s: f32) -> f32 {
        let s = s.rem_euclid(self.length());
        let j = self.segment(s);
        let t = (s - self.cum[j]) / (self.cum[j + 1] - self.cum[j]).max(1e-6);
        (j as f32 + t) / self.sub as f32
    }

    /// The dense segment that arc length `s` falls in.
    fn segment(&self, s: f32) -> usize {
        self.cum.partition_point(|&c| c < s).max(1) - 1
    }

    /// Point (x, z, height) at arc length `s` from the first point.
    pub fn point(&self, s: f32) -> Vec3 {
        let dn = self.dense.len();
        let j = self.segment(s).min(dn - 1);
        let t = (s - self.cum[j]) / (self.cum[j + 1] - self.cum[j]).max(1e-6);
        self.dense[j].lerp(self.dense[(j + 1) % dn], t)
    }
}

/// Walk `pieces` with the two fitted straights `fits` long: points every 5m or so as (x, z, h),
/// the marks, where the walk ends up and the total length.
fn walk(pieces: &[Piece], fits: [f32; 2]) -> (Vec<(f32, f32, f32)>, Vec<(&'static str, f32)>, Vec2, f32) {
    let (mut p, mut dir, mut h, mut s) = (Vec2::ZERO, Vec2::X, 0.0f32, 0.0f32);
    let mut pts = vec![];
    let mut marks = vec![];
    let mut fitted = fits.into_iter();
    for &piece in pieces {
        let (len, h1) = match piece {
            Piece::Mark(name) => {
                marks.push((name, s));
                continue;
            }
            Piece::Straight { len, h } => (len, h),
            Piece::Fit { h } => (fitted.next().expect("more than two fit pieces"), h),
            Piece::Corner { r, deg, h } => (r * deg.to_radians().abs(), h),
        };
        let steps = (len / 5.0).ceil().max(1.0) as usize;
        let (start, start_dir, h0) = (p, dir, h);
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            (p, dir) = match piece {
                Piece::Corner { r, deg, .. } => {
                    // Turn about a centre off to the right (or left) of where the corner starts.
                    let side = deg.signum();
                    let centre = start + start_dir.perp() * r * side;
                    let rot = Vec2::from_angle(deg.to_radians() * t);
                    (centre + rot.rotate(-start_dir.perp() * r * side), rot.rotate(start_dir))
                }
                _ => (start + start_dir * len * t, start_dir),
            };
            h = h0 + (h1 - h0) * t;
            pts.push((p.x, p.y, h));
        }
        s += len;
    }
    (pts, marks, p, s)
}

/// Lay out a lap: the points (starting at the start line), the marks and the length. The two
/// fitted straights take whatever lengths close the lap.
fn lay_out(pieces: &[Piece]) -> (Vec<(f32, f32, f32)>, Vec<(&'static str, f32)>, f32) {
    // Where the walk ends is linear in the fitted lengths: solve for the pair that brings it home.
    let end = |fits| walk(pieces, fits).2;
    let e0 = end([0.0, 0.0]);
    let (a, b) = (end([1.0, 0.0]) - e0, end([0.0, 1.0]) - e0);
    let det = a.perp_dot(b);
    assert!(det.abs() > 1e-3, "the fitted straights are parallel");
    let fits = [b.perp_dot(e0) / det, e0.perp_dot(a) / det];
    assert!(fits[0] > 0.0 && fits[1] > 0.0, "the lap doesn't close with positive straights: {fits:?}");
    let (mut pts, marks, home, len) = walk(pieces, fits);
    assert!(home.length() < 0.5, "the lap doesn't close: ends at {home}");
    // The walk ends back at the start line: put that point first.
    pts.rotate_right(1);
    (pts, marks, len)
}

impl Track {
    pub fn new(id: TrackId) -> Self {
        match id {
            TrackId::Meadow => Self::meadow(),
            TrackId::Circuit => Self::circuit(),
            TrackId::Prism => Self::prism(),
            TrackId::Lagoon => Self::lagoon(),
            TrackId::Jungle => Self::jungle(),
            TrackId::Tbilisi => Self::tbilisi(),
            TrackId::Custom(h) => match custom::get(h).filter(|_| custom::raceable(h)) {
                Some(map) => custom::build(&map, id).track,
                // A map that went missing, or one that fails the checks (named with `--track`, or
                // sent by a LAN host): race somewhere rather than crash.
                None => Self::circuit(),
            },
        }
    }

    pub fn lagoon() -> Self {
        Self::from_pieces(
            TrackId::Lagoon,
            LAGOON,
            &[0.08, 0.3, 0.44, 0.74, 0.95],
            &[(0.035, 3.5), (0.18, 0.0), (0.37, -3.0), (0.51, 0.0), (0.75, 0.0), (0.97, -3.5)],
        )
        .with_ramps(&[(0.1, -4.0, false), (0.762, -4.5, true)])
    }

    pub fn jungle() -> Self {
        let mut t = Self::from_pieces(
            TrackId::Jungle,
            JUNGLE,
            &[0.07, 0.3, 0.5, 0.72, 0.93],
            &[(0.155, 0.0), (0.345, 3.0), (0.45, -3.0), (0.6, 0.0), (0.8, 3.5), (0.975, 0.0)],
        );
        let ford = t.mark("ford");
        t.water = vec![(ford, 44.0)];
        // The boosted ramp jumps the river; there's a plain one on the viaduct.
        t.ramps = vec![Ramp { s: ford - 13.0, lateral: 4.5, boosted: true }, Ramp { s: t.mark("viaduct") + 30.0, lateral: -4.0, boosted: false }, Ramp { s: t.length * 0.09, lateral: -4.0, boosted: false }];
        t
    }

    pub fn tbilisi() -> Self {
        let mut t = Self::from_pieces(
            TrackId::Tbilisi,
            TBILISI,
            &[0.07, 0.27, 0.47, 0.71, 0.93],
            &[(0.2, -3.5), (0.375, 0.0), (0.6, 3.0), (0.745, -3.0), (0.985, 0.0)],
        );
        // One ramp throws you off the end of the Bridge of Peace, another down the hill.
        t.ramps = vec![Ramp { s: t.mark("bridge") + 4.0, lateral: 4.0, boosted: true }, Ramp { s: t.mark("descent") + 4.0, lateral: -4.0, boosted: false }, Ramp { s: t.length * 0.06, lateral: -4.0, boosted: false }];
        t
    }

    pub fn prism() -> Self {
        let mut t = Self::from_pieces(
            TrackId::Prism,
            PRISM,
            &[0.06, 0.37, 0.5, 0.75, 0.85],
            &[(0.14, 0.0), (0.29, -3.0), (0.43, 3.0), (0.645, 0.0), (0.85, -3.0), (0.98, 0.0)],
        );
        t.water = vec![(t.mark("ford"), 44.0)];
        // The boosted ramp jumps the ford.
        t.with_ramps(&[(0.378, -4.5, true), (0.77, 4.0, false)])
    }

    /// Is arc length `s` in a ford?
    pub fn in_water(&self, s: f32) -> bool {
        self.water.iter().any(|&(w, len)| (s - w).rem_euclid(self.length) < len)
    }

    pub fn meadow() -> Self {
        let pts: Vec<_> = MEADOW.iter().map(|&(x, z)| (x, z, 0.0)).collect();
        Self::build(TrackId::Meadow, &pts, &[0.08, 0.43, 0.72], &[(0.27, -4.0), (0.60, 3.0), (0.93, 0.0)])
            .with_ramps(&[(0.12, 4.0, false), (0.585, -3.5, true)])
    }

    pub fn circuit() -> Self {
        Self::from_pieces(
            TrackId::Circuit,
            CIRCUIT,
            &[0.085, 0.41, 0.54, 0.69, 0.925],
            &[(0.194, 3.0), (0.4, -3.0), (0.54, 0.0), (0.66, 3.5), (0.98, 0.0)],
        )
        // The boosted ramp is on the bridge.
        .with_ramps(&[(0.115, 4.0, false), (0.392, 4.0, false), (0.668, -3.5, true)])
    }

    /// Ramps at fractions of the lap, as (fraction, lateral, boosted).
    fn with_ramps(mut self, spots: &[(f32, f32, bool)]) -> Self {
        self.ramps = spots.iter().map(|&(f, lateral, boosted)| Ramp { s: self.length * f, lateral, boosted }).collect();
        self
    }

    /// Arc length of the named `mark` piece.
    pub fn mark(&self, name: &str) -> f32 {
        self.find_mark(name).unwrap_or_else(|| panic!("{:?} has no mark {name:?}", self.id))
    }

    /// Arc length of the named `mark` piece, if this track has it (custom tracks have none).
    pub fn find_mark(&self, name: &str) -> Option<f32> {
        self.marks.iter().find(|m| m.0 == name).map(|m| m.1)
    }

    /// Lay out `pieces`, then build the track through the points.
    fn from_pieces(id: TrackId, pieces: &[Piece], item_rows: &[f32], pads: &[(f32, f32)]) -> Self {
        let (pts, marks, laid) = lay_out(pieces);
        let mut t = Self::build(id, &pts, item_rows, pads);
        // The spline through the points is a hair shorter than the pieces.
        let k = t.length / laid;
        t.marks = marks.into_iter().map(|(name, s)| (name, s * k)).collect();
        t
    }

    /// `item_rows` and `pads` are given as fractions of the lap (pads also take a lateral offset).
    fn build(id: TrackId, pts: &[(f32, f32, f32)], item_rows: &[f32], pads: &[(f32, f32)]) -> Self {
        let pts: Vec<Vec3> = pts.iter().map(|&(x, z, h)| Vec3::new(x, z, h)).collect();
        let spline = Spline::new(&pts, false);
        let length = spline.length();
        let item_rows = item_rows.iter().map(|f| length * f).collect();
        let pads = pads.iter().map(|&(f, lateral)| BoostPad { s: length * f, lateral, length: 6.0, half_width: 2.5 }).collect();
        Self::sample(id, id.theme(), &spline, 0.0, item_rows, pads).0
    }

    /// Resample `spline` into ~1m samples, starting at arc length `start` (the start line).
    /// Also returns the spline parameter of each sample.
    pub(super) fn sample(id: TrackId, theme: Theme, spline: &Spline, start: f32, item_rows: Vec<f32>, boost_pads: Vec<BoostPad>) -> (Self, Vec<f32>) {
        let length = spline.length();
        // About a metre apart. Never none (everything here wraps round with `% count`), and never
        // millions, however long a map claims to be.
        let count = (length.round() as usize).clamp(1, MAX_SAMPLES);
        let spacing = length / count as f32;

        let mut positions = Vec::with_capacity(count);
        let mut heights = Vec::with_capacity(count);
        let mut params = Vec::with_capacity(count);
        for i in 0..count {
            let s = start + i as f32 * spacing;
            let s = if s >= length { s - length } else { s };
            let p = spline.point(s);
            positions.push(p.truncate());
            heights.push(p.z.max(0.0));
            params.push(spline.u_at(s));
        }

        let dirs: Vec<Vec2> = (0..count)
            .map(|i| (positions[(i + 1) % count] - positions[(i + count - 1) % count]).normalize())
            .collect();
        let samples = (0..count)
            .map(|i| {
                let a = dirs[(i + count - 1) % count];
                let b = dirs[(i + 1) % count];
                TrackSample {
                    pos: positions[i],
                    dir: dirs[i],
                    s: i as f32 * spacing,
                    height: heights[i],
                    curvature: a.perp_dot(b).atan2(a.dot(b)) / (2.0 * spacing),
                }
            })
            .collect();

        let track = Track { id, theme, samples, spacing, length, item_rows, boost_pads, ramps: vec![], water: vec![], marks: vec![] };
        (track, params)
    }

    /// Road height at sample `idx`.
    pub fn height_idx(&self, idx: usize) -> f32 {
        self.samples[idx % self.samples.len()].height
    }

    /// Road height at arc length `s` (wraps around).
    pub fn height_at(&self, s: f32) -> f32 {
        let n = self.samples.len();
        let f = self.wrap_s(s) / self.spacing;
        let i = (f.floor() as usize) % n;
        let t = f - f.floor();
        let (a, b) = (self.samples[i].height, self.samples[(i + 1) % n].height);
        a + (b - a) * t
    }

    /// The ramp at (`s`, `lateral`), if any, and how far past its foot that is.
    pub fn ramp_at(&self, s: f32, lateral: f32) -> Option<(&Ramp, f32)> {
        self.ramps.iter().find_map(|r| {
            let x = (s - r.s).rem_euclid(self.length);
            (x < RAMP_LENGTH && (lateral - r.lateral).abs() < RAMP_HALF_WIDTH).then_some((r, x))
        })
    }

    /// Whether two points on the track (given by sample index) are on the same level.
    pub fn same_level(&self, a: usize, b: usize) -> bool {
        (self.height_idx(a) - self.height_idx(b)).abs() < LEVEL_GAP
    }

    pub fn wrap_s(&self, s: f32) -> f32 {
        s.rem_euclid(self.length)
    }

    /// Centre point and direction at arc length `s` (wraps around).
    pub fn point_at(&self, s: f32) -> (Vec2, Vec2) {
        let n = self.samples.len();
        let f = self.wrap_s(s) / self.spacing;
        let i = (f.floor() as usize) % n;
        let t = f - f.floor();
        let (a, b) = (&self.samples[i], &self.samples[(i + 1) % n]);
        (a.pos.lerp(b.pos, t), a.dir.lerp(b.dir, t).normalize())
    }

    /// Signed heading change between two points along the track (positive = right).
    pub fn turn_between(&self, s0: f32, s1: f32) -> f32 {
        let (_, a) = self.point_at(s0);
        let (_, b) = self.point_at(s1);
        a.perp_dot(b).atan2(a.dot(b))
    }

    pub fn sector(&self, s: f32) -> usize {
        ((self.wrap_s(s) / self.length * CHECKPOINTS as f32) as usize).min(CHECKPOINTS - 1)
    }

    /// Project `p` onto the track, searching near `hint` first.
    pub fn locate(&self, p: Vec2, hint: usize) -> TrackPos {
        let n = self.samples.len();
        let nearest = |range: &mut dyn Iterator<Item = usize>| {
            let mut best = (0usize, f32::MAX);
            for i in range {
                let d = self.samples[i].pos.distance_squared(p);
                if d < best.1 {
                    best = (i, d);
                }
            }
            best
        };
        let hint = hint % n;
        let mut best = nearest(&mut (0..=120).map(|o| (hint + n + o - 60) % n));
        if best.1 > (WALL_DIST + 15.0).powi(2) {
            best = nearest(&mut (0..n));
        }
        let i = best.0;

        // Refine on whichever neighbouring segment the point projects onto.
        let project = |a: usize| {
            let (pa, pb) = (self.samples[a].pos, self.samples[(a + 1) % n].pos);
            let ab = pb - pa;
            ((p - pa).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        };
        let (seg, t) = {
            let t = project(i);
            if t > 0.0 { (i, t) } else { ((i + n - 1) % n, project((i + n - 1) % n)) }
        };
        let (a, b) = (&self.samples[seg], &self.samples[(seg + 1) % n]);
        let center = a.pos.lerp(b.pos, t);
        let dir = a.dir.lerp(b.dir, t).normalize();
        TrackPos {
            idx: i,
            s: self.wrap_s(a.s + t * self.spacing),
            lateral: (p - center).dot(dir.perp()),
            dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sections may only come close where one passes over the other with room to spare.
    #[test]
    fn track_does_not_overlap_itself() {
        for id in TrackId::ALL {
            let t = Track::new(id);
            let n = t.samples.len();
            let mut min_sep = f32::MAX;
            let mut crossings = 0;
            for i in 0..n {
                for j in i + 1..n {
                    let arc = (j - i).min(n - (j - i)) as f32 * t.spacing;
                    if arc > 70.0 {
                        let d = t.samples[i].pos.distance(t.samples[j].pos);
                        if d < 2.0 * WALL_DIST + 6.0 && (t.samples[i].height - t.samples[j].height).abs() > 7.5 {
                            crossings += 1;
                        } else {
                            min_sep = min_sep.min(d);
                        }
                    }
                }
            }
            let max_curv = t.samples.iter().map(|s| s.curvature.abs()).fold(0.0, f32::max);
            println!(
                "{}: length {:.0}m, min separation {:.1}m, min radius {:.1}m, bridge pairs {crossings}",
                id.name(),
                t.length,
                min_sep,
                1.0 / max_curv
            );
            assert!(min_sep > 2.0 * WALL_DIST + 6.0, "{}: track sections too close: {min_sep}", id.name());
            assert!(1.0 / max_curv > 14.0, "{}: corner too tight", id.name());
            assert_eq!(crossings > 0, matches!(id, TrackId::Circuit | TrackId::Jungle));
        }
    }

    #[test]
    fn locate_roundtrip() {
        let t = Track::circuit();
        for s in [0.0, 100.0, 377.7, 860.0, 1360.0, t.length - 1.0] {
            let (c, d) = t.point_at(s);
            let p = c + d.perp() * 5.0;
            let tp = t.locate(p, (s / t.spacing) as usize);
            assert!((tp.lateral - 5.0).abs() < 0.2, "lateral {}", tp.lateral);
            let ds = (tp.s - s).abs().min(t.length - (tp.s - s).abs());
            assert!(ds < 0.5, "s {} vs {}", tp.s, s);
        }
    }
}

