//! Tekken 3's joint slots in the PS1's integers: the pose solver (0x8003AD10) from a frame's 49 decoded channels to
//! the 18 slots' turns on their parents, and the skeleton (0x8003AFCC) from those to the slots' places in the world
//! (the push and hurt spheres sit there, 0x80041F6C). Matrices are rows of 4096 = 1, as the GTE keeps them.

use kumite_sim::ball::sin12;
use kumite_sim::{gte_sqrt, Turns};

pub type M = [[i32; 3]; 3];

const I: M = [[4096, 0, 0], [0, 4096, 0], [0, 0, 4096]];
/// Each slot's parent (slot 0 hangs on the root's matrix, P+0xF54).
pub const PARENT: [usize; 18] = [0, 0, 1, 1, 3, 4, 5, 1, 7, 8, 9, 0, 11, 12, 13, 11, 15, 16];
/// The slots the ten angle channels turn (0x8001A330).
const ANGLE_SLOTS: [usize; 10] = [0, 1, 11, 2, 3, 7, 6, 10, 14, 17];
/// The solver's own skeleton (0x80096ECC): the chest, the shoulders and the pelvis on their parents, then the upper
/// limbs' starts, the targets measured from them.
const BASE: [[i32; 3]; 8] = [[0, 0, 0], [350, 0, -100], [350, 0, 100], [0, 0, 0], [-140, 0, 0], [-140, 0, 0], [-160, 0, 100], [-160, 0, -100]];
/// 0x80096EAC: slots 2, 3 and 7 turned a quarter about y after their angles.
const QUARTER: M = [[0, 0, -4096], [0, 4096, 0], [4096, 0, 0]];

/// The sine and cosine of an angle's top 12 bits on the 4096 table (0x8001E8C4, `(a >> 3) & 0x1FFE`).
fn sc(a: i32) -> (i32, i32) {
    let k = (a & 0xFFFF) >> 4;
    (sin12(k), sin12(k + 1024))
}

/// A turn's entries as the GTE's s16.
fn s16(v: i32) -> i32 {
    v as i16 as i32
}

/// MulMatrix (0x80082288): a x b, each sum >> 12, kept to the GTE's s16.
fn mul(a: &M, b: &M) -> M {
    std::array::from_fn(|r| std::array::from_fn(|c| ((0..3).map(|k| a[r][k] * b[k][c]).sum::<i32>() >> 12).clamp(-0x8000, 0x7FFF)))
}

/// MVMVA (sf 1, the MAC): a v >> 12.
fn mv(a: &M, v: [i32; 3]) -> [i32; 3] {
    std::array::from_fn(|r| ((0..3).map(|k| i64::from(a[r][k]) * i64::from(v[k])).sum::<i64>() >> 12) as i32)
}

/// 0x8003A338: Rz Ry Rx of the s16 angles, each product of the GPF >> 12 in its order.
fn euler(x: i32, y: i32, z: i32) -> M {
    let ((sx, cx), (sy, cy), (sz, cz)) = (sc(x), sc(y), sc(z));
    let cxcz = (cx * cz) >> 12;
    let (sxcz, sxsz, sxcy, cxsz) = ((sx * cz) >> 12, (sx * sz) >> 12, (sx * cy) >> 12, (cx * sz) >> 12);
    let (a, b, c) = ((sy * sxcz) >> 12, (sy * sxsz) >> 12, (sy * cxcz) >> 12);
    let m = [[(cy * cz) >> 12, a - cxsz, sxsz + c], [(cy * sz) >> 12, cxcz + b, ((cxsz * sy) >> 12) - sxcz], [-sy, sxcy, (cy * cx) >> 12]];
    m.map(|r| r.map(s16))
}

