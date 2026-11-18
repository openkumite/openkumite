//! Tekken Tag Tournament's front end, when the game data has it (roster.json `front_end`, which the Tekken Tag
//! converter reads from the PS2 executable: its docs/FRONTEND.md): the title, the main menu and its modes (Gallery is
//! not here yet; Theater once an ending was seen, Tekken Bowl once unlocked), the character select with the tag
//! partner, OPTIONS, the arcade ladder with CONTINUE, the modes' runs, the unlock schedule, the endings
//! (`gamedata/endings.json`: the arcade clear's staff roll and ending, Theater), the fight HUD and Tekken Bowl's, drawn
//! from its 2D pictures (`pictures/FFF_NNN.png`) on its 640 x 448 screen.
//!
//! Not decoded, OpenKumite's own: the VS screen, the select screen's layout below the grid and its timer (none),
//! which button gives which costume, the banners (in the chrome font), the menu, option and Theater screens' layout,
//! the staff roll's fonts, and which of the six health-bar strips is which.

use crate::art::{additive, Art, Screen as View, SCREEN_H, SCREEN_W};
use crate::menu::{lcg, sfx, FightSetup, Key, Mode, Outcome, Roster};
use egui::{Color32, Pos2, Rect};
use kumite_game::bout::{Announcement, Bout, Phase};
use kumite_game::ending::EndingData;
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
const OPTIONS: u32 = 11;
/// OPTIONS' rows the engine plays (the config's DIFFICULTY LEVEL, FIGHT COUNT, ROUND TIME), then EXIT.
const OPTION_ROWS: [usize; 3] = [0, 1, 2];
/// The last unlock stage (FRONTEND.md 7).
const LAST_STAGE: u32 = 14;
/// Frames the result and message screens stay (START skips).
const MESSAGE_FRAMES: u32 = 360;
/// Frames between every side's last pick and the fight.
const SELECT_DONE: u32 = 40;
/// The VS screen's frames (OpenKumite's: the original's is not decoded).
pub const VS_FRAMES: u32 = 120;

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
    /// The arenas to fight on (stages/index.json).
    pub arenas: Vec<usize>,
    /// ROUND TIME's clocks in frames (rounds.json `round_time`; past them: untimed).
    pub round_frames: Vec<u32>,
    /// The endings, the staff roll and Theater (endings.json; None: a pack without them).
    pub endings: Option<Arc<EndingData>>,
}

impl Data {
    /// From roster.json (None without `front_end`: an older pack, the plain front end); `arenas`: the pack's;
    /// `rounds`: rounds.json; `endings`: endings.json's.
    pub fn load(roster: &Value, arenas: Vec<usize>, rounds: &Value, endings: Option<EndingData>) -> Option<Arc<Self>> {
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
            arenas: arenas.into_iter().filter(|&a| a != 20).collect(),
            round_frames: ints(&rounds["round_time"]["options_frames"]).into_iter().map(|f| f as u32).collect(),
            endings: endings.map(Arc::new),
            raw,
        }))
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
}

/// The save's progress as last loaded or changed (the menus read it; the app keeps it in the save), as the
/// DIFFICULTY of the Tekken 3 menus.
static PROGRESS: std::sync::Mutex<Progress> = std::sync::Mutex::new(Progress { stage: 0, points: 0, seen: 0, options: None });

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

/// One side's fighters: (roster index, costume).
pub type Team = Vec<(usize, usize)>;

/// A mode's run: who fights, the ladder, the score.
#[derive(Clone, Debug)]
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
}

