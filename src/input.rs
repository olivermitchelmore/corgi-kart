//! Keyboard + gamepad input.
//!
//! Alone, every key and gamepad drives your kart. In split screen player 1 has WASD, player 2
//! the arrow keys, and gamepads are shared out (see `pad_player`).
//!
//! Driving: W accelerates, S brakes/reverses, A/D steer, Space/Shift drift, E/F/Ctrl use item,
//! Q trick. Drift also tricks at a ramp's lip. Alone, the arrow keys don't drive: hold Up or
//! Down while using an item to throw it forwards or backwards. In split screen player 2 drives
//! with them instead, and keyboard players throw items the default way.
//!
//! Gamepads use the usual kart-racer layout of a Switch Pro controller, by position, so an Xbox pad's buttons
//! sit where the Switch ones do: the right face button (Xbox B, Switch A) accelerates, the
//! bottom one (Xbox A, Switch B) brakes and reverses, either right shoulder (RB/RT) drifts,
//! either left shoulder (LB/LT) uses the item, aimed forwards or backwards by holding the left
//! stick up or down. Stick or d-pad steers; the top and left face buttons, d-pad up or a flick
//! of the right stick trick. In menus the left stick or d-pad moves, the right face button
//! confirms and the bottom one goes back.

use bevy::prelude::*;

use crate::sim::KartInput;

/// The keys that drive one player.
struct KeySet {
    left: &'static [KeyCode],
    right: &'static [KeyCode],
    go: &'static [KeyCode],
    brake: &'static [KeyCode],
    drift: &'static [KeyCode],
    item: &'static [KeyCode],
    trick: &'static [KeyCode],
    aim_forward: &'static [KeyCode],
    aim_back: &'static [KeyCode],
}

/// Playing alone, WASD drives and the arrow keys aim items.
const ALL_KEYS: KeySet = KeySet {
    left: &[KeyCode::KeyA],
    right: &[KeyCode::KeyD],
    go: &[KeyCode::KeyW],
    brake: &[KeyCode::KeyS],
    drift: &[KeyCode::Space, KeyCode::ShiftLeft, KeyCode::ShiftRight],
    item: &[KeyCode::KeyE, KeyCode::KeyF, KeyCode::ControlLeft, KeyCode::ControlRight],
    trick: &[KeyCode::KeyQ, KeyCode::AltRight],
    aim_forward: &[KeyCode::ArrowUp],
    aim_back: &[KeyCode::ArrowDown],
};

/// In split screen the keyboard is shared: player 1 takes the left side of it...
const LEFT_KEYS: KeySet = KeySet {
    left: &[KeyCode::KeyA],
    right: &[KeyCode::KeyD],
    go: &[KeyCode::KeyW],
    brake: &[KeyCode::KeyS],
    drift: &[KeyCode::Space, KeyCode::ShiftLeft],
    item: &[KeyCode::KeyE, KeyCode::KeyF, KeyCode::ControlLeft],
    trick: &[KeyCode::KeyQ],
    aim_forward: &[],
    aim_back: &[],
};

/// ...and player 2 the arrow keys.
const RIGHT_KEYS: KeySet = KeySet {
    left: &[KeyCode::ArrowLeft],
    right: &[KeyCode::ArrowRight],
    go: &[KeyCode::ArrowUp],
    brake: &[KeyCode::ArrowDown],
    drift: &[KeyCode::ShiftRight],
    item: &[KeyCode::ControlRight, KeyCode::Enter, KeyCode::NumpadEnter],
    trick: &[KeyCode::AltRight, KeyCode::Slash],
    aim_forward: &[],
    aim_back: &[],
};

fn key_set(player: usize, players: usize) -> Option<&'static KeySet> {
    match (player, players) {
        (_, 1) => Some(&ALL_KEYS),
        (0, _) => Some(&LEFT_KEYS),
        (1, _) => Some(&RIGHT_KEYS),
        _ => None,
    }
}

