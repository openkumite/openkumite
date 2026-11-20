//! Tekken Tag's endings (ok_tekken_tag_converter's `gamedata/endings.json` and `endings/`; its BOWL_ENDINGS.md 7): a
//! real-time scene on an arena, not a movie. Two actor slots, filled from the ending's cast by its script, each play
//! their slot's body track; the ending's camera path films them; the script fades from and to black and flashes
//! white. The interpreter (0x3AAFF4) runs every record whose frame has come, in order; the tracks and the camera
//! are sampled at the camera's frame, clamped at their ends, so a frame is a function of its number alone. It ends
//! at op -1, the camera path's end, or 7200 frames.
//!
//! The EMBU (scene 1, 0x3A97B0: Theater's EMBU and EMBU2) is the same scene in a dialect of its own (0x3A9E48,
//! table 0x519B50): a cast of six, ops 0 / 1 / 14 put a member in a slot, 2 / 3 play track a from its frame b to c
//! (the slot is not drawn before or after), 4 takes camera path a of 22 from frame b, 5 / 6 fade from / to white,
//! 12 Jin's mark, 13 starts its music; it ends at op -1 alone.
//!
//! The hand tracks come in the body clips (the converter's format 26: the finger nodes' turns).
//!
//! Ops 17 / 18 throw 0x3A2758's bursts from an actor's node (see [`Burst`]); ops 21-24 lay 2D objects over the picture
//! (see [`Overlay`]); 4's and 27's 0x3465E0, 26 and 31 change nothing drawn. The arena's background characters are a
//! set of the scene's own or none (0x3D85D8, [`EndingScene::people`]); the EMBU's screen films from farther
//! ([`EndingScene::camera`]) and turns the arena half round.
//!
//! Op 25 puts an arena's object in a hand ([`EndingScene::props`]). Op 12 shows Jin's mark (his texture channel 0's frame 0, [`EndingScene::marks`]).
//!
//! Not played: ops 10 (the characters' own effects, 0x3A2BC0), 15 (True
//! Ogre's fire, 0x3A4EE8), 16 (0x378368's table 0x8A7B70), 28 (True Ogre's and Devil's wings, +0xB052), 30 (objects 0x3ACF50), 32 (gp-0x6E78); the EMBU's 7-11; ops 17 / 18 at nodes 0x4000-0x4002
//! (op 25's prop matrix 0x444CD0, 20 objects 0x3ACCD0); the face track (its joints are not mapped).

use crate::UNITS;
use anyhow::{bail, Context, Result};
use glam::{Mat4, Vec3};
use kumite_assets::{Assets, Character};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// A descriptor (0x51B488): the arena, the cast `[character, costume]` (-1: the player's), the script
/// `[frame, op, a, b, c]`, and its files in `endings/`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Descriptor {
    pub arena: usize,
    pub cast: Vec<[i32; 2]>,
    pub script: Vec<[i32; 5]>,
    pub camera: String,
    pub body: Vec<String>,
    /// The background characters it shows (0x3D85D8: `set`, `people`, `walk` as stages.json's), none when null.
    pub people: Option<Value>,
}

/// gamedata/endings.json, and who wears which costume (roster.json `selections`).
#[derive(Debug)]
pub struct EndingData {
    /// The whole file (Theater's labels and music, the staff roll, the playback's constants).
    pub raw: Value,
    /// Character x costume -> ending (0x51B678).
    table: Vec<Vec<i32>>,
    pub endings: Vec<Descriptor>,
    /// The EMBU's two casts as descriptors (endings.json `embu`; none in an older pack).
    pub embu: Vec<Descriptor>,
    /// Selection (character * 8 + costume) -> model record.
    records: HashMap<usize, usize>,
}

impl EndingData {
    pub fn load(gamedata: &std::path::Path) -> Result<Self> {
        Self::new(kumite_sim::data::load(&gamedata.join("endings.json"))?, &kumite_sim::data::load(&gamedata.join("roster.json"))?)
    }

