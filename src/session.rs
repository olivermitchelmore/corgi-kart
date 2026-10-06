//! Game session: solo, LAN host or LAN client. Owns the socket and the running race, advances
//! the simulation at a fixed tick rate, and does client-side prediction + reconciliation.
//!
//! Kept free of Bevy ECS so it can be driven directly from tests.

use std::collections::VecDeque;
use std::net::SocketAddr;

use crate::net::{
    ClientMsg, HostMsg, RacerInfo, Socket, INPUT_REDUNDANCY, PROTOCOL_VERSION, TIMEOUT_SECS,
};
use crate::sim::{ai::bot_input, custom, Item, KartInput, SpeedClass, Track, TrackId, World, COUNTDOWN, DT, MAX_KARTS};

/// Longest name shown for a racer a LAN peer names: 16 for the name, and " (left)".
const MAX_NAME: usize = 24;
/// Most inputs kept waiting for a client. `next_input` skips ahead beyond 4 anyway.
const MAX_QUEUE: usize = 16;

const BOT_NAMES: &[&str] = &["Biscuit", "Waffles", "Pudding", "Nugget", "Mochi", "Peanut", "Toast"];

/// Most players one machine can seat in split screen.
pub const MAX_LOCAL_PLAYERS: usize = 4;

pub struct RaceState {
    pub race_id: u32,
    pub class: SpeedClass,
    pub track: Track,
    pub world: World,
    /// World as of the previous tick, for render interpolation.
    pub prev: World,
    pub roster: Vec<RacerInfo>,
    /// The karts driven on this machine, one per split-screen player (player 1's first).
    pub locals: Vec<usize>,
    acc: f32,
    last_snapshot_tick: u32,
}

impl RaceState {
    fn new(track: TrackId, class: SpeedClass, race_id: u32, seed: u32, roster: Vec<RacerInfo>, locals: Vec<usize>) -> Self {
        let track = Track::new(track);
        let bots: Vec<bool> = roster.iter().map(|r| r.bot).collect();
        let world = World::new(&track, &bots, seed, class);
        RaceState { race_id, class, track, prev: world.clone(), world, roster, locals, acc: 0.0, last_snapshot_tick: 0 }
    }

    /// Fraction of a tick elapsed since the last one, for smooth rendering.
    pub fn alpha(&self) -> f32 {
        (self.acc / DT).clamp(0.0, 1.0)
    }
}

pub struct RemoteClient {
    pub addr: SocketAddr,
    pub name: String,
    pub kart: Option<usize>,
    last_heard: f64,
    queue: VecDeque<(u32, KartInput)>,
    last_recv_seq: u32,
    applied_seq: u32,
    last_input: KartInput,
}

impl RemoteClient {
    fn next_input(&mut self) -> KartInput {
        // If the client's clock runs fast the queue grows; skip ahead to keep latency low.
        while self.queue.len() > 4 {
            self.queue.pop_front();
        }
        if let Some((seq, input)) = self.queue.pop_front() {
            self.applied_seq = seq;
            self.last_input = input;
        }
        self.last_input
    }
}

pub struct Host {
    sock: Socket,
    pub port: u16,
    pub clients: Vec<RemoteClient>,
    lobby_timer: f64,
    info_timer: f64,
    next_race_id: u32,
}

pub struct Client {
    sock: Socket,
    pub host: SocketAddr,
    pub connected: bool,
    started: f64,
    last_heard: f64,
    send_timer: f64,
    pub lobby_players: Vec<String>,
    pub lobby_bots: u8,
    seq: u32,
    history: VecDeque<(u32, KartInput)>,
}

pub enum Role {
    Solo,
    Host(Host),
    Client(Client),
}

pub enum SessionEvent {
    RaceStarted,
    BackToLobby,
    Disconnected(String),
}

