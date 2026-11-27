//! Tekken 3's attract demo (battle type 6) calls nothing: no ROUND, FIGHT or K.O. (PCSX-Redux: no 0x80040C00 in the
//! demo fight). A CPU-against-CPU fight in the demo's mode (`--screen winpose`) run headless with its sounds logged
//! (`--sound-log`). On OPENKUMITE_ASSETS (Tekken 3's), skipped without them or a GPU.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use std::path::PathBuf;
use std::process::Command;

#[test]
fn the_attract_demo_calls_no_round_and_no_ko() {
    let Some(assets) = std::env::var_os("OPENKUMITE_ASSETS").map(PathBuf::from).filter(|a| kumite_assets::Assets::new(a).is_ok()) else {
        eprintln!("skipping: no OPENKUMITE_ASSETS");
        return;
    };
    let save = std::env::temp_dir().join(format!("openkumite_demo_calls_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save);
    std::fs::create_dir_all(&save).unwrap();
    let args = ["--screen", "winpose", "--models", "111,75", "--seed", "1", "--frames", "0", "--sound-log", "--shot"];
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
    if !log.contains("winpose: winner") {
        eprintln!("skipping: no fight ran (a GPU?):\n{log}");
        return;
    }
    assert!(log.lines().any(|l| l.contains("Sfx(")), "the fight's own sounds play:\n{log}");
    let calls: Vec<&str> = log.lines().filter(|l| l.contains("Sfx(") && l.contains("/ann/")).collect();
    assert!(calls.is_empty(), "the announcer: {calls:?}");
}
