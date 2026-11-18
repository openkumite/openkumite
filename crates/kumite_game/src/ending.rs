//! Tekken Tag's endings (ok_tekken_tag_converter's `gamedata/endings.json` and `endings/`; its BOWL_ENDINGS.md 7): a
//! real-time scene on an arena, not a movie. Two actor slots, filled from the ending's cast by its script, each play
//! their slot's body track; the ending's camera path films them; the script fades from and to black and flashes
//! white. The interpreter (0x3AAFF4) runs every record whose frame has come, in order; the tracks and the camera
//! are sampled at the camera's frame, clamped at their ends, so a frame is a function of its number alone. It ends
//! at op -1, the camera path's end, or 7200 frames.
//!
//! Not played: the effect ops (10, 12, 15-18, 21-32: sparks, objects, show / hide [L]), the face and hand tracks
//! (their joints are not mapped), the arenas' background characters.

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
}

/// gamedata/endings.json, and who wears which costume (roster.json `selections`).
#[derive(Debug)]
pub struct EndingData {
    /// The whole file (Theater's labels and music, the staff roll, the playback's constants).
    pub raw: Value,
    /// Character x costume -> ending (0x51B678).
    table: Vec<Vec<i32>>,
    pub endings: Vec<Descriptor>,
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
        Ok(Self { raw, table, endings, records })
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

pub struct EndingScene {
    pub ending: usize,
    pub arena: usize,
    /// The cast's bodies (with the ending's tracks as clips `ending_EE_K`) and their model records.
    pub characters: Vec<Arc<Character>>,
    pub records: Vec<usize>,
    script: Vec<[i32; 5]>,
    /// Per frame: eye, target (game units, y down), H.
    camera: Vec<[f32; 7]>,
    /// The camera's start: (scene frame, path frame).
    camera_at: (i32, i32),
    next: usize,
    /// The scene frame shown (+1 a step; -1 before the first).
    c: i32,
    /// Per actor slot: the cast member in it and the track it plays.
    slots: [(Option<usize>, usize); 2],
    black: Fade,
    white: Fade,
    pub done: bool,
}

/// The longest scene (0x3AAFF4's cap).
const MAX_FRAMES: i32 = 7200;

impl EndingScene {
    /// Ending `ending` with the player's costume `costume` (the game's number): its cast's bodies, tracks and camera.
    pub fn new(assets: &Assets, data: &EndingData, ending: usize, costume: usize) -> Result<Self> {
        let d = data.endings.get(ending).context("no such ending")?;
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
        let n = cam.get(..4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes) as usize;
        let camera: Vec<[f32; 7]> =
            cam.get(4..).unwrap_or_default().as_chunks::<28>().0.iter().take(n).map(|f| std::array::from_fn(|k| f32::from_le_bytes([f[4 * k], f[4 * k + 1], f[4 * k + 2], f[4 * k + 3]]))).collect();
        if camera.is_empty() {
            bail!("ending {ending}: an empty camera path");
        }
        Ok(Self {
            ending,
            arena: d.arena,
            characters,
            records,
            script: d.script.clone(),
            camera,
            camera_at: (0, 0),
            next: 0,
            c: -1,
            slots: [(None, 0); 2],
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
        let c = self.c;
        while let Some(&[frame, op, a, b, _]) = self.script.get(self.next) {
            if frame > c {
                break;
            }
            self.next += 1;
            match op {
                0 | 1 => self.slots[op as usize].0 = usize::try_from(a).ok().filter(|&m| m < self.characters.len()),
                2 | 3 => self.slots[op as usize - 2].1 = a.max(0) as usize,
                4 => self.camera_at = (c, b),
                5 => self.white = Fade { from: (b * 2).min(255), to: 0, at: c, len: a },
                6 => self.white = Fade { from: self.white.level(c) as i32, to: 255, at: c, len: a },
                19 => self.black = Fade { from: self.black.level(c) as i32, to: 0, at: c, len: a },
                20 => self.black = Fade { from: self.black.level(c) as i32, to: 255, at: c, len: a },
                -1 => self.done = true,
                _ => {}
            }
        }
        self.done |= self.camera_frame() >= self.camera.len() || c >= MAX_FRAMES;
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
        let clip = ch.clip(&format!("ending_{:02}_{track}", self.ending))?;
        Some((m, ch.pose(clip, self.camera_frame(), false), Mat4::IDENTITY))
    }

    /// The camera: eye and target in metres (y up), and the vertical field of view.
    pub fn camera(&self) -> (Vec3, Vec3, f32) {
        let f = self.camera[self.camera_frame().min(self.camera.len() - 1)];
        let to_m = |x: f32, y: f32, z: f32| Vec3::new(x, -y, -z) / UNITS;
        // H on Tekken Tag's 512-pixel-wide screen, as its fight camera's (crate::view_camera takes Tekken 3's 368)
        let h = (f[6] * 368.0 / 512.0) as i64;
        (to_m(f[0], f[1], f[2]), to_m(f[3], f[4], f[5]), crate::view_camera(&crate::View { h: h.max(1), ..Default::default() }).2)
    }
}