/// Which player drives with gamepad `pad` (in connection order) when `players` share `pads`
/// gamepads. Pads go to the last players first: with one pad and two players, player 1 keeps
/// WASD and player 2 gets the pad as well as the arrow keys. Spare pads go unused.
pub fn pad_player(pad: usize, pads: usize, players: usize) -> Option<usize> {
    if players <= 1 {
        return Some(0);
    }
    let p = pad + players.saturating_sub(pads);
    (p < players).then_some(p)
}

/// Gamepads in a stable order (the order they connected).
fn sorted_pads<'a>(pads: &'a Query<(Entity, &Gamepad)>) -> Vec<&'a Gamepad> {
    let mut pads: Vec<_> = pads.iter().collect();
    pads.sort_by_key(|(e, _)| *e);
    pads.into_iter().map(|(_, g)| g).collect()
}

/// This frame's input for each of `players` local players.
pub fn read_kart_inputs(keys: &ButtonInput<KeyCode>, pads: &Query<(Entity, &Gamepad)>, players: usize) -> Vec<KartInput> {
    let pads = sorted_pads(pads);
    (0..players.max(1))
        .map(|player| {
            let mine = pads.iter().enumerate().filter(|&(i, _)| pad_player(i, pads.len(), players) == Some(player)).map(|(_, p)| *p);
            read_player(keys, key_set(player, players), mine)
        })
        .collect()
}

fn read_player<'a>(keys: &ButtonInput<KeyCode>, set: Option<&KeySet>, pads: impl Iterator<Item = &'a Gamepad>) -> KartInput {
    let any = |codes: &[KeyCode]| codes.iter().any(|&c| keys.pressed(c));
    let (mut steer, mut throttle, mut drift, mut item, mut trick, mut aim) = (0.0, 0.0, false, false, false, 0.0);
    if let Some(set) = set {
        if any(set.left) {
            steer -= 1.0;
        }
        if any(set.right) {
            steer += 1.0;
        }
        if any(set.go) {
            throttle += 1.0;
        }
        if any(set.brake) {
            throttle -= 1.0;
        }
        drift = any(set.drift);
        item = any(set.item);
        trick = any(set.trick);
        if any(set.aim_forward) {
            aim += 1.0;
        }
        if any(set.aim_back) {
            aim -= 1.0;
        }
    }

    for pad in pads {
        let stick = pad.left_stick();
        if stick.x.abs() > 0.15 {
            steer += stick.x;
        }
        if stick.y.abs() > AIM_DEADZONE {
            aim += stick.y.signum();
        }
        if pad.pressed(GamepadButton::DPadLeft) {
            steer -= 1.0;
        }
        if pad.pressed(GamepadButton::DPadRight) {
            steer += 1.0;
        }
        if pad.pressed(GamepadButton::East) {
            throttle += 1.0;
        }
        if pad.pressed(GamepadButton::South) {
            throttle -= 1.0;
        }
        // The triggers are buttons here; a light squeeze is enough.
        let squeezed = |b| pad.get(b).unwrap_or(0.0) > 0.3;
        drift |= pad.pressed(GamepadButton::RightTrigger) || squeezed(GamepadButton::RightTrigger2);
        item |= pad.pressed(GamepadButton::LeftTrigger) || squeezed(GamepadButton::LeftTrigger2);
        let flick = Vec2::new(pad.get(GamepadAxis::RightStickX).unwrap_or(0.0), pad.get(GamepadAxis::RightStickY).unwrap_or(0.0));
        trick |= flick.length() > 0.6
            || pad.pressed(GamepadButton::DPadUp)
            || pad.pressed(GamepadButton::North)
            || pad.pressed(GamepadButton::West);
    }
    KartInput::new(steer, throttle, drift, item, trick).aimed(aim)
}

