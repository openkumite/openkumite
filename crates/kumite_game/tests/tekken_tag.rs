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

/// Inside its END window a move takes the stance's commands without a new press (0x368F78): Xiaoyu's 2+3+4 pose
/// (alias 116, END window from frame 1) walks back on a held back at once, as in PCSX2, not at its last frame.
#[test]
fn a_move_takes_a_held_direction_in_its_end_window() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [XIAOYU, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut seen = Vec::new();
    for pad in parse_script("n*2,2+3+4*2,n*10,b*4").unwrap() {
        b.step([pad, Pad::default()]);
        seen.push(b.game.fight.fighters[0].alias);
    }
    assert_eq!(seen[3], 116, "{seen:?}");
    assert_eq!(&seen[14..], [65; 4], "{seen:?}");
}

/// A jump goes toward the opponent (0x36FC04): Paul's u+2 (alias 4712, transition 0x10) covers the start distance
/// less 500 over its jump window, about 1030 by its 30th frame as in PCSX2; its motion's root does not move.
#[test]
fn a_jump_goes_toward_the_opponent() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let x0 = b.game.fight.fighters[0].root().x;
    for pad in parse_script("u+2*2,n*29").unwrap() {
        b.step([pad, Pad::default()]);
    }
    let f = &b.game.fight.fighters[0];
    let went = f.root().x - x0;
    assert_eq!(f.alias, 4712);
    assert!((950.0..1100.0).contains(&went), "{went}");
    // against Hwoarang (audit 0_cmd02, the PS2's two frames of pad latency): the last frame's pair distance, 1976
    // less 500 over 29 frames, 50, times the sine 0x7FF8 over 0x7FFF: 49 a frame, -1000 to 29 as in PCSX2
    let mut b = Bout::new(Game::load(&a, [PAUL, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    for pad in parse_script("n*4,u+2*2,n*28").unwrap() {
        b.step([pad, Pad::default()]);
    }
    assert_eq!(b.game.fight.fighters[0].root().x, 29.0);
}

/// A sidestep circles the opponent (0x370698): it leans out of the step and its origin follows the turning basis
/// (+0x126), so Julia's u (alias 332, after her u/f and u/b) ends at (805, 722) against Hwoarang at (1007, -95) as in
/// PCSX2, still facing him (her b then walks back, alias 65); the basis swung round the start took her past him.
#[test]
fn a_sidestep_circles_the_opponent() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [JULIA, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut back = false;
    for pad in parse_script("n*2,u/f*2,n*80,u/b*2,n*80,u*2,n*80,b*2,n*10").unwrap() {
        b.step([pad, Pad::default()]);
        back |= b.game.fight.fighters[0].alias == 65;
    }
    let p = b.game.fight.fighters[0].root();
    assert!((p.x - 805.0).abs() < 80.0 && (-p.z - 722.0).abs() < 80.0 && back, "{p}, walked back {back}");
}

/// A ground roll's basis turns toward where the opponent was at its start (+0x110 from the other's root, 0x36A814):
/// King rolls sideways off his d/f+1+4 (PCSX2: x +309, z +1105 over alias 219), not round in a circle.
#[test]
fn a_ground_roll_goes_sideways() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [KING, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let (mut start, mut back) = (None, 0.0f32);
    for pad in parse_script("d/f+1+4*2,n*80,1+2*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.alias == 219 {
            let s = *start.get_or_insert(f.root());
            // (Lei is on +x: a circle swings back past the start)
            back = back.min(f.root().x - s.x);
        }
    }
    let d = b.game.fight.fighters[0].root() - start.expect("the roll");
    // (the basis turning rebases the origin, 0x37068C: PCSX2's x +311 against z +1105)
    assert!(d.z.abs() > 800.0 && back > -100.0 && d.x.abs() < 500.0, "{d}, back {back}");
}

/// The back-turned commands as in PCSX2: Xiaoyu's f+3+4 ends turned away (its +0x12 turn, 30°, added the game's way
/// round, 0x36AA48), so b+1+2 is the back-turned one (alias 381); Hwoarang's n+1+2 turns him exactly half round,
/// which the game's s16 +0x3E reads as -0x8000: back turned right (0x29), so f+1 is alias 374.
#[test]
fn back_turned_commands_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let run = |model: usize, script: &str| {
        let mut b = Bout::new(Game::load(&a, [model, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
        while b.phase != Phase::Fight {
            b.step([Pad::default(); 2]);
        }
        let mut seen = Vec::new();
        for pad in parse_script(script).unwrap() {
            b.step([pad, Pad::default()]);
            seen.push(b.game.fight.fighters[0].alias);
        }
        seen
    };
    assert!(run(XIAOYU, "n*2,f+3+4*2,n*80,b+1+2*2,n*10").contains(&381));
    assert!(run(HWOARANG, "n*2,n+1+2*2,n*80,n+3+4*2,n*80,f+1*2,n*10").contains(&374));
}

/// A motion input whose last step is a direction alone is looked for back over its window (0x36CDD8): Gun Jack's
/// n, b, d/b, d, d/f (alias 2906) comes out once he is back in his stance, as in PCSX2.
#[test]
fn a_motion_input_ending_in_a_direction_waits_for_the_stance() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [GUN_JACK, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut seen = Vec::new();
    for pad in parse_script("n*2,b*2,d/b*2,d*2,d/f*2,n*20").unwrap() {
        b.step([pad, Pad::default()]);
        seen.push(b.game.fight.fighters[0].alias);
    }
    assert!(seen.contains(&2906), "{seen:?}");
}

/// A jump's drift is +0x16 / 256 a frame (0x36FDE0, the sine 0x7FFF to one): True Ogre's u/f+1 (+0x16 -9848)
/// travels about 38 a frame from its jump window, as in PCSX2 (461 by frame 22).
#[test]
fn a_jumps_drift_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [TRUE_OGRE, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let x0 = b.game.fight.fighters[0].root().x;
    for pad in parse_script("u/f+1*2,n*18").unwrap() {
        b.step([pad, Pad::default()]);
    }
    let went = b.game.fight.fighters[0].root().x - x0;
    assert!((380.0..540.0).contains(&went), "{went}");
}

/// A jump drifts along its basis +0x1A, not the turning facing +0x3C (0x36FDE0): Yoshimitsu's u/f+4 from a walk,
/// blocked by Hwoarang (both holding 1+2 through the intro, as the recording), has its root at z -123 on its frame
/// 25 as in PCSX2 (along the facing: -101).
#[test]
fn a_jump_drifts_along_its_basis() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [YOSHIMITSU, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut z = None;
    for pad in parse_script("n*62,f*30,n*2,u/f+4*2,n*30").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.alias == 4692 && f.frame == 25 {
            z = Some(-f.root().z);
        }
    }
    assert!(z.is_some_and(|z| (-127.0..=-119.0).contains(&z)), "{z:?}");
}

/// A jump's drift is carried in the game's z at its angle +0x1A >> 4 (0x36FDE0): Wang's u/f+4 after a walk has his
/// basis at 16368, a hair off a quarter turn, so the first drift frame's -14/256 across floors to a whole unit and his
/// root goes from z -3 to -2 on frame 8, as in PCSX2 (carried in OpenKumite's z: it stays at -3).
#[test]
fn a_jump_drift_floors_in_the_games_z() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [624, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut z = None;
    for pad in parse_script("n*62,f*30,n*2,u/f+4*2,n*10").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.alias == 4692 && f.frame == 8 {
            z = Some(-f.root().z);
        }
    }
    assert_eq!(z, Some(-2.0));
}

/// The hit pass strikes in the pose made before the push moves the bodies (0x34F3D8, then 0x371C68, 0x371D68): Jun's
/// b+1 from a walk reaches Hwoarang (both holding 1+2 through the intro) on its first active frame, so on frame 29 she
/// is at x 656 as in PCSX2 (in the pushed pose: a frame later, 616).
#[test]
fn a_strike_lands_in_the_pose_before_the_push() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [672, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut x = None;
    for pad in parse_script("n*62,f*30,n*2,b+1*2,n*30").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.alias == 356 && f.frame == 29 {
            x = Some(f.root().x);
        }
    }
    assert!(x.is_some_and(|x| (652.0..=660.0).contains(&x)), "{x:?}");
}

/// The push's spheres are placed by the facing the game builds from its sines at the s16 angle over 16 (0x398690),
/// and the push turns its axes by that angle cut toward zero (0x377338): a facing a hair off a quarter turn (16383)
/// keeps its 6 across. Ogre's d+4 after a dash at Hwoarang (both holding 1+2 through the intro) pushes him on the
/// move's first frame to x 1053 as in PCSX2 (rounded off the table: no push there, 1037).
#[test]
fn the_push_spheres_turn_with_the_games_sines() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [528, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut x = None;
    for pad in parse_script("n*61,f*30,n*2,d+4*2,n*30").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[0].alias == 410 && f[0].frame == 1 {
            x = Some((f[0].root().x, f[1].root().x));
        }
    }
    assert!(x.is_some_and(|(p, q)| (113.0..=117.0).contains(&p) && (1051.0..=1055.0).contains(&q)), "{x:?}");
}

/// The tracking turns in the game's s16 angles (0x370094, 0x3703F0: the angle over the frames left, cut toward zero):
/// King's 1 after a walk at Hwoarang (both holding 1+2 through the intro) faces +0x3C 17423 through frames 16-22 of
/// its follow-up 809 and Paul's reaction 4565 turns 49711, 49823 on its frames 6 and 7, as in PCSX2 (in radians:
/// 17424-17426, 49714, 49825), so the joint slots and the push spheres on them are the game's to the unit.
#[test]
fn the_tracking_turns_in_the_games_angles() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [384, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let u = |y: f32| ((std::f32::consts::PI - y) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF;
    let (mut king, mut paul) = (Vec::new(), Vec::new());
    for pad in parse_script("n*61,f*30,n*2,1*2,n*70").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[0].current.index == 809 && (16..=22).contains(&f[0].frame) {
            king.push(u(f[0].yaw));
        }
        if f[1].current.index == 4565 && (6..=7).contains(&f[1].frame) {
            paul.push(u(f[1].yaw));
        }
    }
    assert_eq!((king, paul), (vec![17423; 7], vec![49711, 49823]));
}

/// A move's auto-turn steps in the game's s16 angles (+0x15E, 0x36AE60: the angle left over the frames, cut toward
/// zero), and a reaction's motion ends the ease toward the next move through its hold (+0x134, kept until the frame
/// moves on, 0x36A500): Paul's sidestep down and 2 at Hwoarang (both holding 1+2 through the intro) push him to x
/// 1046 on its frame 10, and his reaction 4565 slides him to (1492, 580) by its frame 5, as in PCSX2 (a float turn:
/// 1045; the ease kept: 1493, 581).
#[test]
fn a_sidestep_turns_and_a_reaction_eases_as_the_game() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut rows = Vec::new();
    for pad in parse_script("n*61,f*30,n*2,d*1,n*40,2*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        let (m0, m1) = (f[0].shown_move(), f[1].shown_move());
        if (m0.0.index, m0.1) == (534, 10) || (m1.0.index, m1.1) == (4565, 5) {
            rows.push((f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(1046, 41), (1492, 580)]);
}

/// The facing pass (0x36FEF0) turns a fighter in its hit-stop frame too: Hwoarang, struck by Paul's 1 after a walk,
/// starts turning to the attacker on the hit's frame (+0x3C -16384 -> -16350 in PCSX2), so the next push meets his
/// body turned and his slide ends at x 1739, as in PCSX2 (not turning there: 1738).
#[test]
fn the_struck_turn_in_the_hit_stop() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    for pad in parse_script("n*62,f*30,n*2,1*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
    }
    let x = b.game.fight.fighters[1].root().x;
    assert_eq!(x, 1739.0);
}

