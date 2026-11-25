//! Online play: rollback netcode (`ggrs`, GGPO-style) over a match of 2 to 4 players (VS, Ball, the tag
//! brawl, Force). Each computer runs the match at once on its own pads and predicted pads for the others
//! (the last ones repeated); when a real pad arrives and differs, the match goes back to the state saved at that
//! frame and runs again to now. The state is [`NetState`]: the fight's whole [`Bout`], the tag match or the Force
//! run (they clone cheaply: the bodies and data are shared, read-only); its checksum is compared to catch a desync.
//! When a match is over: [`kumite_net::Watch`].
//!
//! - `--net-synctest N`: no network; every frame goes back N frames and runs them again, checking the checksums
//!   match (the rollback's own test: everything the fight depends on is in the saved state).
//! - `--net-local PORT --net-peer HOST:PORT --net-player 0|1`: two players over UDP (two windows on one machine, or a
//!   local network), no room.
//! - `--net-delay MS --net-jitter MS --net-loss PCT`: a bad connection, simulated on the packets received (both
//!   sides: the round trip is twice the delay); `--net-local`'s or a room's matches.
//! - The ONLINE menu (or `--online ROOM`, `--host`, `--join`): a link (`netlink.rs`), then a session per match.

#[cfg(not(target_arch = "wasm32"))]
use anyhow::bail;
use anyhow::{Context, Result};
use ggrs::{GgrsError, GgrsEvent, GgrsRequest, P2PSession, PlayerType, SessionBuilder, SessionState, SyncTestSession};
use kumite_game::bout::Bout;
use kumite_sim::input::Pad;
use std::net::SocketAddr;
#[cfg(not(target_arch = "wasm32"))]
use web_time::{Duration, Instant};

/// The most frames waited at once when ggrs says this computer runs ahead (it reckons from what the peer reports:
/// a lying report must not freeze the match for minutes).
const MAX_SKIP: u32 = 8;

pub use kumite_net::NetInput;

#[derive(Debug)]
pub struct NetConfig;

/// Who another computer is: a UDP address (`--net-local` testing), or its seat in the room (`netlink.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetAddr {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Udp(SocketAddr),
    Seat(u8),
}

impl ggrs::Config for NetConfig {
    type Input = NetInput;
    type InputPredictor = ggrs::PredictRepeatLast;
    type State = NetState;
    type Address = NetAddr;
}

/// What a rollback session saves: the fight (VS, Ball) or the tag match.
#[derive(Clone)]
pub enum NetState {
    Bout(Box<Bout>),
    Tag(Box<kumite_game::tag::TagGame>),
    /// Force: the fight and the stage.
    Force(Box<ForceState>),
}

/// Force's rollback state: the fight and its stage logic.
#[derive(Clone)]
pub struct ForceState(pub Bout, pub kumite_game::force::ForceStage);

impl NetGame for ForceState {
    fn wrap(self) -> NetState {
        NetState::Force(Box::new(self))
    }
    fn unwrap(s: NetState) -> Option<Self> {
        if let NetState::Force(f) = s {
            Some(*f)
        } else {
            None
        }
    }
    fn checksum(&self) -> u64 {
        // (the stage's progress with the fight's)
        self.0.game.state_hash() ^ (self.1.cam_x as u64).rotate_left(17) ^ (self.1.score as u64).rotate_left(41) ^ (self.1.state as u64) << 56
    }
}

/// A match a rollback session runs: saved whole, checked by its hash.
pub trait NetGame: Clone {
    fn wrap(self) -> NetState;
    fn unwrap(s: NetState) -> Option<Self>;
    fn checksum(&self) -> u64;
}

impl NetGame for Bout {
    fn wrap(self) -> NetState {
        NetState::Bout(Box::new(self))
    }
    fn unwrap(s: NetState) -> Option<Self> {
        if let NetState::Bout(b) = s {
            Some(*b)
        } else {
            None
        }
    }
    fn checksum(&self) -> u64 {
        self.game.state_hash()
    }
}

