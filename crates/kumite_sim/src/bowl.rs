//! Tekken Bowl (Tekken Tag Tournament's bowling, state machine 0x3F0008; the Tekken Tag converter's
//! docs/BOWL_ENDINGS.md sections 1-5, `gamedata/bowl.json`): one or two players take turns; a turn is the throw
//! (position, aim and spin, power, the run-up and the release at the throw motion's frame 106; OVER CHARGED's charge
//! down the lane), the ball (its fall, bounce and hook), the ten pins, the spectators the ball can knock over, then
//! the count, ten-pin scoring and the callouts. Worked in the game's units and axes (x, y down, z down the lane), in
//! f32 with only reproducible operations (the trigonometry through `detmath`, no SIMD types), the game's random
//! draws from a seeded generator: the same pads give the same game.
//!
//! Decoded: the throw and its meters, the release, the ball, the rack, the deck walls, the spectators' test, the end
//! of a roll, the count (a pin is down when it lies on the lane or its base left its spot), scoring and callouts.
//! OpenKumite's own: the pins' collision response (the original's 0x3EDC90 / 0x3EDF88 are not decoded): each pin
//! three spheres along its axis and a base, hit by impulses at the contact with friction, in the decoded structure
//! (gravity 9.8, the ground's bounce, the walls' 0.4, a pin moving or at rest); where the hand lets the ball go and
//! how far the charge runs come from the motions (the game layer's [`Bowler::hand`] and [`Bowler::charge`]).

use crate::detmath::DetMath;
use glam::{Mat3, Vec3};
use serde::Deserialize;

/// The throw motion's frames and the one the ball leaves the hand on (phase 4).
pub const THROW_FRAMES: u32 = 150;
pub const RELEASE_FRAME: u32 = 106;
/// The charge (phase 6, motion 0x6A..): its frames, then the motion after it (0x6E..).
pub const CHARGE_FRAMES: u32 = 175;
/// OVER CHARGED's message (phase 5) and a spectator's K.O. (state 4).
const MESSAGE_FRAMES: u32 = 60;
const KO_FRAMES: u32 = 180;
const KO_VOICE_AT: u32 = 90;
/// How long a strike's pose (states 5-7) and a count's callout stay (OpenKumite's: the pose's own length is not
/// decoded; START skips).
pub const STRIKE_FRAMES: u32 = 150;
const RESULT_FRAMES: u32 = 90;
/// The bowler's line, the step and turn a frame and their limits (1/65536 turns), the run-up's steering.
const BOWLER_Z: f32 = -11000.0;
const STEP: f32 = 8.0;
const X_LIMIT: f32 = 1000.0;
const TURN: i32 = 0x10;
const TURN_LIMIT: i32 = 0x800;
const STEER: f32 = 1280.0;
/// The ball (0x3EC768): its radius, the lane's height for its centre, its fall in the air, the bounce, the hook's
/// start and length, the speed from which the hook keeps the speed, where a roll ends (a charge's).
const BALL_RADIUS: f32 = 150.0;
const LANE_Y: f32 = -150.0;
const FALL_Y: f32 = 2.45;
const BALL_GRAVITY: f32 = 4.9;
const BOUNCE: f32 = -0.6;
const HOOK_START: f32 = -11000.0;
const HOOK_LENGTH: f32 = 15000.0;
const HOOK_KEEPS_FROM: f32 = 20.0;
const ROLL_ENDS: f32 = 12000.0;
const ROLL_FRAMES: (u32, u32) = (150, 1000);
/// A first touch slower than this sounds 0x10AA, else 0x10AB.
const TOUCH_FAST: f32 = 270.0;
const TOUCH: [u16; 2] = [0x10AA, 0x10AB];
/// The pins: gravity, the deck walls (from z, at +-x, the back wall) and their restitution, the down test's radius.
const PIN_GRAVITY: f32 = 9.8;
const WALLS_FROM: f32 = 9435.0;
const WALL_X: f32 = 2550.0;
const BACK_WALL: f32 = 12691.0;
const WALL_BOUNCE: f32 = 0.4;
const DOWN_RADIUS2: f32 = 5000.0;
/// The pin sounds (0x3EC200): by the sum of the pins' 5-frame hit timers, each with its quiet frames after.
const PIN_SOUNDS: [(u32, u16, u32); 3] = [(15, 0x10AE, 8), (30, 0x10AD, 14), (u32::MAX, 0x10AC, 30)];
/// The spectators' test (0x3EFDB8): a ball within 300 of one (horizontally) knocks it over; voice 0x1059.
const SPECTATOR_RADIUS2: f32 = 90000.0;
const KO_VOICE: u16 = 0x1059;
/// The strike announcer (0x3EF8B0): by the run of strikes (1, 3, 12, else), then 0x10B5.
const STRIKE_CALLS: [(u32, u16); 3] = [(1, 0x10B2), (3, 0x1046), (12, 0x1058)];
const STRIKE_CALL: u16 = 0x1057;
const STRIKE_AFTER: u16 = 0x10B5;
/// SPLIT: two standing pins more than this apart across the lane with none between (0x3EA3A0).
const SPLIT_GAP: f32 = 500.0;

// OpenKumite's pin body (not decoded): mass against the ball's, the spheres along its axis from the base (height,
// radius), its base's rim, the inertia of a cylinder of its size, the ground's friction and damping, at rest when
// this slow for this many frames; four sub-steps a frame for the contacts.
const PIN_HEIGHT: f32 = 661.0;
const PIN_SPHERES: [(f32, f32); 3] = [(80.0, 80.0), (250.0, 120.0), (520.0, 70.0)];
const BASE_RIM: f32 = 70.0;
const PIN_MASS: f32 = 1.0;
const BALL_MASS: f32 = 4.5;
const PIN_INERTIA: [f32; 2] = [38800.0, 5000.0];
const RESTITUTION: f32 = 0.4;
const FRICTION: f32 = 0.3;
const PIN_RESTITUTION: f32 = 0.5;
const REST_SPEED: f32 = 2.0;
const REST_SPIN: f32 = 0.01;
const REST_FRAMES: u32 = 10;
const SUBSTEPS: u32 = 4;
/// OVER CHARGED's runner as a body against the pins (OpenKumite's): a sphere of this radius at this height.
const BODY: (f32, f32) = (350.0, -500.0);

/// One character's bowling (0x523618, 0x523590, 0x5236A0).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Character {
    pub id: usize,
    pub s: i32,
    pub left: bool,
    pub female: bool,
    /// The strike reactions (file 128 motions).
    pub reactions: Vec<u8>,
    /// Two throw voices and the charge's.
    pub voices: Vec<u16>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Person {
    pub x: f32,
    pub z: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct People {
    pub people: Vec<Person>,
}

/// The rack (0x3EA050): the pins' centre height and spots (x, z).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Rack {
    pub y: f32,
    pub rack: Vec<[f32; 2]>,
}