/// A hit's reaction is queued (0x36A2B4) and committed by the next frame's moves, which ease its first pose
/// (0x34E748 -> 0x373428): the struck one shows its old move on the hit's frame, the reaction's frame 1 once, and the
/// push of that frame meets the reaction's pose. Kuma's d+3 on Hwoarang's walk back, as in PCSX2.
#[test]
fn a_reaction_starts_the_frame_after_the_hit() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [492, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut rows = Vec::new();
    for pad in parse_script("n*62,f*30,n*2,d*20,d+3*2,d*6,n*20").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[0].current.index == 1138 && (20..=22).contains(&f[0].frame) {
            let (m, k) = f[1].shown_move();
            rows.push((f[0].root().x as i32, m.index, k, f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(67, 17, 35, 1255, 0), (-152, 5188, 1, 1407, 383), (-156, 5188, 2, 1522, 563)]);
}

/// A reversal starts the frame after the reversed attack's hit, as a reaction does: Law's b+1+3 on Hwoarang's 2, the
/// hit's frame and the reversal's frame 1 (the pair linked at the thrower's frame-1 root); the victim's move eases in
/// from the link's frame (0x373428), so the pair pushes in the poses PCSX2 shows on frames 1 and 5 of its END.
#[test]
fn a_reversal_starts_the_frame_after_the_hit() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [LAW, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let (p1, p2) = (parse_script("n*61,f*30,n*2,b+1+3*2,n*24").unwrap(), parse_script("n*91,2*2,n*22").unwrap());
    let mut rows = Vec::new();
    for (k, pad) in p1.into_iter().enumerate() {
        b.step([pad, p2.get(k).copied().unwrap_or_default()]);
        let f = &b.game.fight.fighters;
        let ((m0, k0), (m1, k1)) = (f[0].shown_move(), f[1].shown_move());
        if (m1.index, k1) == (852, 10) || (m0.index, k0) == (4790, 1) || m0.index == 4792 && matches!(k0, 1 | 5) {
            rows.push((m0.index, k0, f[0].root().x as i32, -f[0].root().z as i32, m1.index, k1, f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(4769, 8, -65, -159, 852, 10, 927, -16), (4790, 1, -251, -187, 4791, 1, 928, -26), (4792, 1, -183, -156, 4793, 1, 961, -102), (4792, 5, -231, -142, 4793, 5, 603, -475)]);
}

/// A chicken (Hwoarang's b+2+4 on Paul's reversal): the reverser turns round and the breaker faces its new way,
/// 752 straight ahead of its root as drawn last frame; the roots on frames 1 and 3, as in PCSX2.
#[test]
fn a_chicken_turns_the_reverser_round() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let (p1, p2) = (parse_script("n*61,f*30,n*1,b+1+3*2,b*1,n*40").unwrap(), parse_script("n*91,2*2,n*11,b+2+4*2,b*1,n*20").unwrap());
    let mut rows = Vec::new();
    for (k, pad) in p1.into_iter().enumerate() {
        b.step([pad, p2.get(k).copied().unwrap_or_default()]);
        let f = &b.game.fight.fighters;
        let ((m0, k0), (m1, k1)) = (f[0].shown_move(), f[1].shown_move());
        if (m0.index, m1.index) == (6088, 6087) && matches!(k0, 1 | 3) && k0 == k1 {
            rows.push((k0, f[0].root().x as i32, -f[0].root().z as i32, f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(1, -160, -325, 592, -330), (3, -149, -325, 592, -325)]);
}

/// A chicken's reverser plays its move at the breaker's motion scale, as any fighter whose chain stage +0xE0 is below
/// 0 (0x363A80): Nina's reversal (4300) chickened by Hwoarang (4096), frames 3 and 24, as in PCSX2.
#[test]
fn a_chickened_reverser_takes_the_breakers_scale() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [NINA, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let (p1, p2) = (parse_script("n*62,f*30,n*2,b+1+3*2,n*40").unwrap(), parse_script("n*92,2*2,n*11,b+2+4*2,n*20").unwrap());
    let mut rows = Vec::new();
    for (k, pad) in p1.into_iter().enumerate() {
        b.step([pad, p2.get(k).copied().unwrap_or_default()]);
        let f = &b.game.fight.fighters;
        let (m, k0) = f[0].shown_move();
        if m.index == 6088 && matches!(k0, 3 | 24) {
            rows.push((k0, f[0].root().x as i32, -f[0].root().z as i32, f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(3, 111, 500, 853, 500), (24, 529, 498, 1438, 660)]);
}

/// A chain throw's next part (0x36B638 on a linked thrower) keeps the pair's origin: the old part's commit bit is not
/// read there. Nina's 2+4 chain, frame 1 of its second and third parts, as in PCSX2.
#[test]
fn a_chain_throw_part_keeps_the_origin() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [NINA, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut rows = Vec::new();
    for pad in parse_script("n*61,f*60,n*2,2+4*2,n*14,1*2,n*2,2*2,n*2,1*2,n*24,2*2,n*2,1*2,n*2,2*2,n*2,1*2,n*2,3*2,n*90").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        let (m, k) = f[0].shown_move();
        if matches!(m.index, 5758 | 5760) && k == 1 {
            rows.push((m.index, f[0].root().x as i32, -f[0].root().z as i32, f[1].root().x as i32, -f[1].root().z as i32));
        }
    }
    assert_eq!(rows, [(5758, 1433, 0, 1740, 0), (5760, 474, -177, 73, 0)]);
}

/// A throw's link (0x36B638): the thrower goes onto the victim's root as drawn last frame (+0x51C), faces the last
/// facing pass's angle +0x3A by the quadrant, and turns over 16 frames to the victim's facing, which turns round
/// (0x36B77C); the victim's root takes the thrower's scale (0x363A80). Kunimitsu's 1+3 on Hwoarang after a long walk,
/// frames 1 and 5 of the throw, as in PCSX2; Law's off the pair's line too.
#[test]
fn a_throw_links_as_the_game() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [654, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let mut seen = Vec::new();
    for pad in parse_script("n*61,f*60,n*2,1+3*2,n*30").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[0].alias == 3812 && matches!(f[0].frame, 1 | 5) {
            seen.push((f[0].root().x, -f[0].root().z, f[1].root().x));
        }
    }
    assert_eq!(seen, [(492.0, 107.0, 1682.0), (657.0, 72.0, 1732.0)]);
    assert_eq!(play_tag([LAW, 426], "n*61,f*60,n*2,1+3*2,n*30", 3812, &[1, 5]), [(373.0, 54.0), (529.0, 57.0)]);
}

/// Fighter 0's root (x, the game's z) on the given frames of alias `alias`, from the fight's start (both holding 1+2
/// through the intro, partners Paul), P1 playing `script`.
/// A link's transition 2 becomes 0x14 (tracking mode 4) only if the mark the last queue left (+0x178) is before the new
/// move's strike (0x36BE08, called at 0x368CB0 before +0x178 takes the new exec). Jun's 10-hit: its 3rd hit (alias 2547)
/// comes from a link whose mark was 36, so it stays 2 (mode 0xB) and Jun does not turn; frames 30 and 60 as in PCSX2.
#[test]
fn a_link_takes_the_mark_the_last_queue_left() {
    let script = "n*61,f*30,n*2,1*2,n*8,1*2,n*8,2*2,n*8,4*2,n*8,3*2,n*8,4*2,n*8,2*2,n*8,1*2,n*8,2*2,n*8,1*2,n*97";
    if assets().is_some() {
        assert_eq!(play_tag([672, 426], script, 2547, &[30, 60]), [(2196.0, 129.0), (2581.0, 214.0)]);
    }
}

/// King's chain throw (b+1+2, then 1+3): its next part's victim (alias 4181) keeps its own origin and facing 16384
/// (0x36B828), not the thrower's 16399; frames 80, 81 as in PCSX2 (81 was (3618, 443): the last key's heading is
/// stored whole).
#[test]
fn a_chain_throw_victim_keeps_its_facing() {
    let script = "n*61,f*60,n*2,b+1+2*1,1+2*1,n*16,1+3*2,n*199";
    if assets().is_some() {
        assert_eq!(play_pair([384, 426], [script, ""], 1, 4181, &[80, 81]), [(3615.0, 445.0), (3617.0, 446.0)]);
    }
}

/// Paul's reversal of Hwoarang's 2: Hwoarang's +0x3A is Paul's less 0x8000 (0x371330 runs on player 1), 49152
/// straight down the x axis (its own atan2 gives 49151), so as the victim (alias 4621) he faces 16384; PCSX2's roots.
#[test]
fn player_2_sees_player_1s_angle_turned_round() {
    let p1 = "n*62,f*30,n*2,b+1+3*2,n*121";
    if assets().is_some() {
        assert_eq!(play_pair([PAUL, 426], [p1, "n*92,2*2,n*123"], 1, 4621, &[30, 50]), [(324.0, 643.0), (382.0, 1665.0)]);
    }
}

fn play_tag(m: [usize; 2], script: &str, alias: usize, frames: &[u32]) -> Vec<(f32, f32)> {
    play_pair(m, [script, ""], 0, alias, frames)
}

/// Fighter `who`'s root (x, the game's z) on the given frames of alias `alias`, each player playing its script.
fn play_pair(m: [usize; 2], scripts: [&str; 2], who: usize, alias: usize, frames: &[u32]) -> Vec<(f32, f32)> {
    let Some(a) = assets() else { return Vec::new() };
    let mut g = Game::load(&a, m).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut seen = Vec::new();
    let [p1, p2] = scripts.map(|s| parse_script(s).unwrap());
    for (k, pad) in p1.into_iter().enumerate() {
        b.step([pad, p2.get(k).copied().unwrap_or_default()]);
        let f = &b.game.fight.fighters[who];
        if f.alias == alias && frames.contains(&f.frame) && seen.len() < frames.len() {
            seen.push((f.root().x, -f.root().z));
        }
    }
    seen
}

/// Full health keeps the game's 20.12 fraction (0x3C1350): Hwoarang's 108 % of 140 is 151.2 (PCSX2's +0x3F4: 619315),
/// so 151 points of damage leave him standing.
#[test]
fn full_health_keeps_its_fraction() {
    let Some(a) = assets() else { return };
    let b = Bout::new(Game::load(&a, [PAUL, 426]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    let f = &b.game.fight.fighters[1];
    assert_eq!((f.max_health, f.health), (619315 << 4, 619315 << 4));
    assert!(f.health - 151 * kumite_sim::ONE > 0);
}

/// The tag (Kazuya out, Paul in) as in PCSX2: the run-off turns each frame toward the screen axis by the camera
/// (0x370730, +0x50); the partner's entrance starts from the origin the leaving fighter's committed, facing the
/// opponent by the game's angle (0x36BAE0, 0x371330).
#[test]
fn the_tag_runs_off_and_comes_in_as_the_game() {
    if assets().is_none() {
        return;
    }
    let script = "n*62,f*30,n*2,5*2,n*60";
    assert_eq!(play_tag([585, 426], script, 5394, &[2, 4]), [(14.0, -10.0), (-32.0, -24.0)]);
    assert_eq!(play_tag([585, 426], script, 5397, &[1, 2, 20]), [(-2213.0, -14.0), (-2145.0, -14.0), (-329.0, 9.0)]);
}

/// The camera chases Eddy's run-off (4829) with the game's shifts down (0x38FAA8-0x38FC2C, not Tekken 3's toward
/// zero): PCSX2's view yaw (0x41580C) on the run-off's frames 15-18, 20, 21: -284, -292, -299, -306, -320, -326
/// (toward zero: -291 at 16, -325 at 21).
#[test]
fn the_camera_chases_a_run_off_as_the_game() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [450, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut seen = Vec::new();
    for pad in parse_script("n*61,f*30,n*2,5*2,n*60").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 4829 && [15, 16, 17, 18, 20, 21].contains(&f.frame) {
            seen.push(b.game.camera.view.yaw);
        }
    }
    assert_eq!(seen, [-284, -292, -299, -306, -320, -326]);
}

/// After Wang's 2+4 throw the body push measures each one's step from where the pair left the throw (0x3772F0 keeps
/// the roots only while no one is in a throw), not from before it: PCSX2's roots on his stance's first frame, x 1556
/// and 3109 (from before the throw: 716 and 3503).
#[test]
fn the_push_after_a_throw_steps_from_its_end() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [624, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut thrown = false;
    for pad in parse_script("n*61,f*60,n*2,2+4*2,n*119").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        thrown |= f[0].current.index == 5803;
        if thrown && f[0].current.index == 30 {
            assert_eq!((f[0].root().x, f[1].root().x), (1556.0, 3109.0));
            return;
        }
    }
    panic!("no stance after the throw");
}

/// The tracking modes read the jump window (+0x135) as last frame's facing pass left it (0x370858): Heihachi's u/f+4
/// (696, jump frames 9-33), blocked, re-aims once more on frame 34 before mode 6 stops turning him on 35. PCSX2's
/// +0x3C on frames 34-36: 16944, 17064, 17064 (this frame's window: 16944 throughout).
#[test]
fn the_tracking_reads_last_frames_jump_window() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [516, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let u = |y: f32| ((std::f32::consts::PI - y) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF;
    let mut seen = Vec::new();
    for pad in parse_script("n*60,f*31,n*2,u/f+4*2,n*69").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 696 && (34..=36).contains(&f.frame) {
            seen.push(u(f.yaw));
        }
    }
    assert_eq!(seen, [16944, 17064, 17064]);
}

/// An END link takes the facing into the basis even when the move left has no turn (+0x12 0, 0x36AA48): Eddy rising
/// from d+3 (1141) into 2816 lays its root motion along +0x1A 20459, his facing then, and walks on to PCSX2's
/// (149, 690) at 2789's frame 10 (with the old basis 23903 he went ~19 degrees off).
#[test]
fn an_end_link_with_no_turn_takes_the_facing_as_basis() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [450, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let u = |y: f32| ((std::f32::consts::PI - y) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF;
    let mut seen = Vec::new();
    for pad in parse_script("n*61,f*30,n*2,d*20,d+3*2,d*6,n*59").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 2816 && f.frame == 1 {
            seen.push((u(f.root_yaw), 0));
        }
        if f.current.index == 2789 && f.frame == 10 {
            seen.push((f.root().x as i32, -f.root().z as i32));
        }
    }
    assert_eq!(seen, [(20459, 0), (149, 690)]);
}

/// A tag throw's swap (6403): the incoming partner takes the leaving one's last facing angle +0x3A as facing and basis,
/// as in PCSX2 (+0x1A 49615 on 6403's frame 2, the partner off at z 1086), not the angle from its origin (49631).
#[test]
fn the_tag_throws_partner_comes_in_on_the_leaving_ones_angle() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [585, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let u = |y: f32| ((std::f32::consts::PI - y) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF;
    let mut seen = None;
    for pad in parse_script("n*61,f*60,n*2,2+5*2,n*80").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 6403 && f.frame == 2 {
            seen = Some((u(f.root_yaw), -f.root().z as i32));
        }
    }
    assert_eq!(seen, Some((49615, 1086)));
}

/// The pose ease holds while a fighter struck in its move (+0xF3, kept by a move started on the frame after the
/// strike) is in the move's jump window (+0x135): Hwoarang, thrown by Kazuya's tag throw, rolls off in 5286 (window
/// 1-20) with no ease toward the next move until frame 21 (PCSX2's +0xB5DC 3 from frame 21, 10 frames), and the
/// pair's push then splits as there (P2 x 299 on Paul's 6431 frame 55, 301 with the ease from frame 15).
#[test]
fn a_struck_fighter_holds_the_ease_through_its_jump_window() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [585, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let (mut eased, mut x) = (Vec::new(), None);
    for pad in parse_script("n*61,f*60,n*2,2+5*2,n*130").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[1].current.index == 5286 && (19..=22).contains(&f[1].frame) {
            eased.push(f[1].eased().is_some());
        }
        if f[0].current.index == 6431 && f[0].frame == 55 {
            x = Some(f[1].root().x as i32);
        }
    }
    assert_eq!((eased, x), (vec![false, false, true, true], Some(299)));
}

/// A hit reaction's commit always marks a new motion in Tekken Tag (+0xF8 1 -> +0x134, 0x36A9A0), so a re-hit into the
/// same reaction ends the ease toward the next move: Hwoarang, struck again in 4565 by Kazuya's 1523, keeps no ease
/// (PCSX2's +0xB5DC 0) and the push of 1523's frame 6 moves him to x 2494, as there (2426 with the ease running on).
#[test]
fn a_rehit_into_the_same_reaction_ends_its_ease() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [585, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut x = None;
    for pad in parse_script("n*61,f*30,n*2,1*2,n*7,1*2,n*9,2*2,n*8,4*2,n*30").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if f[0].current.index == 1523 && f[0].frame == 6 {
            x = Some(f[1].root().x as i32);
        }
    }
    assert_eq!(x, Some(2494));
}

/// The incoming partner keeps the entrance's tracking mode (+0x11E 8): Paul, tagged in for Wang, turns his body +0x3C
/// toward Hwoarang each frame of his entrance (4936) while the basis +0x1A stays, as in PCSX2: +0x3C 16384, 16399,
/// 16543 on its frames 1, 12, 25, +0x1A 16384 throughout.
#[test]
fn the_partner_comes_in_turning_to_the_opponent() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [624, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let u = |y: f32| ((std::f32::consts::PI - y) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF;
    let mut seen = Vec::new();
    for pad in parse_script("n*61,f*30,n*2,5*2,n*100").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 4936 && [1, 12, 25].contains(&f.frame) {
            seen.push((u(f.yaw), u(f.root_yaw)));
        }
    }
    assert_eq!(seen, [(16384, 16384), (16399, 16384), (16543, 16384)]);
}

/// Yoshimitsu's 1+3 from Hwoarang's side (sidestep, then the grab: move 5903, camera id 0xAE): while he spins, his
/// root swings ~450 units each frame, and the sampler takes his side on its odd frames only (CAM+0xC0 = 0 in
/// PCSX2), so the eye glides. PCSX2 (0x41581C) on the throw's frames 20, 44 and 65: -1934, -1528, -1276 (its row
/// labelled 21 holds frame 20's eye: the log's next rows -1921, -1920, -1905 are ours on frames 21-23).
#[test]
fn yoshimitsus_side_throw_camera_samples_on_odd_frames() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [YOSHIMITSU, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut seen = Vec::new();
    for pad in parse_script("n*61,f*30,n*2,u*1,n*11,1+3*2,n*79").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if f.current.index == 5903 && [20, 44, 65].contains(&f.frame) {
            seen.push(b.game.camera.view.eye[0]);
        }
    }
    assert_eq!(seen.len(), 3, "{seen:?}");
    for (x, ps2) in seen.iter().zip([-1934, -1528, -1276]) {
        assert!((*x - ps2).abs() <= 2, "{seen:?}");
    }
}

/// Eddy's run-off (4829) turns to the screen's axis, so its path follows the camera's yaw: the yaw chases the
/// runner's root (flags bits 20-21 = 2, 0x38F7F8), the eased sample starts from the last one moved by the other's
/// step (0x38F2DC), and the atan is the PS2's table (floor). PCSX2 (+0x51C) on frame 20: (-1702, 1746); ours was
/// (-1625, 1883). Its head's and feet's screen x (+0x628..0x62C) then, through last frame's matrix (0x34FD90): -144,
/// -18, -121 (ours were -148, -21, -125 through this frame's: the middle one under -0x14, off the screen a frame early).
#[test]
fn eddys_run_off_follows_the_cameras_yaw() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [450, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let mut at = None;
    for pad in parse_script("n*61,f*30,n*2,5*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        if (f.alias, f.frame) == (RUN_OFF, 20) {
            at = Some((f.root().x as i32, -f.root().z as i32, f.screen_points));
        }
    }
    let (x, z, p) = at.unwrap();
    assert!((x + 1702).abs() <= 5 && (z - 1746).abs() <= 5, "{x} {z}");
    assert!(p.iter().zip([-144, -18, -121]).all(|(a, b)| (a - b).abs() <= 1), "{p:?}");
}