    /// From endings.json and roster.json.
    pub fn new(raw: Value, roster: &Value) -> Result<Self> {
        let table = serde_json::from_value(raw["table"].clone()).context("endings.json: table")?;
        let endings = serde_json::from_value(raw["endings"].clone()).context("endings.json: endings")?;
        let records = roster["selections"]["entries"].as_array().into_iter().flatten().filter_map(|e| Some((e["selection"].as_u64()? as usize, e["model_record"].as_u64()? as usize))).collect();
        let e = &raw["embu"];
        let embu = (e["casts"].as_array().into_iter().flatten().zip(e["arenas"].as_array().into_iter().flatten()))
            .enumerate()
            .map(|(k, (cast, arena))| {
                let mut d: Descriptor = serde_json::from_value(e.clone()).context("endings.json: embu")?;
                (d.cast, d.arena) = (serde_json::from_value(cast.clone()).context("endings.json: embu casts")?, arena.as_u64().unwrap_or(0) as usize);
                d.people = e["people"].get(k).cloned().filter(|p| !p.is_null());
                Ok(d)
            })
            .collect::<Result<_>>()?;
        Ok(Self { raw, table, endings, embu, records })
    }

    /// The ending of `character` in `costume` (the game's costume number), if it has one.
    pub fn ending(&self, character: usize, costume: usize) -> Option<usize> {
        let e = *self.table.get(character)?.get(costume)?;
        usize::try_from(e).ok().filter(|&e| e < self.endings.len())
    }

    /// An arcade clear's ending and its player's costume (0x3AA9D4), of the team's `members` (character, costume):
    /// the front member's; if it was seen (`seen`: a bit per ending) and the partner's was not, the partner's.
    pub fn pick(&self, members: &[(usize, usize)], seen: u64) -> Option<(usize, usize)> {
        let of = |m: &(usize, usize)| Some((self.ending(m.0, m.1)?, m.1));
        let new = |e: &(usize, usize)| e.0 < 64 && seen >> e.0 & 1 == 0;
        let front = of(members.first()?)?;
        Some(members.get(1).and_then(of).filter(|p| !new(&front) && new(p)).unwrap_or(front))
    }

    /// The model record of `character` in `costume`, else in its first costume there is.
    fn record(&self, character: usize, costume: usize) -> Option<usize> {
        let base = character * 8;
        self.records.get(&(base + costume)).or_else(|| (base..base + 8).find_map(|s| self.records.get(&s))).copied()
    }
}

/// A fade's level 0..=255: from `from` to `to` over `len` frames from frame `at`.
#[derive(Clone, Copy, Debug, Default)]
struct Fade {
    from: i32,
    to: i32,
    at: i32,
    len: i32,
}

impl Fade {
    fn level(&self, c: i32) -> u8 {
        let t = (c - self.at).clamp(0, self.len.max(1));
        (self.from + (self.to - self.from) * t / self.len.max(1)).clamp(0, 255) as u8
    }
}

/// A burst ops 17 / 18 threw this frame (0x3A2758): its kind (0 the hit spark, 1 set 3's, 2 the guard's, 3 the dust),
/// where (metres), the way it turns to, whether it is dust more than 500 mm up (3 particles, not the arena's), its spin
/// (radians a frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub kind: usize,
    pub pos: Vec3,
    pub dir: Option<Vec3>,
    pub high: bool,
    pub spin: f32,
}

/// A 2D object over the picture (ops 21-24's tasks), in the PS2's 640 x 448 screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    /// Black outside the circle about (x, y) of radius r (op 22: 0x3CF850).
    Iris { x: f32, y: f32, r: f32 },
    /// Black outside the oval about (320, 224) of half-height 220 and half-width 220 x 1.33 (op 23: 0x3CFAD8(220, 133)).
    Oval,
    /// Colour [r, g, b] (0-255) over the screen at alpha a (ALPHA (Cs - Cd) x As + Cd: op 21's black or white,
    /// 0x43FD10 entries 9 / 10, at b / 128; op 24 kind 2, table 0x437D10).
    Tint([u8; 3], f32),
    /// The colour less the screen (op 24 kinds 0 and 1: ALPHA (Cs - Cd) x Ad, the frame's alpha taken as 1).
    Negative([u8; 3]),
}

