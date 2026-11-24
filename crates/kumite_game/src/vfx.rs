//! Hit sparks, ground dust, heavy-hit sparks, the hit light and the screen shake, as the game does them
//! (effects research; data in `pictures/effects/effects.json` + `atlas.png`, written by export_effects).
//!
//! The game has two effect systems and no blood:
//! - **sprite slots** (0x80076884 / 0x80076E70): camera-facing 188.6 mm squares animating one 32-texel cell a
//!   frame. Hit sparks share a ring of 3 slots (a 4th overwrites the oldest). The red hit spark (sheet 007, CLUT
//!   (32,503)) flies along the strike at 50 mm a frame with a drag of 118/128 and stays at least 250 mm up; the
//!   block spark (steel blue) and the zero-damage spark (pink) stand still. Additive kinds dim linearly over their
//!   last 10 frames. Ground impacts (op 4, and a body landing from the air) are groups of 6 grey dust puffs (50 %
//!   blend) on a 0.5 m ring, one every 2 frames, 250 mm up; 3 groups at most;
//! - **effect objects** (pool 0x8006F4F4): on a hit of 21+ damage, 6 + 20 small additive particles fly off along
//!   the strike and fade through a palette ramp (white -> yellow -> red; blue for a ki-charged attacker), with the
//!   attacker's own character spark and its **hit light** (the fighters lit in the attacker's colour, fading over
//!   48 frames).
//!
//! Tekken Tag's effects (effects.json `bursts`, its 0x3A2758) are bursts instead: a particle on each of its set's
//! paths (a frame each: the position from where the effect starts, the quad's size, its fade), turned about y so that
//! their +z runs along the strike (0x345A00's matrix); the dust as the arena's row says (stages.json `dust`, table
//! 0x437AC0): so many particles on paths drawn at random, larger by its scale, in its colour.
//!
//! Moves fire effects with timed properties: shakes (ops 1-3), a ground impact (op 4), the own / other character
//! spark (ops 6 / 7) and the red / pink / dust sprites (ops 8 / 9 / 0xA) at a bone.

use crate::{Game, UNITS};
use glam::Vec3;
use kumite_sim::detmath::DetMath;
use kumite_sim::MoveEffect;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Added light (original-hardware semi-transparency mode 1).
    Add,
    /// Half of the sprite over half of the picture (mode 0): the dust.
    Half,
    /// Over the picture at the texture's alpha (Tekken Tag's dust, 0x345A00: ALPHA (Cs - Cd) x As + Cd).
    Alpha,
}

/// One animation of the atlas: its cells, one per frame.
#[derive(Clone, Debug, Default)]
pub struct Strip {
    pub cells: Arc<Vec<u32>>,
    pub life: u32,
}

/// A character's own spark and hit-light colour (and its frames); Tekken Tag's: its burst instead (0x3A2BC0).
#[derive(Clone, Debug, Default)]
pub struct CharSpark {
    pub strip: Strip,
    pub light: [f32; 3],
    pub light_frames: u32,
    pub burst: Option<BurstSet>,
    /// Its special part's slots' cells (Devil's: the beam's eye flare, strip and end, 0x3A8D08).
    pub special: Vec<Vec<u32>>,
    /// The strip's picture sheet (0 the atlas; the crossover's Tekken 3 fighters' under Tekken Tag: Tekken 3's atlas).
    pub sheet: usize,
}

/// A burst's set (effects.json `bursts`): its atlas cell, its particles' size (metres at size 1) and brightness,
/// whether each particle takes a path at random, the paths (frames of x, y, z in metres, the size, the fade, the
/// cell), its picture sheet (0 the atlas, k a character's sheet k - 1) and blend.
#[derive(Clone, Debug, Default)]
pub struct BurstSet {
    pub cell: u32,
    pub sheet: usize,
    pub alpha: bool,
    pub size: f32,
    pub level: f32,
    pub random: bool,
    pub paths: Arc<Vec<Vec<[f32; 6]>>>,
}

/// A burst playing: where it started, the set, the path each particle follows, its age in frames, its turn about y
/// (sin, cos), growing by `spin` radians a frame (0x3A2758's f12 into +0x84), and colour (1 = as drawn).
#[derive(Clone, Debug)]
pub struct Burst {
    pub origin: Vec3,
    pub set: BurstSet,
    pub blend: Blend,
    pub paths: Vec<usize>,
    pub age: usize,
    pub turn: [f32; 2],
    pub spin: f32,
    pub colour: [f32; 3],
}

/// The arena's dust (stages.json `dust`, 0x437AC0): particles, their size's scale, their colour (1 = as drawn).
#[derive(Clone, Copy, Debug)]
pub struct Dust {
    pub count: usize,
    pub scale: f32,
    pub colour: [f32; 3],
}

/// effects.json.
#[derive(Clone, Debug, Default)]
pub struct EffectsData {
    pub cols: u32,
    pub rows: u32,
    pub spark: Strip,
    pub zero: Strip,
    pub block: Strip,
    pub dust: Strip,
    pub heavy: [Arc<Vec<u32>>; 2],
    /// By model record.
    pub characters: HashMap<usize, CharSpark>,
    /// Ground impact puff offsets (metres, world).
    pub ground: Vec<Vec3>,
    /// Tekken Tag's bursts by strip name (spark, zero, block, dust), drawn in those strips' place.
    pub bursts: HashMap<String, BurstSet>,
    /// Tekken Tag's characters' picture sheets (`pictures/effects/` file, columns, rows): sprite sheet k + 1.
    pub sheets: Vec<(String, u32, u32)>,
    /// Gon's fire (its start and loop, its end) and gas, and their sheet (Tekken Tag's: Tekken 3's atlas, `t3`).
    pub fire: Strip,
    pub fire_end: Strip,
    pub gas: Strip,
    pub objects_sheet: usize,
}

