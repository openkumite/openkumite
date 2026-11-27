//! Team Battle, Time Attack and Survival (battle types 2, 3 and 4), as the game runs them (modes research; per-type
//! code in record 5, result screens in record 302): the run each mode keeps between its fights, the fight rules
//! per type (rounds, clock, full health, health carried into the next fight), who comes next, and the result
//! screens. The tables come from the converted screens.json `modes`.

use crate::art::{int as u, num as n, Art, Screen as View, Tex};
use crate::menu::{FightSetup, Mode, Roster};
use egui::Color32;
use serde_json::Value;

/// Frames a result screen stays at most (78 s), the frames a row / tally step takes.
const RESULT_FRAMES: u32 = 4680;

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// A text of the result screens: string, font, colour, x, y.
#[derive(Clone, Debug, Default)]
pub struct Text {
    pub text: String,
    pub font: usize,
    pub colour: u32,
    pub x: f32,
    pub y: f32,
}

impl Text {
    pub fn from(v: &Value) -> Self {
        let colour = v["colour"].as_u64().or_else(|| v["colours"][0].as_u64()).unwrap_or(0) as u32;
        Self { text: s(&v["text"]), font: u(&v["font"]) as usize, colour, x: n(&v["x"]), y: n(&v["y"]) }
    }
    pub fn draw(&self, painter: &egui::Painter, v: &View, art: &Art) {
        art.text(painter, v, self.font, self.colour, self.x, self.y, &self.text, Color32::WHITE);
    }
    fn draw_in(&self, painter: &egui::Painter, v: &View, art: &Art, colour: u32) {
        art.text(painter, v, self.font, colour, self.x, self.y, &self.text, Color32::WHITE);
    }
    /// The text with its conversions given `args` (crate::art::printf), moved by (dx, dy).
    fn draw_args(&self, painter: &egui::Painter, v: &View, art: &Art, [dx, dy]: [f32; 2], args: &[u32]) {
        art.text(painter, v, self.font, self.colour, self.x + dx, self.y + dy, &crate::art::printf(&self.text, args), Color32::WHITE);
    }
}

/// The pieces of a printed text (screens.json, the converter's `printf_pieces`).
fn pieces(v: &Value) -> Vec<Text> {
    v.as_array().into_iter().flatten().map(Text::from).collect()
}

/// screens.json `modes` (and the intro texts of `hud`).
#[derive(Clone, Debug, Default)]
pub struct ModeData {
    /// Health gained by the winner, in 256ths of the maximum, by index (Team: health left; Survival: time taken).
    pub carry_team: Vec<u32>,
    pub carry_survival: Vec<u32>,
    /// Under-bar labels: team, time attack, survival; their colour.
    pub labels: [String; 3],
    /// Ball's under-bar label.
    pub ball_label: String,
    pub label_colour: u32,
    pub game_over: (String, u32, f32),
    pub ranks: Vec<String>,
    pub rank_colour: u32,
    pub wins: Vec<String>,
    pub wins_colour: u32,
    pub wins_dx: f32,
    /// Survival opponents by wins this run: (below this many wins, character ids).
    pub pools: Vec<(Option<u32>, Vec<usize>)>,
    pub team_size: [usize; 3],
    pub team_intro: Text,
    pub survival_intro: Text,
    results: Value,
    /// The result screens' sounds (screens.json `modes.results`): a row or tick, the final line; Force's tick and its
    /// line for a saved character.
    pub result_sfx: (u16, u16),
    pub force_result_sfx: (u16, u16),
    /// The records' defaults: best time per character id (frames, name); the survival top ten (id, wins, name).
    pub ta_defaults: Vec<(u32, String)>,
    pub survival_defaults: Vec<(usize, u32, String)>,
    /// Time Attack's, Survival's and Team Battle's rules (screens.json `modes.rules`): rounds to win, the round clock
    /// (None: the option), full health; the ROUND option's rounds to win, the others'.
    pub rules: [(u32, Option<u32>, i32); 3],
    pub rounds_to_win: u32,
}

impl ModeData {
    pub fn load(screens: &Value) -> Self {
        let m = &screens["modes"];
        let arr = |v: &Value| v.as_array().cloned().unwrap_or_default();
        let ub = &m["under_bar"];
        Self {
            carry_team: arr(&m["carry_over"]["team"]["gain_256ths"]).iter().map(u).collect(),
            carry_survival: arr(&m["carry_over"]["survival"]["gain_256ths"]).iter().map(u).collect(),
            labels: [s(&ub["labels"]["team"]), s(&ub["labels"]["time_attack"]), s(&ub["labels"]["survival"])],
            ball_label: s(&ub["labels"]["ball"]),
            label_colour: ub["labels"]["colour"].as_u64().unwrap_or(0) as u32,
            game_over: (s(&ub["game_over"]["text"]), u(&ub["game_over"]["colour"]), n(&ub["game_over"]["dx"])),
            ranks: arr(&ub["survival"]["ranks"]).iter().map(s).collect(),
            rank_colour: u(&ub["survival"]["rank_colour"]),
            wins: arr(&ub["survival"]["wins"]).iter().map(s).collect(),
            wins_colour: u(&ub["survival"]["wins_colour"]),
            wins_dx: n(&ub["survival"]["wins_dx"]),
            pools: arr(&m["survival"]["pools"]).iter().map(|p| (p["below_wins"].as_u64().map(|w| w as u32), arr(&p["characters"]).iter().map(|c| u(c) as usize).collect())).collect(),
            team_size: [u(&m["team"]["size"]["min"]).max(1) as usize, u(&m["team"]["size"]["max"]).max(1) as usize, u(&m["team"]["size"]["default"]).max(1) as usize],
            team_intro: Text::from(&screens["hud"]["intro"]["team"]),
            survival_intro: Text::from(&screens["hud"]["intro"]["survival"]),
            results: m["results"].clone(),
            result_sfx: (u(&m["results"]["tick_sfx"]) as u16, u(&m["results"]["banner_sfx"]) as u16),
            force_result_sfx: (u(&m["results"]["force"]["tick_sfx"]) as u16, u(&m["results"]["force"]["saved_sfx"]) as u16),
            ta_defaults: arr(&m["records"]["time_attack"]).iter().map(|r| (u(&r["frames"]), s(&r["name"]))).collect(),
            survival_defaults: arr(&m["records"]["survival"]).iter().map(|r| (u(&r["character"]) as usize, u(&r["wins"]), s(&r["name"]))).collect(),
            rules: ["time_attack", "survival", "team"].map(|k| {
                let r = &m["rules"][k];
                (u(&r["rounds_to_win"]).max(1), r["round_frames"].as_u64().map(|f| f as u32), u(&r["max_health"]).max(1) as i32)
            }),
            // (a game without the table, Tekken Tag: two, as Tekken 3's)
            rounds_to_win: m["rules"]["default_rounds_to_win"].as_u64().map_or(2, |r| r.max(1) as u32),
        }
    }