pub struct Session {
    pub role: Role,
    pub name: String,
    /// Split-screen players on this machine (solo only: LAN games seat one per machine).
    pub players: u8,
    pub bots: u8,
    /// Track and engine class for the next race. Chosen by the host (or solo player); clients
    /// are told them.
    pub track: TrackId,
    pub class: SpeedClass,
    pub race: Option<RaceState>,
    /// Let the AI drive the local kart (for testing).
    pub autodrive: bool,
    /// Keep handing this item to the karts driven here, and this to the CPU racers (for testing).
    pub test_items: [Option<(Item, u8)>; 2],
    now: f64,
    solo_race_id: u32,
}

impl Session {
    fn with_role(role: Role, name: &str, bots: u8, track: TrackId, class: SpeedClass) -> Self {
        Session {
            role,
            name: name.to_string(),
            players: 1,
            bots,
            track,
            class,
            race: None,
            autodrive: false,
            test_items: [None; 2],
            now: 0.0,
            solo_race_id: 0,
        }
    }

    pub fn solo(name: &str, players: u8, bots: u8, track: TrackId, class: SpeedClass) -> Self {
        let mut s = Self::with_role(Role::Solo, name, bots, track, class);
        s.players = players.clamp(1, MAX_LOCAL_PLAYERS as u8);
        s
    }

    /// Names of the players on this machine: yours, then "Player 2" and so on.
    pub fn local_names(&self) -> Vec<String> {
        let n = if matches!(self.role, Role::Solo) { self.players.max(1) } else { 1 };
        std::iter::once(self.name.clone()).chain((2..=n).map(|p| format!("Player {p}"))).collect()
    }

    pub fn host(name: &str, bots: u8, port: u16, track: TrackId, class: SpeedClass) -> std::io::Result<Self> {
        let sock = Socket::bind(port)?;
        let host = Host { sock, port, clients: vec![], lobby_timer: 0.0, info_timer: 0.0, next_race_id: 1 };
        Ok(Self::with_role(Role::Host(host), name, bots, track, class))
    }

    pub fn join(name: &str, addr: SocketAddr) -> std::io::Result<Self> {
        let sock = Socket::bind(0)?;
        let client = Client {
            sock,
            host: addr,
            connected: false,
            started: 0.0,
            last_heard: 0.0,
            send_timer: 0.0,
            lobby_players: vec![],
            lobby_bots: 0,
            seq: 0,
            history: VecDeque::new(),
        };
        Ok(Self::with_role(Role::Client(client), name, 0, TrackId::default(), SpeedClass::default()))
    }

    /// Humans that will race: the local player plus connected clients.
    pub fn human_names(&self) -> Vec<String> {
        match &self.role {
            Role::Client(c) => c.lobby_players.clone(),
            Role::Host(h) => std::iter::once(self.name.clone()).chain(h.clients.iter().map(|c| c.name.clone())).collect(),
            Role::Solo => self.local_names(),
        }
    }

    pub fn max_bots(&self) -> u8 {
        (MAX_KARTS - self.human_names().len().min(MAX_KARTS)) as u8
    }

    /// Host / solo: start a new race with the current players.
    pub fn start_race(&mut self) {
        self.bots = self.bots.min(self.max_bots());
        let seed = (self.now * 1000.0) as u32 ^ 0x5eed_c0de;
        // Bots take the front of the grid; humans start at the back.
        let mut roster: Vec<RacerInfo> = (0..self.bots as usize)
            .map(|i| RacerInfo { name: BOT_NAMES[i % BOT_NAMES.len()].to_string(), bot: true })
            .collect();
        let first = roster.len();
        roster.extend(self.local_names().into_iter().map(|name| RacerInfo { name, bot: false }));
        let locals: Vec<usize> = (first..roster.len()).collect();

        let race_id = match &mut self.role {
            Role::Client(_) => return,
            Role::Solo => {
                self.solo_race_id += 1;
                self.solo_race_id
            }
            Role::Host(h) => {
                for c in &mut h.clients {
                    c.kart = Some(roster.len());
                    c.queue.clear();
                    roster.push(RacerInfo { name: c.name.clone(), bot: false });
                }
                let id = h.next_race_id;
                h.next_race_id += 1;
                h.info_timer = 0.0;
                id
            }
        };
        self.race = Some(RaceState::new(self.track, self.class, race_id, seed, roster, locals));
        if let Role::Host(h) = &mut self.role {
            h.send_race_info(self.race.as_ref().unwrap(), seed);
        }
    }

