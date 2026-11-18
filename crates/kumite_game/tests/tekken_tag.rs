//! A Tekken Tag Tournament match through the game layer: Paul against Lei with their real skeletons posed (hurt and
//! push spheres, the strikes' bone slots, the motions' root translation). Paul's jab whiffs from the start distance and
//! lands once he has walked in; a K.O. ends round 1 and round 2 starts; two runs give the same state hashes. Needs a
//! Tekken Tag `assets/` folder (ok_tekken_tag_converter's pack) in `OK_TTT_ASSETS`; skips without one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::sound::{SoundData, SoundState};
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
    assert!(matches!(b.phase, Phase::RoundOver { winner: Some(1) } | Phase::Replay { winner: Some(1) }), "Lei and Jin win the round: {:?}", b.phase);
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

/// Tekken Tag's own camera (camera.json, FIGHT_DATA.md section 18): H 615 on arena 0 with the depth 615 x 1900 / 256,
/// drawn as H 442 on Tekken 3's 368-pixel screen; arena 5's clamp starts at 500.
#[test]
fn tekken_tags_camera() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else { return };
    let a = Assets::new(std::path::Path::new(&root)).unwrap();
    let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
    if !a.root.join("gamedata/camera.json").exists() {
        return; // a pack from before its camera
    }
    g.set_arena(5);
    let d = g.camera.data();
    assert_eq!((d.h, d.h_range, d.distance, d.intro_blend, d.fit_px), (615, [500, 900], 4564, 13, 392));
    assert_eq!(g.camera.view.h, 442);
}

/// What the sound tables make a step play: the files under audio/sfx.
fn heard(a: &Assets, s: &SoundData, b: &Bout, st: &mut SoundState) -> Vec<String> {
    let mut out: Vec<_> = s.step(b, st, ["", ""], [0, 0]);
    out.extend(b.events.iter().filter_map(|&e| s.announce(e, [true; 2])));
    out.iter().map(|x| x.file.strip_prefix(a.root.join("audio/sfx")).unwrap().to_string_lossy().into_owned()).collect()
}

/// Paul walks in jabbing (Lei walks in, unguarded) until a jab lands or the round ends; the sounds of each step.
fn paul_jabs(a: &Assets, s: &SoundData, b: &mut Bout, st: &mut SoundState) -> Vec<Vec<String>> {
    let (walk, jab, toward) = (parse_script("f").unwrap()[0], parse_script("1").unwrap()[0], parse_script("b").unwrap()[0]);
    let before = b.game.fight.fighters[1].health;
    let mut seen = Vec::new();
    for k in 0..400 {
        if b.game.fight.fighters[1].health != before || b.phase != Phase::Fight {
            break;
        }
        b.step([if k % 20 == 0 { jab } else { walk }, toward]);
        seen.push(heard(a, s, b, st));
    }
    seen
}