/// The texts shown (0x523A18, 0x5F69D8, 0x5F6A00).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Callouts {
    pub strikes: Vec<String>,
    pub punch_out: String,
    pub perfect: String,
    pub miss: String,
    pub foul: String,
    pub spare: String,
    pub nice_cover: String,
    pub split: String,
    pub ko: String,
    pub over_charged: String,
}

/// gamedata/bowl.json (what the simulation reads of it).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BowlData {
    pub characters: Vec<Character>,
    pub pins: Rack,
    pub spectators: People,
    pub callouts: Callouts,
}

/// The pad's CROSS and CIRCLE: buttons 3 and 4 (the default layout's), taken on the frame they go down.
pub const CROSS: u8 = 1 << 2;
pub const CIRCLE: u8 = 1 << 3;

/// What the current player's pad does this frame: the d-pad held, the buttons gone down, START pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pad {
    left: bool,
    right: bool,
    cross: bool,
    circle: bool,
    start: bool,
}

/// A bowler: the stats it bowls with and what the motions give the throw.
#[derive(Clone, Debug, PartialEq)]
pub struct Bowler {
    /// The character shown, and the one whose stats it bowls with (Mokujin's and Unknown's copy).
    pub character: usize,
    pub stats: usize,
    pub s: f32,
    pub left: bool,
    pub female: bool,
    /// Gun Jack and Jack-2 (movesets 0x10, 0x13): the narrow spin meter, its slower speed.
    pub jack: bool,
    pub voices: [u16; 3],
    /// Where the hand lets the ball go, from the bowler's spot facing down the lane (game units: the throw motion's
    /// frame 106 with its root's travel).
    pub hand: Vec3,
    /// The charge motion's own travel down the lane, frame by frame.
    pub charge: Vec<f32>,
}

impl Bowler {
    /// Character `character` bowling with `stats`'s numbers (itself, or whom Mokujin and Unknown copy; Unknown is
    /// left-handed unless she copies Yoshimitsu, 0x3E9E90).
    pub fn new(data: &BowlData, character: usize, stats: usize) -> Self {
        let c = data.characters.iter().find(|c| c.id == stats).cloned().unwrap_or_default();
        let left = if character == 33 { stats != 4 } else { c.left };
        let voices = data.characters.iter().find(|c| c.id == character).map_or(c.voices.clone(), |c| c.voices.clone());
        let voice = |k: usize| voices.get(k).copied().unwrap_or(0);
        Self {
            character,
            stats,
            s: c.s.max(1) as f32,
            left,
            female: data.characters.iter().find(|c| c.id == character).is_some_and(|c| c.female),
            jack: matches!(stats, 16 | 21),
            voices: [voice(0), voice(1), voice(2)],
            hand: Vec3::new(if left { -150.0 } else { 150.0 }, -500.0, 4200.0),
            charge: Vec::new(),
        }
    }

    /// S' (0x3E9C58): S, above 400 a third of the rest.
    fn s2(&self) -> f32 {
        if self.s > 400.0 {
            400.0 + 0.3 * (self.s - 400.0)
        } else {
            self.s
        }
    }
}

/// A ball's mark (score card +0x27).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mark {
    #[default]
    None,
    Strike,
    Spare,
    Miss,
    Foul,
    Split,
}

/// Where a card stands: the frame (0-9), the ball in it, whether that ball meets a fresh rack, the card is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    pub frame: usize,
    pub ball: usize,
    pub fresh: bool,
    pub done: bool,
}

/// A player's score card (0x1FC61E0): the pins of each ball and its mark.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Card {
    pub rolls: Vec<u8>,
    pub marks: Vec<Mark>,
}

impl Card {
    /// The next ball (0x3EA2B0): ten-pin's frames; the tenth's extra balls after a strike or a spare.
    pub fn at(&self) -> Turn {
        let r = &self.rolls;
        let get = |i: usize| r.get(i).copied().unwrap_or(0);
        let mut i = 0;
        for frame in 0..9 {
            if i >= r.len() {
                return Turn { frame, ball: 0, fresh: true, done: false };
            }
            if get(i) >= 10 {
                i += 1;
                continue;
            }
            if i + 1 >= r.len() {
                return Turn { frame, ball: 1, fresh: false, done: false };
            }
            i += 2;
        }
        let (a, b) = (get(i), get(i + 1));
        let turn = |ball, fresh| Turn { frame: 9, ball, fresh, done: false };
        match r.len() - i.min(r.len()) {
            0 => turn(0, true),
            1 => turn(1, a >= 10),
            2 if a >= 10 || a + b >= 10 => turn(2, (a >= 10 && b >= 10) || (a < 10 && a + b >= 10)),
            _ => Turn { frame: 9, ball: 3, fresh: false, done: true },
        }
    }

    /// The running total after each frame (None: not known yet).
    pub fn totals(&self) -> [Option<u16>; 10] {
        let r = &self.rolls;
        let get = |i: usize| r.get(i).map(|&p| u16::from(p));
        let mut out = [None; 10];
        let (mut i, mut sum) = (0, 0u16);
        for (frame, slot) in out.iter_mut().enumerate() {
            let Some(a) = get(i) else { break };
            let score = if a >= 10 {
                i += 1;
                get(i).zip(get(i + 1)).map(|(b, c)| 10 + b + c)
            } else {
                let Some(b) = get(i + 1) else { break };
                i += 2;
                if a + b >= 10 {
                    get(i).map(|c| 10 + c)
                } else {
                    Some(a + b)
                }
            };
            let Some(score) = score else { break };
            sum += score;
            *slot = Some(sum);
            if frame == 9 {
                break;
            }
        }
        out
    }

    pub fn total(&self) -> u16 {
        self.totals().iter().flatten().last().copied().unwrap_or(0)
    }

    /// The frame `f`'s balls (index into `rolls`).
    pub fn frame_balls(&self, f: usize) -> std::ops::Range<usize> {
        let mut i = 0;
        for k in 0..f.min(9) {
            if i >= self.rolls.len() {
                break;
            }
            i += if self.rolls.get(i).is_some_and(|&p| p >= 10) && k < 9 { 1 } else { 2 };
        }
        let n = if f >= 9 {
            3
        } else if self.rolls.get(i).is_some_and(|&p| p >= 10) {
            1
        } else {
            2
        };
        i.min(self.rolls.len())..(i + n).min(self.rolls.len())
    }
}

/// A pin (0x1FC5260, 0x180 bytes): its centre, velocity, orientation (its axis is the column -y), spin.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Mat3,
    pub spin: Vec3,
    /// +0x170 standing, +0x174 on the deck (not swept), +0x178 moving.
    pub standing: bool,
    pub on_deck: bool,
    pub moving: bool,
    rest: u32,
    /// +0x17E: the clatter timer a hit sets.
    clatter: u32,
}

/// The ball (0x1FC51E0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ball {
    pub pos: Vec3,
    pub vel: Vec3,
    pub spin: Vec3,
    /// The picture's roll: angles and their speeds (radians).
    pub roll: Vec3,
    pub roll_speed: Vec3,
    pub rolling: bool,
    pub shown: bool,
    pub landed: bool,
}

