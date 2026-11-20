//! What a Tekken Tag arena's code does with its objects that depends on the camera or on chance (stages.json
//! `objects`; the converter's EFFECTS.md 7): Nina's helicopter, which waits on its pad until it has been out of sight
//! long enough, takes off, flies off and lands again nearby (0x3D62A0); the 40 cars and people of her lanes under the
//! grating, kept within reach of the camera (0x3D5B58, 0x3D5E80); King's ten drifting groups of four clouds
//! (0x3D4830, 0x3D4B18); Law-A's lanterns swaying in the arena's wind (0x3D7AB0) and its 40 leaves, lying, blown up
//! by a fighter's landing and falling again, with their shadows (0x3D7378, 0x3D7110). The objects' places in their
//! parents (metres), brightness and shear, by node name (`object_K`), replace what the stage's animation says. Drawn
//! only: their own random numbers.

use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};

/// The length of (x, z).
fn flat_len(x: f32, z: f32) -> f32 {
    (x * x + z * z).sqrt()
}
use serde_json::Value;

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

/// The helicopter's flight (0x3D62A0): its place (mm), turn (x, y, z; drawn Rz Ry Rx), state (0 on its pad, 2 up,
/// 3 down, 1 landed) and its state's count and values.
struct Helicopter {
    name: String,
    c: Value,
    at: Vec3,
    turn: Vec3,
    state: u8,
    count: i32,
    settled: bool,
    base: f32,
    climb: f32,
    phase: f32,
    pitch: [f32; 2],
    heading: f32,
    dir: f32,
}

struct Car {
    name: String,
    lane: usize,
    z: f32,
}

/// A group of clouds: its route, place (mm), speed, and its members: the model (0-9), two angles, their rates and
/// the two reaches of its drift.
struct Cloud {
    route: [f32; 6],
    at: Vec3,
    speed: f32,
    members: Vec<(usize, [f32; 6])>,
}

/// A leaf (0x3D7378): its place (mm), turn (x, y, z; drawn Rx Ry Rz), state (0 lying, 1 blown up, 2 falling), its
/// count and blow's length and push (mm a frame, x and z).
struct Leaf {
    at: Vec3,
    turn: Vec3,
    state: u8,
    count: i32,
    length: i32,
    push: [f32; 2],
}

/// An arena's moving objects as they are now.
pub struct ArenaMotion {
    heli: Option<Helicopter>,
    /// The lanes' x and speed (mm, mm a frame), the cars, and the lanes' table.
    lanes: Vec<(f32, f32)>,
    cars: Vec<Car>,
    l: Value,
    clouds: Vec<Cloud>,
    c: Value,
    /// Law-A's lanterns (name, lattice size mm, heading), their wind; the leaves, their table and the light the
    /// shadows fall along (towards it).
    lanterns: Vec<(String, f32, f32, Vec3)>,
    /// Eddy's grass lattice: its size, gain and drop (mm).
    grass: Option<[f32; 3]>,
    /// The wind's four impulses (0x44B100, 0x30 bytes each: the fighters' landings, picture x, z m), their
    /// strength and frames; a slot free since the last was taken (gp-0x6960).
    impulses: [Option<(Vec3, f32, u32)>; 4],
    impulse_free: bool,
    /// Each fighter's height last frame and fastest fall a frame since its last landing (m; player +0x122, 0x371BDC).
    falls: Vec<(f32, f32)>,
    wind: Option<crate::weather::Wind>,
    gain: f32,
    leaves: Vec<Leaf>,
    lv: Value,
    light: Vec3,
    frame: u32,
    seed: u32,
}

