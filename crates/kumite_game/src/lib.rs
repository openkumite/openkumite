//! Game layer: ties the converted assets (glTF characters, JSON game data) to the fight simulation.
//! No GPU here, so fights can be run and tested headlessly.

use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Quat, Vec3};
use kumite_assets::{Assets, Character};
use kumite_sim::data::{Moveset, MovesetIndex};
use kumite_sim::detmath::DetMath;
use kumite_sim::input::Pad;
use std::sync::Arc;
pub mod arena_motion;
pub mod bout;
pub mod bowl;
pub mod crowd;
pub mod embu;
pub mod ending;
pub mod face;
pub mod force;
pub mod hands;
pub mod head;
pub mod rumble;
pub mod scene;
pub mod sound;
pub mod tag;
pub mod tagface;
pub mod vfx;
pub mod weather;

use kumite_sim::camera::{CameraData, EndCam, FightCamera, Sample, Shot, Subject, View};
use kumite_sim::{Animations, BodyVolumes, Fight, GameData, Sphere, Turns};
use std::collections::VecDeque;

/// Converted glTF space is metres; the simulation works in game units (≈ mm).
pub const UNITS: f32 = 1000.0;
/// The replay records the last 300 frames.
pub const REPLAY_RING: usize = 300;

/// A fighter's converted character plus what the simulation needs from it.
pub struct Body {
    pub character: Character,
    pub model_record: usize,
    /// Root node (carries the root-motion translation track).
    pub root: usize,
    /// Hurt spheres: (node index, world-up offset in metres, radius in game units, zone).
    pub hurt: Vec<(usize, Vec3, f32, usize)>,
    /// Push spheres, same layout.
    pub push: Vec<(usize, Vec3, f32, usize)>,
    /// Bone slot 2 ("head" joint, the game's chest height +0x994): the camera samples its height.
    pub chest: usize,
    /// The "chest" joint (bone slot 1): the frame the head aims from.
    pub torso: usize,
    /// The "pelvis" joint (bone slot 11): its place on the screen is the fighter's side (+0x1278).
    pub pelvis: usize,
    /// Nodes not drawn (the copying character's stick, but as the sword fighter).
    pub hidden: Vec<usize>,
    /// Tekken Tag: the nodes of the 18 bone slots its strikes run between ([`BONE_SLOTS`]); empty for Tekken 3.
    pub bones: Vec<usize>,
    /// Tekken Tag: its strike slots past those ([`strike_slots`]).
    pub extra: Vec<(usize, usize, Vec3)>,
    /// Tekken Tag: each of those 18 slots' parent slot and its place in the parent's axes (metres), the game's
    /// (rules.json `joints`); empty when the data has none.
    pub joints: Vec<(usize, Vec3)>,
    /// Tekken Tag: its hands' poses (None: Tekken 3, or a pack without them).
    pub hands: Option<hands::Poses>,
    /// Tekken Tag: its face's poses (None: Tekken 3, a masked model, or a pack without them).
    pub face: Option<tagface::Poses>,
    /// Tekken Tag: its scale of the motions' root, 4096 = 1 (the moveset index's `motion_scale`).
    pub motion_scale: i32,
    /// Tekken Tag: its spring slots ([`springs`]).
    pub springs: Vec<kumite_sim::Spring>,
}

/// The joints of the bone slots, slot by slot (the arcade engine's matrix slots; Tekken Tag's converter names them so).
pub const BONE_SLOTS: [&str; 18] = [
    "root",
    "chest",
    "head",
    "clavicle_A",
    "upperarm_A",
    "forearm_A",
    "hand_A",
    "clavicle_B",
    "upperarm_B",
    "forearm_B",
    "hand_B",
    "pelvis",
    "thigh_A",
    "shin_A",
    "foot_A",
    "thigh_B",
    "shin_B",
    "foot_B",
];

impl Body {
    /// Root translation of `anim` at `frame` (game units).
    fn root_of(&self, anim: &str, frame: u32) -> Option<Vec3> {
        let clip = &self.character.clips[self.character.clip(anim)?];
        let t = clip.translations[self.root].as_ref()?;
        Some(t[(frame as usize).min(t.len() - 1)] * UNITS)
    }
}

/// One fighter's animations: its own, and the opponent's for borrowed (throw-victim) moves.
struct FighterAnims<'a> {
    own: &'a Body,
    other: &'a Body,
    /// Its spring slots' last frame (Game::springs).
    springs: &'a [Spring],
}

/// A spring slot's turn in its parent's axes and its tip's place (Body::springs).
type Spring = (kumite_sim::Turn, [i32; 3]);

impl Animations for FighterAnims<'_> {
    fn root(&self, anim: &str, frame: u32, partner: bool) -> Option<Vec3> {
        if partner {
            self.other.root_of(anim, frame)
        } else {
            self.own.root_of(anim, frame)
        }
    }

    fn root_scale(&self, partner: bool) -> i32 {
        if partner {
            self.other.motion_scale
        } else {
            self.own.motion_scale
        }
    }

    fn turns(&self, anim: &str, frame: u32, partner: bool) -> Option<Turns> {
        let src = if partner { self.other } else { self.own };
        let (nodes, clip) = (&self.own.character.nodes, src.character.clips.get(src.character.clip(anim)?)?);
        if let Some(t) = angle_turns(clip, frame as usize) {
            return (self.own.bones.len() >= 18).then_some(t);
        }
        let mut turns = [[0; 9]; 18];
        for (t, &n) in turns.iter_mut().zip(&self.own.bones) {
            let node = nodes.get(n)?;
            let j = if partner { src.character.nodes.iter().position(|o| o.name == node.name) } else { Some(n) };
            let r = j.and_then(|j| clip.rotations.get(j)?.as_ref()).and_then(|v| v.get((frame as usize).min(v.len().saturating_sub(1)))).copied().unwrap_or(node.rotation);
            let c = Mat3::from_quat(r).to_cols_array_2d();
            *t = std::array::from_fn(|k| fixed(c[k % 3][k / 3]) as i16);
        }
        (self.own.bones.len() >= 18).then_some(turns)
    }

    fn volumes(&self, f: &kumite_sim::Fighter, gd: &kumite_sim::GameData) -> Option<BodyVolumes> {
        if self.own.bones.is_empty() {
            return None;
        }
        let partner = f.current.bank == kumite_sim::data::Bank::Partner;
        let other = if partner { self.other } else { self.own };
        let (anim, frame) = f.animation(gd);
        let shown = Snapshot { clip: other.character.clip(anim).map(|c| (c, frame as usize)), partner, root: f.root(), yaw: f.yaw, ease: f.eased() };
        let (mut pose, place) = shown_pose(self.own, other, &shown);
        step_springs(self.own, &mut pose, place, self.springs);
        Some(volumes(self.own, &pose, place))
    }
}

/// What one fighter showed on one frame: enough to pose and place it again (the replay's record).
#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    /// Clip (in its owner's character) and frame.
    pub clip: Option<(usize, usize)>,
    /// The clip is the opponent's (a borrowed throw-victim move).
    pub partner: bool,
    pub root: Vec3,
    pub yaw: f32,
    /// Tekken Tag's pose ease: the joint slots' turns and the root's lift (game units) eased.
    pub ease: Option<(Turns, f32)>,
}

/// Frames a secondary joint takes to go over from the last clip's pose to the new one.
pub const SECONDARY_BLEND: u32 = 8;

/// The joints that hang or flap (the extra fighters' chains `chain*` and wing vertices `wing*`) carry their motion
/// baked into each clip, where the game keeps swinging them across moves. When the clip changes they go over from the
/// pose last shown to the new clip's over [`SECONDARY_BLEND`] frames (drawing only), each on its parent as the clip
/// poses it (Tekken 2's links hang from their bone's world matrix, 0x800811C4: the tail stays on the hips).
#[derive(Clone, Debug, Default)]
pub struct SecondaryBlend {
    clip: Option<(usize, bool)>,
    frame: u64,
    left: u32,
    /// The joints' matrices on their parents.
    from: Vec<(usize, Mat4)>,
    last: Vec<(usize, Mat4)>,
}

impl SecondaryBlend {
    /// `pose`: the character's node matrices for the clip shown at game frame `frame` (changed in place).
    pub fn apply(&mut self, character: &kumite_assets::Character, clip: Option<(usize, bool)>, frame: u64, pose: &mut [Mat4]) {
        let nodes = &character.nodes;
        let mut joints: Vec<usize> = nodes.iter().enumerate().filter(|(_, n)| n.name.starts_with("chain") || n.name.starts_with("wing")).map(|(i, _)| i).collect();
        if joints.is_empty() {
            return;
        }
        // (parents first: a link's world is its blended parent's times its own)
        joints.sort_by_key(|&j| std::iter::successors(Some(j), |&n| nodes.get(n).and_then(|n| n.parent)).take(nodes.len() + 1).count());
        let parent = |pose: &[Mat4], j: usize| nodes.get(j).and_then(|n| n.parent).and_then(|p| pose.get(p).copied()).unwrap_or(Mat4::IDENTITY);
        let mut local: Vec<(usize, Mat4)> = joints.iter().filter_map(|&j| Some((j, parent(pose, j).inverse() * *pose.get(j)?))).filter(|(_, m)| m.is_finite()).collect();
        if frame != self.frame {
            self.frame = frame;
            if clip != self.clip {
                self.clip = clip;
                self.from = self.last.clone();
                self.left = SECONDARY_BLEND;
            } else {
                self.left = self.left.saturating_sub(1);
            }
        }
        if self.left > 0 {
            let t = 1.0 - self.left as f32 / (SECONDARY_BLEND + 1) as f32;
            for (j, m) in &mut local {
                if let Some(&(_, a)) = self.from.iter().find(|f| f.0 == *j) {
                    let ((sa, ra, ta), (sb, rb, tb)) = (a.to_scale_rotation_translation(), m.to_scale_rotation_translation());
                    *m = Mat4::from_scale_rotation_translation(sa.lerp(sb, t), ra.slerp(rb, t), ta.lerp(tb, t));
                }
                let world = parent(pose, *j) * *m;
                if let Some(p) = pose.get_mut(*j) {
                    *p = world;
                }
            }
        }
        self.last = local;
    }
}

/// Tekken Tag's swaying nodes (belts, hair, cloth, tails: [`kumite_assets::ClothNode`]) as 0x34D258 moves them each
/// drawn frame (drawing only). A node's tip (its length along its x) is averaged over its `frames` from where last
/// frame's matrix put it; the node turns towards `keep` x that tip's direction + `gravity` x down + the rest x its bind
/// direction (in its bind axes: about z, then y, each within its limits; bit 2, about y, then z), then a tip under the
/// floor is laid on it.
/// shortcut: no collision with the body's capsules (pack part 9) nor the arena's wind (0x34E018), upgrade when a
/// belt through a leg shows
#[derive(Clone, Debug, Default)]
pub struct Cloth {
    frame: Option<u64>,
    /// Each node's averaged tip and its last world matrix.
    last: Vec<(Vec3, Mat4)>,
}

impl Cloth {
    /// `pose`: the character's node matrices (character space) for game frame `frame`, `place` its world matrix;
    /// the swaying nodes changed in place. A frame shown again keeps its matrices (a pause holds them).
    pub fn apply(&mut self, character: &Character, frame: u64, place: Mat4, pose: &mut [Mat4]) {
        let cloth = &character.cloth;
        let to_char = place.inverse();
        if cloth.is_empty() || !to_char.is_finite() {
            return;
        }
        let first = self.last.len() != cloth.len() || self.frame.is_none_or(|f| frame < f || frame > f + 2);
        if self.frame == Some(frame) && !first {
            for (c, &(_, m)) in cloth.iter().zip(&self.last) {
                if let Some(p) = pose.get_mut(c.node) {
                    *p = to_char * m;
                }
            }
            return;
        }
        self.frame = Some(frame);
        if first {
            self.last = vec![(Vec3::ZERO, Mat4::IDENTITY); cloth.len()];
        }
        let nodes = &character.nodes;
        for (c, last) in cloth.iter().zip(self.last.iter_mut()) {
            let (Some(n), Some(parent)) = (nodes.get(c.node), nodes.get(c.node).and_then(|n| n.parent).and_then(|p| pose.get(p).copied())) else { continue };
            let rest = place * parent * Mat4::from_rotation_translation(n.rotation, n.translation);
            let tip = Vec3::X * c.length;
            let mut world = rest;
            if first {
                last.0 = rest.transform_point3(tip);
            } else {
                let frames = c.frames.max(1) as f32;
                last.0 = (last.0 * (frames - 1.0) + last.1.transform_point3(tip)) / frames;
                let back = Mat3::from_mat4(rest).inverse();
                let towards = (back * (last.0 - rest.w_axis.truncate())).normalize_or_zero();
                let v = towards * c.keep + back * Vec3::NEG_Y * c.gravity + Vec3::X * (1.0 - c.keep - c.gravity);
                world = rest * cloth_turn(c, v, true);
                // a tip under the floor: laid on it, as far out as its length reaches
                let (pivot, end) = (rest.w_axis.truncate(), world.transform_point3(tip));
                if end.y < 0.0 {
                    let flat = Vec3::new(end.x - pivot.x, 0.0, end.z - pivot.z).normalize_or_zero();
                    let out = (c.length * c.length - pivot.y * pivot.y).max(0.0).sqrt();
                    world = rest * cloth_turn(c, back * Vec3::new(flat.x * out, -pivot.y, flat.z * out), false);
                }
            }
            last.1 = world;
            if let Some(p) = pose.get_mut(c.node) {
                *p = to_char * world;
            }
        }
    }
}

