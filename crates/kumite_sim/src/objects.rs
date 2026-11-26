//! The effect objects that strike (pool 0x8006F4F4): Gon's fire and gas. A move whose attack descriptor's byte 0 is
//! 0x18 or more has no strike of its own; it starts an object in its active window (0x8006F0F4), and while the owner
//! is still in a move of that byte, past its active window's start, each striking object's travel this frame is a
//! strike of the owner's (0x8006FB6C, read by 0x80047BBC through 0x8006E51C), tested against the opponent's hurt
//! spheres like a move's.
//! - byte 0x1A, Gon's fire (0x8006EB28 -> object 8, 0x80071E5C): from the head (bone slot 2) at (300, 0, 50) in its
//!   axes, heading along its x axis at 128 units a frame, down to 216 units over the floor and on along it; 8 frames
//!   growing, 5-8 looped, 9 ending (it strikes for 3 of them). Gone when its strike lands on the opponent.
//! - byte 0x1B, Gon's gas (0x8006EDFC -> object 0x13, 0x800747EC): on the active window's first frame, a source at
//!   the pelvis (slot 11) at (0, 200, 0), which lets out [`GAS`]'s puffs (object 0x14, 0x80074A30): each at its
//!   height, put its distance along the source's heading on its first frame, then rising; 54 frames, striking for 46.
//! - byte 0x18, Ogre's and True Ogre's breath (0x8006E99C -> object 6, 0x800711AC): Gon's fire from (600, 0, 50) in
//!   the head's axes (the other side's z: -50), its heading turned by rand() (up to 63 / 4096 of a turn in pitch,
//!   31 in yaw) at 160 units a frame, 256 once grown; 7 frames growing, 5-8 looped, 8 ending (it strikes for 4).
//! - Tekken Tag's True Ogre (body 20) on a move striking to slot 0x18 ([`TAG_FIRE`]; 0x3A6390 -> 0x3A4EE8 each
//!   active frame): a flame (task 0x3A44F8) from 200 along the z of the drawn node 13 on the head (player +0x1770)
//!   towards 400, its pitch and yaw turned by up to pi / 32 and pi / 64 (rand(), 0x3A94B0), 160 units a frame; 8
//!   frames growing, then 256 a frame, held 5-8, 7 fading; on along the floor 216 over it. Each striking flame's
//!   travel is a strike of its owner's (0x346230's segments, read by 0x3770E0); none strike once the opponent burns
//!   (+0xC04, 0x346378).
//!
//! - Tekken Tag's moves striking to slot 0x19 ([`TAG_MIST`]; 0x3A6390 -> 0x3A5810 each active frame: Yoshimitsu's
//!   2365, Nina's 2034 and the reactions 5428 / 5429 they give): a puff of mist (task 0x3A55E0) from 100 along the
//!   node's z towards 400, its pitch and yaw each turned by up to pi / 32, 48 units a frame for 21 frames; it
//!   strikes its first 15 (table 0x5198C8) while its owner is still in the move.
//!
//! - bytes 0x19 and 0x1C, Doctor B.'s mist (0x8006ECA4 -> object 10, 0x800729FC): from the head at (100, 0, 70) in
//!   its axes towards (400, 0, 70), its pitch and yaw turned by rand() (up to 127 / 4096 of a turn) at 48 units a
//!   frame, moving from its first frame; 21 frames, striking for its first 16 (0x80025688) while its owner is in a
//!   move of its byte.
//!
//! - Tekken 3's drawn-only objects, run for the rand() draws they make (one stream with the fighters' blinks, sounds and
//!   CPUs, 0x800A39B8) and the pool's count their odds go by: the sparks (type 7, 0x80071A24) the breath and the fire
//!   let out, the flames on the floor under the breath (type 9, 0x800725EC), the fires on a fighter the breath or the
//!   fire struck (type 11, 0x80072E74), each object's slot held two passes after it is over (type 0, 0x8006FD64).
//!
//! - Tekken 3's heavy hits' particles (types 1, 2 and the ki-charged 12, 13, 0x80076D40) and the limb glows (type 3,
//!   the pool's first eight) with their trails (types 4, 5), run as these: their draws, their slots, where they are.

use crate::detmath::DetMath;
use glam::{Mat4, Vec3};

