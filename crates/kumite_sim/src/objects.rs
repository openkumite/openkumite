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
//!
//! shortcut: Ogre's breath (byte 0x18, object 6) and bytes 0x19 / 0x1C (object 10) start nothing yet; port them here.

use glam::{Mat4, Vec3};

/// Gon's fire's and gas's descriptor bytes.
pub const FIRE: u32 = 0x1A;
pub const GAS: u32 = 0x1B;
/// The fire's start and its heading's second point in the head's axes (0x80098820), the gas's in the pelvis's
/// (0x80098840): the bones' axes (x along the facing, y down), turned onto the drawn joints' (x, −y, −z).
const FIRE_AT: [Vec3; 2] = [Vec3::new(300.0, 0.0, -50.0), Vec3::new(800.0, 0.0, -50.0)];
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Fire,
    /// The gas's source: its heading (sine, cosine of the yaw) and the next puff.
    GasSource {
        heading: [f32; 2],
        next: usize,
    },
    Gas,
}

/// One object: its owner (fighter 0 / 1), where it is and was a frame before (game units, world), its travel a frame,
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
}

/// The objects playing and the PsyQ rand state they draw from (0x8007A318).
#[derive(Clone, Debug, Default)]
pub struct Objects {
    pub list: Vec<Object>,
    seed: u32,
    /// Each fighter's move and frame an object last started on (a hit-stop holds the frame: one start a frame).
    pub started: [Option<(usize, u32)>; 2],
}

impl Objects {
    fn rand(&mut self) -> i32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
        ((self.seed >> 16) & 0x7FFF) as i32
    }

    /// Fighter `owner`'s move of descriptor byte `byte` on a frame of its active window (`first`: the window's first
    /// frame) starts its object at bone slots `slots` (head, pelvis: world, game units).
    pub fn start(&mut self, owner: usize, byte: u32, first: bool, slots: &[Mat4; 2]) {
        let (m, at, kind) = match byte {
            FIRE => (slots[0], FIRE_AT, Kind::Fire),
            GAS if first => (slots[1], GAS_AT, Kind::GasSource { heading: [0.0, 1.0], next: 0 }),
            _ => return,
        };
        let (p, q) = (m.transform_point3(at[0]), m.transform_point3(at[1]));
        let d = q - p;
        let kind = match kind {
            Kind::GasSource { next, .. } => Kind::GasSource { heading: glam::Vec2::new(d.x, d.z).normalize_or(glam::Vec2::Y).to_array(), next },
            k => k,
        };
        let vel = if kind == Kind::Fire { d.normalize_or_zero() * FIRE_SPEED } else { Vec3::ZERO };
        self.list.push(Object { kind, owner, pos: p, prev: p, vel, strikes: false, state: 0, count: -1, frames: 0, left: 0, mirror: false });
    }

    /// One frame (0x8006F5C0): `striking[k]` whether fighter k is still in its object's move past the active window's
    /// start (its descriptor byte), `struck[k]` whether fighter k's opponent was struck last frame.
    pub fn step(&mut self, striking: [Option<u32>; 2], struck: [bool; 2]) {
        let mut i = 0;
        // (puffs let out this frame move this frame too: the pool's list runs on to its end)
        while i < self.list.len() {
            let mut o = self.list[i].clone();
            let own = striking.get(o.owner).copied().flatten();
            let on = |byte: u32| own == Some(byte);
            let keep = match o.kind {
                Kind::Fire => {
                    let hit = struck.get(o.owner).copied().unwrap_or(false);
                    self.fire(&mut o, on(FIRE), hit)
                }
                Kind::GasSource { heading, next } => {
                    o.count += 1;
                    if let Some(&[_, far, high]) = GAS_PUFFS.get(next).filter(|p| on(GAS) && o.count as f32 >= p[0]) {
                        let at = Vec3::new(o.pos.x, high, o.pos.z);
                        let vel = Vec3::new(heading[0] * far, 0.0, heading[1] * far);
                        self.list.push(Object { kind: Kind::Gas, owner: o.owner, pos: at, prev: at, vel, strikes: false, state: 0, count: -1, frames: 0, left: 0, mirror: false });
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
            } else {
                self.list.remove(i);
            }
        }
    }

    /// The fire (0x80071E5C): false when it is gone.
    fn fire(&mut self, o: &mut Object, on: bool, struck: bool) -> bool {
        o.strikes &= on;
        // its strike landed: gone (a guarded one ends, 0x800724A4: Gon's fire cannot be guarded)
        if o.strikes && struck && (1..=2).contains(&o.state) {
            return false;
        }
        if o.state == 0 {
            // 0x80072194: strikes, 5-8 looped frames
            o.state = 1;
            let r = self.rand();
            (o.strikes, o.mirror, o.left) = (on, r & 1 != 0, ((r >> 2) & 3) + 5);
        }
        match o.state {
            1 => {
                o.count += 1;
                if o.count >= 7 {
                    (o.state, o.count) = (2, self.rand());
                }
            }
            2 => {
                o.count += 1;
                o.frames += 1;
                if o.frames >= o.left {
                    (o.state, o.count) = (4, -1);
                }
            }
            _ => {
                o.count += 1;
                if o.count >= 9 {
                    return false;
                }
                // (0x800255E0: it strikes for the end's first 3 frames)
                o.strikes &= o.count < 3;
            }
        }
        o.prev = o.pos;
        o.pos += o.vel;
        // low enough: on along the floor, level (0x8006E0D0)
        if o.pos.y < FIRE_FLOOR {
            o.pos.y = FIRE_FLOOR;
            o.vel = Vec3::new(o.vel.x, 0.0, o.vel.z).normalize_or_zero() * FIRE_SPEED;
        }
        true
    }

    /// Fighter `owner`'s strikes this frame: its striking objects' travel.
    pub fn strikes(&self, owner: usize) -> Vec<(Vec3, Vec3)> {
        self.list.iter().filter(|o| o.owner == owner && o.strikes).map(|o| (o.prev, o.pos)).collect()
    }
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
        o.start(0, FIRE, true, &[head, Mat4::IDENTITY]);
        let mut frames = 0;
        let mut floor = false;
        while !o.list.is_empty() && frames < 40 {
            o.step([Some(FIRE), None], [false; 2]);
            frames += 1;
            if let Some(f) = o.list.first() {
                floor |= f.pos.y == FIRE_FLOOR;
                assert!(f.pos.z >= f.prev.z);
            }
        }
        assert!(floor && (20..=26).contains(&frames), "{frames}");
        o.start(1, GAS, true, &[Mat4::IDENTITY, Mat4::from_translation(Vec3::new(0.0, 300.0, 0.0))]);
        for _ in 0..16 {
            o.step([None, Some(GAS)], [false; 2]);
        }
        let puffs: Vec<_> = o.list.iter().filter(|p| p.kind == Kind::Gas).collect();
        assert!(puffs.len() == 8 && puffs.iter().all(|p| p.strikes && p.pos.y > 300.0));
        assert!(!o.strikes(1).is_empty() && o.strikes(0).is_empty());
    }
}