impl NetGame for kumite_game::tag::TagGame {
    fn wrap(self) -> NetState {
        NetState::Tag(Box::new(self))
    }
    fn unwrap(s: NetState) -> Option<Self> {
        if let NetState::Tag(t) = s {
            Some(*t)
        } else {
            None
        }
    }
    fn checksum(&self) -> u64 {
        self.state_hash()
    }
}

/// A bad connection for testing (native: UDP, or a room's link): each packet received waits `delay` ± `jitter`, or is
/// lost (`loss` 0..1).
#[cfg(not(target_arch = "wasm32"))]
pub struct LossySocket<S> {
    inner: S,
    delay: Duration,
    jitter: Duration,
    loss: f32,
    queue: Vec<(Instant, NetAddr, ggrs::Message)>,
    rng: u32,
}

#[cfg(not(target_arch = "wasm32"))]
impl<S> LossySocket<S> {
    pub fn new(inner: S, bad: (Duration, Duration, f32), seed: u32) -> Self {
        Self { inner, delay: bad.0, jitter: bad.1, loss: bad.2, queue: Vec::new(), rng: 0x1234_5678 ^ seed }
    }

    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<S: ggrs::NonBlockingSocket<NetAddr>> ggrs::NonBlockingSocket<NetAddr> for LossySocket<S> {
    fn send_to(&mut self, msg: &ggrs::Message, addr: &NetAddr) {
        self.inner.send_to(msg, addr);
    }

    fn receive_all_messages(&mut self) -> Vec<(NetAddr, ggrs::Message)> {
        let now = Instant::now();
        for (addr, msg) in self.inner.receive_all_messages() {
            if self.random() < self.loss {
                continue;
            }
            let wait = self.delay + self.jitter.mul_f32(self.random());
            self.queue.push((now + wait, addr, msg));
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue).into_iter().partition(|(t, _, _)| *t <= now);
        self.queue = later;
        due.into_iter().map(|(_, a, m)| (a, m)).collect()
    }
}

/// `--net-local`'s UDP socket, its peers by address.
#[cfg(not(target_arch = "wasm32"))]
impl ggrs::NonBlockingSocket<NetAddr> for ggrs::UdpNonBlockingSocket {
    fn send_to(&mut self, msg: &ggrs::Message, addr: &NetAddr) {
        if let NetAddr::Udp(a) = addr {
            ggrs::NonBlockingSocket::<SocketAddr>::send_to(self, msg, a);
        }
    }

    fn receive_all_messages(&mut self) -> Vec<(NetAddr, ggrs::Message)> {
        ggrs::NonBlockingSocket::<SocketAddr>::receive_all_messages(self).into_iter().map(|(a, m)| (NetAddr::Udp(a), m)).collect()
    }
}

enum Session {
    P2P(Box<P2PSession<NetConfig>>),
    Sync(Box<SyncTestSession<NetConfig>>),
}

/// What a tick asks of the fight, in order.
pub type Requests = Vec<GgrsRequest<NetConfig>>;

pub struct NetPlay {
    session: Session,
    /// The players this computer plays (the sync test plays both of its two).
    pub local: Vec<usize>,
    /// The players in the session.
    pub players: usize,
    /// Frames to sit out (the session's time sync: this side ran ahead).
    skip: u32,
    /// When the match is over ([`kumite_net::Watch`]).
    watch: kumite_net::Watch<NetAddr>,
    /// Online: this computer has said its match loaded; another computer has not loaded it yet (the room says).
    pub ready_said: bool,
    pub peers_loading: bool,
    /// What the HUD line shows: synchronizing, the ping, a desync.
    pub status: String,
    /// The session's frame the fight is at, and the newest one simulated (to tell a new frame from one run again
    /// after a rollback).
    frame: i32,
    newest: i32,
    /// Frames since the other player left (the fight stops; the app leaves it after a while).
    pub gone: Option<u32>,
}

pub use kumite_net::{INPUT_DELAY, MAX_PREDICTION};

impl NetPlay {
    /// The session polled, no frame run: a match over here still sends its last inputs again until the others have
    /// them (ggrs resends on a poll; without it a lost last packet left another computer short of the match's end).
    pub fn poll(&mut self) {
        if let Session::P2P(s) = &mut self.session {
            s.poll_remote_clients();
            let _ = s.events().count();
        }
    }