fn tag(mode: u32) -> bool {
    matches!(mode, ARCADE | VS | TIME_ATTACK)
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
            stage: d.arenas.get(lcg(seed, d.arenas.len())).copied().unwrap_or(0),
            options: progress.options(d),
        };
        if ladder_mode(mode) {
            run.ladder = ladder(d, roster, progress.stage, &run.teams[0], tag(mode), seed);
        }
        if run.teams[1].is_empty() && mode != BOWL {
            // Team Battle against the CPU: as many as the player's; Survival: the first opponent
            let n = if mode == TEAM { run.teams[0].len() } else { 1 };
            run.teams[1] = cpu_team(d, roster, progress.stage, &run.teams[0], n, false, seed);
        }
        run
    }

    /// The fight's setup now.
    pub fn setup(&self) -> FightSetup {
        let mode = match self.mode {
            ARCADE | ONE_ARCADE => Mode::Arcade,
            VS | ONE_VS => Mode::Versus,
            TEAM => Mode::TeamBattle,
            TIME_ATTACK => Mode::TimeAttack,
            SURVIVAL => Mode::Survival,
            _ => Mode::Practice,
        };
        let cpu = self.ladder.get(self.fight);
        let side = |k: usize| match (k, cpu) {
            (1, Some((t, _))) => t.first().copied().unwrap_or((0, 0)),
            _ => self.teams[k].get(self.member[k]).or(self.teams[k].first()).copied().unwrap_or((0, 0)),
        };
        let level = (self.fight * 9 / 7).min(9);
        let stage = cpu.map_or(self.stage, |c| c.1);
        FightSetup { mode, players: [side(0), side(1)], stage, difficulty: self.options[0].min(2), level, handicap: [100; 2], ball: None, partners: None }
    }

    /// The CPU plays side `k` (practice's dummy stands).
    pub fn cpu(&self, k: usize) -> bool {
        !self.humans[k] && self.mode != PRACTICE
    }

    /// The arcade ladder's STAGE (1..8).
    pub fn stage_number(&self) -> Option<u32> {
        ladder_mode(self.mode).then_some(self.fight as u32 + 1)
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
    pub fn after(mut self, d: &Data, roster: &Roster, progress: &mut Progress, winner: Option<usize>, health: [i32; 2], max: [i32; 2], frames: u32, seed: &mut u32) -> Next {
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
                let page = Page::Message { lines, frame: 0, title: true };
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
            ARCADE | TIME_ATTACK | ONE_ARCADE => {
                progress.played(d, None);
                Next::Screen(Page::Continue { run: Box::new(self), countdown: CONTINUE })
            }
            SURVIVAL if won => {
                self.wins[0] += 1;
                self.health[0] = Some(health[0]);
                self.teams[1] = cpu_team(d, roster, progress.stage, &self.teams[0], 1, false, seed);
                self.stage = d.arenas.get(lcg(seed, d.arenas.len())).copied().unwrap_or(self.stage);
                Next::Fight(Box::new(self))
            }
            SURVIVAL => {
                progress.played(d, None);
                Next::Screen(Page::Message { lines: vec!["SURVIVAL".into(), format!("WINS {}", self.wins[0])], frame: 0, title: false })
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
fn ladder(d: &Data, roster: &Roster, stage: u32, players: &Team, tag: bool, seed: &mut u32) -> Vec<(Team, usize)> {
    let a = &d.raw["arcade"];
    let n = if tag { 2 } else { 1 };
    let fights = int(&a["fights"]).max(2);
    let mut arenas: Vec<usize> = d.arenas.iter().copied().filter(|&x| x != int(&a["rival_arena"]) && x != int(&a["boss_arena"])).collect();
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

/// CONTINUE?'s frames (0x3BA560: 9 down to 0, 100 frames a digit).
const CONTINUE: u32 = 898;

/// A screen of the front end.
pub enum Page {
    Title,
    Menu {
        cursor: usize,
    },
    Options {
        cursor: usize,
        values: [usize; 3],
    },
    Select(Box<Select>),
    /// The arcade ladder lost: CONTINUE? 9..0, then GAME OVER.
    Continue {
        run: Box<Run>,
        countdown: u32,
    },
    /// A run's end (or GAME OVER): its lines, then the title or the main menu.
    Message {
        lines: Vec<String>,
        frame: u32,
        title: bool,
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
}

pub struct Select {
    pub mode: u32,
    pub sides: [Side; 2],
    pub frame: u32,
    pub done: u32,
    /// VS: the tally so far.
    pub tally: Option<[u32; 2]>,
}

impl Select {
    pub fn new(mode: u32, d: &Data, tally: Option<[u32; 2]>) -> Self {
        let need = match mode {
            TEAM => 8,
            m if tag(m) => 2,
            _ => 1,
        };
        // (the cursors start on the two RANDOM slots' neighbours: Paul's and Jin's rows, OpenKumite's)
        let first = |c: usize| d.slots.iter().position(|s| s.character == Some(c)).unwrap_or(0);
        let p2 = matches!(mode, VS | ONE_VS);
        let side = |player: Option<usize>, c: usize| Side { player, cursor: first(c), picks: vec![], need, done: false };
        let mut sides = [side(Some(0), 0), side(p2.then_some(1), 9)];
        // practice: P1 picks the dummy too
        if mode == PRACTICE {
            sides[1] = Side { player: Some(0), need: 1, ..side(Some(0), 9) };
        }
        Self { mode, sides, frame: 0, done: 0, tally }
    }

    /// The side `player`'s keys move now.
    fn side_of(&self, player: usize) -> Option<usize> {
        (0..2).find(|&s| self.sides[s].player == Some(player) && !self.sides[s].done)
    }

    fn key(&mut self, player: usize, key: Key, d: &Data, roster: &Roster, progress: &Progress, seed: &mut u32) -> Option<Page> {
        // Team Battle, Tekken Bowl: 2P's START joins
        if matches!(self.mode, TEAM | BOWL) && player == 1 && key == Key::Start && self.sides[1].player.is_none() {
            sfx::play(sfx::join());
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
                    sfx::play(sfx::select_cursor());
                    side.cursor = at;
                }
            }
            Key::Start if self.mode == TEAM && !side.picks.is_empty() => {
                sfx::play(sfx::pick());
                side.done = true;
            }
            Key::Button(_) | Key::Start => {
                let slot = d.slots.get(side.cursor)?;
                let taken: Vec<usize> = side.picks.iter().map(|p| p.0).collect();
                let id = match slot.character {
                    Some(c) => Some(c).filter(|&c| d.selectable(progress.stage, c)),
                    None => {
                        let free: Vec<usize> = roster.characters.iter().map(|c| c.id).filter(|&c| d.selectable(progress.stage, c) && roster.index_of(c).is_some_and(|i| !taken.contains(&i))).collect();
                        free.get(lcg(seed, free.len())).copied()
                    }
                };
                let i = id.and_then(|c| roster.index_of(c)).filter(|i| !taken.contains(i))?;
                // shortcut: buttons 1-4 give costumes 1-4, START the fifth (which pad bit gives which costume is not
                // decoded), a costume the character lacks its first
                let n = roster.characters[i].costumes.len().max(1);
                let costume = match key {
                    Key::Button(b) => b,
                    _ => 4,
                };
                sfx::play(sfx::pick());
                side.picks.push((i, if costume < n { costume } else { 0 }));
                side.done = side.picks.len() >= side.need;
            }
            Key::Back => {
                if side.picks.pop().is_none() && s == 0 {
                    sfx::play(sfx::back());
                    return Some(Page::Menu { cursor: 0 });
                }
            }
        }
        None
    }

    fn tick(&mut self, d: &Data, roster: &Roster, progress: &Progress, seed: &mut u32) -> Option<Run> {
        self.frame += 1;
        let ready = self.sides.iter().all(|s| s.player.is_none() || s.done);
        if !ready {
            self.done = 0;
            return None;
        }
        self.done += 1;
        if self.done < SELECT_DONE {
            return None;
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

    pub fn menu() -> Self {
        Self { page: Page::Menu { cursor: 0 }, frame: 0 }
    }

    /// The main menu's entries: of the locked ones, THEATER once an ending was seen (0x3BFED8), TEKKEN BOWL once the
    /// unlock stage opens it (Gallery is not here yet).
    fn items(d: &Data, p: &Progress) -> Vec<(u32, String)> {
        let open = |i: &&Value| {
            let mode = int(&i["mode"]) as u32;
            i["locked"] != true || (mode == THEATER && p.seen != 0 && d.endings.is_some()) || (mode == BOWL && d.bowl_open(p.stage))
        };
        let mut items: Vec<(u32, String)> =
            d.raw["menu"]["items"].as_array().into_iter().flatten().filter(open).map(|i| (int(&i["mode"]) as u32, i["label"].as_str().unwrap_or_default().to_string())).collect();
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
                    sfx::play(sfx::start());
                    Some(Page::Menu { cursor: 0 })
                }
                Key::Back => return Outcome::Quit,
                _ => None,
            },
            Page::Menu { cursor } => {
                let items = Self::items(&d, progress);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sfx::cursor());
                        *cursor = (*cursor + if key == Key::Up { items.len() - 1 } else { 1 }) % items.len().max(1);
                        None
                    }
                    Key::Start | Key::Button(_) => {
                        sfx::play(sfx::confirm());
                        match items.get(*cursor).map(|i| i.0) {
                            Some(u32::MAX) => return Outcome::Exit,
                            Some(OPTIONS) => Some(Page::Options { cursor: 0, values: progress.options(&d) }),
                            Some(THEATER) => Some(Page::Theater { cursor: [0, 0] }),
                            Some(m) => Some(Page::Select(Box::new(Select::new(m, &d, None)))),
                            None => None,
                        }
                    }
                    Key::Back => Some(Page::Title),
                    _ => None,
                }
            }
            Page::Options { cursor, values } => match key {
                Key::Up | Key::Down => {
                    sfx::play(sfx::cursor());
                    *cursor = (*cursor + if key == Key::Up { OPTION_ROWS.len() } else { 1 }) % (OPTION_ROWS.len() + 1);
                    None
                }
                Key::Left | Key::Right if *cursor < OPTION_ROWS.len() => {
                    sfx::play(sfx::cursor());
                    let n = d.option(OPTION_ROWS[*cursor]).1.len().max(1);
                    values[*cursor] = (values[*cursor] + if key == Key::Left { n - 1 } else { 1 }) % n;
                    None
                }
                // EXIT (or back): kept in the save
                Key::Start | Key::Button(_) | Key::Back if key == Key::Back || *cursor == OPTION_ROWS.len() => {
                    sfx::play(sfx::back());
                    progress.options = Some(*values);
                    set_progress(progress.clone());
                    let at = Self::items(&d, progress).iter().position(|i| i.0 == OPTIONS).unwrap_or(0);
                    self.page = Page::Menu { cursor: at };
                    return Outcome::TttSaved;
                }
                _ => None,
            },
            Page::Select(s) => s.key(player, key, &d, roster, progress, seed),
            Page::Continue { run, countdown } => match key {
                // START continues the same fight; a face button skips 100 frames
                Key::Start => {
                    sfx::play(sfx::start());
                    return Outcome::TttFight(run.clone());
                }
                Key::Button(_) => {
                    *countdown = countdown.saturating_sub(100);
                    None
                }
                _ => None,
            },
            Page::Message { title, .. } => match key {
                Key::Start | Key::Button(_) => Some(if *title { Page::Title } else { Page::Menu { cursor: 0 } }),
                _ => None,
            },
            Page::Theater { cursor } => {
                let rows = theater(&d, progress.seen);
                let [r, c] = cursor;
                let len = |r: usize| rows.get(r).map_or(1, Vec::len).max(1);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sfx::cursor());
                        *r = (*r + if key == Key::Up { rows.len() - 1 } else { 1 }) % rows.len();
                        *c = (*c).min(len(*r) - 1);
                        None
                    }
                    Key::Left | Key::Right => {
                        sfx::play(sfx::cursor());
                        *c = (*c + if key == Key::Left { len(*r) - 1 } else { 1 }) % len(*r);
                        None
                    }
                    Key::Start | Key::Button(_) => match rows.get(*r).and_then(|row| row.get(*c)).map(|cell| cell.1) {
                        Some(Cell::Ending(e, costume)) => {
                            sfx::play(sfx::confirm());
                            return Outcome::TttEnding(e, costume, [*r, *c]);
                        }
                        Some(Cell::Music) => {
                            sfx::play(sfx::confirm());
                            Some(Page::Music { cursor: 0, playing: None })
                        }
                        Some(Cell::Exit) => Some(Page::Menu { cursor: Self::items(&d, progress).iter().position(|i| i.0 == THEATER).unwrap_or(0) }),
                        _ => None,
                    },
                    Key::Back => Some(Page::Menu { cursor: 0 }),
                }
            }
            Page::Music { cursor, playing } => {
                let list = music(&d);
                match key {
                    Key::Up | Key::Down => {
                        sfx::play(sfx::cursor());
                        *cursor = (*cursor + if key == Key::Up { list.len() } else { 1 }) % (list.len() + 1);
                        None
                    }
                    Key::Start | Key::Button(_) if *cursor < list.len() => {
                        *playing = list.get(*cursor).map(|m| m.0);
                        None
                    }
                    Key::Start | Key::Button(_) | Key::Back => {
                        sfx::play(sfx::back());
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
            Page::Select(s) => {
                if let Some(run) = s.tick(&d, roster, progress, seed) {
                    return Outcome::TttFight(Box::new(run));
                }
            }
            Page::Continue { countdown, .. } => {
                *countdown = countdown.saturating_sub(1);
                if *countdown == 0 {
                    self.page = Page::Message { lines: vec!["GAME OVER".into()], frame: 0, title: true };
                }
            }
            Page::Message { frame, title, .. } => {
                *frame += 1;
                if *frame >= MESSAGE_FRAMES {
                    self.page = if *title { Page::Title } else { Page::Menu { cursor: 0 } };
                }
            }
            _ => {}
        }
        Outcome::None
    }

    pub fn draw(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster) {
        let progress = &progress();
        let Some(d) = roster.ttt.as_deref() else { return };
        let blink = (self.frame / 30).is_multiple_of(2);
        match &self.page {
            Page::Title => {
                image(painter, v, art, &d.picture(&["title", "picture"]), [0.0, 0.0, W, H], Color32::WHITE);
                if blink {
                    font(painter, v, art, d, &d.picture(&["title", "press_start"]), W / 2.0, 330.0, 0.5, Color32::WHITE);
                }
                for (k, line) in d.raw["title"]["copyright"].as_array().into_iter().flatten().enumerate() {
                    small(painter, v, line.as_str().unwrap_or_default(), W / 2.0, 396.0 + 16.0 * k as f32, Color32::from_gray(200));
                }
            }
            Page::Menu { cursor } => {
                backdrop(painter, v, art);
                let items = Self::items(d, progress);
                for (k, (_, label)) in items.iter().enumerate() {
                    let y = 80.0 + 26.0 * k as f32;
                    if k == *cursor {
                        Art::shade(painter, v, sv([170.0, y - 3.0, 300.0, 25.0]), [Color32::from_rgba_unmultiplied(40, 80, 200, 150); 4]);
                    }
                    let c = if k == *cursor { Color32::WHITE } else { Color32::from_gray(150) };
                    font(painter, v, art, d, label, W / 2.0, y, 0.45, c);
                }
            }
            Page::Options { cursor, values } => {
                backdrop(painter, v, art);
                font(painter, v, art, d, "OPTIONS", W / 2.0, 40.0, 0.6, Color32::WHITE);
                for (k, &row) in OPTION_ROWS.iter().enumerate() {
                    let (_, names) = d.option(row);
                    let label = d.raw["options"][row]["label"].as_str().unwrap_or_default();
                    let c = if k == *cursor { Color32::WHITE } else { Color32::from_gray(150) };
                    let y = 130.0 + 40.0 * k as f32;
                    font(painter, v, art, d, label, 200.0, y, 0.35, c);
                    font(painter, v, art, d, names.get(values[k]).map_or("", |s| s.as_str()), 500.0, y, 0.35, c);
                }
                let c = if *cursor == OPTION_ROWS.len() { Color32::WHITE } else { Color32::from_gray(150) };
                font(painter, v, art, d, "EXIT", W / 2.0, 130.0 + 40.0 * OPTION_ROWS.len() as f32, 0.35, c);
            }
            Page::Select(s) => draw_select(painter, v, art, d, roster, progress, s),
            Page::Continue { countdown, .. } => {
                font(painter, v, art, d, &format!("CONTINUE? {}", ((countdown + 2) / 100).min(9)), W / 2.0, 180.0, 0.8, Color32::WHITE);
            }
            Page::Message { lines, .. } => {
                backdrop(painter, v, art);
                for (k, l) in lines.iter().enumerate() {
                    font(painter, v, art, d, l, W / 2.0, 150.0 + 50.0 * k as f32, if k == 0 { 0.7 } else { 0.45 }, Color32::WHITE);
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
                        small(painter, v, label, x, y, colour);
                    }
                }
                if let Some((_, Cell::None)) = rows.get(*r).and_then(|row| row.get(*c)) {
                    small(painter, v, "NOT AVAILABLE: THE MOVIES AND THE EMBU ARE NOT CONVERTED", W / 2.0, 430.0, Color32::from_gray(200));
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
                    small(painter, v, &format!("{mark}{name}"), W / 2.0, y, colour);
                }
            }
        }
    }

    /// The BGM the page plays (the music player's).
    pub fn music(&self) -> Option<usize> {
        match self.page {
            Page::Music { playing, .. } => playing,
            _ => None,
        }
    }
}

