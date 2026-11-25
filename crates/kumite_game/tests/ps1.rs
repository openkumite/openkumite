//! Tekken 3's VS fights against the PS1 game (Tekken 3 USA in PCSX-Redux, both pads scripted, both fighters' roots
//! P+0xF68, facings and health traced each fight step at the facing driver 0x8003F098). Needs Tekken 3's game data
//! (OPENKUMITE_ASSETS); skips without.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::{parse_script, Game};
use std::path::PathBuf;

fn assets() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

/// A fight's rows: each step both fighters' root as the PS1 keeps it (x, height down, z negated), facing and basis
/// in the PS1's angle (half a turn less OpenKumite's yaw), health (16.16), move and frame.
type Row = [(i32, i32, i32, i16, i16, i32, String); 2];

/// A VS fight of `models` from the round's first fight step, the pads' scripts from there, `steps` of them.
/// `lead`: steps P1's start move runs ahead of P2's (the PS1's round 2 starts them a step apart).
fn rows(a: &Assets, models: [usize; 2], lead: u32, p1: &str, p2: &str, steps: usize) -> Vec<Row> {
    let mut g = Game::load(a, models).unwrap();
    // (the PS1's VS LIFE level 3, "100 %", is the life table's 140, 0x800225EC; round 1's intro ends on a commit facing
    // the opponent, type 0: P2 at the atan of the start marks, 0xBFFF)
    g.max_health = [Some(140); 2];
    g.reset_round();
    // (round 2: the later rounds' start move, as the bout's call puts them)
    g.set_start_moves(g.fight.fighters.each_ref().map(|f| f.moveset.fixed.round_start[2]));
    for f in &mut g.fight.fighters {
        f.frame = 1;
    }
    let f = &mut g.fight.fighters[1];
    f.yaw = (0x8000 - 0xBFFF) as f32 * std::f32::consts::TAU / 65536.0;
    f.root_yaw = f.yaw;
    g.fight.fighters[0].frame += lead;
    let (s1, s2) = (parse_script(p1).unwrap(), parse_script(p2).unwrap());
    let angle = |y: f32| (0x8000 - (y / std::f32::consts::TAU * 65536.0).round() as i32) as i16;
    (0..steps)
        .map(|k| {
            g.step([s1.get(k).copied().unwrap_or_default(), s2.get(k).copied().unwrap_or_default()]);
            std::array::from_fn(|i| {
                let f = &g.fight.fighters[i];
                let r = f.root();
                let m = format!("{}@{}", f.current_move(&g.fight.data).anim, f.frame);
                (r.x.round() as i32, (-r.y - f.anim_root_y).round() as i32, (-r.z).round() as i32, angle(f.yaw), angle(f.root_yaw), f.health, m)
            })
        })
        .collect()
}

/// Prints the rows (VS_TRACE=1; VS_MODELS=a,b, VS_P1 / VS_P2 the pads' scripts, VS_STEPS) as "step vs p1=x,y,z,2c,e,hp
/// m1=move@frame p2=...".
#[test]
fn print_vs_trace() {
    let Some(a) = assets() else { return };
    if std::env::var_os("VS_TRACE").is_none() {
        return;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let models = env("VS_MODELS").split(',').filter_map(|m| m.parse().ok()).collect::<Vec<usize>>().try_into().unwrap_or([71, 79]);
    for (k, row) in rows(&a, models, env("VS_LEAD").parse().unwrap_or(0), &env("VS_P1"), &env("VS_P2"), env("VS_STEPS").parse().unwrap_or(600)).iter().enumerate() {
        let p = |i: usize| {
            let (x, y, z, f, e, hp, m) = &row[i];
            format!("p{}={x},{y},{z},{f},{e},{hp} m{}={m}", i + 1, i + 1)
        };
        eprintln!("{k} vs {} {}", p(0), p(1));
    }
}

/// The PS1's rows of a case recorded in round 2 (vsync 1700 on): its step `row` is our step `row - first - 2` (the
/// trace reads the roots before the step's update; P2 starts its round a step after P1, its pad a step later).
fn at(rows: &[Row], first: usize, row: usize, i: usize) -> &(i32, i32, i32, i16, i16, i32, String) {
    &rows[row - first - 2][i]
}

/// Lei (P1) taps up near the end of the later rounds' start move (240 frames from frame 1, 0x8002D3DC; it kept the
/// last 150): its END window is open from frame 60, so the stance's sidestep is taken there without a new press
/// (0x8002DB74), and the sidesteps go round Julia as on the PS1's rows (it waited for the move's end).
#[test]
fn lei_sidesteps_out_of_the_round_start_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [87, 151], 1, "n*196,u*2,n*40,d*2,n*40,u*2,n*6,u*2,n*40,d*30,n*96", "n*457", 300);
    for (row, want) in [(724, (-956, -953, -708, 15932)), (728, (-992, -956, -548, 14381)), (733, (-976, -816, -58, 18170)), (740, (-759, -828, 425, 22761))] {
        let r = at(&t, 525, row, 0);
        assert_eq!((r.0, r.1, r.2, r.3), want, "PS1 row {row}");
    }
}

/// King's d/f+2 over and over on Jin walking in: Jin's health as the PS1's hit by hit down to the K.O. (13 a hit; one
/// came a step late without the END window open).
#[test]
fn king_knocks_out_jin_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    // (the PS1's presses every 27 vsyncs, on its steps)
    let p1 = "n*178,f*20,n+2*2,n*25,n+2*2,n*25,n+2*2,n*24,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*22";
    let t = rows(&a, [95, 143], 1, p1, "n*179,b*556", 720);
    for (row, want) in [(882, 127), (994, 88), (1127, 49), (1260, 10), (1316, 0)] {
        assert_eq!(at(&t, 673, row, 1).5 >> 16, want, "PS1 row {row}");
    }
}

/// Paul runs into Law: the push (0x80046D34) shares the spheres' deepest overlap by how far each moved since the last
/// step, each backing off along its basis, half a step: Paul's z stays within 10 of the PS1's (the sphere push swung it
/// 124 off). The rest is the spheres' places: the converted joints are up to 75 units off the PS1's slots.
#[test]
fn paul_runs_into_law_and_is_pushed_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [71, 79], 1, "n*202,f*57,n*28,b*60,n*30,f*150,n*120", "n*398,b*90,n*160", 440);
    for (row, z) in [(840, 92), (860, 232), (900, 249), (940, 250)] {
        let r = at(&t, 599, row, 0);
        assert!((r.2 - z).abs() <= 10, "PS1 row {row}: z {} against {z}", r.2);
    }
}
