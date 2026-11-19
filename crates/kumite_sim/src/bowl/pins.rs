//! The pins as the PS2 moves them (0x3EED88's calls; the Tekken Tag converter's docs/BOWL_ENDINGS.md): each pin a
//! centre, a tilt and a heading, four spheres along its axis and its base; a frame moves it whole, then finds the
//! first touch within the frame (the ball's, another pin's, the lane's, the masking's), steps back to it, pushes and
//! goes on for the rest of the frame. Velocities are per frame; y is down.

use super::Ball;
use crate::detmath::DetMath;
use glam::{Mat3, Vec3};

/// The four spheres along the axis from the centre (0x46BC20: y; 0x523570 radii, 0x523580 their share of the
/// mass), the base's centre (0x46BC60), the pin's mass (gp-0x7668) and its turning (gp-0x765C).
const POINTS: [f32; 4] = [-249.342_1, -99.342_1, 100.657_9, 225.657_9];
const RADII: [f32; 4] = [80.0, 60.0, 100.0, 75.0];
const SHARES: [f32; 4] = [1.0, 0.6, 1.5, 0.7];
const BASE: f32 = 250.657_9;
const MASS: f32 = 3.8;
const TURNING: f32 = 6.258_214e-6;
/// The ball against them (0x3EDC90): its radius and mass.
const BALL_RADIUS: f32 = 150.0;
const BALL_MASS: f32 = 30.0;
/// Gravity a frame and its half and quarter (gp-0x7638 ..).
const G: f32 = 9.8;
const G2: f32 = 4.9;
const G4: f32 = 2.45;
/// The base's rim (its radius squared, gp-0x7620), the lying test's angle (asin of gp-0x7654 past a right angle,
/// 0.005 either side), the floor's push (gp-0x7608, gp-0x760C, gp-0x7604, gp-0x7600), the slides' and the turns'
/// friction (0x3ED338).
const RIM2: f32 = 5000.0;
const LYING: f32 = 0.057_142_857;
const SLACK: f32 = 0.005;
const FLOOR_PUSH: f32 = 1.8;
const LYING_PUSH: f32 = -6.84;
const HOLD: f32 = -18.62;
const HOLD_SHARE: f32 = 0.8;
const SLIDE: f32 = 0.25;
const SLIDE_UPRIGHT: f32 = 0.75;
const TURN: f32 = 0.0008;
/// The masking over the deck and the pit's sides (0x3EE598): from z 9585 to 12585, x +-2700, the masking from y -3528
/// to -1176 (the floor 0); the bounce 0.4; only pins past z 9255.66 meet them.
const PIT: [f32; 2] = [9585.0, 12585.0];
const PIT_X: f32 = 2700.0;
const MASKING: [f32; 2] = [-3528.0, -1176.0];
const PIT_BOUNCE: f32 = 0.4;
const PIT_FROM: f32 = 9255.658;
const SQRT_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// A pin (0x1FC5260 + 0x180 each).
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// +0: the centre; +0x10 its velocity; +0x20 the frame's start; +0x30 the frame's move.
    pub pos: Vec3,
    pub vel: Vec3,
    prev: Vec3,
    disp: Vec3,
    /// +0x40: the tilt from upright and the heading (radians); +0x50 their speeds (and the third axis'); +0x60 at
    /// the frame's start; +0x70 the frame's change.
    pub angles: [f32; 2],
    pub rate: Vec3,
    prev_angles: [f32; 2],
    delta: [f32; 2],
    /// +0x80: the spheres' centres and the base (+0x140); at the frame's start (+0xC0, +0x150); their moves (+0x100,
    /// +0x160).
    points: [Vec3; 5],
    prev_points: [Vec3; 5],
    point_disp: [Vec3; 5],
    /// +0x170 standing on its base, +0x174 on the deck (not swept), +0x178 not touched yet.
    pub standing: bool,
    pub on_deck: bool,
    pub untouched: bool,
    /// +0x17E: the clatter timer a touch sets (+0x17C).
    pub clatter: u32,
}

/// `a` within (-pi, pi] (0x34A570).
fn wrap(a: f32) -> f32 {
    let (pi, tau) = (std::f32::consts::PI, std::f32::consts::TAU);
    let r = libm::fmodf(a, tau);
    if r > pi {
        r - tau
    } else if r < -pi {
        r + tau
    } else {
        r
    }
}