    /// Health (16.16) a winner starts the next fight with (0x800517EC): `gain[index]` 256ths of the maximum on top.
    fn carry(gain: &[u32], index: usize, health: i32, max: i32) -> i32 {
        let g = gain.get(index.min(gain.len().saturating_sub(1))).copied().unwrap_or(0) as i64;
        let (h, m) = (health as i64, max as i64);
        (h + (m >> 8) * g).min(m) as i32
    }
}

/// The game's rand (PsyQ): seed * 0x41C64E6D + 12345, bits 16..30.
fn rand(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
    (*seed >> 16) & 0x7FFF
}

/// mm'ss"cc of a frame count, at most 99'59"99.
pub fn time_parts(frames: u32) -> [u32; 3] {
    let f = frames.min(359_999);
    [f / 3600, f / 60 % 60, f % 60 * 100 / 60]
}

/// One cleared Time Attack stage.
#[derive(Clone, Copy, Debug)]
pub struct Split {
    pub frames: u32,
    /// The player's roster index and costume, and the opponent's character id.
    pub player: (usize, usize),
    pub opponent: usize,
}

#[derive(Clone, Debug)]
pub struct TimeAttack {
    pub player: (usize, usize),
    pub ladder: Vec<usize>,
    /// The ladder's stage (ctx+0x24).
    pub stage: usize,
    /// Frames of the cleared stages (ctx+0x34) and of the lost attempts at this one (ctx+0x30).
    pub total: u32,
    pub carried: u32,
    pub splits: Vec<Split>,
}

#[derive(Clone, Debug)]
pub struct Survival {
    pub player: (usize, usize),
    /// Wins this run (ctx+0x24); times each character was beaten (ctx+0x60; by id, the extra fighters too).
    pub wins: u32,
    pub beaten: [u32; 64],
    /// The last four opponents (ctx+0x50).
    pub recent: Vec<usize>,
    /// Rank the run's wins would take in the top ten (1..10, 0 = none).
    pub rank: u32,
    pub opponent: (usize, usize),
    pub level: usize,
    pub difficulty: usize,
}

impl Survival {
    pub fn total(&self) -> u32 {
        self.beaten.iter().sum()
    }
}

/// One side's team (ctx+0x59 + 13p).
#[derive(Clone, Debug, Default)]
pub struct Team {
    /// Members: roster index, costume.
    pub members: Vec<(usize, usize)>,
    /// Defeated members = the index of the one fighting (T[9]).
    pub current: usize,
    /// Members picked by hand (T[0xA]).
    pub picked: usize,
    pub human: bool,
}

#[derive(Clone, Debug)]
pub struct TeamBattle {
    pub teams: [Team; 2],
    /// Match number (ctx+0x58) and each match's result: Some(winner side) or None for a draw.
    pub fight: u32,
    pub log: Vec<Option<usize>>,
    /// The side whose member came in last (its stage and music).
    pub incoming: usize,
    pub difficulty: usize,
    /// The side that just lost a member (its icon blinks).
    pub just_lost: Option<usize>,
}

/// A mode's run: what it keeps between fights.
#[derive(Clone, Debug)]
pub enum Kind {
    TimeAttack(TimeAttack),
    Survival(Box<Survival>),
    Team(TeamBattle),
}

#[derive(Clone, Debug)]
pub struct Run {
    pub kind: Kind,
    /// Fighting frames of the current match (ctx+0x2C): rounds being fought only.
    pub frames: u32,
    /// Health (16.16) each side starts the next fight with.
    pub start_health: [Option<i32>; 2],
}

/// What comes after a match.
pub enum Next {
    /// The next fight.
    Fight,
    /// Time Attack lost: CONTINUE?
    Continue(FightSetup),
    /// The run is over: its result screen.
    Results(Box<Results>),
}

impl Run {
    pub fn time_attack(roster: &Roster, player: (usize, usize), seed: &mut u32) -> Self {
        let ladder = crate::menu::arcade_ladder(roster, roster.characters[player.0].id, seed);
        Self { kind: Kind::TimeAttack(TimeAttack { player, ladder, stage: 0, total: 0, carried: 0, splits: Vec::new() }), frames: 0, start_health: [None; 2] }
    }

    pub fn survival(player: (usize, usize)) -> Self {
        let s = Survival { player, wins: 0, beaten: [0; 64], recent: Vec::new(), rank: 0, opponent: (0, 0), level: 1, difficulty: 1 };
        Self { kind: Kind::Survival(Box::new(s)), frames: 0, start_health: [None; 2] }
    }

    pub fn team(teams: [Team; 2], difficulty: usize) -> Self {
        let t = TeamBattle { teams, fight: 0, log: Vec::new(), incoming: 1, difficulty, just_lost: None };
        Self { kind: Kind::Team(t), frames: 0, start_health: [None; 2] }
    }

    pub fn mode(&self) -> Mode {
        match self.kind {
            Kind::TimeAttack(_) => Mode::TimeAttack,
            Kind::Survival(_) => Mode::Survival,
            Kind::Team(_) => Mode::TeamBattle,
        }
    }

    /// The VS screen's number: STAGE n (Time Attack), FIGHT n (Survival: total wins + 1; Team: match + 1).
    pub fn number(&self) -> u32 {
        match &self.kind {
            Kind::TimeAttack(t) => t.stage as u32 + 1,
            Kind::Survival(s) => s.total() + 1,
            Kind::Team(t) => t.fight + 1,
        }
    }

    /// Rounds to win, the round clock (None: the option) and both fighters' full health (0x800516CC).
    pub fn rules(&self, data: &ModeData) -> (u32, Option<u32>, i32) {
        data.rules[match self.kind {
            Kind::TimeAttack(_) => 0,
            Kind::Survival(_) => 1,
            Kind::Team(_) => 2,
        }]
    }

    /// Which side is a human.
    pub fn humans(&self) -> [bool; 2] {
        match &self.kind {
            Kind::Team(t) => [t.teams[0].human, t.teams[1].human],
            _ => [true, false],
        }
    }

