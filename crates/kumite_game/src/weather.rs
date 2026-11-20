//! Tekken Tag's arenas' own effects (stages.json `weather`, the tasks 0x3A2640 starts by arena; the converter's
//! EFFECTS.md 5): the smoke and steam streams of particles on their paths (Hwoarang's and Law-B's blown by the
//! arena's wind), the Unknown's circling wisps and Nina's and King's blinking beacons; arena 4's lightning and arena
//! 12's signs, the still texture channels their tasks play ([`Weather::flash`]); Yoshimitsu's snow. Drawn only: none
//! of it touches the fight, and it draws its own random numbers (the PS2's rand(), 0x40FBD0).

use crate::vfx::{Blend, Sprite};
use glam::Vec3;
use serde_json::Value;

/// The frames a wind wave keeps (0x3D4178's ring).
const KEPT: usize = 1024;

struct Set {
    cell: u32,
    paths: Vec<Vec<[f32; 5]>>,
}

struct Stream {
    set: usize,
    every: u32,
    at: Vec3,
    /// sin, cos of its heading about y.
    turn: [f32; 2],
    scale: Vec3,
    colour: [f32; 3],
    level: f32,
    size: f32,
    add: bool,
    wind: f32,
    most: usize,
    prewarm: bool,
}

/// A particle of a stream: its path, the frames it has been drawn, and what the wind has pushed it.
struct Particle {
    stream: usize,
    path: usize,
    age: usize,
    drift: Vec3,
}

/// A wind wave (0x3D3988): its Lorenz series of the last 1024 frames, sampled by where a point lies along it.
struct Wave {
    toward: [f32; 2],
    /// The point its front line passes through, the line's slope (x to z), 1 / its speed, its strength.
    from: [f32; 2],
    slope: f32,
    slow: f32,
    strength: f32,
    step: i32,
    lorenz: [f32; 3],
    last: [f32; 2],
    kept: Vec<f32>,
    at: usize,
    frames: u32,
}

impl Wave {
    /// One step of the Lorenz series (0x3D42F8, Euler), as 0..1.
    fn next(&mut self, dt: f32, beta: f32) -> f32 {
        let [x, y, z] = self.lorenz;
        self.lorenz = [x + (y - x) * 10.0 * dt, y + (x * (28.0 - z) - y) * dt, z + (x * y - z * beta) * dt];
        (self.lorenz[0] + 40.0) / 80.0
    }

    /// A frame on (0x3D4178): the next value kept.
    fn advance(&mut self, dt: f32, beta: f32) {
        self.frames += 1;
        let v = if self.step >= 0 {
            (0..self.step.max(1)).fold(0.0, |_, _| self.next(dt, beta))
        } else {
            let n = self.step.unsigned_abs();
            let r = self.frames % n;
            if r == 0 {
                self.last = [self.next(dt, beta), self.last[0]];
                self.last[0]
            } else {
                self.last[0] + (self.last[0] - self.last[1]) * (r as f32 / n as f32)
            }
        };
        if let Some(k) = self.kept.get_mut(self.at) {
            *k = v;
        }
        self.at = (self.at + 1) % KEPT;
    }

    /// Its push at `p` (0x3D3F48): the value kept `d` / speed frames on from the oldest, d the distance along it from
    /// its front line.
    fn at(&self, p: Vec3) -> Vec3 {
        let [fx, fz] = self.from;
        let d = (self.slope * fx - fz - self.slope * p.x + p.z).abs() / (self.slope * self.slope + 1.0).sqrt() * self.slow;
        let back = if d.is_finite() { d as i64 } else { 0 };
        let v = self.kept.get((self.at as i64 + back).rem_euclid(KEPT as i64) as usize).copied().unwrap_or(0.0);
        Vec3::new(self.toward[0], 0.0, self.toward[1]) * (self.strength * v)
    }
}

/// An arena's wind (0x3D3630): its Lorenz step and beta, and its waves.
pub struct Wind {
    dt: f32,
    beta: f32,
    waves: Vec<Wave>,
}

impl Wind {
    /// stages.json's `wind` (`dt`, `beta`, `reach`, `waves`), made with its 1024 frames run.
    pub fn new(w: &Value) -> Self {
        let reach = f(&w["reach"]);
        let waves = (w["waves"].as_array().into_iter().flatten())
            .map(|wave| {
                // (0x3D3988: the front line through a point `reach` out, across the wave)
                let [x, z] = [f(&wave["toward"][0]), f(&wave["toward"][1])];
                let fx = (reach * reach / (x * x / (z * z) + 1.0)).sqrt();
                let fz = z / x * fx;
                let speed = f(&wave["speed"]);
                Wave {
                    toward: [x, z],
                    from: [if fx.is_finite() { fx } else { 0.0 }, if fz.is_finite() { fz } else { 0.0 }],
                    slope: -1.0 / (z / x),
                    slow: 1.0 / speed,
                    strength: f(&wave["strength"]),
                    step: wave["step"].as_i64().unwrap_or(1).clamp(-1000, 1000) as i32,
                    lorenz: [1.0; 3],
                    last: [1.0; 2],
                    kept: vec![0.0; KEPT],
                    at: 0,
                    frames: 0,
                }
            })
            .collect();
        let mut me = Self { dt: f(&w["dt"]), beta: f(&w["beta"]), waves };
        for _ in 0..KEPT {
            me.step();
        }
        me
    }

