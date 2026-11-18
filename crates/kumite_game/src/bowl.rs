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
use kumite_sim::bowl::{Bowl, BowlData, Bowler, Phase, CHARGE_FRAMES, RELEASE_FRAME, THROW_FRAMES};
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
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Models {
    ball: String,
    pin: String,
    deck: String,
    pin_base: f32,
}

/// A bowler's motions (clip indices in its character): by hand, the charge, the strike reaction.
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
    /// The ball's, the pin's and the pin deck's glbs (`stages/`), and the pin's centre above its base.
    pub ball_model: String,
    pub pin_model: String,
    pub deck_model: String,
    pin_base: f32,
    /// The bowler's motion now and its frame; the step clip the pad asked for last.
    anim: (usize, usize),
    stepping: i8,
}

impl BowlScene {
    /// A game for `players`: each one's model record (its character file) and character id, in turn order; `seed`
    /// draws the copies' stats and the game's randomness.
    pub fn new(assets: &Assets, players: &[(usize, usize)], seed: u32) -> Result<Self> {
        let raw: serde_json::Value = assets.gamedata("bowl.json")?;
        let data: BowlData = serde_json::from_value(raw.clone()).context("bowl.json")?;
        let extra: Extra = serde_json::from_value(raw).unwrap_or_default();
        let mut draw = seed;
        let (mut characters, mut clips, mut bowlers) = (vec![], vec![], vec![]);
        for (record, id) in players {
            let stats = if COPIERS.contains(id) {
                // someone of the 32 others, at random
                draw = draw.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let others: Vec<usize> = (0..34).filter(|c| !COPIERS.contains(c)).collect();
                others.get((draw >> 16) as usize % others.len()).copied().unwrap_or(0)
            } else {
                *id
            };
            let mut b = Bowler::new(&data, *id, stats);
            let mut ch = assets.character(*record)?;
            let c = data.characters.iter().find(|c| c.id == *id).cloned().unwrap_or_default();
            let hand = if b.left { 4 } else { 0 };
            let (run, after) = (0x6A + 2 * usize::from(b.female) + usize::from(b.left), 0x6E + 2 * usize::from(b.female) + usize::from(b.left));
            let strike = c.reactions.first().map(|&m| m as usize);
            for m in [hand, hand + 1, hand + 2, hand + 3, run, after].into_iter().chain(strike) {
                let path = assets.root.join(format!("motions/bowl_{m:03}.glb"));
                // (a reaction the disc has none of, Mokujin's 0x72, is not there)
                if let Ok(bytes) = kumite_fs::read(&path) {
                    ch.add_motions(&bytes).with_context(|| format!("{}", path.display()))?;
                }
            }
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
            bowlers.push(b);
            clips.push(k);
            characters.push(Arc::new(ch));
        }
        // (a plain file name in `stages/` only: a folder in it would read outside)
        let [ball_model, pin_model, deck_model] =
            [&extra.models.ball, &extra.models.pin, &extra.models.deck].map(|m| format!("stages/{}", std::path::Path::new(m).file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned())));
        let pin_base = if extra.models.pin_base > 0.0 { extra.models.pin_base } else { 250.66 };
        let first = clips.first().map_or(0, |c| c.stand);
        Ok(Self { bowl: Bowl::new(&data, bowlers, draw), characters, clips, ball_model, pin_model, deck_model, pin_base, anim: (first, 0), stepping: 0 })
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
    /// lane (mode 0) between balls.
    // shortcut: the modes' offsets from the bowler and the ball are OpenKumite's (only their bases are decoded)
    pub fn camera(&self) -> (Vec3, Vec3) {
        let b = &self.bowl;
        let (at, a) = b.bowler_at();
        let (s, c) = a.dsin_cos();
        let ahead = |p: Vec3, d: f32| p + Vec3::new(s * d, 0.0, c * d);
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
            Phase::Strike(_) => (ahead(Vec3::new(at.x, -1600.0, at.z), 4500.0), Vec3::new(at.x, -900.0, at.z)),
            Phase::Result(_) | Phase::Over => (Vec3::new(0.0, -1500.0, 4000.0), Vec3::new(0.0, -300.0, 8000.0)),
        };
        (to_m(eye), to_m(target))
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
