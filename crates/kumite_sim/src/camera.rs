//! The versus fight camera, integer-exact (kumite_converter's reference camera.md, and its prototype in
//! kumite_converter).
//!
//! Game space: Y points down, floor = 0, units ≈ mm; angles 4096 per turn, kept `<< 6` inside the module.
//! Routines: 0x8006465C sample, 0x80064810 yaw control, 0x80064E10 framing, 0x80063D00/0x80064170 blend,
//! 0x80068394 intro fly-in, 0x80062B74 slot mixer, 0x80064080 apply.
//! The sine/atan tables and the intro offsets come from the converted `gamedata/camera.json`.

use anyhow::Result;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

/// Tekken 3's camera.json has every key; a game whose converter writes fewer (Tekken Tag) gets Tekken 3's values for
/// the missing ones: its tables computed, its end cameras, its framing constants.
#[derive(Deserialize)]
struct Json {
    projection_h: i64,
    screen_width: Option<i64>,
    tables: Option<JsonTables>,
    fight: JsonFight,
    intro: JsonIntro,
    gsqrt: Option<JsonValues>,
    end: Option<EndData>,
    arena_projection: Option<JsonArenas>,
    team_intro: Option<TeamIntro>,
    replay_shots: Option<ReplayShots>,
}
#[derive(Deserialize)]
struct JsonArenas {
    rows: Vec<JsonArena>,
}
#[derive(Deserialize)]
struct JsonArena {
    h: i64,
    h_min: i64,
    h_max: i64,
}
#[derive(Deserialize)]
struct JsonTables {
    sin: JsonValues,
    atan: JsonValues,
}
#[derive(Deserialize)]
struct JsonValues {
    values: Vec<i64>,
}
#[derive(Deserialize)]
struct JsonFight {
    distance: i64,
    /// The depth is H * n / d (Tekken Tag), not `distance` at any H.
    distance_per_h: Option<[i64; 2]>,
    initial_eye: [i64; 3],
    fit_px: Option<i64>,
    dead_zone_px: Option<i64>,
    vertical_keep_px: Option<i64>,
    centre_drop: Option<i64>,
    yaw: Option<JsonYaw>,
}
#[derive(Deserialize)]
struct JsonYaw {
    /// Camera id ("0x27") -> yaw speed cap.
    move_caps: std::collections::BTreeMap<String, i64>,
}
#[derive(Deserialize)]
struct JsonIntro {
    frames_first_round: i64,
    frames_other: i64,
    /// The offset eases over the last this many frames (Tekken 3: all of them).
    ease_frames: Option<i64>,
    blend_frames: Option<i64>,
    offsets: Vec<[i64; 3]>,
}

/// Round-end constants (camera.json `end`).
#[derive(Deserialize, Clone, Debug)]
pub struct EndData {
    pub ko_frames: u32,
    pub draw_frames: u32,
    pub loser_frames: u32,
    pub pose_cap: u32,
    pub replay: EndReplay,
    aim: EndAim,
    cam_a: EndCamA,
    cam_b: EndCamB,
    overhead: EndOverhead,
    orbit: EndOrbit,
    pub motions: EndMotions,
}
#[derive(Deserialize, Clone, Debug)]
pub struct EndReplay {
    pub pre_roll: u32,
    pub min_frames: u32,
    pub ring: u32,
    pub blink: u32,
    variant_a: EndVariantA,
    variant_b: EndVariantB,
    offset_easing_shift: u32,
}
#[derive(Deserialize, Clone, Debug)]
struct EndVariantA {
    yaw_from: i64,
    yaw_range: i64,
    offset_degrees: [i64; 2],
}
#[derive(Deserialize, Clone, Debug)]
struct EndVariantB {
    yaw_from: i64,
    yaw_range: i64,
}
#[derive(Deserialize, Clone, Debug)]
struct EndAim {
    min_distance: i64,
    chest_clamp: i64,
    floor_clamp: i64,
    chest_offset: i64,
}
#[derive(Deserialize, Clone, Debug)]
struct EndCamA {
    arc_from: i64,
    arc_range: i64,
    radius: i64,
    eye_y: i64,
    stop_distance: i64,
    approach: i64,
    pitch_shift: u32,
    yaw_shift: u32,
}
#[derive(Deserialize, Clone, Debug)]
struct EndCamB {
    radius: [i64; 3],
    side: [i64; 3],
    height: [i64; 3],
    drift_shift: u32,
    push_distance: i64,
    push_shift: u32,
    pitch_shift: u32,
    yaw_shift: u32,
    height_reach: i64,
    height_shift: u32,
}
#[derive(Deserialize, Clone, Debug)]
struct EndOverhead {
    height: i64,
    pitch: i64,
    yaw_from: i64,
    jitter: [i64; 2],
}
#[derive(Deserialize, Clone, Debug)]
struct EndOrbit {
    arc_from: i64,
    arc_range: i64,
    radius: [i64; 2],
    eye_y: i64,
    target_y: i64,
    stop_distance: [i64; 2],
    approach: i64,
    target_easing: [i64; 2],
    /// The large character's move set.
    large_moveset_key: i64,
}
#[derive(Deserialize, Clone, Debug)]
pub struct EndMotions {
    pub win: Vec<usize>,
    pub lose_ko: usize,
    pub lose_time_up: usize,
}

/// Tekken Tag's team intro before a match's first round (camera.json `team_intro`, 0x3B5328): each team on the stage,
/// a shot each.
#[derive(Deserialize, Clone, Debug)]
pub struct TeamIntro {
    pub frames: i64,
    pub place: TeamPlace,
    shots: Vec<TeamShot>,
    pair_scale: i64,
    /// The pairs with their own places and motions (0x43A180).
    pub pairs: Vec<[PairMember; 2]>,
}
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct TeamPlace {
    pub x: i64,
    pub z: i64,
}
#[derive(Deserialize, Clone, Debug)]
struct TeamShot {
    turn_from: [i64; 3],
    turn_to: [i64; 3],
    target_from: [i64; 3],
    target_to: [i64; 3],
    eye_from: [i64; 3],
    eye_to: [i64; 3],
}
/// A pair member's place (game units, facing 65536 a turn) and motion; costume -1: any.
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct PairMember {
    pub character: i64,
    pub costume: i64,
    pub alias: usize,
    pub x: i64,
    pub z: i64,
    pub facing: i64,
}

/// Tekken Tag's K.O. replay shots (camera.json `replay_shots`, 0x3D0748).
#[derive(Deserialize, Clone, Debug)]
pub struct ReplayShots {
    pub sequences: Vec<[i8; 4]>,
    shots: Vec<ReplayShot>,
    eye_y: i64,
    target_y: i64,
    pitch_max: i64,
    follow_easing: [i64; 2],
    distance_per_h: i64,
}
#[derive(Deserialize, Clone, Debug)]
struct ReplayShot {
    target: i64,
    follow: i64,
    turn: [i64; 3],
    turn_range: [i64; 3],
    distance: i64,
    distance_range: i64,
}