impl ArenaMotion {
    /// The arena's entry of stages.json (its `objects`, its light 0), an object's place in the stage as it stands
    /// (m).
    pub fn new(stage: &Value, rest: impl Fn(&str) -> Option<Mat4>) -> Option<Self> {
        let v = &stage["objects"];
        let t = &stage["lights"]["toward"][0];
        let light = Vec3::new(f(&t[0]), f(&t[1]), f(&t[2]));
        let mut me = Self {
            heli: None,
            lanes: vec![],
            cars: vec![],
            l: v["lanes"].clone(),
            clouds: vec![],
            c: v["clouds"].clone(),
            lanterns: vec![],
            grass: None,
            impulses: [None; 4],
            impulse_free: true,
            falls: vec![],
            wind: None,
            gain: f(&v["lanterns"]["gain"]),
            leaves: vec![],
            lv: v["leaves"].clone(),
            light,
            frame: 0,
            seed: 0x1234,
        };
        // Law-A's lanterns, where they stand in the stage
        let la = &v["lanterns"];
        for o in la["objects"].as_array().into_iter().flatten() {
            let name = format!("object_{}", o[0].as_u64().unwrap_or(0));
            if let Some(m) = rest(&name) {
                me.lanterns.push((name, f(&o[1]).max(1.0), f(&o[2]), m.w_axis.truncate()));
            }
        }
        if !me.lanterns.is_empty() {
            me.wind = Some(crate::weather::Wind::new(&la["wind"]));
        }
        let gr = &v["grass"];
        if gr.is_object() {
            me.grass = Some([f(&gr["size"]).max(1.0), f(&gr["gain"]), f(&gr["drop"])]);
            me.wind = Some(crate::weather::Wind::new(&gr["wind"]));
        }
        // the leaves (0x3D6BC0): within reach of the centre at random, a random heading
        let lv = me.lv.clone();
        for _ in 0..lv["count"].as_u64().unwrap_or(0).min(256) {
            let heading = std::f32::consts::PI - me.rand01() * std::f32::consts::TAU;
            let r = me.rand01() * f(&lv["reach"]);
            let at = Quat::from_rotation_y(heading) * Vec3::new(r, 0.0, 0.0);
            me.leaves.push(Leaf { at, turn: Vec3::new(0.0, heading, 0.0), state: 0, count: 0, length: 0, push: [0.0; 2] });
        }
        if me.wind.is_none() && !me.leaves.is_empty() {
            me.wind = Some(crate::weather::Wind::new(&lv["wind"]));
        }
        let rest = |name: &str| {
            rest(name).map(|m| {
                let (_, q, at) = m.to_scale_rotation_translation();
                (at, q)
            })
        };
        let h = &v["helicopter"];
        if let Some(k) = h["object"].as_u64() {
            let name = format!("object_{k}");
            if let Some((at, q)) = rest(&name) {
                let (z, y, x) = q.to_euler(EulerRot::ZYX);
                // (0x3D6208's heading, pi + pi / 6 - rand x pi / 3: the converter's middle one)
                let heading = y;
                let at = at * 1000.0;
                me.heli = Some(Helicopter {
                    name,
                    c: h.clone(),
                    at,
                    turn: Vec3::new(x, heading, z),
                    state: 0,
                    count: 0,
                    settled: false,
                    base: at.y,
                    climb: 0.0,
                    phase: 0.0,
                    pitch: [x, 0.0],
                    heading,
                    dir: 1.0,
                });
            }
        }
        // the lanes (0x3D5B58): their x, a speed of 15 to 30 mm a frame either way; 40 cars on lanes at random, at most
        // 20 a lane, none within 4 m of another on its lane
        let l = me.l.clone();
        if let (Some(first), Some(count), Some(lanes)) = (l["first"].as_u64(), l["count"].as_u64(), l["lanes"].as_u64()) {
            for k in 0..lanes.min(64) {
                let x = k as f32 * f(&l["apart"]) + f(&l["x"]);
                let speed = (me.rand01() + 1.0) * f(&l["speed"]);
                let speed = if (me.rand() % 255) & 1 == 1 { -speed } else { speed };
                me.lanes.push((x, speed));
            }
            let mut tries = 0;
            while (me.cars.len() as u64) < count.min(256) && tries < 10_000 && !me.lanes.is_empty() {
                tries += 1;
                let lane = me.rand() as usize % me.lanes.len();
                let on: Vec<f32> = me.cars.iter().filter(|c| c.lane == lane).map(|c| c.z).collect();
                if on.len() as u64 >= l["most"].as_u64().unwrap_or(20) {
                    continue;
                }
                let z = f(&l["place"][0]) - me.rand01() * f(&l["place"][1]);
                if on.iter().any(|o| (o - z).abs() < f(&l["gap"])) {
                    continue;
                }
                me.cars.push(Car { name: format!("object_{}", first + me.cars.len() as u64), lane, z });
            }
        }
        // King's clouds (0x3D4830): a route of the two at random, x and y along it, z anywhere on it, 20-40 mm a frame;
        // four of the ten models, each drifting on two angles
        let c = me.c.clone();
        let routes: Vec<[f32; 6]> = (c["routes"].as_array().into_iter().flatten()).map(|r| [0, 1, 2, 3, 4, 5].map(|k| f(&r[k]))).collect();
        let models = c["models"].as_u64().unwrap_or(0).min(64) as usize;
        for _ in 0..c["groups"].as_u64().unwrap_or(0).min(64) {
            if routes.is_empty() || models == 0 {
                break;
            }
            let route = routes[me.rand() as usize % routes.len()];
            let between = |me: &mut Self, a: f32, b: f32| a + me.rand01() * (b - a);
            let at = Vec3::new(between(&mut me, route[0], route[3]), between(&mut me, route[1], route[4]), between(&mut me, route[2], route[5]));
            let speed = between(&mut me, f(&c["speed"][0]), f(&c["speed"][1]));
            let mut used = vec![];
            let mut members = vec![];
            while members.len() < (c["members"].as_u64().unwrap_or(0) as usize).min(models) {
                let m = me.rand() as usize % models;
                if used.contains(&m) {
                    continue;
                }
                used.push(m);
                let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                let angle = [wrap(me.rand01() * f(&c["angle"])), wrap(me.rand01() * f(&c["angle"]))];
                let reach = [me.rand01() * f(&c["radius"]), me.rand01() * f(&c["radius"])];
                let rate = [me.rand01() * f(&c["rate"][0]) + f(&c["rate"][1]), me.rand01() * f(&c["rate"][0]) + f(&c["rate"][1])];
                members.push((m, [angle[0], angle[1], rate[0], rate[1], reach[0], reach[1]]));
            }
            me.clouds.push(Cloud { route, at, speed, members });
        }
        (me.heli.is_some() || !me.cars.is_empty() || !me.clouds.is_empty() || !me.lanterns.is_empty() || !me.leaves.is_empty() || me.grass.is_some()).then_some(me)
    }

    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        self.seed & 0x7FFF_FFFF
    }

    fn rand01(&mut self) -> f32 {
        (self.rand() & 0xFF_FFFF) as f32 / 16_777_216.0
    }

    /// A frame on, the camera at `eye` (m) seeing through `view_proj`, the fighters at `fighters` and the
    /// landings of the last frame at `landings` (m).
    pub fn step(&mut self, eye: Vec3, view_proj: Mat4, fighters: &[Vec3], landings: &[Vec3]) {
        if let Some(w) = self.wind.as_mut() {
            w.step();
        }
        // the impulses a frame older, gone after 300 (0x3D42A8); with the grass, a landing takes a free one
        // (0x3D3C68 -> 0x3D3B80; the arenas of 0x447EC0's third word: Law-A's take their own below)
        for i in &mut self.impulses {
            if let Some((_, _, f)) = i.as_mut() {
                *f += 1;
                if *f > 300 {
                    (*i, self.impulse_free) = (None, true);
                }
            }
        }
        self.falls.resize(fighters.len(), (0.0, 0.0));
        for (f, p) in self.falls.iter_mut().zip(fighters) {
            *f = (p.y, f.1.max(f.0 - p.y));
        }
        for p in landings.iter().filter(|_| self.grass.is_some()) {
            // the landing fighter's fall: 30 x min(1, its fastest / 300 mm)^2, then none again
            let k = (0..fighters.len()).min_by(|&a, &b| flat_len(fighters[a].x - p.x, fighters[a].z - p.z).total_cmp(&flat_len(fighters[b].x - p.x, fighters[b].z - p.z)));
            let fall = k.and_then(|k| self.falls.get_mut(k)).map_or(0.0, |f| std::mem::take(&mut f.1));
            if !self.impulse_free {
                continue;
            }
            match self.impulses.iter_mut().find(|i| i.is_none()) {
                Some(i) => *i = Some((*p, 30.0 * (fall * 1000.0 / 300.0).min(1.0).powi(2), 0)),
                None => self.impulse_free = false,
            }
        }
        self.leaves_step(fighters, landings);
        let r = [self.rand01(), self.rand01(), self.rand01(), self.rand01()];
        let coin = self.rand() & 1 == 1;
        if let Some(h) = self.heli.as_mut() {
            h.step(view_proj, r, coin);
        }
        // the lanes kept within reach of the camera (0x3D5E80), the cars along them
        let (lo, hi, period) = (f(&self.l["span"][0]), f(&self.l["span"][1]), f(&self.l["period"]));
        let wrap = |v: f32, at: f32| {
            if v - at > hi {
                v - period
            } else if v - at < lo {
                v + period
            } else {
                v
            }
        };
        let (ex, ez) = (eye.x * 1000.0, eye.z * 1000.0);
        for lane in &mut self.lanes {
            lane.0 = wrap(lane.0, ex);
        }
        for c in &mut self.cars {
            let speed = self.lanes.get(c.lane).map_or(0.0, |l| l.1);
            c.z = wrap(c.z + speed, ez);
        }
        // the clouds along their routes, back at a route's start past its end (0x3D4B18); their members drift
        self.frame = self.frame.wrapping_add(1);
        for k in 0..self.clouds.len() {
            let mut g = std::mem::replace(&mut self.clouds[k], Cloud { route: [0.0; 6], at: Vec3::ZERO, speed: 0.0, members: vec![] });
            g.at.z += g.speed;
            if g.at.z > g.route[5] {
                g.at.y = g.route[1] + self.rand01() * (g.route[4] - g.route[1]);
                g.at.x = g.route[0] + self.rand01() * (g.route[3] - g.route[0]);
                g.at.z = g.route[2];
            }
            for (_, m) in &mut g.members {
                m[0] += self.rand01() * m[2];
                m[1] += self.rand01() * m[3];
            }
            self.clouds[k] = g;
        }
    }

    /// Law-A's leaves a frame on (0x3D7378).
    fn leaves_step(&mut self, fighters: &[Vec3], landings: &[Vec3]) {
        let lv = self.lv.clone();
        let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let centre = if fighters.is_empty() { Vec3::ZERO } else { fighters.iter().sum::<Vec3>() / fighters.len() as f32 * 1000.0 };
        for k in 0..self.leaves.len() {
            let mut l = std::mem::replace(&mut self.leaves[k], Leaf { at: Vec3::ZERO, turn: Vec3::ZERO, state: 0, count: 0, length: 0, push: [0.0; 2] });
            let wind = self.wind.as_ref().map_or(Vec3::ZERO, |w| w.at(l.at / 1000.0)) * 1000.0;
            // a landing within 5 m and the leaf under 800 mm: blown up, harder nearer and lower
            // shortcut: every landing at full strength (30: the PS2's is the landing's speed, player +0x122, not read)
            for p in landings {
                let c = *p * 1000.0;
                let d = flat_len(l.at.x - c.x, l.at.z - c.z);
                let (near, low, most) = (f(&lv["lift"][2]), f(&lv["lift"][1]), f(&lv["lift"][0]));
                if d < near && l.at.y < low {
                    let strength = 30.0;
                    let k2 = (((near - d) / near) * ((low - l.at.y) / low)).powi(2).min(most);
                    let away = Vec3::new(l.at.x - c.x, 0.0, l.at.z - c.z).normalize_or_zero() * (strength * 100.0 / 30.0 * k2);
                    (l.state, l.count, l.length, l.push) = (1, 0, (strength / 30.0 * 60.0 * k2) as i32, [away.x, away.z]);
                }
            }
            match l.state {
                0 => {
                    // lying: turned back flat (or face down) a little a frame, settling to the ground
                    let rock = f(&lv["rock"]);
                    let flip = |a: f32, lo: f32, hi: f32| if (a < 0.0 && a < lo) || (a >= 0.0 && a <= hi) { -rock } else { rock };
                    l.turn.x = wrap(l.turn.x + flip(l.turn.x, f(&lv["flat"][0]), f(&lv["flat"][1])));
                    l.turn.z = wrap(l.turn.z + flip(l.turn.z, f(&lv["flat"][0]), f(&lv["flat"][1])));
                    if l.at.y > 0.0 {
                        l.at.y -= f(&lv["settle"]);
                    } else if l.at.y < 0.0 {
                        l.at.y = 0.0;
                    }
                }
                1 if l.count + 1 < l.length => {
                    l.count += 1;
                    let k = (l.length - l.count) as f32 / l.length.max(1) as f32;
                    l.at += Vec3::new(l.push[0] * k, 50.0 * k, l.push[1] * k) + wind * f(&lv["blow"]);
                    let r = [self.rand01(), self.rand01(), self.rand01()];
                    for (a, (s, r)) in [&mut l.turn.x, &mut l.turn.y, &mut l.turn.z].into_iter().zip(lv["spin"].as_array().into_iter().flatten().zip(r)) {
                        *a = wrap(*a + r * f(s));
                    }
                }
                _ => {
                    // falling, the flatter the slower, drifting along its heading
                    l.state = 2;
                    let r = [self.rand01(), self.rand01(), self.rand01()];
                    let t = f(&lv["tumble"]);
                    (l.turn.x, l.turn.y, l.turn.z) = (wrap(l.turn.x + r[0] * t), wrap(l.turn.y + r[1] * t), wrap(l.turn.z + r[2] * t));
                    let tilt = |a: f32| {
                        let a = a.abs();
                        let a = if a > f(&lv["tilt"][0]) { f(&lv["tilt"][1]) - a } else { a };
                        (a / f(&lv["tilt"][2])).powi(2)
                    };
                    let (mut fx, mut fz) = (tilt(l.turn.x), tilt(l.turn.z));
                    let mut drop = fx * fz;
                    if drop < f(&lv["drop"][1]) {
                        drop = f(&lv["drop"][1]);
                    } else if drop > f(&lv["drop"][2]) {
                        drop = f(&lv["drop"][2]);
                        fx += f(&lv["drop"][3]);
                        fz += f(&lv["drop"][3]);
                    }
                    let v = Quat::from_rotation_y(l.turn.y) * Vec3::new(f(&lv["drift"]) * fx, -f(&lv["drop"][0]) * drop, f(&lv["drift"]) * fz);
                    l.at += v + wind * f(&lv["blow"]);
                    if l.at.y < f(&lv["land"]) {
                        l.state = 0;
                    }
                }
            }
            // gone too far from the fighters: put back on the edge of reach
            if flat_len(l.at.x - centre.x, l.at.z - centre.z) > f(&lv["reach"]) {
                let a = std::f32::consts::PI - self.rand01() * std::f32::consts::TAU;
                l.at = Quat::from_rotation_y(a) * Vec3::new(f(&lv["reach"]), 0.0, 0.0) + Vec3::new(centre.x, 0.0, centre.z);
                l.state = 0;
            }
            self.leaves[k] = l;
        }
    }

    /// Eddy's grass lattice (0x3D43A8): a top corner at each corner of the floor's window of `window` cells of `cell`
    /// around `centre` (m), moved by the wind there (sampled at x, y, -z as the lanterns') x the gain and lowered by
    /// that move's length and the drop: the first corner (x, z), the lattice's height and the corners' moves (m), row
    /// by row of `window[0] + 1` (17 x 17 at most). The PS2 starts its corners -(n + 1) / 2 cells from the window's
    /// centre (0x3D43A8), half a cell before the cells' own: each corner takes the wind half a cell back from it.
    /// The wind there (0x3D3E00) is the waves' and the impulses': within 1.5 m, away from the landing, its strength x
    /// 2^(-frames / 10) x (1 - the distance / 1500) (0x3D3FF8).
    pub fn grass(&self, centre: [f32; 2], cell: [f32; 2], window: [f32; 2]) -> Option<([f32; 2], f32, Vec<[f32; 4]>)> {
        let ([size, gain, drop], w) = (self.grass?, self.wind.as_ref()?);
        let [nx, nz] = window.map(|n| (n.max(1.0) as usize).min(16));
        let first = [centre[0] - cell[0] * nx as f32 / 2.0, centre[1] - cell[1] * nz as f32 / 2.0];
        let corners = (0..=nz).flat_map(|i| (0..=nx).map(move |j| (i, j))).map(|(i, j)| {
            let (x, z) = (first[0] + (j as f32 - 0.5) * cell[0], first[1] + (i as f32 + 0.5) * cell[1]);
            let mut p = w.at(Vec3::new(x, 0.0, -z));
            for &(at, strength, frames) in self.impulses.iter().flatten() {
                let d = Vec3::new(x - at.x, 0.0, at.z - z);
                let len = d.length() * 1000.0;
                if len < 1500.0 {
                    let d = if len == 0.0 { Vec3::new(0.1, 0.0, 0.1) } else { d };
                    p += d.normalize() * strength * (-0.1 * frames as f32).exp2() * (1.0 - len / 1500.0);
                }
            }
            let p = p * gain;
            [p.x, -(p.x.hypot(p.z) + drop), p.z, 0.0].map(|v| v / 1000.0)
        });
        Some((first, size / 1000.0, corners.collect()))
    }

    /// Law-A's lanterns' shear (0x3D7AB0): each one's lattice (x, z +-s, y 0..s) has its top moved by the wind at it
    /// (sampled at x, y, -z) times the gain, turned into its axes: a vertex moves that times its y / s.
    pub fn shears(&self) -> Vec<(String, Vec3)> {
        let Some(w) = self.wind.as_ref() else { return vec![] };
        let gain = self.gain;
        (self.lanterns.iter())
            .map(|(name, s, heading, at)| {
                let p = w.at(Vec3::new(at.x, at.y, -at.z)) * gain;
                let (sin, cos) = heading.sin_cos();
                (name.clone(), Vec3::new(cos * p.x - sin * p.z, 0.0, sin * p.x + cos * p.z) / (s / 1000.0))
            })
            .collect()
    }

    /// The moved objects' places in their parents (m) and brightness (1 as drawn), by node name; `turn` the
    /// camera's turn about y.
    pub fn places(&self, turn: f32) -> Vec<(String, Mat4, f32)> {
        let mut out = vec![];
        if let Some(h) = &self.heli {
            let q = Quat::from_euler(EulerRot::ZYX, h.turn.z, h.turn.y, h.turn.x);
            out.push((h.name.clone(), Mat4::from_rotation_translation(q, h.at / 1000.0), 1.0));
        }
        // King's clouds: a group turned slowly round from the camera's turn ((frame & 0xFFF) / 2048 x pi less it),
        // fading in and out within 10 m of either end of its route; the members it does not show hidden
        let first = self.c["first"].as_u64().unwrap_or(0) as usize;
        let models = self.c["models"].as_u64().unwrap_or(0) as usize;
        let spin = (self.frame & 0xFFF) as f32 / 2048.0 * f(&self.c["spin"]) - turn;
        let fade_at = f(&self.c["fade"]).max(1.0);
        for (k, g) in self.clouds.iter().enumerate() {
            let mut fade = 1.0;
            for end in [g.route[5], g.route[2]] {
                if (g.at.z - end).abs() < fade_at {
                    fade = (g.at.z - end).abs() / fade_at;
                }
            }
            let group = Mat4::from_rotation_translation(Quat::from_rotation_y(spin), g.at / 1000.0);
            for m in 0..models {
                let name = format!("object_{}", first + k * models + m);
                let place =
                    g.members.iter().find(|x| x.0 == m).map_or(Mat4::from_scale(Vec3::ZERO), |(_, a)| group * Mat4::from_translation(Vec3::new(a[4] * a[0].sin(), a[5] * a[1].sin(), 0.0) / 1000.0));
                out.push((name, place, fade));
            }
        }
        // Law-A's leaves (Rx Ry Rz) and their shadows, flattened onto the ground along light 0 (0x3D7110)
        let first = self.lv["first"].as_u64().unwrap_or(0) as usize;
        let n = self.leaves.len();
        let s = if self.light.y.abs() > 1e-3 { Vec3::new(-self.light.x / self.light.y, 0.0, -self.light.z / self.light.y) } else { Vec3::ZERO };
        let flat = Mat4::from_cols(Vec4::X, Vec4::new(s.x, 0.0, s.z, 0.0), Vec4::Z, Vec4::new(0.0, 0.002, 0.0, 1.0));
        for (k, l) in self.leaves.iter().enumerate() {
            let m = Mat4::from_rotation_translation(Quat::from_euler(EulerRot::XYZ, l.turn.x, l.turn.y, l.turn.z), l.at / 1000.0);
            out.push((format!("object_{}", first + k), m, 1.0));
            out.push((format!("object_{}", first + n + k), flat * m, 1.0));
        }
        let y = f(&self.l["y"]);
        for c in &self.cars {
            if let Some(&(x, speed)) = self.lanes.get(c.lane) {
                // (0x3D5D94: a car of a lane going -z turned half round)
                let q = Quat::from_rotation_y(if speed < 0.0 { std::f32::consts::PI } else { 0.0 });
                out.push((c.name.clone(), Mat4::from_rotation_translation(q, Vec3::new(x, y, c.z) / 1000.0), 1.0));
            }
        }
        out
    }
}

