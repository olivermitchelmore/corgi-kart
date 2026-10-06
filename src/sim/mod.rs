//! Pure game simulation: no rendering, no networking. The host (and solo mode) runs
//! `World::step` authoritatively; clients run `Kart::step` for their own kart to predict.

pub mod ai;
pub mod custom;
pub mod kart;
pub mod track;
pub mod world;

pub use kart::{Item, Kart, KartInput};
pub use track::{Theme, Track, TrackId};
pub use world::World;

pub const TICK_RATE: f32 = 60.0;
pub const DT: f32 = 1.0 / TICK_RATE;
pub const LAPS: u8 = 3;
pub const MAX_KARTS: usize = 8;
/// Seconds from race start until "GO!".
pub const COUNTDOWN: f32 = 4.0;

/// Engine class: 100cc is the standard speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SpeedClass {
    C50,
    #[default]
    C100,
    C150,
}

impl SpeedClass {
    pub const ALL: [SpeedClass; 3] = [SpeedClass::C50, SpeedClass::C100, SpeedClass::C150];

    /// Multiplier on top speeds and acceleration.
    pub fn mult(self) -> f32 {
        match self {
            SpeedClass::C50 => 0.8,
            SpeedClass::C100 => 1.0,
            SpeedClass::C150 => 1.2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SpeedClass::C50 => "50cc",
            SpeedClass::C100 => "100cc",
            SpeedClass::C150 => "150cc",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == k || c.name().trim_end_matches("cc") == k)
    }

    pub fn cycle(self, step: i32) -> Self {
        let i = Self::ALL.iter().position(|&c| c == self).unwrap() as i32;
        Self::ALL[(i + step).rem_euclid(3) as usize]
    }
}

pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}