    /// The next fight (state 2's per-type function): Time Attack's stage, Survival's next opponent (0x800B26D0
    /// and its level 0x800B0B84), Team's next pairing (0x800B09B0).
    pub fn setup(&mut self, roster: &Roster, data: &ModeData, seed: &mut u32, frame: u32) -> FightSetup {
        self.frames = 0;
        match &mut self.kind {
            Kind::TimeAttack(t) => {
                self.frames = t.carried;
                let mut s = crate::menu::arcade_setup(roster, t.player, 1, &t.ladder, t.stage);
                s.mode = Mode::TimeAttack;
                s
            }
            Kind::Survival(s) => {
                let w = s.wins;
                let first = data.pools.first().map(|p| p.1.clone()).unwrap_or_default();
                let pool = data.pools.iter().find(|(below, _)| below.is_none_or(|b| w < b)).map_or_else(|| first.clone(), |p| p.1.clone());
                let mut cands: Vec<usize> = pool.into_iter().filter(|c| !s.recent.contains(c) && roster.by_id(*c).is_some()).collect();
                if cands.is_empty() {
                    cands = first.into_iter().filter(|c| roster.by_id(*c).is_some()).collect();
                }
                // fewest beaten first, ties by id (0x8004CD80's insertion sort), then those within the spread
                cands.sort_by_key(|&c| s.beaten.get(c).copied().unwrap_or(0));
                let least = cands.first().map_or(0, |&c| s.beaten.get(c).copied().unwrap_or(0));
                let spread = if s.total() >= 20 { 4 } else { 0 };
                cands.retain(|&c| s.beaten.get(c).copied().unwrap_or(0) <= least + spread);
                // the draw: 0x8004CE54's x * 5 + 1 (0x8009F2A8) modulo the count; the PS1 also steps it while its
                // main loop waits for the vsync (0x80029720, ~2000-3000 times a frame), so only its picks match
                *seed = seed.wrapping_mul(5).wrapping_add(1);
                // (no candidate in the pools: the roster's first, not a panic)
                let id = cands.get((*seed % cands.len().max(1) as u32) as usize).copied().unwrap_or_else(|| roster.characters.first().map_or(0, |c| c.id));
                let costume = (frame & 1) as usize;
                s.recent.push(id);
                if s.recent.len() > 4 {
                    s.recent.remove(0);
                }
                let idx = roster.characters.iter().position(|c| c.id == id).unwrap_or(0);
                let costume = costume.min(roster.characters[idx].costumes.len() - 1);
                // the CPU's level and difficulty by the wins so far
                let r = rand(seed);
                let total = s.total();
                let (level, difficulty) = match total {
                    0..5 => (r % 3 + 1, 1),
                    5..10 => (r % 3 + 2, 1),
                    10..20 => (r % 5 + 3, 1),
                    20..50 => (r % 5 + 3, if frame & 0x10 != 0 { 2 } else { 1 }),
                    _ if total % 10 == 0 => (0, 1),
                    _ if total % 5 == 3 => (r % 4 + 6, 2),
                    _ => (r % 7 + 3, if frame & 0x14 != 0 { 2 } else { 1 }),
                };
                (s.level, s.difficulty, s.opponent) = (level.min(9) as usize, difficulty, (idx, costume));
                // the new opponent comes in at full health
                self.start_health[1] = None;
                let stage = roster.home_stage(id, costume);
                FightSetup { mode: Mode::Survival, players: [s.player, (idx, costume)], stage, difficulty, level: s.level, handicap: [100; 2], ball: None, partners: None }
            }
            Kind::Team(t) => {
                let pick = |k: usize| t.teams[k].members.get(t.teams[k].current).copied().unwrap_or((0, 0));
                let (p1, p2) = (pick(0), pick(1));
                // a CPU team's member: difficulty + 3 + (frame & 3), +1 for its second-to-last, +3 for its last
                let cpu = (0..2).find(|&k| !t.teams[k].human).unwrap_or(1);
                let left = t.teams[cpu].members.len().saturating_sub(t.teams[cpu].current);
                let bonus = match left {
                    1 => 3,
                    2 => 1,
                    _ => 0,
                };
                let level = (t.difficulty + 3 + (frame & 3) as usize + bonus).min(9);
                let side = t.incoming;
                let (who, costume) = [p1, p2][side];
                let stage = roster.home_stage(roster.characters[who].id, costume);
                FightSetup { mode: Mode::TeamBattle, players: [p1, p2], stage, difficulty: t.difficulty, level, handicap: [100; 2], ball: None, partners: None }
            }
        }
    }

    /// The match is over (the per-type end handlers): `health` and `max` are both fighters' (16.16 / whole).
    #[allow(clippy::too_many_arguments)]
    pub fn after(&mut self, setup: &FightSetup, winner: Option<usize>, health: [i32; 2], max: [i32; 2], data: &ModeData, records: &mut crate::save::Records, roster: &Roster) -> Next {
        let frames = self.frames;
        match &mut self.kind {
            Kind::TimeAttack(t) => {
                if winner == Some(0) {
                    t.total += frames;
                    t.carried = 0;
                    let opponent = roster.characters.get(setup.players[1].0).map_or(0, |c| c.id);
                    t.splits.push(Split { frames, player: t.player, opponent });
                    t.stage += 1;
                    if t.stage < t.ladder.len() {
                        return Next::Fight;
                    }
                    // the record: every stage with the same character (0x800B46B8)
                    let id = roster.characters[t.player.0].id;
                    let total: u32 = t.splits.iter().map(|s| s.frames).sum::<u32>().min(359_999);
                    let same = t.splits.iter().all(|s| s.player.0 == t.player.0);
                    let new_record = same && records.time_attack(id, total);
                    // the records screen opens on the time page after any finish (0x800B46EC)
                    records.page = 0;
                    return Next::Results(Box::new(Results::time_attack(t.clone(), total, new_record, data)));
                }
                // lost (or a draw): the attempt's time is kept for the retry
                t.carried = frames;
                Next::Continue(setup.clone())
            }
            Kind::Survival(s) => {
                if winner == Some(0) {
                    let cpu = roster.characters.get(setup.players[1].0).map_or(0, |c| c.id);
                    s.wins += 1;
                    if let Some(b) = s.beaten.get_mut(cpu) {
                        *b += 1;
                    }
                    s.rank = records.survival_rank(s.total());
                    // the human winner's health carries over, more for a quicker win (0x80051898)
                    let index = (frames / 300).min(7) as usize;
                    self.start_health[0] = (health[0] > 0).then(|| ModeData::carry(&data.carry_survival, index, health[0], max[0]));
                    return Next::Fight;
                }
                let id = roster.characters[s.player.0].id;
                let ranked = records.survival(id, s.total());
                Next::Results(Box::new(Results::survival((**s).clone(), ranked, data)))
            }
            Kind::Team(t) => {
                t.log.push(winner);
                t.fight += 1;
                // the loser's next member comes in at full health; a draw sends both in
                let losers: Vec<usize> = match winner {
                    Some(w) => vec![1 - w],
                    None => vec![0, 1],
                };
                self.start_health = [None; 2];
                if let Some(w) = winner {
                    // the winner keeps its health plus the team bonus: the less left, the more back (0x80051850)
                    let index = ((health[w].max(0) as i64 * 8 / max[w].max(kumite_sim::ONE) as i64) as usize).min(7);
                    self.start_health[w] = (health[w] > 0).then(|| ModeData::carry(&data.carry_team, index, health[w], max[w]));
                }
                for &l in &losers {
                    t.teams[l].current += 1;
                    t.incoming = l;
                }
                t.just_lost = winner.map(|w| 1 - w);
                if t.teams.iter().all(|team| team.current < team.members.len()) {
                    return Next::Fight;
                }
                Next::Results(Box::new(Results::team(t.clone(), data)))
            }
        }
    }
}

// ---------------------------------------------------------------- result screens (record 302)

pub enum ResultKind {
    TimeAttack { run: Box<TimeAttack>, total: u32, new_record: bool },
    Survival { run: Box<Survival>, ranked: bool, tally: Vec<(usize, u32)> },
    Team { run: TeamBattle },
}

pub struct Results {
    pub kind: ResultKind,
    pub frame: u32,
    /// Rows / tally steps shown so far; the fade (0..256, -4 a frame at the end).
    pub step: u32,
    pub fade: i32,
    pub leaving: bool,
    data: Value,
}

