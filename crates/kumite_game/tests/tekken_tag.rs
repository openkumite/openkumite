//! A Tekken Tag Tournament match through the game layer: Paul against Lei with their real skeletons posed (hurt and
//! push spheres, the strikes' bone slots, the motions' root translation). Paul's jab whiffs from the start distance and
//! lands once he has walked in; a K.O. ends round 1 and round 2 starts; two runs give the same state hashes. Needs a
//! Tekken Tag `assets/` folder (ok_tekken_tag_converter's pack) in `OK_TTT_ASSETS`; skips without one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::{parse_script, Game};
use kumite_sim::input::Pad;

/// Paul's and Lei's first models (movesets/index.json).
const PAUL: usize = 345;
const LEI: usize = 372;
/// Paul's jab (his alias for 1).
const JAB: usize = 346;

/// What a run saw from the fight's first step: state hashes, Paul's alias, Lei's health, the round, the phase.
struct Run {
    hashes: Vec<u64>,
    alias: Vec<usize>,
    health: Vec<i32>,
    round: Vec<u32>,
    ko: bool,
}

fn run(a: &Assets) -> Run {
    let mut b = Bout::new(Game::load(a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    // Paul: a jab from the start, walk in, a jab; Lei walks in (no guard while walking)
    let p1 = parse_script("n*20,1,n*60,f*30,n*10,1,n*60").unwrap();
    let p2 = parse_script("n*20,b*400").unwrap();
    let mut r = Run { hashes: Vec::new(), alias: Vec::new(), health: Vec::new(), round: Vec::new(), ko: false };
    for _ in 0..600 {
        if b.phase == Phase::Fight {
            break;
        }
        b.step([Pad::default(); 2]);
    }
    assert_eq!(b.phase, Phase::Fight, "round 1 starts");
    let one = kumite_sim::ONE;
    for k in 0..1500 {
        let pad = |s: &[Pad]| s.get(k).copied().unwrap_or_default();
        if k == p1.len() {
            // one more jab, on one point left: the K.O.
            b.game.fight.fighters[1].health = one;
        }
        let jab = (p1.len()..p1.len() + 3).contains(&k) && b.round == 1;
        b.step([if jab { parse_script("1").unwrap()[0] } else { pad(&p1) }, pad(&p2)]);
        r.ko |= matches!(b.phase, Phase::RoundOver { winner: Some(0) });
        r.hashes.push(b.game.state_hash());
        r.alias.push(b.game.fight.fighters[0].alias);
        r.health.push(b.game.fight.fighters[1].health / one);
        r.round.push(b.round);
    }
    r
}

#[test]
fn paul_against_lei() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return;
    };
    let a = Assets::new(std::path::Path::new(&root)).unwrap();
    let r = run(&a);
    // Lei's health: the default life (140) times his percent (108)
    assert_eq!(r.health[0], 151);
    // the jab from the start distance whiffs
    assert!(r.alias[20..40].contains(&JAB), "the first jab: {:?}", &r.alias[20..40]);
    assert_eq!(r.health[80], 151, "the jab from afar whiffs");
    // walked in, the jab lands for 5
    assert!(r.alias[120..140].contains(&JAB), "the second jab: {:?}", &r.alias[120..140]);
    assert_eq!(r.health[180], 146, "the jab from near lands");
    // the K.O. ends round 1, and round 2 starts with full health
    assert!(r.ko, "the K.O.");
    let second = r.round.iter().position(|&n| n == 2).expect("round 2");
    assert_eq!(r.health[second + 1], 151);
    assert_eq!(r.hashes, run(&a).hashes, "the same pads, the same match");
}