/// The bowler's part of the turn (throw phases, 0x1FC62A0 +0; then the states of 0x3F0008).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 1: stepping left and right.
    Position,
    /// 2: turning; the spin meter swings.
    Aim,
    /// 3: the power meter.
    Power,
    /// 4: the throw motion's frame (1-150); the ball goes at 106.
    Throw(u32),
    /// 5: OVER CHARGED shown.
    OverCharged(u32),
    /// 6: the charge's frame (motion 0x6A.., then 0x6E..).
    Charge(u32),
    /// State 4: a spectator knocked over.
    Ko(u32),
    /// States 5-7: the strike pose.
    Strike(u32),
    /// The count's callout before the next ball.
    Result(u32),
    /// State 10: every card done.
    Over,
}

/// Something to hear: a sound entry of sound.json (`kind << 12 | id`), a player's (its voices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Sound(u16),
    Voice { player: usize, id: u16 },
}

/// One game of Tekken Bowl.
#[derive(Clone, Debug)]
pub struct Bowl {
    pub players: Vec<Bowler>,
    pub cards: Vec<Card>,
    /// Strikes in a row, per player.
    pub runs: Vec<u32>,
    pub current: usize,
    pub phase: Phase,
    /// The bowler's x and angle (1/65536 turns), the meters (0x1FC62A0 +4..+0x10).
    pub x: f32,
    pub angle: i32,
    pub spin: f32,
    pub spin_speed: f32,
    pub power: f32,
    pub power_speed: f32,
    pub ball: Ball,
    pub pins: Vec<Pin>,
    /// Frames since the ball left the hand (or the charge began).
    pub rolled: Option<u32>,
    /// The ball's speed at the release (the SPEED display's).
    pub speed: f32,
    /// The text shown now (a callout, OVER CHARGED, K.O.), and on which side a K.O. was.
    pub callout: Option<String>,
    pub ko_side: f32,
    pub events: Vec<Event>,
    pub frame: u64,
    data_rack: Vec<[f32; 2]>,
    pin_y: f32,
    people: Vec<[f32; 2]>,
    callouts: Callouts,
    standing_before: [bool; 10],
    /// The roll's mark, its pins and the next ball once counted.
    counted: Option<(u8, Mark)>,
    foul: bool,
    split_first: bool,
    quiet: u32,
    /// The buttons held last frame.
    held: u8,
    seed: u32,
}

/// The game's units of an angle (65536 a turn) in radians.
fn radians(a: i32) -> f32 {
    a as f32 * std::f32::consts::TAU / 65536.0
}

/// Rotation about the unit `axis` by `angle` (Rodrigues; detmath's sine and cosine).
fn axis_angle(axis: Vec3, angle: f32) -> Mat3 {
    let (s, c) = angle.dsin_cos();
    let t = 1.0 - c;
    let Vec3 { x, y, z } = axis;
    Mat3::from_cols(
        Vec3::new(t * x * x + c, t * x * y + s * z, t * x * z - s * y),
        Vec3::new(t * x * y - s * z, t * y * y + c, t * y * z + s * x),
        Vec3::new(t * x * z + s * y, t * y * z - s * x, t * z * z + c),
    )
}

/// `m` made orthonormal again (Gram-Schmidt from its y column: the pin's axis).
fn orthonormal(m: Mat3) -> Mat3 {
    let y = m.y_axis.normalize_or(Vec3::NEG_Y);
    let x = (m.x_axis - y * m.x_axis.dot(y)).normalize_or(Vec3::X);
    Mat3::from_cols(x, y, x.cross(y))
}

impl Pin {
    fn racked(spot: [f32; 2], y: f32) -> Self {
        Self { pos: Vec3::new(spot[0], y, spot[1]), vel: Vec3::ZERO, rot: Mat3::IDENTITY, spin: Vec3::ZERO, standing: true, on_deck: true, moving: false, rest: 0, clatter: 0 }
    }

    /// The pin's axis towards its head (y is down).
    pub fn up(&self) -> Vec3 {
        -self.rot.y_axis
    }

    /// Its base's centre (the centre stands `base` above it).
    pub fn base(&self, base: f32) -> Vec3 {
        self.pos - self.up() * base
    }

    /// The inverse inertia in the world (a cylinder about its axis).
    fn inv_inertia(&self) -> Mat3 {
        let local = Mat3::from_diagonal(Vec3::new(1.0 / PIN_INERTIA[0], 1.0 / PIN_INERTIA[1], 1.0 / PIN_INERTIA[0]));
        self.rot * local * self.rot.transpose()
    }

    /// An impulse `j` at the point `at` (world).
    fn push(&mut self, j: Vec3, at: Vec3) {
        self.vel += j / PIN_MASS;
        self.spin += self.inv_inertia() * (at - self.pos).cross(j);
        self.moving = true;
        self.rest = 0;
    }

    /// The velocity of its point `at`.
    fn velocity_at(&self, at: Vec3) -> Vec3 {
        self.vel + self.spin.cross(at - self.pos)
    }

    /// Its spheres' centres and radii.
    fn spheres(&self, base: f32) -> [(Vec3, f32); 3] {
        let (b, up) = (self.base(base), self.up());
        PIN_SPHERES.map(|(h, r)| (b + up * h, r))
    }
}

/// The impulse that stops `v` (the relative velocity at the contact) along `n` with `bounce`, and its friction,
/// for a pin (inverse mass, inverse inertia, lever `r`) against an immovable body.
fn contact_impulse(v: Vec3, n: Vec3, r: Vec3, inv_inertia: Mat3, inv_mass: f32, bounce: f32) -> Vec3 {
    let vn = v.dot(n);
    if vn >= 0.0 {
        return Vec3::ZERO;
    }
    let k = inv_mass + (inv_inertia * r.cross(n)).cross(r).dot(n);
    let jn = -(1.0 + bounce) * vn / k.max(1e-6);
    let vt = v - n * vn;
    let t = vt.normalize_or_zero();
    let kt = inv_mass + (inv_inertia * r.cross(t)).cross(r).dot(t);
    let jt = (vt.length() / kt.max(1e-6)).min(FRICTION * jn);
    n * jn - t * jt
}

impl Bowl {
    /// A game for `players` (one or two, in turn order), from `seed`.
    pub fn new(data: &BowlData, players: Vec<Bowler>, seed: u32) -> Self {
        let n = players.len().max(1);
        let rack: Vec<[f32; 2]> = (0..10).map(|k| data.pins.rack.get(k).copied().unwrap_or([0.0, 8000.0])).collect();
        let pin_y = if data.pins.y < 0.0 { data.pins.y } else { -250.66 };
        let people = data.spectators.people.iter().map(|p| [p.x, p.z]).collect();
        let mut b = Self {
            players,
            cards: vec![Card::default(); n],
            runs: vec![0; n],
            current: 0,
            phase: Phase::Position,
            x: 0.0,
            angle: 0,
            spin: 0.0,
            spin_speed: 0.0,
            power: 0.0,
            power_speed: 0.0,
            ball: Ball::default(),
            pins: rack.iter().map(|&s| Pin::racked(s, pin_y)).collect(),
            rolled: None,
            speed: 0.0,
            callout: None,
            ko_side: 0.0,
            events: Vec::new(),
            frame: 0,
            data_rack: rack,
            pin_y,
            people,
            callouts: data.callouts.clone(),
            standing_before: [true; 10],
            counted: None,
            foul: false,
            split_first: false,
            quiet: 0,
            held: 0,
            seed,
        };
        b.new_turn();
        b
    }

