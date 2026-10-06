//! Whole-race state: karts, item boxes, projectiles, traps and the items' wider effects. This
//! struct is what the host sends to clients every tick.

use std::f32::consts::TAU;

use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

use super::kart::{Item, Kart, KartInput, BIG_TIME, CHEER_TIME, CLOUD_TIME, GOLDEN_TIME, KART_RADIUS, MUD_TIME, ZOOM_TIME};
use super::track::{Track, WALL_DIST};
use super::{SpeedClass, COUNTDOWN, DT, LAPS};

pub const ITEM_BOX_RESPAWN: f32 = 2.5;
const BONE_SPEED: f32 = 50.0;
const BALL_SPEED: f32 = 48.0;
const FRISBEE_SPEED: f32 = 75.0;
/// A frisbee flies this high, over everyone but the leader.
pub const FRISBEE_HEIGHT: f32 = 3.5;
/// How close a frisbee gets to the leader before it swoops down at them.
pub const FRISBEE_SWOOP: f32 = 30.0;
const MAX_TRAPS: usize = 24;
/// A squeaky toy thrown forwards leaves your hand this much faster than the kart...
const TOY_THROW_SPEED: f32 = 20.0;
/// ...and flies this long, peaking this high, before it lands on the road ahead.
pub const TOY_FLIGHT: f32 = 0.9;
const TOY_ARC: f32 = 4.5;
/// A water balloon is thrown harder and further.
const BALLOON_THROW_SPEED: f32 = 34.0;
const BALLOON_FLIGHT: f32 = 1.0;
/// How long a balloon lasts once it's down: thrown, and dropped behind.
const BALLOON_FUSE: (f32, f32) = (0.5, 3.0);
/// A burst soaks everyone this close, and throws those this close into the air.
pub const BLAST_RADIUS: f32 = 7.0;
const BLAST_CORE: f32 = 4.5;
/// How long a burst is kept (so everyone sees it), and how long it's dangerous.
pub const BURST_LIFE: f32 = 0.6;
const BURST_BITE: f32 = 0.3;
/// Three bones or balls circle their kart this far out, this fast (radians a second).
const ORBIT_RADIUS: f32 = 2.3;
const ORBIT_SPEED: f32 = 4.0;
/// When a big woof's three warning barks come, and when the last one lands.
pub const WOOF_BARKS: [f32; 3] = [0.0, 0.8, 1.6];
pub const WOOF_TIME: f32 = 2.4;
/// The far-reaching items aren't handed out in the first seconds of a race.
const EARLY: f32 = 15.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemBox {
    pub pos: Vec2,
    pub respawn: f32,
    pub track_idx: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub id: u16,
    pub kind: Item,
    pub pos: Vec2,
    pub vel: Vec2,
    pub owner: u8,
    pub age: f32,
    pub target: i8,
    pub bounces: u8,
    pub track_idx: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trap {
    pub id: u16,
    /// A squeaky toy, a decoy box or a water balloon.
    pub kind: Item,
    pub pos: Vec2,
    pub owner: u8,
    pub age: f32,
    pub track_idx: u32,
    /// Horizontal velocity while it's thrown; still once it lands.
    pub vel: Vec2,
    /// Seconds until it lands. Nothing hits it in the air.
    pub flight: f32,
    /// A balloon's seconds left once it's down.
    pub fuse: f32,
}

impl Trap {
    fn flight_time(kind: Item) -> f32 {
        if kind == Item::WaterBalloon { BALLOON_FLIGHT } else { TOY_FLIGHT }
    }

    /// Height above the road.
    pub fn height(&self) -> f32 {
        let t = 1.0 - (self.flight / Self::flight_time(self.kind)).clamp(0.0, 1.0);
        4.0 * TOY_ARC * t * (1.0 - t)
    }
}

/// Something that just went off: a balloon or frisbee bursting (which soaks whoever is near for
/// a moment), or a dog whistle being blown (which is only kept to be seen and heard).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Burst {
    pub id: u16,
    pub kind: Item,
    pub pos: Vec2,
    pub owner: u8,
    pub age: f32,
    pub track_idx: u32,
}

/// A big woof on its way: `t` counts up through the warning barks to `WOOF_TIME`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Woof {
    pub owner: u8,
    pub t: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    pub tick: u32,
    pub time: f32,
    pub karts: Vec<Kart>,
    pub boxes: Vec<ItemBox>,
    pub projectiles: Vec<Projectile>,
    pub traps: Vec<Trap>,
    pub bursts: Vec<Burst>,
    pub woofs: Vec<Woof>,
    pub finish_order: Vec<u8>,
    pub end_timer: f32,
    pub over: bool,
    rng: u32,
    next_id: u16,
}

/// What an item box can hand out, and how likely each is (the columns) by how far back you are
/// (the rows, leader first): the leader gets things to defend with, the back of the field gets
/// the means to catch up.
const ROLLS: [(Item, u8); 19] = [
    (Item::Treat, 1),
    (Item::Treat, 3),
    (Item::GoldenTreat, 1),
    (Item::Bone, 1),
    (Item::Bone, 3),
    (Item::TennisBall, 1),
    (Item::TennisBall, 3),
    (Item::SqueakyToy, 1),
    (Item::SqueakyToy, 3),
    (Item::DecoyBox, 1),
    (Item::WaterBalloon, 1),
    (Item::Frisbee, 1),
    (Item::DogWhistle, 1),
    (Item::Zoomies, 1),
    (Item::Squirrel, 1),
    (Item::Sausage, 1),
    (Item::BigWoof, 1),
    (Item::MudPie, 1),
    (Item::RainCloud, 1),
];
#[rustfmt::skip]
const ODDS: [[u8; 19]; 8] = [
    // tr  tr3 gold bone bn3 ball bl3  toy ty3  dcy bln  fris whis zoom sqrl saus woof mud  cloud
    [   5,   0,   0,  30,   4,   0,   0,  32,   8,  18,   0,   0,   0,   0,   0,   0,   0,   0,   0],
    [  10,   0,   0,  22,   6,  20,   0,  18,   6,  10,   4,   0,   0,   0,   0,   0,   2,   2,   0],
    [  14,   4,   0,  12,   8,  22,   2,   8,   4,   5,   7,   0,   0,   0,   0,   0,   4,   5,   5],
    [  12,  10,   0,   6,   8,  18,   6,   2,   0,   2,   8,   3,   0,   0,   0,   5,   6,   7,   7],
    [   6,  16,   6,   0,   5,  10,  10,   0,   0,   0,   6,   6,   1,   3,   0,  10,   7,   7,   7],
    [   0,  18,  12,   0,   0,   5,  10,   0,   0,   0,   3,   8,   3,   9,   4,  12,   5,   5,   4],
    [   0,  14,  18,   0,   0,   0,   6,   0,   0,   0,   0,   8,   8,  18,  12,   8,   2,   2,   0],
    [   0,  10,  20,   0,   0,   0,   0,   0,   0,   0,   0,   6,  14,  22,  24,   4,   0,   0,   0],
];

