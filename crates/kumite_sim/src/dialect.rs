//! Another game's fight data on this engine: a dialect rewrites the game's own numbers (conditions, transitions,
//! gates, flag bits, timed property ops, the aliases the code names) into the engine's, which are Tekken 3's, once at
//! load. Only game data whose rules.json names a dialect comes through here; Tekken 3's never does.
//!
//! Tekken Tag Tournament (`"dialect": "ttt"`, ok_tekken_tag_converter docs/FIGHT_DATA.md, GAMEDATA.md): one move
//! table and one cancel table for the whole game, which become the common set; a fighter's moveset is only its
//! aliases, all into it.

use crate::data::{
    self, AirPushback, AirReaction, AirReactions, Aliases, Bank, Cancel, CancelKind, CloseRange, CloseRangeEntry, MoveRef, Moveset, PushbackCurve, PushbackCurves, ReactionEntry, Reactions,
    ReversalFamilies, Sequences, Shared, SharedCancels, ThrowVictims,
};
use crate::{BallRules, CharacterRules, GameData, MoveRules, TRANSITION_LINK};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// The widened `req` gates (`kind << 8 | value`, [`crate::Fighter::passes_gate`]); any other kind never passes.
pub const REQ_MOVESET: i64 = 1;
pub const REQ_NOT_MOVESET: i64 = 2;
pub const REQ_OPPONENT_MOVESET: i64 = 3;
pub const REQ_BODY: i64 = 4;
pub const REQ_NOT_BODY: i64 = 5;
pub const REQ_ALWAYS: i64 = 6;
const REQ_NEVER: u32 = 0xFF;
/// A condition code no arm of the engine's takes: never true.
const COND_NEVER: u32 = 0xFF;
/// The reversal-by-family condition (its parameter becomes the family's index).
const COND_FAMILY: u32 = 0x4B;
/// The engine's numbers for what Tekken 3 has not (their arms in lib.rs): the condition "not back turned" (Tekken
/// Tag's 0x25), and transitions 33 (restart facing the opponent), 34 / 35 (turn to the screen axis / across it).
pub const COND_NOT_BACK: u32 = 0x100;
pub const TRANSITION_FACE: u32 = 0x2D;
pub const TRANSITION_AXIS: u32 = 0x2E;
pub const TRANSITION_ACROSS: u32 = 0x2F;

/// A game's numbering, as ranges `(first, last, the engine's number for first)` and pairs `(its, the engine's)`.
pub struct Dialect {
    /// Condition codes; the others never pass.
    pub cond: &'static [(u32, u32, u32)],
    /// `req` gates: `(first, last, kind)`, the value being `req - first`; the others never pass.
    pub req: &'static [(u32, u32, u32)],
    /// Transition types (the low 7 bits); the others become a plain restart (6).
    pub transition: &'static [(u32, u32, u32)],
    /// Transition bits for the engine's 0x80 (turn around first) and 0x40.
    pub turn_bit: u32,
    pub bit6: u32,
    /// The plain-command bit of a button one on one has not (the tag button): its entries never match.
    pub never_button: u32,
    /// flags24 bits.
    pub flag_bits: &'static [(u32, u32)],
    /// Timed property ops; the others are dropped.
    pub ops: &'static [(u32, u32)],
    /// The reaction id meaning none.
    pub reaction_none: usize,
    pub fixed: Aliases,
}

/// Tekken Tag Tournament (FIGHT_DATA.md sections 1, 2, 5, 7, 8).
pub static TTT: Dialect = Dialect {
    // 0-0x24 kept; 0x25 not back turned; Tekken 3's 0x25-0x44 are 3 up; reversals from 0x60 (Tekken 3's 0x45); 0x26 /
    // 0x27 (unused) and 0x48-0x5F (camera, screen sides, off screen, round decided: tag-out runs and one win) never
    cond: &[(0, 0x24, 0), (0x25, 0x25, COND_NOT_BACK), (0x28, 0x47, 0x25), (0x60, 0x66, 0x45)],
    req: &[(0x01, 0x21, 1), (0x22, 0x42, 2), (0x43, 0x63, 3), (0x85, 0xA6, 4), (0xC9, 0xEA, 5), (0xED, 0xED, 6)],
    // 0-28 kept; 29 / 30 are Tekken 3's 0x1D / 0x1E, 31 / 32 its link (33), 36-45 its 0x22-0x2B, 46 its use END (44);
    // 33-35 the engine's own
    transition: &[(0, 28, 0), (29, 30, 0x1D), (31, 31, TRANSITION_LINK), (32, 32, TRANSITION_LINK), (33, 35, TRANSITION_FACE), (36, 45, 0x22), (46, 46, 0x2C)],
    turn_bit: 0x8000,
    bit6: 0x4000,
    never_button: 0x4000,
    flag_bits: &[(0, 8), (2, 10), (3, 11), (4, 12), (5, 13), (6, 14), (9, 16), (11, 18), (19, 28), (20, 29), (22, 30)],
    ops: &[(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8), (9, 9), (10, 10), (0x0C, 0x0B), (0x0D, 0x0C)],
    reaction_none: 0x592,
    fixed: Aliases { neutral: 58, crouch: 80, ko_collapse: 4769, missing: usize::MAX, air_late: usize::MAX, air_side: [usize::MAX; 2], air_recovery: usize::MAX, round_start: [usize::MAX; 3] },
};