impl EffectsData {
    pub fn load(assets: &kumite_assets::Assets) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_slice(&kumite_fs::read(&assets.root.join("pictures/effects/effects.json")).ok()?).ok()?;
        let cells = |c: &serde_json::Value| Arc::new(c.as_array().into_iter().flatten().filter_map(|x| x.as_u64()).map(|x| x as u32).collect::<Vec<_>>());
        let strip = |name: &str| Strip { cells: cells(&v["strips"][name]["cells"]), life: v["strips"][name]["life"].as_u64().unwrap_or(0) as u32 };
        let burst = |b: &serde_json::Value, sheet: usize| {
            let f = |x: &serde_json::Value| x.as_f64().unwrap_or(0.0) as f32;
            let cell = b["cell"].as_u64().unwrap_or(0) as u32;
            // (a frame's cell, Tekken Tag's characters': else the set's)
            let frame = |x: &serde_json::Value| [0, 1, 2, 3, 4, 5].map(|k| if k == 5 && x[5].is_null() { cell as f32 } else { f(&x[k]) });
            let paths = b["paths"].as_array().into_iter().flatten().map(|p| p.as_array().into_iter().flatten().map(frame).collect()).collect();
            let alpha = b["blend"] == "alpha";
            BurstSet { cell, sheet, alpha, size: f(&b["size"]), level: f(&b["level"]), random: b["random"].as_bool().unwrap_or(false), paths: Arc::new(paths) }
        };
        let mut sheets: Vec<(String, u32, u32)> = Vec::new();
        let characters = v["characters"]
            .as_object()?
            .iter()
            .filter_map(|(m, c)| {
                let light = c["light"].as_array()?;
                let rgb = |k: usize| light.get(k).and_then(|x| x.as_f64()).unwrap_or(255.0) as f32 / 255.0;
                let sheet = c["sheet"]["file"].as_str().map(|file| {
                    sheets.iter().position(|s| s.0 == file).unwrap_or_else(|| {
                        let n = |k: &str| c["sheet"][k].as_u64().unwrap_or(1).max(1) as u32;
                        sheets.push((file.to_string(), n("cols"), n("rows")));
                        sheets.len() - 1
                    }) + 1
                });
                Some((
                    m.parse().ok()?,
                    CharSpark {
                        strip: Strip { cells: cells(&c["cells"]), life: c["life"].as_u64().unwrap_or(30) as u32 },
                        light: [rgb(0), rgb(1), rgb(2)],
                        light_frames: c["light_frames"].as_u64().map_or(LIGHT_FRAMES, |n| n.max(1) as u32),
                        burst: sheet.filter(|_| c["burst"].is_object()).map(|k| burst(&c["burst"], k)),
                        sheet: sheet.unwrap_or(0),
                        special: c["special"].as_array().into_iter().flatten().map(|s| cells(s).to_vec()).collect(),
                    },
                ))
            })
            .collect();
        let ground = v["ground_offsets"]
            .as_array()?
            .iter()
            .map(|o| {
                let k = |i: usize| o.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32 / UNITS;
                Vec3::new(k(0), -k(1), -k(2)) // game space (y down, z away) -> world
            })
            .collect();
        let bursts = v["bursts"].as_object().into_iter().flatten().map(|(name, b)| (name.clone(), burst(b, 0))).collect();
        // (Tekken Tag's: Tekken 3's strips on its atlas, a sheet of its own)
        let t3 = &v["t3"];
        let objects_sheet = t3["sheet"]["file"].as_str().map_or(0, |file| {
            let n = |k: &str| t3["sheet"][k].as_u64().unwrap_or(1).max(1) as u32;
            sheets.push((file.to_string(), n("cols"), n("rows")));
            sheets.len()
        });
        let strips = if t3.is_null() { &v["strips"] } else { &t3["strips"] };
        let object = |name: &str| Strip { cells: cells(&strips[name]["cells"]), life: 0 };
        Some(Self {
            cols: v["atlas"]["cols"].as_u64()? as u32,
            rows: v["atlas"]["rows"].as_u64()? as u32,
            spark: strip("spark"),
            zero: strip("zero"),
            block: strip("block"),
            dust: strip("dust"),
            heavy: [cells(&v["strips"]["heavy_a"]["cells"]), cells(&v["strips"]["heavy_b"]["cells"])],
            characters,
            ground,
            bursts,
            sheets,
            fire: object("fire"),
            fire_end: object("fire_end"),
            gas: object("gas"),
            objects_sheet,
        })
    }
}

/// Metres across per GS unit (1/16 pixel) times w, at the hit effects' projection (0x433440: 768.75 pixels a unit
/// at a w of 16782424 / 64 mm).
const GS_W: f32 = 4.0 / (64.0 * 768.75 * 1000.0);
/// The beam's light's frames (0x3531F0).
const LASER_LIGHT: u32 = 54;

/// Devil's beam on an actor: its flare's frame (gp-0x63D8 + 2 x side) and its segments' jitters (0x8D7820 + 0x48 x side).
#[derive(Clone, Debug)]
struct Laser {
    flare: u32,
    /// Its frames with the end on the floor (+0x10).
    floor: u32,
    jitters: [u16; 36],
}

/// A strip drawn across the screen along a segment (0x3A8BC0's quads): from, to (metres), its half width (metres
/// at the segment), its sheet and cell; added.
#[derive(Clone, Copy, Debug)]
pub struct Ribbon {
    pub from: Vec3,
    pub to: Vec3,
    pub half: f32,
    pub sheet: usize,
    pub cell: u32,
}

/// The sprite size: 32 texels at the game's projection = 188.6 mm, whatever the distance.
const SPRITE: f32 = 0.1886;
/// A sprite is sorted this much nearer than its centre (0x800771DC: ordering-table index (OTZ - 0x6E) >> 3, OTZ a
/// quarter of the screen depth), so one at a joint shows over the limb around it (metres).
const SORT_NEARER: f32 = 0x6E as f32 * 4.0 / UNITS;
/// Red spark speed (6400 / 128 units a frame) and drag.
const SPARK_SPEED: f32 = 0.05;
const DRAG: f32 = 118.0 / 128.0;
/// Sprites keep their centre at least this high (y −0x7D00 / 128).
const MIN_HEIGHT: f32 = 0.25;
/// A hit of this much damage adds the character spark, its light and the heavy-hit particles.
const HEAVY: i32 = 21;
const LIGHT_FRAMES: u32 = 48;

/// A sprite slot (system A).
#[derive(Clone, Debug)]
pub struct SpriteFx {
    pub pos: Vec3,
    pub vel: Vec3,
    /// 1 on the frame it appears; it is gone after `life`.
    pub age: u32,
    pub strip: Strip,
    pub blend: Blend,
    /// Dims over the last 10 frames (additive kinds).
    pub fade: bool,
    pub mirror: bool,
    /// Its picture sheet ([`CharSpark::sheet`]).
    pub sheet: usize,
}

/// A ground impact: 6 puffs, puff k appearing on its 2k-th update.
#[derive(Clone, Debug)]
pub struct Ground {
    pub pos: Vec3,
    pub age: u32,
    pub puffs: Vec<SpriteFx>,
}

/// A heavy-hit particle (system B, types 1 / 2 and 12 / 13).
#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec3,
    /// Unit heading and the speed law: constant (type 2) or easing from v0 to v1 (type 1); game units / 16 a frame.
    pub dir: Vec3,
    pub v0: f32,
    pub v1: f32,
    pub easing: bool,
    pub age: u32,
    pub life: u32,
    pub ramp: usize,
    pub size: f32,
}

/// A quad to draw: centre, size, atlas cell, colour (multiplies the texture), blend.
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub pos: Vec3,
    pub size: f32,
    pub cell: u32,
    pub color: [f32; 4],
    pub blend: Blend,
    pub mirror: bool,
    /// Depth the sprite is drawn nearer the camera than it is (metres; its place and size on screen unchanged).
    pub nearer: f32,
    /// Its picture sheet: 0 the atlas, k a character's sheet k - 1 ([`EffectsData::sheets`]).
    pub sheet: usize,
}