/// 0x8003BA28: `target` in the axes of `(turn, at)` (its transpose times the difference, split at bit 15 for the
/// GTE: the high part unshifted times 8, the low part >> 12), plus `off`.
fn toward(target: [i32; 3], turn: &M, at: [i32; 3], off: [i32; 3]) -> [i32; 3] {
    let d: [i32; 3] = std::array::from_fn(|k| target[k] - at[k]);
    let part = |v: i32, hi: bool| {
        let a = if hi { v.abs() >> 15 } else { v.abs() & 0x7FFF };
        if v < 0 {
            -a
        } else {
            a
        }
    };
    std::array::from_fn(|r| {
        let hi: i32 = (0..3).map(|k| turn[k][r] * part(d[k], true)).sum();
        let lo: i32 = (0..3).map(|k| turn[k][r] * part(d[k], false)).sum::<i32>() >> 12;
        let hi = if hi < 0 { -((-hi) << 3) } else { hi << 3 };
        lo + hi + off[r]
    })
}

/// A sum >> 12 cut toward zero (0x8003C148's `addiu 0xFFF`).
fn cut(v: i32) -> i32 {
    (v + if v < 0 { 0xFFF } else { 0 }) >> 12
}

/// 0x8003BCA8: an arm's (`elbow`) or a leg's two slots aimed at the target `v` (the upper's axes), the upper turned by
/// `twist` about its length, the joint bent by the law of cosines on the solver's own lengths (as the constants
/// fold them in). `lower` is kept when the target sits on the joint (the game leaves the slot as it was; shortcut:
/// here it is the frame's own, not the last one's, upgrade if a limb folded onto its joint shows a frame off).
fn limb(v: [i32; 3], twist: i32, elbow: bool, lower: M) -> Option<(M, M)> {
    let [x, y, z] = v;
    let d = x.wrapping_mul(x).wrapping_add(y.wrapping_mul(y)).wrapping_add(z.wrapping_mul(z));
    if d == 0 {
        return None;
    }
    let l = gte_sqrt(d);
    let inv = 0x40_0000 / l.max(1);
    let (nx, ny, nz) = ((x * inv) >> 10, (y * inv) >> 10, (z * inv) >> 10);
    let (t1, t2, a3) = match nx + 0x1000 {
        0 => (-0x800, (y * inv) >> 11, -0x1000),
        k if k >= 0x2000 => (-0x800, (-ny) >> 1, 0x1000),
        k => {
            let t1 = -0x100_0000 / k;
            (t1, (t1 * ny) >> 12, 0x1000)
        }
    };
    let a1 = ((t2 * ny) >> 12) + a3;
    let v0 = (t1 * nz) >> 12;
    let a3 = a3 + nx;
    let (t0, a2, t5, t8, t9) = ((t2 * a3) >> 12, (v0 * a3) >> 12, (v0 * ny) >> 12, (t2 * nz) >> 12, ((v0 * nz) >> 12) + 0x1000);
    let (s, c) = sc(twist);
    let upper =
        [[nx, (c * t0 + s * a2) >> 12, (-s * t0 + c * a2) >> 12], [ny, (c * a1 + s * t5) >> 12, (-s * a1 + c * t5) >> 12], [nz, (c * t8 + s * t9) >> 12, (-s * t8 + c * t9) >> 12]].map(|r| r.map(s16));
    let (far, near, k, ka, kb) = if elbow { (0x216, 0x47, 37380, 217, 282) } else { (0x37C, 3, 1784, 146, 147) };
    if l >= far {
        return Some((upper, I));
    }
    if l < near {
        return Some((upper, lower));
    }
    let (a, b) = (l << 11, (inv * k) >> 11);
    let unit = -0x1000..=0x1000;
    let cos = ((a + b) * ka) >> 16;
    let sin = if unit.contains(&cos) { s16(gte_sqrt(0x100_0000 - cos * cos) * if elbow { 1 } else { -1 }) } else { 0 };
    let cos = cos.clamp(-0x1000, 0x1000);
    // 0x8003BB88: the upper's first two columns turned by the bend
    let upper = upper.map(|r| [s16((r[0] * cos + r[1] * sin) >> 12), s16((-r[0] * sin + r[1] * cos) >> 12), r[2]]);
    let c2 = ((a - b) * kb) >> 16;
    let s2 = if unit.contains(&c2) { (-sin * if elbow { 5332 } else { 4114 }) >> 12 } else { 0 };
    let c2 = c2.clamp(-0x1000, 0x1000);
    let (cc, ss) = (cut(cos * c2 + sin * s2), cut(-sin * c2 + cos * s2));
    Some((upper, [[cc, -ss, 0], [ss, cc, 0], [0, 0, 4096]].map(|r| r.map(s16))))
}