    /// The command line's session, if it asks for one.
    pub fn from_args(args: &[String]) -> Result<Option<Self>> {
        let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        if let Some(d) = arg("--net-synctest") {
            let d: usize = d.parse().context("--net-synctest N (frames to go back)")?;
            return Ok(Some(Self::synctest(d)?));
        }
        #[cfg(not(target_arch = "wasm32"))]
        return Self::udp_from_args(args);
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    /// `--net-local PORT --net-peer HOST:PORT --net-player 0|1 [--net-delay MS --net-jitter MS --net-loss PCT]`.
    #[cfg(not(target_arch = "wasm32"))]
    fn udp_from_args(args: &[String]) -> Result<Option<Self>> {
        let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        let (Some(port), Some(peer)) = (arg("--net-local"), arg("--net-peer")) else { return Ok(None) };
        let port: u16 = port.parse().context("--net-local PORT")?;
        let peer: SocketAddr = peer.parse().context("--net-peer HOST:PORT")?;
        let local: usize = arg("--net-player").map_or(Ok(0), |p| p.parse()).context("--net-player 0|1")?;
        if local > 1 {
            bail!("--net-player 0|1");
        }
        Ok(Some(Self::udp(port, peer, local, Self::bad_from_args(args))?))
    }

    /// `--net-delay MS --net-jitter MS --net-loss PCT`: the simulated delay, jitter and loss of the packets received.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bad_from_args(args: &[String]) -> (Duration, Duration, f32) {
        let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        let ms = |name: &str| arg(name).and_then(|v| v.parse::<u64>().ok()).map_or(Duration::ZERO, Duration::from_millis);
        let loss = arg("--net-loss").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0) / 100.0;
        (ms("--net-delay"), ms("--net-jitter"), loss)
    }

    pub fn synctest(check_distance: usize) -> Result<Self> {
        let s = SessionBuilder::<NetConfig>::new()
            .with_num_players(2)?
            .with_check_distance(check_distance)
            .with_input_delay(INPUT_DELAY)
            .add_player(PlayerType::Local, 0)?
            .add_player(PlayerType::Local, 1)?
            .start_synctest_session()?;
        Ok(Self {
            session: Session::Sync(Box::new(s)),
            local: vec![0, 1],
            players: 2,
            skip: 0,
            status: format!("SYNC TEST {check_distance}"),
            frame: 0,
            newest: 0,
            gone: None,
            watch: Default::default(),
            ready_said: false,
            // (until the room says otherwise; without a room, a window started by hand: the long wait)
            peers_loading: true,
        })
    }