    /// A frame on (0x3D37E0).
    pub fn step(&mut self) {
        let (dt, beta) = (self.dt, self.beta);
        for w in &mut self.waves {
            w.advance(dt, beta);
        }
    }

    /// Its push at `p` (m; 0x3D3D08: its waves summed).
    pub fn at(&self, p: Vec3) -> Vec3 {
        self.waves.iter().map(|w| w.at(p)).sum()
    }
}

struct Wisp {
    group: usize,
    angle: f32,
    turn: f32,
    radius: f32,
    height: f32,
    phase: f32,
    fade: f32,
    fade_rate: f32,
}

struct WispGroup {
    set: usize,
    swing: f32,
    bob: f32,
    size: [f32; 2],
    colour: [f32; 3],
    level: f32,
}

struct Beacon {
    set: usize,
    at: Vec3,
    size: f32,
    level: f32,
    phase: f32,
    rate: f32,
}

/// Arena 4's lightning (task 0x3A76C0, stages.json `weather.lightning`) and arena 12's signs (tasks 0x3A7590,
/// `weather.signs`): the frame each still texture channel shows, the thunder, the fighters' lights turned.
#[derive(Default)]
struct Flashes {
    /// The frame lists (s16 a frame, to a negative) by EMBU op 7's table: 0 the long flicker, 1-3 the middle ones,
    /// 4-5 the short ones, 6-8 the bolts.
    lists: Vec<Vec<i16>>,
    /// The bolts' points (channels 0-3), the flickers' (channels 4-6) with the bolt each lights.
    bolts: Vec<Vec3>,
    flickers: Vec<(Vec3, usize)>,
    thunder: Vec<u16>,
    roll: u16,
    /// How near the screen's centre (pixels of 640 x 448) a bolt and a flicker are heard and light the fighters.
    near: [f32; 2],
    /// The random task runs (0x3A72A8: not on the EMBU's screen).
    task: bool,
    started: bool,
    /// Per bolt (0x8D5DE0): its list and the place in it while it flashes.
    bolt: Vec<Option<(usize, usize)>>,
    /// Per flicker (0x8D5DB0): its count and, while it flickers, its list and place.
    flicker: Vec<(u32, Option<(usize, usize)>)>,
    /// EMBU op 7's (0x3A7EB0): channel, list, place.
    ops: Vec<(usize, usize, usize)>,
    /// The signs (0x3A7590): channel, its frames, state, count (256ths of a frame).
    signs: Vec<(usize, usize, u8, u32)>,
    /// Each channel's frame (None: as the stage has it).
    frames: [Option<usize>; 8],
    /// A bolt lights the fighters (0x353C78: their lights 0 and 1 with x and z negated).
    lit: bool,
}

/// A layer of snow (0x3E6B10 makes its flakes as the arena loads; VU1 program 0x708 moves and draws them).
struct Snow {
    /// Each flake's start (m, about the box's corner), its move a frame (m) and its first frame.
    flakes: Vec<(Vec3, Vec3, u32)>,
    size: f32,
    /// The box (m) the flakes fall in, `ahead` of the eye, its copies `copies` aside in x and z.
    box_: f32,
    ahead: f32,
    copies: f32,
    /// The frames a flake falls (a power of 2).
    frames: u32,
}

