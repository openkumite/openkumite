//! Force's co-op of two and of four (OpenKumite's own) in both games: stage 1 with every player mashing among the
//! others, the soldiers hit the players, no player ever hits another, the run goes on while one stands and is lost
//! when all are down, and a second run gives the same state hashes. Tekken 3 on OPENKUMITE_ASSETS; Tekken Tag on
//! OK_TTT_ASSETS with the extras installed from both (kumite_packs::install_ttt); each skips without its data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::force::{ForceData, ForceEvent, ForceStage};
use kumite_game::{Actor, Game};
use kumite_sim::input::Pad;
use ok_converter_common::io::StdFs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

fn t3() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from)?;
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

/// Tekken Tag's game root with the extras installed over it (once for the tests).
fn ttt() -> Option<Assets> {
    let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
        eprintln!("skipped: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
        return None;
    };
    static EXTRAS: OnceLock<PathBuf> = OnceLock::new();
    let extras = EXTRAS.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("force_four_extras");
        kumite_packs::install_ttt(&StdFs(ttt.clone().into()), &StdFs(t3.into()), &StdFs(dir.clone()), &|l| eprintln!("{l}")).unwrap();
        dir
    });
    kumite_fs::set_layers(std::path::Path::new(&ttt), vec![extras.clone()]);
    Some(Assets::new(std::path::Path::new(&ttt)).unwrap())
}

/// Player `p`'s pad at frame `f`: a new random one every 6 frames (punches, kicks, steps; on towards the soldiers).
fn mash(p: usize, f: usize) -> Pad {
    let mut x = ((f / 6) as u32).wrapping_mul(0x9E37_79B9) ^ (p as u32 + 1).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 13;
    let b = |k: u32| x >> k & 1 != 0;
    Pad { up: b(3) && b(4), down: b(5) && b(6), left: b(7) && b(2), right: !b(7), buttons: (x >> 9) as u8 & 0xF, start: false }
}

/// What a co-op stage saw: hits of soldiers on players, of players on soldiers, of players on players; the hashes.
#[derive(Debug, Default)]
struct Seen {
    struck: u32,
    landed: u32,
    friendly: u32,
    hashes: Vec<u64>,
}