/// The turn taking a node's x to `v` (its bind axes), within its limits when `limited`.
fn cloth_turn(c: &kumite_assets::ClothNode, v: Vec3, limited: bool) -> Mat4 {
    let clamp = |a: f32, max: f32, min: f32| if limited && max != min { a.min(max).max(min) } else { a };
    let [ymax, ymin, zmax, zmin] = c.limits;
    if c.flags & 4 != 0 {
        let y = clamp((-v.z).datan2(v.x), ymax, ymin);
        let z = clamp(v.y.datan2((v.x * v.x + v.z * v.z).sqrt()), zmax, zmin);
        Mat4::from_rotation_y(y) * Mat4::from_rotation_z(z)
    } else {
        let y = clamp((-v.z).datan2((v.x * v.x + v.y * v.y).sqrt()), ymax, ymin);
        let z = clamp(v.y.datan2(v.x), zmax, zmin);
        Mat4::from_rotation_z(z) * Mat4::from_rotation_y(y)
    }
}

/// Who drives the camera.
#[derive(Clone, Debug)]
pub enum CameraMode {
    /// The versus fight camera (fly-in, then the director).
    Fight,
    /// The replay camera (0x80069414), on the shown (replayed) poses.
    Replay,
    /// A round-end camera on fighter `who`.
    End { cam: EndCam, who: usize },
    /// The view stays where it is (after a double K.O.'s replay).
    Frozen,
    /// A scripted shot of Tekken Tag's (the team intro, a K.O. replay pass), its target following fighter `follow`'s
    /// root.
    Shot { shot: Shot, follow: Option<usize> },
}

/// A fight between two characters, loaded from the asset folder.
#[derive(Clone)]
pub struct Game {
    pub fight: Fight,
    /// The two characters' meshes, skeletons and volumes (shared: they never change during a fight).
    pub bodies: [Arc<Body>; 2],
    /// The versus fight camera, stepped with the fight (round-start fly-in first).
    pub camera: FightCamera,
    pub camera_mode: CameraMode,
    /// Fly-in settings for [`Game::reset_round`]: (variant, first round).
    intro: Option<(usize, bool)>,
    /// The last frames as shown (the replay ring, 300 frames), newest last.
    pub recorded: VecDeque<[Snapshot; 2]>,
    /// Frames recorded since the round started.
    pub recorded_total: u64,
    /// Shown instead of the live fighters (the replay).
    pub display: Option<[Snapshot; 2]>,
    /// Which fighters are drawn (the win pose shows only the winner, the loser shot only the loser).
    pub visible: [bool; 2],
    /// Full health per fighter in percent (the VS LIFE handicap, 70..140), applied at every round start.
    pub handicap: [i32; 2],
    /// Full health per fighter set by the battle type (0x800516CC: Team 170, Time Attack 130, Survival 140 ...);
    /// None: the default maximum with the handicap.
    pub max_health: [Option<i32>; 2],
    /// Health (16.16) a fighter starts the next round with when it carries over (Team, Survival: ctx+9); used
    /// once, when it is non-zero and at most the maximum, as 0x8002BE74 does.
    pub start_health: [Option<i32>; 2],
    /// The round clock in frames (the ROUND TIME option, (option + 2) * 600; Time Attack and Survival 40 s).
    pub round_frames: u32,
    /// Ball: the ball's data, its type, and the side that serves next (the loser of the last round).
    pub ball: Option<BallSetup>,
    /// Force's third actor's body (the fight's `extra` fighter) and its pad (from its CPU).
    pub extra_body: Option<Arc<Body>>,
    pub extra_pad: Pad,
    /// Force's co-op partner's body (the fight's `partner`, actor 3) and its pad (the second player's).
    pub partner_body: Option<Arc<Body>>,
    pub partner_pad: Pad,
    /// Force for more than two (OpenKumite's own): actors 4.. (3P, 4P, then more soldiers and bosses), their
    /// bodies (None: a place nobody fills) and pads.
    pub more_body: Vec<Option<Arc<Body>>>,
    pub more_pad: Vec<Pad>,
    /// Force: the roast chickens within reach (simulation positions; the stage's logic sets them), for a CPU player.
    pub pickups: Vec<Vec3>,
    /// Ball 2 ON 2: the partner of each side's fighter (the players' side's, then the other's), put on the
    /// court with them every round (actors 3 and 2).
    pub ball_team: Option<[Actor; 2]>,
    /// Which of the two partners a person plays (it guards by holding back).
    pub ball_team_human: [bool; 2],
    /// Ball's replay ring: the ball of the last 30 frames (EXE 0x800337A0), and the one shown in a replay.
    ball_ring: VecDeque<kumite_sim::ball::Ball>,
    pub display_ball: Option<kumite_sim::ball::Ball>,
    /// Each fighter's character id (roster.json selections; cancel `req` gates).
    characters: [Option<i64>; 2],
    /// The heads turning to the opponent (0x80039074): per fighter, while the round is on (`head_live`, the
    /// round state 2), not for Force's CPU soldiers (`head_off`, actor +0xC9).
    pub heads: [head::HeadLook; 2],
    pub head_live: bool,
    pub head_off: [bool; 2],
    /// Tekken Tag's look instead (bodies with bone slots; 0x351E98, drawn only): per side.
    pub looks: [head::TagLook; 2],
    /// Blinks and the moving pupils (faces.json), and the rand() stream the blinks draw from.
    pub faces: [face::Face; 2],
    /// Tekken Tag's fingers, and whether the round is in round states 1-2 (the fight, the K.O.: gp-0x7268).
    pub hands: [hands::Hands; 2],
    pub hand_live: bool,
    /// Tekken Tag's faces: blinks and expressions.
    pub tag_faces: [tagface::Face; 2],
    /// Tekken Tag's wings (True Ogre's and Devil's).
    pub wings: [face::WingState; 2],
    /// Tekken Tag's Unknown's wolf: its root's place (world, metres) as 0x34CC38 eases it; none: at once (the round's
    /// first frame, 0x34CB50).
    pub wolves: [Option<Vec3>; 2],
    /// Tekken Tag's spring slots (Body::springs): each one's turn in its parent's axes and its tip's place.
    pub springs: [Vec<Spring>; 2],
    /// The mode's K.O. style (Fight::ko_collapse), kept across rounds.
    pub ko_collapse: bool,
    /// Tekken Tag's GUARD DAMAGE option (Fight::guard_damage), kept across rounds.
    pub guard_damage: bool,
    /// Tekken Tag's members carried into a tag battle by side (SURVIVAL): the lead's and the partner's (health, cap),
    /// 16.16; the lead's health also as `start_health`.
    pub tag_carry: [Option<[(i32, i32); 2]>; 2],
    pub face_data: Arc<face::FaceData>,
    face_rand: u32,
    /// The copying character's forms, for each fighter that is one.
    pub copiers: [Option<Copier>; 2],
    /// Added to the copying character's draws: the match's own (a bout's random stream starts the same every
    /// match; a replay keeps it, online every computer has the room's).
    pub copy_seed: u32,
    /// Tekken Tag's tag battle: each side's benched partner (its body and character trade places with the fighter's
    /// when the team swaps, [`Game::start_tag`]).
    pub tag_bench: Option<[Actor; 2]>,
    /// Tekken Tag's round machine (rounds.json); None: Tekken 3's.
    pub rounds: Option<bout::RoundRules>,
    /// A tag battle's win pose: the side whose benched partner poses in the loser's place (0x3B5DB0).
    pub posing_partner: Option<usize>,
    /// Each model record's costume number (roster.json selections; Tekken Tag's team intro pairs).
    costume_of: Arc<[(usize, i64)]>,
}

/// A character's body and move set by model record.
/// The files loading model `record` reads beyond the shared game data: its glTF and its moveset (what a browser
/// fetches ahead of a fight).
pub fn model_files(assets: &Assets, record: usize) -> Vec<std::path::PathBuf> {
    let gd = assets.root.join("gamedata");
    let mut files = vec![assets.root.join(format!("characters/model_{record:03}.glb"))];
    if let Ok(index) = kumite_sim::data::load::<MovesetIndex>(&gd.join("movesets/index.json")) {
        if let Some(e) = index.models.iter().find(|m| m.model_record == record) {
            files.push(gd.join("movesets").join(&e.moveset_file));
            // (the copying character's first fighter; a browser reads the later rounds' as they come)
            if let Some(d) = index.models.iter().find(|m| m.moveset_id == COPY_DEFAULT).filter(|_| e.moveset_id == COPIER) {
                files.extend([assets.root.join("characters").join(&d.glb), gd.join("movesets").join(&d.moveset_file)]);
            }
        }
    }
    files
}

/// `common`: the common set's move count.
fn load_body(assets: &Assets, record: usize, rules: &serde_json::Value, index: &MovesetIndex, common: usize) -> Result<(Body, Arc<Moveset>)> {
    let gd = assets.root.join("gamedata");
    let entry = index.models.iter().find(|m| m.model_record == record).with_context(|| format!("model {record} not in the moveset index"))?;
    let path = gd.join("movesets").join(&entry.moveset_file);
    // Tekken Tag (rules.json's dialect): a moveset is aliases into the common set (a crossover fighter's: its own
    // moves too), the motions are files of their own
    let tag = rules["dialect"] == "ttt";
    // the radii are per character id (0x8003F048: 0x80096FF0[+0x18]); the roster gives the model's character
    let roster: serde_json::Value = kumite_sim::data::load(&gd.join("roster.json")).unwrap_or_default();
    let id = roster["selections"]["entries"].as_array().into_iter().flatten().find(|e| e["model_record"].as_u64() == Some(record as u64)).and_then(|e| e["character"].as_u64()).unwrap_or(0);
    // (Tekken Tag's copying character has no moves of its own: it never plays them)
    let moveset = if tag && id == COPIER as u64 {
        None
    } else if tag {
        Some(kumite_sim::dialect::moveset(common, &path)?)
    } else {
        let moveset = kumite_sim::data::load::<Moveset>(&path)?;
        moveset.check(common).with_context(|| format!("{}", path.display()))?;
        Some(moveset)
    };
    let mut character = assets.character(record)?;
    if tag {
        let list: serde_json::Value = kumite_sim::data::load(&gd.join(format!("movesets/motions_{:02}.json", entry.moveset_id)))?;
        for f in list["motions"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
            let file = assets.root.join("motions").join(f);
            character.add_motions(&kumite_fs::read(&file).with_context(|| format!("reading {}", file.display()))?).with_context(|| format!("{}", file.display()))?;
        }
    }
    // Tekken Tag's hand poses (an older pack has none: the fingers stay as bound)
    if let Some(bytes) = entry.hands.as_ref().and_then(|h| kumite_fs::read(&assets.root.join("characters").join(&h.file)).ok()) {
        character.add_motions(&bytes).with_context(|| format!("the hands of model {record}"))?;
    }
    // its face poses likewise (none: the face stays as bound)
    if let Some(bytes) = entry.face.as_ref().and_then(|f| kumite_fs::read(&assets.root.join("characters").join(&f.file)).ok()) {
        character.add_motions(&bytes).with_context(|| format!("the face of model {record}"))?;
    }
    let hands = entry.hands.as_ref().and_then(|h| hands::Poses::new(&character, h));
    let face = entry.face.as_ref().and_then(|f| tagface::Poses::new(&character, f));
    let root = character.nodes.iter().position(|n| n.parent.is_none()).context("no root node")?;
    let hurt = sphere_layout(&rules["hurt_spheres"], &character, id)?;
    let push = sphere_layout(&rules["push_spheres"], &character, id)?;
    let chest = character.nodes.iter().position(|n| n.name == "head").context("no head joint")?;
    let torso = character.nodes.iter().position(|n| n.name == "chest").context("no chest joint")?;
    let pelvis = character.nodes.iter().position(|n| n.name == "pelvis").unwrap_or(root);
    let bones = if tag { BONE_SLOTS.iter().map(|&j| character.nodes.iter().position(|n| n.name == j).with_context(|| format!("no joint {j}"))).collect::<Result<_>>()? } else { Vec::new() };
    let extra = if tag { strike_slots(id, entry.moveset_id) } else { Vec::new() };
    let joints = if tag { joint_places(&rules["joints"], record, id) } else { Vec::new() };
    let motion_scale = entry.motion_scale.unwrap_or(4096);
    let hidden = blades_hidden(&character, entry.moveset_id);
    let body =
        Body { character, model_record: record, root, hurt, push, chest, torso, pelvis, hidden, bones, extra, joints, hands, face, motion_scale, springs: if tag { springs(id) } else { Vec::new() } };
    // the copying character: its own moveset is never played; outside a bout's rounds it stays the setup's fighter
    // shortcut: Force's bosses and the tag brawl's draw their fighter too (0x800B2770), here they keep this one,
    // and Force's player draws without a seed of the match (the same fighters every run)
    if id == COPIER as u64 {
        if let Some(e) = index.models.iter().find(|m| m.moveset_id == COPY_DEFAULT) {
            if tag {
                let (donor, moves) = load_body(assets, e.model_record, rules, index, common)?;
                return Ok((copy_body(&body, &donor.character, COPY_DEFAULT), moves));
            }
            let donor = kumite_sim::data::load::<Moveset>(&gd.join("movesets").join(&e.moveset_file))?;
            donor.check(common).with_context(|| e.moveset_file.clone())?;
            return Ok((copy_body(&body, &assets.character(e.model_record)?, COPY_DEFAULT), Arc::new(donor)));
        }
    }
    Ok((body, Arc::new(moveset.context("the copying character has no fighter to copy")?)))
}