/// An arena's weather as it is now.
pub struct Weather {
    sets: Vec<Set>,
    streams: Vec<Stream>,
    particles: Vec<Particle>,
    wind: Wind,
    groups: Vec<WispGroup>,
    wisps: Vec<Wisp>,
    beacons: Vec<Beacon>,
    snow: Vec<Snow>,
    /// The snow's set, its alpha (of 1) and how much of it a metre's depth takes.
    snow_look: (usize, f32, f32),
    /// The tasks' frame count (0: not started).
    count: u32,
    seed: u32,
    flashes: Flashes,
    /// Thunder started (sound entries), for the caller to play.
    pub sounds: Vec<u16>,
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn vec3(v: &Value) -> Vec3 {
    Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

impl Weather {
    /// The arena's `weather` (stages.json); None when it has none.
    pub fn new(v: &Value) -> Option<Self> {
        let list = |k: &str| v[k].as_array().into_iter().flatten();
        let sets = list("sets")
            .map(|s| Set {
                cell: s["cell"].as_u64().unwrap_or(0) as u32,
                paths: s["paths"].as_array().into_iter().flatten().map(|p| p.as_array().into_iter().flatten().map(|x| [0, 1, 2, 3, 4].map(|k| f(&x[k]))).collect()).collect(),
            })
            .collect();
        let streams = list("streams")
            .map(|s| Stream {
                set: s["set"].as_u64().unwrap_or(0) as usize,
                every: s["every"].as_u64().unwrap_or(0).clamp(1, 100_000) as u32,
                at: vec3(&s["at"]),
                turn: [f(&s["heading"]).sin(), f(&s["heading"]).cos()],
                scale: vec3(&s["scale"]),
                colour: [0, 1, 2].map(|k| f(&s["colour"][k])),
                level: f(&s["level"]),
                size: f(&s["size"]),
                add: s["add"].as_bool().unwrap_or(false),
                wind: f(&s["wind"]),
                most: s["most"].as_u64().map_or(usize::MAX, |m| m as usize),
                prewarm: s["prewarm"].as_bool().unwrap_or(false),
            })
            .collect();
        let wind = Wind::new(&v["wind"]);
        let groups: Vec<WispGroup> = list("wisps")
            .map(|g| WispGroup {
                set: g["set"].as_u64().unwrap_or(0) as usize,
                swing: f(&g["swing"]),
                bob: f(&g["bob"]),
                size: [f(&g["size"][0]), f(&g["size"][1])],
                colour: [0, 1, 2].map(|k| f(&g["colour"][k])),
                level: f(&g["level"]),
            })
            .collect();
        let l = &v["lightning"];
        let lists: Vec<Vec<i16>> = l["lists"].as_array().into_iter().flatten().map(|x| x.as_array().into_iter().flatten().map(|v| v.as_i64().unwrap_or(-1) as i16).collect()).collect();
        let bolts: Vec<Vec3> = l["bolts"].as_array().into_iter().flatten().map(vec3).collect();
        let flickers: Vec<(Vec3, usize)> = l["flickers"].as_array().into_iter().flatten().map(|x| (vec3(&x["at"]), x["bolt"].as_u64().unwrap_or(0) as usize)).collect();
        let signs = v["signs"].as_array().into_iter().flatten().filter_map(|c| Some((c.as_u64()? as usize, 0, 0, 0))).collect();
        let flashes = Flashes {
            // (a list the code steps through ends in a negative)
            task: lists.len() == 9 && lists.iter().all(|l| l.last().is_some_and(|&v| v < 0)) && bolts.len() <= 4 && flickers.len() <= 3,
            bolt: vec![None; bolts.len()],
            flicker: vec![(0, None); flickers.len()],
            lists,
            bolts,
            flickers,
            thunder: l["thunder"].as_array().into_iter().flatten().map(|t| t.as_u64().unwrap_or(0) as u16).collect(),
            roll: l["roll"].as_u64().unwrap_or(0) as u16,
            near: [f(&l["near"][0]), f(&l["near"][1])],
            signs,
            ..Flashes::default()
        };
        let snow_look = (v["snow"]["set"].as_u64().unwrap_or(0) as usize, f(&v["snow"]["level"]), f(&v["snow"]["fade"]));
        let mut me = Self { sets, streams, particles: vec![], wind, groups, wisps: vec![], beacons: vec![], snow: vec![], snow_look, count: 0, seed: 0, flashes, sounds: vec![] };
        // the wisps (0x3A16C0, 0x3A18F8 as each one starts)
        for (group, g) in list("wisps").enumerate() {
            for k in 0..g["count"].as_u64().unwrap_or(0).min(256) {
                let height = f(&g["height"][0]) + me.rand01() * f(&g["height"][1]);
                let phase = me.rand01() * std::f32::consts::TAU;
                let radius = me.rand01() * f(&g["radius"][1]) + f(&g["radius"][0]);
                let turn = (me.rand01() * 2.0 - 1.0) * f(&g["turn"][0]);
                let turn = if turn > 0.0 { turn + f(&g["turn"][1]) } else { turn - f(&g["turn"][1]) };
                let fade = me.rand01() * f(&g["fade"][0]);
                let fade_rate = me.rand01() * f(&g["fade"][1]);
                me.wisps.push(Wisp { group, angle: k as f32 * f(&g["step"]), turn, radius, height, phase, fade, fade_rate });
            }
        }
        // the beacons (0x3A1378)
        for b in list("beacons") {
            let period = f(&b["period"]);
            let phase = b["phase"].as_f64().map_or_else(|| me.rand01() * std::f32::consts::PI, |p| p as f32);
            let rate = if period > 0.0 { std::f32::consts::TAU / period } else { 0.0 };
            me.beacons.push(Beacon { set: b["set"].as_u64().unwrap_or(0) as usize, at: vec3(&b["at"]), size: f(&b["size"]), level: f(&b["level"]), phase, rate });
        }
        // the snow (0x3E6B10: x and z at random in the box, falling `fall` a frame from `top`, drifting up to `drift`
        // a frame along a heading at random, from a frame at random)
        for l in v["snow"]["layers"].as_array().into_iter().flatten() {
            let (box_, frames) = (f(&l["box"]), l["frames"].as_u64().unwrap_or(1).clamp(1, 4096).next_power_of_two() as u32);
            let mut flakes = vec![];
            for _ in 0..l["count"].as_u64().unwrap_or(0).min(4096) {
                let (x, z) = ((me.rand01() - 0.5) * box_, (me.rand01() - 0.5) * box_);
                let a = me.rand01() * std::f32::consts::TAU - std::f32::consts::PI;
                let r = me.rand01() * f(&l["drift"]);
                let first = me.rand() & (frames - 1);
                flakes.push((Vec3::new(x, f(&l["top"]), z), Vec3::new(r * a.cos(), -f(&l["fall"]), r * a.sin()), first));
            }
            me.snow.push(Snow { flakes, size: f(&l["size"]), box_, ahead: f(&l["ahead"]), copies: f(&l["copies"]), frames });
        }
        (!me.streams.is_empty() || !me.wisps.is_empty() || !me.beacons.is_empty() || me.flashes.task || !me.flashes.signs.is_empty() || !me.snow.is_empty()).then_some(me)
    }

    /// The EMBU's screen (11) starts no lightning task (0x3A72A8): only its op 7 flashes.
    pub fn without_lightning(&mut self) {
        self.flashes.task = false;
    }

    /// Sign channel `channel` has `frames` frames (the stage's texture channel).
    pub fn sign_frames(&mut self, channel: usize, frames: usize) {
        for s in self.flashes.signs.iter_mut().filter(|s| s.0 == channel) {
            s.1 = frames;
        }
    }

    /// EMBU op 7 (0x3A7FA8): channel `channel` plays list `list` (op 7's table), then rests (channels 0-3 on frame 1,
    /// 4-6 on frame 3).
    pub fn flash_op(&mut self, channel: i32, list: i32) {
        if let (Ok(c @ 0..7), Ok(l)) = (usize::try_from(channel), usize::try_from(list)) {
            if self.flashes.lists.get(l).is_some_and(|l| l.first().is_some_and(|&v| v >= 0)) {
                self.flashes.ops.push((c, l, 0));
            }
        }
    }

    /// The frame texture channel `channel` shows (None: the stage's own).
    pub fn channel_frame(&self, channel: usize) -> Option<usize> {
        self.flashes.frames.get(channel).copied().flatten()
    }

    /// A bolt lights the fighters (their lights 0 and 1 with x and z negated, 0x353758).
    pub fn lit(&self) -> bool {
        self.flashes.lit
    }

    /// A frame of arena 4's lightning and arena 12's signs, the camera `view_proj` (y up, the arena's axes). The
    /// lightning (0x3A76C0): each bolt flashes at 6 in 1000 a frame through one of the three bolt lists, with its
    /// thunder when its point is within `near`[0] of the screen's centre; each flicker counts its frames from rand %
    /// 240: at 240 back to 0 and at 30 in 100 the long list (the roll of thunder), from 168 at 20 in 1000 a middle
    /// list, before at 10 in 1000 a short one; its frame 0 shows its bolt's frame 0. A list's frame 0 near the
    /// centre turns the fighters' lights; its end rests the channel (a bolt on frame 1, a flicker on 3) and turns
    /// them back. Each sign (0x3A7590) at 7 in 1000 a frame plays its frames once at half speed.
    pub fn flash(&mut self, view_proj: glam::Mat4) {
        let Self { flashes: fl, seed, sounds, .. } = self;
        let mut rand = || {
            *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            *seed & 0x7FFF_FFFF
        };
        let near = |at: Vec3, reach: f32| {
            let c = view_proj * at.extend(1.0);
            c.w > 0.0 && (c.x / c.w * 320.0).hypot(c.y / c.w * 224.0) < reach
        };
        let at = |lists: &[Vec<i16>], l: usize, i: usize| lists.get(l).and_then(|l| l.get(i)).copied().unwrap_or(-1);
        if fl.task {
            if !fl.started {
                // (state 0: the flickers' counts from rand % 240; the bolts' counters, unread, from rand % 1000)
                fl.started = true;
                for f in &mut fl.flicker {
                    f.0 = rand() % 240;
                }
                for _ in &fl.bolt {
                    rand();
                }
            } else {
                for k in 0..fl.bolt.len() {
                    let heard = near(fl.bolts[k], fl.near[0]);
                    match fl.bolt[k] {
                        None => {
                            if rand() % 1000 < 6 {
                                let r = (rand() % 3) as usize;
                                fl.bolt[k] = Some((6 + r, 0));
                                if let Some(&t) = fl.thunder.get(r).filter(|_| heard) {
                                    sounds.push(t);
                                }
                            }
                        }
                        Some((l, i)) => {
                            let v = at(&fl.lists, l, i);
                            fl.frames[k] = Some(v.max(0) as usize);
                            if heard {
                                fl.lit = v == 0;
                            }
                            fl.bolt[k] = Some((l, i + 1));
                            if at(&fl.lists, l, i + 1) < 0 {
                                (fl.bolt[k], fl.frames[k], fl.lit) = (None, Some(1), false);
                            }
                        }
                    }
                }
                for k in 0..fl.flicker.len() {
                    let ((point, bolt), channel) = (fl.flickers[k], 4 + k);
                    let heard = near(point, fl.near[1]);
                    match fl.flicker[k] {
                        (count, None) => {
                            let count = count + 1;
                            fl.flicker[k].0 = count;
                            if count >= 240 {
                                fl.flicker[k].0 = 0;
                                if rand() % 100 < 30 {
                                    fl.flicker[k].1 = Some((0, 0));
                                    if heard {
                                        sounds.push(fl.roll);
                                    }
                                }
                            } else if count >= 168 {
                                if rand() % 1000 < 20 {
                                    fl.flicker[k].1 = Some((1 + (rand() % 3) as usize, 0));
                                }
                            } else if rand() % 1000 < 10 {
                                fl.flicker[k].1 = Some((4 + (rand() & 1) as usize, 0));
                            }
                        }
                        (_, Some((l, i))) => {
                            let v = at(&fl.lists, l, i);
                            fl.frames[channel] = Some(v.max(0) as usize);
                            if v == 0 {
                                // (its bolt's frame 0, that bolt's own flash ended)
                                if let Some(b) = fl.bolt.get_mut(bolt) {
                                    *b = None;
                                }
                                fl.frames[bolt.min(7)] = Some(0);
                                fl.lit |= heard;
                            } else {
                                fl.frames[bolt.min(7)] = Some(1);
                            }
                            fl.flicker[k].1 = Some((l, i + 1));
                            if at(&fl.lists, l, i + 1) < 0 {
                                (fl.flicker[k].1, fl.frames[channel], fl.lit) = (None, Some(3), false);
                            }
                        }
                    }
                }
            }
        }
        // EMBU op 7's: a frame of the list each, then the channel at rest
        for (c, l, i) in &mut fl.ops {
            fl.frames[*c] = Some(at(&fl.lists, *l, *i).max(0) as usize);
            *i += 1;
        }
        let lists = &fl.lists;
        let frames = &mut fl.frames;
        fl.ops.retain(|&(c, l, i)| {
            let on = at(lists, l, i) >= 0;
            if !on {
                frames[c] = Some(if c < 4 { 1 } else { 3 });
            }
            on
        });
        // the signs: held on frame 0, now and then played once at 0x80 (frame t >> 8), back to 0 past the last
        for (c, n, state, t) in &mut fl.signs {
            match state {
                0 => *state = 1,
                1 if rand() % 1000 < 7 => (*state, *t) = (2, 0),
                2 => {
                    *t += 0x80;
                    if (*t >> 8) as usize > *n {
                        *state = 1;
                    }
                }
                _ => {}
            }
            let k = (*t >> 8) as usize;
            if let Some(f) = frames.get_mut(*c) {
                *f = Some(if *state == 2 && k < *n { k } else { 0 });
            }
        }
    }

    /// The PS2's rand() (0x40FBD0).
    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        self.seed & 0x7FFF_FFFF
    }

    /// 0..1 from its low 24 bits (0x3A94B0).
    fn rand01(&mut self) -> f32 {
        (self.rand() & 0xFF_FFFF) as f32 / 16_777_216.0
    }

    /// A particle of stream `stream` on a path drawn at random, `age` frames along it.
    fn spawn(&mut self, stream: usize, age: usize) {
        let Some(s) = self.streams.get(stream) else { return };
        let (set, most) = (s.set, s.most);
        let n = self.sets.get(set).map_or(0, |s| s.paths.len());
        if n > 0 {
            let path = (self.rand() as usize % n).min(most);
            self.particles.push(Particle { stream, path, age, drift: Vec3::ZERO });
        }
    }

    fn wind_at(&self, p: Vec3) -> Vec3 {
        self.wind.at(p)
    }

    /// Particle `p`'s place `age` frames along its path, with what its path says there (size, fade).
    fn place(&self, p: &Particle, age: usize) -> Option<(Vec3, [f32; 5])> {
        let s = self.streams.get(p.stream)?;
        let fr = *self.sets.get(s.set)?.paths.get(p.path)?.get(age)?;
        let o = Vec3::new(fr[0], fr[1], fr[2]) * s.scale;
        let [sin, cos] = s.turn;
        Some((s.at + Vec3::new(o.x * cos + o.z * sin, o.y, o.z * cos - o.x * sin) + p.drift, fr))
    }

    /// A frame on.
    pub fn step(&mut self) {
        self.wind.step();
        self.count += 1;
        // the tasks: a stream starts its particles every so many frames from the second (the first prewarms)
        for k in 0..self.streams.len() {
            let (every, prewarm, set) = (self.streams[k].every, self.streams[k].prewarm, self.streams[k].set);
            if self.count == 1 {
                if prewarm {
                    let len = self.sets.get(set).and_then(|s| s.paths.first()).map_or(0, Vec::len);
                    for j in 0..len / every as usize {
                        self.spawn(k, j * every as usize);
                    }
                }
            } else if self.count.is_multiple_of(every) {
                self.spawn(k, 0);
            }
        }
        // the particles: a frame along their paths, the wind's push (0x3A08D8), gone at their paths' ends
        let mut parts = std::mem::take(&mut self.particles);
        for p in &mut parts {
            p.age += 1;
            let gain = self.streams.get(p.stream).map_or(0.0, |s| s.wind);
            if let Some((at, _)) = self.place(p, p.age - 1).filter(|_| gain != 0.0) {
                p.drift += self.wind_at(at) * (self.rand01() * gain);
            }
        }
        parts.retain(|p| self.streams.get(p.stream).and_then(|s| self.sets.get(s.set)?.paths.get(p.path)).is_some_and(|path| p.age <= path.len()));
        self.particles = parts;
        // the wisps turn, fade and bob; the beacons' phase (0x3A1298)
        for k in 0..self.wisps.len() {
            let bob = self.groups.get(self.wisps[k].group).map_or(0.0, |g| g.bob) * self.rand01();
            let w = &mut self.wisps[k];
            (w.angle, w.fade, w.phase) = (w.angle + w.turn, w.fade + w.fade_rate, w.phase + bob);
        }
        for b in &mut self.beacons {
            b.phase += b.rate;
            if b.phase > std::f32::consts::TAU {
                b.phase -= std::f32::consts::TAU;
            }
        }
    }

    /// SPECIAL EFFECTS' burst (0x3A2298): a particle on every path of the arena's set 1 (the PS2's set 7) from `at`
    /// (metres), 8.16 times its path (gp-0x7E88), brightness 180 and size 75 (as streams' `level` x 128 and `size`
    /// x 175), added.
    pub fn burst(&mut self, at: Vec3) {
        let Some(paths) = self.sets.get(1).map(|s| s.paths.len()) else { return };
        let stream = self.streams.len();
        self.streams.push(Stream {
            set: 1,
            every: u32::MAX,
            at,
            turn: [0.0, 1.0],
            scale: Vec3::splat(8.16),
            colour: [1.0; 3],
            level: 180.0 / 128.0,
            size: 75.0 / 175.0,
            add: true,
            wind: 0.0,
            most: usize::MAX,
            prewarm: false,
        });
        self.particles.extend((0..paths).map(|path| Particle { stream, path, age: 0, drift: Vec3::ZERO }));
    }

    /// What to draw now (0x345A00): a quad of `size` x the path's at each particle, its brightness the level x (1 -
    /// the fade), none at size 0 or fully faded; blended ones as alpha, added ones as colour.
    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = vec![];
        let mut quad = |pos: Vec3, set: usize, size: f32, fade: f32, colour: [f32; 3], level: f32, add: bool| {
            let Some(cell) = self.sets.get(set).map(|s| s.cell).filter(|_| size > 0.0 && fade < 1.0) else { return };
            let k = level * (1.0 - fade);
            let [r, g, b] = colour;
            let (color, blend) = if add { ([k * r, k * g, k * b, 1.0], Blend::Add) } else { ([r, g, b, k.min(1.0)], Blend::Half) };
            out.push(Sprite { pos, size, cell, color, blend, mirror: false, nearer: 0.0 });
        };
        for p in self.particles.iter().filter(|p| p.age > 0) {
            if let (Some((at, fr)), Some(s)) = (self.place(p, p.age - 1), self.streams.get(p.stream)) {
                quad(at, s.set, s.size * fr[3], fr[4], s.colour, s.level, s.add);
            }
        }
        // (0x3A16C0: x sin, z cos of its angle)
        for w in &self.wisps {
            let Some(g) = self.groups.get(w.group) else { continue };
            let fade = (w.fade.sin() + 1.0) * 0.5;
            let pos = Vec3::new(w.angle.sin() * w.radius, w.phase.sin() * g.swing + w.height, w.angle.cos() * w.radius);
            quad(pos, g.set, g.size[0] + fade * g.size[1], fade, g.colour, g.level, false);
        }
        for b in &self.beacons {
            quad(b.at, b.set, b.size, (b.phase.cos() + 1.0) * 0.5, [1.0; 3], b.level, true);
        }
        out
    }