/// Camera constants and tables from camera.json.
#[derive(Clone)]
pub struct CameraData {
    sin: Vec<i64>,
    gsqrt_table: Vec<i64>,
    pub end: EndData,
    atn: Vec<i64>,
    pub distance: i64,
    pub h: i64,
    /// The projection's clamp (Tekken 3: H itself).
    pub h_range: [i64; 2],
    /// The width in pixels H is measured on (Tekken 3 368).
    pub screen_width: i64,
    pub initial_eye: [i64; 3],
    pub intro_frames: [i64; 2],
    pub intro_offsets: Vec<[i64; 3]>,
    /// The fly-in's easing length and its hand-off to the fight camera (frames).
    pub intro_ease: Option<i64>,
    pub intro_blend: i64,
    /// Framing (0x80064E10): the fit width, the dead zone and the top fighter's keep-in (pixels), the centre drop.
    pub fit_px: i64,
    pub dead_zone_px: i64,
    pub keep_px: i64,
    pub centre_drop: i64,
    /// (camera id, yaw speed cap) of the moves that cap it, by id.
    pub move_caps: Vec<(i64, i64)>,
    /// Per arena: H, its clamp; the depth H * n / d.
    arenas: Vec<[i64; 3]>,
    distance_per_h: Option<[i64; 2]>,
    pub team_intro: Option<TeamIntro>,
    pub replay_shots: Option<ReplayShots>,
}

impl CameraData {
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(crate::data::load(path)?)
    }

    /// A game without a decoded camera (Tekken Tag's packs before their camera.json): Tekken 3's constants, its
    /// tables computed (see `from_json`), Tekken Tag's win poses (4770..4773).
    pub fn provisional() -> Result<Self> {
        let j = serde_json::json!({
            "projection_h": 500,
            "fight": { "distance": 5517, "initial_eye": [0, -1450, -8000] },
            "intro": { "frames_first_round": 120, "frames_other": 53, "offsets": [[0, 0, -7500], [0, 375, -6000], [6000, 375, 0], [-6000, 375, 0], [11250, 0, 0], [-11250, 0, 0], [9000, 0, 7500], [-9000, 0, 7500], [7500, -6000, -3750], [-7500, -6000, -3750]] },
        });
        Self::from_json(serde_json::from_value(j)?)
    }

    /// The end cameras of a game whose camera.json has none: Tekken 3's, with Tekken Tag's win poses.
    // shortcut: Tekken 3's end cameras on Tekken Tag; upgrade when its win-pose and replay cameras are converted
    fn provisional_end() -> Result<EndData> {
        let j = serde_json::json!({
            "ko_frames": 90, "draw_frames": 75, "loser_frames": 60, "pose_cap": 300,
            "replay": { "pre_roll": 30, "min_frames": 150, "ring": 300, "blink": 16, "variant_a": { "yaw_from": -29127, "yaw_range": 123790, "offset_degrees": [25, 30] }, "variant_b": { "yaw_from": 21846, "yaw_range": 87381 }, "offset_easing_shift": 7 },
            "aim": { "min_distance": 5800, "chest_clamp": 4004, "floor_clamp": -6917, "chest_offset": -290 },
            "cam_a": { "arc_from": 683, "arc_range": 683, "radius": 3953, "eye_y": -850, "stop_distance": 2513, "approach": 8, "pitch_shift": 5, "yaw_shift": 5 },
            "cam_b": { "radius": [4500, 5000, 5500], "side": [1800, 2000, 2200], "height": [-950, -1100, -1650], "drift_shift": 9, "push_distance": 2308, "push_shift": 4, "pitch_shift": 5, "yaw_shift": 4, "height_reach": -7500, "height_shift": 4 },
            "overhead": { "height": -7853, "pitch": 1024, "yaw_from": 2048, "jitter": [-64, 63] },
            "orbit": { "arc_from": 512, "arc_range": 1025, "radius": [3978, 6596], "large_moveset_key": 11, "eye_y": -1500, "target_y": -1425, "stop_distance": [3664, 6282], "approach": 2, "target_easing": [6, 7] },
            // (the losers' motions are not known: none)
            "motions": { "win": [4770, 4771, 4772, 4773], "lose_ko": usize::MAX, "lose_time_up": usize::MAX }
        });
        Ok(serde_json::from_value(j)?)
    }

    /// Missing tables are computed: the sine and the square root as Tekken 3's disc has them, the atan within 1.
    fn from_json(j: Json) -> Result<Self> {
        use crate::detmath::DetMath;
        let (pi, tau) = (std::f64::consts::PI, std::f64::consts::TAU);
        let (sin, atn) = match j.tables {
            Some(t) => (t.sin.values, t.atan.values),
            None => (
                (0..5120).map(|i| (4096.0 * (i as f64 * tau / 4096.0).dsin()).round() as i64).collect(),
                (0..=1024).map(|q| ((q as f64 / 1024.0).datan2(1.0) * 2048.0 / pi).round() as i64 - (q >> 1)).collect(),
            ),
        };
        let gsqrt_table: Vec<i64> = match j.gsqrt {
            Some(g) => g.values,
            None => (0..192).map(|k| (libm::sqrt((k + 64) as f64 / 64.0) * 4096.0) as i64).collect(),
        };
        anyhow::ensure!(sin.len() == 5120, "camera.json: sin table needs 5120 entries");
        anyhow::ensure!(atn.len() == 1025, "camera.json: atan table needs 1025 entries");
        anyhow::ensure!(!j.intro.offsets.is_empty(), "camera.json: no intro offsets");
        anyhow::ensure!(gsqrt_table.len() == 192, "camera.json: gsqrt table needs 192 entries");
        let f = j.fight;
        let mut move_caps = Vec::new();
        for (id, cap) in f.yaw.map(|y| y.move_caps).unwrap_or_else(|| [("0x27", 0x200), ("0x28", 0x180), ("0x29", 0x80), ("0x2A", 0x40)].map(|(k, v)| (k.to_string(), v)).into()) {
            let id = i64::from_str_radix(id.trim_start_matches("0x"), 16).map_err(|e| anyhow::anyhow!("camera.json: move cap id {id}: {e}"))?;
            move_caps.push((id, cap));
        }
        move_caps.sort_unstable();
        let blend = j.intro.blend_frames.unwrap_or(16);
        anyhow::ensure!(blend > 0 && j.intro.ease_frames.is_none_or(|e| e > 0), "camera.json: intro frames must be positive");
        anyhow::ensure!(j.screen_width.is_none_or(|w| w > 0), "camera.json: screen_width must be positive");
        Ok(Self {
            sin,
            gsqrt_table,
            end: match j.end {
                Some(e) => e,
                None => Self::provisional_end()?,
            },
            atn,
            distance: f.distance,
            h: j.projection_h,
            h_range: [j.projection_h; 2],
            screen_width: j.screen_width.unwrap_or(368),
            initial_eye: f.initial_eye,
            intro_frames: [j.intro.frames_first_round, j.intro.frames_other],
            intro_offsets: j.intro.offsets,
            intro_ease: j.intro.ease_frames,
            intro_blend: blend,
            fit_px: f.fit_px.unwrap_or(280),
            dead_zone_px: f.dead_zone_px.unwrap_or(48),
            keep_px: f.vertical_keep_px.unwrap_or(112),
            centre_drop: f.centre_drop.unwrap_or(100),
            move_caps,
            arenas: j.arena_projection.map_or_else(Vec::new, |a| a.rows.iter().map(|r| [r.h, r.h_min, r.h_max]).collect()),
            distance_per_h: f.distance_per_h,
            team_intro: j.team_intro,
            replay_shots: j.replay_shots,
        })
    }

    /// The camera of arena `arena` (0x38DBC0 at the round reset): its H, the clamp, the depth from H; None for a game
    /// with one projection (Tekken 3).
    pub fn for_arena(&self, arena: usize) -> Option<Self> {
        let &[h, lo, hi] = self.arenas.get(arena)?;
        let mut d = self.clone();
        (d.h, d.h_range) = (h, [lo, hi]);
        if let Some([n, den]) = self.distance_per_h {
            d.distance = tdiv(h * n, den);
        }
        Some(d)
    }

    /// The move set of the large character ([`Subject::large`]).
    pub fn large_moveset(&self) -> i64 {
        self.end.orbit.large_moveset_key
    }

    /// rsin / rcos of a 4096-unit angle.
    pub fn sin(&self, a: i64) -> i64 {
        self.sin[(a & 0xFFF) as usize]
    }
    pub fn cos(&self, a: i64) -> i64 {
        self.sin[(a & 0xFFF) as usize + 1024]
    }

    /// 0x8004AEDC: the GTE's approximate square root (SquareRoot0, table 0x8004AF40).
    pub fn gsqrt(&self, a: i64) -> i64 {
        if a <= 0 {
            return 0;
        }
        let a = a.min(u32::MAX as i64) as u32;
        let lz = (a.leading_zeros() & !1) as i64;
        let m = if lz <= 24 { (a >> (24 - lz)) as i64 } else { (a as i64) << (lz - 24) };
        (self.gsqrt_table[(m - 64).clamp(0, 191) as usize] << ((31 - lz) >> 1)) >> 12
    }

    fn sin6(&self, a6: i64) -> i64 {
        self.sin[idx6(a6)]
    }
    fn cos6(&self, a6: i64) -> i64 {
        self.sin[idx6(a6) + 1024]
    }

    /// 0x8004B39C: atan2(y, x) in 0..4096.
    pub fn ratan2(&self, x: i64, y: i64) -> i64 {
        if x == 0 && y == 0 {
            return 0;
        }
        let at = |q: i64| self.atn[q as usize] + (q >> 1);
        let r = if x >= 0 {
            if y >= 0 {
                if x >= y {
                    at(tdiv(y << 10, x))
                } else {
                    0x400 - at(tdiv(x << 10, y))
                }
            } else {
                let ny = -y;
                if x >= ny {
                    0x1000 - at(tdiv(ny << 10, x))
                } else {
                    at(tdiv(x << 10, ny)) + 0xC00
                }
            }
        } else {
            let nx = -x;
            if y >= 0 {
                if nx >= y {
                    0x800 - at(tdiv(y << 10, nx))
                } else {
                    at(tdiv(nx << 10, y)) + 0x400
                }
            } else if nx < -y {
                0xC00 - at(tdiv(x << 10, y))
            } else {
                at(tdiv(y << 10, x)) + 0x800
            }
        };
        s16(r)
    }
}