/// A body ready to be swapped into a fight (Force's soldiers and bosses).
#[derive(Clone)]
pub struct Actor {
    pub body: Arc<Body>,
    pub moveset: Arc<Moveset>,
    pub character: Option<i64>,
}

impl Actor {
    pub fn load(assets: &Assets, record: usize) -> Result<Self> {
        let gd = assets.root.join("gamedata");
        let rules: serde_json::Value = kumite_sim::data::load(&gd.join("rules.json"))?;
        let index: MovesetIndex = kumite_sim::data::load(&gd.join("movesets/index.json"))?;
        // (Tekken Tag: the whole move table is the common set)
        let common = kumite_sim::data::move_count(&gd.join(if rules["dialect"] == "ttt" { "moves.json" } else { "movesets/common.json" }))?;
        let (body, moveset) = load_body(assets, record, &rules, &index, common)?;
        let roster: serde_json::Value = kumite_sim::data::load(&gd.join("roster.json")).unwrap_or_default();
        let character = roster["selections"]["entries"].as_array().into_iter().flatten().find(|e| e["model_record"].as_u64() == Some(record as u64)).and_then(|e| e["character"].as_i64());
        Ok(Self { body: Arc::new(body), moveset, character })
    }
}

/// The copying character (character id [`COPIER`]): each round it fights with another fighter's moves and
/// animations on its own body (0x8004F2DC writes only its moveset id, actor +0x16), its stick in hand only as the
/// sword fighter (0x8003615C: drawn while +0x16 is that moveset).
#[derive(Clone)]
pub struct Copier {
    assets: Assets,
    own: Arc<Body>,
    /// The fighters it can become: (moveset id, a model record with that moveset).
    pub pool: Vec<(i64, usize)>,
    /// The forms made so far, by model record: shared by the game's copies, so a rollback over a round's start
    /// takes its form from here instead of reading the files again.
    made: Arc<std::sync::Mutex<Vec<(usize, Actor)>>>,
}

/// The movesets it takes (0x8004F2DC: characters 0..13 and 16 of the roster, mask 0x13FFF; their moveset ids are
/// their character ids).
// shortcut: the original takes only the fighters unlocked so far; here always all (two computers online, and a
// replay played elsewhere, must draw from the same list), upgrade if the unlocks ever travel with the match
const COPY_POOL: [i64; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16];
/// Tekken Tag's (0x35C850: Mokujin draws among the select's slots flagged in 0x51E8A0, the unlocked ones, and takes
/// their moveset: not Ogre, True Ogre, Roger, Devil nor Unknown; slots 16, 18-19, 21-28, 30-32 have movesets 16-26,
/// 28-30).
const COPY_POOL_TTT: [i64; 28] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 28, 29, 30];
/// The moveset the fight's setup gives it before the first round's draw (0x8002A40C: its descriptor's).
pub const COPY_DEFAULT: i64 = 9;
/// The copying character's id (actor +0x18).
pub const COPIER: i64 = 15;
/// The moveset whose fighter holds a sword: the copying character's stick shows with it.
const SWORD_MOVESET: i64 = 4;

impl Copier {
    fn new(assets: &Assets, own: Arc<Body>, index: &MovesetIndex, tag: bool) -> Self {
        let mut pool: Vec<(i64, usize)> = Vec::new();
        let draws: &[i64] = if tag { &COPY_POOL_TTT } else { &COPY_POOL };
        for m in &index.models {
            if draws.contains(&m.moveset_id) && !pool.iter().any(|p| p.0 == m.moveset_id) {
                pool.push((m.moveset_id, m.model_record));
            }
        }
        pool.sort_unstable();
        // (a browser fetches every fighter it can become while the first round is on; a round that comes before one
        // has arrived reads it at once, the page waiting)
        let files = index.models.iter().filter(|m| pool.iter().any(|p| p.1 == m.model_record));
        kumite_fs::prefetch(&files.flat_map(|m| [assets.root.join("characters").join(&m.glb), assets.root.join("gamedata/movesets").join(&m.moveset_file)]).collect::<Vec<_>>());
        Self { assets: assets.clone(), own, pool, made: Default::default() }
    }

    /// The copying character as the pool's fighter `k`: its own body playing that fighter's clips, with that
    /// fighter's moveset. None when that fighter's files cannot be read (the form stays as it is).
    fn form(&self, k: usize) -> Option<Actor> {
        let (id, record) = *self.pool.get(k)?;
        let mut made = self.made.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = made.iter().find(|m| m.0 == record) {
            return Some(m.1.clone());
        }
        let donor = Actor::load(&self.assets, record).map_err(|e| log::warn!("the copying character as model {record}: {e:#}")).ok()?;
        let actor = Actor { body: Arc::new(copy_body(&self.own, &donor.body.character, id)), moveset: donor.moveset, character: Some(COPIER) };
        made.push((record, actor.clone()));
        Some(actor)
    }
}

/// The copying character's body `own` playing `donor`'s clips, as the fighter of moveset `id`: its accessory nodes
/// (`bone_*`: the stick) hidden but with the sword fighter's.
fn copy_body(own: &Body, donor: &Character, id: i64) -> Body {
    let hidden = copy_hidden(&own.character, id);
    Body {
        character: own.character.with_clips_of(donor),
        model_record: own.model_record,
        root: own.root,
        hurt: own.hurt.clone(),
        push: own.push.clone(),
        chest: own.chest,
        torso: own.torso,
        pelvis: own.pelvis,
        hidden,
        bones: own.bones.clone(),
        extra: if own.bones.is_empty() { Vec::new() } else { strike_slots(COPIER as u64, id) },
        joints: own.joints.clone(),
        hands: own.hands.clone(),
        face: own.face.clone(),
        motion_scale: own.motion_scale,
        springs: Vec::new(),
    }
}

/// Tekken Tag's blades not drawn with moveset `id` (0x34D108: scaled to nothing).
/// The copying character's nodes hidden as the fighter of moveset `id`: its accessories (`bone_*`: the stick) but as
/// the sword fighter, and the blades of the other movesets.
pub fn copy_hidden(c: &Character, id: i64) -> Vec<usize> {
    let mut hidden = if id == SWORD_MOVESET { Vec::new() } else { c.nodes.iter().enumerate().filter(|(_, n)| n.name.starts_with("bone_")).map(|(i, _)| i).collect() };
    hidden.extend(blades_hidden(c, id));
    hidden
}

fn blades_hidden(c: &Character, id: i64) -> Vec<usize> {
    c.blades.iter().filter(|b| b[1] as i64 != id).map(|b| b[0]).collect()
}

/// A body's joint places (rules.json `joints`: its model's own, else its body's): each slot's parent and place.
fn joint_places(j: &serde_json::Value, record: usize, id: u64) -> Vec<(usize, Vec3)> {
    let find = |key: &str, field: &str, v: u64| j[key].as_array().into_iter().flatten().find(|e| e[field].as_u64() == Some(v)).cloned();
    let Some(set) = find("per_model", "model_record", record as u64).or_else(|| find("per_character", "character", id)) else { return Vec::new() };
    let f = |p: &serde_json::Value, k: usize| p[k].as_f64().unwrap_or(0.0) as f32;
    let parents = j["parents"].as_array().into_iter().flatten();
    let places = set["places"].as_array().into_iter().flatten();
    parents.zip(places).enumerate().map(|(s, (p, at))| (p.as_u64().map_or(s, |p| p as usize), Vec3::new(f(at, 0), f(at, 1), f(at, 2)) / UNITS)).collect()
}

/// Tekken Tag's spring slots (0x34E640's tables by body): Roger's tail and True Ogre's 20 (his 18 and 19 are his
/// wings' instead, 0x3626B8).
fn springs(body: u64) -> Vec<kumite_sim::Spring> {
    let at = |slot, rest, len, lag, down| kumite_sim::Spring { slot, rest, len, lag, down };
    match body {
        17 => vec![at(18, [16615, 125, 9392], 128, 128, 24), at(19, [1, 110, 370], 128, 128, 24), at(20, [-6, -258, -386], 128, 128, 24)],
        20 => vec![at(20, [0, 0, 9658], 128, 1, 0)],
        _ => Vec::new(),
    }
}

/// Tekken Tag's spring slots (0x34C508, each frame after the moves, before the strikes) one after the other (a slot
/// on a spring follows its new turn): each one's turn set in `pose`, from their last frame `last`; their new state.
fn step_springs(body: &Body, pose: &mut [Mat4], place: Mat4, last: &[Spring]) -> Vec<Spring> {
    let c = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
    let mut out = Vec::with_capacity(body.springs.len());
    for (k, sp) in body.springs.iter().enumerate() {
        let Some(n) = slot_node(body, sp.slot) else { break };
        let (Some(&parent), Some(&own)) = (body.character.nodes[n].parent.and_then(|p| pose.get(p)), pose.get(n)) else { break };
        // (the game's world axes: y down, z flipped; the node frames are the game's)
        let r = c * Mat3::from_mat4(place * parent);
        let turn: kumite_sim::Turn = std::array::from_fn(|row| std::array::from_fn(|col| (r.col(col)[row] * 4096.0).round() as i32));
        let at = c * place.transform_point3(own.w_axis.truncate()) * UNITS;
        let (local, tip) = sp.step(&turn, [at.x, at.y, at.z].map(|v| v.round() as i32), last.get(k).map(|s| s.1));
        head::set_local(&body.character.nodes, pose, n, turn_mat3(&local));
        out.push((local, tip));
    }
    out
}

/// A game turn (rows of 4096) as the node frames' matrix.
fn turn_mat3(t: &kumite_sim::Turn) -> Mat3 {
    Mat3::from_cols_array_2d(&t.map(|r| r.map(|v| v as f32 / 4096.0))).transpose()
}

/// The drawn node of strike slot `slot` past 17 (18-20): its joint's child nearest its place.
fn slot_node(body: &Body, slot: usize) -> Option<usize> {
    let &(_, joint, at) = body.extra.iter().find(|e| e.0 == slot && (18..=20).contains(&slot))?;
    let parent = if joint < body.bones.len() { body.bones.get(joint).copied() } else { slot_node(body, joint) }?;
    let nodes = &body.character.nodes;
    let d = |n: usize| nodes[n].translation.distance(at);
    (0..nodes.len()).filter(|&n| nodes[n].parent == Some(parent) && d(n) < 0.03).min_by(|&a, &b| d(a).total_cmp(&d(b)))
}

/// Tekken Tag's strike slots past the 18 joints (player +0x630 + 0x50 a slot) a body has with a moveset: (slot, the
/// joint slot it hangs on, its place in that joint's axes, metres).
/// 0x34F3D8 puts them in before each frame's strikes (0x5F5E50): the sword fighter's (moveset 4) 22 and 23 on hand B,
/// moveset 26's and 32's 22 on hand A, Ogre's moveset's 23 on the head of bodies 20 and 15. 0x3AFAA0 adds joints
/// (0x439A80's places, the drawn glbs' nodes 98-100): True Ogre's 18 on hand A, 19 on 18, 20 on the pelvis (PCSX2: his
/// 20 behind the hips); Roger's tail, 18 on the pelvis, 19 on 18, 20 on 19. A slot on a slot past 17 is turned as
/// that one's node.
/// Roger's three and True Ogre's 20 sway on springs instead ([`springs`]).
/// shortcut: True Ogre's wing code (0x3626B8: his 18 / 19, and his slots 5 / 6, flap by a counter and his rise) is left
/// out, his 18 / 19 take arm A's tracks (PCSX2's are some 600 from these); port it with his wings' flaps
fn strike_slots(body: u64, moveset: i64) -> Vec<(usize, usize, Vec3)> {
    let at = |slot, joint, [x, y, z]: [f32; 3]| (slot, joint, Vec3::new(x, y, z) / UNITS);
    let mut out = Vec::new();
    match moveset {
        4 => out.extend([at(22, 10, [100.0, 0.0, 720.0]), at(23, 10, [100.0, 0.0, 360.0])]),
        26 | 32 => out.push(at(22, 6, [100.0, 0.0, -400.0])),
        14 if body == 20 || body == 15 => out.push(at(23, 2, [800.0, 0.0, 600.0])),
        _ => {}
    }
    match body {
        20 => out.extend([at(18, 6, [273.0, 2.0, 0.0]), at(19, 18, [171.0, 0.0, 0.0]), at(20, 11, [368.0, 510.0, 0.0])]),
        17 => out.extend([at(18, 11, [175.0, 218.0, 0.0]), at(19, 18, [368.0, 0.0, 0.0]), at(20, 19, [328.0, 0.0, 0.0])]),
        _ => {}
    }
    out
}

/// Ball's setup kept between rounds.
#[derive(Clone)]
pub struct BallSetup {
    pub data: Arc<kumite_sim::ball::BallData>,
    pub kind: usize,
    pub server: usize,
    /// Rounds started (the first serve waits longer).
    pub rounds: u32,
}

