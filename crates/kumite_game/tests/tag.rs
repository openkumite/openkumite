//! The tag brawl (four on the stage at once: two teams of two, or each for themselves) on the real converted
//! assets.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests and tools: a failure stops them

use kumite_assets::Assets;
use kumite_game::tag::{Slot, TagGame, TagPhase};
use kumite_sim::input::Pad;
use kumite_sim::melee::RETARGET_MARGIN;
use std::path::PathBuf;

mod brawl_fx;

fn assets() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

const MODEL_71: usize = 71;
const MODEL_79: usize = 79;
const MODEL_103: usize = 103;
const MODEL_87: usize = 87;
/// Models of the second pack (whose forward from a back turned stance is no turn).
const MODEL_902: usize = 902;
const MODEL_904: usize = 904;

#[test]
fn four_cpus_play_a_match() {
    let Some(a) = assets() else { return };
    let cpu = Some((2, 9));
    let slots = [MODEL_71, MODEL_79, MODEL_103, MODEL_87].map(|record| Slot { record, cpu });
    let mut g = TagGame::load(&a, slots, vec![0, 0, 1, 1], kumite_game::ai_data(&a).unwrap(), 2).expect("load");
    let mut struck = [false; 4];
    let mut hit = [false; 4];
    let mut frames = 0;
    while !g.finished() && frames < 60 * 60 * 6 {
        g.step([Pad::default(); 4]);
        for h in &g.melee.hits {
            assert_ne!(g.melee.team[h.attacker], g.melee.team[h.victim], "no friendly hits");
            struck[h.victim] = true;
            hit[h.attacker] = true;
        }
        for f in &g.melee.actors {
            assert!(f.position.is_finite());
        }
        frames += 1;
    }
    println!("{frames} frames, wins {:?}, rounds {}, struck {struck:?}, hit {hit:?}", g.wins, g.round);
    assert!(matches!(g.phase, TagPhase::MatchOver { .. }), "the match ends");
    assert!(g.wins.iter().any(|&w| w >= 2));
    assert!(struck.iter().filter(|&&s| s).count() >= 3, "most fighters get hit: {struck:?}");
    assert!(hit.iter().filter(|&&s| s).count() >= 3, "most fighters land hits: {hit:?}");
}

/// Each round's clock as set (Tekken Tag's ROUND TIME, `--round-time`): four idle humans' first and second rounds
/// end on time at it.
#[test]
fn the_rounds_run_on_the_clock_set() {
    let Some(a) = assets() else { return };
    let slots = [MODEL_71, MODEL_79, MODEL_103, MODEL_87].map(|record| Slot { record, cpu: None });
    let mut g = TagGame::load(&a, slots, vec![0, 0, 1, 1], kumite_game::ai_data(&a).unwrap(), 2).expect("load").with_round_frames(300);
    for round in 1..=2 {
        assert_eq!(g.melee.clock, 300);
        let (mut fought, mut frames) = (0, 0);
        while !matches!(g.phase, TagPhase::RoundOver { .. }) && frames < 2000 {
            fought += u32::from(g.phase == TagPhase::Fight);
            g.step([Pad::default(); 4]);
            frames += 1;
        }
        assert!(matches!(g.melee.state, kumite_sim::melee::MeleeState::TimeUp { .. }) && fought == 300, "round {round}: {fought} frames fought, {:?}", g.melee.state);
        while g.round == round && frames < 4000 {
            g.step([Pad::default(); 4]);
            frames += 1;
        }
    }
}

#[test]
fn targets_are_the_cheapest_foes() {
    let Some(a) = assets() else { return };
    let slots = [MODEL_71, MODEL_79, MODEL_103, MODEL_87].map(|record| Slot { record, cpu: None });
    let mut g = TagGame::load(&a, slots, vec![0, 0, 1, 1], kumite_game::ai_data(&a).unwrap(), 1).expect("load");
    for _ in 0..400 {
        g.step([Pad::default(); 4]);
    }
    assert_eq!(g.phase, TagPhase::Fight);
    // each fights a foe no more than the retarget margin dearer than the cheapest (the distance, one behind
    // counting further), as the pick before the CPUs think leaves them
    g.melee.retarget_now();
    let m = &g.melee;
    for i in 0..4 {
        // the pick's own pool: one in a throw keeps his foe; foes standing, else all those in play
        if m.linked_with(i).is_some() {
            continue;
        }
        let foes: Vec<usize> = (0..4).filter(|&j| m.team[j] != m.team[i] && !m.actors[j].inactive).collect();
        let standing: Vec<usize> = foes.iter().copied().filter(|&j| m.standing(j)).collect();
        let pool = if standing.is_empty() { &foes } else { &standing };
        let cheapest = pool.iter().map(|&j| m.pick_cost(i, j)).fold(f32::MAX, f32::min);
        let t = m.target[i];
        assert_ne!(m.team[i], m.team[t]);
        assert!(m.pick_cost(i, t) <= cheapest + RETARGET_MARGIN, "actor {i} targets {t} at cost {} (cheapest {cheapest})", m.pick_cost(i, t));
    }
    // the leash keeps everyone near the group's centre
    let centre = m.actors.iter().map(|f| f.root()).sum::<glam::Vec3>() / 4.0;
    for f in &m.actors {
        let d = f.root() - centre;
        assert!(glam::Vec2::new(d.x, d.z).length() <= kumite_sim::melee::LEASH + 50.0);
    }
}

