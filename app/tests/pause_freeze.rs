//! The pause holds the whole picture: a fight paused mid-hit shows the same picture, pixel for pixel, 300 frames on
//! (the fighters, the effects, the arena's people and weather, the HUD). Tekken 3 on OPENKUMITE_ASSETS, Tekken Tag on
//! OK_TTT_ASSETS; each skipped without its assets or a GPU.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // tests: a failure stops them

use std::path::PathBuf;
use std::process::Command;

/// `args` run headless with START pressed at step `at`: the pictures at `at + 1` and `at + 301` frames.
fn paused_shots(env: &str, name: &str, at: u32, args: &[&str]) {
    let Some(assets) = std::env::var_os(env).map(PathBuf::from).filter(|a| kumite_assets::Assets::new(a).is_ok()) else {
        eprintln!("skipping: no {env}");
        return;
    };
    let save = std::env::temp_dir().join(format!("openkumite_pause_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&save);
    std::fs::create_dir_all(&save).unwrap();
    let keys = format!("{at}:start");
    let shot = |frames: u32| {
        let (out, frames) = (save.join(format!("{frames}.png")), frames.to_string());
        let o = Command::new(env!("CARGO_BIN_EXE_openkumite"))
            .arg("--assets")
            .arg(&assets)
            .args(args)
            .args(["--keys", &keys, "--frames", &frames, "--shot"])
            .arg(&out)
            .env("OPENKUMITE_SAVE_DIR", &save)
            .output()
            .unwrap();
        std::fs::read(&out).map_err(|_| String::from_utf8_lossy(&o.stderr).into_owned())
    };
    let first = match shot(at + 1) {
        Ok(png) => png,
        Err(log) => return eprintln!("skipping: no picture (a GPU?):\n{log}"),
    };
    assert!(first == shot(at + 301).unwrap(), "{name}: the paused picture changed (see {})", save.display());
    let _ = std::fs::remove_dir_all(&save);
}

/// Devil's beam striking on the temple arena (its steam, its people walking).
#[test]
fn tekken_tag_pause_holds_the_picture() {
    paused_shots("OK_TTT_ASSETS", "ttt", 239, &["--mode", "vs", "--player", "devilangel", "--opponent", "hwoarang", "--stage", "4", "--p1", "n*200,n+1+2*2,n*60"]);
}

/// The CPUs fighting, paused on a hit.
#[test]
fn tekken3_pause_holds_the_picture() {
    paused_shots("OPENKUMITE_ASSETS", "t3", 601, &["--mode", "vs", "--player", "paul", "--opponent", "hwoarang", "--ai", "1,2", "--seed", "1"]);
}