impl Game {
    pub fn load(assets: &Assets, models: [usize; 2]) -> Result<Self> {
        let gd = assets.root.join("gamedata");
        let rules: serde_json::Value = kumite_sim::data::load(&gd.join("rules.json"))?;
        let index: MovesetIndex = kumite_sim::data::load(&gd.join("movesets/index.json"))?;
        let data = Arc::new(GameData::load(&gd)?);
        let mut movesets = Vec::new();
        let mut bodies = Vec::new();
        for record in models {
            let (body, moveset) = load_body(assets, record, &rules, &index, data.common.moves.len())?;
            movesets.push(moveset);
            bodies.push(body);
        }
        let [b0, b1]: [Body; 2] = bodies.try_into().map_err(|_| anyhow::anyhow!("two fighters"))?;
        let camera = gd.join("camera.json");
        let camera_data = Arc::new(if kumite_fs::exists(&camera) { CameraData::load(&camera)? } else { CameraData::provisional()? });
        let roster: serde_json::Value = kumite_sim::data::load(&gd.join("roster.json")).unwrap_or_default();
        let character = |m: usize| roster["selections"]["entries"].as_array().into_iter().flatten().find(|e| e["model_record"].as_u64() == Some(m as u64)).and_then(|e| e["character"].as_i64());
        let characters = [character(models[0]), character(models[1])];
        let rounds = gd.join("rounds.json");
        let rounds = if kumite_fs::exists(&rounds) { Some(bout::RoundRules::load(&rounds)?) } else { None };
        let mut game = Self {
            fight: Fight::new(data, movesets[0].clone(), movesets[1].clone()),
            bodies: [Arc::new(b0), Arc::new(b1)],
            camera: FightCamera::new(camera_data, false, 0, true),
            camera_mode: CameraMode::Fight,
            intro: None,
            recorded: VecDeque::new(),
            recorded_total: 0,
            display: None,
            visible: [true; 2],
            handicap: [100; 2],
            max_health: [None; 2],
            start_health: [None; 2],
            round_frames: rounds.as_ref().map_or(kumite_sim::ROUND_FRAMES, |r| r.round_frames),
            ball: None,
            extra_body: None,
            extra_pad: Pad::default(),
            partner_body: None,
            partner_pad: Pad::default(),
            more_body: Vec::new(),
            more_pad: Vec::new(),
            pickups: Vec::new(),
            ball_team: None,
            ball_team_human: [false; 2],
            ball_ring: VecDeque::new(),
            display_ball: None,
            characters,
            heads: Default::default(),
            looks: Default::default(),
            head_live: false,
            head_off: [false; 2],
            faces: Default::default(),
            hands: Default::default(),
            hand_live: false,
            tag_faces: Default::default(),
            wings: Default::default(),
            wolves: Default::default(),
            springs: Default::default(),
            ko_collapse: true,
            guard_damage: false,
            tag_carry: [None; 2],
            face_data: Arc::new(face::FaceData::load(&assets.root.join("gamedata"))),
            face_rand: 1,
            copiers: [None, None],
            copy_seed: 0,
            tag_bench: None,
            rounds,
            posing_partner: None,
            costume_of: roster["selections"]["entries"].as_array().into_iter().flatten().filter_map(|e| Some((e["model_record"].as_u64()? as usize, e["costume"].as_i64()?))).collect(),
        };
        for (f, c) in game.fight.fighters.iter_mut().zip(characters) {
            f.character = c;
        }
        for i in 0..2 {
            if characters[i] == Some(COPIER) {
                game.copiers[i] = Some(Copier::new(assets, game.bodies[i].clone(), &index, game.fight.data.tag_moves));
            }
        }
        game.step_camera();
        Ok(game)
    }

    /// Tekken Tag's tag battle (2 on 2): each fighter gets a partner, benched; every round starts with the fighters
    /// given at load.
    pub fn start_tag(&mut self, partners: [Actor; 2]) {
        let gd = self.fight.data.clone();
        let members = std::array::from_fn(|i| {
            let p = &partners[i];
            let max = self.max_health[i].unwrap_or(gd.max_health_of(p.character) * self.handicap[i] / 100);
            kumite_sim::TagMember::new(p.moveset.clone(), p.character, max, &gd.tag)
        });
        self.fight.start_tag(members);
        // (a team's members carried from the last match: the lead's recoverable cap, the partner's health and cap)
        if let Some(t) = self.fight.teams.as_deref_mut() {
            for (team, carry) in t.iter_mut().zip(self.tag_carry) {
                if let Some([(_, lead_cap), (health, cap)]) = carry {
                    team.members[0].cap = lead_cap;
                    (team.members[1].health, team.members[1].cap) = (health, cap);
                }
            }
        }
        self.tag_bench = Some(partners);
    }

    /// The fighter and the benched partner of side `i` trade bodies (they swapped in the fight).
    fn tag_trade(&mut self, i: usize) {
        if let Some(b) = self.tag_bench.as_mut() {
            std::mem::swap(&mut self.bodies[i], &mut b[i].body);
            std::mem::swap(&mut self.characters[i], &mut b[i].character);
        }
        // shortcut: the benched member's fingers are not kept, they ease in from the model's again; upgrade by
        // benching them with the body
        self.hands[i] = Default::default();
        self.tag_faces[i] = Default::default();
        self.springs[i].clear();
    }

    /// The fight's arena, for a game whose camera has a projection per arena (Tekken Tag; Tekken 3's is one).
    pub fn set_arena(&mut self, arena: usize) {
        if let Some(d) = self.camera.data().for_arena(arena) {
            self.camera = FightCamera::new(Arc::new(d), false, 0, true);
            self.step_camera();
        }
    }

    /// The win pose of a tag battle (0x3B5DB0): side `w`'s benched partner takes the loser's place, x 0x400 and z
    /// -0xC00 from the winner (taken as to its right and behind it), playing the winner's alias. False without a
    /// partner.
    pub fn pose_partner(&mut self, w: usize) -> bool {
        let (Some(t), Some(_), None) = (self.fight.teams.as_deref(), self.tag_bench.as_ref(), self.posing_partner) else { return false };
        let m = t[w].partner().clone();
        self.partner_trade(w);
        self.posing_partner = Some(w);
        let winner = &self.fight.fighters[w];
        let (sin, cos) = winner.yaw.dsin_cos();
        let at = winner.root() + Vec3::new(cos, 0.0, -sin) * 1024.0 - Vec3::new(sin, 0.0, cos) * 3072.0;
        let (alias, yaw, side) = (winner.alias, winner.yaw, self.fight.fighters[1 - w].side);
        let mut f = kumite_sim::Fighter::new(m.moveset, m.max_health, Vec3::new(at.x, 0.0, at.z), yaw, side);
        f.character = m.character;
        f.play_alias(alias);
        self.fight.fighters[1 - w] = f;
        true
    }

    /// Tekken Tag's team intro before a match's first round (0x3B5328): team `t` on the stage, its fighter in its
    /// slot and its benched partner in the other's, each in its round-1 intro alias of `pick` (0 / 1: alias 4 / 6),
    /// on the team shot. A pair of the table (0x3B4F88) stands at its own places in its own motions. False without a
    /// partner or the data.
    pub fn team_intro(&mut self, t: usize, pick: [usize; 2]) -> bool {
        // (the round's health, a carried one too, is set again after: the round restarts)
        // (not a tag battle: no intro, and the round's fly-in goes on)
        let data = self.camera.data().clone();
        if data.team_intro.is_none() || self.fight.teams.is_none() || self.tag_bench.is_none() {
            return false;
        }
        let health = self.fight.fighters.each_ref().map(|f| Some(f.health));
        self.restart(None);
        self.start_health = health;
        let (Some(ti), Some(teams), Some(_)) = (data.team_intro.as_ref(), self.fight.teams.as_deref(), self.tag_bench.as_ref()) else { return false };
        let m = teams[t].partner().clone();
        self.partner_trade(t);
        self.posing_partner = Some(t);
        let o = 1 - t;
        let f = &self.fight.fighters[o];
        let mut p = kumite_sim::Fighter::new(m.moveset, m.max_health, f.position, f.yaw, f.side);
        p.character = m.character;
        self.fight.fighters[o] = p;
        // the pair's row: its first member in player 0's slot or in player 1's (0x3B4F40 compares the costume unless -1)
        let costume = |i: usize| self.costume_of.iter().find(|c| c.0 == self.bodies[i].model_record).map_or(-1, |c| c.1);
        let is = |m: &kumite_sim::camera::PairMember, i: usize| Some(m.character) == self.characters[i] && (m.costume == -1 || m.costume == costume(i));
        let pair = ti.pairs.iter().find_map(|[a, b]| match () {
            _ if is(a, 0) && is(b, 1) => Some([*a, *b]),
            _ if is(a, 1) && is(b, 0) => Some([*b, *a]),
            _ => None,
        });
        let chars = self.characters;
        let gd = self.fight.data.clone();
        for (i, f) in self.fight.fighters.iter_mut().enumerate() {
            let alias = match pair {
                Some(r) => {
                    // (facing 65536 a turn, 0 toward the fight camera)
                    f.position = Vec3::new(r[i].x as f32, 0.0, -r[i].z as f32);
                    f.yaw = -(r[i].facing as f32) * std::f32::consts::TAU / 65536.0;
                    r[i].alias
                }
                None => {
                    // (0x3B5150: both facing 0, toward the fight camera)
                    f.position = Vec3::new([-1.0, 1.0][i] * ti.place.x as f32, 0.0, 0.0);
                    f.yaw = 0.0;
                    f.moveset.fixed.round_start[pick[i].min(1)]
                }
            };
            if f.play_alias(alias) {
                // 0x368398 under the team intro's flag: the character's frame for the alias (0x3D29E8; 1 for none),
                // or the motion's frames (record +0x18, a byte) - 90 when shorter than it + 90: its last 90 frames
                let len = i64::from(f.current_move(&gd).frames.min(255));
                let start = chars[i].and_then(|c| ti.start_frames.get(usize::try_from(c).ok()?)).and_then(|l| l.iter().find(|e| e[0] == alias as i64)).map_or(1, |e| e[1]);
                f.frame = (if len < start + 90 { len - 90 } else { start }).max(1) as u32;
            }
        }
        if pair.is_none() {
            // player 0 moved (game z, into the screen)
            self.fight.fighters[0].position.z -= if t == 0 { ti.place.z } else { -ti.place.z } as f32;
        }
        self.visible = [true; 2];
        // the chest points as 0x3B4DC0 sets them (the place, 1425 up); the shot (0x3D2330) on the team's own fighter
        // (checked against a PS2 recording of Jin and Jun's: the shot, the partner's place and the stage behind as
        // here), a pair's midpoint. shortcut: a pair's points set the same way (0x3B4E10 not read)
        let chest = |i: usize| {
            let p = self.fight.fighters[i].position;
            [p.x.round() as i64, -1425, -(p.z.round() as i64)]
        };
        let base = match pair {
            Some(_) => {
                let [a, b] = [chest(0), chest(1)];
                std::array::from_fn(|k| kumite_sim::camera::tdiv(a[k] + b[k], 2))
            }
            None => chest(t),
        };
        let Some(shot) = Shot::team(&data, base, t, pair.is_some()) else { return false };
        self.camera_mode = CameraMode::Shot { shot, follow: None };
        self.step_camera();
        true
    }

    /// The loser's slot and side `w`'s benched partner trade bodies (the partner's win pose; again: back).
    fn partner_trade(&mut self, w: usize) {
        if let Some(b) = self.tag_bench.as_mut() {
            std::mem::swap(&mut self.bodies[1 - w], &mut b[w].body);
            std::mem::swap(&mut self.characters[1 - w], &mut b[w].character);
        }
    }

    /// Start with the round-start camera fly-in (variant = round counter % 10). While it plays the
    /// fighters stand in their stance, pads are ignored and the round clock does not run.
    pub fn with_intro(mut self, variant: usize, first_round: bool) -> Self {
        let (rng, lcg) = (self.camera.rng.clone(), self.camera.lcg);
        self.camera = FightCamera::new(self.camera.data().clone(), true, variant, first_round);
        (self.camera.rng, self.camera.lcg) = (rng, lcg);
        self.intro = Some((variant, first_round));
        self.step_camera();
        self
    }

    /// Start the round again: both fighters back at their start positions with full health, fresh clock and
    /// camera (and the fly-in again if the game has one). Characters and stage stay loaded.
    pub fn reset_round(&mut self) {
        self.restart(self.intro);
    }