/// The fight camera samples as the game (0x38EC08): a walk's root (flags bits 20-21 = 1), so the eye follows Nina's
/// walk from its first frames; the tag's run-off (id 0x18) eases toward the opponent by 6/128 and the framing steps
/// 1/16 while it runs; she is off the screen and the partner comes in 20 frames after the run-off starts. PCSX2 (the
/// view block's eye x, 0x41581C): 84 on the walk's frame 11, 623 as the partner enters (the log's
/// row there repeats the run-off's last 619 and skips 626: a read a frame late).
#[test]
fn the_fight_camera_follows_as_the_game() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [NINA, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    let (mut walk, mut run, mut entrance) = (None, None, None);
    for (k, pad) in parse_script("n*61,f*30,n*2,5*2,n*60").unwrap().into_iter().enumerate() {
        b.step([pad, Pad::default()]);
        let (f, x) = (&b.game.fight.fighters[0], b.game.camera.view.eye[0]);
        match (f.alias, f.frame) {
            (63, 11) => walk = Some(x),
            (5394, 1) => run = Some(k),
            (5397, 1) => entrance = Some((k, x)),
            _ => {}
        }
    }
    let (r, (e, ex)) = (run.unwrap(), entrance.unwrap());
    assert_eq!((walk, e - r), (Some(84), 20));
    assert!((ex - 623).abs() <= 2, "{ex}");
}

/// The yaw freezes only while the fighters are within 200 (gp-0x74C8, 0x38F9CC), not for a round's first 200 frames:
/// Eddy's stance sways him 387 off the axis and the camera turns with him. PCSX2 (0x41580C): 112 after his walk.
#[test]
fn the_fight_camera_turns_from_the_rounds_start() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [450, 426]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad { buttons: 3, ..Pad::default() }; 2]);
    }
    for pad in parse_script("n*62,f*30").unwrap().into_iter().take(61) {
        b.step([pad, Pad::default()]);
    }
    assert!((b.game.camera.view.yaw - 112).abs() <= 2, "{}", b.game.camera.view.yaw);
}

/// Ogre's strike slot 0x13 is his slot 6 (0x34F2C0, body 14 only): his b+2 (alias 359, bones 0x13) reaches Lei from
/// 1000 as in PCSX2.
#[test]
fn ogres_slot_0x13_strikes() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [OGRE_FIRST, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let f = &mut b.game.fight.fighters;
    let along = (f[1].position - f[0].position).normalize();
    f[1].position = f[0].position + along * 1000.0;
    f[1].guard_input = false;
    let before = b.game.fight.fighters[1].health;
    for pad in parse_script("b+2*2,n*50").unwrap() {
        b.step([pad, Pad::default()]);
    }
    assert!(b.game.fight.fighters[1].health < before, "Lei is hit");
}

/// Yoshimitsu's sword is strike slots 22 / 23 (0x34F3D8: hand B's (100, 0, 720) and (100, 0, 360)): his d, d/f, f+1
/// slash (moves 2350 / 2351, both segments ending at 22) reaches Hwoarang from the fight's start: he reels (alias
/// 3224) on frame 50, PCSX2's 51.
#[test]
fn yoshimitsus_sword_strikes() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [YOSHIMITSU, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let hit = parse_script("n*4,d*2,d/f*2,f+1*2,n*80").unwrap().into_iter().position(|pad| {
        b.step([pad, Pad::default()]);
        b.game.fight.fighters[1].alias == 3224
    });
    assert!(hit.is_some_and(|k| (48..=53).contains(&k)), "Hwoarang reels on frame {hit:?}");
}

