//! Tekken Tag Tournament's front end, when the game data has it (roster.json `front_end`, which the Tekken Tag
//! converter reads from the PS2 executable: its docs/FRONTEND.md): the title, the main menu and its modes (Gallery is
//! not here yet; Theater once an ending was seen, Tekken Bowl once unlocked), the character select with the tag
//! partner, OPTIONS, the arcade ladder with CONTINUE, the modes' runs, the unlock schedule, the endings
//! (`gamedata/endings.json`: the arcade clear's staff roll and ending, Theater), the fight HUD and Tekken Bowl's, drawn
//! from its 2D pictures (`pictures/FFF_NNN.png`) on its 640 x 448 screen. The title, main menu and select move, light
//! and sound as the PS2's (FRONTEND.md 9: the title out of white, the menu's sliding rows and band, the select's
//! lightning, frame, pulsing cursors, RANDOM's roll, timer and plates, its 3D characters standing and posing), in frames.
//!
//! The fight HUD is the PS2's (FRONTEND.md 4): the tag bars, plates, lamps, timer, the ladder's STAGE and time, the
//! round's texts in the chrome font as its printf lays them out. The pause menu, OPTIONS and GAME OPTIONS are the
//! PS2's with their help line (FRONTEND.md 8); OPTIONS' CONTROLLER SETUP and ADJUST DISPLAY open OpenKumite's KEY
//! CONFIGURATION and GRAPHICS. GAME OPTIONS keeps every value; of them the fights read DIFFICULTY LEVEL, FIGHT COUNT,
//! ROUND TIME, CHARACTER CHANGE AT CONTINUE, and the menus HELP MESSAGES (shortcut: GUARD DAMAGE, CHARACTER SELECT
//! CURSOR, QUICK SELECT, CHICKEN MARKING, SPEAKER OUT and the volumes are kept but not played by).
//!
//! Not decoded, OpenKumite's own: the VS screen's code (laid out as the PS2 shows it), which button gives which
//! costume, Theater's layout.

use crate::art::{additive, Art, Screen as View, SCREEN_H, SCREEN_W};
use crate::menu::{lcg, sfx, FightSetup, Key, Mode, Outcome, Roster};
use egui::{Color32, Pos2, Rect};
use kumite_game::bout::{Announcement, Bout, Phase};
use kumite_game::ending::EndingData;
use kumite_game::ending::Overlay;
use serde_json::Value;
use std::sync::Arc;

/// Tekken Tag's screen.
const W: f32 = 640.0;
const H: f32 = 448.0;
/// Main menu ids (= the game's mode numbers, FRONTEND.md 2).
const ARCADE: u32 = 0;
const VS: u32 = 1;
const TEAM: u32 = 2;
const TIME_ATTACK: u32 = 3;
const SURVIVAL: u32 = 4;
const ONE_ARCADE: u32 = 5;
const ONE_VS: u32 = 6;
const PRACTICE: u32 = 7;
const THEATER: u32 = 9;
pub const BOWL: u32 = 8;
/// Tekken Ball (OpenKumite's: Tekken 3's mode, with the Tekken 3 slice's ball.json): no mode of the game's.
pub const BALL: u32 = 0x100;
/// Tekken Force (OpenKumite's: Tekken 3's mode, with the Tekken 3 slice's force.json): no mode of the game's.
pub const FORCE: u32 = 0x101;
const OPTIONS: u32 = 11;
/// Armor King's character id.
const ARMOR_KING: usize = 26;
/// The options the fights read (the config's DIFFICULTY LEVEL, FIGHT COUNT, ROUND TIME).
const OPTION_ROWS: [usize; 3] = [0, 1, 2];
/// OPTIONS' entries (0x439CC8): GAME OPTIONS, CONTROLLER SETUP, RECORDS, ADJUST DISPLAY, MEMORY CARD(PS2), EXIT.
const OPT_GAME: usize = 0;
const OPT_CONTROLLER: usize = 1;
const OPT_RECORDS: usize = 2;
const OPT_DISPLAY: usize = 3;
const OPT_MEMORY_CARD: usize = 4;
/// GAME OPTIONS' row of OpenKumite's ARCADE ARENAS (with the arcade pack), before DEFAULT.
const ARCADE_ROW: usize = usize::MAX;
/// GAME OPTIONS' CHARACTER CHANGE AT CONTINUE row.
const CHANGE_ROW: usize = 3;
/// GAME OPTIONS' GUARD DAMAGE, HELP MESSAGES, SPEAKER OUT, BGM / SE VOLUME rows.
const GUARD_ROW: usize = 4;
/// GAME OPTIONS' CHARACTER SELECT CURSOR (1 HOLD), QUICK SELECT, CHICKEN MARKING rows.
const CURSOR_ROW: usize = 5;
const QUICK_ROW: usize = 6;
const HELP_ROW: usize = 8;
const SPEAKER_ROW: usize = 9;
const BGM_ROW: usize = 10;
const SE_ROW: usize = 11;
/// The last unlock stage (FRONTEND.md 7).
const LAST_STAGE: u32 = 14;
/// Frames the result and message screens stay (START skips).
const MESSAGE_FRAMES: u32 = 360;
/// The title's logo after its fade from white, before the attract goes on (0x3BCC88).
const TITLE_FRAMES: u32 = 300;
/// The VS screen's frames: the PS2 holds it while the fight loads (0x3C45B0's states 4-6), about 4 s in PCSX2.
pub const VS_FRAMES: u32 = 240;
/// The arcade pack's arenas (OpenKumite's ids, apart from Tekken Tag's 0-22): its stage_NN.glb is 100 + NN, its
/// hidden arena 199.
const ARCADE_ARENA: usize = 100;
const ARCADE_HIDDEN: usize = 199;
/// The Tekken Tag arena whose camera row (camera.json `arena_projection`, which the simulation reads: condition 0x5C)
/// and floor sounds an arcade arena takes, the same for every player: Paul's, the row most arenas share (H 615,
/// 600..900, the dolly).
pub const ARCADE_STAND_IN: usize = 0;

fn int(v: &Value) -> usize {
    v.as_u64().unwrap_or(0) as usize
}
fn num(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}
fn ints(v: &Value) -> Vec<usize> {
    v.as_array().into_iter().flatten().filter_map(Value::as_u64).map(|x| x as usize).collect()
}

/// A select-grid slot (0x440A68): its neighbours, its place, its character (None: RANDOM).
#[derive(Clone, Debug, Default)]
pub struct Slot {
    pub next: [Option<usize>; 4],
    pub x: f32,
    pub y: f32,
    pub character: Option<usize>,
}

/// roster.json `front_end`.
#[derive(Debug)]
pub struct Data {
    raw: Value,
    pub slots: Vec<Slot>,
    /// Character ids selectable and in the CPU pool from the start (0x51E850).
    start: Vec<usize>,
    /// The unlock schedule (0x440818): stage, kind (0 CPU pool, 1 selectable), character.
    schedule: Vec<(u32, u32, usize)>,
    /// The arcade's rival teams per character (0x51F3C8), their fallback (0x51F560), the CPU pairs (0x5207F0).
    rivals: Vec<Vec<Option<usize>>>,
    fallback: Vec<usize>,
    pairs: Vec<[usize; 2]>,
    /// The arenas to fight on and their names (stages/index.json): those of the stage select table 0x440770, 0-19
    /// (20 is Tekken Bowl's hall, 21 and 22 are in no list: FIGHT_DATA.md).
    pub arenas: Vec<(usize, String)>,
    /// The arcade pack's arenas there are and their titles ([`arcade_arenas`]).
    pub arcade: Vec<(usize, String)>,
    /// ROUND TIME's clocks in frames (rounds.json `round_time`; past them: untimed).
    pub round_frames: Vec<u32>,
    /// The endings, the staff roll and Theater (endings.json; None: a pack without them).
    pub endings: Option<Arc<EndingData>>,
    /// Tekken Ball's court (None: no Tekken 3 slice, no TEKKEN BALL on the menu).
    pub ball: Option<usize>,
    /// The Tekken 3 slice's Tekken Force is there (TEKKEN FORCE on the menu).
    pub force: bool,
    /// SURVIVAL's CPU profiles (ai.json `survival`): max wins, difficulty, lowest and highest level.
    survival: Vec<[usize; 4]>,
}

impl Data {
    /// From roster.json (None without `front_end`: an older pack, the plain front end); `arenas`: the pack's
    /// (stages/index.json); `rounds`: rounds.json; `endings`: endings.json's; `arcade`: the arcade pack's arenas; `ball`:
    /// Tekken Ball's court; `force`: Tekken Force is there; `ai`: ai.json.
    #[allow(clippy::too_many_arguments)]
    pub fn load(roster: &Value, arenas: &Value, rounds: &Value, endings: Option<EndingData>, arcade: Vec<(usize, String)>, ball: Option<usize>, force: bool, ai: &Value) -> Option<Arc<Self>> {
        let raw = roster.get("front_end").filter(|f| f.is_object())?.clone();
        let opt = |v: &Value| v.as_u64().map(|x| x as usize);
        let slots = raw["select"]["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| Slot { next: ["up", "down", "left", "right"].map(|k| opt(&s[k])), x: num(&s["x"]), y: num(&s["y"]), character: opt(&s["character"]) })
            .collect();
        let schedule = raw["unlocks"]["schedule"].as_array().into_iter().flatten().map(|e| (int(&e["stage"]) as u32, int(&e["kind"]) as u32, int(&e["character"]))).collect();
        let rivals = raw["arcade"]["rivals"].as_array().into_iter().flatten().map(|r| r.as_array().into_iter().flatten().map(opt).collect()).collect();
        let pairs = raw["arcade"]["pairs"].as_array().into_iter().flatten().map(|p| [int(&p[0]), int(&p[1])]).collect();
        Some(Arc::new(Self {
            slots,
            start: ints(&raw["select"]["selectable"]),
            schedule,
            rivals,
            fallback: ints(&raw["arcade"]["fallback"]),
            pairs,
            arenas: arenas.as_array().into_iter().flatten().filter_map(|a| Some((a["id"].as_u64()? as usize, a["name"].as_str().unwrap_or_default().to_string()))).filter(|a| a.0 < 20).collect(),
            arcade,
            round_frames: ints(&rounds["round_time"]["options_frames"]).into_iter().map(|f| f as u32).collect(),
            endings: endings.map(Arc::new),
            ball,
            force,
            survival: ai["survival"]["rows"].as_array().into_iter().flatten().map(|r| [int(&r["max_wins"]), int(&r["difficulty"]), int(&r["levels"][0]), int(&r["levels"][1])]).collect(),
            raw,
        }))
    }

    /// SURVIVAL's CPU after `streak` wins (0x380120): the first row whose max wins reach it, its difficulty on an odd
    /// draw (else NORMAL, EASY for an EASY row), a level in its range.
    /// shortcut: drawn per fight from the streak, the game's per round from the AI generator; draw it there once
    /// replays record the streak.
    fn survival_profile(&self, streak: u32) -> Option<(usize, usize)> {
        let &[_, diff, lo, hi] = self.survival.iter().find(|r| r[0] >= streak as usize)?;
        let mut seed = streak ^ 0x5EED;
        let diff = if crate::menu::lcg(&mut seed, 256) & 1 == 1 { diff } else { diff.min(1) };
        Some((diff, lo + crate::menu::lcg(&mut seed, hi.saturating_sub(lo) + 1)))
    }

    /// The arenas a fight is drawn on: Tekken Tag's, and the arcade pack's unless OPTIONS turned them off.
    // shortcut: Tekken Tag has no online rooms; once it has, leave the arcade arenas out online (the arcade pack is
    // outside the fingerprint: a peer may lack it)
    pub fn pool(&self, p: &Progress) -> Vec<usize> {
        self.arenas.iter().chain(self.arcade.iter().filter(|_| !p.arcade_off)).map(|a| a.0).collect()
    }

    /// Character `c` is selectable (kind 1) or in the CPU pool (kind 0) at unlock stage `stage` (0x3BFBB0).
    fn has(&self, kind: u32, stage: u32, c: usize) -> bool {
        self.start.contains(&c) || self.schedule.iter().any(|&(s, k, ch)| s <= stage && k == kind && ch == c)
    }
    pub fn selectable(&self, stage: u32, c: usize) -> bool {
        self.has(1, stage, c)
    }
    fn in_pool(&self, stage: u32, c: usize) -> bool {
        self.has(0, stage, c)
    }
    /// Tekken Bowl is open at unlock stage `stage` (the schedule's kind 5).
    fn bowl_open(&self, stage: u32) -> bool {
        self.schedule.iter().any(|&(s, k, _)| k == 5 && s <= stage)
    }

    /// An option's default (0x439CE0 descriptors) and its values.
    fn option(&self, row: usize) -> (usize, Vec<String>) {
        let o = &self.raw["options"][row];
        (int(&o["default"]), o["values"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).collect())
    }

    fn picture(&self, path: &[&str]) -> String {
        path.iter()
            .fold(&self.raw, |v, k| match k.parse::<usize>() {
                Ok(i) => &v[i],
                Err(_) => &v[*k],
            })
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

/// The sound id the front end plays for `role` (roster.json `front_end.sounds`: FRONTEND.md 9).
fn sound(d: &Data, role: &str) -> u16 {
    int(&d.raw["sounds"][role]) as u16
}

/// An arena of `pool` drawn at random.
fn pick(pool: &[usize], seed: &mut u32) -> Option<usize> {
    pool.get(lcg(seed, pool.len())).copied()
}

/// The arcade pack's file of arena `stage` (None: a Tekken Tag arena, or Tekken 3's).
pub fn arcade_file(stage: usize) -> Option<String> {
    match stage {
        ARCADE_HIDDEN => Some("arcade/hidden.glb".into()),
        s if (ARCADE_ARENA..ARCADE_HIDDEN).contains(&s) => Some(format!("arcade/stage_{:02}.glb", s - ARCADE_ARENA)),
        _ => None,
    }
}

/// An arcade arena's fight music (BGM id): of the fight tracks the game's random music draws from (9 + rand() % 14,
/// 0x3BFEC0), 9 + NN for stage_NN, the last (22, Unknown's arena's) for the hidden one.
pub fn arcade_bgm(stage: usize) -> Option<usize> {
    match stage {
        ARCADE_HIDDEN => Some(22),
        s => arcade_file(s).map(|_| 9 + s - ARCADE_ARENA),
    }
}

/// The arcade pack's arenas over Tekken Tag's game root `root` (its layer: `arcade/`), by their ids, with the titles
/// of the pack's manifest.json (a browser's page puts it at `arcade/manifest.json`; natively it is beside the pack's
/// assets/: packs/t3arcade/, the game root being packs/ttt/assets/).
pub fn arcade_arenas(root: &std::path::Path) -> Vec<(usize, String)> {
    let beside = root.parent().and_then(std::path::Path::parent).map(|p| p.join("t3arcade/manifest.json"));
    let manifest: Value = [Some(root.join("arcade/manifest.json")), beside].iter().flatten().find_map(|f| kumite_fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let title = |file: &str| {
        let m = manifest["stages"].as_array().into_iter().flatten().find(|s| s["game_file"].as_str().is_some_and(|g| g.ends_with(file)));
        m.and_then(|m| m["title"].as_str()).map(|t| t.split(" (").next().unwrap_or(t).to_string())
    };
    (ARCADE_ARENA..ARCADE_ARENA + 13)
        .chain([ARCADE_HIDDEN])
        .filter_map(|id| {
            let file = arcade_file(id)?;
            let name = || title(&file).unwrap_or_else(|| if id == ARCADE_HIDDEN { "HIDDEN ARENA".into() } else { format!("ARCADE {}", id - ARCADE_ARENA + 1) });
            kumite_fs::exists(&root.join(&file)).then(|| (id, name()))
        })
        .collect()
}

/// What the Tekken Tag save keeps (FRONTEND.md 7: the config's unlock stage, play points, endings, options).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Progress {
    /// The unlock stage (config +0x18) and the play points towards the next (+0x20).
    pub stage: u32,
    pub points: u32,
    /// The endings seen (+0x10): a bit per ending (endings.json).
    pub seen: u64,
    /// DIFFICULTY LEVEL, FIGHT COUNT, ROUND TIME as set (None: the defaults).
    pub options: Option<[usize; 3]>,
    /// OPTIONS' ARCADE ARENAS (OpenKumite's) set OFF.
    pub arcade_off: bool,
    /// Tekken Bowl's TOP 5 BOWLERS (save block 0x46BC70; None: bowl.json's defaults), and its bonuses kept: a
    /// 200-point game (+0x202, JUKEBOX's), a 300 (+0x204).
    pub bowlers: Option<Vec<kumite_game::bowl::Record>>,
    pub bowl_bonus: [bool; 2],
    /// BOWLING OPTIONS' HELP MESSAGES and SPECIAL EFFECTS set OFF (+0x1FE, +0x200: both ON at first); the JUKEBOX's
    /// track (the music player's list, 0x455D80; None: OFF, the hall's own).
    pub bowl_off: [bool; 2],
    pub jukebox: Option<usize>,
    /// Tekken Force's keys and hi-score here (apart from Tekken 3's records).
    pub force_keys: u32,
    pub force_hiscore: u32,
    /// The other GAME OPTIONS (rows 3.., CHARACTER CHANGE AT CONTINUE to SE VOLUME) as set (missing: the defaults).
    pub settings: Vec<usize>,
    /// RECORDS (0x440D50): the picks of each character (by id), TIME ATTACK's best times, SURVIVAL's most wins.
    pub usage: Vec<u32>,
    pub times: Vec<Record>,
    pub survivors: Vec<Record>,
    /// The main menu's entries entered once (a bit by mode; config +0x24): an unlocked one blinks until then.
    pub entered: u32,
    /// CHARACTER SELECT CURSOR HOLD's memory (0x4157F8, config +0x38 / +0x39): per side the slots of its first and
    /// second pick (None: the select's `start_slots`, none for the second).
    pub cursors: [[Option<usize>; 2]; 2],
}

/// A TIME ATTACK (frames) or SURVIVAL (wins) record: the team's character ids and the name entered.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Record {
    pub score: u32,
    pub members: Vec<usize>,
    pub name: String,
}

/// The records kept of each (TIME ATTACK's table 0x440D58 holds 10; shortcut: SURVIVAL's count is not decoded).
const RECORDS: usize = 10;

/// The save's progress as last loaded or changed (the menus read it; the app keeps it in the save), as the
/// DIFFICULTY of the Tekken 3 menus.
static PROGRESS: std::sync::Mutex<Progress> = std::sync::Mutex::new(Progress {
    stage: 0,
    points: 0,
    seen: 0,
    options: None,
    arcade_off: false,
    bowlers: None,
    bowl_bonus: [false; 2],
    bowl_off: [false; 2],
    jukebox: None,
    force_keys: 0,
    force_hiscore: 0,
    settings: Vec::new(),
    usage: Vec::new(),
    times: Vec::new(),
    survivors: Vec::new(),
    entered: 0,
    cursors: [[None; 2]; 2],
});

pub fn progress() -> Progress {
    PROGRESS.lock().map_or_else(|e| e.into_inner().clone(), |p| p.clone())
}

pub fn set_progress(p: Progress) {
    match PROGRESS.lock() {
        Ok(mut g) => *g = p,
        Err(e) => *e.into_inner() = p,
    }
}

impl Progress {
    pub fn options(&self, d: &Data) -> [usize; 3] {
        self.options.unwrap_or_else(|| OPTION_ROWS.map(|r| d.option(r).0))
    }

    /// Where `r` would stand among SURVIVAL's (`survival`: most wins first) or TIME ATTACK's records (fastest first);
    /// None: not among them.
    pub fn rank(&self, d: &Data, survival: bool, r: &Record) -> Option<usize> {
        let list = self.list(d, survival);
        let at = list.iter().position(|o| if survival { r.score > o.score } else { r.score < o.score }).unwrap_or(list.len());
        (at < RECORDS && (r.score > 0 || !survival)).then_some(at)
    }

    /// Keep `r` among the records.
    pub fn record(&mut self, d: &Data, survival: bool, r: Record) {
        if let Some(at) = self.rank(d, survival, &r) {
            let mut list = self.list(d, survival);
            list.insert(at, r);
            list.truncate(RECORDS);
            *if survival { &mut self.survivors } else { &mut self.times } = list;
        }
    }

    /// SURVIVAL's (`survival`) or TIME ATTACK's records: the save's, else those a new save starts with
    /// (`records.defaults`, 0x3C88A0).
    fn list(&self, d: &Data, survival: bool) -> Vec<Record> {
        let kept = if survival { &self.survivors } else { &self.times };
        if !kept.is_empty() {
            return kept.clone();
        }
        let key = if survival { "survivors" } else { "times" };
        d.raw["records"]["defaults"][key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Record { score: int(&r["score"]) as u32, members: ints(&r["members"]), name: r["name"].as_str().unwrap_or_default().to_string() })
            .collect()
    }

    /// GAME OPTIONS row `row`'s value as set, else its default.
    pub fn setting(&self, d: &Data, row: usize) -> usize {
        match row {
            0..=2 => self.options(d)[row],
            _ => self.settings.get(row - 3).copied().unwrap_or_else(|| d.option(row).0),
        }
    }

    /// A match finished: a play point (shortcut: one a match; the per-match value 0x35AE14 is not decoded); 50 make
    /// the next stage. An arcade clear with a new ending (`clear`) makes the next stage too.
    /// True when the stage advanced.
    pub fn played(&mut self, d: &Data, clear: Option<usize>) -> bool {
        let per = d.raw["unlocks"]["points_per_stage"].as_u64().unwrap_or(50) as u32;
        self.points += 1;
        let mut up = false;
        if self.points >= per {
            self.points = 0;
            up = true;
        }
        if let Some(c) = clear.filter(|&c| c < 64 && self.seen >> c & 1 == 0) {
            self.seen |= 1 << c;
            up = true;
        }
        if up && self.stage < LAST_STAGE {
            self.stage += 1;
            return true;
        }
        false
    }
}

/// GAME OPTIONS' sound: the music's level (BGM VOLUME by `volumes.bgm`, of 128), the effects' (SE VOLUME / `volumes.se`),
/// SPEAKER OUT MONO; None with a pack without `volumes`.
pub fn volumes(d: &Data, p: &Progress) -> Option<(f32, f32, bool)> {
    let v = &d.raw["volumes"];
    let bgm = v["bgm"].get(p.setting(d, BGM_ROW))?.as_f64()? as f32 / 128.0;
    Some((bgm, p.setting(d, SE_ROW) as f32 / v["se"].as_f64()?.max(1.0) as f32, p.setting(d, SPEAKER_ROW) == 0))
}

/// A tag battle's members at its end by side, in their run order: (health, recoverable cap, full health), 16.16.
pub type Members = [[(i32, i32, i32); 2]; 2];

/// One side's fighters: (roster index, costume).
pub type Team = Vec<(usize, usize)>;

/// A mode's run: who fights, the ladder, the score.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Run {
    /// The main menu id.
    pub mode: u32,
    pub teams: [Team; 2],
    pub humans: [bool; 2],
    /// Arcade, Time Attack, 1 ON 1: the CPU teams and arenas, fight by fight.
    pub ladder: Vec<(Team, usize)>,
    pub fight: usize,
    /// Team Battle: who of each team fights now and the health carried (16.16).
    pub member: [usize; 2],
    pub health: [Option<i32>; 2],
    /// Survival: the wins; VS: the tally; Time Attack: the frames.
    pub wins: [u32; 2],
    pub frames: u32,
    pub stage: usize,
    pub options: [usize; 3],
    /// GUARD DAMAGE ON (settings +0xC, 0x43B804): a guarded hit takes a tenth of its damage.
    #[serde(default)]
    pub guard_damage: bool,
    /// SURVIVAL: the team's members carried (Game::tag_carry).
    #[serde(default)]
    pub carry: [Option<[(i32, i32); 2]>; 2],
    /// Tekken Ball: the ball type (ball.json `types`: BEACH, GUM, IRON), and 2 ON 2 (OpenKumite's: each side's second
    /// in the court too, the CPU's).
    #[serde(default)]
    pub ball: usize,
    #[serde(default)]
    pub doubles: bool,
}

/// A tag battle's mode (SURVIVAL too: its records keep two members, 0x3C1620 refills both; PRACTICE: two a side, as
/// seen in PCSX2).
fn tag(mode: u32) -> bool {
    matches!(mode, ARCADE | VS | TIME_ATTACK | SURVIVAL | PRACTICE)
}
fn ladder_mode(mode: u32) -> bool {
    matches!(mode, ARCADE | TIME_ATTACK | ONE_ARCADE)
}

impl Run {
    /// The run of `mode` with each side's picks (side 1 empty: the CPU's).
    pub fn new(d: &Data, roster: &Roster, progress: &Progress, mode: u32, teams: [Team; 2], seed: &mut u32) -> Self {
        let humans = [true, !teams[1].is_empty() && !matches!(mode, PRACTICE)];
        let mut run = Self {
            mode,
            teams,
            humans,
            ladder: vec![],
            fight: 0,
            member: [0; 2],
            health: [None; 2],
            wins: [0; 2],
            frames: 0,
            stage: pick(&d.pool(progress), seed).unwrap_or(0),
            options: progress.options(d),
            guard_damage: progress.setting(d, GUARD_ROW) == 1,
            carry: [None; 2],
            ball: 0,
            doubles: false,
        };
        if ladder_mode(mode) {
            run.ladder = ladder(d, roster, progress.stage, &d.pool(progress), &run.teams[0], tag(mode), seed);
        }
        if mode == BALL {
            run.stage = d.ball.unwrap_or(run.stage);
        }
        if run.teams[1].is_empty() && mode != BOWL {
            // Team Battle against the CPU: as many as the player's; Survival: the first opponents
            let n = if mode == TEAM { run.teams[0].len() } else { 1 + usize::from(tag(mode) || mode == BALL) };
            run.teams[1] = cpu_team(d, roster, progress.stage, &run.teams[0], n, false, seed);
        }
        run
    }

    /// The fight's setup now.
    pub fn setup(&self, d: &Data) -> FightSetup {
        let mode = match self.mode {
            ARCADE | ONE_ARCADE => Mode::Arcade,
            VS | ONE_VS => Mode::Versus,
            TEAM => Mode::TeamBattle,
            TIME_ATTACK => Mode::TimeAttack,
            SURVIVAL => Mode::Survival,
            BALL => Mode::Ball,
            _ => Mode::Practice,
        };
        let cpu = self.ladder.get(self.fight);
        let side = |k: usize| match (k, cpu) {
            (1, Some((t, _))) => t.first().copied().unwrap_or((0, 0)),
            _ => self.teams[k].get(self.member[k]).or(self.teams[k].first()).copied().unwrap_or((0, 0)),
        };
        // the CPU's profile (0x380120): the DIFFICULTY option and the fight's number (0..7); TIME ATTACK at NORMAL;
        // SURVIVAL by the streak
        let (difficulty, level) = match self.mode {
            TIME_ATTACK => (1, self.fight.min(7)),
            SURVIVAL => d.survival_profile(self.wins[0]).unwrap_or((self.options[0], 0)),
            _ => (self.options[0], self.fight.min(7)),
        };
        let stage = cpu.map_or(self.stage, |c| c.1);
        FightSetup { mode, players: [side(0), side(1)], stage, difficulty, level, handicap: [100; 2], ball: (self.mode == BALL).then_some((self.ball, 0)), partners: self.doubles() }
    }

    /// Tekken Ball 2 ON 2: each side's second (the CPU plays both).
    fn doubles(&self) -> Option<crate::menu::Doubles> {
        let second = |k: usize| self.teams[k].get(1).copied();
        let partners = [second(0)?, second(1)?];
        (self.mode == BALL && self.doubles).then_some(crate::menu::Doubles { partners, seats: [Some(0), self.humans[1].then_some(1), None, None] })
    }

    /// The CPU plays side `k` (practice's dummy stands).
    pub fn cpu(&self, k: usize) -> bool {
        !self.humans[k] && self.mode != PRACTICE
    }

    /// The player's character ids.
    fn members(&self, roster: &Roster) -> Vec<usize> {
        self.teams[0].iter().filter_map(|p| roster.characters.get(p.0)).map(|c| c.id).collect()
    }

    /// The arcade ladder's STAGE (1..8).
    pub fn stage_number(&self) -> Option<u32> {
        ladder_mode(self.mode).then_some(self.fight as u32 + 1)
    }

    /// The VS screen's mode and fight number (1..).
    pub fn card(&self) -> (u32, u32) {
        (self.mode, self.fight as u32 + 1)
    }

    /// A ladder that offers CONTINUE? when lost (and loses as a team, settings +0x1F).
    pub fn continues(&self) -> bool {
        matches!(self.mode, ARCADE | TIME_ATTACK | ONE_ARCADE)
    }

    /// Time Attack: its clock runs over the whole run.
    pub fn timed(&self) -> bool {
        self.mode == TIME_ATTACK
    }

    /// The tag partners of this fight (None: one on one).
    pub fn bench(&self) -> Option<[(usize, usize); 2]> {
        let cpu = self.ladder.get(self.fight).map(|c| &c.0).unwrap_or(&self.teams[1]);
        tag(self.mode).then_some([*self.teams[0].get(1)?, *cpu.get(1)?])
    }

    /// The fight's rounds to win and round clock (frames; None: untimed), from OPTIONS: FIGHT COUNT, ROUND TIME; the
    /// final fight one round (settings +0x1E = 0); Team Battle and Survival one round a match.
    pub fn rules(&self, round_frames: &[u32]) -> (u32, Option<u32>) {
        let last = ladder_mode(self.mode) && self.fight + 1 == self.ladder.len();
        let rounds = if last || matches!(self.mode, SURVIVAL) { 1 } else { self.options[1] as u32 + 1 };
        (rounds.clamp(1, 5), round_frames.get(self.options[2]).copied())
    }

