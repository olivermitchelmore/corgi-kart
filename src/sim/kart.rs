//! Arcade kart physics, including hop-drifting with charged mini-turbos, and jumps off ramps
//! with a trick for a boost if you time it at the lip.
//!
//! `Kart::step` depends only on the kart, its input, the track and the clock, so a client can
//! replay its unacknowledged inputs on top of a server snapshot and land in the same place.

use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

use super::track::{Ramp, Track, CHECKPOINTS, RAMP_HEIGHT, RAMP_LENGTH, ROAD_HALF_WIDTH, WALL_DIST};
use super::{wrap_angle, COUNTDOWN, DT, LAPS};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    #[default]
    None,
    /// Speed boost (sometimes you get three).
    Treat,
    /// Thrown straight, forwards or backwards, and bounces off walls.
    Bone,
    /// Homes in on the kart ahead of you, or flies straight back if thrown backwards.
    TennisBall,
    /// Dropped behind you as a trap, or lobbed onto the road ahead.
    SqueakyToy,
    /// As many boosts as you can press for, for a few seconds.
    GoldenTreat,
    /// A trap that looks like an item box.
    DecoyBox,
    /// Lobbed ahead (or dropped behind); bursts and soaks everyone near it.
    WaterBalloon,
    /// Flies round the track to whoever is leading and knocks them flying.
    Frisbee,
    /// Everyone else cowers: they spin out, drop their items and stay small and slow for a while.
    DogWhistle,
    /// A mad dash: faster, unstoppable, and anyone you touch spins out.
    Zoomies,
    /// The corgi tears off after a squirrel, steering itself at great speed.
    Squirrel,
    /// Grow huge: flatten the karts you drive over and shrug off most items.
    Sausage,
    /// Three barks, then one that shakes everyone ahead of you off the road, unless they're in
    /// the air for it.
    BigWoof,
    /// Splatters mud over the view of everyone ahead of you.
    MudPie,
    /// Hangs over you the moment you get it and drenches you when it bursts, unless you bump
    /// it onto someone else first. Until then you're a little faster.
    RainCloud,
}

impl Item {
    /// Every item, in the order the HUD's roulette spins through them.
    pub const ALL: [Item; 15] = [
        Item::Treat,
        Item::Bone,
        Item::TennisBall,
        Item::SqueakyToy,
        Item::GoldenTreat,
        Item::DecoyBox,
        Item::WaterBalloon,
        Item::Frisbee,
        Item::DogWhistle,
        Item::Zoomies,
        Item::Squirrel,
        Item::Sausage,
        Item::BigWoof,
        Item::MudPie,
        Item::RainCloud,
    ];

    /// An item by its name without spaces or capitals ("tennisball"); "bone3" is three of them.
    pub fn from_key(key: &str) -> Option<(Item, u8)> {
        let (name, count) = match key.strip_suffix('3') {
            Some(name) => (name, 3),
            None => (key, 1),
        };
        let squash = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
        Self::ALL.into_iter().find(|i| squash(i.name()) == squash(name)).map(|i| (i, count))
    }

    pub fn name(self) -> &'static str {
        match self {
            Item::None => "",
            Item::Treat => "Treat",
            Item::Bone => "Bone",
            Item::TennisBall => "Tennis ball",
            Item::SqueakyToy => "Squeaky toy",
            Item::GoldenTreat => "Golden treat",
            Item::DecoyBox => "Decoy box",
            Item::WaterBalloon => "Water balloon",
            Item::Frisbee => "Frisbee",
            Item::DogWhistle => "Dog whistle",
            Item::Zoomies => "Zoomies",
            Item::Squirrel => "Squirrel!",
            Item::Sausage => "Sausage",
            Item::BigWoof => "Big woof",
            Item::MudPie => "Mud pie",
            Item::RainCloud => "Rain cloud",
        }
    }
}

pub const BTN_DRIFT: u8 = 1;
pub const BTN_ITEM: u8 = 2;
pub const BTN_TRICK: u8 = 4;
/// Held while using an item to throw it forwards or backwards.
pub const BTN_AIM_FORWARD: u8 = 8;
pub const BTN_AIM_BACK: u8 = 16;

/// One tick of player intent, quantised so prediction and the host see identical values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KartInput {
    pub steer: i8,
    pub throttle: i8,
    pub buttons: u8,
}