/// asin, NaN-free (the PS2's arguments stay within [-1, 1]; rounding could step out).
fn asin(x: f32) -> f32 {
    x.clamp(-1.0, 1.0).dasin()
}

/// The first time in [0, `limit`) two spheres `r` apart (their centres `from` apart at the start, moving `d`
/// apart in the frame) touch (0x3EDB98): 0 when they already do, `limit` when they don't.
fn touch(d: Vec3, from: Vec3, r: f32, limit: f32) -> f32 {
    let c = from.dot(from);
    if c == 0.0 {
        return limit;
    }
    let c = c - r * r;
    if c <= 0.0 {
        return 0.0;
    }
    let a = d.dot(d);
    if a == 0.0 {
        return limit;
    }
    let b = d.dot(from);
    if b >= 0.0 {
        return limit;
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return limit;
    }
    ((-b - disc.sqrt()) / a).min(limit)
}

impl Pin {
    /// A pin standing on its spot (0x3EA050).
    pub fn racked(spot: [f32; 2], y: f32) -> Self {
        let pos = Vec3::new(spot[0], y, spot[1]);
        let mut p = Self {
            pos,
            vel: Vec3::ZERO,
            prev: pos,
            disp: Vec3::ZERO,
            angles: [0.0; 2],
            rate: Vec3::ZERO,
            prev_angles: [0.0; 2],
            delta: [0.0; 2],
            points: [Vec3::ZERO; 5],
            prev_points: [Vec3::ZERO; 5],
            point_disp: [Vec3::ZERO; 5],
            standing: true,
            on_deck: true,
            untouched: true,
            clatter: 0,
        };
        p.update();
        p.prev_points = p.points;
        p.update();
        p
    }

    /// Its axis from the centre to the base (the matrix' second row: 0x3E9120 turns the tilt about x, then the
    /// heading about y).
    fn axis(&self) -> Vec3 {
        let (sx, cx) = wrap(self.angles[0]).dsin_cos();
        let (sy, cy) = wrap(self.angles[1]).dsin_cos();
        Vec3::new(sy * sx, cx, cy * sx)
    }

    /// Its orientation (the model's axes in the world: the matrix' rows as columns).
    pub fn rot(&self) -> Mat3 {
        let (sx, cx) = wrap(self.angles[0]).dsin_cos();
        let (sy, cy) = wrap(self.angles[1]).dsin_cos();
        Mat3::from_cols(Vec3::new(cy, 0.0, -sy), Vec3::new(sy * sx, cx, cy * sx), Vec3::new(sy * cx, -sx, cy * cx))
    }

    /// Its base's centre (the centre stands `base` above it).
    pub fn base(&self, base: f32) -> Vec3 {
        self.pos + self.axis() * base
    }

    /// Moving at all (0x3EF3A8: any speed not exactly 0).
    pub fn moving(&self) -> bool {
        self.vel != Vec3::ZERO || self.rate != Vec3::ZERO
    }

    /// 0x3ECAD8: the frame's moves, the spheres and the base where the pin is now.
    fn update(&mut self) {
        self.disp = self.pos - self.prev;
        self.delta = [self.angles[0] - self.prev_angles[0], self.angles[1] - self.prev_angles[1]];
        let u = self.axis();
        for (k, h) in POINTS.iter().chain([&BASE]).enumerate() {
            if let (Some(p), Some(q), Some(d)) = (self.points.get_mut(k), self.prev_points.get(k), self.point_disp.get_mut(k)) {
                *p = self.pos + u * *h;
                *d = *p - *q;
            }
        }
    }

    /// 0x3EC9A0: a new frame: the start kept, the centre moved by its velocity, the angles by their speeds.
    pub fn begin(&mut self) {
        self.prev_points = self.points;
        self.prev = self.pos;
        self.pos += self.vel;
        self.prev_angles = self.angles;
        self.turn(self.rate);
        self.update();
    }

