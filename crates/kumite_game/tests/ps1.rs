//! Tekken 3's VS fights against the PS1 game (Tekken 3 USA in PCSX-Redux, both pads scripted, both fighters' roots
//! P+0xF68, facings and health traced each fight step at the facing driver 0x8003F098). Needs Tekken 3's game data
//! (OPENKUMITE_ASSETS); skips without. The fighters are their second costumes' models, as the PS1's VS select gave
//! them (× picks costume 1, 0x8010DAB8): a few of them stand on joints a unit off the first costume's.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::{parse_script, Game};
use std::path::PathBuf;

fn assets() -> Option<Assets> {
    let root = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    Assets::new(&root).map_err(|e| eprintln!("skipping: {e:#}")).ok()
}

/// A fight's rows: each step both fighters' root as the PS1 keeps it (x, height down, z negated), facing and basis
/// in the PS1's angle (half a turn less OpenKumite's yaw), health (16.16), move and frame, the push spheres' centres
/// as the PS1 keeps them (P+0x324).
type Row = [(i32, i32, i32, i16, i16, i32, String, Vec<[i32; 3]>); 2];

/// A VS fight of `models` from the round's first fight step, the pads' scripts from there, `steps` of them.
/// `lead`: steps P1's start move runs ahead of P2's (the PS1's round 2 starts them a step apart); `copy`: the copying
/// character P1's draw for the round (its pool's entry).
fn rows(a: &Assets, models: [usize; 2], copy: Option<usize>, lead: u32, p1: &str, p2: &str, steps: usize) -> Vec<Row> {
    let mut g = Game::load(a, models).unwrap();
    // (the PS1's VS LIFE level 3, "100 %", is the life table's 140, 0x800225EC; round 1's intro ends on a commit facing
    // the opponent, type 0: P2 at the atan of the start marks, 0xBFFF)
    g.max_health = [Some(140); 2];
    if let Some(k) = copy {
        g.copy_as(0, k);
    }
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
                let push = g.spheres()[i].push.iter().map(|s| [s.centre.x, -s.centre.y, -s.centre.z].map(|v| v.round() as i32)).collect();
                (r.x.round() as i32, (-r.y - f.anim_root_y).round() as i32, (-r.z).round() as i32, angle(f.yaw), angle(f.root_yaw), f.health, m, push)
            })
        })
        .collect()
}

/// Prints the rows (VS_TRACE=1; VS_MODELS=a,b, VS_P1 / VS_P2 the pads' scripts, VS_STEPS, VS_COPY P1's copy draw) as
/// "step vs p1=x,y,z,2c,e,hp m1=move@frame p2=...", with VS_PUSH=1 each push sphere's centre after the step
/// ("s10=x,y,z ...").
#[test]
fn print_vs_trace() {
    let Some(a) = assets() else { return };
    if std::env::var_os("VS_TRACE").is_none() {
        return;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let models = env("VS_MODELS").split(',').filter_map(|m| m.parse().ok()).collect::<Vec<usize>>().try_into().unwrap_or([71, 79]);
    for (k, row) in rows(&a, models, env("VS_COPY").parse().ok(), env("VS_LEAD").parse().unwrap_or(0), &env("VS_P1"), &env("VS_P2"), env("VS_STEPS").parse().unwrap_or(600)).iter().enumerate() {
        let p = |i: usize| {
            let (x, y, z, f, e, hp, m, push) = &row[i];
            let mut s = format!("p{}={x},{y},{z},{f},{e},{hp} m{}={m}", i + 1, i + 1);
            if std::env::var_os("VS_PUSH").is_some() {
                for (k, c) in push.iter().enumerate() {
                    s += &format!(" s{}{k}={},{},{}", i + 1, c[0], c[1], c[2]);
                }
            }
            s
        };
        eprintln!("{k} vs {} {}", p(0), p(1));
    }
}

/// The PS1's rows of a case recorded in round 2 (vsync 1700 on): its step `row` is our step `row - first - 2` (the
/// trace reads the roots before the step's update; P2 starts its round a step after P1, its pad a step later).
fn at(rows: &[Row], first: usize, row: usize, i: usize) -> &(i32, i32, i32, i16, i16, i32, String, Vec<[i32; 3]>) {
    &rows[row - first - 2][i]
}

/// Lei (P1) taps up near the end of the later rounds' start move (240 frames from frame 1, 0x8002D3DC; it kept the
/// last 150): its END window is open from frame 60, so the stance's sidestep is taken there without a new press
/// (0x8002DB74), and the sidesteps go round Julia as on the PS1's rows (it waited for the move's end).
#[test]
fn lei_sidesteps_out_of_the_round_start_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [91, 155], None, 1, "n*196,u*2,n*40,d*2,n*40,u*2,n*6,u*2,n*40,d*30,n*96", "n*457", 300);
    for (row, want) in [(724, (-956, -953, -708, 15932)), (728, (-992, -956, -548, 14381)), (733, (-976, -816, -58, 18170)), (740, (-759, -828, 425, 22761))] {
        let r = at(&t, 525, row, 0);
        assert_eq!((r.0, r.1, r.2, r.3), want, "PS1 row {row}");
    }
}

