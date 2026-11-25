//! Tekken Ball against the PS1 game (Tekken 3 USA in PCSX-Redux: the ball struct *0x800ADD74 and the fighters' roots
//! P+0xF68 traced each frame of a VS Ball match, Gon against Jin, both pads scripted). Needs Tekken 3's game data
//! (OPENKUMITE_ASSETS); skips without.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

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
    let mut g = Game::load(a, [GON, JIN]).unwrap();
    let data = std::sync::Arc::new(kumite_sim::data::load(&a.root.join("gamedata/ball.json")).unwrap());
    g.ball = Some(kumite_game::BallSetup { data, kind: 0, server: 1, rounds: 0 });
    g.round_frames = u32::MAX / 2;
    let mut b = Bout::new(g, kumite_game::ai_data(a).unwrap(), 2, false);
    let (s1, s2) = (parse_script(p1).unwrap(), parse_script(p2).unwrap());
    let mut out = Vec::new();
    for k in 0..frames {
        let pad = |s: &[Pad]| s.get(k).copied().unwrap_or_default();
        b.step([pad(&s1), pad(&s2)]);
        let ball = b.game.fight.ball.as_ref().unwrap();
        if std::env::var_os("BALL_TRACE").is_some() {
            let r = |i: usize| b.game.fight.fighters[i].root();
            let q = |i: usize| b.game.fight.fighters[i].position;
            let m = |i: usize| b.game.fight.fighters[i].current_move(&b.game.fight.data).anim.clone();
            eprintln!(
                "{k} st={} pos={},{},{} vel={},{},{} g={} p1={:.0},{:.0} p2={:.0},{:.0} q1={:.0},{:.0} q2={:.0},{:.0} m1={}@{} m2={}@{} speed={} heading={} power={} charge={:?} cool={}",
                ball.state,
                ball.pos[0],
                ball.pos[1],
                ball.pos[2],
                ball.vel[0],
                ball.vel[1],
                ball.vel[2],
                ball.g,
                r(0).x,
                -r(0).z,
                r(1).x,
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
        }
        out.push((ball.state, ball.pos, ball.vel, ball.g));
    }
    out
}

/// Prints the trace (BALL_TRACE=1, BALL_P1 / BALL_P2 the pads' scripts, BALL_FRAMES).
#[test]
fn print_trace() {
    let Some(a) = assets() else { return };
    if std::env::var_os("BALL_TRACE").is_none() {
        return;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    trace(&a, &env("BALL_P1"), &env("BALL_P2"), env("BALL_FRAMES").parse().unwrap_or(1800));
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
    let ps1: [(usize, Frame); 4] = [
        (167, (2, [3243, -1126, 0], [-21728, -23544, 0], 512)),
        (169, (2, [3074, -1312, 0], [-21466, -23101, 0], 512)),
        (172, (2, [2822, -1584, 0], [-21497, -22314, 0], 512)),
        (174, (2, [2654, -1760, 0], [-21348, -21712, 0], 512)),
    ];
    for (row, want) in ps1 {
        assert_eq!(t[row + PS1_ROW], want, "PS1 row {row}");
    }
}