/// Where the bone or ball in `slot` (0..3) is as it circles kart `k`.
pub fn orbit_pos(k: &Kart, slot: u8, time: f32) -> Vec2 {
    k.pos + Vec2::from_angle(time * ORBIT_SPEED + slot as f32 * TAU / 3.0) * ORBIT_RADIUS * k.size()
}

impl World {
    /// `bots[i]` says whether kart `i` is AI-controlled. Karts fill the grid front to back.
    pub fn new(track: &Track, bots: &[bool], seed: u32, class: SpeedClass) -> Self {
        let mut rng = seed | 1;
        let karts = bots
            .iter()
            .enumerate()
            .map(|(i, &bot)| {
                let row = (i / 2) as f32;
                let col = i % 2;
                let s = -8.0 - row * 7.0 - col as f32 * 3.5;
                let (c, d) = track.point_at(s);
                let lat = if col == 0 { -4.5 } else { 4.5 };
                let scale = if bot { 0.93 + 0.06 * xorshift_f(&mut rng) } else { 1.0 };
                Kart::new(track, c + d.perp() * lat, d.to_angle(), bot, scale, class.mult())
            })
            .collect();
        let boxes = track
            .item_rows
            .iter()
            .flat_map(|&s| {
                let (c, d) = track.point_at(s);
                let track_idx = (track.wrap_s(s) / track.spacing) as u32;
                [-8.0, -4.0, 0.0, 4.0, 8.0].map(|lat| ItemBox { pos: c + d.perp() * lat, respawn: 0.0, track_idx })
            })
            .collect();
        let mut w = World {
            tick: 0,
            time: 0.0,
            karts,
            boxes,
            projectiles: vec![],
            traps: vec![],
            bursts: vec![],
            woofs: vec![],
            finish_order: vec![],
            end_timer: 0.0,
            over: false,
            rng,
            next_id: 1,
        };
        w.update_places(track);
        w
    }

    pub fn race_time(&self) -> f32 {
        (self.time - COUNTDOWN).max(0.0)
    }

    fn rand(&mut self) -> f32 {
        xorshift_f(&mut self.rng)
    }

    fn new_id(&mut self) -> u16 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    /// Kart indices from first to last place.
    /// Whether a world sent by a LAN host is one for a race of `karts` racers: that many karts,
    /// and nothing that names a kart that isn't there.
    pub fn fits(&self, karts: usize) -> bool {
        let kart = |i: u8| (i as usize) < karts;
        self.karts.len() == karts
            && self.finish_order.iter().all(|&i| kart(i))
            && self.projectiles.iter().all(|p| kart(p.owner) && (p.target as i32) < karts as i32)
            && self.traps.iter().all(|t| kart(t.owner))
            && self.bursts.iter().all(|b| kart(b.owner))
            && self.woofs.iter().all(|w| kart(w.owner))
    }

    pub fn standings(&self, track: &Track) -> Vec<usize> {
        let mut order: Vec<usize> = self.finish_order.iter().map(|&i| i as usize).collect();
        let mut rest: Vec<usize> = (0..self.karts.len()).filter(|i| !order.contains(i)).collect();
        rest.sort_by(|&a, &b| {
            self.karts[b].progress(track).total_cmp(&self.karts[a].progress(track))
        });
        order.extend(rest);
        order
    }

    fn update_places(&mut self, track: &Track) {
        for (place, i) in self.standings(track).into_iter().enumerate() {
            self.karts[i].place = place as u8;
        }
    }

    /// How far back `place` is: 0 for the leader, 1 for last.
    fn behind(&self, place: u8) -> f32 {
        place as f32 / (self.karts.len().max(2) - 1) as f32
    }

    pub fn step(&mut self, track: &Track, inputs: &[KartInput]) {
        self.tick += 1;
        self.time += DT;
        let time = self.time;

        let mut uses = vec![];
        for (i, kart) in self.karts.iter_mut().enumerate() {
            let input = inputs.get(i).copied().unwrap_or_default();
            if kart.step(input, track, time).use_item {
                uses.push((i, input));
            }
            // A rain cloud doesn't wait to be used.
            if kart.item == Item::RainCloud && kart.roulette <= 0.0 {
                kart.drop_item();
                if !kart.immune() {
                    kart.cloud = CLOUD_TIME;
                }
            }
        }
        for (i, input) in uses {
            self.use_item(i, input);
        }

        self.collide_karts(track);
        self.update_boxes(track);
        self.update_orbits(track);
        self.update_projectiles(track);
        self.update_traps(track);
        self.update_bursts(track);
        self.update_woofs();
        self.update_places(track);

        for i in 0..self.karts.len() {
            let k = &mut self.karts[i];
            if !k.finished && k.lap > LAPS {
                k.finished = true;
                k.finish_time = (time - COUNTDOWN).max(0.0);
                self.finish_order.push(i as u8);
            }
        }
        let humans_done = self.karts.iter().filter(|k| !k.bot).all(|k| k.finished);
        let all_done = self.karts.iter().all(|k| k.finished);
        if humans_done || all_done {
            self.end_timer += DT;
            if self.end_timer > 3.0 {
                self.over = true;
            }
        }
    }