/// The strike slots past 17 that 0x3AFAA0 adds, at the drawn nodes the motions turn: True Ogre's 20 behind his hips
/// (PCSX2 at the fight's start: 478 behind the root, 704 up), Roger's tail from his pelvis (18: 167 behind, 818 up),
/// not at the head nor the world's origin.
#[test]
fn the_tails_strike_slots_are_where_the_ps2_has_them() {
    let Some(a) = assets() else { return };
    for (model, slot, want) in [(TRUE_OGRE, 20, glam::Vec3::new(-478.0, 704.0, -174.0)), (ROGER, 18, glam::Vec3::new(-167.0, 818.0, -140.0))] {
        let mut b = Bout::new(Game::load(&a, [model, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
        while b.phase != Phase::Fight {
            b.step([Pad::default(); 2]);
        }
        let (pose, place) = b.game.pose(0);
        let at = kumite_game::volumes(&b.game.bodies[0], &pose, place).bones[slot] - b.game.fight.fighters[0].root();
        assert!(at.distance(want) < 150.0, "model {model} slot {slot}: {at}");
    }
}

/// True Ogre's fire (1+2, move 3799, a strike to slot 0x18): his flames (kumite_sim::objects, 0x3A4EE8) burn
/// Hwoarang (alias 3572) 1.95 m away on frame 63-65 of it, as in PCSX2 (the move audit's `trueogre_12`, the
/// fighters and partners picked on the select: Hwoarang burns on its frame 64); the strike to slot 0x18 had no
/// segment.
#[test]
fn true_ogres_fire_burns() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [TRUE_OGRE, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut at = None;
    for pad in parse_script("n*62,f*30,n*2,1+2*2,n*90").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if at.is_none() && f[1].alias == 3572 {
            at = Some((f[0].current.index, f[0].frame, (f[1].root() - f[0].root()).length()));
        }
    }
    eprintln!("burns at {at:?}");
    assert!(at.is_some_and(|(m, k, d)| m == 3799 && (62..=66).contains(&k) && (1700.0..2200.0).contains(&d)), "{at:?}");
}

/// True Ogre's d/b+1+2 (move 3800) as in PCSX2 (the move audit's `trueogre_sdb12`, both holding 1+2 through the
/// intro): the first flame starts at (865, 1381, -807), 200 along the z of node 13 as drawn, the neck split
/// (0x3A4EE8); the hit pass reads the segments the task runner registered after the frame before (0x346230, 0x3770E0),
/// so Hwoarang burns (5527) on frame 38 of it (the animated node 13 and this frame's segments: frame 36).
#[test]
fn true_ogres_low_fire_lands_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [TRUE_OGRE, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let hold = Pad { buttons: 3, ..Pad::default() };
    while b.phase != Phase::Fight {
        b.step([hold; 2]);
    }
    let (mut start, mut burns) = (None, None);
    for pad in parse_script("n*61,f*30,n*2,d/b+1+2*2,n*129").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        start = start.or_else(|| b.game.fight.objects.list.first().map(|o| o.pos));
        if burns.is_none() && f[1].shown_move().0.index == 5527 {
            burns = Some((f[0].shown_move().0.index, f[0].shown_move().1));
        }
    }
    assert!(start.is_some_and(|p| p.distance(glam::Vec3::new(865.0, 1381.0, -807.0)) < 20.0), "{start:?}");
    assert_eq!(burns, Some((3800, 38)));
}

/// True Ogre (Paul) against Hwoarang (Paul) with the fight's sounds, to the fight; then (the move audit's
/// `trueogre_wN`), Tekken Tag's one rand() where PCSX2 had it then (audit38's r3, arena 9,
/// which draws nothing): its state (0x4F57F8) and seed (0x439C7C), the vblank count (0x8BD2F4), the main loop's
/// frame (gp-0x74D8) and the faces (Hwoarang's blink 75 frames off; True Ogre's never comes).
fn ogre_from_the_ps2s_rand(a: &Assets) -> Bout {
    let mut g = Game::load(a, [TRUE_OGRE, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(a, PAUL).unwrap(), kumite_game::Actor::load(a, PAUL).unwrap()]);
    let sound = std::sync::Arc::new(SoundData::load(a).unwrap());
    g.sound = Some(Box::new(kumite_game::sound::FightSound::new(sound, [String::new(), String::new()], [0, 1], SoundState::default())));
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b
}

/// The PS2's state at the move audit's row 1, the walk's first frame (step 63 of `n*63,...`: the PS2's moves take a
/// pad a frame after it is read, 0x36C4C0, so its rows' moves are the script's a frame later; its press seeds rand()
/// as it is read, a frame before ours, so the vblank count one less).
fn ps2_rand_at_the_walk(b: &mut Bout) {
    let o = &mut b.game.fight.objects;
    (o.seed, o.reseed, o.vblanks, o.loops) = (464_375, 464_375, 2039, 1238);
    let f = &mut b.game.tag_faces;
    (f[0].face, f[0].timer, f[1].face, f[1].timer) = (0, -210, 0, 74);
}

/// Tekken Tag's one rand() (0x40FBD0) as on the PS2: True Ogre's 1+2 after a walk of 30 / 10 frames, from PCSX2's
/// state as the walk starts, draws as PCSX2's does frame by frame: the press of 1+2 seeds it again (0x35C370: the seed
/// plus the vblank count), then the attack voice (0x3764D8), Hwoarang's blink (0x362B38), the swing (0x376680), the
/// flames (0x3A44F8) with their sparks (0x3A4970) and fires on the floor (0x3A4C28), the hurt voice on main-loop
/// frames not a multiple of 4 (0x376410) and the burns (0x3A54D8, 0x3A50B8); Hwoarang burns on frame 64 / 65.
#[test]
fn true_ogres_fire_draws_from_the_ps2s_one_rand() {
    let Some(a) = assets() else { return };
    let w30: [(usize, u32); 18] = [
        (1, 464_375),
        (13, 464_375),
        (33, 3_216_496_188),
        (57, 3_216_496_188),
        (69, 3_216_496_188),
        (70, 3_216_496_188),
        (75, 3_216_496_188),
        (76, 3_216_496_188),
        (77, 2_465_509_317),
        (89, 387_648_794),
        (90, 1_081_443_137),
        (95, 1_889_600_573),
        (96, 3_250_949_969),
        (100, 204_053_683),
        (120, 390_489_922),
        (150, 2_414_296_842),
        (163, 1_290_056_701),
        (183, 2_145_025_512),
    ];
    let w10: [(usize, u32); 17] = [
        (1, 464_375),
        (13, 2_332_280_328),
        (33, 2_332_280_328),
        (57, 2_332_280_328),
        (69, 2_300_703_137),
        (70, 85_146_292),
        (75, 3_803_897_841),
        (76, 3_162_766_352),
        (77, 2_943_799_250),
        (89, 2_802_513_485),
        (90, 2_020_555_851),
        (95, 2_178_714_258),
        (96, 2_704_211_318),
        (100, 471_568_000),
        (120, 2_224_671_712),
        (150, 3_076_873_747),
        (163, 2_149_010_223),
    ];
    for (walk, ps2, burn) in [(30, &w30[..], 64), (10, &w10[..], 65)] {
        let mut b = ogre_from_the_ps2s_rand(&a);
        let (mut seen, mut burns) = (Vec::new(), None);
        for (k, pad) in parse_script(&format!("n*63,f*{walk},n*2,1+2*2,n*200")).unwrap().into_iter().enumerate().take(62 + 184) {
            b.step([pad, Pad::default()]);
            if k == 63 {
                ps2_rand_at_the_walk(&mut b);
            }
            let f = &b.game.fight.fighters;
            if let Some(&(row, _)) = ps2.iter().find(|r| k == 62 + r.0) {
                seen.push((row, b.game.fight.objects.seed));
            }
            if burns.is_none() && f[1].shown_move().0.index == 5527 {
                burns = Some(f[0].shown_move().1);
            }
        }
        assert_eq!(seen, ps2, "walk {walk}");
        assert_eq!(burns, Some(burn), "walk {walk}");
    }
}

/// True Ogre's fire sets sparks off (task 0x3A4970: quad 0x80 at 0.2, 0.044 m) and fires on the floor (0x3A4C28:
/// quad 0x400 at 0.2-0.5) at Hwoarang's feet and where the stream licks the floor: his 1+2 after a walk of 30 frames from
/// the PS2's rand() (as [`true_ogres_fire_draws_from_the_ps2s_one_rand`]): 5 floor fires burn at once by the move audit's
/// last row, as PCSX2's draws start them (0x3A4C84's lives).
#[test]
fn true_ogres_fire_sparks_and_sets_the_floor_alight() {
    let Some(a) = assets() else { return };
    let mut b = ogre_from_the_ps2s_rand(&a);
    let mut v = kumite_game::vfx::Vfx::new(&a);
    let (mut sparks, mut floor, mut feet) = (0, 0, None);
    for (k, pad) in parse_script("n*63,f*30,n*2,1+2*2,n*150").unwrap().into_iter().enumerate() {
        // (the fire at his feet stands at his root as drawn the frame before the hit, player +0x590 / +0x598)
        let drawn = b.game.fight.fighters[1].root() / 1000.0;
        b.step([pad, Pad::default()]);
        if k == 63 {
            ps2_rand_at_the_walk(&mut b);
        }
        v.step(&b.game);
        let s = v.sprites();
        if feet.is_none() && b.game.fight.hits.iter().any(|h| h.attacker == 0 && h.limb == 0x18) {
            feet = Some(s.iter().any(|x| (x.pos.y - 0.366).abs() < 1e-4 && (x.pos.x - drawn.x).abs() < 1e-4 && (x.pos.z - drawn.z).abs() < 1e-4));
        }
        let n = s.iter().filter(|x| (x.size - 0.0437).abs() < 0.001).count();
        let f = s.iter().filter(|x| x.size > 0.34 && x.size < 0.88 && (x.pos.y - 0.366).abs() < 1e-4).count();
        if std::env::var_os("EMBERS").is_some() {
            println!("{} {} {} sparks {n} floor {f}", k as i64 - 62, b.game.fight.fighters[0].current.index, b.game.fight.fighters[0].frame);
        }
        (sparks, floor) = (sparks.max(n), floor.max(f));
    }
    assert!((8..=40).contains(&sparks) && floor == 5 && feet == Some(true), "sparks {sparks} floor {floor} feet {feet:?}");
}

/// The effects frame by frame (for a comparison with PCSX2's effect tasks): `OK_FX_MODEL` (Paul) vs Hwoarang,
/// crouching, P1's script in `OK_FX_SCRIPT`; each frame P1's move and frame, then per kind (the character's burst, the atlas's
/// bursts, the heavy hit's fireballs) the particles drawn, their mean and largest quad (metres), mean level.
#[test]
#[ignore]
fn effects_log() {
    let Some(a) = assets() else { return };
    let model = std::env::var("OK_FX_MODEL").ok().and_then(|m| m.parse().ok()).unwrap_or(PAUL);
    let mut g = Game::load(&a, [model, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, KING).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut v = kumite_game::vfx::Vfx::new(&a);
    let script = std::env::var("OK_FX_SCRIPT").unwrap_or_else(|_| "f*60,n*6,b+1+2*2,n*120".into());
    let p2 = parse_script("d*400").unwrap();
    for (k, pad) in parse_script(&script).unwrap().into_iter().enumerate() {
        b.step([pad, p2.get(k).copied().unwrap_or_default()]);
        v.step(&b.game);
        let s = v.sprites();
        let heavy: std::collections::HashSet<u32> = v.data.as_ref().map(|d| d.heavy.iter().flat_map(|h| h.iter().copied()).collect()).unwrap_or_default();
        let dust = |x: &&kumite_game::vfx::Sprite| x.blend == kumite_game::vfx::Blend::Alpha;
        let kinds: [(&str, Vec<&kumite_game::vfx::Sprite>); 4] = [
            ("char", s.iter().filter(|x| x.sheet > 0).collect()),
            ("burst", s.iter().filter(|x| x.sheet == 0 && !heavy.contains(&x.cell) && !dust(x)).collect()),
            ("heavy", s.iter().filter(|x| x.sheet == 0 && heavy.contains(&x.cell)).collect()),
            ("dust", s.iter().filter(|x| x.sheet == 0 && dust(x)).collect()),
        ];
        let f = &b.game.fight.fighters[0];
        let mut line = format!("{k} {} {}", f.current.index, f.frame);
        for (name, x) in kinds.iter().filter(|x| !x.1.is_empty()) {
            let n = x.len() as f32;
            let level = x.iter().map(|s| s.color[0].max(s.color[1]).max(s.color[2]) * s.color[3]).sum::<f32>() / n;
            let max = x.iter().map(|s| s.size).fold(0.0, f32::max);
            line += &format!("  {name} n{} sz{:.3}/{max:.3} lv{level:.2}", x.len(), x.iter().map(|s| s.size).sum::<f32>() / n);
        }
        println!("{line}");
    }
}

/// Paul's b+1+2 (move 1068) flames at his slots 4, 5 and 6 on its frames 7, 22 and 37 (its op 6, 0x372C40 ->
/// 0x3A2BC0), as PCSX2 draws it while he winds up; Tekken Tag's rules name no slots, so the ops were dropped.
#[test]
fn pauls_burning_fist_flames_as_it_charges() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, JIN).unwrap(), kumite_game::Actor::load(&a, KING).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut v = kumite_game::vfx::Vfx::new(&a);
    let (mut seen, mut flames) = (0, vec![]);
    for pad in parse_script("n*4,b+1+2*2,n*60").unwrap() {
        b.step([pad, Pad::default()]);
        v.step(&b.game);
        let n = v.bursts.iter().filter(|x| x.set.sheet > 0).count();
        if n > seen && b.game.fight.fighters[0].current.index == 1068 {
            flames.push(b.game.fight.fighters[0].frame);
        }
        seen = n;
    }
    assert_eq!(flames, [7, 22, 37]);
}

/// Devil's laser (0x378150: slot 0x18, from his head, 200 longer each active frame): his n+1+2 (move 4175) from
/// -554 reaches Hwoarang at 944 and he falls (alias 3626) on frame 36 of it, as in PCSX2 (its 37th: audit
/// 29_cmd00); the strike to slot 0x18 had no segment.
#[test]
fn devils_laser_strikes() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [DEVIL, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut at = None;
    // (drawn as the PS2's 0x3A8D08: strips along it, the eye flare its first 15 frames; a pack of format 65 on)
    let mut v = kumite_game::vfx::Vfx::new(&a);
    let drawn = v.data.as_ref().is_some_and(|d| d.characters.get(&DEVIL).is_some_and(|c| c.burst.is_some()));
    let (mut strips, mut flares) = (0, 0);
    for pad in parse_script("n*4,1+2+3+4*2,n*80,n+1+2*2,n*60").unwrap() {
        b.step([pad, Pad::default()]);
        v.step(&b.game);
        strips = strips.max(v.ribbons.len());
        flares += usize::from(v.sprites().iter().any(|s| s.sheet > 0 && (s.size - 0.899).abs() < 0.01));
        let f = &b.game.fight.fighters;
        if at.is_none() && f[1].alias == 3626 {
            at = Some((f[0].current.index, f[0].frame));
        }
    }
    assert!(at.is_some_and(|(m, k)| m == 4175 && (35..=38).contains(&k)), "Hwoarang falls at {at:?}");
    assert!(!drawn || (strips > 2 && flares == 15), "{strips} strips, {flares} frames of the flare");
}

/// True Ogre's arm A and slots 18 / 19 wave by their counter (0x3626B8), with each other's places: standing, his
/// counter at 209 on the fight's first frame, the four slots stand where PCSX2's do (+0x630 + 0x50 k + 0x38, his
/// position off: within 3).
#[test]
fn true_ogres_arm_waves_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [TRUE_OGRE, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    // (his intro alias 6, as the recording's flip gave him: × held at the call)
    b.step([parse_script("3").unwrap()[0], Pad::default()]);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b.game.flaps[0].count = 208;
    b.step([Pad::default(); 2]);
    let s = b.game.spheres();
    let p = b.game.fight.fighters[0].position;
    let ps2 = [(5, [20, -1235, -566]), (6, [19, -917, -691]), (18, [86, -767, -733]), (19, [331, -677, -797])];
    for (slot, at) in ps2 {
        let q = s[0].bones[slot] - p;
        let ours = [q.x as i32, -q.y as i32, -q.z as i32];
        assert!(ours.iter().zip(at).all(|(o, a)| (o - a).abs() <= 3), "slot {slot}: {ours:?}, PCSX2 {at:?}");
    }
}

/// True Ogre's wave counter (+0xB120) starts again with each round: PCSX2 (t3sim46, a time-up into round 2) calls
/// 0x363150 at round 2's set-up and on its next frame, and the counter stands at 90 on round 2's first fight frame.
#[test]
fn true_ogres_wave_starts_again_with_the_round_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [TRUE_OGRE, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b.game.fight.clock = 30;
    while !(b.round == 2 && b.phase == Phase::Fight) {
        b.step([Pad::default(); 2]);
    }
    assert_eq!(b.game.flaps[0].count, 90);
}

/// Yoshimitsu's b, b+1+2, b, 1 (move 2365) blows mist (slot 0x19, 0x3A5810) on each of its active frames 21-28:
/// eight puffs, drawn magenta; from where he stands it misses Hwoarang, as in PCSX2.
#[test]
fn yoshimitsus_mist_puffs() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [YOSHIMITSU, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut v = kumite_game::vfx::Vfx::new(&a);
    let (mut puffs, mut drawn, mut hit) = (0, 0, false);
    for pad in parse_script("n*20,b*3,b+1+2*2,b*12,1*2,n*100").unwrap() {
        b.step([pad, Pad::default()]);
        v.step(&b.game);
        let mist = b.game.fight.objects.list.iter().filter(|o| o.kind == kumite_sim::objects::Kind::TagMist).count();
        puffs = puffs.max(mist);
        drawn = drawn.max(v.sprites().iter().filter(|s| s.color[0] == 1.0 && s.color[1] == 0.0).count());
        hit |= b.game.fight.hits.iter().any(|h| h.attacker == 0);
    }
    assert_eq!((puffs, drawn, hit), (8, 8, false));
}

/// Jack-2 and Paul against Hwoarang and Paul, picked on the select (PCSX2): Jack walks in, his body pushes Hwoarang
/// back as he comes, and his 1+3 (alias 3811) then misses from 850 away: no grab (alias 3924), as on the PS2 (the
/// grab seen before came from a run that kept the select's models). The stance's phase is the PS2 run's (11 frames
/// on), the pad read 2 frames late.
#[test]
fn jack2s_walk_pushes_and_his_grab_misses_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [603, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let ps2 = [(30, 89, 1012), (35, 207, 1093), (40, 263, 1122), (44, 284, 1134)];
    let mut grab = false;
    for (k, pad) in parse_script("n*13,f*40,n*2,1+3*2,n*60").unwrap().into_iter().enumerate() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        grab |= f[0].alias == 3924;
        if let Some(&(_, x0, x1)) = ps2.iter().find(|p| p.0 + 11 == k) {
            let (r0, r1) = (f[0].root().x as i32, f[1].root().x as i32);
            assert!((r0 - x0).abs() <= 5 && (r1 - x1).abs() <= 5, "frame {}: {r0} {r1}, the PS2 {x0} {x1}", k - 11);
        }
    }
    assert!(!grab, "the grab");
}

