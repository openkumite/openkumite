//! Tekken Ball against the PS1 game (Tekken 3 USA in PCSX-Redux: the ball struct *0x800ADD74 and the fighters' roots
//! P+0xF68 traced each frame of a VS Ball match, Gon against Jin, both pads scripted). Needs Tekken 3's game data
//! (OPENKUMITE_ASSETS); skips without.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

mod ball_doubles;

use kumite_assets::Assets;
use kumite_game::bout::Bout;
use kumite_game::{parse_script, Game};
use kumite_sim::input::Pad;
use std::path::PathBuf;

const GON: usize = 239;
const JIN: usize = 143;

fn assets() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

/// One frame of the ball: state, position, velocity, gravity.
type Frame = (u8, [i32; 3], [i32; 3], i32);

/// A Ball match of Gon (P1) against Jin, P2 serving (as the PS1's first round of VS Ball), the pads' scripts from the
/// round's first frame; the ball each frame, `frames` of them.
fn trace(a: &Assets, p1: &str, p2: &str, frames: usize) -> Vec<Frame> {
    trace_of(a, [GON, JIN], 1, p1, p2, frames)
}

/// A Ball match of `models`, `server` serving.
fn trace_of(a: &Assets, models: [usize; 2], server: usize, p1: &str, p2: &str, frames: usize) -> Vec<Frame> {
    roots_of(a, models, server, p1, p2, frames).into_iter().map(|(f, _)| f).collect()
}

/// The fighters' roots as the PS1 keeps them (P+0xF68: x, height down, z negated).
type Roots = [[i32; 3]; 2];

/// A Ball match of `models`, `server` serving, the pads' scripts from the round's first frame: `each` sees the bout
/// after each of `frames` steps.
fn ball_match(a: &Assets, models: [usize; 2], server: usize, p1: &str, p2: &str, frames: usize, mut each: impl FnMut(usize, &Bout)) {
    let mut g = Game::load(a, models).unwrap();
    let data = std::sync::Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap());
    g.ball = Some(kumite_game::BallSetup { data, kind: 0, server, rounds: 0 });
    g.round_frames = u32::MAX / 2;
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    let (s1, s2) = (parse_script(p1).unwrap(), parse_script(p2).unwrap());
    for k in 0..frames {
        let pad = |s: &[Pad]| s.get(k).copied().unwrap_or_default();
        b.step([pad(&s1), pad(&s2)]);
        each(k, &b);
    }
}

/// A Ball match of `models`, the ball and the roots each frame.
fn roots_of(a: &Assets, models: [usize; 2], server: usize, p1: &str, p2: &str, frames: usize) -> Vec<(Frame, Roots)> {
    let mut out = Vec::new();
    ball_match(a, models, server, p1, p2, frames, |k, b| {
        let ball = b.game.fight.ball.as_ref().unwrap();
        if std::env::var_os("BALL_TRACE").is_some() {
            let r = |i: usize| b.game.fight.fighters[i].root();
            let q = |i: usize| b.game.fight.fighters[i].position;
            let m = |i: usize| b.game.fight.fighters[i].current_move(&b.game.fight.data).anim.clone();
            eprintln!(
                "{k} st={} pos={},{},{} vel={},{},{} g={} p1={:.0},{:.0},{:.0} p2={:.0},{:.0},{:.0} q1={:.0},{:.0} q2={:.0},{:.0} m1={}@{} m2={}@{} speed={} heading={} power={} charge={:?} cool={}",
                ball.state,
                ball.pos[0],
                ball.pos[1],
                ball.pos[2],
                ball.vel[0],
                ball.vel[1],
                ball.vel[2],
                ball.g,
                r(0).x,
                -r(0).y - b.game.fight.fighters[0].anim_root_y,
                -r(0).z,
                r(1).x,
                -r(1).y - b.game.fight.fighters[1].anim_root_y,
                -r(1).z,
                q(0).x,
                -q(0).z,
                q(1).x,
                -q(1).z,
                m(0),
                b.game.fight.fighters[0].frame,
                m(1),
                b.game.fight.fighters[1].frame,
                ball.speed,
                ball.heading,
                ball.power,
                ball.charge,
                ball.cooldown
            );
            // the facings in the PS1's angle (+0x2C, +0xE: half a turn less OpenKumite's yaw) and the health
            let a = |y: f32| (0x8000 - (y / std::f32::consts::TAU * 65536.0).round() as i32) as i16;
            let f = &b.game.fight.fighters;
            eprintln!("{k} ang a1={},{},{} a2={},{},{}", a(f[0].yaw), a(f[0].root_yaw), f[0].health, a(f[1].yaw), a(f[1].root_yaw), f[1].health);
        }
        let root = |f: &kumite_sim::Fighter| [f.root().x, -f.root().y - f.anim_root_y, -f.root().z].map(|v| v.round() as i32);
        out.push(((ball.state, ball.pos, ball.vel, ball.g), b.game.fight.fighters.each_ref().map(root)));
    });
    out
}