/// What a Theater cell does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    /// Ending (number, the player's costume).
    Ending(usize, usize),
    Music,
    Exit,
    /// Not playable here: a movie (the converter keeps the MPEG-2 streams as they are), Unknown's (a movie), the
    /// embu.
    None,
}

/// Theater's rows (0x3544C8, 0x3545A8): each character with an ending seen, its cells the costumes labelled whose
/// ending was seen; then OPENING MOVIE, ARCADE MOVIE, EMBU (and EMBU2, not shown: the last unlock's), MUSIC PLAYER,
/// EXIT THEATER MENU.
fn theater(d: &Data, seen: u64) -> Vec<Vec<(String, Cell)>> {
    let Some(e) = d.endings.as_deref() else { return vec![vec![("EXIT THEATER MENU".into(), Cell::Exit)]] };
    let t = &e.raw["theater"];
    let unknown = e.raw["playback"]["unknown"].as_u64().unwrap_or(37) as usize;
    let mut rows: Vec<Vec<(String, Cell)>> = (0..34)
        .map(|c| {
            (0..8)
                .filter_map(|k| {
                    let label = t["labels"][c][k].as_str()?;
                    let n = e.ending(c, k).filter(|&n| n < 64 && seen >> n & 1 == 1)?;
                    Some((label.to_string(), if n == unknown { Cell::None } else { Cell::Ending(n, k) }))
                })
                .collect()
        })
        .filter(|r: &Vec<_>| !r.is_empty())
        .collect();
    rows.extend(t["extras"].as_array().into_iter().flatten().filter(|x| x["cast"].is_null()).map(|x| vec![(x["label"].as_str().unwrap_or_default().to_string(), Cell::None)]));
    rows.push(vec![("MUSIC PLAYER".into(), Cell::Music)]);
    rows.push(vec![("EXIT THEATER MENU".into(), Cell::Exit)]);
    rows
}