    /// Host: finish the race and return everyone to the lobby.
    pub fn back_to_lobby(&mut self) {
        self.race = None;
        if let Role::Host(h) = &mut self.role {
            for c in &mut h.clients {
                c.kart = None;
            }
            h.lobby_timer = 0.0;
            h.send_lobby(&self.name, self.bots, self.track, self.class);
        }
    }

    pub fn leave(&mut self) {
        match &self.role {
            Role::Host(h) => {
                for c in &h.clients {
                    h.sock.send(c.addr, &HostMsg::Bye);
                }
            }
            Role::Client(c) => c.sock.send(c.host, &ClientMsg::Bye),
            Role::Solo => {}
        }
    }

    /// Advance by one rendered frame, with an input for each local player.
    pub fn update(&mut self, frame_dt: f32, local_inputs: &[KartInput]) -> Vec<SessionEvent> {
        self.now += frame_dt as f64;
        let mut events = vec![];
        match &mut self.role {
            Role::Solo => {}
            Role::Host(h) => h.poll(self.now, &mut self.race, &self.name, self.bots, self.track, self.class),
            Role::Client(c) => c.poll(self.now, &mut self.race, (&mut self.track, &mut self.class), &self.name, &mut events),
        }
        if events.iter().any(|e| matches!(e, SessionEvent::Disconnected(_))) {
            return events;
        }

        if let Some(race) = &mut self.race {
            race.acc += frame_dt.min(0.25);
            while race.acc >= DT {
                race.acc -= DT;
                let inputs: Vec<KartInput> = (race.locals.iter().enumerate())
                    .map(|(p, &k)| if self.autodrive { bot_input(&race.world, &race.track, k) } else { local_inputs.get(p).copied().unwrap_or_default() })
                    .collect();
                for (i, k) in race.world.karts.iter_mut().enumerate() {
                    let given = self.test_items[if race.locals.contains(&i) { 0 } else { 1 }];
                    if let Some((item, count)) = given.filter(|_| k.item == Item::None && race.world.time > COUNTDOWN) {
                        (k.item, k.item_count, k.triple) = (item, count, count == 3);
                    }
                }
                match &mut self.role {
                    Role::Solo => tick_authoritative(race, &inputs, None),
                    Role::Host(h) => tick_authoritative(race, &inputs, Some(h)),
                    Role::Client(c) => c.tick(race, inputs[0]),
                }
            }
        }
        events
    }
}

fn tick_authoritative(race: &mut RaceState, local_inputs: &[KartInput], mut host: Option<&mut Host>) {
    let track = &race.track;
    let w = &race.world;
    let inputs: Vec<KartInput> = (0..w.karts.len())
        .map(|i| {
            // Always consume remote input so the client's acks keep advancing.
            let remote = host
                .as_mut()
                .and_then(|h| h.clients.iter_mut().find(|c| c.kart == Some(i)))
                .map(|c| c.next_input());
            let k = &w.karts[i];
            match remote {
                _ if k.bot || k.finished => bot_input(w, track, i),
                Some(input) => input,
                None => match race.locals.iter().position(|&k| k == i) {
                    Some(p) => local_inputs[p],
                    None => bot_input(w, track, i),
                },
            }
        })
        .collect();
    race.prev = race.world.clone();
    race.world.step(track, &inputs);

    if let Some(h) = host {
        for c in &h.clients {
            if c.kart.is_some() {
                let msg = HostMsg::Snapshot { race_id: race.race_id, ack: c.applied_seq, world: Box::new(race.world.clone()) };
                h.sock.send(c.addr, &msg);
            }
        }
    }
}

impl Host {
    fn send_lobby(&self, host_name: &str, bots: u8, track: TrackId, class: SpeedClass) {
        let players: Vec<String> =
            std::iter::once(host_name.to_string()).chain(self.clients.iter().map(|c| c.name.clone())).collect();
        let msg = HostMsg::Lobby { players, bots, track, map: custom::map_of(track), class };
        for c in &self.clients {
            if c.kart.is_none() {
                self.sock.send(c.addr, &msg);
            }
        }
    }