/// What a result screen wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultEvent {
    None,
    /// A row or tick appeared, the final line, the player's win voice (`ModeData::result_sfx`).
    Tick,
    Banner,
    Voice,
    /// Faded out: the records screen (mode 16).
    Done,
}

impl Results {
    fn time_attack(run: TimeAttack, total: u32, new_record: bool, data: &ModeData) -> Self {
        Self { kind: ResultKind::TimeAttack { run: Box::new(run), total, new_record }, frame: 0, step: 0, fade: 0, leaving: false, data: data.results.clone() }
    }
    fn survival(run: Survival, ranked: bool, data: &ModeData) -> Self {
        let mut tally: Vec<(usize, u32)> = run.beaten.iter().enumerate().filter(|(_, n)| **n > 0).map(|(c, n)| (c, *n)).collect();
        tally.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Self { kind: ResultKind::Survival { run: Box::new(run), ranked, tally }, frame: 0, step: 0, fade: 0, leaving: false, data: data.results.clone() }
    }
    fn team(run: TeamBattle, data: &ModeData) -> Self {
        Self { kind: ResultKind::Team { run }, frame: 0, step: 0, fade: 0, leaving: false, data: data.results.clone() }
    }

    /// The steps the screen counts up through: TA's ten rows, Survival's ticks, Team's log rows.
    fn steps(&self) -> u32 {
        match &self.kind {
            ResultKind::TimeAttack { .. } => 10,
            ResultKind::Survival { run, .. } => run.total(),
            ResultKind::Team { run } => run.log.len() as u32,
        }
    }

    /// Frames per step: TA 17 + 6, Survival 4, Team 9 + 2.
    fn step_frames(&self) -> u32 {
        match self.kind {
            ResultKind::TimeAttack { .. } => 23,
            ResultKind::Survival { .. } => 4,
            ResultKind::Team { .. } => 11,
        }
    }

    pub fn music(&self) -> usize {
        self.data["music"].as_u64().unwrap_or(0) as usize
    }

    /// START: on to the fade.
    pub fn skip(&mut self) {
        if self.fade >= 256 {
            self.leaving = true;
        }
    }

    pub fn tick(&mut self) -> ResultEvent {
        self.frame += 1;
        if self.leaving {
            self.fade -= 4;
            return if self.fade <= 0 { ResultEvent::Done } else { ResultEvent::None };
        }
        if self.fade < 256 {
            self.fade = (self.fade + 8).min(256);
            return ResultEvent::None;
        }
        let steps = self.steps();
        let per = self.step_frames();
        let start = 32;
        if self.step < steps {
            if self.frame >= start + per * (self.step + 1) {
                self.step += 1;
                return ResultEvent::Tick;
            }
            return ResultEvent::None;
        }
        // 30 frames after the last step: the final line and (TA, a ranked survival) the win voice
        let end = start + per * steps + 30;
        if self.frame == end {
            return match &self.kind {
                ResultKind::TimeAttack { .. } => ResultEvent::Voice,
                ResultKind::Survival { ranked: true, .. } => ResultEvent::Voice,
                _ => ResultEvent::Banner,
            };
        }
        // Team Battle counts its 1800 from the banner's end (0x800EF4AC sub-state 6), the others from the start
        let limit = if matches!(self.kind, ResultKind::Team { .. }) { end + 1800 } else { RESULT_FRAMES };
        if self.frame >= limit {
            self.leaving = true;
        }
        ResultEvent::None
    }

    /// Team Battle's banner: how far left of its place its text is, None before the last step. It slides in from the
    /// left, a screen's width over the 30 frames from the last step, the box drawn at their end (PS1 t3sim52).
    fn banner_slide(&self) -> Option<f32> {
        let last = 32 + self.step_frames() * self.steps();
        (self.step >= self.steps() && self.frame >= last).then(|| (crate::art::SCREEN_W as u32 * (last + 30).saturating_sub(self.frame) / 30) as f32)
    }

    fn done_counting(&self) -> bool {
        self.step >= self.steps() && self.frame >= 32 + self.step_frames() * self.steps() + 30
    }

    pub fn draw(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster) {
        Art::shade(painter, v, crate::art::FULL, [Color32::BLACK; 4]);
        // Survival's and Team Battle's backdrop (0x800F12DC): the select screen's quads in 0xA0C000 / 0x1830F0, its
        // banner and letters at 0x60
        let backdrop = match self.kind {
            ResultKind::Survival { .. } => Some(Color32::from_rgb(0, 192, 160)),
            ResultKind::Team { .. } => Some(Color32::from_rgb(240, 48, 24)),
            ResultKind::TimeAttack { .. } => None,
        };
        if let Some(c) = backdrop {
            roster.grid.bands(painter, v, c);
            roster.grid.scroll(painter, v, art, self.frame, Color32::from_gray(0xC0));
        }
        // Time Attack's backdrop: record 302's full-screen picture
        if let (ResultKind::TimeAttack { .. }, Some((tex, _))) = (&self.kind, art.pic("r302/000_full.png")) {
            Art::image(painter, v, &tex, 0.0, 0.0, crate::art::SCREEN_W, crate::art::SCREEN_H, Color32::WHITE);
        }
        match &self.kind {
            ResultKind::TimeAttack { run, total, new_record } => self.draw_ta(painter, v, art, roster, run, *total, *new_record),
            ResultKind::Survival { run, ranked, tally } => self.draw_survival(painter, v, art, roster, run, *ranked, tally),
            ResultKind::Team { run } => self.draw_team(painter, v, art, roster, run),
        }
        // the fade (subtractive black in the original)
        if self.fade < 256 {
            let a = (255 - self.fade.clamp(0, 255)) as u8;
            Art::shade(painter, v, crate::art::FULL, [Color32::from_black_alpha(a); 4]);
        }
    }