    /// The match is over (`winner`: side; `health`, `max`: the fighters' at the end, 16.16): what comes next.
    #[allow(clippy::too_many_arguments)]
    pub fn after(mut self, d: &Data, roster: &Roster, progress: &mut Progress, winner: Option<usize>, health: [i32; 2], max: [i32; 2], tag: Option<Members>, frames: u32, seed: &mut u32) -> Next {
        self.frames = frames;
        let won = winner == Some(0);
        match self.mode {
            ARCADE | TIME_ATTACK | ONE_ARCADE if won && self.fight + 1 >= self.ladder.len() => {
                // (by the game's costume numbers)
                let members: Vec<(usize, usize)> =
                    (self.teams[0].iter()).filter_map(|p| roster.characters.get(p.0).map(|c| (c.id, c.costumes.get(p.1).and_then(|r| roster.costume_of.get(r)).copied().unwrap_or(0)))).collect();
                let ending = d.endings.as_ref().and_then(|e| e.pick(&members, progress.seen));
                let up = progress.played(d, ending.map(|e| e.0));
                let mut lines = vec!["CONGRATULATIONS!".to_string()];
                if self.mode == TIME_ATTACK {
                    lines.push(clock(self.frames));
                }
                if up {
                    lines.push(unlocked(d, roster, progress.stage));
                }
                let mut page = Page::Message { lines, frame: 0, title: true };
                let record = Record { score: self.frames, members: self.members(roster), name: String::new() };
                if self.mode == TIME_ATTACK && progress.rank(d, false, &record).is_some() {
                    page = Page::NameEntry { survival: false, record, letter: 0, then: Box::new(page) };
                }
                match ending {
                    Some((e, costume)) => Next::Ending(e, costume, page),
                    None => Next::Screen(page),
                }
            }
            ARCADE | TIME_ATTACK | ONE_ARCADE if won => {
                progress.played(d, None);
                self.fight += 1;
                Next::Fight(Box::new(self))
            }
            _ if self.continues() => {
                progress.played(d, None);
                Next::Screen(Page::Continue { run: Box::new(self), countdown: CONTINUE })
            }
            SURVIVAL if won => {
                // each of the player's members 15 % of its full health back, to its health and its recoverable cap
                // (0x3C1620, mode 4); the first member leads again
                self.wins[0] += 1;
                let refill = d.raw["team_battle"]["refill_percent"].as_i64().unwrap_or(15) as i32;
                let up = |(h, cap, m): (i32, i32, i32)| ((h + m / 100 * refill).min(m), (cap + m / 100 * refill).min(m));
                match tag {
                    Some([t, _]) => {
                        let carry = t.map(up);
                        (self.health[0], self.carry[0]) = (Some(carry[0].0), Some(carry));
                    }
                    None => self.health[0] = Some(up((health[0], 0, max[0])).0),
                }
                self.teams[1] = cpu_team(d, roster, progress.stage, &self.teams[0], 1 + usize::from(tag.is_some()), false, seed);
                self.stage = pick(&d.pool(progress), seed).unwrap_or(self.stage);
                Next::Fight(Box::new(self))
            }
            SURVIVAL => {
                progress.played(d, None);
                let page = Page::Message { lines: vec!["SURVIVAL".into(), format!("WINS {}", self.wins[0])], frame: 0, title: false };
                let record = Record { score: self.wins[0], members: self.members(roster), name: String::new() };
                if progress.rank(d, true, &record).is_some() {
                    return Next::Screen(Page::NameEntry { survival: true, record, letter: 0, then: Box::new(page) });
                }
                Next::Screen(page)
            }
            TEAM => {
                // the loser's next member comes in; the winner stays, 15 % of its full health back (0x3C1620)
                let Some(w) = winner else { return Next::Fight(Box::new(self)) };
                let refill = d.raw["team_battle"]["refill_percent"].as_i64().unwrap_or(15) as i32;
                self.health[w] = Some((health[w] + max[w] / 100 * refill).min(max[w]));
                self.health[1 - w] = None;
                self.member[1 - w] += 1;
                if self.member[1 - w] < self.teams[1 - w].len() {
                    return Next::Fight(Box::new(self));
                }
                progress.played(d, None);
                let text = if self.humans[1] {
                    format!("{}P WINS", w + 1)
                } else if w == 0 {
                    "YOU WIN".into()
                } else {
                    "YOU LOSE".into()
                };
                Next::Screen(Page::Message { lines: vec!["TEAM BATTLE".into(), text], frame: 0, title: false })
            }
            // VS BATTLE, 1 ON 1 VS: the tally, both pick again
            _ => {
                progress.played(d, None);
                if let Some(w) = winner {
                    self.wins[w] += 1;
                }
                Next::Screen(Page::Select(Box::new(Select::new(self.mode, d, Some(self.wins)))))
            }
        }
    }
}

/// What follows a match.
pub enum Next {
    Fight(Box<Run>),
    Screen(Page),
    /// The arcade cleared: the staff roll and the ending (its number, the player's costume), then the page.
    Ending(usize, usize, Page),
}

/// The next unlock's news.
fn unlocked(d: &Data, roster: &Roster, stage: u32) -> String {
    let names: Vec<String> = d.schedule.iter().filter(|e| e.0 == stage && e.1 == 1).filter_map(|e| roster.by_id(e.2)).map(|c| c.name.to_uppercase()).collect();
    if names.is_empty() {
        "NEW FEATURE AVAILABLE".into()
    } else {
        format!("{} AVAILABLE", names.join(", "))
    }
}

/// Time Attack's clock `mm'ss"cc` (0x51ED40).
fn clock(f: u32) -> String {
    format!("{:02}'{:02}\"{:02}", f / 3600, f / 60 % 60, f % 60 * 100 / 60)
}

/// A CPU team of `n` from the CPU pool, never the players' characters; costumes rand(5) (FRONTEND.md 5); `early`:
/// fights 1-3 (no Ogre, True Ogre, Devil, Mokujin).
fn cpu_team(d: &Data, roster: &Roster, stage: u32, players: &Team, n: usize, early: bool, seed: &mut u32) -> Team {
    let out_early = ints(&d.raw["arcade"]["early_excluded"]);
    let mine: Vec<usize> = players.iter().map(|p| p.0).collect();
    let mut pool: Vec<usize> = (0..roster.characters.len())
        .filter(|&i| {
            let id = roster.characters[i].id;
            d.in_pool(stage, id) && !mine.contains(&i) && !(early && out_early.contains(&id)) && id != int(&d.raw["arcade"]["boss"])
        })
        .collect();
    let mut team = vec![];
    while team.len() < n && !pool.is_empty() {
        let i = pool.remove(lcg(seed, pool.len()));
        team.push((i, lcg(seed, roster.characters[i].costumes.len().min(5))));
    }
    team
}

/// The arcade ladder (0x3C02D8): fights 1-6 CPU teams from the pool, one of them a real pair (0x5207F0), fight 7 the
/// rival team in Ogre's arena, fight 8 Unknown alone in hers; arenas 1-6 drawn without repeats.
fn ladder(d: &Data, roster: &Roster, stage: u32, pool: &[usize], players: &Team, tag: bool, seed: &mut u32) -> Vec<(Team, usize)> {
    let a = &d.raw["arcade"];
    let n = if tag { 2 } else { 1 };
    let fights = int(&a["fights"]).max(2);
    let mut arenas: Vec<usize> = pool.iter().copied().filter(|&x| x != int(&a["rival_arena"]) && x != int(&a["boss_arena"])).collect();
    let mut out = vec![];
    let paired = lcg(seed, fights - 2);
    let costume = |i: usize, seed: &mut u32| lcg(seed, roster.characters[i].costumes.len().min(5));
    let mine: Vec<usize> = players.iter().filter_map(|p| roster.characters.get(p.0)).map(|c| c.id).collect();
    for f in 0..fights - 2 {
        let mut team = cpu_team(d, roster, stage, players, n, f < int(&a["early_fights"]), seed);
        if tag && f == paired {
            let ok = |c: usize| d.in_pool(stage, c) && !mine.contains(&c);
            let fit: Vec<&[usize; 2]> = d.pairs.iter().filter(|p| ok(p[0]) && ok(p[1]) && p[0] != p[1]).collect();
            if let Some(p) = fit.get(lcg(seed, fit.len())) {
                if let (Some(x), Some(y)) = (roster.index_of(p[0]), roster.index_of(p[1])) {
                    team = vec![(x, costume(x, seed)), (y, costume(y, seed))];
                }
            }
        }
        let arena = if arenas.is_empty() { 0 } else { arenas.remove(lcg(seed, arenas.len())) };
        out.push((team, arena));
    }
    // the rival team: the first free of the front character's three, then the fallback
    let front = mine.first().copied().unwrap_or(0);
    let candidates = d.rivals.get(front).cloned().unwrap_or_default().into_iter().flatten().chain(d.fallback.iter().copied());
    let mut rival = vec![];
    for c in candidates {
        if rival.len() < n && !mine.contains(&c) && !rival.iter().any(|r: &(usize, usize)| roster.characters[r.0].id == c) {
            if let Some(i) = roster.index_of(c) {
                rival.push((i, costume(i, seed)));
            }
        }
    }
    out.push((rival, int(&a["rival_arena"])));
    let boss = roster.index_of(int(&a["boss"])).unwrap_or(0);
    out.push((vec![(boss, lcg(seed, 2).min(roster.characters.get(boss).map_or(1, |c| c.costumes.len()) - 1))], int(&a["boss_arena"])));
    out
}

/// The ranking after GAME OVER (screen 7) without an entry, at `frame` of it (0x3C8A88's steps 4-10, `ranking.pace`):
/// in from black, the last page's rows (`per` a page, `dy` apart) down from over their places `scroll` a frame, each
/// page held `hold` frames, then the page before, the first held `after` more, out to black. The rows' top (the first
/// row's foot) and the black's level (1 opaque); None once it is over.
fn ranking_pace(d: &Data, kind: usize, n: usize, frame: u32) -> Option<(f32, f32)> {
    let (rk, r) = (&d.raw["ranking"], &d.raw["ranking"]["rows"][kind]);
    let (pace, base, dy, per) = (&rk["pace"], int(&r["first"]) as i32, int(&r["dy"]) as i32, int(&r["shown"]).max(1));
    let (fade, scroll, hold, after) = (int(&pace["fade"]).max(1) as i32, int(&pace["scroll"]).max(1) as i32, int(&r["hold"]) as u32, int(&pace["after"]) as u32);
    let steps = (256 + fade - 1) / fade;
    let black = |k: i32| 1.0 - (k * fade).min(256) as f32 / 256.0;
    let mut f = frame as i32;
    // (in from black, then a frame setting up)
    if f < steps {
        return Some(((base - n as i32 * dy) as f32, black(f + 1)));
    }
    f -= steps + 1;
    let mut first = n.saturating_sub(if n.is_multiple_of(per) { per } else { n % per }) as i32;
    let mut top = base - n as i32 * dy;
    loop {
        // (down to the page's place, then held)
        let target = base - first * dy;
        let travel = ((target - top) + scroll - 1).max(0) / scroll;
        if f < travel {
            return Some(((top + scroll * (f + 1)).min(target) as f32, 0.0));
        }
        f -= travel;
        top = target;
        if f < hold as i32 {
            return Some((top as f32, 0.0));
        }
        f -= hold as i32;
        if first == 0 {
            break;
        }
        first = (first - per as i32).max(0);
    }
    // (the first page `after` more frames, a frame setting up, out to black)
    let out = f - after as i32 - 1;
    (out < steps).then(|| (top as f32, if out < 0 { 0.0 } else { 1.0 - black(out + 1) }))
}

/// CONTINUE?'s frames (0x3BA560: 9 down to 0, 100 frames a digit).
const CONTINUE: u32 = 898;

/// A screen of the front end.
#[derive(Clone)]
pub enum Page {
    Title,
    /// The main menu: the cursor, the rows' scroll (0x3BD2E0: a row's height towards 0), the frames since a key.
    Menu {
        cursor: usize,
        scroll: i32,
        idle: u32,
    },
    /// OPTIONS (screen 9): its entries' cursor.
    Options {
        cursor: usize,
    },
    /// GAME OPTIONS: the cursor (on `game_rows`), every option's value, OpenKumite's ARCADE ARENAS on, DEFAULT's YES /
    /// NO open (YES chosen).
    GameOptions {
        cursor: usize,
        values: Vec<usize>,
        arcade: bool,
        reset: Option<bool>,
    },
    Select(Box<Select>),
    /// The VS modes' arena (OpenKumite's): RANDOM, Tekken Tag's, the arcade pack's.
    Stage {
        run: Box<Run>,
        cursor: usize,
    },
    /// Tekken Ball's ball type (Tekken 3 picks it on its select, 0x800B5478; OpenKumite's list here).
    BallType {
        run: Box<Run>,
        cursor: usize,
    },
    /// The arcade ladder lost: CONTINUE? 9..0, then GAME OVER, over the fight's scene and HUD.
    Continue {
        run: Box<Run>,
        countdown: u32,
    },
    /// GAME OVER (0x35B0E4) over the fight's scene, then the CHARACTER USAGE ranking (screen 7), then the title.
    GameOver {
        frame: u32,
    },
    /// A run's end (or GAME OVER): its lines, then the title or the main menu.
    Message {
        lines: Vec<String>,
        frame: u32,
        title: bool,
    },
    /// RECORDS (0x3B2978): the page (CHARACTER USAGE, GREATEST SURVIVORS, TIME ATTACK), the first row shown;
    /// `attract`: after GAME OVER, CHARACTER USAGE for a while, then the title.
    Records {
        page: usize,
        scroll: usize,
        attract: bool,
    },
    /// A new record's name entry: the record (its name so far), the letter under the cursor of `records.letters`,
    /// then `then`.
    NameEntry {
        survival: bool,
        record: Record,
        letter: usize,
        then: Box<Page>,
    },
    /// THEATER (screen 0xC): its rows and cells, the cursor's [row, cell].
    Theater {
        cursor: [usize; 2],
    },
    /// The music player (screen 0xE): the cursor, the BGM chosen.
    Music {
        cursor: usize,
        playing: Option<usize>,
    },
}

pub struct Front {
    pub page: Page,
    pub frame: u32,
}

/// One side of the select screen.
#[derive(Clone, Debug, Default)]
pub struct Side {
    /// Who picks for it (None: nobody).
    pub player: Option<usize>,
    pub cursor: usize,
    pub picks: Team,
    /// How many it picks (Team Battle: up to 8, START ends).
    pub need: usize,
    pub done: bool,
    /// The select's frame of its last pick.
    pub since: u32,
}

#[derive(Clone)]
pub struct Select {
    pub mode: u32,
    pub sides: [Side; 2],
    pub frame: u32,
    pub done: u32,
    /// VS: the tally so far.
    pub tally: Option<[u32; 2]>,
    /// CONTINUE with CHARACTER CHANGE AT CONTINUE: the run the picks go on with.
    pub cont: Option<Box<Run>>,
    /// HOLD's memory ([`Progress::cursors`]) as the picks change it.
    memory: [[Option<usize>; 2]; 2],
}

impl Select {
    pub fn new(mode: u32, d: &Data, tally: Option<[u32; 2]>) -> Self {
        // (Tekken Bowl: a team too, its second the strike's partner and in the records, 0x3E99C0, 0x3F1408)
        let need = match mode {
            TEAM => 8,
            // (Tekken Ball: a pair, its second the partner in 2 ON 2)
            m if tag(m) || m == BOWL || m == BALL => 2,
            _ => 1,
        };
        // (the cursors start on the first pick's slot HOLD kept (0x3C993C), at first `start_slots`: 1P's on Xiaoyu's, as
        // the PS2 shows)
        let memory = progress().cursors;
        let first = |k: usize| memory[k][0].unwrap_or(int(&d.raw["select"]["start_slots"][k])).min(d.slots.len().saturating_sub(1));
        let p2 = matches!(mode, VS | ONE_VS);
        let side = |player: Option<usize>, k: usize| Side { player, cursor: first(k), picks: vec![], need, done: false, since: 0 };
        let mut sides = [side(Some(0), 0), side(p2.then_some(1), 1)];
        // practice: P1 picks the dummy's team too
        if mode == PRACTICE {
            sides[1] = side(Some(0), 1);
        }
        Self { mode, sides, frame: 0, done: 0, tally, cont: None, memory }
    }

    /// The side `player`'s keys move now.
    fn side_of(&self, player: usize) -> Option<usize> {
        (0..2).find(|&s| self.sides[s].player == Some(player) && !self.sides[s].done)
    }

    /// The character RANDOM shows and picks for `side` now (0x3C99C8: the next selectable character every `random_roll`
    /// frames, in id order, not one the side picked).
    fn roll(&self, d: &Data, roster: &Roster, progress: &Progress, side: usize) -> Option<usize> {
        let taken: Vec<usize> = self.sides.get(side).map_or(vec![], |s| s.picks.iter().map(|p| p.0).collect());
        let mut free: Vec<usize> = roster.characters.iter().map(|c| c.id).filter(|&c| d.selectable(progress.stage, c) && roster.index_of(c).is_some_and(|i| !taken.contains(&i))).collect();
        free.sort_unstable();
        let at = self.frame as usize / int(&d.raw["select"]["random_roll"]).max(1);
        free.get(at % free.len().max(1)).copied()
    }

    /// Side `k`'s characters shown: its picks (roster index, costume), then the one under its cursor (costume None).
    fn shown(&self, d: &Data, roster: &Roster, p: &Progress, k: usize) -> Vec<(usize, Option<usize>)> {
        let Some(side) = self.sides.get(k) else { return vec![] };
        let hover = (!side.done)
            .then(|| d.slots.get(side.cursor))
            .flatten()
            .and_then(|sl| match sl.character {
                Some(c) => Some(c).filter(|&c| d.selectable(p.stage, c)),
                None => self.roll(d, roster, p, k),
            })
            .and_then(|c| roster.index_of(c))
            .filter(|i| !side.picks.iter().any(|q| q.0 == *i));
        side.picks.iter().map(|q| (q.0, Some(q.1))).chain(hover.map(|i| (i, None))).collect()
    }

    /// The 3D characters standing now (FRONTEND.md 9): per side its last two of [`Select::shown`], as (slot = side + 2
    /// * member, model record, picked).
    pub fn posers(&self, d: &Data, roster: &Roster, p: &Progress) -> Vec<(usize, usize, bool)> {
        let mut out = vec![];
        for k in (0..2).filter(|&k| self.sides[k].player.is_some()) {
            let list = self.shown(d, roster, p, k);
            for (j, &(i, costume)) in list.iter().skip(list.len().saturating_sub(2)).enumerate() {
                if let Some(&record) = roster.characters.get(i).and_then(|c| c.costumes.get(costume.unwrap_or(0)).or(c.costumes.first())) {
                    out.push((k + 2 * j, record, costume.is_some()));
                }
            }
        }
        out
    }

    fn key(&mut self, player: usize, key: Key, d: &Data, roster: &Roster, progress: &Progress) -> Option<Page> {
        // Team Battle, Tekken Bowl, Tekken Ball: 2P's START joins
        if matches!(self.mode, TEAM | BOWL | BALL) && player == 1 && key == Key::Start && self.sides[1].player.is_none() {
            sfx::play(sound(d, "start"));
            self.sides[1].player = Some(1);
            return None;
        }
        let Some(s) = self.side_of(player) else {
            // a side that chose: back takes its last pick again
            if key == Key::Back {
                if let Some(s) = (0..2).rev().find(|&s| self.sides[s].player == Some(player) && self.sides[s].done) {
                    self.sides[s].done = false;
                    self.sides[s].picks.pop();
                    self.done = 0;
                }
            }
            return None;
        };
        let side = &mut self.sides[s];
        let visible = |slot: usize| d.slots.get(slot).is_some_and(|sl| !hidden(d, roster, progress, slot) && sl.character.is_none_or(|c| roster.by_id(c).is_some()));
        match key {
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let k = [Key::Up, Key::Down, Key::Left, Key::Right].iter().position(|&x| x == key).unwrap_or(0);
                // (a hidden slot is passed over the way it leads)
                let mut at = side.cursor;
                for _ in 0..4 {
                    match d.slots.get(at).and_then(|sl| sl.next[k]) {
                        Some(n) if visible(n) => {
                            at = n;
                            break;
                        }
                        Some(n) => at = n,
                        None => break,
                    }
                }
                if visible(at) && at != side.cursor {
                    sfx::play(sound(d, "cursor"));
                    side.cursor = at;
                }
            }
            Key::Start if self.mode == TEAM && !side.picks.is_empty() => {
                sfx::play(sound(d, "last_pick"));
                side.done = true;
            }
            Key::Button(_) | Key::Start => {
                // (the pad's raw bits as 0x3C9ED8 reads them: square, triangle, cross, circle, a shoulder, START)
                let button = match key {
                    Key::Button(b) => [0x80, 0x10, 0x40, 0x20, 0x04].get(b).copied()?,
                    _ => 0x800,
                };
                self.take(s, button, d, roster, progress)?;
            }
            Key::Back => {
                if side.picks.pop().is_none() && s == 0 {
                    sfx::play(sound(d, "cancel"));
                    return Some(Page::Menu { cursor: 0, scroll: 0, idle: 0 });
                }
                // (HOLD forgets the second pick's slot, 0x3C94C0)
                if progress.setting(d, CURSOR_ROW) == 1 {
                    self.memory[s][1] = None;
                }
            }
        }
        None
    }

    /// Side `s` takes the character under its cursor with the costume pad bits `button` give (0x3C9ED8): None when
    /// it cannot.
    fn take(&mut self, s: usize, button: u32, d: &Data, roster: &Roster, progress: &Progress) -> Option<()> {
        let side = self.sides.get(s)?;
        let slot = d.slots.get(side.cursor)?;
        let taken: Vec<usize> = side.picks.iter().map(|p| p.0).collect();
        let id = match slot.character {
            Some(c) => Some(c).filter(|&c| d.selectable(progress.stage, c)),
            None => self.roll(d, roster, progress, s),
        };
        let i = id.and_then(|c| roster.index_of(c)).filter(|i| !taken.contains(i))?;
        let c = roster.characters.get(i)?;
        let index = |n: usize| c.costumes.iter().position(|r| roster.costume_of.get(r) == Some(&n));
        // (a costume the character has; Armor King's fifth once his ending was seen: 0x3AE608, config +0x10 bit 18)
        let has = |n: usize| index(n).is_some() && !(n == 4 && c.id == ARMOR_KING && progress.seen >> 18 & 1 == 0);
        let or = |n: usize, other: usize| if has(n) { n } else { other };
        let mut costume = match button {
            0x80 => 0,
            0x40 => 1,
            0x10 => or(2, 0),
            0x20 => or(3, 1),
            0x04 => or(4, 0),
            0x800 if has(4) && self.mode != TEAM => 4,
            _ => return None,
        };
        // both sides played: one taking the other's character in its costume gets another (0x3CA2BC -> 0x3C0210)
        let other = &self.sides[1 - s.min(1)];
        if self.sides.iter().all(|x| x.player.is_some()) && other.picks.iter().any(|q| q.0 == i && c.costumes.get(q.1).and_then(|r| roster.costume_of.get(r)) == Some(&costume)) {
            costume = match costume {
                0 => or(2, 1),
                1 => or(3, 0),
                _ => 0,
            };
        }
        let frame = self.frame;
        let side = &mut self.sides[s];
        // HOLD keeps the pick's slot (0x3CAB18); after the first pick the cursor goes to the second's kept (0x3CAB8C)
        if progress.setting(d, CURSOR_ROW) == 1 {
            if let Some(m) = self.memory[s].get_mut(side.picks.len()) {
                *m = Some(side.cursor);
            }
        }
        side.picks.push((i, index(costume).unwrap_or(0)));
        side.done = side.picks.len() >= side.need;
        side.since = frame;
        if let (1, false, Some(slot)) = (side.picks.len(), side.done, self.memory[s][1]) {
            side.cursor = slot;
        }
        // (0x3CA3BC: select 1 for a pick before the last, select 2 for the last)
        sfx::play(sound(d, if side.done { "last_pick" } else { "pick" }));
        Some(())
    }

    fn tick(&mut self, d: &Data, roster: &Roster, progress: &Progress, seed: &mut u32) -> Option<Run> {
        self.frame += 1;
        // the timer out (`select.timer`, 0x3C9888): each side still picking takes what its cursor is on, else RANDOM's
        // character (shortcut: what the original does then is not decoded), in a costume of a button drawn at random
        // (0x3CA070: rand() % 5 of square, triangle, a tag button, cross, circle); all its picks on that frame, as the
        // PS2 does
        if d.raw["select"]["timer"]["frames"].as_u64().is_some_and(|t| u64::from(self.frame) >= t) {
            let random = d.slots.iter().position(|s| s.character.is_none()).unwrap_or(0);
            for k in 0..2 {
                while self.sides[k].player.is_some() && !self.sides[k].done {
                    let button = pick(&[0x80, 0x10, 0x04, 0x40, 0x20], seed).unwrap_or(0x80) as u32;
                    if self.take(k, button, d, roster, progress).is_none() {
                        self.sides[k].cursor = random;
                        if self.take(k, button, d, roster, progress).is_none() {
                            break;
                        }
                    }
                    self.sides[k].done |= self.mode == TEAM;
                }
            }
        }
        let ready = self.sides.iter().all(|s| s.player.is_none() || s.done);
        if !ready {
            self.done = 0;
            return None;
        }
        self.done += 1;
        if self.done < int(&d.raw["select"]["done"]).max(1) as u32 {
            return None;
        }
        if let Some(mut run) = self.cont.take() {
            run.teams[0] = self.sides[0].picks.clone();
            return Some(*run);
        }
        let teams = [self.sides[0].picks.clone(), if self.sides[1].player.is_some() { self.sides[1].picks.clone() } else { vec![] }];
        let mut run = Run::new(d, roster, progress, self.mode, teams, seed);
        if let Some(t) = self.tally {
            run.wins = t;
        }
        Some(run)
    }
}

/// A grid slot not shown: Mokujin's and Unknown's (FRONTEND.md 3), until their character can be picked.
// shortcut: the original shows them by settings +8 (QUICK SELECT, [M]); here once unlocked
fn hidden(d: &Data, roster: &Roster, p: &Progress, slot: usize) -> bool {
    ints(&d.raw["select"]["hidden_slots"]).contains(&slot) && d.slots.get(slot).and_then(|s| s.character).is_none_or(|c| !d.selectable(p.stage, c) || roster.by_id(c).is_none())
}

impl Front {
    pub fn title() -> Self {
        Self { page: Page::Title, frame: 0 }
    }

    /// Practice's select (its menu's CHARACTER SELECT).
    pub fn practice_select(d: &Data) -> Self {
        Self { page: Page::Select(Box::new(Select::new(PRACTICE, d, None))), frame: 0 }
    }

    pub fn menu() -> Self {
        Self { page: Page::Menu { cursor: 0, scroll: 0, idle: 0 }, frame: 0 }
    }

    /// OPTIONS on CONTROLLER SETUP (`controller`) or ADJUST DISPLAY: back from OpenKumite's screens standing for them.
    pub fn options(controller: bool) -> Self {
        Self { page: Page::Options { cursor: if controller { OPT_CONTROLLER } else { OPT_DISPLAY } }, frame: 0 }
    }

    /// The main menu with its cursor on `mode`'s entry.
    fn menu_at(d: &Data, p: &Progress, mode: u32) -> Page {
        Page::Menu { cursor: Self::items(d, p).iter().position(|i| i.0 == mode).unwrap_or(0), scroll: 0, idle: 0 }
    }

    /// The main menu's entries: of the locked ones, THEATER once an ending was seen (0x3BFED8), TEKKEN BOWL once the
    /// unlock stage opens it (Gallery is not here yet); OpenKumite's TEKKEN BALL and TEKKEN FORCE before OPTIONS with the
    /// Tekken 3 slice.
    fn items(d: &Data, p: &Progress) -> Vec<(u32, String)> {
        let open = |i: &&Value| {
            let mode = int(&i["mode"]) as u32;
            i["locked"] != true || (mode == THEATER && p.seen != 0 && d.endings.is_some()) || (mode == BOWL && d.bowl_open(p.stage))
        };
        let mut items: Vec<(u32, String)> =
            d.raw["menu"]["items"].as_array().into_iter().flatten().filter(open).map(|i| (int(&i["mode"]) as u32, i["label"].as_str().unwrap_or_default().to_string())).collect();
        if d.ball.is_some() {
            items.insert(items.iter().position(|i| i.0 == OPTIONS).unwrap_or(items.len()), (BALL, "TEKKEN BALL".into()));
        }
        if d.force {
            items.insert(items.iter().position(|i| i.0 == OPTIONS).unwrap_or(items.len()), (FORCE, "TEKKEN FORCE".into()));
        }
        if cfg!(not(target_arch = "wasm32")) {
            items.push((u32::MAX, "EXIT".into()));
        }
        items
    }

