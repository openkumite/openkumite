//! Tekken Tag's arenas' background characters as their scripts move them (stages.json `people`; the converter's
//! FORMATS.md 12, the PS2's 0x3D8908 start, 0x3D8B20 / 0x3D8848 a step's motion, 0x3D8D38 / 0x3D8E30 a motion's end,
//! 0x3D9718 the frames): unseen while they wait, a motion of the step's at random, steps repeated and gone back to,
//! moved on by how far each motion takes them. Drawn only: their own random numbers (the PS2's rand()).

use glam::Vec3;
use serde_json::Value;
use std::collections::HashMap;

/// A step of a script.
struct Step {
    motions: Vec<usize>,
    model: i8,
    repeat: i8,
    back: i8,
    wait: i16,
    start: i16,
    frames: i16,
}

/// A person's script and where it is in it.
pub struct Walker {
    at: Vec3,
    heading: f32,
    wait_range: [i32; 2],
    start_range: [i32; 2],
    /// The motion and repeat of one without a script.
    motion0: usize,
    repeat0: i8,
    script: Vec<Step>,
    stop: bool,
    /// How far each motion takes it: [dx, dz] (m, its own axes), the turn (degrees); its model, and each model's
    /// scale of that and of its root's motion (stages.json `scales`, 0x3D87A8).
    walk: HashMap<usize, [f32; 3]>,
    model: usize,
    scales: Vec<f32>,
    // the state (the PS2's person +0x50 .. +0x8E)
    pos: Vec3,
    turn: f32,
    step: usize,
    /// The repeats being counted: (count left, the step), innermost last (three at most).
    loops: Vec<(i8, usize)>,
    motion: usize,
    frame: i32,
    wait: i32,
    budget: i32,
    gone: bool,
    seed: u32,
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or(-1)
}

impl Walker {
    /// A person of stages.json `people`, `walk` and `scales` the arena's tables, `seed` its own random numbers' start.
    pub fn new(p: &Value, walk: &Value, scales: &Value, seed: u32) -> Self {
        let f = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
        let pair = |v: &Value| [i(&v[0]).max(0) as i32, i(&v[1]).max(0) as i32];
        let script = (p["script"].as_array().into_iter().flatten())
            .map(|s| Step {
                motions: s["motions"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|m| m as usize).collect(),
                model: i(&s["model"]).clamp(-16, 16) as i8,
                repeat: i(&s["repeat"]) as i8,
                back: i(&s["back"]) as i8,
                wait: i(&s["wait"]) as i16,
                start: i(&s["start"]) as i16,
                frames: i(&s["frames"]) as i16,
            })
            .collect();
        let walk = (walk.as_object().into_iter().flatten()).filter_map(|(m, w)| Some((m.parse().ok()?, [f(&w[0]), f(&w[1]), f(&w[2])]))).collect();
        let at = Vec3::new(f(&p["at"][0]), f(&p["at"][1]), f(&p["at"][2]));
        let motion0 = p["motions"][0].as_u64().unwrap_or(0) as usize;
        let mut me = Self {
            at,
            heading: f(&p["heading"]),
            wait_range: pair(&p["wait"]),
            start_range: pair(&p["start"]),
            motion0,
            repeat0: i(&p["repeat"]) as i8,
            script,
            stop: p["stop"].as_bool().unwrap_or(false),
            walk,
            model: p["model"].as_u64().unwrap_or(0) as usize,
            scales: scales.as_array().into_iter().flatten().map(f).collect(),
            pos: at,
            turn: 0.0,
            step: 0,
            loops: vec![],
            motion: motion0,
            frame: 0,
            wait: 0,
            budget: -1,
            gone: false,
            seed,
        };
        me.restart();
        me
    }