/// Prints the trace (BALL_TRACE=1, BALL_P1 / BALL_P2 the pads' scripts, BALL_FRAMES; BALL_MODELS=a,b and BALL_SERVER
/// for other fighters than Gon and Jin, P2 serving).
#[test]
fn print_trace() {
    let Some(a) = assets() else { return };
    if std::env::var_os("BALL_TRACE").is_none() {
        return;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let models = env("BALL_MODELS").split(',').filter_map(|m| m.parse().ok()).collect::<Vec<usize>>().try_into().unwrap_or([GON, JIN]);
    trace_of(&a, models, env("BALL_SERVER").parse().unwrap_or(1), &env("BALL_P1"), &env("BALL_P2"), env("BALL_FRAMES").parse().unwrap_or(1800));
}

/// The PS1 trace's rows (the ball as the step before left it) are our steps `row + 3` (its first hover frame, row
/// 128, is our step 131).
const PS1_ROW: usize = 3;

/// Jin walks up to the served ball and punches it (2): it flies to Gon's side, lands for a point and Gon takes the
/// forced hit. The PS1's ball at its rows, from the serve's hover through the point: the punch meets the ball a step
/// after its active frame (P+0x1AC is read in the next step's hook); in round 1 Ball starts the fighters in the later
/// rounds' move, not an intro (0x8002D31C, battle type 7).
#[test]
fn jins_punch_on_the_serve_flies_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = trace(&a, "n*507", "n*145,b*15,n+2*2,n*30,n+2*2,n*30,n+1*2,n*281", 340);
    let ps1: [(usize, Frame); 7] = [
        (128, (9, [3328, -1280, 0], [0, 0, 0], 512)),
        (167, (2, [3282, -1106, 0], [-11776, -18432, 0], 512)),
        (200, (2, [1820, -2426, 0], [-10720, -1536, 0], 512)),
        (251, (2, [-172, -474, 0], [-9088, -10752, 0], 512)),
        (296, (2, [-1660, -384, 0], [-7648, 12288, 0], 512)),
        (322, (7, [-2377, -384, 0], [0, 512, 0], 512)),
        (330, (6, [-2377, -384, 0], [0, 512, 0], 512)),
    ];
    for (row, want) in ps1 {
        assert_eq!(t[row + PS1_ROW], want, "PS1 row {row}");
    }
}

/// Jin walks up to the served ball and kicks (4): the ball bumps off his body each step before the kick meets it,
/// its speed each time from the fighters' distance two steps back (P1+0xF8), as on the PS1.
#[test]
fn the_ball_bumps_off_jin_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = trace(&a, "n*468", "n*146,b*14,n+4*2,n*306", 180);
    let ps1: [(usize, Frame); 6] = [
        (167, (2, [3243, -1126, 0], [-21728, -23544, 0], 512)),
        (169, (2, [3074, -1312, 0], [-21466, -23101, 0], 512)),
        (172, (2, [2822, -1584, 0], [-21497, -22314, 0], 512)),
        (174, (2, [2654, -1760, 0], [-21348, -21712, 0], 512)),
        // no bump in the step the kick's strike comes out (+0xE3 set); the kick meets it the step after
        (175, (2, [2570, -1845, 0], [-21316, -21200, 0], 512)),
        (176, (2, [2520, -1919, 0], [-12609, -18432, 0], 512)),
    ];
    for (row, want) in ps1 {
        assert_eq!(t[row + PS1_ROW], want, "PS1 row {row}");
    }
}