    pub fn key(&mut self, player: usize, key: Key, roster: &Roster, seed: &mut u32) -> Outcome {
        let mut progress = progress();
        let progress = &mut progress;
        let Some(d) = roster.ttt.clone() else { return Outcome::None };
        let next = match &mut self.page {
            Page::Title => match key {
                Key::Start => {
                    sfx::play(sound(&d, "start"));
                    Some(Page::Menu { cursor: 0, scroll: 0, idle: 0 })
                }
                Key::Back => return Outcome::Quit,
                _ => None,
            },
            Page::Menu { cursor, scroll, idle } => {
                let items = Self::items(&d, progress);
                *idle = 0;
                match key {
                    // (0x3BD2E0: a move once the rows stand still; they slide a row's height into place)
                    Key::Up | Key::Down if *scroll == 0 => {
                        sfx::play(sound(&d, "cursor"));
                        *cursor = (*cursor + if key == Key::Up { items.len() - 1 } else { 1 }) % items.len().max(1);
                        *scroll = int(&d.raw["menu"]["row_h"]) as i32 * if key == Key::Up { -1 } else { 1 };
                        None
                    }
                    Key::Start | Key::Button(_) => {
                        sfx::play(sound(&d, "decide"));
                        // (a newly opened entry no longer blinks once entered: config +0x24, 0x3BFE78)
                        if let Some(&(m, _)) = items.get(*cursor).filter(|i| i.0 < 32) {
                            progress.entered |= 1 << m;
                            set_progress(progress.clone());
                        }
                        match items.get(*cursor).map(|i| i.0) {
                            Some(u32::MAX) => return Outcome::Exit,
                            Some(OPTIONS) => Some(Page::Options { cursor: 0 }),
                            Some(THEATER) => Some(Page::Theater { cursor: [0, 0] }),
                            Some(m) => Some(Page::Select(Box::new(Select::new(m, &d, None)))),
                            None => None,
                        }
                    }
                    Key::Back => Some(Page::Title),
                    _ => None,
                }
            }
            // (0x3B19B0: the entries round, sound 3; a choice sound 5; back to the main menu)
            Page::Options { cursor } => {
                let n = d.raw["options_menu"]["items"].as_array().map_or(0, Vec::len).max(1);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sound(&d, "cursor"));
                        *cursor = (*cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
                        None
                    }
                    Key::Start | Key::Button(_) => match *cursor {
                        OPT_GAME => {
                            sfx::play(sound(&d, "decide"));
                            let values = (0..option_count(&d)).map(|r| progress.setting(&d, r)).collect();
                            Some(Page::GameOptions { cursor: 0, values, arcade: !progress.arcade_off, reset: None })
                        }
                        // (OpenKumite's KEY CONFIGURATION and GRAPHICS stand for the PS2's pages)
                        OPT_CONTROLLER => {
                            sfx::play(sound(&d, "decide"));
                            return Outcome::OpenKeyConfig(crate::menu::difficulty());
                        }
                        OPT_DISPLAY => {
                            sfx::play(sound(&d, "decide"));
                            return Outcome::OpenGraphics(crate::menu::difficulty());
                        }
                        OPT_RECORDS => {
                            sfx::play(sound(&d, "decide"));
                            Some(Page::Records { page: 0, scroll: 0, attract: false })
                        }
                        OPT_MEMORY_CARD => None,
                        c if c + 1 < n => None,
                        _ => Some(Self::menu_at(&d, progress, OPTIONS)),
                    },
                    Key::Back => {
                        sfx::play(sound(&d, "cancel"));
                        Some(Self::menu_at(&d, progress, OPTIONS))
                    }
                    _ => None,
                }
            }
            Page::GameOptions { cursor, values, arcade, reset } => {
                let rows = game_rows(&d);
                let row = rows.get(*cursor).copied().unwrap_or(ARCADE_ROW);
                let o = &d.raw["options"][row];
                let kind = int(&o["kind"]);
                match (reset.as_mut(), key) {
                    // DEFAULT's YES / NO
                    (Some(yes), Key::Left | Key::Right) => {
                        sfx::play(sound(&d, "cursor"));
                        *yes = !*yes;
                        None
                    }
                    (Some(yes), Key::Start | Key::Button(_) | Key::Back) => {
                        if *yes && key != Key::Back {
                            sfx::play(sound(&d, "decide"));
                            *values = (0..values.len()).map(|r| d.option(r).0).collect();
                            *arcade = true;
                        } else {
                            sfx::play(sound(&d, "cancel"));
                        }
                        *reset = None;
                        None
                    }
                    (None, Key::Up | Key::Down) => {
                        sfx::play(sound(&d, "cursor"));
                        let n = rows.len().max(1);
                        *cursor = (*cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
                        None
                    }
                    (None, Key::Left | Key::Right) if row == ARCADE_ROW => {
                        sfx::play(sound(&d, "cursor"));
                        *arcade = !*arcade;
                        None
                    }
                    // (a list goes round; a number 0-10 stops at its ends)
                    (None, Key::Left | Key::Right) if row < values.len() && kind != 2 => {
                        let v = &mut values[row];
                        let n = d.option(row).1.len();
                        let next = match (kind, key) {
                            (1, Key::Left) => v.saturating_sub(1),
                            (1, _) => (*v + 1).min(10),
                            (_, Key::Left) => (*v + n.max(1) - 1) % n.max(1),
                            _ => (*v + 1) % n.max(1),
                        };
                        if next != *v {
                            sfx::play(sound(&d, "cursor"));
                            *v = next;
                        }
                        None
                    }
                    (None, Key::Start | Key::Button(_)) if kind == 2 && *cursor + 1 < rows.len() => {
                        sfx::play(sound(&d, "cursor"));
                        *reset = Some(false);
                        None
                    }
                    // EXIT (or back): kept in the save
                    (None, Key::Start | Key::Button(_) | Key::Back) if key == Key::Back || kind == 2 => {
                        sfx::play(sound(&d, "cancel"));
                        progress.options = Some(OPTION_ROWS.map(|r| values.get(r).copied().unwrap_or(0)));
                        progress.settings = values.iter().skip(OPTION_ROWS.len()).copied().collect();
                        progress.arcade_off = !*arcade;
                        set_progress(progress.clone());
                        self.page = Page::Options { cursor: OPT_GAME };
                        return Outcome::TttSaved;
                    }
                    _ => None,
                }
            }
            Page::Select(s) => s.key(player, key, &d, roster, progress),
            Page::Stage { run, cursor } => {
                let n = stage_list(&d).len().max(1);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sound(&d, "cursor"));
                        *cursor = (*cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
                        None
                    }
                    Key::Start | Key::Button(_) => {
                        sfx::play(sound(&d, "decide"));
                        let mut run = run.clone();
                        // RANDOM: the arenas a fight is drawn on
                        run.stage = stage_list(&d).get(*cursor).and_then(|a| a.0).or_else(|| pick(&d.pool(progress), seed)).unwrap_or(run.stage);
                        return Outcome::TttFight(run);
                    }
                    Key::Back => {
                        sfx::play(sound(&d, "cancel"));
                        Some(Page::Select(Box::new(Select::new(run.mode, &d, matches!(run.mode, VS | ONE_VS).then_some(run.wins).filter(|w| *w != [0; 2])))))
                    }
                    _ => None,
                }
            }
            Page::BallType { run, cursor } => {
                let n = roster.ball_types();
                match key {
                    Key::Left | Key::Right => {
                        sfx::play(sound(&d, "cursor"));
                        run.doubles = !run.doubles;
                        None
                    }
                    Key::Up | Key::Down => {
                        sfx::play(sound(&d, "cursor"));
                        *cursor = (*cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
                        None
                    }
                    Key::Start | Key::Button(_) => {
                        sfx::play(sound(&d, "decide"));
                        return Outcome::TttFight(Box::new(Run { ball: *cursor, ..*run.clone() }));
                    }
                    Key::Back => {
                        sfx::play(sound(&d, "cancel"));
                        Some(Page::Select(Box::new(Select::new(run.mode, &d, None))))
                    }
                }
            }
            Page::Continue { run, countdown } => match key {
                // START continues the same fight, through the select with CHARACTER CHANGE AT CONTINUE (row 3) YES
                // (0x3BA560); a face button skips 100 frames
                Key::Start => {
                    sfx::play(sound(&d, "start"));
                    if progress.setting(&d, CHANGE_ROW) == 1 {
                        Some(Page::Select(Box::new(Select { cont: Some(run.clone()), ..Select::new(run.mode, &d, None) })))
                    } else {
                        return Outcome::TttFight(run.clone());
                    }
                }
                Key::Button(_) => {
                    *countdown = countdown.saturating_sub(100);
                    None
                }
                _ => None,
            },
            Page::Message { title, .. } => match key {
                Key::Start | Key::Button(_) => Some(if *title { Page::Title } else { Page::Menu { cursor: 0, scroll: 0, idle: 0 } }),
                _ => None,
            },
            Page::GameOver { .. } => None,
            // (left / right the page, up / down the rows; back to OPTIONS, or after GAME OVER to the title)
            Page::Records { page, scroll, attract } => match key {
                _ if *attract => matches!(key, Key::Start | Key::Button(_)).then_some(Page::Title),
                Key::Left | Key::Right => {
                    sfx::play(sound(&d, "cursor"));
                    *page = (*page + if key == Key::Left { 2 } else { 1 }) % 3;
                    *scroll = 0;
                    None
                }
                Key::Up | Key::Down => {
                    let max = record_rows(&d, progress, *page).len().saturating_sub(RECORD_ROWS);
                    let next = if key == Key::Up { scroll.saturating_sub(1) } else { (*scroll + 1).min(max) };
                    if next != *scroll {
                        sfx::play(sound(&d, "cursor"));
                        *scroll = next;
                    }
                    None
                }
                Key::Start | Key::Button(_) | Key::Back => {
                    sfx::play(sound(&d, "cancel"));
                    Some(Page::Options { cursor: OPT_RECORDS })
                }
            },
            // (0x3D0350: right / down the next letter, left / up the one before, round, sound 3; a face button takes
            // the one under the cursor, "<" takes the last back, "=" or the third letter ends, sound 4)
            Page::NameEntry { survival, record, letter, then } => {
                let letters: Vec<char> = d.raw["records"]["letters"].as_str().unwrap_or_default().chars().collect();
                let n = letters.len().max(1);
                let mut done = false;
                match (key, letters.get(*letter)) {
                    (Key::Left | Key::Right | Key::Up | Key::Down, _) => {
                        sfx::play(sound(&d, "cursor"));
                        *letter = (*letter + if matches!(key, Key::Left | Key::Up) { n - 1 } else { 1 }) % n;
                    }
                    (Key::Button(_), Some('<')) => {
                        sfx::play(sound(&d, "pick"));
                        record.name.pop();
                    }
                    (Key::Button(_), Some('=')) => done = true,
                    (Key::Button(_), Some(&c)) => {
                        record.name.push(c);
                        done = record.name.chars().count() >= int(&d.raw["records"]["name_len"]).max(1);
                        if !done {
                            sfx::play(sound(&d, "pick"));
                        }
                    }
                    _ => {}
                }
                if !done {
                    return Outcome::None;
                }
                sfx::play(sound(&d, "pick"));
                progress.record(&d, *survival, record.clone());
                set_progress(progress.clone());
                self.page = std::mem::replace(then.as_mut(), Page::Title);
                self.frame = 0;
                return Outcome::TttSaved;
            }
            Page::Theater { cursor } => {
                let rows = theater(&d, progress.seen);
                let [r, c] = cursor;
                let len = |r: usize| rows.get(r).map_or(1, Vec::len).max(1);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sound(&d, "cursor"));
                        *r = (*r + if key == Key::Up { rows.len() - 1 } else { 1 }) % rows.len();
                        *c = (*c).min(len(*r) - 1);
                        None
                    }
                    Key::Left | Key::Right => {
                        sfx::play(sound(&d, "cursor"));
                        *c = (*c + if key == Key::Left { len(*r) - 1 } else { 1 }) % len(*r);
                        None
                    }
                    Key::Start | Key::Button(_) => match rows.get(*r).and_then(|row| row.get(*c)).map(|cell| cell.1) {
                        Some(Cell::Ending(e, costume)) => {
                            sfx::play(sound(&d, "decide"));
                            return Outcome::TttEnding(e, costume, [*r, *c]);
                        }
                        Some(Cell::Movie(m)) => {
                            sfx::play(sound(&d, "decide"));
                            return Outcome::TttMovie(m, Box::new(Page::Theater { cursor: [*r, *c] }));
                        }
                        Some(Cell::Embu(cast)) => {
                            sfx::play(sound(&d, "decide"));
                            return Outcome::TttEmbu(cast, [*r, *c]);
                        }
                        Some(Cell::Music) => {
                            sfx::play(sound(&d, "decide"));
                            Some(Page::Music { cursor: 0, playing: None })
                        }
                        Some(Cell::Exit) => Some(Page::Menu { cursor: Self::items(&d, progress).iter().position(|i| i.0 == THEATER).unwrap_or(0), scroll: 0, idle: 0 }),
                        _ => None,
                    },
                    Key::Back => Some(Page::Menu { cursor: 0, scroll: 0, idle: 0 }),
                }
            }
            Page::Music { cursor, playing } => {
                let list = music(&d);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sound(&d, "cursor"));
                        *cursor = (*cursor + if key == Key::Up { list.len() } else { 1 }) % (list.len() + 1);
                        None
                    }
                    Key::Start | Key::Button(_) if *cursor < list.len() => {
                        *playing = list.get(*cursor).map(|m| m.0);
                        None
                    }
                    Key::Start | Key::Button(_) | Key::Back => {
                        sfx::play(sound(&d, "cancel"));
                        let rows = theater(&d, progress.seen);
                        let at = rows.iter().position(|r| r.first().is_some_and(|c| matches!(c.1, Cell::Music))).unwrap_or(0);
                        Some(Page::Theater { cursor: [at, 0] })
                    }
                    _ => None,
                }
            }
        };
        if let Some(p) = next {
            self.page = p;
            self.frame = 0;
        }
        Outcome::None
    }

    pub fn tick(&mut self, roster: &Roster, seed: &mut u32) -> Outcome {
        let progress = &progress();
        self.frame += 1;
        let Some(d) = roster.ttt.clone() else { return Outcome::None };
        match &mut self.page {
            // the rows slide `scroll` a frame; `idle` frames without a key: back to the title (the PS2's attract)
            Page::Menu { scroll, idle, .. } => {
                let step = int(&d.raw["menu"]["scroll"]) as i32;
                *scroll = if scroll.abs() <= step { 0 } else { *scroll - step * scroll.signum() };
                *idle += 1;
                if *idle >= int(&d.raw["menu"]["idle"]).max(1) as u32 {
                    self.page = Page::Title;
                    self.frame = 0;
                }
            }
            Page::Select(s) => {
                if let Some(run) = s.tick(&d, roster, progress, seed) {
                    // CHARACTER USAGE: each of the players' picks
                    let mut p = progress.clone();
                    // (HOLD's memory into the save, 0x3CB6CC; 1 ON 1 keeps the first pick's only)
                    if p.setting(&d, CURSOR_ROW) == 1 {
                        for (k, m) in s.memory.iter().enumerate() {
                            p.cursors[k][0] = m[0];
                            if !matches!(s.mode, ONE_ARCADE | ONE_VS) {
                                p.cursors[k][1] = m[1];
                            }
                        }
                    }
                    for c in run.teams.iter().zip(run.humans).filter(|t| t.1).flat_map(|t| t.0).filter_map(|m| roster.characters.get(m.0)) {
                        if p.usage.len() <= c.id {
                            p.usage.resize(c.id + 1, 0);
                        }
                        p.usage[c.id] += 1;
                    }
                    set_progress(p);
                    if run.mode == FORCE {
                        return Outcome::StartForce(run.teams[0].first().copied().unwrap_or((0, 0)), run.options[0].min(2));
                    }
                    // VS BATTLE, 1 ON 1 VS, TEAM BATTLE: the arena chosen first (OpenKumite's stage select)
                    if matches!(run.mode, VS | ONE_VS | TEAM) {
                        self.page = Page::Stage { run: Box::new(run), cursor: 0 };
                        return Outcome::None;
                    }
                    if run.mode == BALL && roster.ball_types() > 1 {
                        self.page = Page::BallType { run: Box::new(run), cursor: 0 };
                        return Outcome::None;
                    }
                    return Outcome::TttFight(Box::new(run));
                }
            }
            Page::Continue { countdown, .. } => {
                *countdown = countdown.saturating_sub(1);
                // the announcer counts each digit below 9 as it comes (0x3BA6F4: sound 0x1047 + digit)
                if *countdown % 100 == 99 && *countdown < 900 {
                    sfx::play(0x1047 + (*countdown as u16 + 100) / 100);
                }
                if *countdown == 0 {
                    sfx::play(int(&d.raw["arcade"]["continue"]["game_over"]["voice"]) as u16);
                    self.page = Page::GameOver { frame: 0 };
                }
            }
            Page::GameOver { frame } => {
                *frame += 1;
                // then the CHARACTER USAGE ranking, as the PS2 shows it
                if *frame >= int(&d.raw["arcade"]["continue"]["game_over"]["frames"]) as u32 {
                    self.page = Page::Records { page: 0, scroll: 0, attract: true };
                    self.frame = 0;
                }
            }
            // (0x3C9124: the time to name it out, the name is taken as it is)
            Page::NameEntry { survival, record, then, .. } if self.frame >= int(&d.raw["ranking"]["frames"]).max(1) as u32 && d.raw["ranking"].is_object() => {
                let mut p = progress.clone();
                p.record(&d, *survival, record.clone());
                set_progress(p);
                self.page = std::mem::replace(then.as_mut(), Page::Title);
                self.frame = 0;
                return Outcome::TttSaved;
            }
            Page::Records { attract: true, .. } if ranking_pace(&d, 2, record_rows(&d, progress, 0).len(), self.frame).is_none() => {
                self.page = Page::Title;
                self.frame = 0;
            }
            Page::Message { frame, title, .. } => {
                *frame += 1;
                if *frame >= MESSAGE_FRAMES {
                    self.page = if *title { Page::Title } else { Page::Menu { cursor: 0, scroll: 0, idle: 0 } };
                }
            }
            // the attract (0x3BCC88): the logo's 300 frames out of white, then the opening movie (the first of Theater's);
            // shortcut: the PS2's demo fight and CHARACTER USAGE ranking between them are not done
            Page::Title if self.frame == num(&d.raw["title"]["from_white"]) as u32 + TITLE_FRAMES => {
                let opening = d.endings.as_ref().and_then(|e| e.raw["theater"]["extras"].as_array()?.iter().find_map(|x| x["movie"].as_u64()));
                if let Some(m) = opening {
                    return Outcome::TttMovie(m as usize, Box::new(Page::Title));
                }
            }
            _ => {}
        }
        Outcome::None
    }

    pub fn draw(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster) {
        let progress = &progress();
        let Some(d) = roster.ttt.as_deref() else { return };
        match &self.page {
            // (0x3BCC88: the logo out of white, then the line blinking by the frame's `blink_mask` bits, the namco logo
            // and the copyright)
            Page::Title => {
                let t = &d.raw["title"];
                image(painter, v, art, &d.picture(&["title", "picture"]), [0.0, 0.0, W, H], Color32::WHITE);
                let from_white = num(&t["from_white"]);
                if self.frame as f32 >= from_white {
                    // (the small font from `press_x` / `copyright_x`, the y its foot; an older pack's: centred)
                    let foot = num(&d.raw["fonts"]["small"]["glyph"][1]) + 2.0;
                    let line = |text: &str, x: &Value, y: f32, colour: &Value, c: Color32| match x.as_f64() {
                        Some(x) => small_in(painter, v, art, d, colour.as_u64().map(|c| c as usize), text, x as f32, y - foot, Color32::WHITE),
                        None => small(painter, v, art, d, text, W / 2.0, y - 6.0, c),
                    };
                    if self.frame & int(&t["blink_mask"]) as u32 != 0 {
                        line(&d.picture(&["title", "press_start"]), &t["press_x"], num(&t["press_y"]), &t["press_colour"], Color32::from_rgb(140, 240, 120));
                    }
                    sprite(painter, v, art, t["namco"]["picture"].as_str().unwrap_or_default(), num(&t["namco"]["x"]), num(&t["namco"]["y"]), Color32::WHITE);
                    for (k, text) in t["copyright"].as_array().into_iter().flatten().enumerate() {
                        line(text.as_str().unwrap_or_default(), &t["copyright_x"][k], num(&t["copyright_y"][k]), &t["copyright_colour"], Color32::from_gray(200));
                    }
                } else {
                    let a = (255.0 * (1.0 - self.frame as f32 / from_white)) as u8;
                    Art::shade(painter, v, sv([0.0, 0.0, W, H]), [Color32::from_rgba_unmultiplied(255, 255, 255, a); 4]);
                }
            }
            Page::Menu { cursor, scroll, .. } => draw_menu(painter, v, art, d, &Self::items(d, progress), *cursor, *scroll, self.frame, progress.entered),
            // (the logo dimmed: 0x3B4B20, 0x3C4868 a step a frame down to `dim`)
            Page::Options { cursor } => {
                let m = &d.raw["menu"];
                let level = (128.0 - num(&m["logo_step"]) * self.frame as f32).max(num(&m["dim"]));
                options_backdrop(painter, v, art, d, level);
                draw_options(painter, v, art, d, progress, *cursor);
            }
            Page::GameOptions { cursor, values, arcade, reset } => {
                options_backdrop(painter, v, art, d, num(&d.raw["menu"]["dim"]));
                draw_game_options(painter, v, art, d, *cursor, values, *arcade, *reset);
            }
            Page::Stage { cursor, .. } => {
                backdrop(painter, v, art);
                font(painter, v, art, d, "STAGE SELECT", W / 2.0, 30.0, 0.6, Color32::WHITE);
                // a window of the list around the cursor
                let list = stage_list(d);
                let first = cursor.saturating_sub(STAGE_ROWS / 2).min(list.len().saturating_sub(STAGE_ROWS));
                for (k, (id, name)) in list.iter().enumerate().skip(first).take(STAGE_ROWS) {
                    let y = 90.0 + 28.0 * (k - first) as f32;
                    if k == *cursor {
                        Art::shade(painter, v, sv([40.0, y - 3.0, 560.0, 26.0]), [Color32::from_rgba_unmultiplied(40, 80, 200, 150); 4]);
                    }
                    let c = if k == *cursor { Color32::WHITE } else { Color32::from_gray(150) };
                    font(painter, v, art, d, name, 280.0, y, 0.32, c);
                    if id.is_some_and(|id| arcade_file(id).is_some()) {
                        small(painter, v, art, d, "TEKKEN 3 ARCADE", 545.0, y + 5.0, Color32::from_gray(170));
                    }
                }
            }
            Page::BallType { cursor, run } => {
                backdrop(painter, v, art);
                font(painter, v, art, d, "BALL SELECT", W / 2.0, 30.0, 0.6, Color32::WHITE);
                // (left / right: OpenKumite's 2 ON 2)
                font(painter, v, art, d, if run.doubles { "2 ON 2" } else { "1 ON 1" }, W / 2.0, 330.0, 0.4, Color32::WHITE);
                for (k, t) in roster.ball.iter().flat_map(|b| &b.types).enumerate() {
                    let y = 150.0 + 40.0 * k as f32;
                    if k == *cursor {
                        Art::shade(painter, v, sv([160.0, y - 3.0, 320.0, 34.0]), [Color32::from_rgba_unmultiplied(40, 80, 200, 150); 4]);
                    }
                    let c = if k == *cursor { Color32::WHITE } else { Color32::from_gray(150) };
                    font(painter, v, art, d, &format!("{} {}", t.name, t.level), W / 2.0, y, 0.4, c);
                }
            }
            Page::Select(s) => draw_select(painter, v, art, d, roster, progress, s),
            // (0x3BA560: the chrome font's own colour)
            Page::Continue { countdown, .. } => {
                let c = &d.raw["arcade"]["continue"];
                let text = crate::art::printf(c["text"].as_str().unwrap_or_default(), &[((countdown + 2) / 100).min(9)]);
                chrome(painter, v, art, d, &text, num(&c["x"]), num(&c["y"]), None, 10.0);
            }
            Page::GameOver { .. } => banner_at(painter, v, art, d, &d.raw["arcade"]["continue"]["game_over"], 0),
            Page::Records { page, scroll, attract } => {
                if *attract && d.raw["ranking"]["usage"].is_object() {
                    // (after GAME OVER the ranking, screen 7: CHARACTER USAGE from its last page to its first, at the
                    // PS2's pace; shortcut: the arena behind is not decoded)
                    let rows: Vec<Record> = record_rows(d, progress, 0).into_iter().map(|(members, score, name)| Record { score, members, name }).collect();
                    let (top, black) = ranking_pace(d, 2, rows.len(), self.frame).unwrap_or((0.0, 1.0));
                    let rise = num(&d.raw["ranking"]["rows"][2]["first"]) - top;
                    draw_ranking(painter, v, art, d, 2, &rows, (usize::MAX, 0), rise, self.frame);
                    painter.rect_filled(v.rect, 0.0, Color32::from_black_alpha((black * 255.0) as u8));
                    return;
                } else if *attract {
                    image(painter, v, art, &d.picture(&["title", "picture"]), [0.0, 0.0, W, H], Color32::WHITE);
                } else {
                    options_backdrop(painter, v, art, d, num(&d.raw["menu"]["dim"]));
                }
                draw_records(painter, v, art, d, roster, progress, *page, *scroll, self.frame, !*attract);
            }
            // (the ranking, screen 7: the records with the new one in its place, its name so far and the letter under
            // the cursor in its place, as 0x3D0350 writes it each frame)
            Page::NameEntry { survival, record, letter, .. } => {
                let r = &d.raw["records"];
                let mut list = progress.list(d, *survival);
                let at = progress.rank(d, *survival, record).unwrap_or(list.len()).min(list.len());
                let typed = record.name.chars().count();
                let mut shown = record.clone();
                if typed < int(&r["name_len"]) {
                    shown.name.extend(r["letters"].as_str().unwrap_or_default().chars().nth(*letter));
                }
                list.insert(at, shown);
                list.truncate(RECORDS);
                draw_ranking(painter, v, art, d, usize::from(*survival), &list, (at, typed), 0.0, self.frame);
            }
            Page::Message { lines, .. } => {
                backdrop(painter, v, art);
                // (a long one, Tekken Bowl's results, closer and smaller)
                let (y0, step, size) = if lines.len() > 6 { (40.0, 400.0 / lines.len() as f32, 0.3) } else { (150.0, 50.0, 0.45) };
                for (k, l) in lines.iter().enumerate() {
                    font(painter, v, art, d, l, W / 2.0, y0 + step * k as f32, if k == 0 { size + 0.25 } else { size }, Color32::WHITE);
                }
            }
            // OpenKumite's layout: a row a character, its cells side by side, 13 rows about the cursor's
            Page::Theater { cursor: [r, c] } => {
                backdrop(painter, v, art);
                font(painter, v, art, d, "THEATER", W / 2.0, 20.0, 0.6, Color32::WHITE);
                let rows = theater(d, progress.seen);
                let first = r.saturating_sub(6).min(rows.len().saturating_sub(13));
                for (k, row) in rows.iter().enumerate().skip(first).take(13) {
                    let y = 80.0 + 26.0 * (k - first) as f32;
                    // cells as wide as the row's longest label (about 7.5 a letter), squeezed to fit the screen
                    let longest = row.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0) as f32;
                    let n = row.len().max(1) as f32;
                    let step = (longest * 7.5 + 14.0).max(72.0).min((W - 60.0) / n);
                    for (j, (label, cell)) in row.iter().enumerate() {
                        let x = if row.len() == 1 { W / 2.0 } else { 30.0 + step * (j as f32 + 0.5) };
                        if k == *r && j == *c {
                            Art::shade(painter, v, sv([x - step / 2.0, y - 3.0, step, 22.0]), [Color32::from_rgba_unmultiplied(40, 80, 200, 150); 4]);
                        }
                        let colour = match cell {
                            Cell::None => Color32::from_gray(90),
                            _ if k == *r && j == *c => Color32::WHITE,
                            _ => Color32::from_gray(170),
                        };
                        small(painter, v, art, d, label, x, y, colour);
                    }
                }
                if let Some((_, Cell::None)) = rows.get(*r).and_then(|row| row.get(*c)) {
                    small(painter, v, art, d, "NOT AVAILABLE: CONVERT THE GAME AGAIN", W / 2.0, 430.0, Color32::from_gray(200));
                }
            }
            Page::Music { cursor, playing } => {
                backdrop(painter, v, art);
                font(painter, v, art, d, "MUSIC PLAYER", W / 2.0, 20.0, 0.6, Color32::WHITE);
                let list = music(d);
                for (k, (bgm, name)) in list.iter().map(|(b, n)| (Some(*b), n.as_str())).chain([(None, "EXIT")]).enumerate() {
                    let y = 70.0 + 16.0 * k as f32;
                    let colour = if k == *cursor { Color32::WHITE } else { Color32::from_gray(150) };
                    let mark = if bgm.is_some() && bgm == *playing { "> " } else { "" };
                    small(painter, v, art, d, &format!("{mark}{name}"), W / 2.0, y, colour);
                }
            }
        }
    }

    /// The BGM the page plays: the music player's, the select's from its frame `select.music.from` on (0x3CB900).
    pub fn music(&self, d: Option<&Data>) -> Option<usize> {
        match &self.page {
            Page::Music { playing, .. } => *playing,
            Page::Select(s) => d.map(|d| &d.raw["select"]["music"]).filter(|m| m.is_object() && s.frame >= int(&m["from"]) as u32).map(|m| int(&m["bgm"])),
            _ => None,
        }
    }
}

/// What a Theater cell does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    /// Ending (number, the player's costume).
    Ending(usize, usize),
    /// A movie (video/NN.avi).
    Movie(usize),
    /// The EMBU with a cast.
    Embu(usize),
    Music,
    Exit,
    /// Not playable here (an older conversion's EMBU).
    None,
}

/// Theater's rows (0x3544C8, 0x3545A8): each character with an ending seen, its cells the costumes labelled whose
/// ending was seen; then OPENING MOVIE, ARCADE MOVIE, EMBU (and EMBU2, not shown: the last unlock's), MUSIC PLAYER,
/// EXIT THEATER MENU.
fn theater(d: &Data, seen: u64) -> Vec<Vec<(String, Cell)>> {
    let Some(e) = d.endings.as_deref() else { return vec![vec![("EXIT THEATER MENU".into(), Cell::Exit)]] };
    let t = &e.raw["theater"];
    let mut rows: Vec<Vec<(String, Cell)>> = (0..34)
        .map(|c| {
            (0..8)
                .filter_map(|k| {
                    let label = t["labels"][c][k].as_str()?;
                    let n = e.ending(c, k).filter(|&n| n < 64 && seen >> n & 1 == 1)?;
                    Some((label.to_string(), Cell::Ending(n, k)))
                })
                .collect()
        })
        .filter(|r: &Vec<_>| !r.is_empty())
        .collect();
    let cell = |x: &Value| match (x["movie"].as_u64(), x["scene"].as_u64()) {
        (Some(m), _) => Cell::Movie(m as usize),
        (_, Some(_)) if !e.embu.is_empty() => Cell::Embu(x["cast"].as_u64().unwrap_or(0) as usize),
        _ => Cell::None,
    };
    rows.extend(t["extras"].as_array().into_iter().flatten().filter(|x| x["cast"].is_null()).map(|x| vec![(x["label"].as_str().unwrap_or_default().to_string(), cell(x))]));
    rows.push(vec![("MUSIC PLAYER".into(), Cell::Music)]);
    rows.push(vec![("EXIT THEATER MENU".into(), Cell::Exit)]);
    rows
}

/// The staff roll's lines (0x51B880) to its end mark (a negative colour): each kind, text and colour; of an
/// alternative pair the first (the one until Tekken Bowl is unlocked, 0x3BFEB0, which is not here yet).
fn roll_lines(e: &EndingData) -> Vec<(u64, &str, usize)> {
    let mut alt = false;
    let lines = e.raw["staff_roll"]["lines"].as_array().into_iter().flatten().take_while(|l| l["font"].as_u64().is_some_and(|f| f < 0x8000));
    lines
        .filter(|l| {
            let (a, was) = (l["alternative"] == true, alt);
            alt = a;
            !(a && was)
        })
        .map(|l| (l["kind"].as_u64().unwrap_or(0), l["text"].as_str().unwrap_or_default(), l["font"].as_u64().unwrap_or(5) as usize))
        .collect()
}

/// The staff roll's start, the px a line waits for and the speed (px, px a frame).
fn roll_metrics(e: &EndingData) -> (f32, f32, f32) {
    let r = &e.raw["staff_roll"];
    (r["start_y"].as_f64().unwrap_or(480.0) as f32, r["line_px"].as_f64().unwrap_or(32.0) as f32, r["scroll_px"].as_f64().unwrap_or(1.5).max(0.1) as f32)
}

/// A card's states 2-7 (0x3AC018): 171 frames each (a 170-frame timer to -1), three a card after state 1's frame.
const CARD_STATE: u32 = 171;
/// The first line's frame (state 8).
const LINES_FROM: u32 = 1 + 6 * CARD_STATE;

/// The frames between two lines: a line each time the timer passes 32 px at 1.5 a frame (22: it restarts from 0,
/// so the lines are 33 px apart).
fn line_frames(e: &EndingData) -> u32 {
    let (_, line, speed) = roll_metrics(e);
    (line / speed).ceil() as u32
}

/// The frame the lines stop on: the end mark's, then 139 frames on (state 10: to 0xD000 at 0x180 a frame).
fn roll_still(e: &EndingData) -> u32 {
    LINES_FROM + line_frames(e) * roll_lines(e).len().saturating_sub(1) as u32 + 140
}

/// The staff roll's length in frames: the lines stand still for 210 (state 11).
pub fn roll_frames(e: &EndingData) -> u32 {
    roll_still(e) + 210
}

/// An ending's overlay: the scene's fades (black, then white), and the staff roll at its frame `roll` (0x3AC018): the
/// cards "TEKKEN TAG TOURNAMENT" and "NAMCO PRODUCTION STAFF" (0x3ABEC0, 0x3ABF18: the chrome font at size 5 in
/// colour 11, centred, foot at y 200), each from black over 171 frames (0x3A67D0) and from its 343rd frame to black
/// over 171 / 172 (0x3A6770); then the lines (size 4 in their colour; x by kind, 0x3ABF70: 0 from 48, 1 from 304, 2
/// ending at 608, 3 and 4 centred) rising from y 480 at 1.5 px a frame.
pub fn draw_ending(painter: &egui::Painter, v: &View, art: &Art, d: &Data, roll: Option<u32>, black: u8, white: u8, overlays: &[Overlay]) {
    // the script's 2D objects (op 24's negative is the renderer's)
    let at = |x: f32, y: f32| v.pos(x * SCREEN_W / W, y * SCREEN_H / H);
    for o in overlays {
        match *o {
            Overlay::Iris { x, y, r } => outside(painter, |a: f32| at(x + r * a.cos(), y + r * a.sin()), at(x, y)),
            Overlay::Oval => outside(painter, |a: f32| at(320.0 + 220.0 * 1.33 * a.cos(), 224.0 + 220.0 * a.sin()), at(320.0, 224.0)),
            Overlay::Tint([r, g, b], a) => {
                painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_rgba_unmultiplied(r, g, b, (a.min(1.0) * 255.0) as u8));
            }
            Overlay::Negative(_) => {}
        }
    }
    // (the whole window: the scene fills it)
    painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_black_alpha(black));
    painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_white_alpha(white));
    let (Some(f), Some(e)) = (roll, d.endings.as_deref()) else { return };
    let width = |text: &str, size: f32| chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, text, 0.0, 0.0, None, size);
    if f < LINES_FROM {
        let card = f / (3 * CARD_STATE);
        let text = e.raw["staff_roll"]["cards"][card as usize].as_str().unwrap_or_default();
        chrome(painter, v, art, d, text, (W - width(text, 5.0)) / 2.0, 200.0, Some(11), 5.0);
        let t = f - card * 3 * CARD_STATE;
        let level = match t {
            t if t < CARD_STATE => 1.0 - t as f32 / CARD_STATE as f32,
            t => (t.saturating_sub(2 * CARD_STATE) as f32 / (CARD_STATE + card) as f32).min(1.0),
        };
        painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_black_alpha((level * 255.0) as u8));
        return;
    }
    let (y0, _, speed) = roll_metrics(e);
    let (each, still) = (line_frames(e), roll_still(e));
    for (k, (kind, text, colour)) in roll_lines(e).into_iter().enumerate() {
        let Some(t) = f.min(still).checked_sub(LINES_FROM + each * k as u32) else { break };
        let y = (y0 - speed * t as f32).floor();
        if !(0.0..H + 32.0).contains(&y) || text.is_empty() {
            continue;
        }
        let x = match kind {
            0 => 48.0,
            1 => 304.0,
            2 => 608.0 - width(text, 4.0),
            _ => (W - width(text, 4.0)) / 2.0,
        };
        chrome(painter, v, art, d, text, x, y, Some(colour), 4.0);
    }
}