/// Op 24's screen fills (table 0x437D10: r, g, b, a, then ALPHA's A, B, C, D).
const FILLS: [([u8; 3], u8, bool); 3] = [([255, 255, 255], 126, true), ([205, 157, 200], 76, true), ([162, 51, 50], 64, false)];

pub struct EndingScene {
    /// The ending (None: the EMBU).
    pub ending: Option<usize>,
    pub arena: usize,
    /// The background characters on it (the descriptor's `people`; none when none).
    pub people: Option<Value>,
    /// The cast's bodies (with the ending's tracks as clips `ending_EE_K`) and their model records.
    pub characters: Vec<Arc<Character>>,
    pub records: Vec<usize>,
    script: Vec<[i32; 5]>,
    /// The camera paths (an ending's one, the EMBU's 22), per frame: eye, target (game units, y down), H.
    cameras: Vec<Vec<[f32; 7]>>,
    /// The camera's path and start: (scene frame, path frame).
    path: usize,
    camera_at: (i32, i32),
    next: usize,
    /// The scene frame shown (+1 a step; -1 before the first).
    c: i32,
    /// Per actor slot: the cast member in it and the track it plays; the EMBU's: from scene frame s, frames b..=c.
    slots: [(Option<usize>, usize); 2],
    ranges: [Option<(i32, i32, i32)>; 2],
    /// Per actor slot: its texture channel shows frame 0, Jin's mark (op 12; a cast member put in shows frame 1, 0x3A82C8).
    pub marks: [bool; 2],
    /// The music started (the EMBU's op 13: its BGM).
    pub music: Option<usize>,
    /// The bursts thrown this frame.
    pub bursts: Vec<Burst>,
    /// Ops 21-24's objects: (scene frame started, op, a, b, c).
    objects: Vec<(i32, i32, i32, i32, i32)>,
    /// Op 25's props: (scene frame started, actor a, object b, frames c).
    props: Vec<(i32, i32, i32, i32)>,
    black: Fade,
    white: Fade,
    pub done: bool,
}

/// The longest scene (0x3AAFF4's cap).
const MAX_FRAMES: i32 = 7200;

impl EndingScene {
    /// Ending `ending` with the player's costume `costume` (the game's number): its cast's bodies, tracks and camera.
    pub fn new(assets: &Assets, data: &EndingData, ending: usize, costume: usize) -> Result<Self> {
        Self::with(assets, data, data.endings.get(ending).context("no such ending")?, Some(ending), costume)
    }

    /// The EMBU with cast `cast` (0: Theater's EMBU, 1: EMBU2's animals).
    pub fn embu(assets: &Assets, data: &EndingData, cast: usize) -> Result<Self> {
        let d = data.embu.get(cast).context("no EMBU in this conversion (convert the game again)")?;
        let mut s = Self::with(assets, data, d, None, 0)?;
        s.black = Fade::default();
        Ok(s)
    }

    fn with(assets: &Assets, data: &EndingData, d: &Descriptor, ending: Option<usize>, costume: usize) -> Result<Self> {
        let file = |name: &str| -> Result<Vec<u8>> {
            // (a plain file name only: one with a folder in it would read outside `endings/`)
            if std::path::Path::new(name).file_name() != Some(name.as_ref()) {
                bail!("an ending file that is not a plain file name: {name:?}");
            }
            kumite_fs::read(&assets.root.join("endings").join(name)).with_context(|| format!("endings/{name}"))
        };
        let tracks: Vec<Vec<u8>> = d.body.iter().map(|b| file(b)).collect::<Result<_>>()?;
        let (mut characters, mut records) = (Vec::new(), Vec::new());
        for &[c, k] in &d.cast {
            let k = if k < 0 { costume } else { k as usize };
            let record = data.record(c.max(0) as usize, k).with_context(|| format!("no model of character {c} costume {k}"))?;
            let mut ch = assets.character(record)?;
            for t in &tracks {
                ch.add_motions(t)?;
            }
            characters.push(Arc::new(ch));
            records.push(record);
        }
        let cam = file(&d.camera)?;
        let u32_at = |o: usize| cam.get(o..o.checked_add(4)?).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
        // a path: u32 frames, then 7 floats a frame; the EMBU's file: u32 paths, their offsets, the paths
        let path = |o: usize| -> Vec<[f32; 7]> {
            let n = u32_at(o).unwrap_or(0) as usize;
            let frames = cam.get(o + 4..).unwrap_or_default().as_chunks::<28>().0;
            frames.iter().take(n).map(|f| std::array::from_fn(|k| f32::from_le_bytes([f[4 * k], f[4 * k + 1], f[4 * k + 2], f[4 * k + 3]]))).collect()
        };
        let cameras = match ending {
            Some(_) => vec![path(0)],
            None => (0..u32_at(0).unwrap_or(0).min(64) as usize).map(|k| path(u32_at(4 + 4 * k).unwrap_or(u32::MAX) as usize)).collect(),
        };
        if cameras.iter().any(Vec::is_empty) || cameras.is_empty() {
            bail!("{}: an empty camera path", d.camera);
        }
        Ok(Self {
            ending,
            arena: d.arena,
            people: d.people.clone(),
            characters,
            records,
            script: d.script.clone(),
            cameras,
            path: 0,
            camera_at: (0, 0),
            next: 0,
            c: -1,
            slots: [(None, 0); 2],
            ranges: [None; 2],
            marks: [false; 2],
            music: None,
            bursts: Vec::new(),
            objects: Vec::new(),
            props: Vec::new(),
            black: Fade { from: 255, to: 255, at: 0, len: 1 },
            white: Fade::default(),
            done: false,
        })
    }