    /// Tekken Tag's COMBO TRAINING (0x37D8C8): fighter `k` put `d / 2` left of the middle facing right, the other as
    /// far right facing left, both in their stance; the fight camera afresh (0x38C0F0, 0x390E08).
    pub fn put_apart(&mut self, k: usize, d: f32) {
        for (i, x) in [(k, -d / 2.0), (1 - k, d / 2.0)] {
            let Some(f) = self.fight.fighters.get_mut(i) else { continue };
            f.position = Vec3::new(x, 0.0, 0.0);
            f.yaw = if x < 0.0 { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
            f.root_yaw = f.yaw;
            f.play_alias(f.moveset.fixed.neutral);
        }
        let (rng, lcg) = (self.camera.rng.clone(), self.camera.lcg);
        self.camera = FightCamera::new(self.camera.data().clone(), false, 0, true);
        (self.camera.rng, self.camera.lcg) = (rng, lcg);
    }

    /// Put each fighter into its round-start move (alias; 0x8002D31C): from frame 1, or with exactly 150
    /// frames left for longer clips. An empty alias leaves the fighter in its stance.
    pub fn set_start_moves(&mut self, aliases: [usize; 2]) {
        let gd = self.fight.data.clone();
        for (f, alias) in self.fight.fighters.iter_mut().zip(aliases) {
            if f.play_alias(alias) {
                let len = f.current_move(&gd).frames;
                f.frame = if len >= 151 { len - 149 } else { 1 };
            }
        }
    }

    /// The copying character `i` becomes its pool's fighter `k` (for the round about to start).
    pub fn copy_as(&mut self, i: usize, k: usize) {
        let Some(form) = self.copiers[i].as_ref().and_then(|c| c.form(k)) else { return };
        log::debug!("the copying character {i}: moveset {:?}", form.moveset.moveset_id);
        self.fight.fighters[i].moveset = form.moveset;
        self.bodies[i] = form.body;
    }

    /// Start a new round with the given fly-in (variant, first round), or none.
    pub fn start_round(&mut self, intro: Option<(usize, bool)>) {
        self.intro = intro;
        self.restart(intro);
    }

    fn restart(&mut self, intro_setting: Option<(usize, bool)>) {
        // Ball: the loser of the round serves the next (0x8003CE14)
        if let (Some(ball), kumite_sim::RoundState::Ko { winner }) = (self.ball.as_mut(), self.fight.state) {
            ball.server = 1 - winner;
        }
        // the partner's win pose over: the loser's member back in its place
        if let (Some(w), Some(t)) = (self.posing_partner.take(), self.fight.teams.as_deref()) {
            let l = 1 - w;
            self.fight.fighters[l].moveset = t[l].members[t[l].active].moveset.clone();
            self.partner_trade(w);
        }
        // a tag battle: the round starts with the first members again
        if let Some(t) = self.fight.teams.take() {
            for i in (0..2).filter(|&i| t[i].active != 0) {
                self.tag_trade(i);
                self.fight.fighters[i].moveset = t[i].members[0].moveset.clone();
            }
        }
        let [a, b] = &self.fight.fighters;
        let (ma, mb) = (a.moveset.clone(), b.moveset.clone());
        let guard_input = [a.guard_input, b.guard_input];
        let walls = self.fight.walls.take();
        self.fight = Fight::new(self.fight.data.clone(), ma, mb);
        (self.fight.ko_collapse, self.fight.guard_damage) = (self.ko_collapse, self.guard_damage);
        self.fight.walls = walls;
        for (i, (f, g)) in self.fight.fighters.iter_mut().zip(guard_input).enumerate() {
            f.guard_input = g; // who is CPU-controlled stays the same
            f.max_health = self.max_health[i].unwrap_or(self.fight.data.max_health_of(self.characters[i]) * self.handicap[i] / 100);
            f.character = self.characters[i];
            f.health = match self.start_health[i].take() {
                Some(h) if h > 0 && h <= f.max_health * kumite_sim::ONE => h,
                _ => f.max_health * kumite_sim::ONE,
            };
        }
        self.fight.clock = self.round_frames;
        if let Some(b) = self.tag_bench.clone() {
            self.start_tag(b);
        }
        if let Some(setup) = self.ball.as_mut() {
            // the fighters start on the back lines (0x800B07C8), the ball waits over the server's side
            for (i, f) in self.fight.fighters.iter_mut().enumerate() {
                f.position.x = if i == 0 { -kumite_sim::ball::FIGHTER_X } else { kumite_sim::ball::FIGHTER_X };
            }
            self.fight.ball = Some(kumite_sim::ball::Ball::new(&setup.data, setup.kind, setup.server, setup.rounds == 0));
            setup.rounds += 1;
        }
        if let (Some([partner, extra]), true) = (self.ball_team.clone(), self.ball.is_some()) {
            self.ball_doubles(&partner, &extra);
        }
        let (intro, (variant, first)) = (intro_setting.is_some(), intro_setting.unwrap_or((0, true)));
        let (rng, lcg) = (self.camera.rng.clone(), self.camera.lcg);
        self.camera = FightCamera::new(self.camera.data().clone(), intro, variant, first);
        (self.camera.rng, self.camera.lcg) = (rng, lcg);
        self.camera_mode = CameraMode::Fight;
        self.recorded.clear();
        self.recorded_total = 0;
        self.ball_ring.clear();
        self.display_ball = None;
        self.display = None;
        self.visible = [true; 2];
        // the head state's round reset (0x80039034) writes player 1's entry twice: player 2's carries on
        self.heads[0].reset();
        for h in &mut self.heads {
            h.local = None;
        }
        self.looks = Default::default();
        self.wolves = Default::default();
        self.step_camera();
    }

    /// What the camera samples from both fighters (0x8006465C), in game space (Y down, Z = −glTF Z):
    /// the root on the floor, chest height − 290, and the side each is on (its pelvis on the screen, +0x1278).
    pub fn camera_samples(&self) -> [Sample; 2] {
        std::array::from_fn(|i| {
            let (pose, place) = self.pose(i);
            let chest = place.transform_point3(pose[self.bodies[i].chest].w_axis.truncate()) * UNITS;
            let root = self.fight.fighters[i].root();
            Sample { x: root.x.round() as i64, y: -(chest.y.round() as i64) - 290, z: -(root.z.round() as i64), side: self.fight.fighters[i].side as i64 }
        })
    }

    /// What Tekken Tag's director reads of each fighter (0x38C1E8): its throw role, move and its camera id, facing,
    /// place, and the frames to its move's throw-end marks (0x393CB8).
    fn camera_actors(&self) -> [kumite_sim::camera::Actor; 2] {
        std::array::from_fn(|i| {
            let f = &self.fight.fighters[i];
            let mv = f.current_move(&self.fight.data);
            let s = self.subject(i);
            let mark = |op| mv.properties.iter().flatten().rfind(|p| p.op == op).map_or(-1, |p| i64::from(p.frame) - i64::from(f.frame));
            let p = f.position;
            kumite_sim::camera::Actor {
                throw: if !f.locked {
                    0
                } else if f.thrown {
                    -1
                } else {
                    1
                },
                mv: (f.current.bank as u64) << 32 | f.current.index as u64,
                cam: i64::from(mv.flags24.camera_script),
                facing: s.front,
                at: [p.x.round() as i64, -(p.y.round() as i64), -(p.z.round() as i64)],
                root: s.root,
                marks: [mark(0x0C), mark(0x0D)],
                release: mv.posture.raw & 0x40_0000 != 0,
                follow: i64::from(mv.flags24.raw >> 20 & 3),
                ko: f.health <= 0,
                character: self.characters[i].unwrap_or(-1),
                alias: f.alias as i64,
                shot: self.fight.hits.iter().filter(|h| h.victim == i && !h.blocked).map(|h| if h.counter { 2 } else { usize::from(h.clean) }).max().unwrap_or(0),
            }
        })
    }

    /// The replay camera's samples (0x80069304): root x/z, and y = the higher of the chest and the root bone.
    pub fn replay_samples(&self) -> [Sample; 2] {
        std::array::from_fn(|i| {
            let s = self.subject(i);
            Sample { x: s.root[0], y: s.chest.min(s.root[1]), z: s.root[2], side: [-100, 100][i] }
        })
    }

    /// What an end camera needs from fighter `i` (game space, the shown pose): root bone, chest height,
    /// facing angle, large character (the data's large move set).
    pub fn subject(&self, i: usize) -> Subject {
        let (pose, place) = self.pose(i);
        let world = |node: usize| place.transform_point3(pose[node].w_axis.truncate()) * UNITS;
        let root = world(self.bodies[i].root);
        let chest = world(self.bodies[i].chest);
        let yaw = self.display.map_or(self.fight.fighters[i].yaw, |d| d[i].yaw);
        // the front in game XZ (Z flipped): (sin yaw, -cos yaw)
        let front = self.camera.data().ratan2((yaw.dsin() * 4096.0) as i64, (-yaw.dcos() * 4096.0) as i64) & 0xFFF;
        Subject {
            root: [root.x.round() as i64, -(root.y.round() as i64), -(root.z.round() as i64)],
            chest: -(chest.y.round() as i64),
            front,
            large: self.fight.fighters[i].moveset.moveset_id == Some(self.camera.data().large_moveset()),
        }
    }

    /// The root translation (game units) of fighter `i`'s animation frame, in the clip of its owner.
    pub fn anim_root(&self, i: usize) -> Option<Vec3> {
        let (anim, frame) = self.fight.fighters[i].animation(&self.fight.data);
        self.clip_owner(i).root_of(anim, frame)
    }

    /// What fighter `i` shows now (for the replay record).
    pub fn snapshot(&self, i: usize) -> Snapshot {
        let f = &self.fight.fighters[i];
        Snapshot { clip: self.clip(i), partner: !std::ptr::eq(self.clip_owner(i), &*self.bodies[i]), root: f.root(), yaw: f.yaw, ease: f.eased() }
    }

    /// The camera's frame, then each fighter's side for the next one: where its pelvis falls on the screen
    /// (0x80037924 projects bone 11 to +0x1278 as it draws; 0x80046234 compares the two for the pad's forward and
    /// the camera's framing).
    fn step_camera(&mut self) -> View {
        let v = self.step_view();
        self.camera.turn_world();
        self.project_sides(&v);
        v
    }

    /// The fighters' sides under view `v`: their pelvis's horizontal place on the screen (scaled to whole units).
    fn project_sides(&mut self, v: &View) {
        let (eye, forward, _) = view_camera(v);
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let axis = right.x.datan2(right.z);
        for f in self.fight.fighters.iter_mut() {
            f.screen_right = axis;
        }
        for i in 0..2 {
            let (pose, place) = self.pose(i);
            let Some(m) = pose.get(self.bodies[i].pelvis) else { continue };
            let d = place.transform_point3(m.w_axis.truncate()) - eye;
            let depth = d.dot(forward);
            if depth > 1e-3 {
                self.fight.fighters[i].side = (d.dot(right) / depth * 10000.0).round() as i32;
            }
            // off the screen (Tekken Tag's condition 0x5C, 0x369B98): the head's and both feet's x on its 512 pixels
            // (0x34FD90: 256 + x * H / z, z at least 1000) all at 0x215 or more, or all below -0x14
            let sx: Vec<f32> = [2, 14, 17]
                .iter()
                .filter_map(|&k| self.bodies[i].bones.get(k).and_then(|&n| pose.get(n)))
                .map(|m| {
                    let d = (place.transform_point3(m.w_axis.truncate()) - eye) * UNITS;
                    256.0 + d.dot(right) * (v.h as f32 * 512.0 / 368.0) / d.dot(forward).max(1000.0)
                })
                .collect();
            if sx.len() == 3 {
                self.fight.fighters[i].off_screen = sx.iter().all(|&x| x >= 533.0) || sx.iter().all(|&x| x < -20.0);
            }
        }
    }

    fn step_view(&mut self) -> View {
        match &self.camera_mode {
            CameraMode::Fight => {}
            CameraMode::Replay => {
                let s = self.replay_samples();
                return self.camera.replay_step(s);
            }
            CameraMode::End { who, .. } => {
                let p = self.subject(*who);
                let data = self.camera.data().clone();
                if let CameraMode::End { cam, .. } = &mut self.camera_mode {
                    self.camera.view = cam.step(&data, &p);
                }
                return self.camera.view;
            }
            CameraMode::Frozen => return self.camera.view,
            CameraMode::Shot { follow, .. } => {
                let p = follow.map(|i| self.subject(i).root);
                let data = self.camera.data().clone();
                if let CameraMode::Shot { shot, .. } = &mut self.camera_mode {
                    self.camera.view = shot.step(&data, p);
                }
                return self.camera.view;
            }
        }
        let gd = self.fight.data.clone();
        self.camera.ball = self.fight.ball.as_ref().map(|b| b.pos.map(|v| v as i64));
        let mc = std::array::from_fn(|i| self.fight.fighters[i].current_move(&gd).flags24.camera_script as i64);
        if self.camera.data().director.is_some() {
            self.camera.actors = self.camera_actors();
        }
        let v = self.camera.step(self.camera_samples(), self.fight.frame as i64, mc);
        // screen right in game space is (cos yaw, 0, sin yaw); Z flips into the simulation's axes
        let w = v.yaw as f32 * std::f32::consts::TAU / 4096.0;
        self.fight.screen_right = Vec3::new(w.dcos(), 0.0, -w.dsin());
        v
    }

    /// One 60 Hz step with both players' pads.
    pub fn step(&mut self, pads: [Pad; 2]) {
        let intro = self.camera.in_intro();
        let (pads, clock) = if intro { ([Pad::default(); 2], self.fight.clock) } else { (pads, 0) };
        if self.fight.coop.is_some() && self.fight.partner.is_some() {
            self.step_coop(pads);
        } else {
            let spheres = self.spheres();
            let [b0, b1] = &self.bodies;
            let (a0, a1) = (FighterAnims { own: b0, other: b1, springs: &self.springs[0] }, FighterAnims { own: b1, other: b0, springs: &self.springs[1] });
            // Force's other soldier
            let extra_spheres = self.actor_spheres(2);
            let extra_anims = self.extra_body.as_deref().map(|b| FighterAnims { own: b, other: b0, springs: &[] });
            let extra = match (&extra_spheres, &extra_anims) {
                (Some(sp), Some(an)) => Some((self.extra_pad, sp, an as &dyn Animations)),
                _ => None,
            };
            let active = self.fight.teams.as_ref().map(|t| [t[0].active, t[1].active]);
            self.fight.step_with(pads, &spheres, [&a0 as &dyn Animations, &a1 as &dyn Animations], extra);
            // a tag swap: the bodies follow; the replay ring holds the other body's clips, so it starts again
            let now = self.fight.teams.as_ref().map(|t| [t[0].active, t[1].active]);
            for i in (0..2).filter(|&i| active.zip(now).is_some_and(|(a, b)| a[i] != b[i])) {
                self.tag_trade(i);
                self.recorded.clear();
            }
        }
        if intro {
            self.fight.clock = clock;
        }
        self.step_faces();
        self.step_heads();
        self.step_hands();
        self.step_springs();
        let shot = [self.snapshot(0), self.snapshot(1)];
        if self.recorded.len() >= REPLAY_RING {
            self.recorded.pop_front();
        }
        self.recorded.push_back(shot);
        if let Some(b) = &self.fight.ball {
            if self.ball_ring.len() >= 30 {
                self.ball_ring.pop_front();
            }
            self.ball_ring.push_back(b.clone());
        }
        self.recorded_total += 1;
        self.step_camera();
    }

    /// Force's co-op step: the four actors, each borrowing from the one it fights.
    fn step_coop(&mut self, pads: [Pad; 2]) {
        let n = self.fight.actor_count();
        let spheres: Vec<BodyVolumes> = (0..n).map(|k| self.actor_spheres(k).unwrap_or_default()).collect();
        let bodies: Vec<Arc<Body>> = (0..n).map(|k| self.body_of(k).cloned().unwrap_or_else(|| self.bodies[0].clone())).collect();
        let anims: Vec<FighterAnims> = (0..n).map(|k| FighterAnims { own: &bodies[k], other: &bodies[self.fight.opponent_index(k).min(n - 1)], springs: &[] }).collect();
        let a: Vec<&dyn Animations> = anims.iter().map(|a| a as &dyn Animations).collect();
        let mut pads = vec![pads[0], pads[1], self.extra_pad, self.partner_pad];
        pads.extend((4..n).map(|k| self.more_pad.get(k - 4).copied().unwrap_or_default()));
        let refs: Vec<&BodyVolumes> = spheres.iter().collect();
        self.fight.step_coop(&pads, &refs, &a);
    }

    /// The players in the fight (Force): actor 0, the co-op partner (3), 3P and 4P (4, 5), those who joined.
    pub fn players(&self) -> Vec<usize> {
        let mut v = vec![0];
        if self.partner_body.is_some() && self.fight.partner.is_some() {
            v.push(3);
        }
        v.extend((4..6).filter(|&k| self.more_body.get(k - 4).is_some_and(Option::is_some)));
        v
    }

    /// Force for more than two: the melee and the bodies' places widened to `n` actors.
    pub fn widen_coop(&mut self, n: usize) {
        self.fight.widen_coop(n);
        let m = self.fight.more.len();
        self.more_body.resize(m, None);
        self.more_pad.resize(m, Pad::default());
    }

    /// Force: player `p` (2 = 3P, 3 = 4P) joins as `actor` beside the player (or, down, gets up there again
    /// at full health): actor 4 or 5 of the widened melee.
    pub fn join_player(&mut self, p: usize, actor: &Actor) {
        let k = 2 + p.clamp(2, 3);
        self.widen_coop(12);
        let max = self.fight.data.max_health;
        let lead = &self.fight.fighters[0];
        let pos = lead.position + Vec3::new(-400.0 * (p as f32 - 1.0), 0.0, if p == 2 { -900.0 } else { 1800.0 });
        let mut f = kumite_sim::Fighter::new(actor.moveset.clone(), max, pos, lead.yaw, lead.side);
        f.character = actor.character;
        f.guard_input = true;
        if let Some(m) = self.fight.coop.as_mut() {
            m.links.retain(|l| l.0 != k && l.1 != k);
        }
        self.fight.more[k - 4] = f;
        self.more_body[k - 4] = Some(actor.body.clone());
    }

    /// Ball 2 ON 2: each side's two on its back line, apart in depth; the partners (CPU) join the fight as
    /// actors 3 (the players' side) and 2, and from then the four play as the co-op melee.
    fn ball_doubles(&mut self, partner: &Actor, extra: &Actor) {
        let max = self.fight.data.max_health;
        let depth = kumite_sim::ball::COURT_Z_DOUBLES / 2.0;
        for f in &mut self.fight.fighters {
            f.position.z = -depth;
        }
        let beside = |leader: &kumite_sim::Fighter, a: &Actor, human: bool| {
            let mut f = kumite_sim::Fighter::new(a.moveset.clone(), max, Vec3::new(leader.position.x, leader.position.y, depth), leader.yaw, leader.side);
            f.character = a.character;
            f.guard_input = human;
            f
        };
        let human = self.ball_team_human;
        let (p, e) = (beside(&self.fight.fighters[0], partner, human[0]), beside(&self.fight.fighters[1], extra, human[1]));
        self.extra_body = Some(extra.body.clone());
        self.partner_body = Some(partner.body.clone());
        self.fight.extra = Some(Box::new(e));
        self.fight.join_partner(p);
    }

    /// Actor `k`'s body ([`Fight::actor`] indices).
    pub fn body_of(&self, k: usize) -> Option<&Arc<Body>> {
        match k {
            0 | 1 => Some(&self.bodies[k]),
            2 => self.extra_body.as_ref(),
            3 => self.partner_body.as_ref(),
            _ => self.more_body.get(k - 4).and_then(Option::as_ref),
        }
    }

    /// Force's co-op: the second player joins as `actor` beside the player (or, down, gets up there
    /// again at full health).
    pub fn join_partner(&mut self, actor: &Actor) {
        // (1P's full health, Force co-op's balance: 2P's the same)
        let max = self.fight.data.max_health * self.handicap[0] / 100;
        let p = &self.fight.fighters[0];
        let pos = p.position + Vec3::new(-400.0, 0.0, 900.0);
        let mut f = kumite_sim::Fighter::new(actor.moveset.clone(), max, pos, p.yaw, p.side);
        f.character = actor.character;
        f.guard_input = true;
        self.partner_body = Some(actor.body.clone());
        if self.fight.coop.is_some() && self.fight.partner.is_some() {
            if let Some(m) = self.fight.coop.as_mut() {
                m.links.retain(|l| l.0 != 3 && l.1 != 3);
            }
            self.fight.partner = Some(Box::new(f));
        } else {
            self.fight.join_partner(f);
        }
    }

    /// Show a recorded frame (or, between two, the roots averaged as the replay does on odd half-frames) and
    /// step the camera on it. None = live again.
    pub fn show(&mut self, frame: Option<(usize, bool)>) {
        // the ball's ring holds the record's last 30 frames
        self.display_ball = frame.and_then(|(k, _)| (k + self.ball_ring.len()).checked_sub(self.recorded.len()).and_then(|i| self.ball_ring.get(i).cloned()));
        self.display = frame.and_then(|(k, half)| {
            let a = *self.recorded.get(k)?;
            if !half {
                return Some(a);
            }
            let b = self.recorded.get(k + 1).copied().unwrap_or(a);
            Some(std::array::from_fn(|i| Snapshot { root: (a[i].root + b[i].root) / 2.0, ..a[i] }))
        });
        if self.display.is_some() {
            self.step_camera();
        }
    }

    /// Advance only the camera (the sim is paused: the replay, a frozen end view).
    pub fn step_camera_only(&mut self) {
        self.step_camera();
    }

    /// The character whose clips fighter `i` is playing: its own, or the opponent's for a borrowed move.
    fn clip_owner(&self, i: usize) -> &Body {
        let other = self.body_of(self.fight.opponent_index(i)).unwrap_or(&self.bodies[1 - i]);
        let f = &self.fight.fighters[i];
        if f.current.bank == kumite_sim::data::Bank::Partner {
            return other;
        }
        // Tekken Tag: a victim move resolved through the opponent's aliases (a crossover fighter's, Force's soldiers':
        // Tekken 3's merged table) has its motion in the opponent's list, not its own
        let anim = f.animation(&self.fight.data).0;
        let own = &self.bodies[i];
        if self.fight.data.tag_moves && own.character.clip(anim).is_none() && other.character.clip(anim).is_some() {
            other
        } else {
            own
        }
    }

    /// Animation clip index (in [`Game::clip_owner`]'s character) and frame currently shown for fighter `i`.
    pub fn clip(&self, i: usize) -> Option<(usize, usize)> {
        let (anim, frame) = self.fight.fighters[i].animation(&self.fight.data);
        Some((self.clip_owner(i).character.clip(anim)?, frame as usize))
    }

    /// Force: the third actor's pose and placement (its live state; no replays in Force).
    pub fn pose_extra(&self) -> Option<(Vec<Mat4>, Mat4)> {
        self.pose_actor(2)
    }

    /// Actor `k`'s pose and placement (2, 3: Force's extra and partner, live; 0, 1 as shown).
    pub fn pose_actor(&self, k: usize) -> Option<(Vec<Mat4>, Mat4)> {
        if k < 2 {
            return Some(self.pose(k));
        }
        let (f, body) = (self.fight.actor(k)?, self.body_of(k)?);
        let (anim, frame) = f.animation(&self.fight.data);
        let opponent = self.body_of(self.fight.opponent_index(k)).unwrap_or(body);
        // (a victim move's motion may be the opponent's alone, as in [`Game::clip_owner`])
        let borrowed = self.fight.data.tag_moves && body.character.clip(anim).is_none();
        let partner = f.current.bank == kumite_sim::data::Bank::Partner || borrowed;
        let other = if partner { opponent } else { body };
        let shown = Snapshot { clip: other.character.clip(anim).map(|c| (c, frame as usize)), partner, root: f.root(), yaw: f.yaw, ease: f.eased() };
        Some(shown_pose(body, other, &shown))
    }

    /// Actor `k`'s collision volumes.
    fn actor_spheres(&self, k: usize) -> Option<BodyVolumes> {
        let (pose, place) = self.pose_actor(k)?;
        Some(volumes(self.body_of(k)?, &pose, place))
    }

    /// Put `actor` in slot 1 (the player's target) or 2 (the extra), a fresh fighter where the old one stood.
    pub fn set_actor(&mut self, slot: usize, actor: &Actor) {
        let max = self.fight.data.max_health;
        let (pos, yaw, side) = match slot {
            1 => (self.fight.fighters[1].position, self.fight.fighters[1].yaw, self.fight.fighters[1].side),
            2 => self.fight.extra.as_ref().map_or((Vec3::ZERO, std::f32::consts::FRAC_PI_2, 100), |f| (f.position, f.yaw, f.side)),
            _ => self.fight.actor(slot).map_or((Vec3::ZERO, std::f32::consts::FRAC_PI_2, 100), |f| (f.position, f.yaw, f.side)),
        };
        let mut f = kumite_sim::Fighter::new(actor.moveset.clone(), max, pos, yaw, side);
        f.character = actor.character;
        f.guard_input = false;
        match slot {
            1 => {
                self.fight.fighters[1] = f;
                self.bodies[1] = actor.body.clone();
                self.characters[1] = actor.character;
            }
            2 => {
                self.fight.extra = Some(Box::new(f));
                self.extra_body = Some(actor.body.clone());
            }
            // (the widened melee's soldiers and bosses: actors 6..)
            k => {
                if let (Some(place), Some(body)) = (self.fight.more.get_mut(k - 4), self.more_body.get_mut(k - 4)) {
                    if let Some(m) = self.fight.coop.as_mut() {
                        m.links.retain(|l| l.0 != k && l.1 != k);
                    }
                    *place = f;
                    *body = Some(actor.body.clone());
                }
            }
        }
    }

    /// Swap the player's target with the other soldier (fighter 1 and the extra, with their bodies).
    pub fn swap_target(&mut self) {
        if let (Some(f), Some(b)) = (self.fight.extra.as_mut(), self.extra_body.as_mut()) {
            std::mem::swap(&mut self.fight.fighters[1], f);
            std::mem::swap(&mut self.bodies[1], b);
            self.characters[1] = self.fight.fighters[1].character;
            self.fight.swapped_target();
        }
    }

    /// A hash of the fight's state (the fighters' places, turns, health, moves and frames, the clock, the round, the
    /// ball): two runs of the same inputs that agree frame by frame have the same hashes (online play's determinism
    /// check).
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
            }
        };
        let f = &self.fight;
        eat(f.frame);
        eat(f.clock as u64);
        eat(match f.state {
            kumite_sim::RoundState::Fighting => 0,
            kumite_sim::RoundState::Ko { winner } => 1 + winner as u64,
            kumite_sim::RoundState::TimeUp { winner } => 4 + winner.map_or(0, |w| 1 + w as u64),
        });
        let fighters = f.fighters.iter().chain(f.extra.as_deref()).chain(f.partner.as_deref()).chain(&f.more);
        // a tag battle's teams (the benched partners are only their entries)
        for t in f.teams.iter().flat_map(|t| t.iter()) {
            eat(t.active as u64);
            for m in &t.members {
                eat(u64::from(m.health as u32) | u64::from(m.cap as u32) << 32);
                eat(m.anger as u64);
            }
        }
        for x in fighters {
            for v in [x.position.x, x.position.y, x.position.z, x.yaw, x.root_yaw, x.offset.x, x.offset.y, x.offset.z, x.anim_root_y] {
                eat(v.to_bits() as u64);
            }
            eat(x.health as u64);
            eat(x.current.index as u64 | (x.current.bank as u64) << 32);
            eat(x.frame as u64);
            eat(x.stun as u64 | (x.juggle as u64) << 32);
        }
        if let Some(b) = &f.ball {
            for v in b.pos.iter().chain(&b.vel) {
                eat(*v as u64);
            }
            eat(b.state as u64);
        }
        h
    }

    /// The model id the face tables use ((model record − 71) / 4).
    pub fn model_id(&self, i: usize) -> usize {
        self.bodies[i].model_record.saturating_sub(71) / 4
    }

    /// Blinks (0x80034100, every frame; models without a blink rectangle show nothing) and the moving pupils
    /// (0x800397FC, from the skeleton before the head turns; not for Force's CPU fighters).
    fn step_faces(&mut self) {
        let c = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
        for i in 0..2 {
            let gd = &self.fight.data;
            let m = self.fight.fighters[i].current_move(gd);
            let flags = m.flags24.raw;
            let forced = flags & 0x4_0000 != 0 || (m.posture.raw & 4 != 0 && flags & 0x4000_0000 == 0);
            let seed = &mut self.face_rand;
            let mut rand = || {
                *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
                (*seed >> 16) & 0x7FFF
            };
            self.faces[i].blink(forced, &mut rand);
            // (open while airborne and not juggled: 0x370C94's +0x135 and +0xF7)
            let f = &self.fight.fighters[i];
            self.wings[i].state = i32::from(f.airborne_flag(gd) && f.juggle == 0);
            self.wings[i].step();
            // Unknown's wolf: its root 150 up and 800 back from hers (in her root's parent's axes), eased there 1/12
            // a frame in x, 1/16 in y and z (0x34CC38)
            // shortcut: its noise (the sine table 0x4FCE88 at +0x3120 + rand() & 0x3F, some 20 mm) is not added
            self.wolves[i] = (!self.bodies[i].character.follow.is_empty()).then(|| {
                let (pose, place) = self.pose_animated(i);
                let root = pose.get(self.bodies[i].root).map_or(Vec3::ZERO, |m| m.w_axis.truncate());
                let target = place.transform_point3(root + Vec3::new(0.0, 0.15, -0.8));
                self.wolves[i].map_or(target, |at| at + (target - at) * Vec3::new(1.0 / 12.0, 1.0 / 16.0, 1.0 / 16.0))
            });
            if self.face_data.pupils_character.is_some_and(|c| self.characters[i] == Some(c)) && !self.head_off[i] {
                let head = |k: usize| {
                    let (pose, place) = self.pose_animated(k);
                    place * pose[self.bodies[k].chest]
                };
                let (own, opp) = (head(i), head(1 - i));
                let rot = c * Mat3::from_mat4(own) * c;
                let d = c * (opp.w_axis - own.w_axis).truncate() * UNITS;
                let l = rot.transpose() * d;
                let a = ((l.y.datan2(l.x) * 4096.0 / std::f32::consts::TAU).round() as i32).rem_euclid(4096);
                self.faces[i].pupils(a);
            }
        }
    }

    /// Tekken Tag's fingers (hands.rs): this frame's cues and the move's hand ops.
    fn step_hands(&mut self) {
        for i in 0..2 {
            let Some(p) = &self.bodies[i].hands else { continue };
            let f = &self.fight.fighters[i];
            let m = f.current_move(&self.fight.data);
            let arm = m.attack.as_ref().and_then(|a| a.bones.as_ref()).is_none_or(|b| b.end0 < 0xC);
            let cues = hands::Cues {
                started: f.started_with,
                motion: &m.anim,
                ko: f.health <= 0,
                live: self.hand_live,
                // (+0xCA: the hit's damage with the timed damage ops' added, as the boot routine adds them; none for a throw's grab)
                damage: m.damage != 0 && m.damage_type != 1 || m.properties.iter().flatten().any(|q| q.op == 0x0B && q.arg != 0),
                arm_end: arm && (m.active[0] as i64 - f.frame as i64) < 8,
                thrown_10: f.thrown && f.frame == 10,
            };
            let ops = m.properties.iter().flatten().filter(|q| q.frame == f.frame).map(|q| (q.op, q.arg));
            self.hands[i].step(p, cues, ops);
        }
        // the faces (0x362C28, after the hands' ops; the game's rand() as the blinks')
        for i in 0..2 {
            let Some(p) = &self.bodies[i].face else { continue };
            let f = &self.fight.fighters[i];
            let m = f.current_move(&self.fight.data);
            // (Tekken Tag's flag 0x800 and bit 22 are bits 18 and 30 once loaded, kumite_sim::dialect)
            let cues = tagface::Cues { moving: m.flags24.raw & 0x4_0000 != 0, hit: m.posture.raw & 4 != 0 && m.flags24.raw & 0x4000_0000 == 0 };
            let ops = m.properties.iter().flatten().filter(|q| q.frame == f.frame).map(|q| (q.op, q.arg));
            let seed = &mut self.face_rand;
            let mut rand = || {
                *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
                (*seed >> 16) & 0x7FFF
            };
            self.tag_faces[i].step(p, cues, ops, &mut rand);
        }
    }

    /// Each head's frame (0x8003B17C → 0x80039074): on while the round is live and the move lets the head
    /// follow (record +0x24 bit 30; an attack only from frame max(+0x2E, +0x18 − 17)), aimed at the
    /// opponent's chest and head.
    fn step_heads(&mut self) {
        // glTF (y up, metres) to game space (y down, z flipped, units)
        let c = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
        let world = |g: &Game, i: usize| {
            let (pose, place) = g.pose_animated(i);
            let b = &g.bodies[i];
            let at = |n: usize| place * pose[n];
            (at(b.torso), at(b.chest))
        };
        for i in 0..2 {
            let gd = &self.fight.data;
            let f = &self.fight.fighters[i];
            let m = f.current_move(gd);
            // (Tekken Tag's record bit 22 is this bit once loaded, kumite_sim::dialect)
            let mut on = self.head_live && !self.head_off[i] && m.flags24.raw & 0x4000_0000 != 0;
            if on && m.active[0] != 0 {
                let from = (m.active[1] as i32).max(m.bytes_18.frame_count_ram as i32 - 0x11);
                on = f.frame as i32 >= from;
            }
            if !self.bodies[i].bones.is_empty() {
                let nodes = &self.bodies[i].character.nodes;
                if head::TagLook::fits(|n| nodes.get(n)?.parent) {
                    let (pose, place) = self.pose_animated(i);
                    let w: Vec<Mat4> = pose.iter().map(|m| place * *m).collect();
                    let (o, o_place) = self.pose_animated(1 - i);
                    let target = o_place.transform_point3(o[self.bodies[1 - i].chest].w_axis.truncate());
                    self.looks[i].step(on, &w, target);
                }
                continue;
            }
            let no_yaw = self.characters[i].is_some_and(|c| gd.characters.head_without_yaw.contains(&c));
            let frame = || {
                let (torso, head) = world(self, i);
                let (o_torso, o_head) = world(self, 1 - i);
                let rot = |m: Mat4| c * Mat3::from_mat4(m) * c;
                let pos = |m: Mat4| c * m.w_axis.truncate() * UNITS;
                let chest = rot(torso);
                head::Frame { chest, head: pos(head), head_local: chest.transpose() * rot(head), target: (pos(o_torso) + pos(o_head)) / 2.0, no_yaw }
            };
            let mut h = self.heads[i];
            h.step(on, frame);
            self.heads[i] = h;
        }
    }

    /// Node world matrices (metres) and the placement transform for fighter `i` (as shown: the replay's
    /// frame while one plays), the head turned to the opponent when it follows.
    pub fn pose(&self, i: usize) -> (Vec<Mat4>, Mat4) {
        let (mut pose, place) = self.pose_animated(i);
        if let (Some(at), Some(root)) = (self.wolves[i], pose.get(self.bodies[i].root).copied()) {
            let at = place.inverse().transform_point3(at).extend(1.0);
            self.bodies[i].character.pose_follow(&mut pose, Mat4::from_cols(root.x_axis, root.y_axis, root.z_axis, at));
        }
        if let (None, Some(local)) = (self.display, self.heads[i].local) {
            turn_head(&self.bodies[i], &mut pose, local);
        }
        if let Some(p) = &self.bodies[i].face {
            self.tag_faces[i].apply(p, &self.bodies[i].character, &mut pose);
        }
        if let Some(p) = &self.bodies[i].hands {
            self.hands[i].apply(p, &self.bodies[i].character, &mut pose);
        }
        self.bodies[i].character.pose_wings(&mut pose, self.wings[i].frame);
        if let (None, Some(locals)) = (self.display, self.looks[i].locals) {
            for (n, rot) in head::TAG_LOOK_NODES.into_iter().zip(locals) {
                head::set_local(&self.bodies[i].character.nodes, &mut pose, n, rot);
            }
        }
        (pose, place)
    }

    /// The clip fighter `i` shows (clip index, borrowed from the opponent), for [`SecondaryBlend`].
    pub fn shown_clip(&self, i: usize) -> Option<(usize, bool)> {
        let shown = self.display.map_or_else(|| self.snapshot(i), |d| d[i]);
        shown.clip.map(|(c, _)| (c, shown.partner))
    }

    /// The pose as animated, without the head turned (tests).
    #[doc(hidden)]
    pub fn pose_animated_for_tests(&self, i: usize) -> (Vec<Mat4>, Mat4) {
        self.pose_animated(i)
    }

    /// The pose as animated.
    fn pose_animated(&self, i: usize) -> (Vec<Mat4>, Mat4) {
        // (live, a borrowed clip is its owner's, as `clip` took it: in Force's co-op, any foe's)
        let (shown, other) = self.display.map_or_else(|| (self.snapshot(i), self.clip_owner(i)), |d| (d[i], &self.bodies[1 - i]));
        let (mut pose, place) = shown_pose(&self.bodies[i], other, &shown);
        if self.display.is_none() {
            let b = &self.bodies[i];
            for (sp, (local, _)) in b.springs.iter().zip(&self.springs[i]) {
                if let Some(n) = slot_node(b, sp.slot) {
                    head::set_local(&b.character.nodes, &mut pose, n, turn_mat3(local));
                }
            }
        }
        (pose, place)
    }

    /// Tekken Tag's spring slots, stepped as the fight step's volumes stepped them.
    fn step_springs(&mut self) {
        for i in 0..2 {
            if !self.bodies[i].springs.is_empty() {
                let (shown, other) = (self.snapshot(i), self.clip_owner(i));
                let (mut pose, place) = shown_pose(&self.bodies[i], other, &shown);
                self.springs[i] = step_springs(&self.bodies[i], &mut pose, place, &self.springs[i]);
            }
        }
    }

    /// Collision volumes of both fighters for the pose currently shown (game units).
    pub fn spheres(&self) -> [BodyVolumes; 2] {
        std::array::from_fn(|i| {
            // (Tekken Tag's look only changes what is drawn)
            let (pose, place) = if self.bodies[i].bones.is_empty() { self.pose(i) } else { self.pose_animated(i) };
            volumes(&self.bodies[i], &pose, place)
        })
    }
}