/// The staff roll's lines (0x51B880, after its two cards; the logos 0xAB / 0xAC are not drawn): each text and whether
/// a heading; of an alternative pair the first (the one until Tekken Bowl is unlocked, which is not here yet).
fn roll_lines(e: &EndingData) -> Vec<(String, bool)> {
    let r = &e.raw["staff_roll"];
    let cards = r["cards"].as_array().into_iter().flatten().flat_map(|c| [(c.as_str().unwrap_or_default().to_string(), true), (String::new(), false)]);
    let mut alt = false;
    let lines = r["lines"].as_array().into_iter().flatten().filter(|l| {
        let (a, was) = (l["alternative"] == true, alt);
        alt = a;
        !(a && was)
    });
    cards.chain(lines.map(|l| (l["text"].as_str().unwrap_or_default().to_string(), l["font"] == 7))).collect()
}

/// The staff roll's start, line spacing and speed (px, px a frame).
fn roll_metrics(e: &EndingData) -> (f32, f32, f32) {
    let r = &e.raw["staff_roll"];
    (r["start_y"].as_f64().unwrap_or(480.0) as f32, r["line_px"].as_f64().unwrap_or(32.0) as f32, r["scroll_px"].as_f64().unwrap_or(1.5).max(0.1) as f32)
}

/// The staff roll's length in frames: until its last line has gone off the top.
pub fn roll_frames(e: &EndingData) -> u32 {
    let (y, line, speed) = roll_metrics(e);
    ((y + line * (roll_lines(e).len() + 1) as f32) / speed) as u32
}

/// An ending's overlay: the scene's fades (black, then white), and the staff roll at its frame `roll` (0x3AC018: the
/// lines rise from y 480 at 1.5 px a frame, 32 px apart; the fonts are egui's).
pub fn draw_ending(painter: &egui::Painter, v: &View, d: &Data, roll: Option<u32>, black: u8, white: u8) {
    // (the whole window: the scene fills it)
    painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_black_alpha(black));
    painter.rect_filled(painter.clip_rect(), 0.0, Color32::from_white_alpha(white));
    let (Some(f), Some(e)) = (roll, d.endings.as_deref()) else { return };
    let (y0, line, speed) = roll_metrics(e);
    for (k, (text, heading)) in roll_lines(e).iter().enumerate() {
        let y = y0 + line * k as f32 - speed * f as f32;
        if (-line..H).contains(&y) {
            let [x, y, _, h] = sv([W / 2.0, y, 0.0, if *heading { 17.0 } else { 14.0 }]);
            let c = if *heading { Color32::from_rgb(255, 214, 120) } else { Color32::WHITE };
            painter.text(v.pos(x, y), egui::Align2::CENTER_TOP, text, egui::FontId::proportional(h * v.rect.height() / SCREEN_H), c);
        }
    }
}

/// The music player's list (0x455D80): BGM and name, EXIT left out.
fn music(d: &Data) -> Vec<(usize, String)> {
    let list = d.endings.as_ref().and_then(|e| e.raw["theater"]["music"].as_array().cloned()).unwrap_or_default();
    list.iter().filter_map(|m| Some((m["bgm"].as_u64().filter(|&b| b != 254)? as usize, m["name"].as_str().unwrap_or_default().to_string()))).collect()
}