/// Black over the window outside the closed curve `edge(angle)` about `centre` (the GS's 15-step fans, 0x3CF850).
fn outside(painter: &egui::Painter, edge: impl Fn(f32) -> egui::Pos2, centre: egui::Pos2) {
    let far = 4.0 * painter.clip_rect().size().length();
    let mut mesh = egui::Mesh::default();
    for k in 0..=64 {
        let a = k as f32 * std::f32::consts::TAU / 64.0;
        let p = edge(a);
        let out = centre + (p - centre).normalized() * far;
        mesh.colored_vertex(p, Color32::BLACK);
        mesh.colored_vertex(if p == centre { centre + egui::vec2(a.cos(), a.sin()) * far } else { out }, Color32::BLACK);
        if k > 0 {
            let i = 2 * k as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(mesh);
}

/// The music player's list (0x455D80): BGM and name, EXIT left out.
fn music(d: &Data) -> Vec<(usize, String)> {
    let list = d.endings.as_ref().and_then(|e| e.raw["theater"]["music"].as_array().cloned()).unwrap_or_default();
    list.iter().filter_map(|m| Some((m["bgm"].as_u64().filter(|&b| b != 254)? as usize, m["name"].as_str().unwrap_or_default().to_string()))).collect()
}

/// The stage select's rows shown at once.
const STAGE_ROWS: usize = 11;

/// The stage select's list: RANDOM, then Tekken Tag's arenas, then the arcade pack's (their ids and names).
fn stage_list(d: &Data) -> Vec<(Option<usize>, String)> {
    std::iter::once((None, "RANDOM".to_string())).chain(d.arenas.iter().chain(&d.arcade).map(|(id, name)| (Some(*id), name.clone()))).collect()
}

/// OPTIONS' rows: the game's, then ARCADE ARENAS with the arcade pack.
/// The options with a value (`options` of kind 0 or 1).
fn option_count(d: &Data) -> usize {
    d.raw["options"].as_array().map_or(0, |o| o.iter().take_while(|o| int(&o["kind"]) != 2).count())
}

/// GAME OPTIONS' rows: the options by number, ARCADE ARENAS (OpenKumite's, with the arcade pack), then DEFAULT and
/// EXIT.
fn game_rows(d: &Data) -> Vec<usize> {
    let n = d.raw["options"].as_array().map_or(0, Vec::len);
    let k = option_count(d);
    (0..k).chain((!d.arcade.is_empty()).then_some(ARCADE_ROW)).chain(k..n).collect()
}

/// A title of `options_menu` (`title`, `title_xy`, `title_size`, `title_colour`).
fn title(painter: &egui::Painter, v: &View, art: &Art, d: &Data, t: &Value) {
    let m = &d.raw["options_menu"];
    let size = t["title_size"].as_f64().map_or(num(&m["title_size"]), |s| s as f32);
    chrome(painter, v, art, d, t["title"].as_str().unwrap_or_default(), num(&t["title_xy"][0]), num(&t["title_xy"][1]), Some(int(&m["title_colour"])), size);
}

/// The options screens' logo dimmed to `dim`, under `options_menu.backdrop` (0x3BF2F8): black at the sides to its
/// colour in the middle, at its alpha.
fn options_backdrop(painter: &egui::Painter, v: &View, art: &Art, d: &Data, dim: f32) {
    image(painter, v, art, &d.picture(&["title", "picture"]), [0.0, 0.0, W, H], ps2(dim).0);
    let b = &d.raw["options_menu"]["backdrop"];
    if b.is_array() {
        let (c, edge) = (rgba(b), Color32::from_rgba_unmultiplied(0, 0, 0, rgba(b).a()));
        Art::shade(painter, v, sv([0.0, 0.0, W / 2.0, H]), [edge, c, edge, c]);
        Art::shade(painter, v, sv([W / 2.0, 0.0, W / 2.0, H]), [c, edge, c, edge]);
    }
}

/// OPTIONS (0x3B19B0, `options_menu`): the title, the entries `item_dy` apart, each over its `line` (the cursor's
/// white, over the green `band`), in their colours (MEMORY CARD(PS2) in the unavailable one: OpenKumite saves by
/// itself), the cursor entry's help line.
fn draw_options(painter: &egui::Painter, v: &View, art: &Art, d: &Data, p: &Progress, cursor: usize) {
    let m = &d.raw["options_menu"];
    title(painter, v, art, d, m);
    let (x, y0, dy, size) = (num(&m["item_xy"][0]), num(&m["item_xy"][1]), num(&m["item_dy"]), num(&m["size"]));
    let (line, band) = (&m["line"], &m["band"]);
    for (k, label) in m["items"].as_array().into_iter().flatten().enumerate() {
        let y = y0 + dy * k as f32;
        if k == cursor {
            let (on, [b0, b1, b2], [t, b]) = (rgba(&band["colour"]), [0, 1, 2].map(|i| num(&band["x"][i])), [num(&band["y"][0]), num(&band["y"][1])]);
            Art::shade(painter, v, sv([b0, y + t, b1 - b0, b - t]), [Color32::TRANSPARENT, on, Color32::TRANSPARENT, on]);
            Art::shade(painter, v, sv([b1, y + t, b2 - b1, b - t]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
        }
        let (l0, l1, t, b) = (num(&line["x"][0]), num(&line["x"][1]), num(&line["y"][0]), num(&line["y"][1]));
        let on = rgba(if k == cursor { &line["cursor"] } else { &line["colour"] });
        Art::shade(painter, v, sv([l0, y + t, l1 - l0, b - t]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
        let colour = match k {
            _ if k == cursor => int(&m["colours"]["cursor"]),
            OPT_MEMORY_CARD => int(&m["game"]["colours"]["off"]),
            _ => int(&m["colours"]["item"]),
        };
        chrome(painter, v, art, d, label.as_str().unwrap_or_default(), x, y, Some(colour), size);
    }
    if p.setting(d, HELP_ROW) != 0 {
        help_line(painter, v, art, d, int(&m["help"]) + cursor, 0);
    }
}

/// RECORDS' rows shown at once.
const RECORD_ROWS: usize = 6;

/// A RECORDS page's rows: CHARACTER USAGE (each selectable character: its id, its per-mille of the picks; most picked
/// first, by id among equals: 0x3C84B8), GREATEST SURVIVORS, TIME ATTACK.
fn record_rows(d: &Data, p: &Progress, page: usize) -> Vec<(Vec<usize>, u32, String)> {
    match page {
        0 => {
            let total: u32 = p.usage.iter().sum();
            let mut rows: Vec<(Vec<usize>, u32, String)> =
                (0..34).filter(|&c| d.selectable(p.stage, c)).map(|c| (vec![c], p.usage.get(c).copied().unwrap_or(0) * 1000 / total.max(1), String::new())).collect();
            rows.sort_by_key(|a| std::cmp::Reverse(a.1));
            rows
        }
        _ => p.list(d, page == 1).into_iter().map(|r| (r.members, r.score, r.name)).collect(),
    }
}

/// RECORDS (0x3B2978, `records`): the title; the page's name between its arrows (blinking every `arrows_blink`
/// frames); `rows` rows `row_dy` apart, each over a bar in the page's colour (0x3B15F8): the rank and its suffix
/// (0x3B1EC8), the faces (0x3CE318, 40 x 42), then CHARACTER USAGE's name and share "%3d.%1d%", SURVIVORS' wins and
/// WIN(S) and name, TIME ATTACK's time and name, at the page's `columns`; the arrows of more rows above and below
/// (0x3B1F78); the page's help line.
#[allow(clippy::too_many_arguments)]
fn draw_records(painter: &egui::Painter, v: &View, art: &Art, d: &Data, roster: &Roster, p: &Progress, page: usize, scroll: usize, frame: u32, help: bool) {
    let r = &d.raw["records"];
    let pg = &r["pages"][page];
    title(painter, v, art, d, r);
    let py = num(&r["page_y"]);
    chrome(painter, v, art, d, pg["title"].as_str().unwrap_or_default(), num(&pg["x"]), py, Some(int(&pg["colour"])), num(&r["page_size"]));
    let arrow = |k: usize, x: f32, y: f32| {
        let pic = d.raw["menu"]["arrows"]["pictures"][k].as_str().unwrap_or_default();
        let h = art.pic(pic).map_or(0.0, |p| p.1[1] as f32);
        sprite(painter, v, art, pic, x, y - h, Color32::WHITE);
    };
    let rows = record_rows(d, p, page);
    if frame / int(&r["arrows_blink"]).max(1) as u32 % 2 == 1 {
        arrow(0, num(&r["arrows_x"][0]), py);
        arrow(1, num(&r["arrows_x"][1]), py);
        // (rows above, rows below: `more`'s up and down at their feet)
        let m = &r["more"];
        let (tex, n) = (art.pic(m["picture"].as_str().unwrap_or_default()), num(&m["cell"]));
        for (k, more) in [scroll > 0, scroll + RECORD_ROWS < rows.len()].into_iter().enumerate() {
            if let (true, Some((tex, [tw, th]))) = (more, tex.as_ref().map(|(t, s)| (t, *s))) {
                let u = num(&m["cells"][k]) * n / tw as f32;
                let [x, y, w, h] = sv([num(&m["x"]), num(&m["y"][k]) - n, n, n]);
                let c = |i: usize| int(&m["tint"][i]).min(255) as u8;
                painter.image(tex.id(), v.rect(x, y, w, h), Rect::from_min_max(Pos2::new(u, 0.0), Pos2::new(u + n / tw as f32, n / th as f32)), Color32::from_rgb(c(0), c(1), c(2)));
            }
        }
    }
    let col = |k: usize, key: &str| num(&pg["columns"][k][key]);
    let colour = |k: usize| Some(int(&pg["columns"][k]["colour"]));
    let (y0, dy, bar_w) = (num(&r["row_y"]), num(&r["row_dy"]), num(&r["bar_w"]));
    for (k, (members, score, name)) in rows.iter().enumerate().skip(scroll).take(RECORD_ROWS) {
        let y = y0 + dy * (k - scroll) as f32;
        let on = rgba(&pg["bar"]);
        let (b, x) = (&d.raw["pause"]["bar"], num(&d.raw["options_menu"]["game"]["row_xy"][0]));
        let (t, h) = (num(&b["y"][0]), num(&b["y"][1]) - num(&b["y"][0]));
        Art::shade(painter, v, sv([0.0, y + 2.0 + t, x, h]), [Color32::TRANSPARENT, on, Color32::TRANSPARENT, on]);
        Art::shade(painter, v, sv([x, y + 2.0 + t, bar_w, h]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
        let rank = k + 1;
        let suffix = match rank % 10 {
            1 if rank != 11 => 0,
            2 if rank != 12 => 1,
            3 if rank != 13 => 2,
            _ => 3,
        };
        let end = chrome(painter, v, art, d, &format!("{rank:2}"), num(&r["rank_x"]), y - 4.0, Some(int(&pg["colour"])), 5.0);
        chrome(painter, v, art, d, r["suffixes"][suffix].as_str().unwrap_or_default(), end, y, Some(11), 4.0);
        for (m, &c) in members.iter().enumerate().take(2) {
            let fx = num(&r["faces_x"][m]);
            image(painter, v, art, &d.picture(&["select", "faces", &c.to_string()]), [fx, y - 46.0, 40.0, 42.0], Color32::WHITE);
        }
        match page {
            0 => {
                let n = members.first().and_then(|&c| roster.by_id(c)).map_or(String::new(), |c| c.name.to_uppercase());
                chrome(painter, v, art, d, &n, col(0, "x"), y + col(0, "dy"), colour(0), col(0, "size"));
                chrome(painter, v, art, d, &format!("{:3}.{}%", score / 10, score % 10), col(1, "x"), y + col(1, "dy"), colour(1), col(1, "size"));
            }
            1 => {
                chrome(painter, v, art, d, &format!("{:3}", (*score).min(999)), col(0, "x"), y + col(0, "dy"), colour(0), col(0, "size"));
                chrome(painter, v, art, d, if *score >= 2 { "WINS" } else { "WIN" }, col(1, "x"), y + col(1, "dy"), colour(1), col(1, "size"));
                chrome(painter, v, art, d, name, col(2, "x"), y + col(2, "dy"), colour(2), col(2, "size"));
            }
            _ => {
                chrome(painter, v, art, d, &clock(*score), col(0, "x"), y + col(0, "dy"), colour(0), col(0, "size"));
                chrome(painter, v, art, d, name, col(1, "x"), y + col(1, "dy"), colour(1), col(1, "size"));
            }
        }
    }
    if help && p.setting(d, HELP_ROW) != 0 {
        help_line(painter, v, art, d, int(&r["help"]) + page, 0);
    }
}

/// The ranking (screen 7, `ranking`) of TIME ATTACK (`kind` 0) or SURVIVAL (1) where a new record is named: the banner,
/// `shown` rows about the entry's (0x3C9034), each over its separator: the rank, the members' faces and name plates, the
/// time or the wins, the name; the entry's row over its band, the place being named underlined (red to white), the
/// seconds left. shortcut: the rows' slide and the arena behind are not done; upgrade: decode 0x3C8A88's steps.
#[allow(clippy::too_many_arguments)]
fn draw_ranking(painter: &egui::Painter, v: &View, art: &Art, d: &Data, kind: usize, list: &[Record], (entry, typed): (usize, usize), rise: f32, frame: u32) {
    let rk = &d.raw["ranking"];
    let p = |v: &Value| [num(&v[0]), num(&v[1])];
    let [bx, by] = p(&rk["banner_xy"]);
    let [c0, c1] = p(&rk["clip"]);
    let [_, top, _, h] = sv([0.0, c0, 0.0, c1 - c0]);
    let clip = painter.with_clip_rect(Rect::from_min_max(v.pos(0.0, top), v.pos(SCREEN_W, top + h)));
    let rows = &rk["rows"][kind];
    let (dy, y0, n) = (num(&rows["dy"]), num(&rows["first"]), int(&rows["shown"]).max(1));
    // (a list without an entry: every row,  up)
    let (first, n) = if entry >= list.len() {
        (0, list.len())
    } else if entry < 2 {
        (0, n)
    } else {
        ((entry - 2).min(list.len().saturating_sub(n)), n)
    };
    let colour = |k: &str| Some(int(&rk["colours"][k]));
    let measure = |text: &str, size: f32| chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, text, 0.0, 0.0, None, size);
    for (k, r) in list.iter().enumerate().skip(first).take(n) {
        let y = y0 + dy * (k - first) as f32 - rise;
        if y < c0 || y - dy > c1 {
            continue;
        }
        let on = k == entry;
        let c = if on {
            colour("entry")
        } else if kind == 2 {
            Some(int(&rk["usage"]["rank_colour"]))
        } else {
            colour("row")
        };
        if on {
            let b = &rk["band"];
            Art::shade(&clip, v, sv([0.0, y - num(&b["h"]), W, num(&b["h"])]), [rgba(&b["colour"]); 4]);
        }
        sprite(&clip, v, art, rk["separator"].as_str().unwrap_or_default(), 0.0, y, Color32::WHITE);
        let rank = &rk["rank"];
        let end = chrome(&clip, v, art, d, &format!("{:2}", k + 1), num(&rank["x"]), y + num(&rank["dy"]), c, 10.0);
        let suffix = match (k + 1) % 10 {
            1 if k + 1 != 11 => 0,
            2 if k + 1 != 12 => 1,
            3 if k + 1 != 13 => 2,
            _ => 3,
        };
        chrome(&clip, v, art, d, d.raw["records"]["suffixes"][suffix].as_str().unwrap_or_default(), end, y + num(&rank["suffix_dy"]), c, num(&rank["suffix_size"]));
        // (the second member's face first, the first's over it; then their name plates; CHARACTER USAGE one of each)
        let (fs, u) = (num(&rk["face_size"]), &rk["usage"]);
        let place = |key: &str, one: &str, m: usize| if kind == 2 { p(&u[one]) } else { p(&rk[key][m]) };
        for (m, body) in [1, 0].into_iter().filter_map(|m| Some((m, *r.members.get(m)?))) {
            let [fx, fy] = place("faces", "face", m);
            sprite(&clip, v, art, rk["face_frame"].as_str().unwrap_or_default(), fx - 2.0, y + fy - 2.0, Color32::WHITE);
            image(&clip, v, art, &d.picture(&["select", "faces", &body.to_string()]), [fx, y + fy, fs, fs], Color32::WHITE);
        }
        for (m, &body) in r.members.iter().enumerate().take(2) {
            let [px, py] = place("plates", "plate", m);
            // (0x3CF1E8: less the plate's own x)
            let plate = &d.raw["hud"]["plates"][body];
            sprite(&clip, v, art, plate["picture"].as_str().unwrap_or_default(), px - num(&plate["x"]), y + py, Color32::WHITE);
        }
        if kind == 2 {
            // (0x3C7530: the bar in the character's two colours; 0x3C74A8: the share, its tenth smaller)
            let (b, s) = (&u["bar"], &u["share"]);
            let w = num(&b["w"]) * r.score.min(1000) as f32 / 1000.0;
            let cs = &b["colours"][r.members.first().copied().unwrap_or(0)];
            let rgb = |o: usize| Color32::from_rgb(int(&cs[o]) as u8, int(&cs[o + 1]) as u8, int(&cs[o + 2]) as u8);
            if w > 0.0 {
                Art::shade(&clip, v, sv([num(&b["x"]), y + num(&b["dy"]), w, num(&b["h"])]), [rgb(0), rgb(3), rgb(0), rgb(3)]);
            }
            let sc = Some(int(&s["colour"]));
            let (whole, small) = if r.score >= 1000 { ("100".to_string(), "%".to_string()) } else { (format!("{:2}", r.score / 10), format!(".{}%", r.score % 10)) };
            let end = chrome(&clip, v, art, d, &whole, num(&s["x"]), y + num(&s["dy"]), sc, 10.0);
            chrome(&clip, v, art, d, &small, end, y + num(&s["small_dy"]), sc, num(&s["small_size"]));
            continue;
        }
        if kind == 0 {
            let t = &rk["time"];
            chrome(&clip, v, art, d, &clock(r.score), num(&t["x"]), y + num(&t["dy"]), if on { c } else { colour("time") }, num(&t["size"]));
        } else {
            // (0x3C7428: "WINS" then the number, a space between below 100)
            let w = &rk["wins"];
            let label = if r.score < 100 { "WINS " } else { "WINS" };
            let end = chrome(&clip, v, art, d, label, num(&w["x"]), y + num(&w["dy"]), if on { c } else { colour("wins") }, num(&w["size"]));
            chrome(&clip, v, art, d, &r.score.min(999).to_string(), end, y + num(&w["number_dy"]), if on { c } else { colour("wins") }, num(&w["number_size"]));
        }
        let nm = &rk["name"];
        chrome(&clip, v, art, d, &r.name, num(&nm["x"]), y + num(&nm["dy"]), c, 10.0);
        if on && typed < int(&d.raw["records"]["name_len"]) {
            // (the place being named: (255, 2t + 1, 2t + 1), t from `low` to `high` and back by `step` a frame)
            let cu = &rk["cursor"];
            let (lo, hi, st) = (num(&cu["low"]), num(&cu["high"]), num(&cu["step"]).max(1.0));
            let span = ((hi - lo) / st).max(1.0) as u32;
            let i = frame % (2 * span);
            let t = lo + st * if i < span { i } else { 2 * span - i } as f32;
            let g = (2.0 * t + 1.0).min(255.0) as u8;
            let x = num(&nm["x"]) + measure(&r.name.chars().take(typed).collect::<String>(), 10.0);
            Art::shade(&clip, v, sv([x, y + num(&cu["dy"]), num(&cu["w"]), num(&cu["h"])]), [Color32::from_rgb(255, g, g); 4]);
            let n = &rk["counter"];
            let left = int(&rk["frames"]).saturating_sub(frame as usize).div_ceil(60);
            chrome(&clip, v, art, d, &left.to_string(), num(&n["x"]), y + num(&n["dy"]), Some(int(&n["colour"])), num(&n["size"]));
        }
    }
    // (the banner over the rows, 0x3C93A0 after them)
    sprite(painter, v, art, rk["banners"][kind].as_str().unwrap_or_default(), bx, by, Color32::WHITE);
}

/// The cursor's bar (0x3B15F8; the pause's `bar` colour): from `x0` (nothing) to `x1` (its colour) to `x2` (nothing),
/// `bar.y` about the foot `y`.
fn bar(painter: &egui::Painter, v: &View, d: &Data, [x0, x1, x2]: [f32; 3], y: f32) {
    let b = &d.raw["pause"]["bar"];
    let (on, t, h) = (rgba(&b["colour"]), num(&b["y"][0]), num(&b["y"][1]) - num(&b["y"][0]));
    Art::shade(painter, v, sv([x0, y + t, x1 - x0, h]), [Color32::TRANSPARENT, on, Color32::TRANSPARENT, on]);
    Art::shade(painter, v, sv([x1, y + t, x2 - x1, h]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
}

/// GAME OPTIONS (0x3B42A0, `options_menu.game`): the title; a row each `row_dy` apart, "LABEL:" then its value in
/// `default`'s colour while at its default, else `value`'s (a number 0 as OFF); the cursor's label gold over its bar,
/// the arrows towards the values it can go to (a list's both, a two-value list's right one only); DEFAULT's YES / NO
/// once taken; the cursor row's help line with its value's alternative. OpenKumite's ARCADE ARENAS is a row of its
/// own before DEFAULT.
#[allow(clippy::too_many_arguments)]
fn draw_game_options(painter: &egui::Painter, v: &View, art: &Art, d: &Data, cursor: usize, values: &[usize], arcade: bool, reset: Option<bool>) {
    let g = &d.raw["options_menu"]["game"];
    title(painter, v, art, d, g);
    let size = num(&d.raw["options_menu"]["size"]);
    let (x, y0, dy) = (num(&g["row_xy"][0]), num(&g["row_xy"][1]), num(&g["row_dy"]));
    let colour = |k: &str| Some(int(&g["colours"][k]));
    let arrow = |right: bool, ax: f32, y: f32| {
        let pic = d.raw["menu"]["arrows"]["pictures"][usize::from(right)].as_str().unwrap_or_default();
        let h = art.pic(pic).map_or(0.0, |p| p.1[1] as f32);
        sprite(painter, v, art, pic, ax, y - h, Color32::WHITE);
    };
    // the cursor's option's modes (0x3B3F88): the main menu's ARCADE..TEKKEN BOWL (once open) right-aligned over their
    // box, in their colours where it applies, else `off`
    if let Some(&row) = game_rows(d).get(cursor).filter(|&&r| r != ARCADE_ROW) {
        let (m, mask) = (&g["modes"], int(&d.raw["options"][row]["modes"]));
        let open = d.bowl_open(progress().stage);
        let items: Vec<&Value> = d.raw["menu"]["items"].as_array().into_iter().flatten().filter(|i| int(&i["mode"]) < BOWL as usize || (int(&i["mode"]) == BOWL as usize && open)).collect();
        let (top, dy, size) = (num(&m["y"]), num(&m["dy"]), num(&m["size"]));
        let [b0, b1] = [num(&m["box"]["x"][0]), num(&m["box"]["x"][1])];
        let bottom = top + dy * items.len() as f32 + num(&m["box"]["below"]);
        let by = num(&m["box"]["y"]);
        Art::shade(painter, v, sv([b0, by, b1 - b0, bottom - by]), [Color32::TRANSPARENT, Color32::BLACK, Color32::TRANSPARENT, Color32::BLACK]);
        for (k, i) in items.iter().enumerate() {
            let label = i["label"].as_str().unwrap_or_default();
            let w = chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, label, 0.0, 0.0, None, size);
            let c = if mask >> int(&i["mode"]) & 1 == 1 { int(&i["colour"]) } else { int(&m["off"]) };
            chrome(painter, v, art, d, label, num(&m["right"]) - w, top + dy * k as f32, Some(c), size);
        }
    }
    for (k, &row) in game_rows(d).iter().enumerate() {
        let y = y0 + dy * k as f32;
        let o = &d.raw["options"][row];
        let (label, kind) = if row == ARCADE_ROW { ("ARCADE ARENAS", 0) } else { (o["label"].as_str().unwrap_or_default(), int(&o["kind"])) };
        let on = k == cursor;
        if kind == 2 {
            // (DEFAULT in the default's colour)
            let c = if on {
                colour("cursor")
            } else if k + 1 < game_rows(d).len() {
                colour("default")
            } else {
                colour("label")
            };
            if on {
                let end = chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, label, x, y, None, size);
                bar(painter, v, d, [0.0, x, end + num(&g["bar_past"])], y);
            }
            chrome(painter, v, art, d, label, x, y, c, size);
            if let (true, Some(yes)) = (on, reset) {
                let (yes_c, no_c) = if yes { (colour("cursor"), colour("off")) } else { (colour("off"), colour("cursor")) };
                let end = chrome(painter, v, art, d, "YES  ", num(&g["yes_no_x"]), y, yes_c, size);
                chrome(painter, v, art, d, "NO", end, y, no_c, size);
                arrow(!yes, num(&o["arrows"][usize::from(!yes)]), y);
            }
            continue;
        }
        let (value, default) = if row == ARCADE_ROW {
            (usize::from(arcade), 1)
        } else {
            let default = d.option(row).0;
            (values.get(row).copied().unwrap_or(default), default)
        };
        let text = match (row, kind) {
            (ARCADE_ROW, _) => ["OFF", "ON"][value.min(1)].to_string(),
            (_, 1) if value == 0 => " OFF".into(),
            (_, 1) => format!(" {value}"),
            _ => o["values"][value].as_str().unwrap_or_default().to_string(),
        };
        let head = format!("{label}:");
        let end = chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, &head, x, y, None, size);
        if on {
            bar(painter, v, d, [0.0, x, end + num(&g["bar_past"])], y);
        }
        chrome(painter, v, art, d, &head, x, y, if on { colour("cursor") } else { colour("label") }, size);
        let value_end = chrome(painter, v, art, d, &text, end, y, if value == default { colour("default") } else { colour("value") }, size);
        if on {
            let [l, r] = [num(&o["arrows"][0]), num(&o["arrows"][1])];
            let (l, r) = if row == ARCADE_ROW { (-1.0, value_end + 2.0) } else { (l, r) };
            if l >= 0.0 && (kind == 0 || value > 0) {
                arrow(false, l, y);
            }
            if r >= 0.0 && (kind == 0 || value < 10) {
                arrow(true, r, y);
            }
        }
    }
    let row = game_rows(d).get(cursor).copied().unwrap_or(ARCADE_ROW);
    if row != ARCADE_ROW && values.get(HELP_ROW).is_none_or(|&h| h != 0) {
        help_line(painter, v, art, d, int(&g["help"]) + row, values.get(row).copied().unwrap_or(0));
    }
}

/// Tekken Tag's 640 x 448 rectangle in the 368 x 480 one the screens draw on.
fn sv([x, y, w, h]: [f32; 4]) -> [f32; 4] {
    [x * SCREEN_W / W, y * SCREEN_H / H, w * SCREEN_W / W, h * SCREEN_H / H]
}

/// Character `id`'s select face (None: RANDOM's "?") at [x, y, w, h] of the 368 x 480 screen (a place of Tekken 3's).
pub fn face_in(painter: &egui::Painter, v: &View, art: &Art, d: &Data, id: Option<usize>, [x, y, w, h]: [f32; 4]) {
    let file = id.map_or_else(|| d.picture(&["select", "random_face"]), |c| d.picture(&["select", "faces", &c.to_string()]));
    if let Some((t, _)) = art.pic(&file) {
        Art::image(painter, v, &t, x, y, w, h, Color32::WHITE);
    }
}

/// A picture (pictures/`file`) at [x, y, w, h] of the 640 x 448 screen (a negative w mirrors it, to the left of x).
fn image(painter: &egui::Painter, v: &View, art: &Art, file: &str, r: [f32; 4], tint: Color32) {
    let Some((tex, _)) = art.pic(file) else { return };
    let [x, y, w, h] = sv(r);
    let (x0, x1) = if w < 0.0 { (x + w, x) } else { (x, x + w) };
    let uv = if w < 0.0 { Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(0.0, 1.0)) } else { Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)) };
    painter.image(tex.id(), Rect::from_min_max(v.pos(x0, y), v.pos(x1, y + h)), uv, tint);
}

/// A picture at its own size.
fn sprite(painter: &egui::Painter, v: &View, art: &Art, file: &str, x: f32, y: f32, tint: Color32) {
    if let Some((_, [w, h])) = art.pic(file) {
        image(painter, v, art, file, [x, y, w as f32, h as f32], tint);
    }
}

/// The menus' backdrop: the blue flame (file 182) on each side, mirrored (0x3C3A74).
fn backdrop(painter: &egui::Painter, v: &View, art: &Art) {
    let w = 292.0 * H / 391.0;
    image(painter, v, art, "182_001.png", [0.0, 0.0, w, H], Color32::from_gray(140));
    image(painter, v, art, "182_001.png", [W, 0.0, -w, H], Color32::from_gray(140));
}

/// The menus' text size (the printf's size 5: 19 of the 640 x 448 screen high, as the PS2 shows it) as the chrome
/// font's scale, and its height.
const TEXT_SCALE: [f32; 2] = [0.67, 0.45];
const TEXT_H: f32 = 42.0 * TEXT_SCALE[1];

/// A PS2 colour level (0x80 = as drawn): the tint, and an additive tint for what goes past 0x80.
fn ps2(level: f32) -> (Color32, Color32) {
    let g = |l: f32| Color32::from_gray((l.clamp(0.0, 128.0) * 255.0 / 128.0) as u8);
    (g(level), additive(g(level - 128.0)))
}

/// A picture at [x, y, w, h] at a PS2 colour level.
fn lit(painter: &egui::Painter, v: &View, art: &Art, file: &str, r: [f32; 4], level: f32) {
    let (tint, over) = ps2(level);
    image(painter, v, art, file, r, tint);
    if level > 128.0 {
        image(painter, v, art, file, r, over);
    }
}

/// The main menu (0x3BD2E0): the logo, `rows` rows about the cursor, `row_h` apart and `scroll` off, each in its text
/// colour, clipped to `clip` under the black fades; the band behind the cursor's row, its alpha up `alpha_step` a
/// frame; the arrows on its sides, blinking every `period` frames once the band is whole. An unlocked entry not yet
/// entered (`entered`) blinks colour 9 every 4 frames.
#[allow(clippy::too_many_arguments)]
fn draw_menu(painter: &egui::Painter, v: &View, art: &Art, d: &Data, items: &[(u32, String)], cursor: usize, scroll: i32, frame: u32, entered: u32) {
    let m = &d.raw["menu"];
    image(painter, v, art, &d.picture(&["title", "picture"]), [0.0, 0.0, W, H], Color32::WHITE);
    let band = &m["band"];
    let alpha = (num(&band["alpha_step"]) * frame as f32).min(128.0);
    let a = (alpha * 255.0 / 128.0) as u8;
    let [y0, y1] = [num(&band["y"][0]), num(&band["y"][1])];
    let c = |k: usize| num(&band["colour"][k]) as u8;
    let (black, blue) = (Color32::from_rgba_unmultiplied(0, 0, 0, a), Color32::from_rgba_unmultiplied(c(0), c(1), c(2), a));
    Art::shade(painter, v, sv([0.0, y0, W / 2.0, y1 - y0]), [black, blue, black, blue]);
    Art::shade(painter, v, sv([W / 2.0, y0, W / 2.0, y1 - y0]), [blue, black, blue, black]);
    let [cy0, cy1] = [num(&m["clip"][0]), num(&m["clip"][1])];
    let [_, top, _, bottom] = sv([0.0, cy0, 0.0, cy1 - cy0]);
    let clip = painter.with_clip_rect(Rect::from_min_max(v.pos(0.0, top), v.pos(SCREEN_W, top + bottom)));
    let rows = int(&m["rows"]);
    let n = items.len().max(1);
    let colour_of = |mode: u32| m["items"].as_array().into_iter().flatten().find(|i| int(&i["mode"]) as u32 == mode).map(|i| int(&i["colour"]));
    for k in 0..rows {
        let Some((mode, label)) = items.get((cursor + n * rows + k - rows / 2) % n) else { continue };
        // (the row's y is the text's foot; OpenKumite's entries take OPTIONS' colour)
        let y = num(&m["y"]) + num(&m["row_h"]) * k as f32 + scroll as f32 - TEXT_H;
        let new = *mode < 32 && entered >> mode & 1 == 0 && m["items"].as_array().into_iter().flatten().any(|i| int(&i["mode"]) as u32 == *mode && i["locked"] == true);
        let colour = if new && frame / 4 % 2 == 1 { Some(9) } else { colour_of(*mode).or_else(|| colour_of(OPTIONS)) };
        font_in(&clip, v, art, d, colour, label, W / 2.0, y, TEXT_SCALE, Color32::WHITE);
    }
    for (k, f) in m["fades"].as_array().into_iter().flatten().enumerate() {
        let [f0, f1] = [num(&f[0]), num(&f[1])];
        let (o, t) = (Color32::BLACK, Color32::TRANSPARENT);
        let cs = if k == 0 { [o, o, t, t] } else { [t, t, o, o] };
        Art::shade(painter, v, sv([0.0, f0, W, f1 - f0]), cs);
    }
    let arrows = &m["arrows"];
    if alpha >= 128.0 && (frame / int(&arrows["period"]).max(1) as u32) % 2 == 1 {
        for k in 0..2 {
            sprite(painter, v, art, arrows["pictures"][k].as_str().unwrap_or_default(), num(&arrows["x"][k]), num(&arrows["y"]), Color32::WHITE);
        }
    }
}

/// A text in the chrome font (`fonts.big`: its glyphs' size and rows), centred on `x`, at `scale`.
#[allow(clippy::too_many_arguments)]
fn font(painter: &egui::Painter, v: &View, art: &Art, d: &Data, text: &str, x: f32, y: f32, scale: f32, tint: Color32) {
    font_in(painter, v, art, d, None, text, x, y, [scale; 2], tint);
}

/// A text in the chrome font in text colour `colour` (the printf's `%c`: `fonts.big.colours`), else its own.
#[allow(clippy::too_many_arguments)]
fn font_in(painter: &egui::Painter, v: &View, art: &Art, d: &Data, colour: Option<usize>, text: &str, x: f32, y: f32, [sx, sy]: [f32; 2], tint: Color32) {
    let f = &d.raw["fonts"]["big"];
    let rows: Vec<&str> = f["rows"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
    let picture = colour.and_then(|c| f["colours"][c].as_str()).or(f["picture"].as_str()).unwrap_or_default();
    let Some((tex, [tw, th])) = art.pic(picture) else { return };
    let (gw, gh) = (num(&f["glyph"][0]).max(1.0), num(&f["glyph"][1]).max(1.0));
    // shortcut: the glyphs' widths are not read: the narrow ones and the space take less
    let advance = |c: char| gw * sx * if "I1!.:'- ".contains(c) { 0.42 } else { 0.78 };
    let text = text.to_uppercase().replace(['´', '`'], "'");
    let mut at = x - text.chars().map(advance).sum::<f32>() / 2.0;
    for c in text.chars() {
        // (the font has no '+', which Tekken 3's "+N SEC" wants: its '-', and the '-' turned a quarter about its middle)
        let g = if c == '+' { '-' } else { c };
        if let Some((r, k)) = rows.iter().enumerate().find_map(|(r, row)| row.chars().position(|x| x == g).map(|k| (r, k))) {
            let (u, vv) = (k as f32 * gw / tw as f32, r as f32 * gh / th as f32);
            let uv = Rect::from_min_max(Pos2::new(u, vv), Pos2::new(u + gw / tw as f32, vv + gh / th as f32));
            let [rx, ry, rw, rh] = sv([at - (gw * sx - advance(c)) / 2.0, y, gw * sx, gh * sy]);
            let cell = v.rect(rx, ry, rw, rh);
            painter.image(tex.id(), cell, uv, tint);
            if c == '+' {
                // (about the dash's ink, below and left of its cell's middle: read off the picture)
                let m = cell.center() + egui::vec2(-0.14 * cell.width(), 0.13 * cell.height());
                let turn = |p: Pos2| m + egui::vec2(m.y - p.y, p.x - m.x);
                let mut mesh = egui::Mesh::with_texture(tex.id());
                for (p, t) in [(cell.left_top(), uv.left_top()), (cell.right_top(), uv.right_top()), (cell.right_bottom(), uv.right_bottom()), (cell.left_bottom(), uv.left_bottom())] {
                    mesh.vertices.push(egui::epaint::Vertex { pos: turn(p), uv: t, color: tint });
                }
                mesh.indices.extend([0, 1, 2, 0, 2, 3]);
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        at += advance(c);
    }
}

/// A small text centred on `x`, its top at `y`: in the game's small font tinted `c` (egui's font with a pack that has
/// none).
#[allow(clippy::too_many_arguments)]
fn small(painter: &egui::Painter, v: &View, art: &Art, d: &Data, text: &str, x: f32, y: f32, c: Color32) {
    if d.raw["fonts"]["small"].is_object() {
        small_in(painter, v, art, d, None, text, x - small_width(d, text) / 2.0, y, c);
        return;
    }
    let [px, py, _, h] = sv([x, y, 0.0, 12.0]);
    painter.text(v.pos(px, py), egui::Align2::CENTER_TOP, text, egui::FontId::proportional(h * v.rect.height() / SCREEN_H), c);
}

/// A text's width in the small font (`fonts.small.advance` a glyph, whatever its ink).
fn small_width(d: &Data, text: &str) -> f32 {
    num(&d.raw["fonts"]["small"]["advance"]) * text.chars().count() as f32
}

/// A text in the small font (`fonts.small`: its glyphs' size and rows) from `x`, its top at `y`, in text colour
/// `colour` (the printf's `%c`: `fonts.small.colours`), else its own tinted `tint`.
#[allow(clippy::too_many_arguments)]
fn small_in(painter: &egui::Painter, v: &View, art: &Art, d: &Data, colour: Option<usize>, text: &str, x: f32, y: f32, tint: Color32) {
    let f = &d.raw["fonts"]["small"];
    let rows: Vec<&str> = f["rows"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
    let picture = colour.and_then(|c| f["colours"][c].as_str()).or(f["picture"].as_str()).unwrap_or_default();
    let Some((tex, [tw, th])) = art.pic(picture) else { return };
    let (gw, gh) = (num(&f["glyph"][0]).max(1.0), num(&f["glyph"][1]).max(1.0));
    let mut at = x;
    // (the font's @ is drawn as ©)
    for c in text.to_uppercase().replace('©', "@").chars() {
        if let Some((r, k)) = rows.iter().enumerate().find_map(|(r, row)| row.chars().position(|g| g == c).map(|k| (r, k))) {
            let (u, vv) = (k as f32 * gw / tw as f32, r as f32 * gh / th as f32);
            let uv = Rect::from_min_max(Pos2::new(u, vv), Pos2::new(u + gw / tw as f32, vv + gh / th as f32));
            let [rx, ry, rw, rh] = sv([at, y, gw, gh]);
            painter.image(tex.id(), v.rect(rx, ry, rw, rh), uv, tint);
        }
        at += num(&f["advance"]);
    }
}

/// The select screen as the PS2 draws it with QUICK SELECT off (0x3CCCB8): the lightning `select.lightning`, a picture a
/// frame, under the frame and its mirror; the faces (0x3CC6C8: brighter under a cursor, dark under the cursor of the
/// side that took them, RANDOM's showing its roll); each side's name plates (0x3CC3E0) and "1ST"; the timer
/// (0x3CC190, blinking in its last seconds); the cursors, pulsing (0x3CC9E0), with their tags; the screen darkening
/// to the fight. Each side's 3D characters ([`Posers`]) stand between the lightning and the rest.
/// QUICK SELECT on (and its layout in the data): the select without its lightning, frame, 3D characters or timer.
fn quick_select(d: &Data, p: &Progress) -> bool {
    p.setting(d, QUICK_ROW) == 1 && d.raw["select"]["quick"].is_object()
}

/// QUICK SELECT's own layer (`select.quick`): the mode's title centred by its width (0x3BBC40), the white lines fading
/// out under the plates (0x3BE8C0).
fn draw_quick_select(painter: &egui::Painter, v: &View, art: &Art, d: &Data, mode: u32) {
    let q = &d.raw["select"]["quick"];
    let (text, t) = (d.raw["select"]["titles"][mode as usize].as_str().unwrap_or_default(), &q["titles"][mode as usize]);
    chrome(painter, v, art, d, text, ((W - num(&t[0])) / 2.0).floor(), num(&q["title_y"]), Some(int(&t[1])), num(&q["title_size"]));
    let (l, one) = (&q["line"], matches!(mode, ONE_ARCADE | ONE_VS));
    let points = q[if mode == TEAM { "team_lines" } else { "lines" }].as_array().cloned().unwrap_or_default();
    let on = Color32::from_white_alpha((int(&l["alpha"]) * 255 / 128).min(255) as u8);
    for (k, pt) in points.iter().take(if one { 2 } else { points.len() }).enumerate() {
        let (x, y) = (num(&pt[0]), num(&pt[1]) + if one { num(&q["one_on_one_dy"]) } else { 0.0 });
        let (near, far) = (num(&l["near"]), num(&l["far"]));
        let (y0, y1) = (y + num(&l["y"][0]), y + num(&l["y"][1]));
        // (even: from 4 right of the point leftwards; odd: from 4 left of it rightwards)
        let (a, b) = if k % 2 == 0 { (x - far, x + near) } else { (x - near, x + far) };
        let cs = if k % 2 == 0 { [Color32::TRANSPARENT, on, Color32::TRANSPARENT, on] } else { [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT] };
        Art::shade(painter, v, sv([a, y0, b - a, y1 - y0]), cs);
    }
}

fn draw_select(painter: &egui::Painter, v: &View, art: &Art, d: &Data, roster: &Roster, p: &Progress, s: &Select) {
    let sel = &d.raw["select"];
    let quick = quick_select(d, p);
    if quick {
        draw_quick_select(painter, v, art, d, s.mode);
    }
    let l = &sel["lightning"];
    let pics: Vec<&str> = l["pictures"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
    // (stretched twice with the GS's bilinear filter, as the PS2 shows it)
    if let Some((tex, _)) = pics.get(s.frame as usize % pics.len().max(1)).and_then(|file| art.pic_linear(file)).filter(|_| !quick) {
        let [x, y, w, h] = sv([0, 1, 2, 3].map(|k| num(&l["rect"][k])));
        painter.image(tex.id(), Rect::from_min_max(v.pos(x, y), v.pos(x + w, y + h)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), ps2(num(&l["level"])).0);
    }
    let shown = |k: usize| -> Vec<usize> { s.shown(d, roster, p, k).into_iter().map(|q| q.0).collect() };
    let body = |i: usize| roster.characters.get(i).map_or(0, |c| c.id);
    // (the 3D characters stand in front of the lightning, behind the rest)
    crate::debug::mark_under(painter);
    let frame = sel["frame"]["picture"].as_str().unwrap_or_default();
    let [fw, fh] = [num(&sel["frame"]["size"][0]), num(&sel["frame"]["size"][1])];
    if !quick {
        image(painter, v, art, frame, [0.0, 0.0, fw, fh], Color32::WHITE);
        image(painter, v, art, frame, [W - 1.0, 0.0, -fw, fh], Color32::WHITE);
    }
    // (QUICK SELECT puts the grid 52 lower: 0x3CC6D4, 0x3CCA28)
    let dy = num(&sel["face_dy"]) + if quick { num(&sel["quick"]["face_dy"]) } else { 0.0 };
    let size = num(&sel["face_size"]);
    let level = |k: &str| num(&sel["face_level"][k]);
    for (k, slot) in d.slots.iter().enumerate() {
        if hidden(d, roster, p, k) {
            continue;
        }
        let mut face = match slot.character {
            None => d.picture(&["select", "random_face"]),
            Some(c) if d.selectable(p.stage, c) && roster.by_id(c).is_some() => d.picture(&["select", "faces", &c.to_string()]),
            Some(_) => d.picture(&["select", "locked_face"]),
        };
        let mut l = level("normal");
        if let Some(j) = (0..2).find(|&j| s.sides[j].player.is_some() && !s.sides[j].done && s.sides[j].cursor == k) {
            l = level("hover");
            if slot.character.is_some_and(|c| roster.index_of(c).is_some_and(|i| s.sides[j].picks.iter().any(|q| q.0 == i))) {
                l = level("picked");
            }
            if let (None, Some(c)) = (slot.character, s.roll(d, roster, p, j)) {
                face = d.picture(&["select", "faces", &c.to_string()]);
            }
        }
        lit(painter, v, art, &face, [slot.x, slot.y + dy, size, size], l);
    }
    for k in (0..2).filter(|&k| s.sides[k].player.is_some()) {
        let list = shown(k);
        let plates = sel["plates"][k].as_array().map_or(0, Vec::len);
        for (j, &i) in list.iter().skip(list.len().saturating_sub(plates)).enumerate() {
            let at = &sel["plates"][k][j];
            sprite(painter, v, art, &d.picture(&["hud", "plates", &body(i).to_string(), "picture"]), num(&at[0]), num(&at[1]), Color32::WHITE);
        }
        // "1ST" while the side has no pick (0x3CC660), "2ND" once `second_after` frames after a pick passed while
        // its partner's place stands nobody (0x3CB4B4); neither in 1 ON 1 (modes 5, 6; 1ST shows in 6); the number
        // big, its suffix small
        let o = &sel["ordinal"];
        let side = &s.sides[k];
        let first = side.picks.is_empty() && s.mode != ONE_ARCADE;
        let second =
            !side.picks.is_empty() && !matches!(s.mode, ONE_ARCADE | ONE_VS) && s.frame >= side.since + int(&o["second_after"]) as u32 && list.len() <= side.picks.len() && o["second"].is_string();
        if !side.done && (first || second) {
            let text = if first { o["text"].as_str() } else { o["second"].as_str() }.unwrap_or_default();
            let (n, suffix) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(0));
            // (sized and placed as the PS2 shows them: both on one foot, about y 371)
            let (x, y) = (num(&sel["ordinal"]["x"][k]), num(&sel["ordinal"]["y"]));
            font(painter, v, art, d, n, x + 27.0, y - 24.0, 1.15, Color32::WHITE);
            font(painter, v, art, d, suffix, x + 63.0, y - 2.0, 0.57, Color32::WHITE);
        }
    }
    // the timer: its seconds as (t + 59) / 60 - 1, off `off` frames every `period` in the last `below`; after the last
    // pick too (0x3CC190, as the PS2 shows it)
    let t = &sel["timer"];
    if let Some(frames) = t["frames"].as_u64().filter(|_| !quick) {
        let left = (frames as u32).saturating_sub(s.frame);
        let secs = left.div_ceil(60).saturating_sub(1);
        let b = &t["blink"];
        let (below, period, blank) = (int(&b["below"]) as u32, int(&b["period"]).max(1) as u32, int(&b["off"]) as u32);
        let off = secs < below && (left + blank) % period < blank;
        if left > 0 && !off {
            digits(painter, v, art, &d.raw["hud"]["timer"], secs, num(&t["x"]), num(&t["y"]));
        }
    }
    // the cursors, 2P's under 1P's: a level between `low` and `high`, `step` a frame up and down
    let pulse = &sel["cursor_pulse"];
    let (low, high, step) = (num(&pulse["low"]), num(&pulse["high"]), num(&pulse["step"]).max(1.0));
    let span = (high - low).max(1.0);
    let at = (s.frame as f32 * step) % (2.0 * span);
    let level = low + if at < span { at } else { 2.0 * span - at };
    let r = &sel["cursor_rect"];
    for (k, side) in s.sides.iter().enumerate().rev() {
        let (Some(player), false, Some(slot)) = (side.player, side.done, d.slots.get(side.cursor)) else { continue };
        let (x, y) = (slot.x, slot.y + dy);
        lit(painter, v, art, sel["cursors"][k].as_str().unwrap_or_default(), [x + num(&r[0]), y + num(&r[1]), num(&r[2]), num(&r[3])], level);
        let tag = if player == k { d.picture(&["select", "tags", &k.to_string()]) } else { d.picture(&["select", "tags", "6"]) };
        sprite(painter, v, art, &tag, x + num(&sel["tag_dx"][k]), y + num(&sel["tag_dy"]), Color32::WHITE);
    }
    // the side nobody plays: INSERT COIN, or the credits, while the frame has `show_mask`'s bits (`select.insert`, from
    // its left at x minus `measure` a letter halved)
    let ins = &sel["insert"];
    let f = s.frame;
    for (k, side) in s.sides.iter().enumerate() {
        if side.player.is_none() && f & int(&ins["show_mask"]) as u32 != 0 {
            let text = ins["texts"][usize::from(f & int(&ins["second_mask"]) as u32 != 0)].as_str().unwrap_or_default();
            let x = num(&ins["x"][k]) - num(&ins["measure"]) * text.chars().count() as f32 / 2.0;
            let top = num(&ins["y"]) - num(&d.raw["fonts"]["small"]["glyph"][1]) - 2.0;
            small_in(painter, v, art, d, Some(int(&ins["colour"])), text, x, top, Color32::WHITE);
        }
        if side.player.is_some() && s.mode == TEAM {
            small(painter, v, art, d, &format!("{}P  {} / 8", k + 1, side.picks.len()), if k == 0 { 120.0 } else { 520.0 }, 286.0, Color32::WHITE);
        }
    }
    if let Some(t) = s.tally {
        font(painter, v, art, d, &format!("{} - {}", t[0], t[1]), W / 2.0, 300.0, 0.5, Color32::WHITE);
    }
    // (no fade to the fight: the PS2 holds the select's last picture while the VS screen loads)
}

/// A number in the HUD timer's digits (`hud.timer`: `digits`, `digit_w`), two digits centred on `x`.
fn digits(painter: &egui::Painter, v: &View, art: &Art, t: &Value, n: u32, x: f32, y: f32) {
    let dw = num(&t["digit_w"]);
    if let Some((tex, [tw, _])) = art.pic(t["digits"].as_str().unwrap_or_default()) {
        for (k, digit) in [n.min(99) / 10, n.min(99) % 10].into_iter().enumerate() {
            let u = digit as f32 * dw / tw as f32;
            let [rx, ry, rw, rh] = sv([x - dw + dw * k as f32, y, dw, 47.0]);
            painter.image(tex.id(), v.rect(rx, ry, rw, rh), Rect::from_min_max(Pos2::new(u, 0.0), Pos2::new(u + dw / tw as f32, 1.0)), Color32::WHITE);
        }
    }
}

/// One 3D character standing on the select (`select.models`, FRONTEND.md 9): its slot (side + 2 * member), model
/// record, whether it is a pick, its body with the clips of its aliases 0 and 3, and the frames since it came or was
/// picked.
pub struct Poser {
    pub slot: usize,
    pub record: usize,
    picked: bool,
    pub character: Arc<kumite_assets::Character>,
    clips: [usize; 2],
    frame: u32,
}

/// The select's 3D characters, and the bodies loaded so far (a model record's, with its two clips).
#[derive(Default)]
pub struct Posers {
    pub shown: Vec<Poser>,
    loaded: Vec<(usize, Arc<kumite_assets::Character>, [usize; 2])>,
}

impl Posers {
    /// One frame of the select `s`: who stands where (a new one, or one just picked, from its first frame), then a frame
    /// on.
    pub fn step(&mut self, assets: &kumite_assets::Assets, d: &Data, roster: &Roster, s: &Select) {
        let progress = &progress();
        let wanted = if d.raw["select"]["models"].is_object() && !quick_select(d, progress) { s.posers(d, roster, progress) } else { vec![] };
        self.shown.retain(|m| wanted.iter().any(|w| w.0 == m.slot && w.1 == m.record));
        for (slot, record, picked) in wanted {
            match self.shown.iter_mut().find(|m| m.slot == slot) {
                Some(m) if m.picked != picked => (m.picked, m.frame) = (picked, 0),
                Some(_) => {}
                None => {
                    let id = roster.characters.iter().find(|c| c.costumes.contains(&record)).map_or(0, |c| c.id);
                    match self.load(assets, d, record, id) {
                        Ok((character, clips)) => self.shown.push(Poser { slot, record, picked, character, clips, frame: 0 }),
                        Err(e) => log::warn!("select model {record}: {e:#}"),
                    }
                }
            }
        }
        for m in &mut self.shown {
            m.frame += 1;
        }
    }

    /// Model `record` (character `id`) with its select clips (`select.models.motions[id]`), loaded once.
    fn load(&mut self, assets: &kumite_assets::Assets, d: &Data, record: usize, id: usize) -> anyhow::Result<(Arc<kumite_assets::Character>, [usize; 2])> {
        if let Some(l) = self.loaded.iter().find(|l| l.0 == record) {
            return Ok((l.1.clone(), l.2));
        }
        let names = &d.raw["select"]["models"]["motions"][id];
        let mut ch = assets.character(record)?;
        let mut clip = |k: usize| -> anyhow::Result<usize> {
            let name = names[k].as_str().ok_or_else(|| anyhow::anyhow!("no select motions for character {id}"))?;
            if ch.clip(name).is_none() {
                // (a plain clip name only: one with a folder in it would read outside `motions/`)
                anyhow::ensure!(std::path::Path::new(name).file_name() == Some(name.as_ref()), "a motion that is not a plain name: {name:?}");
                ch.add_motions(&kumite_fs::read(&assets.root.join("motions").join(format!("{name}.glb")))?)?;
            }
            ch.clip(name).ok_or_else(|| anyhow::anyhow!("motions/{name}.glb holds no clip {name}"))
        };
        let clips = [clip(0)?, clip(1)?];
        let ch = Arc::new(ch);
        self.loaded.push((record, ch.clone(), clips));
        Ok((ch, clips))
    }

    /// Each one's node matrices and placement (metres): a pick's pose (alias 3) once, then alias 0 over and over, at
    /// its slot's place (`select.models.places`, 1 ON 1's in its mode).
    pub fn poses(&self, d: &Data, mode: u32) -> Vec<(Vec<glam::Mat4>, glam::Mat4)> {
        let m = &d.raw["select"]["models"];
        let places = if mode == int(&m["one_on_one_mode"]) as u32 { &m["places_one_on_one"] } else { &m["places"] };
        self.shown
            .iter()
            .map(|p| {
                let n = |c: usize| p.character.clips.get(c).map_or(1, |c| c.frames.max(1)) as u32;
                let f = p.frame.saturating_sub(1);
                let (clip, f) = if p.picked && f < n(p.clips[1]) { (p.clips[1], f) } else { (p.clips[0], (f - if p.picked { n(p.clips[1]) } else { 0 }) % n(p.clips[0])) };
                let at = &places[p.slot];
                // (the game's x, z as the picture's x, -z, on the floor (the game sets y from the pose: the table's is not
                // kept); its 65536ths of a turn, the picture facing -z)
                let place = kumite_game::placement(glam::Vec3::new(num(&at[0]), 0.0, -num(&at[2])), std::f32::consts::PI - num(&at[3]) * std::f32::consts::TAU / 65536.0);
                (p.character.pose(clip, f as usize, false), place)
            })
            .collect()
    }
}

/// The select's camera (`select.models.camera`, 0x3AE6E0): eye, look direction and field of view, as a fight view's;
/// and the arena whose light table lights the characters (`select.models.light`).
pub fn select_camera(d: &Data) -> (glam::Vec3, glam::Vec3, f32, Option<usize>) {
    let c = &d.raw["select"]["models"]["camera"];
    let i = |v: &Value| v.as_i64().unwrap_or(0);
    // (H on Tekken Tag's 512-pixel-wide screen; the view's is on the 368-pixel one the renderer draws)
    let view = kumite_sim::camera::View { eye: [0, 1, 2].map(|k| i(&c["eye"][k])), pitch: i(&c["pitch"]), yaw: i(&c["yaw"]), h: i(&c["h"]) * 368 / 512, roll: 0 };
    let (eye, forward, fov_y) = kumite_game::view_camera(&view);
    (eye, forward, fov_y, d.raw["select"]["models"]["light"].as_u64().map(|a| a as usize))
}

/// The VS screen as the PS2 draws it (`vs`, 0x3C3FB8): the fire over the screen, each team's leader high and its
/// partner low in front, sliding `steps` frames in from the sides with their edges faded (2P's mirrored), "VS" at the
/// foot; Force's: the player and the stage card (its number and name). An older pack's: the menus' flame, OpenKumite's
/// places.
// shortcut: the arcade's STAGE n / FINAL texts and the special portraits (0x3C3948's bodies 0x32 and 0x7B, over the
// blue flame) are not drawn: "VS" for every mode, as before; TEAM BATTLE's `team_dy` is not applied
pub fn draw_vs(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, setup: &FightSetup, extra: &crate::menu::VsExtra, frame: u32) {
    let Some(d) = roster.ttt.as_deref() else { return };
    let vs = &d.raw["vs"];
    match vs["background"].as_str() {
        Some(bg) => image(painter, v, art, bg, [0.0, 0.0, W, H], Color32::WHITE),
        None => backdrop(painter, v, art),
    }
    let step = frame.min(int(&vs["steps"]) as u32) as f32;
    if let Some((number, name, _)) = &extra.force {
        font(painter, v, art, d, &number.map_or("FINAL STAGE".into(), |n| format!("STAGE {}", n + 1)), W * 0.68, 150.0, 0.9, Color32::WHITE);
        font(painter, v, art, d, &name.join(" ").split_whitespace().collect::<Vec<_>>().join(" "), W * 0.68, 210.0, 0.55, Color32::WHITE);
    }
    let [pw, ph] = [num(&vs["portrait"][0]), num(&vs["portrait"][1])];
    for k in 0..if extra.force.is_some() { 1 } else { 2 } {
        let team: Vec<(usize, usize)> = [Some(setup.players[k]), extra.bench.map(|b| b[k])].into_iter().flatten().collect();
        for (j, m) in team.iter().enumerate() {
            let id = roster.characters.get(m.0).map_or(0, |c| c.id);
            let file = d.picture(&["select", "portraits", &id.to_string()]);
            let at = &vs["places"][k][j];
            if at.is_array() {
                let (x, y) = (num(&at[0]) + num(&at[1]) * step, num(&at[2]) + num(&at[3]) * step);
                feathered(painter, v, art, &file, [x, y, pw, ph], num(&vs["feather"]), k == 1);
                continue;
            }
            // (an older pack: the leader's left edge at x 120, y 40; the partner's at x 10, y 190)
            let slide = (1.0 - frame as f32 / 12.0).max(0.0) * 200.0;
            let (off, y) = if j == 0 { (120.0 - slide, 40.0) } else { (10.0 - slide, 190.0) };
            let x = if k == 0 { off } else { W - off };
            image(painter, v, art, &file, [x, y, if k == 0 { 188.0 } else { -188.0 }, 256.0], Color32::WHITE);
        }
    }
    // not two players: the mode's texts (0x3C410C, `vs.texts`): STAGE and the fight's number, FINAL over STAGE, FIGHT
    // and the number, PRACTICE
    let t = &vs["texts"];
    if let (Some((mode, n)), true, None, true) = (extra.stage, t.is_object(), &extra.force, extra.humans < 2) {
        let has = |key: &str| ints(&t[key]).contains(&(mode as usize));
        let text = |key: &str| t[key].as_str().unwrap_or_default().to_string();
        let lines = if has("stage_modes") && n == int(&t["final_number"]) as u32 {
            [(text("final"), "final"), (text("stage"), "final")]
        } else if has("stage_modes") {
            [(text("stage"), "text"), (n.to_string(), "number")]
        } else if has("fight_modes") {
            [(text("fight"), "text"), (n.to_string(), "number")]
        } else if has("practice_modes") {
            [(String::new(), "text"), (text("practice"), "text")]
        } else {
            Default::default()
        };
        for (k, (line, scale)) in lines.iter().enumerate() {
            let s = num(&t["scales"][*scale]);
            font_in(painter, v, art, d, t["colour"].as_u64().map(|c| c as usize), line, W / 2.0, num(&t["tops"][k]), [s; 2], Color32::WHITE);
        }
        return;
    }
    if extra.force.is_none() {
        match vs["logo"].as_str().and_then(|l| art.pic(l).map(|p| (l, p.1))) {
            Some((logo, [w, _])) => sprite(painter, v, art, logo, (W - w as f32) / 2.0, num(&vs["logo_y"]), Color32::WHITE),
            None => font(painter, v, art, d, "VS", W / 2.0, 330.0, 1.2, Color32::WHITE),
        }
    }
}

/// A picture at [x, y, w, h] with its edges faded out over `edge` (0x3C3370: nine pieces, gouraud alpha 0x80 inside,
/// 0 at the border), mirrored when `flip`.
#[allow(clippy::too_many_arguments)]
fn feathered(painter: &egui::Painter, v: &View, art: &Art, file: &str, [x, y, w, h]: [f32; 4], edge: f32, flip: bool) {
    let Some((tex, _)) = art.pic(file) else { return };
    let (xs, ys) = ([0.0, edge, w - edge, w], [0.0, edge, h - edge, h]);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (r, &dy) in ys.iter().enumerate() {
        for (c, &dx) in xs.iter().enumerate() {
            let [px, py, _, _] = sv([x + dx, y + dy, 0.0, 0.0]);
            let u = dx / w;
            let inner = (1..3).contains(&r) && (1..3).contains(&c);
            let tint = if inner { Color32::WHITE } else { Color32::TRANSPARENT };
            mesh.vertices.push(egui::epaint::Vertex { pos: v.pos(px, py), uv: Pos2::new(if flip { 1.0 - u } else { u }, dy / h), color: tint });
        }
    }
    for r in 0..3u32 {
        for c in 0..3u32 {
            let i = r * 4 + c;
            mesh.indices.extend([i, i + 1, i + 4, i + 1, i + 5, i + 4]);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The tag HUD's own state (0x3C23F0, 0x3C2F40): per side and member the trail of the health it lost, which member
/// is in front, the swap's frames left after a tag, and whether its bar shows angry this frame.
#[derive(Clone, Debug, Default)]
pub struct TagHud {
    trail: [[i32; 2]; 2],
    front: [usize; 2],
    swap: [u32; 2],
    blink: [[u32; 2]; 2],
    mad: [[bool; 2]; 2],
}

impl TagHud {
    /// Per side its members' health, full health (16.16) and anger; the active one's health is its fighter's.
    fn members(bout: &Bout, i: usize) -> Vec<(i32, i32, i32, i32)> {
        let f = &bout.game.fight.fighters[i];
        match bout.game.fight.teams.as_deref().and_then(|t| t.get(i)) {
            Some(t) => (0..2)
                .map(|k| {
                    let m = &t.members[k];
                    (if k == t.active { f.health } else { m.health }, m.cap, m.max_health * kumite_sim::ONE, m.anger)
                })
                .collect(),
            None => vec![(f.health, 0, f.max_health * kumite_sim::ONE, 0)],
        }
    }

    /// A simulation step: the trails drop max / 400 a frame to the health (none at 0), the swap counts down while
    /// the round is fought (else ends), the angry bars blink 5 frames on, 3 off (0x3C2670).
    pub fn step(&mut self, bout: &Bout) {
        let angry = bout.game.fight.data.tag.angry;
        for i in 0..2 {
            for (k, (health, _, max, anger)) in Self::members(bout, i).into_iter().enumerate() {
                let t = &mut self.trail[i][k];
                *t = if health <= 0 {
                    0
                } else if *t > health {
                    (*t - max / 400).max(health)
                } else {
                    health
                };
                let b = &mut self.blink[i][k];
                if anger >= angry {
                    *b = if *b == 0 { 8 } else { *b };
                    self.mad[i][k] = *b >= 4;
                    *b -= 1;
                } else {
                    (*b, self.mad[i][k]) = (0, false);
                }
            }
            let active = bout.game.fight.teams.as_deref().and_then(|t| t.get(i)).map_or(0, |t| t.active);
            if active != self.front[i] {
                // shortcut: the swap starts as the active member changes (the PS2 holds it at 30 while 0x3D3228 says
                // the tag is under way), until a tag's frames are matched
                (self.front[i], self.swap[i]) = (active, 30);
            }
            self.swap[i] = if bout.phase == Phase::Fight { self.swap[i].saturating_sub(1) } else { 0 };
        }
    }
}

/// A text in the chrome font as the printf draws it (0x3C6C60, 0x3C69D8): from the left `x`, its foot at `y`, at
/// printf size `size` (10 the glyphs' own), each glyph `fonts.big.advance` on (an A `kern_a` further left), in text
/// colour `colour`.
#[allow(clippy::too_many_arguments)]
/// Returns the x it ends at.
fn chrome(painter: &egui::Painter, v: &View, art: &Art, d: &Data, text: &str, x: f32, y: f32, colour: Option<usize>, size: f32) -> f32 {
    let f = &d.raw["fonts"]["big"];
    let rows: Vec<&str> = f["rows"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
    let Some((tex, [tw, th])) = art.pic(colour.and_then(|c| f["colours"][c].as_str()).or(f["picture"].as_str()).unwrap_or_default()) else { return x };
    let (gw, gh, s) = (num(&f["glyph"][0]).max(1.0), num(&f["glyph"][1]).max(1.0), size / 10.0);
    let (per_row, top) = (rows.first().map_or(1, |r| r.chars().count()), y + num(&f["foot_dy"]));
    let mut at = x;
    for c in text.chars() {
        let place = rows.iter().enumerate().find_map(|(r, row)| row.chars().position(|g| g == c).map(|k| (r, k)));
        if c == 'A' {
            at -= num(&f["kern_a"]) * s;
        }
        if let Some((r, k)) = place.filter(|_| c != ' ') {
            let (u, vv) = (k as f32 * gw / tw as f32, r as f32 * gh / th as f32);
            let uv = Rect::from_min_max(Pos2::new(u, vv), Pos2::new(u + gw / tw as f32, vv + gh / th as f32));
            let [rx, ry, rw, rh] = sv([at, top, gw * s, gh * s]);
            painter.image(tex.id(), v.rect(rx, ry, rw, rh), uv, Color32::WHITE);
        }
        let (r, k) = place.unwrap_or((0, 0));
        at += num(&f["advance"][r * per_row + k]) * s;
    }
    at
}

/// A round text of `hud.banners` (its format with `n`).
fn banner(painter: &egui::Painter, v: &View, art: &Art, d: &Data, key: &str, n: u32) {
    banner_at(painter, v, art, d, &d.raw["hud"]["banners"][key], n);
}

/// A chrome text `{text, x, y, colour, size}` (its format with `n`; size 10 when not given).
fn banner_at(painter: &egui::Painter, v: &View, art: &Art, d: &Data, b: &Value, n: u32) {
    let text = crate::art::printf(b["text"].as_str().unwrap_or_default(), &[n]);
    chrome(painter, v, art, d, &text, num(&b["x"]), num(&b["y"]), Some(int(&b["colour"])), b["size"].as_f64().map_or(10.0, |s| s as f32));
}

/// The help line (0x3E5268 -> 0x3E06A0, roster.json `front_end.help`): the dark band (0x3DBCF0), then help text `n`
/// in the help font from (`x`, `y`) at colour `level`, each glyph its `widths` on (a space `space`), `\n` a line
/// `line_h` lower, of the `\S[...]` alternatives only the `value`-th (gp-0x67BA).
/// shortcut: a line wider than `wrap` is not broken (0x3DF808); the texts drawn so far fit.
fn help_line(painter: &egui::Painter, v: &View, art: &Art, d: &Data, n: usize, value: usize) {
    if let Some(text) = d.raw["help"]["texts"][n].as_str() {
        help_text(painter, v, art, d, text, value);
    }
}

/// A help line's text (with its codes) as [`help_line`] draws it.
fn help_text(painter: &egui::Painter, v: &View, art: &Art, d: &Data, text: &str, value: usize) {
    let h = &d.raw["help"];
    let f = &h["font"];
    let Some((tex, [tw, th])) = art.pic(f["picture"].as_str().unwrap_or_default()) else { return };
    let band = &h["band"];
    let r = &band["rect"];
    Art::shade(painter, v, sv([num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])]), [rgba(&band["colour"]); 4]);
    let (gw, gh, cols, first) = (num(&f["glyph"][0]), num(&f["glyph"][1]), int(&f["columns"]).max(1), int(&f["first"]) as u32);
    let tint = ps2(num(&h["level"])).0;
    let (mut x, mut y, mut alt, mut chars) = (num(&h["x"]), num(&h["y"]), -1i64, text.chars());
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => (x, y) = (num(&h["x"]), y + num(&f["line_h"])),
                Some('S') => alt += 1,
                _ => {}
            }
            continue;
        }
        if alt >= 0 && alt != value as i64 {
            continue;
        }
        let g = (ch as u32).saturating_sub(first) as usize;
        if ch != ' ' {
            let (u, w) = ((g % cols) as f32 * gw / tw as f32, (g / cols) as f32 * gh / th as f32);
            let uv = Rect::from_min_max(Pos2::new(u, w), Pos2::new(u + gw / tw as f32, w + gh / th as f32));
            let [rx, ry, rw, rh] = sv([x, y, gw, gh]);
            painter.image(tex.id(), v.rect(rx, ry, rw, rh), uv, tint);
        }
        x += if ch == ' ' { num(&f["space"]) } else { num(&f["widths"][g]) };
    }
}

/// Practice's menu (`practice`, START in a practice fight): open, the mode (FREESTYLE, VS CPU, CHAIN TRAINING, COMBO
/// TRAINING), the cursor's row, each mode's values by row.
#[derive(Clone, Debug, Default)]
pub struct Practice {
    pub open: bool,
    pub mode: usize,
    pub cursor: usize,
    values: Vec<Vec<usize>>,
    /// The damage each side took, and the last hit's on the dummy (damage, base damage).
    total: [i32; 2],
    last: (i32, i32),
}

/// What a practice menu key asks of the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeAction {
    None,
    /// The menu closed: the dummy as set (VS CPU's CPU).
    Close,
    /// CHARACTER SELECT: back to practice's select.
    Select,
    /// RESET: the main menu (the soft reset).
    Reset,
}

impl Practice {
    pub fn new(d: &Data) -> Self {
        let modes = d.raw["practice"]["modes"].as_array().cloned().unwrap_or_default();
        Self { values: modes.iter().map(|m| m["rows"].as_array().into_iter().flatten().map(|r| int(&r["default"])).collect()).collect(), ..Self::default() }
    }

    fn rows<'a>(&self, d: &'a Data) -> &'a [Value] {
        d.raw["practice"]["modes"][self.mode]["rows"].as_array().map_or(&[], Vec::as_slice)
    }

    /// The value of the current mode's row `label`.
    fn value(&self, d: &Data, label: &str) -> Option<usize> {
        let k = self.rows(d).iter().position(|r| r["label"] == label)?;
        self.values.get(self.mode)?.get(k).copied()
    }

    /// FREESTYLE's TRAINING DUMMY value (0 STAND elsewhere; 5 AUTO GUARD, 11 CONTROLLER).
    pub fn dummy(&self, d: &Data) -> usize {
        self.value(d, "TRAINING DUMMY").unwrap_or(0)
    }

    /// VS CPU: the CPU's difficulty (CPU LEVEL).
    pub fn cpu(&self, d: &Data) -> Option<usize> {
        self.value(d, "CPU LEVEL")
    }

    /// The hits of the last step: each side's total, the last hit on the dummy.
    pub fn hits(&mut self, hits: &[kumite_sim::HitEvent]) {
        for h in hits.iter().filter(|h| !h.blocked && h.damage > 0) {
            if let Some(t) = self.total.get_mut(h.victim) {
                *t += h.damage;
            }
            if h.victim == 1 {
                self.last = (h.damage, h.base_damage);
            }
        }
    }

    pub fn open(&mut self, d: &Data) {
        sfx::play(sound(d, "decide"));
        (self.open, self.cursor) = (true, 0);
    }

    /// A key while the menu is open: up / down the rows, left / right MODE SELECT's modes or a row's values (round),
    /// a button takes CANCEL, CHARACTER SELECT or RESET; back closes.
    pub fn key(&mut self, d: &Data, key: Key) -> PracticeAction {
        let n = self.rows(d).len().max(1);
        let modes = d.raw["practice"]["modes"].as_array().map_or(1, Vec::len).max(1);
        let row = self.rows(d).get(self.cursor).cloned().unwrap_or_default();
        let kind = row["kind"].as_str().unwrap_or_default();
        match key {
            Key::Up | Key::Down => {
                sfx::play(sound(d, "cursor"));
                self.cursor = (self.cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
            }
            Key::Left | Key::Right if kind == "mode" => {
                sfx::play(sound(d, "cursor"));
                self.mode = (self.mode + if key == Key::Left { modes - 1 } else { 1 }) % modes;
            }
            Key::Left | Key::Right => {
                let len = row["values"].as_array().map_or(0, Vec::len);
                if let (true, Some(v)) = (len > 1, self.values.get_mut(self.mode).and_then(|m| m.get_mut(self.cursor))) {
                    sfx::play(sound(d, "cursor"));
                    *v = (*v + if key == Key::Left { len - 1 } else { 1 }) % len;
                }
            }
            Key::Back => {
                self.open = false;
                return PracticeAction::Close;
            }
            Key::Start | Key::Button(_) => {
                let action = match kind {
                    "close" => PracticeAction::Close,
                    "select" => PracticeAction::Select,
                    "reset" => PracticeAction::Reset,
                    _ => return PracticeAction::None,
                };
                sfx::play(sound(d, "decide"));
                self.open = false;
                return action;
            }
        }
        PracticeAction::None
    }
}

/// The practice dummy's pad by TRAINING DUMMY (`dummy`: its value; 0x37DC98 writes the dummy's pad +0x430): lying,
/// it holds up to get up (0x37DDE0); else STAND GUARD holds back while the player's move hits high or mid and is not
/// past its last active frame (0x37E010), CROUCH holds down (0x37DFF0), CROUCH GUARD down and down-back against a
/// low (0x37E180), GUARD ALL down-back against a low and back against a high or mid (0x37E090). AUTO GUARD stands
/// guarding without the input (+0x12C: the caller's guard input flag). The recoveries (QUICK ROLL 3+4 into the
/// foreground, 1+2 into the background, QUICK TAG OUT the tag button, SPRING FORWARD forward, QUICK ROLL BACK back)
/// in a juggle reaction (the move's posture class 9 / 10, 0x37E0E8) press theirs on a frame it is taken (0x37E1D8).
/// shortcut: "juggled" is the posture class alone (the PS2 also needs +0xF7, set by the hit; not tracked here).
pub fn practice_pad(dummy: usize, f: &kumite_sim::Fight, me: usize) -> kumite_sim::input::Pad {
    use kumite_sim::input::{Pad, TAG};
    let (gd, d, p) = (&f.data, &f.fighters[me], &f.fighters[1 - me]);
    let right = f.facing_right(me);
    let dir = |fwd: bool, down: bool| Pad { down, right: fwd == right, left: fwd != right, ..Pad::default() };
    if (6..=10).contains(&dummy) && matches!((d.current_move(gd).posture.raw >> 11) & 0x1F, 9 | 10) {
        let pad = match dummy {
            6 => Pad { buttons: 0xC, ..Pad::default() },
            7 => Pad { buttons: 0x3, ..Pad::default() },
            8 => Pad { buttons: TAG, ..Pad::default() },
            9 => dir(true, false),
            _ => dir(false, false),
        };
        return if d.accepts(gd, pad, right) { pad } else { Pad::default() };
    }
    if d.current_move(gd).posture.raw & 0x204 != 0 {
        return Pad { up: true, ..Pad::default() };
    }
    // (the player's attack guarded standing (mask 0x10) or crouching (0x08), not past its last active frame)
    let pm = p.current_move(gd);
    let hits = |bit: u32| pm.attack_mask.mask & bit != 0 && p.frame <= pm.active[1];
    match dummy {
        1 if hits(0x10) => dir(false, false),
        3 | 4 if hits(0x08) => dir(false, true),
        2 | 3 => Pad { down: true, ..Pad::default() },
        4 if hits(0x10) => dir(false, false),
        _ => Pad::default(),
    }
}

/// Practice's fight (`practice.hud`): with ATTACK DATA on, each side's TOTAL DMG and the last hit's DMG on the dummy
/// with its share of the move's damage; the mode's name.
pub fn draw_practice_hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, p: &Practice) {
    let h = &d.raw["practice"]["hud"];
    if !h.is_object() {
        return;
    }
    let at = |t: &Value, text: &str| {
        chrome(painter, v, art, d, text, num(&t["xy"][0]), num(&t["xy"][1]), Some(int(&t["colour"])), num(&t["size"]));
    };
    if p.value(d, "ATTACK DATA") == Some(1) {
        let t = &h["total"];
        for (k, total) in p.total.iter().enumerate() {
            chrome(painter, v, art, d, &format!("TOTAL DMG:{total}"), num(&t["x"][k]), num(&t["y"]), Some(int(&t["colour"])), num(&t["size"]));
        }
        let (dmg, base) = p.last;
        at(&h["damage"], &format!("DMG:{dmg} ({:03}%)", if base > 0 { dmg * 100 / base } else { 0 }));
    }
    at(&h["mode"], d.raw["practice"]["modes"][p.mode]["title"].as_str().unwrap_or_default());
    // FREESTYLE's COMMAND CAPTURE (0x37FE58) and its keys: SELECT, then the small font's "+", the circle button and
    // "=REC" (the small font's top 20 over the foot). shortcut: recording and playing it back are not done
    let c = &h["capture"];
    if p.mode == 0 && c.is_object() {
        at(c, c["text"].as_str().unwrap_or_default());
        let k = &c["keys"];
        let y = num(&k["xy"][1]);
        let mut x = chrome(painter, v, art, d, k["select"].as_str().unwrap_or_default(), num(&k["xy"][0]), y, Some(int(&k["colour"])), num(&k["size"]));
        for part in k["small"].as_array().into_iter().flatten() {
            let text = part[0].as_str().unwrap_or_default();
            if text == "o" {
                let cell = int(&k["button"]) as f32 * 16.0;
                if let Some((tex, [tw, th])) = art.pic("003_003.png") {
                    let uv = Rect::from_min_max(Pos2::new(cell / tw as f32, 0.0), Pos2::new((cell + 16.0) / tw as f32, 16.0 / th as f32));
                    let [rx, ry, rw, rh] = sv([x, y - 20.0, 16.0, 16.0]);
                    painter.image(tex.id(), v.rect(rx, ry, rw, rh), uv, Color32::WHITE);
                }
            } else {
                small_in(painter, v, art, d, Some(int(&part[1])), text, x, y - 20.0, Color32::WHITE);
            }
            x += small_width(d, text);
        }
    }
}

/// Practice's menu (`practice`) in the pause's window: the mode's name, its rows in their colours (the cursor's over
/// the pause's bar, from its value's line where it has one), the values right-aligned (ON / OFF in GAME OPTIONS'
/// default / off colours), the arrow after MODE SELECT or the cursor's value, the help line (the mode's, the row's
/// or its value's). `name`: the player's character (COMBO DISPLAY's "%s (YOU)").
pub fn draw_practice(painter: &egui::Painter, v: &View, art: &Art, d: &Data, p: &Practice, name: &str) {
    let pr = &d.raw["practice"];
    let rows = p.rows(d);
    let (x0, y0, w) = (num(&pr["window"][0]), num(&pr["window"][1]), num(&pr["window"][2]));
    let last = rows.iter().map(|r| num(&r["value_y"]).max(num(&r["y"]))).fold(0.0, f32::max);
    let h = last + num(&pr["below"]) - y0;
    for r in [[x0 - 10.0, y0 - 10.0, w + 20.0, 4.0], [x0 - 10.0, y0 + h + 6.0, w + 20.0, 4.0], [x0 - 10.0, y0 - 6.0, 4.0, h + 12.0], [x0 + w + 6.0, y0 - 6.0, 4.0, h + 12.0]] {
        Art::shade(painter, v, sv(r), [Color32::from_gray(195); 4]);
    }
    let (top, foot) = (rgba(&pr["colour"][0]), rgba(&pr["colour"][1]));
    Art::shade(painter, v, sv([x0 - 6.0, y0 - 6.0, w + 12.0, h + 12.0]), [top, top, foot, foot]);
    let mode = &pr["modes"][p.mode];
    chrome(painter, v, art, d, mode["title"].as_str().unwrap_or_default(), num(&pr["title_xy"][0]), num(&pr["title_xy"][1]), Some(int(&pr["title_colour"])), num(&pr["title_size"]));
    let (x, size, right) = (num(&pr["x"]), num(&pr["size"]), num(&pr["value_right"]));
    let colour = |k: &str| Some(int(&pr["colours"][k]));
    let measure = |t: &str| chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, t, 0.0, 0.0, None, size);
    let arrow = |ax: f32, y: f32| {
        let pic = d.raw["menu"]["arrows"]["pictures"][1].as_str().unwrap_or_default();
        let ah = art.pic(pic).map_or(0.0, |q| q.1[1] as f32);
        sprite(painter, v, art, pic, ax, y - ah, Color32::WHITE);
    };
    for (k, r) in rows.iter().enumerate() {
        let (label, y, on) = (r["label"].as_str().unwrap_or_default(), num(&r["y"]), k == p.cursor);
        let vy = r["value_y"].as_f64().map_or(y, |v| v as f32);
        let value = r["values"][p.values.get(p.mode).and_then(|m| m.get(k)).copied().unwrap_or(0)].as_str();
        if on {
            bar(painter, v, d, [x0, x, right], y);
        }
        let end = chrome(painter, v, art, d, label, x, y, if on { colour("cursor") } else { colour("item") }, size);
        if let Some(t) = value {
            let c = match t {
                "ON" => colour("on"),
                "OFF" => colour("off"),
                _ => colour("value"),
            };
            chrome(painter, v, art, d, t, right - measure(t), vy, c, size);
            for (i, l) in r["lines"].as_array().into_iter().flatten().enumerate() {
                let l = l.as_str().unwrap_or_default().replace("%s", &name.to_uppercase());
                chrome(painter, v, art, d, &l, right - measure(&l), vy - 21.0 * (i + 1) as f32, colour("value"), size);
            }
            if on {
                arrow(right + 2.0, vy);
            }
        } else if on && r["kind"] == "mode" {
            arrow(end + 2.0, y);
        }
    }
    // (the mode's line, then the row's or its value's)
    if progress().setting(d, HELP_ROW) != 0 {
        let texts = &d.raw["help"]["texts"];
        let r = rows.get(p.cursor).cloned().unwrap_or_default();
        let first = rows.first().map_or(0, |m| int(&m["help"]));
        let second = match (r["kind"].as_str(), r["value_help"].as_u64()) {
            (Some("mode"), _) => first + 1,
            (_, Some(base)) => {
                let v = p.values.get(p.mode).and_then(|m| m.get(p.cursor)).copied().unwrap_or(0);
                // (a value without its own, CONTROLLER: the row's)
                if v < int(&r["value_helps"]) {
                    base as usize + v
                } else {
                    int(&r["help"])
                }
            }
            _ => int(&r["help"]),
        };
        let text = format!("{}\\n{}", texts[first].as_str().unwrap_or_default(), texts[second].as_str().unwrap_or_default());
        help_text(painter, v, art, d, &text, 0);
    }
}

/// A PS2 RGBA `[r, g, b, a]` (alpha 0x80 opaque).
fn rgba(c: &Value) -> Color32 {
    let k = |i: usize| int(&c[i]).min(255) as u8;
    Color32::from_rgba_unmultiplied(k(0), k(1), k(2), (int(&c[3]) * 255 / 128).min(255) as u8)
}

/// The pause menu's entries (0x3B7B98: `front_end.pause.items` by number): CANCEL, RESET, COMMAND LIST, ADJUST
/// DISPLAY, CONTROLLER SETUP. SCREEN CAPTURE (3) shows once the Gallery is open (no Gallery here); Tekken Bowl's
/// (mode 8) has TOP BOWLERS and BOWLING OPTIONS (4, 5) instead of COMMAND LIST and SCREEN CAPTURE.
pub const PAUSE_ITEMS: [usize; 5] = [0, 1, 2, 6, 7];
pub const BOWL_PAUSE_ITEMS: [usize; 6] = [0, 1, 4, 5, 6, 7];

/// The pause entries OpenKumite does not have, drawn in the game's colour for one unavailable (`colours.off`).
/// shortcut: COMMAND LIST (no move list), ADJUST DISPLAY and CONTROLLER SETUP (OpenKumite's GRAPHICS and KEY
/// CONFIGURATION are in OPTIONS) do nothing in a fight.
fn pause_off(item: usize) -> bool {
    matches!(item, 2 | 6 | 7)
}

/// What a key does to the pause menu (0x3B7B98): up / down move the cursor round the entries (sound 3); a button takes
/// the entry (CANCEL closes, sound 3; RESET opens RESET?, sound 3); START and back close it. RESET? (`reset`: YES
/// chosen): left / right choose (sound 3); a button takes it (YES: the reset, sound 4; NO: back to the entries, sound
/// 3); START and back close it. True: the game is reset.
pub fn pause_key(d: &Data, items: &[usize], pause: &mut Option<(usize, usize)>, reset: &mut Option<bool>, key: Key) -> bool {
    let Some((cursor, _)) = pause.as_mut() else { return false };
    let n = items.len();
    match (reset.as_mut(), key) {
        (Some(yes), Key::Left | Key::Right) => {
            sfx::play(sound(d, "cursor"));
            *yes = !*yes;
        }
        (Some(true), Key::Button(_)) => {
            sfx::play(sound(d, "start"));
            (*pause, *reset) = (None, None);
            return true;
        }
        (Some(_), Key::Button(_) | Key::Start | Key::Back) => {
            sfx::play(sound(d, "cursor"));
            *reset = None;
        }
        (None, Key::Up | Key::Down) => {
            sfx::play(sound(d, "cursor"));
            *cursor = (*cursor + if key == Key::Up { n - 1 } else { 1 }) % n;
        }
        (None, Key::Button(_)) => match items.get(*cursor) {
            Some(0) => {
                sfx::play(sound(d, "cursor"));
                *pause = None;
            }
            Some(1) => {
                sfx::play(sound(d, "cursor"));
                *reset = Some(false);
            }
            _ => {}
        },
        (None, Key::Start | Key::Back) => {
            sfx::play(sound(d, "cursor"));
            *pause = None;
        }
        _ => {}
    }
    false
}

/// The pause menu as the PS2 draws it (0x3B7B98, `front_end.pause`): the window (0x3DC768: navy over the scene in a
/// grey frame, 8 out), "PAUSE Pn" in the chrome font while the frame's 0x30 bits are on, the entries `item_dy` apart
/// (the cursor's in `colours.cursor` over its bar, 0x3BB680: green fading out to the right, `bar.dx` past the text),
/// the cursor entry's help line (HELP MESSAGES on); or RESET? alone with YES / NO (the chosen gold, the other grey), the arrow towards
/// the choice and its help line.
#[allow(clippy::too_many_arguments)]
pub fn draw_pause(painter: &egui::Painter, v: &View, art: &Art, d: &Data, items: &[usize], cursor: usize, player: usize, reset: Option<bool>, frame: u32) {
    let p = &d.raw["pause"];
    let help = progress().setting(d, HELP_ROW) != 0;
    let window = |r: &Value| {
        let [x, y, w, h] = [num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])];
        // (the frame 4 wide, 10 out)
        for r in [[x - 10.0, y - 10.0, w + 20.0, 4.0], [x - 10.0, y + h + 6.0, w + 20.0, 4.0], [x - 10.0, y - 6.0, 4.0, h + 12.0], [x + w + 6.0, y - 6.0, 4.0, h + 12.0]] {
            Art::shade(painter, v, sv(r), [Color32::from_gray(195); 4]);
        }
        Art::shade(painter, v, sv([x - 6.0, y - 6.0, w + 12.0, h + 12.0]), [rgba(&p["colour"]); 4]);
    };
    let size = num(&p["size"]);
    let colour = |k: &str| Some(int(&p["colours"][k]));
    if let Some(yes) = reset {
        let r = &p["reset"];
        window(&r["window"]);
        chrome(painter, v, art, d, r["text"].as_str().unwrap_or_default(), num(&r["xy"][0]), num(&r["xy"][1]), Some(int(&r["colour"])), size);
        let (yes_c, no_c) = if yes { (colour("cursor"), colour("off")) } else { (colour("off"), colour("cursor")) };
        let (x, y) = (num(&r["yes_no_xy"][0]), num(&r["yes_no_xy"][1]));
        let end = chrome(painter, v, art, d, &format!("{}  ", r["yes_no"][0].as_str().unwrap_or_default()), x, y, yes_c, size);
        chrome(painter, v, art, d, r["yes_no"][1].as_str().unwrap_or_default(), end, y, no_c, size);
        // (the printf's "{" / "}" in its default font: the main menu's arrows, as the PS2 shows them)
        let arrow = d.raw["menu"]["arrows"]["pictures"][usize::from(!yes)].as_str().unwrap_or_default();
        let ah = art.pic(arrow).map_or(0.0, |p| p.1[1] as f32);
        sprite(painter, v, art, arrow, num(&r["arrow_xy"][0]), num(&r["arrow_xy"][1]) - ah, Color32::WHITE);
        if help {
            help_line(painter, v, art, d, int(&r["help"]), 0);
        }
        return;
    }
    // (six entries: the window `h_capture` high)
    let mut w = p["window"].clone();
    if items.len() > PAUSE_ITEMS.len() {
        w[3] = p["h_capture"].clone();
    }
    window(&w);
    if frame & 0x30 != 0 {
        let text = crate::art::printf(p["header"].as_str().unwrap_or_default(), &[player as u32 + 1]);
        chrome(painter, v, art, d, &text, num(&p["header_xy"][0]), num(&p["header_xy"][1]), Some(0), num(&p["header_size"]));
    }
    let (x, y0, dy) = (num(&p["item_xy"][0]), num(&p["item_xy"][1]), num(&p["item_dy"]));
    for (k, &item) in items.iter().enumerate() {
        let label = p["items"][item].as_str().unwrap_or_default();
        let y = y0 + dy * k as f32;
        if k == cursor {
            // (the text measured first, as the printf's %T)
            let bar = &p["bar"];
            let w = chrome(&painter.with_clip_rect(Rect::NOTHING), v, art, d, label, x, y, None, size) - x + num(&bar["dx"]);
            let (on, b0, b1) = (rgba(&bar["colour"]), num(&bar["y"][0]), num(&bar["y"][1]));
            Art::shade(painter, v, sv([x, y + b0, w, b1 - b0]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
        }
        let c = if k == cursor {
            colour("cursor")
        } else if pause_off(item) {
            colour("off")
        } else {
            colour("item")
        };
        chrome(painter, v, art, d, label, x, y, c, size);
    }
    if help {
        help_line(painter, v, art, d, int(&p["help"]) + items.get(cursor).copied().unwrap_or(0), 0);
    }
}

/// The fight HUD (0x3C2F40, 0x3C23F0, 0x3C1F08, 0x3BC540; FRONTEND.md 4): per side the bar of the member in back
/// (15 up and out, darker) and over it the one in front (`TagHud`: they swap after a tag), each from its inner end
/// health, the trail of what it lost, the recoverable cap and empty, its strips angry while the member is; the front
/// member's name plate on its streak; the round lamps; the timer's digits; in the ladder modes the player's STAGE and
/// the run's time (`run`, frames), the other side's INSERT COIN (Time Attack's label); the round's texts by its state
/// (0x36EA88): ROUND n and READY! in the call, the result while it ends and its replay plays, YOU WIN! / YOU LOSE in
/// the pose.
#[allow(clippy::too_many_arguments)]
pub fn hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, bout: &Bout, info: &crate::hud::HudInfo, tag: &TagHud, result: Option<Announcement>, frame: u32, run: u32) {
    let h = &d.raw["hud"];
    let fight = &bout.game.fight;
    if !matches!(bout.phase, Phase::Replay { .. }) && !info.hidden {
        let b = &h["bars"];
        let (bx, by, bw, bh, cap) = ([num(&b["x"][0]), num(&b["x"][1])], num(&b["y"]), num(&b["w"]), num(&b["h"]), num(&b["cap_w"]));
        let strip = |k: usize, mad: bool| h[if mad { "angry_strips" } else { "strips" }][k].as_str().unwrap_or_default().to_string();
        for i in 0..2 {
            let members = TagHud::members(bout, i);
            let front = tag.front[i].min(members.len() - 1);
            let n = (tag.swap[i] / 2) as f32;
            // (the member in back first, then the one in front)
            let order: Vec<(usize, f32)> = if members.len() > 1 { vec![(1 - front, num(&b["back"]) - n), (front, n)] } else { vec![(0, 0.0)] };
            for (k, off) in order {
                let (health, cap_h, max, _) = members[k];
                let (is_front, mad) = (k == front, tag.mad[i][k]);
                let level = if is_front {
                    num(&b["levels"][1])
                } else if mad {
                    num(&b["angry_level"])
                } else {
                    num(&b["levels"][0])
                };
                let (y, inner) = (by - off, if i == 0 { bx[0] + bw - off } else { bx[1] + off });
                if is_front {
                    // the name plate (0x3C1DA0) on its streak, added (draw mode 4)
                    let body = fight.teams.as_deref().and_then(|t| t[i].members[k].character).or(fight.fighters[i].character).unwrap_or(0).max(0) as usize;
                    let plate = &h["plates"][body];
                    let (px, py) = (num(&h["plate_x"][i]) + if i == 0 { -off } else { off }, num(&h["plate_y"]) - off);
                    let streak = h["streaks"][int(&plate["streak"])].as_str().unwrap_or_default();
                    image(painter, v, art, streak, [px, py, num(&h["streak_size"][0]), num(&h["streak_size"][1])], additive(Color32::WHITE));
                    sprite(painter, v, art, plate["picture"].as_str().unwrap_or_default(), px, py, Color32::WHITE);
                }
                // widths from the inner end: health, the trail past it, the recoverable cap
                let w = |x: i32| bw * x.max(0) as f32 / max.max(1) as f32;
                let (wh, wc, wt) = (w(health), w(cap_h).max(w(health)), w(tag.trail[i][k] - health));
                // a part [from, to) of the bar, counted from its inner end
                let part = |s: usize, from: f32, to: f32| {
                    let (a, z) = (from.clamp(0.0, bw), to.clamp(0.0, bw));
                    if z > a {
                        let x = if i == 0 { inner - z } else { inner + a };
                        lit(painter, v, art, &strip(s, mad), [x, y, z - a, bh], level);
                    }
                };
                let (left, right) = if i == 0 { (inner - bw, inner) } else { (inner, inner + bw) };
                lit(painter, v, art, &strip(3, mad), [left - cap, y, cap, bh], level);
                lit(painter, v, art, &strip(4, mad), [right, y, cap, bh], level);
                part(2, wc, bw);
                part(5, wh, wc);
                part(0, 0.0, wh);
                part(1, wh, wh + wt);
            }
            // the round lamps: one a round to win when 2 or more (a new win blinks 180 frames, 11-frame period); 8
            // higher without a partner
            let l = &h["lamps"];
            if bout.rounds_to_win >= 2 {
                let y = num(&l["y"]) + if members.len() > 1 { 0.0 } else { num(&l["solo_dy"]) };
                for k in 0..bout.rounds_to_win {
                    let x = if i == 0 { num(&l["x"][0]) - num(&l["dx"]) * k as f32 } else { num(&l["x"][1]) + num(&l["dx"]) * k as f32 };
                    let lit = k < bout.wins[i]
                        && !(k + 1 == bout.wins[i] && bout.phase_frame < 180 && bout.phase_frame / 11 % 2 == 1 && matches!(bout.phase, Phase::RoundOver { .. } | Phase::Result { .. }));
                    sprite(painter, v, art, l[if lit { "lit" } else { "unlit" }].as_str().unwrap_or_default(), x, y, Color32::WHITE);
                }
            }
        }
        // the timer (0x3C6738): seconds (t * 6 / 7 - 1) / 60, the tens 39 left of the ones; INFINITY untimed
        let t = &h["timer"];
        let y = num(&t["y"]);
        if bout.game.round_frames > 100 * 70 {
            sprite(painter, v, art, t["infinity"].as_str().unwrap_or_default(), num(&t["infinity_x"]), y, Color32::WHITE);
        } else {
            let (dw, secs) = (num(&t["digit_w"]), (fight.clock * 6 / 7).saturating_sub(1) / 60);
            for (k, digit) in [secs.min(99) / 10, secs.min(99) % 10].into_iter().enumerate() {
                if let Some((tex, [tw, _])) = art.pic(t["digits"].as_str().unwrap_or_default()) {
                    let u = digit as f32 * dw / tw as f32;
                    let [rx, ry, rw, rh] = sv([num(&t["x"]) + if k == 0 { num(&t["tens_dx"]) } else { 0.0 }, y, dw, 47.0]);
                    painter.image(tex.id(), v.rect(rx, ry, rw, rh), Rect::from_min_max(Pos2::new(u, 0.0), Pos2::new(u + dw / tw as f32, 1.0)), Color32::WHITE);
                }
            }
        }
        // the ladder modes (0x3BC048): the player's STAGE (FINAL from the last fight, every 32 frames) and the run's
        // time; the other side INSERT COIN / CREDIT, or Time Attack's label
        let top = &h["top"];
        if let (Some(stage), Some(me)) = (info.stage, (0..2).find(|&i| info.humans[i] && !info.humans[1 - i])) {
            let (y, small_h) = (num(&top["y"]), num(&d.raw["fonts"]["small"]["glyph"][1]));
            let text_at = |text: &str, x: f32, colour: &str| small_in(painter, v, art, d, Some(int(&top[colour])), text, x, y - small_h - 2.0, Color32::WHITE);
            let ins = &d.raw["select"]["insert"];
            // (INSERT COIN's blinking and alternation, centred on x)
            let blink = |texts: [&str; 2], x: f32| {
                if frame & int(&ins["show_mask"]) as u32 != 0 {
                    let text = texts[usize::from(frame & int(&ins["second_mask"]) as u32 != 0)];
                    text_at(text, x - num(&ins["measure"]) * text.chars().count() as f32 / 2.0, "insert_colour");
                }
            };
            let (other, s) = (1 - me, |v: &Value| v.as_str().unwrap_or_default().to_string());
            if info.under[me] == crate::hud::Under::Push {
                // under CONTINUE?: PRESS Pn START and the credits on the player's side, nothing on the other
                blink([&s(&top["press"][me]), &s(&top["free_play"])], num(&top["insert_x"][me]));
            } else {
                let last = stage >= int(&d.raw["arcade"]["fights"]) as u32;
                let label = if last && frame & 0x20 == 0 { s(&top["final"]) } else { crate::art::printf(&s(&top["stage"]), &[stage]) };
                text_at(&label, num(&top["stage_x"][me]), "stage_colour");
                text_at(&clock(run), num(&top["stage_x"][me]) + num(&top["time_dx"]), "time_colour");
                if info.time_base.is_some() {
                    text_at(&s(&top["time_attack"]), num(&top["label_x"][other]), "label_colour");
                } else {
                    blink([&s(&ins["texts"][0]), &s(&ins["texts"][1])], num(&top["insert_x"][other]));
                }
            }
        }
    }
    // (Force: FIGHT! and K.O. only, OpenKumite's; its stage card instead of the round, STAGE CLEAR!! instead of the
    // result)
    if info.no_result {
        if let Some(call) = crate::hud::call(bout, info, result).filter(|_| matches!(bout.phase, Phase::Fight | Phase::RoundOver { .. })) {
            font(painter, v, art, d, &call, W / 2.0, 190.0, 0.9, Color32::WHITE);
        }
        return;
    }
    // the round's texts
    let rounds = bout.game.rounds.as_ref();
    let later = usize::from(bout.round > 1);
    let f = bout.phase_frame;
    match bout.phase {
        Phase::Call if info.intro.is_none() => {
            if rounds.is_some_and(|r| f >= r.ready_from[later]) {
                banner(painter, v, art, d, "ready", 0);
            } else if rounds.is_none_or(|r| (r.banner[later][0]..=r.banner[later][1]).contains(&f)) {
                let pairs = fight.teams.is_some();
                banner(painter, v, art, d, if bout.rounds_to_win == 1 && pairs { "tag_battle" } else { "round" }, bout.round);
            }
        }
        Phase::RoundOver { .. } | Phase::Replay { .. } if bout.game.display.is_none() => {
            let key = match result {
                Some(Announcement::TimeUp) => "time_up",
                Some(Announcement::Perfect) => "perfect",
                Some(Announcement::DoubleKo) => "double_ko",
                _ => "ko",
            };
            banner(painter, v, art, d, key, 0);
        }
        Phase::Result { winner } => match winner {
            None => banner(painter, v, art, d, "draw", 0),
            // (two players: the winner's name, not done)
            Some(w) if info.humans[w] != info.humans[1 - w] => banner(painter, v, art, d, if info.humans[w] { "you_win" } else { "you_lose" }, 0),
            _ => {}
        },
        _ => {}
    }
}

/// Tekken Force's HUD in Tekken Tag (OpenKumite's: Tekken 3's is sprites and fonts of its own, 0x800B2C34): the
/// player's bar (the co-op partner's under it) and the soldiers' (each its class's part of the full length) in Tekken
/// Tag's strips, the score, the clock, the stage card while the round is called, +N SEC and +LIFE! where they rise,
/// NOW LOADING..., the boss's bonus time, STAGE CLEAR!! and the tally in the chrome font. `project`: world metres to
/// the 368 x 480 screen.
#[allow(clippy::too_many_arguments)]
pub fn force_hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, run: &crate::forcerun::ForceRun, bout: &Bout, frame: u64, project: &dyn Fn(glam::Vec3) -> Option<(f32, f32)>) {
    let Some(st) = &run.stage else { return };
    let (h, game) = (&d.raw["hud"], &bout.game);
    let strip = |k: usize| h["strips"][k].as_str().unwrap_or_default();
    let (bx, by, bw) = ([num(&h["bars"]["x"][0]), num(&h["bars"]["x"][1])], num(&h["bars"]["y"]), num(&h["bars"]["w"]));
    let full = run.bar(0).2.max(1) as f32;
    let slot_in = |k: usize| st.slots.get(k).is_some_and(|s| (4..=7).contains(&s.state));
    // the player, the two soldiers, the partner: bar, side, row
    for (k, side, row, show) in [(0, 0, 0, true), (1, 1, 0, slot_in(0)), (2, 1, 1, slot_in(1)), (3, 0, 1, game.fight.partner.is_some())] {
        if !show || matches!(bout.phase, Phase::Replay { .. }) {
            continue;
        }
        let (shown, trail, w) = run.bar(k);
        let (w, y, hh) = (bw * w as f32 / full, by + 26.0 * row as f32, if row == 0 { 16.0 } else { 10.0 });
        let x = if side == 0 { bx[0] + bw - w } else { bx[1] };
        image(painter, v, art, strip(2), [x, y, w, hh], Color32::WHITE);
        for (s, part) in [(4, trail), (1, shown)] {
            let pw = bw * part.max(0) as f32 / full;
            image(painter, v, art, strip(s), [if side == 0 { x + w - pw } else { x }, y, pw, hh], Color32::WHITE);
        }
    }
    let texts = &run.data.hud;
    small(painter, v, art, d, &crate::art::printf(&texts.score, &[st.score]), bx[0] + bw / 2.0, by + 46.0, Color32::WHITE);
    small(painter, v, art, d, &crate::art::printf(&texts.hi_score, &[st.hiscore]), bx[1] + bw / 2.0, by + 46.0, Color32::WHITE);
    font(painter, v, art, d, &format!("{}", game.fight.clock / 60), W / 2.0, by - 6.0, 0.6, Color32::WHITE);
    if st.keys > 0 {
        small(painter, v, art, d, &format!("KEYS {}", st.keys), W / 2.0, by + 40.0, Color32::from_rgb(255, 210, 90));
    }
    // (the 368 x 480 screen's place on the 640 x 448)
    let at = |(x, y): (f32, f32)| (x * W / SCREEN_W, y * H / SCREEN_H);
    let chrome = |text: &str, (x, y): (f32, f32), scale: f32, c: Color32| font(painter, v, art, d, text, x, y, scale, c);
    if bout.phase == Phase::Call {
        let (number, name, _) = run.card();
        chrome(&number.map_or("FINAL STAGE".into(), |n| format!("STAGE {}", n + 1)), (W / 2.0, 150.0), 0.8, Color32::WHITE);
        chrome(&name.join(" ").split_whitespace().collect::<Vec<_>>().join(" "), (W / 2.0, 200.0), 0.5, Color32::WHITE);
    }
    for (secs, p, left) in st.popups() {
        if let Some((x, y)) = project(p + glam::Vec3::new(0.0, 0.3, 0.0)).map(at) {
            chrome(&format!("+{secs} {}", texts.sec), (x, y - (60 - left) as f32 / 2.0), 0.3, Color32::from_rgb(120, 190, 255));
        }
    }
    for c in st.chickens.iter().filter(|c| c.state == 2) {
        if let Some((x, y)) = project(c.at / kumite_game::UNITS + glam::Vec3::new(0.0, 0.3, 0.0)).map(at) {
            let tint = if frame & 2 != 0 { Color32::from_rgb(255, 120, 110) } else { Color32::WHITE };
            chrome(&texts.life, (x, y - (60 - st.life_text(c).min(60)) as f32 / 2.0), 0.3, tint);
        }
    }
    if st.state == 4 {
        chrome(&texts.now_loading, (W / 2.0, H - 50.0), 0.35, Color32::WHITE);
    }
    if st.state == 6 && st.bonus_time > 0 && game.fight.fighters[1].health > 0 {
        let b = st.bonus_time;
        if let Some(p) = project(game.fight.fighters[1].root() / kumite_game::UNITS + glam::Vec3::new(0.0, 1.9, 0.0)).map(at) {
            chrome(&format!("{:2}\"{:02}", b / 60 % 60, b * 100 / 60 % 100), p, 0.3, Color32::from_rgb(255, 230, 90));
        }
    }
    if st.state == 7 || st.state == 10 {
        chrome(&texts.stage_clear, (W / 2.0, 110.0), 0.8, Color32::WHITE);
    }
    if st.state == 7 || st.tally_done && run.index < run.data.final_index() {
        let t = st.tally_frame();
        let time = |fmt: &str, f: u32| crate::art::printf(fmt, &[f / 3600, f / 60 % 60, f * 100 / 60 % 100]);
        let bonus = run.data.stage_bonus.get(run.index).copied().unwrap_or(0);
        let mut lines = vec![(30, crate::art::printf(&texts.stage_bonus, &[bonus])), (60, crate::art::printf(&texts.clear_bonus, &[st.clear_bonus])), (90, time(&texts.clear_time, st.elapsed))];
        if st.key_earned {
            lines.push((120, texts.key.clone()));
        } else if st.final_challenge {
            lines.push((120, texts.final_challenge.clone()));
        } else if run.index + 1 < run.data.final_index() && st.bonus_time > 0 {
            lines.push((120, time(&texts.bonus_time, st.bonus_time)));
        }
        for (k, (_, line)) in lines.iter().enumerate().filter(|(_, l)| t >= l.0) {
            chrome(line, (W / 2.0, 200.0 + 40.0 * k as f32), 0.4, Color32::WHITE);
        }
    }
}