/// A snapshot's pose (node world matrices in metres, as animated) and placement; `other` lends a borrowed clip.
pub fn shown_pose(body: &Body, other: &Body, shown: &Snapshot) -> (Vec<Mat4>, Mat4) {
    let place = placement(shown.root, shown.yaw);
    let mut pose = match shown.clip {
        Some((clip, frame)) if shown.partner => body.character.pose_from(&other.character, clip, frame, true),
        Some((clip, frame)) => body.character.pose(clip, frame, true),
        None => body.character.pose(0, 0, true),
    };
    // (Tekken Tag: the 18 slots at the game's turns of the motion's angles)
    let owner = if shown.partner { &other.character } else { &body.character };
    if let Some(turns) = shown.clip.and_then(|(clip, frame)| angle_turns(owner.clips.get(clip)?, frame)) {
        for (t, &n) in turns.iter().zip(&body.bones) {
            head::set_local(&body.character.nodes, &mut pose, n, turn_mat(t));
        }
    }
    if let Some((turns, lift)) = shown.ease {
        // (the slots past the hips take the eased turns, rows of 4096; the body rises by the eased height)
        for (t, &n) in turns.iter().zip(&body.bones).skip(1) {
            head::set_local(&body.character.nodes, &mut pose, n, turn_mat(t));
        }
        for m in &mut pose {
            *m = Mat4::from_translation(Vec3::Y * lift / UNITS) * *m;
        }
    }
    // (a hidden node's mesh shrinks into its joint)
    for &n in &body.hidden {
        if let Some(m) = pose.get_mut(n) {
            *m *= Mat4::from_scale(Vec3::ZERO);
        }
    }
    (pose, place)
}