/// The round call (rounds.json, 0x36E198): the fight starts on the call's frame 151 in round 1, 91 later, after
/// "Round" (0x5A), the number (0x48 + round - 1) and "Fight" (0x5B) on Tekken 3's frames; the start motions are
/// aliases 4 / 6, then 5. The arena's music is its BGM (arena 0: id 9, file 22).
#[test]
fn the_round_call_starts_the_fight_on_frame_151_then_91() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    let mut b = Bout::new(Game::load(&a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    let mut st = SoundState::default();
    for (round, starts, motions, calls) in
        [(1, 151, [4, 6], [(61, "001/090.wav"), (100, "001/072.wav"), (150, "001/091.wav")]), (2, 91, [5, 5], [(1, "001/090.wav"), (40, "001/073.wav"), (90, "001/091.wav")])]
    {
        assert_eq!(b.round, round);
        let (mut k, mut said, mut start) = (0, Vec::new(), [0; 2]);
        while b.phase == Phase::Call {
            b.step([Pad::default(); 2]);
            k += 1;
            if k == 1 {
                start = [0, 1].map(|i| b.game.fight.fighters[i].alias);
            }
            said.extend(heard(&a, &s, &b, &mut st).into_iter().filter(|f| f.starts_with("001/0")).map(|f| (k, f)));
        }
        // the fight's first step is the call's frame `starts` (inputs on)
        assert_eq!((k + 1, b.phase), (starts, Phase::Fight), "round {round}");
        assert!(start.iter().all(|x| motions.contains(x)), "round {round}'s start motions: {start:?}");
        assert_eq!(said, calls.map(|(k, f)| (k, f.to_string())), "round {round}'s call");
        b.resume_match(1, [0, 0]);
    }
    let (music, looped) = s.arena_music(0).unwrap();
    assert!(music.ends_with("audio/music/22.flac") && looped, "{}", music.display());
}

/// Paul's kick, a voiced move, a guarded jab and a K.O. by a jab: the samples sound.json names.
#[test]
fn hits_and_voices_play_their_samples() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    let mut b = tag_bout(&a, LAW);
    let mut st = SoundState::default();
    // walked in, Paul's d/f+4 (alias 416: 15 damage, end bone 14 a leg, the generic 0x7007): band 1's leg, id 0x0C
    let (walk, toward) = (parse_script("f").unwrap()[0], parse_script("b").unwrap()[0]);
    for _ in 0..40 {
        b.step([walk, toward]);
        heard(&a, &s, &b, &mut st);
    }
    let strike = |b: &mut Bout, st: &mut SoundState, input: &str, p2: Pad| {
        let p1 = parse_script(input).unwrap();
        (0..120).find_map(|k| {
            b.step([p1.get(k).copied().unwrap_or_default(), p2]);
            let s = heard(&a, &s, b, st);
            (!b.game.fight.hits.is_empty()).then_some(s)
        })
    };
    let sounds = strike(&mut b, &mut st, "d/f+4", toward).expect("the kick lands");
    assert!(b.game.fight.hits.iter().all(|h| !h.blocked) && sounds.contains(&"001/012.wav".to_string()), "{sounds:?}");
    // a jab on Lei standing (guarding): 0x10
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
    }
    let sounds = strike(&mut b, &mut st, "f*20,1,f*20,1,f*20,1,f*20,1,f*20,1", Pad::default()).expect("the jab is guarded");
    assert!(b.game.fight.hits.iter().all(|h| h.blocked) && sounds.contains(&"001/016.wav".to_string()), "{sounds:?}");
    // a move with a voice (alias 367: type 2, voice 7, Paul's id 0xDA, sample 8 of his bank 344) on its first frame
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
    }
    b.game.fight.fighters[0].play_alias(367);
    assert_eq!(heard(&a, &s, &b, &mut st), ["344/008.wav"]);
    // the K.O. by a jab (end bone 10, an arm): 0x0A; Lei's K.O. voice from his own bank (371)
    b.game.fight.fighters[1].health = kumite_sim::ONE;
    let ko = paul_jabs(&a, &s, &mut b, &mut st).pop().unwrap();
    assert!(ko.contains(&"001/010.wav".to_string()) && ko.iter().any(|f| f.starts_with("371/")), "{ko:?}");
}

/// After the K.O. the replay from frame 16, played twice; then a win pose 4770-4773 for 300 frames, the winner's
/// partner posing the same in the loser's place; round 2 with the loser back.
#[test]
fn the_winner_poses_with_its_partner() {
    let Some(a) = assets() else { return };
    let s = SoundData::load(&a).unwrap();
    for partner in [false, true] {
        let mut b = if partner {
            tag_bout(&a, LAW)
        } else {
            let mut b = Bout::new(Game::load(&a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
            while b.phase != Phase::Fight {
                b.step([Pad::default(); 2]);
            }
            b
        };
        let lei = b.game.fight.fighters[1].moveset.moveset_id;
        b.game.fight.fighters[1].health = kumite_sim::ONE;
        paul_jabs(&a, &s, &mut b, &mut SoundState::default());
        assert_eq!(b.phase, Phase::RoundOver { winner: Some(0) });
        let mut phases = Vec::new();
        while b.round == 1 {
            phases.push((b.phase, b.phase_frame, b.game.fight.fighters.each_ref().map(|f| f.alias), b.game.bodies[1].model_record, b.game.visible));
            b.step([Pad::default(); 2]);
        }
        let replay = phases.iter().position(|p| matches!(p.0, Phase::Replay { .. })).expect("the replay");
        assert_eq!(phases[replay - 1].1, 15, "the replay from the K.O.'s frame 16");
        let pose: Vec<_> = phases.iter().filter(|p| matches!(p.0, Phase::Result { .. })).collect();
        assert_eq!(pose.len(), 300, "the pose stage");
        let alias = pose[0].2[0];
        assert!((4770..=4773).contains(&alias), "the win pose {alias}");
        if partner {
            // Law in Lei's place, posing the same alias
            assert_eq!((pose[0].2[1], pose[0].3, pose[0].4), (alias, LAW, [true; 2]));
        } else {
            assert_eq!((pose[0].3, pose[0].4), (LEI, [true, false]));
        }
        assert_eq!((b.game.bodies[1].model_record, b.game.fight.fighters[1].moveset.moveset_id), (LEI, lei), "Lei back for round 2");
    }
}