// ---------------------------------------------------------------- MIPS integer helpers

/// MIPS `div`: truncate toward zero (by zero, which the original's data never does: 0).
pub fn tdiv(a: i64, b: i64) -> i64 {
    a.checked_div(b).unwrap_or(0)
}
/// `bgez; addiu 2^n-1; sra n`: divide by 2^n toward zero.
pub fn sra_tz(a: i64, n: u32) -> i64 {
    (if a < 0 { a + (1 << n) - 1 } else { a }) >> n
}
fn half(a: i64) -> i64 {
    a / 2
}
/// Angle difference in <<6 units wrapped to [-0x20000, 0x20000).
pub fn wrap18(v: i64) -> i64 {
    let v = v & 0x3FFFF;
    if v & 0x20000 != 0 {
        v - 0x40000
    } else {
        v
    }
}
fn s16(v: i64) -> i64 {
    v as i16 as i64
}
fn idx6(a6: i64) -> usize {
    (sra_tz(a6, 6) & 0xFFF) as usize
}
/// 0x8002A324 (Newton, unsigned 32-bit).
pub fn isqrt(n: i64) -> i64 {
    let n = n & 0xFFFF_FFFF;
    if n == 0 {
        return 0;
    }
    let (mut v, mut a) = (1i64, n);
    while v < a {
        v <<= 1;
        a >>= 1;
    }
    loop {
        a = v;
        v = (n / a + a) >> 1;
        if v >= a {
            return a;
        }
    }
}

// ---------------------------------------------------------------- state

/// 24-byte key slot (table 0x800A02A8): eye, pitch<<6, yaw<<6, projection distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Key {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub p: i64,
    pub w: i64,
    pub h: i64,
}

/// 48-byte per-slot state (0x800A0328 + 0x30*slot).
#[derive(Clone, Copy, Debug, Default)]
struct SlotState {
    state: i64,
    moving: bool,
    count: i64,
    yawvel: i64,
    yawacc: i64,
    pitch: i64,
    pitch_tgt: i64,
    pitch_step: i64,
    yawerr: i64,
    vstep: i64,
    vpull: i64,
    hpull: i64,
}

/// What the camera samples from one fighter (0x8006465C), game space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub x: i64,
    /// Chest (bone slot 2) Y − 290: about −1715 standing.
    pub y: i64,
    pub z: i64,
    /// Start side: −100 for the player who began on the left, +100 for the right.
    pub side: i64,
}

/// Render view (RVIEW): eye in game space, pitch/yaw/roll in 4096 units, projection distance H.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub eye: [i64; 3],
    pub pitch: i64,
    pub yaw: i64,
    pub h: i64,
    /// The picture turned anticlockwise (only Force's slanted stage has one).
    pub roll: i64,
}

#[derive(Clone)]
pub struct FightCamera {
    data: Arc<CameraData>,
    h: i64,
    hmin: i64,
    hmax: i64,
    keys: [Key; 5],
    st: [SlotState; 5],
    samp: [Sample; 2],
    /// Ball: the ball, a third framing point (record 8 0x800B4484; game units, y down).
    pub ball: Option<[i64; 3]>,
    w: [i64; 5],
    wprev: [i64; 5],
    pub frame: i64,
    intro: bool,
    intro_len: i64,
    intro_t: Option<i64>,
    intro_var: usize,
    intro_base: Key,
    replay: Option<Replay>,
    pub view: View,
}

/// The replay camera's block (0x800A04C8): distance, yaw and its sweeping offset (both kept in 16 bits, as the
/// game stores them: replay angles wrap every 90 degrees), first-frame snap.
#[derive(Clone, Copy, Debug)]
struct Replay {
    dist: i64,
    yaw: i64,
    off: i64,
    target: i64,
    snap: bool,
}

impl FightCamera {
    /// `intro`: play the round-start fly-in (variant = round counter % 10).
    pub fn new(data: Arc<CameraData>, intro: bool, intro_variant: usize, first_round: bool) -> Self {
        let (h, [hmin, hmax]) = (data.h, data.h_range);
        let [ex, ey, ez] = data.initial_eye;
        let mut keys = [Key::default(); 5];
        for k in &mut keys[..2] {
            *k = Key { x: ex, y: ey, z: ez, p: 0, w: 0, h };
        }
        let n = data.intro_offsets.len();
        let intro_len = data.intro_frames[if first_round { 0 } else { 1 }];
        Self {
            data,
            h,
            hmin,
            hmax,
            keys,
            st: [SlotState::default(); 5],
            samp: [Sample { x: 0, y: 0, z: 0, side: 0 }; 2],
            ball: None,
            w: [0x1000, 0, 0, 0, 0],
            wprev: [0; 5],
            frame: 0,
            intro,
            intro_len,
            intro_t: None,
            intro_var: intro_variant % n,
            intro_base: Key::default(),
            replay: None,
            view: View::default(),
        }
    }

