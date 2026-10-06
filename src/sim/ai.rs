//! CPU racers. Stateless: everything is derived from the world each tick, so the host can also
//! use it to drive karts of players who finished or disconnected.

use super::kart::{drift_stage, Item, KartInput, TrickTry, BTN_DRIFT, BTN_ITEM, BTN_TRICK};
use super::track::Track;
use super::{wrap_angle, World, COUNTDOWN};

fn hash(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9) ^ b.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65536.0
}

pub fn bot_input(w: &World, track: &Track, i: usize) -> KartInput {
    let k = &w.karts[i];
    if w.time < COUNTDOWN {
        // Most bots attempt a rocket start.
        let go = COUNTDOWN - w.time < 0.45 && hash(i as u32, 7) < 0.7;
        return KartInput::new(0.0, if go { 1.0 } else { 0.0 }, false, false, false);
    }

    let speed = k.speed.max(0.0);
    let curve = track.turn_between(k.s + 5.0, k.s + 45.0);
    let mut wobble = (w.time * 0.23 + i as f32 * 1.7).sin() * 4.0;
    // With mud in its eyes a corgi weaves about.
    if k.mud > 0.0 {
        wobble += (w.time * 2.1 + i as f32).sin() * 4.0;
    }
    let mut lat_target = (wobble + curve.clamp(-1.0, 1.0) * 5.0).clamp(-6.5, 6.5);
    // Line up for a ramp ahead, more keenly for a boosted one.
    for (ri, r) in track.ramps.iter().enumerate() {
        let ahead = (r.s - k.s).rem_euclid(track.length);
        let keen = if r.boosted { 0.75 } else { 0.45 };
        if (8.0..70.0).contains(&ahead) && hash(i as u32 * 13 + ri as u32, k.lap as u32) < keen {
            lat_target = r.lateral;
        }
    }
    let look = 10.0 + speed * 0.5;
    let (c, d) = track.point_at(k.s + look);
    let target = c + d.perp() * lat_target;
    let err = wrap_angle((target - k.pos).to_angle() - k.heading);
    let mut steer = (err * 2.8).clamp(-1.0, 1.0);
    let mut throttle = if err.abs() > 1.2 && speed > 10.0 { 0.3 } else { 1.0 };

    // Facing the wrong way at low speed: back up while turning.
    if err.abs() > 2.2 && speed < 4.0 {
        throttle = -1.0;
        steer = -steer;
    }

    // Drift through sustained corners and cash in the turbo when the corner opens up.
    let mut drift = false;
    if k.drift_dir != 0 {
        let still_turning = track.turn_between(k.s, k.s + 25.0) * k.drift_dir as f32 > 0.2;
        drift = still_turning && drift_stage(k.drift_charge) < 3 && !k.offroad;
        // Pure pursuit: pick the steer whose drift arc passes through the target point.
        let d = k.drift_dir as f32;
        let want_curv = 2.0 * err.sin() / look;
        steer = d * crate::sim::kart::drift_steer_for(want_curv * d);
    } else if k.drift_pending {
        drift = true;
        steer = curve.signum();
    } else if speed > 17.0 && curve.abs() > 0.7 && k.prev_buttons & BTN_DRIFT == 0 && hash(i as u32, 3) < 0.85 {
        drift = true;
        steer = curve.signum();
    }

    // Most of the time, trick right at the lip. Don't hop on a ramp: that would waste the try.
    let near_lip = k.trick_timing(track);
    if near_lip.is_some() {
        drift = drift && k.drift_dir != 0;
    }
    let trick = k.trick_try == TrickTry::Ready
        && k.prev_buttons & BTN_TRICK == 0
        && near_lip == Some(TrickTry::Done)
        && hash(i as u32 * 7 + k.lap as u32, (k.s / 50.0) as u32) < 0.8;

    let aim = if k.item != Item::None && k.roulette <= 0.0 && k.prev_buttons & BTN_ITEM == 0 { wants_item(w, track, i, curve) } else { None };

    KartInput::new(steer, throttle, drift, aim.is_some(), trick).aimed(aim.unwrap_or(0.0))
}

