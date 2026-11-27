//! Tekken 3's match call fades the music (0x8003E060 -> 0x8006B6C8; PS1 t3sim51: out over 180 vsyncs from YOU WIN):
//! an arcade match run headless with its sounds logged (`--sound-log`), P1 (the human's seat, played by the AI) winning
//! on time; the music's level from the step after the call's voice. On OPENKUMITE_ASSETS (Tekken 3's), skipped
//! without them or a GPU.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use std::path::PathBuf;
use std::process::Command;

/// A sound log's line: its step and event ("step N: Duck(level)").
fn at(l: &str) -> Option<(u64, &str)> {
    let (n, e) = l.split_once("step ")?.1.split_once(": ")?;
    Some((n.parse().ok()?, e))
}

#[test]
fn you_win_fades_the_music_as_on_the_ps1() {
    let Some(assets) = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).filter(|a| kumite_assets::Assets::new(a).is_ok()) else {
        eprintln!("skipping: no OPENKUMITE_ASSETS");
        return;
    };
    let save = std::env::temp_dir().join(format!("openkumite_fade_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save);
    std::fs::create_dir_all(&save).unwrap();
    let args = ["--mode", "arcade", "--player", "paul", "--ai", "1", "--rounds", "1", "--round-time", "10", "--seed", "1", "--frames", "1000", "--sound-log", "--shot"];
    let o = Command::new(env!("CARGO_BIN_EXE_openkumite"))
        .arg("--assets")
        .arg(&assets)
        .args(args)
        .arg(save.join("end.png"))
        .env("OPENKUMITE_SAVE_DIR", &save)
        .env("RUST_LOG", "openkumite=info,sound=debug")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&o.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&save);
    if !log.contains("step 0: Music") {
        eprintln!("skipping: no match ran (a GPU?):\n{log}");
        return;
    }
    assert!(log.contains("Result { winner: Some(0) }"), "P1 wins (YOU WIN):\n{log}");
    let ducks: Vec<(u64, f32)> = log.lines().filter_map(at).filter_map(|(n, e)| Some((n, e.strip_prefix("Duck(")?.strip_suffix(')')?.parse().ok()?))).collect();
    let call = ducks.first().expect("the music fades").0 - 1;
    assert!(log.lines().filter_map(at).any(|(n, e)| n == call && e.contains("Sfx") && e.contains("ann")), "the fade starts the step after the call's voice");
    for (k, &(n, level)) in ducks.iter().take(60).enumerate() {
        assert_eq!(n, call + 1 + k as u64, "a step of the fade each step");
        assert!((level - (1.0 - (k + 1) as f32 / 180.0)).abs() < 1e-4, "step {n}: {level}");
    }
}
