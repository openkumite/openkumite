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
    // (the stagger's commit sets +0xDA, 0x8002F900, which its hold step's ease sees: the end-of-move ease left from
    // the reaction before stops there; its height ran 1 to 2 off)
    for (row, want) in [(883, (514, -1042, -90)), (886, (923, -1034, -71)), (890, (1073, -1024, -63)), (894, (1112, -1020, -51))] {
        let r = at(&t, 673, row, 1);
        assert_eq!((r.0, r.1, r.2), want, "PS1 row {row}");
    }
    assert_eq!(at(&t, 673, 883, 1).1, -1042, "PS1 row 883: frame 1's height");
    // King's d/f+2 again from the END look-ahead (+0x8B 3) eases in from the pose shown, as a new motion (+0xDA): his
    // height was 41 off
    let h: Vec<i32> = (902..=909).map(|row| at(&t, 673, row, 0).1).collect();
    assert_eq!(h, [-1158, -1163, -1172, -1177, -1182, -1181, -1179, -1179]);
    // the collapse stays where it fell, 3954 on the PS1 (a few units off by then; it slid 729 on)
    let ko = [1321, 1339].map(|row| at(&t, 673, row, 1).0);
    assert!(ko[0] == ko[1] && (ko[0] - 3954).abs() < 10, "{ko:?}");
}

/// Doctor B. against Gon: Gon's walk, pushed, commits itself again each step (condition 0x33), which keeps +0xDA, the
/// new motion, on (0x8002F4A8 is not reached): his end-of-move ease starts over each step and his facing tracks as
/// from frame 1 (+0x5C, 0x80030ED0). Doctor B.'s crawl (descriptor byte 0x1C) lets out his mist (object 10,
/// 0x800729FC), which strikes Gon, and Gon plays Doctor B.'s reaction at his own size (45 %, 0x80038970). Ours missed
/// (1894 off).
#[test]
fn doctor_bs_mist_strikes_gon_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [251, 243], None, 1, "n*155,f*40,n+2*2,n*30,d+4*2,n*40,b+3*2,n*144", "n*156,b*40,n+4*2,n*40,n+1+2*2,n*176", 260);
    for (row, want) in [(917, (439, -16416)), (920, (500, -16414)), (922, (521, -16412))] {
        let r = at(&t, 739, row, 1);
        assert_eq!((r.0, r.3), want, "PS1 row {row}");
    }
    assert_eq!(at(&t, 739, 941, 1).5 >> 16, 137, "PS1 row 941: the mist's 3");
    let r = at(&t, 739, 942, 1);
    assert_eq!((r.0, r.1), (391, -467), "PS1 row 942");
}

/// Kuma's 1+2 throws Heihachi into Kuma's reaction: Heihachi plays it at his own size (98 %, the raw root times
/// P+0x4EC, 0x80038970), not Kuma's (90 %): his height was 81 off.
#[test]
fn a_borrowed_reaction_plays_at_the_victims_size_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [159, 179], None, 1, "n*152,f*39,n+2*2,n*20,n+2+4*2,n*60,d/f+1*2,n*114", "n*153,b*39,d+1*2,n*198", 240);
    for (row, want) in [(804, (190, -990)), (805, (391, -986)), (806, (494, -984))] {
        let r = at(&t, 601, row, 1);
        assert_eq!((r.0, r.1), want, "PS1 row {row}");
    }
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

/// King's crouching throw (d+1+3) on Nina crouching: the throw links on the step its attempt makes contact (+0xD1,
/// 0x800442C8; the link 0x80030E78 before the next facing pass), King facing from the push's angle to her turned by
/// the quadrant (0x800306D4) and Nina turned round where she stands (0x80030858). Ours linked a step later, King 1064
/// off and Nina facing away.
#[test]
fn king_links_his_crouching_throw_on_contact_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [99, 115], None, 1, "n*254,f*60,n+1+3*2,n*40,d+1+3*2,n*136", "n*285,d*200,n*10", 380);
    let r = at(&t, 622, 994, 0);
    assert_eq!((r.0, r.2, r.3), (790, 99, 17439), "PS1 row 994: King");
    let r = at(&t, 622, 994, 1);
    assert_eq!((r.0, r.2, r.3), (1944, -19, 16383), "PS1 row 994: Nina");
}