    /// `bad`: the simulated delay, jitter and loss of the packets received.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn udp(port: u16, peer: SocketAddr, local: usize, bad: (Duration, Duration, f32)) -> Result<Self> {
        let inner = ggrs::UdpNonBlockingSocket::bind_to_port(port).with_context(|| format!("UDP port {port}"))?;
        let socket = LossySocket::new(inner, bad, port as u32);
        log::info!("netplay: player {} on UDP {port}, the other at {peer}; simulated delay {:?} jitter {:?} loss {}%", local + 1, bad.0, bad.1, bad.2 * 100.0);
        Self::p2p(socket, &[if local == 0 { None } else { Some(NetAddr::Udp(peer)) }, if local == 1 { None } else { Some(NetAddr::Udp(peer)) }])
    }

    /// A session over a socket: each player's computer (None: this one).
    pub fn p2p(socket: impl ggrs::NonBlockingSocket<NetAddr> + 'static, players: &[Option<NetAddr>]) -> Result<Self> {
        let mut b = SessionBuilder::<NetConfig>::new()
            .with_num_players(players.len())?
            .with_input_delay(INPUT_DELAY)
            .with_max_prediction_window(MAX_PREDICTION)
            .with_desync_detection_mode(ggrs::DesyncDetection::On { interval: 60 })
            .with_disconnect_notify_delay(kumite_net::QUIET_NOTICE)
            .with_disconnect_timeout(kumite_net::GGRS_TIMEOUT)
            .with_fps(60)?;
        for (k, at) in players.iter().enumerate() {
            b = b.add_player(at.map_or(PlayerType::Local, PlayerType::Remote), k)?;
        }
        let s = b.start_p2p_session(socket)?;
        let local = (0..players.len()).filter(|&k| players[k].is_none()).collect();
        Ok(Self {
            session: Session::P2P(Box::new(s)),
            local,
            players: players.len(),
            skip: 0,
            status: "CONNECTING".to_string(),
            frame: 0,
            newest: 0,
            gone: None,
            watch: Default::default(),
            ready_said: false,
            // (until the room says otherwise; without a room, a window started by hand: the long wait)
            peers_loading: true,
        })
    }

    /// One 60 Hz tick: the local pads in (this computer's players' in order: its first's, its second's), what the
    /// match must do out (nothing while connecting or waiting).
    pub fn tick(&mut self, pads: &[Pad]) -> Requests {
        match &mut self.session {
            Session::Sync(s) => {
                for k in 0..2 {
                    if let Err(e) = s.add_local_input(k, NetInput::of(pads.get(k).copied().unwrap_or_default())) {
                        log::error!("netplay: {e}");
                    }
                }
                match s.advance_frame() {
                    Ok(r) => r,
                    Err(e) => {
                        // (a checksum that differs after the rollback: the saved state misses something)
                        self.status = format!("SYNC TEST FAILED: {e}");
                        log::error!("netplay sync test: {e}");
                        Vec::new()
                    }
                }
            }
            Session::P2P(s) => {
                // over: the session is left alone (not even polled: the others then hear nothing, and end theirs)
                if let Some(g) = self.gone.as_mut() {
                    *g += 1;
                    return Vec::new();
                }
                s.poll_remote_clients();
                for e in s.events() {
                    self.watch.event(&e);
                    match e {
                        GgrsEvent::WaitRecommendation { skip_frames } => self.skip = skip_frames.min(MAX_SKIP),
                        GgrsEvent::DesyncDetected { frame, local_checksum, remote_checksum, .. } => {
                            self.status = format!("DESYNC AT {frame}");
                            log::error!("netplay: desync at frame {frame}: {local_checksum:x} here, {remote_checksum:x} there");
                        }
                        GgrsEvent::NetworkInterrupted { .. } => self.status = "CONNECTION LOST...".to_string(),
                        e => log::info!("netplay: {e:?}"),
                    }
                }
                // over (a peer disconnected, quiet, never acknowledging, or the match stalled): no frame runs after
                // (kumite_net::Watch)
                // (another computer still loading its match: said, not CONNECTING)
                if s.current_state() != SessionState::Running {
                    self.status = if self.peers_loading { "WAITING FOR THE OTHERS" } else { "CONNECTING" }.to_string();
                }
                let local = &self.local;
                if let Some(why) = self.watch.over(s, (0..self.players).filter(|h| !local.contains(h)), self.peers_loading) {
                    self.status = why.to_string();
                    self.gone = Some(1);
                    return Vec::new();
                }
                if s.current_state() != SessionState::Running {
                    return Vec::new();
                }
                if self.skip > 0 {
                    self.skip -= 1;
                    return Vec::new();
                }
                for (k, &h) in self.local.iter().enumerate() {
                    if let Err(e) = s.add_local_input(h, NetInput::of(pads.get(k).copied().unwrap_or_default())) {
                        log::error!("netplay: {e}");
                        return Vec::new();
                    }
                }
                // the worst ping of the others
                let ping = (0..self.players).filter(|k| !self.local.contains(k)).filter_map(|k| s.network_stats(k).ok()).map(|st| st.ping).max();
                if let Some(ping) = ping {
                    if !self.status.starts_with("DESYNC") && !self.status.starts_with("DISCONNECTED") {
                        self.status = format!("PING {ping} MS");
                    }
                }
                match s.advance_frame() {
                    Ok(r) => r,
                    // (too far ahead of the other's pads: this frame waits)
                    Err(GgrsError::PredictionThreshold) => Vec::new(),
                    Err(e) => {
                        log::error!("netplay: {e}");
                        Vec::new()
                    }
                }
            }
        }
    }
}