pub struct Vfx {
    pub data: Option<EffectsData>,
    /// The hit-spark ring and its write index.
    pub slots: [Option<SpriteFx>; 3],
    next: usize,
    pub grounds: [Option<Ground>; 3],
    next_ground: usize,
    pub particles: Vec<Particle>,
    pub bursts: Vec<Burst>,
    /// The hit light: colour and frames left (0x80039F0C), where it is, and its frames (Tekken Tag's 54, 0x3531F0).
    pub light: Option<([f32; 3], u32, Vec3, u32)>,
    /// The last step's landings and ground impacts (m: the roots), for an arena's leaves.
    pub landings: Vec<Vec3>,
    /// Camera shake scripts (camera.json `shake`: pitch offsets per frame).
    scripts: Vec<Vec<i32>>,
    shake: Option<(usize, usize)>,
    /// PsyQ rand state (0x8007A318).
    seed: u32,
    /// Bone slot -> joint name (rules.json `bone_slots`).
    slots_joints: Vec<String>,
    /// Each arena's dust (stages.json `dust`), and the fight's arena's.
    dusts: HashMap<usize, Dust>,
    pub dust: Option<Dust>,
    /// Devil's beams playing, by actor (0x8A7AD0 + 0x50 x side), and their strips this frame.
    lasers: Vec<Option<Laser>>,
    pub ribbons: Vec<Ribbon>,
    flares: Vec<Sprite>,
    /// The effect objects' pictures this frame (Gon's fire and gas), and the fires on struck fighters.
    objects: Vec<Sprite>,
    burns: Vec<Burn>,
}

/// A fire on a fighter Gon's fire struck (effect object 0xB, 0x80072E74): on a bone slot of the victim for 60-75
/// frames, its loop's cells (`count` its phase), then rising off it (`rise`, m a frame) 7 frames looped and 5 ending.
#[derive(Clone, Debug)]
pub struct Burn {
    pub victim: usize,
    pub slot: usize,
    pub pos: Vec3,
    pub state: u8,
    pub count: i32,
    pub left: i32,
    pub rise: f32,
    pub mirror: bool,
    /// Tekken Tag's (task 0x3A50B8): `slot` is a drawn node, the cells the attacker's flame's.
    pub tag: bool,
}

/// The bone slots a struck fighter burns on (0x80025180).
const BURN_SLOTS: [usize; 10] = [1, 2, 4, 8, 5, 9, 12, 13, 15, 16];
/// The drawn nodes a fighter True Ogre's fire struck burns on (0x3A54D8's table 0x5198A0).
const TAG_BURN_NODES: [usize; 10] = [1, 31, 37, 56, 62, 79, 81, 86, 90, 95];
/// The pictures' sizes (their corners: 0x80025584 ±432, 0x800256A0 ±345, 0x80025770 ±250 units) and the fires'
/// light (0x8002517C, 48 frames).
const FIRE_SIZE: f32 = 0.864;
/// Tekken Tag's True Ogre's flame at its full size (metres; as PCSX2 shows it at 0.6, a stream as thick as his foe).
const TAG_FIRE_SIZE: f32 = 2.8;
const BURN_SIZE: f32 = 0.690;
const GAS_SIZE: f32 = 0.5;
const FIRE_LIGHT: [f32; 3] = [1.0, 128.0 / 255.0, 0.0];

impl Vfx {
    pub fn new(assets: &kumite_assets::Assets) -> Self {
        let read = |name: &str| -> serde_json::Value { kumite_fs::read(&assets.root.join("gamedata").join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default() };
        let camera = read("camera.json");
        let scripts =
            camera["shake"]["scripts"].as_array().into_iter().flatten().map(|s| s["pitch_offsets"].as_array().into_iter().flatten().map(|v| v.as_i64().unwrap_or(0) as i32).collect()).collect();
        let slots_joints = read("rules.json")["bone_slots"]["joints"].as_array().into_iter().flatten().map(|v| v.as_str().unwrap_or("").to_string()).collect();
        let dusts = read("stages.json")["stages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let d = s.get("dust")?;
                let c = |k: usize| d["colour"][k].as_f64().unwrap_or(128.0) as f32 / 128.0;
                Some((s["id"].as_u64()? as usize, Dust { count: d["count"].as_u64()?.min(64) as usize, scale: d["scale"].as_f64()? as f32, colour: [c(0), c(1), c(2)] }))
            })
            .collect();
        let data = EffectsData::load(assets);
        if data.is_none() {
            log::warn!("no pictures/effects/effects.json: no hit sparks (convert with export_effects)");
        }
        Self {
            data,
            slots: [None, None, None],
            next: 0,
            grounds: [None, None, None],
            next_ground: 0,
            particles: Vec::new(),
            bursts: Vec::new(),
            light: None,
            landings: vec![],
            scripts,
            shake: None,
            seed: 1,
            slots_joints,
            dusts,
            dust: None,
            lasers: Vec::new(),
            ribbons: Vec::new(),
            flares: Vec::new(),
            objects: Vec::new(),
            burns: Vec::new(),
        }
    }

    /// The fight's arena: its dust, when stages.json has one.
    pub fn set_stage(&mut self, stage: usize) {
        self.dust = self.dusts.get(&stage).copied();
    }