/// King's d/f+2 over and over on Jin walking in: Jin's health as the PS1's hit by hit down to the K.O. (13 a hit; one
/// came a step late without the END window open), and his root. Each stagger's commit step (no hold at hit-stop 0 or
/// 1, +0x76, 0x8002F4A4) has the reaction's frame 1 root, pose and pushback (the K.O. chain was 729 off); the
/// pushback steps on the sine table (+0x10C, 0x8003F820); the K.O.'s collapse does not slide (0x8002D82C).
#[test]
fn king_knocks_out_jin_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    // (the PS1's presses every 27 vsyncs, on its steps)
    let p1 = "n*178,f*20,n+2*2,n*25,n+2*2,n*25,n+2*2,n*24,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*25,n+2*2,n*22";
    let t = rows(&a, [99, 147], None, 1, p1, "n*179,b*556", 720);
    for (row, want) in [(882, 127), (994, 88), (1127, 49), (1260, 10), (1316, 0)] {
        assert_eq!(at(&t, 673, row, 1).5 >> 16, want, "PS1 row {row}");
    }
    // (the stagger's height later runs a unit off)
    for (row, want) in [(883, (514, -90)), (886, (923, -71)), (890, (1073, -63))] {
        let r = at(&t, 673, row, 1);
        assert_eq!((r.0, r.2), want, "PS1 row {row}");
    }
    assert_eq!(at(&t, 673, 883, 1).1, -1042, "PS1 row 883: frame 1's height");
    // the collapse stays where it fell, 3954 on the PS1 (a few units off by then; it slid 729 on)
    let ko = [1321, 1339].map(|row| at(&t, 673, row, 1).0);
    assert!(ko[0] == ko[1] && (ko[0] - 3954).abs() < 10, "{ko:?}");
}

/// Julia's d/f+2 launches Nina and her jabs juggle her: Nina's root as the PS1's through the reactions (the launcher's
/// reaction came a step late, 197 off).
#[test]
fn julia_juggles_nina_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [155, 115], None, 1, "n*141,f*40,d/f+2*2,n*16,n+1*2,n*10,n+1*2,n*10,n+2*2,n*136", "n*142,b*40,n*180", 360);
    for (row, want) in [(753, (938, -994, 10)), (756, (1265, -987, 26)), (790, (1728, -989, 54)), (820, (1875, -1017, 62))] {
        let r = at(&t, 554, row, 1);
        assert_eq!((r.0, r.1, r.2), want, "PS1 row {row}");
    }
}

/// Mokujin (P1) stands while Ogre walks in, jabs and kicks low: with the PS1's draw for round 2 (Kuma's moves, the
/// pool's entry 11; rand() on the PS1, 0x8004F2DC) both roots are the PS1's. (Mokujin's height runs 2 or 3 off: the
/// PS1 scales Kuma's motion root by Mokujin's size, the converted clips have Kuma's.)
#[test]
fn mokujin_as_kuma_stands_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [195, 187], Some(11), 1, "n*287", "n*128,b*40,n+2*2,n*30,d+4*2,n*86", 280);
    for (row, want) in [(640, [(-1000, 0), (1000, 0)]), (760, [(-1000, 0), (638, 0)]), (790, [(-1152, 0), (-338, 0)])] {
        assert_eq!([0, 1].map(|i| (at(&t, 622, row, i).0, at(&t, 622, row, i).2)), want, "PS1 row {row}");
    }
}

/// Xiaoyu and Jin walk away from each other for ten seconds: past 0x1D00 apart each goes back to where it stood
/// (0x800430FC), so they stop 7434 apart as on the PS1 (they walked on 5000 off).
#[test]
fn walking_apart_stops_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [131, 147], None, 1, "n*154,b*599,n*60,f*2,n*4,f*2,n*80,n+2*2,n*50", "n*155,f*599,n*60,b*2,n*4,b*2,n*132", 480);
    for (row, want) in [(994, [-3698, 3736]), (1000, [-3698, 3736]), (1100, [-3701, 3736])] {
        assert_eq!([0, 1].map(|i| at(&t, 630, row, i).0), want, "PS1 row {row}");
    }
}