    /// The game's rand(): a uniform draw in [0, 1).
    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.seed >> 8) as f32 / 16_777_216.0
    }

    fn bowler(&self) -> Option<&Bowler> {
        self.players.get(self.current)
    }

    /// The pin's centre height above its base.
    fn base(&self) -> f32 {
        -self.pin_y
    }

    /// State 0 (0x3F00FC, 0x3E9C58): the bowler set up, the meters started, the ball in hand.
    fn new_turn(&mut self) {
        let Some(b) = self.bowler().cloned() else { return };
        self.x = (self.random() * 128.0).floor() - 64.0;
        self.angle = 0;
        let s2 = b.s2();
        self.spin = self.random() * 2.0 - 1.0;
        let sign = if self.random() < 0.5 { -1.0 } else { 1.0 };
        self.spin_speed = sign * (s2 * if b.jack { 0.048 } else { 0.06 } / 200.0 + 0.03);
        self.power = 0.0;
        self.power_speed = 2.0 * (s2 + 180.0) / 340.0;
        self.ball = Ball { shown: true, ..Ball::default() };
        self.rolled = None;
        self.phase = Phase::Position;
        self.callout = None;
        self.counted = None;
        self.foul = false;
        self.standing_before = std::array::from_fn(|k| self.pins.get(k).is_some_and(|p| p.standing));
    }

    /// The bowler's spot (game units) and facing (radians).
    pub fn bowler_at(&self) -> (Vec3, f32) {
        let z = match self.phase {
            Phase::Charge(f) => BOWLER_Z + charge_distance(f),
            _ => BOWLER_Z,
        };
        (Vec3::new(self.x, 0.0, z), radians(self.angle))
    }

    /// Where the ball is while in the hand (the release point: the motion gives it).
    fn release_point(&self) -> Vec3 {
        let (at, a) = self.bowler_at();
        let hand = self.bowler().map_or(Vec3::new(150.0, -500.0, 4200.0), |b| b.hand);
        let (s, c) = a.dsin_cos();
        at + Vec3::new(hand.x * c + hand.z * s, hand.y, -hand.x * s + hand.z * c)
    }

    /// One frame (0x3F0008) with the current player's pad.
    pub fn step(&mut self, pad: crate::input::Pad) {
        let down = pad.buttons & !self.held;
        self.held = pad.buttons;
        let pad = Pad { left: pad.left, right: pad.right, cross: down & CROSS != 0, circle: down & CIRCLE != 0, start: pad.start };
        self.events.clear();
        self.frame += 1;
        let Some(b) = self.bowler().cloned() else { return };
        match self.phase {
            Phase::Position => {
                if pad.left != pad.right {
                    self.x = (self.x + if pad.left { -STEP } else { STEP }).clamp(-X_LIMIT, X_LIMIT);
                }
                if pad.cross {
                    self.phase = Phase::Aim;
                }
            }
            Phase::Aim => {
                if pad.left != pad.right {
                    self.angle = (self.angle + if pad.left { -TURN } else { TURN }).clamp(-TURN_LIMIT, TURN_LIMIT);
                }
                self.swing(&b);
                if pad.circle {
                    self.phase = Phase::Position;
                } else if pad.cross {
                    self.phase = Phase::Power;
                }
            }
            Phase::Power => {
                // the meter wraps past 100: CROSS on the very frame it is 0 is over charged, S / 2000 of the time
                if self.power < 100.0 {
                    self.power += self.power_speed;
                } else {
                    self.power = 0.0;
                }
                if pad.cross {
                    self.phase = if self.power == 0.0 && self.random() < b.s / 2000.0 {
                        self.callout = Some(self.callouts.over_charged.clone());
                        Phase::OverCharged(0)
                    } else {
                        Phase::Throw(1)
                    };
                }
            }
            Phase::Throw(f) => {
                if self.rolled.is_none() {
                    if pad.left != pad.right {
                        let d = STEER / b.s * if pad.left { -1.0 } else { 1.0 };
                        self.x = (self.x + d).clamp(-X_LIMIT, X_LIMIT);
                    }
                    if f >= RELEASE_FRAME {
                        self.release(&b);
                    }
                }
                self.phase = Phase::Throw((f + 1).min(THROW_FRAMES + 1));
            }
            Phase::OverCharged(f) => {
                self.phase = if f + 1 >= MESSAGE_FRAMES {
                    self.callout = None;
                    self.foul = true;
                    self.rolled = Some(0);
                    self.ball.shown = false;
                    Phase::Charge(0)
                } else {
                    Phase::OverCharged(f + 1)
                };
            }
            Phase::Charge(f) => {
                if f == 25 {
                    self.events.push(Event::Voice { player: self.current, id: b.voices[2] });
                }
                self.phase = Phase::Charge((f + 1).min(CHARGE_FRAMES + 75));
            }
            Phase::Ko(f) => {
                if f == KO_VOICE_AT {
                    self.events.push(Event::Sound(KO_VOICE));
                }
                if f + 1 >= KO_FRAMES {
                    self.count();
                } else {
                    self.phase = Phase::Ko(f + 1);
                }
            }
            Phase::Strike(f) => {
                if f + 1 >= STRIKE_FRAMES || pad.start {
                    self.next_ball();
                } else {
                    self.phase = Phase::Strike(f + 1);
                }
            }
            Phase::Result(f) => {
                if f + 1 >= RESULT_FRAMES || pad.start {
                    self.next_ball();
                } else {
                    self.phase = Phase::Result(f + 1);
                }
            }
            Phase::Over => {}
        }
        if let Some(r) = self.rolled.filter(|_| !matches!(self.phase, Phase::Ko(_) | Phase::Strike(_) | Phase::Result(_) | Phase::Over)) {
            self.roll(r);
        }
    }

    /// The spin meter (phase 2): it swings within the bowler's bounds.
    fn swing(&mut self, b: &Bowler) {
        let (lo, hi) = if b.jack {
            (-1.2, 1.2)
        } else if b.left {
            (-1.0, 2.0)
        } else {
            (-2.0, 1.0)
        };
        self.spin += self.spin_speed;
        if (self.spin > hi && self.spin_speed > 0.0) || (self.spin < lo && self.spin_speed < 0.0) {
            self.spin_speed = -self.spin_speed;
        }
    }

    /// The ball leaves the hand (phase 4 at frame 106): each meter off by up to its own speed.
    fn release(&mut self, b: &Bowler) {
        let spin = self.spin + self.spin_speed * (self.random() * 2.0 - 1.0);
        let power = (self.power + self.power_speed * (self.random() * 2.0 - 1.0)).clamp(0.0, 100.0);
        let speed = b.s * (power + 100.0) / 200.0;
        let a = radians(self.angle);
        let (s, c) = a.dsin_cos();
        self.ball = Ball {
            pos: self.release_point(),
            vel: Vec3::new(speed * s, 0.0, speed * c),
            spin: Vec3::new(spin * c, 0.0, -spin * s),
            roll: Vec3::new(0.0, a, 0.0),
            roll_speed: Vec3::new(-speed / 800.0, 0.0, spin / 2.0),
            rolling: true,
            shown: true,
            landed: false,
        };
        self.speed = speed;
        self.rolled = Some(0);
        let voice = b.voices[usize::from(self.random() < 0.5)];
        self.events.push(Event::Voice { player: self.current, id: voice });
    }

    /// The roll (state 3, 0x3EED88): the ball or the runner, the pins, the spectators; the decision (0x3EFB00).
    fn roll(&mut self, r: u32) {
        self.rolled = Some(r + 1);
        let body = match self.phase {
            Phase::Charge(f) => {
                let (at, _) = self.bowler_at();
                let travel = self.bowler().and_then(|b| b.charge.get(f.min(CHARGE_FRAMES - 1) as usize)).copied().unwrap_or(0.0);
                let next = self.bowler().and_then(|b| b.charge.get((f + 1).min(CHARGE_FRAMES - 1) as usize)).copied().unwrap_or(travel);
                let speed = charge_distance(f + 1) - charge_distance(f) + next - travel;
                Some((Vec3::new(at.x, BODY.1, at.z + travel), Vec3::new(0.0, 0.0, speed)))
            }
            _ => None,
        };
        for s in 0..SUBSTEPS {
            let dt = 1.0 / SUBSTEPS as f32;
            if self.ball.rolling {
                self.ball.pos += self.ball.vel * dt;
            }
            let hitter = match body {
                Some((at, v)) => Some((at + v * (dt * (s + 1) as f32 - 1.0), v, 0.0, BODY.0)),
                None => self.ball.rolling.then_some((self.ball.pos, self.ball.vel, 1.0 / BALL_MASS, BALL_RADIUS)),
            };
            self.pins_step(dt, hitter);
        }
        if self.ball.rolling {
            self.ball_step();
        }
        // the spectators (0x3EFDB8)
        if self.ball.rolling {
            let p = self.ball.pos;
            if let Some(&[x, _]) = self.people.iter().find(|q| (q[0] - p.x) * (q[0] - p.x) + (q[1] - p.z) * (q[1] - p.z) < SPECTATOR_RADIUS2) {
                self.ball.rolling = false;
                self.ball.shown = false;
                self.ko_side = x;
                self.callout = Some(self.callouts.ko.clone());
                self.phase = Phase::Ko(0);
                return;
            }
        }
        // pin sounds (0x3EC200)
        let sum: u32 = self.pins.iter().map(|p| p.clatter).sum();
        for p in &mut self.pins {
            p.clatter = p.clatter.saturating_sub(1);
        }
        if self.quiet > 0 {
            self.quiet -= 1;
        } else if sum > 0 {
            if let Some(&(_, id, quiet)) = PIN_SOUNDS.iter().find(|s| sum < s.0) {
                self.events.push(Event::Sound(id));
                self.quiet = quiet;
            }
        }
        // the decision (0x3EFB00): every pin at rest and the ball gone, after 150 frames at least, 1000 at most
        let charge = matches!(self.phase, Phase::Charge(_));
        let gone = if charge { matches!(self.phase, Phase::Charge(f) if f >= CHARGE_FRAMES) } else { !self.ball.rolling };
        let done = (gone && r + 1 >= ROLL_FRAMES.0 && self.pins.iter().all(|p| !p.moving)) || r + 1 >= ROLL_FRAMES.1;
        let thrown = !matches!(self.phase, Phase::Throw(f) if f <= THROW_FRAMES);
        if done && (thrown || r + 1 >= ROLL_FRAMES.1) {
            self.count();
        }
    }

    /// The ball's frame (0x3EC768 after the move): the fall, the touch, the hook; past the deck it drops out.
    fn ball_step(&mut self) {
        let b = &mut self.ball;
        b.roll += b.roll_speed;
        b.roll = Vec3::new(wrap(b.roll.x), wrap(b.roll.y), wrap(b.roll.z));
        if b.pos.y < LANE_Y {
            b.pos.y += FALL_Y;
            b.vel.y += BALL_GRAVITY;
        } else {
            if !b.landed {
                b.landed = true;
                let flat = (b.vel.x * b.vel.x + b.vel.z * b.vel.z).sqrt();
                self.events.push(Event::Sound(TOUCH[usize::from(flat >= TOUCH_FAST)]));
            }
            b.pos.y = LANE_Y;
            b.vel.y = if b.vel.y < BALL_GRAVITY { 0.0 } else { b.vel.y * BOUNCE };
        }
        let k = (b.pos.z - HOOK_START) / HOOK_LENGTH;
        let before = b.vel.length();
        b.vel += b.spin * (k * k);
        if b.vel.z >= HOOK_KEEPS_FROM {
            b.vel = b.vel.normalize_or_zero() * before;
        }
        wall(&mut b.pos, &mut b.vel, BALL_RADIUS);
        if b.pos.z > ROLL_ENDS {
            b.rolling = false;
            b.shown = false;
        }
    }

    /// The pins' sub-step: the hitter (centre, velocity, inverse mass, radius) against each pin on the deck, each
    /// pin moved, every pair, the lane and the walls.
    fn pins_step(&mut self, dt: f32, hitter: Option<(Vec3, Vec3, f32, f32)>) {
        let base = self.base();
        if let Some((c, v, inv_mass, radius)) = hitter {
            for p in self.pins.iter_mut().filter(|p| p.on_deck) {
                for (s, r) in p.spheres(base) {
                    let d = s - c;
                    let dist = d.length();
                    if dist >= radius + r || dist <= 1e-3 {
                        continue;
                    }
                    let n = d / dist;
                    let at = c + n * radius;
                    let rel = p.velocity_at(at) - v;
                    let vn = rel.dot(n);
                    p.pos += n * (radius + r - dist) * if inv_mass > 0.0 { PIN_MASS / (PIN_MASS + BALL_MASS) } else { 1.0 };
                    if vn < 0.0 {
                        let ra = at - p.pos;
                        let k = 1.0 / PIN_MASS + inv_mass + (p.inv_inertia() * ra.cross(n)).cross(ra).dot(n);
                        let j = -(1.0 + PIN_RESTITUTION) * vn / k;
                        p.push(n * j, at);
                        p.clatter = 5;
                        if inv_mass > 0.0 {
                            self.ball.vel -= n * (j * inv_mass);
                        }
                    }
                }
            }
        }
        for p in self.pins.iter_mut().filter(|p| p.moving && p.on_deck) {
            p.vel.y += PIN_GRAVITY * dt;
            p.pos += p.vel * dt;
            let w = p.spin.length();
            if w > 1e-6 {
                p.rot = orthonormal(axis_angle(p.spin / w, w * dt) * p.rot);
            }
        }
        // every pair
        for i in 0..self.pins.len() {
            for k in i + 1..self.pins.len() {
                let (a, b) = self.pins.split_at_mut(k);
                let (Some(p), Some(q)) = (a.get_mut(i), b.first_mut()) else { continue };
                if !(p.on_deck && q.on_deck) || !(p.moving || q.moving) || p.pos.distance_squared(q.pos) > PIN_HEIGHT * PIN_HEIGHT * 1.5 {
                    continue;
                }
                for (sp, rp) in p.spheres(base) {
                    for (sq, rq) in q.spheres(base) {
                        let d = sp - sq;
                        let dist = d.length();
                        if dist >= rp + rq || dist <= 1e-3 {
                            continue;
                        }
                        let n = d / dist;
                        let at = sq + n * rq;
                        let fix = n * ((rp + rq - dist) / 2.0);
                        p.pos += fix;
                        q.pos -= fix;
                        let rel = p.velocity_at(at) - q.velocity_at(at);
                        let vn = rel.dot(n);
                        if vn < 0.0 {
                            let (ra, rb) = (at - p.pos, at - q.pos);
                            let k = 2.0 / PIN_MASS + (p.inv_inertia() * ra.cross(n)).cross(ra).dot(n) + (q.inv_inertia() * rb.cross(n)).cross(rb).dot(n);
                            let j = -(1.0 + PIN_RESTITUTION) * vn / k;
                            p.push(n * j, at);
                            q.push(-n * j, at);
                            p.clatter = 5;
                            q.clatter = 5;
                        }
                    }
                }
            }
        }
        // the lane (0x3ED338) and the deck walls (0x3EE598)
        for p in self.pins.iter_mut().filter(|p| p.moving && p.on_deck) {
            let (b, up) = (p.base(base), p.up());
            let side = p.rot.x_axis;
            let front = p.rot.z_axis;
            let rim = [side, -side, front, -front].map(|d| b + d * BASE_RIM);
            let mut points: Vec<(Vec3, f32)> = rim.iter().map(|&q| (q, 0.0)).collect();
            points.extend(PIN_SPHERES.iter().map(|&(h, r)| (b + up * h, r)));
            points.push((b + up * PIN_HEIGHT, 0.0));
            // the deepest point lifted out once; an impulse at each point touching
            let touching: Vec<Vec3> = points.iter().filter(|(q, r)| q.y + r > 0.0).map(|(q, _)| *q).collect();
            let lift = points.iter().map(|(q, r)| q.y + r).fold(0.0f32, f32::max);
            p.pos.y -= lift;
            for q in &touching {
                let at = Vec3::new(q.x, 0.0, q.z);
                let j = contact_impulse(p.velocity_at(at), Vec3::NEG_Y, at - p.pos, p.inv_inertia(), 1.0 / PIN_MASS, RESTITUTION);
                p.vel += j / PIN_MASS;
                p.spin += p.inv_inertia() * (at - p.pos).cross(j);
            }
            if !touching.is_empty() {
                p.vel *= 1.0 - 0.08 * dt;
                p.spin *= 1.0 - 0.2 * dt;
            }
            let r = PIN_SPHERES[1].1;
            wall(&mut p.pos, &mut p.vel, r);
            // at rest: slow on the lane for a while
            let slow = !touching.is_empty() && p.vel.length() < REST_SPEED && p.spin.length() < REST_SPIN;
            p.rest = if slow { p.rest + 1 } else { 0 };
            if p.rest >= REST_FRAMES * SUBSTEPS {
                (p.moving, p.vel, p.spin) = (false, Vec3::ZERO, Vec3::ZERO);
            }
        }
    }

    /// The count (0x3EA4C8) once a roll is over: the fallen pins, the mark, the callout; a strike's pose.
    fn count(&mut self) {
        let base = self.base();
        let rack = self.data_rack.clone();
        for (k, p) in self.pins.iter_mut().enumerate() {
            let spot = rack.get(k).copied().unwrap_or_default();
            let b = p.base(base);
            let off = (b.x - spot[0]) * (b.x - spot[0]) + (b.z - spot[1]) * (b.z - spot[1]);
            let lying = p.spheres(base).iter().skip(1).any(|(c, r)| c.y + r > -2.0) || p.up().y > -0.7;
            if lying || off > DOWN_RADIUS2 {
                p.standing = false;
            }
        }
        let standing: [bool; 10] = std::array::from_fn(|k| self.pins.get(k).is_some_and(|p| p.standing));
        let knocked = if self.foul { 0 } else { (0..10).filter(|&k| self.standing_before[k] && !standing[k]).count() as u8 };
        let Some(card) = self.cards.get(self.current) else { return };
        let turn = card.at();
        let fresh = self.standing_before.iter().all(|&s| s);
        let left = self.standing_before.iter().filter(|&&s| s).count() as u8;
        let mark = if self.foul {
            Mark::Foul
        } else if fresh && knocked == 10 {
            Mark::Strike
        } else if !fresh && knocked == left && left > 0 {
            Mark::Spare
        } else if knocked == 0 {
            Mark::Miss
        } else if fresh && !standing[0] && self.split(&standing) {
            Mark::Split
        } else {
            Mark::None
        };
        let run = self.runs.get(self.current).copied().unwrap_or(0);
        let run = if mark == Mark::Strike { run + 1 } else { 0 };
        if let Some(r) = self.runs.get_mut(self.current) {
            *r = run;
        }
        let c = &self.callouts;
        let text = match mark {
            Mark::Strike if run >= 12 => Some(c.perfect.clone()),
            Mark::Strike if turn.frame == 9 && turn.ball == 2 && card.rolls.iter().rev().take(2).all(|&p| p >= 10) => Some(c.punch_out.clone()),
            Mark::Strike => c.strikes.get(run.clamp(1, 4) as usize - 1).cloned(),
            Mark::Spare if self.split_first => Some(c.nice_cover.clone()),
            Mark::Spare => Some(c.spare.clone()),
            Mark::Miss => Some(c.miss.clone()),
            Mark::Foul => Some(c.foul.clone()),
            Mark::Split => Some(c.split.clone()),
            Mark::None => None,
        };
        if !matches!(self.phase, Phase::Ko(_)) {
            self.callout = text;
        }
        if mark == Mark::Strike {
            let id = STRIKE_CALLS.iter().find(|s| s.0 == run).map_or(STRIKE_CALL, |s| s.1);
            self.events.extend([Event::Sound(id), Event::Sound(STRIKE_AFTER)]);
        }
        self.split_first = fresh && mark == Mark::Split;
        if let Some(card) = self.cards.get_mut(self.current) {
            card.rolls.push(knocked);
            card.marks.push(mark);
        }
        self.counted = Some((knocked, mark));
        self.ball.rolling = false;
        self.ball.shown = false;
        self.phase = if mark == Mark::Strike { Phase::Strike(0) } else { Phase::Result(0) };
    }

    /// SPLIT's test (0x3EA3A0): two standing pins more than 500 apart across the lane with none standing between.
    fn split(&self, standing: &[bool; 10]) -> bool {
        let mut xs: Vec<f32> = (0..10).filter(|&k| standing[k]).filter_map(|k| self.data_rack.get(k).map(|s| s[0])).collect();
        xs.sort_by(f32::total_cmp);
        xs.windows(2).any(|w| w[1] - w[0] > SPLIT_GAP)
    }

    /// The next ball (state 8, 0x3EA2B0): the fallen pins swept and the standing put back on their spots, or a fresh
    /// rack; the other player after a frame; the end once every card is done.
    fn next_ball(&mut self) {
        let foul = self.foul;
        let Some(card) = self.cards.get(self.current) else { return };
        let turn = card.at();
        if turn.done || turn.ball == 0 {
            // the next player's frame (round robin over the cards not done)
            let n = self.cards.len();
            if let Some(next) = (1..=n).map(|k| (self.current + k) % n).find(|&k| !self.cards[k].at().done) {
                self.current = next;
            } else {
                self.phase = Phase::Over;
                self.callout = None;
                return;
            }
        }
        let fresh = foul || self.cards.get(self.current).is_some_and(|c| c.at().fresh);
        let (rack, y) = (self.data_rack.clone(), self.pin_y);
        for (k, p) in self.pins.iter_mut().enumerate() {
            let spot = rack.get(k).copied().unwrap_or_default();
            *p = Pin { standing: fresh || p.standing, on_deck: fresh || p.standing, ..Pin::racked(spot, y) };
        }
        self.new_turn();
    }

    /// The winner (state 10): the higher total; None on a draw.
    pub fn winner(&self) -> Option<usize> {
        let totals: Vec<u16> = self.cards.iter().map(Card::total).collect();
        let best = totals.iter().copied().max()?;
        let mut top = totals.iter().enumerate().filter(|t| *t.1 == best);
        let first = top.next()?.0;
        top.next().is_none().then_some(first)
    }

    /// The SPEED display (0x3EAC88, "SPEED %3d.%02d MPH"): hundredths of a mile an hour.
    pub fn mph(&self) -> u32 {
        let v = self.speed * 0.216;
        let v = if v > 20.0 { v - 6.0 } else { v * 0.7 };
        (v * 0.625 * 100.0).max(0.0) as u32
    }

    /// The pins on the deck standing now.
    pub fn standing(&self) -> usize {
        self.pins.iter().filter(|p| p.on_deck && p.standing).count()
    }

    /// A hash of the whole state (determinism checks).
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u32| {
            h ^= u64::from(v);
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        let v3 = |v: Vec3| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
        for p in &self.pins {
            for v in [p.pos, p.vel, p.spin, p.rot.x_axis, p.rot.y_axis, p.rot.z_axis] {
                v3(v).into_iter().for_each(&mut eat);
            }
            eat(u32::from(p.standing) | u32::from(p.on_deck) << 1 | u32::from(p.moving) << 2);
        }
        for v in [self.ball.pos, self.ball.vel, self.ball.spin] {
            v3(v).into_iter().for_each(&mut eat);
        }
        for c in &self.cards {
            c.rolls.iter().for_each(|&r| eat(u32::from(r)));
        }
        [self.x.to_bits(), self.angle as u32, self.spin.to_bits(), self.power.to_bits(), self.seed, self.current as u32].into_iter().for_each(&mut eat);
        h
    }
}

