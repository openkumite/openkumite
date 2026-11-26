//! Face texture animation (gamedata/faces.json; EXE 0x80034100, 0x800397FC / 0x80033ECC): every fighter whose
//! model has a blink rectangle blinks, 4 frames shut then (rand & 0xFE) + 1 open, shut on the first frame, held
//! shut by the move (flags24 bit 0x40000, or lying without bit 0x40000000); one character's pupils slide toward the
//! opponent's head. The app copies the rectangles in the fighter's texture when the state changes.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

/// A blink: the eyes' rectangle in the atlas (x, y, w, h) and where the closed eyes are (x, y).
#[derive(Clone, Copy, Debug)]
pub struct Blink {
    pub dest: [u32; 4],
    pub closed: [u32; 2],
}

/// One of the moving pupils' eye strips: columns x..x+w; rows source_y + offset are shown at dest_y (h rows).
#[derive(Clone, Copy, Debug)]
pub struct Pupil {
    pub x: u32,
    pub w: u32,
    pub h: u32,
    pub dest_y: u32,
    pub source_y: u32,
}

/// faces.json by model id ((model record − 71) / 4).
#[derive(Clone, Debug, Default)]
pub struct FaceData {
    pub blink: HashMap<usize, Blink>,
    pub pupils: HashMap<usize, [Pupil; 2]>,
    /// The character whose pupils move (faces.json `pupils.character`).
    pub pupils_character: Option<i64>,
}

impl FaceData {
    pub fn load(gamedata: &Path) -> Self {
        let Some(v) = kumite_fs::read(&gamedata.join("faces.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            return Self::default();
        };
        let u = |v: &Value| v.as_u64().unwrap_or(0) as u32;
        let mut out = Self { pupils_character: v["pupils"]["character"].as_i64(), ..Self::default() };
        for (k, m) in v["models"].as_object().into_iter().flatten() {
            let Ok(id) = k.parse::<usize>() else { continue };
            let b = &m["blink"];
            if b.is_object() {
                out.blink.insert(id, Blink { dest: [0, 1, 2, 3].map(|i| u(&b["dest"][i])), closed: [u(&b["closed"][0]), u(&b["closed"][1])] });
            }
            if let Some(p) = m["pupils"].as_array().filter(|p| p.len() == 2) {
                let pupil = |p: &Value| Pupil { x: u(&p["x"]), w: u(&p["w"]), h: u(&p["h"]), dest_y: u(&p["dest_y"]), source_y: u(&p["source_y"]) };
                out.pupils.insert(id, [pupil(&p[0]), pupil(&p[1])]);
            }
        }
        out
    }
}

/// Tekken Tag's wings (0x3632E0, every frame for True Ogre and Devil): `state` (+0xB052: 0 folded, 1 open, 2 beating;
/// in a fight 1 while airborne and not juggled, 0x370C94; ending op 28 sets it) runs the frames of their track: opening
/// 0-16 (list 0x4161A0), a beat 18-46 (0x4161B8), back to 16 and down to 0 to fold.
#[derive(Clone, Copy, Debug, Default)]
pub struct WingState {
    pub state: i32,
    beating: bool,
    step: i32,
    /// The track's frame shown (+0xB058).
    pub frame: usize,
}

/// The opening's last step and the beat's (lists 0x4161A0 = 0..=16, 0x4161B8 = 18..=46, each ended by 0xFF).
const OPENING: i32 = 16;
const BEAT: i32 = 28;

impl WingState {
    /// One frame.
    pub fn step(&mut self) {
        match (self.state, self.beating) {
            (3, _) => {}
            (0, false) => self.step = (self.step - 1).max(0),
            (_, false) if self.step == OPENING => (self.beating, self.step) = (true, 0),
            (_, false) => self.step += 1,
            // (held open: a beat under way runs to its end, then stays on its first frame)
            (1, true) if self.step != 0 => self.step = if self.step == BEAT { 0 } else { self.step + 1 },
            (1, true) => {}
            (_, true) if self.step == BEAT => (self.beating, self.step) = (false, OPENING),
            (_, true) => self.step += 1,
        }
        self.frame = (if self.beating { 18 + self.step } else { self.step }) as usize;
    }
}

/// A fighter's face state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Face {
    blink: kumite_sim::Blink,
    /// The eyes are shut.
    pub closed: bool,
    /// The moving pupils: each eye's row offset once it has been moved (None: as loaded).
    pub pupils: [Option<i32>; 2],
    cache: [i32; 2],
}

impl Face {
    /// One blink frame (0x80034100; Tekken 3's run in the fight, [`kumite_sim::Fight::blinks`]); `rand` the game's
    /// rand() (called on each reopening).
    pub fn blink(&mut self, forced: bool, rand: &mut impl FnMut() -> u32) {
        self.blink.step(forced, rand);
        self.closed = self.blink.closed;
    }

    /// The moving pupils (0x80033ECC) for the opponent's head at angle `a` (4096ths of a turn) in his head's frame.
    pub fn pupils(&mut self, a: i32) {
        let e = if a < 0x800 { (-(a / 60)).max(-17) } else { (-((a - 0xF00) / 60)).min(17) };
        let v = [e.max(0), e.min(0)];
        for (k, v) in v.into_iter().enumerate() {
            if v != self.cache[k] {
                self.cache[k] = v;
                self.pupils[k] = Some(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wings_open_beat_and_fold() {
        let mut w = WingState::default();
        let run = |w: &mut WingState, state: i32, n: usize| -> Vec<usize> {
            w.state = state;
            (0..n)
                .map(|_| {
                    w.step();
                    w.frame
                })
                .collect()
        };
        // opening: 1..=16, then the beat's first frame, held
        assert_eq!(run(&mut w, 1, 19), [(1..=16).collect::<Vec<_>>(), vec![18, 18, 18]].concat());
        // beating: 19..=46, back to 16, then the beat again
        assert_eq!(run(&mut w, 2, 31), [(19..=46).collect::<Vec<_>>(), vec![16, 18, 19]].concat());
        // folding: the beat to its end, then 16 down to 0
        let fold = run(&mut w, 0, 60);
        assert_eq!((&fold[..2], fold[26], fold[27], *fold.last().unwrap()), (&[20, 21][..], 46, 16, 0));
    }

    #[test]
    fn blinks_four_frames_shut() {
        let mut f = Face::default();
        let mut r = || 9u32; // opens for (9 & 0xFE) + 1 = 9 frames
        let seq: Vec<bool> = (0..20)
            .map(|_| {
                f.blink(false, &mut r);
                f.closed
            })
            .collect();
        assert_eq!(&seq[..4], &[true; 4], "shut on the first frame, for 4 frames");
        assert_eq!(&seq[4..13], &[false; 9]);
        assert!(seq[13]);
    }

    #[test]
    fn pupils_follow_the_angle() {
        let mut f = Face::default();
        f.pupils(0);
        assert_eq!(f.pupils, [None, None], "nothing moves until the offset changes");
        f.pupils(600);
        assert_eq!(f.pupils, [None, Some(-10)]);
        f.pupils(0xF00 - 1200);
        assert_eq!(f.pupils[0], Some(17));
    }
}