/// How far up or down the left stick has to be held to aim an item.
const AIM_DEADZONE: f32 = 0.5;

/// How a split-screen player drives, for their HUD during the countdown.
pub fn controls_hint(player: usize, players: usize, pads: usize) -> String {
    if players <= 1 {
        return "W: go    A D: steer    SPACE: drift (hold, release to boost)    E: use item (hold Up / Down to aim)    Q: trick off a ramp".into();
    }
    let pad = (0..pads).any(|i| pad_player(i, pads, players) == Some(player));
    let keys = match player {
        0 => Some("W: go   A D: steer   SPACE: drift   E: item   Q: trick"),
        1 => Some("Up: go   Left Right: steer   R-SHIFT: drift   R-CTRL / ENTER: item   R-ALT or /: trick"),
        _ => None,
    };
    match (keys, pad) {
        (Some(k), true) => format!("{k}   (or your gamepad)"),
        (Some(k), false) => k.into(),
        (None, true) => "Gamepad:  right button go   bottom button brake   stick steer   R drift   L item   top button trick".into(),
        (None, false) => "Connect a gamepad to drive!".into(),
    }
}

/// A direction the left stick points in menus.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StickDir {
    Up,
    Down,
    Left,
    Right,
}

/// Menu navigation from the left stick of whichever gamepad is pushed furthest. Pushing it fires
/// once, and holding it repeats like a held key.
#[derive(Resource, Default)]
pub struct StickNav {
    held: Option<StickDir>,
    held_for: f32,
    /// The direction to move this frame, if any.
    fired: Option<StickDir>,
}

/// How long a stick is held before it repeats, and then how often.
const REPEAT_DELAY: f32 = 0.4;
const REPEAT_EVERY: f32 = 0.12;

impl StickNav {
    fn update(&mut self, dir: Option<StickDir>, dt: f32) {
        if dir != self.held {
            self.held = dir;
            self.held_for = 0.0;
            self.fired = dir;
            return;
        }
        let repeats = |t: f32| if t < REPEAT_DELAY { 0 } else { 1 + ((t - REPEAT_DELAY) / REPEAT_EVERY) as u32 };
        let before = repeats(self.held_for);
        self.held_for += dt;
        self.fired = dir.filter(|_| repeats(self.held_for) > before);
    }
}

/// Which way `stick` points, if clearly one way. It has to be pushed well over to start, but
/// only stay over a little to keep going, so a wobble at the edge doesn't fire twice.
fn stick_dir(stick: Vec2, held: Option<StickDir>) -> Option<StickDir> {
    let along = |d| match d {
        StickDir::Up => stick.y,
        StickDir::Down => -stick.y,
        StickDir::Left => -stick.x,
        StickDir::Right => stick.x,
    };
    if held.is_some_and(|d| along(d) > 0.35) {
        return held;
    }
    let d = if stick.y.abs() > stick.x.abs() {
        if stick.y > 0.0 { StickDir::Up } else { StickDir::Down }
    } else if stick.x > 0.0 {
        StickDir::Right
    } else {
        StickDir::Left
    };
    (along(d) > 0.6).then_some(d)
}

pub fn stick_nav(time: Res<Time>, pads: Query<&Gamepad>, mut nav: ResMut<StickNav>) {
    let stick = pads.iter().map(|p| p.left_stick()).max_by(|a, b| a.length_squared().total_cmp(&b.length_squared())).unwrap_or_default();
    let dir = stick_dir(stick, nav.held);
    nav.update(dir, time.delta_secs());
}

/// Menu navigation from keyboard or any gamepad, edge-triggered.
#[derive(Default, Clone, Copy)]
pub struct Nav {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub confirm: bool,
    pub back: bool,
}