    fn use_item(&mut self, i: usize, input: KartInput) {
        let k = &self.karts[i];
        let (pos, dir, speed, place, item, cc) = (k.pos, k.dir(), k.speed, k.place, k.item, k.cc);
        let track_idx = k.track_idx;
        let owner = i as u8;
        match item {
            // A rain cloud uses itself.
            Item::None | Item::RainCloud => return,
            Item::Treat => self.karts[i].boost = self.karts[i].boost.max(1.3),
            Item::GoldenTreat => {
                // The first bite starts the clock; it's gone when that runs out.
                let k = &mut self.karts[i];
                if k.golden <= 0.0 {
                    k.golden = GOLDEN_TIME;
                }
                k.boost = k.boost.max(1.0);
                return;
            }
            Item::Bone | Item::TennisBall | Item::Frisbee => {
                // Holding brake still throws a bone back, for keyboards with nothing to aim.
                let backwards = item != Item::Frisbee && (input.aim() < 0 || (item == Item::Bone && input.aim() == 0 && input.throttle() < -0.5));
                let d = if backwards { -dir } else { dir };
                let spd = cc * match item {
                    Item::Bone => BONE_SPEED,
                    Item::TennisBall => BALL_SPEED,
                    _ => FRISBEE_SPEED,
                };
                // A ball thrown backwards flies straight.
                let target = if item == Item::TennisBall && !backwards && place > 0 {
                    self.karts.iter().position(|o| o.place == place - 1).map_or(-1, |t| t as i8)
                } else {
                    -1
                };
                let id = self.new_id();
                self.projectiles.push(Projectile {
                    id,
                    kind: item,
                    pos: pos + d * (KART_RADIUS + 1.2),
                    vel: d * (spd + speed.max(0.0) * 0.3),
                    owner,
                    age: 0.0,
                    target,
                    bounces: 0,
                    track_idx,
                });
            }
            Item::SqueakyToy | Item::DecoyBox | Item::WaterBalloon => {
                let id = self.new_id();
                let balloon = item == Item::WaterBalloon;
                // Toys and decoys are dropped unless they're aimed ahead; a balloon is thrown
                // unless it's aimed behind.
                let thrown = if balloon { input.aim() >= 0 } else { input.aim() > 0 };
                let trap = if thrown {
                    let throw = if balloon { BALLOON_THROW_SPEED } else { TOY_THROW_SPEED };
                    let vel = dir * (cc * throw + speed.max(0.0));
                    Trap { id, kind: item, pos: pos + dir * (KART_RADIUS + 0.5), owner, age: 0.0, track_idx, vel, flight: Trap::flight_time(item), fuse: BALLOON_FUSE.0 }
                } else {
                    Trap { id, kind: item, pos: pos - dir * (KART_RADIUS + 1.3), owner, age: 0.0, track_idx, vel: Vec2::ZERO, flight: 0.0, fuse: BALLOON_FUSE.1 }
                };
                self.traps.push(trap);
                if self.traps.len() > MAX_TRAPS {
                    self.traps.remove(0);
                }
            }
            Item::DogWhistle => {
                // The further up the field, the longer they cower.
                for o in 0..self.karts.len() {
                    let secs = 3.5 + 3.5 * (1.0 - self.behind(self.karts[o].place));
                    if o != i {
                        self.karts[o].shrink(secs);
                    }
                }
                self.burst(item, pos, track_idx, owner);
            }
            Item::Zoomies => self.karts[i].zoom = ZOOM_TIME,
            Item::Squirrel => {
                // The further back, the longer the squirrel keeps ahead.
                let secs = 2.5 + 3.5 * self.behind(place);
                let k = &mut self.karts[i];
                (k.chase, k.small, k.flat, k.cloud) = (secs, 0.0, 0.0, 0.0);
            }
            Item::Sausage => {
                let k = &mut self.karts[i];
                (k.big, k.small, k.flat) = (BIG_TIME, 0.0, 0.0);
            }
            Item::BigWoof => self.woofs.push(Woof { owner, t: 0.0 }),
            Item::MudPie => {
                for o in self.karts.iter_mut().filter(|o| o.place < place && !o.immune()) {
                    o.mud = MUD_TIME;
                }
            }
        }
        let k = &mut self.karts[i];
        k.item_count = k.item_count.saturating_sub(1);
        if k.item_count == 0 {
            k.drop_item();
        }
    }

    fn burst(&mut self, kind: Item, pos: Vec2, track_idx: u32, owner: u8) {
        let id = self.new_id();
        self.bursts.push(Burst { id, kind, pos, owner, age: 0.0, track_idx });
    }

    /// Kart `i` hit someone with an item: its corgi celebrates, unless it's busy spinning out.
    fn cheer(&mut self, i: usize) {
        if let Some(k) = self.karts.get_mut(i).filter(|k| k.spin <= 0.0) {
            k.cheer = CHEER_TIME;
        }
    }

    fn collide_karts(&mut self, track: &Track) {
        let n = self.karts.len();
        let mut cheers = vec![];
        for ai in 0..n {
            for bi in ai + 1..n {
                let (left, right) = self.karts.split_at_mut(bi);
                let (a, b) = (&mut left[ai], &mut right[0]);
                if !track.same_level(a.track_idx as usize, b.track_idx as usize) || (a.air - b.air).abs() > 1.2 {
                    continue;
                }
                let d = b.pos - a.pos;
                let dist = d.length();
                let min = a.radius() + b.radius();
                if dist >= min || dist <= 1e-4 {
                    continue;
                }
                let nrm = d / dist;
                let overlap = min - dist;
                let rel = (b.velocity() - a.velocity()).dot(nrm);
                let shove = (-rel).max(0.0) * 0.5 + 1.5;

                // A kart with the zoomies or after a squirrel goes straight through anyone, and
                // a huge one through anyone smaller: only the other kart is moved.
                let might = |k: &Kart| if k.immune() { 2 } else if k.big > 0.0 { 1 } else { 0 };
                if might(a) != might(b) {
                    let (strong, weak, si, away) = if might(a) > might(b) { (a, b, ai, nrm) } else { (b, a, bi, -nrm) };
                    let done = if strong.chase > 0.0 {
                        weak.blast()
                    } else if strong.zoom > 0.0 {
                        weak.big = 0.0;
                        weak.hit()
                    } else {
                        weak.squash()
                    };
                    weak.pos += away * overlap;
                    weak.push += away * (shove + 3.0);
                    if done {
                        cheers.push(si);
                    }
                    continue;
                }

                a.pos -= nrm * overlap * 0.5;
                b.pos += nrm * overlap * 0.5;
                a.push -= nrm * shove;
                b.push += nrm * shove;
                // A cowering kart is flattened by one that isn't.
                if (a.small > 0.0) != (b.small > 0.0) && might(a) == 0 {
                    let (small, other) = if a.small > 0.0 { (a, bi) } else { (b, ai) };
                    if small.squash() {
                        cheers.push(other);
                    }
                }
                // A bump passes the rain cloud on (but not straight back).
                let (a, b) = (&mut left[ai], &mut right[0]);
                let (from, to) = if a.cloud > 0.0 { (a, b) } else { (b, a) };
                if from.cloud > 0.0 && to.cloud <= 0.0 && from.bump <= 0.0 {
                    (to.cloud, from.cloud) = (from.cloud, 0.0);
                    to.bump = 0.35;
                }
            }
        }
        for i in cheers {
            self.cheer(i);
        }
    }