    /// PsyQ `rand()`.
    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
        (self.seed >> 16) & 0x7FFF
    }

    /// Two rand()s as one 30-bit value (R = r1 | r2 << 16).
    fn rand2(&mut self) -> u32 {
        let a = self.rand();
        a | self.rand() << 16
    }

    /// The camera pitch offset of the running shake (4096 units per turn), added to the view.
    pub fn shake_pitch(&self) -> i32 {
        self.shake.and_then(|(s, f)| self.scripts.get(s)?.get(f).copied()).unwrap_or(0)
    }

    /// The hit light now: colour and strength (1 when lit, fading to 0 over 48 frames), and its position.
    pub fn hit_light(&self) -> Option<([f32; 3], f32, Vec3)> {
        self.light.map(|(c, t, p, n)| (c, t as f32 / n as f32, p))
    }

    pub fn clear(&mut self) {
        self.slots = [None, None, None];
        self.grounds = [None, None, None];
        self.particles.clear();
        self.bursts.clear();
        self.light = None;
        self.shake = None;
        self.lasers.clear();
        self.ribbons.clear();
        self.flares.clear();
        self.objects.clear();
        self.burns.clear();
    }

    /// A hit-spark slot (the 4th overwrites the oldest). `dir`: the strike direction (moving sparks only).
    fn sprite(&mut self, strip: &Strip, sheet: usize, pos: Vec3, dir: Option<Vec3>, blend: Blend, eye: Vec3) {
        if strip.cells.is_empty() {
            return;
        }
        let vel = dir.map_or(Vec3::ZERO, |d| d.normalize_or_zero() * SPARK_SPEED);
        // the strike direction picks a horizontal flip so the streaks point along the hit
        let d = dir.unwrap_or(Vec3::ZERO);
        let mirror = d.x * (pos.z - eye.z) - d.z * (pos.x - eye.x) < 0.0;
        let pos = Vec3::new(pos.x, pos.y.max(MIN_HEIGHT), pos.z);
        self.slots[self.next] = Some(SpriteFx { pos, vel, age: 1, strip: strip.clone(), blend, fade: blend == Blend::Add, mirror, sheet });
        self.next = (self.next + 1) % self.slots.len();
    }

    /// Effect `name` (a strip's): Tekken Tag's burst where the data has one (its particles from `pos`), else the
    /// strip's sprite.
    fn effect(&mut self, name: &str, strip: &Strip, pos: Vec3, dir: Option<Vec3>, blend: Blend, eye: Vec3) {
        let Some(mut set) = self.data.as_ref().and_then(|d| d.bursts.get(name)).cloned() else {
            return self.sprite(strip, 0, pos, dir, blend, eye);
        };
        let blend = if blend == Blend::Half { Blend::Alpha } else { blend };
        let (n, mut count, mut colour) = (set.paths.len(), set.paths.len(), [1.0; 3]);
        if let Some(d) = self.dust.filter(|_| name == "dust") {
            (count, colour, set.size) = (d.count, d.colour, set.size * d.scale);
        }
        let paths: Vec<usize> = (0..count).map(|k| if set.random { self.rand() as usize % n.max(1) } else { k }).collect();
        self.burst(set, pos, dir, blend, colour);
        if let Some(b) = self.bursts.last_mut() {
            b.paths = paths;
        }
    }

    /// A burst of `set` from `pos`, a particle on each path, turned along `dir`.
    fn burst(&mut self, set: BurstSet, pos: Vec3, dir: Option<Vec3>, blend: Blend, colour: [f32; 3]) {
        // (heading atan2(dx, dz) of the strike, 0x3A2758 -> +0x74)
        let flat = dir.map_or(glam::Vec2::ZERO, |d| glam::Vec2::new(d.x, d.z)).normalize_or_zero();
        let turn = if flat == glam::Vec2::ZERO { [0.0, 1.0] } else { [flat.x, flat.y] };
        let blend = if set.alpha { Blend::Alpha } else { blend };
        let paths = (0..set.paths.len()).collect();
        self.bursts.push(Burst { origin: pos, set, blend, paths, age: 0, turn, spin: 0.0, colour });
    }

    /// A ground impact at a root (x, z): 6 dust puffs 250 mm up (Tekken Tag's: the dust burst at the feet).
    fn ground(&mut self, root: Vec3) {
        if let Some(dust) = self.data.as_ref().filter(|d| d.bursts.contains_key("dust")).map(|d| d.dust.clone()) {
            return self.effect("dust", &dust, Vec3::new(root.x, 0.0, root.z), None, Blend::Half, Vec3::ZERO);
        }
        let pos = Vec3::new(root.x, MIN_HEIGHT, root.z);
        let Some(d) = &self.data else { return };
        let first = SpriteFx { pos: pos + d.ground.first().copied().unwrap_or_default(), vel: Vec3::ZERO, age: 1, strip: d.dust.clone(), blend: Blend::Half, fade: false, mirror: false, sheet: 0 };
        self.grounds[self.next_ground] = Some(Ground { pos, age: 0, puffs: vec![first] });
        self.next_ground = (self.next_ground + 1) % self.grounds.len();
    }

    /// The character spark of model `model` at `pos`, with its hit light; Tekken Tag's its burst (0x3A2BC0: a
    /// particle on each path of its fighter's set, turned along the strike `dir`).
    fn char_spark(&mut self, model: usize, pos: Vec3, dir: Option<Vec3>, eye: Vec3) {
        let Some(c) = self.data.as_ref().and_then(|d| d.characters.get(&model)).cloned() else { return };
        match c.burst {
            Some(set) => self.burst(set, pos, dir, Blend::Add, [1.0; 3]),
            None => self.sprite(&c.strip, c.sheet, pos, None, Blend::Add, eye),
        }
        self.light = Some((c.light, c.light_frames, pos, c.light_frames));
    }

    /// A timed move effect at a position (ops 6..0xA) outside a fight (the embu): `own` / `other` are the model
    /// records whose character sparks ops 6 / 7 show.
    pub fn effect_at(&mut self, op: u32, pos: Vec3, own: usize, other: usize, eye: Vec3) {
        let Some(data) = self.data.clone() else { return };
        match op {
            6 => self.char_spark(own, pos, None, eye),
            7 => self.char_spark(other, pos, None, eye),
            8 => self.effect("spark", &data.spark, pos, None, Blend::Add, eye),
            9 => self.effect("zero", &data.zero, pos, None, Blend::Add, eye),
            _ => self.effect("dust", &data.dust, pos, None, Blend::Half, eye),
        }
    }

    /// A burst of 0x3A2758's kind (0 the hit spark, 1 set 3's, 2 the guard's, 3 the dust) outside a fight (the
    /// endings' ops 17 / 18), turned along `dir`, spinning `spin` radians a frame; dust more than 500 mm up (`high`)
    /// is 3 particles at the set's own size and colour, not the arena's.
    pub fn burst_at(&mut self, kind: usize, pos: Vec3, dir: Option<Vec3>, high: bool, spin: f32) {
        let Some(d) = self.data.clone() else { return };
        let (name, strip, blend) = match kind {
            0 => ("spark", &d.spark, Blend::Add),
            1 => ("zero", &d.zero, Blend::Add),
            2 => ("block", &d.block, Blend::Add),
            _ => ("dust", &d.dust, Blend::Half),
        };
        let (arena, n) = (self.dust, self.bursts.len());
        if high {
            self.dust = Some(Dust { count: 3, scale: 1.0, colour: [1.0; 3] });
        }
        self.effect(name, strip, pos, dir, blend, Vec3::ZERO);
        self.dust = arena;
        if let Some(b) = self.bursts.get_mut(n) {
            b.spin = spin;
        }
    }

    /// A ground impact at a root (metres) outside a fight.
    pub fn ground_at(&mut self, root: Vec3) {
        self.ground(root);
    }

    /// Advance the effects one frame without a fight (the embu).
    pub fn tick(&mut self) {
        self.advance();
        self.ribbons.clear();
        self.flares.clear();
    }

    /// The heavy-hit particles along the strike (0x8006DF34): 6 easing ones and 20 fast straight ones.
    fn heavy(&mut self, at: Vec3, strike: Vec3, ki: bool) {
        let d = strike.normalize_or_zero();
        let (yaw, pitch) = (d.x.datan2(d.z), d.y.datan2(glam::Vec2::new(d.x, d.z).length()));
        let unit = std::f32::consts::TAU / 4096.0;
        let ramp = usize::from(ki);
        let size = if ki { 0.070 } else { 0.110 };
        let heading = |yaw: f32, pitch: f32| Vec3::new(pitch.dcos() * yaw.dsin(), pitch.dsin(), pitch.dcos() * yaw.dcos());
        for _ in 0..6 {
            let r = self.rand2();
            let dy = ((r & 0x7FF) as f32 - 0x3FF as f32) * 2.0 / 3.0 * unit;
            let r2 = self.rand2();
            let dp = ((r2 & 0x7FF) as f32 - 0x3FF as f32) * 2.0 / 3.0 * unit;
            let v0 = ((r >> 6) & 0x1FF) as f32;
            let v1 = ((r >> 8) & 0x1FF) as f32 + 0x680 as f32;
            let life = ((r >> 4) & 31) + 10;
            self.particles.push(Particle { pos: at, dir: heading(yaw + dy, pitch + dp), v0, v1, easing: true, age: 0, life, ramp, size });
        }
        for _ in 0..20 {
            let r = self.rand2();
            let off = |s: u32| ((r >> s) & 0xFF) as f32 - 127.0;
            let jitter = Vec3::new(off(0), off(8), off(16)) / UNITS;
            let r2 = self.rand2();
            let dy = ((r2 & 0x7F) as f32 - 63.0) * unit;
            let dp = (((r2 >> 7) & 0x7F) as f32 - 63.0) * unit;
            let speed = ((r2 >> 12) & 0x7FF) as f32 * 5.0 / 4.0;
            let life = ((r2 >> 10) & 31) + 20;
            let dir = heading(yaw + dy, pitch + dp);
            // the first frame jumps about 8 frames of travel
            let pos = at + jitter + dir * speed / 16.0 / UNITS * 8.0;
            self.particles.push(Particle { pos, dir, v0: speed, v1: speed, easing: false, age: 0, life, ramp, size });
        }
        if self.particles.len() > 80 {
            let extra = self.particles.len() - 80;
            self.particles.drain(..extra); // the pool holds 80
        }
    }

    /// One 60 Hz step: the effects move (each is drawn before it moves), then the last fight step's hits, move
    /// effects and landings spawn new ones.
    pub fn step(&mut self, game: &Game) {
        let (eye, _, _) = crate::view_camera(&game.camera.view);
        let models: Vec<usize> = (0..4).map(|k| game.body_of(k).map_or(0, |b| b.model_record)).collect();
        let fight = &game.fight;
        // (the co-op partner's move effects too)
        let mut actors = vec![(&fight.fighters[0], 0, fight.opponent_index(0)), (&fight.fighters[1], 1, fight.opponent_index(1))];
        if let Some(p) = fight.partner.as_deref() {
            actors.push((p, 3, fight.opponent_index(3)));
        }
        self.step_actors(
            &fight.hits,
            &models,
            &actors,
            eye,
            &|i, node| {
                let (pose, place) = game.pose_actor(i)?;
                Some(place.transform_point3(pose.get(node)?.w_axis.truncate()))
            },
            &|i, name| game.body_of(i)?.character.nodes.iter().position(|n| n.name == name),
        );
        self.objects(&fight.objects.list, &models);
        // Devil's beam (0x378600): from the active window's start to 30 frames past its end, its width narrowing
        self.ribbons.clear();
        self.flares.clear();
        for (i, f) in fight.fighters.iter().enumerate() {
            let (pose, place) = game.pose(i);
            let bones = crate::volumes(&game.bodies[i], &pose, place).bones;
            let [a0, a1] = f.current_move(&fight.data).active.map(|v| v as i32);
            let (k, n) = (f.frame as i32 - a0, a1 - a0 + 30);
            match f.laser(&fight.data, &bones).filter(|_| (0..n).contains(&k)) {
                Some((p, q)) => {
                    // (0x378A28 and 0x378E70, moves 4177, 4179, 4180, set +0xC: the wider beam)
                    let wide = matches!(f.current.index, 4177 | 4179 | 4180);
                    let head = bones.get(2).copied().unwrap_or(p) / UNITS;
                    self.laser(i, models[i], p / UNITS, q / UNITS, head, (n - k) as f32 / n as f32, wide);
                }
                None => self.laser_off(i),
            }
        }
    }

    /// Devil's beam from `from` to `to` (metres) this frame on actor `i` of model record `model` (0x3A8D08): a strip
    /// a 500 mm segment from its end back (at most 36), the end's of its sheet's special slot 2, the rest of slot 1,
    /// each segment's frame its jitter (0..6, random at the start, + 1 a frame); half as wide on screen as 7200 x the
    /// width (100 t + 50, the wider 130 t + 70) over w; the first 15 frames the flare at the eyes `head` (slot 0's
    /// frames, 5529600 / w); where its end lies on the floor, the arena's dust. Its start (0x3A8C50) lights the fighters pink from the eyes (gp-0x6EA8, 54 frames).
    pub fn laser(&mut self, i: usize, model: usize, from: Vec3, to: Vec3, head: Vec3, t: f32, wide: bool) {
        let Some(c) = self.data.as_ref().and_then(|d| d.characters.get(&model)).cloned() else { return };
        let (Some(set), [flare, rest, end, ..]) = (c.burst, c.special.as_slice()) else { return };
        if self.lasers.len() <= i {
            self.lasers.resize(i + 1, None);
        }
        if self.lasers[i].is_none() {
            let mut jitters = [0u16; 36];
            for j in jitters.iter_mut().take(18) {
                *j = (self.rand() & 7) as u16;
            }
            self.lasers[i] = Some(Laser { flare: 0, floor: 0, jitters });
            self.light = Some(([1.0, 0x80 as f32 / 255.0, 1.0], LASER_LIGHT, head, LASER_LIGHT));
        }
        let Some(l) = self.lasers[i].as_mut() else { return };
        if let Some(&cell) = flare.get(l.flare as usize) {
            self.flares.push(Sprite { pos: head, size: 2.0 * 5_529_600.0 * GS_W, cell, color: [1.0; 4], blend: Blend::Add, mirror: false, nearer: 0.0, sheet: set.sheet });
        }
        l.flare += 1;
        // an end on the floor throws the dust there, its first frame and every 8th (0x378500 -> 0x379090)
        let floored = to.y <= 0.0;
        let dust = floored && l.floor & 7 == 0;
        l.floor += u32::from(floored);
        let width = if wide { 130.0 * t + 70.0 } else { 100.0 * t + 50.0 };
        let n = (((to - from).length() * UNITS / 500.0) as usize).min(36);
        let step = (to - from) / n.max(1) as f32;
        for (k, j) in l.jitters.iter_mut().enumerate().take(n) {
            *j %= 7;
            let cells = if k == 0 { end } else { rest };
            let cell = cells.get(*j as usize).copied().unwrap_or(0);
            self.ribbons.push(Ribbon { from: to - step * k as f32, to: to - step * (k + 1) as f32, half: 7200.0 * width * GS_W, sheet: set.sheet, cell });
            *j += 1;
        }
        if dust {
            self.burst_at(3, to, None, false, 0.0);
        }
    }

    /// Actor `i`'s beam is over.
    pub fn laser_off(&mut self, i: usize) {
        if let Some(l) = self.lasers.get_mut(i) {
            *l = None;
        }
    }

    /// The same for any number of actors: `models[k]` actor k's model record; `actors` (fighter, own index,
    /// opponent index) whose move effects play; `at(k, node)` a node's world position (metres) in actor k's pose
    /// shown, `node(k, joint)` the node of a joint name.
    pub fn step_actors(
        &mut self,
        hits: &[kumite_sim::HitEvent],
        models: &[usize],
        actors: &[(&kumite_sim::Fighter, usize, usize)],
        eye: Vec3,
        at: &dyn Fn(usize, usize) -> Option<Vec3>,
        node: &dyn Fn(usize, &str) -> Option<usize>,
    ) {
        self.advance();
        self.landings.clear();
        let Some(data) = self.data.clone() else { return };
        for h in hits {
            let pos = h.point / UNITS;
            let strike = Vec3::new(h.strike.x, h.strike.y, h.strike.z);
            // (Tekken Tag's bursts turn with the strike; Tekken 3's guard and zero sparks stand still)
            let turn = (!data.bursts.is_empty()).then_some(strike);
            if h.blocked {
                self.effect("block", &data.block, pos, turn, Blend::Add, eye);
            } else if h.base_damage == 0 {
                self.effect("zero", &data.zero, pos, turn, Blend::Add, eye);
            } else {
                // every hit: the red spark thrown along the strike (0x80076C28)
                self.effect("spark", &data.spark, pos, Some(strike), Blend::Add, eye);
                // Gon's fire sets the struck one burning (0x80076D24 -> 0x8006EF60)
                // shortcut: a struck Gon burns on all ten slots, the game on one, larger (+0xD bit 5)
                if h.limb == 0x18 && !data.bursts.is_empty() {
                    // Tekken Tag's: a move of descriptor byte 0x18 burns the struck one instead of its sparks (0x35C0E0
                    // -> 0x3A54D8): ten flames on his nodes, each 60-75 frames at its own phase of the 30 cells
                    // shortcut: the smoke (0x3A4970, one in 32 frames) and the embers (0x3A4C28) are not drawn; port them
                    for slot in TAG_BURN_NODES {
                        let count = (self.rand() % 30) as i32;
                        let left = 60 + (self.rand() & 15) as i32;
                        self.burns.push(Burn { victim: h.victim, slot, pos, state: 1, count, left, rise: 0.0, mirror: false, tag: true });
                    }
                } else if matches!(h.limb, 0x18 | 0x1A) {
                    for slot in BURN_SLOTS {
                        let r = self.rand() as i32;
                        self.burns.push(Burn { victim: h.victim, slot, pos, state: 1, count: r >> 4, left: (r & 15) + 60, rise: 0.0, mirror: r & 1 != 0, tag: false });
                    }
                }
                if h.damage.max(h.base_damage) >= HEAVY {
                    self.char_spark(models.get(h.attacker).copied().unwrap_or(0), pos, Some(strike), eye);
                    // fire / projectile attacks (limb bytes 0x18..0x1C) have their own effects instead
                    if !(0x18..=0x1C).contains(&h.limb) {
                        self.heavy(pos, strike, h.ki);
                    }
                }
            }
        }
        self.burn(at, node);
        for &(f, i, o) in actors {
            for e in &f.effects {
                match *e {
                    MoveEffect::Shake(n) => self.shake = Some((n as usize, 0)),
                    MoveEffect::GroundImpact | MoveEffect::LandingImpact => {
                        self.landings.push(f.root() / UNITS);
                        self.ground(f.root() / UNITS);
                    }
                    MoveEffect::AtBone { op, slot } => {
                        let Some(p) = self.slots_joints.get(slot as usize).and_then(|name| node(i, name)).and_then(|n| at(i, n)) else { continue };
                        match op {
                            6 => self.char_spark(models[i], p, None, eye),
                            7 => {
                                // (an opponent past the four bodies: no spark, as its sound is skipped)
                                if let Some(&m) = models.get(o) {
                                    self.char_spark(m, p, None, eye);
                                }
                            }
                            8 => self.effect("spark", &data.spark, p, None, Blend::Add, eye),
                            9 => self.effect("zero", &data.zero, p, None, Blend::Add, eye),
                            _ => self.effect("dust", &data.dust, p, None, Blend::Half, eye),
                        }
                    }
                }
            }
        }
    }

    /// The struck fighters' fires one frame (0x80073054 / 0x800732A4 / 0x80073394).
    fn burn(&mut self, at: &dyn Fn(usize, usize) -> Option<Vec3>, node: &dyn Fn(usize, &str) -> Option<usize>) {
        let mut burns = std::mem::take(&mut self.burns);
        for b in &mut burns {
            b.count += 1;
            match b.state {
                1 => {
                    let n = if b.tag { Some(b.slot) } else { crate::BONE_SLOTS.get(b.slot).and_then(|name| node(b.victim, name)) };
                    if let Some(p) = n.and_then(|n| at(b.victim, n)) {
                        b.pos = p;
                    }
                    if self.rand() >> 8 & 15 == 0 {
                        self.light = Some((FIRE_LIGHT, LIGHT_FRAMES, b.pos, LIGHT_FRAMES));
                    }
                    b.left -= 1;
                    if b.left < 0 && b.tag {
                        b.state = 2;
                    } else if b.left < 0 {
                        (b.state, b.count) = (2, 0);
                    }
                }
                _ => {
                    // (rising: 0x18000 more a frame in 16.16 units)
                    b.rise += 1.5 / UNITS;
                    b.pos.y += b.rise;
                    // (Tekken Tag's: 16 frames, fading, 0x3A5318)
                    b.left -= 1;
                    if b.state == 2 && (b.count >= 8 && !b.tag || b.left < -16) {
                        (b.state, b.count) = (3, 0);
                    }
                }
            }
        }
        burns.retain(|b| b.state < 3 || b.count < 5 && !b.tag);
        self.burns = burns;
    }

    /// The effect objects' pictures (drawn after their move, 0x8006FC84): the fire's growing, looped and ending
    /// cells, 2 frames each, lighting the fighters now and then (0x800720EC); the gas puff's.
    fn objects(&mut self, list: &[kumite_sim::objects::Object], models: &[usize]) {
        use kumite_sim::objects::Kind;
        self.objects.clear();
        let Some(d) = self.data.clone() else { return };
        let sprite = |strip: &Strip, k: i32, pos: Vec3, size: f32, blend: Blend, mirror: bool| {
            let cell = *strip.cells.get(usize::try_from(k).ok()?)?;
            let a = if blend == Blend::Half { 0.5 } else { 1.0 };
            Some(Sprite { pos, size, cell, color: [1.0, 1.0, 1.0, a], blend, mirror, nearer: 0.0, sheet: d.objects_sheet })
        };
        for o in list {
            let pos = o.pos / UNITS;
            let s = match (o.kind, o.state) {
                (Kind::Fire, 1) => sprite(&d.fire, o.count >> 1, pos, FIRE_SIZE, Blend::Add, o.mirror),
                (Kind::Fire, 2) => sprite(&d.fire, 4 + ((o.count & 6) >> 1), pos, FIRE_SIZE, Blend::Add, o.mirror),
                (Kind::Fire, _) => sprite(&d.fire_end, o.count >> 1, pos, FIRE_SIZE, Blend::Add, o.mirror),
                // (Ogre's breath: the fire's cells, its end's corners turned over every other frame, 0x800719C0)
                // shortcut: drawn square at the fire's size, the PS1's quad is 690 x 864; upgrade with a sprite of two sizes
                (Kind::Breath, 1) => sprite(&d.fire, o.count >> 1, pos, FIRE_SIZE, Blend::Add, o.mirror),
                (Kind::Breath, 2) => sprite(&d.fire, 4 + ((o.count & 6) >> 1), pos, FIRE_SIZE, Blend::Add, o.mirror),
                (Kind::Breath, _) => sprite(&d.fire_end, o.count >> 1, pos, FIRE_SIZE, Blend::Add, o.count & 1 != 0),
                // (Tekken Tag's True Ogre's flame: its special part's first slot, frame by frame of 30, growing from
                // 0.2 to 0.6 its first 8 frames, fading its last 7 (+0x54, +0x58), added)
                // shortcut: its quad's size (0x345A00's for +0x44 192) is not read, it is matched by eye to PCSX2's
                (Kind::TagFire, 1..) => {
                    let c = models.get(o.owner).and_then(|m| d.characters.get(m));
                    let cell = c.and_then(|c| c.special.first()).and_then(|s| s.get(usize::try_from(o.frames).ok()?)).copied();
                    let scale = if o.state == 1 { 0.2 + 0.05 * o.count as f32 } else { 0.6 };
                    let fade = if o.state == 4 { o.count as f32 / 14.0 } else { 0.0 };
                    c.and_then(|c| c.burst.as_ref()).zip(cell).map(|(set, cell)| Sprite {
                        pos,
                        size: TAG_FIRE_SIZE * scale,
                        cell,
                        color: [1.0, 1.0, 1.0, 1.0 - fade],
                        blend: Blend::Add,
                        mirror: false,
                        nearer: 0.0,
                        sheet: set.sheet,
                    })
                }
                (Kind::Gas, _) => sprite(&d.gas, o.count >> 1, pos, GAS_SIZE, Blend::Half, o.mirror),
                _ => None,
            };
            self.objects.extend(s);
            if matches!(o.kind, Kind::Fire | Kind::Breath) && self.rand() >> 8 & 15 == 0 {
                self.light = Some((FIRE_LIGHT, LIGHT_FRAMES, pos, LIGHT_FRAMES));
            }
        }
        for b in &self.burns {
            if b.tag {
                // (the attacker's flame cells at +0x54 0.4 of its quad +0x44 128, the flame's 192 at 0.6 being
                // TAG_FIRE_SIZE; added, fading as it rises)
                let c = models.get(1 - b.victim.min(1)).and_then(|m| d.characters.get(m));
                let cell = c.and_then(|c| c.special.first()).and_then(|s| s.get(b.count.rem_euclid(30) as usize)).copied();
                let fade = if b.state == 2 { (-b.left) as f32 / 16.0 } else { 0.0 };
                self.objects.extend(c.and_then(|c| c.burst.as_ref()).zip(cell).map(|(set, cell)| Sprite {
                    pos: b.pos,
                    size: TAG_FIRE_SIZE * 0.4 * 128.0 / 192.0,
                    cell,
                    color: [1.0, 1.0, 1.0, 1.0 - fade],
                    blend: Blend::Add,
                    mirror: false,
                    nearer: 0.0,
                    sheet: set.sheet,
                }));
                continue;
            }
            let s = match b.state {
                1 => sprite(&d.fire, 4 + ((b.count & 6) >> 1), b.pos, BURN_SIZE, Blend::Add, b.mirror),
                2 => sprite(&d.fire, 4 + ((b.count - 1) >> 1), b.pos, BURN_SIZE, Blend::Add, b.mirror),
                _ => sprite(&d.fire_end, b.count >> 1, b.pos, BURN_SIZE, Blend::Add, b.mirror),
            };
            self.objects.extend(s);
        }
    }

    /// Move everything one frame.
    fn advance(&mut self) {
        for s in self.slots.iter_mut() {
            if let Some(fx) = s {
                fx.age += 1;
                fx.vel *= DRAG;
                fx.pos += fx.vel;
                fx.pos.y = fx.pos.y.max(MIN_HEIGHT);
                if fx.age > fx.strip.life.max(1) {
                    *s = None;
                }
            }
        }
        // puff k of a ground impact appears on its 2k-th update (puff 0 with the impact)
        let dust = self.data.as_ref().map(|d| (d.dust.clone(), d.ground.clone()));
        for g in self.grounds.iter_mut().flatten() {
            g.age += 1;
            for p in &mut g.puffs {
                p.age += 1;
            }
            g.puffs.retain(|p| p.age <= p.strip.life.max(1));
            if let Some((strip, offsets)) = &dust {
                let k = g.age as usize / 2;
                if g.age % 2 == 0 && k < offsets.len() {
                    g.puffs.push(SpriteFx { pos: g.pos + offsets[k], vel: Vec3::ZERO, age: 1, strip: strip.clone(), blend: Blend::Half, fade: false, mirror: false, sheet: 0 });
                }
            }
        }
        for g in self.grounds.iter_mut() {
            if g.as_ref().is_some_and(|g| g.age > 12 && g.puffs.is_empty()) {
                *g = None;
            }
        }
        for p in &mut self.particles {
            p.age += 1;
            let t = p.age as f32 / p.life.max(1) as f32;
            let s = if p.easing { p.v0 + (p.v1 - p.v0) * (0.25 + 0.5 * (std::f32::consts::FRAC_PI_2 * t).dcos()) } else { p.v0 };
            p.pos += p.dir * s / 16.0 / UNITS;
        }
        self.particles.retain(|p| p.age < p.life && p.pos.y > 0.0);
        for b in &mut self.bursts {
            b.age += 1;
        }
        self.bursts.retain(|b| b.set.paths.iter().any(|p| b.age < p.len()));
        if let Some((_, t, _, _)) = &mut self.light {
            *t = t.saturating_sub(1);
        }
        if self.light.is_some_and(|l| l.1 == 0) {
            self.light = None;
        }
        if let Some((s, f)) = &mut self.shake {
            *f += 1;
            if self.scripts.get(*s).is_none_or(|v| *f >= v.len()) {
                self.shake = None;
            }
        }
    }

    /// The atlas size in cells (columns, rows).
    pub fn atlas(&self) -> (u32, u32) {
        self.data.as_ref().map_or((1, 1), |d| (d.cols, d.rows))
    }

    /// What to draw this frame.
    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        let sprite = |fx: &SpriteFx| {
            let cell = fx.strip.cells[(fx.age as usize - 1).min(fx.strip.cells.len() - 1)];
            // the last 10 frames dim linearly: 128 - k * 9.6, over 128
            let k = fx.age as i32 - fx.strip.life as i32 + 10;
            let level = if fx.fade && k > 0 { (128.0 - (k as f32 * 9.6).floor()) / 128.0 } else { 1.0 };
            let alpha = if fx.blend == Blend::Half { 0.5 } else { 1.0 };
            Sprite { pos: fx.pos, size: SPRITE, cell, color: [level, level, level, alpha], blend: fx.blend, mirror: fx.mirror, nearer: SORT_NEARER, sheet: fx.sheet }
        };
        out.extend(self.slots.iter().flatten().map(sprite));
        for g in self.grounds.iter().flatten() {
            out.extend(g.puffs.iter().map(sprite));
        }
        // a burst's particles where their paths are at its age: their size, and the brightness times 1 - the fade
        // (0x345C10, 0x345E2C)
        for b in &self.bursts {
            for f in b.paths.iter().filter_map(|&k| b.set.paths.get(k)?.get(b.age)).filter(|f| f[3] > 0.0 && f[4] < 1.0) {
                let a = 1.0 - f[4];
                let k = b.set.level * a;
                let [r, g, bl] = b.colour;
                let color = if b.blend == Blend::Alpha { [r, g, bl, a] } else { [k * r, k * g, k * bl, 1.0] };
                let [s, c] = if b.spin == 0.0 {
                    b.turn
                } else {
                    let a = b.turn[0].datan2(b.turn[1]) + b.spin * b.age as f32;
                    [a.dsin(), a.dcos()]
                };
                let at = Vec3::new(f[0] * c + f[2] * s, f[1], f[2] * c - f[0] * s);
                out.push(Sprite { pos: b.origin + at, size: b.set.size * f[3], cell: f[5] as u32, color, blend: b.blend, mirror: false, nearer: 0.0, sheet: b.set.sheet });
            }
        }
        if let Some(d) = &self.data {
            out.extend_from_slice(&self.flares);
            out.extend_from_slice(&self.objects);
            for p in &self.particles {
                let ramp = &d.heavy[p.ramp];
                let i = (p.age as usize * 16 / p.life.max(1) as usize).min(ramp.len().saturating_sub(1));
                if let Some(&cell) = ramp.get(i) {
                    out.push(Sprite { pos: p.pos, size: p.size, cell, color: [1.0; 4], blend: Blend::Add, mirror: false, nearer: 0.0, sheet: 0 });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A burst: a particle on each path from where it starts, its size and fade the path's; gone after its paths.
    #[test]
    fn a_burst_follows_its_paths() {
        let set = BurstSet { cell: 2, size: 0.1, level: 0.25, paths: Arc::new(vec![vec![[0.0, 0.0, 0.0, 0.0, 1.0, 2.0], [0.1, 0.2, 0.3, 0.5, 0.2, 2.0]], vec![[0.0; 6]]]), ..Default::default() };
        let data = EffectsData { bursts: [("block".to_string(), set)].into_iter().collect(), ..Default::default() };
        let mut v = Vfx {
            data: Some(data.clone()),
            slots: [None, None, None],
            next: 0,
            grounds: [None, None, None],
            next_ground: 0,
            particles: vec![],
            bursts: vec![],
            light: None,
            landings: vec![],
            scripts: vec![],
            shake: None,
            seed: 1,
            slots_joints: vec![],
            dusts: HashMap::new(),
            dust: None,
            lasers: vec![],
            ribbons: vec![],
            flares: vec![],
            objects: vec![],
            burns: vec![],
        };
        v.effect("block", &data.block, Vec3::new(1.0, 1.0, 0.0), None, Blend::Add, Vec3::ZERO);
        assert!(v.sprites().is_empty() && v.slots.iter().all(Option::is_none));
        v.advance();
        let s = v.sprites();
        assert_eq!(s.len(), 1);
        assert!((s[0].pos - Vec3::new(1.1, 1.2, 0.3)).length() < 1e-6 && (s[0].color[0] - 0.2).abs() < 1e-6 && (s[0].size - 0.05).abs() < 1e-6 && s[0].cell == 2);
        v.advance();
        assert!(v.bursts.is_empty());
        // turned with a strike along +x: the path's +z runs along it
        v.effect("block", &data.block, Vec3::ZERO, Some(Vec3::X), Blend::Add, Vec3::ZERO);
        v.advance();
        assert!((v.sprites()[0].pos - Vec3::new(0.3, 0.2, -0.1)).length() < 1e-6);
        // the arena's dust: so many particles at random, larger, in its colour
        v.bursts.clear();
        v.data.as_mut().unwrap().bursts.insert("dust".into(), BurstSet { random: true, paths: Arc::new(vec![vec![[0.0; 6], [0.1, 0.2, 0.3, 0.5, 0.2, 2.0]]; 2]), ..data.bursts["block"].clone() });
        v.dust = Some(Dust { count: 3, scale: 2.0, colour: [0.5, 1.0, 1.0] });
        v.ground(Vec3::ZERO);
        assert_eq!(v.bursts[0].paths.len(), 3);
        v.advance();
        let s = v.sprites();
        assert!(!s.is_empty() && s.iter().all(|s| (s.size - 0.1).abs() < 1e-6 && s.color[0] == 0.5));
        // an ending's dust high up: 3 particles at the set's size and colour, the arena's kept; a spin turns it
        v.bursts.clear();
        v.burst_at(3, Vec3::Y, None, true, std::f32::consts::FRAC_PI_2);
        assert_eq!(v.dust.map(|d| d.count), Some(3));
        v.advance();
        let s = v.sprites();
        assert!(!s.is_empty() && s.iter().all(|s| (s.size - 0.05).abs() < 1e-6 && s.color[0] == 1.0));
        assert!(s.iter().all(|s| (s.pos - Vec3::new(0.3, 1.2, -0.1)).length() < 1e-5), "{:?}", s[0].pos);
        // Tekken Tag's character effect: its burst on its own sheet, each frame's own cell, its light's frames
        v.bursts.clear();
        let paths = Arc::new(vec![vec![[0.0, 0.0, 0.0, 1.0, 0.0, 5.0], [0.0, 0.0, 0.0, 1.0, 0.0, 6.0]]]);
        let burst = Some(BurstSet { sheet: 2, size: 1.0, level: 1.0, paths, ..Default::default() });
        v.data.as_mut().unwrap().characters.insert(7, CharSpark { light: [1.0; 3], light_frames: 54, burst, ..Default::default() });
        v.char_spark(7, Vec3::ZERO, Some(Vec3::X), Vec3::ZERO);
        assert_eq!(v.light.map(|l| l.1), Some(54));
        let cells = |v: &Vfx| v.sprites().iter().map(|s| (s.sheet, s.cell)).collect::<Vec<_>>();
        assert_eq!(cells(&v), [(2, 5)]);
        v.advance();
        assert_eq!(cells(&v), [(2, 6)]);
        // a crossover fighter's (Tekken 3's): its strip on Tekken 3's sheet, no burst
        v.bursts.clear();
        let strip = Strip { cells: Arc::new(vec![1392, 1393]), life: 22 };
        v.data.as_mut().unwrap().characters.insert(247, CharSpark { strip, light: [0.0, 0.75, 1.0], light_frames: 48, sheet: 3, ..Default::default() });
        v.char_spark(247, Vec3::Y, None, Vec3::ZERO);
        assert_eq!(cells(&v), [(3, 1392)]);
    }
}