#[test]
fn each_for_themselves_the_last_one_standing_wins() {
    let Some(a) = assets() else { return };
    let cpu = Some((2, 9));
    let slots = [MODEL_71, MODEL_79, MODEL_103, MODEL_87].map(|record| Slot { record, cpu });
    let mut g = TagGame::load(&a, slots, vec![0, 1, 2, 3], kumite_game::ai_data(&a).unwrap(), 2).expect("load");
    assert_eq!(g.wins.len(), 4);
    let mut hit_by = [[false; 4]; 4];
    let mut frames = 0;
    while !g.finished() && frames < 60 * 60 * 8 {
        g.step([Pad::default(); 4]);
        for h in &g.melee.hits {
            hit_by[h.victim][h.attacker] = true;
        }
        if let TagPhase::RoundOver { winner: Some(w) } = g.phase {
            if g.phase_frame == 1 && !matches!(g.melee.state, kumite_sim::melee::MeleeState::TimeUp { .. }) {
                let standing: Vec<usize> = (0..4).filter(|&i| g.melee.standing(i)).collect();
                assert_eq!(standing, vec![w], "only the winner stands");
            }
        }
        frames += 1;
    }
    println!("{frames} frames, wins {:?}, rounds {}, hit by {hit_by:?}", g.wins, g.round);
    assert!(matches!(g.phase, TagPhase::MatchOver { winner: Some(_) } | TagPhase::MatchOver { winner: None }));
    assert!(g.wins.iter().any(|&w| w >= 2));
    // anyone may hit anyone: someone gets hit by two or more others
    assert!(hit_by.iter().any(|row| row.iter().filter(|&&h| h).count() >= 2), "{hit_by:?}");
}

#[test]
fn a_cpu_turns_round_to_its_target_behind() {
    // actor 0 a CPU with its target (2, idle) straight behind, out of reach (where the game's routine turns
    // forward, which in these sets is no turn), the other foe out: it faces him within a second and a half
    let Some(a) = assets() else { return };
    if !a.root.join(format!("characters/model_{MODEL_904}.glb")).exists() {
        return eprintln!("skipping: no Tekken 2 pack (model {MODEL_904})");
    }
    let slot = |record, cpu| Slot { record, cpu };
    let slots = [slot(MODEL_904, Some((2, 9))), slot(MODEL_902, Some((2, 9))), slot(MODEL_904, None), slot(MODEL_902, None)];
    let mut g = TagGame::load(&a, slots, vec![0, 0, 1, 1], kumite_game::ai_data(&a).unwrap(), 1).expect("load");
    while g.phase != TagPhase::Fight {
        g.step([Pad::default(); 4]);
    }
    let east = std::f32::consts::FRAC_PI_2;
    for (i, (x, z, yaw)) in [(1000.0, 0.0, east), (0.0, -1800.0, east), (-1500.0, 0.0, east), (0.0, 1800.0, -east)].into_iter().enumerate() {
        let f = &mut g.melee.actors[i];
        (f.position, f.yaw, f.root_yaw) = (glam::Vec3::new(x, 0.0, z), yaw, yaw);
    }
    g.melee.actors[3].health = 0;
    g.melee.target[0] = 2;
    let off = |g: &TagGame| {
        let m = &g.melee;
        let d = m.actors[m.target[0]].root() - m.actors[0].root();
        let a = (m.actors[0].yaw - d.x.atan2(d.z)).rem_euclid(std::f32::consts::TAU);
        a.min(std::f32::consts::TAU - a).to_degrees()
    };
    assert!(off(&g) > 150.0, "set up back to him: {}", off(&g));
    let turned = (0..90).any(|_| {
        g.step([Pad::default(); 4]);
        off(&g) < 90.0
    });
    assert!(turned, "still {:.0} degrees off target {}", off(&g), g.melee.target[0]);
}

const GON: usize = 239;
const TRUE_OGRE: usize = 199;