    /// A time mm'ss"cc: its digits' and marks' pieces, moved by `at`.
    fn time(art: &Art, painter: &egui::Painter, v: &View, t: &Value, frames: u32, at: [f32; 2]) {
        for (piece, p) in pieces(&t["digits"]).iter().zip(time_parts(frames)) {
            piece.draw_args(painter, v, art, at, &[p]);
        }
        for piece in pieces(&t["marks"]) {
            piece.draw_args(painter, v, art, at, &[]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_ta(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, run: &TimeAttack, total: u32, new_record: bool) {
        let d = &self.data["time_attack"];
        Text::from(&d["title"]).draw(painter, v, art);
        let xs: Vec<f32> = d["rows_x"].as_array().into_iter().flatten().map(n).collect();
        let ys: Vec<f32> = d["rows_y"].as_array().into_iter().flatten().map(n).collect();
        let fmt = [s(&d["stage"][0]), s(&d["stage"][1])];
        let per = self.step_frames();
        let row = &d["row"];
        let (label, b, f) = (Text::from(&row["label"]), &row["box"], [n(&row["face"][0]), n(&row["face"][1])]);
        let ([bx0, bx1], [by0, by1]) = ([n(&b["x"][0]), n(&b["x"][1])], [n(&b["y"][0]), n(&b["y"][1])]);
        for (k, split) in run.splits.iter().enumerate().take(self.step as usize + 1).take(10) {
            let (Some(&x), Some(&y)) = (xs.get(k), ys.get(k)) else { continue };
            // the row's box: a subtractive grey gradient to black (PS1: the backdrop less 222 at its left, nothing at
            // its right)
            let dark = Color32::from_black_alpha(u(&b["rgb"][0]) as u8);
            Art::shade(painter, v, [x + bx0, y + by0, bx1 - bx0, by1 - by0], [dark, Color32::TRANSPARENT, dark, Color32::TRANSPARENT]);
            let text = if k + 1 < 10 { fmt[0].replace("%1x", &format!("{:x}", k + 1)) } else { fmt[1].replace("%2d", &format!("{:2}", k + 1)) };
            Text { text, ..label.clone() }.draw_args(painter, v, art, [x, y], &[]);
            // the opponent's face at half size (0x800F0CFC), slashed once its row is done (flags 0x40, 0x800EFD24)
            face_sized(painter, v, art, roster, split.opponent, x + f[0], y + f[1], if (k as u32) < self.step { 0x40 } else { 0 }, 0.5);
            // the time counts up over the row's 16 frames
            let shown = if (k as u32) < self.step { split.frames } else { split.frames * ((self.frame.saturating_sub(32 + per * k as u32)).min(16)) / 16 };
            Self::time(art, painter, v, row, shown, [x, y]);
        }
        if self.done_counting() {
            Text::from(&d["your_time"]).draw(painter, v, art);
            // (the big font has no '"': the marks are three ')
            Self::time(art, painter, v, &d["total"], total, [0.0, 0.0]);
            let id = roster.characters[run.player.0].id;
            framed_face(painter, v, art, roster, id, n(&d["total"]["face"][0]), n(&d["total"]["face"][1]), 0);
            if new_record {
                let t = Text::from(&d["new_record"]);
                let colours: Vec<u32> = d["new_record"]["colours"].as_array().into_iter().flatten().map(u).collect();
                let c = colours.get(((self.frame >> 2) & 1) as usize).copied().unwrap_or(t.colour);
                t.draw_in(painter, v, art, c);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_survival(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, run: &Survival, ranked: bool, tally: &[(usize, u32)]) {
        let d = &self.data["survival"];
        Text::from(&d["title"]).draw(painter, v, art);
        // the faces of the beaten, 7 x 3, each counting up with the total
        let mut left = self.step;
        let colours: Vec<u32> = d["count_colours"].as_array().into_iter().flatten().map(u).collect();
        let l = &d["layout"];
        let (cf, cdx, cdy) = (u(&l["count"]["font"]) as usize, n(&l["count"]["dx"]), n(&l["count"]["dy"]));
        let places = l["faces"].as_array().cloned().unwrap_or_default();
        let fc = &l["face"];
        for (&(id, count), p) in tally.iter().zip(&places) {
            let (x, y) = (n(&p[0]), n(&p[1]));
            framed_face(painter, v, art, roster, id, x + n(&fc["dx"]), y + n(&fc["dy"]), u(&fc["flags"]));
            let shown = count.min(left);
            left -= shown;
            let c = colours.get(((shown / 100) & 3) as usize).copied().unwrap_or(6);
            let t = format!("{shown}");
            art.text(painter, v, cf, c, x + cdx - art.text_w(cf, &t), y + cdy, &t, Color32::WHITE);
        }
        Text::from(&d["total"]).draw(painter, v, art);
        // the total (0x800EFEA4), WIN / WINS after it
        let tl = &l["total"];
        art.text(painter, v, u(&tl["font"]) as usize, u(&tl["colour"]), n(&tl["x"]), n(&tl["y"]), &format!("{:>1$}", self.step, (u(&tl["width"]) as usize).min(8)), Color32::WHITE);
        let wins = if self.step == 1 { s(&d["win"][0]) } else { s(&d["win"][1]) };
        art.text(painter, v, 1, 6, n(&d["win_at"][0]), n(&d["win_at"][1]), &wins, Color32::WHITE);
        if self.done_counting() {
            if ranked {
                // "YOU ARE THE" and the rank with its suffix (by min(rank, 4), from 1st)
                let (you, suffix) = (s(&d["rank"]["text"]), s(&d["rank"]["suffixes"][(run.rank.clamp(1, 4) - 1) as usize]));
                let r = &l["rank"];
                let (f, x, y) = (u(&r["font"]) as usize, n(&r["x"]), n(&r["y"]));
                art.text(painter, v, f, u(&r["colour"]), x, y, &you, Color32::WHITE);
                let c = u(&r["blink"][usize::from(self.frame & 8 == 0)]);
                art.text(painter, v, f, c, x + art.text_w(f, &you), y, &format!(" {}{suffix}", run.rank), Color32::WHITE);
                Text::from(&d["greatest"]).draw(painter, v, art);
            } else {
                let t = Text::from(&d["practice"]);
                let colours: Vec<u32> = d["practice"]["colours"].as_array().into_iter().flatten().map(u).collect();
                t.draw_in(painter, v, art, colours.get(usize::from(self.frame & 0x10 != 0)).copied().unwrap_or(2));
            }
        }
    }

    fn draw_team(&self, painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, run: &TeamBattle) {
        let d = &self.data["team"];
        Text::from(&d["title"]).draw(painter, v, art);
        let l = &d["layout"];
        let (mb, lg, hd, bn) = (&l["members"], &l["log"], &l["headers"], &l["banner"]);
        let (n0, n1) = (run.teams[0].members.len(), run.teams[1].members.len());
        let (max_rows, columns) = (u(&mb["rows"]) as usize, (u(&lg["columns"]) as usize).max(1));
        // (the log moves down by the rows it does not use: (n0 + n1 + 1) / columns of them, 0x800EE40C)
        let dy = n(&mb["row_dy"]) * (max_rows - ((n0 + n1 + 1) / columns).min(max_rows)) as f32;
        // the headers' bars (0x800EDAE0): two additive gradients each, P1's red to black, P2's black to orange
        for (k, [x0, x1, x2, y0, y1], [c0, c1, c2]) in
            [(0, [16.0, 108.0, 168.0, 84.0, 98.0], [(255, 0, 0), (255, 0, 0), (0, 0, 0)]), (1, [200.0, 256.0, 352.0, 268.0, 282.0], [(0, 0, 0), (255, 192, 0), (255, 192, 0)])]
        {
            let [c0, c1, c2] = [c0, c1, c2].map(|(r, g, b)| crate::art::additive(Color32::from_rgb(r, g, b)));
            let y = y0 + if k == 1 { dy } else { 0.0 };
            Art::shade(painter, v, [x0, y, x1 - x0, y1 - y0], [c0, c1, c0, c1]);
            Art::shade(painter, v, [x1, y, x2 - x1, y1 - y0], [c1, c2, c1, c2]);
        }
        let header = |k: usize| if run.teams[k].human { s(&d["players"][k]) } else { "CPU".to_string() };
        art.text(painter, v, 1, 6, n(&hd["x"][0]), n(&hd["y"][0]), &header(0), Color32::WHITE);
        let h1 = header(1);
        let x1 = if run.teams[1].human { n(&hd["x"][1]) } else { n(&hd["x"][2]) };
        art.text(painter, v, 1, 6, x1, n(&hd["y"][1]) + dy, &h1, Color32::WHITE);
        // the members as they stand after the matches shown so far
        let mut down = [0usize; 2];
        for r in run.log.iter().take(self.step as usize) {
            match r {
                Some(w) => down[1 - w] += 1,
                None => {
                    down[0] += 1;
                    down[1] += 1;
                }
            }
        }
        // (0x800EE074: 8 slots, the beaten greyed, the one fighting framed red (P1) or yellow (P2), the empty ones grey
        // boxes)
        for (k, (team, down)) in run.teams.iter().zip(down).enumerate() {
            let y = if k == 0 { n(&mb["y"][0]) } else { n(&mb["y"][1]) + dy };
            for m in 0..8 {
                let x = n(&mb["x"]) + n(&mb["dx"]) * m as f32;
                let Some(&(idx, _)) = team.members.get(m) else {
                    Art::shade(painter, v, [x, y, 36.0, 64.0], [Color32::from_gray(48); 4]);
                    let (top, bottom) = (Color32::BLACK, Color32::from_gray(12));
                    Art::shade(painter, v, [x + 1.0, y + 2.0, 34.0, 60.0], [top, top, bottom, bottom]);
                    continue;
                };
                let id = roster.characters.get(idx).map_or(0, |c| c.id);
                let flags = match m.cmp(&down) {
                    std::cmp::Ordering::Less => 0x60,
                    std::cmp::Ordering::Equal => 2 - k as u32,
                    std::cmp::Ordering::Greater => 0,
                };
                framed_face(painter, v, art, roster, id, x, y, flags);
            }
        }
        // the log: "n.  VS" row by row in its columns (0x800EE250), with the two members who fought at half size, the
        // loser greyed
        // (its box, 0x800EE688: black, the lower half a gradient to the winner's colour; the matches to come as dim
        // rows, and the lines between the members who fought, 0x800EEAB8: the one shown next drawn over 8 frames)
        let colour = |r: &Option<usize>| match r {
            Some(0) => Color32::from_rgb(255, 0, 0),
            Some(_) => Color32::from_rgb(255, 192, 0),
            None => Color32::from_gray(192),
        };
        for i in 0..(n0 + n1).saturating_sub(1) {
            let (c, row) = (i % columns, i / columns);
            let (x, y) = (n(&lg["dx"]) * c as f32 + n(&lg["x"]) - 4.0, n(&lg["y"]) + n(&lg["dy"]) * row as f32 + dy);
            Art::shade(painter, v, [x, y, 95.0, 29.0], [Color32::BLACK; 4]);
            let low = run.log.get(i).filter(|_| (i as u32) < self.step).map_or(Color32::from_gray(64), colour);
            Art::shade(painter, v, [x, y + 15.0, 95.0, 14.0], [Color32::BLACK, Color32::BLACK, low, low]);
            if i as u32 >= self.step {
                for fx in [31.0, 71.0] {
                    Art::shade(painter, v, [x + fx, y, 16.0, 29.0], [Color32::from_gray(16); 4]);
                }
                art.text(painter, v, 0, 0x1A, x + 4.0, y + n(&lg["text_dy"]), &format!("{:2}.  VS", i + 1), Color32::WHITE);
            }
        }
        let mut at = [0usize; 2];
        for (i, r) in run.log.iter().enumerate().take(self.step as usize + 1) {
            let grow = if (i as u32) < self.step { 8 } else { self.frame.saturating_sub(32 + self.step_frames() * self.step).min(8) };
            let (x0, x1) = (57.0 + 36.0 * at[0] as f32, 57.0 + 36.0 * at[1] as f32);
            let (top, bottom) = (164.0, n(&mb["y"][1]) + dy);
            let part = |a: f32, b: f32| a + (b - a) * grow as f32 / 8.0;
            // (P1's win grows down from its member, P2's up, a draw from the middle)
            let [ax, ay, bx, by] = match r {
                Some(0) => [x0, top, part(x0, x1), part(top, bottom)],
                Some(_) => [part(x1, x0), part(bottom, top), x1, bottom],
                None => {
                    let (mx, my) = ((x0 + x1) / 2.0, (top + bottom) / 2.0);
                    [part(mx, x0), part(my, top), part(mx, x1), part(my, bottom)]
                }
            };
            if grow > 0 {
                let mut line = egui::Mesh::default();
                for (px, py) in [(ax, ay), (ax + 3.0, ay), (bx, by), (bx + 3.0, by)] {
                    line.colored_vertex(v.pos(px, py), colour(r));
                }
                line.indices.extend([0, 1, 2, 1, 3, 2]);
                painter.add(egui::Shape::mesh(line));
            }
            match r {
                Some(w) => at[1 - w] += 1,
                None => at = [at[0] + 1, at[1] + 1],
            }
        }
        let mut at = [0usize; 2];
        for (i, r) in run.log.iter().enumerate().take(self.step as usize) {
            let (c, row) = (i % columns, i / columns);
            let (x, y) = (n(&lg["dx"]) * c as f32 + n(&lg["x"]), n(&lg["y"]) + n(&lg["dy"]) * row as f32 + dy + n(&lg["text_dy"]));
            art.text(painter, v, 0, 6, x, y, &format!("{:2}.  VS", i + 1), Color32::WHITE);
            // (the faces' flags by the result: the loser's greyed and slashed)
            let flags = &lg["face_flags"][match r {
                Some(0) => "p1_won",
                Some(_) => "p2_won",
                None => "draw",
            }];
            for k in 0..2 {
                let id = run.teams[k].members.get(at[k]).and_then(|m| roster.characters.get(m.0)).map_or(roster.ui.question, |c| c.id);
                face_sized(painter, v, art, roster, id, x + n(&lg["faces_dx"][k]), y + n(&lg["faces_dy"]), u(&flags[k]), 0.5);
            }
            match r {
                Some(w) => at[1 - w] += 1,
                None => at = [at[0] + 1, at[1] + 1],
            }
        }
        if let Some(slide) = self.banner_slide() {
            let left = [n0.saturating_sub(down[0]), n1.saturating_sub(down[1])];
            let both = run.teams[0].human && run.teams[1].human;
            let b = &d["banners"];
            let (text, colour) = if left[0] == left[1] {
                (s(&b["draw"][0]), u(&b["draw"][1]))
            } else if both {
                let w = usize::from(left[1] > left[0]);
                (s(&b["player"][w][0]), u(&b["player"][w][1]))
            } else if left[0] > left[1] {
                (s(&b["win"][0]), u(&b["win"][1]))
            } else {
                (s(&b["lose"][0]), u(&b["lose"][1]))
            };
            let y = n(&bn["y"]) + n(&bn["dy"]) * (max_rows - ((n0 + n1 + 1) / 3).min(max_rows)) as f32;
            // (its half-transparent box, 0x800EDFD8, in the winner's colour)
            let (w, x) = (art.text_w(2, &text), crate::art::CENTRE - art.text_w(2, &text) / 2.0);
            let box_colour = match left[0].cmp(&left[1]) {
                std::cmp::Ordering::Equal => (192, 192, 192),
                std::cmp::Ordering::Greater => (255, 0, 0),
                std::cmp::Ordering::Less => (255, 192, 0),
            };
            if self.done_counting() {
                Art::shade(painter, v, [x - 8.0, y - 2.0, w + 16.0, 46.0], [Color32::from_rgba_unmultiplied(box_colour.0, box_colour.1, box_colour.2, 128); 4]);
            }
            art.text(painter, v, 2, colour, x - slide, y, &text, Color32::WHITE);
        }
    }
}

/// A face (0x8004B928): the character's, some in slices. Flags: 0x20 greyed (screens.json `grey_clut`), 0x40
/// the red slash over it.
#[allow(clippy::too_many_arguments)]
pub fn face(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, id: usize, x: f32, y: f32, flags: u32) {
    face_sized(painter, v, art, roster, id, x, y, flags, 1.0);
}

/// [`face`] in its frame, as 0x8004B928 draws one without flags bit 8: the cell frame's lines in set `flags & 3`, the
/// face 2, 3 in (0x8004BA58).
#[allow(clippy::too_many_arguments)]
fn framed_face(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, id: usize, x: f32, y: f32, flags: u32) {
    roster.grid.frame_lines(painter, v, x, y, (flags & 3) as usize, 0x80, None);
    face(painter, v, art, roster, id, x + 2.0, y + 3.0, flags);
}

/// [`face`] drawn at `scale`.
#[allow(clippy::too_many_arguments)]
pub fn face_sized(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, id: usize, x: f32, y: f32, flags: u32, scale: f32) {
    let src: &[&str] = &["exe"];
    let [fw, fh] = roster.ui.face_size;
    let (w, hh) = (fw as f32, fh as f32);
    // (full size: its sprites; half size: the same as quads, 0x800F0CFC)
    let slash = || {
        if flags & 0x40 == 0 {
            return;
        }
        let (size, half) = &roster.ui.half_slash;
        for (k, (t, w, h, dy)) in roster.ui.slash.iter().enumerate() {
            match half.get(k).filter(|_| scale != 1.0) {
                Some(hy) => art.quad(painter, v, src, t, *w, *h, [x, y + hy, *size, *size], [Color32::WHITE; 4]),
                None => art.sprite(painter, v, src, t, *w, *h, x, y + dy, Color32::WHITE),
            }
        }
    };
    // an extra fighter: its own face picture (greyed when beaten), slashed as the others
    let grey = (flags & 0x20 != 0).then(|| art.extra_face_grey(roster, id, roster.ui.grey_clut)).flatten();
    if let Some(t) = grey.or_else(|| art.extra_face_tex(roster, id)) {
        Art::image(painter, v, &t, x, y, w * scale, hh * scale, Color32::WHITE);
        slash();
        return;
    }
    // "?" (the gold one, or the blue one after it), in slices
    if id >= roster.ui.question || roster.ui.faces.get(id).is_none() {
        let clut = Some(roster.ui.question_cluts[usize::from(id != roster.ui.question)]);
        for &(t, h, dy) in &roster.ui.question_face {
            art.quad(painter, v, src, &Tex { clut, ..t }, fw, h, [x, y + dy * scale, w * scale, h as f32 * scale], [Color32::WHITE; 4]);
        }
        return;
    }
    let Some(t) = roster.ui.faces.get(id) else { return };
    let mut t = *t;
    if flags & 0x20 != 0 {
        t.clut = Some(roster.ui.grey_clut);
    }
    if let Some(slices) = roster.ui.face_slices.get(&id) {
        for &[du, dy, h] in slices {
            art.quad(painter, v, src, &t.offset(du, 0), fw, h, [x, y + dy as f32 * scale, w * scale, h as f32 * scale], [Color32::WHITE; 4]);
        }
    } else {
        art.quad(painter, v, src, &t, fw, fh, [x, y, w * scale, hh * scale], [Color32::WHITE; 4]);
    }
    slash();
}

/// Team Battle's VS screen extras (record 5 0x800B3460 / 0x800B3240 / 0x800B33E8): the two additive gradients,
/// and per side a grey box behind the strip of the other members' faces (upcoming ones first, "?" when not picked
/// by hand, then the defeated, greyed and slashed), sliding in with the portraits (`cnt` 7..0).
pub fn draw_team_vs(painter: &egui::Painter, v: &View, art: &Art, roster: &Roster, teams: &[Team; 2], cnt: f32) {
    use crate::art::additive;
    let d = &roster.screens["vs"]["team"];
    let rgb = |c: &Value| Color32::from_rgb(u(&c[0]) as u8, u(&c[1]) as u8, u(&c[2]) as u8);
    if cnt == 0.0 {
        let (g, h) = (additive(rgb(&d["bright"])), additive(rgb(&d["dim"])));
        let k = additive(Color32::BLACK);
        let rect = |q: &Value| [n(&q[0]), n(&q[1]), n(&q[2]) - n(&q[0]), n(&q[3]) - n(&q[1])];
        Art::shade(painter, v, rect(&d["gradients"][0]), [g, k, h, k]);
        Art::shade(painter, v, rect(&d["gradients"][1]), [k, h, k, g]);
    }
    // {x, y, step, x with eight members}
    let table = [0, 1].map(|p| [0, 1, 2, 3].map(|k| n(&d["strips"][p][k])));
    let (bdy, bh, grey) = (n(&d["box"]["dy"]), n(&d["box"]["h"]), rgb(&d["box"]["rgb"]));
    for p in [1, 0] {
        let t = &teams[p];
        let n = t.members.len().min(8);
        let [x0, y, step, x8] = table[p];
        let x = if n == 8 { x8 } else { x0 } + step * cnt;
        if cnt == 0.0 && n >= 2 {
            let bx = if p == 0 { x - 1.0 } else { x - 1.0 + (n as f32 - 2.0) * step };
            Art::shade(painter, v, [bx, y + bdy, (n as f32 - 1.0) * step.abs() + 1.0, bh], [grey; 4]);
        }
        let order = (t.current + 1..n).chain(0..t.current.min(n));
        for (k, m) in order.enumerate() {
            let fx = x + step * k as f32;
            let idx = t.members[m].0;
            let id = roster.characters.get(idx).map_or(roster.ui.question, |c| c.id);
            if m < t.current {
                face(painter, v, art, roster, id, fx, y, 0x60);
            } else {
                let hidden = m >= t.picked.max(t.current + 1);
                face(painter, v, art, roster, if hidden { roster.ui.question } else { id }, fx, y, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PS1 t3sim52 (TEAM BATTLE RESULTS, 1 v 1, YOU LOSE!; its frames 3092 on are ours from 0): the one log row's
    /// step at frame 43, the text's right edge at x 6 at 51 (3143), at its place (275) with the box at 73 (3165).
    #[test]
    fn the_team_banner_slides_in_as_on_the_ps1() {
        let run = TeamBattle { teams: [Team::default(), Team::default()], fight: 1, log: vec![Some(1)], incoming: 1, difficulty: 0, just_lost: None };
        let mut r = Results { kind: ResultKind::Team { run }, frame: 0, step: 0, fade: 0, leaving: false, data: Value::Null };
        let mut at = std::collections::BTreeMap::new();
        for _ in 0..80 {
            r.tick();
            at.insert(r.frame, (r.banner_slide().map(|s| (275.0 - s).round() as i32), r.done_counting()));
        }
        assert_eq!(at[&42], (None, false));
        assert_eq!(at[&43], (Some(-93), false));
        assert_eq!(at[&51], (Some(6), false));
        assert_eq!(at[&72], (Some(263), false));
        assert_eq!(at[&73], (Some(275), true));
    }

    /// #385, PS1 issue385/ps1 (0x800EF4AC): START once faded in opens TEAM BATTLE SELECT on the next frame (state 8),
    /// not the records; left alone, the fade starts 1800 frames after the banner, then the ranking (state 16).
    #[test]
    fn team_results_start_goes_back_to_team_select() {
        let gd = crate::test_assets().join("gamedata");
        if !kumite_fs::exists(&gd.join("screens.json")) {
            return;
        }
        let roster = Roster::load(&gd);
        let run = TeamBattle { teams: [Team::default(), Team::default()], fight: 1, log: vec![Some(1)], incoming: 1, difficulty: 0, just_lost: None };
        let results = || Results { kind: ResultKind::Team { run: run.clone() }, frame: 0, step: 0, fade: 0, leaving: false, data: Value::Null };
        let mut r = results();
        while r.tick() != ResultEvent::Banner {}
        let banner = r.frame;
        while !r.leaving {
            r.tick();
        }
        assert_eq!(r.frame, banner + 1800);
        for (frames, grid) in [(10, false), (40, true)] {
            let mut s = crate::menu::Screen::Results(Box::new(results()));
            if let crate::menu::Screen::Results(r) = &mut s {
                (0..frames).for_each(|_| _ = r.tick());
            }
            s.key(1, crate::menu::Key::Start, &roster, &mut 1);
            assert_eq!(matches!(s, crate::menu::Screen::Grid(_)), grid, "START after {frames} frames");
        }
    }

    #[test]
    fn survival_brings_the_extra_fighters_after_seven_wins() {
        let gd = crate::test_assets().join("gamedata");
        if !kumite_fs::exists(&gd.join("screens.json")) {
            return;
        }
        let roster = Roster::load(&gd);
        let extra: Vec<usize> = roster.characters.iter().filter(|c| !c.prefix.is_empty()).map(|c| c.id).collect();
        let bosses: Vec<usize> = roster.arcade.bosses().into_iter().filter(|c| extra.contains(c)).collect();
        if extra.is_empty() || bosses.is_empty() {
            return;
        }
        let data = ModeData::load(&roster.screens);
        let mut seed = 9;
        // the opponents met at a number of wins, over many draws
        let mut met = |wins: u32| {
            let mut ids = std::collections::BTreeSet::new();
            for k in 0..400 {
                let mut run = Run::survival((0, 0));
                if let Kind::Survival(s) = &mut run.kind {
                    s.wins = wins;
                }
                let setup = run.setup(&roster, &data, &mut seed, k);
                ids.insert(roster.characters[setup.players[1].0].id);
            }
            ids
        };
        assert!(met(0).iter().all(|&c| c < 10), "the original ten first");
        let mid = met(10);
        assert!(mid.iter().any(|c| extra.contains(c)), "the extra fighters from 7 wins: {mid:?}");
        assert!(!mid.iter().any(|c| bosses.contains(c)), "not yet their bosses: {mid:?}");
        let late = met(30);
        assert!(late.iter().any(|c| bosses.contains(c)), "their bosses from 17: {late:?}");
    }

    #[test]
    fn survival_carries_the_winners_health_as_on_the_ps1() {
        let gd = crate::test_assets().join("gamedata");
        if !kumite_fs::exists(&gd.join("screens.json")) {
            return;
        }
        let roster = Roster::load(&gd);
        let data = ModeData::load(&roster.screens);
        let mut records = crate::save::Records::default();
        let (mut run, mut seed) = (Run::survival((0, 0)), 1);
        let max = 140 * kumite_sim::ONE;
        // PS1 (t3sim45 surv3, Xiaoyu): health at the K.O. and fighting frames -> the next fight's health (16.16)
        for (health, frames, next) in [(0x46_0000, 54, 0x83_4000), (0xA_4000, 555, 0x31_A000), (0x12_A000, 440, 0x3A_0000)] {
            let setup = run.setup(&roster, &data, &mut seed, 0);
            run.frames = frames;
            assert!(matches!(run.after(&setup, Some(0), [health, 0], [max; 2], &data, &mut records, &roster), Next::Fight));
            assert_eq!(run.start_health[0], Some(next));
        }
    }

    #[test]
    fn survival_picks_as_on_the_ps1() {
        let gd = crate::test_assets().join("gamedata");
        if !kumite_fs::exists(&gd.join("screens.json")) {
            return;
        }
        let roster = Roster::load(&gd);
        let data = ModeData::load(&roster.screens);
        // PS1 (t3sim46 sv1, Xiaoyu from Eddy on): the beaten so far, the last opponents, 0x8004CE54's draw and the
        // frame counter 0x800AE218 -> the opponent (id, costume)
        let picks: [(&[usize], &[usize], u32, u32, (usize, usize)); 4] = [
            (&[8], &[8], 0x50D0_8171, 0x7BB, (5, 1)),
            (&[8, 5], &[8, 5], 0x32CF_C196, 0xC88, (7, 0)),
            (&[8, 5, 7], &[8, 5, 7], 0x0573_D6E4, 0x117D, (3, 1)),
            (&[8, 5, 7, 3], &[8, 5, 7, 3], 0x81D6_5037, 0x167F, (1, 1)),
        ];
        for (beaten, recent, draw, frame, want) in picks {
            let mut run = Run::survival((0, 0));
            if let Kind::Survival(s) = &mut run.kind {
                s.wins = beaten.len() as u32;
                beaten.iter().for_each(|&c| s.beaten[c] += 1);
                s.recent = recent.to_vec();
            }
            // (the state before the draw: x * 5 + 1 undone)
            let mut seed = draw.wrapping_sub(1).wrapping_mul(0xCCCC_CCCD);
            let (idx, costume) = run.setup(&roster, &data, &mut seed, frame).players[1];
            assert_eq!((roster.characters[idx].id, costume), want, "after {beaten:?}");
        }
    }

    #[test]
    fn carry_over_tables() {
        // a winner at 10 % of 170, index 0: gain[0] 256ths of the maximum back (made-up gains)
        let max = 170 * kumite_sim::ONE;
        let h = max / 10;
        let gain = [64, 50, 40, 30, 20, 10, 10, 10];
        let index = ((h as i64 * 8 / max as i64) as usize).min(7);
        assert_eq!(index, 0);
        assert_eq!(ModeData::carry(&gain, index, h, max), h + (max >> 8) * 64);
        // never above the maximum
        assert_eq!(ModeData::carry(&gain, 0, max - 1, max), max);
    }

    #[test]
    fn time_format() {
        assert_eq!(time_parts(3600 * 2 + 60 * 5 + 30), [2, 5, 50]);
        assert_eq!(time_parts(10_000_000), [99, 59, 98]);
    }
}