/// The 18 slots' turns on their parents for a frame's channels (0x8003AD10), as rows of 9.
pub fn locals(c: &[i16; 49]) -> Turns {
    let ch = |k: usize| i32::from(c[k]);
    let mut l = [I; 18];
    for (i, &s) in ANGLE_SLOTS.iter().enumerate() {
        l[s] = euler(ch(19 + 3 * i), ch(20 + 3 * i), ch(21 + 3 * i));
    }
    for s in [2, 3, 7] {
        l[s] = mul(&l[s], &QUARTER);
    }
    // the solver's frames: the root at the motion's place, the chest, the shoulders and the pelvis on its skeleton
    let on = |p: &(M, [i32; 3]), r: &M, t: [i32; 3]| -> (M, [i32; 3]) {
        let at = mv(&p.0, t);
        (mul(&p.0, r), std::array::from_fn(|k| at[k] + p.1[k]))
    };
    let root = (l[0], [ch(0), ch(1), ch(2)]);
    let chest = on(&root, &l[1], BASE[0]);
    let shoulders = [on(&chest, &l[3], BASE[1]), on(&chest, &l[7], BASE[2])];
    let pelvis = on(&root, &l[11], BASE[3]);
    for (k, (base, upper, elbow)) in [(&shoulders[0], 4, true), (&shoulders[1], 8, true), (&pelvis, 12, false), (&pelvis, 15, false)].into_iter().enumerate() {
        let v = toward([ch(3 + 3 * k), ch(4 + 3 * k), ch(5 + 3 * k)], &base.0, base.1, BASE[4 + k]);
        if let Some((u, low)) = limb(v, ch(15 + k), elbow, l[upper + 1]) {
            (l[upper], l[upper + 1]) = (u, low);
        }
    }
    l.map(|m| std::array::from_fn(|k| m[k / 3][k % 3] as i16))
}

/// The slots' places in the world (0x8003B5C0 down the chain, from the root's matrix P+0xF54: the facing `facing`,
/// +0x2C, as Rz Ry Rx of (0, facing - 0x8000, 0), at `root`, P+0xF68) for their turns and the body's slot places.
pub fn places(turns: &Turns, slots: &[[i32; 3]], facing: i32, root: [i32; 3]) -> [(M, [i32; 3]); 18] {
    let mut world = [(I, [0; 3]); 18];
    let top = (euler(0, facing - 0x8000, 0), root);
    for s in 0..18 {
        let (pr, pt) = if s == 0 { top } else { world[PARENT[s]] };
        let l: M = std::array::from_fn(|r| std::array::from_fn(|c| i32::from(turns[s][3 * r + c])));
        let at = mv(&pr, slots.get(s).copied().unwrap_or_default());
        world[s] = (mul(&pr, &l), std::array::from_fn(|k| at[k] + pt[k]));
    }
    world
}

