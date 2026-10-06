//! LAN networking over plain UDP.
//!
//! The host is authoritative. Clients send their inputs every tick (with a few previous inputs
//! repeated, in case packets drop). The host sends the whole `World` every tick plus the last
//! input sequence it applied for that client, so the client can predict its own kart and
//! reconcile.

use std::net::{SocketAddr, UdpSocket};

use serde::{Deserialize, Serialize};

use crate::sim::custom::CustomMap;
use crate::sim::{KartInput, SpeedClass, TrackId, World};

pub const DEFAULT_PORT: u16 = 7777;
pub const PROTOCOL_VERSION: u32 = 5;
pub const TIMEOUT_SECS: f64 = 5.0;
/// How many past inputs each input packet repeats.
pub const INPUT_REDUNDANCY: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RacerInfo {
    pub name: String,
    pub bot: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello { name: String, version: u32 },
    KeepAlive,
    /// `inputs` are consecutive, the last one has sequence number `last_seq`.
    Inputs { last_seq: u32, inputs: Vec<KartInput> },
    Bye,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum HostMsg {
    Welcome,
    Reject { reason: String },
    /// `map` comes with a custom track, so clients can build it.
    Lobby { players: Vec<String>, bots: u8, track: TrackId, map: Option<CustomMap>, class: SpeedClass },
    RaceInfo { race_id: u32, seed: u32, track: TrackId, map: Option<CustomMap>, class: SpeedClass, roster: Vec<RacerInfo>, your_kart: u8 },
    Snapshot { race_id: u32, ack: u32, world: Box<World> },
    Bye,
}

pub struct Socket {
    sock: UdpSocket,
    buf: Vec<u8>,
}

impl Socket {
    pub fn bind(port: u16) -> std::io::Result<Self> {
        let sock = UdpSocket::bind(("0.0.0.0", port))?;
        sock.set_nonblocking(true)?;
        Ok(Socket { sock, buf: vec![0; 65536] })
    }

    pub fn send<T: Serialize>(&self, to: SocketAddr, msg: &T) {
        if let Ok(bytes) = bincode::serialize(msg) {
            // Dropped packets are fine: everything important is re-sent.
            let _ = self.sock.send_to(&bytes, to);
        }
    }

    /// Drain every datagram that has arrived.
    pub fn recv_all<T: for<'de> Deserialize<'de>>(&mut self) -> Vec<(SocketAddr, T)> {
        let mut out = vec![];
        loop {
            match self.sock.recv_from(&mut self.buf) {
                Ok((n, from)) => {
                    if let Ok(msg) = bincode::deserialize(&self.buf[..n]) {
                        out.push((from, msg));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                // e.g. ICMP "connection refused" from a closed peer on some platforms
                Err(_) => continue,
            }
        }
        out
    }
}

/// Best-effort guess at this machine's LAN address, to show the host what to tell friends.
pub fn local_ip() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    // No packet is sent; this just asks the OS which interface it would route through.
    s.connect("192.168.0.1:9").or_else(|_| s.connect("10.0.0.1:9")).ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

pub fn parse_addr(text: &str) -> Option<SocketAddr> {
    let text = text.trim();
    if let Ok(a) = text.parse::<SocketAddr>() {
        return Some(a);
    }
    if let Ok(ip) = text.parse::<std::net::IpAddr>() {
        return Some(SocketAddr::new(ip, DEFAULT_PORT));
    }
    use std::net::ToSocketAddrs;
    let with_port = if text.contains(':') { text.to_string() } else { format!("{text}:{DEFAULT_PORT}") };
    with_port.to_socket_addrs().ok()?.find(|a| a.is_ipv4())
}
