//! A NEW CHALLENGER (#395): in an arcade fight 2P presses START, picks on the select, the two fight, and the winner
//! (2P: the AI plays its seat, 1P stands) goes on with the run's fight on its own side, the CPU on the loser's.
//! Tekken 3 on OPENKUMITE_ASSETS, Tekken Tag on OK_TTT_ASSETS; each skipped without its assets or a GPU.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use std::path::PathBuf;
use std::process::Command;

/// The fights `args` load, headless for `frames` frames ("A vs B on stage N"), and the challenger's match's winner.
fn fights(env: &str, name: &str, frames: u32, args: &[&str]) -> Option<(Vec<String>, String)> {
    let Some(assets) = std::env::var_os(env).map(PathBuf::from).filter(|a| kumite_assets::Assets::new(a).is_ok()) else {
        eprintln!("skipping: no {env}");
        return None;
    };
    let save = std::env::temp_dir().join(format!("openkumite_challenger_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save);
    std::fs::create_dir_all(&save).unwrap();
    let out = save.join("end.png");
    let o = Command::new(env!("CARGO_BIN_EXE_openkumite"))
        .arg("--assets")
        .arg(&assets)
        .args(args)
        .args(["--ai", "2", "--seed", "1", "--rounds", "1", "--frames", &frames.to_string(), "--shot"])
        .arg(&out)
        .env("OPENKUMITE_SAVE_DIR", &save)
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&o.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&save);
    if !log.contains("wrote") {
        eprintln!("skipping: no picture (a GPU?):\n{log}");
        return None;
    }
    let loads = log.lines().filter_map(|l| Some(l.split("Some(Arcade) fight ").nth(1)?.split(" loaded").next()?.to_string())).collect();
    let won = log.lines().find_map(|l| l.split("a challenger's match won by ").nth(1)).unwrap_or_default().to_string();
    Some((loads, won))
}

#[test]
fn tekken3_challenger_wins_and_goes_on() {
    let Some((loads, won)) = fights("OPENKUMITE_ASSETS", "t3", 2600, &["--mode", "arcade", "--player", "xiaoyu", "--keys", "300:p2:start,460:p2:right,500:p2:b1"]) else { return };
    // stage 1 against the ladder's Jin; Gon picked by 2P, on his home stage; then 2P's Gon against Jin on Jin's
    assert_eq!(loads, ["Xiaoyu vs Jin on stage 8", "Xiaoyu vs Gon on stage 13", "Jin vs Gon on stage 8"], "the fights");
    assert_eq!(won, "Some(1)");
}

#[test]
fn tekken_tag_challenger_wins_and_goes_on() {
    let args = ["--mode", "ttt-arcade", "--player", "xiaoyu", "--opponent", "jin", "--keys", "400:p2:start,500:p2:b1,520:p2:right,530:p2:b1,560:b1,580:b1"];
    let Some((loads, won)) = fights("OK_TTT_ASSETS", "ttt", 6000, &args) else { return };
    // both pick again (2P Jin, 1P Xiaoyu); 2P's Jin goes on against stage 1's CPU on 1P's side
    assert_eq!(loads.len(), 3, "the fights: {loads:?}");
    assert!(loads[1].starts_with("Xiaoyu vs Jin"), "{loads:?}");
    assert!(loads[2].starts_with(&format!("{} vs Jin", loads[0].split(" vs ").nth(1).unwrap().split(" on").next().unwrap())), "{loads:?}");
    assert_eq!(won, "Some(1)");
}
