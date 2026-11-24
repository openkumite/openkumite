//! TAG BRAWL (OpenKumite's own mode) in Tekken Tag Tournament: four of its fighters on the stage at once, Tekken 3's
//! Gon and Doctor B. among them, on its pair rules. Needs a Tekken 3 game root (OPENKUMITE_ASSETS) and a Tekken Tag one
//! (OK_TTT_ASSETS); skips without either.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::tag::{Slot, TagGame, TagPhase};
use kumite_sim::input::Pad;
use ok_converter_common::io::StdFs;
use std::path::PathBuf;
use std::sync::OnceLock;

mod brawl_fx;

/// Paul's, Lei's and Xiaoyu's first models; Tekken 3's Gon and Doctor B.
const PAUL: usize = 345;
const LEI: usize = 372;
const XIAOYU: usize = 432;
const GON: usize = 239;
const DOCTOR_B: usize = 247;
/// Tekken Tag's True Ogre.
const TRUE_OGRE: usize = 591;

/// Tekken Tag's game root with the extras installed over it (once for the tests).
fn assets() -> Option<Assets> {
    let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
        eprintln!("skipped: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
        return None;
    };
    static EXTRAS: OnceLock<PathBuf> = OnceLock::new();
    let extras = EXTRAS.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ttt_brawl_extras");
        kumite_packs::install_ttt(&StdFs(ttt.clone().into()), &StdFs(t3.into()), &StdFs(dir.clone()), &|l| eprintln!("{l}")).unwrap();
        dir
    });
    kumite_fs::set_layers(std::path::Path::new(&ttt), vec![extras.clone()]);
    Some(Assets::new(std::path::Path::new(&ttt)).unwrap())
}

/// A brawl of `records` in `teams`, a CPU in each seat but the first (it is played by `p1`, idle: its pad empty) or
/// in every seat: to its end; the state hashes, the hits (attacker, victim), the widest a pair stood apart.
fn brawl(a: &Assets, records: [usize; 4], teams: [usize; 4], human: bool) -> (TagGame, Vec<u64>, Vec<(usize, usize)>, f32) {
    let cpu = Some((2, 9));
    let slots: [Slot; 4] = std::array::from_fn(|k| Slot { record: records[k], cpu: if k == 0 && human { None } else { cpu } });
    let mut g = TagGame::load(a, slots, teams.to_vec(), kumite_game::ai_data(a).unwrap(), 2).unwrap();
    g.set_arena(1);
    let (mut hashes, mut hits, mut widest) = (Vec::new(), Vec::new(), 0.0f32);
    while !g.finished() && hashes.len() < 60 * 60 * 8 {
        g.step([Pad::default(); 4]);
        hashes.push(g.state_hash());
        hits.extend(g.melee.hits.iter().map(|h| (h.attacker, h.victim)));
        let r: Vec<glam::Vec3> = g.melee.actors.iter().map(|f| f.root()).collect();
        for i in 0..4 {
            for j in i + 1..4 {
                widest = widest.max(glam::Vec2::new(r[i].x - r[j].x, r[i].z - r[j].z).length());
            }
        }
    }
    (g, hashes, hits, widest)
}

/// Two on two, a person (idle) in 1P's seat and the CPU in the other three, Gon and Doctor B. among them: the CPUs hit
/// across the teams only, a team wins the match, the pairs never part past Tekken Tag's limit (0x1D00, 7424), and the
/// same pads give the same match.
#[test]
fn two_on_two_runs_to_a_winner() {
    let Some(a) = assets() else { return };
    let records = [PAUL, GON, LEI, DOCTOR_B];
    let (g, hashes, hits, widest) = brawl(&a, records, [0, 0, 1, 1], true);
    println!("{} steps, wins {:?}, rounds {}, {} hits, widest {widest:.0}", hashes.len(), g.wins, g.round, hits.len());
    assert!(matches!(g.phase, TagPhase::MatchOver { winner: Some(_) }), "a team wins: {:?}", g.phase);
    assert!(hits.iter().all(|&(a, v)| g.melee.team[a] != g.melee.team[v]), "no friendly hits");
    assert!([1, 2, 3].iter().all(|&k| hits.iter().any(|h| h.0 == k)), "every CPU lands hits: {hits:?}");
    // (the limit pulls back before the moves: a frame's moves past it at most)
    assert!(widest < 7424.0 + 300.0, "a pair {widest:.0} apart");
    assert_eq!(hashes, brawl(&a, records, [0, 0, 1, 1], true).1, "the same pads, the same match");
}

/// Each for themselves, four CPUs: the last one standing takes the rounds; anyone hits anyone.
#[test]
fn free_for_all_runs_to_a_winner() {
    let Some(a) = assets() else { return };
    let (g, hashes, hits, _) = brawl(&a, [PAUL, LEI, XIAOYU, GON], [0, 1, 2, 3], false);
    println!("{} steps, wins {:?}, rounds {}, {} hits", hashes.len(), g.wins, g.round, hits.len());
    assert_eq!(g.wins.len(), 4);
    assert!(matches!(g.phase, TagPhase::MatchOver { .. }) && g.wins.iter().any(|&w| w >= 2), "{:?} {:?}", g.phase, g.wins);
    assert!((0..4).any(|v| (0..4).filter(|&k| hits.contains(&(k, v))).count() >= 2), "someone hit by two others");
}

/// Gon's fire (1+2) and True Ogre's (1+2, his strike to slot 0x18) in Tekken Tag's brawl: two on two they pass the
/// team mate in the way and strike and burn the opponent behind him; each for themselves they strike the one in the
/// way; the same pads give the same hashes.
#[test]
fn fire_strikes_across_teams() {
    let Some(a) = assets() else { return };
    for (model, gap) in [(GON, 1500.0), (TRUE_OGRE, 1800.0)] {
        let records = [model, PAUL, LEI, XIAOYU];
        let (hits, near, hashes) = brawl_fx::in_the_way(&a, records, [0, 0, 1, 1], "1+2*2,n*200", gap);
        assert!(hits.iter().all(|h| h.1 != 1), "no friendly hits: {hits:?}");
        assert!(hits.iter().any(|h| (h.0, h.1) == (0, 2)), "{hits:?}");
        assert!(near[2] >= 60, "the opponent burns: {near:?}");
        assert_eq!(hashes, brawl_fx::in_the_way(&a, records, [0, 0, 1, 1], "1+2*2,n*200", gap).2, "the same pads, the same match");
        let (hits, _, _) = brawl_fx::in_the_way(&a, records, [0, 1, 2, 3], "1+2*2,n*200", gap);
        assert!(hits.first().is_some_and(|h| (h.0, h.1) == (0, 1)), "the one in the way: {hits:?}");
    }
}