/// The joint slots in the game's integers (0x34EF20): Kazuya and Paul against Hwoarang (model 426) and Paul, as the
/// move audit picks them in PCSX2. In the stance his 18 slots are PCSX2's to the unit (the turns from the motion's
/// angles, as the game's), and his walk's push leaves him where PCSX2 does to the unit (`$W/moves10`,
/// kazuya_1: 3 and 26 at his walk's frames 9 and 10; the float skeleton's spheres left 2 and 25).
#[test]
fn kazuyas_slots_and_walk_push_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [585, 426]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    // (PCSX2's slots 0-17 in its axes: y down, z flipped; the stance's frame 61, Hwoarang's 11)
    let slots = [
        [-992, -1041, -8],
        [-992, -1041, -8],
        [-993, -1496, -47],
        [-1080, -1384, -86],
        [-1161, -1392, -176],
        [-981, -1189, -299],
        [-798, -1272, -192],
        [-906, -1392, 10],
        [-805, -1408, 78],
        [-556, -1247, 118],
        [-406, -1323, -36],
        [-992, -1041, -8],
        [-1039, -885, -97],
        [-1100, -466, -213],
        [-1300, -103, -73],
        [-893, -889, 35],
        [-663, -518, -6],
        [-626, -98, 116],
    ];
    let (mut stance, mut walk) = (false, Vec::new());
    for pad in parse_script("n*62,f*30,n*2,1*2,n*20").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters;
        if (f[0].current.index, f[0].frame, f[1].frame) == (26, 61, 11) && !stance {
            stance = true;
            for (s, (p, q)) in b.game.spheres()[0].bones.iter().zip(slots).enumerate() {
                let p = [p.x as i32, -p.y as i32, -p.z as i32];
                assert_eq!(p, q, "slot {s}");
            }
        }
        if f[0].current.index == 101 && (9..=10).contains(&f[0].frame) {
            walk.push((f[0].root().x as i32, f[1].root().x as i32));
        }
    }
    assert!(stance, "the stance's frame 61");
    assert_eq!(walk, [(3, 1010), (26, 1033)]);
}

/// Kazuya's d+4 on Hwoarang as PCSX2 logs it (P1 x, P2 x and z by Kazuya's frame): the reaction's first frame shows
/// its root step through the hit-stop, so the push on the hit frame shares it (P1 -204, P2 +62); his u/f+4's jump
/// pushes Hwoarang along x only. Armor King's root rises by his motion scale (4423) in his slots: his stance at
/// PCSX2's height.
#[test]
fn the_hit_frames_push_and_a_scaled_roots_height_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let run = |model: usize, script: &str, each: &mut dyn FnMut(&Bout)| {
        let mut g = Game::load(&a, [model, 426]).unwrap();
        g.start_tag([kumite_game::Actor::load(&a, PAUL).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
        let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
        while b.phase != Phase::Fight {
            b.step([Pad::default(); 2]);
        }
        for pad in parse_script(script).unwrap() {
            b.step([pad, Pad::default()]);
            each(&b);
        }
    };
    let mut hit = Vec::new();
    run(585, "n*62,f*30,n*2,d+4*2,n*20", &mut |b: &Bout| {
        let f = &b.game.fight.fighters;
        if f[0].current.index == 1243 && (12..=14).contains(&f[0].frame) {
            hit.push((f[0].root().x as i32, f[1].root().x as i32, -f[1].root().z as i32));
        }
    });
    assert_eq!(hit, [(472, 1326, 0), (261, 1529, 351), (149, 1728, 499)]);
    // Kazuya's u/f+4 lands beside Hwoarang and turns his body to him; the push keeps to his basis (+0x1A), so it
    // drives Hwoarang straight back, not sideways
    let mut jump = Vec::new();
    run(585, "n*61,f*30,n*2,u/f+4*2,n*20", &mut |b: &Bout| {
        let f = &b.game.fight.fighters;
        if f[0].current.index == 694 && (13..=17).contains(&f[0].frame) {
            jump.push((f[1].root().x as i32, -f[1].root().z as i32));
        }
    });
    assert_eq!(jump, [(1143, 0), (1195, 0), (1247, 0), (1297, 0), (1340, 0)]);
    let mut height = None;
    run(639, "n*62", &mut |b: &Bout| {
        if b.game.fight.fighters[0].current.index == 32 && b.game.fight.fighters[0].frame == 60 && height.is_none() {
            height = Some(-b.game.spheres()[0].bones[0].y as i32);
        }
    });
    assert_eq!(height, Some(-1126));
}

/// Mokujin (no moves of his own) fights as another fighter, drawn from Tekken Tag's pool (0x35C850): he jabs.
#[test]
fn mokujin_fights_with_a_copied_moveset() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [540, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let stance = b.game.fight.fighters[0].alias;
    let moved = parse_script("1*2,n*20").unwrap().into_iter().any(|pad| {
        b.step([pad, Pad::default()]);
        b.game.fight.fighters[0].alias != stance && b.game.clip(0).is_some()
    });
    assert!(moved, "Mokujin did not jab");
    // as Anna (moveset 17, the PS2's draw in the audit): her stance and her n+1+2 (move 808) on his body, then her
    // n+3+4 (887), as in PCSX2 (audit m15_cmd05)
    b.game.copy_as(0, 15);
    b.game.fight.fighters[0].play_alias(58);
    let mut seen = Vec::new();
    for pad in parse_script("n*40,n+1+2*2,n*80,n+3+4*2,n*20").unwrap() {
        b.step([pad, Pad::default()]);
        let f = &b.game.fight.fighters[0];
        assert!(b.game.clip(0).is_some(), "no motion for move {}", f.current.index);
        seen.push(f.current.index);
    }
    assert!(seen.contains(&25) && seen.contains(&808) && seen.contains(&887), "{seen:?}");
}

/// The drawn skeleton is the game's joint places too (0x3478F0): Jack-2's knee is 390 down his thigh.
#[test]
fn jack2_is_drawn_with_the_games_joints() {
    let Some(a) = assets() else { return };
    let jack = a.character(603).unwrap();
    let shin = jack.nodes.iter().find(|n| n.name == "shin_A").unwrap();
    assert!((shin.translation.length() * 1000.0 - 390.0).abs() < 1.0, "the knee {}", shin.translation.length() * 1000.0);
}

/// A slot's twist copies are drawn with its matrix (0x3478F0), eased or not: no shoulder, elbow or knee torn
/// between the eased pose and the next motion's as moves change.
#[test]
fn twist_copies_turn_with_their_slot() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [PAUL, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    let twins = b.game.bodies[0].twins.clone();
    assert!(twins.iter().any(|&(c, n)| b.game.bodies[0].character.nodes[c].name == "upperarm_A.1" && b.game.bodies[0].character.nodes[n].name == "upperarm_A"), "{twins:?}");
    let mut eased = 0;
    for pad in parse_script("n*60,1*2,n*30,3+4*2,n*40,f*30,2*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
        eased += usize::from(b.game.fight.fighters[0].eased().is_some());
        let (pose, _) = b.game.pose(0);
        for &(c, n) in &twins {
            assert!(pose[c].abs_diff_eq(pose[n], 1e-5), "node {c} off its slot's {n} at frame {}", b.game.fight.frame);
        }
    }
    assert!(eased > 0);
}

/// Paul's belt ends sway (0x34D258): off their bind pose as he walks, never under the floor.
#[test]
fn pauls_belt_sways() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [PAUL, HWOARANG]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    let belt = b.game.bodies[0].character.cloth.first().map(|c| c.node).expect("Paul's belt");
    let mut cloth = kumite_game::Cloth::default();
    let mut moved = 0.0f32;
    for (k, pad) in parse_script("n*60,f*60,n*60").unwrap().into_iter().enumerate() {
        b.step([pad, Pad::default()]);
        let (mut pose, place) = b.game.pose(0);
        let rest = pose[belt];
        cloth.apply(&b.game.bodies[0].character, b.game.fight.frame, place, &mut pose);
        let tip = (place * pose[belt]).transform_point3(glam::Vec3::X * 0.12);
        assert!(pose[belt].is_finite() && tip.y > -0.001, "frame {k}: the tip {tip}");
        if (60..120).contains(&k) {
            moved = moved.max((pose[belt].w_axis - rest.w_axis).length().max((pose[belt].x_axis - rest.x_axis).length()));
        }
    }
    assert!(moved > 0.05, "the belt kept its bind pose ({moved})");
}

/// The body push measures each side's step from where the last push left it (0x377338 against 0x8A7A90): Paul's
/// u/f+3 walks Hwoarang's guard back and they part 1004 apart in PCSX2 (1102 when the step left the push out). The
/// move eases in from the stance's pose (0x373428): his hips 103, 94, 86, ..., 8, 0 below the motion's, as in PCSX2.
#[test]
fn the_push_after_a_guard_parts_them_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [PAUL, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(&a, LAW).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let mut hips = Vec::new();
    for pad in parse_script("n*2,u+1+2*2,n*80,u+3+4*2,n*80,u/f+1*2,n*80,u/f+3*2,n*40").unwrap() {
        b.step([pad, Pad::default()]);
        let p = &b.game.fight.fighters[0];
        if p.alias == 4677 && p.frame <= 13 {
            hips.push(p.eased().map_or(1, |e| e.1 as i32));
        }
    }
    assert_eq!(hips, [-103, -94, -86, -77, -68, -60, -51, -43, -34, -25, -17, -8, 0]);
    let f = &b.game.fight.fighters;
    let apart = f[0].root().distance(f[1].root());
    assert!((apart - 1004.0).abs() < 30.0, "{apart}");
}

/// As PCSX2 has them (Paul and Law against Hwoarang and Paul, picked on the select; the pad read 2 frames late): the
/// sidestep faces the angle to the opponent of the roots before the frame's motion (0x371330) plus its lean
/// (0x370698); f,f+2's pushback starts on the reaction's first frame, in integers; the dash pushing Hwoarang goes on
/// (condition 0x36) instead of breaking into the run.
#[test]
fn sidestep_pushback_and_dash_as_the_ps2() {
    let Some(a) = assets() else { return };
    let run = |script: &str, mut each: Box<dyn FnMut(&kumite_sim::Fighter, &kumite_sim::Fighter) + '_>| {
        let mut g = Game::load(&a, [PAUL, HWOARANG]).unwrap();
        g.start_tag([kumite_game::Actor::load(&a, LAW).unwrap(), kumite_game::Actor::load(&a, PAUL).unwrap()]);
        let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
        while b.phase != Phase::Fight {
            b.step([Pad::default(); 2]);
        }
        for pad in parse_script(script).unwrap() {
            b.step([pad, Pad::default()]);
            each(&b.game.fight.fighters[0], &b.game.fight.fighters[1]);
        }
    };
    // the sidestep (move 531): its facing (+0x1A, within the angle table's step) frame by frame, and where it ends
    let ps2 = [16990, 17469, 18060, 18732, 19451, 20218, 21002, 21769, 0, 23192, 23847, 24519, 25174, 25734];
    let mut end = None;
    run(
        "n*2,u*2,n*40",
        Box::new(|p, _| {
            let (k, facing) = (p.frame as usize, ((std::f32::consts::PI - p.yaw) * 32768.0 / std::f32::consts::PI).round() as i32 & 0xFFFF);
            if p.current.index == 531 && (2..=15).contains(&k) && ps2[k - 2] != 0 {
                assert!((facing - ps2[k - 2]).abs() <= 16, "frame {k}: {facing}");
            }
            if p.current.index == 531 && k == 15 {
                end = Some(p.root());
            }
        }),
    );
    let end = end.unwrap();
    assert!((end.x + 605.0).abs() <= 2.0 && (-end.z - 809.0).abs() <= 2.0, "{end}");
    // f,f+2's reaction (alias 3677): Hwoarang's root on its frames 2-5
    let mut back = Vec::new();
    run(
        "n*4,f*2,n*2,f+2*2,n*30",
        Box::new(|_, h| {
            if h.alias == 3677 && (2..=5).contains(&h.frame) && back.last().is_none_or(|&(f, _, _)| f != h.frame) {
                back.push((h.frame, h.root().x as i32, -h.root().z as i32));
            }
        }),
    );
    assert_eq!(back, [(2, 1346, 10), (3, 1445, 12), (4, 1474, 12), (5, 1493, 12)]);
    // the dash (move 129) into Hwoarang is still on at its frame 24
    let mut dash = 0;
    run(
        "n*2,f*2,n*2,f*30",
        Box::new(|p, _| {
            if p.current.index == 129 {
                dash = dash.max(p.frame);
            }
        }),
    );
    assert!(dash >= 24, "the dash broke off at frame {dash}");
}

/// Ogre's and True Ogre's first models.
const OGRE_FIRST: usize = 528;
const TRUE_OGRE: usize = 591;

/// Gun Jack's first model.
const GUN_JACK: usize = 555;

/// Hwoarang's first model.
const HWOARANG: usize = 420;

/// Julia's first model.
const JULIA: usize = 480;
/// King's first model.
const KING: usize = 390;
/// Devil's first model.
const DEVIL: usize = 663;
/// Roger's first model.
const ROGER: usize = 567;

/// The arcade cells' models (the 1P's partner, the CPU teams).
const YOSHIMITSU: usize = 396;
const NINA: usize = 408;
const MICHELLE: usize = 615;
/// Unknown's first model (stage 8's team, both slots, as the PS2's arcade sets it).
const UNKNOWN: usize = 693;

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