/// Jin jumps forward (u/b) and kicks the served ball in the air: his jump drifts as far as on the PS1 (the drift's
/// sine is the 4096 table times 8, 0x8002A194: it went 8 times short).
#[test]
fn jins_jump_kick_flies_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = trace(&a, "n*468", "n*146,b*10,u/b*3,n*8,n+3*2,n*300", 200);
    let ps1: [(usize, Frame); 2] = [(184, (2, [3163, -1214, 0], [-14080, -3368, 0], 256)), (186, (2, [3053, -1238, 0], [-14080, -1471, 0], 256))];
    for (row, want) in ps1 {
        assert_eq!(t[row + PS1_ROW], want, "PS1 row {row}");
    }
}

/// Jin's uppercut (d/b+2) eases its root height in from the walk's over the frames to its active one and out to the
/// stance's over its last 16 (Tekken 3's motion blend, 0x8003C270, on the root at 0x8003AB78): Jin's root as the PS1's
/// (it went up to 117 units high).
#[test]
fn jins_uppercut_eases_its_root_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = roots_of(&a, [GON, JIN], 1, "n*469", "n*146,b*14,d/b,d/b+2*2,n*306", 220);
    let ps1 = [(159, [4646, -965, 6]), (162, [4394, -918, 22]), (165, [4158, -874, 40]), (201, [4298, -1048, 2]), (204, [4312, -1035, 0])];
    for (row, want) in ps1 {
        assert_eq!(t[row + PS1_ROW].1[1], want, "PS1 row {row}");
    }
}

/// Jin's low kick drops the ball on Gon's side: the point's forced hit throws Gon up as a juggle (the air table's
/// alias 0xD56 and its pop), and the ball comes back at his place (it came back at his clip's root offset).
#[test]
fn the_points_forced_hit_juggles_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = trace(&a, "n*468", "n*146,b*14,n+3*2,n*307", 340);
    assert_eq!(t[324 + PS1_ROW], (1, [0, -3648, 2], [0, 0, 0], 256), "PS1 row 324");
}

/// Doctor B. (P1, serving, as VS Ball from the 1P select) against Gon: his hit aims at Gon's root two steps back. The
/// PS1 trace's rows are our steps `row - 2` here.
#[test]
fn doctor_bs_hit_aims_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let p1 = "n*203,f*20,n+1*2,n*20,f*10,n+4*2,n*30,u/f*3,n*10,n+3*2,n*100";
    let p2 = "n*234,b*25,n+1*2,n*25,n+4*2,n*30,b*10,n+2*2,n*30,u/b*3,n*100";
    let t = trace_of(&a, [247, 239], 0, p1, p2, 290);
    assert_eq!(t[267 - 2], (2, [-3300, -1305, -3], [7168, -18432, -23], 512), "PS1 row 267");
}

/// 2 ON 2 (OpenKumite's own): a partner that loses a point takes the forced hit as the fighters do, thrown up as a
/// juggle.
#[test]
fn a_partner_losing_a_point_is_juggled() {
    let Some(a) = assets() else { return };
    let mut g = Game::load(&a, [GON, JIN]).unwrap();
    let data = std::sync::Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap());
    g.ball = Some(kumite_game::BallSetup { data, kind: 0, server: 1, rounds: 0 });
    g.ball_team = Some([kumite_game::Actor::load(&a, 71).unwrap(), kumite_game::Actor::load(&a, 95).unwrap()]);
    g.round_frames = u32::MAX / 2;
    let mut b = Bout::new(g, kumite_game::ai_data(&a).unwrap(), 2, false);
    let mut juggled = 0;
    for _ in 0..20000 {
        let before = [2, 3].map(|i| b.game.fight.actor(i).map_or(0, |f| f.health));
        b.step([Pad::default(); 2]);
        for (k, i) in [2, 3].into_iter().enumerate() {
            if let Some(f) = b.game.fight.actor(i).filter(|f| f.health < before[k] && b.game.fight.ball.as_ref().is_some_and(|x| x.state == 7)) {
                assert!(f.air.is_some(), "partner {i} lost a point standing");
                juggled += 1;
            }
        }
        if juggled > 0 {
            return;
        }
    }
    panic!("no partner lost a point");
}