    /// The scene frame shown.
    pub fn frame(&self) -> i32 {
        self.c
    }

    /// The camera path's (and the tracks') frame.
    fn camera_frame(&self) -> usize {
        (self.c - self.camera_at.0 + self.camera_at.1).max(0) as usize
    }

    /// One 60 Hz frame: the next frame, its records.
    pub fn step(&mut self) {
        if self.done {
            return;
        }
        self.c += 1;
        self.bursts.clear();
        let c = self.c;
        while let Some(&[frame, op, a, b, last]) = self.script.get(self.next) {
            if frame > c {
                break;
            }
            self.next += 1;
            if self.ending.is_none() {
                self.embu_op(op, a, b, last);
                continue;
            }
            match op {
                0 | 1 => (self.slots[op as usize].0, self.marks[op as usize]) = (usize::try_from(a).ok().filter(|&m| m < self.characters.len()), false),
                2 | 3 => self.slots[op as usize - 2].1 = a.max(0) as usize,
                4 => self.camera_at = (c, b),
                5 => self.white = Fade { from: (b * 2).min(255), to: 0, at: c, len: a },
                6 => self.white = Fade { from: self.white.level(c) as i32, to: 255, at: c, len: a },
                19 => self.black = Fade { from: self.black.level(c) as i32, to: 0, at: c, len: a },
                20 => self.black = Fade { from: self.black.level(c) as i32, to: 255, at: c, len: a },
                // kind a >> 1 at slot a & 1's node b, turned away from the other slot's node c (0x3AB504)
                17 => {
                    let s = (a & 1) as usize;
                    if let (Some(p), Some(q)) = (self.node_at(s, b), self.node_at(1 - s, last)) {
                        self.bursts.push(Burst { kind: (a as usize >> 1) & 3, pos: p, dir: Some(p - q), high: false, spin: 0.0 });
                    }
                }
                // the dust at slot a & 1's node b (0x62: its place on the floor), spinning c x 2 pi / 4096 a frame
                // (0x3AB644)
                18 => {
                    let s = (a & 1) as usize;
                    let at = if b == 0x62 { self.node_at(s, 0).map(|p| (p * Vec3::new(1.0, 0.0, 1.0), false)) } else { self.node_at(s, b).map(|p| (p, p.y * UNITS > 500.0)) };
                    if let Some((pos, high)) = at {
                        self.bursts.push(Burst { kind: 3, pos, dir: None, high, spin: last as f32 * std::f32::consts::TAU / 4096.0 });
                    }
                }
                // slot a & 1's texture channel 0 to frame 1 (b = 0) or 0 (0x3AB924)
                12 => self.marks[(a & 1) as usize] = b != 0,
                21..=24 => self.objects.push((c, op, a, b, last)),
                25 => self.props.push((c, a, b, last)),
                -1 => self.done = true,
                _ => {}
            }
        }
        let ending = self.ending.is_some() && self.camera_frame() >= self.frames().len();
        self.done |= ending || c >= MAX_FRAMES;
    }