/// True Ogre's arm (0x80033A00, the skeleton's call for each of his slots 5, 6, 18 and 19 at 0x8003B310 /
/// 0x8003B434): each takes a part of a wave instead of its motion's turn, Rz Ry Rx of (0, the sine at the counter x 32
/// times `amp` / 512, the sine at the counter x 64 times `amp` / 256 plus slot 5's motion turn about z), the parts a
/// quarter of a wave apart; each part's turn and place on the one before, the place another slot's turn keeps
/// (P+0xF74 + 0x14: slot 5 at slot 18's, 6 at 5's, 18 at 19's, 19 at 6's). `count`: P+0x1294 this frame; `amp`: half
/// slot 6's height the frame before, 0 to 256 (P+0xAA4).
/// shortcut: his hand's value P+0x1296 (the wave's 0x100 less it, the rows' scales) is taken as 0, as the PS1's VS
/// fights keep it; slots 18 and 19's places are not in the pack (its 18): the PS1's, upgrade when the converter adds them
pub fn ogre_wave(turns: &Turns, slots: &[[i32; 3]], count: u16, amp: i32) -> [(M, [i32; 3]); 4] {
    let t5 = turns[5];
    let z5 = -(kumite_sim::camera::ratan2_on(kumite_sim::t3_atan_table, t5[0].into(), t5[1].into()) as i32) * 4;
    let place = |s: usize| slots.get(s).copied().unwrap_or_default();
    let at = [[273, -1, 2], place(5), [171, 0, 0], place(6)];
    std::array::from_fn(|part| {
        let wave = |times: i32| sin12(i32::from(count) * times + part as i32 * 0x400) * amp;
        (euler(0, s16(wave(32) / 512), s16(wave(64) / 256 + z5)), at[part])
    })
}

/// The four slots of True Ogre's arm ([`ogre_wave`]) in the world, from slot 4's.
pub fn ogre_arm(turns: &Turns, upper: (M, [i32; 3]), slots: &[[i32; 3]], count: u16, amp: i32) -> [(M, [i32; 3]); 4] {
    let mut on = upper;
    ogre_wave(turns, slots, count, amp).map(|(turn, at)| {
        let p = mv(&on.0, at);
        on = (mul(&on.0, &turn), std::array::from_fn(|i| p[i] + on.1[i]));
        on
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// King's frame as the PS1 logged it (PCSX-Redux, VS, P+0xF74 at the facing driver, ease off): the solver's turns.
    #[test]
    fn kings_pose_as_on_the_ps1() {
        let c: [i16; 49] = [
            44, -904, -126, -230, -1164, -448, 178, -1166, -710, -80, -186, 184, 176, -112, -446, 27596, 3720, 1194, 306, 0, 0, 0, -22850, 2892, -17696, 23850, 1052, 16288, 11648, 342, 30426, -31712,
            -1262, 16356, 400, -76, 16304, -1908, -442, 1342, -3690, -834, 36, 598, -922, -22384, -2244, -1108, -20482,
        ];
        let l = locals(&c);
        assert_eq!(l[4], [22, 2554, -3224, -2067, -2758, -2208, -3570, 1615, 1336]);
        assert_eq!(l[5], [-508, 4051, 0, -4051, -508, 0, 0, 0, 4096]);
        assert_eq!(l[13], [-281, -4078, 0, 4078, -281, 0, 0, 0, 4096]);
        assert_eq!(l[17], [-1564, 3658, 975, -3760, -1624, 59, 439, -874, 3977]);
    }

    /// True Ogre's arm as the PS1 had it (t3sim42's c24 RAM at vsync 1790: P+0x1294 1314, amplitude 256, slot 5's
    /// motion turn about z 0): slots 5, 6, 18 and 19's places from slot 4's (P+0x8F4 + 0x44 a slot).
    #[test]
    fn true_ogres_arm_waves_as_on_the_ps1() {
        let upper = ([[2273, -3413, -16], [-1471, -989, 3695], [-3099, -2036, -1766]], [-845, -3055, -371]);
        let mut turns = [[0; 9]; 18];
        turns[5][0] = 4096;
        let mut slots = [[0; 3]; 18];
        (slots[5], slots[6]) = ([343, 0, 0], [270, 0, 0]);
        let arm = ogre_arm(&turns, upper, &slots, 1314, 256);
        assert_eq!(arm.map(|w| w.1), [[-693, -3151, -578], [-485, -3326, -791], [-338, -3387, -856], [-116, -3446, -1000]]);
        assert_eq!(arm[3].0[0], [2262, -3413, -220]);
    }
}