    /// 0x3EC388: the angles turned by `w` (about x, y, z): the heading by y; the tilt by the turn across the
    /// heading, the heading again by the turn along it.
    // shortcut: the twist about the pin's own axis (+0x4C, drawn only) is not kept; a pin turns without it
    fn turn(&mut self, w: Vec3) {
        let [ax, ay] = [wrap(self.angles[0]), wrap(self.angles[1])];
        self.angles[1] += w.y;
        let cy = ay.dcos();
        let sy = (1.0 - cy * cy).sqrt() * if ay < 0.0 { -1.0 } else { 1.0 };
        let (swx, swz) = (w.x.dsin(), w.z.dsin());
        let across = swx * cy - swz * sy;
        let along = swz * cy + swx * sy;
        let tilt = asin(across);
        self.angles[0] += tilt;
        let cx = ax.dcos();
        let sx = (1.0 - cx * cx).sqrt() * if ax < 0.0 { -1.0 } else { 1.0 };
        let (f24, f22) = (sx * along, cx * along);
        let mut f20 = ax + tilt;
        let pi = std::f32::consts::PI;
        if f20 > pi {
            f20 -= std::f32::consts::TAU;
        } else if f20 < -pi {
            f20 += std::f32::consts::TAU;
        }
        self.angles[0] += asin(f24 * f22);
        if cx != 0.0 && along != 0.0 {
            let s = f20.dsin();
            self.angles[1] += if f20 >= 0.0 { -f22.datan2(s) } else { f22.datan2(-s) };
        }
    }

    /// 0x3ECA38: back to time `t` of the frame (its start plus that much of its move).
    fn back_to(&mut self, t: f32) {
        self.pos = self.prev + self.disp * t;
        self.angles = [self.prev_angles[0] + self.delta[0] * t, self.prev_angles[1] + self.delta[1] * t];
    }

    /// 0x3ECA80: on by `t` of a frame at its velocities.
    fn on_by(&mut self, t: f32) {
        self.pos += self.vel * t;
        self.turn(self.rate * t);
    }

    /// 0x3ECBF0: sphere `k`'s move at time `t`: its own plus the centre's along the axis, by its share of the mass.
    fn point_move(&self, k: usize, t: f32) -> Vec3 {
        let start = self.prev_points[0] - self.prev_points[4];
        let axis = start + ((self.points[0] - self.points[4]) - start) * t;
        let w = SHARES[k];
        let f = axis.dot(self.disp) * (MASS - w) / (axis.dot(axis) * w);
        self.point_disp[k] + axis * f
    }

    /// 0x3ECD00: the mass the floor meets at sphere `k` at time `t`: its share, more as the axis stands up.
    fn floor_mass(&self, k: usize, t: f32) -> f32 {
        let start = self.prev_points[0] - self.prev_points[4];
        let axis = start + ((self.points[0] - self.points[4]) - start) * t;
        let up = (axis.y * axis.y / axis.dot(axis)).sqrt();
        (MASS - SHARES[k]) * up + SHARES[k]
    }

    /// 0x3ECDE8: an impulse `j` at `at`: its part along the lever moves the pin (its velocity becomes the frame's
    /// move plus that over the mass), the rest turns it.
    fn push(&mut self, j: Vec3, at: Vec3) {
        let jj = j.dot(j);
        if jj <= 0.0 {
            return;
        }
        let r = at - self.pos;
        let rr = r.dot(r);
        let along = if rr == 0.0 {
            j
        } else {
            let d = j.dot(r);
            j * (d * d / (jj * rr))
        };
        self.vel = self.disp + along * (1.0 / MASS);
        let rest = (j - along) * TURNING;
        self.rate += r.cross(rest);
    }

    /// The touch flags (+0x178 cleared, +0x17C set: the clatter).
    fn touched(&mut self) {
        self.untouched = false;
        self.clatter = 5;
    }
}

/// 0x3EDC90: the ball against pin `p` within the frame: the first of its spheres the ball meets; both back to then,
/// the ball's velocity bounced off it, the pin pushed; both on for the rest of the frame.
pub fn ball_hit(ball: &mut Ball, p: &mut Pin) {
    let mut best = (1.0, None);
    for k in 0..4 {
        let t = touch(ball.disp - p.point_disp[k], ball.prev - p.prev_points[k], RADII[k] + BALL_RADIUS, best.0);
        if t < best.0 {
            best = (t, Some(k));
        }
    }
    let (t, Some(k)) = best else { return };
    ball.pos = ball.prev + ball.disp * t;
    p.back_to(t);
    let c = p.prev_points[k] + p.point_disp[k] * t;
    let n = ball.pos - c;
    let at = c + n * (RADII[k] / (RADII[k] + BALL_RADIUS));
    let v = p.point_move(k, t) - ball.disp;
    let vn = v.dot(n);
    if vn > 0.0 {
        let w = SHARES[k];
        let j = 2.0 * vn * w / ((w + BALL_MASS) * n.dot(n));
        p.touched();
        ball.vel = ball.disp + n * j;
        ball.vel.y = 0.0;
        p.push(n * (-j * BALL_MASS), at);
    }
    let rest = 1.0 - t;
    ball.pos += ball.vel * rest;
    p.on_by(rest);
    ball.disp = ball.pos - ball.prev;
    p.update();
}