    pub fn data(&self) -> &Arc<CameraData> {
        &self.data
    }

    /// True while the round-intro fly-in is still playing.
    pub fn in_intro(&self) -> bool {
        self.intro
    }

    /// One frame of the versus camera director (phase 1 intro, phase 2 normal).
    /// `frames_since_start`: the yaw controller stays frozen for the first 200 frames of a round.
    /// `move_cam`: camera-control values of the two current moves (`move_caps`: Tekken 3 0x27..0x2A, Tekken Tag 0x13..0x16 cap the yaw speed).
    pub fn step(&mut self, p: [Sample; 2], frames_since_start: i64, move_cam: [i64; 2]) -> View {
        self.sample(p);
        if self.intro && self.intro_t.is_none() {
            self.slot0_update(true, frames_since_start, move_cam);
            self.intro_base = self.keys[0];
            self.intro_t = Some(self.intro_len);
            self.w = [0, 0, 0x1000, 0, 0];
            self.wprev = [0; 5];
        }
        if self.intro && self.intro_t.is_some_and(|t| t > 0) {
            let v = self.intro_key();
            self.w = [0x1000 - v, 0, v, 0, 0];
            if self.w[0] > 0 {
                self.slot0_update(self.wprev[0] < 1, frames_since_start, move_cam);
            }
            self.mix();
            if self.w[2] <= 0 {
                self.intro = false;
            }
        } else {
            self.w = [0x1000, 0, 0, 0, 0];
            self.slot0_update(self.wprev[0] < 1, frames_since_start, move_cam);
            self.mix();
        }
        self.frame += 1;
        self.view
    }

    /// 0x8006953C: start the replay camera. `variant_a`: the sweeping variant (the game alternates, B first);
    /// `random(n)`: 0..n. The samples are the replay's (root x/z, y = min(chest, root y)).
    pub fn start_replay(&mut self, p: [Sample; 2], variant_a: bool, random: &mut dyn FnMut(i64) -> i64) {
        let [ex, ey, ez] = self.data.initial_eye;
        self.keys[1] = Key { x: ex, y: ey, z: ez, p: 0, w: 0, h: self.data.h };
        self.st[1] = SlotState::default();
        self.intro = false;
        self.sample(p);
        let r = &self.data.end.replay;
        let [a, b] = self.samp;
        let axis = self.data.ratan2(b.x - a.x, b.z - a.z);
        let yaw = s16(axis << 6);
        let deg6 = |d: i64| d * 0x40000 / 360;
        let (yaw, off, target) = if variant_a {
            let va = &r.variant_a;
            let yaw = s16(u16v(yaw) + va.yaw_from + random(va.yaw_range));
            let [lo, span] = va.offset_degrees;
            let (mut off, mut tgt) = (s16(deg6(lo + random(span))), -s16(deg6(lo + random(span))));
            if random(2) == 1 {
                off = -off;
                tgt = -tgt;
            }
            (yaw, off, tgt)
        } else {
            let vb = &r.variant_b;
            (s16(u16v(yaw) + vb.yaw_from + random(vb.yaw_range)), 0, 0)
        };
        self.replay = Some(Replay { dist: 0x2B1A, yaw, off, target, snap: true });
    }

    /// 0x80069414: one replay camera frame (slot 1 framing, the offset and distance eased).
    pub fn replay_step(&mut self, p: [Sample; 2]) -> View {
        self.sample(p);
        let Some(mut r) = self.replay else { return self.view };
        self.frame_key(1, 0, r.yaw + r.off, r.snap, r.dist);
        r.snap = false;
        r.off = s16(r.off + sra_tz(wrap18(r.target - r.off), self.data.end.replay.offset_easing_shift));
        r.dist += sra_tz((0x113D - r.dist) * 6, 8);
        self.replay = Some(r);
        let k = self.keys[1];
        self.apply(k);
        self.view
    }

    fn sample(&mut self, p: [Sample; 2]) {
        self.samp = p;
        let [a, b] = &mut self.samp;
        if a.x == b.x && a.z == b.z {
            a.x -= 1;
            a.z -= 1;
            b.x += 1;
            b.z += 1;
        }
    }

    fn slot0_update(&mut self, snap: bool, fss: i64, mc: [i64; 2]) {
        let (yaw, flag) = self.yaw_control(snap, fss, mc);
        self.frame_key(0, self.st[0].pitch, yaw, flag, self.data.distance);
    }

    /// 0x80064810 yaw controller (slot 0) -> (yaw<<6, snap).
    fn yaw_control(&mut self, snap: bool, fss: i64, move_cam: [i64; 2]) -> (i64, bool) {
        let d_ = self.data.clone();
        let key = self.keys[0];
        let st = &mut self.st[0];
        let [a, b] = self.samp;
        let mut ang = d_.ratan2(b.x - a.x, b.z - a.z);
        if a.side >= b.side {
            ang = (ang + 0x800) & 0xFFF;
        }
        let tgt = ang << 6;
        let diff = wrap18(tgt - key.w);
        let sgn = diff.signum();
        let d = diff.abs();
        if snap {
            st.yawvel = 0;
            st.yawacc = 0;
            st.pitch = 0;
            st.pitch_tgt = 0;
            st.pitch_step = 0;
            return (tgt, true);
        }
        if d > 0x1AAAA || fss < 200 {
            return (key.w, false); // frozen (side switch / round start)
        }
        let mut dyaw = 0;
        if st.state == 0 {
            st.count = if d >= 0xE38 { st.count + 1 } else { 0 };
            if st.count > 0 {
                st.state = 1;
                st.yawacc += sgn * 0x600;
                st.count = 0;
            }
        } else if st.state == 1 {
            if d < 0x38E4 {
                let w = st.yawvel;
                st.yawacc -= half(st.yawacc);
                st.yawvel = w - sra_tz(w * 14, 7);
                st.yawacc += sgn * 0x300;
                if d < 0xE39 {
                    st.yawacc -= half(st.yawacc);
                    st.count += 1;
                    if st.count >= 0x20 {
                        st.yawvel = 0;
                        st.state = 0;
                        st.count = 0;
                    }
                } else {
                    st.count = 0;
                }
            } else {
                st.yawacc += sgn * 0x600;
            }
            st.yawvel += st.yawacc;
            let lim = d_.move_caps.iter().find(|c| move_cam.contains(&c.0)).map_or(-1, |c| c.1);
            if lim > 0 {
                let l = (lim * 56784) >> 8;
                if l < st.yawvel {
                    st.yawacc -= 0x600;
                    st.yawvel = l;
                } else if st.yawvel < -l {
                    st.yawacc += 0x600;
                    st.yawvel = -l;
                }
            } else if st.yawvel > 0xDDD0 {
                st.yawvel = 0xDDD0;
                st.yawacc -= 0x600;
            } else if st.yawvel < -0xDDD0 {
                st.yawvel = -0xDDD0;
                st.yawacc += 0x600;
            }
            dyaw = sra_tz(st.yawvel, 6);
        }
        // pitch lag
        st.yawerr = d;
        st.pitch_tgt = if d >= 0x38E3 {
            let v = (((d - 0x38E3) << 12) * 0x14923A73) >> 44;
            sra_tz(d_.sin[((sra_tz(v, 7) & 0x1FFE) >> 1) as usize] * 279616, 12)
        } else {
            0
        };
        if st.moving {
            let step = sra_tz(st.pitch_tgt - st.pitch, 4).clamp(-0x100, 0x100);
            st.pitch_step = step;
            st.pitch += step;
        } else {
            st.pitch_step = 0;
        }
        (key.w + dyaw, false)
    }