impl KartInput {
    pub fn new(steer: f32, throttle: f32, drift: bool, item: bool, trick: bool) -> Self {
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
        let buttons = if drift { BTN_DRIFT } else { 0 } | if item { BTN_ITEM } else { 0 } | if trick { BTN_TRICK } else { 0 };
        KartInput { steer: q(steer), throttle: q(throttle), buttons }
    }
    /// Aim thrown items forwards (`aim > 0`) or backwards (`aim < 0`).
    pub fn aimed(mut self, aim: f32) -> Self {
        self.buttons &= !(BTN_AIM_FORWARD | BTN_AIM_BACK);
        if aim > 0.0 {
            self.buttons |= BTN_AIM_FORWARD;
        } else if aim < 0.0 {
            self.buttons |= BTN_AIM_BACK;
        }
        self
    }
    /// 1 aimed forwards, -1 backwards, 0 not aimed.
    pub fn aim(self) -> i8 {
        (self.buttons & BTN_AIM_FORWARD != 0) as i8 - (self.buttons & BTN_AIM_BACK != 0) as i8
    }
    pub fn steer(self) -> f32 {
        self.steer as f32 / 127.0
    }
    pub fn throttle(self) -> f32 {
        self.throttle as f32 / 127.0
    }
    pub fn drift(self) -> bool {
        self.buttons & BTN_DRIFT != 0
    }
    #[allow(dead_code)]
    pub fn item(self) -> bool {
        self.buttons & BTN_ITEM != 0
    }
}

pub const KART_RADIUS: f32 = 1.3;
pub const MAX_SPEED: f32 = 30.0;
pub const BOOST_SPEED: f32 = 40.0;
pub const OFFROAD_SPEED: f32 = 13.0;
/// Top speed while splashing through a ford.
pub const WATER_SPEED: f32 = 22.0;
pub const HOP_TIME: f32 = 0.28;
/// How long a corgi celebrates hitting someone with an item.
pub const CHEER_TIME: f32 = 1.0;
/// How long a kart spins after being hit.
pub const SPIN_TIME: f32 = 1.3;
/// How long a golden treat, the zoomies and a sausage last.
pub const GOLDEN_TIME: f32 = 7.5;
pub const ZOOM_TIME: f32 = 7.5;
pub const BIG_TIME: f32 = 7.5;
/// Top speeds with the zoomies and while chasing a squirrel.
pub const ZOOM_SPEED: f32 = 36.0;
pub const CHASE_SPEED: f32 = 48.0;
/// How much bigger a kart that ate the sausage is, and how much smaller a cowering one.
pub const BIG_SIZE: f32 = 1.8;
pub const SMALL_SIZE: f32 = 0.55;
/// How long a flattened kart stays flat.
pub const FLAT_TIME: f32 = 2.5;
/// How long mud stays on the screen.
pub const MUD_TIME: f32 = 6.0;
/// How long a rain cloud hangs over a kart before it bursts, and how long the soaking lasts.
pub const CLOUD_TIME: f32 = 10.0;
const CLOUD_SOAK: f32 = 5.0;
/// How hard a blast throws a kart into the air.
const BLAST_LAUNCH: f32 = 11.0;
/// Charge needed for blue / orange / purple sparks.
pub const DRIFT_STAGES: [f32; 3] = [1.0, 2.2, 3.6];
/// Boost seconds granted when releasing a drift at each stage.
pub const DRIFT_BOOST: [f32; 3] = [0.7, 1.25, 2.0];
const MIN_DRIFT_SPEED: f32 = 10.0;

/// Gravity in the air: stronger than real, so jumps feel snappy.
pub const GRAVITY: f32 = 26.0;
/// A boosted ramp throws you this much higher than a plain one.
const BOOSTED_LIFT: f32 = 1.15;
/// Fastest a ramp can launch you upwards.
const MAX_LAUNCH: f32 = 16.0;
/// A trick counts if it's pressed at most this long before reaching the lip...
pub const TRICK_BEFORE: f32 = 0.15;
/// ...or this long after leaving it.
pub const TRICK_AFTER: f32 = 0.2;
/// How long a trick takes to play out.
pub const TRICK_TIME: f32 = 0.5;
/// Boost for landing a trick.
pub const TRICK_BOOST: f32 = 1.0;

/// How this jump's trick went. There's one try per jump, so mashing the button doesn't work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrickTry {
    /// Not tried yet.
    #[default]
    Ready,
    /// Timed right: boost on landing.
    Done,
    /// Pressed on the ramp before the lip.
    Early,
    /// Pressed too long after leaving the lip.
    Late,
    /// No trick this time: fell off the side of a ramp, or got hit.
    Spent,
}

pub fn base_turn(speed: f32) -> f32 {
    2.3 - 0.025 * speed.abs().min(MAX_SPEED)
}

/// Drift path curvature (1/radius) with neutral steering, steering fully into the drift, and
/// steering fully out of it. Sized to this track's corners (roughly 20-60m radius).
const DRIFT_CURV_NEUTRAL: f32 = 1.0 / 45.0;
const DRIFT_CURV_IN: f32 = 1.0 / 22.0;
const DRIFT_CURV_OUT: f32 = 1.0 / 110.0;