    fn send_race_info(&self, race: &RaceState, seed: u32) {
        for c in &self.clients {
            if let Some(k) = c.kart {
                let msg = HostMsg::RaceInfo {
                    race_id: race.race_id,
                    seed,
                    track: race.track.id,
                    map: custom::map_of(race.track.id),
                    class: race.class,
                    roster: race.roster.clone(),
                    your_kart: k as u8,
                };
                self.sock.send(c.addr, &msg);
            }
        }
    }

    fn drop_client(&mut self, idx: usize, race: &mut Option<RaceState>) {
        let c = self.clients.remove(idx);
        if let (Some(k), Some(r)) = (c.kart, race.as_mut()) {
            // Their corgi keeps racing on autopilot.
            r.world.karts[k].bot = true;
            r.roster[k].name = format!("{} (left)", r.roster[k].name);
        }
    }

    fn poll(&mut self, now: f64, race: &mut Option<RaceState>, host_name: &str, bots: u8, track: TrackId, class: SpeedClass) {
        for (from, msg) in self.sock.recv_all::<ClientMsg>() {
            let idx = self.clients.iter().position(|c| c.addr == from);
            if let Some(i) = idx {
                self.clients[i].last_heard = now;
            }
            match msg {
                ClientMsg::Hello { name, version } => {
                    if version != PROTOCOL_VERSION {
                        self.sock.send(from, &HostMsg::Reject { reason: "Game version mismatch".into() });
                    } else if idx.is_none() && self.clients.len() + 1 >= MAX_KARTS {
                        self.sock.send(from, &HostMsg::Reject { reason: "Lobby is full".into() });
                    } else {
                        if idx.is_none() {
                            let name: String = name.chars().take(16).collect();
                            self.clients.push(RemoteClient {
                                addr: from,
                                name: if name.trim().is_empty() { "Corgi".into() } else { name },
                                kart: None,
                                last_heard: now,
                                queue: VecDeque::new(),
                                last_recv_seq: 0,
                                applied_seq: 0,
                                last_input: KartInput::default(),
                            });
                        }
                        self.sock.send(from, &HostMsg::Welcome);
                    }
                }
                ClientMsg::KeepAlive => {}
                ClientMsg::Inputs { last_seq, inputs } => {
                    // A real client sends 1 to `INPUT_REDUNDANCY` inputs, and only during a race.
                    let c = idx.map(|i| &mut self.clients[i]).filter(|c| c.kart.is_some() && (1..=INPUT_REDUNDANCY).contains(&inputs.len()));
                    if let Some(c) = c {
                        let first = last_seq.wrapping_sub(inputs.len() as u32 - 1);
                        for (n, input) in inputs.into_iter().enumerate() {
                            let seq = first.wrapping_add(n as u32);
                            if seq > c.last_recv_seq {
                                c.queue.push_back((seq, input));
                                c.last_recv_seq = seq;
                            }
                        }
                        while c.queue.len() > MAX_QUEUE {
                            c.queue.pop_front();
                        }
                    }
                }
                ClientMsg::Bye => {
                    if let Some(i) = idx {
                        self.drop_client(i, race);
                    }
                }
            }
        }

        let mut i = 0;
        while i < self.clients.len() {
            if now - self.clients[i].last_heard > TIMEOUT_SECS {
                self.drop_client(i, race);
            } else {
                i += 1;
            }
        }

        if now - self.lobby_timer > 0.25 {
            self.lobby_timer = now;
            self.send_lobby(host_name, bots, track, class);
        }
        if let Some(r) = race {
            if now - self.info_timer > 1.0 {
                self.info_timer = now;
                // Seed is irrelevant once snapshots flow; clients only need the roster.
                self.send_race_info(r, 0);
            }
        }
    }
}