/// 0x3EDF88: pins `p` and `q` within the frame: their first spheres to meet; both back to then, pushed apart by
/// their spheres' shares, on for the rest of the frame.
pub fn pin_hit(p: &mut Pin, q: &mut Pin) {
    let mut best = (1.0, None);
    for i in 0..4 {
        for k in 0..4 {
            let t = touch(p.point_disp[i] - q.point_disp[k], p.prev_points[i] - q.prev_points[k], RADII[i] + RADII[k], best.0);
            if t < best.0 {
                best = (t, Some((i, k)));
            }
        }
    }
    let (t, Some((i, k))) = best else { return };
    p.back_to(t);
    q.back_to(t);
    let cp = p.prev_points[i] + p.point_disp[i] * t;
    let cq = q.prev_points[k] + q.point_disp[k] * t;
    let n = cp - cq;
    let at = cq + n * (RADII[k] / (RADII[i] + RADII[k]));
    let v = q.point_move(k, t) - p.point_move(i, t);
    let vn = v.dot(n);
    if vn > 0.0 {
        let (wi, wk) = (SHARES[i], SHARES[k]);
        let j = 2.0 * vn * wi * wk / ((wi + wk) * n.dot(n));
        p.touched();
        q.touched();
        p.push(n * j, at);
        q.push(n * -j, at);
    }
    let rest = 1.0 - t;
    p.on_by(rest);
    q.on_by(rest);
    p.update();
    q.update();
}

/// 0x3EEBF8: OVER CHARGED's runner (a sphere at `at`, `radius`) against pin `p`: each sphere it overlaps pushes the
/// pin out of it and away by 150.
// shortcut: the runner is one sphere here; the PS2 takes the fourteen of its body (+0x1F0), whose data isn't converted
pub fn body_hit(at: Vec3, radius: f32, p: &mut Pin) {
    for k in 0..4 {
        let mut d = p.points[k] - at;
        let dist = d.length();
        let reach = RADII[k] + radius;
        if dist < reach && dist > 0.0 {
            p.pos += d * ((reach - dist) / dist);
            p.untouched = false;
            d *= 150.0 / dist;
            let point = p.points[k];
            p.push(d, point);
            p.update();
        }
    }
}

/// 0x3ED268: sphere `k`'s depth into the floor (k 4: the belly's) after `rest` more of the frame and its fall.
fn depth_after(p: &Pin, k: usize, rest: f32) -> f32 {
    let mut c = p.clone();
    c.on_by(rest);
    c.pos.y += G4 * rest * rest;
    c.update();
    let k = if k == 4 { 2 } else { k };
    c.points[k].y + RADII[k]
}

/// The lying test (0x3ED118, 0x3ED338): the tilt past a right angle by asin(gp-0x7654), less `slack`.
fn lying(tilt: f32, slack: f32) -> bool {
    let a = LYING.dasin() + std::f32::consts::FRAC_PI_2;
    a - slack < tilt || tilt < -a + slack
}

/// 0x3ED118: after `rest` more of the frame, the pin rubs the floor (its belly as it lies, its head short of
/// upside down).
fn rubs(p: &Pin, k: usize, rest: f32) -> bool {
    let mut c = p.clone();
    c.on_by(rest);
    let tilt = c.angles[0];
    let a = LYING.dasin() + std::f32::consts::FRAC_PI_2;
    match k {
        2 => lying(tilt, SLACK),
        0 => -a - SLACK < tilt && tilt < a + SLACK,
        _ => false,
    }
}

/// The lowest point of a base of axis `axis` (top minus base) and centre `base`: on its rim, `rim` of the axis'
/// length out (0x3ED338).
fn rim_low(base: Vec3, axis: Vec3, rim: f32) -> Vec3 {
    let h = axis.x * axis.x + axis.z * axis.z;
    if h <= 0.0 {
        return base;
    }
    let h = h.sqrt();
    base + Vec3::new(-axis.x * axis.y / h, h, -axis.z * axis.y / h) * rim
}

