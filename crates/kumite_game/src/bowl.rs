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
}

impl BowlScene {
    /// A game for `players`: each one's model record (its character file) and character id, in turn order, and
    /// its partner's if it has one; `seed` draws the copies' stats and the game's randomness.
    pub fn new(assets: &Assets, players: &[(usize, usize)], partners: &[Option<(usize, usize)>], seed: u32) -> Result<Self> {
        let raw: serde_json::Value = assets.gamedata("bowl.json")?;
        let data: BowlData = serde_json::from_value(raw.clone()).context("bowl.json")?;
        let extra: Extra = serde_json::from_value(raw).unwrap_or_default();
        let mut draw = seed;
        let (mut characters, mut clips, mut bowlers, mut mates) = (vec![], vec![], vec![], vec![]);
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
            let hand = if b.left { 4 } else { 0 };
            let (run, after) = (0x6A + 2 * usize::from(b.female) + usize::from(b.left), 0x6E + 2 * usize::from(b.female) + usize::from(b.left));
            let strike = strike_of(if *id == COPIERS[0] { stats } else { *id });
            let ch = with_motions(*record, &[hand, hand + 1, hand + 2, hand + 3, run, after], strike)?;
            let clip = |m: usize| ch.clip(&format!("bowl_{m:03}"));
            let k = Clips {
                stand: clip(hand).context("no bowl motions (motions/bowl_NNN.glb): convert the disc again")?,
                step_right: clip(hand + 1).context("bowl motion")?,
                step_left: clip(hand + 2).context("bowl motion")?,
                throw: clip(hand + 3).context("bowl motion")?,
                run: clip(run).context("bowl motion")?,
                after: clip(after).context("bowl motion")?,
                strike: strike.and_then(clip),
            };
            b.hand = hand_at_release(&ch, k.throw, b.left).unwrap_or(b.hand);
            b.charge = travel(&ch, k.run);
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
            characters.push(Arc::new(ch));
        }
        // (a plain file name in `stages/` only: a folder in it would read outside)
        let [ball_model, pin_model, deck_model] =
            [&extra.models.ball, &extra.models.pin, &extra.models.deck].map(|m| format!("stages/{}", std::path::Path::new(m).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned())));
        let pin_base = if extra.models.pin_base > 0.0 { extra.models.pin_base } else { 250.66 };
        let first = clips.first().map_or(0, |c| c.stand);
        let (records, texts) = (extra.records, extra.texts);
        Ok(Self { bowl: Bowl::new(&data, bowlers, draw), characters, clips, partners: mates, ball_model, pin_model, deck_model, pin_base, records, texts, anim: (first, 0), stepping: 0 })
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
        let (at, a) = b.bowler_at();
        let (s, c) = a.dsin_cos();
        let ahead = |p: Vec3, d: f32| p + Vec3::new(s * d, 0.0, c * d);
        if b.replay.on {
            // the pivot, and the eye at the distance turned by pitch (about x) and yaw (about y)
            let ((sp, cp), (sy, cy)) = (b.replay.view.0.dsin_cos(), b.replay.view.1.dsin_cos());
            let pivot = Vec3::from(REPLAY_PIVOT);
            let d = REPLAY_DISTANCE;
            return (to_m(pivot + Vec3::new(-d * cp * sy, d * sp, -d * cp * cy)), to_m(pivot));
        }
        // the strike's pose: after the bowler's body as its reaction moves it (mode 5 follows it)
        // shortcut: on it at once, not eased (mode 5's 0.97 / 0.03 a frame)
        if let (Phase::Strike(_), Some((ch, pose, place))) = (b.phase, self.pose()) {
            let root = ch.nodes.iter().position(|n| n.parent.is_none()).and_then(|r| pose.get(r));
            let at = root.map_or(place.w_axis.truncate(), |m| place.transform_point3(m.w_axis.truncate()));
            let pivot = Vec3::new(at.x, 0.5, at.z);
            return (pivot + Vec3::Z * 3.5, pivot);
        }
        let (eye, target) = match b.phase {
            Phase::Position | Phase::Aim | Phase::Power | Phase::OverCharged(_) => (ahead(Vec3::new(at.x, -2350.0, at.z), -4500.0), ahead(Vec3::new(at.x, -400.0, at.z), 9000.0)),
            Phase::Throw(f) if b.rolled.is_none() || f < RELEASE_FRAME + 10 => (ahead(Vec3::new(at.x, -2350.0, at.z), -4500.0 + 30.0 * f as f32), ahead(Vec3::new(at.x, -400.0, at.z), 9000.0)),
            Phase::Charge(_) => {
                let z = (at.z + b.players.get(b.current).and_then(|p| p.charge.last()).copied().unwrap_or(0.0)).min(4000.0);
                (Vec3::new(0.0, -1500.0, z - 3000.0), Vec3::new(0.0, -300.0, 8000.0))
            }
            Phase::Throw(_) | Phase::Ko(_) => {
                // behind the ball, over to the middle by the time it is past z 4000
                let ball = b.ball.pos;
                let t = ((ball.z + 3000.0) / 10000.0).clamp(0.0, 1.0);
                let x = ball.x * (1.0 - t);
                (Vec3::new(x, -1500.0, (ball.z - 3000.0).min(4000.0)), Vec3::new(x, -300.0, (ball.z + 4000.0).min(8000.0)))
            }
            Phase::Strike(_) => (Vec3::new(at.x, -500.0, at.z + 3500.0), Vec3::new(at.x, -500.0, at.z)),
            Phase::Result(_) | Phase::Over => (Vec3::new(0.0, -1500.0, 4000.0), Vec3::new(0.0, -300.0, 8000.0)),
        };
        (to_m(eye), to_m(target))
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
