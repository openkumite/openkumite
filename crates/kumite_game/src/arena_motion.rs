//! What a Tekken Tag arena's code does with its objects that depends on the camera or on chance (stages.json
//! `objects`; the converter's EFFECTS.md 7): Nina's helicopter, which waits on its pad until it has been out of sight
//! long enough, takes off, flies off and lands again nearby (0x3D62A0); the 40 cars and people of her lanes under the
//! grating, kept within reach of the camera (0x3D5B58, 0x3D5E80); King's ten drifting groups of four clouds
//! (0x3D4830, 0x3D4B18). The objects' places in their parents (metres) and brightness, by node name (`object_K`),
//! replace what the stage's animation says. Drawn only: their own random numbers.

use glam::{EulerRot, Mat4, Quat, Vec3};
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

/// An arena's moving objects as they are now.
pub struct ArenaMotion {
    heli: Option<Helicopter>,
    /// The lanes' x and speed (mm, mm a frame), the cars, and the lanes' table.
    lanes: Vec<(f32, f32)>,
    cars: Vec<Car>,
    l: Value,
    clouds: Vec<Cloud>,
    c: Value,
    frame: u32,
    seed: u32,
}

impl ArenaMotion {
    /// The arena's `objects` (stages.json), the helicopter's place and turn in its parent as the stage stands (m).
    pub fn new(v: &Value, rest: impl Fn(&str) -> Option<(Vec3, Quat)>) -> Option<Self> {
        let mut me = Self { heli: None, lanes: vec![], cars: vec![], l: v["lanes"].clone(), clouds: vec![], c: v["clouds"].clone(), frame: 0, seed: 0x1234 };
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
        (me.heli.is_some() || !me.cars.is_empty() || !me.clouds.is_empty()).then_some(me)
    }

    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        self.seed & 0x7FFF_FFFF
    }

    fn rand01(&mut self) -> f32 {
        (self.rand() & 0xFF_FFFF) as f32 / 16_777_216.0
    }

    /// A frame on, the camera at `eye` (m) seeing through `view_proj`.
    pub fn step(&mut self, eye: Vec3, view_proj: Mat4) {
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
    fn the_helicopter_waits_out_of_sight_then_flies_off_and_lands_again() {
        let v = json!({"helicopter": {"object": 1, "bob": 0.0316, "wait": 600, "pitch": [0.26, 0.26], "veer": [0.52, 1.05, 0.52, 1.05],
            "rise": 450.0, "ease": 330.0, "swing": 0.0035, "top": 50000.0, "back": [0.87, 1.75], "land": [2000.0, 4000.0, 14000.0, 5000.0],
            "swing_down": 0.0005, "settle": [120.0, 30.0, 3.0, 6.0]}});
        let mut m = ArenaMotion::new(&v, |_| Some((Vec3::new(0.0, 0.4, 14.0), Quat::IDENTITY))).unwrap();
        // in sight: it waits
        let seen = Mat4::IDENTITY * Mat4::from_scale(Vec3::splat(0.01));
        for _ in 0..700 {
            m.step(Vec3::ZERO, seen);
        }
        assert_eq!(m.heli.as_ref().unwrap().state, 0);
        // out of sight: up after 600 frames, gone past 50 m, down and landed
        let away = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
        let mut states = vec![];
        for _ in 0..5000 {
            m.step(Vec3::ZERO, away);
            let s = m.heli.as_ref().unwrap().state;
            if states.last() != Some(&s) {
                states.push(s);
            }
        }
        assert_eq!(states[..4], [0, 2, 3, 1]);
    }
}
