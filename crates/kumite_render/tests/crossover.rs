//! The crossover's first question: do Tekken 3's skeletons and Tekken Tag's take each other's motions? The 18
//! animated joints share their names and their parents (Tekken Tag puts an unanimated `node_2` between the chest and
//! the clavicles and head, and animates twist helpers `<joint>.1`, `.2` beside them), the joints stand where the other
//! game's do, but each joint's local axes differ by a fixed turn ([`turn`]): a motion played by name alone bends
//! every limb the wrong way. Rendered offscreen to `t3_bodies_ttt_motion.png` and `ttt_body_t3_motion.png` in the
//! test's temporary folder. On a Tekken 3 game root (OPENKUMITE_ASSETS), a Tekken Tag one (OK_TTT_ASSETS) and a GPU;
//! skipped without either.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use glam::{Mat4, Quat, Vec3};
use kumite_assets::{Assets, Character, Clip};
use kumite_render::{Camera, Renderer, SkinVertex};
use std::f32::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;

const BONES: [&str; 18] = [
    "root",
    "chest",
    "pelvis",
    "thigh_A",
    "shin_A",
    "foot_A",
    "thigh_B",
    "shin_B",
    "foot_B",
    "clavicle_A",
    "upperarm_A",
    "forearm_A",
    "hand_A",
    "clavicle_B",
    "upperarm_B",
    "forearm_B",
    "hand_B",
    "head",
];

/// Each joint's turn D from Tekken 3's axes to Tekken Tag's (the same body: W_ttt = W_t3 · D), measured on the
/// stances of the 14 fighters both games have (whole quarter turns; [`the_skeletons_differ_by_a_turn_per_joint`]).
/// A motion of one game on the other's skeleton: L' = D_parent⁻¹ · L · D (the other way, D⁻¹), the root's
/// translation as it is, as ok_tekken2_converter's transplant takes Tekken 2's motions onto Tekken 3's skeleton.
fn turn(joint: &str) -> Quat {
    match joint {
        "root" | "clavicle_A" | "clavicle_B" => Quat::IDENTITY,
        "foot_A" | "foot_B" => Quat::from_rotation_y(PI),
        "hand_A" | "hand_B" => Quat::from_rotation_x(-FRAC_PI_2),
        _ => Quat::from_rotation_x(PI),
    }
}

fn roots() -> Option<(Assets, Assets)> {
    let (Some(t3), Some(ttt)) = (std::env::var_os("OPENKUMITE_ASSETS"), std::env::var_os("OK_TTT_ASSETS")) else {
        eprintln!("skipping: OPENKUMITE_ASSETS or OK_TTT_ASSETS unset");
        return None;
    };
    Some((Assets::new(PathBuf::from(t3)).unwrap(), Assets::new(PathBuf::from(ttt)).unwrap()))
}

/// A Tekken 3 fighter's first costume (by its roster name) and its moveset file.
fn t3_fighter(a: &Assets, name: &str) -> (Character, serde_json::Value) {
    let roster: serde_json::Value = a.gamedata("roster.json").unwrap();
    let e = roster["selections"]["entries"].as_array().unwrap().iter().find(|e| e["name"] == name && e["costume"] == 0).unwrap();
    (a.character(e["model_record"].as_u64().unwrap() as usize).unwrap(), a.gamedata(e["moveset_file"].as_str().unwrap()).unwrap())
}

/// A Tekken Tag body's first costume with the motion file `anim` added; the clip's index.
fn ttt_fighter(a: &Assets, body: u64, anim: &str) -> (Character, usize) {
    let index: serde_json::Value = a.gamedata("movesets/index.json").unwrap();
    let m = index["models"].as_array().unwrap().iter().find(|m| m["body"] == body).unwrap();
    let mut c = a.character(m["model_record"].as_u64().unwrap() as usize).unwrap();
    c.add_motions(&std::fs::read(a.root.join("motions").join(format!("{anim}.glb"))).unwrap()).unwrap();
    let k = c.clip(anim).unwrap();
    (c, k)
}