fn range(t: &[(u32, u32, u32)], x: u32) -> Option<u32> {
    t.iter().find(|r| (r.0..=r.1).contains(&x)).map(|r| r.2 + x - r.0)
}

impl Dialect {
    /// A cancel entry in the engine's numbers. `offsets`: where each reversal family starts (the family
    /// condition's parameter is one). (The partner bit, `cond_tag`, tests the opponent's tagging, which one on one
    /// never happens: the bit's test passes and the code alone decides, 0x3693E0.)
    fn cancel(&self, mut c: Cancel, offsets: &[u32]) -> Cancel {
        if c.kind == CancelKind::Command && c.cmd & self.never_button != 0 {
            c.kind = CancelKind::Other;
        }
        if c.req != 0 {
            c.req = self.req.iter().find(|r| (r.0..=r.1).contains(&c.req)).map_or(REQ_NEVER << 8, |r| r.2 << 8 | (c.req - r.0));
        }
        c.cond = range(self.cond, c.cond).unwrap_or(COND_NEVER);
        if c.cond == COND_FAMILY {
            c.param = offsets.iter().position(|&o| o == c.param).map_or(u32::MAX, |k| k as u32);
        }
        let t = c.transition;
        c.transition = range(self.transition, t & 0x7F).unwrap_or(6) | if t & self.turn_bit != 0 { 0x80 } else { 0 } | if t & self.bit6 != 0 { 0x40 } else { 0 };
        c
    }

    fn reaction(&self, id: usize) -> usize {
        if id == self.reaction_none {
            usize::MAX
        } else {
            id
        }
    }
}

#[derive(Deserialize)]
struct TttCancels {
    entries: Vec<Cancel>,
}

#[derive(Deserialize)]
struct CancelsFile {
    cancels: TttCancels,
    shared_cancels: TttCancels,
    sequences: Sequences,
    reversal_families: Families,
}

#[derive(Deserialize)]
struct Families {
    families: Vec<Vec<i32>>,
    offsets: Vec<u32>,
}

#[derive(Deserialize)]
struct Hits {
    records: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    gate: Option<u32>,
    reaction: usize,
    close_reaction: usize,
    close_distance: i32,
}

#[derive(Deserialize)]
struct ReactionsFile {
    entries: Vec<TttReaction>,
    pushback_curves: TttCurves,
    /// (format 5 on)
    #[serde(default)]
    air_reactions: Option<TttAirs>,
}

#[derive(Deserialize)]
struct TttAirs {
    entries: Vec<TttAir>,
}

#[derive(Deserialize)]
struct TttAir {
    alias: usize,
    target_height: i32,
    pushback: Vec<i32>,
}

#[derive(Deserialize)]
struct TttReaction {
    victims: Vec<usize>,
    params: Vec<Option<i32>>,
}

#[derive(Deserialize)]
struct TttCurves {
    entries: Vec<TttCurve>,
}

#[derive(Deserialize)]
struct TttCurve {
    index: u32,
    constant_frames: i32,
    velocity: i32,
    impulses: Vec<i32>,
}

#[derive(Deserialize)]
struct MovesetFile {
    moveset_id: i64,
    aliases: Vec<Option<usize>>,
}

#[derive(Deserialize)]
struct Index {
    fighters: Vec<IndexFighter>,
}

#[derive(Deserialize)]
struct IndexFighter {
    moveset_file: String,
}