/// Tekken Tag's 640 x 448 rectangle in the 368 x 480 one the screens draw on.
fn sv([x, y, w, h]: [f32; 4]) -> [f32; 4] {
    [x * SCREEN_W / W, y * SCREEN_H / H, w * SCREEN_W / W, h * SCREEN_H / H]
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

/// A text in the chrome font (`fonts.big`: its glyphs' size and rows), centred on `x`, at `scale`.
#[allow(clippy::too_many_arguments)]
fn font(painter: &egui::Painter, v: &View, art: &Art, d: &Data, text: &str, x: f32, y: f32, scale: f32, tint: Color32) {
    let f = &d.raw["fonts"]["big"];
    let rows: Vec<&str> = f["rows"].as_array().into_iter().flatten().filter_map(|r| r.as_str()).collect();
    let Some((tex, [tw, th])) = art.pic(f["picture"].as_str().unwrap_or_default()) else { return };
    let (gw, gh) = (num(&f["glyph"][0]).max(1.0), num(&f["glyph"][1]).max(1.0));
    // shortcut: the glyphs' widths are not read: the narrow ones and the space take less
    let advance = |c: char| gw * scale * if "I1!.:'- ".contains(c) { 0.42 } else { 0.78 };
    let text = text.to_uppercase().replace(['´', '`'], "'");
    let mut at = x - text.chars().map(advance).sum::<f32>() / 2.0;
    for c in text.chars() {
        if let Some((r, k)) = rows.iter().enumerate().find_map(|(r, row)| row.chars().position(|g| g == c).map(|k| (r, k))) {
            let (u, vv) = (k as f32 * gw / tw as f32, r as f32 * gh / th as f32);
            let uv = Rect::from_min_max(Pos2::new(u, vv), Pos2::new(u + gw / tw as f32, vv + gh / th as f32));
            let [rx, ry, rw, rh] = sv([at - (gw * scale - advance(c)) / 2.0, y, gw * scale, gh * scale]);
            painter.image(tex.id(), v.rect(rx, ry, rw, rh), uv, tint);
        }
        at += advance(c);
    }
}

/// A small text (egui's font: the game's small fonts' glyph order is not read), centred on `x`.
fn small(painter: &egui::Painter, v: &View, text: &str, x: f32, y: f32, c: Color32) {
    let [px, py, _, h] = sv([x, y, 0.0, 12.0]);
    painter.text(v.pos(px, py), egui::Align2::CENTER_TOP, text, egui::FontId::proportional(h * v.rect.height() / SCREEN_H), c);
}

/// The select screen: the grid (0x3CC6C8: each slot's face at (x, y + 52), the cursors' frames), each side's picks
/// as their portraits (file 143-181) and name plates below.
fn draw_select(painter: &egui::Painter, v: &View, art: &Art, d: &Data, roster: &Roster, p: &Progress, s: &Select) {
    image(painter, v, art, "141_000.png", [0.0, 0.0, W, H], Color32::from_gray(90));
    let dy = num(&d.raw["select"]["face_dy"]);
    let title = d.raw["select"]["titles"][s.mode as usize].as_str().unwrap_or_default();
    font(painter, v, art, d, title, W / 2.0, 14.0, 0.5, Color32::WHITE);
    for (k, slot) in d.slots.iter().enumerate() {
        if hidden(d, roster, p, k) {
            continue;
        }
        let face = match slot.character {
            None => d.picture(&["select", "random_face"]),
            Some(c) if d.selectable(p.stage, c) && roster.by_id(c).is_some() => d.picture(&["select", "faces", &c.to_string()]),
            Some(_) => d.picture(&["select", "locked_face"]),
        };
        sprite(painter, v, art, &face, slot.x, slot.y + dy, Color32::WHITE);
    }
    let colours = [Color32::from_rgb(255, 80, 60), Color32::from_rgb(80, 140, 255)];
    for (k, side) in s.sides.iter().enumerate() {
        let Some(player) = side.player else {
            if k == 1 && s.mode == TEAM && (s.frame / 30).is_multiple_of(2) {
                small(painter, v, "PRESS P2 START", 520.0, 300.0, Color32::WHITE);
            }
            continue;
        };
        if let (false, Some(slot)) = (side.done, d.slots.get(side.cursor)) {
            let pulse = Color32::from_gray(160 + (crate::menu::triangle(s.frame * 8) / 3) as u8);
            image(painter, v, art, &d.picture(&["select", "cursor"]), [slot.x - 2.0, slot.y + dy - 2.0, 56.0, 57.0], additive(pulse));
            let [rx, ry, rw, rh] = sv([slot.x - 1.0, slot.y + dy - 1.0, 54.0, 54.0]);
            painter.rect_stroke(v.rect(rx, ry, rw, rh), 0.0, egui::Stroke::new(v.rect.height() / 240.0, colours[k]), egui::StrokeKind::Inside);
            let tag = if player == k { d.picture(&["select", "tags", &k.to_string()]) } else { d.picture(&["select", "tags", "6"]) };
            sprite(painter, v, art, &tag, slot.x + (if k == 0 { 2.0 } else { 26.0 }), slot.y + dy + 40.0, Color32::WHITE);
        }
        // the picks and the one under the cursor, as portraits, P1's from the left, P2's from the right
        let hover = (!side.done)
            .then(|| d.slots.get(side.cursor).and_then(|sl| sl.character))
            .flatten()
            .filter(|&c| d.selectable(p.stage, c))
            .and_then(|c| roster.index_of(c))
            .filter(|i| !side.picks.iter().any(|q| q.0 == *i));
        let shown: Vec<(usize, bool)> = side.picks.iter().map(|q| (q.0, true)).chain(hover.map(|h| (h, false))).collect();
        let (pw, ph) = (188.0 * 0.6, 256.0 * 0.6);
        for (j, &(i, picked)) in shown.iter().rev().take(4).rev().enumerate() {
            let id = roster.characters.get(i).map_or(0, |c| c.id);
            let file = d.picture(&["select", "portraits", &id.to_string()]);
            let x = if k == 0 { 4.0 + 96.0 * j as f32 } else { W - 4.0 - 96.0 * j as f32 };
            let tint = if picked { Color32::WHITE } else { Color32::from_gray(110) };
            image(painter, v, art, &file, [x, H - ph - 18.0, if k == 0 { pw } else { -pw }, ph], tint);
            let plate = d.picture(&["hud", "plates", &id.to_string(), "picture"]);
            sprite(painter, v, art, &plate, if k == 0 { x } else { x - 128.0 }, H - 18.0, tint);
        }
        if s.mode == TEAM {
            small(painter, v, &format!("{}P  {} / 8", k + 1, side.picks.len()), if k == 0 { 120.0 } else { 520.0 }, 286.0, colours[k]);
        }
    }
    if let Some(t) = s.tally {
        font(painter, v, art, d, &format!("{} - {}", t[0], t[1]), W / 2.0, 300.0, 0.5, Color32::WHITE);
    }
}

/// The VS screen (OpenKumite's: the original's file 136 layout is not decoded): the two teams' portraits face to
/// face, VS between.
pub fn draw_vs(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, setup: &FightSetup, bench: Option<[(usize, usize); 2]>, frame: u32) {
    let Some(d) = roster.ttt.as_deref() else { return };
    backdrop(painter, v, art);
    let slide = (1.0 - frame as f32 / 12.0).max(0.0) * 200.0;
    for k in 0..2 {
        let team: Vec<(usize, usize)> = [Some(setup.players[k]), bench.map(|b| b[k])].into_iter().flatten().collect();
        for (j, m) in team.iter().enumerate().rev() {
            let id = roster.characters.get(m.0).map_or(0, |c| c.id);
            let (pw, ph) = (188.0 * (1.2 - 0.2 * j as f32), 256.0 * (1.2 - 0.2 * j as f32));
            let off = 30.0 + 110.0 * j as f32 + slide;
            let x = if k == 0 { off - 20.0 } else { W - off + 20.0 };
            let tint = if j == 0 { Color32::WHITE } else { Color32::from_gray(150) };
            image(painter, v, art, &d.picture(&["select", "portraits", &id.to_string()]), [x, 360.0 - ph, if k == 0 { pw } else { -pw }, ph], tint);
            let plate = d.picture(&["hud", "plates", &id.to_string(), "picture"]);
            sprite(painter, v, art, &plate, if k == 0 { 40.0 } else { W - 168.0 }, 370.0 + 18.0 * j as f32, Color32::WHITE);
        }
    }
    font(painter, v, art, d, "VS", W / 2.0, 170.0, 1.4, Color32::WHITE);
}

/// The fight HUD (0x3C1F08, 0x3C23F0; FRONTEND.md 4): per side the health bar of the member in front (its six strips:
/// empty, the recoverable cap, the damage trail, the health; angry strips past the anger mark), the benched partner's
/// smaller bar under it (shortcut: its place and size are OpenKumite's), the name plate on its streak, the round
/// lamps, the timer's digits, the arcade's STAGE, Time Attack's clock (`time`, frames) and the round's banner in the
/// chrome font. `trails`: per side the damage trail (16.16).
#[allow(clippy::too_many_arguments)]
pub fn hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, bout: &Bout, info: &crate::hud::HudInfo, trails: [i32; 2], result: Option<Announcement>, frame: u32, time: Option<u32>) {
    let h = &d.raw["hud"];
    let fight = &bout.game.fight;
    if !matches!(bout.phase, Phase::Replay { .. }) && !info.hidden {
        let (bx, by, bw, bh) = ([num(&h["bars"]["x"][0]), num(&h["bars"]["x"][1])], num(&h["bars"]["y"]), num(&h["bars"]["w"]), num(&h["bars"]["h"]));
        let strips: Vec<String> = h["strips"].as_array().into_iter().flatten().filter_map(|s| s.as_str()).map(String::from).collect();
        let angry: Vec<String> = h["angry_strips"].as_array().into_iter().flatten().filter_map(|s| s.as_str()).map(String::from).collect();
        let strip = |k: usize, mad: bool| if mad { angry.get(k) } else { strips.get(k) }.cloned().unwrap_or_default();
        // a bar: [from, to) of its width (side 0 full at its right, side 1 at its left)
        let bar = |i: usize, x: f32, y: f32, w: f32, hh: f32, parts: &[(usize, f32)], mad: bool| {
            image(painter, v, art, &strip(2, mad), [x, y, w, hh], Color32::WHITE);
            for &(k, part) in parts {
                let pw = w * part.clamp(0.0, 1.0);
                let x0 = if i == 0 { x + w - pw } else { x };
                image(painter, v, art, &strip(k, mad), [x0, y, pw, hh], Color32::WHITE);
            }
            image(painter, v, art, &strip(3, mad), [x - 4.0, y, 8.0, hh], Color32::WHITE);
            image(painter, v, art, &strip(3, mad), [x + w + 4.0, y, -8.0, hh], Color32::WHITE);
        };
        let teams = fight.teams.as_deref();
        for (i, f) in fight.fighters.iter().enumerate() {
            let full = (f.max_health * kumite_sim::ONE).max(1) as f32;
            let part = |x: i32| x.max(0) as f32 / full;
            let (cap, mad, partner, body) = match teams {
                Some(t) => {
                    let team = &t[i];
                    let mad = team.members[team.active].anger >= int(&h["angry_at"]) as i32 && (frame / int(&h["angry_blink"]).max(1) as u32).is_multiple_of(2);
                    (team.members[team.active].cap, mad, Some(team.partner()), team.members[team.active].character)
                }
                None => (f.health, false, None, f.character),
            };
            // strips (shortcut: which is which is a guess, FRONTEND.md 4 [M]): 2 empty, 5 the recoverable cap, 4 the
            // trail, 1 the health
            bar(i, bx[i], by, bw, bh, &[(5, part(cap)), (4, part(trails[i].max(f.health))), (1, part(f.health))], mad);
            if let Some(m) = partner {
                let full = (m.max_health * kumite_sim::ONE).max(1) as f32;
                let (pw, px) = (bw * 0.5, if i == 0 { bx[i] + bw * 0.5 } else { bx[i] });
                bar(i, px, by + bh + 22.0, pw, 8.0, &[(5, m.cap.max(0) as f32 / full), (1, m.health.max(0) as f32 / full)], false);
            }
            // the name plate (0x3C1DA0: the member in front) on its streak
            let b = body.unwrap_or(0).max(0) as usize;
            let plate = &h["plates"][b];
            let py = num(&h["plate_y"]);
            let px = if i == 0 { bx[0] + num(&plate["x"]) } else { bx[1] + bw - 128.0 - num(&plate["x"]) };
            let streak = h["streaks"][int(&plate["streak"])].as_str().unwrap_or_default();
            image(painter, v, art, streak, [if i == 0 { px - 8.0 } else { px + 136.0 }, py - 9.0, if i == 0 { 124.0 } else { -124.0 }, 32.0], Color32::WHITE);
            sprite(painter, v, art, plate["picture"].as_str().unwrap_or_default(), px, py, Color32::WHITE);
            // the round lamps: one a round to win when 2 or more (a new win blinks 180 frames, 11-frame period)
            let l = &h["lamps"];
            if bout.rounds_to_win >= 2 {
                for k in 0..bout.rounds_to_win {
                    let x = if i == 0 { num(&l["x"][0]) - num(&l["dx"]) * k as f32 } else { num(&l["x"][1]) + num(&l["dx"]) * k as f32 };
                    let lit = k < bout.wins[i]
                        && !(k + 1 == bout.wins[i] && bout.phase_frame < 180 && bout.phase_frame / 11 % 2 == 1 && matches!(bout.phase, Phase::RoundOver { .. } | Phase::Result { .. }));
                    sprite(painter, v, art, l[if lit { "lit" } else { "unlit" }].as_str().unwrap_or_default(), x, num(&l["y"]), Color32::WHITE);
                }
            }
        }
        // the timer: seconds (t * 6 / 7 - 1) / 60 in the digits' picture, INFINITY untimed
        let t = &h["timer"];
        let y = num(&t["y"]);
        if bout.game.round_frames > 100 * 70 {
            sprite(painter, v, art, t["infinity"].as_str().unwrap_or_default(), W / 2.0 - 36.0, y, Color32::WHITE);
        } else {
            let secs = (fight.clock * 6 / 7).saturating_sub(1) / 60;
            let dw = num(&t["digit_w"]);
            if let Some((tex, [tw, _])) = art.pic(t["digits"].as_str().unwrap_or_default()) {
                for (k, digit) in [secs.min(99) / 10, secs.min(99) % 10].into_iter().enumerate() {
                    let u = digit as f32 * dw / tw as f32;
                    let [rx, ry, rw, rh] = sv([W / 2.0 - dw + dw * k as f32, y, dw, 47.0]);
                    painter.image(tex.id(), v.rect(rx, ry, rw, rh), Rect::from_min_max(Pos2::new(u, 0.0), Pos2::new(u + dw / tw as f32, 1.0)), Color32::WHITE);
                }
            }
        }
        // the arcade's STAGE n / FINAL (alternating every 32 frames on the last stage)
        if let Some(stage) = info.stage {
            let last = stage >= int(&d.raw["arcade"]["fights"]) as u32;
            let s = &h["stage"];
            let text = if last && frame / int(&s["alternate"]).max(1) as u32 % 2 == 1 { s["final"].as_str().unwrap_or_default().to_string() } else { format!("STAGE {stage}") };
            font(painter, v, art, d, &text, W / 2.0, 70.0, 0.3, Color32::WHITE);
        }
        if let Some(t) = time {
            small(painter, v, &clock(t), W / 2.0, 86.0, Color32::WHITE);
        }
    }
    if let Some(call) = crate::hud::call(bout, info, result) {
        font(painter, v, art, d, &call, W / 2.0, 190.0, 0.9, Color32::WHITE);
    }
}