/// Carry out a tick's requests on the fight: save, load, advance (`step`: one frame with every player's pad).
/// Returns how many frames advanced and whether the last one is a frame never simulated before (its sounds and
/// effects play; frames run again after a rollback stay silent).
pub fn apply<G: NetGame>(net: &mut NetPlay, game: &mut G, requests: Requests, step: &mut dyn FnMut(&mut G, &[Pad])) -> (usize, bool) {
    let (mut advanced, mut fresh) = (0, false);
    for r in requests {
        match r {
            GgrsRequest::SaveGameState { cell, frame } => cell.save(frame, Some(game.clone().wrap()), Some(game.checksum() as u128)),
            GgrsRequest::LoadGameState { cell, frame } => {
                if let Some(g) = cell.load().and_then(G::unwrap) {
                    *game = g;
                    net.frame = frame;
                }
            }
            GgrsRequest::AdvanceFrame { inputs } => {
                let pads: Vec<Pad> = inputs.iter().map(|i| i.0.pad()).collect();
                step(game, &pads);
                advanced += 1;
                net.frame += 1;
                fresh = net.frame > net.newest;
                net.newest = net.newest.max(net.frame);
            }
        }
    }
    (advanced, fresh)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// Every computer's packets on the way: when each arrives (a tick), from whom, to whom.
    type Wire = Rc<RefCell<Vec<(u32, u8, u8, ggrs::Message)>>>;

    /// An in-process socket: each packet takes 2 to 7 ticks (jittered: they overtake each other).
    struct Pipe {
        me: u8,
        wire: Wire,
        now: Rc<Cell<u32>>,
        rng: u32,
    }

    impl ggrs::NonBlockingSocket<NetAddr> for Pipe {
        fn send_to(&mut self, msg: &ggrs::Message, addr: &NetAddr) {
            if let NetAddr::Seat(to) = *addr {
                self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                self.wire.borrow_mut().push((self.now.get() + 2 + (self.rng >> 24) % 6, self.me, to, msg.clone()));
            }
        }
        fn receive_all_messages(&mut self) -> Vec<(NetAddr, ggrs::Message)> {
            let (me, now) = (self.me, self.now.get());
            let mut wire = self.wire.borrow_mut();
            let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut *wire).into_iter().partition(|m| m.2 == me && m.0 <= now);
            *wire = later;
            due.into_iter().map(|m| (NetAddr::Seat(m.1), m.3)).collect()
        }
    }

    /// A player's pad at a tick: a new random one every 7 ticks (the others' predictions miss: rollbacks).
    fn pad(player: usize, tick: u32) -> Pad {
        let mut x = (tick / 7).wrapping_mul(0x9E37_79B9) ^ (player as u32 + 1).wrapping_mul(0x85EB_CA6B);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 13;
        let b = |k: u32| x >> k & 1 != 0;
        Pad { up: b(3) && b(4), down: b(5) && b(6), left: b(7), right: b(8) && !b(7), buttons: (x >> 9) as u8 & 0x1F, start: false }
    }

    /// `game` played over `n` computers (one player each) on the in-process sockets: each computer's checksum of
    /// every frame (as last simulated, after its rollbacks) up to `frames`, and the rollbacks made.
    fn play<G: NetGame>(game: G, n: usize, frames: usize, step: &dyn Fn(&mut G, &[Pad])) -> (Vec<Vec<u64>>, usize) {
        let (wire, now): (Wire, Rc<Cell<u32>>) = Default::default();
        let mut peers: Vec<(NetPlay, G, Vec<u64>)> = (0..n)
            .map(|me| {
                let players: Vec<Option<NetAddr>> = (0..n).map(|p| (p != me).then_some(NetAddr::Seat(p as u8))).collect();
                let pipe = Pipe { me: me as u8, wire: wire.clone(), now: now.clone(), rng: me as u32 + 1 };
                let mut net = NetPlay::p2p(pipe, &players).unwrap();
                net.peers_loading = false;
                (net, game.clone(), Vec::new())
            })
            .collect();
        let mut rollbacks = 0;
        for tick in 0..frames as u32 * 4 {
            now.set(tick);
            for (me, (net, g, sums)) in peers.iter_mut().enumerate() {
                for r in net.tick(&[pad(me, tick)]) {
                    let advance = matches!(r, GgrsRequest::AdvanceFrame { .. });
                    rollbacks += usize::from(matches!(r, GgrsRequest::LoadGameState { .. }));
                    apply(net, g, vec![r], &mut |g, pads| step(g, pads));
                    if advance {
                        let f = net.frame as usize;
                        sums.resize(sums.len().max(f), 0);
                        sums[f - 1] = g.checksum();
                    }
                }
                assert!(net.gone.is_none() && !net.status.starts_with("DESYNC"), "computer {me}: {}", net.status);
            }
            if peers.iter().all(|p| p.2.len() >= frames + MAX_PREDICTION + INPUT_DELAY) {
                break;
            }
        }
        (peers.into_iter().map(|p| p.2).collect(), rollbacks)
    }

    /// Every computer saw the same match: the checksums of its first `frames` frames alike, after the rollbacks.
    fn same_everywhere(sums: &[Vec<u64>], rollbacks: usize, frames: usize) {
        assert!(sums.iter().all(|s| s.len() >= frames), "frames played: {:?}", sums.iter().map(Vec::len).collect::<Vec<_>>());
        assert!(rollbacks > 100, "rollbacks: {rollbacks}");
        for (k, s) in sums.iter().enumerate().skip(1) {
            if let Some(f) = (0..frames).find(|&f| s[f] != sums[0][f]) {
                panic!("computer {k} differs from the first at frame {f}");
            }
        }
        println!("{frames} frames alike on {} computers, {rollbacks} rollbacks", sums.len());
    }

    fn ttt_assets() -> Option<kumite_assets::Assets> {
        let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
            eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
            return None;
        };
        Some(kumite_assets::Assets::new(std::path::Path::new(&root)).unwrap())
    }

    /// Paul's, Lei's, Xiaoyu's and Jin's first models.
    const TTT_MODELS: [usize; 4] = [345, 372, 432, 351];

    /// Tekken Tag's VS BATTLE online: two pairs, two computers, a jittered latency: the same match on both, frame
    /// by frame, through the rollbacks.
    #[test]
    fn a_tekken_tag_battle_online_is_the_same_on_both_computers() {
        let Some(a) = ttt_assets() else { return };
        let [p1, p2, b1, b2] = TTT_MODELS;
        let mut g = kumite_game::Game::load(&a, [p1, p2]).unwrap();
        g.start_tag([kumite_game::Actor::load(&a, b1).unwrap(), kumite_game::Actor::load(&a, b2).unwrap()]);
        let bout = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, true);
        let (sums, rollbacks) = play(bout, 2, 1500, &|b, p| b.step([p[0], p[1]]));
        same_everywhere(&sums, rollbacks, 1500);
    }

    /// Tekken Tag's game root with the Tekken 3 slice (its extras: Ball, Force) installed from both games' data, as
    /// `kumite_game`'s crossover tests do; needs OPENKUMITE_ASSETS too.
    fn ttt_slice_assets() -> Option<kumite_assets::Assets> {
        let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
            eprintln!("skipped: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
            return None;
        };
        use ok_converter_common::io::StdFs;
        static EXTRAS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        let extras = EXTRAS.get_or_init(|| {
            let dir = std::env::temp_dir().join("openkumite_netplay_extras");
            kumite_packs::install_ttt(&StdFs(ttt.clone().into()), &StdFs(t3.into()), &StdFs(dir.clone()), &|l| eprintln!("{l}")).unwrap();
            dir
        });
        kumite_fs::set_layers(std::path::Path::new(&ttt), vec![extras.clone()]);
        Some(kumite_assets::Assets::new(std::path::Path::new(&ttt)).unwrap())
    }

    /// TEKKEN BALL online in Tekken Tag (the slice's ball.json, as the app sets it up): Paul against Jin, two
    /// computers.
    #[test]
    fn a_tekken_tag_ball_match_online_is_the_same_on_both_computers() {
        let Some(a) = ttt_slice_assets() else { return };
        let mut g = kumite_game::Game::load(&a, [TTT_MODELS[0], TTT_MODELS[3]]).unwrap();
        g.load_camera(&a.root.join("gamedata/ball_camera.json")).unwrap();
        g.ball = Some(kumite_game::BallSetup { data: std::sync::Arc::new(a.gamedata("ball.json").unwrap()), kind: 0, server: 0, rounds: 0 });
        g.round_frames = u32::MAX / 2;
        (g.ko_collapse, g.fight.ko_collapse) = (false, false);
        let bout = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
        let (sums, rollbacks) = play(bout, 2, 1500, &|b, p| b.step([p[0], p[1]]));
        same_everywhere(&sums, rollbacks, 1500);
    }

    /// Tekken 3's TEKKEN BALL online: Gon against Jin, two computers.
    #[test]
    fn a_tekken_3_ball_match_online_is_the_same_on_both_computers() {
        let Ok(a) = kumite_assets::Assets::new(crate::test_assets()) else {
            eprintln!("skipped: no Tekken 3 assets");
            return;
        };
        let mut g = kumite_game::Game::load(&a, [239, 143]).unwrap();
        g.ball = Some(kumite_game::BallSetup { data: std::sync::Arc::new(a.gamedata("ball.json").unwrap()), kind: 0, server: 1, rounds: 0 });
        g.round_frames = u32::MAX / 2;
        let bout = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
        let (sums, rollbacks) = play(bout, 2, 1500, &|b, p| b.step([p[0], p[1]]));
        same_everywhere(&sums, rollbacks, 1500);
    }

    /// Force online, stage 1 of `a`'s force.json: 1P (`models[0]`, selections `stride` a character) with the fight
    /// and the stage in the rollback, as the app's Force online; the co-op players (`models[1..]`: 2P, 3P, 4P) each
    /// on a computer of their own, every computer's pad the partner's (or 3P's, 4P's).
    fn force_online(a: &kumite_assets::Assets, models: &[usize], stride: usize, computers: usize) {
        use kumite_game::force::{ForceData, ForceEvent, ForceStage};
        let data: std::sync::Arc<ForceData> = std::sync::Arc::new(a.gamedata("force.json").unwrap());
        let roster: serde_json::Value = a.gamedata("roster.json").unwrap();
        let soldier = |k: usize| {
            let sel = data.soldier_selection_base + data.soldier_costumes[0][k].min(3);
            let e = roster["selections"]["entries"].as_array().unwrap().iter().find(|e| e["selection"] == sel).unwrap();
            e["model_record"].as_u64().unwrap() as usize
        };
        let soldiers = [0, 1].map(|k| kumite_game::Actor::load(a, soldier(k)).unwrap());
        let mut g = kumite_game::Game::load(a, [models[0], soldier(0)]).unwrap();
        g.round_frames = kumite_game::force::CLOCK;
        (g.ko_collapse, g.fight.ko_collapse) = (false, false);
        let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 1, false);
        b.force = true;
        b.set_cpu(1, Some((1, data.ai_level(0))));
        b.extra_cpu = Some(kumite_sim::ai::Ai::new(1, data.ai_level(0), 1));
        let body = b.game.fight.fighters[0].character.unwrap_or(0).max(0) as usize;
        let mut st = ForceStage::new(data.clone(), 0, soldiers, body * stride, body, 0, 0, 0, 0);
        st.setup(&mut b.game);
        // (the players join as the app's do: 2P, the soldiers' places widened, then 3P and 4P)
        if let Some(&m) = models.get(1) {
            b.game.join_partner(&kumite_game::Actor::load(a, m).unwrap());
            st.widen(&mut b.game);
            let n = b.game.fight.more.len();
            b.more_cpu = (4..4 + n).map(|k| (!kumite_sim::Fight::is_player(k)).then(|| kumite_sim::ai::Ai::new(1, data.ai_level(0), 1))).collect();
            b.more_human = vec![None; n];
        }
        for (p, &m) in models.iter().enumerate().skip(2) {
            b.game.join_player(p, &kumite_game::Actor::load(a, m).unwrap());
        }
        assert_eq!(b.game.players().len(), models.len());
        let (sums, rollbacks) = play(ForceState(b, st), computers, 1500, &|s, p| {
            s.0.partner_pad = p[1];
            for k in 0..2 {
                if let (Some(slot), Some(&pad)) = (s.0.more_human.get_mut(k), p.get(2 + k)) {
                    *slot = Some(pad);
                }
            }
            s.0.step([p[0], Pad::default()]);
            let fighting = s.0.phase == kumite_game::bout::Phase::Fight;
            let won = matches!(s.0.phase, kumite_game::bout::Phase::RoundOver { winner: Some(0) } | kumite_game::bout::Phase::MatchOver { winner: Some(0) });
            s.1.step(&mut s.0.game, fighting, won);
            // (stage 1's first area: no boss to load)
            if std::mem::take(&mut s.1.events).iter().any(|e| matches!(e, ForceEvent::Swapped)) {
                s.0.swap_cpus();
            }
        });
        same_everywhere(&sums, rollbacks, 1500);
    }

    /// TEKKEN FORCE online in Tekken Tag (the slice's force.json, stage 1): Lei with the fight and the stage in the
    /// rollback, as the app's Force online; 1P's computer plays him, the other's pad the partner's.
    #[test]
    fn a_tekken_tag_force_stage_online_is_the_same_on_both_computers() {
        let Some(a) = ttt_slice_assets() else { return };
        force_online(&a, &[TTT_MODELS[1]], 8, 2);
    }

    /// TEKKEN FORCE's co-op of four in Tekken Tag: Lei, Paul, Xiaoyu and Jin on four computers.
    #[test]
    fn a_tekken_tag_force_co_op_of_four_online_is_the_same_on_every_computer() {
        let Some(a) = ttt_slice_assets() else { return };
        let [p, l, x, j] = TTT_MODELS;
        force_online(&a, &[l, p, x, j], 8, 4);
    }

    /// Tekken Force's co-op of four in Tekken 3: Paul, Law, King and Xiaoyu on four computers.
    #[test]
    fn a_tekken_3_force_co_op_of_four_online_is_the_same_on_every_computer() {
        let root = crate::test_assets();
        let Ok(a) = kumite_assets::Assets::new(&root) else {
            eprintln!("skipped: no Tekken 3 assets");
            return;
        };
        force_online(&a, &[71, 79, 95, 127], 4, 4);
    }

    /// TAG BRAWL online in Tekken Tag: four fighters, four computers, two on two.
    #[test]
    fn a_tekken_tag_brawl_online_is_the_same_on_every_computer() {
        let Some(a) = ttt_assets() else { return };
        let slots = TTT_MODELS.map(|record| kumite_game::tag::Slot { record, cpu: None });
        let mut g = kumite_game::tag::TagGame::load(&a, slots, vec![0, 0, 1, 1], kumite_game::ai_data(&a).unwrap(), 2).unwrap();
        g.set_arena(1);
        let (sums, rollbacks) = play(g, 4, 1200, &|g, p| g.step(std::array::from_fn(|k| p[k])));
        same_everywhere(&sums, rollbacks, 1200);
    }
}
