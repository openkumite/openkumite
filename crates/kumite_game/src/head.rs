//! The head turning to the opponent (EXE 0x80039074, run by the skeleton 0x8003AFCC between the pose solver
//! and the world matrices). While the round is on and the move allows it (move record +0x24 bit 30; an attack
//! only from frame max(+0x2E, +0x18 − 17)), the head's local rotation is replaced by base × Rzyx(angles): the
//! angles head for the midpoint of the opponent's chest and head seen from the chest, pitch within −60°..25°,
//! yaw within ±80°..30° by how far the chest leans, half the way each frame at most 10°; while the opponent stays
//! near the line ahead the aim is renewed only every 60 frames. Taking over from the animation, the angles start
//! from the animated head. Everything here is in game space (y down, game units, 65536 per turn).

use glam::{Mat3, Mat4, Vec3};
use kumite_sim::detmath::DetMath;

/// The head's base rotation (angles 0x8001A0F4: x 90°, z 90°).
const BASE: [i32; 3] = [0x4000, 0, 0x4000];

/// Rz · Ry · Rx of angles in 65536ths of a turn (EXE 0x8003A338, the pose solver's Euler angles).
pub fn rot_zyx(a: [i32; 3]) -> Mat3 {
    let r = |v: i32| v as f32 * std::f32::consts::TAU / 65536.0;
    Mat3::from_rotation_z(r(a[2])) * Mat3::from_rotation_y(r(a[1])) * Mat3::from_rotation_x(r(a[0]))
}

/// atan2(y, x) in 65536ths of a turn on the game's 16-step grid (0x8002A224).
fn ratan2(y: f32, x: f32) -> i32 {
    if x == 0.0 && y == 0.0 {
        return 0;
    }
    let a = (y.datan2(x) * 65536.0 / std::f32::consts::TAU).round() as i32;
    (a as i16 as i32) & !15
}

/// The pose the aim is taken from (game space).
pub struct Frame {
    /// The chest's world rotation (bone slot 1).
    pub chest: Mat3,
    /// The head's world position (slot 2) and its animated local rotation (relative to the chest).
    pub head: Vec3,
    pub head_local: Mat3,
    /// The target: the midpoint of the opponent's chest and head.
    pub target: Vec3,
    /// That character's head does not turn sideways.
    pub no_yaw: bool,
}

/// One fighter's state (0x8009E578 + 20 per player).
#[derive(Clone, Copy, Debug, Default)]
pub struct HeadLook {
    cur: [i32; 3],
    target: [i32; 3],
    active: bool,
    snap: bool,
    timer: u8,
    /// The head's local rotation this frame (game space), or None: the animation's.
    pub local: Option<Mat3>,
}

impl HeadLook {
    /// The angles (x, pitch, yaw) while the head follows.
    pub fn angles(&self) -> Option<[i32; 3]> {
        self.active.then_some(self.cur)
    }

    /// The round start (0x80039034): angles (0, 0x38E, 0), off, and the first aim taken at once.
    pub fn reset(&mut self) {
        self.cur = [0, 0x38E, 0];
        self.active = false;
        self.snap = true;
    }