/// Tekken Bowl's HUD (OpenKumite's: the original's, 0x3E4BC0, and its gauges' sprites are not decoded): each player's
/// score sheet (ten frames, the balls' marks, the running totals; the one bowling lit), the prompt and the meters of
/// the throw's step (the spin meter between the hand's bounds, the power meter), SPEED in MPH (0x3EAC88) once the
/// ball goes, and the callout in the chrome font.
pub fn bowl_hud(painter: &egui::Painter, v: &View, art: &Art, d: &Data, roster: &Roster, scene: &kumite_game::bowl::BowlScene, frame: u64) {
    use kumite_sim::bowl::{Mark, Phase};
    let b = &scene.bowl;
    let box_ = |c: Color32, r: [f32; 4]| Art::shade(painter, v, sv(r), [c; 4]);
    let (frame_w, x0) = (48.0, 92.0);
    for (k, card) in b.cards.iter().enumerate() {
        let y = 14.0 + 46.0 * k as f32;
        let lit = k == b.current && b.phase != Phase::Over;
        box_(Color32::from_rgba_unmultiplied(0, 0, 0, 170), [12.0, y, 616.0, 42.0]);
        let name = b.players.get(k).and_then(|p| roster.by_id(p.character)).map_or(String::new(), |c| c.name.to_uppercase());
        small(painter, v, &format!("{}P {name}", k + 1), 52.0, y + 15.0, if lit { Color32::YELLOW } else { Color32::from_gray(200) });
        let totals = card.totals();
        for (f, total) in totals.iter().enumerate() {
            let x = x0 + frame_w * f as f32;
            let here = lit && card.at().frame == f;
            box_(if here { Color32::from_rgba_unmultiplied(40, 80, 200, 200) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 40) }, [x + 1.0, y + 2.0, frame_w - 2.0, 38.0]);
            // the balls' marks: X a strike, / a spare, - nothing, F a foul (a split's count in red)
            let balls = card.frame_balls(f);
            let mut prev = 0u8;
            for (n, i) in balls.enumerate() {
                let (pins, mark) = (card.rolls.get(i).copied().unwrap_or(0), card.marks.get(i).copied().unwrap_or_default());
                let text = match mark {
                    Mark::Strike => "X".to_string(),
                    Mark::Spare => "/".into(),
                    Mark::Foul => "F".into(),
                    _ if pins == 0 => "-".into(),
                    _ if n > 0 && f == 9 && prev < 10 && prev + pins == 10 => "/".into(),
                    _ => pins.to_string(),
                };
                prev = pins;
                let c = if mark == Mark::Split { Color32::from_rgb(255, 90, 90) } else { Color32::WHITE };
                small(painter, v, &text, x + 9.0 + 15.0 * n as f32, y + 3.0, c);
            }
            if let Some(t) = total {
                small(painter, v, &t.to_string(), x + frame_w / 2.0, y + 22.0, Color32::WHITE);
            }
        }
        small(painter, v, &card.total().to_string(), x0 + frame_w * 10.0 + 24.0, y + 15.0, Color32::WHITE);
    }
    // the throw's step: its prompt and meters
    let meter = |y: f32, label: &str, part: f32, mark: Option<f32>| {
        small(painter, v, label, 70.0, y - 1.0, Color32::WHITE);
        box_(Color32::from_rgba_unmultiplied(0, 0, 0, 170), [110.0, y, 200.0, 12.0]);
        match mark {
            Some(m) => {
                box_(Color32::from_gray(90), [209.0, y, 2.0, 12.0]);
                box_(Color32::from_rgb(255, 210, 60), [110.0 + 196.0 * m.clamp(0.0, 1.0), y, 4.0, 12.0]);
            }
            None => box_(Color32::from_rgb(255, 80 + (175.0 * (1.0 - part)) as u8, 40), [110.0, y, 200.0 * part.clamp(0.0, 1.0), 12.0]),
        }
    };
    let bowler = b.players.get(b.current);
    let prompt = match b.phase {
        Phase::Position => Some("LEFT / RIGHT: POSITION   CROSS: OK"),
        Phase::Aim => Some("LEFT / RIGHT: AIM   CROSS: SPIN   CIRCLE: BACK"),
        Phase::Power => Some("CROSS: POWER"),
        _ => None,
    };
    if let Some(p) = prompt {
        small(painter, v, p, W / 2.0, 410.0, Color32::from_gray(230));
        // the spin meter between -2 and 2 (the hand's bounds), the power meter 0-100
        meter(372.0, "SPIN", 0.0, Some((b.spin + 2.0) / 4.0));
        if b.phase == Phase::Power {
            meter(390.0, "POWER", b.power / 100.0, None);
        }
        let hand = if bowler.is_some_and(|p| p.left) { "LEFT-HANDED" } else { "RIGHT-HANDED" };
        small(painter, v, hand, 560.0, 372.0, Color32::from_gray(200));
    }
    if b.rolled.is_some() && b.speed > 0.0 && !matches!(b.phase, Phase::Charge(_)) {
        let mph = b.mph();
        small(painter, v, &format!("SPEED {:3}.{:02} MPH", mph / 100, mph % 100), 540.0, 410.0, Color32::WHITE);
    }
    if let Some(c) = b.callout.as_deref().filter(|c| !c.is_empty()) {
        // (a strike's callout pulses)
        let k = if matches!(b.phase, Phase::Strike(_)) { 0.85 + 0.1 * ((frame / 8) % 2) as f32 } else { 0.8 };
        font(painter, v, art, d, c, W / 2.0, 190.0, k, Color32::WHITE);
    }
    if b.phase == Phase::Over {
        let text = match b.winner() {
            Some(w) if b.cards.len() > 1 => format!("{}P WINS", w + 1),
            _ if b.cards.len() > 1 => "DRAW".into(),
            _ => format!("SCORE {}", b.cards.first().map_or(0, |c| c.total())),
        };
        font(painter, v, art, d, &text, W / 2.0, 190.0, 0.8, Color32::WHITE);
        if (frame / 30).is_multiple_of(2) {
            small(painter, v, "PRESS START", W / 2.0, 410.0, Color32::WHITE);
        }
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
                {"x": 106, "y": 0, "character": 2, "right": 3}, {"x": 159, "y": 0, "character": null}], "hidden_slots": []},
            "unlocks": {"schedule": [{"stage": 0, "kind": 0, "character": 2}, {"stage": 1, "kind": 1, "character": 2}, {"stage": 1, "kind": 0, "character": 3}], "points_per_stage": 50},
            "arcade": {"fights": 8, "rivals": [[2, 3, null], [0, null, null]], "fallback": [3], "pairs": [[1, 2]], "rival_arena": 6, "boss": 3, "boss_arena": 14, "early_fights": 3, "early_excluded": []},
            "team_battle": {"refill_percent": 15},
            "options": [{"default": 1, "values": ["EASY", "NORMAL"]}, {"default": 1, "values": ["1", "2", "3"]}, {"default": 2, "values": ["30", "40", "60"]}],
        }});
        // (each character's ending: its id)
        let endings = EndingData::new(serde_json::json!({"table": [[0], [1], [2], [3]], "endings": [{}, {}, {}, {}]}), &Value::Null).unwrap();
        Data::load(&fe, (0..23).collect(), &serde_json::json!({"round_time": {"options_frames": [2160, 2820, 4260]}}), Some(endings)).unwrap()
    }

    fn roster() -> Roster {
        let mut r = Roster::load(std::path::Path::new("/nonexistent"));
        r.characters = (0..4).map(|id| crate::menu::Character { id, name: format!("C{id}"), costumes: vec![10 + id, 20 + id], extra: false, prefix: String::new(), face: None }).collect();
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
        assert!(matches!(run.clone().after(&d, &r, &mut p, Some(1), [0; 2], [1; 2], 0, &mut seed), Next::Screen(Page::Continue { .. })));
        assert!(matches!(run.clone().after(&d, &r, &mut p, Some(0), [0; 2], [1; 2], 0, &mut seed), Next::Fight(f) if f.fight == 1));
        assert!(matches!(last.after(&d, &r, &mut p, Some(0), [0; 2], [1; 2], 0, &mut seed), Next::Ending(0, 0, Page::Message { title: true, .. })));
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
        let Next::Fight(next) = run.after(&d, &r, &mut Progress::default(), Some(0), [max / 2, 0], [max; 2], 0, &mut seed) else { panic!() };
        assert_eq!((next.health[0], next.member), (Some(max / 2 + max / 100 * 15), [0, 1]));
        assert!(matches!(next.after(&d, &r, &mut Progress::default(), Some(0), [max, 0], [max; 2], 0, &mut seed), Next::Screen(Page::Message { .. })));
    }

    #[test]
    fn theater_lists_the_endings_seen() {
        // Paul's costumes play ending 1, Law's ending 2 (here Unknown's, a movie)
        let endings = serde_json::json!({"table": [[1, 1], [2]], "endings": [{}, {}, {}], "playback": {"unknown": 2},
            "theater": {"labels": [["(1P)PAUL", "(2P)PAUL"], ["(1P)LAW"]], "extras": [{"label": "OPENING MOVIE"}, {"label": "EMBU2", "cast": 1}]}});
        let e = EndingData::new(endings, &Value::Null).unwrap();
        let d = Data::load(&serde_json::json!({"front_end": {}}), vec![], &serde_json::json!({}), Some(e)).unwrap();
        let rows = |seen| theater(&d, seen).into_iter().map(|r| r.into_iter().map(|c| (c.0, c.1)).collect::<Vec<_>>()).collect::<Vec<_>>();
        let tail = [vec![("OPENING MOVIE".to_string(), Cell::None)], vec![("MUSIC PLAYER".into(), Cell::Music)], vec![("EXIT THEATER MENU".into(), Cell::Exit)]];
        assert_eq!(rows(0), tail);
        let paul = vec![("(1P)PAUL".to_string(), Cell::Ending(1, 0)), ("(2P)PAUL".into(), Cell::Ending(1, 1))];
        assert_eq!(rows(1 << 1)[..2], [paul.clone(), tail[0].clone()]);
        assert_eq!(rows(1 << 1 | 1 << 2)[..2], [paul, vec![("(1P)LAW".into(), Cell::None)]]);
        // the menu shows THEATER once an ending was seen
        assert!(!Front::items(&d, &Progress::default()).iter().any(|i| i.0 == THEATER));
    }

    #[test]
    fn tekken_bowl_is_on_the_menu_once_unlocked() {
        let fe = serde_json::json!({"front_end": {"menu": {"items": [{"mode": 0, "label": "ARCADE", "locked": false},
            {"mode": 8, "label": "TEKKEN BOWL", "locked": true}, {"mode": 9, "label": "THEATER", "locked": true}]},
            "unlocks": {"schedule": [{"stage": 10, "kind": 5, "character": 0}]}}});
        let d = Data::load(&fe, vec![0], &serde_json::json!({}), None).unwrap();
        let modes = |stage| Front::items(&d, &Progress { stage, ..Progress::default() }).iter().map(|i| i.0).filter(|&m| m != u32::MAX).collect::<Vec<_>>();
        assert_eq!((modes(9), modes(10)), (vec![ARCADE], vec![ARCADE, BOWL]));
        // its select: P1 picks one; 2P's START joins
        let mut s = Select::new(BOWL, &d, None);
        assert_eq!((s.sides[0].need, s.sides[1].player), (1, None));
        let (mut seed, roster) = (1, roster());
        s.key(1, Key::Start, &d, &roster, &Progress::default(), &mut seed);
        assert_eq!(s.sides[1].player, Some(1));
    }

    #[test]
    fn select_picks_a_tag_pair_and_skips_locked() {
        let (d, r) = (data(), roster());
        let (mut p, mut seed) = (Progress::default(), 3);
        let mut s = Select::new(ARCADE, &d, None);
        assert_eq!(s.sides[0].cursor, 0);
        // the locked character's slot takes no pick; Paul's (0) and Law's (1) do
        s.sides[0].cursor = 2;
        assert!(s.key(0, Key::Button(0), &d, &r, &p, &mut seed).is_none() && s.sides[0].picks.is_empty());
        s.sides[0].cursor = 0;
        s.key(0, Key::Button(1), &d, &r, &p, &mut seed);
        s.key(0, Key::Button(0), &d, &r, &p, &mut seed);
        assert_eq!(s.sides[0].picks, vec![(0, 1)]);
        s.key(0, Key::Right, &d, &r, &p, &mut seed);
        s.key(0, Key::Button(0), &d, &r, &p, &mut seed);
        assert!(s.sides[0].done && s.sides[0].picks == vec![(0, 1), (1, 0)]);
        let run = (0..SELECT_DONE).find_map(|_| s.tick(&d, &r, &p, &mut seed)).unwrap();
        assert_eq!((run.mode, run.ladder.len(), run.teams[0].len()), (ARCADE, 8, 2));
        p.stage = 1;
        assert!(d.selectable(p.stage, 2));
    }
}