/// A fighter's theme in Tekken Force (OpenKumite's: Tekken 3 plays the player's and the boss's, Tekken Tag's fighters
/// have none): the music of the arena named after it (the name's first word), else the first arena's.
pub fn theme<'a>(d: Option<&Data>, sound: &'a kumite_game::sound::SoundData, name: &str) -> Option<&'a kumite_game::sound::Track> {
    let first = |s: &str| s.split(['-', ' ']).next().unwrap_or_default().to_lowercase();
    let arena = d?.arenas.iter().find(|a| first(&a.1) == first(name)).map_or(0, |a| a.0);
    sound.arena_music(arena)
}

/// Tekken Ball's gauges and floating texts in Tekken Tag (OpenKumite's: Tekken 3's are sprites of its own,
/// 0x800B49A8 / 0x800B3F98): each side's charge in Tekken Tag's bar strips at the screen's foot, filling towards the
/// middle, the owner's blinking while the ball has power; the texts in the chrome font. `gauges`: per side the shown
/// and the trail part of the full; `texts`: text, Tekken 3's colour, place on the 368 x 480 screen.
#[allow(clippy::too_many_arguments)]
/// Ball 2 ON 2 (OpenKumite's): each side's partner's health, a thin bar of Tekken Tag's strips over its side's
/// (filling from the inner end, as the bars).
pub fn ball_partner_bars(painter: &egui::Painter, v: &View, art: &Art, d: &Data, bout: &Bout) {
    let fight = &bout.game.fight;
    if !fight.ball_doubles() {
        return;
    }
    let h = &d.raw["hud"];
    let strip = |k: usize| h["strips"][k].as_str().unwrap_or_default();
    let (bx, by, bw) = ([num(&h["bars"]["x"][0]), num(&h["bars"]["x"][1])], num(&h["bars"]["y"]), num(&h["bars"]["w"]));
    // (actor 3 is P1's side's partner, 2 the other side's)
    for (i, actor) in [3, 2].into_iter().enumerate() {
        let Some(f) = fight.actor(actor) else { continue };
        let part = (f.health.max(0) as f32 / (f.max_health * kumite_sim::ONE).max(1) as f32).clamp(0.0, 1.0);
        // (above the side's bar, where a tag battle shows the member in back)
        let y = by - 10.0;
        image(painter, v, art, strip(2), [bx[i], y, bw, 6.0], Color32::WHITE);
        let w = bw * part;
        image(painter, v, art, strip(1), [if i == 0 { bx[i] + bw - w } else { bx[i] }, y, w, 6.0], Color32::WHITE);
    }
}