    fn roll_item(&mut self, place: u8) -> (Item, u8) {
        // A small field is closer together, so its back markers get less help.
        let n = self.karts.len().max(2) as f32;
        let spread = 0.5 + 0.5 * (n - 1.0) / 7.0;
        let row = ((self.behind(place) * spread * 7.0).round() as usize).min(7);
        let early = self.race_time() < EARLY;
        let mut w = ODDS[row];
        for (wt, (item, _)) in w.iter_mut().zip(ROLLS) {
            // Only one of each of these at a time.
            let barred = match item {
                Item::Frisbee => early || self.projectiles.iter().any(|p| p.kind == Item::Frisbee),
                Item::DogWhistle => early || self.karts.iter().any(|k| k.small > 0.0 || k.item == Item::DogWhistle),
                Item::BigWoof => early || !self.woofs.is_empty(),
                Item::RainCloud => self.karts.iter().any(|k| k.cloud > 0.0 || k.item == Item::RainCloud),
                _ => false,
            };
            if barred {
                *wt = 0;
            }
        }
        let total: u32 = w.iter().map(|&x| x as u32).sum();
        let mut r = self.rand() * total as f32;
        for (wt, opt) in w.iter().zip(ROLLS) {
            if r < *wt as f32 {
                return opt;
            }
            r -= *wt as f32;
        }
        ROLLS[0]
    }

    fn update_boxes(&mut self, track: &Track) {
        for bi in 0..self.boxes.len() {
            if self.boxes[bi].respawn > 0.0 {
                self.boxes[bi].respawn = (self.boxes[bi].respawn - DT).max(0.0);
                continue;
            }
            let (bpos, bidx) = (self.boxes[bi].pos, self.boxes[bi].track_idx as usize);
            let hit = self
                .karts
                .iter()
                .position(|k| k.pos.distance(bpos) < k.radius() + 1.0 && k.air < 2.5 && track.same_level(k.track_idx as usize, bidx));
            if let Some(ki) = hit {
                self.boxes[bi].respawn = ITEM_BOX_RESPAWN;
                let k = &self.karts[ki];
                if k.item == Item::None && k.roulette <= 0.0 {
                    let (item, count) = self.roll_item(k.place);
                    let k = &mut self.karts[ki];
                    k.item = item;
                    k.item_count = count;
                    k.triple = count == 3;
                    k.roulette = 1.2;
                }
            }
        }
    }

    /// Bones and balls circling a kart: each takes out the first kart, thrown item or trap it
    /// touches, and is used up doing it.
    fn update_orbits(&mut self, track: &Track) {
        for i in 0..self.karts.len() {
            let mut slot = 0;
            while slot < self.karts[i].orbiting() {
                let k = &self.karts[i];
                let (pos, idx) = (orbit_pos(k, slot, self.time), k.track_idx as usize);
                let (air, owner) = (k.air, i as u8);
                let kart = self.karts.iter().position(|o| {
                    !std::ptr::eq(o, k)
                        && o.spin <= 0.0
                        && (o.air - air).abs() < 1.4
                        && o.pos.distance(pos) < o.radius() + 0.6
                        && track.same_level(o.track_idx as usize, idx)
                });
                // Anything thrown at the kart is stopped as it crosses the ring, wherever on the
                // ring that is: a shield with gaps to slip through wouldn't be much of one.
                let (centre, ring) = (k.pos, ORBIT_RADIUS * k.size() + 0.8);
                let thrown = self.projectiles.iter().position(|p| {
                    p.owner != owner && p.kind != Item::Frisbee && p.pos.distance(centre) < ring && track.same_level(p.track_idx as usize, idx)
                });
                let trap = self.traps.iter().position(|t| t.flight <= 0.0 && t.pos.distance(pos) < 1.6 && track.same_level(t.track_idx as usize, idx));
                if let Some(o) = kart {
                    if self.karts[o].hit() {
                        self.cheer(i);
                    }
                } else if let Some(p) = thrown {
                    self.projectiles.remove(p);
                } else if let Some(t) = trap {
                    self.spring(t);
                } else {
                    slot += 1;
                    continue;
                }
                let k = &mut self.karts[i];
                k.item_count -= 1;
                if k.item_count == 0 {
                    k.drop_item();
                }
            }
        }
    }

    /// Trap `ti` has been set off by something other than a kart: it's gone, and a balloon bursts.
    fn spring(&mut self, ti: usize) {
        let t = self.traps.remove(ti);
        if t.kind == Item::WaterBalloon {
            self.burst(t.kind, t.pos, t.track_idx, t.owner);
        }
    }

    /// A frisbee follows the road round to whoever is leading, then swoops on them.
    fn fly_frisbee(&mut self, track: &Track, p: &mut Projectile) -> bool {
        let cc = self.karts[p.owner as usize].cc;
        let Some(leader) = self.karts.iter().position(|k| k.place == 0) else { return true };
        p.target = leader as i8;
        let t = &self.karts[leader];
        let tp = track.locate(p.pos, p.track_idx as usize);
        let close = p.age > 0.5 && t.pos.distance(p.pos) < FRISBEE_SWOOP && track.same_level(t.track_idx as usize, tp.idx);
        let (aim, turn) = if close { (t.pos, 12.0) } else { (track.point_at(tp.s + 18.0).0, 6.0) };
        let want = (aim - p.pos).normalize_or_zero();
        let cur = p.vel.normalize_or_zero();
        let ang = cur.perp_dot(want).atan2(cur.dot(want)).clamp(-turn * DT, turn * DT);
        p.vel = Vec2::from_angle(cur.to_angle() + ang) * FRISBEE_SPEED * cc;
        p.pos += p.vel * DT;
        p.track_idx = track.locate(p.pos, tp.idx).idx as u32;
        if close && t.pos.distance(p.pos) < 3.0 {
            let (pos, idx) = (t.pos, t.track_idx);
            self.burst(Item::Frisbee, pos, idx, p.owner);
            return true;
        }
        p.age > 40.0
    }