/// A slot's turn (rows, 4096 = 1) as a matrix.
fn turn_mat(t: &[i16; 9]) -> Mat3 {
    let c = |k: usize| Vec3::new(t[k].into(), t[k + 3].into(), t[k + 6].into()) / 4096.0;
    Mat3::from_cols(c(0), c(1), c(2))
}

/// Tekken Tag: the 18 joint slots' turns at `frame` of `clip` as the game builds them from the motion's angles (0x34ED90
/// -> 0x3986D8: x and y negated (the arcade's mirror), each angle's top 12 bits, Rz x Ry x Rx on its sine table,
/// 0x39BAE8; PCSX2's +0x634 to the unit); None for a clip without them.
fn angle_turns(clip: &kumite_assets::Clip, frame: usize) -> Option<Turns> {
    let a = clip.angles.get(frame.min(clip.angles.len().checked_sub(1)?))?;
    Some(std::array::from_fn(|s| {
        let at = |k: usize| i32::from(a.get(3 + 3 * s + k).map_or(0, |&v| v as i16));
        let g = kumite_sim::euler([-at(0) >> 4, -at(1) >> 4, at(2) >> 4]);
        std::array::from_fn(|k| g[k / 3][k % 3] as i16)
    }))
}

/// Turn the head (and everything hanging from it: the second head section, hair) to `local`, its rotation in
/// the chest's frame (game axes).
pub fn turn_head(b: &Body, pose: &mut [Mat4], local: Mat3) {
    let c = Mat3::from_diagonal(Vec3::new(1.0, -1.0, -1.0));
    let old = pose[b.chest];
    let rot = Mat3::from_mat4(pose[b.torso]) * (c * local * c);
    let new = Mat4::from_cols(rot.x_axis.extend(0.0), rot.y_axis.extend(0.0), rot.z_axis.extend(0.0), old.w_axis);
    let delta = new * old.inverse();
    let nodes = &b.character.nodes;
    for (n, m) in pose.iter_mut().enumerate() {
        let mut p = Some(n);
        while let Some(k) = p {
            if k == b.chest {
                *m = if n == b.chest { new } else { delta * *m };
                break;
            }
            p = nodes[k].parent;
        }
    }
}

