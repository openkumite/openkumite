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
pub mod head;
pub mod rumble;
pub mod scene;
pub mod sound;
pub mod tag;
pub mod vfx;
pub mod weather;

use kumite_sim::camera::{CameraData, EndCam, FightCamera, Sample, Shot, Subject, View};
use kumite_sim::{Animations, BodyVolumes, Fight, GameData, Sphere};
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
}

impl Animations for FighterAnims<'_> {
    fn root(&self, anim: &str, frame: u32, partner: bool) -> Option<Vec3> {
        if partner {
            self.other.root_of(anim, frame)
        } else {
            self.own.root_of(anim, frame)
        }
    }

    fn volumes(&self, f: &kumite_sim::Fighter, gd: &kumite_sim::GameData) -> Option<BodyVolumes> {
        if self.own.bones.is_empty() {
            return None;
        }
        let partner = f.current.bank == kumite_sim::data::Bank::Partner;
        let other = if partner { self.other } else { self.own };
        let (anim, frame) = f.animation(gd);
        let shown = Snapshot { clip: other.character.clip(anim).map(|c| (c, frame as usize)), partner, root: f.root(), yaw: f.yaw };
        let (pose, place) = shown_pose(self.own, other, &shown);
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
}

/// Frames a secondary joint takes to go over from the last clip's pose to the new one.
pub const SECONDARY_BLEND: u32 = 8;

/// The joints that hang or flap (the extra fighters' chains `chain*` and wing vertices `wing*`) carry their motion
/// baked into each clip, where the game keeps swinging them across moves. When the clip changes they go over from the
/// pose last shown to the new clip's over [`SECONDARY_BLEND`] frames (drawing only).
#[derive(Clone, Debug, Default)]
pub struct SecondaryBlend {
    clip: Option<(usize, bool)>,
    frame: u64,
    left: u32,
    from: Vec<(usize, Mat4)>,
    last: Vec<(usize, Mat4)>,
}