/// Gon's fire's and gas's descriptor bytes.
pub const FIRE: u32 = 0x1A;
pub const GAS: u32 = 0x1B;
pub const BREATH: u32 = 0x18;
pub const MIST: [u32; 2] = [0x19, 0x1C];
/// Tekken Tag's True Ogre's fire: its moves' strike to slot 0x18 (no descriptor byte; this stands for it).
pub const TAG_FIRE: u32 = 0x118;
/// Tekken Tag's mist: its moves' strike to slot 0x19 (as [`TAG_FIRE`]).
pub const TAG_MIST: u32 = 0x119;
/// The mist's start and heading's second point (0x437C30, as `TAG_FIRE_AT`), speed (0x3A3518's 48), frames (+0x14)
/// and striking frames (0x5198C8: 16 ones, its counter past its first move).
const TAG_MIST_AT: [Vec3; 2] = [Vec3::new(0.0, 0.0, -100.0), Vec3::new(0.0, 0.0, -400.0)];
const TAG_MIST_SPEED: f32 = 48.0;
pub const TAG_MIST_LIFE: i32 = 21;
const TAG_MIST_STRIKES: i32 = 16;
/// The flame's start and its heading's second point (0x437BF0: along the z of the drawn node 13, player +0x1770; -z
/// in the joints' turned axes).
const TAG_FIRE_AT: [Vec3; 2] = [Vec3::new(0.0, 0.0, -200.0), Vec3::new(0.0, 0.0, -400.0)];
/// The flame's speeds (0x3A3518's 160, then 256) and its height over the floor.
const TAG_FIRE_SPEED: [f32; 2] = [160.0, 256.0];
const TAG_FIRE_FLOOR: f32 = 216.0;
/// The fire's start and its heading's second point in the head's axes (0x80098820), the gas's in the pelvis's
/// (0x80098840): the bones' axes (x along the facing, y down), turned onto the drawn joints' (x, −y, −z).
const FIRE_AT: [Vec3; 2] = [Vec3::new(300.0, 0.0, -50.0), Vec3::new(800.0, 0.0, -50.0)];
/// The breath's start and second point in the head's axes by its owner's side (0x80098800), as `FIRE_AT`.
const BREATH_AT: [[Vec3; 2]; 2] = [[Vec3::new(600.0, 0.0, -50.0), Vec3::new(800.0, 0.0, -50.0)], [Vec3::new(600.0, 0.0, 50.0), Vec3::new(800.0, 0.0, 50.0)]];
/// The breath's speeds (16ths of a unit: 0xA00, then 0x1000 at 0x800717A0).
const BREATH_SPEED: [i32; 2] = [0xA00, 0x1000];
/// The mist's start and second point in the head's axes (0x80098830, as `FIRE_AT`), speed (0x300), frames and striking
/// frames.
const MIST_AT: [Vec3; 2] = [Vec3::new(100.0, 0.0, -70.0), Vec3::new(400.0, 0.0, -70.0)];
const MIST_SPEED: f32 = 48.0;
const MIST_LIFE: i32 = 21;
const MIST_STRIKES: i32 = 16;
const GAS_AT: [Vec3; 2] = [Vec3::new(0.0, -200.0, 0.0), Vec3::new(0.0, -600.0, 0.0)];
/// The fire's speed (0x800 at 0x8006E120: 128 units a frame) and its lowest height (y −0xD8).
const FIRE_SPEED: f32 = 128.0;
const FIRE_FLOOR: f32 = 216.0;
/// The gas source's puffs (0x80025738): the frame of the source each comes on, its distance along the heading and
/// its height.
pub const GAS_PUFFS: [[f32; 3]; 8] =
    [[4.0, 0.0, 600.0], [6.0, 200.0, 500.0], [8.0, 300.0, 700.0], [10.0, 350.0, 350.0], [10.0, 500.0, 800.0], [10.0, 500.0, 400.0], [10.0, 650.0, 750.0], [10.0, 600.0, 600.0]];
/// A puff's frames (0x36), the frames it strikes for (0x800257E8: 23 cells of 2), its rise's growth a frame (0x3999
/// in 16.16).
pub const PUFF_LIFE: i32 = 54;
const PUFF_STRIKES: i32 = 46;
const PUFF_RISE: f32 = 0x3999 as f32 / 65536.0;

