//! Force's co-op of two and of four (OpenKumite's own) in both games: stage 1 with every player mashing among the
//! others, the soldiers hit the players, no player ever hits another, the run goes on while one stands and is lost
//! when all are down, and a second run gives the same state hashes. Tekken 3 on OPENKUMITE_ASSETS; Tekken Tag on
//! OK_TTT_ASSETS with the extras installed from both (kumite_packs::install_ttt); each skips without its data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::force::{ForceData, ForceEvent, ForceStage};
use kumite_game::{Actor, Game};
use kumite_sim::ai::{Ai, Style};
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

/// Stage `stage` (0..) of `a`'s force.json with `models` as the players (1P, 2P, 3P, 4P; selections `stride` a
/// character), all mashing (`ai`: the AI seats in these styles, as `--ai 1,2,3,4` plays them, hard at level 9, and
/// the game's seed, as `--seed`), for `frames` frames; the bout and stage after.
fn coop(a: &Assets, models: &[usize], stride: usize, stage: usize, frames: usize, ai: Option<([Style; 4], u32)>, hook: &mut dyn FnMut(usize, &mut Bout, &ForceStage)) -> (Seen, Bout, ForceStage) {
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
    // (co-op's balance, as the app sets it)
    game.handicap[0] = kumite_game::force::COOP_PLAYER_HEALTH;
    (game.ko_collapse, game.fight.ko_collapse) = (false, false);
    if let Some((_, seed)) = ai {
        game.copy_seed = seed;
    }
    let mut b = Bout::new(game, kumite_game::ai_data(a).unwrap(), 1, false);
    b.force = true;
    let (d, l) = (1, data.ai_level(stage));
    b.set_cpu(1, Some((d, l)));
    b.extra_cpu = Some(kumite_sim::ai::Ai::new(d, l, 1));
    let body = b.game.fight.fighters[0].character.unwrap_or(0).max(0) as usize;
    let mut st = ForceStage::new(data.clone(), stage, soldiers, body * stride, body, 0, 0, 0, 0);
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
    if let Some((styles, _)) = ai {
        let seat = |k: usize| Ai::new(2, 9, 0).with_style(styles[k]);
        b.set_cpu(0, Some((2, 9)));
        b.cpu[0] = Some(seat(0));
        b.partner_cpu = Some(seat(1));
        for (k, cpu) in b.more_cpu.iter_mut().enumerate().take(models.len() - 2) {
            *cpu = Some(seat(2 + k));
        }
        for i in b.game.players() {
            b.game.fight.actor_mut(i).unwrap().guard_input = false;
        }
    }
    let mut seen = Seen::default();
    for f in 0..frames {
        let pad = if ai.is_some() {
            Pad::default()
        } else {
            b.partner_pad = mash(1, f);
            for (k, slot) in b.more_human.iter_mut().take(2).enumerate() {
                *slot = Some(mash(2 + k, f));
            }
            mash(0, f)
        };
        b.step([pad, Pad::default()]);
        let fighting = b.phase == Phase::Fight;
        let won = matches!(b.phase, Phase::RoundOver { winner: Some(0) } | Phase::MatchOver { winner: Some(0) });
        st.step(&mut b.game, fighting, won);
        for e in std::mem::take(&mut st.events) {
            match e {
                ForceEvent::Swapped => b.swap_cpus(),
                // (a boss a player, as the app loads them)
                ForceEvent::LoadBoss(ch, cos) => {
                    let picks: Vec<(usize, usize)> = (0..b.game.players().len()).map(|j| if j == 0 { (ch, cos) } else { data.boss(body * stride, body, j) }).collect();
                    let model = |(c, k): (usize, usize)| {
                        let sels = roster["characters"].as_array().unwrap().iter().find(|e| e["id"] == c).unwrap()["selections"].as_array().unwrap().clone();
                        record(sels[k.min(sels.len() - 1)].as_u64().unwrap() as usize)
                    };
                    let actors: Vec<Actor> = picks.iter().map(|&p| Actor::load(a, model(p)).unwrap()).collect();
                    st.set_bosses(&mut b.game, &actors, &picks);
                    b.extra_cpu = Some(Ai::new(d, l, 1));
                    b.set_cpu(1, Some((d, l)));
                }
                _ => {}
            }
        }
        let player = kumite_sim::Fight::is_player;
        for h in &b.game.fight.hits {
            seen.friendly += u32::from(player(h.attacker) && player(h.victim));
            seen.struck += u32::from(!player(h.attacker) && player(h.victim));
            seen.landed += u32::from(player(h.attacker) && !player(h.victim));
        }
        seen.hashes.push(b.game.state_hash());
        hook(f, &mut b, &st);
        if !matches!(b.phase, Phase::Fight | Phase::Call) {
            break;
        }
    }
    (seen, b, st)
}