/// The track the host picked. A custom one comes with its map, which we remember; if the map
/// doesn't match the id, or isn't one that can be raced, something is badly wrong, so race the
/// default track instead.
fn learn_track(track: TrackId, map: Option<custom::CustomMap>) -> TrackId {
    match (track, map) {
        (TrackId::Custom(h), Some(map)) if map.sane() && map.hash() == h => {
            let id = custom::register(map, None);
            if custom::raceable(h) { id } else { TrackId::default() }
        }
        (TrackId::Custom(_), _) => TrackId::default(),
        (t, _) => t,
    }
}

/// A name a LAN peer sent, cut to a length that fits on screen.
fn short_name(name: &str) -> String {
    name.chars().take(MAX_NAME).collect()
}

impl Client {
    fn poll(
        &mut self,
        now: f64,
        race: &mut Option<RaceState>,
        (lobby_track, lobby_class): (&mut TrackId, &mut SpeedClass),
        name: &str,
        events: &mut Vec<SessionEvent>,
    ) {
        if self.started == 0.0 {
            self.started = now;
        }
        let mut latest: Option<(u32, Box<World>)> = None;
        for (from, msg) in self.sock.recv_all::<HostMsg>() {
            // Until the host answers, match on port only: a multi-homed host may reply from a
            // different address. From then on only that address is the host.
            if from.port() != self.host.port() || (self.connected && from != self.host) {
                continue;
            }
            self.last_heard = now;
            if !self.connected && matches!(msg, HostMsg::Welcome | HostMsg::Lobby { .. } | HostMsg::RaceInfo { .. }) {
                self.host = from;
            }
            match msg {
                HostMsg::Welcome => self.connected = true,
                HostMsg::Reject { reason } => {
                    events.push(SessionEvent::Disconnected(reason));
                    return;
                }
                HostMsg::Lobby { players, bots, track, map, class } => {
                    self.connected = true;
                    let track = learn_track(track, map);
                    self.lobby_players = players.iter().take(MAX_KARTS).map(|n| short_name(n)).collect();
                    self.lobby_bots = bots.min(MAX_KARTS as u8);
                    *lobby_track = track;
                    *lobby_class = class;
                    if race.take().is_some() {
                        events.push(SessionEvent::BackToLobby);
                    }
                }
                HostMsg::RaceInfo { race_id, seed, track, map, class, mut roster, your_kart } => {
                    self.connected = true;
                    // The roster is the grid: we must be on it, and it only changes in its names.
                    let same_race = race.as_ref().filter(|r| r.race_id == race_id).map(|r| r.roster.len());
                    if roster.is_empty() || roster.len() > MAX_KARTS || your_kart as usize >= roster.len() || same_race.is_some_and(|n| n != roster.len()) {
                        continue;
                    }
                    roster.iter_mut().for_each(|r| r.name = short_name(&r.name));
                    let track = learn_track(track, map);
                    *lobby_track = track;
                    *lobby_class = class;
                    if race.as_ref().map(|r| r.race_id) != Some(race_id) {
                        *race = Some(RaceState::new(track, class, race_id, seed, roster, vec![your_kart as usize]));
                        self.history.clear();
                        events.push(SessionEvent::RaceStarted);
                    } else if let Some(r) = race {
                        r.roster = roster; // names may change when players leave
                    }
                }
                HostMsg::Snapshot { race_id, ack, world } => {
                    if race.as_ref().is_some_and(|r| r.race_id == race_id && world.fits(r.roster.len()))
                        && latest.as_ref().is_none_or(|(_, w)| world.tick > w.tick)
                    {
                        latest = Some((ack, world));
                    }
                }
                HostMsg::Bye => {
                    events.push(SessionEvent::Disconnected("The host ended the game".into()));
                    return;
                }
            }
        }

        if let (Some((ack, world)), Some(r)) = (latest, race.as_mut()) {
            if world.tick > r.last_snapshot_tick {
                r.last_snapshot_tick = world.tick;
                self.reconcile(r, ack, *world);
            }
        }

        let silent_for = now - if self.connected { self.last_heard } else { self.started };
        if silent_for > if self.connected { TIMEOUT_SECS } else { 8.0 } {
            let msg = if self.connected { "Lost connection to the host" } else { "Couldn't reach the host" };
            events.push(SessionEvent::Disconnected(msg.into()));
            return;
        }

        if race.is_none() && now - self.send_timer > 0.25 {
            self.send_timer = now;
            if self.connected {
                self.sock.send(self.host, &ClientMsg::KeepAlive);
            } else {
                self.sock.send(self.host, &ClientMsg::Hello { name: name.to_string(), version: PROTOCOL_VERSION });
            }
        }
    }