/// Gun Jack blocks Xiaoyu's 1, 2 and Hwoarang takes Eddy's counter hit: the reaction's basis is the target +0x34 on
/// the roots as the step leaves them (0x800309E4), the attacker's own root, not the one the victim saw before the
/// root update: it was 16 and 32 off (and in 18 more of the 60 cases).
#[test]
fn a_block_and_a_counter_turn_their_basis_on_the_attacker_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [131, 211], None, 1, "n*198,f*60,n+1*2,n*10,n+2*2,n*30,n+4*2,n*94", "n*399", 270);
    assert_eq!(at(&t, 632, 899, 1).4, -15649, "PS1 row 899: the block");
    let t = rows(&a, [139, 123], None, 1, "n*246,f*37,n+1*2,n*86", "n*247,b*33,n*8,n+4*2,n*82", 300);
    assert_eq!(at(&t, 598, 892, 1).4, -11825, "PS1 row 892: the counter hit");
}

/// Law's throw (1+3) on Heihachi, linked on contact: the link commits in the next step's move logic, so that step's
/// ease sees +0xDA (0x80030E78) and Law's height eases in from the pose shown over 4 frames (it was 48 off).
#[test]
fn a_throw_linked_on_contact_eases_in_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [83, 179], None, 1, "n*265,f*60,n+1+3*2,n*135", "n*463", 350);
    let h: Vec<i32> = (801..=804).map(|row| at(&t, 462, row, 0).1).collect();
    assert_eq!(h, [-988, -1004, -1020, -1034]);
}

/// Eddy's d/f+4 links from his 3 at the END with no net turn: the link sets the basis to the facing all the same
/// (0x8002F9F0, +0xC1 without +0x12); it kept the 3's, 3060 off.
#[test]
fn an_end_link_without_a_turn_sets_the_basis_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [139, 131], None, 1, "n*219,f*40,d+4*2,n*20,n+4*2,n*20,n+3*2,n*19,d/f+4*2,n*127", "n*220,b*40,n+2*2,n*192", 320);
    assert_eq!((at(&t, 561, 862, 0).3, at(&t, 561, 862, 0).4), (26269, 26411), "PS1 row 862");
}

/// Lei's sidestep (transition 0x1C) 22 frames after his first, from an entry of a shared sub-list: the get-up lock
/// (+0x128, 24 frames) skips the sub-list's entries too (one walk tests both, 0x8002E024), so he stays on his facing;
/// he turned 2447 off it.
#[test]
fn the_get_up_lock_skips_a_sub_lists_entries_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [131, 91], None, 1, "n*156,f*40,n+2*2,n*10,n+4*2,n*30,b+4*2,n*30,d/f+2*2,n*162", "n*197,u*2,n*20,u*2,n*20,d*2,n*20,d*2,n*172", 300);
    let r = at(&t, 663, 930, 1);
    assert_eq!((r.0, r.1, r.2, r.3), (1722, -994, 344, -11203), "PS1 row 930");
}

/// Paul's 1+3 throws King: the victim plays Paul's motion at Paul's size (P+0x4EE from the thrower's +0x4EC while
/// +0x74 < 0, 0x8003890C), not King's 106 %: his height was 65 off. Thrown down, King's END link out of it turns him
/// by its +0x12 the PS1's way round (0x8002F9F0): his facing was 1454 off.
#[test]
fn a_throws_victim_plays_at_the_throwers_size_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [75, 99], None, 1, "n*357,f*60,n+1+3*2,n*80,n+2+4*2,n*80,f+1+3*2,n*134", "n*420,n+2*2,n*4,n+2*2,n*80,n+1*2,n*4,n+1*2,n*202", 650);
    let r = at(&t, 549, 1077, 1);
    assert_eq!((r.0, r.1, r.2), (1192, -1086, 43), "PS1 row 1077");
    assert_eq!(at(&t, 549, 1182, 1).3, -15810, "PS1 row 1182");
}