pub fn read_nav(keys: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>, stick: &StickNav) -> Nav {
    let k = |c: &[KeyCode]| c.iter().any(|&c| keys.just_pressed(c));
    let p = |b: GamepadButton| pads.iter().any(|g| g.just_pressed(b));
    let s = |d: StickDir| stick.fired == Some(d);
    Nav {
        up: k(&[KeyCode::ArrowUp]) || p(GamepadButton::DPadUp) || s(StickDir::Up),
        down: k(&[KeyCode::ArrowDown, KeyCode::Tab]) || p(GamepadButton::DPadDown) || s(StickDir::Down),
        left: k(&[KeyCode::ArrowLeft]) || p(GamepadButton::DPadLeft) || s(StickDir::Left),
        right: k(&[KeyCode::ArrowRight]) || p(GamepadButton::DPadRight) || s(StickDir::Right),
        confirm: k(&[KeyCode::Enter, KeyCode::NumpadEnter]) || p(GamepadButton::East) || p(GamepadButton::Start),
        back: k(&[KeyCode::Escape]) || p(GamepadButton::South) || p(GamepadButton::Select),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamepads_go_to_the_last_players_first() {
        let owners = |pads, players| (0..pads).map(|i| pad_player(i, pads, players)).collect::<Vec<_>>();
        assert_eq!(owners(2, 1), [Some(0), Some(0)]);
        assert_eq!(owners(1, 2), [Some(1)]);
        assert_eq!(owners(2, 2), [Some(0), Some(1)]);
        assert_eq!(owners(3, 2), [Some(0), Some(1), None]);
        assert_eq!(owners(2, 4), [Some(2), Some(3)]);
        assert_eq!(owners(4, 4), [Some(0), Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn arrow_keys_aim_items_alone_but_drive_in_split_screen() {
        let mut keys = ButtonInput::<KeyCode>::default();
        for k in [KeyCode::ArrowUp, KeyCode::ArrowLeft, KeyCode::KeyE] {
            keys.press(k);
        }
        let alone = read_player(&keys, key_set(0, 1), std::iter::empty());
        assert_eq!((alone.steer(), alone.throttle(), alone.item(), alone.aim()), (0.0, 0.0, true, 1));
        let p2 = read_player(&keys, key_set(1, 2), std::iter::empty());
        assert_eq!((p2.steer(), p2.throttle(), p2.aim()), (-1.0, 1.0, 0));

        keys.release(KeyCode::ArrowUp);
        keys.press(KeyCode::ArrowDown);
        assert_eq!(read_player(&keys, key_set(0, 1), std::iter::empty()).aim(), -1);
    }

    #[test]
    fn stick_fires_once_then_repeats_while_held() {
        let mut nav = StickNav::default();
        let fired = |nav: &mut StickNav, stick: Vec2, secs: f32| {
            let mut n = 0;
            for _ in 0..(secs * 100.0).round() as usize {
                let dir = stick_dir(stick, nav.held);
                nav.update(dir, 0.01);
                n += nav.fired.is_some() as usize;
            }
            n
        };
        // A quick push moves once; letting go moves nowhere.
        assert_eq!(fired(&mut nav, Vec2::new(0.1, -0.9), 0.2), 1);
        assert_eq!(nav.held, Some(StickDir::Down));
        assert_eq!(fired(&mut nav, Vec2::ZERO, 0.5), 0);
        // Half-pushed doesn't count, and easing back while held doesn't fire again.
        assert_eq!(fired(&mut nav, Vec2::new(0.45, 0.0), 0.2), 0);
        assert_eq!(fired(&mut nav, Vec2::new(0.9, 0.0), 0.1), 1);
        assert_eq!(fired(&mut nav, Vec2::new(0.45, 0.0), 0.1), 0);
        assert_eq!(fired(&mut nav, Vec2::ZERO, 0.1), 0);
        // Held for most of a second: once, then after 0.4s, 0.52s, 0.64s, 0.76s and 0.88s.
        assert_eq!(fired(&mut nav, Vec2::new(0.0, 1.0), 0.95), 6);
    }
}
