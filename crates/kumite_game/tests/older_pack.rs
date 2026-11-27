//! A Tekken Tag pack of an older conversion (the shape of a format-78 pack, 2026-11-24: before rounds.json's pair
//! poses and nudges, sound.json's swings, kind-3 steps and attack / hurt voices, roster.json's WINNERS and quick
//! bodies, stages.json's scene lights) plays: the fight loads with its sounds, round 1 starts, the pairs pose the
//! normal way. The newer fields are stripped from `OK_TTT_ASSETS`' files into a layer over it (kumite_fs); skips
//! without one. (A file of its own: the layers are the process's.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use kumite_assets::Assets;
use kumite_game::bout::{Bout, Phase};
use kumite_game::Game;
use kumite_sim::input::Pad;
use serde_json::Value;

/// What a format-78 pack's game data has not: (file, JSON pointers; `*` every element of an array).
const NEWER: [(&str, &[&str]); 4] = [
    ("rounds.json", &["/win_poses/pairs", "/win_poses/nudges", "/win_poses/turn", "/team_lose/pairs", "/team_lose/partner", "/team_lose/turn"]),
    ("sound.json", &["/attack_voice", "/hurt_voice", "/script_kind_3", "/swing", "/voice_rows/rows/*/attack", "/voice_rows/rows/*/hurt"]),
    ("roster.json", &["/front_end/hud/winners", "/front_end/select/quick/names/bodies"]),
    ("stages.json", &["/fill", "/stages/*/scene_lights"]),
];

fn strip(v: &mut Value, path: &[&str]) {
    match path {
        [] => {}
        ["*", rest @ ..] => v.as_array_mut().into_iter().flatten().for_each(|e| strip(e, rest)),
        [last] => drop(v.as_object_mut().and_then(|o| o.remove(*last))),
        [k, rest @ ..] => strip(&mut v[*k], rest),
    }
}

#[test]
fn an_older_tekken_tag_pack_plays() {
    let Some(root) = std::env::var_os("OK_TTT_ASSETS") else {
        eprintln!("skipped: OK_TTT_ASSETS (a Tekken Tag assets/ folder) is not set");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let layer = std::env::temp_dir().join(format!("ok_older_pack_{}", std::process::id()));
    std::fs::create_dir_all(layer.join("gamedata")).unwrap();
    for (file, paths) in NEWER {
        let mut v: Value = serde_json::from_slice(&std::fs::read(root.join("gamedata").join(file)).unwrap()).unwrap();
        for p in paths {
            assert!(p.contains('*') || v.pointer(p).is_some(), "{file}: {p} is no longer where it was");
            strip(&mut v, &p.split('/').skip(1).collect::<Vec<_>>());
        }
        std::fs::write(layer.join("gamedata").join(file), v.to_string()).unwrap();
    }
    kumite_fs::set_layers(&root, vec![layer.clone()]);
    let a = Assets::new(&root).unwrap();
    kumite_game::sound::SoundData::load(&a).unwrap();
    let game = Game::load(&a, [345, 372]).unwrap();
    let rounds = game.rounds.clone().unwrap();
    assert!(rounds.win_pairs.is_empty() && rounds.lose_pairs.is_empty() && rounds.win_nudges.is_empty());
    let mut b = Bout::new(game, kumite_game::ai_data(&a).unwrap(), 2, false);
    for _ in 0..600 {
        if b.phase == Phase::Fight {
            break;
        }
        b.step([Pad::default(); 2]);
    }
    assert_eq!(b.phase, Phase::Fight, "round 1 starts");
    kumite_fs::set_layers(&root, Vec::new());
    std::fs::remove_dir_all(&layer).unwrap();
}