/// Tekken 3's pool (0x8006F4F4): its 80 objects, the eight limb glows (type 3) the round's first pass puts in
/// (0x8006F678) among them.
const POOL: usize = 80;
pub const GLOWS: i32 = 8;
/// The joint slots each fighter's glows sit on (0x8006F678's +0x40).
pub const GLOW_SLOTS: [usize; 4] = [6, 10, 14, 17];
/// Where each character's (+0x18) hands glow (slots 6 and 10): these points on the hand's matrix (0x80025334, 0x800707F4).
pub const HAND_GLOWS: [[[i32; 3]; 2]; 21] = [
    [[80, 0, 0], [80, 0, 0]],
    [[100, 0, 0], [100, 0, 0]],
    [[60, 0, 0], [60, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[280, 0, 0], [280, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[70, 0, 0], [70, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[58, 0, 0], [58, 0, 0]],
    [[90, 0, 0], [90, 0, 0]],
    [[80, 0, 0], [80, 0, 0]],
    [[300, 0, -300], [180, 0, 300]],
];
/// The joint slot a descriptor's limb byte glows on (0x800251C4; 0 none).
pub const LIMB_GLOWS: [u32; 26] = [0, 0, 0, 0, 6, 6, 6, 0, 10, 10, 10, 0, 14, 14, 14, 17, 17, 17, 10, 10, 0, 0, 0, 0, 0, 0];
/// The bone slots a struck fighter burns on (0x80025180), one for Gon (character 17, +0x18 0x11).
pub const BURN_SLOTS: [usize; 10] = [1, 2, 4, 8, 5, 9, 12, 13, 15, 16];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Fire,
    Breath,
    TagFire,
    TagMist,
    /// Doctor B.'s mist and the descriptor byte it strikes for (+0x4C).
    Mist(u32),
    /// The gas's source: its heading (sine, cosine of the yaw) and the next puff.
    GasSource {
        heading: [f32; 2],
        next: usize,
    },
    Gas,
    /// A spark (type 7).
    Spark,
    /// A flame on the floor (type 9).
    FloorFlame,
    /// A fire on fighter `victim`'s bone slot `slot` (type 11).
    Burn {
        victim: usize,
        slot: usize,
    },
    /// A hit's particle thrown off where it struck (type 2, 0x800701F0; type 13, 0x800738AC, for a ki-charged
    /// attacker).
    Particle {
        ki: bool,
    },
    /// A hit's particle easing along the strike (type 1, 0x8006FE04; 12, 0x800734B4): its speed from the first to the
    /// second (16ths of a unit a frame).
    Streak {
        ki: bool,
        speed: [i32; 2],
    },
    /// A limb glow's trail (type 4, 0x80070B74) and its spark (type 5, 0x80070EA0), rising off.
    Trail,
    TrailSpark,
    /// An object over (type 0): its slot held for `count` more passes.
    Done,
}

/// One object: its owner (fighter 0 / 1; a brawl's actor), where it is and was a frame before (game units, world), its travel a frame,
/// its state (+0xE: the fire's 1 growing, 2 looped, 4 ending), frame counter (+0x48) and frames (+0x40, +0x44).
#[derive(Clone, Debug)]
pub struct Object {
    pub kind: Kind,
    pub owner: usize,
    pub pos: Vec3,
    pub prev: Vec3,
    pub vel: Vec3,
    pub strikes: bool,
    pub state: u8,
    pub count: i32,
    pub frames: i32,
    pub left: i32,
    /// Its picture turned over (its corners' other order, rand bit 0 at its start).
    pub mirror: bool,
    /// Tekken 3's breath and fire: their pitch and yaw as the PS1 keeps them (+0x10, +0x12, 4096 a turn), which their
    /// sparks start from.
    pub angles: [i32; 2],
    /// Tekken 3's breath, sparks, floor flames and burns: where it is and its travel a frame (16.16; the flames' and
    /// burns' rise in y) in the PS1's integers and axes (y down, z negated, +0x28, +0x1C).
    pub at: [i32; 3],
    pub travel: [i32; 3],
}

/// The objects playing and the PsyQ rand state they draw from (0x8007A318).
#[derive(Clone, Debug, Default)]
pub struct Objects {
    pub list: Vec<Object>,
    /// rand()'s state (0x800A39B8). Tekken 3's one stream: the blinks, the sounds and the CPUs draw from it too, in the
    /// game's order (Tekken Tag's flames keep their own).
    pub seed: u32,
    /// Tekken 3: the objects the last pass ran (0x800A9194), the limb glows with them: the sparks' odds.
    pub count: i32,
    /// Tekken 3: the display's field this frame (0x800ADEFC, 0x80080D98), every other frame: the breath strikes for
    /// P1 on the even ones, for P2 on the odd (0x80071364), from where it was the frame before (0x80071244).
    pub field: bool,
    /// Tekken 3: each fighter's 18 joint slots where the PS1 keeps them this frame (+0x908; y down, z negated), its
    /// burns' places (filled while any burns).
    pub joints: Vec<Vec<[i32; 3]>>,
    /// Each fighter's move and frame an object last started on (a hit-stop holds the frame: one start a frame).
    pub started: Vec<Option<(usize, u32)>>,
    /// Tekken Tag: each fighter's opponent burns from its fire (+0xC04): its new flames do not strike.
    pub burned: Vec<bool>,
    /// Tekken 3: each fighter's limb glows this frame (0x8006F2F0: bit k for joint slot k, bit 31 an attack's, whose
    /// glows leave trails), set before the pass.
    pub glow_mask: Vec<u32>,
    /// Tekken 3: the eight limb glows (type 3, 0x800705C8; P1's on [`GLOW_SLOTS`], then P2's), the pool's first
    /// objects: where each is (its joint slot this frame, [`Objects::joints`]), whether it shows and its frames shown
    /// (+0x44, its palette step).
    pub glows: [([i32; 3], bool, i32); 8],
}

impl Objects {
    /// PsyQ rand() (0x8007A318): 0..0x7FFF, the state's bits 16-30.
    pub fn rand(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
        ((self.seed >> 16) & 0x7FFF) as i32
    }

    /// A draw's odds of a spark or a flame, out of 0x8000: fewer as the pool fills (0x800713BC, `k` 15, 9 or 10).
    fn odds(&self, k: i32) -> i32 {
        (0x50 - self.count) * k * 0x4000 / 80_000 * 4
    }

    /// Lets out `kind` from `owner`'s object at `angles` (0x8006F4F4: none once the pool is full).
    fn let_out(&mut self, owner: usize, kind: Kind, at: [i32; 3], angles: [i32; 2]) {
        if self.list.len() + (GLOWS as usize) < POOL {
            self.list.push(Object {
                kind,
                owner,
                pos: Vec3::ZERO,
                prev: Vec3::ZERO,
                vel: Vec3::ZERO,
                strikes: false,
                state: 0,
                count: -1,
                frames: 0,
                left: 0,
                mirror: false,
                angles,
                at,
                travel: [0; 3],
            });
        }
    }

    /// Tekken 3's objects of a frame's clean hits (0x80076C28): the breath's or the fire's sets the struck one burning
    /// (0x80076D24; `gon[k]` whether fighter k is Gon), one of 21 or more but the objects' bytes' throws its particles
    /// off ([`Objects::heavy`]).
    pub fn on_hits(&mut self, hits: &[crate::HitEvent], gon: &[bool]) {
        let ps1 = |v: Vec3| [v.x, -v.y, -v.z].map(|c| c.round() as i32);
        for h in hits.iter().filter(|h| !h.blocked && h.base_damage != 0 && !h.throw) {
            if matches!(h.limb, 0x18 | 0x1A) {
                self.burn(h.victim, gon.get(h.victim).copied().unwrap_or(false));
            } else if h.damage.max(h.base_damage) >= 21 && !(0x18..=0x1C).contains(&h.limb) {
                self.heavy(h.attacker, ps1(h.point), ps1(h.strike), h.ki);
            }
        }
    }

    /// Tekken 3: fighter `owner`'s hit of 21 or more (0x80076D40) at `at` along `strike` (the PS1's integers and axes)
    /// throws off 20 particles (type 2; 13 when it is ki-charged), then 6 easing along the strike (type 1; 12).
    pub fn heavy(&mut self, owner: usize, at: [i32; 3], strike: [i32; 3], ki: bool) {
        let angles = heading_angles(strike);
        for _ in 0..20 {
            self.let_out(owner, Kind::Particle { ki }, at, angles);
        }
        for _ in 0..6 {
            self.let_out(owner, Kind::Streak { ki, speed: [0; 2] }, at, angles);
        }
    }

    /// Tekken 3: fighter `victim`, struck by the breath or the fire, burns (0x8006EF60): on ten bone slots, Gon on one.
    pub fn burn(&mut self, victim: usize, gon: bool) {
        for &slot in &BURN_SLOTS[..if gon { 1 } else { 10 }] {
            self.let_out(victim, Kind::Burn { victim, slot }, [0; 3], [0; 2]);
        }
    }

    /// Fighter `owner`'s move of descriptor byte `byte` on a frame of its active window (`first`: the window's first
    /// frame) starts its object at bone slots `slots` (head, pelvis: world, game units); Tekken 3's breath on `head`,
    /// the head's matrix in the PS1's integers ([`crate::BodyVolumes::head`]).
    pub fn start(&mut self, owner: usize, byte: u32, first: bool, slots: &[Mat4; 2], head: Option<([[i32; 3]; 3], [i32; 3])>) {
        let (m, at, kind) = match byte {
            FIRE => (slots[0], FIRE_AT, Kind::Fire),
            // (a brawl's actors by their index's parity)
            BREATH => (slots[0], BREATH_AT[owner % 2], Kind::Breath),
            TAG_FIRE => (slots[0], TAG_FIRE_AT, Kind::TagFire),
            TAG_MIST => (slots[0], TAG_MIST_AT, Kind::TagMist),
            b if MIST.contains(&b) => (slots[0], MIST_AT, Kind::Mist(b)),
            GAS if first => (slots[1], GAS_AT, Kind::GasSource { heading: [0.0, 1.0], next: 0 }),
            _ => return,
        };
        let (mut p, q) = (m.transform_point3(at[0]), m.transform_point3(at[1]));
        let d = q - p;
        let ps1 = |v: Vec3| [v.x, -v.y, -v.z].map(|c| c.round() as i32);
        let head = head.filter(|_| kind == Kind::Breath);
        let (at, b) = match head {
            // (0x8006E99C: ApplyRotMatrix of 0x80098800's points on the head's matrix, plus its place)
            Some((r, t)) => {
                let on = |v: Vec3| -> [i32; 3] {
                    let v = ps1(v);
                    std::array::from_fn(|k| ((0..3).map(|c| i64::from(r[k][c]) * i64::from(v[c])).sum::<i64>() >> 12) as i32 + t[k])
                };
                (on(at[0]), on(at[1]))
            }
            None => (ps1(p), ps1(q)),
        };
        // (0x8006DF34: the yaw and the pitch of the way it heads, a quarter turn less, in the PS1's integers and axes)
        let e = [b[0] - at[0], b[1] - at[1], b[2] - at[2]];
        let angles = if head.is_some() {
            p = Vec3::new(at[0] as f32, -at[1] as f32, -at[2] as f32);
            heading_angles(e)
        } else {
            let e = e.map(|c| c as f32);
            let a12 = |r: f32| ((r * 4096.0 / std::f32::consts::TAU).round() as i32 - 0x400) & 0xFFF;
            [a12(glam::Vec2::new(e[0], e[2]).length().datan2(e[1])), a12(e[2].datan2(e[0]))]
        };
        let kind = match kind {
            Kind::GasSource { next, .. } => Kind::GasSource { heading: glam::Vec2::new(d.x, d.z).normalize_or(glam::Vec2::Y).to_array(), next },
            k => k,
        };
        let vel = match kind {
            Kind::Fire => d.normalize_or_zero() * FIRE_SPEED,
            // (its heading: turned by rand() on its first frame)
            Kind::Breath | Kind::TagFire | Kind::TagMist | Kind::Mist(_) => d.normalize_or_zero(),
            _ => Vec3::ZERO,
        };
        self.list.push(Object { kind, owner, pos: p, prev: p, vel, strikes: false, state: 0, count: -1, frames: 0, left: 0, mirror: false, angles, at, travel: [0; 3] });
    }

    /// One frame (0x8006F5C0): `striking[k]` whether fighter k is still in its object's move past the active window's
    /// start (its descriptor byte), `struck[k]` whether fighter k's opponent was struck last frame; `t3` Tekken 3's
    /// objects (their draws, sparks and slots).
    pub fn step(&mut self, striking: &[Option<u32>], struck: &[bool], t3: bool) {
        // (Tekken Tag: a fire's strike landed, the opponent burns; over with the move)
        self.burned.resize(striking.len(), false);
        for (k, b) in self.burned.iter_mut().enumerate() {
            *b = striking[k] == Some(TAG_FIRE) && (*b || struck.get(k).copied().unwrap_or(false));
        }
        // (the glows first: the last one's offspring are not run when it ran as the newest)
        let newest_glow = t3 && self.glow();
        let (mut i, mut ran) = (0, 0);
        // (puffs let out this frame move this frame too: the pool's list runs on to its end; Tekken 3's stops at the
        // object newest as it ran, 0x8006F95C)
        while i < self.list.len() && !newest_glow {
            let newest = t3 && i + 1 == self.list.len();
            let mut o = self.list[i].clone();
            let own = striking.get(o.owner).copied().flatten();
            let on = |byte: u32| own == Some(byte);
            ran += 1;
            let keep = match o.kind {
                Kind::Fire => {
                    let hit = struck.get(o.owner).copied().unwrap_or(false);
                    self.fire(&mut o, on(FIRE), hit, t3)
                }
                Kind::Breath => {
                    let hit = struck.get(o.owner).copied().unwrap_or(false);
                    self.fire(&mut o, on(BREATH), hit, t3)
                }
                Kind::Spark => self.spark(&mut o),
                Kind::Particle { .. } => self.particle(&mut o),
                Kind::Streak { .. } => self.streak(&mut o),
                Kind::Trail | Kind::TrailSpark => self.trail(&mut o),
                Kind::FloorFlame => self.floor_flame(&mut o),
                Kind::Burn { .. } => self.burning(&mut o),
                Kind::Done => {
                    o.count -= 1;
                    o.count > 0
                }
                Kind::TagFire => self.tag_fire(&mut o, on(TAG_FIRE)),
                Kind::TagMist => self.tag_mist(&mut o, on(TAG_MIST)),
                Kind::Mist(byte) => self.mist(&mut o, on(byte)),
                Kind::GasSource { heading, next } => {
                    o.count += 1;
                    if let Some(&[_, far, high]) = GAS_PUFFS.get(next).filter(|p| on(GAS) && o.count as f32 >= p[0]) {
                        let at = Vec3::new(o.pos.x, high, o.pos.z);
                        let vel = Vec3::new(heading[0] * far, 0.0, heading[1] * far);
                        self.list.push(Object {
                            kind: Kind::Gas,
                            owner: o.owner,
                            pos: at,
                            prev: at,
                            vel,
                            strikes: false,
                            state: 0,
                            count: -1,
                            frames: 0,
                            left: 0,
                            mirror: false,
                            angles: [0; 2],
                            at: [0; 3],
                            travel: [0; 3],
                        });
                        o.kind = Kind::GasSource { heading, next: next + 1 };
                    }
                    on(GAS)
                }
                Kind::Gas => {
                    o.strikes &= on(GAS);
                    o.prev = o.pos;
                    if o.state == 0 {
                        // its first frame (0x80074B24): put its distance along the heading
                        (o.state, o.strikes, o.mirror) = (1, on(GAS), self.rand() & 1 != 0);
                        o.pos += o.vel;
                        o.vel = Vec3::ZERO;
                    }
                    o.count += 1;
                    o.vel.y += PUFF_RISE;
                    o.pos.y += o.vel.y.ceil();
                    o.strikes &= o.count < PUFF_STRIKES;
                    o.count < PUFF_LIFE
                }
            };
            if keep {
                self.list[i] = o;
                i += 1;
            } else if t3 && o.kind != Kind::Done {
                // (0x8006F5AC: its slot held two more passes)
                self.list[i] = Object { kind: Kind::Done, strikes: false, count: 2, ..o };
                i += 1;
            } else {
                self.list.remove(i);
            }
            if newest {
                break;
            }
        }
        self.count = ran + GLOWS;
    }

    /// The fire (0x80071E5C) and the breath (0x800711AC): false when it is gone.
    fn fire(&mut self, o: &mut Object, on: bool, struck: bool, t3: bool) -> bool {
        o.strikes &= on;
        let breath = o.kind == Kind::Breath;
        // the fire's strike landed: gone (a guarded one ends, 0x800724A4: Gon's fire cannot be guarded)
        if !breath && o.strikes && struck && (1..=2).contains(&o.state) {
            return false;
        }
        if o.state == 0 {
            // 0x80072194: strikes, 5-8 looped frames
            o.state = 1;
            let r = self.rand();
            (o.strikes, o.mirror, o.left) = (on, r & 1 != 0, ((r >> 2) & 3) + 5);
            if breath {
                // 0x80071540: the heading's pitch and yaw turned, at 0xA00
                o.angles = [o.angles[0] + ((r >> 4) & 0x7F) - 0x3F, o.angles[1] + ((r >> 6) & 0x3F) - 0x1F];
                o.travel = heading(o.angles, BREATH_SPEED[0]);
            }
        }
        // (its end's last frame still moves and draws, 0x8006F5AC)
        let mut over = false;
        match o.state {
            1 => {
                o.count += 1;
                if o.count >= 7 {
                    (o.state, o.count) = (2, self.rand());
                    if breath {
                        o.travel = heading(o.angles, BREATH_SPEED[1]);
                    }
                }
            }
            2 => {
                o.count += 1;
                o.frames += 1;
                if o.frames >= o.left {
                    (o.state, o.count) = (4, -1);
                    // (the breath's end picks its corners again, 0x800718A4)
                    if breath {
                        o.mirror = self.rand() & 1 != 0;
                    }
                }
            }
            _ => {
                o.count += 1;
                over = o.count >= if breath { 8 } else { 9 };
                // (0x800255E0: it strikes for the end's first 3 frames; the breath's 4, 0x80025534)
                o.strikes &= o.count < if breath { 4 } else { 3 } && !over;
            }
        }
        if !breath || o.strikes && self.field == o.owner.is_multiple_of(2) {
            o.prev = o.pos;
        }
        if breath {
            // (the travel's whole units, 0x8007130C; low enough: on along the floor, level, 0x8006E0D0)
            for k in 0..3 {
                o.at[k] += o.travel[k] >> 16;
            }
            if o.at[1] > -(FIRE_FLOOR as i32) {
                o.at[1] = -(FIRE_FLOOR as i32);
                o.angles[0] = 0;
                o.travel = heading(o.angles, BREATH_SPEED[usize::from(o.state != 1)]);
            }
            o.pos = Vec3::new(o.at[0] as f32, -o.at[1] as f32, -o.at[2] as f32);
        } else {
            o.pos += o.vel;
            // low enough: on along the floor, level (0x8006E0D0)
            if o.pos.y < FIRE_FLOOR {
                o.pos.y = FIRE_FLOOR;
                o.angles[0] = 0;
                o.vel = Vec3::new(o.vel.x, 0.0, o.vel.z).normalize_or_zero() * FIRE_SPEED;
            }
        }
        if t3 {
            // a spark one draw in the odds, else the breath along the floor a flame on it (0x800713B0, 0x800720EC)
            let at = [o.pos.x, -o.pos.y, -o.pos.z].map(|v| v.round() as i32);
            if self.rand() < self.odds(15) {
                self.let_out(o.owner, Kind::Spark, at, o.angles);
            } else if breath && o.pos.y == FIRE_FLOOR && o.strikes && self.rand() < self.odds(9) {
                self.let_out(o.owner, Kind::FloorFlame, at, [0; 2]);
            }
        }
        !over
    }

    /// A spark (0x80071A24): spread off where it came out, 0-31 frames, then 12 ending; rising 3 a frame faster, gone
    /// at the floor.
    fn spark(&mut self, o: &mut Object) -> bool {
        let (mut at, mut vel) = (o.at, o.travel);
        let mut over = false;
        if o.state == 0 {
            // 0x80071AFC: two draws, the second over the first's 16 bits
            let r = self.rand();
            let r = r | self.rand() << 16;
            for (k, a) in at.iter_mut().enumerate() {
                *a += ((r >> (2 * k)) & 0xFF) - 0x7F;
            }
            o.angles = [o.angles[0] + ((r >> 6) & 0x7F) - 0x3F, o.angles[1] + ((r >> 8) & 0x7F) - 0x3F];
            (o.left, o.frames, o.state) = ((r >> 10) & 0x1F, 0, 1);
            vel = heading(o.angles, ((r >> 12) & 0x3FF) * 5 / 4 + 0x200);
        }
        o.frames += 1;
        if o.state == 1 && o.frames >= o.left {
            (o.frames, o.state) = (-1, 2);
        } else if o.state == 2 {
            over = o.frames >= 12;
        }
        vel[1] -= 3 << 16;
        for k in 0..3 {
            at[k] += vel[k] >> 16;
        }
        (o.at, o.travel) = (at, vel);
        !over && at[1] < 0
    }

    /// The limb glows (0x800707F4): one showing on its fighter's mask is at its joint, lets out a trail in an attack
    /// and a spark one draw in the odds (0x80070AA8). True when the last glow ran as the pool's newest object.
    fn glow(&mut self) -> bool {
        let mut newest = false;
        for g in 0..8 {
            newest = self.list.is_empty();
            let (k, slot) = (g / 4, GLOW_SLOTS[g % 4]);
            let mask = self.glow_mask.get(k).copied().unwrap_or(0);
            self.glows[g].1 = mask >> slot & 1 != 0;
            if !self.glows[g].1 {
                continue;
            }
            if let Some(&p) = self.joints.get(k).and_then(|j| j.get(slot)) {
                self.glows[g].0 = p;
            }
            self.glows[g].2 += 1;
            let at = self.glows[g].0;
            if mask & 0x8000_0000 != 0 {
                self.let_out(k, Kind::Trail, at, [0; 2]);
            }
            if self.rand() < self.odds(50) {
                self.let_out(k, Kind::TrailSpark, at, [0; 2]);
            }
        }
        newest
    }

    /// A hit's thrown particle (0x80070244): two draws spread it off where it struck, turn its heading and set its
    /// speed and its 20-51 frames; it jumps 8 frames of its travel at once, then moves on until the floor.
    fn particle(&mut self, o: &mut Object) -> bool {
        if o.state == 0 {
            let r = self.rand();
            let r = r | self.rand() << 16;
            (o.left, o.frames, o.state) = (((r >> 10) & 0x1F) + 0x14, 0, 1);
            for (k, a) in o.at.iter_mut().enumerate() {
                *a += ((r >> (2 * k)) & 0xFF) - 0x7F;
            }
            o.angles = [o.angles[0] + ((r >> 6) & 0x7F) - 0x3F, o.angles[1] + ((r >> 8) & 0x7F) - 0x3F];
            o.travel = heading(o.angles, ((r >> 12) & 0x7FF) * 5 / 4);
            for k in 0..3 {
                o.at[k] += o.travel[k] >> 13;
            }
        }
        // (0x80070508)
        o.frames += 1;
        if o.frames >= o.left {
            return false;
        }
        for k in 0..3 {
            o.at[k] += o.travel[k] >> 16;
        }
        o.at[1] < 0
    }

    /// A hit's easing particle (0x8006FE58): two draws turn its heading by up to a sixth of a turn and set its speeds
    /// and its 10-41 frames; its speed eases on a quarter cosine of its life (0x800700BC) until the floor.
    fn streak(&mut self, o: &mut Object) -> bool {
        let Kind::Streak { ki, mut speed } = o.kind else { return false };
        if o.state == 0 {
            let r = self.rand();
            let r = r | self.rand() << 16;
            let turn = |v: i32| (((v & 0x7FF) - 0x3FF) * 2) / 3;
            o.angles = [o.angles[0] + turn(r), o.angles[1] + turn(r >> 2)];
            (o.left, o.frames, o.state) = (((r >> 4) & 0x1F) + 10, 0, 1);
            speed = [(r >> 6) & 0x1FF, ((r >> 8) & 0x1FF) + 0x680];
            o.kind = Kind::Streak { ki, speed };
        }
        o.frames += 1;
        if o.frames >= o.left {
            return false;
        }
        let c = crate::ball::cos12((o.frames * 0x3FF / o.left) & 0x3FF);
        o.travel = heading(o.angles, (speed[0] + (((speed[1] - speed[0]) * (c * 4 + 0x2000)) >> 15)) as i16 as i32);
        for k in 0..3 {
            o.at[k] += o.travel[k] >> 16;
        }
        o.at[1] < 0
    }

    /// A glow's trail (0x80070BC8) or its spark (0x80070EF4): two draws spread it about where it came out (the
    /// spark not in height), then it rises faster each frame, 16 frames (the spark 32).
    fn trail(&mut self, o: &mut Object) -> bool {
        let spark = o.kind == Kind::TrailSpark;
        if o.state == 0 {
            let r = self.rand();
            let r = r | self.rand() << 16;
            if spark {
                (o.at[0], o.at[2]) = (o.at[0] + (r & 0xFF) - 0x7F, o.at[2] + ((r >> 4) & 0xFF) - 0x7F);
            } else {
                for (k, a) in o.at.iter_mut().enumerate() {
                    *a += ((r >> (2 * k)) & 0x7F) - 0x3F;
                }
            }
            (o.frames, o.travel[1], o.state) = (-1, 0, 1);
        }
        o.frames += 1;
        if o.frames >= if spark { 0x20 } else { 0x10 } {
            return false;
        }
        o.travel[1] += 0x12000;
        o.at[1] -= o.travel[1] >> 16;
        true
    }

    /// A flame on the floor (0x800725EC): 60-91 frames, then 16 rising off; a spark one draw in the odds each frame.
    fn floor_flame(&mut self, o: &mut Object) -> bool {
        let (mut at, mut rise) = (o.at, o.travel[1]);
        let mut over = false;
        if o.state == 0 {
            // 0x8007270C: two draws as the spark's
            let r = self.rand();
            let r = r | self.rand() << 16;
            (at[1], rise, o.frames, o.left, o.state) = (-0x100, 0, r >> 2, (r & 0x1F) + 0x3C, 1);
        }
        if o.state == 1 {
            o.frames += 1;
            o.left -= 1;
            if o.left < 0 {
                (o.frames, o.state) = (-1, 2);
            }
        } else {
            o.frames += 1;
            over = o.frames >= 0x10;
            if !over {
                rise -= 0x24000;
                at[1] += rise >> 16;
            }
        }
        if self.rand() < self.odds(10) {
            self.let_out(o.owner, Kind::Spark, at, [0x400, 0]);
        }
        (o.at, o.travel[1]) = (at, rise);
        !over
    }

    /// A fire on a struck fighter (0x80072E74): on its bone slot 60-75 frames, a spark one draw in the odds each
    /// frame, then rising off it 7 frames and 5 ending.
    fn burning(&mut self, o: &mut Object) -> bool {
        let Kind::Burn { victim, slot } = o.kind else { return false };
        let (mut at, mut rise) = (o.at, o.travel[1]);
        let mut over = false;
        if o.state == 0 {
            // 0x80072F38
            let r = self.rand();
            (o.frames, o.left, o.state, o.mirror) = ((r & 0xF) + 0x3C, r >> 4, 1, r & 1 != 0);
        }
        match o.state {
            1 => {
                o.left += 1;
                if let Some(&p) = self.joints.get(victim).and_then(|j| j.get(slot)) {
                    at = p;
                }
                if self.rand() < self.odds(15) {
                    self.let_out(o.owner, Kind::Spark, at, [0x400, 0]);
                }
                o.frames -= 1;
                if o.frames < 0 {
                    (o.frames, rise, o.state) = (0, 0, 2);
                }
            }
            2 => {
                rise -= 0x18000;
                at[1] += rise >> 16;
                o.frames += 1;
                if o.frames >= 7 {
                    (o.frames, o.state) = (-1, 3);
                }
            }
            _ => {
                o.frames += 1;
                over = o.frames >= 5;
                if !over {
                    rise -= 0x18000;
                    at[1] += rise >> 16;
                }
            }
        }
        (o.at, o.travel[1]) = (at, rise);
        !over
    }

    /// Tekken Tag's flame (0x3A44F8): false when it is gone. `count` its frames in the stage (+0x16), `frames` its
    /// picture of the 30 (+0x1E), `left` its frames held.
    fn tag_fire(&mut self, o: &mut Object, on: bool) -> bool {
        let burned = self.burned.get(o.owner).copied().unwrap_or(false);
        // (libc rand, 0x40FBD0, and 0x3A94B0's 0..1 of its low 24 bits)
        let mut rand = || {
            self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            (self.seed & 0x7FFF_FFFF) as i32
        };
        let along = |pitch: f32, yaw: f32, speed: f32| Vec3::new(yaw.dsin() * pitch.dcos(), pitch.dsin(), yaw.dcos() * pitch.dcos()) * speed;
        let angles = |v: Vec3| (v.y.datan2(glam::Vec2::new(v.x, v.z).length()), v.x.datan2(v.z));
        o.strikes &= on && !burned;
        match o.state {
            0 => {
                let mut unit = || (rand() & 0xFF_FFFF) as f32 / 16_777_216.0 * 2.0 - 1.0;
                let (pitch, yaw) = angles(o.vel);
                let (pitch, yaw) = (pitch + unit() * std::f32::consts::PI / 32.0, yaw + unit() * std::f32::consts::PI / 64.0);
                o.vel = along(pitch, yaw, TAG_FIRE_SPEED[0]);
                (o.state, o.count, o.frames, o.strikes) = (1, 0, rand() % 30, on && !burned);
                return true;
            }
            1 if o.count >= 8 => {
                (o.state, o.count, o.left) = (2, 0, ((rand() >> 2) & 3) + 5);
                let (pitch, yaw) = angles(o.vel);
                o.vel = along(pitch, yaw, TAG_FIRE_SPEED[1]);
            }
            2 if o.count >= o.left => (o.state, o.count) = (4, 0),
            4 if o.count >= 7 => return false,
            _ => {}
        }
        o.frames = (o.frames + 1) % 30;
        o.prev = o.pos;
        o.pos += o.vel;
        o.count += 1;
        if o.pos.y < TAG_FIRE_FLOOR {
            o.pos.y = TAG_FIRE_FLOOR;
            o.vel = Vec3::new(o.vel.x, 0.0, o.vel.z).normalize_or_zero() * TAG_FIRE_SPEED[1];
        }
        true
    }

    /// Tekken Tag's mist (0x3A55E0): false when it is gone. `count` its frames moved (+0x16).
    fn tag_mist(&mut self, o: &mut Object, on: bool) -> bool {
        o.strikes &= on;
        o.prev = o.pos;
        if o.state == 0 {
            let mut unit = || {
                self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                ((self.seed & 0x7FFF_FFFF) as i32 & 0xFF_FFFF) as f32 / 16_777_216.0 * 2.0 - 1.0
            };
            let v = o.vel;
            let (pitch, yaw) = (v.y.datan2(glam::Vec2::new(v.x, v.z).length()), v.x.datan2(v.z));
            let (pitch, yaw) = (pitch + unit() * std::f32::consts::PI / 32.0, yaw + unit() * std::f32::consts::PI / 32.0);
            o.vel = Vec3::new(yaw.dsin() * pitch.dcos(), pitch.dsin(), yaw.dcos() * pitch.dcos()) * TAG_MIST_SPEED;
            (o.state, o.count, o.strikes) = (1, 0, on);
            return true;
        }
        o.pos += o.vel;
        o.count += 1;
        o.strikes &= o.count < TAG_MIST_STRIKES;
        o.count < TAG_MIST_LIFE
    }

    /// Doctor B.'s mist (0x800729FC): false when it is gone. `count` its frames (+0x48).
    fn mist(&mut self, o: &mut Object, on: bool) -> bool {
        o.strikes &= on;
        if o.state == 0 {
            // 0x80072B88: two draws, the first's low bits turning the heading (as the breath's), then on as any frame
            let r = self.rand();
            self.rand();
            let turn = |v: i32| v as f32 * std::f32::consts::TAU / 4096.0;
            let d = Vec3::new(o.vel.x, -o.vel.y, -o.vel.z);
            let yaw = d.z.datan2(d.x) + turn(((r >> 2) & 0xFF) - 0x7F);
            let pitch = glam::Vec2::new(d.x, d.z).length().datan2(d.y) + turn((r & 0xFF) - 0x7F);
            let (h, v) = (pitch.dsin(), pitch.dcos());
            o.vel = Vec3::new(h * yaw.dcos(), -v, -h * yaw.dsin()) * MIST_SPEED;
            (o.state, o.strikes) = (1, on);
        }
        o.count += 1;
        if o.count >= MIST_LIFE {
            return false;
        }
        o.strikes &= o.count < MIST_STRIKES;
        o.prev = o.pos;
        o.pos += o.vel;
        true
    }

    /// Fighter `owner`'s strikes this frame: its striking objects' travel.
    pub fn strikes(&self, owner: usize) -> Vec<(Vec3, Vec3)> {
        self.list.iter().filter(|o| o.owner == owner && o.strikes && (o.kind != Kind::Breath || self.field == (o.owner % 2 == 1))).map(|o| (o.prev, o.pos)).collect()
    }
}

/// 0x8006DF34: the pitch and the yaw of `e`, a quarter turn less (4096 a turn).
fn heading_angles(e: [i32; 3]) -> [i32; 2] {
    let yaw = (ratan2(e[0], e[2]) - 0x400) & 0xFFF;
    [(ratan2(e[1], reach(e[0], e[2], yaw)) - 0x400) & 0xFFF, yaw]
}

/// 0x8004B39C on Tekken 3's table (0x800210C8).
fn ratan2(x: i32, y: i32) -> i32 {
    crate::camera::ratan2_on(crate::t3_atan_table, x.into(), y.into()) as i32
}

/// 0x8006DFC4: the length along `yaw` of (dx, dz), the larger over the yaw's sine or cosine (x 8 in 16 bits, 0 as 1).
fn reach(dx: i32, dz: i32, yaw: i32) -> i32 {
    let (v, t) = if dz.abs() < dx.abs() { (dx, crate::ball::sin12(yaw)) } else { (dz, crate::ball::cos12(yaw)) };
    let t = match (t << 3) as i16 {
        0 => 1,
        t => i32::from(t),
    };
    (v.wrapping_shl(16).wrapping_div(t) >> 1).wrapping_abs()
}

/// The travel a frame of `speed` (16ths of a unit) at `angles` (0x8006E120: 16.16, on the 4096 sine table).
fn heading(angles: [i32; 2], speed: i32) -> [i32; 3] {
    let (p, y) = (-angles[0], -angles[1]);
    let t = (speed * crate::ball::cos12(p)) >> 12;
    [t * crate::ball::sin12(y), speed * crate::ball::sin12(p), t * crate::ball::cos12(y)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fire heads out of the head and rolls on along the floor at 128 a frame until it ends; the gas's eight puffs
    /// come out of the source by frame 10 and rise.
    #[test]
    fn fire_rolls_and_gas_rises() {
        let mut o = Objects::default();
        // a head 900 up, its x axis pointing forward (+z) and down
        let head = Mat4::from_cols(Vec3::new(0.0, -0.5, 0.8).extend(0.0), Vec3::Y.extend(0.0), Vec3::X.extend(0.0), Vec3::new(0.0, 900.0, 0.0).extend(1.0));
        o.start(0, FIRE, true, &[head, Mat4::IDENTITY], None);
        let mut frames = 0;
        let mut floor = false;
        while !o.list.is_empty() && frames < 40 {
            o.step(&[Some(FIRE), None], &[false; 2], true);
            frames += 1;
            if let Some(f) = o.list.first() {
                floor |= f.pos.y == FIRE_FLOOR;
                assert!(f.pos.z >= f.prev.z);
            }
        }
        assert!(floor && (20..=26).contains(&frames), "{frames}");
        o.start(1, GAS, true, &[Mat4::IDENTITY, Mat4::from_translation(Vec3::new(0.0, 300.0, 0.0))], None);
        for _ in 0..16 {
            o.step(&[None, Some(GAS)], &[false; 2], true);
        }
        let puffs: Vec<_> = o.list.iter().filter(|p| p.kind == Kind::Gas).collect();
        assert!(puffs.len() == 8 && puffs.iter().all(|p| p.strikes && p.pos.y > 300.0));
        assert!(!o.strikes(1).is_empty() && o.strikes(0).is_empty());
    }

    /// A heavy hit's 26 particles each draw twice on their first pass (20 thrown off, then 6 easing) and are over by
    /// the end of their lives (20-51 and 10-41 frames); a ki-charged fighter's glowing limbs draw once a pass each,
    /// an attack's lets out a trail, which draws twice on its first pass.
    #[test]
    fn heavy_hits_and_glows_draw_as_on_the_ps1() {
        let draws = |o: &Objects, n: usize| {
            let mut p = o.clone();
            (0..n).for_each(|_| _ = p.rand());
            p.seed
        };
        let mut o = Objects { seed: 1, ..Default::default() };
        o.heavy(0, [0, -900, 0], [100, 0, 0], false);
        assert_eq!(o.list.len(), 26);
        let want = draws(&o, 52);
        o.step(&[None, None], &[false; 2], true);
        assert_eq!(o.seed, want);
        for _ in 0..60 {
            o.step(&[None, None], &[false; 2], true);
        }
        assert!(o.list.is_empty());
        let mut o = Objects { seed: 1, glow_mask: vec![0x440, 1 << 14 | 1 << 31], joints: vec![vec![[0; 3]; 18]; 2], ..Default::default() };
        let start = o.clone();
        o.step(&[None, None], &[false; 2], true);
        assert!(o.list.iter().any(|x| x.kind == Kind::Trail) && o.list.iter().all(|x| x.state == 1));
        assert_eq!(o.seed, draws(&start, 3 + 2 * o.list.len()));
        assert_eq!(o.glows.iter().filter(|g| g.1).count(), 3);
    }

    /// Tekken Tag's mist moves 48 a frame for 21 frames, striking its first 15 moves while its owner is in the move.
    #[test]
    fn a_mist_puff_drifts_and_strikes_its_first_frames() {
        let mut o = Objects::default();
        o.start(0, TAG_MIST, true, &[Mat4::from_translation(Vec3::new(0.0, 1500.0, 0.0)), Mat4::IDENTITY], None);
        let mut strikes = Vec::new();
        while !o.list.is_empty() && strikes.len() < 40 {
            o.step(&[Some(TAG_MIST), None], &[false; 2], false);
            strikes.push(o.strikes(0).first().map(|(a, b)| (*b - *a).length()));
        }
        assert_eq!(strikes.len(), 22);
        assert!(strikes[1..16].iter().all(|d| d.is_some_and(|d| (d - 48.0).abs() < 0.01)) && strikes[16..].iter().all(Option::is_none), "{strikes:?}");
    }
}