/// Nina's 2+4 throws Paul down: his stun timer starts on the step the PS1's move logic links him (0x80030910), so the
/// drained pool lets him roll over (common_034, cond 0x31) on the PS1's frame; it came a step early.
#[test]
fn a_throws_victim_gets_up_on_the_ps1s_frame() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [115, 75], None, 1, "n*199,f*60,n+2+4*2,n*157", "n*419", 400);
    let r = at(&t, 637, 1034, 1);
    assert_eq!((r.1, r.3), (-157, -16386), "PS1 row 1034");
}

/// Anna's d+1 hits Law in his u/b+4: in the flight his drift (+0x9A, +0x9E) is the integer one found the frame before
/// (0x8003F268, the speed +0x98 by the sine away from her), and on the reaction's hold step the flight comes after
/// the root and the ease as on any step. He drifted 21 units a frame from the hit's step, 8 too high on the hold.
#[test]
fn a_juggled_fighter_drifts_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [83, 235], None, 1, "n*242,f*40,u/b+4*2,n*40,d/b+4*2,n*40,f/f+4*2,n*173", "n*243,b*40,d+1*2,n*30,n+2*2,n*225", 330);
    assert_eq!(at(&t, 475, 768, 0).1, -1198, "PS1 row 768");
    assert_eq!(at(&t, 475, 787, 0).0 - at(&t, 475, 771, 0).0, -320, "PS1 rows 771..787");
}

/// Ogre's d/f+2 from afar: Ogre and Mokujin (character 14, 15) strike by the descriptor's alternate set (fmt bits
/// 4..14, 0x8006A3BC's mode 1), which falls short of Julia; the primary one reached her and she blocked, slid 500 off.
#[test]
fn ogre_strikes_by_the_alternate_set_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [187, 155], None, 1, "n*174,f*40,d/f+2*2,n*40,b+1*2,n*40,n+2+3*2,n*174", "n*175,b*40,d+2*2,n*30,n+1*2,n*226", 330);
    let r = at(&t, 552, 866, 1);
    assert_eq!((r.0, r.1, r.2), (1114, -936, 53), "PS1 row 866");
}

/// Lei's 2 on Eddy in his handstand while the bodies push: the strike is built before the push (0x80041F6C, the push
/// 0x8003EC64), 25 units back, and whiffs; built after it, it hit Eddy.
#[test]
fn a_strike_is_placed_before_the_push_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [139, 91], None, 1, "n*228,f*39,d+3+4,n*30,n+3*2,n*30,d+3+4*2,n*30,n+4*2,n*162", "n*229,b*39,n+2,n*258", 300);
    let r = at(&t, 601, 885, 0);
    assert_eq!((r.0, r.1, r.2, r.5), (-264, -817, 1398, 9175040), "PS1 row 885");
}

/// Heihachi juggles Yoshimitsu, still sliding at 10 a frame from the hit before: the air hit's pushback (no constant
/// part) leaves that slide (+0x106 frames at +0x108, 0x8003F798) running out in its own direction, on the sine table,
/// through the flight. Ours stopped it, 9 short a frame (971 off by the landing).
#[test]
fn an_air_hit_keeps_the_slide_going_on_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [179, 107], None, 1, "n*151,f*40,d/f+2*2,n*40,f+1*2,n*40,d+4*2,n*172", "n*152,b*40,n+3*2,n*30,u+1*2,n*224", 300);
    let r = at(&t, 455, 706, 1);
    assert_eq!((r.0, r.1, r.2), (1334, -2170, 0), "PS1 row 706");
}

/// Eddy's u+3 whiffs Lei: once landed past its active frames the jumping attack stops turning (0x8003FD80: by its
/// damage +0x5E, not by a contact). Ours kept re-aiming, 563 off; then stopped a step early (the PS1's landing step,
/// a frame past the jump window, still turns), 25 off.
#[test]
fn a_landed_jump_attack_stops_turning_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [139, 123], None, 1, "n*246,f*37,d+4*2,n*40,u+3*2,n*40,f+1*2,n*174", "n*247,b*37,n+2*2,n*30,d+4*2,n*226", 400);
    let f: Vec<i16> = [958, 959, 965, 975, 985].map(|row| at(&t, 598, row, 0).3).to_vec();
    assert_eq!(f, [20395, 20370, 20370, 20370, 20370]);
}