    /// An EMBU record (0x3A9E48): cast member a into slot 0 / 1 (14: slot 0, as it plays), slot 0 / 1 plays track a
    /// from frame b to `last`, camera path a from frame b, white from / to over a frames, the music, the end.
    fn embu_op(&mut self, op: i32, a: i32, b: i32, last: i32) {
        let c = self.c;
        // (0x3A9E94: a member past the six is the first)
        let member = usize::try_from(a).ok().filter(|&m| m < self.characters.len()).or(Some(0));
        match op {
            0 | 1 => (self.slots[op as usize].0, self.ranges[op as usize], self.marks[op as usize]) = (member, None, false),
            14 => self.slots[0].0 = member,
            2 | 3 => {
                let s = op as usize - 2;
                self.slots[s].1 = a.max(0) as usize;
                self.ranges[s] = Some((c, b, last));
            }
            4 => (self.path, self.camera_at) = (usize::try_from(a).unwrap_or(0).min(self.cameras.len() - 1), (c, b)),
            5 => self.white = Fade { from: 255, to: 0, at: c, len: a },
            6 => self.white = Fade { from: self.white.level(c) as i32, to: 255, at: c, len: a },
            // slot a & 1's Jin: the mark off (b = 0, 0x3A88A0) or on (0x3A8890)
            12 => self.marks[(a & 1) as usize] = b != 0,
            13 => self.music = Some(1),
            -1 => self.done = true,
            _ => {}
        }
    }

    /// The camera path playing.
    fn frames(&self) -> &[[f32; 7]] {
        self.cameras.get(self.path).map_or(&[], Vec::as_slice)
    }