/// Stage 1 of `a`'s force.json with `models` as the players (1P, 2P, 3P, 4P; selections `stride` a character),
/// all mashing, for `frames` frames; the bout and stage after.
fn coop(a: &Assets, models: &[usize], stride: usize, frames: usize) -> (Seen, Bout, ForceStage) {
    let data: Arc<ForceData> = Arc::new(a.gamedata("force.json").unwrap());
    let roster: serde_json::Value = a.gamedata("roster.json").unwrap();
    let record = |sel: usize| {
        let e = roster["selections"]["entries"].as_array().unwrap().iter().find(|e| e["selection"] == sel).unwrap();
        e["model_record"].as_u64().unwrap() as usize
    };
    let soldier = |k: usize| record(data.soldier_selection_base + data.soldier_costumes[0][k].min(3));
    let soldiers = [0, 1].map(|k| Actor::load(a, soldier(k)).unwrap());
    let mut game = Game::load(a, [models[0], soldier(0)]).unwrap();
    game.round_frames = kumite_game::force::CLOCK;
    (game.ko_collapse, game.fight.ko_collapse) = (false, false);
    let mut b = Bout::new(game, kumite_game::ai_data(a).unwrap(), 1, false);
    b.force = true;
    let (d, l) = (1, data.ai_level(0));
    b.set_cpu(1, Some((d, l)));
    b.extra_cpu = Some(kumite_sim::ai::Ai::new(d, l, 1));
    let body = b.game.fight.fighters[0].character.unwrap_or(0).max(0) as usize;
    let mut st = ForceStage::new(data.clone(), 0, soldiers, body * stride, body, 0, 0, 0, 0);
    st.setup(&mut b.game);
    // (as the app joins them: 2P, the soldiers' places widened, then 3P and 4P)
    b.game.join_partner(&Actor::load(a, models[1]).unwrap());
    st.widen(&mut b.game);
    let n = b.game.fight.more.len();
    b.more_cpu = (4..4 + n).map(|k| (!kumite_sim::Fight::is_player(k)).then(|| kumite_sim::ai::Ai::new(d, l, 1))).collect();
    b.more_human = vec![None; n];
    for (p, &m) in models.iter().enumerate().skip(2) {
        b.game.join_player(p, &Actor::load(a, m).unwrap());
    }
    assert_eq!(b.game.players().len(), models.len());
    let mut seen = Seen::default();
    for f in 0..frames {
        b.partner_pad = mash(1, f);
        for (k, slot) in b.more_human.iter_mut().take(2).enumerate() {
            *slot = Some(mash(2 + k, f));
        }
        b.step([mash(0, f), Pad::default()]);
        let fighting = b.phase == Phase::Fight;
        let won = matches!(b.phase, Phase::RoundOver { winner: Some(0) } | Phase::MatchOver { winner: Some(0) });
        st.step(&mut b.game, fighting, won);
        if std::mem::take(&mut st.events).iter().any(|e| matches!(e, ForceEvent::Swapped)) {
            b.swap_cpus();
        }
        let player = kumite_sim::Fight::is_player;
        for h in &b.game.fight.hits {
            seen.friendly += u32::from(player(h.attacker) && player(h.victim));
            seen.struck += u32::from(!player(h.attacker) && player(h.victim));
            seen.landed += u32::from(player(h.attacker) && !player(h.victim));
        }
        seen.hashes.push(b.game.state_hash());
        if !matches!(b.phase, Phase::Fight | Phase::Call) {
            break;
        }
    }
    (seen, b, st)
}

/// The co-op checks on `models`: hits both ways and none between players, the same hashes again, each player's
/// points adding to the total's share, the round on while one stands and lost when all are down.
fn check(a: &Assets, models: &[usize], stride: usize) {
    let (seen, mut b, st) = coop(a, models, stride, 60 * 40);
    println!("{} players: soldiers struck {}, players landed {}, friendly {}; score {} {:?}", models.len(), seen.struck, seen.landed, seen.friendly, st.score, st.player_scores);
    assert_eq!(seen.friendly, 0, "no player hits another");
    assert!(seen.struck > 0, "the soldiers hit the players");
    assert!(seen.landed > 0, "the players hit the soldiers");
    assert!(st.player_scores.iter().sum::<u32>() <= st.score && st.player_scores.iter().filter(|&&s| s > 0).count() > 1, "{:?} of {}", st.player_scores, st.score);
    let again = coop(a, models, stride, 60 * 40).0;
    assert_eq!(seen.hashes, again.hashes, "a second run is the same");
    // down one by one: the round goes on until the last
    let players = b.game.players();
    for (k, &i) in players.iter().enumerate() {
        b.game.fight.actor_mut(i).unwrap().health = 0;
        for _ in 0..30 {
            b.step([Pad::default(); 2]);
        }
        let last = k + 1 == players.len();
        assert_eq!(b.phase == Phase::Fight, !last, "{} of {} players down: {:?}", k + 1, players.len(), b.phase);
    }
}

#[test]
fn tekken_3_force_co_op_of_two_and_of_four() {
    let Some(a) = t3() else { return };
    // Paul, Law, King, Xiaoyu
    check(&a, &[71, 79], 4);
    check(&a, &[71, 79, 95, 127], 4);
}

#[test]
fn tekken_tag_force_co_op_of_two_and_of_four() {
    let Some(a) = ttt() else { return };
    // Lei, Paul, Xiaoyu, Jin
    check(&a, &[372, 345], 8);
    check(&a, &[372, 345, 432, 351], 8);
}