/// Gon's fire (1+2) and True Ogre's breath (1+2) in the brawl: two on two they pass the team mate in the way and
/// strike and burn the opponent behind him; each for themselves they strike the one in the way; the same pads give
/// the same hashes.
#[test]
fn fire_and_breath_strike_across_teams() {
    use kumite_sim::objects::{BREATH, FIRE};
    let Some(a) = assets() else { return };
    for (model, byte, gap) in [(GON, FIRE, 1500.0), (TRUE_OGRE, BREATH, 2000.0)] {
        let records = [model, MODEL_71, MODEL_79, MODEL_103];
        let (hits, near, hashes) = brawl_fx::in_the_way(&a, records, [0, 0, 1, 1], "1+2,n*200", gap);
        assert!(hits.iter().all(|h| h.1 != 1), "no friendly hits: {hits:?}");
        assert!(hits.contains(&(0, 2, byte)), "{hits:?}");
        assert!(near[2] >= 60, "the opponent burns: {near:?}");
        assert_eq!(hashes, brawl_fx::in_the_way(&a, records, [0, 0, 1, 1], "1+2,n*200", gap).2, "the same pads, the same match");
        let (hits, _, _) = brawl_fx::in_the_way(&a, records, [0, 1, 2, 3], "1+2,n*200", gap);
        assert!(hits.first().is_some_and(|&h| h == (0, 1, byte)), "the one in the way: {hits:?}");
    }
}

/// TARGET through the step (each for themselves, 1P on the pad): a press locks on to the foe fought, the next ones move
/// on; no other foe is taken while the one locked on stands. The match from its tape, and saved, run on other pads
/// and put back (a rollback), is the same match frame by frame.
#[test]
fn target_presses_lock_on_and_play_again_alike() {
    let Some(a) = assets() else { return };
    let mut slots = [MODEL_71, MODEL_79, MODEL_103, MODEL_87].map(|record| Slot { record, cpu: Some((2, 9)) });
    slots[0].cpu = None;
    let load = || TagGame::load(&a, slots, vec![0, 1, 2, 3], kumite_game::ai_data(&a).unwrap(), 2).expect("load");
    // a press every 2 s, and some moves between
    let pad = |f: usize| Pad {
        right: f % 50 < 20,
        buttons: if f.is_multiple_of(120) {
            kumite_sim::input::TARGET
        } else if f.is_multiple_of(37) {
            1
        } else {
            0
        },
        ..Pad::default()
    };
    let mut g = load();
    g.tape = Some(Default::default());
    let (mut presses, mut held) = (0, 0);
    let mut seen = Vec::new();
    for f in 0..3000 {
        let before = (g.melee.target[0], g.melee.locked[0], g.phase == TagPhase::Fight);
        g.step([pad(f), Pad::default(), Pad::default(), Pad::default()]);
        seen.push((g.state_hash(), g.melee.target.clone(), g.melee.locked.clone()));
        let m = &g.melee;
        if g.phase != TagPhase::Fight || !before.2 || m.linked_with(0).is_some() {
            continue;
        }
        if f.is_multiple_of(120) {
            presses += 1;
            assert!(m.locked[0] && m.lockable(0, m.target[0]), "frame {f}: locked on to a foe in play");
            assert!(!before.1 || m.target[0] != before.0 || (0..4).filter(|&j| m.lockable(0, j)).count() == 1, "frame {f}: the next foe");
        } else if before.1 && m.lockable(0, before.0) {
            held += 1;
            assert_eq!((m.target[0], m.locked[0]), (before.0, true), "frame {f}: the lock holds");
        }
    }
    println!("{presses} presses, {held} frames held");
    assert!(presses >= 5 && held >= 500);
    // its tape played again
    let mut r = load();
    r.play(g.tape.as_ref().unwrap().to_vec().into());
    for (f, s) in seen.iter().enumerate() {
        r.step([Pad::default(); 4]);
        assert_eq!(&(r.state_hash(), r.melee.target.clone(), r.melee.locked.clone()), s, "replay frame {f}");
    }
    // rolled back every 5th frame after 7 frames of other pads (TARGET pressed in them too)
    let mut r = load();
    for (f, s) in seen.iter().enumerate() {
        if f.is_multiple_of(5) {
            let saved = r.clone();
            for k in 0..7 {
                r.step([pad(f + k + 60), Pad::default(), Pad::default(), Pad::default()]);
            }
            r = saved;
        }
        r.step([pad(f), Pad::default(), Pad::default(), Pad::default()]);
        assert_eq!(&(r.state_hash(), r.melee.target.clone(), r.melee.locked.clone()), s, "rollback frame {f}");
    }
}