/// The charge's own run (phase 6): 256 f to frame 25, 256 f - (f - 25)^2 to 50, an ease to 75, then 14825.
pub fn charge_distance(f: u32) -> f32 {
    let f = f as f32;
    if f < 25.0 {
        256.0 * f
    } else if f < 50.0 {
        256.0 * f - (f - 25.0) * (f - 25.0)
    } else if f < 75.0 {
        let t = (f - 50.0) / 25.0;
        12175.0 + (14825.0 - 12175.0) * (1.0 - (1.0 - t) * (1.0 - t))
    } else {
        14825.0
    }
}

/// The pin deck's walls (0x3EE2F8, 0x3EE598) on a body of radius `r` inside them: the sides past z 9435, the back.
// shortcut: the walls' 106-unit chamfers are left out
fn wall(pos: &mut Vec3, vel: &mut Vec3, r: f32) {
    if pos.z <= WALLS_FROM || pos.x.abs() > WALL_X + r {
        return;
    }
    if pos.x.abs() > WALL_X - r {
        pos.x = (WALL_X - r) * pos.x.signum();
        if vel.x * pos.x > 0.0 {
            vel.x *= -WALL_BOUNCE;
        }
    }
    if pos.z > BACK_WALL - r {
        pos.z = BACK_WALL - r;
        vel.z = vel.z.min(0.0) * -WALL_BOUNCE;
    }
}