/// Gon walks into Paul: his walk's f entry on the body push's contact (condition 0x33, +0xD5 from the push vector,
/// 0x80043EA8) turns it into his shove at frame 14, as on the PS1 (ours walked on, 406 off).
#[test]
fn gon_shoves_paul_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [243, 75], None, 1, "n*223,f*40,n+1+2*2,n*217", "n*224,b*60,n*199", 260);
    for (row, want) in [(862, [-443, 510]), (870, [-404, 417]), (880, [-552, 368])] {
        assert_eq!([0, 1].map(|i| at(&t, 624, row, i).0), want, "PS1 row {row}");
    }
}

/// Yoshimitsu's 1+2 strikes Eddy: Eddy's reaction turns to him from its commit step on, the facing pass running in
/// the hold as in any step (0x8003F098; it started a step late, 62 off).
#[test]
fn a_reaction_turns_from_its_commit_step_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [107, 139], None, 1, "n*174,f*38,n+1+2*2,n*60,b*2,n*4,b*2,n*60,f+2*2,n*88", "n*175,b*38,d+4*2,n*218", 260);
    for (row, want) in [(865, -16135), (868, -15632)] {
        assert_eq!(at(&t, 643, row, 1).3, want, "PS1 row {row}");
    }
}

/// Paul runs into Law: the push (0x80046D34) shares the spheres' deepest overlap by how far each moved since the last
/// step, each backing off along its basis, half a step, the spheres on the slots in the PS1's integers as this step's
/// pose puts them: Paul's root is the PS1's (the sphere push swung z 124 off, the converted joints still 9 and x 51).
#[test]
fn paul_runs_into_law_and_is_pushed_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [75, 83], None, 1, "n*202,f*57,n*28,b*60,n*30,f*150,n*120", "n*398,b*90,n*160", 440);
    for (row, want) in [(840, (141, 92)), (860, (400, 232)), (900, (202, 249)), (940, (-316, 250))] {
        let r = at(&t, 599, row, 0);
        assert_eq!((r.0, r.2), want, "PS1 row {row}");
    }
}

/// King dashes at Nina standing (her stand eases toward its loop's first frame): every push sphere of both as the
/// PS1 places it (P+0x324, 0x80041F6C): the slots from the frame's channels by the pose solver (0x8003AD10, two-bone
/// limbs on its own skeleton), eased (0x8003B0A0) and chained (0x8003AFCC) in the GTE's integers. The converted
/// joints were up to 136 off.
#[test]
fn push_spheres_sit_on_the_ps1s_slots() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [99, 115], None, 1, "n*254,f*2,n*4,f*2,n*40,b*2,n*4,b*2,n*60,f*2,n*4,f*2,n*175", "n*554", 300);
    let ps1: [(usize, [[[i32; 3]; 8]; 2]); 3] = [
        (
            735,
            [
                [[-800, -1512, -94], [-465, -1278, 216], [-791, -1285, -422], [-944, -1101, -41], [-558, -605, -83], [-973, -518, -323], [-564, -181, 113], [-1206, -167, -120]],
                [[1016, -1375, 0], [625, -1180, -110], [938, -1043, 226], [1000, -996, 0], [548, -534, 53], [918, -413, 271], [548, -131, -177], [1241, -132, 96]],
            ],
        ),
        (
            878,
            [
                [[-613, -1413, -43], [-275, -1174, 246], [-585, -1157, -352], [-741, -997, 3], [-316, -551, -41], [-721, -448, -348], [-402, -124, 125], [-1066, -196, -162]],
                [[1016, -1354, 0], [622, -1172, -116], [928, -1035, 223], [1000, -975, 0], [553, -515, 74], [907, -396, 270], [550, -125, -178], [1242, -128, 96]],
            ],
        ),
        (
            886,
            [
                [[-191, -1477, 1], [156, -1267, 210], [-154, -1233, -322], [-364, -1075, 3], [87, -660, 121], [-451, -493, -294], [146, -197, 180], [-847, -254, -226]],
                [[1016, -1379, 0], [626, -1183, -108], [943, -1046, 225], [1000, -1000, 0], [549, -538, 49], [921, -418, 271], [550, -134, -177], [1241, -136, 95]],
            ],
        ),
    ];
    // (the PS1's row shows the spheres of the step before it, as at())
    for (row, want) in ps1 {
        for (i, w) in want.iter().enumerate() {
            assert_eq!(at(&t, 622, row, i).7, w.to_vec(), "PS1 row {row}, P{}", i + 1);
        }
    }
}
