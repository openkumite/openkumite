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

/// Law's and Jin's first models: Paul and Law against Lei and Jin.
const LAW: usize = 357;
const JIN: usize = 465;
/// The tag-out run off (alias 5394; from the stance straight, from other moves through 5398's turn) and the
/// entrance (5397).
const RUN_OFF: usize = 5394;
const ENTRANCE: usize = 5397;

fn tag_bout(a: &Assets, p1_partner: usize) -> Bout {
    let mut g = Game::load(a, [PAUL, LEI]).unwrap();
    g.start_tag([kumite_game::Actor::load(a, p1_partner).unwrap(), kumite_game::Actor::load(a, JIN).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b
}

/// P1 walks in (no guard while walking), Lei walks in jabbing until one lands (or the round ends), then 80 frames.
fn lei_jabs(b: &mut Bout) {
    let (walk, jab, toward) = (parse_script("f").unwrap()[0], parse_script("1").unwrap()[0], parse_script("b").unwrap()[0]);
    let before = b.game.fight.fighters[0].health;
    let mut k = 0;
    while k < 400 && b.game.fight.fighters[0].health == before && b.phase == Phase::Fight {
        b.step([walk, if k % 20 == 0 { jab } else { toward }]);
        k += 1;
    }
    for _ in 0..80 {
        b.step([Pad::default(); 2]);
    }
}

fn assets() -> Option<Assets> {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return None;
    };
    Some(Assets::new(std::path::Path::new(&root)).unwrap())
}

/// The tag button on frame 1 of the fight; then each frame: P1's alias, its team's active member, its moveset, its
/// root, the state hash.
fn tag_out(a: &Assets) -> Vec<(usize, usize, Option<i64>, glam::Vec3, u64)> {
    let mut b = tag_bout(a, LAW);
    let tag = parse_script("5").unwrap()[0];
    (0..150)
        .map(|k| {
            b.step([if k == 1 { tag } else { Pad::default() }, Pad::default()]);
            let f = &b.game.fight.fighters[0];
            let t = &b.game.fight.teams.as_ref().unwrap()[0];
            (f.alias, t.active, f.moveset.moveset_id, f.root(), b.game.state_hash())
        })
        .collect()
}

#[test]
fn tag_out_runs_off_and_the_partner_comes_in() {
    let Some(a) = assets() else { return };
    let r = tag_out(&a);
    let alias: Vec<usize> = r.iter().map(|x| x.0).collect();
    let tagging = alias.iter().position(|&x| x == RUN_OFF).unwrap_or_else(|| panic!("the tag button starts the run off: {alias:?}"));
    let swap = r.iter().position(|x| x.1 == 1).expect("the swap");
    // Law comes in on his own entrance where Paul left
    assert_eq!((r[swap].0, r[swap].2), (ENTRANCE, Some(1)));
    assert!(r[swap].3.distance(r[swap - 1].3) < 150.0, "the partner enters at the same spot: {} -> {}", r[swap - 1].3, r[swap].3);
    assert!(r[swap].3.distance(r[tagging].3) > 300.0, "Paul ran off first: {} -> {}", r[tagging].3, r[swap].3);
    assert!(r[swap..].iter().all(|x| x.1 == 1), "one swap");
    assert_eq!(r.iter().map(|x| x.4).collect::<Vec<_>>(), tag_out(&a).iter().map(|x| x.4).collect::<Vec<_>>(), "the same pads, the same match");
}

#[test]
fn recoverable_health_and_anger() {
    let Some(a) = assets() else { return };
    let mut b = tag_bout(&a, LAW);
    let one = kumite_sim::ONE;
    lei_jabs(&mut b);
    let (f, t) = (&b.game.fight.fighters[0], &b.game.fight.teams.as_ref().unwrap()[0]);
    let max = f.max_health * one;
    let damage = (max - f.health) / one;
    assert!(damage > 0, "the jab lands");
    assert_eq!(t.members[0].cap, max - damage * one * 6 / 10, "the cap loses 0.6 x the damage");
    // a hit on Paul angers Law: 4, and 1 more for the pair (the relation table)
    assert_eq!(t.members[1].anger, 100 + 5);
    // Paul tags out: benched, he regains max / 6000 a frame up to his cap
    let tag = parse_script("5").unwrap()[0];
    let mut swapped = None;
    for k in 0..150 {
        b.step([if k == 0 { tag } else { Pad::default() }, Pad::default()]);
        let t = &b.game.fight.teams.as_ref().unwrap()[0];
        if t.active == 1 && swapped.is_none() {
            swapped = Some((k, t.members[0].health));
        }
    }
    let (k, at) = swapped.expect("the swap");
    let paul = &b.game.fight.teams.as_ref().unwrap()[0].members[0];
    let step = paul.max_health * one / 6000;
    assert_eq!(paul.health, (at + step * (149 - k)).min(paul.cap), "max / 6000 a frame up to the cap {}", paul.cap);
    // an angry member (anger at 120 or more) deals 1.3 x: Law's jab, calm and angry
    let jab = |anger: i32| {
        let mut b = b.clone();
        b.game.fight.teams.as_mut().unwrap()[0].members[1].anger = anger;
        // (Lei walks in unguarded, Law jabs until one lands)
        let (jab, toward) = (parse_script("1").unwrap()[0], parse_script("b").unwrap()[0]);
        let lei = b.game.fight.fighters[1].health;
        for k in 0..400 {
            if b.game.fight.fighters[1].health != lei {
                break;
            }
            b.step([if k % 20 == 0 { jab } else { Pad::default() }, toward]);
        }
        (lei - b.game.fight.fighters[1].health) / one
    };
    let calm = jab(100);
    assert!(calm > 0, "Law's jab lands");
    // (it drains 1 a frame while Law walks up)
    assert_eq!(jab(1000), calm * 13 / 10);
}

#[test]
fn a_team_loses_when_its_member_falls() {
    let Some(a) = assets() else { return };
    let mut b = tag_bout(&a, LAW);
    // Law is in (Paul benched at full health), one point left: Lei's jab ends the round
    let tag = parse_script("5").unwrap()[0];
    for k in 0..150 {
        b.step([if k == 0 { tag } else { Pad::default() }, Pad::default()]);
    }
    assert_eq!(b.game.fight.teams.as_ref().unwrap()[0].active, 1);
    b.game.fight.fighters[0].health = kumite_sim::ONE;
    lei_jabs(&mut b);
    assert!(matches!(b.phase, Phase::RoundOver { winner: Some(1) }), "Lei and Jin win the round: {:?}", b.phase);
    // round 2 starts with Paul again, his body back, everyone at full health
    while !(b.round == 2 && b.phase == Phase::Fight) {
        b.step([Pad::default(); 2]);
    }
    let (f, t) = (&b.game.fight.fighters[0], &b.game.fight.teams.as_ref().unwrap()[0]);
    assert_eq!((t.active, f.moveset.moveset_id, b.game.bodies[0].model_record), (0, Some(0), PAUL));
    assert_eq!((t.members[1].health, t.members[1].cap), (t.members[1].max_health * kumite_sim::ONE, t.members[1].max_health * kumite_sim::ONE));
}

/// Paul's crouching u/f+3+4 (his alias 80 list): alias 1040 with Law as his partner only (req 0x65).
#[test]
fn a_partner_move_needs_its_partner() {
    let Some(a) = assets() else { return };
    let with = |partner: usize| {
        let mut b = tag_bout(&a, partner);
        let mut seen = Vec::new();
        for p in parse_script("d*20,u/f+3+4,n*30").unwrap() {
            b.step([p, Pad::default()]);
            seen.push(b.game.fight.fighters[0].alias);
        }
        seen.contains(&1040)
    };
    assert!(with(LAW), "with Law");
    assert!(!with(JIN), "not with Jin");
}