    /// Ops 21-24's objects this frame, in the order drawn. Op 22's iris closes from radius 640 to c & 0xFF over
    /// c >> 8 frames and stays (0x3AC8B0); 23's oval stands a frames (0x3AC978), 24's fill b (0x3ACAB0), 21's c
    /// (0x3AC7C0).
    pub fn overlays(&self) -> Vec<Overlay> {
        let mut out = Vec::new();
        for &(at, op, a, b, last) in &self.objects {
            let t = self.c - at;
            let shown = |frames: i32| t < frames.max(1);
            match op {
                21 if shown(last) => out.push(Overlay::Tint(if a < 2 { [0; 3] } else { [255; 3] }, b as f32 / 128.0)),
                22 => {
                    let (frames, end) = ((last >> 8) & 0xFF, last & 0xFF);
                    // (+0x54 less (640 - end) / frames a frame, floats)
                    let r = (640.0 - (640 - end) as f32 / frames.max(1) as f32 * t as f32).max(end as f32);
                    out.push(Overlay::Iris { x: a as f32, y: b as f32, r });
                }
                23 if shown(a) => out.push(Overlay::Oval),
                24 if shown(b) => {
                    if let Some(&(rgb, alpha, negative)) = FILLS.get(a as usize) {
                        out.push(if negative { Overlay::Negative(rgb) } else { Overlay::Tint(rgb, alpha as f32 / 128.0) });
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The objects ending op 25 holds this frame (0x3ACB08 -> 0x3D10D0): the arena's object (its file
    /// `stages/stage_NN_prop_K.glb`, [`EndingScene::prop_files`]) and where it is drawn (metres): for c frames at
    /// actor a's (0 or not) node 38 or 63 by arena and object, or with b's 0x4000 where arena 3 lays it (its matrix
    /// 0x8D7E00: turned -0.349 about x and 0.785 about y, at z 1500). On other arenas, or other objects, none.
    pub fn props(&self) -> Vec<(usize, Mat4)> {
        let mut out = vec![];
        for &(at, a, b, frames) in &self.props {
            let (k, laid) = (b & 0x3FFF, b & 0x4000 != 0);
            if self.c - at >= frames.max(1) {
                continue;
            }
            let node = match (self.arena, k) {
                (3, 19) if laid => {
                    let m = Mat4::from_translation(Vec3::new(0.0, 0.0, -1.5)) * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_4) * Mat4::from_rotation_x(-0.349_065_84);
                    out.push((19, m));
                    continue;
                }
                (7, 0) => 63,
                (3, 19) | (5, 0) | (7, 1 | 2) => 38,
                _ => continue,
            };
            if let Some((_, pose, place)) = self.pose(usize::from(a != 0)) {
                if let Some(m) = pose.get(node) {
                    out.push((k as usize, place * *m));
                }
            }
        }
        out
    }

    /// The files of the props the script holds (op 25: [`EndingScene::props`]), each with its object.
    pub fn prop_files(&self) -> Vec<(usize, String)> {
        let mut out: Vec<(usize, String)> =
            (self.script.iter()).filter(|r| r[1] == 25 && self.ending.is_some()).map(|r| (r[3] & 0x3FFF) as usize).map(|k| (k, format!("stages/stage_{:02}_prop_{k}.glb", self.arena))).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Black over the picture (fades in and out), then white (flashes), 0..=255.
    pub fn fades(&self) -> (u8, u8) {
        (self.black.level(self.c), self.white.level(self.c))
    }

    /// Actor slot `s` this frame: its cast member, node matrices and placement (its track holds where it stands).
    pub fn pose(&self, s: usize) -> Option<(usize, Vec<Mat4>, Mat4)> {
        let (m, track) = self.slots.get(s).copied()?;
        let m = m?;
        let ch = self.characters.get(m)?;
        let (clip, frame) = match self.ending {
            Some(e) => (ch.clip(&format!("ending_{e:02}_{track}"))?, self.camera_frame()),
            // (the EMBU's slot is drawn only while its range plays)
            None => {
                let (at, from, to) = self.ranges.get(s).copied().flatten()?;
                let f = from + self.c - at;
                (ch.clip(&format!("embu_{track}"))?, usize::try_from(f).ok().filter(|_| f <= to)?)
            }
        };
        Some((m, ch.pose(clip, frame, false), self.turn()))
    }

    /// The EMBU's screen (11) draws the arena turned half round (0x36712C: its matrix 0x8794A0 a turn of pi about y,
    /// identity in the endings): the actors and the camera turned so about it instead.
    fn turn(&self) -> Mat4 {
        if self.ending.is_none() {
            Mat4::from_rotation_y(std::f32::consts::PI)
        } else {
            Mat4::IDENTITY
        }
    }

    /// Slot `s`'s node `n` (the game's numbering, the model's nodes') in metres; none for an empty slot or 0x4000 up.
    fn node_at(&self, s: usize, n: i32) -> Option<Vec3> {
        let (_, pose, place) = self.pose(s)?;
        Some(place.transform_point3(pose.get(usize::try_from(n).ok()?)?.w_axis.truncate()))
    }

    /// The camera: eye and target in metres (y up), and the vertical field of view.
    pub fn camera(&self) -> (Vec3, Vec3, f32) {
        let path = self.frames();
        let Some(&(mut f)) = path.get(self.camera_frame().min(path.len().saturating_sub(1))) else { return (Vec3::ZERO, Vec3::Z, 1.0) };
        if self.ending.is_none() {
            // the EMBU's screen (11) films from 1.2 times as far from a target 8 higher, at 0.8 of H (0x3930B4)
            let d = [f[0] - f[3], f[1] - f[4], f[2] - f[5]];
            f[4] -= 8.0;
            (f[0], f[1], f[2], f[6]) = (f[3] + d[0] * 1.2, f[4] + d[1] * 1.2, f[5] + d[2] * 1.2, f[6] * 0.8);
        }
        let to_m = |x: f32, y: f32, z: f32| Vec3::new(x, -y, -z) / UNITS;
        // H on Tekken Tag's 512-pixel-wide screen, as its fight camera's (crate::view_camera takes Tekken 3's 368)
        let h = (f[6] * 368.0 / 512.0) as i64;
        let turn = self.turn();
        (turn.transform_point3(to_m(f[0], f[1], f[2])), turn.transform_point3(to_m(f[3], f[4], f[5])), crate::view_camera(&crate::View { h: h.max(1), ..Default::default() }).2)
    }
}