/// Ball 2 ON 2's rules (OpenKumite's own; [`ball_doubles::rules`]) in Tekken 3: Gon and model 95 against Jin and model 71.
#[test]
fn ball_2_on_2_only_the_ball_hurts_and_a_side_falls_together() {
    let Some(a) = assets() else { return };
    ball_doubles::rules(&a, [GON, JIN], [95, 71], None);
}

/// Doctor B.'s hit on the ball makes the ball's actor slot, put on his own root, his target that step: the target
/// facing comes out of ratan2(0, 0), so motion_145 (committed on transition 0x18) runs its root 45° off the line to Gon,
/// as on the PS1 (it ran along the line). PS1 rows are our steps `row - 2`.
#[test]
fn doctor_bs_motion_145_runs_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let p1 = "n*203,f*20,n+1*2,n*20,f*10,n+4*2,n*30,u/f*3,n*10,n+3*2,n*100";
    let p2 = "n*234,b*25,n+1*2,n*25,n+4*2,n*30,b*10,n+2*2,n*30,u/b*3,n*100";
    let t = roots_of(&a, [247, 239], 0, p1, p2, 300);
    for (row, want) in [(290, [-4153, -377, 48]), (296, [-4040, -641, 161]), (300, [-3943, -791, 256])] {
        assert_eq!(t[row - 2].1[0], want, "PS1 row {row}");
    }
}

/// The round start of King (106 %) and Kuma (90 %) against Paul (P1 serving; PS1 rows are our steps `row - 1`): their
/// roots scaled by the model's size in the game's integers (raw · ⌊size · 4096 / 100⌋ >> 12, 0x80038970) and turned
/// on its sine table, each sum cut toward zero (0x8003AAF4), as on the PS1 (they were a unit off as floats).
#[test]
fn king_and_kumas_roots_are_the_ps1s_integers() {
    let Some(a) = assets() else { return };
    let king = roots_of(&a, [127, 95], 0, "n*20", "n*20", 20);
    for (row, want) in [(1, [5111, -1137, -2]), (6, [5060, -1101, -29]), (10, [5024, -1031, -48]), (16, [4990, -956, -52])] {
        assert_eq!(king[row - 1].1[1], want, "King, PS1 row {row}");
    }
    let kuma = roots_of(&a, [127, 159], 0, "n*10", "n*10", 10);
    for (row, want) in [(1, [5120, -899, 0]), (3, [5120, -897, 0]), (8, [5120, -890, 0])] {
        assert_eq!(kuma[row - 1].1[1], want, "Kuma, PS1 row {row}");
    }
}

/// Doctor B.'s side roll (common_037, tracking mode 9) at the court's corner: the roll's basis turns toward where the
/// opponent's root was at the commit (+0xAC, 0x8002F600) and the origin is rebased as it turns (+0xBE, 0x80040070), at
/// the next step's root update (0x8003A9C8), so the court holds the root at z 256 as on the PS1 (it swung back to 0).
/// PS1 rows are our steps `row - 1`.
#[test]
fn doctor_bs_side_roll_holds_the_court_edge_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = roots_of(&a, [127, 247], 0, "n*600", "n*285,u/b*3,n*60,u/b*19,n*70,u/f*19,n*69,u*20,n*67", 600);
    for (row, want) in [(526, [5120, -136, 2]), (530, [5120, -229, 89]), (533, [5120, -254, 184]), (536, [5119, -256, 256]), (537, [5119, -258, 256])] {
        assert_eq!(t[row - 1].1[1], want, "PS1 row {row}");
    }
    for row in 535..=575 {
        assert_eq!(t[row - 1].1[1][2], 256, "PS1 row {row}");
    }
}