    fn update_projectiles(&mut self, track: &Track) {
        let mut remove = vec![];
        let mut cheers = vec![];
        for pi in 0..self.projectiles.len() {
            let mut p = self.projectiles[pi].clone();
            p.age += DT;

            if p.kind == Item::Frisbee {
                if self.fly_frisbee(track, &mut p) {
                    remove.push(pi);
                }
                self.projectiles[pi] = p;
                continue;
            }
            if p.kind == Item::TennisBall && p.target >= 0 {
                let t = &self.karts[p.target as usize];
                let tp = track.locate(p.pos, p.track_idx as usize);
                let aim = if t.pos.distance(p.pos) < 30.0 {
                    t.pos
                } else {
                    let (c, d) = track.point_at(tp.s + 14.0);
                    c + d.perp() * tp.lateral * 0.5
                };
                let want = (aim - p.pos).normalize_or_zero();
                let cur = p.vel.normalize_or_zero();
                let ang = cur.perp_dot(want).atan2(cur.dot(want)).clamp(-5.0 * DT, 5.0 * DT);
                p.vel = Vec2::from_angle(cur.to_angle() + ang) * BALL_SPEED * self.karts[p.owner as usize].cc;
            }
            p.pos += p.vel * DT;

            let tp = track.locate(p.pos, p.track_idx as usize);
            p.track_idx = tp.idx as u32;
            let limit = WALL_DIST - 0.6;
            if tp.lateral.abs() > limit {
                let normal = -tp.dir.perp() * tp.lateral.signum();
                p.pos += normal * (tp.lateral.abs() - limit);
                let vn = p.vel.dot(normal);
                if vn < 0.0 {
                    p.vel -= normal * vn * 2.0;
                    p.bounces += 1;
                }
            }

            let life = if p.kind == Item::Bone { 6.0 } else { 10.0 };
            let mut dead = p.age > life || p.bounces > 5;

            for (ki, k) in self.karts.iter_mut().enumerate() {
                if ki == p.owner as usize && p.age < 0.6 {
                    continue;
                }
                // Jumping clears thrown items and traps.
                if k.spin <= 0.0
                    && k.air < 1.4
                    && k.pos.distance(p.pos) < k.radius() + 0.7
                    && track.same_level(k.track_idx as usize, p.track_idx as usize)
                {
                    dead = true;
                    if k.hit() && ki != p.owner as usize {
                        cheers.push(p.owner as usize);
                    }
                    break;
                }
            }
            if let Some(ti) = self.traps.iter().position(|t| {
                t.flight <= 0.0 && t.pos.distance(p.pos) < 1.6 && track.same_level(t.track_idx as usize, p.track_idx as usize)
            })
            {
                self.spring(ti);
                dead = true;
            }
            if dead {
                remove.push(pi);
            }
            self.projectiles[pi] = p;
        }
        // projectile vs projectile (a frisbee flies over the rest)
        for a in 0..self.projectiles.len() {
            for b in a + 1..self.projectiles.len() {
                let (pa, pb) = (&self.projectiles[a], &self.projectiles[b]);
                if pa.kind != Item::Frisbee
                    && pb.kind != Item::Frisbee
                    && pa.pos.distance(pb.pos) < 1.4
                    && track.same_level(pa.track_idx as usize, pb.track_idx as usize)
                {
                    remove.extend([a, b]);
                }
            }
        }
        for i in cheers {
            self.cheer(i);
        }
        remove.sort_unstable();
        remove.dedup();
        for i in remove.into_iter().rev() {
            self.projectiles.remove(i);
        }
    }

