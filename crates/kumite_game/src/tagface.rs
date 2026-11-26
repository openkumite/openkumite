//! Tekken Tag's faces (the converter's FIGHT_DATA.md 3.2): the face nodes 7-28 show a pose of the model's face
//! track (`characters/face_NNN.glb`, clip `face`), eased in by a weight as the hands are (0x3DD918): each frame a
//! node's turn goes `weight` of the way from what it was to the pose's (rows mixed, then squared up; a scaled turn's
//! rows mixed only), its place likewise, the weight rising by 1 / (blend + 1) a frame to 1. The face (0x362C28) blinks
//! (its model's blink pose for 3 frames, then (rand & 0xFE) + 1 frames open), shows its move pose while the move's
//! flag 0x800 is on and its hit pose while hit, and the timed ops' face kinds (0x362E90: kind - 10, blend 20, held
//! the op's argument).

use glam::{Mat3, Mat4, Vec3};
use kumite_assets::Character;
use kumite_sim::data::FaceEntry;
use kumite_sim::dialect::HAND_OPS;

/// A face node's look in a pose: its turn (None: the bind's) with its rows scaled, and its place (None: kept).
#[derive(Clone, Copy, Debug)]
struct Target {
    turn: Option<Mat3>,
    scale: Option<Vec3>,
    place: Option<Vec3>,
}

/// A body's face: its face nodes (in drawing order), their bind turns and places, and each pose's targets.
#[derive(Clone, Debug)]
pub struct Poses {
    nodes: Vec<usize>,
    bind: Vec<(Mat3, Vec3)>,
    poses: Vec<Vec<Target>>,
    pub entry: FaceEntry,
}