/// The stance's frame 0: Tekken 3's fighter `id` (alias 3), Tekken Tag's body `id` (alias 58).
fn stances(t3: &Assets, ttt: &Assets, id: u64) -> (Character, Vec<Mat4>, Character, Vec<Mat4>) {
    let roster: serde_json::Value = t3.gamedata("roster.json").unwrap();
    let name = roster["selections"]["entries"].as_array().unwrap().iter().find(|e| e["character"] == id).unwrap()["name"].as_str().unwrap().to_string();
    let (c3, set) = t3_fighter(t3, &name);
    let clip = c3.clip(set["moves"][set["aliases"][3]["move"].as_u64().unwrap() as usize]["anim"].as_str().unwrap()).unwrap();
    let index: serde_json::Value = ttt.gamedata("movesets/index.json").unwrap();
    let file = index["models"].as_array().unwrap().iter().find(|m| m["body"] == id).unwrap()["moveset_file"].as_str().unwrap().to_string();
    let aliases: serde_json::Value = ttt.gamedata(&format!("movesets/{file}")).unwrap();
    let moves: serde_json::Value = ttt.gamedata("moves.json").unwrap();
    let (ct, k) = ttt_fighter(ttt, id, moves["moves"][aliases["aliases"][58].as_u64().unwrap() as usize]["anim"].as_str().unwrap());
    let (p3, pt) = (c3.pose(clip, 0, true), ct.pose(k, 0, true));
    (c3, p3, ct, pt)
}

fn world_rotation(c: &Character, pose: &[Mat4], joint: &str) -> (Quat, Vec3) {
    let (_, r, t) = pose[c.nodes.iter().position(|n| n.name == joint).unwrap()].to_scale_rotation_translation();
    (r, t)
}

/// The 14 fighters both games have, in the same stance: every joint where the other game's is, and its axes turned
/// by its [`turn`] (on average: the next quarter turn is 90° off; the arms differ most, as the two stances do).
#[test]
fn the_skeletons_differ_by_a_turn_per_joint() {
    let Some((t3, ttt)) = roots() else { return };
    let (mut off, mut apart) = ([0.0f32; 18], [0.0f32; 18]);
    for id in 0..14 {
        let (c3, p3, ct, pt) = stances(&t3, &ttt, id);
        for (k, b) in BONES.iter().enumerate() {
            let ((r3, t3), (rt, tt)) = (world_rotation(&c3, &p3, b), world_rotation(&ct, &pt, b));
            apart[k] += t3.distance(tt) / 14.0;
            off[k] += (r3 * turn(b)).angle_between(rt).to_degrees() / 14.0;
            // played by name alone, a joint other than these is turned half round or more
            if !["root", "clavicle_A", "clavicle_B"].contains(b) {
                assert!(r3.angle_between(rt).to_degrees() > 60.0, "fighter {id} {b}");
            }
        }
    }
    for ((b, o), a) in BONES.iter().zip(off).zip(apart) {
        eprintln!("{b:11} {:?}  off by {o:.1}°, {:.0} mm apart on average", turn(b).to_axis_angle(), a * 1000.0);
        assert!(o < 20.0 && a < 0.1, "{b}: {o}°, {a} m");
    }
}

/// Clip `clip` of `from` as a clip of `to` (`to_ttt`: from Tekken 3's axes to Tekken Tag's): each joint's track by
/// name, turned (a twist helper, `<joint>.N`, takes its joint's); the root's translation at `to`'s size (by the
/// legs, as `Character::with_clips_of`). Added to `to`'s clips; its index.
fn retarget(to: &mut Character, from: &Character, clip: usize, to_ttt: bool) -> usize {
    let c = &from.clips[clip];
    let d = |j: &str| if to_ttt { turn(j) } else { turn(j).inverse() };
    let joint = |n: &str| BONES.iter().copied().find(|b| n.split('.').next() == Some(b));
    let leg = |c: &Character| c.nodes.iter().filter(|n| n.name == "shin_A" || n.name == "foot_A").map(|n| n.translation.length()).sum::<f32>();
    let size = leg(to) / leg(from);
    let mut out = Clip { name: format!("{}_retargeted", c.name), frames: c.frames, rotations: vec![None; to.nodes.len()], translations: vec![None; to.nodes.len()] };
    for (i, n) in to.nodes.iter().enumerate() {
        let Some(j) = joint(&n.name) else { continue };
        let Some(s) = from.nodes.iter().position(|o| o.name == j) else { continue };
        // (the parent joint: past Tekken Tag's node_2)
        let mut p = n.parent;
        while let Some(q) = p.filter(|&q| joint(&to.nodes[q].name).is_none()) {
            p = to.nodes[q].parent;
        }
        let dp = p.and_then(|p| joint(&to.nodes[p].name)).map_or(Quat::IDENTITY, d);
        let local = |f: usize| c.rotations[s].as_ref().map_or(from.nodes[s].rotation, |v| v[f.min(v.len() - 1)]);
        out.rotations[i] = Some((0..c.frames).map(|f| dp.inverse() * local(f) * d(j)).collect());
        if n.parent.is_none() {
            out.translations[i] = c.translations[s].as_ref().map(|v| v.iter().map(|t| *t * size).collect());
        }
    }
    to.clips.push(out);
    to.clips.len() - 1
}