    fn update_traps(&mut self, track: &Track) {
        let mut i = 0;
        while i < self.traps.len() {
            let t = &mut self.traps[i];
            t.age += DT;
            if t.flight > 0.0 {
                t.flight -= DT;
                t.pos += t.vel * DT;
                let tp = track.locate(t.pos, t.track_idx as usize);
                t.track_idx = tp.idx as u32;
                // Land on the road, not in the wall.
                let limit = WALL_DIST - 1.0;
                if tp.lateral.abs() > limit {
                    let normal = -tp.dir.perp() * tp.lateral.signum();
                    t.pos += normal * (tp.lateral.abs() - limit);
                    t.vel -= normal * t.vel.dot(normal).min(0.0);
                }
                if t.flight <= 0.0 {
                    t.flight = 0.0;
                    t.vel = Vec2::ZERO;
                }
                i += 1;
                continue;
            }
            let balloon = t.kind == Item::WaterBalloon;
            if balloon {
                t.fuse -= DT;
            }
            let t = &self.traps[i];
            let hit = self.karts.iter().enumerate().position(|(ki, k)| {
                !(ki == t.owner as usize && t.age < 0.8)
                    && k.spin <= 0.0
                    && k.air < 0.6
                    && k.pos.distance(t.pos) < k.radius() + 0.6
                    && track.same_level(k.track_idx as usize, t.track_idx as usize)
            });
            // A balloon goes off when it's touched, when its time is up, or when another bursts
            // beside it.
            let soaked = |b: &Burst| b.kind != Item::DogWhistle && b.pos.distance(t.pos) < BLAST_RADIUS && track.same_level(b.track_idx as usize, t.track_idx as usize);
            if balloon {
                if hit.is_some() || t.fuse <= 0.0 || self.bursts.iter().any(soaked) {
                    self.spring(i);
                } else {
                    i += 1;
                }
            } else if let Some(ki) = hit {
                let owner = t.owner as usize;
                if self.karts[ki].hit() && ki != owner {
                    self.cheer(owner);
                }
                self.traps.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn update_bursts(&mut self, track: &Track) {
        let mut cheers = vec![];
        for b in &mut self.bursts {
            b.age += DT;
            if b.kind == Item::DogWhistle || b.age > BURST_BITE {
                continue;
            }
            for (ki, k) in self.karts.iter_mut().enumerate() {
                let dist = k.pos.distance(b.pos) - k.radius();
                if dist > BLAST_RADIUS || !track.same_level(k.track_idx as usize, b.track_idx as usize) {
                    continue;
                }
                let done = if dist < BLAST_CORE { k.blast() } else { k.hit() };
                if done && ki != b.owner as usize {
                    cheers.push(b.owner as usize);
                }
            }
        }
        self.bursts.retain(|b| b.age < BURST_LIFE);
        for i in cheers {
            self.cheer(i);
        }
    }

    /// When a big woof lands, everyone ahead of its owner who's on the ground is shaken off
    /// the road. A hop at the right moment is enough to miss it.
    fn update_woofs(&mut self) {
        let mut landed = vec![];
        self.woofs.retain_mut(|w| {
            w.t += DT;
            if w.t >= WOOF_TIME {
                landed.push(w.owner as usize);
            }
            w.t < WOOF_TIME
        });
        for owner in landed {
            let Some(place) = self.karts.get(owner).map(|k| k.place) else { continue };
            let mut any = false;
            for k in self.karts.iter_mut().filter(|k| k.place < place) {
                if !k.airborne && k.air < 0.3 && k.hop <= 0.0 {
                    any |= k.shake();
                }
            }
            if any {
                self.cheer(owner);
            }
        }
    }
}

fn xorshift_f(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two karts parked on the grid after GO; kart `i` uses `item`, aimed `aim`.
    fn throw(track: &Track, i: usize, item: Item, aim: f32) -> World {
        let mut w = World::new(track, &[false, false], 1, SpeedClass::C100);
        w.time = COUNTDOWN + 1.0;
        w.karts[i].item = item;
        w.karts[i].item_count = 1;
        let mut inputs = [KartInput::default(); 2];
        inputs[i] = KartInput::new(0.0, 0.0, false, true, false).aimed(aim);
        w.step(track, &inputs);
        assert_eq!(w.karts[i].item, Item::None, "{item:?} wasn't used");
        w
    }

    #[test]
    fn bones_and_balls_go_the_way_they_are_aimed() {
        let track = Track::circuit();
        for item in [Item::Bone, Item::TennisBall] {
            for (aim, ahead) in [(0.0, true), (1.0, true), (-1.0, false)] {
                // Kart 1 starts second, so a ball thrown forwards homes in on kart 0.
                let w = throw(&track, 1, item, aim);
                let p = &w.projectiles[0];
                assert_eq!(p.vel.dot(w.karts[1].dir()) > 0.0, ahead, "{item:?} aimed {aim}");
                if item == Item::TennisBall {
                    assert_eq!(p.target, if ahead { 0 } else { -1 }, "ball aimed {aim}");
                }
            }
        }
    }

    #[test]
    fn squeaky_toy_drops_behind_or_lands_ahead() {
        let track = Track::circuit();
        for aim in [0.0, -1.0] {
            let w = throw(&track, 0, Item::SqueakyToy, aim);
            let t = &w.traps[0];
            assert!(t.flight == 0.0 && (t.pos - w.karts[0].pos).dot(w.karts[0].dir()) < 0.0, "aimed {aim}");
        }

        let mut w = throw(&track, 0, Item::SqueakyToy, 1.0);
        assert!(w.traps[0].flight > 0.0);
        let mut peak = 0.0f32;
        while w.traps[0].flight > 0.0 {
            peak = peak.max(w.traps[0].height());
            w.step(&track, &[KartInput::default(); 2]);
        }
        let (k, t) = (&w.karts[0], &w.traps[0]);
        let ahead = (t.pos - k.pos).dot(k.dir());
        assert!((12.0..30.0).contains(&ahead), "landed {ahead}m ahead");
        assert!(peak > 3.0, "peaked at {peak}m");
        assert_eq!(t.height(), 0.0);

        // Once it's down, it's a trap like any other.
        w.karts[1].pos = t.pos;
        w.step(&track, &[KartInput::default(); 2]);
        assert!(w.traps.is_empty() && w.karts[1].spin > 0.0);
    }

    #[test]
    fn hitting_someone_makes_the_thrower_cheer() {
        let track = Track::circuit();
        // Kart 1 throws a bone at kart 0, parked just up the road.
        let mut w = World::new(&track, &[false, false], 1, SpeedClass::C100);
        w.time = COUNTDOWN + 1.0;
        w.karts[0].pos = w.karts[1].pos + w.karts[1].dir() * 10.0;
        (w.karts[1].item, w.karts[1].item_count) = (Item::Bone, 1);
        let idle = KartInput::default();
        w.step(&track, &[idle, KartInput::new(0.0, 0.0, false, true, false).aimed(1.0)]);
        while w.karts[0].spin == 0.0 {
            assert!(!w.projectiles.is_empty() && w.karts[1].cheer == 0.0);
            w.step(&track, &[idle; 2]);
        }
        assert_eq!((w.karts[1].cheer, w.karts[0].cheer), (CHEER_TIME, 0.0));

        // Running over your own toy is nothing to cheer about.
        let mut w = throw(&track, 0, Item::SqueakyToy, 0.0);
        for _ in 0..60 {
            w.step(&track, &[KartInput::default(); 2]);
        }
        w.karts[0].pos = w.traps[0].pos;
        w.step(&track, &[KartInput::default(); 2]);
        assert!(w.karts[0].spin > 0.0 && w.karts[0].cheer == 0.0);
    }

    #[test]
    fn a_toy_in_the_air_is_not_hit() {
        let track = Track::circuit();
        let mut w = throw(&track, 0, Item::SqueakyToy, 1.0);
        w.karts[1].pos = w.traps[0].pos + w.traps[0].vel * DT;
        w.step(&track, &[KartInput::default(); 2]);
        assert_eq!(w.traps.len(), 1);
        assert_eq!(w.karts[1].spin, 0.0);
    }

    /// `n` karts parked on the grid after GO. Kart 0 is at the front.
    fn grid(track: &Track, n: usize) -> World {
        let mut w = World::new(track, &vec![false; n], 1, SpeedClass::C100);
        w.time = COUNTDOWN + 1.0;
        w
    }

    /// Park kart `i` on the first lap, `s` metres round it.
    fn put(w: &mut World, track: &Track, i: usize, s: f32) {
        let (c, d) = track.point_at(s);
        let mut k = Kart::new(track, c, d.to_angle(), false, 1.0, 1.0);
        (k.lap, k.checkpoint) = (1, track.sector(s) as u8);
        w.karts[i] = k;
    }

    fn give(w: &mut World, i: usize, item: Item, count: u8) {
        let k = &mut w.karts[i];
        (k.item, k.item_count, k.triple) = (item, count, count == 3);
    }

    fn press(w: &mut World, track: &Track, i: usize, aim: f32) {
        let mut inputs = vec![KartInput::default(); w.karts.len()];
        inputs[i] = KartInput::new(0.0, 0.0, false, true, false).aimed(aim);
        w.step(track, &inputs);
    }

    fn idle(w: &mut World, track: &Track, secs: f32) {
        let inputs = vec![KartInput::default(); w.karts.len()];
        for _ in 0..(secs / DT).round() as usize {
            w.step(track, &inputs);
        }
    }

    #[test]
    fn the_back_of_the_field_gets_the_strong_items() {
        let track = Track::circuit();
        let mut w = World::new(&track, &[true; 8], 7, SpeedClass::C100);
        w.time = COUNTDOWN + 60.0;
        let mut seen: Vec<Item> = vec![];
        let strong = [Item::Squirrel, Item::Zoomies, Item::GoldenTreat, Item::DogWhistle, Item::Frisbee];
        for place in [0u8, 7] {
            let rolls: Vec<_> = (0..4000).map(|_| w.roll_item(place)).collect();
            seen.extend(rolls.iter().map(|r| r.0));
            let share = rolls.iter().filter(|r| strong.contains(&r.0)).count() as f32 / rolls.len() as f32;
            println!("place {place}: {:.0}% strong items", share * 100.0);
            assert_eq!(share > 0.5, place == 7);
            if place == 0 {
                assert_eq!(share, 0.0, "the leader got a catch-up item");
            }
        }
        // Somewhere in the field, everything turns up.
        for place in 1..7 {
            seen.extend((0..4000).map(|_| w.roll_item(place).0));
        }
        for item in Item::ALL {
            assert!(seen.contains(&item), "{item:?} is never handed out");
        }
        // The far-reaching ones are held back at the start, and only one frisbee flies at a time.
        w.time = COUNTDOWN + 5.0;
        assert!((0..2000).all(|_| !matches!(w.roll_item(7).0, Item::Frisbee | Item::DogWhistle | Item::BigWoof)));
    }

    #[test]
    fn golden_treat_boosts_until_it_runs_out() {
        let track = Track::circuit();
        let mut w = grid(&track, 2);
        give(&mut w, 0, Item::GoldenTreat, 1);
        for _ in 0..3 {
            press(&mut w, &track, 0, 0.0);
            assert!(w.karts[0].boost > 0.9 && w.karts[0].item == Item::GoldenTreat);
            idle(&mut w, &track, 1.5);
        }
        idle(&mut w, &track, GOLDEN_TIME);
        assert_eq!((w.karts[0].item, w.karts[0].golden), (Item::None, 0.0));
    }

    #[test]
    fn water_balloon_soaks_everyone_near_it() {
        let track = Track::circuit();
        let mut w = grid(&track, 3);
        give(&mut w, 2, Item::WaterBalloon, 1);
        press(&mut w, &track, 2, 0.0);
        assert!(w.traps[0].flight > 0.0, "a balloon is thrown ahead unless it's aimed behind");
        while w.traps[0].flight > 0.0 {
            idle(&mut w, &track, DT);
        }
        // Kart 0 is right where it lands, kart 1 at the edge of the splash.
        let at = w.traps[0].pos;
        let dir = w.karts[2].dir();
        w.karts[0].pos = at + dir * 2.0;
        w.karts[1].pos = at - dir * (BLAST_CORE + KART_RADIUS + 1.0);
        w.karts[2].pos = at - dir * 40.0;
        idle(&mut w, &track, BALLOON_FUSE.0 + 0.1);
        assert!(w.traps.is_empty() && w.bursts.len() == 1);
        assert!(w.karts[0].spin > 0.0 && w.karts[0].airborne, "thrown into the air");
        assert!(w.karts[1].spin > 0.0 && !w.karts[1].airborne, "spun round");
        assert_eq!(w.karts[2].spin, 0.0);
        assert!(w.karts[2].cheer > 0.0);
        idle(&mut w, &track, 2.0);
        assert!(w.bursts.is_empty() && !w.karts[0].airborne);

        // Aimed behind, it's dropped and waits.
        give(&mut w, 2, Item::WaterBalloon, 1);
        press(&mut w, &track, 2, -1.0);
        assert_eq!(w.traps[0].flight, 0.0);
    }

    #[test]
    fn frisbee_finds_the_leader() {
        let track = Track::circuit();
        let mut w = grid(&track, 4);
        // The leader is a long way up the road, with someone else in between.
        put(&mut w, &track, 0, 300.0);
        put(&mut w, &track, 1, 150.0);
        idle(&mut w, &track, DT);
        assert_eq!((w.karts[0].place, w.karts[1].place), (0, 1));
        give(&mut w, 3, Item::Frisbee, 1);
        press(&mut w, &track, 3, 0.0);
        let mut flew = 0.0;
        while !w.projectiles.is_empty() && flew < 20.0 {
            idle(&mut w, &track, DT);
            flew += DT;
        }
        println!("frisbee flew for {flew:.1}s");
        assert!(w.karts[0].spin > 0.0 && w.karts[0].airborne, "the leader wasn't hit");
        assert_eq!(w.karts[1].spin, 0.0, "it flies over everyone else");
        assert!(flew < 8.0);
    }

    #[test]
    fn dog_whistle_shrinks_everyone_else() {
        let track = Track::circuit();
        let mut w = grid(&track, 4);
        give(&mut w, 3, Item::DogWhistle, 1);
        give(&mut w, 0, Item::Bone, 1);
        w.karts[1].zoom = 5.0;
        w.karts[2].big = 5.0;
        press(&mut w, &track, 3, 0.0);
        let k = &w.karts;
        assert!(k[0].small > 0.0 && k[0].spin > 0.0 && k[0].item == Item::None);
        assert!(k[1].small == 0.0 && k[1].spin == 0.0, "the zoomies shrug it off");
        assert!(k[2].small == 0.0 && k[2].big == 0.0, "a huge kart only goes back to normal");
        assert_eq!(k[3].small, 0.0);
        assert!(k[0].size() < 1.0);
        idle(&mut w, &track, 8.0);
        assert_eq!(w.karts[0].small, 0.0);
    }

    #[test]
    fn zoomies_and_sausage_plough_through() {
        let track = Track::circuit();
        for item in [Item::Zoomies, Item::Sausage, Item::Squirrel] {
            let mut w = grid(&track, 2);
            give(&mut w, 1, item, 1);
            press(&mut w, &track, 1, 0.0);
            // Nothing thrown at it does anything.
            let at = w.karts[1].pos;
            give(&mut w, 0, Item::Bone, 1);
            w.karts[0].pos = at + w.karts[1].dir() * 12.0;
            press(&mut w, &track, 0, -1.0);
            w.karts[1].pos = at;
            w.karts[1].speed = 0.0;
            w.karts[1].chase = w.karts[1].chase.min(0.3);
            idle(&mut w, &track, 0.4);
            assert!(w.projectiles.is_empty(), "{item:?}: the bone should have got there");
            assert_eq!(w.karts[1].spin, 0.0, "{item:?} was hit by a bone");

            // And driving into someone bowls them over.
            let mut w = grid(&track, 2);
            give(&mut w, 1, item, 1);
            press(&mut w, &track, 1, 0.0);
            w.karts[0].pos = w.karts[1].pos + w.karts[1].dir() * 2.0;
            idle(&mut w, &track, DT);
            let k = &w.karts[0];
            match item {
                Item::Sausage => assert!(k.flat > 0.0 && k.spin == 0.0),
                Item::Zoomies => assert!(k.spin > 0.0 && !k.airborne),
                _ => assert!(k.spin > 0.0 && k.airborne),
            }
            assert_eq!(w.karts[1].cheer, CHEER_TIME, "{item:?}");
        }
    }

    #[test]
    fn squirrel_chase_steers_itself_round_the_track() {
        let track = Track::circuit();
        let mut w = grid(&track, 2);
        give(&mut w, 1, Item::Squirrel, 1);
        press(&mut w, &track, 1, 0.0);
        w.karts[1].chase = 12.0;
        let from = w.karts[1].progress(&track);
        let mut worst = 0.0f32;
        for _ in 0..(11.0 / DT) as usize {
            idle(&mut w, &track, DT);
            worst = worst.max(w.karts[1].lateral.abs());
        }
        let went = w.karts[1].progress(&track) - from;
        println!("chased {went:.0}m, at most {worst:.1}m from the middle of the road");
        assert!(went > 450.0 && worst < 6.0);
        idle(&mut w, &track, 1.5);
        assert!(w.karts[1].chase == 0.0);
    }

    #[test]
    fn big_woof_shakes_everyone_ahead_on_the_ground() {
        let track = Track::circuit();
        let mut w = grid(&track, 4);
        give(&mut w, 2, Item::BigWoof, 1);
        give(&mut w, 0, Item::Treat, 3);
        press(&mut w, &track, 2, 0.0);
        idle(&mut w, &track, WOOF_TIME - 0.1);
        assert!(w.woofs.len() == 1 && w.karts.iter().all(|k| k.spin == 0.0));
        // Kart 1 hops just in time.
        let mut inputs = vec![KartInput::default(); 4];
        inputs[1] = KartInput::new(0.0, 0.0, true, false, false);
        for _ in 0..8 {
            w.step(&track, &inputs);
        }
        assert!(w.woofs.is_empty());
        assert!(w.karts[0].spin > 0.0 && w.karts[0].item == Item::None);
        assert_eq!(w.karts[1].spin, 0.0, "a hop dodges it");
        assert!(w.karts[2].spin == 0.0 && w.karts[3].spin == 0.0, "only those ahead");
    }

    #[test]
    fn mud_pie_only_splatters_those_ahead() {
        let track = Track::circuit();
        let mut w = grid(&track, 4);
        give(&mut w, 2, Item::MudPie, 1);
        press(&mut w, &track, 2, 0.0);
        let mud: Vec<bool> = w.karts.iter().map(|k| k.mud > 0.0).collect();
        assert_eq!(mud, [true, true, false, false]);
        idle(&mut w, &track, MUD_TIME);
        assert!(w.karts.iter().all(|k| k.mud == 0.0));
    }

    #[test]
    fn rain_cloud_is_passed_on_by_a_bump() {
        let track = Track::circuit();
        let mut w = grid(&track, 3);
        give(&mut w, 1, Item::RainCloud, 1);
        idle(&mut w, &track, DT);
        assert!(w.karts[1].cloud > 0.0 && w.karts[1].item == Item::None, "it starts by itself");
        idle(&mut w, &track, 1.0);
        w.karts[2].pos = w.karts[1].pos + Vec2::new(0.5, 0.5);
        idle(&mut w, &track, 3.0 * DT);
        assert!(w.karts[1].cloud == 0.0 && w.karts[2].cloud > 0.0);
        assert!(w.karts[2].cloud < CLOUD_TIME - 1.0, "the clock keeps running");
        // Whoever has it when the time's up gets drenched.
        w.karts[2].pos += Vec2::new(8.0, 0.0);
        idle(&mut w, &track, CLOUD_TIME);
        assert!(w.karts[1].small == 0.0 && w.karts[1].spin == 0.0);
        assert!(w.karts[2].cloud == 0.0);
        let mut w2 = grid(&track, 2);
        w2.karts[0].cloud = 0.05;
        idle(&mut w2, &track, 0.1);
        assert!(w2.karts[0].small > 0.0 && w2.karts[0].spin > 0.0);
    }

    #[test]
    fn three_bones_circle_the_kart_as_a_shield() {
        let track = Track::circuit();
        let mut w = grid(&track, 2);
        give(&mut w, 0, Item::Bone, 3);
        assert_eq!(w.karts[0].orbiting(), 3);
        // Kart 1 throws a bone at kart 0, parked just up the road: one of the three stops it.
        w.karts[0].pos = w.karts[1].pos + w.karts[1].dir() * 14.0;
        give(&mut w, 1, Item::Bone, 1);
        press(&mut w, &track, 1, 1.0);
        idle(&mut w, &track, 1.0);
        assert!(w.projectiles.is_empty());
        assert_eq!((w.karts[0].spin, w.karts[0].item_count), (0.0, 2));
        // Anyone who drives into the rest is hit.
        w.karts[1].pos = w.karts[0].pos + w.karts[0].dir() * (ORBIT_RADIUS + KART_RADIUS);
        idle(&mut w, &track, 1.2);
        assert!(w.karts[1].spin > 0.0 && w.karts[0].item_count == 1);
        // Throwing uses them one at a time.
        let mut w = grid(&track, 2);
        give(&mut w, 1, Item::TennisBall, 3);
        press(&mut w, &track, 1, 1.0);
        assert_eq!((w.projectiles.len(), w.karts[1].orbiting()), (1, 2));
    }

    #[test]
    fn decoy_box_is_a_trap() {
        let track = Track::circuit();
        let mut w = throw(&track, 0, Item::DecoyBox, 0.0);
        assert_eq!((w.traps[0].kind, w.traps[0].flight), (Item::DecoyBox, 0.0));
        w.karts[1].pos = w.traps[0].pos;
        w.step(&track, &[KartInput::default(); 2]);
        assert!(w.traps.is_empty() && w.karts[1].spin > 0.0);
    }
}