pub fn ball_hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, gauges: [[f32; 2]; 2], flash: Option<usize>, texts: &[(&str, u32, [f32; 2])], frame: u32) {
    let h = &d.raw["hud"];
    let strip = |k: usize| h["strips"][k].as_str().unwrap_or_default();
    let (bx, bw, y) = ([num(&h["bars"]["x"][0]), num(&h["bars"]["x"][1])], num(&h["bars"]["w"]), H - 40.0);
    for (i, parts) in gauges.into_iter().enumerate() {
        let tint = if flash == Some(i) && frame / 8 % 2 == 1 { Color32::from_gray(110) } else { Color32::WHITE };
        image(painter, v, art, strip(2), [bx[i], y, bw, 10.0], tint);
        // (the trail, then the charge)
        for (k, part) in [(4, parts[1]), (1, parts[0])] {
            let pw = bw * part.clamp(0.0, 1.0);
            image(painter, v, art, strip(k), [if i == 0 { bx[i] } else { bx[i] + bw - pw }, y, pw, 10.0], tint);
        }
    }
    for &(s, c, [x, y]) in texts {
        let tint = match c {
            2 => Color32::from_rgb(120, 190, 255),
            5 => Color32::from_rgb(255, 120, 110),
            _ => Color32::from_rgb(255, 230, 90),
        };
        font(painter, v, art, d, s, x * W / SCREEN_W, y * H / SCREEN_H, 0.35, tint);
    }
}