/// Curvature of the drift for steering `x` relative to the drift (1 = into it, -1 = out of it).
pub fn drift_curvature(x: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    if x >= 0.0 {
        DRIFT_CURV_NEUTRAL + (DRIFT_CURV_IN - DRIFT_CURV_NEUTRAL) * x
    } else {
        DRIFT_CURV_NEUTRAL + (DRIFT_CURV_NEUTRAL - DRIFT_CURV_OUT) * x
    }
}

/// Inverse of `drift_curvature`: the relative steer that produces curvature `c`.
pub fn drift_steer_for(c: f32) -> f32 {
    if c >= DRIFT_CURV_NEUTRAL {
        ((c - DRIFT_CURV_NEUTRAL) / (DRIFT_CURV_IN - DRIFT_CURV_NEUTRAL)).min(1.0)
    } else {
        ((c - DRIFT_CURV_NEUTRAL) / (DRIFT_CURV_NEUTRAL - DRIFT_CURV_OUT)).max(-1.0)
    }
}

pub fn drift_stage(charge: f32) -> usize {
    DRIFT_STAGES.iter().filter(|&&t| charge >= t).count()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Kart {
    pub pos: Vec2,
    /// Direction of travel (radians, see `Vec2::from_angle`).
    pub heading: f32,
    /// Signed speed along `heading`.
    pub speed: f32,
    /// Knockback from walls and other karts; decays quickly.
    pub push: Vec2,
    /// Smoothed steering, -1..1.
    pub steer: f32,
    pub drift_dir: i8,
    pub drift_charge: f32,
    pub drift_pending: bool,
    pub hop: f32,
    /// Visual yaw offset so the kart points into a drift.
    pub slip: f32,
    pub boost: f32,
    pub spin: f32,
    pub bump: f32,
    pub item: Item,
    pub item_count: u8,
    pub roulette: f32,
    pub lap: u8,
    pub checkpoint: u8,
    pub place: u8,
    pub track_idx: u32,
    pub s: f32,
    pub lateral: f32,
    pub offroad: bool,
    pub in_water: bool,
    pub prev_buttons: u8,
    pub finished: bool,
    pub finish_time: f32,
    /// Countdown time remaining when the throttle was first held (-1 = not held).
    pub rocket_press: f32,
    pub speed_scale: f32,
    /// Engine class multiplier on top speeds and acceleration (see `SpeedClass`).
    pub cc: f32,
    pub bot: bool,
    /// Height above the road: riding up a ramp, or in the air.
    pub air: f32,
    /// Upward speed while airborne.
    pub vz: f32,
    pub airborne: bool,
    /// Seconds in the air.
    pub airtime: f32,
    /// The ramp under the kart: 0 none, 1 plain, 2 boosted.
    pub on_ramp: u8,
    pub trick_try: TrickTry,
    /// Counts down while a trick plays out.
    pub trick: f32,
    /// Which trick: a roll to the left (-1) or right (1), or a flip (0).
    pub trick_style: i8,
    /// Counts down while the corgi celebrates hitting someone with an item.
    pub cheer: f32,
    /// The held bones or balls came as three, so they circle the kart as a shield.
    pub triple: bool,
    /// Seconds left of each item's effect: golden treat, zoomies, chasing a squirrel, being
    /// huge, being small, being flat, mud on the screen, and the rain cloud overhead.
    pub golden: f32,
    pub zoom: f32,
    pub chase: f32,
    pub big: f32,
    pub small: f32,
    pub flat: f32,
    pub mud: f32,
    pub cloud: f32,
}

#[derive(Default, Clone, Copy)]
pub struct StepEvents {
    pub use_item: bool,
}

impl Kart {
    pub fn new(track: &Track, pos: Vec2, heading: f32, bot: bool, speed_scale: f32, cc: f32) -> Self {
        let mut k = Kart {
            pos,
            heading,
            checkpoint: (CHECKPOINTS - 1) as u8,
            rocket_press: -1.0,
            speed_scale,
            cc,
            bot,
            ..Default::default()
        };
        let tp = track.locate(pos, 0);
        k.track_idx = tp.idx as u32;
        k.s = tp.s;
        k.lateral = tp.lateral;
        k
    }

    pub fn dir(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }

    pub fn velocity(&self) -> Vec2 {
        self.dir() * self.speed + self.push
    }

    pub fn is_boosting(&self) -> bool {
        self.boost > 0.0
    }

    /// Continuous race progress in metres, used for ranking.
    pub fn progress(&self, track: &Track) -> f32 {
        let mut p = self.lap as f32 * track.length + self.s;
        if (self.checkpoint as usize) < CHECKPOINTS / 2 && self.s > track.length * 0.5 {
            p -= track.length;
        }
        p
    }

    /// Nothing touches a kart with the zoomies or one chasing a squirrel.
    pub fn immune(&self) -> bool {
        self.zoom > 0.0 || self.chase > 0.0
    }

    /// How big the kart is, as a multiple of its usual size.
    pub fn size(&self) -> f32 {
        if self.big > 0.0 {
            BIG_SIZE
        } else if self.small > 0.0 {
            SMALL_SIZE
        } else {
            1.0
        }
    }

    pub fn radius(&self) -> f32 {
        KART_RADIUS * self.size()
    }

    /// How many bones or balls are circling the kart.
    pub fn orbiting(&self) -> u8 {
        if self.triple && matches!(self.item, Item::Bone | Item::TennisBall) && self.roulette <= 0.0 { self.item_count } else { 0 }
    }

    pub fn drop_item(&mut self) {
        self.item = Item::None;
        self.item_count = 0;
        self.triple = false;
        self.roulette = 0.0;
        self.golden = 0.0;
    }

    fn spin_out(&mut self) {
        self.spin = SPIN_TIME;
        self.cheer = 0.0;
        self.speed *= 0.3;
        self.boost = 0.0;
        self.cancel_drift();
        self.trick = 0.0;
        if self.trick_try == TrickTry::Ready || self.trick_try == TrickTry::Done {
            self.trick_try = TrickTry::Spent;
        }
    }

    /// Hit by an item: spin out. Says whether it did anything (a huge kart just shrugs).
    pub fn hit(&mut self) -> bool {
        if self.spin > 0.0 || self.immune() || self.big > 0.0 {
            return false;
        }
        self.spin_out();
        true
    }

    /// Caught in a burst: thrown into the air, spinning. Even a huge kart is, and it's cut
    /// back down to size.
    pub fn blast(&mut self) -> bool {
        if self.spin > 0.0 || self.immune() {
            return false;
        }
        self.big = 0.0;
        self.spin_out();
        self.speed = 0.0;
        self.airborne = true;
        // Too late for a trick.
        self.airtime = 1.0;
        self.on_ramp = 0;
        self.vz = BLAST_LAUNCH;
        true
    }

    /// Cower for `secs`: spin out, drop what you're holding and stay small. A huge kart only
    /// goes back to its usual size.
    pub fn shrink(&mut self, secs: f32) -> bool {
        if self.immune() {
            return false;
        }
        if self.big > 0.0 {
            self.big = 0.0;
            return false;
        }
        self.small = secs;
        self.drop_item();
        if self.spin <= 0.0 {
            self.spin_out();
        }
        true
    }

    /// Driven over by something much bigger.
    pub fn squash(&mut self) -> bool {
        if self.flat > 0.0 || self.immune() || self.big > 0.0 {
            return false;
        }
        self.flat = FLAT_TIME;
        self.speed *= 0.2;
        self.boost = 0.0;
        self.cheer = 0.0;
        self.cancel_drift();
        true
    }

    /// Shaken off the road: spin out and drop what you're holding.
    pub fn shake(&mut self) -> bool {
        if !self.hit() {
            return false;
        }
        self.drop_item();
        true
    }

    /// How a trick pressed now would go, or `None` if the kart isn't near enough a ramp's lip for
    /// it to be a try at all.
    pub fn trick_timing(&self, track: &Track) -> Option<TrickTry> {
        if self.airborne {
            return Some(if self.airtime <= TRICK_AFTER { TrickTry::Done } else { TrickTry::Late });
        }
        let (_, x) = track.ramp_at(self.s, self.lateral)?;
        let (_, dir) = track.point_at(self.s);
        let fwd = self.velocity().dot(dir);
        if fwd <= 1.0 {
            return None;
        }
        Some(if (RAMP_LENGTH - x) / fwd <= TRICK_BEFORE { TrickTry::Done } else { TrickTry::Early })
    }

    fn land(&mut self, surface: f32) {
        self.air = surface;
        self.vz = 0.0;
        self.airborne = false;
        self.airtime = 0.0;
        self.trick = 0.0;
        if self.trick_try == TrickTry::Done {
            self.boost = self.boost.max(TRICK_BOOST);
        }
        self.trick_try = TrickTry::Ready;
    }

    /// Ride up ramps, launch off their lips, fly and land. `road0` is the road height under the
    /// kart before it moved this tick.
    fn vertical(&mut self, track: &Track, road0: f32, dt: f32) {
        let ramp = track.ramp_at(self.s, self.lateral);
        let surface = ramp.map_or(0.0, |(_, x)| Ramp::height(x));
        if self.airborne {
            self.airtime += dt;
            self.vz -= GRAVITY * dt;
            // Hold a steady course through the air as the road beneath rises and falls.
            self.air += self.vz * dt + road0 - track.height_at(self.s);
            if self.air <= surface && self.vz <= 0.0 {
                self.land(surface);
            }
            return;
        }
        if let Some((r, _)) = ramp {
            self.air = surface;
            self.on_ramp = if r.boosted { 2 } else { 1 };
            if r.boosted {
                self.boost = self.boost.max(1.0);
            }
            return;
        }
        if self.on_ramp != 0 {
            // Just left a ramp: off the lip going forwards launches, off the side just drops.
            let (_, dir) = track.point_at(self.s);
            let fwd = self.velocity().dot(dir);
            let lip = self.air > RAMP_HEIGHT * 0.8 && fwd > 0.0;
            self.airborne = true;
            self.airtime = 0.0;
            self.vz = 0.0;
            if lip {
                let grade = (track.height_at(self.s + 1.0) - track.height_at(self.s - 1.0)) * 0.5;
                let lift = if self.on_ramp == 2 { BOOSTED_LIFT } else { 1.0 };
                self.vz = (fwd * (Ramp::LIP_SLOPE * lift + grade)).clamp(0.0, MAX_LAUNCH);
            } else if self.trick_try == TrickTry::Ready {
                self.trick_try = TrickTry::Spent;
            }
            self.on_ramp = 0;
        }
    }

    fn cancel_drift(&mut self) {
        self.drift_dir = 0;
        self.drift_charge = 0.0;
        self.drift_pending = false;
    }

    pub fn step(&mut self, input: KartInput, track: &Track, time: f32) -> StepEvents {
        let dt = DT;
        let mut ev = StepEvents::default();
        let prev = self.prev_buttons;
        let pressed = |b: u8| input.buttons & b != 0 && prev & b == 0;

        if time < COUNTDOWN {
            // Rocket start: hit the throttle shortly before GO.
            if input.throttle > 0 {
                if self.rocket_press < 0.0 {
                    self.rocket_press = COUNTDOWN - time;
                }
            } else {
                self.rocket_press = -1.0;
            }
            self.prev_buttons = input.buttons;
            return ev;
        }
        if self.rocket_press >= 0.0 {
            if (0.1..=0.8).contains(&self.rocket_press) {
                self.boost = 1.2;
            }
            self.rocket_press = -2.0;
        }

        self.hop = (self.hop - dt).max(0.0);
        self.trick = (self.trick - dt).max(0.0);
        self.boost = (self.boost - dt).max(0.0);
        self.bump = (self.bump - dt).max(0.0);
        self.cheer = (self.cheer - dt).max(0.0);
        self.roulette = (self.roulette - dt).max(0.0);
        self.push *= (-5.0 * dt).exp();
        self.zoom = (self.zoom - dt).max(0.0);
        self.big = (self.big - dt).max(0.0);
        self.small = (self.small - dt).max(0.0);
        self.flat = (self.flat - dt).max(0.0);
        // A boost blows the mud off sooner.
        self.mud = (self.mud - dt * if self.is_boosting() { 3.0 } else { 1.0 }).max(0.0);
        if self.golden > 0.0 {
            self.golden = (self.golden - dt).max(0.0);
            if self.golden <= 0.0 && self.item == Item::GoldenTreat {
                self.drop_item();
            }
        }
        if self.cloud > 0.0 {
            self.cloud = (self.cloud - dt).max(0.0);
            if self.cloud <= 0.0 {
                self.shrink(CLOUD_SOAK);
            }
        }
        if self.chase > 0.0 {
            self.chase = (self.chase - dt).max(0.0);
            self.chase_squirrel(track, dt);
            self.prev_buttons = input.buttons;
            return ev;
        }

        if self.spin > 0.0 {
            self.spin = (self.spin - dt).max(0.0);
            self.speed *= (-2.5 * dt).exp();
            self.slip *= (-8.0 * dt).exp();
            let road0 = track.height_at(self.s);
            self.integrate(track, dt);
            self.vertical(track, road0, dt);
            self.prev_buttons = input.buttons;
            return ev;
        }

        let steer_in = input.steer();
        let throttle = input.throttle();
        self.steer += (steer_in - self.steer) * (1.0 - (-14.0 * dt).exp());

        // --- Tricks ---------------------------------------------------------------------
        // Press trick (or drift) as you leave a ramp's lip. On the ramp or in the air, a press
        // is the jump's one try at a trick, so it doesn't hop.
        let mut jumping = self.airborne;
        if (pressed(BTN_TRICK) || pressed(BTN_DRIFT))
            && let Some(timing) = self.trick_timing(track)
        {
            jumping = true;
            if self.trick_try == TrickTry::Ready {
                self.trick_try = timing;
                if timing == TrickTry::Done {
                    self.trick = TRICK_TIME;
                    self.trick_style = if steer_in.abs() > 0.3 { steer_in.signum() as i8 } else { 0 };
                }
            }
        }

        // --- Drifting -------------------------------------------------------------------
        // Press drift to hop; steering during the hop commits to a drift in that direction.
        if pressed(BTN_DRIFT) && !jumping && self.hop <= 0.0 && self.drift_dir == 0 {
            self.hop = HOP_TIME;
            self.drift_pending = true;
        }
        if self.drift_pending {
            if !input.drift() || self.hop <= 0.0 {
                self.drift_pending = false;
            } else if steer_in.abs() > 0.3 && self.speed > MIN_DRIFT_SPEED {
                self.drift_dir = steer_in.signum() as i8;
                self.drift_charge = 0.0;
                self.drift_pending = false;
            }
        }
        if self.drift_dir != 0 {
            if !input.drift() {
                // Release: cash in the mini-turbo.
                let stage = drift_stage(self.drift_charge);
                if stage > 0 {
                    self.boost = self.boost.max(DRIFT_BOOST[stage - 1]);
                }
                self.cancel_drift();
            } else if self.speed < MIN_DRIFT_SPEED * 0.7 {
                self.cancel_drift();
            } else if !self.offroad && !self.airborne {
                // Steering into the drift charges faster; steering out still charges slowly.
                let inward = self.steer * self.drift_dir as f32;
                self.drift_charge += (0.9 + 0.6 * inward) * dt;
            }
        }
        let slip_target = self.drift_dir as f32 * 0.3;
        self.slip += (slip_target - self.slip) * (1.0 - (-8.0 * dt).exp());

        // --- Speed ----------------------------------------------------------------------
        // The zoomies, a sausage and running from a rain cloud all carry you over grass and
        // through water, and make you a bit faster.
        let (sure_footed, pace) = if self.zoom > 0.0 {
            (true, ZOOM_SPEED / MAX_SPEED)
        } else if self.big > 0.0 {
            (true, 1.1)
        } else if self.cloud > 0.0 {
            (true, 1.08)
        } else {
            (false, 1.0)
        };
        let slowed = self.offroad && !self.is_boosting() && !sure_footed;
        // Small or flattened karts are slow whatever they do.
        let cc = self.cc * if self.small > 0.0 { 0.75 } else { 1.0 } * if self.flat > 0.0 { 0.6 } else { 1.0 };
        let top = cc * if self.is_boosting() {
            BOOST_SPEED
        } else if slowed {
            OFFROAD_SPEED
        } else if self.in_water && !sure_footed {
            WATER_SPEED
        } else {
            MAX_SPEED * self.speed_scale * pace
        };
        if self.is_boosting() {
            self.speed = (self.speed + 60.0 * cc * dt).min(top);
        } else if self.airborne {
            // Coast through the air.
        } else if self.speed > top + 0.1 {
            let decel = if slowed { 35.0 } else { 14.0 };
            self.speed = (self.speed - decel * dt).max(top);
        } else if throttle > 0.05 {
            let frac = (self.speed / top).clamp(0.0, 1.0);
            let accel = 24.0 * cc * (1.0 - frac * frac).max(0.12) * throttle;
            self.speed = (self.speed + accel * dt).min(top);
        } else if throttle < -0.05 {
            if self.speed > 0.5 {
                self.speed -= 38.0 * -throttle * dt;
            } else {
                self.speed = (self.speed - 16.0 * -throttle * dt).max(-11.0);
            }
        } else {
            let s = self.speed.abs();
            self.speed = self.speed.signum() * (s - 9.0 * dt).max(0.0);
        }

        // --- Turning --------------------------------------------------------------------
        let base_turn = base_turn(self.speed);
        let air_turn = if self.airborne { 0.5 } else { 1.0 };
        let yaw_rate = air_turn * if self.drift_dir != 0 {
            let d = self.drift_dir as f32;
            d * drift_curvature(self.steer * d) * self.speed
        } else {
            let speed_factor = (self.speed.abs() / 8.0).min(1.0);
            self.steer * base_turn * speed_factor * self.speed.signum()
        };
        self.heading = wrap_angle(self.heading + yaw_rate * dt);
        if self.drift_dir != 0 {
            // Drifts swing slightly wide, so line choice matters.
            self.pos -= self.dir().perp() * self.drift_dir as f32 * 1.8 * dt;
        }

        let road0 = track.height_at(self.s);
        self.integrate(track, dt);
        self.vertical(track, road0, dt);

        // Boost pads
        for pad in track.boost_pads.iter().filter(|_| !self.airborne) {
            let ds = (self.s - pad.s).rem_euclid(track.length);
            if ds < pad.length && (self.lateral - pad.lateral).abs() < pad.half_width {
                self.boost = self.boost.max(1.0);
            }
        }

        if pressed(BTN_ITEM) && self.item != Item::None && self.roulette <= 0.0 {
            ev.use_item = true;
        }
        self.prev_buttons = input.buttons;
        ev
    }

    /// After the squirrel: flat out along the middle of the road, steering itself.
    fn chase_squirrel(&mut self, track: &Track, dt: f32) {
        let (ahead, _) = track.point_at(self.s + 14.0);
        let err = wrap_angle((ahead - self.pos).to_angle() - self.heading);
        self.heading = wrap_angle(self.heading + err.clamp(-5.0 * dt, 5.0 * dt));
        self.steer = (err * 2.0).clamp(-1.0, 1.0);
        self.speed = (self.speed + 80.0 * self.cc * dt).min(CHASE_SPEED * self.cc);
        self.spin = 0.0;
        self.cancel_drift();
        self.slip *= (-8.0 * dt).exp();
        let road0 = track.height_at(self.s);
        self.integrate(track, dt);
        self.vertical(track, road0, dt);
        if self.chase <= 0.0 {
            // It got away. Carry some of the speed on.
            self.boost = self.boost.max(0.6);
        }
    }

    /// Move, collide with the walls and update track progress / laps.
    fn integrate(&mut self, track: &Track, dt: f32) {
        self.pos += self.velocity() * dt;

        let tp = track.locate(self.pos, self.track_idx as usize);
        let limit = WALL_DIST - self.radius();
        if tp.lateral.abs() > limit {
            let side = tp.lateral.signum();
            let normal = -tp.dir.perp() * side; // points back onto the track
            self.pos += normal * (tp.lateral.abs() - limit);
            let v = self.velocity();
            let impact = -v.dot(normal);
            if impact > 0.0 {
                if self.bump <= 0.0 {
                    self.speed -= self.speed.signum() * (impact * 0.7).min(self.speed.abs());
                    self.push += normal * (impact * 0.5 + 2.0);
                    self.bump = 0.35;
                    if impact > 8.0 {
                        self.cancel_drift();
                    }
                }
                // Scrape along the wall: gently align to the track.
                let along = tp.dir * tp.dir.dot(self.dir()).signum();
                let target = along.to_angle();
                let diff = wrap_angle(target - self.heading);
                self.heading = wrap_angle(self.heading + diff.clamp(-1.5 * dt, 1.5 * dt));
            }
        }

        let tp = track.locate(self.pos, tp.idx);
        self.track_idx = tp.idx as u32;
        self.s = tp.s;
        self.lateral = tp.lateral;
        // Flying over the grass or a ford doesn't slow you down.
        self.offroad = !self.airborne && tp.lateral.abs() > ROAD_HALF_WIDTH + 0.3;
        self.in_water = !self.airborne && track.in_water(tp.s);

        let sector = track.sector(tp.s);
        let next = (self.checkpoint as usize + 1) % CHECKPOINTS;
        if sector == next {
            self.checkpoint = next as u8;
            if next == 0 && self.lap <= LAPS {
                self.lap += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::track::ROAD_HALF_WIDTH;

    /// Drift through every real corner the way a keyboard player would (steering only
    /// in / neutral / out) and check the drift can hold a line that stays on the road.
    #[test]
    fn keyboard_drift_follows_corners() {
        for id in crate::sim::TrackId::ALL {
            // 100cc and 150cc speeds
            for (cc, speed) in [(1.0, 28.0), (1.2, 34.0)] {
                println!("{} at {speed} m/s", id.name());
                drift_corners(&Track::new(id), cc, speed);
            }
        }
    }

    /// Drive over each of a track's ramps, pressing trick at various moments.
    #[test]
    fn ramps_launch_and_tricks_need_timing() {
        #[derive(Clone, Copy, Debug, PartialEq)]
        enum Press {
            Never,
            OnRampFoot,
            AtLip,
            LateInAir,
        }
        for id in crate::sim::TrackId::ALL {
            let track = Track::new(id);
            assert!(!track.ramps.is_empty(), "{} has no ramps", id.name());
            for ramp in track.ramps.clone() {
                for press in [Press::Never, Press::OnRampFoot, Press::AtLip, Press::LateInAir] {
                    let (c, d) = track.point_at(ramp.s - 45.0);
                    let mut k = Kart::new(&track, c + d.perp() * ramp.lateral, d.to_angle(), false, 1.0, 1.0);
                    k.speed = 29.0;
                    let mut time = COUNTDOWN + 1.0;
                    let (mut max_air, mut flew, mut landed_at, mut took_off_at) = (0.0f32, 0.0f32, None, None);
                    let mut outcome = TrickTry::Ready;
                    for _ in 0..300 {
                        time += DT;
                        // Hold the ramp's line.
                        let (tc, _) = track.point_at(k.s + 12.0);
                        let target = tc + track.point_at(k.s + 12.0).1.perp() * ramp.lateral;
                        let steer = (wrap_angle((target - k.pos).to_angle() - k.heading) * 3.0).clamp(-1.0, 1.0);
                        let on = track.ramp_at(k.s, k.lateral).map(|(_, x)| x);
                        let trick = match press {
                            Press::Never => false,
                            Press::OnRampFoot => on.is_some_and(|x| x < 1.5),
                            Press::AtLip => k.trick_timing(&track) == Some(TrickTry::Done),
                            Press::LateInAir => k.airborne && k.airtime > 0.35,
                        };
                        let was_airborne = k.airborne;
                        k.step(KartInput::new(steer, 1.0, false, false, trick), &track, time);
                        if k.trick_try != TrickTry::Ready {
                            outcome = k.trick_try;
                        }
                        max_air = max_air.max(k.air);
                        if k.airborne {
                            flew += DT;
                            took_off_at.get_or_insert(k.s);
                        }
                        if was_airborne && !k.airborne {
                            landed_at = Some(k.s);
                            break;
                        }
                    }
                    let dist = (landed_at.unwrap_or(0.0) - took_off_at.unwrap_or(0.0)).rem_euclid(track.length);
                    println!(
                        "{} ramp at {:.0} (boosted {}), {press:?}: air {max_air:.1}m for {flew:.2}s over {dist:.0}m, {outcome:?}, boost {:.2}",
                        id.name(), ramp.s, ramp.boosted, k.boost
                    );
                    assert!(landed_at.is_some(), "never landed");
                    assert!(flew > 0.7 && max_air > 2.5, "barely left the ground");
                    assert!(k.lateral.abs() < WALL_DIST - KART_RADIUS - 0.1, "landed in the wall");
                    let want = match press {
                        Press::Never => TrickTry::Ready,
                        Press::OnRampFoot => TrickTry::Early,
                        Press::AtLip => TrickTry::Done,
                        Press::LateInAir => TrickTry::Late,
                    };
                    assert_eq!(outcome, want);
                    // The landing boost only comes from a good trick (a boosted ramp's own boost
                    // has worn off by then).
                    assert_eq!(k.boost > 0.5, want == TrickTry::Done, "boost {}", k.boost);
                    assert_eq!(k.trick_try, TrickTry::Ready, "the try resets on landing");
                }
            }
        }
    }

    fn drift_corners(track: &Track, cc: f32, speed: f32) {
        let track = track;
        let mut corners = vec![];
        let mut s = 0.0;
        while s < track.length {
            let turn = track.turn_between(s + 5.0, s + 45.0);
            if turn.abs() > 0.6 {
                corners.push((s, turn.signum()));
                s += 90.0;
            } else {
                s += 5.0;
            }
        }
        assert!(!corners.is_empty());

        let mut failures = vec![];
        for (entry, dir) in corners {
            // Start where the bend really begins (an S-bend may first go the other way).
            let mut entry = entry;
            for _ in 0..8 {
                if track.turn_between(entry, entry + 25.0) * dir > 0.15 {
                    break;
                }
                entry += 5.0;
            }
            let (c, d) = track.point_at(entry);
            let mut k = Kart::new(track, c, d.to_angle(), false, 1.0, cc);
            k.speed = speed;
            let mut time = COUNTDOWN + 1.0;
            let mut worst: f32 = 0.0;
            let mut drift_ticks = 0;
            for tick in 0..300 {
                time += DT;
                let look = 10.0 + 0.5 * k.speed;
                let (tc, _) = track.point_at(k.s + look);
                let err = wrap_angle((tc - k.pos).to_angle() - k.heading);
                let still_turning = track.turn_between(k.s, k.s + 25.0) * dir > 0.15;
                let (steer, drift) = if tick == 0 {
                    (dir, true)
                } else if k.drift_dir != 0 && still_turning {
                    let e = err * dir; // > 0: need to turn further into the corner
                    let x = if e > 0.08 { 1.0 } else if e < -0.08 { -1.0 } else { 0.0 };
                    (x * dir, true)
                } else {
                    break;
                };
                k.step(KartInput::new(steer, 1.0, drift, false, false), track, time);
                if k.drift_dir != 0 {
                    drift_ticks += 1;
                    worst = worst.max(k.lateral.abs());
                }
            }
            println!("corner at {entry:.0}m: drifted {drift_ticks} ticks, worst |lateral| {worst:.1}m");
            if drift_ticks < 30 || worst > ROAD_HALF_WIDTH + 1.0 {
                failures.push((entry, drift_ticks, worst));
            }
        }
        assert!(failures.is_empty(), "couldn't drift these corners (s, ticks, worst lateral): {failures:?}");
    }
}
