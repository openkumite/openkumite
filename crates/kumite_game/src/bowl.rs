//! Tekken Bowl's scene (Tekken Tag Tournament's mode 8, `gamedata/bowl.json`): `kumite_sim::bowl`'s game with its
//! bowlers' characters playing the bowl motions (file 128's, `motions/bowl_NNN.glb`), the camera (0x3EBC98), and the
//! ball's and pins' places for the pictures, on the bowling hall (arena 20). The motions also give the simulation
//! where the hand lets the ball go and how far the charge runs.
//!
//! The game's axes (x, y down, z down the lane) are the picture's (-x, -y, z) in metres, the hall's models' own (its
//! sign reads the right way round from the bowler's place, a right-hander's ball is on the screen's right).

use crate::{placement, UNITS};
use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Vec3};
use kumite_assets::{Assets, Character};
use kumite_sim::bowl::{Bowl, BowlData, Bowler, Phase, CHARGE_FRAMES, RELEASE_FRAME, REPLAY_DISTANCE, REPLAY_PIVOT, STRIKE_SIDE, STRIKE_Z, THROW_FRAMES};
use kumite_sim::detmath::DetMath;
use kumite_sim::input::Pad;
use std::sync::Arc;

/// The bowling hall.
pub const ARENA: usize = 20;
/// Mokujin and Unknown bowl with someone else's stats (0x3E9C58).
const COPIERS: [usize; 2] = [15, 33];

/// What the scene reads of bowl.json beyond the simulation's part.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Extra {
    models: Models,
    records: Records,
    texts: std::collections::BTreeMap<String, serde_json::Value>,
}

/// TOP 5 BOWLERS (save block 0x46BC70): their defaults (0x3F1F58), the scores that keep a bonus (200: JUKEBOX).
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Records {
    pub size: usize,
    pub defaults: Vec<Record>,
    pub bonus: Bonus,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Bonus {
    pub jukebox: u16,
    pub perfect: u16,
}

/// A record (0x66 bytes: the name's three letters, the team's two bodies, the score card).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Record {
    pub name: String,
    pub bodies: [usize; 2],
    pub score: u16,
}

