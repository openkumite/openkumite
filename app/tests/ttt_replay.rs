//! Tekken Tag's replays: a VS tag match (both sides scripted, tags and Devil's and True Ogre's moves), a TAG BRAWL of
//! four CPUs and a Tekken Bowl game, recorded, then watched in another run: the state hash the same every step (Bowl:
//! at its end); a seek back lands on the state and the picture playing straight has; REPLAYS on Tekken Tag's main
//! menu plays one and deletes one. On a Tekken Tag `assets/` (OK_TTT_ASSETS) and a GPU; skipped without either.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use std::path::{Path, PathBuf};
use std::process::Command;

fn assets() -> Option<PathBuf> {
    let a = std::env::var_os("OK_TTT_ASSETS").map(PathBuf::from).filter(|a| kumite_assets::Assets::new(a).is_ok());
    if a.is_none() {
        eprintln!("skipping: no OK_TTT_ASSETS");
    }
    a
}

fn run(assets: &Path, save: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_openkumite")).arg("--assets").arg(assets).args(args).env("OPENKUMITE_SAVE_DIR", save).env("RUST_LOG", "info").output().unwrap();
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `--hash-log 1` lines: (step, hash).
fn hashes(log: &str) -> Vec<&str> {
    log.lines().filter_map(|l| l.find("HASH ").map(|i| &l[i..])).collect()
}

fn replays(save: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(save.join("replays")).into_iter().flatten().flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
    ids.sort();
    ids
}

fn save_dir(name: &str) -> PathBuf {
    let save = std::env::temp_dir().join(format!("openkumite_ttt_replay_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save);
    std::fs::create_dir_all(&save).unwrap();
    save
}

/// Record with `args`, then watch it: every step's hash as recorded, its end checked. The save folder.
fn recorded_and_replayed(assets: &Path, name: &str, args: &[&str]) -> Option<PathBuf> {
    let save = save_dir(name);
    let rec = save.join("rec.png").to_string_lossy().into_owned();
    let mut a = args.to_vec();
    a.extend(["--hash-log", "1", "--shot", &rec]);
    let log = run(assets, &save, &a);
    let Some(id) = replays(&save).pop() else {
        assert!(!log.contains("loaded in"), "{name}: a match was played but no replay saved:\n{log}");
        eprintln!("skipping: no match ran (a GPU?)");
        return None;
    };
    let live = hashes(&log);
    assert!(live.len() > 1000, "{name}: {} steps", live.len());
    let out = save.join("play.png").to_string_lossy().into_owned();
    let frames = (live.len() + 10).to_string();
    let watched = run(assets, &save, &["--replay", &id, "--hash-log", "1", "--shot", &out, "--frames", &frames]);
    assert!(watched.contains("the state as recorded: Some(true)"), "{name}: the replay's end is not as recorded");
    let replayed = hashes(&watched);
    for (k, (a, b)) in live.iter().zip(&replayed).enumerate() {
        assert_eq!(a, b, "{name}: step {} differs", k + 1);
    }
    assert!(replayed.len() >= live.len());
    Some(save)
}

/// The newest replay sought back from step `from` (the viewer's LEFT: 200 steps back; the viewer pauses there) shows
/// the match and the picture playing it straight to there shows.
fn seek_lands(assets: &Path, save: &Path, name: &str, from: usize) {
    let play = |shot: &str, frames: usize, keys: Option<String>| {
        let out = save.join(shot).to_string_lossy().into_owned();
        let frames = frames.to_string();
        let mut a = vec!["--replay", "latest", "--shot", &out, "--frames", &frames];
        a.extend(keys.iter().flat_map(|k| ["--keys", k.as_str()]));
        let log = run(assets, save, &a);
        let line = log.lines().find_map(|l| l.find("replay at step").map(|i| l[i..].to_string())).unwrap();
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        (line.split(", HUD").next().unwrap().to_string(), pixels)
    };
    let to = from - 200;
    let (seek, straight) = (play("seek.png", from + 100, Some(format!("{from}:left"))), play("straight.png", to, None));
    assert!(seek.0.starts_with(&format!("replay at step {to},")), "{name}: {}", seek.0);
    assert_eq!(seek.0, straight.0, "{name}: after the seek back, the match as when played straight");
    let differ = seek.1.iter().zip(&straight.1).filter(|(a, b)| a != b).count();
    assert!(differ == 0, "{name}: after the seek back, {differ} bytes of the picture differ from playing straight");
}

/// A pad script of `pattern` repeated.
fn script(pattern: &str) -> String {
    vec![pattern; 40].join(",")
}

#[test]
fn a_vs_tag_match_replays_step_by_step() {
    let Some(assets) = assets() else { return };
    // VS from the menu: 1P Devil / Angel and True Ogre, 2P Jin and King; the select's clock runs out, the stage
    // select's RANDOM
    let keys = "10:down,40:start,110:right,120:right,130:right,140:right,150:down,170:start,190:right,210:start,230:p2:start,250:p2:left,270:p2:start,2800:start,2900:start";
    let p1 = script("n*20,1,n*10,f+2,n*10,d/f+3,n*15,5,n*30,f*20,1+2,n*20,4,n*10,d+3,n*25,b*10,2,n*20,u/f+4,n*30,f*8,1,2,n*12,d/f+1+2,n*30,3+4,n*30");
    let p2 = script("n*33,2,n*14,d/f+1,n*20,f*12,3+4,n*25,5,n*40,b*15,d+4,n*18,1,1,2,n*22,f+2,n*9");
    let args = ["--screen", "ttt-menu", "--keys", keys, "--p1", &p1, "--p2", &p2, "--round-time", "30", "--rounds", "2", "--frames", "14000"];
    let Some(save) = recorded_and_replayed(&assets, "vs", &args) else { return };
    // a seek back (1700 to 1500) lands on the state playing straight to 1500 has, and on its picture: the arena's
    // own steps (its people, objects, weather) and the swaying nodes are in the seek's copy
    seek_lands(&assets, &save, "vs", 1700);
    let _ = std::fs::remove_dir_all(&save);
}

/// `--mode ttt-vs` with four fighters: Tekken Tag's VS tag battle (A & C vs B & D), the AI on both sides, recorded
/// as the select's would be and replayed.
#[test]
fn a_ttt_vs_tag_battle_of_the_ai_replays() {
    let Some(assets) = assets() else { return };
    let args = ["--mode", "ttt-vs", "--fighters", "jin,paul,xiaoyu,king", "--ai", "1,2", "--round-time", "20", "--rounds", "1", "--frames", "6000"];
    if let Some(save) = recorded_and_replayed(&assets, "ttt-vs", &args) {
        let file = std::fs::read_to_string(save.join("replays").join(format!("{}.okreplay", replays(&save).pop().unwrap()))).unwrap();
        assert!(file.contains("VS · Jin & Xiaoyu vs Paul & King"), "{}", &file[..300.min(file.len())]);
        let _ = std::fs::remove_dir_all(&save);
    }
}

#[test]
fn a_tag_brawl_replays_step_by_step() {
    let Some(assets) = assets() else { return };
    let args = ["--mode", "tag-teams", "--fighters", "true ogre,roger / alex,ogre,kazuya", "--ai", "1,2,3,4", "--round-time", "30", "--frames", "15000"];
    if let Some(save) = recorded_and_replayed(&assets, "brawl", &args) {
        let _ = std::fs::remove_dir_all(&save);
    }
}

/// Tekken Bowl is recorded as Tekken 3's Ball is (a game to its end, `--screen bowl`'s scripted throws), and played
/// back to the state recorded; a seek back lands on the game and the hall playing straight shows.
#[test]
fn a_tekken_bowl_game_replays() {
    let Some(assets) = assets() else { return };
    let save = save_dir("bowl");
    let shot = save.join("rec.png").to_string_lossy().into_owned();
    let log = run(&assets, &save, &["--screen", "bowl", "--shot", &shot, "--frames", "30000"]);
    let Some(id) = replays(&save).pop() else {
        eprintln!("skipping: no game was bowled (a GPU?):\n{log}");
        return;
    };
    assert!(log.contains("saved: BOWL · "), "{log}");
    let watched = run(&assets, &save, &["--replay", &id, "--shot", &shot, "--frames", "30000"]);
    assert!(watched.contains("the state as recorded: Some(true)"), "the replay's end is not as recorded:\n{watched}");
    seek_lands(&assets, &save, "bowl", 2500);
    let _ = std::fs::remove_dir_all(&save);
}

/// REPLAYS on Tekken Tag's main menu (up from ARCADE: EXIT, OPTIONS, REPLAYS): START plays the newest, button 2
/// twice deletes it.
#[test]
fn the_menu_plays_and_deletes_a_replay() {
    let Some(assets) = assets() else { return };
    let save = save_dir("menu");
    let shot = save.join("x.png").to_string_lossy().into_owned();
    let log = run(&assets, &save, &["--mode", "tag-teams", "--ai", "1,2,3,4", "--round-time", "10", "--shot", &shot, "--frames", "6000"]);
    let Some(id) = replays(&save).pop() else {
        eprintln!("skipping: no match ran (a GPU?):\n{log}");
        return;
    };
    let menu = |keys: &str| run(&assets, &save, &["--screen", "ttt-menu", "--keys", keys, "--shot", &shot, "--frames", "200"]);
    let watched = menu("10:up,40:up,70:up,100:start,130:start");
    assert!(watched.contains("replay run through"), "START on the list plays it:\n{watched}");
    menu("10:up,40:up,70:up,100:start,130:b2,140:b2");
    assert!(!replays(&save).contains(&id), "deleted");
    let _ = std::fs::remove_dir_all(&save);
}
