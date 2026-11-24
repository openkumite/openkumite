//! Online play's rollback in Tekken Tag Tournament: a match saved (cloned), run on with other pads, put back and run
//! again is the match run straight, frame by frame, with every character in a tag battle (their moves' effects:
//! fire, beams, tails, the wolf, Mokujin's copies, the tag runs). Needs a Tekken Tag `assets/` folder in
//! `OK_TTT_ASSETS`; skips without one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::Bout;
use kumite_game::{Actor, Game};
use kumite_sim::input::Pad;

/// Frames each match runs.
const FRAMES: usize = 3000;

/// 1P's pad at frame `f` (`salt` 0: the match's; else the wrong guess a rollback undoes): a new one every 9 frames,
/// the tag button among the buttons.
fn pad(f: usize, salt: usize) -> Pad {
    let mut x = ((f / 9) as u32 ^ (salt as u32).wrapping_mul(0x85EB_CA6B)).wrapping_mul(0x9E37_79B9);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 13;
    let b = |k: u32| x >> k & 1 != 0;
    Pad { up: b(3) && b(4), down: b(5) && b(6), left: b(7), right: b(8) && !b(7), buttons: (x >> 9) as u8 & 0x1F, start: false }
}

/// A tag battle of `models` (1P's pair, then the other's), 1P on the pads, the CPU the other side.
fn bout(a: &Assets, models: [usize; 4]) -> Bout {
    let mut g = Game::load(a, [models[0], models[2]]).unwrap();
    g.start_tag([Actor::load(a, models[1]).unwrap(), Actor::load(a, models[3]).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, true);
    b.set_cpu(1, Some((2, 7)));
    b
}

/// Every 5th frame the match is saved, run 7 frames on other pads, and put back (as a rollback does), then goes on:
/// its state hash every frame is the straight run's.
#[test]
fn a_rollback_resimulates_every_characters_match_as_run_straight() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return;
    };
    let a = Assets::new(std::path::Path::new(&root)).unwrap();
    // each character's first model, in id order
    let roster: serde_json::Value = serde_json::from_slice(&std::fs::read(a.root.join("gamedata/roster.json")).unwrap()).unwrap();
    let mut models: Vec<(u64, usize)> = Vec::new();
    for e in roster["selections"]["entries"].as_array().unwrap() {
        let (c, m) = (e["character"].as_u64().unwrap(), e["model_record"].as_u64().unwrap() as usize);
        if !models.iter().any(|x| x.0 == c) {
            models.push((c, m));
        }
    }
    assert!(models.len() >= 34, "{} characters", models.len());
    // (each character in the CPU's pair once: it plays its moves, effects and all)
    for k in (0..models.len()).step_by(2) {
        let four: [usize; 4] = std::array::from_fn(|j| models[(k + j + 2) % models.len()].1);
        let mut straight = bout(&a, four);
        let mut rolled = straight.clone();
        let mut rollbacks = 0;
        for f in 0..FRAMES {
            straight.step([pad(f, 0), Pad::default()]);
            if f % 5 == 0 {
                let saved = rolled.clone();
                for g in 0..7 {
                    rolled.step([pad(f + g, 1 + f), Pad::default()]);
                }
                rolled = saved;
                rollbacks += 1;
            }
            rolled.step([pad(f, 0), Pad::default()]);
            assert_eq!(rolled.game.state_hash(), straight.game.state_hash(), "models {four:?}: frame {f} after {rollbacks} rollbacks");
            assert_eq!((rolled.phase, rolled.round, rolled.wins), (straight.phase, straight.round, straight.wins), "models {four:?}: frame {f}");
        }
        println!("models {four:?}: {FRAMES} frames alike through {rollbacks} rollbacks; rounds {} wins {:?}", straight.round, straight.wins);
    }
}