impl Helicopter {
    fn step(&mut self, view_proj: Mat4, r: [f32; 4], coin: bool) {
        let c = &self.c;
        let pi = std::f32::consts::PI;
        // out of sight: its centre in front of the eye and off the screen (0x3D6348)
        let clip = view_proj * (self.at / 1000.0).extend(1.0);
        let unseen = clip.w > 0.0 && (clip.x.abs() > clip.w || clip.y.abs() > clip.w);
        let take_off = |h: &mut Self, pitch: f32, veer: [f32; 2]| {
            (h.state, h.count, h.climb, h.base) = (2, 0, 0.0, h.at.y);
            h.pitch = [h.turn.x, pitch];
            h.heading = h.turn.y + (veer[0] - r[0] * veer[1]);
            h.dir = if coin { 1.0 } else { -1.0 };
        };
        let rise = f(&c["rise"]).max(1.0);
        let ease = f(&c["ease"]).max(1.0);
        let wait = c["wait"].as_i64().unwrap_or(600) as i32;
        match self.state {
            0 => {
                if unseen {
                    self.count += 1;
                }
                self.phase += f(&c["bob"]);
                self.at.y = self.base + self.phase.sin() * 100.0;
                if self.count >= wait {
                    take_off(self, f(&c["pitch"][0]), [f(&c["veer"][0]), f(&c["veer"][1])]);
                }
            }
            2 => {
                self.count += 1;
                let t = self.count as f32 / rise;
                self.climb += t * 20.0;
                self.at.y = self.base + self.climb;
                self.turn.y += self.dir * f(&c["swing"]) * t;
                if (self.count as f32) < ease {
                    let k = ((self.count as f32 * pi / ease + pi).cos() + 1.0) * 0.5;
                    self.turn.x = self.pitch[0] + (self.pitch[1] - self.pitch[0]) * k;
                }
                if self.at.y > f(&c["top"]) {
                    // gone: back down elsewhere near the pad
                    (self.state, self.climb, self.base) = (3, 0.0, self.at.y);
                    self.pitch = [self.turn.x, 0.0];
                    self.turn.y = f(&c["back"][0]) - r[1] * f(&c["back"][1]) + pi;
                    self.at.x = f(&c["land"][0]) - r[2] * f(&c["land"][1]);
                    self.at.z = r[3] * f(&c["land"][3]) + f(&c["land"][2]);
                    self.dir = if coin { 1.0 } else { -1.0 };
                }
            }
            3 => {
                self.count -= 1;
                let t = self.count as f32 / rise;
                self.climb -= t * 20.0;
                self.at.y = self.base + self.climb;
                if (self.count as f32) < ease {
                    let k = ((self.count as f32 * pi / ease + pi).cos() + 1.0) * 0.5;
                    self.turn.x = self.pitch[0] + (self.pitch[1] - self.pitch[0]) * (1.0 - k);
                }
                self.turn.y += self.dir * f(&c["swing_down"]) * self.count as f32 / ease;
                if self.count < 0 {
                    (self.state, self.count, self.settled, self.base) = (1, 0, false, self.at.y);
                }
            }
            _ => {
                // landed: down onto the ground, a bounce, then out of sight long enough: up again
                if !self.settled {
                    self.at.y -= 2.0;
                    if self.at.y < 0.0 {
                        (self.at.y, self.settled) = (0.0, true);
                    }
                } else if self.count < f(&c["settle"][0]) as i32 {
                    self.count += 1;
                    let n = f(&c["settle"][0]).max(1.0);
                    let a = self.count as f32 * f(&c["settle"][2]) / 60.0 + f(&c["settle"][2]);
                    self.at.y = (self.count as f32 / n).powi(2) * f(&c["settle"][1]) * a.sin();
                } else {
                    if unseen {
                        self.count += 1;
                    }
                    if self.count >= wait {
                        take_off(self, f(&c["pitch"][1]), [f(&c["veer"][2]), f(&c["veer"][3])]);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_landing_pushes_the_grass_away_for_300_frames() {
        let stage = json!({"objects": {"grass": {"size": 1000.0, "gain": 120.0, "drop": 200.0, "wind": {"waves": []}}}});
        let mut m = ArenaMotion::new(&stage, |_| None).unwrap();
        let moves = |m: &ArenaMotion| m.grass([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).unwrap().2;
        assert!(moves(&m).iter().all(|c| c[0] == 0.0 && c[2] == 0.0));
        // a landing at the centre; the middle corner takes the wind half a cell back, at (-0.5, 0.5) m: pushed -x
        // and (the wind's z the picture's -z) -z
        // (a fighter falling 300 mm a frame onto it)
        m.step(Vec3::ZERO, Mat4::IDENTITY, &[Vec3::new(0.0, 0.3, 0.0)], &[]);
        m.step(Vec3::ZERO, Mat4::IDENTITY, &[Vec3::ZERO], &[Vec3::ZERO]);
        let c = moves(&m)[4];
        assert!(c[0] < 0.0 && c[2] < 0.0, "{c:?}");
        for _ in 0..300 {
            m.step(Vec3::ZERO, Mat4::IDENTITY, &[], &[]);
        }
        assert!(moves(&m)[4][0] < 0.0);
        m.step(Vec3::ZERO, Mat4::IDENTITY, &[], &[]);
        assert!(moves(&m).iter().all(|c| c[0] == 0.0 && c[2] == 0.0));
    }

    #[test]
    fn the_helicopter_waits_out_of_sight_then_flies_off_and_lands_again() {
        let v = json!({"helicopter": {"object": 1, "bob": 0.0316, "wait": 600, "pitch": [0.26, 0.26], "veer": [0.52, 1.05, 0.52, 1.05],
            "rise": 450.0, "ease": 330.0, "swing": 0.0035, "top": 50000.0, "back": [0.87, 1.75], "land": [2000.0, 4000.0, 14000.0, 5000.0],
            "swing_down": 0.0005, "settle": [120.0, 30.0, 3.0, 6.0]}});
        let mut m = ArenaMotion::new(&json!({"objects": v}), |_| Some(Mat4::from_translation(Vec3::new(0.0, 0.4, 14.0)))).unwrap();
        // in sight: it waits
        let seen = Mat4::IDENTITY * Mat4::from_scale(Vec3::splat(0.01));
        for _ in 0..700 {
            m.step(Vec3::ZERO, seen, &[], &[]);
        }
        assert_eq!(m.heli.as_ref().unwrap().state, 0);
        // out of sight: up after 600 frames, gone past 50 m, down and landed
        let away = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
        let mut states = vec![];
        for _ in 0..5000 {
            m.step(Vec3::ZERO, away, &[], &[]);
            let s = m.heli.as_ref().unwrap().state;
            if states.last() != Some(&s) {
                states.push(s);
            }
        }
        assert_eq!(states[..4], [0, 2, 3, 1]);
    }
}