    /// One frame: `on` whether the head follows this frame; `frame` the pose to aim from.
    pub fn step(&mut self, on: bool, frame: impl FnOnce() -> Frame) {
        let started = on && !self.active;
        self.active = on;
        if !on {
            self.local = None;
            return;
        }
        let f = frame();
        // the target in the chest's frame (0x8004B218: chestᵀ · d), and the chest's x axis in the world
        let d = f.target - f.head;
        let l = f.chest.transpose() * d;
        let w = f.chest.x_axis;
        let mut pitch = -ratan2(l.x, (l.y * l.y + l.z * l.z).sqrt());
        pitch = pitch.clamp(-0x2AAA, 0x11C7);
        let mut yaw = if f.no_yaw { 0 } else { ratan2(l.z, l.y) };
        // the sideways limit: 80° with the chest upright down to 30° bent over
        let lean = ratan2(w.y, (w.x * w.x + w.z * w.z).sqrt());
        let a0 = lean + 0x4000;
        let limit = if a0 < 0xAAB {
            0x38E3
        } else if a0 >= 0x38E3 {
            0x1555
        } else {
            0x38E3 - ((lean + 0x3556) as i64 * 9102 * 9 / 26 / 4096) as i32
        };
        yaw = yaw.clamp(-limit, limit);
        // renewed at once when taking over or when the opponent is off the line ahead, else every 60 frames
        let renew = if started || l.y.abs() >= 400.0 || l.z.abs() >= 300.0 {
            self.timer = 0;
            true
        } else {
            self.timer += 1;
            if self.timer >= 60 {
                self.timer = 0;
                true
            } else {
                false
            }
        };
        if renew {
            self.target = [0, pitch, yaw];
        }
        if self.snap {
            self.cur = self.target;
            self.snap = false;
        } else if started {
            self.cur = decompose(f.head_local);
        } else {
            let approach = |cur: i32, to: i32| {
                let diff = to - cur;
                let step = diff - diff / 2;
                cur + step.clamp(-0x71C, 0x71C)
            };
            self.cur = [self.target[0], approach(self.cur[1], self.target[1]) as i16 as i32, approach(self.cur[2], self.target[2]) as i16 as i32];
        }
        self.local = Some(rot_zyx(BASE) * rot_zyx(self.cur));
    }
}

/// Tekken Tag's look (0x351E98, called by the drawing 0x3478F0 once the world matrices are made: it changes only
/// what is drawn, never the bone slots the strikes and spheres use). The neck (node 5) and the head (node 6) each
/// turn towards the opponent's neck (0x351E78: its node 5), seen from eye height (the neck's origin raised by the
/// first eye's height in the neck's frame): yaw within ±40°, pitch within −32.5°..15°, an eighth of the way a frame;
/// the pitch only moves while the yaw is within 5° of where it was and the aim is 5° or more off. Past ±160° (behind)
/// the yaw stays on the side it went round. The eyes (nodes 21, 22) then look straight at it, sideways within ±35°,
/// up and down within −7°..9°. On (fp) while the round is on or the K.O. time runs (gp-0x7268 1 or 2) and the move
/// allows it (record +0x24 bit 22; an attack only from frame max(+0x2D, +0x18 − 17)); off, the angles follow a point
/// 1 m ahead of the face at once, and the neck and head are still written from them. State per side
/// (gp-0x6100..0x60D8, radians), reset with the fighter (0x351E10). Node indices are the PS2 model's (every
/// fighter's first 98 nodes are the same).
#[derive(Clone, Copy, Debug, Default)]
pub struct TagLook {
    /// Neck's and head's last (yaw, pitch) and the side the aim went behind on (0, 3 = past +160°, 4 = past −160°).
    saved: [[f32; 2]; 2],
    behind: [u8; 2],
    /// The local rotations of nodes 5, 6, 21 and 22 this frame, or None: the motion's.
    pub locals: Option<[Mat3; 4]>,
}

/// The nodes the look turns, and the neck's parent.
pub const TAG_LOOK_NODES: [usize; 4] = [5, 6, 21, 22];
const TAG_NECK_PARENT: usize = 2;

impl TagLook {
    /// Whether `parent` (node → its parent) is the PS2 fighter skeleton the look's node numbers are for.
    pub fn fits(parent: impl Fn(usize) -> Option<usize>) -> bool {
        let [neck, head, a, b] = TAG_LOOK_NODES;
        parent(neck) == Some(TAG_NECK_PARENT) && parent(head) == Some(neck) && parent(a) == Some(head) && parent(b) == Some(head)
    }