    /// Adopt the host's world, then replay inputs the host hasn't processed yet on our kart.
    fn reconcile(&mut self, race: &mut RaceState, ack: u32, mut world: World) {
        let track = &race.track;
        while self.history.front().is_some_and(|(s, _)| *s <= ack) {
            self.history.pop_front();
        }
        let me = race.locals[0];
        let mut t = world.time;
        for (_, input) in &self.history {
            t += DT;
            world.karts[me].step(*input, track, t);
        }
        race.world = world;
    }

    fn tick(&mut self, race: &mut RaceState, input: KartInput) {
        let track = &race.track;
        let me = race.locals[0];
        let input = if race.world.karts[me].finished { bot_input(&race.world, track, me) } else { input };
        self.seq += 1;
        self.history.push_back((self.seq, input));
        while self.history.len() > 240 {
            self.history.pop_front();
        }
        let n = self.history.len().min(INPUT_REDUNDANCY);
        let inputs: Vec<KartInput> = self.history.iter().skip(self.history.len() - n).map(|(_, i)| *i).collect();
        self.sock.send(self.host, &ClientMsg::Inputs { last_seq: self.seq, inputs });

        race.prev = race.world.clone();
        let w = &mut race.world;
        let t = w.time + DT;
        w.karts[me].step(input, track, t);
        // Dead-reckon everything else until the next snapshot arrives.
        if t >= COUNTDOWN {
            for (i, k) in w.karts.iter_mut().enumerate() {
                if i != me {
                    k.pos += k.velocity() * DT;
                }
            }
            for p in &mut w.projectiles {
                p.pos += p.vel * DT;
            }
            for t in w.traps.iter_mut().filter(|t| t.flight > 0.0) {
                t.pos += t.vel * DT;
                t.flight = (t.flight - DT).max(1e-3);
                t.age += DT;
            }
        }
        w.time = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::parse_addr;

    /// Host and client in one process over localhost: join, race, finish.
    #[test]
    fn lan_race_over_localhost() {
        let port = 47000 + (std::process::id() % 1000) as u16;
        let mut host = Session::host("Hosty", 2, port, TrackId::Circuit, SpeedClass::C150).unwrap();
        let mut client = Session::join("Clienty", parse_addr(&format!("127.0.0.1:{port}")).unwrap()).unwrap();
        host.autodrive = true;
        client.autodrive = true;
        let dt = 1.0 / 60.0;
        let idle = KartInput::default();

        for _ in 0..60 {
            host.update(dt, &[idle]);
            client.update(dt, &[idle]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.human_names(), vec!["Hosty", "Clienty"]);
        assert_eq!(client.human_names(), vec!["Hosty", "Clienty"]);

        assert_eq!(client.track, TrackId::Circuit);
        assert_eq!(client.class, SpeedClass::C150);
        host.start_race();
        let mut started = false;
        let mut max_err: f32 = 0.0;
        for frame in 0..60 * 320 {
            host.update(dt, &[idle]);
            for e in client.update(dt, &[idle]) {
                match e {
                    SessionEvent::RaceStarted => started = true,
                    SessionEvent::Disconnected(m) => panic!("client disconnected: {m}"),
                    SessionEvent::BackToLobby => {}
                }
            }
            if frame % 3 == 0 {
                std::thread::sleep(std::time::Duration::from_micros(300));
            }
            if let (Some(h), Some(c)) = (&host.race, &client.race) {
                let me = c.locals[0];
                let e = h.world.karts[me].pos.distance(c.world.karts[me].pos);
                max_err = max_err.max(e);
                if h.world.over {
                    break;
                }
            }
        }
        assert!(started, "client never started the race");
        let h = host.race.as_ref().unwrap();
        let c = client.race.as_ref().unwrap();
        assert!(h.world.over, "race never finished");
        assert!(h.world.karts[c.locals[0]].finished, "client's kart didn't finish");
        assert!(c.world.over, "client didn't see the race end");
        println!("max predicted-vs-host position error: {max_err:.2}m");
        assert!(max_err < 5.0, "prediction diverged: {max_err}");

        host.back_to_lobby();
        let mut back = false;
        for _ in 0..60 {
            host.update(dt, &[idle]);
            back |= client.update(dt, &[idle]).iter().any(|e| matches!(e, SessionEvent::BackToLobby));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(back, "client didn't return to lobby");
    }

    /// A client learns a custom track from the host's lobby messages, and they race on it.
    #[test]
    fn lan_custom_track() {
        let map = crate::sim::custom::tests::sample_maps().remove(1);
        let id = TrackId::Custom(map.hash());
        // Hash checking is the client's side of it: a map that doesn't match its id is refused.
        let mut other = map.clone();
        other.name = "Imposter".into();
        assert_eq!(learn_track(id, Some(other)), TrackId::default());
        assert_eq!(learn_track(id, Some(map.clone())), id);

        let port = 48000 + (std::process::id() % 1000) as u16;
        let mut host = Session::host("Hosty", 1, port, id, SpeedClass::C150).unwrap();
        let mut client = Session::join("Clienty", parse_addr(&format!("127.0.0.1:{port}")).unwrap()).unwrap();
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            host.update(dt, &[KartInput::default()]);
            client.update(dt, &[KartInput::default()]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(client.track, id);
        host.start_race();
        for _ in 0..60 {
            host.update(dt, &[KartInput::default()]);
            client.update(dt, &[KartInput::default()]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let (h, c) = (host.race.as_ref().unwrap(), client.race.as_ref().expect("client never started"));
        assert_eq!(c.track.id, id);
        assert_eq!(c.track.samples.len(), h.track.samples.len());
        assert_eq!(c.track.item_rows, h.track.item_rows);
    }

    fn pump(s: &mut Session, frames: usize) {
        for _ in 0..frames {
            s.update(1.0 / 60.0, &[KartInput::default()]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// A host that sends nonsense is ignored rather than crashing the client.
    #[test]
    fn client_ignores_a_bad_host() {
        let port = 49000 + (std::process::id() % 1000) as u16;
        let mut fake = Socket::bind(port).unwrap();
        let mut client = Session::join("Clienty", parse_addr(&format!("127.0.0.1:{port}")).unwrap()).unwrap();
        let mut addr = None;
        for _ in 0..200 {
            pump(&mut client, 1);
            if let Some((from, _)) = fake.recv_all::<ClientMsg>().pop() {
                addr = Some(from);
                break;
            }
        }
        let addr = addr.expect("the client never said hello");
        let roster = |n: usize| -> Vec<RacerInfo> { (0..n).map(|i| RacerInfo { name: format!("Racer {i}"), bot: false }).collect() };
        let info = |roster: Vec<RacerInfo>, your_kart: u8| HostMsg::RaceInfo { race_id: 1, seed: 1, track: TrackId::Circuit, map: None, class: SpeedClass::C150, roster, your_kart };

        // A grid we aren't on, one that's far too big, and an empty one.
        fake.send(addr, &HostMsg::Welcome);
        fake.send(addr, &info(roster(2), 5));
        fake.send(addr, &info(roster(200), 0));
        fake.send(addr, &info(vec![], 0));
        pump(&mut client, 20);
        assert!(client.race.is_none(), "raced on a grid that makes no sense");

        let mut long = roster(2);
        long[0].name = "x".repeat(5000);
        fake.send(addr, &info(long, 1));
        pump(&mut client, 20);
        let race = client.race.as_ref().expect("a sensible grid didn't start the race");
        assert_eq!(race.roster[0].name.chars().count(), MAX_NAME);

        // A world with the wrong number of karts, one that names a kart that isn't there, and a
        // grid that changes size mid-race.
        let track = Track::new(TrackId::Circuit);
        let mut world = World::new(&track, &[false; 5], 1, SpeedClass::C150);
        world.tick = 10;
        fake.send(addr, &HostMsg::Snapshot { race_id: 1, ack: 0, world: Box::new(world) });
        let mut world = World::new(&track, &[false; 2], 1, SpeedClass::C150);
        world.tick = 11;
        world.finish_order = vec![7];
        fake.send(addr, &HostMsg::Snapshot { race_id: 1, ack: 0, world: Box::new(world) });
        fake.send(addr, &info(roster(1), 0));
        pump(&mut client, 20);
        let race = client.race.as_ref().unwrap();
        assert_eq!((race.world.karts.len(), race.roster.len()), (2, 2));
        assert!(race.world.finish_order.is_empty());

        // A custom track that would be enormous, or isn't made of numbers, or can't be raced.
        let good = crate::sim::custom::tests::sample_maps().remove(1);
        for wreck in [1e30, 3e8, f32::NAN, 0.0] {
            let mut map = good.clone();
            map.points.iter_mut().for_each(|p| (p.x, p.z) = (p.x * wreck, p.z * wreck));
            assert_eq!(learn_track(TrackId::Custom(map.hash()), Some(map)), TrackId::default(), "took a map scaled by {wreck}");
        }
    }

    /// A client that sends nonsense is ignored too.
    #[test]
    fn host_ignores_a_bad_client() {
        let port = 50000 + (std::process::id() % 1000) as u16;
        let mut host = Session::host("Hosty", 1, port, TrackId::Circuit, SpeedClass::C150).unwrap();
        let to = parse_addr(&format!("127.0.0.1:{port}")).unwrap();
        let fake = Socket::bind(0).unwrap();
        fake.send(to, &ClientMsg::Hello { name: "y".repeat(5000), version: PROTOCOL_VERSION });
        // Inputs in the lobby, where nobody is driving.
        fake.send(to, &ClientMsg::Inputs { last_seq: 3, inputs: vec![KartInput::default(); 3] });
        pump(&mut host, 20);
        host.start_race();
        // No inputs at all, far too many, and a flood of ordinary ones.
        fake.send(to, &ClientMsg::Inputs { last_seq: 0, inputs: vec![] });
        fake.send(to, &ClientMsg::Inputs { last_seq: 9000, inputs: vec![KartInput::default(); 5000] });
        for seq in 1..=200 {
            fake.send(to, &ClientMsg::Inputs { last_seq: seq * 8, inputs: vec![KartInput::default(); INPUT_REDUNDANCY] });
        }
        if let Role::Host(h) = &mut host.role {
            h.poll(host.now, &mut host.race, "Hosty", 1, TrackId::Circuit, SpeedClass::C150);
            assert_eq!(h.clients.len(), 1);
            assert_eq!(h.clients[0].name.chars().count(), 16);
            assert!((1..=MAX_QUEUE).contains(&h.clients[0].queue.len()), "{} inputs queued", h.clients[0].queue.len());
        }
        pump(&mut host, 20);
        assert_eq!(host.race.as_ref().unwrap().world.karts.len(), 3);
    }

    /// Two players on one machine start at the back of the grid and both finish.
    #[test]
    fn split_screen_race() {
        let mut s = Session::solo("Ollie", 2, 3, TrackId::Meadow, SpeedClass::C150);
        s.autodrive = true;
        assert_eq!(s.human_names(), vec!["Ollie", "Player 2"]);
        assert_eq!(s.max_bots(), 6);
        s.start_race();
        let race = s.race.as_ref().unwrap();
        assert_eq!(race.locals, vec![3, 4]);
        assert!(race.locals.iter().all(|&k| !race.roster[k].bot));
        for _ in 0..60 * 400 {
            s.update(1.0 / 60.0, &[]);
            if s.race.as_ref().unwrap().world.over {
                break;
            }
        }
        let race = s.race.as_ref().unwrap();
        assert!(race.world.over, "race never finished");
        assert!(race.locals.iter().all(|&k| race.world.karts[k].finished), "a local player didn't finish");
    }
}