/// Fighters side by side facing the camera (a motion faces +z), each with its pose, to `name` in the test's temporary folder.
fn render(r: &mut Renderer, fighters: &[(&Character, Vec<Mat4>)], name: &str) {
    let mut gpu = Vec::new();
    for (k, (c, pose)) in fighters.iter().enumerate() {
        let verts: Vec<SkinVertex> = c.vertices.iter().map(|v| SkinVertex { pos: v.pos, uv: v.uv, joints: v.joints, weights: v.weights, normal: v.normal, look: v.look }).collect();
        let model = r.skinned(&verts, &c.indices, c.blend_from, c.layers, r.texture(c.texture.width, c.texture.height, &c.texture.rgba));
        let x = (k as f32 - (fighters.len() - 1) as f32 / 2.0) * 1.3;
        r.update_bones(&model, &c.skin_matrices(pose, Mat4::from_translation(Vec3::new(x, 0.0, 0.0))));
        gpu.push(model);
    }
    let mut cam = Camera::new(Vec3::new(0.0, 1.0, 4.2), Vec3::new(0.0, 0.8, 0.0), 0.8);
    cam.background = Some([70, 80, 100]);
    r.render_with(&cam, &[], &[], &gpu.iter().collect::<Vec<_>>(), None, &[], None).unwrap();
    let img = r.capture().unwrap();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let mut enc = png::Encoder::new(std::fs::File::create(&path).unwrap(), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&img.rgba).unwrap();
    eprintln!("wrote {}", path.display());
    assert!(img.rgba.chunks(4).any(|p| p != &img.rgba[..4]), "a blank frame");
}

/// Left to right: the motion on its own game's fighter, then on the other game's played by name alone, then
/// retargeted ([`turn`]). Tekken Tag's Paul's 30th motion at frame 20 on Gon and Doctor B. (Tekken 3), and Tekken 3's
/// Paul's 30th motion at frame 20 on Tekken Tag's Paul.
#[test]
fn motions_across_the_games() {
    let Some((t3, ttt)) = roots() else { return };
    let Ok(mut r) = Renderer::headless(1600, 720).map_err(|e| eprintln!("skipping: {e:#}")) else { return };
    let list: serde_json::Value = ttt.gamedata("movesets/motions_00.json").unwrap();
    let anim = list["motions"][30].as_str().unwrap().trim_end_matches(".glb").to_string();
    let (paul, k) = ttt_fighter(&ttt, 0, &anim);
    let (mut gon, _) = t3_fighter(&t3, "GON");
    let (mut doctor, _) = t3_fighter(&t3, "DOCTOR B.");
    let named = gon.pose_from(&paul, k, 20, true);
    let (g, d) = (retarget(&mut gon, &paul, k, false), retarget(&mut doctor, &paul, k, false));
    let poses = vec![(&paul, paul.pose(k, 20, true)), (&gon, named), (&gon, gon.pose(g, 20, true)), (&doctor, doctor.pose(d, 20, true))];
    render(&mut r, &poses, "t3_bodies_ttt_motion.png");

    let (t3_paul, _) = t3_fighter(&t3, "PAUL");
    let k = t3_paul.clip("motion_030").unwrap();
    let (mut ttt_paul, _) = ttt_fighter(&ttt, 0, &anim);
    let named = ttt_paul.pose_from(&t3_paul, k, 20, true);
    let back = retarget(&mut ttt_paul, &t3_paul, k, true);
    let poses = vec![(&t3_paul, t3_paul.pose(k, 20, true)), (&ttt_paul, named), (&ttt_paul, ttt_paul.pose(back, 20, true))];
    // (the same fighter: retargeted, every joint within 3 cm of Tekken 3's; by name, the sides swapped, an arm elsewhere)
    let apart = |p: usize, b: &str| world_rotation(poses[0].0, &poses[0].1, b).1.distance(world_rotation(poses[p].0, &poses[p].1, b).1);
    assert!(BONES.iter().all(|b| apart(2, b) < 0.03) && apart(1, "foot_A") > 0.15 && apart(1, "hand_A") > 0.5);
    render(&mut r, &poses, "ttt_body_t3_motion.png");
}