    /// One frame: `w` the animated world matrices (a skeleton that [`TagLook::fits`]), `target` the opponent's neck.
    pub fn step(&mut self, on: bool, w: &[Mat4], target: Vec3) {
        let [neck, head, eye_a, eye_b] = TAG_LOOK_NODES;
        let k = if on { 0.125 } else { 1.0 };
        let p = w[TAG_NECK_PARENT];
        let (l5, l6) = (p.inverse() * w[neck], w[neck].inverse() * w[head]);
        let rise = w[neck].inverse().transform_point3(w[eye_a].w_axis.truncate()).z;
        let target = if on { target } else { w[head].transform_point3(Vec3::new(1.0, 0.0, 0.08)) };
        // the neck: Rx(yaw) · Rz(pitch) · its base (0x349EE0 of 90°, 90°, 0), the motion's offset kept
        let eye_level = |mut l: Mat4| {
            l.w_axis.x += rise;
            p * l
        };
        // (aimed from the base, not the motion's neck: a stance's turned neck measures 40° on the PS2, as from the base)
        let base = Mat3::from_cols(Vec3::Y, Vec3::Z, Vec3::X);
        let (yaw, pitch) = self.aim(0, on, k, eye_level(Mat4::from_mat3_translation(base, l5.w_axis.truncate())), target);
        let r5 = Mat3::from_rotation_x(yaw) * Mat3::from_rotation_z(pitch) * base;
        let l5 = Mat4::from_mat3_translation(r5, l5.w_axis.truncate());
        // the head, from the turned neck: Rz(yaw) · Ry(pitch) (0x349D70 of 0, pitch, yaw)
        let (yaw, pitch) = self.aim(1, on, k, eye_level(l5), target);
        let r6 = Mat3::from_rotation_z(yaw) * Mat3::from_rotation_y(pitch);
        if !on {
            // (off, the neck and head are written all the same, aimed at once at where the face looks, 0x352238; the
            // eyes keep the motion's)
            let keep = |n: usize| Mat3::from_mat4(w[head].inverse() * w[n]);
            self.locals = Some([r5, r6, keep(eye_a), keep(eye_b)]);
            return;
        }
        let w6 = p * l5 * Mat4::from_mat3_translation(r6, l6.w_axis.truncate());
        // each eye: Rz(sideways) · Ry(up) · Rx(90°), aimed from where the turned head puts it
        let eye = |n: usize| {
            let e = w6 * (w[head].inverse() * w[n]);
            let d = Mat3::from_mat4(e).transpose() * (target - e.w_axis.truncate());
            let up = (-d.y.datan2(d.x)).clamp(-7f32.to_radians(), 9f32.to_radians());
            let side = (-d.z).datan2((d.x * d.x + d.y * d.y).sqrt()).clamp(-35f32.to_radians(), 35f32.to_radians());
            Mat3::from_rotation_z(side) * Mat3::from_rotation_y(up) * Mat3::from_rotation_x(std::f32::consts::FRAC_PI_2)
        };
        self.locals = Some([r5, r6, eye(eye_a), eye(eye_b)]);
    }

    /// The neck's (`s` 0) or the head's (1) yaw and pitch towards `target` seen from `from`, eased by `k`.
    fn aim(&mut self, s: usize, on: bool, k: f32, from: Mat4, target: Vec3) -> (f32, f32) {
        let (deg, [was_yaw, was_pitch], was) = (f32::to_radians, self.saved[s], self.behind[s]);
        let d = Mat3::from_mat4(from).transpose() * (target - from.w_axis.truncate());
        let a = d.y.datan2(d.x);
        let behind = if a < -deg(160.0) {
            4
        } else if a > deg(160.0) {
            3
        } else {
            0
        };
        let (mut yaw, mut pitch) = if (behind, was) == (3, 4) || (behind, was) == (4, 3) { (was_yaw, was_pitch) } else { (a, (-d.z).datan2((d.x * d.x + d.y * d.y).sqrt())) };
        if behind != 0 {
            pitch = 0.0;
            if behind == was {
                yaw = if behind == 3 { deg(40.0) } else { -deg(40.0) };
            }
        }
        yaw = was_yaw + (yaw.clamp(-deg(40.0), deg(40.0)) - was_yaw) * k;
        if !on || ((yaw - was_yaw).abs() <= deg(5.0) && (pitch - was_pitch).abs() >= deg(5.0)) {
            pitch = was_pitch + (pitch.clamp(-deg(32.5), deg(15.0)) - was_pitch) * k;
        } else {
            pitch = was_pitch;
        }
        self.saved[s] = [yaw, pitch];
        if behind == 0 || was == 0 {
            self.behind[s] = behind;
        }
        (yaw, pitch)
    }
}