/// 0x3ED338: the lane under pin `p`: the first sphere (or its base's rim, or its belly as it lies) to reach the
/// floor within the frame; back to then under gravity, the floor's push, the friction of a pin lying or standing,
/// on for the rest; then out of the floor (0x3ECF70). Not on its base: no longer standing.
pub fn lane(p: &mut Pin) {
    let mut best = (1.0, None);
    for k in 0..4 {
        let y = p.prev_points[k].y + RADII[k];
        let t = if y >= 0.0 {
            0.0
        } else {
            let d = p.point_disp[k].y;
            ((d * d - y * G).sqrt() - d) / G2
        };
        if t < 1.0 && t <= best.0 {
            // (the belly first: as the pin lies, the body's whole length)
            let lies = k == 2 && lying(p.prev_angles[0] + p.delta[0] * t, SLACK);
            best = (t, Some(if lies { 4 } else { k }));
        }
    }
    let (mut t, Some(k)) = best else {
        p.pos.y += G4;
        p.vel.y += G2;
        p.update();
        return;
    };
    let mut on_base = false;
    let (mut at, mut v) = if k == 4 {
        (p.prev + p.disp * t, p.disp)
    } else {
        let mut at = p.prev_points[k] + p.point_disp[k] * t;
        at.y += RADII[k];
        if k == 3 {
            let base = p.prev_points[4] + p.point_disp[4] * t;
            if (at - base).length_squared() < RIM2 {
                let axis = p.prev_points[0] - p.prev_points[4];
                on_base = true;
                let rim = (RIM2 / axis.dot(axis)).sqrt();
                at = rim_low(p.prev_points[4], axis, rim);
                if at.y <= 0.0 {
                    let now = rim_low(p.points[4], p.points[0] - p.points[4], rim) - at;
                    t = ((now.y * now.y - at.y * G).sqrt() - now.y) / G2;
                    if t >= 1.0 {
                        p.pos.y += G4;
                        p.vel.y += G2;
                        p.update();
                        return;
                    }
                    at += now * t;
                }
            }
        }
        (at, p.point_disp[k])
    };
    p.back_to(t);
    v.y += G2 * t;
    at.y += G4 * t * t;
    p.pos.y += G4 * t * t;
    p.vel.y += G2 * t;
    if v.y > 0.0 {
        let y = if k == 4 { v.y * LYING_PUSH } else { v.y * (-p.floor_mass(k, t) * FLOOR_PUSH) };
        p.push(Vec3::new(0.0, y, 0.0), at);
    }
    let rest = 1.0 - t;
    if depth_after(p, k, rest) > 0.0 {
        let y = if k == 4 { rest * HOLD * HOLD_SHARE } else { -p.floor_mass(k, 1.0 - rest) * G2 * rest * HOLD_SHARE };
        p.push(Vec3::new(0.0, y, 0.0), at);
        p.vel.y = 0.0;
        if k == 4 || rubs(p, k, rest) {
            slide(p, rest, SLIDE);
            let r = (p.rate.x * p.rate.x + p.rate.z * p.rate.z).sqrt();
            let f = 1.0 - rest * TURN / r;
            if f <= 0.0 {
                p.rate = Vec3::ZERO;
            } else {
                p.rate *= f;
            }
        } else if on_base && -SLACK < p.angles[0] && p.angles[0] < SLACK {
            slide(p, rest, SLIDE_UPRIGHT);
            let f = 1.0 - rest * TURN / p.rate.length();
            if f <= 0.0 {
                p.rate = Vec3::ZERO;
                p.angles[0] = 0.0;
            } else {
                p.rate *= f;
            }
        }
    }
    p.on_by(rest);
    p.pos.y += G4 * rest * rest;
    p.vel.y += G2 * rest;
    p.update();
    out_of_floor(p);
    if !on_base {
        p.standing = false;
    }
}