    /// 0x80064E10 framing solver ("camera key") for a slot (0 = fight, 1 = replay).
    fn frame_key(&mut self, slot: usize, pitch6: i64, yaw6: i64, flag: bool, dist: i64) {
        let d_ = self.data.clone();
        let (sin6, cos6) = (|a| d_.sin6(a), |a| d_.cos6(a));
        let h = self.h;
        // Tekken 3: fit 280 px, dead zone +-48, keep-in 112; Tekken Tag x 1.4
        let (fit, dead, keep) = (d_.fit_px, d_.dead_zone_px, d_.keep_px);
        let dist = if dist < 0x321 { 0x320 } else { dist };
        let key = self.keys[slot];
        let st = &mut self.st[slot];
        let (s0, s5) = (sin6(-pitch6), cos6(-pitch6));
        let (s4, s3) = (sin6(-yaw6), cos6(-yaw6));
        let [a, b] = self.samp;
        let (mx, mz) = (half(a.x + b.x), half(a.z + b.z));
        let (ex, ez) = if flag {
            (mx + sra_tz(dist * cos6(yaw6 + 0x30000), 12), mz + sra_tz(dist * sin6(yaw6 + 0x30000), 12))
        } else {
            let dd = wrap18(yaw6 - key.w);
            let (c, s) = (cos6(dd), sin6(dd));
            let (ox, oz) = (key.x - mx, key.z - mz);
            (mx + sra_tz(c * ox - s * oz, 12), mz + sra_tz(s * ox + c * oz, 12))
        };
        let ey = key.y;
        // the fighters' samples, and in Ball the ball as a third point (index 2)
        let (mut xs, mut ys, mut zs, mut hds) = ([0i64; 3], [0i64; 3], [0i64; 3], [0i64; 3]);
        let ball = self.ball;
        let points = self.samp.iter().map(|p| [p.x, p.y, p.z]).chain(ball);
        for (i, p) in points.enumerate() {
            let (dx, dz, dy) = (p[0] - ex, p[2] - ez, p[1] - ey);
            let hd = sra_tz(s4 * dx + s3 * dz, 12);
            xs[i] = sra_tz(s3 * dx - s4 * dz, 12);
            zs[i] = sra_tz(s5 * hd - s0 * dy, 12);
            ys[i] = sra_tz(s0 * hd + s5 * dy, 12);
            hds[i] = hd;
        }
        let (mut dx_, mut dz_) = (0i64, 0i64);
        // depth: centre point (mid horizontal depth, height -450) to depth = dist
        let ahd = half(hds[0] + hds[1]);
        let dyc = -0x1C2 - ey;
        let mut zc = sra_tz(s5 * ahd - s0 * dyc, 12);
        let yc = sra_tz(s0 * ahd + s5 * dyc, 12);
        let a0 = dist - zc;
        zc = dist;
        dz_ -= a0;
        zs[0] += a0;
        zs[1] += a0;
        zs[2] += a0;
        // horizontal framing (150-unit margins, 280-px window, 48-px dead zone)
        let (mut l, mut r) = if xs[0] < xs[1] {
            xs[0] -= 150;
            xs[1] += 150;
            (0, 1)
        } else {
            xs[1] -= 150;
            xs[0] += 150;
            (1, 0)
        };
        // the ball widens the pair on the side it is out on (0x80064E10, battle type 7)
        if ball.is_some() {
            if xs[2] < xs[l] {
                xs[2] -= 150;
                l = 2;
            } else if xs[2] > xs[r] {
                xs[2] += 150;
                r = 2;
            }
        }
        let sxl = tdiv(xs[l] * h, zs[l]);
        let sxr = tdiv(xs[r] * h, zs[r]);
        if sxr - sxl > fit {
            let v1 = tdiv(h * (xs[r] - xs[l]) - (fit / 2) * (zs[r] - zs[l]), fit);
            let mut a1 = zs[l] - v1;
            if !flag {
                let prev = st.hpull;
                a1 = if prev < a1 { prev + sra_tz(a1 - prev, 5) } else { prev - sra_tz(prev - a1, 3) };
            }
            st.hpull = a1;
            dz_ += a1;
            dx_ = sra_tz(xs[l] - tdiv(-((fit / 2) * v1), h), 3);
            zs[0] -= a1;
            zs[1] -= a1;
            zs[2] -= a1;
            zc -= a1;
        } else {
            let mut decay = true;
            let b2 = tdiv(dead * zs[r], h);
            let a2 = tdiv(-dead * zs[l], h);
            if flag || (b2 - (xs[r] - xs[l])) < a2 {
                dx_ = sra_tz(xs[l] + tdiv((xs[r] - xs[l]) * zs[l], zs[l] + zs[r]), 3);
                if flag {
                    st.hpull = 0;
                    decay = false;
                }
            } else if sxl < -dead {
                dx_ += sra_tz(xs[l] - a2, 3);
            } else if sxr > dead {
                dx_ += sra_tz(xs[r] - b2, 3);
            }
            if decay {
                let prev = st.hpull;
                let n = if prev < 0 {
                    prev - sra_tz(prev, 5)
                } else if prev > 0 {
                    prev - (prev >> 3)
                } else {
                    0
                };
                st.hpull = n;
                dz_ += n;
                zs[0] -= n;
                zs[1] -= n;
                zs[2] -= n;
                zc -= n;
            }
        }
        // vertical framing
        let sy = [tdiv(ys[0] * h, zs[0]), tdiv(ys[1] * h, zs[1])];
        let mut top = if sy[0] < sy[1] { 0 } else { 1 };
        // the ball can be the top point
        if ball.is_some() && zs[2] != 0 && tdiv(ys[2] * h, zs[2]) < sy[top] {
            top = 2;
        }
        let (ytop, ztop) = (ys[top], zs[top]);
        let t1 = d_.centre_drop;
        let mut a3 = sra_tz(s5 * (t1 - tdiv(450 * h, dist)), 12);
        let mut pull = (tdiv(-(keep * (ztop - zc)) - h * (ytop - yc), a3 + keep) - zc).max(0);
        if flag {
            pull = 0;
            st.vstep = 0;
        } else {
            let prev = st.vpull;
            if pull < prev {
                let dlt = (prev - pull).min(0x4B0);
                st.vstep = sra_tz(dlt * 120, 7) - dlt;
                pull = prev + st.vstep;
            } else if pull > prev {
                let dlt = (pull - prev).min(0x320);
                st.vstep = sra_tz(dlt, 3);
                pull = prev + st.vstep;
            }
        }
        st.vpull = pull;
        if pull > 0 {
            dz_ -= pull;
            zc += pull;
        }
        a3 += sra_tz(tdiv(sra_tz(pull * s5, 12) * h, dist), 4);
        let dy_ = yc - tdiv(a3 * zc, h);
        // view-space delta -> world
        let (sp, cp) = (sin6(pitch6), cos6(pitch6));
        let hd = sra_tz(dz_ * cp - dy_ * sp, 12);
        let wdy = sra_tz(dz_ * sp + dy_ * cp, 12);
        let (sy_, cy_) = (sin6(yaw6), cos6(yaw6));
        let wdx = sra_tz(dx_ * cy_ - hd * sy_, 12);
        let wdz = sra_tz(dx_ * sy_ + hd * cy_, 12);
        let (nx, ny, nz) = (ex + wdx, ey + wdy, ez + wdz);
        if !flag {
            let (ddx, ddz) = (nx - key.x, nz - key.z);
            st.moving = ddx * ddx + ddz * ddz > 1000;
        }
        self.keys[slot] = Key { x: nx, y: ny, z: nz, p: pitch6, w: yaw6, h: key.h };
    }