/// A body's collision volumes for a pose (game units).
pub fn volumes(body: &Body, pose: &[Mat4], place: Mat4) -> BodyVolumes {
    // Tekken Tag's slots (0x3AFAA0, 0x34EF20): each on its parent slot at the game's place for it, turned as the parent
    // (the drawn skeleton's bones are other lengths for a few bodies); then the strike slots past them, those the body
    // does not set at the origin (their matrices stay cleared)
    let turn = |s: usize| body.bones.get(s).and_then(|&n| pose.get(n)).copied().unwrap_or(Mat4::IDENTITY);
    let mut slots: Vec<Vec3> = Vec::with_capacity(18);
    for s in 0..body.bones.len() {
        let p = match body.joints.get(s) {
            Some(&(parent, at)) if parent < s => slots.get(parent).map_or(Vec3::ZERO, |&q| q + turn(parent).transform_vector3(at)),
            _ => turn(s).w_axis.truncate(),
        };
        slots.push(p);
    }
    let mut bones: Vec<Vec3> = slots.iter().map(|&p| place.transform_point3(p) * UNITS).collect();
    if !bones.is_empty() {
        bones.resize(24, Vec3::ZERO);
    }
    // (slots 18-20 past 17 are the drawn nodes at their places, which the motions turn: the joint's child nearest the
    // place, then that node's; a slot on one of them follows it)
    let mut more: Vec<(usize, Vec3)> = Vec::new();
    for &(slot, joint, at) in &body.extra {
        let Some(q) = slots.get(joint).copied().or_else(|| more.iter().find(|m| m.0 == joint).map(|m| m.1)) else { continue };
        let p = match slot_node(body, slot).and_then(|n| pose.get(n)) {
            Some(m) => m.w_axis.truncate(),
            None => q + turn(joint).transform_vector3(at),
        };
        if let Some(b) = bones.get_mut(slot) {
            *b = place.transform_point3(p) * UNITS;
        }
        more.push((slot, p));
    }
    // (the 18 joint slots in the game's integers: its strikes and spheres start there)
    let fixed = fixed_slots(body, pose, place);
    for (b, p) in bones.iter_mut().zip(&fixed) {
        *b = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    }
    let make = |layout: &[(usize, Vec3, f32, usize)]| -> Vec<Sphere> {
        layout
            .iter()
            .map(|&(node, offset, radius, zone)| {
                let s = body.bones.iter().position(|&n| n == node);
                let centre = match s.and_then(|s| fixed.get(s)) {
                    Some(p) => Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) + offset * UNITS,
                    None => (place.transform_point3(s.and_then(|s| slots.get(s)).copied().unwrap_or_else(|| pose[node].w_axis.truncate())) + offset) * UNITS,
                };
                Sphere { centre, radius, zone }
            })
            .collect()
    };
    // the head and the pelvis, in units, in Tekken 3's joint axes (where Gon's fire and gas start; Tekken Tag's are
    // those turned half a turn about x, ok_tekken3_converter::ttt's `turn`)
    let axes = if body.bones.is_empty() { Mat4::IDENTITY } else { Mat4::from_diagonal(glam::Vec4::new(1.0, -1.0, -1.0, 1.0)) };
    let slot = |n: usize| {
        let m = place * pose.get(n).copied().unwrap_or(Mat4::IDENTITY) * axes;
        Mat4::from_cols(m.x_axis, m.y_axis, m.z_axis, (m.w_axis.truncate() * UNITS).extend(1.0))
    };
    BodyVolumes { hurt: make(&body.hurt), push: make(&body.push), bones, slots: [slot(body.chest), slot(body.pelvis)] }
}

/// A turn's float entry as the game's 4096 = 1 (0x34ED90: times 4096, truncated; a float a hair off a whole number,
/// as ours from the converted motions are where the game's sines were exact, is that number).
fn fixed(v: f32) -> i32 {
    let x = v * 4096.0;
    if (x - x.round()).abs() < 0.01 {
        x.round() as i32
    } else {
        x as i32
    }
}

/// Tekken Tag's 18 joint slots as the game builds them (0x34EF20), in units in our world axes (its flips change
/// signs only): the facing (+0x508) and each slot's turn as [`fixed`]; a slot's world turn its parent's times its own
/// (0x39A670), its place the parent's plus the parent's turn times its joint place (0x39B310), each product's
/// twelfth bits cut toward zero. Empty for a body without the game's joint places.
fn fixed_slots(body: &Body, pose: &[Mat4], place: Mat4) -> Vec<[i32; 3]> {
    type Fixed = [[i32; 3]; 3];
    let fix = |m: Mat3| -> Fixed { std::array::from_fn(|r| std::array::from_fn(|c| fixed(m.col(c)[r]))) };
    let mul = |a: &Fixed, b: &Fixed| -> Fixed { std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c] / 4096).sum())) };
    let rot = |s: usize| body.bones.get(s).and_then(|&n| pose.get(n)).map_or(Mat3::IDENTITY, |m| Mat3::from_mat4(*m));
    if body.joints.len() < body.bones.len() || body.bones.len() < 18 {
        return Vec::new();
    }
    let root = place.transform_point3(pose.get(body.bones[0]).map_or(Vec3::ZERO, |m| m.w_axis.truncate())) * UNITS;
    let mut out: Vec<(Fixed, [i32; 3])> = vec![(mul(&fix(Mat3::from_mat4(place)), &fix(rot(0))), root.round().to_array().map(|v| v as i32))];
    for (s, &(parent, at)) in body.joints.iter().enumerate().take(18).skip(1) {
        let Some(&(turn, p)) = out.get(parent) else { return Vec::new() };
        let at = (at * UNITS).round().to_array().map(|v| v as i64);
        let to: [i32; 3] = std::array::from_fn(|r| p[r] + (0..3).map(|k| turn[r][k] as i64 * at[k] / 4096).sum::<i64>() as i32);
        // (the parent's inverse, not its transpose: the game's integer turns are a hair off orthonormal)
        out.push((mul(&turn, &fix(rot(parent).inverse() * rot(s))), to));
    }
    out.into_iter().map(|(_, p)| p).collect()
}

/// Render camera for a game view: eye and look direction in glTF metres (y up), vertical field of view from
/// the projection distance H and the 368x480 screen with its 7122/4096 Y-row scale (4:3 picture).
pub fn view_camera(v: &View) -> (Vec3, Vec3, f32) {
    let a = |u: i64| u as f32 * std::f32::consts::TAU / 4096.0;
    // a view straight down (the loser shot) keeps a hair of its yaw direction so the look-at has an up
    // pitch as a signed angle (4075 is a little up, not almost a full turn)
    let pitch = ((v.pitch + 2048) & 0xFFF) - 2048;
    let (p, w) = (a(pitch).clamp(-1.5698, 1.5698), a(v.yaw));
    let eye = Vec3::new(v.eye[0] as f32, -v.eye[1] as f32, -v.eye[2] as f32) / UNITS;
    // game forward (-sin w cos p, sin p, cos w cos p), Y and Z flipped into glTF
    let forward = Vec3::new(-w.dsin() * p.dcos(), -p.dsin(), -w.dcos() * p.dcos());
    (eye, forward, 2.0 * (240.0 / (v.h as f32 * 7122.0 / 4096.0)).atan())
}

/// Tekken Tag's fight z guard (0x341E0C): the depth (m, along `forward` from `eye`) nearer than which the arena's
/// scenery is not drawn. The guard is the nearer fighter's depth (+0x620, its root `roots`, m) less 500 mm, d, on
/// 0x433440's projection (2d / z(d)); the arena's own (0x433480, its z row 3/4 of that one's; read in PCSX2) meets it
/// at 3d / (4 - d / 196608 mm).
pub fn scenery_cut(eye: Vec3, forward: Vec3, roots: [Vec3; 2]) -> f32 {
    let depth = |r: Vec3| (r - eye).dot(forward.normalize_or_zero());
    let d = (depth(roots[0]).min(depth(roots[1])) - 0.5).max(0.0);
    3.0 * d / (4.0 - d / 196.608)
}

/// The CPU AI tables (gamedata/ai.json; a game without them, a Tekken Tag pack before format 7, gets none).
pub fn ai_data(assets: &Assets) -> Result<Arc<kumite_sim::ai::AiData>> {
    let path = assets.root.join("gamedata/ai.json");
    if !kumite_fs::exists(&path) {
        return Ok(Default::default());
    }
    Ok(Arc::new(kumite_sim::ai::AiData::load(&path)?))
}

/// Fighter placement (simulation position in game units, facing yaw) as a world transform in metres.
pub fn placement(position: Vec3, yaw: f32) -> Mat4 {
    Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), position / UNITS)
}

/// Sphere layout from rules.json (`hurt_spheres` or `push_spheres`): each sphere follows a joint, with an
/// optional world-up offset; radii by character id (the first set when the section has one for everybody).
fn sphere_layout(section: &serde_json::Value, c: &Character, id: u64) -> Result<Vec<(usize, Vec3, f32, usize)>> {
    let layout = section["layout"].as_array().context("rules.json: sphere layout")?;
    let sets = section["per_character"].as_array().context("rules.json: sphere radii")?;
    let set = sets.iter().find(|e| e["character"].as_u64() == Some(id)).or(sets.first()).context("rules.json: sphere radii")?;
    let radii = set["radii"].as_array().context("rules.json: sphere radii")?;
    layout
        .iter()
        .map(|s| {
            let joint = s["gltf_joint"].as_str().unwrap_or_default();
            let node = c.nodes.iter().position(|n| n.name == joint).with_context(|| format!("no joint {joint}"))?;
            let o = &s["offset_gltf_m"];
            let f = |k: usize| o[k].as_f64().unwrap_or(0.0) as f32;
            let i = s["index"].as_u64().unwrap_or(0) as usize;
            Ok((node, Vec3::new(f(0), f(1), f(2)), radii.get(i).and_then(serde_json::Value::as_f64).unwrap_or(200.0) as f32, i))
        })
        .collect()
}

/// One pad as a script token: "n", "f", "d/b", "1+2", "d/f+1", "5" Tekken Tag's tag button (the inverse of
/// [`parse_script`]).
pub fn format_pad(p: &Pad) -> String {
    let mut dir: Vec<&str> = Vec::new();
    // same priorities as the input decoder: up over down, right over left
    if p.up {
        dir.push("u");
    } else if p.down {
        dir.push("d");
    }
    if p.right {
        dir.push("f");
    } else if p.left {
        dir.push("b");
    }
    let buttons: Vec<String> = (0..5).filter(|b| p.buttons & (1 << b) != 0).map(|b| (b + 1).to_string()).collect();
    match (dir.is_empty(), buttons.is_empty()) {
        (true, true) => "n".into(),
        (false, true) => dir.join("/"),
        (true, false) => buttons.join("+"),
        (false, false) => format!("{}+{}", dir.join("/"), buttons.join("+")),
    }
}

/// Pads as a run-length script ("f*30,n*5,1,n*40") that [`parse_script`] reads back to the same pads
/// (for the player on the left; "f" = right on the pad).
pub fn format_script(pads: &[Pad]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < pads.len() {
        let n = pads[i..].iter().take_while(|p| **p == pads[i]).count();
        let t = format_pad(&pads[i]);
        out.push(if n > 1 { format!("{t}*{n}") } else { t });
        i += n;
    }
    out.join(",")
}

/// The longest pad script: ten hours of frames (a bigger count is a typo, not memory to ask for).
pub const MAX_SCRIPT_FRAMES: usize = 10 * 60 * 60 * 60;

/// Parse an input script like "f*30,n*5,1,d/f+1,n*40" into one pad per frame (the player on the left).
pub fn parse_script(s: &str) -> Result<Vec<Pad>> {
    let mut out = Vec::new();
    for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let (body, n) = tok.split_once('*').map_or((tok, 1), |(b, n)| (b, n.parse().unwrap_or(1)));
        let starts_with_button = body.chars().next().is_some_and(|c| c.is_ascii_digit());
        let (dir, buttons) = if starts_with_button { ("n", body) } else { body.split_once('+').unwrap_or((body, "")) };
        let mut p = Pad::default();
        for part in dir.split('/') {
            match part {
                "f" => p.right = true,
                "b" => p.left = true,
                "u" => p.up = true,
                "d" => p.down = true,
                "n" | "" => {}
                other => anyhow::bail!("unknown direction {other:?} in {tok:?}"),
            }
        }
        for b in buttons.split('+').filter(|b| !b.is_empty()) {
            let k: u8 = b.parse().with_context(|| format!("bad button in {tok:?}"))?;
            anyhow::ensure!((1..=5).contains(&k), "button {k} out of range in {tok:?}");
            p.buttons |= 1 << (k - 1);
        }
        anyhow::ensure!(n <= MAX_SCRIPT_FRAMES - out.len(), "the script is longer than {MAX_SCRIPT_FRAMES} frames");
        out.extend(std::iter::repeat_n(p, n));
    }
    Ok(out)
}