/// Yoshimitsu's u+2 jumps toward Gun Jack (link 0x10 sets +0x82, 0x8002FF40): from the jump window's first frame the
/// PS1 moves him the pair distance less 500 over the frames left (0x8003F104), 17 a frame; Gun Jack's hit ends it
/// (0x8002F5F4). Ours kept that to Tekken Tag and stood still, 51 short.
#[test]
fn a_jump_goes_toward_the_opponent_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [107, 99], None, 1, "n*235,f*40,u+2*2,n*40,d/f+3*2,n*40,n+1+3*2,n*174", "n*236,b*40,n+4*2,n*30,n+2*2,n*226", 400);
    for (row, want) in [(977, (-820, -1154, 23)), (979, (-786, -1389, 23)), (1030, (-2428, -249, 22))] {
        let r = at(&t, 691, row, 0);
        assert_eq!((r.0, r.1, r.2), want, "PS1 row {row}");
    }
}

/// Heihachi's d+4 on Yoshimitsu falling from a juggle: an airborne move (state 9) counts as lying from its jump
/// window's first frame (+0x60 | 4, 0x800409A0), so the kick's frame-1 cancel on a lying opponent (cond 0x35) takes
/// the ground kick (motion_073). Ours read the move's posture alone and kept the standing kick, 426 off.
#[test]
fn an_airborne_opponent_counts_as_lying_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [179, 107], None, 1, "n*151,f*40,d/f+2*2,n*40,f+1*2,n*40,d+4*2,n*172", "n*152,b*40,n+3*2,n*30,u+1*2,n*224", 300);
    for (row, want) in [(736, (-329, -999, -65)), (746, (173, -1106, -101))] {
        let r = at(&t, 455, row, 0);
        assert_eq!((r.0, r.1, r.2), want, "PS1 row {row}");
    }
}

/// Anna, hit out of her u/f+4 by Yoshimitsu's f+2: the flight's basis is the angle to him (+0x34, 0x80030B88) from his
/// root as he struck, not the one he had at the step's start. Ours flew 176 off the line.
#[test]
fn an_air_hit_flies_along_the_line_to_the_striker_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [235, 171], None, 1, "n*186,f*36,u/f+4*2,n*40,d/b+2*2,n*40,n+1+4*2,n*174", "n*187,b*36,n+4*2,n*30,f+2*2,n*226", 300);
    for (row, want) in [(759, (-872, 18, 17775)), (771, (-1336, 63, 17775))] {
        let r = at(&t, 522, row, 0);
        assert_eq!((r.0, r.2, r.4), want, "PS1 row {row}");
    }
}

/// Gun Jack blocks Xiaoyu's 2 and then her 4 in the same block motion: the hit pass's +0x8B 1 (0x80045394) makes the
/// commit set +0xDA (0x8002F910) with the motion the same, which ends the ease toward the end of the first block.
/// Ours went on easing over the second one, 94 too high.
#[test]
fn a_second_block_ends_the_ease_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [131, 211], None, 1, "n*198,f*60,n+1*2,n*10,n+2*2,n*30,n+4*2,n*94", "n*399", 300);
    for (row, want) in [(916, -942), (922, -942), (929, -978)] {
        assert_eq!(at(&t, 632, row, 1).1, want, "PS1 row {row}");
    }
}

/// Law, juggled by Paul, is hit again in the air: the flight's next step still takes the drift found the frame
/// before (+0x9A / +0x9E, 0x8003F268) on top of the new pop's. Ours started the new flight from none, 28 short.
#[test]
fn an_air_hit_keeps_the_drift_found_the_frame_before_as_on_the_ps1() {
    let Some(a) = assets() else { return };
    let t = rows(&a, [83, 211], None, 1, "n*149,f*40,u+3*2,n*40,d+1*2,n*40,f/f+4*2,n*173", "n*150,b*40,d+2*2,n*30,n+1*2,n*225", 300);
    for (row, want) in [(894, (-1967, -678, -562)), (905, (-2388, -1426, -588))] {
        let r = at(&t, 655, row, 0);
        assert_eq!((r.0, r.1, r.2), want, "PS1 row {row}");
    }
}