/// Tekken Tag's game data (rules.json read): the move and cancel tables as the common set, the hit records'
/// unconditional reactions on the moves, the reactions and pushback curves, the health table.
pub fn load(gamedata: &Path, rules: &serde_json::Value) -> Result<GameData> {
    let d = &TTT;
    let file: CancelsFile = data::load(&gamedata.join("cancels.json"))?;
    let offsets = &file.reversal_families.offsets;
    let norm = |l: Vec<Cancel>| l.into_iter().map(|c| d.cancel(c, offsets)).collect::<Vec<_>>();
    let (cancels, shared) = (norm(file.cancels.entries), norm(file.shared_cancels.entries));
    let hits: Hits = data::load(&gamedata.join("hits.json"))?;
    // the move record's hit chain: the first record whose gate passes; taken here is the one without a gate
    // shortcut: the gates (by the opponent's moveset) are not tried; walk the chain in the hit pass if one matters
    let ungated = |first: usize| hits.records.get(first..).and_then(|r| r.iter().find(|h| h.gate.is_none()));
    let mut moves: serde_json::Value = data::load(&gamedata.join("moves.json"))?;
    let list = moves.get_mut("moves").and_then(|m| m.as_array_mut()).context("moves.json: no moves")?;
    for (k, m) in list.iter_mut().enumerate() {
        let m = m.as_object_mut().with_context(|| format!("moves.json: move {k}"))?;
        let hit = m.get("hit").and_then(|h| h.as_u64()).unwrap_or(0) as usize;
        let h = ungated(hit);
        m.insert("reaction".into(), serde_json::json!({ "index": h.map_or(usize::MAX, |h| d.reaction(h.reaction)) }));
        m.insert("close_range".into(), h.filter(|h| h.close_distance != 0).map_or(0, |_| hit).into());
        // the reversal families hold motion numbers
        let motion = m.get("motion").cloned().unwrap_or_default();
        m.insert("motion_tag".into(), motion);
        // (a signed byte; one move's is negative)
        if m.get("hitstop").and_then(|h| h.as_i64()).is_some_and(|h| h < 0) {
            m.insert("hitstop".into(), 0.into());
        }
        if let Some(a) = m.get_mut("attack").and_then(|a| a.as_object_mut()) {
            let b = |k: &str| a.get("bones").and_then(|b| b.get(k)).and_then(|v| v.as_u64()).unwrap_or(0);
            let (e0, s0, e1, s1) = (b("end0"), b("start0"), b("end1"), b("start1"));
            let pairs: Vec<[u64; 2]> = [[s0, e0], [s1, e1]].into_iter().filter(|p| p[1] != 0).collect();
            a.insert("pairs".into(), pairs.into());
            a.insert("header_bytes".into(), vec![e0, s0, e1, s1].into());
        }
    }
    let mut moves: Vec<data::Move> = serde_json::from_value(moves["moves"].take()).context("moves.json")?;
    let map_bits = |raw: u32| d.flag_bits.iter().filter(|b| raw >> b.0 & 1 != 0).fold(0, |a, b| a | 1 << b.1);
    let op = |o: u32| d.ops.iter().find(|p| p.0 == o).map(|p| p.1);
    for m in &mut moves {
        m.flags24.raw = map_bits(m.flags24.raw);
        if let Some(ps) = m.properties.as_mut() {
            ps.retain_mut(|p| op(p.op).map(|o| p.op = o).is_some());
        }
    }
    // throws: a link move's reaction names the victim's alias in its first slot (Tekken 3: a throw-victim table)
    let r: ReactionsFile = data::load(&gamedata.join("reactions.json"))?;
    let air = r.air_reactions.map_or_else(Vec::new, |a| a.entries);
    let links: std::collections::BTreeSet<usize> = cancels.iter().chain(&shared).filter(|c| c.transition & 0x3F == TRANSITION_LINK).map(|c| c.dest).collect();
    let index: Index = data::load(&gamedata.join("movesets/index.json"))?;
    let files: std::collections::BTreeSet<&str> = index.fighters.iter().map(|f| f.moveset_file.as_str()).collect();
    for f in files {
        let set: MovesetFile = data::load(&gamedata.join("movesets").join(f))?;
        for &a in &links {
            if let Some(m) = set.aliases.get(a).copied().flatten().and_then(|i| moves.get_mut(i)) {
                m.reaction.table = "throw_victims".into();
            }
        }
    }
    let reactions = Reactions {
        throw_victims: ThrowVictims { entries: r.entries.iter().map(|e| e.victims.first().copied().unwrap_or(usize::MAX)).collect() },
        entries: r.entries.into_iter().map(|e| ReactionEntry { victims: e.victims, params: e.params.into_iter().map(|p| p.unwrap_or(-1)).collect() }).collect(),
        // shortcut: the curves' last two impulses are cut (zero in every curve of the USA disc)
        pushback_curves: PushbackCurves {
            entries: r
                .pushback_curves
                .entries
                .into_iter()
                .map(|c| PushbackCurve { index: c.index, constant_frames: c.constant_frames, velocity: c.velocity, impulses: std::array::from_fn(|k| c.impulses.get(k).copied().unwrap_or(0)) })
                .collect(),
        },
        // shortcut: every air hit takes entry 0's pushback (Tekken 3's one curve); the moves' own entries (8-43) have
        // others: give AirReaction its samples if those juggles drift wrong
        air_pushback_curve: AirPushback { samples: air.first().map_or_else(Vec::new, |a| a.pushback.clone()) },
        air_reactions: AirReactions { entries: air.into_iter().map(|a| AirReaction { target_height: a.target_height, alias: a.alias }).collect() },
        close_range: CloseRange { entries: hits.records.iter().map(|h| CloseRangeEntry { reaction_id: d.reaction(h.close_reaction), max_distance: h.close_distance }).collect() },
    };
    let health = &rules["health"];
    let juggle = |k: &str, d: i64| rules["juggle"][k].as_i64().unwrap_or(d) as i32;
    let percent = health["percent_per_character"].as_array().into_iter().flatten().map(|v| v.as_i64().unwrap_or(100) as i32).collect();
    Ok(GameData {
        common: Moveset { moveset_id: None, moves, cancels, aliases: None, camera_motions: Default::default(), fixed: d.fixed.clone() },
        shared: Shared { shared_cancels: SharedCancels { entries: shared }, sequences: file.sequences, reversal_families: ReversalFamilies { families: file.reversal_families.families } },
        reactions,
        zone_percent: Vec::new(),
        // life option 3, every match's (0x3B1488)
        max_health: health["default_max"].as_i64().unwrap_or(140) as i32,
        juggle_percent: [juggle("first_air_hit_percent", 80), juggle("later_air_hits_percent", 50)],
        characters: CharacterRules::default(),
        moves: MoveRules::default(),
        ball: BallRules::default(),
        health_percent: percent,
        // (0x3720C8) one 3 / 10, and chip without Tekken 3's + 2
        counter_bonus: rules["counter_hit"]["bonus_tenths"].as_array().map_or_else(|| vec![3], |v| v.iter().map(|k| k.as_i64().unwrap_or(0) as i32).collect()),
        chip_add: rules["chip_damage"]["add"].as_i64().unwrap_or(0) as i32,
        tag_juggles: true,
    })
}