fn wrap(a: f32) -> f32 {
    let pi = std::f32::consts::PI;
    if a > pi {
        a - 2.0 * pi
    } else if a < -pi {
        a + 2.0 * pi
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Pad as In;

    fn card(rolls: &[u8]) -> Card {
        Card { rolls: rolls.to_vec(), marks: vec![Mark::None; rolls.len()] }
    }

    #[test]
    fn ten_pin_scoring() {
        // strikes, spares, an open frame
        assert_eq!(card(&[10, 7, 3, 9, 0, 10, 0, 8, 8, 2, 0, 6, 10, 10, 10, 8, 1]).total(), 167);
        // a perfect game, all spares of 5, all nines
        assert_eq!(card(&[10; 12]).total(), 300);
        assert_eq!(card(&[5; 21]).total(), 150);
        assert_eq!(card(&[9, 0].repeat(10)).total(), 90);
        // a strike waits for its two balls
        assert_eq!(card(&[10, 3]).totals()[0], None);
        assert_eq!(card(&[10, 3, 4]).totals()[..2], [Some(17), Some(24)]);
    }

    #[test]
    fn the_tenth_frame() {
        let nine = [0u8, 0].repeat(9);
        let at = |extra: &[u8]| card(&[nine.as_slice(), extra].concat()).at();
        assert_eq!(at(&[]), Turn { frame: 9, ball: 0, fresh: true, done: false });
        // a strike: two more, each on a fresh rack after a strike
        assert_eq!(at(&[10]), Turn { frame: 9, ball: 1, fresh: true, done: false });
        assert_eq!(at(&[10, 10]), Turn { frame: 9, ball: 2, fresh: true, done: false });
        assert_eq!(at(&[10, 4]), Turn { frame: 9, ball: 2, fresh: false, done: false });
        // a spare: one more on a fresh rack; an open frame ends the game
        assert_eq!(at(&[6, 4]), Turn { frame: 9, ball: 2, fresh: true, done: false });
        assert!(at(&[6, 3]).done && at(&[10, 10, 10]).done && at(&[6, 4, 7]).done);
        assert_eq!(card(&[nine.as_slice(), &[10, 10, 10]].concat()).total(), 30);
        // the frames' balls
        let c = card(&[10, 3, 4, 10]);
        assert_eq!((c.frame_balls(0), c.frame_balls(1), c.frame_balls(2)), (0..1, 1..3, 3..4));
    }

    fn data() -> BowlData {
        let rack = vec![[0.0, 7211.9], [-227.5, 7606.0], [227.5, 7606.0], [-455.0, 8000.0], [0.0, 8000.0], [455.0, 8000.0], [-682.5, 8394.0], [-227.5, 8394.0], [227.5, 8394.0], [682.5, 8394.0]];
        let strikes = ["STRIKE!", "DOUBLE!!", "TURKEY!!!", "!!GREAT!!"].map(String::from).to_vec();
        BowlData {
            characters: vec![
                Character { id: 0, s: 360, left: false, female: false, reactions: vec![8, 39, 70, 71], voices: vec![0x2001, 0x2000, 0x4002] },
                Character { id: 8, s: 280, left: true, female: false, reactions: vec![], voices: vec![1, 2, 3] },
            ],
            pins: Rack { y: -250.66, rack },
            spectators: People { people: vec![Person { x: 4200.0, z: 6300.0 }] },
            callouts: Callouts { strikes, foul: "FOUL".into(), over_charged: "OVER CHARGED".into(), ko: "K.O.".into(), ..Default::default() },
        }
    }

    /// One ball thrown from x, at angle, the spin meter and power as given (no random error: their speeds 0).
    fn throw(character: usize, x: f32, angle: i32, spin: f32, power: f32, seed: u32) -> Bowl {
        let d = data();
        let mut b = Bowl::new(&d, vec![Bowler::new(&d, character, character)], seed);
        b.x = x;
        b.angle = angle;
        (b.spin, b.spin_speed, b.power, b.power_speed) = (spin, 0.0, power, 0.0);
        b.phase = Phase::Throw(1);
        for _ in 0..1200 {
            if !matches!(b.phase, Phase::Throw(_)) {
                break;
            }
            b.step(In::default());
        }
        b
    }

    #[test]
    fn a_full_power_throw_down_the_middle_knocks_most_pins_down() {
        let b = throw(0, 0.0, 0, 0.0, 100.0, 1);
        let knocked = b.cards[0].rolls.first().copied().unwrap_or(0);
        assert!(knocked >= 7, "{knocked} down, {:?}", b.pins.iter().map(|p| p.standing).collect::<Vec<_>>());
        assert!(matches!(b.phase, Phase::Strike(_) | Phase::Result(_)), "{:?}", b.phase);
    }

    #[test]
    fn the_hook_curves_by_the_hand() {
        // the ball's x where it reaches the head pin, a straight ball's and each hand's strongest hook
        let x_at = |character: usize, spin: f32| {
            let d = data();
            let mut b = Bowl::new(&d, vec![Bowler::new(&d, character, character)], 7);
            (b.x, b.angle, b.spin, b.spin_speed, b.power, b.power_speed) = (0.0, 0, spin, 0.0, 50.0, 0.0);
            b.phase = Phase::Throw(1);
            while b.ball.pos.z < 7000.0 && b.frame < 400 {
                b.step(In::default());
            }
            b.ball.pos.x
        };
        let straight = x_at(0, 0.0);
        // right-handed: the meter reaches -2 (towards -x); left-handed: +2
        assert!(x_at(0, -2.0) < straight - 300.0, "{} {}", x_at(0, -2.0), straight);
        assert!(x_at(8, 2.0) > straight + 300.0, "{} {}", x_at(8, 2.0), straight);
    }

    #[test]
    fn over_charged_is_a_foul() {
        let d = data();
        let mut b = Bowl::new(&d, vec![Bowler::new(&d, 0, 0)], 5);
        (b.power, b.phase) = (100.0, Phase::Power);
        // a draw on the wrap frame below S / 2000 (Paul: 0.18), CROSS on that frame
        let Some(seed) = (0..1000).find(|&s| Bowl { seed: s, ..b.clone() }.random() < 0.18) else { panic!("no seed") };
        b.seed = seed;
        b.step(In { buttons: CROSS, ..In::default() });
        assert_eq!(b.phase, Phase::OverCharged(0));
        for _ in 0..2000 {
            if matches!(b.phase, Phase::Result(_) | Phase::Strike(_)) {
                break;
            }
            b.step(In::default());
        }
        assert_eq!((b.cards[0].rolls.as_slice(), b.cards[0].marks.as_slice()), (&[0u8][..], &[Mark::Foul][..]));
        assert_eq!(b.callout.as_deref(), Some("FOUL"));
        // and the next ball meets a fresh rack
        b.step(In { start: true, ..In::default() });
        assert_eq!(b.standing(), 10);
    }

    #[test]
    fn the_same_pads_give_the_same_game() {
        let run = || {
            let d = data();
            let mut b = Bowl::new(&d, vec![Bowler::new(&d, 0, 0), Bowler::new(&d, 8, 8)], 42);
            let mut hashes = vec![];
            for f in 0..6000u32 {
                let pad = In { left: f % 97 < 20, right: f % 89 > 70, buttons: if f % 53 == 0 { CROSS } else { 0 }, ..In::default() };
                b.step(pad);
                hashes.push(b.hash());
            }
            (hashes, b.cards.clone())
        };
        let (a, cards) = run();
        assert_eq!(a, run().0);
        assert!(cards.iter().all(|c| !c.rolls.is_empty()), "{cards:?}");
    }

    #[test]
    fn a_wide_ball_knocks_a_spectator_over() {
        // from the right edge, turned all the way right: past the deck, a miss; with someone standing on its path
        // (where it was at z 6000), a K.O. and the pins as they are
        let d = data();
        let mut b = Bowl::new(&d, vec![Bowler::new(&d, 0, 0)], 3);
        (b.x, b.angle, b.spin, b.spin_speed, b.power, b.power_speed, b.people) = (1000.0, TURN_LIMIT, 1.0, 0.0, 100.0, 0.0, vec![]);
        b.phase = Phase::Throw(1);
        let mut path = None;
        while matches!(b.phase, Phase::Throw(_)) {
            b.step(In::default());
            path = path.or((b.ball.pos.z > 6000.0).then_some([b.ball.pos.x + 100.0, b.ball.pos.z]));
        }
        assert_eq!(b.cards[0].marks, [Mark::Miss]);
        let Some(person) = path else { panic!("the ball never got to z 6000") };
        let mut b = Bowl::new(&d, vec![Bowler::new(&d, 0, 0)], 3);
        (b.x, b.angle, b.spin, b.spin_speed, b.power, b.power_speed, b.people) = (1000.0, TURN_LIMIT, 1.0, 0.0, 100.0, 0.0, vec![person]);
        b.phase = Phase::Throw(1);
        while matches!(b.phase, Phase::Throw(_)) {
            b.step(In::default());
        }
        assert_eq!((b.phase, b.callout.as_deref()), (Phase::Ko(0), Some("K.O.")));
        while matches!(b.phase, Phase::Ko(_)) {
            b.step(In::default());
        }
        assert_eq!(b.cards[0].rolls, [0]);
    }
}