    /// 0x80063D00: look target of a key (distance = mean eye-fighter distance).
    fn target(&self, k: &Key) -> (i64, [i64; 3]) {
        let d_ = &self.data;
        let (sp, cp) = (d_.sin6(k.p), d_.cos6(k.p));
        let yy = k.w + 0x10000;
        let (sy, cy) = (d_.sin6(yy), d_.cos6(yy));
        let mut d = 0;
        for p in &self.samp {
            d += isqrt((p.x - k.x).pow(2) + (p.y - k.y).pow(2) + (p.z - k.z).pow(2));
        }
        d >>= 1;
        (d, [k.x + sra_tz(sra_tz(d * cy, 12) * cp, 12), k.y + sra_tz(d * sp, 12), k.z + sra_tz(sra_tz(d * sy, 12) * cp, 12)])
    }

    /// 0x80064170: orbit blend of two keys around their look targets.
    fn blend(&self, a: &Key, b: &Key, wa: i64) -> Key {
        let d_ = &self.data;
        let wb = 0x1000 - wa;
        let (da, ta) = self.target(a);
        let (db, tb) = self.target(b);
        let d = sra_tz(da * wa + db * wb, 12);
        let t: [i64; 3] = std::array::from_fn(|i| sra_tz(ta[i] * wa + tb[i] * wb, 12));
        let p = a.p + sra_tz(wrap18(b.p - a.p) * wb, 12);
        let yw = a.w + sra_tz(wrap18(b.w - a.w) * wb, 12);
        let (sp, cp) = (d_.sin6(p), d_.cos6(p));
        let i = ((sra_tz(yw, 6) + 0x400) & 0xFFF) as usize;
        let e = [t[0] - sra_tz(sra_tz(d * d_.sin[i + 1024], 12) * cp, 12), t[1] - sra_tz(d * sp, 12), t[2] - sra_tz(sra_tz(d * d_.sin[i], 12) * cp, 12)];
        let h = sra_tz(a.h * wa + b.h * wb, 12);
        self.lookat(e, t, if h > 0 { h } else { self.keys[3].h })
    }

    /// 0x80063EEC: key from eye and target.
    fn lookat(&self, e: [i64; 3], t: [i64; 3], h: i64) -> Key {
        let (dx, dy, dz) = (t[0] - e[0], t[1] - e[1], t[2] - e[2]);
        let hd = isqrt(dx * dx + dz * dz);
        Key { x: e[0], y: e[1], z: e[2], p: s16(self.data.ratan2(hd, dy)) << 6, w: s16(self.data.ratan2(dz, -dx)) << 6, h }
    }

    /// 0x80064080: apply a key -> RVIEW + H.
    fn apply(&mut self, k: Key) {
        self.keys[3] = k;
        self.h = k.h.clamp(self.hmin, self.hmax);
        // the view's H is on Tekken 3's 368-pixel-wide screen, the one the renderer draws (Tekken Tag's is 512)
        let h = tdiv(self.h * 368, self.data.screen_width);
        self.view = View { eye: [k.x, k.y, k.z], pitch: sra_tz(k.p, 6), yaw: sra_tz(k.w, 6), h, roll: 0 };
    }

    /// 0x80068394 / 0x800684B4: intro fly-in key (slot 2); returns its weight.
    fn intro_key(&mut self) -> i64 {
        let base = self.intro_base;
        let t = self.intro_t.unwrap_or(0);
        // Tekken Tag eases over the first 90 frames only (0x3954E8: max(t - T + 90, 1)): round 1 then holds the base
        let (k, ease) = match self.data.intro_ease {
            Some(e) => ((t - self.intro_len + e).max(1), e),
            None => (t, self.intro_len),
        };
        let f = self.data.sin[((0xC00 + tdiv(k << 10, ease)) & 0xFFF) as usize] + 0x1000;
        let o = self.data.intro_offsets[self.intro_var];
        let eye = [base.x + sra_tz(o[0] * f, 12), base.y + sra_tz(o[1] * f, 12), base.z + sra_tz(o[2] * f, 12)];
        self.keys[2] = self.lookat(eye, [base.x, base.y, 0], base.h);
        let t = t - 1;
        self.intro_t = Some(t);
        // the hand-off: 16 frames (Tekken 3), 13 (Tekken Tag)
        tdiv(t << 12, self.data.intro_blend).min(0x1000)
    }

    /// 0x80062B74 slot mixer (the versus director only uses slots 0 and 2).
    fn mix(&mut self) {
        let m: u32 = (0..5).filter(|&i| self.w[i] > 0).map(|i| 1 << i).sum();
        let k = self.keys;
        let key = match m {
            1 => Some(k[0]),
            2 => Some(k[1]),
            4 => Some(k[2]),
            3 => Some(self.blend(&k[0], &k[1], self.w[0])),
            5 => Some(self.blend(&k[0], &k[2], self.w[0])),
            0x10 => Some(k[4]),
            0x11 => {
                let ww = self.data.sin[((half(self.w[0]) - 0x400) & 0xFFF) as usize];
                Some(self.blend(&k[0], &k[4], half(ww) + 0x800))
            }
            _ => None,
        };
        if let Some(key) = key {
            self.apply(key);
        }
        self.wprev = self.w;
    }
}

/// The 16-bit unsigned view of a value (the game's `lhu`).
fn u16v(v: i64) -> i64 {
    v & 0xFFFF
}

// ---------------------------------------------------------------- round-end cameras

/// What an end camera looks at (game space): the root bone, the chest height, the facing as the angle of the
/// fighter's front in the XZ plane (4096 units), and whether it is the large character (moveset key 11).
#[derive(Clone, Copy, Debug)]
pub struct Subject {
    pub root: [i64; 3],
    pub chest: i64,
    pub front: i64,
    pub large: bool,
}

/// An end camera's view in `<< 6` units (RVIEW blocks 0x8009E6A0 / 0x8009E6C8).
#[derive(Clone, Copy, Debug, Default)]
pub struct Block {
    eye: [i64; 3],
    p: i64,
    y: i64,
}

/// The cameras after the replay: winner camera A or B (phases 3/4), the loser's overhead shot or the orbit
/// (phases 6/7). Every switch is a cut; H stays 500.
#[derive(Clone, Debug)]
pub enum EndCam {
    WinnerA { block: Block },
    WinnerB { block: Block, drift: [i64; 2] },
    Overhead { yaw: i64 },
    Orbit { eye: [i64; 3], target: [i64; 3] },
}