/// Set node `n`'s local rotation to `rot` (its offset kept), carrying everything hanging from it along.
pub fn set_local(nodes: &[kumite_assets::Node], pose: &mut [Mat4], n: usize, rot: Mat3) {
    let Some(&old) = pose.get(n) else { return };
    let parent = match nodes.get(n).and_then(|x| x.parent) {
        Some(p) => match pose.get(p) {
            Some(&m) => m,
            None => return,
        },
        None => Mat4::IDENTITY,
    };
    let new = parent * Mat4::from_mat3_translation(rot, (parent.inverse() * old).w_axis.truncate());
    let delta = new * old.inverse();
    for (i, m) in pose.iter_mut().enumerate() {
        let mut at = Some(i);
        while let Some(a) = at {
            if a == n {
                *m = if i == n { new } else { delta * *m };
                break;
            }
            at = nodes.get(a).and_then(|x| x.parent);
        }
    }
}

/// The angles whose base × Rzyx gives `h` (0x800394AC): from its x and z columns.
fn decompose(h: Mat3) -> [i32; 3] {
    let (mut b0, mut b1) = (h.x_axis, h.z_axis);
    let rad = |a: i32| a as f32 * std::f32::consts::TAU / 65536.0;
    let tz = ratan2(b0.z, b0.y);
    let (s, c) = rad(-tz).dsin_cos();
    (b0.y, b0.z) = (b0.y * c - b0.z * s, b0.y * s + b0.z * c);
    (b1.y, b1.z) = (b1.y * c - b1.z * s, b1.y * s + b1.z * c);
    let ty = (ratan2(b0.y, b0.x) - 0x4000) as i16 as i32;
    let (s, c) = rad(-ty).dsin_cos();
    b1.x = b1.x * c - b1.y * s;
    let tx = (ratan2(b1.x, b1.z) - 0x4000) as i16 as i32;
    [tx, ty, tz]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_inverts_base_times_euler() {
        for a in [[100, -2000, 3000], [-5000, 4000, -1000], [0, 0x38E, 0], [6000, -6000, 2000]] {
            let h = rot_zyx(BASE) * rot_zyx(a);
            let back = rot_zyx(BASE) * rot_zyx(decompose(h));
            assert!((h - back).to_cols_array().iter().all(|v| v.abs() < 0.01), "{a:?}");
        }
    }

    #[test]
    fn turns_half_way_capped() {
        let mut h = HeadLook::default();
        // an upright chest: its x axis (the spine) points up (−y in game space), y ahead (+x)
        let chest = Mat3::from_cols(Vec3::new(0.0, -1.0, 0.0), Vec3::X, Vec3::Z);
        let frame = |target: Vec3| Frame { chest, head: Vec3::ZERO, head_local: rot_zyx(BASE), target, no_yaw: false };
        // taking over: the animated head (angles 0); then towards an opponent off to the side (+z, yaw 90° → 80°)
        h.step(true, || frame(Vec3::new(1.0, 0.0, 2000.0)));
        assert_eq!(h.cur, [0, 0, 0]);
        h.step(true, || frame(Vec3::new(1.0, 0.0, 2000.0)));
        assert_eq!(h.target[2], 0x38E3);
        assert_eq!(h.cur[2], 0x71C, "at most 10° a frame");
        for _ in 0..40 {
            h.step(true, || frame(Vec3::new(1.0, 0.0, 2000.0)));
        }
        assert!((h.cur[2] - 0x38E3).abs() <= 16);
        h.step(false, || frame(Vec3::ZERO));
        assert!(h.local.is_none());
    }
}