/// Tekken Bowl's HUD (OpenKumite's: the original's, 0x3E4BC0, and its gauges' sprites are not decoded): each player's
/// score sheet (ten frames, the balls' marks, the running totals; the one bowling lit), the prompt and the meters of
/// the throw's step (the spin meter between the hand's bounds, the power meter), SPEED in MPH (0x3EAC88) once the
/// ball goes, and the callout in the chrome font.
pub fn bowl_hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, frame: u64) {
    use kumite_sim::bowl::Phase;
    let b = &scene.bowl;
    // the score card while the bowler takes its place (0x3EB248 -> 0x3EA868): at (20, 20), four frames to the one
    // bowled from three before it
    if b.phase == Phase::Position {
        let n = b.cards.get(b.current).map_or(1, |c| c.at().frame + 1);
        let first = n.saturating_sub(3).clamp(1, 7);
        bowl_card(painter, v, art, d, &b.cards, Some(b.current), [20.0, 20.0], (first, first + 3), false, frame);
    }
    // the throw's step: its prompt and meters
    let bowler = b.players.get(b.current);
    bowl_meters(painter, v, art, b);
    // HELP MESSAGES on: the step's instructions for the frame's first, second or third ball (0x3F0330: help texts 93 on,
    // four a ball: position, spin, power, the run-up's steering)
    let step = match b.phase {
        Phase::Position => Some(0),
        Phase::Aim => Some(1),
        Phase::Power => Some(2),
        Phase::Throw(_) if b.rolled.is_none() => Some(3),
        _ => None,
    };
    if let (Some(s), false) = (step, progress().bowl_off[0]) {
        let ball = b.cards.get(b.current).map_or(0, |c| c.at().ball.min(2));
        help_line(painter, v, art, d, 93 + 4 * ball + s, 0);
    }
    let text = |t: &str, x: f32, y: f32, colour: usize, size: f32| chrome(painter, v, art, d, t, x, y, Some(colour), size);
    // BOWL STYLE / SWITCH HANDS:R or L, 120 frames after SELECT switched the hand (0x3EBADC)
    if b.style > 0 {
        text(&scene.text("bowl_style", "BOWL STYLE"), 239.0, 364.0, 15, 4.0);
        text(&scene.text("switch_hands", "SWITCH HANDS:%C").replace("%C", if bowler.is_some_and(|p| p.left) { "L" } else { "R" }), 174.0, 384.0, 15, 5.0);
    }
    // REPLAY while the slow replay after the roll plays, blinking (0x3EF348)
    if b.replay.on && b.replay.slow && b.replay.left == 0 && frame & 0x30 != 0 {
        text(&scene.text("replay", "REPLAY"), 48.0, 412.0, 14, 5.0);
    }
    // SPEED from z 4000 on and over a spectator's K.O. (0x3EAD18): the mph's whole part, its hundredths, MPH; dashes
    // for a charge
    let past = b.rolled.is_some() && (b.ball.pos.z >= 4000.0 || matches!(b.phase, Phase::Charge(_)));
    if past && matches!(b.phase, Phase::Throw(_) | Phase::Charge(_) | Phase::Ko(_)) {
        let mph = b.mph();
        let (whole, part) = if matches!(b.phase, Phase::Charge(_)) || b.speed <= 0.0 { (" --".into(), ".--".into()) } else { (format!("{:3}", mph / 100), format!(".{:02}", mph % 100)) };
        text("SPEED", 464.0, 396.0, 5, 4.0);
        text(&whole, 448.0, 412.0, 5, 5.0);
        text(&part, 512.0, 417.0, 5, 4.0);
        text("MPH", 564.0, 422.0, 5, 3.0);
    }
    // OVER CHARGED, blinking (0x3EB830)
    if let (Phase::OverCharged(_), Some(c)) = (b.phase, b.callout.as_deref()) {
        text(c, 113.0, 240.0, if frame & 2 != 0 { 12 } else { 24 }, 8.0);
    }
    // a spectator's K.O. (state 4, 0x3EFF6C): from frame 90, blinking, on the side of the one hit, an arrow at it
    if let Phase::Ko(f) = b.phase {
        let side = if b.ko_side > 0.0 { -1.0 } else { 1.0 };
        if f >= 90 && frame & 0x30 != 0 {
            text("K.O.", 270.0 + 224.0 * side, 200.0, 12, 5.0);
            text(if side < 0.0 { "{" } else { "}" }, 304.0 + 274.0 * side, 204.0, 2, 10.0);
        }
    }
    bowl_callout(painter, v, art, d, scene, frame);
}

/// Tekken Bowl's results (state 10, 0x3F0FF0), a player every 176 down: its team's faces in their frames from (216, 64),
/// 64 apart; PLAYERn at (48, 116) (size 5, colour 14); WINNERS (blinking 2 / 11), LOSERS (12) or DRAW (10) right-aligned
/// on 472 at 78 and the total at (415, 110) (size 8, the same colour); the score card from (36, 128) (0x3C59D8, the
/// ten frames and the total); a new record's NEW RECORD! and its place, taking turns, blinking (0x3F0E28 at (123, 84)).
/// Over the hall going to black over the game's first 60 frames over (`since`, 0x3F1370: a black quad, alpha t x 128 /
/// 60).
#[allow(clippy::too_many_arguments)]
pub fn bowl_results(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, teams: &[[usize; 2]], placed: &[Option<usize>], (frame, since): (u64, u64)) {
    let b = &scene.bowl;
    Art::shade(painter, v, crate::art::FULL, [Color32::from_black_alpha((since.min(60) * 255 / 60) as u8); 4]);
    for (p, card) in b.cards.iter().enumerate().take(2) {
        let dy = 176.0 * p as f32;
        for (m, body) in teams.get(p).into_iter().flatten().enumerate() {
            let x = 216.0 + 64.0 * m as f32;
            image(painter, v, art, "003_110.png", [x - 2.0, 62.0 + dy, 55.0, 56.0], Color32::WHITE);
            image(painter, v, art, &d.picture(&["select", "faces", &body.to_string()]), [x, 64.0 + dy, 51.0, 52.0], Color32::WHITE);
        }
        chrome(painter, v, art, d, &format!("PLAYER{}", p + 1), 48.0, 116.0 + dy, Some(14), 5.0);
        let (word, w, colour) = match b.winner() {
            None => (scene.text("draw", "DRAW"), 70.0, 10),
            Some(k) if k == p => (scene.text("winners", "WINNERS"), 115.0, if frame & 2 != 0 { 2 } else { 11 }),
            Some(_) => (scene.text("losers", "LOSERS"), 101.0, 12),
        };
        chrome(painter, v, art, d, &word, 472.0 - w, 78.0 + dy, Some(colour), 8.0);
        chrome(painter, v, art, d, &format!("{:3}", card.total()), 415.0, 110.0 + dy, Some(colour), 8.0);
        bowl_card_of(painter, v, art, d, b, p, [36.0, 128.0 + dy], frame);
        if let Some(place) = placed.get(p).copied().flatten().filter(|&k| k < 5 && frame & 0x30 != 0) {
            let c = Some(if frame & 2 != 0 { 11 } else { 2 });
            if frame & 0x40 != 0 {
                chrome(painter, v, art, d, &scene.text("new", "NEW"), 97.0, 78.0 + dy, c, 4.0);
                chrome(painter, v, art, d, &scene.text("record", "RECORD!"), 63.0, 96.0 + dy, c, 4.0);
            } else {
                chrome(painter, v, art, d, ["1ST", "2ND", "3RD", "4TH", "5TH"][place], 91.0, 84.0 + dy, c, 5.0);
            }
        }
    }
}

/// The JUKEBOX's track (a BGM id) for Tekken Bowl's music, when one is chosen.
pub fn jukebox(d: &Data) -> Option<usize> {
    progress().jukebox.and_then(|k| music(d).get(k).map(|m| m.0))
}

/// The name entry's letters (`records.letters`, 0x3D0450's: "<" takes the last back, "=" ends).
pub fn record_letters(d: &Data) -> Vec<char> {
    d.raw["records"]["letters"].as_str().unwrap_or_default().chars().collect()
}

/// THE KING OF IRONBOWL (0x3F3108): TOP 5 BOWLERS with the new record's row lit (0x3F2A38) and its name being entered
/// (the letter under the cursor blinking), EXIT:START on the help band once it is done.
#[allow(clippy::too_many_arguments)]
pub fn bowl_records(
    painter: &egui::Painter,
    v: &View,
    art: &Art,
    d: &Data,
    scene: &kumite_game::bowl::BowlScene,
    list: &[kumite_game::bowl::Record],
    new: Option<usize>,
    cursor: Option<usize>,
    frame: u64,
) {
    let mut list = list.to_vec();
    let letters = record_letters(d);
    if let (Some(k), Some(c)) = (new, cursor) {
        if let (Some(r), true) = (list.get_mut(k), frame & 0x10 == 0) {
            r.name.push(letters.get(c).copied().unwrap_or(' '));
        }
    }
    bowl_top(painter, v, art, d, scene, &list, new, 255);
    chrome(painter, v, art, d, &scene.text("king", "THE KING OF IRONBOWL"), 112.0, 40.0, Some(15), 5.0);
    if cursor.is_none() {
        exit_start(painter, v, art, d);
    }
}

/// EXIT:START on the help band (y 419).
fn exit_start(painter: &egui::Painter, v: &View, art: &Art, d: &Data) {
    let h = &d.raw["help"]["band"]["rect"];
    Art::shade(painter, v, sv([num(&h[0]), 419.0, num(&h[2]), 48.0]), [rgba(&d.raw["help"]["band"]["colour"]); 4]);
    chrome(painter, v, art, d, "EXIT:START", 72.0, 440.0, Some(5), 4.0);
}

/// TOP 5 BOWLERS from the Bowl pause (0x3F3118): over the hall as it stands, with no shade of its own; the row under
/// the cursor lit by the band (0x3F2A38) before the list (0x3F2D00), THE KING OF IRONBOWL above, the help line 109
/// (or EXIT:START with the help off).
// shortcut: a button on a row opens its score card on the PS2 (0x3F25D8); records keep no card here, so it does nothing
pub fn bowl_pause_top(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, list: &[kumite_game::bowl::Record], cursor: usize) {
    record_band(painter, v, 135.0 + 71.0 * cursor as f32);
    bowl_top(painter, v, art, d, scene, list, None, 0);
    chrome(painter, v, art, d, &scene.text("king", "THE KING OF IRONBOWL"), 112.0, 40.0, Some(15), 5.0);
    if progress().setting(d, HELP_ROW) != 0 {
        help_line(painter, v, art, d, 109, 0);
    } else {
        exit_start(painter, v, art, d);
    }
}

/// A record's band (0x3F2A38): the width of the screen, 63 high above `y`, grey 128 at alpha 0x40 (half).
fn record_band(painter: &egui::Painter, v: &View, y: f32) {
    Art::shade(painter, v, sv([0.0, y - 63.0, W, 63.0]), [Color32::from_rgba_unmultiplied(128, 128, 128, 128); 4]);
}

/// A score card as the PS2 draws it (0x3C59D8): a row a player (68 high, the second's 46), of pieces of file 130 each
/// over a faint green (0x3DBE98) — the left edge, frames `from`..=`to` (47 wide, the tenth 70) with their number, the
/// balls' marks (a strike's and a spare's pictures, G a first ball's miss, - a second's, F a foul in red, a split's
/// count in colour 5) and the running totals, the total cell (54) when `total`, the right edge — in the small font
/// (colour 6). The ball to bowl blinks as an arrow (frame & 0x30).
#[allow(clippy::too_many_arguments)]
fn bowl_card(
    painter: &egui::Painter,
    v: &View,
    art: &Art,
    d: &Data,
    cards: &[kumite_sim::bowl::Card],
    current: Option<usize>,
    [x0, y0]: [f32; 2],
    (from, to): (usize, usize),
    total: bool,
    frame: u64,
) {
    use kumite_sim::bowl::Mark;
    let rows: Vec<(f32, f32)> = [(y0, 68.0), (y0 + 68.0, 46.0)].into_iter().take(cards.len().clamp(1, 2)).collect();
    let bottom = y0 + rows.iter().map(|r| r.1).sum::<f32>();
    let piece = |k: usize, x: f32, y: f32, w: f32, h: f32| {
        Art::shade(painter, v, sv([x, y, w, h]), [Color32::from_rgba_unmultiplied(80, 144, 0, 64); 4]);
        sprite(painter, v, art, &format!("130_{k:03}.png"), x, y, Color32::WHITE);
    };
    // a column: its piece in each row and the bottom's
    let column = |first: usize, x: f32, w: f32| {
        for (r, &(y, h)) in rows.iter().enumerate() {
            piece(first + r, x, y, w, h);
        }
        piece(first + 2, x, bottom, w, 4.0);
    };
    let text = |s: &str, x: f32, foot: f32, colour: usize| small_in(painter, v, art, d, Some(colour), s, x, foot - 20.0, Color32::WHITE);
    let centred = |s: &str, x: f32, foot: f32| text(s, x - small_width(d, s) / 2.0, foot, 6);
    column(0, x0, 4.0);
    let mut x = x0 + 4.0;
    for n in from..=to.min(10) {
        let (w, dk): (f32, &[f32]) = if n < 10 { (47.0, &[-10.0, 12.0]) } else { (70.0, &[-10.0, 12.0, 35.0]) };
        column(if n < 10 { 3 } else { 6 }, x, w);
        text(&n.to_string(), x + 15.0, y0 + 25.0, 6);
        for (p, (card, &(y, _))) in cards.iter().zip(&rows).enumerate() {
            let totals = card.totals();
            let mark_y = if p == 0 { y + 47.0 } else { y + 26.0 };
            let next = (current == Some(p) && frame & 0x30 != 0).then(|| card.at()).filter(|t| t.frame + 1 == n && !t.done);
            for (k, i) in card.frame_balls(n - 1).enumerate() {
                let bx = x + 15.0 + dk.get(k).copied().unwrap_or(0.0);
                let pins = card.rolls.get(i).copied().unwrap_or(0).to_string();
                match card.marks.get(i).copied().unwrap_or_default() {
                    Mark::Strike => sprite(painter, v, art, "130_017.png", bx - 5.0, mark_y - 21.0, Color32::WHITE),
                    Mark::Spare => sprite(painter, v, art, "130_016.png", bx - 3.0, mark_y - 21.0, Color32::WHITE),
                    Mark::Miss => text(if k == 0 { "G" } else { "-" }, bx, mark_y, 6),
                    Mark::Foul => text("F", bx, mark_y, 1),
                    Mark::Split => text(&pins, bx, mark_y, 5),
                    Mark::None => text(&pins, bx, mark_y, 6),
                }
            }
            if let Some(t) = next {
                sprite(painter, v, art, "130_018.png", x + 15.0 + dk.get(t.ball).copied().unwrap_or(0.0) - 5.0, mark_y - 21.0, Color32::WHITE);
            }
            if let Some(t) = totals.get(n - 1).copied().flatten() {
                centred(&t.to_string(), x + if n < 10 { 24.0 } else { 34.0 }, if p == 0 { y + 69.0 } else { y + 48.0 });
            }
        }
        x += w;
    }
    if total {
        column(9, x, 54.0);
        for (p, (card, &(y, _))) in cards.iter().zip(&rows).enumerate() {
            let (cx, foot) = if p == 0 { (x + 27.0, y + 56.0) } else { (x + 24.0, y + 36.0) };
            centred(&card.total().to_string(), cx, foot);
        }
        x += 54.0;
    }
    column(12, x, 4.0);
}