/// Tekken 3's facings are its 16-bit integers (0x8003F890; atan 0x8002A224 on its own table): Doctor B.'s side roll
/// turns its basis (+0xE) as the PS1's step by step, toward where Paul's root was at the commit, from the roots before
/// the step, so the roll's x along the court's edge is the PS1's (it ran up to 5 units off as floats, then 10 with the
/// roots after the step). PS1 rows are our steps `row - 1`.
#[test]
fn doctor_bs_roll_turns_in_the_ps1s_integers() {
    let Some(a) = assets() else { return };
    let mut basis = Vec::new();
    let mut x = Vec::new();
    ball_match(&a, [127, 247], 0, "n*600", "n*285,u/b*3,n*60,u/b*19,n*70,u/f*19,n*69,u*20,n*67", 580, |_, b| {
        let f = &b.game.fight.fighters[1];
        // (the PS1's angle is half a turn less OpenKumite's yaw)
        basis.push((0x8000 - (f.root_yaw / std::f32::consts::TAU * 65536.0).round() as i32) as i16);
        x.push(f.root().x as i32);
    });
    for (row, want) in [(527, -16380), (530, -16375), (534, -16421), (540, -16543), (546, -16559), (558, -16559)] {
        assert_eq!(basis[row - 1], want, "PS1 row {row}");
    }
    for (row, want) in [(538, 5119), (542, 5115), (550, 5110), (558, 5106), (566, 5101), (574, 5099)] {
        assert_eq!(x[row - 1], want, "PS1 row {row}");
    }
}

/// A jump's drift runs along the basis (+0xE, 0x8003F3F8) at the game's own angle's top 12 bits: Kuma's long u/b jump
/// (common_046) lands where the PS1's does (its z was up to 6 off, the drift turned at the yaw's bits). PS1 rows are our
/// steps `row - 1`.
#[test]
fn kumas_long_jump_drifts_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = roots_of(&a, [127, 159], 0, "n*400", "n*250,u/b*3,n*60,u/b*20,n*70", 400);
    for (row, want) in [(340, [2967, -1800, 4]), (346, [2621, -1453, 5]), (352, [2332, -894, 6]), (359, [2332, -825, 6])] {
        assert_eq!(t[row - 1].1[1], want, "PS1 row {row}");
    }
}

/// Tekken 3's pose ease turns the joints too (0x8003C270's deltas at P+0x15F0, added at the weight at 0x8003B0A0, made
/// orthonormal again at 0x80075FC8): Jin's (P2, against Paul) is on at the PS1's steps, and his chest and forearm are
/// the PS1's (P+0x1390 + 32 j), each entry: the turns are the pose solver's of the frame's channels (they were within
/// 16 and 40 of it from the converted pose, the clip's own 565 and 1800 off). PS1 rows are our steps `row - 1`.
#[test]
fn jins_joints_ease_between_moves_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let mut eased = Vec::new();
    ball_match(&a, [127, JIN], 0, "n*300", "n*191,u/b*3,n*60,u/b*20,n*70", 300, |_, b| eased.push(b.game.fight.fighters[1].eased().map(|e| e.0)));
    let on = [112..=128, 192..=198, 221..=240, 255..=258, 289..=300];
    for row in 100..=300 {
        assert_eq!(eased[row - 1].is_some(), on.iter().any(|r| r.contains(&row)), "PS1 row {row}");
    }
    let ps1 = [
        (195, 1, [-80, -1872, 3647, -3213, 2291, 1106, -2542, -2837, -1514]),
        (195, 5, [-1792, 3691, 0, -3685, -1796, 0, 0, 0, 4106]),
        (229, 1, [-180, -2157, 3488, -2918, 2515, 1409, -2878, -2416, -1648]),
    ];
    for (row, slot, want) in ps1 {
        assert_eq!(eased[row - 1].unwrap()[slot], want, "PS1 row {row}, slot {slot}");
    }
}
