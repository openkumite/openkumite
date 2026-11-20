//! Tekken Tag's fingers (the converter's FIGHT_DATA.md 3.1). Each hand shows a pose of its character's track
//! (`characters/hands_NNN.glb`, clip `hands`: 0 open to 16 the fist, then the character's own), eased in by a
//! weight: each frame the fingers' turns go `weight` of the way from what they were to the pose (0x34A180, rows
//! mixed then squared up, 0x34A710), the weight rising by 1 / (blend + 1) a frame to 1 (0x3634F0). A hand is set
//! (0x362E90) by the move's timed ops (Tekken Tag's 0x0F + v: v >> 5 the hands, v & 31 the kind, the argument the
//! blend; kind 0 pose 0, k pose k + 15, 9 the character's own kind), and by the fight each frame (0x370BB4): a
//! K.O.'d fighter opens them, and while its move has set none, a new motion in the round goes back to the
//! character's kind, so does an arm's strike from 7 frames before its active end, and a throw's victim opens them at
//! its frame 10. Hand 0 is hand_B, 1 hand_A (0x3DE0D8).

use glam::{Mat3, Mat4, Quat};
use kumite_assets::Character;
use kumite_sim::data::HandsEntry;
use kumite_sim::dialect::HAND_OPS;

/// A body's hands: the finger nodes of each, their turns as bound and in each pose.
#[derive(Clone, Debug)]
pub struct Poses {
    pub nodes: [[usize; 15]; 2],
    pub bind: [[Quat; 15]; 2],
    pub poses: Vec<[[Quat; 15]; 2]>,
    pub entry: HandsEntry,
}

impl Poses {
    /// From the character's clip `hands` (its hands file added); None without it or its fingers.
    pub fn new(character: &Character, entry: &HandsEntry) -> Option<Self> {
        let clip = character.clips.get(character.clip("hands")?)?;
        let node = |n: usize| character.nodes.iter().position(|x| x.name == format!("node_{n}"));
        let mut nodes = [[0; 15]; 2];
        for (h, first) in [64, 39].into_iter().enumerate() {
            for (k, n) in nodes[h].iter_mut().enumerate() {
                *n = node(first + k)?;
            }
        }
        let poses = (0..clip.frames)
            .map(|p| {
                let turn = |n: usize| clip.rotations.get(n)?.as_ref()?.get(p).copied();
                let mut q = [[Quat::IDENTITY; 15]; 2];
                for h in 0..2 {
                    for k in 0..15 {
                        q[h][k] = turn(nodes[h][k])?;
                    }
                }
                Some(q)
            })
            .collect::<Option<Vec<_>>>()?;
        let bind = nodes.map(|h| h.map(|n| character.nodes[n].rotation));
        (!poses.is_empty()).then(|| Self { nodes, bind, poses, entry: entry.clone() })
    }

    fn pose(&self, p: u32) -> &[[Quat; 15]; 2] {
        &self.poses[(p as usize).min(self.poses.len() - 1)]
    }
}

/// What the fight did this frame, for [`Hands::step`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Cues<'a> {
    /// The transition a move started with this frame (the commit clears "the move set the hands", +0x11F).
    pub started: Option<u32>,
    /// The move's motion (a new one, or transitions 1 and 3, set +0x134: 0x36A9A0).
    pub motion: &'a str,
    /// K.O.'d (+0x131).
    pub ko: bool,
    /// Round states 1-2, the fight and the K.O. (gp-0x7268).
    pub live: bool,
    /// The move does damage (+0xCA).
    pub damage: bool,
    /// An arm's strike (record +0x28 < 0xC) within 7 frames of its active end or past it.
    pub arm_end: bool,
    /// A throw's victim at its frame 10 (+0xE0 < 0, +0xC8).
    pub thrown_10: bool,
}

/// One fighter's hands (fighter +0x8000 + 0x305E..0x3080).
#[derive(Clone, Debug, Default)]
pub struct Hands {
    pose: [u32; 2],
    speed: [i32; 2],
    weight: [f32; 2],
    /// The move set them (+0x11F).
    set: bool,
    motion: String,
    /// The fingers' turns as shown; None until loaded.
    pub turns: Option<[[Quat; 15]; 2]>,
}

impl Hands {
    /// Hands `mask` (bit 0 hand 0, bit 1 hand 1) to kind `kind` over `blend` frames (0x362E90).
    fn to(&mut self, mask: u32, kind: u32, blend: i32) {
        for h in (0..2).filter(|h| mask >> h & 1 != 0) {
            self.pose[h] = if kind == 0 { 0 } else { kind + 15 };
            self.speed[h] = 10000 / (blend.abs() + 1);
            self.weight[h] = 0.0;
        }
    }

    /// The load again (0x363150, as the replay's end does, 0x364C84): both eased to the character's kind from the
    /// fingers as shown.
    pub fn reload(&mut self, p: &Poses) {
        self.to(3, p.entry.default, 10);
    }