impl CameraData {
    /// 0x80045FEC: pitch and yaw (`<< 6`, pitch signed) looking from `e` to `t`.
    fn lookat6(&self, e: [i64; 3], t: [i64; 3]) -> (i64, i64) {
        let (dx, dy, dz) = (t[0] - e[0], t[1] - e[1], t[2] - e[2]);
        let hd = isqrt(dx * dx + dz * dz);
        (s12(self.ratan2(hd, dy)) << 6, (self.ratan2(dz, -dx) & 0xFFF) << 6)
    }

    /// 0x80046270: aim at the fighter with the pitch clamps and the 5800-unit minimum horizontal distance.
    fn aim(&self, p: &Subject, eye: [i64; 3]) -> Block {
        let a = &self.end.aim;
        let [rx, _, rz] = p.root;
        let (pt, y) = self.lookat6(eye, p.root);
        let pc = self.lookat6(eye, [rx, p.chest + a.chest_offset, rz]).0;
        let pf = self.lookat6(eye, [rx, 0, rz]).0;
        let pitch = pt.min(pc + a.chest_clamp).min(pf + a.floor_clamp);
        let mut out = Block { eye, p: pitch, y };
        let (dx, dz) = (eye[0] - rx, eye[2] - rz);
        if dx * dx + dz * dz < a.min_distance * a.min_distance {
            let i = sra_tz(y, 6);
            out.eye[0] = rx + sra_tz(self.sin(i) * a.min_distance, 12);
            out.eye[2] = rz - sra_tz(self.cos(i) * a.min_distance, 12);
        }
        out
    }

    fn view6(&self, b: &Block) -> View {
        View { eye: b.eye, pitch: sra_tz(b.p, 6), yaw: sra_tz(b.y, 6), h: self.h, roll: 0 }
    }
}

/// A signed 12-bit angle (wrapped to -2048..2047).
fn s12(v: i64) -> i64 {
    ((v + 2048) & 0xFFF) - 2048
}

impl EndCam {
    /// Winner camera A (0x80046460) or B (0x800467A0) on the round winner. `random(n)`: 0..n.
    pub fn winner(d: &CameraData, p: &Subject, b_variant: bool, random: &mut dyn FnMut(i64) -> i64) -> Self {
        let [rx, _, rz] = p.root;
        if !b_variant {
            let c = &d.end.cam_a;
            let a = (c.arc_from + random(c.arc_range) + p.front - 0x400) & 0xFFF;
            let eye = [rx + sra_tz(d.cos(a) * c.radius, 12), c.eye_y, rz + sra_tz(d.sin(a) * c.radius, 12)];
            return EndCam::WinnerA { block: d.aim(p, eye) };
        }
        let c = &d.end.cam_b;
        let pick = |t: &[i64; 3], r: &mut dyn FnMut(i64) -> i64| t[r(3) as usize];
        let (rr, ss, hh) = (pick(&c.radius, random), pick(&c.side, random), pick(&c.height, random));
        let (cs, sn) = (d.cos(p.front), d.sin(p.front));
        let drift = [sra_tz(-cs, c.drift_shift), sra_tz(-sn, c.drift_shift)];
        let sigma = if random(2) == 1 { 1 } else { -1 };
        let eye = [rx + sra_tz(rr * cs + sigma * ss * sn, 12), hh, rz + sra_tz(rr * sn - sigma * ss * cs, 12)];
        EndCam::WinnerB { block: d.aim(p, eye), drift }
    }

    /// The loser camera (0x8006875C): the overhead shot when it was knocked out, else the orbit.
    pub fn loser(d: &CameraData, p: &Subject, knocked_out: bool, random: &mut dyn FnMut(i64) -> i64) -> Self {
        if knocked_out {
            let o = &d.end.overhead;
            let jitter = o.jitter[0] + random(o.jitter[1] - o.jitter[0] + 1);
            return EndCam::Overhead { yaw: o.yaw_from + p.front - 0x400 + jitter };
        }
        let o = &d.end.orbit;
        let [rx, _, rz] = p.root;
        let a = (o.arc_from + random(o.arc_range) + p.front - 0x400) & 0xFFF;
        let r = o.radius[usize::from(p.large)];
        EndCam::Orbit { eye: [rx + sra_tz(d.cos(a) * r, 12), o.eye_y, rz + sra_tz(d.sin(a) * r, 12)], target: [rx, o.target_y, rz] }
    }

    /// One frame on the subject; returns the view.
    pub fn step(&mut self, d: &CameraData, p: &Subject) -> View {
        let [rx, ry, rz] = p.root;
        match self {
            EndCam::WinnerA { block } => {
                let c = &d.end.cam_a;
                let (dx, dy, dz) = (rx - block.eye[0], ry - block.eye[1], rz - block.eye[2]);
                let n = d.gsqrt(dx * dx + dy * dy + dz * dz);
                if n > c.stop_distance {
                    block.eye[0] += tdiv(c.approach * dx, n);
                    block.eye[1] += tdiv(c.approach * dy, n);
                    block.eye[2] += tdiv(c.approach * dz, n);
                }
                let t = d.aim(p, block.eye);
                block.p += sra_tz(wrap18(t.p - block.p), c.pitch_shift);
                block.y += sra_tz(wrap18(t.y - block.y), c.yaw_shift);
                d.view6(block)
            }
            EndCam::WinnerB { block, drift } => {
                let c = &d.end.cam_b;
                block.eye[0] += drift[0];
                block.eye[2] += drift[1];
                let (dx, dy, dz) = (rx - block.eye[0], ry - block.eye[1], rz - block.eye[2]);
                let k = c.push_distance - d.gsqrt(dx * dx + dy * dy + dz * dz);
                if k > 0 {
                    let h = d.gsqrt(dx * dx + dz * dz).max(1);
                    block.eye[0] -= sra_tz(tdiv(k * dx, h), c.push_shift);
                    block.eye[2] -= sra_tz(tdiv(k * dz, h), c.push_shift);
                }
                let t = d.aim(p, block.eye);
                block.p += sra_tz(wrap18(t.p - block.p), c.pitch_shift);
                block.y += sra_tz(wrap18(t.y - block.y), c.yaw_shift);
                if block.p > 0 {
                    let v = sra_tz(c.height_reach * d.sin(block.p >> 6), 12);
                    if v < block.eye[1] {
                        block.eye[1] += sra_tz(v - block.eye[1], c.height_shift);
                    }
                }
                d.view6(block)
            }
            EndCam::Overhead { yaw } => {
                let o = &d.end.overhead;
                View { eye: [rx, o.height, rz], pitch: o.pitch, yaw: *yaw & 0xFFF, h: d.h, roll: 0 }
            }
            EndCam::Orbit { eye, target } => {
                let o = &d.end.orbit;
                let (dx, dz) = (target[0] - eye[0], target[2] - eye[2]);
                let n = d.gsqrt(dx * dx + dz * dz);
                if n > o.stop_distance[usize::from(p.large)] {
                    eye[0] += tdiv(o.approach * dx, n);
                    eye[2] += tdiv(o.approach * dz, n);
                }
                let [m, sh] = o.target_easing;
                target[0] += sra_tz((rx - target[0]) * m, sh as u32);
                target[1] += sra_tz((p.chest - target[1]) * m, sh as u32);
                target[2] += sra_tz((rz - target[2]) * m, sh as u32);
                let (pt, y) = d.lookat6(*eye, *target);
                View { eye: *eye, pitch: sra_tz(pt, 6), yaw: sra_tz(y, 6), h: d.h, roll: 0 }
            }
        }
    }
}