/// Whether kart `i` uses its item this tick, and which way it aims it (1 ahead, -1 behind).
fn wants_item(w: &World, track: &Track, i: usize, curve: f32) -> Option<f32> {
    let k = &w.karts[i];
    let roll = hash(i as u32 * 31 + 1, w.tick / 20);
    let tick_chance = |p: f32| hash(i as u32, w.tick) < p;
    let dist_ahead = |o: &crate::sim::Kart| o.progress(track) - k.progress(track);
    // Someone lined up in front, or close behind.
    let in_sights = |near: f32, far: f32| {
        let dir = k.dir();
        w.karts.iter().enumerate().any(|(j, o)| {
            j != i && {
                let to = o.pos - k.pos;
                let dist = to.length();
                dist < far && dist > near && to.normalize().dot(dir) > 0.97
            }
        })
    };
    let chased = w.karts.iter().enumerate().any(|(j, o)| j != i && (2.0..20.0).contains(&-dist_ahead(o)));
    let straight = curve.abs() < 0.35;
    let yes = |b: bool| b.then_some(0.0);
    match k.item {
        Item::None | Item::RainCloud => None,
        Item::Treat => yes(straight && roll < 0.5),
        // Keep eating while it lasts.
        Item::GoldenTreat => yes(k.golden > 0.0 || straight),
        Item::Bone => yes(in_sights(3.0, 45.0) || tick_chance(1.0 / if k.triple { 150.0 } else { 400.0 })),
        Item::TennisBall => yes(k.place > 0 && tick_chance(1.0 / 90.0)),
        Item::SqueakyToy | Item::DecoyBox => yes(chased || tick_chance(1.0 / 500.0)),
        Item::WaterBalloon => {
            if in_sights(20.0, 50.0) {
                Some(1.0)
            } else {
                (chased && tick_chance(1.0 / 20.0)).then_some(-1.0)
            }
        }
        Item::Frisbee | Item::BigWoof | Item::MudPie => yes(tick_chance(1.0 / if k.place > 0 { 90.0 } else { 600.0 })),
        Item::DogWhistle => yes(tick_chance(1.0 / 120.0)),
        Item::Zoomies | Item::Sausage | Item::Squirrel => yes(tick_chance(1.0 / 40.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{DT, LAPS};

    #[test]
    fn bots_finish_race() {
        for id in crate::sim::TrackId::ALL {
            for class in crate::sim::SpeedClass::ALL {
                println!("{} {}", id.name(), class.name());
                bots_race(&Track::new(id), class, true);
            }
        }
    }

    #[test]
    fn bots_finish_custom_tracks() {
        for map in crate::sim::custom::tests::sample_maps() {
            println!("{}", map.name);
            let track = crate::sim::custom::build(&map, crate::sim::TrackId::Custom(map.hash())).track;
            bots_race(&track, crate::sim::SpeedClass::C150, false);
        }
    }

    fn bots_race(track: &Track, class: crate::sim::SpeedClass, drifty: bool) {
        let mut w = World::new(track, &[true; 8], 1234, class);
        let mut max_stage = 0;
        let mut items_used = 0;
        while !w.karts.iter().all(|k| k.finished) && w.time < 600.0 {
            let inputs: Vec<_> = (0..w.karts.len()).map(|i| bot_input(&w, track, i)).collect();
            items_used += inputs.iter().filter(|i| i.item()).count();
            for k in &w.karts {
                max_stage = max_stage.max(drift_stage(k.drift_charge));
            }
            w.step(track, &inputs);
        }
        for (i, k) in w.karts.iter().enumerate() {
            println!("kart {i}: lap {} finished {} time {:.1} place {}", k.lap, k.finished, k.finish_time, k.place);
        }
        println!("max drift stage {max_stage}, items used {items_used}, sim time {:.1}s ({} ticks)", w.time, (w.time / DT) as u32);
        assert!(w.karts.iter().all(|k| k.finished && k.lap > LAPS));
        assert!(!drifty || max_stage >= 1, "bots never charged a mini-turbo");
    }
}