    /// One frame (fight block 0x370BB4, then 0x3634F0, then the move's ops, then the fingers drawn); `ops` this
    /// frame's (op, argument) of the move.
    pub fn step(&mut self, p: &Poses, cues: Cues<'_>, ops: impl Iterator<Item = (u32, u32)>) {
        let e = &p.entry;
        // the load (0x363150): from the model's fingers, both eased to the character's kind (the rest poses it also
        // sets are never drawn: no weight)
        if self.turns.is_none() {
            self.to(3, e.default, 10);
        }
        let turns = self.turns.unwrap_or(p.bind);
        let new_motion = cues.started.is_some_and(|t| matches!(t & 0x3F, 1 | 3) || cues.motion != self.motion);
        if cues.started.is_some() {
            self.set = false;
            self.motion = cues.motion.to_string();
        }
        if cues.ko {
            self.to(3, 0, 10);
        } else if !self.set {
            if (new_motion && !cues.damage && cues.live) || (cues.damage && cues.arm_end) {
                self.to(3, e.default, 10);
            }
            if cues.thrown_10 {
                self.to(3, 0, 20);
            }
        }
        for h in 0..2 {
            if self.weight[h] >= 1.0 {
                self.weight[h] = 0.0;
                self.speed[h] = 0;
            }
            if self.speed[h] > 0 {
                self.weight[h] = (self.weight[h] + self.speed[h] as f32 * 0.0001).min(1.0);
            }
        }
        for (op, arg) in ops {
            let Some(v) = op.checked_sub(HAND_OPS).filter(|v| *v < 0x60) else { continue };
            self.set = true;
            if !e.ops {
                continue;
            }
            let mask = match v >> 5 {
                0 => 3,
                m => m,
            };
            let kind = v & 31;
            if kind == 9 {
                self.to(mask, e.default, arg as i32);
            } else if kind < 9 {
                self.to(mask, kind, arg as i32);
            }
        }
        // the fingers: the rows of what they were and of the pose mixed, then squared up
        let mut now = turns;
        for h in 0..2 {
            let w = self.weight[h];
            for (k, q) in now[h].iter_mut().enumerate() {
                let (a, b) = (Mat3::from_quat(*q), Mat3::from_quat(p.pose(self.pose[h])[h][k]));
                let m = a * (1.0 - w) + b * w;
                let x = m.x_axis.normalize_or_zero();
                let y = (m.y_axis - x * m.y_axis.dot(x)).normalize_or_zero();
                *q = Quat::from_mat3(&Mat3::from_cols(x, y, x.cross(y))).normalize();
            }
        }
        self.turns = Some(now);
    }

    /// Pose the fingers of `pose` (node world matrices) with these turns.
    pub fn apply(&self, p: &Poses, character: &Character, pose: &mut [Mat4]) {
        let Some(turns) = &self.turns else { return };
        for h in 0..2 {
            for (k, &n) in p.nodes[h].iter().enumerate() {
                let (Some(node), Some(parent)) = (character.nodes.get(n), character.nodes.get(n).and_then(|x| x.parent)) else { continue };
                if let (Some(&up), true) = (pose.get(parent), n < pose.len()) {
                    pose[n] = up * Mat4::from_rotation_translation(turns[h][k], node.translation);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poses() -> Poses {
        let open = [[Quat::IDENTITY; 15]; 2];
        let mut poses = vec![open; 17];
        poses[16] = [[Quat::from_rotation_z(1.0); 15]; 2];
        Poses { nodes: [[0; 15]; 2], bind: open, poses, entry: HandsEntry { file: String::new(), default: 0, ops: true } }
    }

    fn angle(h: &Hands) -> f32 {
        h.turns.unwrap()[0][0].angle_between(Quat::IDENTITY)
    }

    /// A fist over 3 frames (op 0x10, argument 3) from the next frame: 1/4, 2/4, 3/4, then all of the way left each
    /// frame, then held; a K.O. opens them 1/11 of the way a frame.
    #[test]
    fn eased_by_the_weight() {
        let p = poses();
        let mut h = Hands::default();
        h.step(&p, Cues::default(), std::iter::empty());
        h.step(&p, Cues::default(), [(HAND_OPS + 1, 3)].into_iter());
        assert!(angle(&h) < 1e-4, "not before the next frame");
        // (a turn about z mixed by rows: atan2(w sin d, 1 - w + w cos d) of the d left)
        let mut want = 0.0f32;
        for k in 1..=5 {
            let (w, d) = ((k.min(4) as f32) / 4.0, 1.0 - want);
            want += (w * d.sin()).atan2(1.0 - w + w * d.cos());
            h.step(&p, Cues::default(), std::iter::empty());
            assert!((angle(&h) - want).abs() < 1e-3, "frame {k}: {} {want}", angle(&h));
        }
        assert!((angle(&h) - 1.0).abs() < 1e-3);
        h.step(&p, Cues { ko: true, ..Default::default() }, std::iter::empty());
        let w = 1.0f32 / 11.0;
        assert!((angle(&h) - (1.0 - (w * 1f32.sin()).atan2(1.0 - w + w * 1f32.cos()))).abs() < 1e-3, "{}", angle(&h));
    }
}