// ---------------------------------------------------------------- Tekken Tag's scripted shots

/// A shot of Tekken Tag's: the team intro's (eye and target moving from one place to another, 0x3D2818) or the K.O.
/// replay's (the eye still, the target easing after a fighter, 0x3D0A38). Game space, y down.
#[derive(Clone, Debug)]
pub struct Shot {
    eye: [[i64; 3]; 2],
    target: [[i64; 3]; 2],
    /// The team intro's frames and the frame now; None: the replay's.
    frames: Option<(i64, i64)>,
}

impl CameraData {
    /// 0x39BA60 then 0x39B310: `v` turned by Ry * Rx * Rz of `a` (4096 a turn), each product's terms truncated.
    pub fn turn(&self, a: [i64; 3], v: [i64; 3]) -> [i64; 3] {
        let (sx, cx, sy, cy, sz, cz) = (self.sin(a[0]), self.cos(a[0]), self.sin(a[1]), self.cos(a[1]), self.sin(a[2]), self.cos(a[2]));
        let rx = [[0x1000, 0, 0], [0, cx, -sx], [0, sx, cx]];
        let ry = [[cy, 0, sy], [0, 0x1000, 0], [-sy, 0, cy]];
        let rz = [[cz, -sz, 0], [sz, cz, 0], [0, 0, 0x1000]];
        let mul = |a: [[i64; 3]; 3], b: [[i64; 3]; 3]| -> [[i64; 3]; 3] { std::array::from_fn(|i| std::array::from_fn(|j| s16((0..3).map(|k| tdiv(a[i][k] * b[k][j], 0x1000)).sum()))) };
        let m = mul(mul(ry, rx), rz);
        std::array::from_fn(|i| (0..3).map(|j| tdiv(m[i][j] * v[j], 0x1000)).sum::<i64>() as i32 as i64)
    }

    /// A view looking from `eye` at `target` (0x35B690) on this arena's H, the pitch at most `pitch_max`.
    fn look(&self, eye: [i64; 3], target: [i64; 3], pitch_max: i64) -> View {
        let (p, y) = self.lookat6(eye, target);
        View { eye, pitch: sra_tz(p, 6).min(pitch_max), yaw: sra_tz(y, 6), h: tdiv(self.h * 368, self.screen_width), roll: 0 }
    }
}

impl Shot {
    /// The team intro's shot `k` (0x3D2330) on `base` (a chest, or a pair's chests' midpoint); `pair`: a pair's (its
    /// target's from y and its eyes' z scaled). None without the data.
    pub fn team(d: &CameraData, base: [i64; 3], k: usize, pair: bool) -> Option<Self> {
        let t = d.team_intro.as_ref()?;
        let s = t.shots.get(k)?;
        let scale = |v: [i64; 3], axis: Option<usize>| -> [i64; 3] {
            std::array::from_fn(|i| {
                let v = tdiv(v[i] * d.h, 600);
                if pair && axis == Some(i) {
                    tdiv(v * t.pair_scale, 0x1000)
                } else {
                    v
                }
            })
        };
        // (kept in 16 bits, as the game stores them)
        let at = |turn: [i64; 3], v: [i64; 3]| -> [i64; 3] {
            let r = d.turn(turn, v);
            std::array::from_fn(|i| s16(base[i] + r[i]))
        };
        Some(Self {
            target: [at(s.turn_from, scale(s.target_from, Some(1))), at(s.turn_to, scale(s.target_to, None))],
            eye: [at(s.turn_from, scale(s.eye_from, Some(2))), at(s.turn_to, scale(s.eye_to, Some(2)))],
            frames: Some((t.frames.max(1), 0)),
        })
    }

    /// The replay's shot `k` (0x3D0748) on `target` (game space), its yaw turned by `facing` (the winner's, the
    /// loser's after a throw; 4096 a turn, the game's +0x3C / 16). `random(n)`: 0..n (the AI's, 0x3800D8). The eye at
    /// least `eye_y` up, the target at most `target_y`. None without the data or the shot.
    pub fn replay(d: &CameraData, k: usize, target: [i64; 3], facing: i64, random: &mut dyn FnMut(i64) -> i64) -> Option<Self> {
        let r = d.replay_shots.as_ref()?;
        let s = r.shots.get(k)?;
        let [pr, yr, rr] = s.turn_range;
        let pitch = s.turn[0] + pr - random(2 * pr);
        let yaw = s.turn[1] + facing + yr - random(2 * yr);
        let roll = s.turn[2] + rr - random(2 * rr);
        let dist = tdiv((s.distance + s.distance_range - random(2 * s.distance_range)) * d.h, r.distance_per_h);
        let v = d.turn([pitch, yaw, roll], [0, 0, dist]);
        let mut eye: [i64; 3] = std::array::from_fn(|i| target[i] + v[i]);
        eye[1] = eye[1].min(r.eye_y);
        let target = [target[0], target[1].max(r.target_y), target[2]];
        Some(Self { eye: [eye; 2], target: [target; 2], frames: None })
    }

    /// What the replay's shot `k` looks at and its target then follows: 0 the winner's root (+0x51C), 1 the loser's,
    /// 2 (looks at) the loser's point kept at the K.O. (+0x39C), 3 (follows) none.
    pub fn replay_roles(d: &CameraData, k: usize) -> (i64, i64) {
        d.replay_shots.as_ref().and_then(|r| r.shots.get(k)).map_or((1, 1), |s| (s.target, s.follow))
    }

    /// The replay's shot for pass `pass` of sequence `sequence` (0x520F88).
    pub fn replay_sequence(d: &CameraData, sequence: usize, pass: usize) -> Option<usize> {
        let s = d.replay_shots.as_ref()?.sequences.get(sequence)?.get(pass)?;
        usize::try_from(*s).ok()
    }

    /// One frame: the team intro's eased move (0x3D2818), or the replay's target easing 6/128 of the way to `follow`
    /// (0x3D0A38).
    pub fn step(&mut self, d: &CameraData, follow: Option<[i64; 3]>) -> View {
        if let Some((n, t)) = self.frames.as_mut() {
            *t = (*t + 1).min(*n);
            let w = d.sin(tdiv(*t << 10, *n));
            let lerp = |p: [[i64; 3]; 2]| -> [i64; 3] { std::array::from_fn(|i| p[0][i] + tdiv((p[1][i] - p[0][i]) * w, 0x1000)) };
            return d.look(lerp(self.eye), lerp(self.target), i64::MAX);
        }
        let (target_y, pitch_max, [m, sh]) = d.replay_shots.as_ref().map_or((i64::MIN, i64::MAX, [0, 1]), |r| (r.target_y, r.pitch_max, r.follow_easing));
        let t = &mut self.target[0];
        if let Some(p) = follow {
            for i in 0..3 {
                t[i] += tdiv((p[i] - t[i]) * m, sh);
            }
        }
        t[1] = t[1].max(target_y);
        d.look(self.eye[0], self.target[0], pitch_max)
    }
}