impl Records {
    /// `score` placed (0x3F2128) among `list` from place `from` on: below the higher scores, above the equal and
    /// lower ones, the last one out; its place, if it got one.
    pub fn insert(&self, list: &mut Vec<Record>, from: usize, record: Record) -> Option<usize> {
        let size = self.size.max(1);
        let at = (from..size.min(list.len() + 1)).find(|&k| list.get(k).is_none_or(|r| record.score >= r.score))?;
        list.insert(at, record);
        list.truncate(size);
        Some(at)
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Models {
    ball: String,
    pin: String,
    deck: String,
    pin_base: f32,
}

/// A bowler's motions (clip indices in its character): by hand, the charge, the strike's reaction (0x523590's
/// second, 0x3E9550).
#[derive(Clone, Debug)]
struct Clips {
    left: bool,
    stand: usize,
    step_right: usize,
    step_left: usize,
    throw: usize,
    run: usize,
    after: usize,
    strike: Option<usize>,
}

/// The game's point (units) in the picture's metres.
fn to_m(g: Vec3) -> Vec3 {
    Vec3::new(-g.x, -g.y, g.z) / UNITS
}

/// A game rotation as the picture's, for a model in the picture's axes (the hall's ball and pin).
fn model_rotation(r: Mat3) -> Mat3 {
    let flip = Mat3::from_diagonal(Vec3::new(-1.0, -1.0, 1.0));
    flip * r * flip
}

pub struct BowlScene {
    pub bowl: Bowl,
    /// Per player: its character with the bowl clips.
    pub characters: Vec<Arc<Character>>,
    clips: Vec<Clips>,
    /// Per player: the other hand's motions, when SELECT switches its hand (0x3EAE28).
    other_clips: Vec<Option<Clips>>,
    /// Per player: its tag partner, who joins the strike's pose (0x3E99C0's slot 1), with its strike reaction.
    pub partners: Vec<Option<(Arc<Character>, usize)>>,
    /// The ball's, the pin's and the pin deck's glbs (`stages/`), and the pin's centre above its base.
    pub ball_model: String,
    pub pin_model: String,
    pub deck_model: String,
    pin_base: f32,
    /// TOP 5 BOWLERS' rules and defaults; the texts of the results and the pause menu (bowl.json `texts`).
    pub records: Records,
    pub texts: std::collections::BTreeMap<String, serde_json::Value>,
    /// The bowler's motion now and its frame; the step clip the pad asked for last.
    anim: (usize, usize),
    stepping: i8,
    /// The strike's pose: the point the camera looks at (metres), after the bowler's body (0x3EBC98 modes 4, 5).
    pivot: Vec3,
    /// The throw's and the roll's camera (0x1FC51B0, game units): its pivot, yaw (radians) and distance.
    view: (Vec3, f32, f32),
}

/// The camera's pitch (0x3E9C10: -0.2 rad, looking down), the fixed view behind the lane (mode 0: its pivot and
/// distance), the bowler's eye height (mode 1), where the roll's camera goes (mode 2, 0x523960) and how far behind the
/// ball it stays.
const PITCH: f32 = -0.2;
const LANE_VIEW: ([f32; 3], f32) = ([0.0, -2300.0, -12000.0], 2000.0);
const EYE_Y: f32 = -2350.0;
const ROLL_TO: [f32; 3] = [0.0, -1500.0, 4000.0];
const ROLL_BEHIND: f32 = 4000.0;

impl BowlScene {
    /// A game for `players`: each one's model record (its character file) and character id, in turn order, and
    /// its partner's if it has one; `seed` draws the copies' stats and the game's randomness.
    pub fn new(assets: &Assets, players: &[(usize, usize)], partners: &[Option<(usize, usize)>], seed: u32) -> Result<Self> {
        let raw: serde_json::Value = assets.gamedata("bowl.json")?;
        let data: BowlData = serde_json::from_value(raw.clone()).context("bowl.json")?;
        let extra: Extra = serde_json::from_value(raw).unwrap_or_default();
        let mut draw = seed;
        let (mut characters, mut clips, mut others, mut bowlers, mut mates) = (vec![], vec![], vec![], vec![], vec![]);
        // a character with the bowl motions `m`, and the clip of its strike reaction (Mokujin: who he copies')
        let with_motions = |record: usize, ms: &[usize], strike: Option<usize>| -> Result<Character> {
            let mut ch = assets.character(record)?;
            for &m in ms.iter().chain(&strike) {
                let path = assets.root.join(format!("motions/bowl_{m:03}.glb"));
                // (a reaction the disc has none of, Mokujin's 0x72, is not there)
                if let Ok(bytes) = kumite_fs::read(&path) {
                    ch.add_motions(&bytes).with_context(|| format!("{}", path.display()))?;
                }
            }
            Ok(ch)
        };
        let strike_of = |id: usize| data.characters.iter().find(|c| c.id == id).and_then(|c| c.reactions.get(1)).map(|&m| m as usize);
        for (n, (record, id)) in players.iter().enumerate() {
            let stats = if COPIERS.contains(id) {
                // someone of the 32 others, at random
                draw = draw.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let others: Vec<usize> = (0..34).filter(|c| !COPIERS.contains(c)).collect();
                others.get((draw >> 16) as usize % others.len()).copied().unwrap_or(0)
            } else {
                *id
            };
            let mut b = Bowler::new(&data, *id, stats);
            // a hand's motions: stand, the steps, the throw (0-3, a left-hander's 4-7), the charge and after it
            let (female, both) = (usize::from(b.female), b.switches());
            let set = |left: bool| {
                let (h, l) = (4 * usize::from(left), usize::from(left));
                [h, h + 1, h + 2, h + 3, 0x6A + 2 * female + l, 0x6E + 2 * female + l]
            };
            let strike = strike_of(if *id == COPIERS[0] { stats } else { *id });
            let wanted: Vec<usize> = [b.left, !b.left].iter().take(1 + usize::from(both)).flat_map(|&l| set(l)).collect();
            let ch = with_motions(*record, &wanted, strike)?;
            let clips_of = |left: bool| -> Result<Clips> {
                let clip = |m: usize| ch.clip(&format!("bowl_{m:03}")).context("no bowl motions (motions/bowl_NNN.glb): convert the disc again");
                let [stand, step_right, step_left, throw, run, after] = set(left);
                Ok(Clips {
                    left,
                    stand: clip(stand)?,
                    step_right: clip(step_right)?,
                    step_left: clip(step_left)?,
                    throw: clip(throw)?,
                    run: clip(run)?,
                    after: clip(after)?,
                    strike: strike.and_then(|m| clip(m).ok()),
                })
            };
            let k = clips_of(b.left)?;
            let other = if both { Some(clips_of(!b.left)?) } else { None };
            let at_release = |c: &Clips| hand_at_release(&ch, c.throw, c.left).unwrap_or(Vec3::new(if c.left { -150.0 } else { 150.0 }, -500.0, 4200.0));
            b.hand = at_release(&k);
            b.charge = travel(&ch, k.run);
            b.other_hand = other.as_ref().map(|o| (at_release(o), travel(&ch, o.run)));
            let mate = match partners.get(n).copied().flatten() {
                Some((record, id)) => {
                    // (a Mokujin partner takes the bowler's moveset, 0x3F0684)
                    let reaction = if id == COPIERS[0] { strike } else { strike_of(id) };
                    let m = with_motions(record, &[], reaction)?;
                    reaction.and_then(|r| m.clip(&format!("bowl_{r:03}"))).map(|c| (Arc::new(m), c))
                }
                None => None,
            };
            // the pose lasts until both reactions are over (0x3E9628)
            let frames = |ch: &Character, c: Option<usize>| c.and_then(|c| ch.clips.get(c)).map_or(0, |c| c.frames as u32);
            let longest = frames(&ch, k.strike).max(mate.as_ref().map_or(0, |(m, c)| frames(m, Some(*c))));
            if longest > 0 {
                b.strike_frames = longest;
            }
            mates.push(mate);
            bowlers.push(b);
            clips.push(k);
            others.push(other);
            characters.push(Arc::new(ch));
        }
        // (a plain file name in `stages/` only: a folder in it would read outside)
        let [ball_model, pin_model, deck_model] =
            [&extra.models.ball, &extra.models.pin, &extra.models.deck].map(|m| format!("stages/{}", std::path::Path::new(m).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned())));
        let pin_base = if extra.models.pin_base > 0.0 { extra.models.pin_base } else { 250.66 };
        let first = clips.first().map_or(0, |c| c.stand);
        let (records, texts) = (extra.records, extra.texts);
        Ok(Self {
            bowl: Bowl::new(&data, bowlers, draw),
            characters,
            clips,
            other_clips: others,
            partners: mates,
            ball_model,
            pin_model,
            deck_model,
            pin_base,
            records,
            texts,
            anim: (first, 0),
            stepping: 0,
            pivot: Vec3::ZERO,
            view: (Vec3::from(LANE_VIEW.0), 0.0, LANE_VIEW.1),
        })
    }

    /// One frame with the current player's pad.
    pub fn step(&mut self, pad: Pad) {
        self.stepping = if self.bowl.phase == Phase::Position && pad.left != pad.right {
            if pad.left {
                -1
            } else {
                1
            }
        } else {
            0
        };
        self.bowl.step(pad);
        // (SELECT switched the hand: its other motions)
        let k = self.bowl.current;
        if let (Some(p), Some(c), Some(Some(o))) = (self.bowl.players.get(k), self.clips.get_mut(k), self.other_clips.get_mut(k)) {
            if o.left == p.left {
                std::mem::swap(c, o);
            }
        }
        let Some(c) = self.clips.get(self.bowl.current) else { return };
        let (clip, frame) = match self.bowl.phase {
            Phase::Throw(f) if f <= THROW_FRAMES => (c.throw, Some(f as usize - 1)),
            Phase::Charge(f) if f < CHARGE_FRAMES => (c.run, Some(f as usize)),
            Phase::Charge(f) => (c.after, Some((f - CHARGE_FRAMES) as usize)),
            Phase::Strike(f) => (c.strike.unwrap_or(c.stand), Some(f as usize)),
            // (the bowler steps the way the screen moves: the picture's x is the game's -x)
            Phase::Position if self.stepping < 0 => (c.step_left, None),
            Phase::Position if self.stepping > 0 => (c.step_right, None),
            _ => (c.stand, None),
        };
        self.anim = match frame {
            Some(f) => (clip, f),
            None if self.anim.0 == clip => (clip, self.anim.1 + 1),
            None => (clip, 0),
        };
        // the camera (0x3EBC98, its mode set by the throw's phases): behind the lane while the bowler takes its place
        // (mode 0), at the bowler's eye turned with its aim (mode 1), held for the power and the run-up, then from the
        // release (or the charge) 4000 behind the ball, on the way to over the lane by the time the ball is at 8000
        // (mode 2)
        let (at, angle) = self.bowl.bowler_at();
        match self.bowl.phase {
            Phase::Position => self.view = (Vec3::from(LANE_VIEW.0), 0.0, LANE_VIEW.1),
            Phase::Aim => self.view = (Vec3::new(at.x, EYE_Y, at.z), angle, 0.0),
            Phase::Throw(_) | Phase::Charge(_) if self.bowl.rolled.is_some() && !self.bowl.replay.on => {
                let z = if matches!(self.bowl.phase, Phase::Charge(_)) { at.z } else { self.bowl.ball.pos.z };
                let (pivot, yaw, dist) = &mut self.view;
                let to = Vec3::from(ROLL_TO);
                let gap = z - ROLL_BEHIND - pivot.z;
                if gap > 0.0 {
                    let t = gap / (to.z - pivot.z);
                    if t < 1.0 {
                        *pivot += (to - *pivot) * t;
                        (*yaw, *dist) = (*yaw * (1.0 - t), *dist * (1.0 - t));
                    } else {
                        self.view = (to, 0.0, 0.0);
                    }
                }
            }
            _ => {}
        }
        // the strike's camera: on the body 500 above its root (mode 4), then a frame after it halfway there, a
        // Hwoarang (moveset 6, a Mokujin copying him too) 0.03 of the way (mode 5)
        if let (Phase::Strike(f), Some(root)) = (self.bowl.phase, self.root()) {
            let to = root + Vec3::Y * 500.0 / UNITS;
            let slow = self.bowl.players.get(self.bowl.current).is_some_and(|p| p.character == 6 || (p.character == COPIERS[0] && p.stats == 6));
            self.pivot = if f == 0 { to } else { self.pivot.lerp(to, if slow { 0.03 } else { 0.5 }) };
        }
    }

    /// The bowler's root (metres).
    fn root(&self) -> Option<Vec3> {
        let (ch, pose, place) = self.pose()?;
        let root = ch.nodes.iter().position(|n| n.parent.is_none()).and_then(|r| pose.get(r));
        Some(root.map_or(place.w_axis.truncate(), |m| place.transform_point3(m.w_axis.truncate())))
    }

    /// The bowler drawn now: its character, node matrices and placement.
    pub fn pose(&self) -> Option<(&Arc<Character>, Vec<Mat4>, Mat4)> {
        let ch = self.characters.get(self.bowl.current)?;
        let (clip, frame) = self.anim;
        let n = ch.clips.get(clip)?.frames.max(1);
        // the stand and step motions loop; the throw, charge and reaction hold their last frame
        let looped = self.clips.get(self.bowl.current).is_some_and(|c| [c.stand, c.step_left, c.step_right].contains(&clip));
        let f = if looped { frame % n } else { frame.min(n - 1) };
        let (at, a) = self.bowl.bowler_at();
        Some((ch, ch.pose(clip, f, false), placement(to_m(at) * UNITS, -a)))
    }

    /// The partner in the strike's pose (state 5): 3000 behind the bowler and 1000 to the side (towards -x for 1P),
    /// its reaction at the bowler's frame.
    pub fn partner_pose(&self) -> Option<(&Arc<Character>, Vec<Mat4>, Mat4)> {
        let Phase::Strike(f) = self.bowl.phase else { return None };
        let (ch, clip) = self.partners.get(self.bowl.current)?.as_ref()?;
        let n = ch.clips.get(*clip)?.frames.max(1);
        let side = if self.bowl.current == 0 { -STRIKE_SIDE } else { STRIKE_SIDE };
        let at = Vec3::new(self.bowl.x + side, 0.0, STRIKE_Z[1]);
        Some((ch, ch.pose(*clip, (f as usize).min(n - 1), false), placement(to_m(at) * UNITS, 0.0)))
    }

    /// The ball's centre (metres) and orientation, when shown: in the hand until it goes.
    pub fn ball(&self) -> Option<(Vec3, Mat3)> {
        let b = &self.bowl.ball;
        if !b.shown {
            return None;
        }
        if !b.rolling && self.bowl.rolled.is_none() {
            let (ch, pose, place) = self.pose()?;
            let left = self.bowl.players.get(self.bowl.current).is_some_and(|p| p.left);
            let hand = hand_of(ch, &pose, left)?;
            let side = place.transform_vector3(Vec3::new(if left { 0.15 } else { -0.15 }, 0.0, 0.0));
            return Some((place.transform_point3(hand) + side, Mat3::IDENTITY));
        }
        let r = b.roll;
        let spin = Mat3::from_rotation_y(r.y) * Mat3::from_rotation_x(r.x) * Mat3::from_rotation_z(r.z);
        Some((to_m(b.pos), model_rotation(spin)))
    }

    /// Each pin: its base (metres) and orientation; None once swept.
    pub fn pins(&self) -> Vec<Option<(Vec3, Mat3)>> {
        (self.bowl.pins.iter()).map(|p| p.on_deck.then(|| (to_m(p.base(self.pin_base)), model_rotation(p.rot)))).collect()
    }

    /// The camera (0x3EBC98): eye and target in metres. Behind the bowler while it bowls (mode 1), after the ball
    /// as it rolls, easing to its place over the lane by z 4000 (mode 2), then held; the fixed view behind the
    /// lane (mode 0) between balls. A replay circles the pins at its drawn angles (mode 3); the strike's pose
    /// looks back at the bowler from 3500 in front, 500 up (mode 4).
    // shortcut: the modes' offsets from the bowler and the ball are OpenKumite's (only their bases are decoded)
    pub fn camera(&self) -> (Vec3, Vec3) {
        let b = &self.bowl;
        if b.replay.on {
            // the pivot, and the eye at the distance turned by pitch (about x) and yaw (about y)
            let ((sp, cp), (sy, cy)) = (b.replay.view.0.dsin_cos(), b.replay.view.1.dsin_cos());
            let pivot = Vec3::from(REPLAY_PIVOT);
            let d = REPLAY_DISTANCE;
            return (to_m(pivot + Vec3::new(-d * cp * sy, d * sp, -d * cp * cy)), to_m(pivot));
        }
        // the strike's pose: 3500 down the lane from the point after the bowler's body, looking back (yaw pi)
        if let Phase::Strike(_) = b.phase {
            return (self.pivot + Vec3::Z * 3500.0 / UNITS, self.pivot);
        }
        // the eye `dist` back from the pivot along the view, turned by the pitch and the yaw
        let (pivot, yaw, dist) = self.view;
        let ((sp, cp), (sy, cy)) = (PITCH.dsin_cos(), yaw.dsin_cos());
        let ahead = Vec3::new(cp * sy, -sp, cp * cy);
        (to_m(pivot - ahead * dist), to_m(pivot + ahead * 1000.0))
    }
}

impl BowlScene {
    /// A text of bowl.json's `texts` (`fallback` without it).
    pub fn text(&self, key: &str, fallback: &str) -> String {
        self.texts.get(key).and_then(|t| t.as_str()).unwrap_or(fallback).to_string()
    }
}

/// The hand joints (Tekken 3's names: A the right side's, B the left's).
fn hand_of(ch: &Character, pose: &[Mat4], left: bool) -> Option<Vec3> {
    let name = if left { "hand_B" } else { "hand_A" };
    let k = ch.nodes.iter().position(|n| n.name == name)?;
    pose.get(k).map(|m| m.w_axis.truncate())
}

/// Where the ball leaves the hand (game units, from the bowler's spot facing down the lane): the throw motion's
/// frame 106 with its root's travel, 150 out to the side (0x3EBB80).
fn hand_at_release(ch: &Character, throw: usize, left: bool) -> Option<Vec3> {
    let pose = ch.pose(throw, RELEASE_FRAME as usize - 1, false);
    let p = hand_of(ch, &pose, left)? + Vec3::new(if left { 0.15 } else { -0.15 }, 0.0, 0.0);
    // (the picture's metres facing +z: the game's x is -x, y is -y)
    Some(Vec3::new(-p.x, -p.y.max(0.15), p.z) * UNITS)
}

/// The charge motion's root travel down the lane, frame by frame (game units).
fn travel(ch: &Character, run: usize) -> Vec<f32> {
    let root = ch.nodes.iter().position(|n| n.parent.is_none());
    let t = root.and_then(|r| ch.clips.get(run)?.translations.get(r)?.clone()).unwrap_or_default();
    t.iter().map(|v| v.z * UNITS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_score_takes_its_place_among_the_top_five() {
        let r = Records { size: 5, ..Records::default() };
        let rec = |score| Record { name: "PAU".into(), bodies: [0, 1], score };
        let mut list: Vec<Record> = [268, 214, 157, 103, 30].map(rec).to_vec();
        // above an equal score, the last one out; too low: no place; the second player not above the first's
        assert_eq!(r.insert(&mut list, 0, rec(157)), Some(2));
        assert_eq!(list.iter().map(|r| r.score).collect::<Vec<_>>(), [268, 214, 157, 157, 103]);
        assert_eq!(r.insert(&mut list, 0, rec(100)), None);
        assert_eq!(r.insert(&mut list, 3, rec(300)), Some(3));
    }
}