/// Round 1's intros with no button held (0x368398): rand() & 1, odd alias 4, even 6. 0x40FBD0 returns the state's low
/// 31 bits, whose bit 0 turns over each draw, and P1's and P2's flips are drawn in a row (0x373038): the two always
/// take different intros (PCSX2, four rand() states written before the round: 4 / 6 once, 6 / 4 three times).
#[test]
fn the_round_one_intros_differ_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    b.step([Pad::default(); 2]);
    let aliases = b.game.fight.fighters.each_ref().map(|f| f.alias);
    assert!(aliases == [4, 6] || aliases == [6, 4], "{aliases:?}");
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
    // the camera frames Paul's position, not his running root, so he leaves the screen 14 frames into the run as in
    // PCSX2
    assert!((12..=16).contains(&(swap - tagging)), "the partner comes in {} frames into the run", swap - tagging);
    // Law comes in on his own entrance where Paul left
    assert_eq!((r[swap].0, r[swap].2), (ENTRANCE, Some(1)));
    assert!(r[swap].3.distance(r[swap - 1].3) < 150.0, "the partner enters at the same spot: {} -> {}", r[swap - 1].3, r[swap].3);
    assert!(r[swap].3.distance(r[tagging].3) > 300.0, "Paul ran off first: {} -> {}", r[tagging].3, r[swap].3);
    assert!(r[swap..].iter().all(|x| x.1 == 1), "one swap");
    assert_eq!(r.iter().map(|x| x.4).collect::<Vec<_>>(), tag_out(&a).iter().map(|x| x.4).collect::<Vec<_>>(), "the same pads, the same match");
}

/// The run off by the way P1 faces on the screen (conditions 0x54-0x5B): facing Lei it runs straight off its side
/// (5394), turned away (facing its own edge, +0x54 2 on the left) it takes 5431.
#[test]
fn the_run_off_follows_the_facing_on_the_screen() {
    let Some(a) = assets() else { return };
    let tag = parse_script("5").unwrap()[0];
    let run = |turned: bool| {
        let mut b = tag_bout(&a, LAW);
        if turned {
            b.game.fight.fighters[0].yaw += std::f32::consts::PI;
        }
        b.step([tag, Pad::default()]);
        b.step([Pad::default(); 2]);
        b.game.fight.fighters[0].alias
    };
    assert_eq!((run(false), run(true)), (RUN_OFF, 5431));
}

#[test]
fn recoverable_health_and_anger() {
    let Some(a) = assets() else { return };
    let mut b = tag_bout(&a, LAW);
    let one = kumite_sim::ONE;
    lei_jabs(&mut b);
    let (f, t) = (&b.game.fight.fighters[0], &b.game.fight.teams.as_ref().unwrap()[0]);
    let max = f.max_health;
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
    let step = ((paul.max_health >> 4) / 6000) << 4; // (in the game's 20.12)
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
    assert_eq!((t.members[1].health, t.members[1].cap), (t.members[1].max_health, t.members[1].max_health));
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

/// Outside a tag battle the team intro does nothing: round 1's fly-in plays through the call (it starts 3700 nearer,
/// 0x3954E8).
#[test]
fn a_single_match_keeps_its_fly_in() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [XIAOYU, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, true);
    b.team_intro = true;
    b.step([Pad::default(); 2]);
    b.step([Pad::default(); 2]);
    assert!(b.phase == Phase::Call && b.game.camera.in_intro(), "{:?}", b.phase);
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
            seen.push((b.phase, b.game.camera.view.eye, b.game.fight.fighters[1].frame));
        }
        seen
    };
    let seen = run();
    // the intro motions (150 frames) from frame 60, their last 90 (0x368398 under the intro's flag; 0x447480's 255)
    if a.gamedata::<serde_json::Value>("camera.json").is_ok_and(|c| c["team_intro"]["start_frames"].is_array()) {
        assert_eq!(seen[0].2, 61, "{:?}", &seen[..3]);
    }
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
    let v = kumite_sim::camera::EndCam::winner(&d, &p, true, 0, &mut |_| 0).step(&d, &p);
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

/// A move with a per-fighter camera id (0x23-0x66; alias 5459, id 0x29 in every set: kind 3) starts its camera about
/// the other fighter (0x38DEA0: 17 frames' delay and 30 of blend (state 10), held to 30 frames left (8), 30 back (9))
/// and goes back to the fight camera; the same each run.
#[test]
fn a_move_has_its_per_fighter_camera() {
    let Some(a) = assets() else { return };
    let run = || {
        let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
        g.camera.data().director.as_ref()?;
        for _ in 0..60 {
            g.step([Pad::default(); 2]);
        }
        g.fight.fighters[0].play_alias(5459);
        let mut seen = Vec::new();
        for _ in 0..400 {
            g.step([Pad::default(); 2]);
            seen.push((g.camera.special(), g.camera.view));
        }
        Some(seen)
    };
    let Some(seen) = run() else { return };
    let states: Vec<u8> = seen.iter().map(|s| s.0).collect();
    if !states.iter().any(|&s| (7..=15).contains(&s)) {
        eprintln!("(a pack without the per-fighter cameras) {states:?}");
        return;
    }
    assert_eq!(states.last(), Some(&1), "{states:?}");
    assert_eq!(Some(seen), run());
}

/// Once the other fighter is down (+0x131) a mover's alias picks its row's camera (0x509820: Paul's alias 370, list
/// 0x12C); standing, none.
#[test]
fn a_finishing_move_has_its_camera_once_the_other_is_down() {
    let Some(a) = assets() else { return };
    let run = |down: bool| {
        let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
        g.camera.data().director.as_ref()?;
        if !a.gamedata::<serde_json::Value>("camera.json").is_ok_and(|c| c["fighter_cameras"]["rows"].is_array()) {
            return None;
        }
        for _ in 0..60 {
            g.step([Pad::default(); 2]);
        }
        if down {
            g.fight.fighters[1].health = 0;
        }
        g.fight.fighters[0].play_alias(370);
        let mut states = Vec::new();
        for _ in 0..3 {
            g.step([Pad::default(); 2]);
            states.push(g.camera.special());
        }
        Some(states)
    };
    let (Some(down), Some(up)) = (run(true), run(false)) else { return };
    assert!(down.iter().any(|s| (7..=15).contains(s)), "{down:?}");
    assert!(up.iter().all(|&s| s == 1), "{up:?}");
}

