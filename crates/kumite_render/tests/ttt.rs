//! Tekken Tag Tournament's converted data drawn: two fighters of its roster with their movesets' motions, posed on
//! its first arena, rendered offscreen to `ttt.png` in the test's temporary folder. On a Tekken Tag game root
//! (OK_TTT_ASSETS: the Tekken Tag converter's `assets/`) and a GPU; skipped without either.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use glam::{Mat4, Vec3};
use kumite_render::{Camera, Renderer, SkinVertex, StaticVertex};
use std::path::PathBuf;

#[test]
fn ttt_fighters_on_an_arena() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS").map(PathBuf::from) else {
        eprintln!("skipping: OK_TTT_ASSETS unset");
        return;
    };
    let assets = kumite_assets::Assets::new(&root).unwrap();
    let Ok(mut r) = Renderer::headless(960, 720).map_err(|e| eprintln!("skipping: {e:#}")) else { return };
    let index: serde_json::Value = assets.gamedata("movesets/index.json").unwrap();
    let models = index["models"].as_array().unwrap();
    // the first two bodies' first costumes, each with its moveset's motions
    let picks = [&models[0], models.iter().find(|m| m["body"] != models[0]["body"]).unwrap()];
    let fighters: Vec<kumite_assets::Character> = picks
        .iter()
        .map(|m| {
            let mut c = assets.character(m["model_record"].as_u64().unwrap() as usize).unwrap();
            let set: serde_json::Value = assets.gamedata(&format!("movesets/motions_{:02}.json", m["moveset_id"].as_u64().unwrap())).unwrap();
            for f in set["motions"].as_array().unwrap() {
                c.add_motions(&std::fs::read(root.join("motions").join(f.as_str().unwrap())).unwrap()).unwrap();
            }
            c
        })
        .collect();
    // (an opaque primitive, then a blended one)
    assert!(fighters.iter().all(|c| c.blend_from > 0 && c.blend_from < c.indices.len()));
    // Law's satin (converter format 67): environment-mapped, multiplied by the next layer's alpha, the third added;
    // his trousers 50 mm behind; filtered texels
    if let Ok(law) = assets.character(363).map_err(|e| eprintln!("no model 363: {e:#}")) {
        let [shade, add] = law.layers;
        if law.blend_from < shade && shade < add && law.filtered {
            assert!(law.vertices.iter().any(|v| v.look[1] == 2.0 && v.look[2] > 0.0) && law.vertices.iter().any(|v| v.look[0] > 0.04));
        } else {
            eprintln!("an older pack: no draw modes");
        }
    }
    // the select's programs (0x4FA7E0): Ogre's highlight lit plainly, his highlight-alone strips not drawn
    if let Ok(mut ogre) = assets.character(534).map_err(|e| eprintln!("no model 534: {e:#}")) {
        if ogre.vertices.iter().any(|v| v.look[1] == 4.0) {
            let n = ogre.indices.len();
            ogre.select_look();
            assert!(ogre.vertices.iter().all(|v| v.look[1] != 3.0) && ogre.indices.len() < n && ogre.indices.len() % 3 == 0);
            assert!(ogre.blend_from <= ogre.layers[0] && ogre.layers[0] <= ogre.layers[1] && ogre.layers[1] <= ogre.indices.len());
            assert!(ogre.indices.chunks(3).all(|t| t.iter().any(|&i| ogre.vertices[i as usize].look[1] != 4.0)));
        }
    }
    let stage = assets.stage(0).unwrap();
    assert!(stage.sky_model && !stage.sky.is_empty() && !stage.meshes.is_empty());

    // (the samplers the glb asks for, as the game picks them)
    let tex: Vec<_> = stage
        .textures
        .iter()
        .enumerate()
        .map(|(i, t)| match (stage.filtered[i], stage.smooth[i], stage.tiled[i]) {
            (true, ..) => r.texture_filtered(t.width, t.height, &t.rgba),
            (_, true, _) => r.texture_smooth_tiled(t.width, t.height, &t.rgba),
            (.., true) => r.texture_tiled(t.width, t.height, &t.rgba),
            _ => r.texture(t.width, t.height, &t.rgba),
        })
        .collect();
    let conv = |m: &kumite_assets::Mesh| -> Vec<StaticVertex> { m.vertices.iter().map(|v| StaticVertex { pos: v.pos, uv: v.uv, color: v.color, cell: v.cell, normal: v.normal }).collect() };
    let sky: Vec<_> = stage.sky.iter().map(|m| r.sorted_mesh(&conv(m), &m.indices, tex[m.texture].clone())).collect();
    let statics: Vec<_> = stage.meshes.iter().map(|m| r.static_mesh(&conv(m), &m.indices, tex[m.texture].clone())).collect();

    // facing each other 2 m apart (a motion's forward is -z): one standing, one at frame 20 of its moveset's 30th motion
    let mut gpu = Vec::new();
    for (k, c) in fighters.iter().enumerate() {
        let verts: Vec<SkinVertex> = c.vertices.iter().map(|v| SkinVertex { pos: v.pos, uv: v.uv, joints: v.joints, weights: v.weights, normal: v.normal, look: v.look }).collect();
        assert!(c.clips.len() > 30);
        let model = r.skinned(&verts, &c.indices, c.blend_from, c.layers, r.texture(c.texture.width, c.texture.height, &c.texture.rgba));
        let (clip, frame) = if k == 0 { (c.clip("motion_0000").unwrap(), 0) } else { (30, 20) };
        let pose = c.pose(clip, frame, true);
        // (the motion's tracks bound to the fighter's joints by name)
        let arm = c.nodes.iter().position(|n| n.name == "upperarm_A").unwrap();
        assert!(c.clips[clip].rotations[arm].is_some() && c.clips[clip].translations[0].is_some());
        let side = if k == 0 { -1.0 } else { 1.0 };
        let place = Mat4::from_translation(Vec3::new(side, 0.0, 0.0)) * Mat4::from_rotation_y(side * std::f32::consts::FRAC_PI_2);
        r.update_bones(&model, &c.skin_matrices(&pose, place));
        gpu.push(model);
    }
    let mut cam = Camera::new(Vec3::new(0.0, 1.3, 4.5), Vec3::new(0.0, 0.9, 0.0), 0.8);
    (cam.sky_cull, cam.sky_depth, cam.background) = (stage.sky_cull, true, Some(stage.background));
    r.render_with(&cam, &sky.iter().collect::<Vec<_>>(), &statics.iter().collect::<Vec<_>>(), &gpu.iter().collect::<Vec<_>>(), None, &[], None).unwrap();
    let img = r.capture().unwrap();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ttt.png");
    let mut enc = png::Encoder::new(std::fs::File::create(&path).unwrap(), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&img.rgba).unwrap();
    eprintln!("wrote {}", path.display());
    assert!(img.rgba.chunks(4).any(|p| p != &img.rgba[..4]), "a blank frame");
}