/// A fighter's moveset (`movesets/moveset_NN.json`): its aliases, all into the common set.
/// `n`: the common set's move count.
pub fn moveset(n: usize, path: &Path) -> Result<Moveset> {
    let f: MovesetFile = data::load(path)?;
    let aliases = f.aliases.into_iter().map(|a| MoveRef { bank: if a.is_some() { Bank::Common } else { Bank::None }, index: a }).collect::<Vec<_>>();
    for (k, r) in aliases.iter().enumerate() {
        anyhow::ensure!(r.index.is_none_or(|i| i < n), "{}: alias {k} past the {n} moves", path.display());
    }
    let fixed = TTT.fixed.clone();
    anyhow::ensure!(aliases.get(fixed.neutral).is_some_and(|r| r.index.is_some()), "{}: no stance", path.display());
    Ok(Moveset { moveset_id: Some(f.moveset_id), moves: Vec::new(), cancels: Vec::new(), aliases: Some(aliases), camera_motions: Default::default(), fixed })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Tekken Tag condition code and transition type as FIGHT_DATA.md sections 7 and 8 pair them.
    #[test]
    fn codes_map_as_fight_data_says() {
        let cond = |x: u32| range(TTT.cond, x).unwrap_or(COND_NEVER);
        for x in 0..0x80 {
            let want = match x {
                0..=0x24 => x,
                0x25 => COND_NOT_BACK,
                // back turned left / right, own quadrants, opponent's, the always-false arm, then 0x30-0x44
                0x28..=0x47 => x - 3,
                0x60..=0x66 => x - 0x60 + 0x45,
                _ => COND_NEVER,
            };
            assert_eq!(cond(x), want, "condition {x:#x}");
        }
        let trans = |x: u32| range(TTT.transition, x).unwrap_or(6);
        for x in 0..0x80 {
            let want = match x {
                0..=28 => x,
                29 | 30 => x - 29 + 0x1D,
                31 | 32 => TRANSITION_LINK,
                33..=35 => x - 33 + TRANSITION_FACE,
                36..=45 => x - 36 + 0x22,
                46 => 0x2C,
                _ => 6,
            };
            assert_eq!(trans(x), want, "transition {x}");
        }
    }
}