/// The throw's gauges (0x3C65D0), each step's alpha (table 0x51FD30: 128 drawn, 64 half): the rack for the frame's
/// second ball (0x3C6280: `130_015` at (524, 20), a dot a pin, `130_024` standing, `130_023` down), the spin meter
/// (0x3C6410: the arc `130_019` at (50, 280); the needle `130_022` centred 94 from (138, 369) at (spin / 4 - 0.5) x
/// 3.14, turned spin / 1.3), the power bar (0x3C64E8: `130_020` at (550, 140), 64 x 246, its lowest power x 236 / 100 + 4
/// in `130_021`).
fn bowl_meters(painter: &egui::Painter, v: &View, art: &Art, b: &kumite_sim::bowl::Bowl) {
    use kumite_sim::bowl::Phase;
    let alpha = match b.phase {
        Phase::Position => [128, 0, 0],
        Phase::Aim => [128, 128, 64],
        Phase::Power => [128, 64, 128],
        Phase::Throw(_) if b.rolled.is_none() => [64; 3],
        _ => return,
    };
    let tint = |a: u32| Color32::from_white_alpha((a * 255 / 128).min(255) as u8);
    const RACK: [[f32; 2]; 10] = [[37.0, 66.0], [26.0, 46.0], [48.0, 46.0], [15.0, 26.0], [37.0, 26.0], [59.0, 26.0], [4.0, 6.0], [26.0, 6.0], [48.0, 6.0], [70.0, 6.0]];
    if b.cards.get(b.current).is_some_and(|c| c.at().ball == 1) {
        sprite(painter, v, art, "130_015.png", 524.0, 20.0, tint(alpha[0]));
        for (p, [dx, dy]) in b.pins.iter().zip(RACK) {
            sprite(painter, v, art, if p.standing { "130_024.png" } else { "130_023.png" }, 524.0 + dx, 20.0 + dy, tint(alpha[0]));
        }
    }
    if alpha[1] > 0 {
        sprite(painter, v, art, "130_019.png", 50.0, 280.0, tint(alpha[1]));
        // (the executable's 3.14, gp-0x7D8C, not pi)
        #[allow(clippy::approx_constant)]
        let a = (b.spin / 4.0 - 0.5) * 3.14;
        let (x, y) = (138.0 + 94.0 * a.cos(), 369.0 + 94.0 * a.sin());
        if let Some((tex, _)) = art.pic("130_022.png") {
            let (s, c) = (b.spin / 1.3).sin_cos();
            let mut mesh = egui::Mesh::with_texture(tex.id());
            for (dx, dy, u, w) in [(-8.0, -10.0, 0.0, 0.0), (8.0, -10.0, 1.0, 0.0), (-8.0, 10.0, 0.0, 1.0), (8.0, 10.0, 1.0, 1.0)] {
                let [px, py, _, _] = sv([x + dx * c - dy * s, y + dx * s + dy * c, 0.0, 0.0]);
                mesh.vertices.push(egui::epaint::Vertex { pos: v.pos(px, py), uv: Pos2::new(u, w), color: tint(alpha[1]) });
            }
            mesh.indices.extend([0, 1, 2, 1, 3, 2]);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    if alpha[2] > 0 {
        let fill = (b.power * 236.0 / 100.0).floor() + 4.0;
        let part = |file: &str, v0: f32, v1: f32| {
            if let Some((tex, _)) = art.pic(file) {
                let [x, y, w, h] = sv([550.0, 140.0 + v0, 64.0, v1 - v0]);
                painter.image(tex.id(), Rect::from_min_max(v.pos(x, y), v.pos(x + w, y + h)), Rect::from_min_max(Pos2::new(0.0, v0 / 246.0), Pos2::new(1.0, v1 / 246.0)), tint(alpha[2]));
            }
        };
        part("130_020.png", 0.0, 246.0 - fill);
        part("130_021.png", 246.0 - fill, 246.0);
    }
}

/// The count's callout at y 360 in the chrome font: a strike's (0x3EF8B0: its x and colour by the run, PERFECT
/// blinking, a chicken a strike from a TURKEY on), SPARE / NICE COVER sliding in from the bowler's side (0x3EF660), MISS /
/// FOUL dropping in and bouncing (0x3EF470).
fn bowl_callout(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, frame: u64) {
    use kumite_sim::bowl::Phase;
    let b = &scene.bowl;
    let Some(c) = b.callout.as_deref().filter(|c| !c.is_empty()) else { return };
    let co = &b.callouts;
    let (f, x, y, colour) = match b.phase {
        Phase::Strike(f) => {
            let k = co.strikes.iter().chain([&co.punch_out, &co.perfect]).position(|s| s == c).unwrap_or(0);
            let run = b.runs.get(b.current).copied().unwrap_or(0);
            if run >= 3 {
                let x0 = (644.0 - 36.0 * run as f32) / 2.0;
                for n in 0..run {
                    image(painter, v, art, "003_016.png", [x0 + 36.0 * n as f32, 296.0, 32.0, 32.0], Color32::WHITE);
                }
            }
            let colour = [2, 9, 18, 11, 18, if frame & 2 != 0 { 11 } else { 2 }];
            (f, [178.0, 144.0, 129.0, 125.0, 126.0, 170.0][k.min(5)], 360.0, colour[k.min(5)])
        }
        Phase::Result(f) if c == co.spare || c == co.nice_cover => {
            let (w, colour) = if c == co.spare { (214.0, 17) } else { (420.0, 7) };
            let centre = (640.0 - w) / 2.0;
            let x = if b.current == 0 { (32.0 * f as f32 - w).min(centre) } else { (640.0 - 32.0 * f as f32).max(centre) };
            (f, x, 360.0, colour)
        }
        Phase::Result(f) if c == co.miss || c == co.foul => {
            // from y -20 at 5 a frame, 0.5 more each frame; past 360 back at 360, rebounding at half the speed
            let (mut y, mut vy) = (-20.0f32, 5.0f32);
            for _ in 0..f {
                y += vy;
                vy += 0.5;
                if y as i32 >= 361 {
                    (y, vy) = (360.0, -vy * 0.5);
                }
            }
            (f, if c == co.miss { 242.0 } else { 232.0 }, y, if c == co.miss { 1 } else { 10 })
        }
        _ => return,
    };
    if f < 150 {
        chrome(painter, v, art, d, c, x, y, Some(colour), 10.0);
    }
}

/// BOWLING OPTIONS (the Bowl pause's entry 5, 0x3F1968): HELP MESSAGES and SPECIAL EFFECTS (ON / OFF), the bonus row
/// (BONUS AT 200 PTS until a 200 game unlocks the JUKEBOX), EXIT. Up / down move the cursor (past the bonus row while
/// it is locked), left / right switch the one at it, a button on EXIT or START leave (true). The switches are kept in
/// the save (block 0x46BC70 +0x1FE, +0x200; both ON at first, 0x3F20E8).
/// Once a 200 game unlocked it (+0x202) the bonus row is the JUKEBOX: left / right go round the music player's tracks
/// and OFF.
pub fn bowl_options_key(d: &Data, cursor: &mut usize, key: Key) -> bool {
    let mut p = progress();
    let juke = p.bowl_bonus[0];
    match key {
        Key::Up | Key::Down => {
            sfx::play(sound(d, "cursor"));
            // (0 HELP MESSAGES, 1 SPECIAL EFFECTS, 2 JUKEBOX once unlocked, 3 EXIT)
            let rows: &[usize] = if juke { &[0, 1, 2, 3] } else { &[0, 1, 3] };
            let k = rows.iter().position(|r| r == cursor).unwrap_or(0);
            *cursor = rows[(k + if key == Key::Up { rows.len() - 1 } else { 1 }) % rows.len()];
        }
        Key::Left | Key::Right if *cursor < 2 => {
            sfx::play(sound(d, "cursor"));
            p.bowl_off[*cursor] = !p.bowl_off[*cursor];
            set_progress(p);
        }
        Key::Left | Key::Right if *cursor == 2 && juke => {
            sfx::play(sound(d, "cursor"));
            // (the tracks, then OFF)
            let n = music(d).len() + 1;
            let at = p.jukebox.unwrap_or(n - 1);
            let at = (at + if key == Key::Left { n - 1 } else { 1 }) % n;
            p.jukebox = (at + 1 < n).then_some(at);
            set_progress(p);
        }
        Key::Start | Key::Back => return true,
        Key::Button(_) if *cursor == 3 => {
            sfx::play(sound(d, "decide"));
            return true;
        }
        _ => {}
    }
    false
}

/// BOWLING OPTIONS as the PS2 draws it (0x3F1968): the pause's window at (135, 172, 370, 88), each row's label at x 155
/// from y 200 every 20 (size 4; the cursor's in colour 11 over its bar, the others 5) with its value after it (ON 16,
/// OFF 12), an arrow towards the other value, and the cursor row's help line.
pub fn draw_bowl_options(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, cursor: usize) {
    let p = &d.raw["pause"];
    let [x, y, w, h] = [135.0, 172.0, 370.0, 88.0];
    for r in [[x - 10.0, y - 10.0, w + 20.0, 4.0], [x - 10.0, y + h + 6.0, w + 20.0, 4.0], [x - 10.0, y - 6.0, 4.0, h + 12.0], [x + w + 6.0, y - 6.0, 4.0, h + 12.0]] {
        Art::shade(painter, v, sv(r), [Color32::from_gray(195); 4]);
    }
    Art::shade(painter, v, sv([x - 6.0, y - 6.0, w + 12.0, h + 12.0]), [rgba(&p["colour"]); 4]);
    let off = progress().bowl_off;
    let bar = |bx: f32, by: f32| {
        let (on, b0, b1) = (rgba(&p["bar"]["colour"]), num(&p["bar"]["y"][0]), num(&p["bar"]["y"][1]));
        Art::shade(painter, v, sv([155.0, by + b0, bx + 30.0, b1 - b0]), [on, Color32::TRANSPARENT, on, Color32::TRANSPARENT]);
    };
    // (the menu's arrow towards the other value, 0x3BB890)
    let arrow = d.raw["menu"]["arrows"]["pictures"][1].as_str().unwrap_or_default();
    let ah = art.pic(arrow).map_or(0.0, |p| p.1[1] as f32);
    for (k, (key, label)) in [("help_messages", "HELP MESSAGES:"), ("special_effects", "SPECIAL EFFECTS:")].iter().enumerate() {
        let y = 200.0 + 20.0 * k as f32;
        let on = !off[k];
        // (the label before its value's printf codes)
        let text = scene.text(key, label);
        let label = text.split('%').next().unwrap_or_default();
        if cursor == k {
            bar([291.0, 322.0][k], y);
            sprite(painter, v, art, arrow, [450.0, 483.0][k], y - ah, Color32::WHITE);
        }
        let end = chrome(painter, v, art, d, label, 155.0, y, Some(if cursor == k { 11 } else { 5 }), 4.0);
        chrome(painter, v, art, d, if on { "ON" } else { "OFF" }, end, y, Some(if on { 16 } else { 12 }), 4.0);
    }
    if progress().bowl_bonus[0] {
        // JUKEBOX: its track (OFF: none), the arrows either side under the cursor
        let name = progress().jukebox.and_then(|k| music(d).get(k).map(|m| m.1.clone())).unwrap_or_else(|| "OFF".into());
        if cursor == 2 {
            bar(298.0, 240.0);
            let left = d.raw["menu"]["arrows"]["pictures"][0].as_str().unwrap_or_default();
            sprite(painter, v, art, left, 296.0, 240.0 - ah, Color32::WHITE);
            sprite(painter, v, art, arrow, 475.0, 240.0 - ah, Color32::WHITE);
        }
        let label = scene.text("jukebox", "JUKEBOX: ");
        let end = chrome(painter, v, art, d, label.split('%').next().unwrap_or_default(), 155.0, 240.0, Some(if cursor == 2 { 11 } else { 5 }), 4.0);
        chrome(painter, v, art, d, &name, end, 240.0, Some(17), 4.0);
    } else {
        chrome(painter, v, art, d, &scene.text("bonus", "BONUS AT 200 PTS"), 155.0, 240.0, Some(17), 4.0);
    }
    if cursor == 3 {
        bar(70.0, 260.0);
    }
    chrome(painter, v, art, d, &scene.text("exit", "EXIT"), 155.0, 260.0, Some(if cursor == 3 { 11 } else { 5 }), 4.0);
    if cursor < 3 {
        help_line(painter, v, art, d, 106 + cursor, usize::from(!off.get(cursor).copied().unwrap_or(false)));
    }
}

/// Player `p`'s whole score card alone (the results', 0x3F1230).
fn bowl_card_of(painter: &egui::Painter, v: &View, art: &Art, d: &Data, b: &kumite_sim::bowl::Bowl, p: usize, at: [f32; 2], frame: u64) {
    bowl_card(painter, v, art, d, b.cards.get(p..=p).unwrap_or_default(), None, at, (1, 10), true, frame);
}

/// The whole score card under the Bowl pause (state 9, 0x3F0A3C -> 0x3EA868: at (63, 128), the ten frames and the
/// totals).
pub fn bowl_pause_card(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, frame: u64) {
    let b = &scene.bowl;
    bowl_card(painter, v, art, d, &b.cards, Some(b.current), [63.0, 128.0], (1, 10), true, frame);
}

/// TOP 5 BOWLERS (the Bowl pause's entry 4, 0x3F2D00): the title at (183, 64); a record every 71 from y 135 (0x3F2AC0):
/// its place large at x 16 with ST / ND / RD / TH after it, the team's two faces in their frames from x 176 (52 apart),
/// the score right-aligned on 364 (size 9) and PTS at 408, the name at 468.
pub fn bowl_top(painter: &egui::Painter, v: &View, art: &Art, d: &Data, scene: &kumite_game::bowl::BowlScene, list: &[kumite_game::bowl::Record], new: Option<usize>, back: u8) {
    // (`back`: after the results on their black, 255; from the pause over the hall, 0)
    Art::shade(painter, v, crate::art::FULL, [Color32::from_black_alpha(back); 4]);
    chrome(painter, v, art, d, &scene.text("top", "TOP 5 BOWLERS"), 183.0, 64.0, Some(10), 5.0);
    for (k, r) in list.iter().enumerate().take(5) {
        let y = 135.0 + 71.0 * k as f32;
        // (the new record: its row's band, colour 11)
        let lit = new == Some(k);
        if lit {
            record_band(painter, v, y);
        }
        let (c, score_c) = if lit { (11, 11) } else { (15, 21) };
        let end = chrome(painter, v, art, d, &format!("{:2}", k + 1), 16.0, y - 27.0, Some(c), 10.0);
        chrome(painter, v, art, d, ["ST", "ND", "RD"].get(k).copied().unwrap_or("TH"), end, y - 7.0, Some(c), 5.0);
        for (m, body) in r.bodies.iter().enumerate() {
            let x = 176.0 + 52.0 * m as f32;
            image(painter, v, art, "003_110.png", [x - 2.0, y - 59.0, 55.0, 56.0], Color32::WHITE);
            image(painter, v, art, &d.picture(&["select", "faces", &body.to_string()]), [x, y - 57.0, 51.0, 52.0], Color32::WHITE);
        }
        let x = match r.score {
            0..=9 => 364.0,
            10..=99 => 326.0,
            _ => 288.0,
        };
        chrome(painter, v, art, d, &r.score.to_string(), x, y - 23.0, Some(score_c), 9.0);
        chrome(painter, v, art, d, "PTS", 408.0, y - 3.0, Some(score_c), 4.0);
        chrome(painter, v, art, d, &r.name, 468.0, y - 27.0, Some(c), 10.0);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A front end in miniature: four characters, two selectable from the start, the schedule's next ones.
    fn data() -> Arc<Data> {
        let fe = serde_json::json!({"front_end": {
            "select": {"selectable": [0, 1], "slots": [{"x": 0, "y": 0, "character": 0, "right": 1}, {"x": 53, "y": 0, "character": 1, "right": 2},
                {"x": 106, "y": 0, "character": 2, "right": 3}, {"x": 159, "y": 0, "character": null}], "hidden_slots": [],
                "start_slots": [0, 3], "random_roll": 3, "timer": {"frames": 2400}, "done": 90, "music": {"bgm": 0, "from": 18}},
            "sounds": {"cursor": 3, "start": 4, "decide": 5, "cancel": 3, "pick": 4, "last_pick": 5},
            "menu": {"items": [{"mode": 0, "label": "ARCADE"}, {"mode": 11, "label": "OPTIONS"}], "row_h": 26, "scroll": 4, "idle": 480},
            "unlocks": {"schedule": [{"stage": 0, "kind": 0, "character": 2}, {"stage": 1, "kind": 1, "character": 2}, {"stage": 1, "kind": 0, "character": 3}], "points_per_stage": 50},
            "arcade": {"fights": 8, "rivals": [[2, 3, null], [0, null, null]], "fallback": [3], "pairs": [[1, 2]], "rival_arena": 6, "boss": 3, "boss_arena": 14, "early_fights": 3, "early_excluded": []},
            "team_battle": {"refill_percent": 15},
            "records": {"defaults": {"survivors": [{"members": [0, 1], "score": 5, "name": "PAU"}, {"members": [2, 3], "score": 1, "name": "KIN"}]},
                "letters": "ABC<=", "name_len": 3},
            "ranking": {"frames": 10, "rows": [{}, {}, {"dy": 75, "first": 135, "shown": 5, "hold": 130}], "pace": {"fade": 8, "scroll": 12, "after": 60}},
            "practice": {"modes": [
                {"rows": [{"label": "MODE SELECT", "kind": "mode"}, {"label": "CANCEL", "kind": "close"},
                    {"label": "TRAINING DUMMY", "kind": "list", "values": ["STAND", "STAND GUARD"], "default": 0}]},
                {"rows": [{"label": "MODE SELECT", "kind": "mode"}, {"label": "CPU LEVEL", "kind": "list", "values": ["EASY", "NORMAL"], "default": 1},
                    {"label": "RESET", "kind": "reset"}]}]},
            "volumes": {"bgm": [0, 32, 40, 48, 56, 64, 72, 80, 96, 112, 128], "se": 10},
            "options": [{"default": 1, "values": ["EASY", "NORMAL"]}, {"default": 1, "values": ["1", "2", "3"]}, {"default": 2, "values": ["30", "40", "60"]}],
        }});
        // (each character's ending: its id)
        let endings = EndingData::new(serde_json::json!({"table": [[0], [1], [2], [3]], "endings": [{}, {}, {}, {}]}), &Value::Null).unwrap();
        let arenas: Vec<Value> = (0..23).map(|id| serde_json::json!({"id": id, "name": format!("A{id}")})).collect();
        Data::load(&fe, &Value::from(arenas), &serde_json::json!({"round_time": {"options_frames": [2160, 2820, 4260]}}), Some(endings), vec![], None, false, &Value::Null).unwrap()
    }

    fn roster() -> Roster {
        let mut r = Roster::load(std::path::Path::new("/nonexistent"));
        r.characters = (0..4).map(|id| crate::menu::Character { id, name: format!("C{id}"), costumes: vec![10 + id, 20 + id], extra: false, prefix: String::new(), face: None }).collect();
        r.costume_of = (0..4).flat_map(|id| [(10 + id, 0), (20 + id, 1)]).collect();
        r
    }

    #[test]
    fn unlocks_advance_on_a_new_clear_and_points() {
        let d = data();
        let mut p = Progress::default();
        assert!(d.selectable(0, 1) && !d.selectable(0, 2) && d.in_pool(0, 2) && !d.in_pool(0, 3));
        // a clear with a new ending: the next stage (character 2 selectable); the same ending again: no
        assert!(p.played(&d, Some(0)));
        assert!(p.stage == 1 && d.selectable(p.stage, 2) && d.in_pool(p.stage, 3));
        assert!(!p.played(&d, Some(0)));
        // 50 matches make a stage
        for _ in 0..47 {
            assert!(!p.played(&d, None));
        }
        assert!(p.played(&d, None) && p.stage == 2 && p.points == 0);
        // the save keeps it
        let back: Progress = serde_json::from_str(&serde_json::to_string(&Progress { options: Some([2, 3, 5]), ..p.clone() }).unwrap()).unwrap();
        assert_eq!((back.stage, back.options), (2, Some([2, 3, 5])));
        assert_eq!(Progress::default().options(&d), [1, 1, 2]);
    }

    #[test]
    fn arcade_ladder_rival_then_the_boss_alone() {
        let (d, r) = (data(), roster());
        let mut seed = 7;
        let run = Run::new(&d, &r, &Progress::default(), ARCADE, [vec![(0, 0), (1, 0)], vec![]], &mut seed);
        assert_eq!(run.ladder.len(), 8);
        // fights 1-6: two from the pool (character 2 alone with the players taken: one), never the players'
        for (team, arena) in &run.ladder[..6] {
            assert!(!team.is_empty() && team.iter().all(|m| m.0 == 2) && ![6, 14, 20].contains(arena));
        }
        // fight 7: the rival team in Ogre's arena; 8: the boss alone, one round, in hers
        assert_eq!((run.ladder[6].0.iter().map(|m| m.0).collect::<Vec<_>>(), run.ladder[6].1), (vec![2, 3], 6));
        assert_eq!((run.ladder[7].0.len(), run.ladder[7].0[0].0, run.ladder[7].1), (1, 3, 14));
        let mut last = run.clone();
        last.fight = 7;
        assert_eq!(last.rules(&[2160, 2820, 4260]), (1, Some(4260)));
        assert_eq!(run.rules(&[2160, 2820, 4260]).0, 2);
        let mut rival = run.clone();
        rival.fight = 6;
        assert_eq!(rival.bench().map(|b| (b[0], r.characters[b[1].0].id)), Some(((1, 0), 3)));
        assert_eq!(last.bench(), None);
        // a lost fight: CONTINUE?; a won one: the next fight; the eighth won: the clear, the unlock
        let mut p = Progress::default();
        assert!(matches!(run.clone().after(&d, &r, &mut p, Some(1), [0; 2], [1; 2], None, 0, &mut seed), Next::Screen(Page::Continue { .. })));
        assert!(matches!(run.clone().after(&d, &r, &mut p, Some(0), [0; 2], [1; 2], None, 0, &mut seed), Next::Fight(f) if f.fight == 1));
        assert!(matches!(last.after(&d, &r, &mut p, Some(0), [0; 2], [1; 2], None, 0, &mut seed), Next::Ending(0, 0, Page::Message { title: true, .. })));
        assert_eq!((p.stage, p.seen), (1, 1));
    }

    #[test]
    fn team_battle_refills_the_winner() {
        let (d, r) = (data(), roster());
        let mut seed = 1;
        // against the CPU: as many as the player picked (here the pool has one); 1P against 2P: their teams
        assert_eq!(Run::new(&d, &r, &Progress::default(), TEAM, [vec![(0, 0)], vec![]], &mut seed).teams[1].len(), 1);
        let run = Run::new(&d, &r, &Progress::default(), TEAM, [vec![(0, 0), (1, 0)], vec![(2, 0), (3, 0)]], &mut seed);
        assert!(!run.cpu(1));
        assert_eq!(run.teams[1].len(), 2);
        let max = 140 * kumite_sim::ONE;
        let Next::Fight(next) = run.after(&d, &r, &mut Progress::default(), Some(0), [max / 2, 0], [max; 2], None, 0, &mut seed) else { panic!() };
        assert_eq!((next.health[0], next.member), (Some(max / 2 + max / 100 * 15), [0, 1]));
        assert!(matches!(next.after(&d, &r, &mut Progress::default(), Some(0), [max, 0], [max; 2], None, 0, &mut seed), Next::Screen(Page::Message { .. })));
    }

    /// SURVIVAL is a tag battle: a won match gives each of the player's members 15 % of its full health back, to its
    /// health and its recoverable cap (0x3C1620), carried into the next.
    #[test]
    fn survival_refills_both_members() {
        let (d, r) = (data(), roster());
        let mut seed = 1;
        let run = Run::new(&d, &r, &Progress::default(), SURVIVAL, [vec![(0, 0), (1, 0)], vec![]], &mut seed);
        assert!(tag(SURVIVAL));
        let max = 140 * kumite_sim::ONE;
        let (q, add) = (max / 4, max / 100 * 15);
        let members = [[(q, 2 * q, max), (max, max, max)], [(0, 0, max); 2]];
        let Next::Fight(next) = run.after(&d, &r, &mut Progress::default(), Some(0), [q, 0], [max; 2], Some(members), 0, &mut seed) else { panic!() };
        assert_eq!((next.wins[0], next.health[0], next.carry[0]), (1, Some(q + add), Some([(q + add, 2 * q + add), (max, max)])));
    }

    #[test]
    fn theater_lists_the_endings_seen() {
        // Paul's costumes play ending 1, Law's ending 2 (here Unknown's, a movie)
        let endings = serde_json::json!({"table": [[1, 1], [2]], "endings": [{}, {}, {}], "playback": {"unknown": 2},
            "theater": {"labels": [["(1P)PAUL", "(2P)PAUL"], ["(1P)LAW"]], "extras": [{"label": "OPENING MOVIE", "movie": 14}, {"label": "EMBU", "scene": 1}, {"label": "EMBU2", "cast": 1}]}});
        let e = EndingData::new(endings, &Value::Null).unwrap();
        let d = Data::load(&serde_json::json!({"front_end": {}}), &Value::Null, &serde_json::json!({}), Some(e), vec![], None, false, &Value::Null).unwrap();
        let rows = |seen| theater(&d, seen).into_iter().map(|r| r.into_iter().map(|c| (c.0, c.1)).collect::<Vec<_>>()).collect::<Vec<_>>();
        let tail =
            [vec![("OPENING MOVIE".to_string(), Cell::Movie(14))], vec![("EMBU".into(), Cell::None)], vec![("MUSIC PLAYER".into(), Cell::Music)], vec![("EXIT THEATER MENU".into(), Cell::Exit)]];
        assert_eq!(rows(0), tail);
        let paul = vec![("(1P)PAUL".to_string(), Cell::Ending(1, 0)), ("(2P)PAUL".into(), Cell::Ending(1, 1))];
        assert_eq!(rows(1 << 1)[..2], [paul.clone(), tail[0].clone()]);
        assert_eq!(rows(1 << 1 | 1 << 2)[..2], [paul, vec![("(1P)LAW".into(), Cell::Ending(2, 0))]]);
        // the menu shows THEATER once an ending was seen
        assert!(!Front::items(&d, &Progress::default()).iter().any(|i| i.0 == THEATER));
    }

    #[test]
    fn staff_roll_lines_and_length() {
        // a heading, an alternative pair (the first shows), a name, the end mark, a line past it
        let line = |kind: u32, text: &str, font: u32, alt: bool| serde_json::json!({"kind": kind, "text": text, "font": font, "alternative": alt});
        let lines = [line(0, "A", 7, false), line(0, "SECRET", 7, true), line(0, "BOWL", 7, true), line(1, "B", 5, false), line(2, "", 65535, false), line(1, "C", 5, false)];
        let e = EndingData::new(serde_json::json!({"table": [], "endings": [], "staff_roll": {"lines": lines}}), &Value::Null).unwrap();
        assert_eq!(roll_lines(&e), [(0, "A", 7), (0, "SECRET", 7), (1, "B", 5)]);
        // the cards' 1027 frames, a line each 22, 140 to stop, 210 still
        assert_eq!(roll_frames(&e), 1027 + 2 * 22 + 140 + 210);
    }

    #[test]
    fn tekken_bowl_is_on_the_menu_once_unlocked() {
        let fe = serde_json::json!({"front_end": {"menu": {"items": [{"mode": 0, "label": "ARCADE", "locked": false},
            {"mode": 8, "label": "TEKKEN BOWL", "locked": true}, {"mode": 9, "label": "THEATER", "locked": true}]},
            "unlocks": {"schedule": [{"stage": 10, "kind": 5, "character": 0}]}}});
        let d = Data::load(&fe, &Value::from(vec![0]), &serde_json::json!({}), None, vec![], None, false, &Value::Null).unwrap();
        let modes = |stage| Front::items(&d, &Progress { stage, ..Progress::default() }).iter().map(|i| i.0).filter(|&m| m != u32::MAX).collect::<Vec<_>>();
        assert_eq!((modes(9), modes(10)), (vec![ARCADE], vec![ARCADE, BOWL]));
        // its select: P1 picks two; 2P's START joins
        let mut s = Select::new(BOWL, &d, None);
        assert_eq!((s.sides[0].need, s.sides[1].player), (2, None));
        let roster = roster();
        s.key(1, Key::Start, &d, &roster, &Progress::default());
        assert_eq!(s.sides[1].player, Some(1));
    }

    /// TEKKEN BALL is on the menu (before OPTIONS) only with the Tekken 3 slice's court; its run is Ball there, one
    /// on one against a CPU.
    #[test]
    fn tekken_ball_with_the_slice() {
        let fe = serde_json::json!({"front_end": {"menu": {"items": [{"mode": 0, "label": "ARCADE"}, {"mode": 11, "label": "OPTIONS"}]}}});
        let modes = |ball| {
            Front::items(&Data::load(&fe, &Value::Null, &serde_json::json!({}), None, vec![], ball, false, &Value::Null).unwrap(), &Progress::default())
                .iter()
                .map(|i| i.0)
                .filter(|&m| m != u32::MAX)
                .collect::<Vec<_>>()
        };
        assert_eq!((modes(None), modes(Some(23))), (vec![ARCADE, OPTIONS], vec![ARCADE, BALL, OPTIONS]));
        let (d, roster, mut seed) = (data(), roster(), 1);
        let d = Arc::new(Data { ball: Some(23), ..Arc::try_unwrap(d).unwrap() });
        let run = Run::new(&d, &roster, &Progress::default(), BALL, [vec![(0, 0)], vec![]], &mut seed);
        let s = run.setup(&d);
        assert_eq!((s.mode, s.stage, s.ball, run.bench(), run.teams[1].len()), (Mode::Ball, 23, Some((0, 0)), None, 2));
        // the ball type chosen after the select (IRON)
        let mut front = Front { page: Page::BallType { run: Box::new(run), cursor: 0 }, frame: 0 };
        let mut roster = roster;
        let mesh = serde_json::json!({"vertices": [], "normals": [], "triangles": [], "quads": []});
        let kind = serde_json::json!({"speed_unit": 0, "lob_unit": 0, "gravity": 0, "ambient": [0, 0, 0], "damage_percent": 0, "zone": 0, "level": "", "name": "", "colours": []});
        let types = vec![kind; 3];
        roster.ball = Some(Arc::new(serde_json::from_value(serde_json::json!({"types": types, "mesh": mesh, "body_offsets": []})).unwrap()));
        roster.ttt = Some(d.clone());
        // (with a pair each and RIGHT, 2 ON 2: the seconds as the partners, the CPU's)
        if let Page::BallType { run, .. } = &mut front.page {
            run.teams = [vec![(0, 0), (1, 0)], vec![(2, 0), (3, 0)]];
        }
        for k in [Key::Down, Key::Down, Key::Right, Key::Start] {
            if let Outcome::TttFight(r) = front.key(0, k, &roster, &mut seed) {
                let s = r.setup(&d);
                assert_eq!(s.ball, Some((2, 0)));
                assert_eq!(s.partners, Some(crate::menu::Doubles { partners: [(1, 0), (3, 0)], seats: [Some(0), None, None, None] }));
                return;
            }
        }
        panic!("no fight");
    }

    /// The CPU's profile (0x380120, measured in PCSX2: fight n of the arcade plays NORMAL level n - 1): the
    /// DIFFICULTY option (all five) and the fight's number, TIME ATTACK at NORMAL, SURVIVAL by ai.json's streak ladder.
    #[test]
    fn the_cpu_profile_follows_the_mode() {
        let (d, roster, seed) = (data(), roster(), 1u32);
        let rows = serde_json::json!({"survival": {"rows": [{"max_wins": 4, "difficulty": 0, "levels": [0, 3]}, {"max_wins": 99, "difficulty": 4, "levels": [5, 7]}, {"max_wins": 764, "difficulty": 0, "levels": [0, 0]}]}});
        let d = Arc::new(Data {
            survival: Data::load(&serde_json::json!({"front_end": {}}), &Value::Null, &Value::Null, None, vec![], None, false, &rows).unwrap().survival.clone(),
            ..Arc::try_unwrap(d).unwrap()
        });
        let profile = |mode, fight, wins| {
            let mut run = Run::new(&d, &roster, &Progress::default(), mode, [vec![(0, 0)], vec![]], &mut seed.clone());
            (run.fight, run.wins[0], run.options[0]) = (fight, wins, 4);
            let s = run.setup(&d);
            (s.difficulty, s.level)
        };
        assert_eq!((0..9).map(|f| profile(ARCADE, f, 0)).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5, 6, 7, 7].map(|l| (4, l)));
        assert_eq!(profile(TIME_ATTACK, 5, 0), (1, 5));
        assert_eq!(profile(SURVIVAL, 0, 764), (0, 0));
        for wins in 0..100 {
            let (diff, level) = profile(SURVIVAL, 0, wins);
            assert!(if wins <= 4 { diff == 0 && level <= 3 } else { (diff == 4 || diff == 1) && (5..=7).contains(&level) }, "{wins}: {diff} {level}");
        }
    }

    /// TEKKEN FORCE is on the menu (before OPTIONS) only with the Tekken 3 slice's force.json; one pick.
    #[test]
    fn tekken_force_with_the_slice() {
        let fe = serde_json::json!({"front_end": {"menu": {"items": [{"mode": 0, "label": "ARCADE"}, {"mode": 11, "label": "OPTIONS"}]}}});
        let d = |force| Data::load(&fe, &Value::Null, &serde_json::json!({}), None, vec![], None, force, &Value::Null).unwrap();
        let modes = |force| Front::items(&d(force), &Progress::default()).iter().map(|i| i.0).filter(|&m| m != u32::MAX).collect::<Vec<_>>();
        assert_eq!((modes(false), modes(true)), (vec![ARCADE, OPTIONS], vec![ARCADE, FORCE, OPTIONS]));
        assert_eq!(Select::new(FORCE, &d(true), None).sides[0].need, 1);
    }

    #[test]
    fn arcade_arenas_join_the_pool_and_the_stage_select() {
        let mut d = Arc::try_unwrap(data()).unwrap();
        let (mut p, mut seed) = (Progress::default(), 5);
        // without the arcade pack: Tekken Tag's fight arenas (not Tekken Bowl's, 20, nor 21, 22), no ARCADE ARENAS row
        assert_eq!(d.pool(&p), (0..20).collect::<Vec<_>>());
        assert_eq!((game_rows(&d).contains(&ARCADE_ROW), stage_list(&d).len()), (false, 21));
        // with it: its 13 arenas and the hidden one, ids of their own, its files, the music
        d.arcade = (ARCADE_ARENA..ARCADE_ARENA + 13).chain([ARCADE_HIDDEN]).map(|id| (id, format!("T{id}"))).collect();
        let pool = d.pool(&p);
        assert_eq!(pool.len(), 20 + 14);
        assert!(d.arcade.iter().all(|a| d.arenas.iter().all(|t| t.0 != a.0) && arcade_file(a.0).is_some()));
        assert_eq!((arcade_file(112).as_deref(), arcade_file(ARCADE_HIDDEN).as_deref(), arcade_file(22)), (Some("arcade/stage_12.glb"), Some("arcade/hidden.glb"), None));
        assert_eq!((arcade_bgm(100), arcade_bgm(112), arcade_bgm(ARCADE_HIDDEN), arcade_bgm(3)), (Some(9), Some(21), Some(22), None));
        assert!((0..200).any(|_| Run::new(&d, &roster(), &p, SURVIVAL, [vec![(0, 0)], vec![]], &mut seed).stage >= ARCADE_ARENA));
        // OPTIONS' ARCADE ARENAS off: Tekken Tag's alone again; the stage select lists both kinds still
        p.arcade_off = true;
        assert_eq!(d.pool(&p).len(), 20);
        assert!((0..200).all(|_| Run::new(&d, &roster(), &p, SURVIVAL, [vec![(0, 0)], vec![]], &mut seed).stage < ARCADE_ARENA));
        let list = stage_list(&d);
        assert_eq!((game_rows(&d).contains(&ARCADE_ROW), list.len(), list[0].0, list[1].0, list[21].0, list[34].0), (true, 35, None, Some(0), Some(100), Some(ARCADE_HIDDEN)));
        // VS: the select's run waits on the stage select; a pick there is the fight's arena
        let (r, d) = (roster(), Arc::new(d));
        let mut s = Select::new(VS, &d, None);
        s.sides[0].picks = vec![(0, 0), (1, 0)];
        s.sides[1].picks = vec![(2, 0), (3, 0)];
        s.sides.iter_mut().for_each(|x| x.done = true);
        let run = (0..90).find_map(|_| s.tick(&d, &r, &p, &mut seed)).unwrap();
        let mut front = Front { page: Page::Stage { run: Box::new(run), cursor: 21 }, frame: 0 };
        let mut r = r;
        r.ttt = Some(d.clone());
        assert!(matches!(front.key(0, Key::Start, &r, &mut seed), Outcome::TttFight(run) if run.stage == 100));
    }

    #[test]
    fn select_picks_a_tag_pair_and_skips_locked() {
        let (d, r) = (data(), roster());
        let (mut p, mut seed) = (Progress::default(), 3);
        let mut s = Select::new(ARCADE, &d, None);
        assert_eq!(s.sides[0].cursor, 0);
        // the locked character's slot takes no pick; Paul's (0) and Law's (1) do
        s.sides[0].cursor = 2;
        assert!(s.key(0, Key::Button(0), &d, &r, &p).is_none() && s.sides[0].picks.is_empty());
        s.sides[0].cursor = 0;
        s.key(0, Key::Button(2), &d, &r, &p);
        s.key(0, Key::Button(0), &d, &r, &p);
        assert_eq!(s.sides[0].picks, vec![(0, 1)]);
        s.key(0, Key::Right, &d, &r, &p);
        s.key(0, Key::Button(0), &d, &r, &p);
        assert!(s.sides[0].done && s.sides[0].picks == vec![(0, 1), (1, 0)]);
        // the fight 90 frames after the last pick (0x3C9888)
        assert!((1..90).all(|_| s.tick(&d, &r, &p, &mut seed).is_none()));
        let run = s.tick(&d, &r, &p, &mut seed).unwrap();
        assert_eq!((run.mode, run.ladder.len(), run.teams[0].len()), (ARCADE, 8, 2));
        p.stage = 1;
        assert!(d.selectable(p.stage, 2));
    }

    /// The ranking's pace (0x3C8A88): 32 frames in from black; 7 rows: the last 2 down 150 from above at 12 a frame
    /// (13 frames), 130 held, the first 5 down 375 (32 frames), 130 held, 60 more, 32 out to black.
    #[test]
    fn ranking_pages_down_at_the_ps2s_pace() {
        let d = data();
        let at = |f: u32| ranking_pace(&d, 2, 7, f);
        assert_eq!(at(0), Some((135.0 - 525.0, 1.0 - 8.0 / 256.0)));
        assert_eq!(at(33), Some((-390.0 + 12.0, 0.0)));
        assert_eq!(at(33 + 12), Some((-240.0, 0.0)));
        assert_eq!(at(33 + 13 + 129), Some((-240.0, 0.0)));
        assert_eq!(at(33 + 13 + 130 + 31), Some((135.0, 0.0)));
        let end = 33 + 13 + 130 + 32 + 130 + 60 + 1 + 32;
        assert!(at(end - 1).is_some_and(|(top, black)| top == 135.0 && black == 1.0) && at(end).is_none());
    }

    /// CHARACTER SELECT CURSOR HOLD: a pick keeps its slot, the cursor goes on to the second pick's slot kept, back
    /// forgets it (0x3CAB18, 0x3CAB8C, 0x3C94C0).
    #[test]
    fn select_cursor_hold_keeps_the_picks_slots() {
        let (d, r) = (data(), roster());
        let p = Progress { settings: vec![0, 0, 1], ..Progress::default() };
        let mut s = Select::new(ARCADE, &d, None);
        (s.memory, s.sides[0].cursor) = ([[None, Some(1)], [None; 2]], 0);
        s.key(0, Key::Button(0), &d, &r, &p);
        assert_eq!((s.memory[0], s.sides[0].cursor), ([Some(0), Some(1)], 1));
        s.key(0, Key::Back, &d, &r, &p);
        assert_eq!(s.memory[0], [Some(0), None]);
    }

    /// The select's sounds (group 0: 3 cursor, 4 select 1, 5 select 2), RANDOM's roll and the timer (FRONTEND.md 9).
    #[test]
    fn select_sounds_random_roll_and_timer() {
        let (d, r, p, mut seed) = (data(), roster(), Progress::default(), 3);
        let mut s = Select::new(ARCADE, &d, None);
        // RANDOM rolls through the selectable characters, one every 3 frames, but the side's picks
        assert_eq!(
            [0, 3, 6].map(|f| {
                s.frame = f;
                s.roll(&d, &r, &p, 0)
            }),
            [Some(0), Some(1), Some(0)]
        );
        sfx::take();
        s.key(0, Key::Right, &d, &r, &p);
        s.key(0, Key::Button(0), &d, &r, &p);
        s.sides[0].cursor = 3;
        s.frame = 3;
        assert_eq!(s.roll(&d, &r, &p, 0), Some(0), "Law is taken");
        s.key(0, Key::Button(0), &d, &r, &p);
        assert_eq!((sfx::take(), s.sides[0].picks.clone()), (vec![3, 4, 5], vec![(1, 0), (0, 0)]));
        // the select's music from its frame 18
        let mut f = Front { page: Page::Select(Box::new(Select::new(ARCADE, &d, None))), frame: 0 };
        assert_eq!(f.music(Some(&d)), None);
        if let Page::Select(s) = &mut f.page {
            s.frame = 18;
        }
        assert_eq!((f.music(Some(&d)), f.music(None)), (Some(0), None));
        // CONTINUE?'s announcer: 8 as the 8 comes
        let mut r2 = roster();
        r2.ttt = Some(d.clone());
        let run = Run::new(&d, &r2, &p, ARCADE, [vec![(0, 0)], vec![]], &mut seed);
        let mut f = Front { page: Page::Continue { run: Box::new(run), countdown: 800 }, frame: 0 };
        sfx::take();
        f.tick(&r2, &mut seed);
        f.tick(&r2, &mut seed);
        assert_eq!(sfx::take(), vec![0x1047 + 8]);
        // 2400 frames: both picks on one frame, the cursor's character then RANDOM's (the cursor's is taken)
        let mut s = Select::new(ARCADE, &d, None);
        assert!((1..2400).all(|_| s.tick(&d, &r, &p, &mut seed).is_none()) && s.sides[0].picks.is_empty());
        s.tick(&d, &r, &p, &mut seed);
        assert!(s.sides[0].done && s.sides[0].picks.len() == 2);
    }

    /// The costume a button gives (0x3C9ED8): square 0, cross 1, triangle 2 else 0, circle 3 else 1, START the fifth
    /// only; two players: the second to take the first's character and costume gets another (0x3C0210).
    #[test]
    fn select_costume_per_button() {
        let (d, mut r, p) = (data(), roster(), Progress::default());
        let picked = |key: Key, r: &Roster| {
            let mut s = Select::new(ARCADE, &d, None);
            s.key(0, key, &d, r, &p);
            s.sides[0].picks.first().map(|q| q.1)
        };
        assert_eq!([Key::Button(0), Key::Button(2), Key::Button(1), Key::Button(3), Key::Start].map(|k| picked(k, &r)), [Some(0), Some(1), Some(0), Some(1), None]);
        // a character with a third, fourth and fifth costume
        r.characters[0].costumes = vec![10, 20, 30, 40, 50];
        r.costume_of.extend([(30, 2), (40, 3), (50, 4)]);
        assert_eq!([Key::Button(1), Key::Button(3), Key::Start, Key::Button(4)].map(|k| picked(k, &r)), [Some(2), Some(3), Some(4), Some(4)]);
        let mut s = Select::new(VS, &d, None);
        s.sides[1].cursor = 0;
        s.key(0, Key::Button(0), &d, &r, &p);
        s.key(1, Key::Button(0), &d, &r, &p);
        assert_eq!((s.sides[0].picks[0], s.sides[1].picks[0]), ((0, 0), (0, 2)));
    }

    /// The pause menu: the cursor goes round its five entries; RESET opens RESET? on NO, left takes YES, a button
    /// resets; CANCEL closes.
    #[test]
    fn pause_reset_asks_first() {
        let d = data();
        let (mut pause, mut reset) = (Some((0, 0)), None);
        assert!(!pause_key(&d, &PAUSE_ITEMS, &mut pause, &mut reset, Key::Up));
        assert_eq!(pause, Some((PAUSE_ITEMS.len() - 1, 0)));
        pause = Some((1, 0));
        assert!(!pause_key(&d, &PAUSE_ITEMS, &mut pause, &mut reset, Key::Button(0)));
        assert_eq!(reset, Some(false));
        pause_key(&d, &PAUSE_ITEMS, &mut pause, &mut reset, Key::Left);
        assert!(pause_key(&d, &PAUSE_ITEMS, &mut pause, &mut reset, Key::Button(0)));
        assert_eq!((pause, reset), (None, None));
        pause = Some((0, 1));
        pause_key(&d, &PAUSE_ITEMS, &mut pause, &mut reset, Key::Button(0));
        assert_eq!(pause, None);
    }

    /// Records: TIME ATTACK fastest first, SURVIVAL most wins first (none for 0 wins), ten of each.
    #[test]
    fn records_keep_the_best_ten() {
        let d = data();
        let mut p = Progress::default();
        let r = |score: u32| Record { score, members: vec![7], name: "ABC".into() };
        for t in [500, 300, 400] {
            p.record(&d, false, r(t));
        }
        assert_eq!(p.times.iter().map(|t| t.score).collect::<Vec<_>>(), [300, 400, 500]);
        assert_eq!(p.rank(&d, true, &r(0)), None);
        // a new save's SURVIVAL records are the PS2's (`records.defaults`); CHARACTER USAGE lists the selectable at 0.0 %
        assert_eq!(p.rank(&d, true, &r(3)), Some(1));
        assert_eq!(record_rows(&d, &p, 0), [(vec![0], 0, String::new()), (vec![1], 0, String::new())]);
        // BGM VOLUME 5 of the table, SE VOLUME 10, STEREO
        assert_eq!(volumes(&d, &Progress { settings: vec![1, 0, 0, 0, 1, 1, 1, 5, 10], ..p.clone() }), Some((0.5, 1.0, false)));
        for w in 1..=12 {
            p.record(&d, true, r(w));
        }
        assert_eq!((p.survivors.len(), p.survivors[0].score, p.rank(&d, true, &r(2))), (RECORDS, 12, None));
    }

    /// The name entry (0x3D0350): right / down the next letter, left / up the one before, round; a face button takes
    /// the letter; the time out (0x3C9124) takes the name as it is.
    #[test]
    fn name_entry_keys_and_time_out() {
        let mut r = roster();
        r.ttt = Some(data());
        let mut seed = 0;
        let record = Record { score: 900, members: vec![0, 1], name: String::new() };
        let mut f = Front { page: Page::NameEntry { survival: true, record, letter: 0, then: Box::new(Page::Title) }, frame: 0 };
        for k in [Key::Up, Key::Down, Key::Right, Key::Button(2), Key::Left] {
            f.key(0, k, &r, &mut seed);
        }
        assert!(matches!(&f.page, Page::NameEntry { record, letter: 0, .. } if record.name == "B"));
        while matches!(f.page, Page::NameEntry { .. }) && f.frame < 20 {
            f.tick(&r, &mut seed);
        }
        assert!(matches!(f.page, Page::Title) && progress().survivors.first().is_some_and(|s| s.name == "B" && s.score == 900));
    }

    /// Practice's menu: MODE SELECT's left / right turn the modes, a row's left / right its values (round), CANCEL and
    /// RESET take; the dummy and VS CPU's CPU as set.
    #[test]
    fn practice_menu_modes_and_values() {
        let d = data();
        let mut p = Practice::new(&d);
        p.open(&d);
        for k in [Key::Down, Key::Down, Key::Right] {
            assert_eq!(p.key(&d, k), PracticeAction::None);
        }
        assert_eq!(p.dummy(&d), 1);
        p.cursor = 0;
        p.key(&d, Key::Left);
        assert_eq!((p.mode, p.cpu(&d)), (1, Some(1)));
        assert_eq!(p.key(&d, Key::Up), PracticeAction::None);
        assert!(p.key(&d, Key::Button(0)) == PracticeAction::Reset && !p.open);
    }

    /// The main menu's rows slide a row's height in 4 a frame; no move while they slide (0x3BD2E0).
    #[test]
    fn menu_rows_slide_into_place() {
        let mut r = roster();
        r.ttt = Some(data());
        let mut seed = 0;
        let mut f = Front::menu();
        f.key(0, Key::Down, &r, &mut seed);
        f.key(0, Key::Down, &r, &mut seed);
        let scrolls: Vec<i32> = (0..8)
            .map(|_| {
                f.tick(&r, &mut seed);
                match f.page {
                    Page::Menu { scroll, .. } => scroll,
                    _ => -1,
                }
            })
            .collect();
        assert_eq!(scrolls, [22, 18, 14, 10, 6, 2, 0, 0]);
        assert!(matches!(f.page, Page::Menu { cursor: 1, .. }));
    }
}