/// The lane's friction on the slide: `by` a frame off the horizontal speed, to a stop.
fn slide(p: &mut Pin, rest: f32, by: f32) {
    let h = (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt();
    let f = 1.0 - rest * by / h;
    if f <= 0.0 {
        (p.vel.x, p.vel.z) = (0.0, 0.0);
    } else {
        (p.vel.x, p.vel.z) = (p.vel.x * f, p.vel.z * f);
    }
}

/// 0x3ECF70: the pin lifted out of the floor by its deepest sphere (the base's: its rim's lowest point).
fn out_of_floor(p: &mut Pin) {
    let mut deepest: f32 = 0.0;
    for k in 0..4 {
        let d = p.points[k].y + RADII[k];
        if d <= deepest {
            continue;
        }
        if k != 3 {
            deepest = d;
            continue;
        }
        let mut low = p.points[3];
        low.y += RADII[3];
        if (low - p.points[4]).length_squared() < RIM2 {
            let a = p.points[0] - p.points[4];
            let rim = p.points[4].y + ((a.x * a.x + a.z * a.z) * RIM2 / a.dot(a)).sqrt();
            deepest = deepest.max(rim);
        } else {
            deepest = d;
        }
    }
    if deepest > 0.0 {
        p.vel.y = 0.0;
        p.pos.y -= deepest;
        p.update();
    }
}

/// 0x3EE598: the masking's front (z 9585) and underside (y -1176), the pit's sides (x -+2700): the first sphere to
/// cross one within its edges (by 0.707 of its radius) puts the pin back and bounces it.
pub fn walls(p: &mut Pin) {
    if p.pos.z < PIT_FROM {
        return;
    }
    // (each wall: the axis, the plane for sphere k, the side it comes from, the other two axes' ranges)
    let cross = |p: &Pin, axis: usize, plane: &dyn Fn(f32) -> f32, from_below: bool, ranges: [(usize, [f32; 2]); 2]| -> Option<f32> {
        let mut best: Option<f32> = None;
        for k in 0..4 {
            let at = plane(RADII[k]);
            let (now, then) = (p.points[k][axis], p.prev_points[k][axis]);
            let (past, before) = (now - at, at - then);
            let crossed = if from_below { past >= 0.0 && before > 0.0 } else { past <= 0.0 && before < 0.0 };
            if !crossed {
                continue;
            }
            let r = RADII[k] * SQRT_HALF;
            let c = p.prev_points[k] + (p.points[k] - p.prev_points[k]) * (before / (before + past));
            if ranges.iter().all(|&(a, [lo, hi])| lo - r < c[a] && c[a] < hi + r) && best.is_none_or(|b| if from_below { past > b } else { past < b }) {
                best = Some(past);
            }
        }
        best
    };
    let pit_y = (1, [MASKING[1], 0.0]);
    let pit_z = (2, PIT);
    if let Some(d) = cross(p, 2, &|r| PIT[0] - r, true, [(0, [-PIT_X, PIT_X]), (1, MASKING)]) {
        p.pos.z -= d;
        if p.vel.z > 0.0 {
            p.vel.z *= -PIT_BOUNCE;
        }
        return p.update();
    }
    if let Some(d) = cross(p, 1, &|r| MASKING[1] + r, false, [(0, [-PIT_X, PIT_X]), pit_z]) {
        p.pos.y -= d;
        if p.vel.y < 0.0 {
            p.vel.y *= -PIT_BOUNCE;
        }
        p.update();
    }
    if let Some(d) = cross(p, 0, &|r| -PIT_X + r, false, [pit_y, pit_z]) {
        p.pos.x -= d;
        if p.vel.x < 0.0 {
            p.vel.x *= -PIT_BOUNCE;
        }
        return p.update();
    }
    if let Some(d) = cross(p, 0, &|r| PIT_X - r, true, [pit_y, pit_z]) {
        p.pos.x -= d;
        if p.vel.x > 0.0 {
            p.vel.x *= -PIT_BOUNCE;
        }
        p.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_racked_pin_stands_still_on_the_lane() {
        let mut p = Pin::racked([0.0, 10000.0], -BASE);
        for _ in 0..120 {
            p.begin();
            lane(&mut p);
            walls(&mut p);
        }
        assert!(p.standing && !p.moving(), "{p:?}");
        assert!((p.pos.y + BASE).abs() < 1e-3 && p.angles[0] == 0.0);
    }

    #[test]
    fn spheres_meeting_head_on() {
        assert_eq!(touch(Vec3::new(0.0, 0.0, -100.0), Vec3::new(0.0, 0.0, 300.0), 250.0, 1.0), 0.5);
        assert_eq!(touch(Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, 300.0), 250.0, 1.0), 1.0);
    }
}