impl SecondaryBlend {
    /// `pose`: the character's node matrices for the clip shown at game frame `frame` (changed in place).
    pub fn apply(&mut self, character: &kumite_assets::Character, clip: Option<(usize, bool)>, frame: u64, pose: &mut [Mat4]) {
        let joints: Vec<usize> = character.nodes.iter().enumerate().filter(|(_, n)| n.name.starts_with("chain") || n.name.starts_with("wing")).map(|(i, _)| i).collect();
        if joints.is_empty() {
            return;
        }
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
            for &(j, m) in &self.from {
                if let Some(p) = pose.get_mut(j) {
                    let (_, ra, ta) = m.to_scale_rotation_translation();
                    let (_, rb, tb) = p.to_scale_rotation_translation();
                    *p = Mat4::from_rotation_translation(ra.slerp(rb, t), ta.lerp(tb, t));
                }
            }
        }
        self.last = joints.iter().filter_map(|&j| pose.get(j).map(|m| (j, *m))).collect();
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
    /// Blinks and the moving pupils (faces.json), and the rand() stream the blinks draw from.
    pub faces: [face::Face; 2],
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
    let moveset = if tag {
        kumite_sim::dialect::moveset(common, &path)?
    } else {
        let moveset = kumite_sim::data::load::<Moveset>(&path)?;
        moveset.check(common).with_context(|| format!("{}", path.display()))?;
        moveset
    };
    let moveset = Arc::new(moveset);
    let mut character = assets.character(record)?;
    if tag {
        let list: serde_json::Value = kumite_sim::data::load(&gd.join(format!("movesets/motions_{:02}.json", entry.moveset_id)))?;
        for f in list["motions"].as_array().into_iter().flatten().filter_map(|f| f.as_str()) {
            let file = assets.root.join("motions").join(f);
            character.add_motions(&kumite_fs::read(&file).with_context(|| format!("reading {}", file.display()))?).with_context(|| format!("{}", file.display()))?;
        }
    }
    let root = character.nodes.iter().position(|n| n.parent.is_none()).context("no root node")?;
    // the radii are per character id (0x8003F048: 0x80096FF0[+0x18]); the roster gives the model's character
    let roster: serde_json::Value = kumite_sim::data::load(&gd.join("roster.json")).unwrap_or_default();
    let id = roster["selections"]["entries"].as_array().into_iter().flatten().find(|e| e["model_record"].as_u64() == Some(record as u64)).and_then(|e| e["character"].as_u64()).unwrap_or(0);
    let hurt = sphere_layout(&rules["hurt_spheres"], &character, id)?;
    let push = sphere_layout(&rules["push_spheres"], &character, id)?;
    let chest = character.nodes.iter().position(|n| n.name == "head").context("no head joint")?;
    let torso = character.nodes.iter().position(|n| n.name == "chest").context("no chest joint")?;
    let pelvis = character.nodes.iter().position(|n| n.name == "pelvis").unwrap_or(root);
    let bones = if tag { BONE_SLOTS.iter().map(|&j| character.nodes.iter().position(|n| n.name == j).with_context(|| format!("no joint {j}"))).collect::<Result<_>>()? } else { Vec::new() };
    let body = Body { character, model_record: record, root, hurt, push, chest, torso, pelvis, hidden: Vec::new(), bones };
    // the copying character: its own moveset is never played; outside a bout's rounds it stays the setup's fighter
    // shortcut: Force's bosses and the tag brawl's draw their fighter too (0x800B2770), here they keep this one,
    // and Force's player draws without a seed of the match (the same fighters every run)
    if id == COPIER as u64 {
        if let Some(e) = index.models.iter().find(|m| m.moveset_id == COPY_DEFAULT) {
            let donor = kumite_sim::data::load::<Moveset>(&gd.join("movesets").join(&e.moveset_file))?;
            donor.check(common).with_context(|| e.moveset_file.clone())?;
            return Ok((copy_body(&body, &assets.character(e.model_record)?, COPY_DEFAULT), Arc::new(donor)));
        }
    }
    Ok((body, moveset))
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
/// The moveset the fight's setup gives it before the first round's draw (0x8002A40C: its descriptor's).
const COPY_DEFAULT: i64 = 9;
/// The copying character's id (actor +0x18).
pub const COPIER: i64 = 15;
/// The moveset whose fighter holds a sword: the copying character's stick shows with it.
const SWORD_MOVESET: i64 = 4;

impl Copier {
    fn new(assets: &Assets, own: Arc<Body>, index: &MovesetIndex) -> Self {
        let mut pool: Vec<(i64, usize)> = Vec::new();
        for m in &index.models {
            if COPY_POOL.contains(&m.moveset_id) && !pool.iter().any(|p| p.0 == m.moveset_id) {
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
    let hidden = if id == SWORD_MOVESET { Vec::new() } else { own.character.nodes.iter().enumerate().filter(|(_, n)| n.name.starts_with("bone_")).map(|(i, _)| i).collect() };
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
    }
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
            head_live: false,
            head_off: [false; 2],
            faces: Default::default(),
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
                game.copiers[i] = Some(Copier::new(assets, game.bodies[i].clone(), &index));
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
                let len = f.current_move(&gd).frames;
                f.frame = if len >= 151 { len - 149 } else { 1 };
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
        let seed = self.camera.seed;
        self.camera = FightCamera::new(self.camera.data().clone(), true, variant, first_round);
        self.camera.seed = seed;
        self.intro = Some((variant, first_round));
        self.step_camera();
        self
    }

    /// Start the round again: both fighters back at their start positions with full health, fresh clock and
    /// camera (and the fly-in again if the game has one). Characters and stage stay loaded.
    pub fn reset_round(&mut self) {
        self.restart(self.intro);
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
        let seed = self.camera.seed;
        self.camera = FightCamera::new(self.camera.data().clone(), intro, variant, first);
        self.camera.seed = seed;
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
        Snapshot { clip: self.clip(i), partner: f.current.bank == kumite_sim::data::Bank::Partner, root: f.root(), yaw: f.yaw }
    }

    /// The camera's frame, then each fighter's side for the next one: where its pelvis falls on the screen
    /// (0x80037924 projects bone 11 to +0x1278 as it draws; 0x80046234 compares the two for the pad's forward and
    /// the camera's framing).
    fn step_camera(&mut self) -> View {
        let v = self.step_view();
        self.project_sides(&v);
        v
    }

    /// The fighters' sides under view `v`: their pelvis's horizontal place on the screen (scaled to whole units).
    fn project_sides(&mut self, v: &View) {
        let (eye, forward, fov) = view_camera(v);
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
                // off the screen (Tekken Tag's condition 0x5C: x >= 0x215 or < -0x14 on its 512 pixels, about 1.08 of the
                // half width; the 4:3 view's). shortcut: the pelvis, not the three bones +0x628..+0x62C
                let half = (fov / 2.0).tan() * 4.0 / 3.0;
                self.fight.fighters[i].off_screen = (d.dot(right) / depth / half).abs() >= 1.08;
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
            let (a0, a1) = (FighterAnims { own: b0, other: b1 }, FighterAnims { own: b1, other: b0 });
            // Force's other soldier
            let extra_spheres = self.actor_spheres(2);
            let extra_anims = self.extra_body.as_deref().map(|b| FighterAnims { own: b, other: b0 });
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
        let anims: Vec<FighterAnims> = (0..n).map(|k| FighterAnims { own: &bodies[k], other: &bodies[self.fight.opponent_index(k).min(n - 1)] }).collect();
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
        if self.fight.fighters[i].current.bank == kumite_sim::data::Bank::Partner {
            self.body_of(self.fight.opponent_index(i)).unwrap_or(&self.bodies[1 - i])
        } else {
            &self.bodies[i]
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
        let partner = f.current.bank == kumite_sim::data::Bank::Partner;
        let other = if partner { self.body_of(self.fight.opponent_index(k)).unwrap_or(body) } else { body };
        let (anim, frame) = f.animation(&self.fight.data);
        let shown = Snapshot { clip: other.character.clip(anim).map(|c| (c, frame as usize)), partner, root: f.root(), yaw: f.yaw };
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
            // shortcut: Tekken Tag's heads (bodies with bone slots) do not turn: its head joint's rest frame is not
            // Tekken 3's, which the turn assumes; upgrade by turning from the joint's own rest frame
            let mut on = self.head_live && !self.head_off[i] && self.bodies[i].bones.is_empty() && m.flags24.raw & 0x4000_0000 != 0;
            if on && m.active[0] != 0 {
                let from = (m.active[1] as i32).max(m.bytes_18.frame_count_ram as i32 - 0x11);
                on = f.frame as i32 >= from;
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
        if let (None, Some(local)) = (self.display, self.heads[i].local) {
            turn_head(&self.bodies[i], &mut pose, local);
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
        shown_pose(&self.bodies[i], other, &shown)
    }

    /// Collision volumes of both fighters for the pose currently shown (game units).
    pub fn spheres(&self) -> [BodyVolumes; 2] {
        std::array::from_fn(|i| {
            let (pose, place) = self.pose(i);
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
    // (a hidden node's mesh shrinks into its joint)
    for &n in &body.hidden {
        if let Some(m) = pose.get_mut(n) {
            *m *= Mat4::from_scale(Vec3::ZERO);
        }
    }
    (pose, place)
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
    let make = |layout: &[(usize, Vec3, f32, usize)]| -> Vec<Sphere> {
        layout.iter().map(|&(node, offset, radius, zone)| Sphere { centre: (place.transform_point3(pose[node].w_axis.truncate()) + offset) * UNITS, radius, zone }).collect()
    };
    let bones = body.bones.iter().map(|&n| place.transform_point3(pose[n].w_axis.truncate()) * UNITS).collect();
    BodyVolumes { hurt: make(&body.hurt), push: make(&body.push), bones }
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