impl Poses {
    /// From the character's clip `face` (its face file added: a key a pose) on its face nodes (`node_N`); None without
    /// it.
    pub fn new(character: &Character, entry: &FaceEntry) -> Option<Self> {
        let clip = character.clips.get(character.clip("face")?)?;
        // (parents before their children: the nodes by depth; the face nodes only, the file's body joints are at rest)
        let depth = |n: usize| std::iter::successors(Some(n), |&a| character.nodes.get(a)?.parent).take(character.nodes.len()).count();
        let keyed = |n: usize| {
            let some = |v: &Vec<Option<Vec<Vec3>>>| v.get(n).is_some_and(Option::is_some);
            clip.rotations.get(n).is_some_and(Option::is_some) || some(&clip.translations) || some(&clip.scales)
        };
        let mut nodes: Vec<usize> = (0..character.nodes.len()).filter(|&n| character.nodes[n].name.starts_with("node_") && keyed(n)).collect();
        nodes.sort_by_key(|&n| depth(n));
        let from = &nodes;
        let poses = (0..clip.frames)
            .map(|p| {
                from.iter()
                    .map(|&k| {
                        let at = |v: &Vec<Option<Vec<Vec3>>>| v.get(k)?.as_ref()?.get(p).copied();
                        Target { turn: clip.rotations.get(k).and_then(|r| r.as_ref()?.get(p)).map(|q| Mat3::from_quat(*q)), scale: at(&clip.scales), place: at(&clip.translations) }
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        let bind = nodes.iter().map(|&n| (Mat3::from_quat(character.nodes[n].rotation), character.nodes[n].translation)).collect();
        (!nodes.is_empty() && !poses.is_empty()).then(|| Self { nodes, bind, poses, entry: entry.clone() })
    }
}

/// What the move does this frame, for [`Face::step`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Cues {
    /// The move's flag 0x800 (+0x24).
    pub moving: bool,
    /// Hit: the move's +4 bit 2, +0x24 bit 22 off.
    pub hit: bool,
}

/// One fighter's face (fighter +0x8000 + 0x3066..0x3090).
#[derive(Clone, Debug, Default)]
pub struct Face {
    /// The face shown (+0x3066) and its frames left (+0x3068).
    pub face: i16,
    pub timer: i16,
    pose: i16,
    speed: f32,
    weight: f32,
    /// Each face node's turn and place as shown; None until the first frame.
    shown: Option<Vec<(Mat3, Vec3)>>,
}

impl Face {
    /// Face `face` over `blend` frames, held `hold` frames (0x362B38; face 0: (rand & 0xFE) + 1).
    fn set(&mut self, face: i16, blend: i32, hold: i16, rand: &mut impl FnMut() -> u32) {
        self.timer = if face == 0 { (rand() & 0xFE) as i16 + 1 } else { hold };
        if self.face != face {
            (self.face, self.pose, self.weight, self.speed) = (face, face, 0.0, 1.0 / (blend.abs() + 1) as f32);
        }
    }

    /// One frame (0x362C28, then the timed ops' faces, then the face drawn 0x3DD918); `ops` this frame's (op,
    /// argument) of the move; `rand` the game's rand().
    pub fn step(&mut self, p: &Poses, cues: Cues, ops: impl Iterator<Item = (u32, u32)>, rand: &mut impl FnMut() -> u32) {
        let e = &p.entry;
        if cues.moving {
            self.set(e.moving, 4, -1, rand);
        } else if cues.hit {
            self.set(e.hit, 8, -1, rand);
        }
        if self.face != 0 && ((self.face == e.moving && !cues.moving) || (self.face == e.hit && !cues.hit)) {
            self.set(0, 1, 0, rand);
        }
        if self.timer == 0 {
            match self.face {
                0 => self.set(e.blink, 1, 3, rand),
                f if f == e.blink => self.set(0, 1, 0, rand),
                _ => self.set(e.blink, 30, 30, rand),
            }
        }
        self.timer = self.timer.wrapping_sub(1);
        if self.weight >= 1.0 {
            (self.weight, self.speed) = (0.0, 0.0);
        }
        if self.speed > 0.0 {
            self.weight = (self.weight + self.speed).min(1.0);
        }
        for (op, arg) in ops {
            let Some(kind) = op.checked_sub(HAND_OPS).filter(|v| *v < 0x60).map(|v| v & 31).filter(|k| *k >= 10) else { continue };
            self.set(kind as i16 - 10, 20, arg as i16, rand);
        }
        // the nodes: each the weight of the way to the pose's
        let w = self.weight;
        let pose = &p.poses[(self.pose.max(0) as usize).min(p.poses.len() - 1)];
        let mut shown = self.shown.take().unwrap_or_else(|| p.bind.clone());
        for ((s, t), b) in shown.iter_mut().zip(pose).zip(&p.bind) {
            if let Some(turn) = t.turn {
                let m = s.0 * (1.0 - w) + turn * w;
                let x = m.x_axis.normalize_or_zero();
                let y = (m.y_axis - x * m.y_axis.dot(x)).normalize_or_zero();
                s.0 = Mat3::from_cols(x, y, x.cross(y));
            } else if let Some(k) = t.scale {
                s.0 = s.0 * (1.0 - w) + b.0 * Mat3::from_diagonal(k) * w;
            }
            if let Some(at) = t.place {
                s.1 = s.1 * (1.0 - w) + at * w;
            }
        }
        self.shown = Some(shown);
    }

    /// Pose the face nodes of `pose` (node world matrices) as shown (a turned node's scale, the eyes', as it is).
    pub fn apply(&self, p: &Poses, character: &Character, pose: &mut [Mat4]) {
        let Some(shown) = &self.shown else { return };
        let current = &p.poses[(self.pose.max(0) as usize).min(p.poses.len() - 1)];
        for ((&n, s), t) in p.nodes.iter().zip(shown).zip(current) {
            let turn = match (t.turn, t.scale) {
                (Some(_), Some(k)) => s.0 * Mat3::from_diagonal(k),
                _ => s.0,
            };
            let Some(parent) = character.nodes.get(n).and_then(|x| x.parent) else { continue };
            if let (Some(&up), true) = (pose.get(parent), n < pose.len()) {
                let mut local = Mat4::from_mat3(turn);
                local.w_axis = s.1.extend(1.0);
                pose[n] = up * local;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poses() -> Poses {
        let open = Target { turn: Some(Mat3::IDENTITY), scale: None, place: Some(Vec3::ZERO) };
        let shut = Target { turn: Some(Mat3::from_rotation_z(1.0)), scale: None, place: Some(Vec3::X) };
        Poses { nodes: vec![0], bind: vec![(Mat3::IDENTITY, Vec3::ZERO)], poses: vec![vec![open], vec![open], vec![shut]], entry: FaceEntry { file: String::new(), blink: 2, moving: 1, hit: 3 } }
    }

    /// A blink: the first frame sets the blink (blend 1: half way, then all), held 3 frames, then back open for
    /// (rand & 0xFE) + 1 frames.
    #[test]
    fn blinks() {
        let p = poses();
        let mut f = Face::default();
        let mut rand = || 9;
        let place = |f: &Face| f.shown.as_ref().unwrap()[0].1.x;
        let mut seen = vec![];
        for _ in 0..16 {
            f.step(&p, Cues::default(), std::iter::empty(), &mut rand);
            seen.push(place(&f));
        }
        assert_eq!(seen[..6], [0.5, 1.0, 1.0, 0.5, 0.0, 0.0], "{seen:?}");
        // open again: 9 & 0xFE + 1 = 9 frames, then the next blink
        assert!(seen[6..12].iter().all(|&x| x == 0.0) && seen[12] == 0.5, "{seen:?}");
        // the move's pose while its flag is on, back to open (pose 0) once off
        f.step(&p, Cues { moving: true, hit: false }, std::iter::empty(), &mut rand);
        assert_eq!(f.pose, 1);
        f.step(&p, Cues::default(), std::iter::empty(), &mut rand);
        assert_eq!(f.pose, 0);
        // a face op: kind 12 is face 2, held its argument
        f.step(&p, Cues::default(), [(HAND_OPS + 12, 40)].into_iter(), &mut rand);
        assert_eq!((f.pose, f.timer), (2, 40));
    }
}