    /// The snow now, seen from `eye` looking along `forward` (VU1 program 0x708): each flake in the box of its layer
    /// `ahead` of the eye (its start moved by whole boxes, 0x3E7270's corner), `frames` along its fall, drawn at the
    /// four places `copies` aside; its alpha the level less the fade by its depth.
    pub fn snow(&self, eye: Vec3, forward: Vec3) -> Vec<Sprite> {
        let (set, level, fade) = self.snow_look;
        let Some(cell) = self.sets.get(set).map(|s| s.cell) else { return vec![] };
        let ahead = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        let mut out = vec![];
        for l in self.snow.iter().filter(|l| l.box_ > 0.0) {
            let corner = eye + ahead * l.ahead + Vec3::splat(l.box_ * 0.5);
            let fit = |start: f32, corner: f32| start + ((corner - start) / l.box_).floor() * l.box_;
            for (start, step, first) in &l.flakes {
                let t = first.wrapping_add(self.count) & (l.frames - 1);
                let at = Vec3::new(fit(start.x, corner.x), start.y, fit(start.z, corner.z)) + *step * t as f32;
                for aside in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
                    let pos = at + aside * l.copies;
                    let alpha = level - fade * (pos - eye).dot(forward);
                    if alpha > 0.0 {
                        out.push(Sprite { pos, size: l.size, cell, color: [1.0, 1.0, 1.0, alpha.min(1.0)], blend: Blend::Half, mirror: false, nearer: 0.0 });
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_stream_starts_a_particle_every_so_often_and_follows_its_path() {
        let path: Vec<Value> = (0..5).map(|k| json!([0.0, k as f64, 0.0, 1.0, 0.0])).collect();
        let v = json!({"sets": [{"cell": 30, "paths": [path]}],
            "streams": [{"set": 0, "every": 2, "at": [1.0, 0.0, 0.0], "heading": 0.0, "scale": [1.0, 2.0, 1.0], "colour": [1.0, 1.0, 1.0],
                "level": 1.0, "size": 0.5, "add": false, "wind": 0.0, "most": null, "prewarm": false}]});
        let mut w = Weather::new(&v).unwrap();
        w.step();
        assert!(w.sprites().is_empty());
        w.step();
        w.step();
        // started on frame 2, drawn 2 frames along: (1, 2 x 1, 0)
        let s = w.sprites();
        assert_eq!((s.len(), s[0].pos, s[0].cell, s[0].size), (1, Vec3::new(1.0, 2.0, 0.0), 30, 0.5));
        for _ in 0..10 {
            w.step();
        }
        // (each gone after its 5 frames: at most 3 at a time)
        assert!(w.particles.len() <= 3);
    }

    #[test]
    fn the_burst_starts_a_particle_on_every_path_of_set_1() {
        let path: Vec<Value> = (0..3).map(|k| json!([k as f64, 0.0, 0.0, 1.0, 0.0])).collect();
        let v = json!({"sets": [{"cell": 26, "paths": []}, {"cell": 30, "paths": [path.clone(), path]}],
            "streams": [{"set": 0, "every": 32, "at": [0.0, 0.0, 0.0], "heading": 0.0, "scale": [1.0, 1.0, 1.0], "colour": [1.0, 1.0, 1.0],
                "level": 1.0, "size": 1.0, "add": false, "wind": 0.0, "most": null, "prewarm": false}]});
        let mut w = Weather::new(&v).unwrap();
        w.burst(Vec3::new(0.0, 1.0, 8.0));
        w.step();
        let s = w.sprites();
        assert_eq!((s.len(), s[0].cell, s[0].pos), (2, 30, Vec3::new(0.0, 1.0, 8.0)));
        w.step();
        assert_eq!(w.sprites()[0].pos, Vec3::new(8.16, 1.0, 8.0));
    }

    #[test]
    fn arena_4s_lightning_and_arena_12s_signs_play_their_still_channels() {
        let short: Vec<Value> = vec![json!([1, 1, 2, -1]); 6];
        let bolt = json!([0, 1, 0, -1]);
        let lists: Vec<Value> = short.into_iter().chain([bolt.clone(), bolt.clone(), bolt]).collect();
        let v = json!({"lightning": {"lists": lists, "bolts": [[0.0, 0.0, -10.0]], "flickers": [{"at": [0.0, 0.0, -10.0], "bolt": 0}],
            "thunder": [0x1072, 0x1074, 0x1075], "roll": 0x1073, "near": [625.0, 312.5]}, "signs": [2]});
        let mut w = Weather::new(&v).unwrap();
        let looking = glam::camera::rh::proj::directx::perspective(1.0, 1.0, 0.1, 100.0);
        // the EMBU: no task, only op 7 (channel 4, list 1: frames 1, 1, then 3: the list ends on the frame it shows
        // its last, 0x3A7EB0, before 0x3A6C80 sends it)
        w.without_lightning();
        w.sign_frames(2, 3);
        w.flash_op(4, 1);
        let shown: Vec<Option<usize>> = (0..4)
            .map(|_| {
                w.flash(looking);
                w.channel_frame(4)
            })
            .collect();
        assert_eq!(shown, [Some(1), Some(1), Some(3), Some(3)]);
        assert!(!w.lit() && w.sounds.is_empty());
        // a sign plays its 3 frames once, each 2 frames, back on 0
        let mut seen = vec![];
        for _ in 0..3000 {
            w.flash(looking);
            seen.push(w.channel_frame(2).unwrap_or(9));
            if seen.ends_with(&[0, 0, 1, 1, 2, 2, 0]) {
                break;
            }
        }
        assert!(seen.ends_with(&[0, 0, 1, 1, 2, 2, 0]), "{seen:?}");
        // the task: a bolt in sight flashes (frame 0 lights the fighters) with its thunder, then rests on frame 1
        let mut w = Weather::new(&v).unwrap();
        let mut lit = false;
        for _ in 0..3000 {
            w.flash(looking);
            lit |= w.lit();
            if !w.sounds.is_empty() && w.channel_frame(0) == Some(1) {
                break;
            }
        }
        assert!(lit && w.sounds.iter().all(|s| (0x1072..=0x1075).contains(s)), "{:?}", w.sounds);
    }

    #[test]
    fn snow_falls_in_its_box_ahead_of_the_eye_drawn_four_times_aside() {
        let v = json!({"sets": [{"cell": 24, "paths": []}], "snow": {"set": 0, "level": 0.75, "fade": 0.01, "layers": [
            {"count": 1, "box": 10.0, "top": 5.0, "ahead": 30.0, "copies": 1.0, "fall": 0.1, "drift": 0.0, "frames": 4, "size": 0.06}]}});
        let mut w = Weather::new(&v).unwrap();
        w.step();
        let (eye, forward) = (Vec3::new(100.0, 1.0, 50.0), Vec3::new(0.0, -0.6, -0.8));
        let s = w.snow(eye, forward);
        assert_eq!(s.len(), 4);
        // its place: within the box 30 m ahead (z 20), its copies 1 m aside, `t` frames down from 5 m
        let at = s[0].pos - Vec3::X;
        assert!((95.0..=105.0).contains(&at.x) && (15.0..=25.0).contains(&at.z), "{at}");
        let t = (w.snow[0].flakes[0].2 + 1) & 3;
        assert!((at.y - (5.0 - 0.1 * t as f32)).abs() < 1e-5);
        assert_eq!([s[1].pos - at, s[2].pos - at, s[3].pos - at], [Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z]);
        assert_eq!((s[0].cell, s[0].size), (24, 0.06));
        assert!((s[0].color[3] - (0.75 - 0.01 * (s[0].pos - eye).dot(forward))).abs() < 1e-6);
    }

    #[test]
    fn the_wind_is_a_lorenz_series_seen_later_further_along() {
        let v = json!({"sets": [{"cell": 1, "paths": []}], "beacons": [{"set": 0, "at": [0.0, 0.0, 0.0], "period": 0.0, "phase": std::f64::consts::PI, "size": 1.0, "level": 1.0}],
            "wind": {"dt": 0.01, "beta": 8.0 / 3.0, "reach": 10.0, "waves": [{"step": 1, "speed": 0.3, "strength": 0.001, "toward": [0.6, 0.8]}]}});
        let w = Weather::new(&v).unwrap();
        let wave = &w.wind.waves[0];
        // (1, 1, 1) a step on: x stays 1, (1 + 40) / 80
        let mut fresh = Wave { lorenz: [1.0; 3], ..Weather::new(&v).unwrap().wind.waves.remove(0) };
        assert!((fresh.next(0.01, 8.0 / 3.0) - 41.0 / 80.0).abs() < 1e-6);
        // the oldest value on the front line (through x = sqrt(10^2 / (x^2 / z^2 + 1)) = 8, z = 8 z / x), later ones
        // further along the wave
        let (front, toward) = (Vec3::new(wave.from[0], 0.0, wave.from[1]), Vec3::new(0.6, 0.0, 0.8));
        assert!((front - Vec3::new(8.0, 0.0, 32.0 / 3.0)).length() < 1e-4);
        assert_eq!(wave.at(front), toward * 0.001 * wave.kept[0]);
        assert_eq!(wave.at(front + toward * 0.95), toward * 0.001 * wave.kept[3]);
        // a beacon of period 0: always lit
        assert_eq!(w.sprites()[0].color, [1.0, 1.0, 1.0, 1.0]);
    }
}
