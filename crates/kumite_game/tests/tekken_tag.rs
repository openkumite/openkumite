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
/// Xiaoyu's first model.
const XIAOYU: usize = 432;
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
    let p1 = parse_script("n*4,1,n*76,f*30,n*10,1,n*60").unwrap();
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
    assert!(r.alias[4..24].contains(&JAB), "the first jab: {:?}", &r.alias[4..24]);
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

/// Xiaoyu's jab at Lei standing `gap` away: did it reach him (a hit or a block)?
fn xiaoyu_jab_reaches(a: &Assets, gap: f32) -> bool {
    let mut b = Bout::new(Game::load(a, [XIAOYU, LEI]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let f = &mut b.game.fight.fighters;
    let along = (f[1].position - f[0].position).normalize();
    f[1].position = f[0].position + along * gap;
    let mut reached = false;
    for k in 0..20 {
        b.step([if k < 3 { parse_script("1").unwrap()[0] } else { Pad::default() }, Pad::default()]);
        reached |= b.game.fight.hits.iter().any(|h| h.attacker == 0);
    }
    reached
}

/// A strike is tested in the pose of its frame (the PS2's strike segments at the active frame end at the bones it
/// shows then, PCSX2): Xiaoyu's jab, at full reach only on its active frame 8, reaches Lei from 1550 (the pose of
/// frame 7, 1300).
#[test]
fn a_strike_reaches_in_the_pose_of_its_frame() {
    let Some(a) = assets() else { return };
    let reach = (24..32).map(|k| k as f32 * 50.0).take_while(|&g| xiaoyu_jab_reaches(&a, g)).last();
    eprintln!("Xiaoyu's jab reaches Lei from {reach:?}");
    assert_eq!(reach, Some(1550.0), "the jab's full reach");
}

/// King's first model.
const KING: usize = 390;

/// The arcade cells' models (the 1P's partner, the CPU teams).
const YOSHIMITSU: usize = 396;
const NINA: usize = 408;
const MICHELLE: usize = 615;
const JULIA: usize = 480;
const ARMOR_KING: usize = 639;

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

/// Against the CPU, round 1 starts with the CPU's team alone on its shot for 92 frames (0x3B5328), the call after;
/// the same frames each time.
#[test]
fn the_cpu_team_shows_before_round_1() {
    let Some(a) = assets() else { return };
    if !a.root.join("gamedata/camera.json").exists() {
        return;
    }
    let run = || {
        let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
        g.start_tag([kumite_game::Actor::load(&a, LAW).unwrap(), kumite_game::Actor::load(&a, JIN).unwrap()]);
        let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
        b.team_intro = true;
        b.set_cpu(1, Some((1, 0)));
        let mut seen = Vec::new();
        for _ in 0..100 {
            b.step([Pad::default(); 2]);
            seen.push((b.phase, b.game.camera.view.eye));
        }
        seen
    };
    let seen = run();
    let shown = seen.iter().take_while(|s| s.0 == Phase::TeamIntro { team: 1 }).count();
    assert_eq!((shown, seen[shown].0), (92, Phase::Call), "{:?}", &seen[..3]);
    assert_ne!(seen[0].1, seen[60].1, "the shot moves");
    assert_eq!(seen, run());
}

/// The win pose's camera (0x35BB58): one eye H * 5200 / 955 + 1440 from the winner, closing 8 a frame.
#[test]
fn the_win_pose_camera_stands_by_h() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
    g.set_arena(5);
    let d = g.camera.data().clone();
    if d.win.is_none() {
        return; // a pack from before it
    }
    let p = g.subject(0);
    let v = kumite_sim::camera::EndCam::winner(&d, &p, true, &mut |_| 0).step(&d, &p);
    let (dx, dz) = ((v.eye[0] - p.root[0]) as f64, (v.eye[2] - p.root[2]) as f64);
    let r = (615 * 5200 / 955 + 1440 - 8) as f64;
    assert!(((dx * dx + dz * dz).sqrt() - r).abs() < 600.0, "{v:?} from {:?}", p.root);
}

/// A throw starts its move's camera (0x38C1E8): Paul's throw from up close goes to a situation (2-5) or a camera
/// motion (6) and back to the fight camera; the same each run.
#[test]
fn a_throw_has_its_camera() {
    let Some(a) = assets() else { return };
    let run = || {
        let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
        g.camera.data().director.as_ref()?; // (none in a pack from before it)
        let p1 = parse_script("f*40,n*2,1+3,n*200").unwrap();
        let mut seen = Vec::new();
        for pad in p1 {
            g.step([pad, Pad::default()]);
            seen.push((g.camera.special(), g.camera.view));
        }
        Some(seen)
    };
    let Some(seen) = run() else { return };
    let states: Vec<u8> = seen.iter().map(|s| s.0).collect();
    assert!(states.iter().any(|&s| s != 1), "no special camera: {states:?}");
    assert_eq!(states.last(), Some(&1), "{states:?}");
    assert_eq!(Some(seen), run());
}

/// The K.O. replay runs on Tekken Tag's own shots (0x3D0748): a still eye per pass, a new one each pass; the same
/// each time.
#[test]
fn the_ko_replay_has_its_own_shots() {
    let Some(a) = assets() else { return };
    if !a.root.join("gamedata/camera.json").exists() {
        return;
    }
    let run = || {
        let mut b = tag_bout(&a, LAW);
        b.game.fight.fighters[0].health = kumite_sim::ONE;
        let (walk, jab) = (parse_script("f").unwrap()[0], parse_script("1").unwrap()[0]);
        for k in 0..400 {
            if b.phase != Phase::Fight {
                break;
            }
            b.step([walk, if k % 20 == 0 { jab } else { Pad::default() }]);
        }
        let mut eyes = Vec::new();
        while !matches!(b.phase, Phase::Result { .. }) && eyes.len() < 400 {
            if matches!(b.phase, Phase::Replay { .. }) {
                assert!(matches!(b.game.camera_mode, kumite_game::CameraMode::Shot { .. }));
                eyes.push(b.game.camera.view.eye);
            }
            b.step([Pad::default(); 2]);
        }
        eyes
    };
    let eyes = run();
    let mut shots = eyes.clone();
    shots.dedup();
    assert!((2..=4).contains(&shots.len()), "a still eye a pass: {shots:?} of {}", eyes.len());
    assert_eq!(eyes, run());
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
    let music = s.arena_music(0).unwrap();
    assert!(music.file.ends_with("audio/music/22.flac") && music.looped, "{}", music.file.display());
    // (arena 0's BGM 9: its loop point and volume 9830, the PS2's level)
    assert_eq!((music.loop_at, music.gain), (3.779047619047619, 9830.0 / 16384.0 * 0.85 / 0.55));
    // BGM 7 is the staff roll's stream further on
    let (roll, ending) = (s.music(6).unwrap(), s.music(7).unwrap());
    assert!(roll.file == ending.file && roll.start == 0.0 && ending.start > 134.0);
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

/// After the K.O. the replay from frame 16: three passes of the record's last 20 frames, then the fight on to the
/// state's frame 135; then a win pose 4770-4773 until the first pose ends (Paul's all end before the 300 frames),
/// the winner's partner posing the same in the loser's place; round 2 with the loser back.
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
        let replayed = phases.iter().filter(|p| matches!(p.0, Phase::Replay { .. })).count();
        assert_eq!(replayed, 1 + 3 * 19 + 118, "three passes, then the live frames");
        assert!(pose.len() < 300, "the pose stage ends with the pose: {}", pose.len());
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

/// What a match of Paul against Lei did over `frames` steps, each side a CPU at (difficulty, level) or an idle pad:
/// the state hashes, and per side its fight steps holding forward without a button (walks), its button presses
/// (attacks), its steps in a guard, the hits it landed unblocked, and the rounds won.
#[derive(Debug, Default)]
struct CpuRun {
    hashes: Vec<u64>,
    walks: [u32; 2],
    attacks: [u32; 2],
    guards: [u32; 2],
    hits: [u32; 2],
    wins: [u32; 2],
    /// Per side: attack moves started (a new move with active frames); fight steps; side 1 turned away (back to
    /// side 0).
    starts: [u32; 2],
    fight: u32,
    turned: u32,
    /// The first CPU's: string follow-ups, canned throws, delayed inputs.
    stats: [u32; 3],
}

fn cpu_run(a: &Assets, cpu: [Option<(usize, usize)>; 2], frames: usize) -> CpuRun {
    cpu_run_of(a, [PAUL, LEI], cpu, frames, 2)
}

/// `rounds` to win the match: more fighting time for rare behaviour (the round flow takes its own frames).
fn cpu_run_of(a: &Assets, models: [usize; 2], cpu: [Option<(usize, usize)>; 2], frames: usize, rounds: u32) -> CpuRun {
    let mut b = Bout::new(Game::load(a, models).unwrap(), kumite_game::ai_data(a).unwrap(), rounds, false);
    for (k, c) in cpu.into_iter().enumerate() {
        b.set_cpu(k, c);
    }
    let mut r = CpuRun::default();
    let mut last = [0; 2];
    for _ in 0..frames {
        b.step([Pad::default(); 2]);
        r.hashes.push(b.game.state_hash());
        if b.phase != Phase::Fight {
            continue;
        }
        r.fight += 1;
        r.turned += u32::from(b.game.fight.fighters[1].angles().turn > 0x4000);
        for (k, f) in b.game.fight.fighters.iter().enumerate() {
            if f.alias != last[k] && f.current_move(&b.game.fight.data).active[0] != 0 {
                r.starts[k] += 1;
            }
            last[k] = f.alias;
            r.walks[k] += u32::from(f.input.dir == 6 && f.input.held == 0);
            r.attacks[k] += u32::from(f.input.pressed != 0);
            r.guards[k] += u32::from(f.posture(&b.game.fight.data) & kumite_sim::posture::GUARDS != 0);
        }
        for h in b.game.fight.hits.iter().filter(|h| !h.blocked && h.attacker < 2) {
            r.hits[h.attacker] += 1;
        }
    }
    r.wins = b.wins;
    if let Some(ai) = b.cpu.iter().flatten().next() {
        r.stats = [ai.strings, ai.canned, ai.delayed];
    }
    r
}

/// Two CPUs at NORMAL (the default difficulty), fight 1's level: they walk, attack, guard and land hits within 40
/// seconds, and the same match plays again the same.
#[test]
fn normal_cpus_walk_attack_guard_and_hit() {
    let Some(a) = assets() else { return };
    let r = cpu_run(&a, [Some((1, 0)); 2], 2400);
    eprintln!("{r:?}", r = (r.walks, r.attacks, r.guards, r.hits, r.wins));
    for k in 0..2 {
        assert!(r.walks[k] > 0 && r.attacks[k] > 0 && r.guards[k] > 0 && r.hits[k] > 0, "side {k}: {:?}", (r.walks, r.attacks, r.guards, r.hits));
    }
    assert_eq!(r.hashes, cpu_run(&a, [Some((1, 0)); 2], 2400).hashes, "the same match twice");
}

/// ULTRA HARD's top level beats a fighter who stands still, and attacks more often than EASY's first level (its
/// cooldowns average 63 frames against 350, FIGHT_DATA.md 21.5) and goes on into strings (P3A 1 at level 0).
/// Clean hits are no measure: an idle fighter turned round by a hit stays turned (no input), so whoever turns him
/// first hits his back for the rest of the round, and EASY's 221-frame rule (any entry after 220 frames in a move,
/// 0x385C80) keeps its attack rate far above what its cooldowns alone give.
#[test]
fn the_top_cpu_beats_a_standing_dummy() {
    let Some(a) = assets() else { return };
    let top = cpu_run(&a, [Some((4, 7)), None], 15000);
    let easy = cpu_run(&a, [Some((0, 0)), None], 15000);
    for (n, r) in [("top", &top), ("easy", &easy)] {
        eprintln!("{n}: presses {:?} starts {:?} hits {:?} wins {:?} fight {} turned {}", r.attacks, r.starts, r.hits, r.wins, r.fight, r.turned);
    }
    assert_eq!(top.wins[0], 2, "the top CPU wins the match");
    // (per fight step: the matches run different lengths)
    assert!(easy.starts[0] * top.fight < top.starts[0] * easy.fight, "EASY starts fewer attacks a step");
    assert!(easy.stats[0] == 0 && top.stats[0] > 0, "strings: EASY none, the top level {}", top.stats[0]);
}

/// In a tag battle the CPUs tag (the tag button of their cancel entries with command bit 14, 0x381278).
#[test]
fn cpus_tag_in_a_tag_battle() {
    let Some(a) = assets() else { return };
    let mut b = tag_bout(&a, LAW);
    b.set_cpu(0, Some((4, 7)));
    b.set_cpu(1, Some((4, 7)));
    let mut tags = 0;
    for _ in 0..9000 {
        b.step([Pad::default(); 2]);
        tags += b.game.fight.fighters.iter().filter(|f| f.input.tag).count();
    }
    assert!(tags > 0, "a tag press");
}

/// The string follow-ups (lists C and D, 0x387600 / 0x387DA8): never at NORMAL level 0 (P34 0, P3A 1), from level 4
/// on (P3A 5, P3C 2048).
#[test]
fn strings_go_on_from_normal_level_4() {
    let Some(a) = assets() else { return };
    let (l0, l4) = (cpu_run(&a, [Some((1, 0)), None], 4000), cpu_run(&a, [Some((1, 4)), None], 4000));
    eprintln!("NORMAL L0 {:?}, L4 {:?}", l0.stats, l4.stats);
    assert_eq!(l0.stats[0], 0, "level 0: no follow-up");
    assert!(l4.stats[0] > 0, "level 4: follow-ups");
}

/// King's handler (0x38BC00): his canned throws (P1A) against a fighter standing still. The delayed input (step 0x15)
/// holds back a buffered entry the neutral pick takes: at level 0 list D never inputs (P3A 1), so the neutral pick
/// goes on into the strings, and ULTRA HARD level 0 delays them (P74 2048), HARD level 0 never (P74 0). (From
/// level 4 list D, earlier in the chain, takes the string follow-ups first: delays are rare there.)
#[test]
fn king_throws_canned_and_delays_inputs() {
    let Some(a) = assets() else { return };
    let (normal, hard) = (cpu_run_of(&a, [KING, LEI], [Some((1, 7)), None], 9000, 2), cpu_run_of(&a, [KING, LEI], [Some((2, 0)), None], 15000, 2));
    // (rare: a few a match)
    let ultra = cpu_run_of(&a, [LEI, KING], [None, Some((4, 0))], 60000, 9).stats[2];
    eprintln!("King NORMAL L7 {:?}, ULTRA HARD L0 delays {ultra}, HARD L0 {:?}", normal.stats, hard.stats);
    assert!(normal.stats[1] > 0, "a canned throw");
    assert!(ultra > 0 && hard.stats[2] == 0, "delayed inputs with P74 only");
}

/// The distance the top CPU (Lei) keeps over 30 frames from Yoshimitsu starting `alias` `gap` away.
fn against_yoshimitsu(a: &Assets, alias: usize, gap: f32) -> f32 {
    let mut b = Bout::new(Game::load(a, [YOSHIMITSU, LEI]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    b.set_cpu(1, Some((4, 7)));
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let f = &mut b.game.fight.fighters;
    let along = (f[1].position - f[0].position).normalize();
    f[1].position = f[0].position + along * gap;
    assert!(f[0].play_alias(alias));
    for _ in 0..30 {
        b.step([Pad::default(); 2]);
    }
    let f = &b.game.fight.fighters;
    (f[1].root() - f[0].root()).length()
}

/// The CPU against Yoshimitsu (0x38B368, Tekken 3's handler with his aliases, pack format 20): from his sweep 0x1A1
/// it backdashes within 0xA8D at his zone 3 (without the handler it closes in to about 1700), on his stance 0x5C7
/// from afar it walks in.
#[test]
fn the_cpu_reads_yoshimitsu() {
    let Some(a) = assets() else { return };
    let (sweep, stance) = (against_yoshimitsu(&a, 0x1A1, 2400.0), against_yoshimitsu(&a, 0x5C7, 3500.0));
    eprintln!("after his sweep {sweep:.0}, his stance {stance:.0}");
    assert!(sweep > 2200.0, "kept off the sweep: {sweep:.0}");
    assert!(stance < 3200.0, "walked in on the stance: {stance:.0}");
}

/// Ogre's first model and his fire breath (0xBA9).
const OGRE: usize = 534;
const FIRE_BREATH: usize = 0xBA9;

/// The frames of 90 the top CPU (Lei), lying 1500 from Ogre, stays down while Ogre plays `alias`.
fn lying_against_ogre(a: &Assets, alias: usize) -> usize {
    let mut b = Bout::new(Game::load(a, [OGRE, LEI]).unwrap(), kumite_game::ai_data(a).unwrap(), 2, false);
    b.set_cpu(1, Some((4, 7)));
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let f = &mut b.game.fight.fighters;
    let along = (f[1].position - f[0].position).normalize();
    f[1].position = f[0].position + along * 1500.0;
    assert!(f[0].play_alias(alias) && f[1].play_alias(214));
    (0..90)
        .filter(|_| {
            b.step([Pad::default(); 2]);
            b.game.fight.fighters[1].posture(&b.game.fight.data) & kumite_sim::posture::LYING != 0
        })
        .count()
}

/// The wake-up waits out Ogre's fire breath within 2000 (0x3890D0, pack format 26), not his other moves.
#[test]
fn the_cpu_stays_down_under_the_fire_breath() {
    let Some(a) = assets() else { return };
    let (breath, other) = (lying_against_ogre(&a, FIRE_BREATH), lying_against_ogre(&a, 422));
    eprintln!("frames down under the breath {breath}, under 422 {other}");
    assert!(breath >= 60 && other < 30, "waits under the breath only");
}

/// The arcade CPU, team `cpu` at NORMAL `level`, against Xiaoyu and Yoshimitsu over the seeds' matches: (damage a
/// fight minute, the fight steps of each match); side 0 stands (`hold`: off) or holds back. The match's seed starts
/// each CPU on its own draws.
fn arcade_cell(a: &Assets, cpu: [usize; 2], level: usize, hold: bool, seeds: impl Iterator<Item = u32>) -> (u32, Vec<Vec<u64>>) {
    let (mut damage, mut steps, mut hashes) = (0, 0, Vec::new());
    for seed in seeds {
        let mut g = Game::load(a, [XIAOYU, cpu[0]]).unwrap();
        g.start_tag([kumite_game::Actor::load(a, YOSHIMITSU).unwrap(), kumite_game::Actor::load(a, cpu[1]).unwrap()]);
        g.copy_seed = seed;
        let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
        b.set_cpu(1, Some((1, level)));
        let mut h = Vec::new();
        for _ in 0..4600 {
            let back = b.game.fight.facing_right(0);
            b.step([Pad { left: hold && back, right: hold && !back, ..Pad::default() }, Pad::default()]);
            h.push(b.game.state_hash());
            if b.phase == Phase::Fight {
                steps += 1;
                damage += b.game.fight.hits.iter().filter(|h| h.attacker == 1 && !h.blocked).map(|h| h.damage).sum::<i32>() as u32;
            }
        }
        hashes.push(h);
    }
    (damage * 3600 / steps.max(1), hashes)
}

/// The arcade's CPU against a fighter who stands still or holds back, damage a fight minute as measured on the PS2
/// (PCSX2, the default NORMAL, single matches against Xiaoyu: stage 1 level 0 about 200 still, 30-40 holding back;
/// stage 4 level 3 200-240 / 140-160; stage 8 level 7 (Unknown) 240 / 140), here a cell's eight matches each (stage
/// 8 by Julia and Armor King); a seed replays the same.
#[test]
fn the_arcade_cpus_beat_a_still_fighter_as_the_ps2_does() {
    let Some(a) = assets() else { return };
    let seeds = || (0..8).map(|k| k * 11);
    let cells = [([LEI, NINA], 0), ([MICHELLE, KING], 3), ([JULIA, ARMOR_KING], 7)].map(|(cpu, level)| {
        let ((still, runs), (back, _)) = (arcade_cell(&a, cpu, level, false, seeds()), arcade_cell(&a, cpu, level, true, seeds()));
        (still, back, runs)
    });
    let dmg: Vec<(u32, u32)> = cells.iter().map(|c| (c.0, c.1)).collect();
    eprintln!("damage a minute, still / holding back: {dmg:?}");
    for (k, (still, back)) in [((140, 260), (40, 120)), ((180, 300), (100, 200)), ((150, 300), (100, 200))].into_iter().enumerate() {
        assert!((still.0..=still.1).contains(&dmg[k].0) && (back.0..=back.1).contains(&dmg[k].1), "cell {k}: {:?}", dmg[k]);
        assert!(dmg[k].1 < dmg[k].0, "holding back takes less");
    }
    let runs = &cells[0].2;
    assert!(runs.windows(2).any(|w| w[0] != w[1]), "the matches differ");
    assert_eq!(runs[3], arcade_cell(&a, [LEI, NINA], 0, false, [33].into_iter()).1[0], "a seed replays the same");
}

/// Each frame of an ending: the scene frame, the camera, each slot's cast member and root, the fades.
fn play_ending(a: &Assets, d: &kumite_game::ending::EndingData, e: usize) -> Vec<String> {
    let mut s = kumite_game::ending::EndingScene::new(a, d, e, 0).unwrap();
    let mut out = Vec::new();
    while !s.done && out.len() < 8000 {
        s.step();
        let slots: Vec<_> = (0..2).map(|k| s.pose(k).map(|(m, pose, _)| (m, pose[0].w_axis))).collect();
        out.push(format!("{} {:?} {slots:?} {:?}", s.frame(), s.camera(), s.fades()));
    }
    out
}

#[test]
fn an_ending_plays_to_its_end() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    // King's ending: King, then Armor King and Mokujin swapped into slot 1; 1800 frames, op -1 on the last
    assert_eq!(d.ending(3, 0), Some(4));
    let r = play_ending(&a, &d, 4);
    assert_eq!(r.len(), 1800);
    assert!(r[0].ends_with("(255, 0)") && r[100].ends_with("(0, 0)"), "black, then faded in: {} / {}", r[0], r[100]);
    assert!(r[30].contains("[Some((0,") && r[30].contains("Some((1,") && r[100].contains("Some((2,"), "the cast swaps: {}", r[100]);
    assert_eq!(r, play_ending(&a, &d, 4), "the same frames");
    // the costume picks Xiaoyu's; a seen ending gives the partner's when it is new
    assert_eq!((d.ending(7, 0), d.ending(7, 4), d.ending(33, 0)), (Some(9), Some(10), Some(37)));
    assert_eq!(d.pick(&[(7, 4), (0, 2)], 0), Some((10, 4)));
    assert_eq!(d.pick(&[(7, 4), (0, 2)], 1 << 10), Some((1, 2)));
    assert_eq!(d.pick(&[(7, 4), (0, 2)], 1 << 10 | 1 << 1), Some((10, 4)));
    assert_eq!(d.pick(&[(7, 0)], 1 << 9), Some((9, 0)));
}

/// Tekken Bowl from the pack: Paul's throw (its hand's release point from his motion, ahead of him, above the lane,
/// on his right), the ball rolled down the middle and counted, Paul and Lei taking turns; two runs give the same
/// states.
#[test]
fn tekken_bowl_from_the_pack() {
    use kumite_game::bowl::BowlScene;
    use kumite_sim::bowl::{Phase as Bowl, CROSS};
    let Some(a) = assets() else { return };
    let play = || {
        let mut s = BowlScene::new(&a, &[(PAUL, 0), (LEI, 2)], &[], 7).unwrap();
        let hand = s.bowl.players[0].hand;
        assert!(hand.y < -150.0 && hand.z > 3000.0 && hand.x > 0.0, "Paul's release point {hand}");
        assert_eq!(s.bowl.players[0].charge.len(), 175);
        let mut hashes = vec![];
        for f in 0..2000u32 {
            let b = &s.bowl;
            let press = |go: bool| if go && f % 2 == 0 { CROSS } else { 0 };
            // the ball down the middle: the bowler steps left until the hand is over the centre
            let centre = -b.players[b.current].hand.x;
            let pad = match b.phase {
                Bowl::Position => Pad { left: b.x > centre, buttons: press(b.x <= centre), ..Pad::default() },
                Bowl::Aim => Pad { buttons: press(true), ..Pad::default() },
                Bowl::Power => Pad { buttons: press(b.power >= 90.0), ..Pad::default() },
                _ => Pad::default(),
            };
            s.step(pad);
            assert!(s.pose().is_some());
            hashes.push(s.bowl.hash());
        }
        (hashes, s.bowl.cards.clone())
    };
    let (hashes, cards) = play();
    // both bowled: a frame each at least, most pins down on Paul's first ball from the middle
    assert!(cards.iter().all(|c| !c.rolls.is_empty()), "{cards:?}");
    assert!(cards[0].rolls[0] >= 5, "{cards:?}");
    assert_eq!(hashes, play().0);
}