/// The co-op checks on `models`: hits both ways and none between players, the same hashes again, each player's
/// points adding to the total's share, the round on while one stands and lost when all are down.
fn check(a: &Assets, models: &[usize], stride: usize) {
    let (seen, mut b, st) = coop(a, models, stride, 0, 60 * 40, None, &mut |_, _, _| {});
    println!("{} players: soldiers struck {}, players landed {}, friendly {}; score {} {:?}", models.len(), seen.struck, seen.landed, seen.friendly, st.score, st.player_scores);
    assert_eq!(seen.friendly, 0, "no player hits another");
    assert!(seen.struck > 0, "the soldiers hit the players");
    assert!(seen.landed > 0, "the players hit the soldiers");
    assert!(st.player_scores.iter().sum::<u32>() <= st.score && st.player_scores.iter().filter(|&&s| s > 0).count() > 1, "{:?} of {}", st.player_scores, st.score);
    let again = coop(a, models, stride, 0, 60 * 40, None, &mut |_, _, _| {}).0;
    assert_eq!(seen.hashes, again.hashes, "a second run is the same");
    // down one by one: the round goes on until the last
    let players = b.game.players();
    for (k, &i) in players.iter().enumerate() {
        b.game.fight.actor_mut(i).unwrap().health = 0;
        // (a throw under way holds the K.O. off)
        for _ in 0..90 {
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

/// A player down while the others stand (one life a stage): it lies where it fell, nobody moves it (the stage's
/// scroll, the push, the soldiers' hits) and nobody takes it on, while the others fight on; the camera follows those
/// standing.
fn lies_where_it_fell(a: &Assets, models: &[usize], stride: usize, who: usize) {
    let (mut down_at, mut lying, mut moved, mut struck, mut targeted) = (None, None::<glam::Vec3>, 0.0f32, 0, 0);
    let (seen, b, _) = coop(a, models, stride, 0, 60 * 60, None, &mut |f, b, _| {
        if f == 120 {
            b.game.fight.actor_mut(who).unwrap().health = kumite_sim::ONE;
        }
        // (the others kept standing: how long the CPUs last is the run's luck, not this rule)
        for i in b.game.players().into_iter().filter(|&i| i != who && f > 120) {
            let p = b.game.fight.actor_mut(i).unwrap();
            p.health = p.max_health;
        }
        let me = b.game.fight.actor(who).unwrap();
        if down_at.is_none() && me.health <= 0 {
            down_at = Some(f);
        }
        let Some(d) = down_at else { return };
        // (its fall played out)
        if f >= d + 150 {
            let r = me.root();
            moved = moved.max((r - *lying.get_or_insert(r)).length());
            struck += b.game.fight.hits.iter().filter(|h| h.victim == who).count();
            let m = b.game.fight.coop.as_ref().unwrap();
            targeted += (0..m.actors.len()).filter(|&i| !kumite_sim::Fight::is_player(i) && m.target[i] == who && !m.actors[i].inactive).count();
        }
    });
    let d = down_at.expect("the player went down");
    let others = b.game.players().into_iter().filter(|&i| i != who).filter_map(|i| b.game.fight.actor(i)).filter(|p| p.health > 0).count();
    println!("{} players, {who} down at {d}: moved {moved}, struck {struck}, targeted {targeted}; others standing {others}, camera {}", models.len(), b.game.camera.view.eye[0]);
    assert!(others > 0 && seen.hashes.len() > d + 600, "the others fought on");
    assert!(moved < 1.0, "the downed player moved {moved} after its fall");
    assert_eq!((struck, targeted), (0, 0), "the downed player is out of the fight");
}

#[test]
fn tekken_3_a_downed_player_lies_where_it_fell() {
    let Some(a) = t3() else { return };
    lies_where_it_fell(&a, &[71, 79], 4, 3);
    lies_where_it_fell(&a, &[71, 79, 95], 4, 4);
    lies_where_it_fell(&a, &[71, 79, 95, 127], 4, 4);
}

#[test]
fn tekken_tag_a_downed_player_lies_where_it_fell() {
    let Some(a) = ttt() else { return };
    lies_where_it_fell(&a, &[372, 345], 8, 3);
    lies_where_it_fell(&a, &[372, 345, 432], 8, 0);
    lies_where_it_fell(&a, &[372, 345, 432, 351], 8, 4);
}

/// How four AI seats played stage `stage` (0..) with the game's `seed`: of the frames each actor stood in the fight,
/// those it spent stuck (in the last second it neither moved 150 units nor struck, threw or was struck: standing
/// about, walking into a wall or a partner, swinging at air), the players' and the soldiers'; the longest stretch with
/// every standing player stuck at once, and the longest a soldier or boss lay with health left; the frame the stage
/// was cleared (None: not within `frames`, or the clock ran out); whether it ended (cleared, lost or out of time);
/// whether the clock ran out.
fn stuck(a: &Assets, models: &[usize], stride: usize, stage: usize, styles: [Style; 4], seed: u32, frames: usize) -> ([[usize; 2]; 2], [usize; 2], Option<usize>, bool, bool) {
    const SECOND: usize = 60;
    let (mut trail, mut last_hit, mut lying) = (vec![Vec::<glam::Vec3>::new(); 12], [0usize; 12], [0usize; 12]);
    let (mut counts, mut all, mut longest, mut cleared, mut timed_out) = ([[0; 2]; 2], 0, [0; 2], None, false);
    let (_, b, _) = coop(a, models, stride, stage, frames, Some((styles, seed)), &mut |f, b, _| {
        let time_up = matches!(b.game.fight.state, kumite_sim::RoundState::TimeUp { .. });
        timed_out |= time_up;
        if matches!(b.phase, Phase::RoundOver { winner: Some(0) } | Phase::MatchOver { winner: Some(0) }) && !time_up {
            cleared.get_or_insert(f);
        }
        let g = &b.game.fight;
        // (a throw or hold under way counts as a hit)
        let links = g.coop.as_ref().map_or(&[][..], |m| &m.links[..]);
        for (a, v) in g.hits.iter().map(|h| (h.attacker, h.victim)).chain(links.iter().map(|l| (l.0, l.1))) {
            for i in [a, v].into_iter().filter(|&i| i < 12) {
                last_hit[i] = f;
            }
        }
        let (mut standing, mut stuck) = (0, 0);
        for i in 0..g.actor_count().min(12) {
            // (a soldier's place empties when it falls: its next one starts a trail of its own)
            let Some(p) = g.actor(i).filter(|p| p.health > 0 && !p.inactive && b.phase == Phase::Fight) else {
                (trail[i], lying[i]) = (Vec::new(), 0);
                continue;
            };
            let player = kumite_sim::Fight::is_player(i);
            lying[i] = if !player && p.posture(&g.data) & kumite_sim::posture::LYING != 0 { lying[i] + 1 } else { 0 };
            longest[1] = longest[1].max(lying[i]);
            let t = &mut trail[i];
            t.push(p.root());
            if t.len() <= SECOND {
                continue;
            }
            let then = t[t.len() - 1 - SECOND];
            let moved = glam::Vec2::new(p.root().x - then.x, p.root().z - then.z).length();
            let s = moved < 150.0 && f >= last_hit[i] + SECOND;
            let c = &mut counts[usize::from(!player)];
            c[0] += usize::from(s);
            c[1] += 1;
            (standing, stuck) = (standing + usize::from(player), stuck + usize::from(player && s));
        }
        all = if standing > 0 && stuck == standing { all + 1 } else { 0 };
        longest[0] = longest[0].max(all);
    });
    (counts, longest, cleared, !matches!(b.phase, Phase::Fight | Phase::Call), timed_out)
}

/// `players` AI seats (2, 3 or 4) on Tekken Tag's Force stages 1 to 3, ten seeds each, play like players: little time
/// stuck, never all of them at once for long, no soldier or boss lying about for good, stage 1 cleared in good time,
/// every stage over (cleared, lost or out of time) within 300 s, each stage cleared on 8 seeds of 10 at least and
/// never out of time. (A time up is GAME OVER: the app's seed 61 went so from stage 2 to the title, logging nothing,
/// which looked like a hang.) (The 0.7.0 video's four stood bunched against a wall for half a minute, a soldier out
/// of their reach past the view's side. After that, stage 2 ran out of time on some seeds with all four standing about
/// for over 90 s: a soldier back in a fallen one's place lay as it fell, in Tekken Tag's K.O. loop (here one lay 35 s,
/// stage 3 14.0% stuck); or one seat ran on away from its foe behind it and held the others' way on; or, the CPUs
/// never counting their attacks in a row down again, everyone stood off. Their attacks counted, the soldiers at the
/// game's health beat every team on stages 2 and 3: [`kumite_game::force`]'s co-op balance.)
fn keep_fighting(players: usize) {
    let Some(a) = ttt() else { return };
    use Style::*;
    // (Lei, Paul, Xiaoyu, Paul; Bryan, Julia, Lee, Anna as the video's; Hwoarang, King, Nina, Kunimitsu; the first
    // `players` of each)
    let picks = [
        ([372, 345, 432, 351], [Classic, Rushdown, Juggler, Thrower]),
        ([351, 372, 345, 432], [Evasive, Classic, Counter, Defensive]),
        ([510, 486, 630, 573], [Classic; 4]),
        ([426, 390, 414, 654], [Rushdown, Thrower, Evasive, Juggler]),
    ];
    let share = |c: [usize; 2]| 100.0 * c[0] as f32 / c[1].max(1) as f32;
    let mut failed = Vec::new();
    // (each stage: the players' stuck share at most, the slowest clear)
    for (stage, (max_stuck, clear)) in [(4.0, Some(60 * 120)), (10.0, None), (12.0, None)].into_iter().enumerate() {
        let (mut all, mut longest, mut lain, mut slowest, mut won) = ([[0; 2]; 2], 0, 0, 0, 0);
        // (ten seeds, the picks in turn)
        for (k, seed) in (11..=101).step_by(10).enumerate() {
            let (models, styles) = picks[k % picks.len()];
            let (c, l, cleared, ended, time_up) = stuck(&a, &models[..players], 8, stage, styles, seed, 60 * 300);
            // (the clock ends every stage: the next one, or GAME OVER, always comes)
            if !ended {
                failed.push(format!("stage {} seed {seed}: still on after 300 s", stage + 1));
            }
            if time_up {
                failed.push(format!("stage {} seed {seed}: out of time", stage + 1));
            }
            println!(
                "{players} players, stage {} {models:?} {styles:?} seed {seed}: players stuck {:?}, soldiers {:?}, all at once {}, a foe lying {}, cleared at {cleared:?}",
                stage + 1,
                c[0],
                c[1],
                l[0],
                l[1]
            );
            for (s, k) in all.iter_mut().flatten().zip(c.iter().flatten()) {
                *s += k;
            }
            (longest, lain, slowest, won) = (longest.max(l[0]), lain.max(l[1]), slowest.max(cleared.unwrap_or(usize::MAX)), won + usize::from(cleared.is_some()));
        }
        println!(
            "{players} players, stage {}: cleared {won} of 10; players stuck {:.1}%, soldiers {:.1}%; all at once at most {longest}, a foe lying {lain}; the slowest clear at {slowest}",
            stage + 1,
            share(all[0]),
            share(all[1])
        );
        if !(share(all[0]) < max_stuck && longest < 60 * 5 && lain < 60 * 10 && clear.is_none_or(|c| slowest < c) && won >= 8) {
            failed.push(format!("stage {}: {all:?}, {longest}, {lain}, {slowest}, cleared {won}", stage + 1));
        }
    }
    assert!(failed.is_empty(), "{players} players: {failed:?}");
}

#[test]
fn tekken_tag_two_ai_players_keep_fighting() {
    keep_fighting(2);
}

#[test]
fn tekken_tag_three_ai_players_keep_fighting() {
    keep_fighting(3);
}

#[test]
fn tekken_tag_four_ai_players_keep_fighting() {
    keep_fighting(4);
}