/// A row's shot is the down fighter's damage flags' (0x394460): Paul's alias 935 has a camera only when its K.O. was
/// a counter hit (+0x13A, the third shot); a plain K.O. takes the first, none.
#[test]
fn a_counter_hit_ko_takes_its_rows_third_shot() {
    let Some(a) = assets() else { return };
    let run = |counter: bool| {
        let mut g = Game::load(&a, [PAUL, LEI]).unwrap();
        g.camera.data().director.as_ref()?;
        if !a.gamedata::<serde_json::Value>("camera.json").is_ok_and(|c| c["fighter_cameras"]["rows"].is_array()) {
            return None;
        }
        for _ in 0..60 {
            g.step([Pad::default(); 2]);
        }
        let f = &mut g.fight.fighters;
        let along = (f[1].position - f[0].position).normalize();
        f[1].position = f[0].position + along * 900.0;
        f[1].health = kumite_sim::ONE;
        f[0].play_alias(935);
        // Lei's slow strike (active from frame 31): Paul's lands in its start-up; else Lei stands without guard
        if counter {
            f[1].play_alias(307);
        }
        f[1].guard_input = false;
        let mut states = Vec::new();
        for _ in 0..30 {
            g.step([Pad::default(); 2]);
            states.push(g.camera.special());
        }
        assert!(g.fight.fighters[1].health <= 0, "the K.O.");
        Some(states)
    };
    let (Some(counter), Some(plain)) = (run(true), run(false)) else { return };
    assert!(counter.iter().any(|s| (7..=15).contains(s)), "{counter:?}");
    assert!(plain.iter().all(|&s| s == 1), "{plain:?}");
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

/// The swaying nodes go on through the K.O. replay (0x34D258 each drawn frame, on the replayed poses): Paul's belt
/// and Lei's hair stay on their parents (held at the K.O.'s places, they streaked across the replay's shots).
#[test]
fn the_swaying_nodes_follow_the_ko_replay() {
    let Some(a) = assets() else { return };
    let mut b = tag_bout(&a, LAW);
    b.game.fight.fighters[0].health = kumite_sim::ONE;
    let (walk, jab) = (parse_script("f").unwrap()[0], parse_script("1").unwrap()[0]);
    let mut cloth: [kumite_game::Cloth; 2] = Default::default();
    let (mut k, mut replayed) = (0, 0);
    while !matches!(b.phase, Phase::Result { .. }) && k < 1200 {
        b.step(if b.phase == Phase::Fight { [walk, if k % 20 == 0 { jab } else { Pad::default() }] } else { [Pad::default(); 2] });
        for (i, c) in cloth.iter_mut().enumerate() {
            let character = &b.game.bodies[i].character;
            let (mut pose, place) = b.game.pose(i);
            c.apply(character, b.frames, place, &mut pose);
            for n in &character.cloth {
                let node = &character.nodes[n.node];
                let at = (pose[node.parent.unwrap()] * glam::Mat4::from_translation(node.translation)).w_axis;
                assert!((pose[n.node].w_axis - at).length() < 0.001, "step {k} ({:?}): fighter {i}'s node {} off its parent", b.phase, n.node);
            }
        }
        replayed += usize::from(matches!(b.phase, Phase::Replay { .. }));
        k += 1;
    }
    assert!(replayed > 30 && b.game.bodies.iter().all(|b| !b.character.cloth.is_empty()), "{replayed} replay steps");
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
    // (the calls' level measured in PCSX2: 0.77 of their volume byte 29's)
    let call = s.announce(kumite_game::bout::Announcement::Round(1), [true; 2]).unwrap();
    assert!((call.volume - 29.0 / 51.0 * 0.77).abs() < 1e-6, "{}", call.volume);
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
    // (the sound player each frame: an attack voice's cooldown runs out, 0x37608C)
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
        heard(&a, &s, &b, &mut st);
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

/// The win pose (0x3685B8, checked in PCSX2 holding square): Xiaoyu's school uniform (costume 4, model 444) never
/// does pose 0, her handstand, but pose 1 + the frame counter % 3 (frame % 3 == 2: 4773); her other costumes do.
#[test]
fn xiaoyus_uniform_never_does_the_handstand() {
    let Some(a) = assets() else { return };
    let game = Game::load(&a, [444, PAUL]).unwrap();
    let r = game.rounds.as_ref().unwrap();
    assert_eq!([0, 1, 2].map(|f| r.win_pose(0, 444, f)), [4771, 4772, 4773]);
    assert_eq!([1, 2, 3].map(|p| r.win_pose(p, 444, 0)), [4771, 4772, 4773]);
    for record in [438, 441, XIAOYU, 435, 447, 717, PAUL] {
        assert_eq!(r.win_pose(0, record, 2), 4770, "model {record}");
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

/// Two CPUs at NORMAL (the default difficulty), fight 1's level: they walk, attack, guard and land hits within 60
/// seconds, and the same match plays again the same.
#[test]
fn normal_cpus_walk_attack_guard_and_hit() {
    let Some(a) = assets() else { return };
    let r = cpu_run(&a, [Some((1, 0)); 2], 3600);
    eprintln!("{r:?}", r = (r.walks, r.attacks, r.guards, r.hits, r.wins));
    for k in 0..2 {
        assert!(r.walks[k] > 0 && r.attacks[k] > 0 && r.guards[k] > 0 && r.hits[k] > 0, "side {k}: {:?}", (r.walks, r.attacks, r.guards, r.hits));
    }
    assert_eq!(r.hashes, cpu_run(&a, [Some((1, 0)); 2], 3600).hashes, "the same match twice");
}

/// ULTRA HARD's top level beats a fighter who stands still, and attacks more often than EASY's first level (its
/// cooldowns average 63 frames against 350, FIGHT_DATA.md 21.5) and goes on into strings (P3A 1 at level 0).
/// Clean hits are no measure: an idle fighter turned round by a hit stays turned (no input), so whoever turns him
/// first hits his back for the rest of the round, and EASY's 221-frame rule (any entry after 220 frames in a move,
/// 0x385C80) keeps its attack rate far above what its cooldowns alone give.
#[test]
fn the_top_cpu_beats_a_standing_dummy() {
    let Some(a) = assets() else { return };
    let top = cpu_run_of(&a, [PAUL, LEI], [Some((4, 7)), None], 30000, 4);
    let easy = cpu_run(&a, [Some((0, 0)), None], 15000);
    for (n, r) in [("top", &top), ("easy", &easy)] {
        eprintln!("{n}: presses {:?} starts {:?} hits {:?} wins {:?} fight {} turned {} stats {:?}", r.attacks, r.starts, r.hits, r.wins, r.fight, r.turned, r.stats);
    }
    assert_eq!(top.wins[0], 4, "the top CPU wins the match");
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
    // (level 4 over a 4-round match: a short one can end before its first string)
    let (l0, l4) = (cpu_run(&a, [Some((1, 0)), None], 8000), cpu_run_of(&a, [PAUL, LEI], [Some((1, 4)), None], 20000, 4));
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
    let (normal, hard) = (cpu_run_of(&a, [KING, LEI], [Some((1, 7)), None], 40000, 9), cpu_run_of(&a, [KING, LEI], [Some((2, 0)), None], 15000, 2));
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

/// The wake-up waits out Ogre's fire breath within 2000 (0x3890D0, pack format 31), not his other moves.
#[test]
fn the_cpu_stays_down_under_the_fire_breath() {
    let Some(a) = assets() else { return };
    // (a pack older than format 31 has no `rules.wake_wait`: the wait is not there to test)
    let ai: serde_json::Value = serde_json::from_slice(&std::fs::read(a.root.join("gamedata/ai.json")).unwrap()).unwrap();
    if ai["rules"]["wake_wait"].is_null() {
        eprintln!("skipped: the pack has no rules.wake_wait (convert it again)");
        return;
    }
    let (breath, other) = (lying_against_ogre(&a, FIRE_BREATH), lying_against_ogre(&a, 422));
    eprintln!("frames down under the breath {breath}, under 422 {other}");
    assert!(breath >= 60 && other < 30, "waits under the breath only");
}

/// Tekken Tag's body push (0x377338): a fighter walking into one standing still carries it along (as PCSX2 shows a
/// lunge carrying a guarding Xiaoyu), the walker taking the whole push back.
#[test]
fn a_walk_carries_the_one_it_pushes() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [XIAOYU, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    let f = &mut b.game.fight.fighters;
    let along = (f[1].position - f[0].position).normalize();
    f[1].position = f[0].position + along * 700.0;
    f[1].push_prev = None;
    let (start, walk) = (b.game.fight.fighters[0].position, parse_script("b").unwrap()[0]);
    for _ in 0..60 {
        b.step([Pad::default(), walk]);
    }
    let f = &b.game.fight.fighters;
    let (carried, gap) = (f[0].position.distance(start), (f[1].root() - f[0].root()).length());
    eprintln!("carried {carried:.0}, gap {gap:.0}");
    assert!(carried > 300.0 && gap > 400.0, "carried {carried:.0}, gap {gap:.0}");
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
/// (PCSX2, the default NORMAL, matches against Xiaoyu: stage 1 level 0 about 190 still, 30-50 holding back), here a
/// cell's eight matches each; a seed replays the same. Stages 4 and 8 are the PS2's arcade with this test's teams
/// (Michelle and King written into ladder 0x440A00, the earlier fights won), from one save state with rand() reseeded
/// a match, 4600 frames each; a match's damage a minute over eight matches (sixteen at stage 8 holding back):
/// stage 4 still 248-580 (mean 402, sd 121), holding back 125-235 (179, 36); stage 8 still 162-396 (328, 86),
/// holding back 48-256 (164, 67). Here (40 seeds): 138-563 (391, 84), 81-290 (161, 47), 194-420 (283, 48),
/// 97-286 (185, 51), each mean within 1.1 of the PS2's standard errors. The bounds are the PS2's matches' range.
/// (Stage 1 holding back is still above the PS2's: Lei runs in and his tackle lands. Neither CPU holds a direction
/// in its own recoveries' END windows: 0 of 72 frames on the PS2, 0 of 70 here.)
#[test]
fn the_arcade_cpus_beat_a_still_fighter_as_the_ps2_does() {
    let Some(a) = assets() else { return };
    let seeds = || (0..8).map(|k| k * 11);
    let cells = [([LEI, NINA], 0), ([MICHELLE, KING], 3), ([UNKNOWN, UNKNOWN], 7)].map(|(cpu, level)| {
        let ((still, runs), (back, _)) = (arcade_cell(&a, cpu, level, false, seeds()), arcade_cell(&a, cpu, level, true, seeds()));
        (still, back, runs)
    });
    let dmg: Vec<(u32, u32)> = cells.iter().map(|c| (c.0, c.1)).collect();
    eprintln!("damage a minute, still / holding back: {dmg:?}");
    for (k, (still, back)) in [((120, 240), (40, 130)), ((248, 580), (125, 235)), ((162, 396), (48, 256))].into_iter().enumerate() {
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

/// Theater (all open by default) plays each cell's ending: every labelled character x costume with an ending loads
/// and runs to its end (Unknown's is a movie), and both EMBU casts load.
#[test]
fn theater_plays_every_ending() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let unknown = d.raw["playback"]["unknown"].as_u64();
    let mut played = 0;
    for c in 0..34 {
        for k in (0..8).filter(|&k| d.raw["theater"]["labels"][c][k].is_string()) {
            let Some(e) = d.ending(c, k).filter(|&e| Some(e as u64) != unknown) else { continue };
            let mut s = kumite_game::ending::EndingScene::new(&a, &d, e, k).unwrap_or_else(|err| panic!("character {c} costume {k}: {err:#}"));
            let mut f = 0;
            while !s.done && f < 20000 {
                s.step();
                f += 1;
            }
            assert!(s.done, "character {c} costume {k}: ending {e} ends");
            played += 1;
        }
    }
    assert!(played > 60, "{played}");
    assert!((0..2).all(|cast| kumite_game::ending::EndingScene::embu(&a, &d, cast).is_ok()));
}

/// The EMBU's camera as PCSX2 showed it at its counter 1100, one ahead of the view (our frame 1099): the view 0x415808's eye (-339, -2152,
/// -2338) game units, here turned half round about the arena (screen 11 turns the arena so: PCSX2 shows the pagoda
/// and the gate's steps on path 0 at counter 420, where the arena as it stands has the lion); and its people: none in
/// the first cast, set 30 in the second.
#[test]
fn the_embus_camera_is_the_ps2s() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let mut s = kumite_game::ending::EndingScene::embu(&a, &d, 0).unwrap();
    while s.frame() < 1099 {
        s.step();
    }
    let (eye, _, _) = s.camera();
    assert!((eye - glam::Vec3::new(0.339, 2.152, -2.338)).abs().max_element() < 0.002, "{eye}");
    assert!(s.people.is_none());
    assert_eq!(kumite_game::ending::EndingScene::embu(&a, &d, 1).unwrap().people.expect("the EMBU's people: a pack before converter format 40, convert the game again")["set"], 30);
}

/// Arena 4's lightning: the arena's stage plays its still channels 0-6 (the bolts' resting on their cleared
/// frame 1) and its weather has the lightning; arena 12's the signs. (The EMBU's op 7 flashes nothing: PCSX2's
/// channel records stay at zero on its screen.)
#[test]
fn arena_4s_lightning() {
    let Some(a) = assets() else { return };
    let st = a.stage(4).unwrap();
    let still: Vec<(Option<usize>, u32, usize)> = st.channels.iter().map(|c| (c.channel, c.start, c.frames)).collect();
    assert!((0..7).all(|k| still.iter().any(|&(c, _, _)| c == Some(k))), "arena 4's channels (convert the game again): {still:?}");
    assert!(still.iter().all(|&(c, start, n)| c.is_none_or(|k| (start, n) == if k < 4 { (1, 2) } else { (3, 4) })), "{still:?}");
    let stages: serde_json::Value = serde_json::from_slice(&std::fs::read(a.root.join("gamedata/stages.json")).unwrap()).unwrap();
    assert!(kumite_game::weather::Weather::new(&stages["stages"][4]["weather"]).is_some());
    assert!(kumite_game::weather::Weather::new(&stages["stages"][12]["weather"]).is_some());
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

/// The 2D objects ending scripts lay over the picture: ending 2's negative (op 24 kind 0, frames 753-850) and iris
/// (op 22 from frame 899: radius 640 to 0 over 120 frames, then held), ending 22's oval (op 23, 136 frames from 120).
#[test]
fn an_endings_overlays() {
    use kumite_game::ending::{EndingScene, Overlay};
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let at = |e: usize, frames: &[i32]| -> Vec<Vec<Overlay>> {
        let mut s = EndingScene::new(&a, &d, e, 0).unwrap();
        let mut out = vec![];
        while !s.done {
            s.step();
            if frames.contains(&s.frame()) {
                out.push(s.overlays());
            }
        }
        out
    };
    let negative = Overlay::Negative([255; 3]);
    let iris = |r: f32| Overlay::Iris { x: 320.0, y: 224.0, r };
    assert_eq!(at(2, &[752, 753, 850, 851, 899, 959, 989]), vec![vec![], vec![negative], vec![negative], vec![], vec![iris(640.0)], vec![iris(320.0)], vec![iris(160.0)]]);
    assert_eq!(at(22, &[119, 120, 255, 256]), vec![vec![], vec![Overlay::Oval], vec![Overlay::Oval], vec![]]);
}

/// Op 12: in ending 13 Jin (slot 1) shows his mark until Jun's touch at frame 510; his texture has the marked texels.
#[test]
fn jins_mark_in_ending_13() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let mut s = kumite_game::ending::EndingScene::new(&a, &d, 13, 0).unwrap();
    let mut marks = vec![];
    while s.frame() < 511 {
        s.step();
        marks.push(s.marks[1]);
    }
    assert!(marks[..510].iter().all(|&m| m) && !marks[510]);
    let jin = &s.characters[0];
    let [x, y, w, h, sx, sy] = jin.mark.expect("Jin's texture has his mark (convert the game again)");
    assert!(w > 0 && h > 0 && (x + w).max(sx + w) <= jin.texture.width && (y + h).max(sy + h) <= jin.texture.height);
}

/// The face track (converter format 68): in ending 28 Jin's mouth (node 23) moves with the track, its place more than
/// 5 mm up and down over the scene.
#[test]
fn jins_face_moves_in_ending_28() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let s = kumite_game::ending::EndingScene::new(&a, &d, 28, 0).unwrap();
    let jin = s.characters.iter().position(|c| c.nodes.get(23).is_some_and(|n| n.name == "node_23")).unwrap();
    let clip = s.characters[jin].clip("ending_28_1").unwrap();
    let Some(places) = s.characters[jin].clips[clip].translations[23].clone() else { return eprintln!("no face track: convert the game again") };
    let z: Vec<f32> = places.iter().map(|p| p.z).collect();
    let (lo, hi) = (z.iter().copied().fold(f32::MAX, f32::min), z.iter().copied().fold(f32::MIN, f32::max));
    assert!(hi - lo > 0.005, "{lo} {hi}");
}

/// Op 28: Devil's wings (model 663: nodes 103-108, 47 frames) open in ending 32 from frame 819.
#[test]
fn devils_wings_open_in_ending_32() {
    let Some(a) = assets() else { return };
    let devil = a.character(663).unwrap();
    let w = devil.wings.as_ref().expect("Devil's wings (convert the game again)");
    assert_eq!((w.nodes, w.frames.len()), ([103, 105, 107, 104, 106, 108], 47));
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let mut s = kumite_game::ending::EndingScene::new(&a, &d, 32, 0).unwrap();
    let mut tip = vec![];
    while s.frame() < 840 {
        s.step();
        if s.frame() >= 817 {
            let pose = s.pose(0).unwrap().1;
            tip.push((pose[2].inverse() * pose[107]).w_axis);
        }
    }
    // (the tip in the chest's frame: folded to 818, opening from 819)
    assert!((tip[0] - tip[1]).length() < 1e-5 && (tip[1] - tip[2]).length() > 1e-4, "{tip:?}");
}

/// The arenas' looping texture channels (0x3A6C80): arena 1's one channel of 60 frames at a quarter frame a frame;
/// arena 4's lightning stands still (none looping).
#[test]
fn arena_texture_channels() {
    let Some(a) = assets() else { return };
    let one = a.stage(1).unwrap();
    assert!(!one.channels.is_empty() && one.channels.iter().all(|c| (c.frames, c.speed, c.start) == (60, 0x40, 0)), "convert the game again");
    // (arena 4: only its still channels, which its lightning plays)
    assert!(a.stage(4).unwrap().channels.iter().all(|c| c.channel.is_some() && c.speed == 0));
}

/// Op 25: in ending 20 Bryan (slot 1) holds arena 5's object 0 at his hand (node 38) for 1080 frames.
#[test]
fn bryans_gun_in_ending_20() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let mut s = kumite_game::ending::EndingScene::new(&a, &d, 20, 0).unwrap();
    assert_eq!(s.prop_files(), vec![(0, "stages/stage_05_prop_0.glb".to_string())]);
    assert!(a.root.join("stages/stage_05_prop_0.glb").exists(), "convert the game again");
    while s.frame() < 300 {
        s.step();
    }
    let (_, pose, place) = s.pose(1).unwrap();
    assert_eq!(s.props(), vec![(0, place * pose[38])]);
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
        // (Paul's two balls and Lei's first: each waits for its pins to settle, a few hundred frames)
        for f in 0..3000u32 {
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

/// The fingers (hands.rs): by the fight both stances have set kind 2 (op 0x11), their characters' own shapes (pose
/// 17: Paul's fight fist, Xiaoyu's open palms), eased in from the model's open hands.
#[test]
fn fingers_take_the_poses() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return;
    };
    let a = Assets::new(std::path::Path::new(&root)).unwrap();
    let mut b = Bout::new(Game::load(&a, [PAUL, XIAOYU]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    for _ in 0..600 {
        if b.phase == Phase::Fight {
            break;
        }
        b.step([Pad::default(); 2]);
    }
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
    }
    let g = &b.game;
    let off = |i: usize, pose: usize| {
        let p = g.bodies[i].hands.as_ref().expect("the hands");
        let t = g.hands[i].turns.expect("drawn");
        (0..2).flat_map(|h| (0..15).map(move |k| (h, k))).map(|(h, k)| t[h][k].angle_between(p.poses[pose][h][k])).fold(0.0f32, f32::max)
    };
    for i in 0..2 {
        assert!(off(i, 17) < 0.01 && off(i, 0) > 0.5, "fighter {i}: {} off its shape, {} off open", off(i, 17), off(i, 0));
    }
}

/// Tekken Tag's look (0x351E98): in the stance the drawn head (node 6) and eyes face the opponent's neck; with Lei
/// moved 15° round Paul the head turns after her as far as it goes; the spheres keep the motion's head (drawn only).
#[test]
fn heads_and_eyes_follow_the_opponent() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [PAUL, LEI]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    // degrees between node `n`'s x axis (drawn or animated) and the line to the opponent's neck (node 5)
    let off = |g: &Game, i: usize, n: usize, drawn: bool| {
        let (pose, place) = if drawn { g.pose(i) } else { g.pose_animated_for_tests(i) };
        let (o, o_place) = g.pose_animated_for_tests(1 - i);
        let m = place * pose[n];
        let to = o_place.transform_point3(o[5].w_axis.truncate()) - m.w_axis.truncate();
        m.x_axis.truncate().normalize().dot(to.normalize()).clamp(-1.0, 1.0).acos().to_degrees()
    };
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
    }
    for i in 0..2 {
        let (anim, drawn, eye) = (off(&b.game, i, 6, false), off(&b.game, i, 6, true), off(&b.game, i, 21, true));
        eprintln!("P{}: head off the opponent {anim:.2} deg animated, {drawn:.2} drawn; eye {eye:.2}", i + 1);
        // (the eased look trails Lei's stance: PCSX2 draws her head 0.3°..5.4° off, 3.07° at this frame, stance 16 f61)
        let ps2 = [None, Some(3.07)][i];
        assert!(ps2.map_or(drawn < 3.0, |d: f32| (drawn - d).abs() < 0.1) && eye < 3.0, "P{} looks at the opponent", i + 1);
    }
    let f = &mut b.game.fight.fighters;
    let along = f[1].position - f[0].position;
    f[1].position = f[0].position + glam::Quat::from_rotation_y(15f32.to_radians()) * along;
    for _ in 0..60 {
        b.step([Pad::default(); 2]);
    }
    let (anim, drawn) = (off(&b.game, 0, 6, false), off(&b.game, 0, 6, true));
    eprintln!("Lei 15 deg round: Paul's head {anim:.1} deg off animated, {drawn:.1} drawn");
    // (Paul stands side-on: the neck and the head both reach their 40° and stop short)
    assert!(drawn < anim - 20.0, "the head follows");
    let (pose, place) = b.game.pose_animated_for_tests(0);
    let centres = |v: &kumite_sim::BodyVolumes| v.hurt.iter().map(|s| s.centre).collect::<Vec<_>>();
    assert_eq!(centres(&b.game.spheres()[0]), centres(&kumite_game::volumes(&b.game.bodies[0], &pose, place)), "the spheres keep the motion's head");
}

/// The Bowl hall's floor edge (ALPHA 0x44, faded by its vertices' alpha) is blended, not cut out black.
#[test]
fn bowl_hall_floor_edge_is_blended() {
    let Some(a) = assets() else { return };
    let hall = a.stage(kumite_game::bowl::ARENA).unwrap();
    assert!(hall.sky.iter().any(|m| m.blend == 3 && m.vertices.iter().any(|v| v.color[3] < 1.0)));
}

/// King's waterfall (arena 11), a mostly see-through picture on opaque vertices, is blended as the GS blends it, not
/// cut out solid white.
#[test]
fn a_see_through_picture_is_blended() {
    let Some(a) = assets() else { return };
    let king = a.stage(11).unwrap();
    assert!(king.sky.iter().any(|m| m.blend == 3 && m.vertices.iter().all(|v| v.color[3] >= 1.0)));
}

/// Eddy's grass (arena 10's floor cells) is drawn as the GS's alpha test leaves it: full texels with depth, the rest
/// blended (blend 4); the scenery's cut-outs stay cut.
#[test]
fn grass_is_cut_and_blended() {
    let Some(a) = assets() else { return };
    let eddy = a.stage(10).unwrap();
    assert!(eddy.meshes.iter().any(|m| m.blend == 4 && m.vertices.iter().all(|v| v.cell[3] > 0.0)));
    assert!(eddy.meshes.iter().chain(&eddy.sky).filter(|m| m.blend == 4).all(|m| m.vertices.iter().any(|v| v.cell[3] > 0.0)));
}

/// Eddy's ground (arenas 10, 13): its sand, overlay and painted shadow run on across the floor's cells, as a GS dump
/// has them (one map of the screen to s, t over every cell): where two cells meet, their texture coordinates agree
/// (the pictures repeat)
#[test]
fn eddys_ground_runs_on_across_the_cells() {
    let Some(a) = assets() else { return };
    for arena in [10, 13] {
        let st = a.stage(arena).unwrap();
        let mut at = std::collections::BTreeMap::new();
        for m in &st.meshes {
            for v in m.vertices.iter().filter(|v| v.cell[3] > 0.0 && v.cell[2] == 0.0) {
                let key = (m.texture, (v.pos[0] * 1000.0).round() as i32, (v.pos[2] * 1000.0).round() as i32);
                let uv = v.uv.map(|c| c.rem_euclid(1.0));
                let first = *at.entry(key).or_insert(uv);
                let off = |a: f32, b: f32| ((a - b).abs() - (a - b).abs().round()).abs();
                assert!(off(first[0], uv[0]) < 1e-3 && off(first[1], uv[1]) < 1e-3, "arena {arena}: a seam at {key:?}: {first:?} {uv:?}");
            }
        }
        assert!(at.len() > 100, "arena {arena}: {} ground points", at.len());
    }
}

/// Eddy's sky panels clamp their parts of the sheet (CLAMP REGION_CLAMP in a GS dump of arena 10), so bilinear
/// filtering draws no line of the far edge at a panel's edge; his floor's tiles repeat.
#[test]
fn eddys_sky_is_clamped_and_his_floor_repeats() {
    let Some(a) = assets() else { return };
    let st = a.stage(10).unwrap();
    assert!(st.sky.iter().any(|m| st.clamped[m.texture]), "no sky panel's picture is clamped");
    assert!(st.meshes.iter().any(|m| !st.clamped[m.texture] && st.smooth[m.texture]), "no picture repeats");
}

/// Op 16 in ending 35: Devil's type-2 beams run along his head's heading, 204 along and 220 down a frame; their ends
/// on the 13th frame where PCSX2 has them (the beam record 0x8A7AD0 + 0x50 read over PINE; mm, our axes).
#[test]
fn devils_beams_in_ending_35_run_as_on_the_ps2() {
    let Some(a) = assets() else { return };
    let d = kumite_game::ending::EndingData::load(&a.root.join("gamedata")).unwrap();
    let mut s = kumite_game::ending::EndingScene::new(&a, &d, 35, 0).unwrap();
    let mut ends = vec![];
    while s.frame() < 660 {
        s.step();
        if [233, 658].contains(&s.frame()) {
            ends.extend(s.beams.iter().filter_map(|b| b.3).map(|(p, q)| (p * 1000.0, q * 1000.0)));
        }
    }
    let want = [((-1642.0, 3831.0, -6188.0), (-1666.0, 1191.0, -3752.0)), ((-15.0, 4037.0, 9182.0), (9.0, 1397.0, 6746.0))];
    assert_eq!(ends.len(), 2, "{ends:?}");
    for ((p, q), (wp, wq)) in ends.iter().zip(want) {
        assert!(p.distance(glam::Vec3::from(wp)) < 150.0 && q.distance(glam::Vec3::from(wq)) < 150.0, "{p} {q}");
    }
}

/// The pair can part no farther than the PS2 lets it (0x371458): both walking back, they stop about 7.4 m apart and
/// stay there, each frame of the walk's cycle where PCSX2 has it (Paul and Hwoarang, `$W/distance`).
#[test]
fn the_pair_parts_no_farther_than_on_the_ps2() {
    let Some(a) = assets() else { return };
    let mut b = paul_hwoarang(&a);
    let (back, away) = (parse_script("b").unwrap()[0], parse_script("f").unwrap()[0]);
    // P1's walk frame: P1's and P2's root x, as PCSX2 shows them past the 240th frame
    let ps2 = [
        (1, -3594, 3830),
        (2, -3594, 3833),
        (3, -3602, 3841),
        (4, -3602, 3837),
        (5, -3602, 3845),
        (7, -3606, 3849),
        (10, -3606, 3849),
        (11, -3610, 3853),
        (13, -3610, 3849),
        (17, -3606, 3845),
        (19, -3602, 3837),
        (20, -3598, 3841),
        (21, -3598, 3833),
        (22, -3596, 3832),
    ];
    let mut seen = 0;
    for k in 0..600 {
        b.step([back, away]);
        let f = &b.game.fight.fighters;
        if k >= 240 {
            if let Some(&(_, x1, x2)) = ps2.iter().find(|p| p.0 == f[0].frame) {
                assert_eq!((f[0].root().x as i32, f[1].root().x as i32), (x1, x2), "step {k}, frame {}", f[0].frame);
                seen += 1;
            }
        }
    }
    assert!(seen > 200, "{seen}");
}

/// No arena's scenery stands between the fight camera and the fighters: the camera at the start distance and with the
/// pair at its limit, turned all the way round them (the arena follows the fighters without turning), every scenery
/// triangle that a line from the eye to either fighter crosses has a corner the fight's z guard cuts (`scenery_cut`;
/// arena 12 has none). Lights and painted shadows are not in the way.
#[test]
fn no_scenery_stands_before_the_fighters() {
    let Some(a) = assets() else { return };
    let mut b = paul_hwoarang(&a);
    let shot = |b: &Bout| {
        let f = &b.game.fight.fighters;
        let (eye, forward, _) = kumite_game::view_camera(&b.game.camera.view);
        (eye, forward, [f[0].root(), f[1].root()].map(|r| r / kumite_game::UNITS))
    };
    let mut shots = vec![shot(&b)];
    let (back, away) = (parse_script("b").unwrap()[0], parse_script("f").unwrap()[0]);
    for _ in 0..600 {
        b.step([back, away]);
    }
    shots.push(shot(&b));
    // (Möller-Trumbore: whether the segment from o along d meets the triangle short of its end)
    let meets = |o: glam::Vec3, d: glam::Vec3, t: [glam::Vec3; 3]| {
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            return false;
        }
        let s = o - t[0];
        let (u, q) = (s.dot(p) / det, s.cross(e1));
        let (v, k) = (d.dot(q) / det, e2.dot(q) / det);
        u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..0.97).contains(&k)
    };
    for arena in (0..23).filter(|&n| n != 12 && n != kumite_game::bowl::ARENA) {
        let st = kumite_assets::Stage::load(&a.root.join(format!("stages/stage_{arena:02}.glb"))).unwrap();
        // (what is near the fight and below the eye; shortcut: the objects the arena's code moves are left out, their
        // vertices are in their own axes)
        let tris: Vec<[glam::Vec3; 3]> = (st.sky.iter().filter(|m| m.blend == 0 && m.motion.is_none()))
            .flat_map(|m| m.indices.as_chunks::<3>().0.iter().map(move |t| t.map(|i| glam::Vec3::from(m.vertices[i as usize].pos))))
            .filter(|t| t.iter().any(|p| p.y < 2.5 && glam::Vec2::new(p.x, p.z).length() < 14.0))
            .collect();
        for &(eye, forward, roots) in &shots {
            let mid = (roots[0] + roots[1]) / 2.0;
            let floor = glam::Vec3::new(mid.x, 0.0, mid.z);
            for k in 0..120 {
                let turn = glam::Quat::from_rotation_y(k as f32 * std::f32::consts::TAU / 120.0);
                let at = |p: glam::Vec3| mid + turn * (p - mid);
                let (eye, forward, roots) = (at(eye), turn * forward, roots.map(at));
                let cut = kumite_game::scenery_cut(eye, forward, roots);
                let drawn = |t: &[glam::Vec3; 3]| t.iter().all(|p| (*p + floor - eye).dot(forward) >= cut);
                for r in roots {
                    for h in [0.4, 0.8, 1.2, 1.6] {
                        let to = glam::Vec3::new(r.x, h, r.z);
                        let hit = tris.iter().find(|t| drawn(t) && meets(eye - floor, to - eye, **t));
                        assert!(hit.is_none(), "arena {arena}, turned {}°, {:.1} m apart: {hit:?} before {to}", k * 3, roots[0].distance(roots[1]));
                    }
                }
            }
        }
    }
}

/// Paul and Law against Hwoarang and Paul at the fight's first frame (the PCSX2 runs' picks).
fn paul_hwoarang(a: &Assets) -> Bout {
    let mut g = Game::load(a, [PAUL, HWOARANG]).unwrap();
    g.start_tag([kumite_game::Actor::load(a, LAW).unwrap(), kumite_game::Actor::load(a, PAUL).unwrap()]);
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    b
}

/// An arena's textures are bilinear without mipmaps (TEX1 MXL 0; converter format 72).
#[test]
fn arena_textures_are_smooth_without_mipmaps() {
    let Some(a) = assets() else { return };
    let hall = a.stage(kumite_game::bowl::ARENA).unwrap();
    assert!(hall.smooth.iter().all(|s| *s) && !hall.filtered.iter().any(|f| *f));
}

/// Unknown's wolf (0x34CC38): its root eases to 150 mm over and 800 mm behind hers (her root's parent's axes, the
/// fighter's own) and stays there while she stands; her blades (0x34D108: Kunimitsu's, Yoshimitsu's) are not drawn.
#[test]
fn unknowns_wolf_stands_behind_her() {
    let Some(a) = assets() else { return };
    let mut b = Bout::new(Game::load(&a, [UNKNOWN, JULIA]).unwrap(), kumite_game::ai_data(&a).unwrap(), 2, false);
    while b.phase != Phase::Fight {
        b.step([Pad::default(); 2]);
    }
    for _ in 0..120 {
        b.step([Pad::default(); 2]);
    }
    let (pose, place) = b.game.pose(0);
    let want = place.transform_point3(pose[0].w_axis.truncate() + glam::Vec3::new(0.0, 0.15, -0.8));
    let wolf = place.transform_point3(pose[98].w_axis.truncate());
    assert!(wolf.distance(want) < 0.05, "the wolf at {wolf}, her mark {want}");
    assert_eq!(b.game.bodies[0].hidden, vec![116, 117, 118, 119]);
}

/// The arena's people are hidden nearer than the nearer fighter (0x3D9468), never nearer than 3 m.
#[test]
fn the_people_are_cut_at_the_nearer_fighter() {
    use glam::Vec3;
    let (eye, forward) = (Vec3::new(0.0, 1.0, 0.0), Vec3::NEG_Z);
    assert_eq!(kumite_game::people_cut(eye, forward, [Vec3::new(-3.0, 0.0, -12.0), Vec3::new(3.0, 0.0, -9.0)]), 9.0);
    assert_eq!(kumite_game::people_cut(eye, forward, [Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, -2.0)]), 3.0);
}