    /// The PS2's rand() (0x40FBD0).
    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        self.seed & 0x7FFF_FFFF
    }

    /// A value from `lo` up to `hi` at random (0x3D8908's ranges: the larger less the smaller added).
    fn between(&mut self, [lo, hi]: [i32; 2]) -> i32 {
        let span = (hi - lo).abs();
        if span == 0 {
            lo
        } else {
            lo + (self.rand() / (0x7FFF_FFFF / span as u32 + 1)) as i32
        }
    }

    /// The script from its start (0x3D8908): back where it stands, its wait and start frame at random.
    fn restart(&mut self) {
        (self.pos, self.turn, self.step) = (self.at, 0.0, 0);
        self.loops.clear();
        self.wait = self.between(self.wait_range);
        self.frame = self.between(self.start_range);
        self.budget = -1;
        self.overrides();
        self.pick();
    }

    /// The step's own wait, start frame and frames where it has them.
    fn overrides(&mut self) {
        if let Some(s) = self.script.get(self.step) {
            if s.wait >= 0 {
                self.wait = i32::from(s.wait);
            }
            if s.start >= 0 {
                self.frame = i32::from(s.start);
            }
            if s.frames > 0 {
                self.budget = i32::from(s.frames);
            }
        }
    }

    /// The step's motion (0x3D8848): one of its motions at random.
    fn pick(&mut self) {
        self.motion = match self.script.get(self.step).map(|s| s.motions.len()) {
            None => self.motion0,
            Some(n) if n >= 2 => {
                let k = (self.rand() / (0x7FFF_FFFF / n as u32 + 1)) as usize;
                self.script[self.step].motions.get(k).copied().unwrap_or(0)
            }
            Some(_) => self.script[self.step].motions.first().copied().unwrap_or(0),
        };
    }

    /// A step begun (0x3D8B20): moved on by how far the motion just played took it, the step's frames from 0.
    fn begin(&mut self) {
        if let Some([dx, dz, turn]) = self.walk.get(&self.motion).copied() {
            let (dx, dz) = (dx * self.scale(), dz * self.scale());
            let (sin, cos) = (self.heading + self.turn).to_radians().sin_cos();
            self.pos += Vec3::new(dx * cos + dz * sin, 0.0, dz * cos - dx * sin);
            self.turn += turn;
        }
        (self.wait, self.frame, self.budget) = (0, 0, -1);
        self.overrides();
        self.pick();
    }

    /// Its motion ended (0x3D8D38 decides, 0x3D8E30 does it).
    fn end(&mut self) {
        let scripted = !self.script.is_empty();
        let (repeat, back, n) = match self.script.get(self.step) {
            Some(s) => (s.repeat, s.back, s.motions.len()),
            None => (self.repeat0, 0, 1),
        };
        // 0 next step, 1 back (once more), 2 the motion on, 3 the step again, 4 restart
        let code = if repeat < 0 {
            if back == 0 && n == 1 {
                2
            } else if back != 0 {
                1
            } else {
                3
            }
        } else if repeat == 0 {
            0
        } else if self.loops.last().is_some_and(|l| l.1 == self.step) {
            let top = self.loops.len() - 1;
            self.loops[top].0 -= 1;
            if self.loops[top].0 > 0 {
                1
            } else {
                self.loops.pop();
                0
            }
        } else if self.loops.len() < 3 {
            self.loops.push((repeat - 1, self.step));
            1
        } else {
            0
        };
        match (code, scripted) {
            (2, _) => self.frame = 0,
            (0, true) => {
                self.step += 1;
                if self.step < self.script.len() {
                    self.begin();
                } else if self.stop {
                    self.gone = true;
                } else {
                    self.restart();
                }
            }
            (1, true) => match self.step.checked_sub(back.max(0) as usize) {
                Some(s) => {
                    self.step = s;
                    self.begin();
                }
                None => self.restart(),
            },
            (1 | 3, _) => self.begin(),
            _ => self.restart(),
        }
    }

    /// A frame on (0x3D9718); `len` a motion's frames.
    pub fn step(&mut self, len: impl Fn(usize) -> i32) {
        if self.gone {
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        if self.frame + 1 < len(self.motion) && self.budget != 0 {
            self.frame += 1;
            if self.budget > 0 {
                self.budget -= 1;
            }
        } else {
            self.end();
        }
    }

    /// The model it shows now: its own plus its step's (0x3D87A8).
    pub fn model(&self) -> usize {
        let add = self.script.get(self.step).map_or(0, |s| s.model);
        self.model.saturating_add_signed(isize::from(add))
    }

    /// Its model's scale of its root's motion and its walk (1 without one).
    pub fn scale(&self) -> f32 {
        self.scales.get(self.model()).copied().filter(|s| *s > 0.0).unwrap_or(1.0)
    }

    /// What it shows now: its motion, the frame of it, where it stands and its heading (degrees); None while unseen.
    pub fn now(&self) -> Option<(usize, usize, Vec3, f32)> {
        (!self.gone && self.wait <= 0).then_some((self.motion, self.frame.max(0) as usize, self.pos, self.heading + self.turn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_script_repeats_goes_back_walks_and_starts_again() {
        // step 0: motion 10 twice (repeat 2); step 1: motion 11, then back to step 0 once (repeat 1, back 1); then the
        // script again from the start; motion 10 takes it 1 m along +z and turns it 90 degrees
        let p = json!({"at": [5.0, 0.0, 0.0], "heading": 0.0, "motions": [10, 11], "wait": [0, 0], "start": [0, 0], "repeat": -1, "stop": false,
            "script": [{"motions": [10], "repeat": 2, "back": 0, "wait": -1, "start": -1, "frames": -1},
                       {"motions": [11], "repeat": 1, "back": 1, "wait": -1, "start": -1, "frames": -1}]});
        let mut w = Walker::new(&p, &json!({"10": [0.0, 1.0, 90.0]}), &json!([]), 1);
        let mut seen = vec![];
        for _ in 0..40 {
            let (m, f, at, h) = w.now().unwrap();
            if f == 0 {
                seen.push((m, at.round(), h));
            }
            w.step(|_| 4);
        }
        // 10 twice, 11, back: 10 twice, 11 (its repeat done), the script again from where it stood
        let order: Vec<usize> = seen.iter().map(|s| s.0).collect();
        assert_eq!(order[..7], [10, 10, 11, 10, 10, 11, 10]);
        // after the first 10: 1 m along +z; after the second, turned 90 degrees: 1 m along +x
        assert_eq!((seen[1].1, seen[2].1, seen[2].2), (Vec3::new(5.0, 0.0, 1.0), Vec3::new(6.0, 0.0, 1.0), 180.0));
        assert_eq!(seen[6].1, Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn it_waits_unseen_and_is_gone_at_a_stopping_end() {
        let p = json!({"at": [0.0, 0.0, 0.0], "heading": 0.0, "motions": [3], "wait": [0, 0], "start": [0, 0], "repeat": -1, "stop": true,
            "script": [{"motions": [3], "repeat": 0, "back": 0, "wait": 2, "start": -1, "frames": 3}]});
        let mut w = Walker::new(&p, &json!({}), &json!([2.0]), 1);
        assert!(w.now().is_none());
        w.step(|_| 100);
        w.step(|_| 100);
        // seen for its 3 frames, then gone
        let frames: Vec<Option<usize>> = (0..6).map(|_| (w.now().map(|n| n.1), w.step(|_| 100)).0).collect();
        assert_eq!(frames, [Some(0), Some(1), Some(2), Some(3), None, None]);
    }
}
